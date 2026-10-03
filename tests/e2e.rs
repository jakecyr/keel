//! Public CLI contracts: no access to compiler internals and no shared build paths.
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
const CLI: &str = env!("CARGO_BIN_EXE_keel");

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        for _ in 0..100 {
            let path = std::env::temp_dir().join(format!(
                "keel-e2e-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("cannot create test workspace: {e}"),
            }
        }
        panic!("cannot reserve test workspace")
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
    fn write(&self, name: &str, source: &str) -> PathBuf {
        let path = self.path(name);
        fs::write(&path, source).unwrap();
        path
    }
    fn example(&self, name: &str) -> PathBuf {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join(name);
        let target = self.path(name);
        fs::copy(source, &target).unwrap();
        target
    }
    fn command(&self, executable: impl AsRef<Path>, args: &[&str]) -> Command {
        let mut command = Command::new(executable.as_ref());
        command.current_dir(&self.0).args(args);
        command
    }
    fn run(&self, args: &[&str]) -> Output {
        self.run_command(self.command(CLI, args))
    }
    fn run_command(&self, mut command: Command) -> Output {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let stdout = self.path(&format!("stdout-{id}"));
        let stderr = self.path(&format!("stderr-{id}"));
        command
            .stdin(Stdio::null())
            .stdout(fs::File::create(&stdout).unwrap())
            .stderr(fs::File::create(&stderr).unwrap());
        let mut child = ChildGuard::spawn(&mut command);
        let deadline = Instant::now() + Duration::from_secs(30);
        let status = loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "command exceeded 30 seconds: {command:?}"
            );
            thread::sleep(Duration::from_millis(5));
        };
        Output {
            status,
            stdout: fs::read(stdout).unwrap(),
            stderr: fs::read(stderr).unwrap(),
        }
    }
    fn json(&self, args: &[&str], success: bool) -> Value {
        let output = self.run(args);
        assert_eq!(
            output.status.success(),
            success,
            "{args:?}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stderr.is_empty(),
            "JSON diagnostics leaked to stderr: {output:?}"
        );
        serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|e| panic!("invalid JSON ({e}): {output:?}"))
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct ChildGuard(Child);
impl ChildGuard {
    fn spawn(command: &mut Command) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        Self(command.spawn().unwrap())
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            // The isolated process group includes a CLI's compiler/test children.
            #[cfg(unix)]
            let _ = Command::new("kill")
                .args(["-KILL", "--", &format!("-{}", self.0.id())])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[test]
fn supplied_examples_build_and_execute_independent_native_artifacts() {
    let work = Workspace::new();
    for name in ["ownership", "web_server", "counterexample"] {
        work.example(&format!("{name}.keel"));
        let source = format!("{name}.keel");
        let binary = format!("artifacts/{name}");
        let emitted = format!("{name}.c");
        let built = work.json(
            &[
                "build", &source, "-o", &binary, "--emit-c", &emitted, "--json",
            ],
            true,
        );
        assert_eq!(built["status"], "BUILT");
        assert!(fs::metadata(work.path(&binary)).unwrap().len() > 0);
        assert!(
            fs::read_to_string(work.path(&emitted))
                .unwrap()
                .contains("int main(")
        );
    }
    // Source removal demonstrates that the native artifact has no runtime dependency on it.
    fs::remove_file(work.path("ownership.keel")).unwrap();
    let binary = work.path("artifacts/ownership");
    let output = work.run_command(work.command(&binary, &["--allow-stdout"]));
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"hello!\n");
    let denied = work.run_command(work.command(&binary, &[]));
    assert!(!denied.status.success());
    assert!(denied.stdout.is_empty());
    let diagnostic: Value = serde_json::from_slice(&denied.stderr).unwrap();
    assert_eq!(diagnostic["kind"], "permission_denied_stdout");
    let output = work.run_command(work.command(work.path("artifacts/counterexample"), &[]));
    assert!(output.status.success());
}

