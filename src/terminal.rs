//! Human-facing presentation only. Protocols and program streams bypass this module.
use crate::project::Project;
use serde_json::Value;
use std::{
    env,
    fmt::Write as _,
    io::{self, IsTerminal, Write},
    sync::mpsc::{self, Sender},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const TEAL: &str = "1;36";
const AMBER: &str = "1;33";
const GREEN: &str = "1;32";
const RED: &str = "1;31";
const MUTED: &str = "2";

#[derive(Clone, Copy)]
struct Style {
    color: bool,
}

struct Environment {
    color: Option<String>,
    no_color: bool,
    ci: bool,
    dumb: bool,
    progress_off: bool,
}

impl Environment {
    fn read() -> Self {
        Self {
            color: env::var("KEEL_COLOR").ok(),
            no_color: env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()),
            ci: env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0"),
            dumb: env::var("TERM").is_ok_and(|v| v == "dumb"),
            progress_off: env::var("KEEL_PROGRESS").is_ok_and(|v| v == "off"),
        }
    }

    fn style(&self, tty: bool, machine: bool) -> Style {
        Style {
            color: !machine
                && !self.no_color
                && !self.dumb
                && match self.color.as_deref() {
                    Some("never") => false,
                    Some("always") => true,
                    _ => tty && !self.ci,
                },
        }
    }

    fn animate(&self, stdout_tty: bool, stderr_tty: bool, machine: bool) -> bool {
        !machine && stdout_tty && stderr_tty && !self.ci && !self.dumb && !self.progress_off
    }
}

impl Style {
    fn paint(self, text: &str, color: &str) -> String {
        let text = clean(text);
        if self.color {
            format!("\x1b[{color}m{text}\x1b[0m")
        } else {
            text
        }
    }

    fn status(self, status: &str) -> String {
        let (mark, color) = match status {
            "FAILED" => ("x", RED),
            "BLOCKED" | "UNKNOWN" | "INCOMPLETE" | "WARNING" => ("!", AMBER),
            "CHECKED" | "TESTED" | "ENFORCED" | "BUILT" | "READY" | "LINTED" | "FORMATTED"
            | "APPLIED" | "INITIALIZED" | "CREATED" => ("+", GREEN),
            _ => (">", TEAL),
        };
        self.paint(&format!("[{mark}] {status}"), color)
    }
}

// Source, paths and compiler messages must not be able to inject terminal controls.
fn clean(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\n' => out.push('\n'),
            '\t' => out.push_str("    "),
            c if c.is_control() => out.extend(c.escape_default()),
            c => out.push(c),
        }
    }
    out
}

fn scalar(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        _ => value.to_string(),
    }
}

struct Renderer<'a> {
    style: Style,
    project: Option<&'a Project>,
    out: String,
}

