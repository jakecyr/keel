//! Bounded subprocess execution. Children get their own process group.
use std::io::{self, Read};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

pub struct Output {
    pub status: ExitStatus,
    pub stderr: String,
    pub timed_out: bool,
    pub truncated: bool,
}

// Concurrent fork/exec can briefly inherit another thread's CLOEXEC writer.
// Linux then rejects a freshly published executable with ETXTBSY until that
// unrelated child execs. Retry only this pre-execution error, never test failures.
// See https://github.com/rust-lang/rust/issues/114554.
fn retry_executable_busy<T>(
    mut spawn: impl FnMut() -> io::Result<T>,
    deadline: Instant,
) -> io::Result<T> {
    loop {
        match spawn() {
            #[cfg(unix)]
            Err(error)
                if error.raw_os_error() == Some(libc::ETXTBSY) && Instant::now() < deadline =>
            {
                thread::sleep(
                    Duration::from_millis(2)
                        .min(deadline.saturating_duration_since(Instant::now())),
                );
            }
            result => return result,
        }
    }
}

struct Guard(Child);
impl Guard {
    fn kill_group(&mut self) {
        #[cfg(unix)]
        // SAFETY: this PID is our live/reaped child and was created as group leader.
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        let _ = self.0.kill();
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        self.kill_group();
        let _ = self.0.wait();
    }
}

fn drain(
    mut stream: impl Read,
    cap: usize,
    stop: Arc<AtomicBool>,
    deadline: Instant,
) -> (Vec<u8>, bool) {
    let mut out = Vec::new();
    let mut buf = [0_u8; 8192];
    let mut truncated = false;
    loop {
        if Instant::now() >= deadline || (stop.load(Ordering::Acquire) && out.len() >= cap) {
            break;
        }
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let keep = n.min(cap.saturating_sub(out.len()));
                out.extend_from_slice(&buf[..keep]);
                truncated |= keep < n;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                thread::sleep(Duration::from_millis(2));
            }
            Err(_) => break,
        }
    }
    (out, truncated)
}

pub fn capture(command: &mut Command, timeout_ms: u64, memory_mib: u64) -> Result<Output, String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
        let cpu_seconds = timeout_ms.div_ceil(1000).saturating_add(1);
        // Only async-signal-safe libc operations are performed between fork and exec.
        unsafe {
            command.pre_exec(move || {
                let cpu = libc::rlimit {
                    rlim_cur: cpu_seconds as libc::rlim_t,
                    rlim_max: cpu_seconds as libc::rlim_t,
                };
                if libc::setrlimit(libc::RLIMIT_CPU, &cpu) != 0 {
                    return Err(io::Error::last_os_error());
                }
                let core = libc::rlimit {
                    rlim_cur: 0,
                    rlim_max: 0,
                };
                if libc::setrlimit(libc::RLIMIT_CORE, &core) != 0 {
                    return Err(io::Error::last_os_error());
                }
                #[cfg(target_os = "linux")]
                if memory_mib > 0 {
                    let bytes = memory_mib.saturating_mul(1024 * 1024) as libc::rlim_t;
                    let memory = libc::rlimit {
                        rlim_cur: bytes,
                        rlim_max: bytes,
                    };
                    if libc::setrlimit(libc::RLIMIT_AS, &memory) != 0 {
                        return Err(io::Error::last_os_error());
                    }
                }
                #[cfg(not(target_os = "linux"))]
                let _ = memory_mib;
                Ok(())
            });
        }
    }
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let spawn_deadline = deadline.min(Instant::now() + Duration::from_millis(250));
    let mut child = Guard(
        retry_executable_busy(|| command.spawn(), spawn_deadline)
            .map_err(|e| format!("cannot start subprocess: {e}"))?,
    );
    let stderr = child
        .0
        .stderr
        .take()
        .ok_or("missing subprocess error stream")?;
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        // SAFETY: owned, valid pipe descriptor; only adds nonblocking behavior.
        unsafe {
            let fd = stderr.as_raw_fd();
            let flags = libc::fcntl(fd, libc::F_GETFL);
            if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
                return Err(io::Error::last_os_error().to_string());
            }
        }
    }
    let stop = Arc::new(AtomicBool::new(false));
    let reader_stop = stop.clone();
    let reader = thread::spawn(move || {
        drain(
            stderr,
            64 * 1024,
            reader_stop,
            deadline + Duration::from_millis(250),
        )
    });
    let mut timed_out = false;
    let status = loop {
        match child.0.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(e) => return Err(e.to_string()),
        }
        if Instant::now() >= deadline {
            timed_out = true;
            child.kill_group();
            break child.0.wait().map_err(|e| e.to_string())?;
        }
        thread::sleep(Duration::from_millis(2));
    };
    // A compiler wrapper must not leave grandchildren holding our pipes open.
    child.kill_group();
    stop.store(true, Ordering::Release);
    let (stderr, truncated) = reader.join().map_err(|_| "subprocess reader panicked")?;
    Ok(Output {
        status,
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        timed_out,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_only_busy_executable_and_keeps_a_bounded_deadline() {
        let mut attempts = 0;
        let value = retry_executable_busy(
            || {
                attempts += 1;
                if attempts < 3 {
                    Err(io::Error::from_raw_os_error(libc::ETXTBSY))
                } else {
                    Ok(17)
                }
            },
            Instant::now() + Duration::from_millis(250),
        )
        .unwrap();
        assert_eq!(value, 17);
        assert_eq!(attempts, 3);
        let mut attempts = 0;
        let error = retry_executable_busy::<()>(
            || {
                attempts += 1;
                Err(io::Error::from_raw_os_error(libc::EACCES))
            },
            Instant::now() + Duration::from_millis(250),
        )
        .unwrap_err();
        assert_eq!(error.raw_os_error(), Some(libc::EACCES));
        assert_eq!(attempts, 1);
        let started = Instant::now();
        let error = retry_executable_busy::<()>(
            || Err(io::Error::from_raw_os_error(libc::ETXTBSY)),
            started + Duration::from_millis(10),
        )
        .unwrap_err();
        assert_eq!(error.raw_os_error(), Some(libc::ETXTBSY));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn temporarily_busy_native_executable_runs_after_writer_closes() {
        let temp = crate::Temp::new().unwrap();
        let binary = temp.0.join("true");
        crate::files::atomic_write(
            &binary,
            &std::fs::read("/bin/true").unwrap(),
            Some(std::fs::metadata("/bin/true").unwrap().permissions()),
        )
        .unwrap();
        let writer = std::fs::OpenOptions::new()
            .write(true)
            .open(&binary)
            .unwrap();
        let closer = thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            drop(writer);
        });
        let result = capture(&mut Command::new(&binary), 1000, 256).unwrap();
        closer.join().unwrap();
        assert!(result.status.success());
        assert!(!result.timed_out);
    }
}