#[test]
fn example_tests_are_repeatable_and_count_recorded_cases() {
    let work = Workspace::new();
    work.example("web_server.keel");
    let args = [
        "test",
        "web_server.keel",
        "--cases",
        "1000",
        "--seed",
        "42",
        "--json",
    ];
    let first = work.json(&args, true);
    let second = work.json(&args, true);
    assert_eq!(first, second);
    assert_eq!(first["status"], "TESTED");
    let cases: u64 = first["tests"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["cases"].as_u64().unwrap())
        .sum();
    assert_eq!(cases, 2004);
    assert!(
        first["tests"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["status"] == "TESTED")
    );
}

#[test]
fn collections_project_builds_runs_and_checks_owned_collection_contracts() {
    let work = Workspace::new();
    work.example("collections.keel");
    let tested = work.json(
        &[
            "test",
            "collections.keel",
            "--cases",
            "1000",
            "--seed",
            "42",
            "--json",
        ],
        true,
    );
    assert_eq!(tested["status"], "TESTED");
    assert_eq!(tested["tests"].as_array().unwrap().len(), 4);
    work.json(
        &[
            "build",
            "collections.keel",
            "-o",
            "collection-app",
            "--json",
        ],
        true,
    );
    let output = work.run_command(work.command(work.path("collection-app"), &["--allow-stdout"]));
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"4\n2\n1\n");
    for source in [
        "fn main() { var values = [1] list.push(edit values, list.len(values)) }",
        "fn main() { var values = [1] for value in values { list.push(edit values, value) } }",
        "fn identity(values: read List<Int>) -> List<Int> { return values }",
        "fn main() { let values = [1] let moved = take values assert list.len(values) == 1 }",
        "fn main() { match option.some(1) { Some(value) => { assert value == 1 } } }",
    ] {
        work.write("invalid.keel", source);
        let rejected = work.json(&["check", "invalid.keel", "--json"], false);
        assert_eq!(rejected["status"], "FAILED");
        assert!(rejected["diagnostics"].is_array(), "{rejected}");
    }
}

#[test]
fn diagnostics_are_source_linked_json_and_exit_nonzero() {
    let work = Workspace::new();
    work.write(
        "invalid.keel",
        "// Unicode 🌊\nfn answer() -> Int {\n return true\n}\n",
    );
    let output = work.run(&["check", "invalid.keel", "--json"]);
    assert_eq!(output.status.code(), Some(1));
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["status"], "FAILED");
    assert_eq!(result["diagnostics"][0]["kind"], "type_mismatch");
    assert_eq!(result["diagnostics"][0]["line"], 3);
    assert!(result["revision"].as_str().unwrap().starts_with('r'));
    let missing = work.run(&["check", "missing.keel", "--json"]);
    assert_eq!(missing.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&missing.stdout).unwrap();
    assert_eq!(value["kind"], "tool_error");
}

#[test]
fn holes_block_reached_tests_and_release_builds_but_leave_other_tests_runnable() {
    let work = Workspace::new();
    work.example("holes.keel");
    let checked = work.json(&["check", "holes.keel", "--json"], false);
    assert_eq!(checked["status"], "INCOMPLETE");
    let tests = work.json(&["test", "holes.keel", "--json"], false);
    assert_eq!(tests["status"], "BLOCKED");
    assert_eq!(tests["tests"][0]["status"], "TESTED");
    assert_eq!(tests["tests"][1]["failure"]["kind"], "hole_reached");
    let selected = work.json(
        &["test", "holes.keel", "--filter", "unaffected", "--json"],
        true,
    );
    assert_eq!(selected["tests"].as_array().unwrap().len(), 1);
    let built = work.json(&["build", "holes.keel", "-o", "forbidden", "--json"], false);
    assert_eq!(built["status"], "FAILED");
    assert!(!work.path("forbidden").exists());
}