impl Renderer<'_> {
    fn line(&mut self, depth: usize, text: &str) {
        let _ = writeln!(self.out, "{}{text}", "  ".repeat(depth));
    }

    fn document(&mut self, content: &str, depth: usize) {
        let mut code = false;
        for line in content.lines() {
            let color = if line.trim_start().starts_with("```") {
                code = !code;
                MUTED
            } else if code || line.trim_start().starts_with("keel ") {
                TEAL
            } else if line.starts_with('#') || line.ends_with(':') {
                AMBER
            } else {
                "0"
            };
            self.line(depth, &self.style.paint(line, color));
        }
    }

    fn diagnostic(&mut self, value: &Value, depth: usize) {
        let warning = value["severity"] == "warning";
        let kind = value["kind"].as_str().unwrap_or("error");
        self.line(
            depth,
            &format!(
                "{} {}",
                self.style
                    .status(if warning { "WARNING" } else { "FAILED" }),
                self.style.paint(kind, if warning { AMBER } else { RED })
            ),
        );
        if let Some(message) = value["message"].as_str() {
            self.document(message, depth + 1);
        }
        if let Some(line) = value["line"].as_u64() {
            let file = value["file"].as_str().unwrap_or("source");
            self.line(
                depth + 1,
                &self
                    .style
                    .paint(&format!("--> {file}:{line}:{}", value["column"]), TEAL),
            );
            let source = self.project.and_then(|project| {
                project
                    .parts
                    .iter()
                    .find(|part| part.path.to_str() == Some(file))
            });
            if let Some(source_line) = source.and_then(|part| {
                usize::try_from(line)
                    .ok()
                    .and_then(|n| n.checked_sub(1))
                    .and_then(|n| part.source.lines().nth(n))
            }) {
                // Clip long lines around the diagnostic; never fill the screen with source.
                let column = value["column"].as_u64().unwrap_or(1).saturating_sub(1) as usize;
                let chars: Vec<_> = source_line.chars().collect();
                let column = column.min(chars.len());
                let start = column.saturating_sub(40);
                let end = (start + 100).min(chars.len());
                let prefix = if start > 0 { "..." } else { "" };
                let snippet: String = chars[start..end].iter().collect();
                let before: String = chars[start..column].iter().collect();
                let gutter = line.to_string().len();
                self.line(
                    depth + 1,
                    &format!(
                        "{line} | {prefix}{}{}",
                        clean(&snippet),
                        if end < chars.len() { "..." } else { "" }
                    ),
                );
                self.line(
                    depth + 1,
                    &format!(
                        "{} | {}{}",
                        " ".repeat(gutter),
                        " ".repeat(prefix.len() + clean(&before).chars().count()),
                        self.style.paint("^", if warning { AMBER } else { RED })
                    ),
                );
            }
        }
    }

    fn field(&mut self, key: &str, value: &Value, depth: usize) {
        let label = self.style.paint(
            &if key == "language_reference" {
                key.to_owned()
            } else {
                key.replace('_', " ")
            },
            TEAL,
        );
        match value {
            Value::Object(map) if !map.is_empty() => {
                self.line(depth, &format!("{label}:"));
                // A rejected edit refers to candidate source, not the loaded files.
                let project = if key == "candidate" {
                    self.project.take()
                } else {
                    None
                };
                self.object(value, depth + 1);
                if key == "candidate" {
                    self.project = project;
                }
            }
            Value::Array(items) if !items.is_empty() => {
                self.line(depth, &format!("{label} ({}):", items.len()));
                for item in items {
                    if item.is_object() {
                        self.line(depth + 1, &self.style.paint("--", MUTED));
                        self.object(item, depth + 1);
                    } else {
                        self.document(&format!("- {}", scalar(item)), depth + 1);
                    }
                }
            }
            Value::String(s) if s.contains('\n') => {
                self.line(depth, &format!("{label}:"));
                self.document(s, depth + 1);
            }
            _ => self.line(depth, &format!("{label}: {}", clean(&scalar(value)))),
        }
    }

    fn object(&mut self, value: &Value, depth: usize) {
        let Some(map) = value.as_object() else {
            self.document(&scalar(value), depth);
            return;
        };
        if let Some(status) = value["status"].as_str() {
            let revision = value["revision"].as_str().unwrap_or("");
            self.line(
                depth,
                &format!(
                    "{}  {}",
                    self.style.status(status),
                    self.style.paint(revision, MUTED)
                ),
            );
        }
        if let Some(message) = value["message"].as_str() {
            self.document(message, depth);
        }
        if let Some(diagnostics) = value["diagnostics"].as_array() {
            self.line(
                depth,
                &self
                    .style
                    .paint(&format!("Diagnostics: {}", diagnostics.len()), AMBER),
            );
            for diagnostic in diagnostics {
                self.diagnostic(diagnostic, depth + 1);
            }
        }
        if let Some(directory) = value["working_directory"].as_str() {
            self.line(
                depth,
                &self
                    .style
                    .paint(&format!("Run these commands from {directory}:"), AMBER),
            );
            if let Some(commands) = value["next"].as_array() {
                for command in commands.iter().filter_map(Value::as_str) {
                    self.document(command, depth + 1);
                }
            }
        }
        for (key, item) in map {
            if key == "status"
                || key == "message"
                || (key == "revision" && value["status"].is_string())
                || (key == "diagnostics" && item.is_array())
                || (matches!(key.as_str(), "working_directory" | "next")
                    && value["working_directory"].is_string())
            {
                continue;
            }
            if key == "tests"
                && let Some(tests) = item.as_array()
                && tests.iter().all(|test| test["status"].is_string())
            {
                self.line(
                    depth,
                    &self.style.paint(&format!("Tests: {}", tests.len()), TEAL),
                );
                for test in tests {
                    self.line(
                        depth + 1,
                        &format!(
                            "{}  {} ({} cases)",
                            self.style.status(test["status"].as_str().unwrap()),
                            clean(test["name"].as_str().unwrap_or("")),
                            test["cases"]
                        ),
                    );
                    if let Some(details) = test.as_object() {
                        for (key, value) in details {
                            if !matches!(key.as_str(), "status" | "name" | "cases") {
                                self.field(key, value, depth + 2);
                            }
                        }
                    }
                }
            } else {
                self.field(key, item, depth);
            }
        }
    }
}

