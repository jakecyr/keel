//! Independent regressions for findings from the integrated-tooling audit.
use super::*;

fn fixture(entry: &str, acceptance: &str) -> Temp {
    let temp = Temp::new().unwrap();
    fs::write(temp.0.join("main.keel"), entry).unwrap();
    fs::write(temp.0.join("acceptance.keel"), acceptance).unwrap();
    fs::write(
        temp.0.join("keel.json"),
        json!({
            "schema": 1, "name": "audit", "entry": "main.keel",
            "tests": ["acceptance.keel"]
        })
        .to_string(),
    )
    .unwrap();
    temp
}

#[test]
fn project_rejects_a_declaration_spanning_physical_files() {
    let temp = fixture("fn value() -> Int {", "return 42 }\nfn main() {}");
    // Concatenation alone produces valid syntax, but editing such a declaration
    // would address a range extending past the owning physical file.
    match project::Project::load(&temp.0) {
        Err(_) => (),
        Ok(project) => assert!(
            checked(&project.source).is_err(),
            "cross-file declaration was accepted"
        ),
    }
}

#[test]
fn structural_edits_cannot_weaken_acceptance_helper_functions() {
    let temp = fixture(
        "fn value() -> Int { return 42 } fn main() {}",
        "fn approved_value() -> Int { return 42 }\ntest \"approved\" { assert value() == approved_value() }",
    );
    let project = project::Project::load(&temp.0).unwrap();
    let (program, _) = checked(&project.source).unwrap();
    let request = temp.0.join("request.json");
    fs::write(
        &request,
        json!({
            "base_revision": revision(&project.source),
            "operation": "replace_body", "target": "fn:approved_value",
            "source": "{ return 0 }"
        })
        .to_string(),
    )
    .unwrap();
    let original = fs::read(temp.0.join("acceptance.keel")).unwrap();
    let args = vec![
        "edit".into(),
        temp.0.to_string_lossy().into_owned(),
        "--request".into(),
        request.to_string_lossy().into_owned(),
    ];
    let result = edit_context(&temp.0, &project.source, &program, &args, Some(&project));
    assert!(
        result.is_err() || result.as_ref().is_ok_and(|v| v["status"] != "APPLIED"),
        "acceptance oracle edit was applied"
    );
    assert_eq!(fs::read(temp.0.join("acceptance.keel")).unwrap(), original);
}

#[test]
fn transaction_cannot_edit_implementation_and_acceptance_oracle_together() {
    // Acceptance files can contain implementations/helpers as well as test
    // declarations; protecting test declaration text alone is insufficient.
    let temp = fixture(
        "fn main() {}",
        "fn value() -> Int { return 42 } fn approved_value() -> Int { return 42 }\ntest \"approved\" { assert value() == approved_value() }",
    );
    let project = project::Project::load(&temp.0).unwrap();
    let (program, _) = checked(&project.source).unwrap();
    let request = temp.0.join("request.json");
    fs::write(&request, json!({
        "base_revision": revision(&project.source),
        "edits": [
            {"operation": "replace_body", "target": "fn:value", "source": "{ return 0 }"},
            {"operation": "replace_body", "target": "fn:approved_value", "source": "{ return 0 }"}
        ], "run": "affected_checks_and_tests"
    }).to_string()).unwrap();
    let original = fs::read(temp.0.join("acceptance.keel")).unwrap();
    let args = vec![
        "edit".into(),
        temp.0.to_string_lossy().into_owned(),
        "--request".into(),
        request.to_string_lossy().into_owned(),
    ];
    let result = edit_context(&temp.0, &project.source, &program, &args, Some(&project));
    assert!(
        result.is_err() || result.as_ref().is_ok_and(|v| v["status"] != "APPLIED"),
        "transaction rewrote implementation and its acceptance oracle"
    );
    assert_eq!(fs::read(temp.0.join("acceptance.keel")).unwrap(), original);
}

#[cfg(unix)]
#[test]
fn not_yet_created_outputs_resolve_symlinked_parent_aliases() {
    let temp = Temp::new().unwrap();
    fs::create_dir(temp.0.join("real")).unwrap();
    std::os::unix::fs::symlink(temp.0.join("real"), temp.0.join("alias")).unwrap();
    let first = temp.0.join("real/new");
    let second = temp.0.join("alias/new");
    assert!(
        files::aliases(&first, &second)
            || files::normalized(&first).unwrap() == files::normalized(&second).unwrap(),
        "two new output paths name the same future file"
    );
}