#[test]
fn shrink_and_explicit_replay_reproduce_the_same_assertion() {
    let work = Workspace::new();
    work.write(
        "property.keel",
        "property \"under ten\" (n in gen.int(min: 0, max: 1000)) { assert n < 10 }",
    );
    let result = work.json(
        &[
            "test",
            "property.keel",
            "--cases",
            "100",
            "--seed",
            "42",
            "--json",
        ],
        false,
    );
    let failure = &result["tests"][0]["failure"];
    assert_eq!(failure["kind"], "assertion_failure");
    assert_eq!(failure["value"], 10);
    assert_eq!(failure["shrunk"], true);
    assert_eq!(failure["replay"]["revision"], result["revision"]);
    let replay = work.json(
        &[
            "test",
            "property.keel",
            "--filter",
            "under ten",
            "--value",
            "10",
            "--json",
        ],
        false,
    );
    assert_eq!(replay["tests"][0]["cases"], 1);
    assert_eq!(replay["tests"][0]["failure"]["offset"], failure["offset"]);
    assert_eq!(replay["tests"][0]["failure"]["value"], failure["value"]);
    let pass = work.json(
        &[
            "test",
            "property.keel",
            "--filter",
            "under ten",
            "--value",
            "9",
            "--json",
        ],
        true,
    );
    assert_eq!(pass["tests"][0]["cases"], 1);
    let outside = work.json(
        &[
            "test",
            "property.keel",
            "--filter",
            "under ten",
            "--value",
            "-1",
            "--json",
        ],
        false,
    );
    assert_eq!(outside["kind"], "tool_error");
    let empty = work.json(
        &[
            "test",
            "property.keel",
            "--filter",
            "does not exist",
            "--json",
        ],
        false,
    );
    assert_eq!(empty["status"], "UNKNOWN");
    assert!(empty["tests"].as_array().unwrap().is_empty());
}

#[test]
fn worker_timeouts_and_empty_suites_are_unknown_instead_of_passed() {
    let work = Workspace::new();
    work.write(
        "loop.keel",
        "test \"loop\" { while true {} } test \"next\" { assert true }",
    );
    let started = Instant::now();
    let result = work.json(
        // This tests termination/continuation, not host startup performance.
        &["test", "loop.keel", "--timeout-ms", "500", "--json"],
        false,
    );
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(result["status"], "UNKNOWN");
    assert_eq!(result["tests"][0]["failure"]["kind"], "execution_limit");
    assert_eq!(result["tests"][1]["status"], "TESTED");
    work.write("empty.keel", "fn main() {}");
    let empty = work.json(&["test", "empty.keel", "--json"], false);
    assert_eq!(empty["status"], "UNKNOWN");
    for args in [
        ["--cases", "0"],
        ["--cases", "1000001"],
        ["--timeout-ms", "0"],
        ["--timeout-ms", "60001"],
    ] {
        let invalid = work.json(&["test", "empty.keel", args[0], args[1], "--json"], false);
        assert_eq!(invalid["kind"], "tool_error");
    }
}

#[test]
fn agent_inspect_edit_test_review_roundtrip_preserves_approved_source() {
    let work = Workspace::new();
    let original = "// Approved contract and test must survive.\npub fn answer() -> Int ensures result == 42 { return 0 }\nfn main() {}\ntest \"approved answer\" { assert answer() == 42 }\n";
    work.write("app.keel", original);
    work.write("baseline.keel", original);
    let inspected = work.json(
        &["inspect", "app.keel", "--symbol", "answer", "--json"],
        true,
    );
    assert_eq!(inspected["functions"][0]["target"], "fn:answer");
    assert_eq!(inspected["functions"][0]["contracts"]["ensures"], 1);
    let request = json!({"base_revision":inspected["revision"],"target":"fn:answer","operation":"replace_body","source":"{ return 42 }","run":"affected_checks_and_tests"});
    work.write("edit.json", &request.to_string());
    let edited = work.json(
        &["edit", "app.keel", "--request", "edit.json", "--json"],
        true,
    );
    assert_eq!(edited["status"], "APPLIED");
    assert_eq!(edited["evidence"]["status"], "TESTED");
    assert_eq!(
        fs::read_to_string(work.path("app.keel")).unwrap(),
        original.replace("{ return 0 }", "{ return 42 }")
    );
    assert_ne!(edited["revision"], inspected["revision"]);
    let tested = work.json(&["test", "app.keel", "--json"], true);
    assert_eq!(tested["revision"], edited["revision"]);
    let review = work.json(
        &["review", "app.keel", "--against", "baseline.keel", "--json"],
        true,
    );
    assert_eq!(review["changes"].as_array().unwrap().len(), 1);
    assert_eq!(review["changes"][0]["body_changed"], true);
    assert_eq!(review["changes"][0]["interface_or_contract_changed"], false);
    let stale = work.json(
        &["edit", "app.keel", "--request", "edit.json", "--json"],
        false,
    );
    assert!(
        stale["message"]
            .as_str()
            .unwrap()
            .contains("stale_revision")
    );
    assert_eq!(
        fs::read_to_string(work.path("app.keel")).unwrap(),
        original.replace("{ return 0 }", "{ return 42 }")
    );
}