fn render(value: &Value, style: Style, project: Option<&Project>) -> String {
    let mut renderer = Renderer {
        style,
        project,
        out: String::new(),
    };
    renderer.line(
        0,
        &style.paint(" KEEL // --------------------------------", TEAL),
    );
    renderer.object(value, 1);
    renderer.out
}

pub fn report(value: &Value, json: bool, project: Option<&Project>) {
    if json {
        println!("{}", serde_json::to_string_pretty(value).unwrap());
    } else {
        print!(
            "{}",
            render(
                value,
                Environment::read().style(io::stdout().is_terminal(), false),
                project
            )
        );
    }
}

pub fn document(content: &str) {
    let style = Environment::read().style(
        io::stdout().is_terminal(),
        env::args().any(|arg| arg == "--json"),
    );
    let mut renderer = Renderer {
        style,
        project: None,
        out: String::new(),
    };
    renderer.line(
        0,
        &style.paint(" KEEL // --------------------------------", TEAL),
    );
    renderer.document(content, 0);
    print!("{}", renderer.out);
}

pub fn reference(content: &str) {
    // Redirected offline references remain byte-for-byte usable as Markdown.
    if !Environment::read()
        .style(io::stdout().is_terminal(), false)
        .color
    {
        println!("{content}");
    } else {
        document(content);
    }
}

pub fn error(message: &str) {
    let style = Environment::read().style(io::stderr().is_terminal(), false);
    eprint!(
        "{}",
        render(
            &serde_json::json!({"status":"FAILED", "message":message}),
            style,
            None
        )
    );
}

