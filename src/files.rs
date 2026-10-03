use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
pub const SOURCE_LIMIT: usize = 4 * 1024 * 1024;
static NEXT: AtomicU64 = AtomicU64::new(0);

pub fn read(path: &Path, limit: usize) -> Result<String, String> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err(format!("{} must be a regular file", path.display()));
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err(format!(
            "input_limit: {} exceeds {limit} bytes",
            path.display()
        ));
    }
    String::from_utf8(bytes).map_err(|_| format!("{} is not UTF-8", path.display()))
}
pub fn aliases(a: &Path, b: &Path) -> bool {
    if let (Ok(a), Ok(b)) = (fs::canonicalize(a), fs::canonicalize(b))
        && a == b
    {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(a), Ok(b)) = (fs::metadata(a), fs::metadata(b))
            && a.dev() == b.dev()
            && a.ino() == b.ino()
        {
            return true;
        }
    }
    a == b
}
pub fn normalized(path: &Path) -> Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(path)
    };
    let mut resolved = PathBuf::new();
    for part in absolute.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                resolved.pop();
            }
            p => {
                resolved.push(p.as_os_str());
                if resolved.exists() {
                    resolved = fs::canonicalize(&resolved).map_err(|e| e.to_string())?;
                }
            }
        }
    }
    Ok(resolved)
}
pub fn protect(output: &Path, inputs: &[PathBuf]) -> Result<(), String> {
    if inputs.iter().any(|input| aliases(output, input)) {
        return Err(format!(
            "output must not overwrite source or project input: {}",
            output.display()
        ));
    }
    if fs::symlink_metadata(output).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(format!(
            "refusing symbolic-link output: {}",
            output.display()
        ));
    }
    Ok(())
}
pub fn atomic_write(
    path: &Path,
    bytes: &[u8],
    permissions: Option<fs::Permissions>,
) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .ok_or("output needs a filename")?
        .to_string_lossy();
    let stage = parent.join(format!(
        ".{name}.keel-tmp-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&stage)
        .map_err(|e| format!("{}: {e}", stage.display()))?;
    let result = (|| -> std::io::Result<()> {
        file.write_all(bytes)?;
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)?;
        }
        file.sync_all()?;
        fs::rename(&stage, path)?;
        Ok(())
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&stage);
        return Err(format!("{}: {e}", path.display()));
    }
    Ok(())
}

pub struct EditLock(PathBuf);
impl EditLock {
    pub fn acquire(path: &Path) -> Result<Self, String> {
        let path = fs::canonicalize(path).map_err(|e| e.to_string())?;
        let lock = path.with_file_name(format!(
            ".{}.keel-lock",
            path.file_name().unwrap().to_string_lossy()
        ));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
            .map_err(|e| {
                format!(
                    "edit_locked: {}: {e}; an active edit or stale lock needs inspection",
                    lock.display()
                )
            })?;
        if let Err(e) = writeln!(file, "{}", std::process::id()) {
            let _ = fs::remove_file(&lock);
            return Err(e.to_string());
        }
        Ok(Self(lock))
    }
}
impl Drop for EditLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
