use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "keel-cli-output-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::write(path.join("main.keel"), "fn main() effects { io.stdout } { io.println(\"hello\") }\nfn answer() -> Int { return 1 }\ntest \"answer\" { assert answer() == 1 }\n").unwrap();
        Self(path)
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_keel"));
        command
            .current_dir(&self.0)
            .env("TERM", "xterm-256color")
            .env_remove("NO_COLOR")
            .env_remove("CI")
            .env_remove("CC")
            .env("KEEL_COLOR", "always")
            .env_remove("KEEL_PROGRESS");
        command
    }
    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn command_reports_keep_json_clean_even_with_forced_color() {
    let fixture = Fixture::new();
    let commands: &[&[&str]] = &[
        &["init", "project"],
        &["check", "main.keel"],
        &["lint", "main.keel"],
        &["fmt", "main.keel", "--check"],
        &["inspect", "main.keel"],
        &["explain", "main.keel", "--offset", "0"],
        &["review", "main.keel", "--against", "main.keel"],
        &["build", "main.keel", "-o", "app"],
        &["test", "main.keel", "--engine", "both"],
        &["doctor"],
        &["api", "list.get"],
        &["agent", "commands"],
        &["agent", "context", "main.keel"],
        &["agent", "spec", "language"],
        &["check", "missing.keel"],
        &["not-a-command"],
    ];
    for args in commands {
        let mut args = args.to_vec();
        args.push("--json");
        let output = fixture.run(&args);
        assert!(!output.stdout.contains(&0x1b), "{args:?}");
        assert!(output.stderr.is_empty(), "{args:?}: {:?}", output.stderr);
        let value: Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|e| panic!("{args:?}: {e}: {output:?}"));
        assert!(value.is_object());
    }
}

#[test]
fn human_output_styles_help_errors_and_nested_edit_diagnostics() {
    let fixture = Fixture::new();
    for args in [
        &["--help"][..],
        &["build", "--help"],
        &["--version"],
        &["api", "list.get"],
        &["agent", "commands"],
        &["check", "main.keel"],
    ] {
        let output = fixture.run(args);
        assert!(output.status.success(), "{output:?}");
        assert!(output.stdout.contains(&0x1b), "{args:?}");
        assert!(output.stderr.is_empty());
    }
    let output = fixture.run(&["check", "missing.keel"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("[x] FAILED"));

    let checked: Value =
        serde_json::from_slice(&fixture.run(&["check", "main.keel", "--json"]).stdout).unwrap();
    fs::write(fixture.0.join("edit.json"), json!({"base_revision":checked["revision"],"target":"fn:answer","operation":"replace_body","source":"{ return false }"}).to_string()).unwrap();
    let output = fixture
        .command()
        .env("NO_COLOR", "1")
        .args(["edit", "main.keel", "--request", "edit.json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("candidate:"), "{text}");
    assert!(text.contains("applied: false"), "{text}");
    assert!(text.contains("Diagnostics: 1"), "{text}");
    assert!(text.contains("Bool"), "{text}");
    assert!(
        fs::read_to_string(fixture.0.join("main.keel"))
            .unwrap()
            .contains("return 1")
    );
}

#[test]
fn pipes_no_color_and_program_protocol_streams_remain_plain() {
    let fixture = Fixture::new();
    for (key, value) in [
        ("KEEL_COLOR", "auto"),
        ("KEEL_COLOR", "never"),
        ("NO_COLOR", "1"),
        ("TERM", "dumb"),
    ] {
        let output = fixture
            .command()
            .env(key, value)
            .args(["check", "main.keel"])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(!output.stdout.contains(&0x1b));
        assert!(output.stderr.is_empty());
    }
    let output = fixture.run(&["run", "main.keel", "--allow-stdout", "-o", "hello"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"hello\n");
    assert!(output.stderr.is_empty());

    let mut child = fixture
        .command()
        .arg("serve")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"{\"id\":1,\"method\":\"check\",\"source\":\"fn main() {}\"}\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(!output.stdout.contains(&0x1b));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["result"]["status"], "CHECKED");
}