#[test]
fn invalid_failed_blocked_timed_out_and_escaping_edits_roll_back_atomically() {
    let work = Workspace::new();
    let original = "pub fn answer() -> Int ensures result == 42 { return 42 }\ntest \"approved\" { assert answer() == 42 }\n";
    work.write("app.keel", original);
    let revision = work.json(&["inspect", "app.keel", "--json"], true)["revision"].clone();
    for body in [
        "{ return true }",
        "{ return 0 }",
        "{ return hole(\"unfinished\") }",
        "{ while true {} return 42 }",
        "{ return 42 } test \"bypass\" { assert true }",
        "{ return 42 } // swallow remaining source",
    ] {
        work.write("edit.json", &json!({"base_revision":revision,"target":"fn:answer","operation":"replace_body","source":body,"run":"affected_checks_and_tests"}).to_string());
        let result = work.json(
            &[
                "edit",
                "app.keel",
                "--request",
                "edit.json",
                "--timeout-ms",
                "40",
                "--filter",
                "nothing",
                "--value",
                "42",
                "--json",
            ],
            false,
        );
        assert_eq!(result["status"], "FAILED", "{body}: {result}");
        assert_eq!(
            fs::read_to_string(work.path("app.keel")).unwrap(),
            original,
            "{body}"
        );
        assert!(fs::read_dir(&work.0).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("keel-tmp")
        }));
    }
    work.write("edit.json", &json!({"base_revision":revision,"target":"fn:answer","operation":"replace_test","source":"{ assert true }"}).to_string());
    work.json(
        &["edit", "app.keel", "--request", "edit.json", "--json"],
        false,
    );
    assert_eq!(fs::read_to_string(work.path("app.keel")).unwrap(), original);
}

#[test]
fn inspect_reports_truncation_and_explain_validates_utf8_boundaries() {
    let work = Workspace::new();
    let source = "// 🌊\npub fn answer() -> Int { return 42 }\n";
    work.write("app.keel", source);
    let full = work.json(
        &["inspect", "app.keel", "--symbol", "answer", "--json"],
        true,
    );
    assert_eq!(full["incomplete"], false);
    let small = work.json(
        &[
            "inspect",
            "app.keel",
            "--symbol",
            "answer",
            "--max-chars",
            "9",
            "--json",
        ],
        true,
    );
    assert_eq!(small["incomplete"], true);
    assert_eq!(
        small["functions"][0]["source"]
            .as_str()
            .unwrap()
            .chars()
            .count(),
        9
    );
    let explained = work.json(&["explain", "app.keel", "--offset", "3", "--json"], true);
    assert_eq!(explained["line"], 1);
    work.json(&["explain", "app.keel", "--offset", "4", "--json"], false);
    work.json(
        &["explain", "app.keel", "--offset", "99999", "--json"],
        false,
    );
}

#[test]
fn build_outputs_cannot_alias_source_or_each_other() {
    let work = Workspace::new();
    let source = "fn main() {}\n";
    work.write("app.keel", source);
    for args in [
        vec!["build", "app.keel", "-o", "app.keel", "--json"],
        vec!["build", "app.keel", "-o", "./app.keel", "--json"],
        vec![
            "build", "app.keel", "-o", "binary", "--emit-c", "app.keel", "--json",
        ],
        vec![
            "build", "app.keel", "-o", "binary", "--emit-c", "./binary", "--json",
        ],
    ] {
        work.json(&args, false);
        assert_eq!(fs::read_to_string(work.path("app.keel")).unwrap(), source);
    }
    fs::hard_link(work.path("app.keel"), work.path("hardlink")).unwrap();
    work.json(&["build", "app.keel", "-o", "hardlink", "--json"], false);
    assert_eq!(fs::read_to_string(work.path("app.keel")).unwrap(), source);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(work.path("app.keel"), work.path("symlink")).unwrap();
        work.json(&["build", "app.keel", "-o", "symlink", "--json"], false);
        work.json(
            &[
                "build", "app.keel", "-o", "binary", "--emit-c", "symlink", "--json",
            ],
            false,
        );
        assert_eq!(fs::read_to_string(work.path("app.keel")).unwrap(), source);
    }
}

