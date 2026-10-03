use crate::{Result, files};
use serde_json::{Value, json};
use std::path::Path;
pub const GUIDE: &str = include_str!("../docs/agent-language.md");
pub const INSTRUCTIONS: &str = "<!-- keel:agent-guide:start -->\n## Keel development\n\nBefore editing Keel code, run `keel agent context . --json`. For a focused change use\n`keel agent context . --symbol FUNCTION --json`. The installed compiler provides\nthe supported-language reference: `keel agent spec language`,\n`keel agent spec collections`, `keel agent spec protocol`, and `keel api BUILTIN --json`.\nUse `keel agent commands --json` to discover tools.\n\nUse current revisions for `keel edit . --request edit.json --json`. Preserve\napproved contracts, assertions, helper oracles, expected results, and generator domains.\nValidate with `keel fmt . --check`, `keel check . --json`, `keel lint . --json`,\nand `keel test . --engine both --json`. TESTED is sampled evidence; UNKNOWN,\nBLOCKED, and INCOMPLETE are not success. Ask the user before expanding runtime\nauthority in keel.policy.json. Ordinary source edits and Git review still work.\n<!-- keel:agent-guide:end -->\n";
pub fn merge_instructions(existing: &str) -> Result<String> {
    let start = "<!-- keel:agent-guide:start -->";
    let end = "<!-- keel:agent-guide:end -->";
    if existing.matches(start).count() > 1 || existing.matches(end).count() > 1 {
        return Err(
            "duplicate or nested Keel-managed instruction markers; repair before init".into(),
        );
    }
    match (existing.find(start), existing.find(end)) {
        (Some(a), Some(b)) if b > a => {
            let mut out = existing.to_string();
            out.replace_range(a..b + end.len(), INSTRUCTIONS.trim_end());
            Ok(out)
        }
        (None, None) => Ok(format!(
            "{}{}{}",
            existing,
            if existing.is_empty() || existing.ends_with("\n\n") {
                ""
            } else if existing.ends_with('\n') {
                "\n"
            } else {
                "\n\n"
            },
            INSTRUCTIONS
        )),
        _ => Err("unbalanced Keel-managed instruction markers; repair them before init".into()),
    }
}
pub fn commands() -> Value {
    json!({"schema":1,"commands":[
        {"command":"keel init PATH","purpose":"Initialize project; preserve existing instructions"},
        {"command":"keel agent context [PROJECT] [--symbol NAME] --json","purpose":"Get supported syntax, APIs, tools and task context"},
        {"command":"keel agent spec language|collections|protocol","purpose":"Read versioned offline reference"},
        {"command":"keel api BUILTIN --json","purpose":"Retrieve exact built-in signature"},
        {"command":"keel check PROJECT --json","purpose":"Check syntax/types/ownership/effects"},
        {"command":"keel lint PROJECT [--deny-warnings] --json","purpose":"Report unused bindings and effects"},
        {"command":"keel fmt PROJECT [--check]","purpose":"Format indentation without changing string bytes"},
        {"command":"keel test PROJECT --engine native|reference|both --json","purpose":"Execute examples/properties with explicit evidence"},
        {"command":"keel build PROJECT -o build/app","purpose":"Compile a standalone native executable"},
        {"command":"keel run PROJECT --policy keel.policy.json","purpose":"Build and run with separately supplied authority"},
        {"command":"keel edit PROJECT --request edit.json --json","purpose":"Validate and atomically replace implementation bodies"},
        {"command":"keel review PROJECT --against BASELINE --json","purpose":"Compare interfaces/effects/test changes"},
        {"command":"keel explain PROJECT --offset N --json","purpose":"Expand source-linked diagnostics"},
        {"command":"keel serve --max-cache-mib 64","purpose":"Persistent JSON-lines checking and inspection"},
        {"command":"keel doctor --json","purpose":"Check native compiler and platform capabilities"}
    ]})
}
pub fn execute(args: &[String]) -> Result<Value> {
    let action = args.get(1).map(String::as_str).unwrap_or("context");
    if action == "commands" {
        if args.iter().skip(2).any(|a| a != "--json") {
            return Err("usage: keel agent commands [--json]".into());
        }
        return Ok(commands());
    }
    if action == "spec" {
        let topic = args
            .get(2)
            .filter(|s| !s.starts_with('-'))
            .map(String::as_str)
            .unwrap_or("language");
        let start = if args.get(2).is_some_and(|s| !s.starts_with('-')) {
            3
        } else {
            2
        };
        if args.iter().skip(start).any(|a| a != "--json") {
            return Err("usage: keel agent spec [language|collections|protocol] [--json]".into());
        }
        let content = match topic {
            "language" => GUIDE,
            "collections" => include_str!("../docs/features.md"),
            "protocol" => include_str!("../docs/agent-protocol.md"),
            _ => return Err("unknown spec topic; use language, collections, or protocol".into()),
        };
        return Ok(
            json!({"schema":1,"version":env!("CARGO_PKG_VERSION"),"topic":topic,"content":content}),
        );
    }
    if action != "context" {
        return Err("agent subcommands: context, spec, commands".into());
    }
    let path = args.get(2).filter(|s| !s.starts_with('-'));
    let first = if path.is_some() { 3 } else { 2 };
    let mut inspect_args = vec![
        "inspect".into(),
        path.cloned().unwrap_or_else(|| "<bootstrap>".into()),
    ];
    inspect_args.extend_from_slice(args.get(first..).unwrap_or(&[]));
    crate::cli::validate(&inspect_args)?;
    let mut context = json!({"schema":1,"version":env!("CARGO_PKG_VERSION"),"language_reference":GUIDE,"commands":commands()["commands"],"project":null,"incomplete":false,"unsupported":["arbitrary generics and nominal records/unions","async and structured concurrency","Cranelift and declaration-level incremental compilation","host capability objects and distributed simulation","formal proof and production certification"]});
    if let Some(path) = path {
        let project = match crate::project::Project::load(Path::new(path)) {
            Ok(project) => project,
            Err(message) => {
                context["status"] = json!("FAILED");
                context["diagnostics"] =
                    json!({"status":"FAILED","kind":"project_load","message":message});
                return Ok(context);
            }
        };
        context["project"] = json!({"name":project.name,"manifest":project.manifest,"files":project.parts.iter().map(|p|json!({"path":p.path,"acceptance_protected":p.protected})).collect::<Vec<_>>(),"source_bytes":project.source.len(),"source_limit_bytes":files::SOURCE_LIMIT});
        match crate::checked(&project.source) {
            Ok((p, a)) => {
                let mut bundle = crate::inspect(&project.source, &p, &a, &inspect_args)?;
                project.annotate(&mut bundle);
                context["incomplete"] = bundle["incomplete"].clone();
                context["context"] = bundle;
            }
            Err(mut diagnostic) => {
                project.annotate(&mut diagnostic);
                context["status"] = json!("FAILED");
                context["diagnostics"] = diagnostic;
            }
        }
    } else if crate::option(&inspect_args, "--symbol")?.is_some() {
        return Err("--symbol requires a project/file path".into());
    }
    Ok(context)
}
