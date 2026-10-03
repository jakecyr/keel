//! A developer/agent can discover, initialize, inspect, and validate a project
//! using the installed CLI alone, without reading compiler implementation files.
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

const CLI: &str = env!("CARGO_BIN_EXE_keel");
const BEGIN: &str = "<!-- keel:agent-guide:start -->";
const END: &str = "<!-- keel:agent-guide:end -->";
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Work(PathBuf);
impl Work {
    fn new() -> Self {
        for _ in 0..100 {
            let path = std::env::temp_dir().join(format!(
                "keel-agent-workflow-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("test workspace: {e}"),
            }
        }
        panic!("cannot allocate workspace")
    }
    fn path(&self, relative: &str) -> PathBuf {
        self.0.join(relative)
    }
    fn write(&self, relative: &str, contents: &str) {
        let path = self.path(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    fn read(&self, relative: &str) -> String {
        fs::read_to_string(self.path(relative)).unwrap()
    }
    fn run(&self, cwd: &str, args: &[&str], offline: bool) -> Output {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let stdout = self.path(&format!("output-{id}"));
        let stderr = self.path(&format!("error-{id}"));
        let mut command = Command::new(CLI);
        command
            .current_dir(self.path(cwd))
            .args(args)
            .stdin(Stdio::null())
            .stdout(fs::File::create(&stdout).unwrap())
            .stderr(fs::File::create(&stderr).unwrap());
        if offline {
            command
                .env("PATH", "")
                .env("CC", "/no-native-toolchain-needed");
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = ChildGuard(command.spawn().unwrap());
        let deadline = Instant::now() + Duration::from_secs(30);
        let status = loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < deadline, "CLI hung: {args:?}");
            thread::sleep(Duration::from_millis(5));
        };
        Output {
            status,
            stdout: fs::read(stdout).unwrap(),
            stderr: fs::read(stderr).unwrap(),
        }
    }
    fn json(&self, cwd: &str, args: &[&str], success: bool) -> Value {
        self.json_mode(cwd, args, success, false)
    }
    fn json_mode(&self, cwd: &str, args: &[&str], success: bool, offline: bool) -> Value {
        let output = self.run(cwd, args, offline);
        assert_eq!(
            output.status.success(),
            success,
            "{args:?}\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stderr.is_empty(),
            "JSON response leaked stderr: {output:?}"
        );
        serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|e| panic!("invalid JSON: {e}: {output:?}"))
    }
}
impl Drop for Work {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            #[cfg(unix)]
            // SAFETY: this test child is the leader of its isolated process group.
            unsafe {
                libc::kill(-(self.0.id() as i32), libc::SIGKILL);
            }
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[test]
fn help_is_discoverable_and_init_prints_next_steps() {
    let work = Work::new();
    for args in [
        vec!["--help"],
        vec!["init", "--help"],
        vec!["agent", "context", "--help"],
    ] {
        let output = work.run(".", &args, true);
        assert!(output.status.success());
        let help = String::from_utf8(output.stdout).unwrap();
        for command in [
            "keel init",
            "keel lint",
            "keel agent context",
            "--engine native|reference|both",
        ] {
            assert!(help.contains(command), "missing {command}");
        }
    }
    let output = work.run(".", &["init", "starter"], true);
    assert!(output.status.success());
    let message = String::from_utf8(output.stdout).unwrap();
    assert!(message.contains("Run these commands from"));
    assert!(message.contains("starter"));
    assert!(message.contains("keel test . --engine both"));
    assert!(!work.path("--help").exists());
}

#[test]
fn installed_binary_supplies_versioned_references_without_external_tools() {
    let work = Work::new();
    let commands = work.json_mode(".", &["agent", "commands", "--json"], true, true);
    assert_eq!(commands["schema"], 1);
    let catalog = commands["commands"].to_string();
    for command in [
        "keel init",
        "keel agent context",
        "keel check",
        "keel test",
        "keel edit",
        "keel lint",
        "keel fmt",
    ] {
        assert!(catalog.contains(command), "missing {command}");
    }
    for topic in ["language", "collections", "protocol"] {
        let reference = work.json_mode(".", &["agent", "spec", topic, "--json"], true, true);
        assert_eq!(reference["schema"], 1);
        assert_eq!(reference["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(reference["topic"], topic);
        assert!(reference["content"].as_str().unwrap().len() > 500);
        let plain = work.run(".", &["agent", "spec", topic], true);
        assert!(plain.status.success());
        assert_eq!(
            String::from_utf8(plain.stdout).unwrap().trim_end(),
            reference["content"].as_str().unwrap().trim_end()
        );
    }
    let signature = work.json_mode(".", &["api", "list.get", "--json"], true, true);
    assert_eq!(signature["name"], "list.get");
}

#[test]
fn initialize_discover_validate_build_and_run_generated_project() {
    let work = Work::new();
    work.json(".", &["init", "hello", "--json"], true);
    for name in ["AGENTS.md", "CLAUDE.md"] {
        let instructions = work.read(&format!("hello/{name}"));
        assert_eq!(instructions.matches(BEGIN).count(), 1);
        assert_eq!(instructions.matches(END).count(), 1);
        assert!(instructions.contains("keel agent context . --json"));
        assert!(instructions.contains("keel agent spec language"));
        assert!(instructions.contains("UNKNOWN"));
    }
    let context = work.json(
        "hello",
        &["agent", "context", ".", "--symbol", "greet", "--json"],
        true,
    );
    assert!(
        context["language_reference"]
            .as_str()
            .unwrap()
            .contains("read")
    );
    assert_eq!(context["context"]["functions"][0]["name"], "greet");
    assert_eq!(context["incomplete"], false);
    assert!(
        context["project"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["acceptance_protected"] == true)
    );
    work.json("hello", &["fmt", ".", "--check", "--json"], true);
    assert_eq!(
        work.json("hello", &["check", ".", "--json"], true)["status"],
        "CHECKED"
    );
    work.json("hello", &["lint", ".", "--json"], true);
    assert_eq!(
        work.json("hello", &["test", ".", "--engine", "both", "--json"], true)["status"],
        "TESTED"
    );
    assert_eq!(
        work.json("hello", &["build", ".", "-o", "build/app", "--json"], true)["status"],
        "BUILT"
    );
    let output = work.run("hello", &["run", ".", "--allow-stdout"], false);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"Hello from Keel!\n");
    let policy: Value = serde_json::from_str(&work.read("hello/keel.policy.json")).unwrap();
    assert_eq!(policy["stdout"], false);
    assert!(policy["listen"].as_array().unwrap().is_empty());
}

#[test]
fn init_current_directory_preserves_human_guidance_and_unrelated_files() {
    let work = Work::new();
    work.write(
        "existing/AGENTS.md",
        "# Human rules\nKeep our approved acceptance criteria.\n",
    );
    work.write(
        "existing/CLAUDE.md",
        "# Team guidance\nAsk before publishing.\n",
    );
    work.write("existing/.gitignore", "# Existing ignores\nprivate-data/\n");
    work.write("existing/notes.txt", "do not touch\n");
    work.json("existing", &["init", ".", "--json"], true);
    assert!(
        work.read("existing/AGENTS.md")
            .starts_with("# Human rules\nKeep our approved acceptance criteria.\n")
    );
    assert!(
        work.read("existing/CLAUDE.md")
            .starts_with("# Team guidance\nAsk before publishing.\n")
    );
    assert!(work.read("existing/.gitignore").contains("private-data/"));
    assert_eq!(work.read("existing/notes.txt"), "do not touch\n");
}

#[test]
fn reinit_updates_only_managed_blocks_and_is_idempotent() {
    let work = Work::new();
    work.json(".", &["init", "app", "--json"], true);
    let paths = [
        "src/main.keel",
        "tests/acceptance.keel",
        "keel.json",
        "keel.policy.json",
    ];
    let original: Vec<_> = paths
        .iter()
        .map(|p| work.read(&format!("app/{p}")))
        .collect();
    for filename in ["AGENTS.md", "CLAUDE.md"] {
        work.write(
            &format!("app/{filename}"),
            &format!("Human prefix 🌊\n{BEGIN}\nStale generated guidance\n{END}\nHuman suffix\n"),
        );
    }
    work.json("app", &["init", ".", "--json"], true);
    for filename in ["AGENTS.md", "CLAUDE.md"] {
        let content = work.read(&format!("app/{filename}"));
        assert!(content.starts_with("Human prefix 🌊\n"));
        assert!(content.ends_with("\nHuman suffix\n"));
        assert!(!content.contains("Stale generated guidance"));
        assert_eq!(content.matches(BEGIN).count(), 1);
        assert_eq!(content.matches(END).count(), 1);
    }
    let guidance = [work.read("app/AGENTS.md"), work.read("app/CLAUDE.md")];
    work.json("app", &["init", ".", "--json"], true);
    assert_eq!(
        guidance,
        [work.read("app/AGENTS.md"), work.read("app/CLAUDE.md")]
    );
    for (path, content) in paths.iter().zip(original) {
        assert_eq!(
            work.read(&format!("app/{path}")),
            content,
            "reinit changed {path}"
        );
    }
}

#[test]
fn init_preflights_conflicting_source_before_writing_scaffold() {
    let work = Work::new();
    for (name, conflict) in [
        ("entry", "src/main.keel"),
        ("tests", "tests/acceptance.keel"),
    ] {
        work.write(
            &format!("{name}/{conflict}"),
            "existing source must survive\n",
        );
        work.write(&format!("{name}/AGENTS.md"), "Human instructions\n");
        work.json(name, &["init", ".", "--json"], false);
        assert_eq!(
            work.read(&format!("{name}/{conflict}")),
            "existing source must survive\n"
        );
        assert_eq!(
            work.read(&format!("{name}/AGENTS.md")),
            "Human instructions\n"
        );
        assert!(!work.path(&format!("{name}/keel.json")).exists());
        assert!(!work.path(&format!("{name}/keel.policy.json")).exists());
        assert!(!work.path(&format!("{name}/CLAUDE.md")).exists());
    }
}

#[test]
fn ambiguous_managed_markers_are_rejected_without_partial_instruction_updates() {
    let work = Work::new();
    for (index, invalid) in [
        format!("Human\n{BEGIN}\nmissing end\n"),
        format!("{BEGIN}\none\n{END}\n{BEGIN}\ntwo\n{END}\n"),
        format!("{BEGIN}\n{BEGIN}\n{END}\n{END}\n"),
    ]
    .iter()
    .enumerate()
    {
        let root = format!("invalid-{index}");
        work.write(&format!("{root}/AGENTS.md"), "Human rules\n");
        work.write(&format!("{root}/CLAUDE.md"), invalid);
        work.json(&root, &["init", ".", "--json"], false);
        assert_eq!(work.read(&format!("{root}/AGENTS.md")), "Human rules\n");
        assert_eq!(work.read(&format!("{root}/CLAUDE.md")), *invalid);
        assert!(!work.path(&format!("{root}/keel.json")).exists());
    }
}

#[test]
fn agent_context_supports_bootstrap_focused_budget_and_revision_bound_edits() {
    let work = Work::new();
    let bootstrap = work.json_mode(".", &["agent", "context", "--json"], true, true);
    assert!(bootstrap["project"].is_null());
    assert!(!bootstrap["language_reference"].as_str().unwrap().is_empty());
    assert!(bootstrap["unsupported"].as_array().unwrap().len() >= 3);
    work.write("app.keel", "pub fn answer() -> Text { return \"🌊 wrong\" }\nfn unrelated() -> Int { return 7 }\ntest \"approved\" { assert answer() == \"🌊 right\" }\n");
    let full = work.json_mode(
        ".",
        &[
            "agent", "context", "app.keel", "--symbol", "answer", "--json",
        ],
        true,
        true,
    );
    assert_eq!(full["context"]["functions"].as_array().unwrap().len(), 1);
    assert_eq!(full["context"]["functions"][0]["target"], "fn:answer");
    let short = work.json_mode(
        ".",
        &[
            "agent",
            "context",
            "app.keel",
            "--symbol",
            "answer",
            "--max-chars",
            "9",
            "--json",
        ],
        true,
        true,
    );
    assert_eq!(short["incomplete"], true);
    assert_eq!(
        short["context"]["functions"][0]["source"]
            .as_str()
            .unwrap()
            .chars()
            .count(),
        9
    );
    assert_eq!(short["context"]["revision"], full["context"]["revision"]);
    work.write("edit.json", &json!({"base_revision":full["context"]["revision"],"target":"fn:answer","operation":"replace_body","source":"{ return \"🌊 right\" }","run":"affected_checks_and_tests"}).to_string());
    assert_eq!(
        work.json(
            ".",
            &["edit", "app.keel", "--request", "edit.json", "--json"],
            true
        )["status"],
        "APPLIED"
    );
    assert_eq!(
        work.json(
            ".",
            &["test", "app.keel", "--engine", "both", "--json"],
            true
        )["status"],
        "TESTED"
    );
}

#[test]
fn context_preserves_offline_reference_when_project_has_static_errors() {
    let work = Work::new();
    work.write("broken.keel", "fn answer() -> Int { return true }");
    let context = work.json_mode(
        ".",
        &["agent", "context", "broken.keel", "--json"],
        false,
        true,
    );
    assert_eq!(context["status"], "FAILED");
    assert!(
        context["language_reference"]
            .as_str()
            .unwrap()
            .contains("Int")
    );
    assert_eq!(
        context["diagnostics"]["diagnostics"][0]["kind"],
        "type_mismatch"
    );
    assert!(
        Path::new(
            context["diagnostics"]["diagnostics"][0]["file"]
                .as_str()
                .unwrap()
        )
        .ends_with("broken.keel")
    );
    let human = work.run(".", &["agent", "context", "broken.keel"], true);
    assert!(!human.status.success());
    let human_text = String::from_utf8(human.stdout).unwrap();
    assert!(human_text.contains("type_mismatch"));
    assert!(human_text.contains("language_reference"));
    for args in [
        vec!["agent", "context", "--symbol", "answer", "--json"],
        vec!["agent", "context", "broken.keel", "--unknown", "--json"],
        vec!["agent", "spec", "not-a-topic", "--json"],
        vec!["agent", "commands", "unexpected", "--json"],
    ] {
        work.json_mode(".", &args, false, true);
    }
}

#[test]
fn context_retains_bootstrap_help_when_project_file_does_not_parse() {
    let work = Work::new();
    work.json(".", &["init", "broken-project", "--json"], true);
    work.write(
        "broken-project/src/main.keel",
        "fn greet() -> Text { return",
    );
    let context = work.json_mode(
        "broken-project",
        &["agent", "context", ".", "--json"],
        false,
        true,
    );
    assert_eq!(context["status"], "FAILED");
    assert_eq!(context["version"], env!("CARGO_PKG_VERSION"));
    assert!(
        context["language_reference"]
            .as_str()
            .unwrap()
            .contains("Syntax")
    );
    assert_eq!(context["diagnostics"]["kind"], "project_load");
    assert!(
        context["diagnostics"]["message"]
            .as_str()
            .unwrap()
            .contains("main.keel")
    );
}
