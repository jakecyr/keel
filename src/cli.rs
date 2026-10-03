use std::collections::BTreeSet;

/// Project commands default to the current directory, before option validation.
pub fn default_project(args: &mut Vec<String>) {
    if args.first().is_some_and(|command| {
        matches!(
            command.as_str(),
            "init"
                | "check"
                | "lint"
                | "inspect"
                | "build"
                | "run"
                | "test"
                | "edit"
                | "review"
                | "explain"
                | "fmt"
        )
    }) && args.get(1).is_none_or(|arg| arg.starts_with('-'))
    {
        args.insert(1, ".".into());
    }
}

pub fn help(command: &str) -> Option<&'static str> {
    Some(match command {
        "init" => {
            "keel init [DIRECTORY] [--json]\n\nCreate a project (default: current directory). Creates src/main.keel, tests/acceptance.keel, keel.json, a deny-by-default policy, AGENTS.md and CLAUDE.md. Existing source and human instructions are preserved.\n\n  keel init hello\n  cd hello\n  keel test\n  keel run --allow-stdout"
        }
        "check" => {
            "keel check [FILE_OR_PROJECT] [--json]\n\nCheck syntax, types, ownership and effects. Defaults to the current project. Does not run tests; incomplete holes exit nonzero."
        }
        "build" => {
            "keel build [FILE_OR_PROJECT] [-o BINARY] [--emit-c FILE] [--json]\n\nBuild a native executable using your C compiler. Defaults to the current project; the result prints its executable path. Unresolved holes are rejected.\n\n  keel build -o build/hello\n  ./build/hello --allow-stdout"
        }
        "run" => {
            "keel run [FILE_OR_PROJECT] [-o BINARY] [--emit-c FILE] [--json]\n         [--policy POLICY.json | --allow-stdout --allow-net=127.0.0.1:PORT]\n         [--allow-connect=ORIGIN --allow-read=PATH --allow-env=NAME]\n         [--allow-write=PATH --allow-exec=PATH --allow-clock=monotonic]\n\nBuild and run the current project by default. No external authority is granted implicitly. The starter greeting needs --allow-stdout. Policy files cannot be combined with permission overrides.\n\n  keel run --allow-stdout"
        }
        "test" => {
            "keel test [FILE_OR_PROJECT] [--engine native|reference|both] [--json]\n          [--cases N] [--seed N] [--filter TEXT] [--value N]\n          [--timeout-ms N] [--budget-ms N] [--memory-mib N] [--no-shrink]\n\nDefaults: current project, native engine, 100 property cases, seed 1, 2000 ms/test, 30000 ms/suite, 256 MiB/worker (Linux only). Reference execution has separate interpreter limits.\nTESTED is sampled evidence; FAILED/BLOCKED/UNKNOWN exit nonzero. Replay --value requires exactly one property selected by --filter.\n\n  keel test --engine both\n  keel test --filter 'property name' --value 17 --json"
        }
        "fmt" => {
            "keel fmt [FILE_OR_PROJECT] [--check] [--json]\n\nFormat indentation and whitespace (current project by default). Preserves comments and string bytes. --check makes no changes and exits nonzero if formatting is needed."
        }
        "lint" => {
            "keel lint [FILE_OR_PROJECT] [--deny-warnings] [--json]\n\nCheck for unused bindings and effects in the current project by default. --deny-warnings makes warnings fail CI. Does not run tests."
        }
        "agent" => {
            "keel agent context [FILE_OR_PROJECT] [--symbol NAME] [--max-chars N] [--json]\nkeel agent spec language|collections|stdlib|protocol [--json]\nkeel agent commands [--json]\n\nVersioned offline documentation and structured tool discovery. Context without a path returns the bootstrap guide; include . for project context. --max-chars limits source snippets, not total metadata.\n\n  keel agent context . --symbol greet --json\n  keel agent spec language"
        }
        "inspect" => {
            "keel inspect [FILE_OR_PROJECT] [--symbol NAME] [--max-chars N] [--json]\n\nRetrieve revision-bound function source, dependencies, callers, contracts and holes. Defaults to current project. Source snippet limits do not bound total metadata."
        }
        "edit" => {
            "keel edit [FILE_OR_PROJECT] --request EDIT.json [--json]\n          [--cases N] [--seed N] [--timeout-ms N] [--budget-ms N] [--memory-mib N] [--no-shrink]\n\nApply revision-bound function-body changes transactionally within one physical file. Acceptance files are protected. Read request examples with: keel agent spec protocol"
        }
        "review" => {
            "keel review [FILE_OR_PROJECT] --against BASELINE [--json]\n\nCompare source/interface/effect/test changes against a baseline source file. This does not run tests or prove behavior."
        }
        "explain" => {
            "keel explain [FILE_OR_PROJECT] --offset N [--json]\n\nShow source context around a diagnostic byte offset, using the matching revision."
        }
        "serve" => {
            "keel serve [--max-cache-mib N]\n\nServe JSON-lines requests on stdin/stdout. Default estimated snapshot-cache budget: 64 MiB. Read the protocol with: keel agent spec protocol"
        }
        "doctor" => {
            "keel doctor [--json]\n\nProbe your C compiler and report platform limitations. Set CC to a compiler executable if needed. On macOS: xcode-select --install. On Debian/Ubuntu: install build-essential. This is a toolchain check, not production certification."
        }
        "api" => {
            "keel api BUILTIN [--json]\n\nLook up an exact supported builtin, e.g. keel api list.get --json. Browse available language features with: keel agent spec stdlib"
        }
        _ => return None,
    })
}
pub fn validate(args: &[String]) -> Result<(), String> {
    let command = args.first().ok_or("expected command")?;
    let (flags, values): (&[&str], &[&str]) = match command.as_str() {
        "check" => (&["--json"], &[]),
        "lint" => (&["--json", "--deny-warnings"], &[]),
        "inspect" => (&["--json"], &["--symbol", "--max-chars"]),
        "build" => (&["--json"], &["-o", "--emit-c"]),
        "run" => (
            &["--json", "--allow-stdout"],
            &["-o", "--emit-c", "--policy"],
        ),
        "test" => (
            &["--json", "--no-shrink"],
            &[
                "--cases",
                "--seed",
                "--filter",
                "--value",
                "--timeout-ms",
                "--budget-ms",
                "--memory-mib",
                "--engine",
            ],
        ),
        "edit" => (
            &["--json", "--no-shrink"],
            &[
                "--request",
                "--cases",
                "--seed",
                "--filter",
                "--value",
                "--timeout-ms",
                "--budget-ms",
                "--memory-mib",
            ],
        ),
        "review" => (&["--json"], &["--against"]),
        "explain" => (&["--json"], &["--offset"]),
        "fmt" => (&["--json", "--check"], &[]),
        "init" => (&["--json"], &[]),
        _ => return Err(format!("unknown command '{command}'; use keel --help")),
    };
    if args.get(1).is_none_or(|s| s.starts_with('-')) {
        return Err("expected source/project path".into());
    }
    let mut i = 2;
    let mut seen = BTreeSet::new();
    while i < args.len() {
        if command == "run"
            && [
                "--allow-connect=",
                "--allow-read=",
                "--allow-env=",
                "--allow-write=",
                "--allow-exec=",
                "--allow-clock=",
            ]
            .iter()
            .any(|prefix| args[i].starts_with(prefix))
        {
            if args[i]
                .split_once('=')
                .is_none_or(|(_, v)| v.is_empty() || v.chars().any(char::is_control))
            {
                return Err("invalid scoped permission".into());
            }
            seen.insert("--scoped-permission".into());
            i += 1;
            continue;
        }
        let key = if command == "run" && args[i].starts_with("--allow-net=") {
            "--allow-net"
        } else {
            args[i].as_str()
        };
        if !seen.insert(key.to_string()) {
            return Err(format!("duplicate option '{key}'"));
        }
        if flags.contains(&key) || key == "--allow-net" {
            i += 1;
        } else if values.contains(&key) {
            if args.get(i + 1).is_none_or(|v| v.starts_with("--")) {
                return Err(format!("{key} needs a value"));
            }
            i += 2;
        } else {
            return Err(format!("unknown option or extra argument '{}'", args[i]));
        }
    }
    if seen.contains("--policy")
        && (seen.contains("--allow-net")
            || seen.contains("--allow-stdout")
            || seen.contains("--scoped-permission"))
    {
        return Err("--policy cannot be combined with permission overrides".into());
    }
    Ok(())
}