/// A transient activity indicator, never a fabricated percentage. Drop joins the
/// worker and clears its line before reports, errors, or child output are written.
pub struct Progress {
    worker: Option<(Sender<&'static str>, JoinHandle<()>)>,
}

impl Progress {
    pub fn new(machine: bool, label: &'static str) -> Self {
        let environment = Environment::read();
        if !environment.animate(
            io::stdout().is_terminal(),
            io::stderr().is_terminal(),
            machine,
        ) {
            return Self { worker: None };
        }
        let style = environment.style(true, false);
        let (send, receive) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("keel-progress".into())
            .spawn(move || {
                let start = Instant::now();
                let mut label = label;
                let mut frame = 0;
                let mut drawn = false;
                loop {
                    match receive.recv_timeout(Duration::from_millis(90)) {
                        Ok(next) => label = next,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    if start.elapsed() < Duration::from_millis(120) {
                        continue;
                    }
                    let text = progress_frame(label, frame, start.elapsed(), terminal_width());
                    let mut stderr = io::stderr().lock();
                    if write!(stderr, "\r\x1b[2K{}", style.paint(&text, TEAL))
                        .and_then(|_| stderr.flush())
                        .is_err()
                    {
                        break;
                    }
                    drawn = true;
                    frame += 1;
                }
                if drawn {
                    let mut stderr = io::stderr().lock();
                    let _ = write!(stderr, "\r\x1b[2K").and_then(|_| stderr.flush());
                }
            });
        Self {
            worker: worker.ok().map(|worker| (send, worker)),
        }
    }

    pub fn stage(&self, label: &'static str) {
        if let Some((send, _)) = &self.worker {
            let _ = send.send(label);
        }
    }

    pub fn finish(&mut self) {
        if let Some((send, worker)) = self.worker.take() {
            drop(send);
            let _ = worker.join();
        }
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        self.finish();
    }
}

fn progress_frame(label: &str, frame: usize, elapsed: Duration, width: usize) -> String {
    let frames = [
        "[=   ]", "[==  ]", "[ == ]", "[  ==]", "[   =]", "[  ==]", "[ == ]", "[==  ]",
    ];
    format!(
        " {} {label}  {:.1}s",
        frames[frame % frames.len()],
        elapsed.as_secs_f64()
    )
    .chars()
    .take(width.saturating_sub(1))
    .collect()
}

fn terminal_width() -> usize {
    #[cfg(unix)]
    {
        let mut size = std::mem::MaybeUninit::<libc::winsize>::zeroed();
        // SAFETY: ioctl writes a winsize into valid, correctly aligned storage.
        if unsafe { libc::ioctl(libc::STDERR_FILENO, libc::TIOCGWINSZ, size.as_mut_ptr()) } == 0 {
            // SAFETY: successful TIOCGWINSZ initialized the structure.
            let width = unsafe { size.assume_init() }.ws_col;
            if width > 0 {
                return usize::from(width);
            }
        }
    }
    80
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn environment() -> Environment {
        Environment {
            color: None,
            no_color: false,
            ci: false,
            dumb: false,
            progress_off: false,
        }
    }

    #[test]
    fn terminal_policy_respects_protocols_pipes_and_accessibility() {
        let mut env = environment();
        assert!(env.style(true, false).color);
        assert!(!env.style(false, false).color);
        assert!(env.animate(true, true, false));
        for (stdout, stderr, machine) in [
            (false, true, false),
            (true, false, false),
            (true, true, true),
        ] {
            assert!(!env.animate(stdout, stderr, machine));
        }
        env.color = Some("always".into());
        assert!(env.style(false, false).color);
        assert!(!env.style(true, true).color);
        env.no_color = true;
        assert!(!env.style(true, false).color);
        env.no_color = false;
        env.dumb = true;
        assert!(!env.style(true, false).color);
        assert!(!env.animate(true, true, false));
        env = environment();
        env.ci = true;
        assert!(!env.style(true, false).color);
        assert!(!env.animate(true, true, false));
        env.ci = false;
        env.progress_off = true;
        assert!(env.style(true, false).color);
        assert!(!env.animate(true, true, false));
    }

    #[test]
    fn human_reports_preserve_uncertain_evidence_and_nested_failures() {
        let report = json!({
            "status":"FAILED", "applied":false,
            "candidate":{"status":"BLOCKED", "tests":[
                {"status":"UNKNOWN", "name":"limits", "cases":3,
                 "failure":{"kind":"suite_execution_limit", "budget_ms":20}},
                {"status":"TESTED", "name":"sample", "cases":1}
            ], "assurance":"Sampled evidence only; never PROVEN"}
        });
        let plain = render(&report, Style { color: false }, None);
        for evidence in [
            "[x] FAILED",
            "[!] BLOCKED",
            "[!] UNKNOWN",
            "[+] TESTED",
            "suite_execution_limit",
            "budget ms: 20",
            "applied: false",
            "never PROVEN",
        ] {
            assert!(plain.contains(evidence), "{plain}");
        }
        assert!(!plain.contains('\x1b'));
        let colored = render(&report, Style { color: true }, None);
        assert!(colored.contains("\x1b[1;33m[!] UNKNOWN\x1b[0m"));
    }

    #[test]
    fn source_diagnostics_expand_tabs_and_escape_terminal_controls() {
        let source = "fn main() {\n\tlet unused = 1\n}\n";
        let project = Project {
            source: source.into(),
            parts: vec![crate::project::Part {
                path: "example.keel".into(),
                source: source.into(),
                start: 0,
                end: source.len(),
                protected: false,
            }],
            inputs: vec![],
            name: "example".into(),
            manifest: None,
        };
        let mut value = json!({"status":"LINTED", "diagnostics":[{
            "severity":"warning", "kind":"unused_binding", "offset":13,
            "message":"unused\u{001b}[2J"
        }]});
        project.annotate(&mut value);
        let result = render(&value, Style { color: false }, Some(&project));
        assert!(result.contains("[!] WARNING unused_binding"), "{result}");
        assert!(result.contains("example.keel:2:2"), "{result}");
        assert!(
            result.contains("2 |     let unused = 1\n        |     ^"),
            "{result}"
        );
        assert!(result.contains("unused\\u{1b}[2J"));
        assert!(!result.contains('\x1b'));
    }

    #[test]
    fn activity_frames_move_without_percentages_and_fit_narrow_terminals() {
        let a = progress_frame("Building", 0, Duration::from_secs(1), 80);
        let b = progress_frame("Building", 1, Duration::from_secs(1), 80);
        assert_ne!(a, b);
        assert!(a.contains("Building  1.0s"));
        assert!(!a.contains('%'));
        for width in [0, 1, 10, 20] {
            assert!(
                progress_frame("Building", 0, Duration::ZERO, width).len()
                    <= width.saturating_sub(1)
            );
        }
    }
}