#[test]
fn unknown_duplicate_and_missing_cli_options_are_errors() {
    let work = Workspace::new();
    work.write("app.keel", "fn main() {}\n");
    for args in [
        vec!["check", "app.keel", "--typo", "--json"],
        vec!["test", "app.keel", "--cases", "1", "--cases", "2", "--json"],
        vec!["test", "app.keel", "--cases", "--json"],
        vec!["test", "app.keel", "--filter", "--json"],
        vec!["build", "app.keel", "-o", "--json"],
    ] {
        let result = work.json(&args, false);
        assert_eq!(result["kind"], "tool_error");
    }
}

#[test]
fn replay_requires_one_property_and_exact_filter_precedes_substring_matching() {
    let work = Workspace::new();
    work.write("app.keel", "test \"ordinary\" { assert true }\nproperty \"range\" (n in gen.int(min: 0, max: 10)) { assert n >= 0 }\nproperty \"range extended\" (n in gen.int(min: 0, max: 10)) { assert n >= 0 }\n");
    for args in [
        vec!["test", "app.keel", "--value", "1", "--json"],
        vec![
            "test", "app.keel", "--value", "1", "--filter", "ordinary", "--json",
        ],
    ] {
        let result = work.json(&args, false);
        assert_eq!(result["kind"], "tool_error");
    }
    let exact = work.json(
        &[
            "test", "app.keel", "--value", "1", "--filter", "range", "--json",
        ],
        true,
    );
    assert_eq!(exact["tests"].as_array().unwrap().len(), 1);
    assert_eq!(exact["tests"][0]["name"], "range");
}

#[cfg(unix)]
#[test]
fn native_compiler_hangs_are_bounded_and_do_not_publish_artifacts() {
    use std::os::unix::fs::PermissionsExt;
    let work = Workspace::new();
    work.write("app.keel", "fn main() {}\n");
    let fake_cc = work.write("fake-cc", "#!/bin/sh\nwhile :; do :; done\n");
    fs::set_permissions(&fake_cc, fs::Permissions::from_mode(0o700)).unwrap();
    let mut command = work.command(CLI, &["build", "app.keel", "-o", "binary", "--json"]);
    command
        .env("CC", &fake_cc)
        .env("KEEL_BUILD_TIMEOUT_MS", "100");
    let start = Instant::now();
    let output = work.run_command(command);
    assert!(start.elapsed() < Duration::from_secs(5));
    assert_eq!(output.status.code(), Some(2));
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(error["kind"], "tool_error");
    assert!(
        error["message"].as_str().unwrap().contains("limit"),
        "{error}"
    );
    assert!(!work.path("binary").exists());
    assert_eq!(
        fs::read_to_string(work.path("app.keel")).unwrap(),
        "fn main() {}\n"
    );
}

#[test]
fn suite_budget_caps_combined_work_and_tests_cannot_request_external_effects() {
    let work = Workspace::new();
    work.write("budget.keel", "test \"first\" { while true {} }\ntest \"second\" { while true {} }\ntest \"last\" { assert true }\n");
    let started = Instant::now();
    let limited = work.json(
        &[
            "test",
            "budget.keel",
            "--timeout-ms",
            "1000",
            "--budget-ms",
            "1",
            "--json",
        ],
        false,
    );
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(limited["status"], "UNKNOWN");
    assert_eq!(limited["limits"]["suite_ms"], 1);
    assert_eq!(limited["tests"][2]["status"], "UNKNOWN");
    assert_eq!(
        limited["tests"][2]["failure"]["kind"],
        "suite_execution_limit"
    );
    assert_eq!(limited["tests"][2]["cases"], 0);
    work.write("permission.keel", "fn write() effects { io.stdout } { io.println(\"secret\") }\ntest \"permission\" { write() }\n");
    let permission = work.json(&["test", "permission.keel", "--json"], false);
    assert_eq!(permission["status"], "FAILED");
    assert_eq!(permission["diagnostics"][0]["kind"], "effect_not_allowed");
    assert_eq!(
        limited["limits"]["memory_enforced"],
        cfg!(target_os = "linux")
    );
}