#[cfg(unix)]
#[test]
fn output_normalization_preserves_symlink_parent_traversal_semantics() {
    let temp = Temp::new().unwrap();
    fs::create_dir_all(temp.0.join("real/sub")).unwrap();
    std::os::unix::fs::symlink(temp.0.join("real/sub"), temp.0.join("alias")).unwrap();
    let first = temp.0.join("real/new");
    let second = temp.0.join("alias/../new");
    assert_eq!(
        files::normalized(&first).unwrap(),
        files::normalized(&second).unwrap(),
        "lexical '..' stripping changes symlink path semantics"
    );
}

#[test]
fn bare_manifest_name_works_from_project_directory() {
    const CHILD: &str = "KEEL_AUDIT_BARE_MANIFEST_CHILD";
    if env::var_os(CHILD).is_some() {
        assert!(
            project::Project::load(Path::new("keel.json")).is_ok(),
            "bare manifest path failed"
        );
        return;
    }
    let temp = fixture("fn main() {}", "test \"ok\" { assert true }");
    // Isolate current-directory changes in a child test process so concurrent
    // tests cannot accidentally read or write this fixture.
    let output = Command::new(env::current_exe().unwrap())
        .current_dir(&temp.0)
        .env(CHILD, "1")
        .args([
            "--exact",
            "audit_tests::bare_manifest_name_works_from_project_directory",
            "--nocapture",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn nonregular_sources_are_rejected_without_waiting_for_fifo_writers() {
    const CHILD: &str = "KEEL_AUDIT_FIFO_CHILD";
    if let Some(path) = env::var_os(CHILD) {
        assert!(files::read(Path::new(&path), 1024).is_err());
        assert!(files::read(Path::new("/dev/null"), 1024).is_err());
        return;
    }
    use std::os::unix::ffi::OsStrExt;
    let temp = Temp::new().unwrap();
    let fifo = temp.0.join("source.keel");
    let path = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
    // SAFETY: path is NUL-terminated and lives through this system call.
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    let mut command = Command::new(env::current_exe().unwrap());
    command.env(CHILD, &fifo).args([
        "--exact",
        "audit_tests::nonregular_sources_are_rejected_without_waiting_for_fifo_writers",
        "--nocapture",
    ]);
    let output = process::capture(&mut command, 1000, 0).unwrap();
    assert!(
        !output.timed_out,
        "opening a FIFO blocked awaiting a writer"
    );
    assert!(output.status.success(), "{}", output.stderr);
}

#[cfg(unix)]
#[test]
fn detached_compiler_descendants_cannot_hold_diagnostic_pipe_forever() {
    use std::os::unix::process::CommandExt;
    const STAGE: &str = "KEEL_AUDIT_DETACHED_PIPE_STAGE";
    const NAME: &str =
        "audit_tests::detached_compiler_descendants_cannot_hold_diagnostic_pipe_forever";
    match env::var(STAGE).as_deref() {
        Ok("sleeper") => {
            std::thread::sleep(Duration::from_secs(2));
            return;
        }
        Ok("wrapper") => {
            let mut command = Command::new(env::current_exe().unwrap());
            command.env(STAGE, "sleeper").args(["--exact", NAME]);
            // SAFETY: setsid is the sole operation between fork and exec.
            unsafe {
                command.pre_exec(|| {
                    if libc::setsid() == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            let mut descendant = command.spawn().unwrap();
            // A real wrapper may exit while a detached descendant runs. Keep
            // normal reaping available without delaying the wrapper's exit.
            std::thread::spawn(move || {
                let _ = descendant.wait();
            });
            return;
        }
        _ => {}
    }
    let mut command = Command::new(env::current_exe().unwrap());
    command.env(STAGE, "wrapper").args(["--exact", NAME]);
    let start = Instant::now();
    let _ = process::capture(&mut command, 100, 0).unwrap();
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "diagnostic pipe drain ignored subprocess deadline"
    );
}

#[test]
fn atomic_write_never_deletes_a_staging_file_it_did_not_create() {
    const CHILD: &str = "KEEL_AUDIT_STAGE_COLLISION_CHILD";
    const NAME: &str = "audit_tests::atomic_write_never_deletes_a_staging_file_it_did_not_create";
    if env::var_os(CHILD).is_some() {
        let output = Path::new("output");
        let stage = PathBuf::from(format!(".output.keel-tmp-{}-0", std::process::id()));
        fs::write(&stage, b"preexisting staging-file content").unwrap();
        assert!(files::atomic_write(output, b"replacement", None).is_err());
        assert_eq!(
            fs::read(&stage).unwrap(),
            b"preexisting staging-file content"
        );
        assert!(!output.exists());
        return;
    }
    let temp = Temp::new().unwrap();
    let output = Command::new(env::current_exe().unwrap())
        .current_dir(&temp.0)
        .env(CHILD, "1")
        .args(["--exact", NAME, "--nocapture"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
