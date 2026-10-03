use super::*;

#[test]
fn process_and_write_effects_and_borrows_are_checked() {
    for source in [
        "fn f(){fs.write_text(\"x\",\"y\")}",
        "fn f(){process.run(\"/bin/echo\",\"[]\")}",
        "fn f(){process.run_timeout(\"/bin/echo\",\"[]\",50)}",
        "fn f(){process.spawn(\"/bin/echo\",\"[]\")}",
        "fn f(){process.poll(1)}",
        "fn f(){process.terminate(1)}",
        "fn f(){clock.millis()}",
    ] {
        rejected(source, "effect_not_allowed");
    }
    rejected(
        "fn consume(x:take Text)->Text{return x} fn f() effects {fs.write}{let x=\"x\" fs.write_text(x,consume(take x))}",
        "borrow_conflict",
    );
    for (expression, kind) in [
        ("fs.write_text(\"x\",\"y\")", "permission_denied_write"),
        (
            "process.run(\"/bin/echo\",\"[]\")",
            "permission_denied_exec",
        ),
        ("clock.millis()", "permission_denied_clock"),
        (
            "process.run_timeout(\"/bin/echo\",\"[]\",50)",
            "permission_denied_exec",
        ),
    ] {
        let program = syntax::parse(&format!("test \"blocked\" {{{expression}}}")).unwrap();
        let result = eval::run_case(&program, 0, None, 1000).unwrap_err();
        assert_eq!(result["status"], "BLOCKED");
        assert_eq!(result["kind"], kind);
        // The CLI rejects effectful tests; directly exercise native worker
        // classification without granting the tested host effect.
        let temp = Temp::new().unwrap();
        let binary = temp.0.join("blocked");
        compile(&program, &check::Analysis::default(), true, &binary, None).unwrap();
        let (status, error) = run_worker(&binary, 0, &options(), None).unwrap();
        assert_eq!(status, "BLOCKED", "{error:?}");
    }
}

#[test]
fn atomic_files_and_owned_process_lifecycle_under_sanitizers() {
    let source = r#"
fn main() effects {fs.read,fs.write,process.exec,clock.read} {
    match fs.write_text("state.json", "{\"n\":1}") {Ok(n)=>{assert n==7} Err(e)=>{assert false}}
    match fs.read_text("state.json") {Ok(t)=>{assert t=="{\"n\":1}"} Err(e)=>{assert false}}
    match fs.write_text("state.json", "new") {Ok(n)=>{assert n==3} Err(e)=>{assert false}}
    match fs.write_text("link", "bad") {Ok(n)=>{assert false} Err(e)=>{assert text.len(e)>0}}
    match process.run("/bin/echo", "[\"literal $(no-shell); hi\"]") {
        Ok(t)=>{assert t=="literal $(no-shell); hi\n"} Err(e)=>{assert false}
    }
    match process.run("/bin/echo", "[1]") {Ok(t)=>{assert false} Err(e)=>{assert text.len(e)>0}}
    match process.run("/bin/echo", "[\"x\",]") {Ok(t)=>{assert false} Err(e)=>{assert text.len(e)>0}}
    match process.run("/bin/echo", "[\"\\u0000\"]") {Ok(t)=>{assert false} Err(e)=>{assert text.len(e)>0}}
    match process.run("/bin/sh", "[\"-c\",\"printf broken; exit 3\"]") {Ok(t)=>{assert false} Err(e)=>{assert e=="broken"}}
    match process.run("/bin/sh", "[\"-c\",\"exec 1>&- 2>&-; sleep 0.05\"]") {Ok(t)=>{assert text.len(t)==0} Err(e)=>{assert false}}
    match process.run("/bin/sh", "[\"-c\",\"test ! -e /dev/fd/9 && printf isolated\"]") {Ok(t)=>{assert t=="isolated"} Err(e)=>{assert false}}
    match process.run_timeout("/bin/echo", "[]", 0) {Ok(t)=>{assert false} Err(e)=>{assert text.len(e)>0}}
    match process.run_timeout("/bin/echo", "[]", 30001) {Ok(t)=>{assert false} Err(e)=>{assert text.len(e)>0}}
    match process.run_timeout("/bin/echo", "[\"bounded\"]", 3000) {Ok(t)=>{assert t=="bounded\n"} Err(e)=>{assert false}}
    let started = clock.millis()
    match process.run_timeout("/bin/sleep", "[\"5\"]", 50) {Ok(t)=>{assert false} Err(e)=>{assert text.len(e)>0}}
    assert clock.millis() - started < 2000
    match process.spawn("/bin/sleep", "[\"5\"]") {
        Ok(handle)=>{
            match process.poll(handle) {Ok(status)=>{assert status == -1} Err(e)=>{assert false}}
            match process.terminate(handle) {Ok(status)=>{assert status>=128} Err(e)=>{assert false}}
            match process.poll(handle) {Ok(status)=>{assert false} Err(e)=>{assert text.len(e)>0}}
        }
        Err(e)=>{assert false}
    }
    match process.spawn("/bin/echo", "[\"background\"]") {
        Ok(handle)=>{
            var finished = false
            let deadline = clock.millis() + 3000
            while !finished && clock.millis() < deadline {
                match process.poll(handle) {Ok(status)=>{if status>=0 {assert status==0 finished=true}} Err(e)=>{assert false}}
            }
            assert finished
        }
        Err(e)=>{assert false}
    }
    let before = clock.millis()
    assert clock.millis() >= before
}
"#;
    let (program, analysis) = checked(source).unwrap();
    let temp = Temp::new().unwrap();
    fs::write(temp.0.join("untouched"), "original").unwrap();
    std::os::unix::fs::symlink("untouched", temp.0.join("link")).unwrap();
    let cpath = temp.0.join("process.c");
    let binary = temp.0.join("process");
    fs::write(&cpath, native::emit(&program, &analysis, false)).unwrap();
    let output = Command::new(env::var("CC").unwrap_or_else(|_| "cc".into()))
        .args([
            "-std=c11",
            "-O1",
            "-g",
            "-fsanitize=address,undefined",
            "-fno-omit-frame-pointer",
        ])
        .arg(cpath)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let denied = Command::new(&binary).current_dir(&temp.0).output().unwrap();
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("permission_denied_write"));
    // Inherit an extra descriptor into the native parent; its children must not
    // receive it. Positional arguments avoid interpolating paths into shell code.
    let output = Command::new("/bin/sh")
        .args(["-c", "exec 9>inherited; exec \"$@\"", "sh"])
        .arg(binary)
        .current_dir(&temp.0)
        .args([
            "--allow-read=state.json",
            "--allow-write=state.json",
            "--allow-write=link",
            "--allow-exec=/bin/echo",
            "--allow-exec=/bin/sh",
            "--allow-exec=/bin/sleep",
            "--allow-clock=monotonic",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(temp.0.join("state.json")).unwrap(),
        "new"
    );
    assert_eq!(
        fs::read_to_string(temp.0.join("untouched")).unwrap(),
        "original"
    );
}