#[cfg(target_os = "linux")]
#[test]
fn linux_memory_limit_is_unknown_and_does_not_skip_following_tests() {
    let work = Workspace::new();
    work.write("allocation.keel", "test \"allocation\" { var data = text.clone(\"a\") while true { data = text.concat(data, data) } }\ntest \"after\" { assert true }\n");
    let limited = work.json(
        &[
            "test",
            "allocation.keel",
            "--memory-mib",
            "32",
            "--timeout-ms",
            "2000",
            "--json",
        ],
        false,
    );
    assert_eq!(limited["status"], "UNKNOWN");
    assert_eq!(limited["tests"][0]["failure"]["kind"], "allocation_failed");
    assert_eq!(limited["tests"][1]["status"], "TESTED");
    assert_eq!(limited["limits"]["memory_enforced"], true);
}

#[cfg(unix)]
#[test]
fn excessive_backend_diagnostics_are_drained_and_bounded() {
    use std::os::unix::fs::PermissionsExt;
    let work = Workspace::new();
    work.write("app.keel", "fn main() {}\n");
    let fake_cc = work.write("fake-cc", "#!/bin/sh\ni=0\nwhile [ \"$i\" -lt 10000 ]; do\n  printf 'compiler error compiler error compiler error compiler error compiler error compiler error\\n' >&2\n  i=$((i + 1))\ndone\nexit 1\n");
    fs::set_permissions(&fake_cc, fs::Permissions::from_mode(0o700)).unwrap();
    let mut command = work.command(CLI, &["build", "app.keel", "-o", "binary", "--json"]);
    command.env("CC", fake_cc);
    let output = work.run_command(command);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        output.stdout.len() < 100_000,
        "backend diagnostics escaped capture limit"
    );
    let failure: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(failure["kind"], "tool_error");
    assert!(!work.path("binary").exists());
}

#[test]
fn input_size_and_recursive_syntax_have_structured_resource_limits() {
    let work = Workspace::new();
    work.write(
        "large.keel",
        &format!("//{}\nfn main() {{}}", "x".repeat(4 * 1024 * 1024)),
    );
    let large = work.json(&["check", "large.keel", "--json"], false);
    assert_eq!(large["kind"], "tool_error");
    assert!(large["message"].as_str().unwrap().contains("input_limit"));
    let samples = [
        format!(
            "fn main() {{ let n = {}1{} }}",
            "(".repeat(1000),
            ")".repeat(1000)
        ),
        format!(
            "fn main() {{ {}assert true {} }}",
            "if true { ".repeat(1000),
            "}".repeat(1000)
        ),
        format!("fn main() {{ let n = {}1 }}", "1 + ".repeat(1000)),
        format!("fn main() {{ let n = {}true }}", "!".repeat(1000)),
    ];
    for source in samples {
        work.write("deep.keel", &source);
        let limited = work.json(&["check", "deep.keel", "--json"], false);
        assert_eq!(
            limited["diagnostics"][0]["kind"], "resource_limit",
            "{limited}"
        );
    }
    fs::write(work.path("invalid-utf8.keel"), [0xff, 0xfe]).unwrap();
    let invalid = work.json(&["check", "invalid-utf8.keel", "--json"], false);
    assert_eq!(invalid["kind"], "tool_error");
}

#[test]
fn malformed_source_corpus_never_panics_or_emits_unstructured_diagnostics() {
    let work = Workspace::new();
    let source = "// Unicode 🌊\npub fn answer(value: Int) -> Int ensures result >= 0 { if value < 0 { return -value } return value }\ntest \"answer\" { assert answer(2) == 2 }\n";
    // Token and delimiter deletions at UTF-8 boundaries exercise both lexer and parser.
    let boundaries: Vec<_> = source.char_indices().map(|(at, _)| at).collect();
    for index in (0..boundaries.len()).step_by(5) {
        let start = boundaries[index];
        let end = boundaries.get(index + 3).copied().unwrap_or(source.len());
        let mutated = format!("{}{}", &source[..start], &source[end..]);
        work.write("mutated.keel", &mutated);
        let output = work.run(&["check", "mutated.keel", "--json"]);
        assert!(
            matches!(output.status.code(), Some(0..=2)),
            "compiler crashed: {output:?}"
        );
        assert!(output.stderr.is_empty(), "{output:?}");
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(result["status"].is_string(), "{result}");
    }
}

fn response(port: u16, fragments: &[&[u8]]) -> String {
    let mut stream =
        TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(2)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    for fragment in fragments {
        stream.write_all(fragment).unwrap();
        if fragments.len() > 1 {
            thread::sleep(Duration::from_millis(5));
        }
    }
    // A bounded server may already have closed after rejecting an oversized
    // request. Still read and validate its full response below.
    if let Err(error) = stream.shutdown(Shutdown::Write) {
        assert_eq!(error.kind(), std::io::ErrorKind::NotConnected);
    }
    let mut result = Vec::new();
    match stream.read_to_end(&mut result) {
        Ok(_) => (),
        // Closing a bounded request with unread excess bytes may reset TCP after the response.
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset && !result.is_empty() => (),
        Err(e) => panic!("reading HTTP response: {e}"),
    }
    String::from_utf8(result).unwrap()
}

#[test]
fn native_web_project_handles_fragmentation_invalid_requests_and_permissions() {
    let work = Workspace::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let source = include_str!("../examples/web_server.keel")
        .replace("http.serve(8080,", &format!("http.serve({port},"));
    work.write("web.keel", &source);
    work.json(&["build", "web.keel", "-o", "server", "--json"], true);
    let binary = work.path("server");
    for args in [
        vec![],
        vec!["--allow-net=0.0.0.0:8080"],
        vec!["--allow-stdout"],
    ] {
        let denied = work.run_command(work.command(&binary, &args));
        assert!(!denied.status.success());
        let error: Value = serde_json::from_slice(&denied.stderr).unwrap();
        assert_eq!(error["kind"], "permission_denied_net");
    }
    drop(listener);
    let mut command = work.command(&binary, &[&format!("--allow-net=127.0.0.1:{port}")]);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut server = ChildGuard::spawn(&mut command);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(
            server.0.try_wait().unwrap().is_none(),
            "server exited before readiness"
        );
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        assert!(Instant::now() < deadline, "server never listened");
        thread::sleep(Duration::from_millis(10));
    }
    for (path, status, body) in [
        ("/", 200, "Hello from Keel!\n"),
        ("/health?ready=1", 200, "ok\n"),
        ("/square", 200, "144\n"),
        ("/missing", 404, "not found\n"),
    ] {
        let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n");
        let actual = response(
            port,
            &[
                &request.as_bytes()[..2],
                &request.as_bytes()[2..request.len() - 2],
                b"\r\n",
            ],
        );
        assert!(
            actual.starts_with(&format!("HTTP/1.1 {status} ")),
            "{actual}"
        );
        assert_eq!(actual.split_once("\r\n\r\n").unwrap().1, body);
        assert!(actual.contains(&format!("Content-Length: {}\r\n", body.len())));
    }
    let oversized = format!("GET / HTTP/1.1\r\nX-Large: {}", "a".repeat(17000));
    for (request, status) in [
        (b"nonsense\r\n\r\n".as_slice(), 400),
        (b"GET / HTTP/2.0\r\n\r\n", 400),
        (b"GET / HTTP/1.1\r\n", 400),
        (b"GET /\0 HTTP/1.1\r\n\r\n", 400),
        (b"POST / HTTP/1.1\r\n\r\n", 405),
        (oversized.as_bytes(), 400),
    ] {
        let actual = response(port, &[request]);
        assert!(
            actual.starts_with(&format!("HTTP/1.1 {status} ")),
            "{actual}"
        );
        assert!(server.0.try_wait().unwrap().is_none());
    }
    let final_response = response(port, &[b"GET /health HTTP/1.0\r\n\r\n"]);
    assert!(final_response.ends_with("ok\n"));
}
