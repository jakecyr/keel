mod check;
mod agent;
mod cli;
mod eval;
mod files;
mod format;
mod lint;
mod native;
mod process;
mod project;
mod service;
#[cfg(test)]
mod audit_tests;
mod syntax;
#[cfg(test)]
mod tests;

use check::Analysis;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
use syntax::{Diagnostic, Program};

type Result<T> = std::result::Result<T, String>;
static TEMP_ID: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Result<Self> {
        for _ in 0..100 {
            let path = env::temp_dir().join(format!(
                "keel-{}-{}",
                std::process::id(),
                TEMP_ID.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.to_string()),
            }
        }
        Err("could not allocate temporary build directory".into())
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn revision(source: &str) -> String {
    // Stable content identity, not an artifact signature or security hash.
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in source.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("r{hash:016x}")
}
fn diagnostic_value(source: &str, d: Diagnostic) -> Value {
    json!({"status":"FAILED","revision":revision(source),"diagnostics":[d]})
}
fn checked(source: &str) -> std::result::Result<(Program, Analysis), Value> {
    let program = syntax::parse(source).map_err(|d| diagnostic_value(source, d))?;
    let analysis = check::check(source, &program).map_err(|d| diagnostic_value(source, d))?;
    Ok((program, analysis))
}
fn report(value: &Value, json_output: bool) {
    if json_output {
        println!("{}", serde_json::to_string_pretty(value).unwrap());
    } else {
        if let Some(status) = value.get("status").and_then(Value::as_str) {
            println!(
                "{status}  {}",
                value.get("revision").and_then(Value::as_str).unwrap_or("")
            );
        }
        if let Some(diagnostics) = value.get("diagnostics").and_then(Value::as_array) {
            for d in diagnostics {
                println!(
                    "{}:{} [{}] {}",
                    d["line"],
                    d["column"],
                    d["kind"].as_str().unwrap_or("error"),
                    d["message"].as_str().unwrap_or("")
                );
            }
        }
        if let Some(tests) = value.get("tests").and_then(Value::as_array) {
            for test in tests {
                println!(
                    "{}  {} ({} cases)",
                    test["status"].as_str().unwrap_or("UNKNOWN"),
                    test["name"].as_str().unwrap_or(""),
                    test["cases"]
                );
                if let Some(failure) = test.get("failure") {
                    println!("  {failure}");
                }
            }
        }
        if let Some(holes) = value.get("holes").and_then(Value::as_array) {
            for hole in holes {
                println!(
                    "HOLE {}: expected {} in {}",
                    hole["id"], hole["expected"], hole["function"]
                );
            }
        }
        if let Some(message) = value.get("message").and_then(Value::as_str) {
            println!("{message}");
        }
        if value.get("status").is_none() {
            println!("{}", serde_json::to_string_pretty(value).unwrap());
        }
    }
}
fn compile(
    program: &Program,
    analysis: &Analysis,
    tests: bool,
    output: &Path,
    emit_c: Option<&str>,
) -> Result<()> {
    let temporary = Temp::new()?;
    let source = native::emit(program, analysis, tests);
    let cpath = temporary.0.join("program.c");
    fs::write(&cpath, &source).map_err(|e| e.to_string())?;
    if let Some(path) = emit_c {
        files::atomic_write(Path::new(path), source.as_bytes(), None)?;
    }
    let target = temporary.0.join("program");
    let mut command = Command::new(env::var("CC").unwrap_or_else(|_| "cc".into()));
    command.args(["-std=c11", "-O2", "-g"]).arg(&cpath).arg("-o").arg(&target);
    let timeout = env::var("KEEL_BUILD_TIMEOUT_MS").ok().map(|v| v.parse::<u64>().map_err(|_| "invalid KEEL_BUILD_TIMEOUT_MS")).transpose()?.unwrap_or(30_000);
    if !(1..=300_000).contains(&timeout) { return Err("KEEL_BUILD_TIMEOUT_MS must be 1..300000".into()); }
    let result = process::capture(&mut command, timeout, 2048)?;
    if result.timed_out { return Err(format!("compiler_execution_limit: native compiler exceeded {timeout} ms")); }
    if !result.status.success() {
        return Err(format!(
            "native backend failed (compiler defect or unsupported host): {}",
            result.stderr
        ));
    }
    files::atomic_write(output, &fs::read(&target).map_err(|e|e.to_string())?, Some(fs::metadata(&target).map_err(|e|e.to_string())?.permissions()))?;
    Ok(())
}
#[derive(Clone)]
struct TestOptions {
    cases: usize,
    seed: u64,
    timeout_ms: u64,
    filter: Option<String>,
    replay: Option<i64>,
    shrink: bool,
    memory_mib: u64,
    budget_ms: u64,
}
fn run_worker(
    binary: &Path,
    index: usize,
    options: &TestOptions,
    value: Option<i64>,
) -> Result<(String, Option<Value>)> {
    let mut command = Command::new(binary);
    command.args([
            index.to_string(),
            options.seed.to_string(),
            options.cases.to_string(),
            value
                .map(|v| v.to_string())
                .unwrap_or_else(|| "auto".into()),
        ]);
    let output = process::capture(&mut command, options.timeout_ms, options.memory_mib)?;
    if output.timed_out {
        return Ok(("UNKNOWN".into(),Some(json!({"kind":"execution_limit","timeout_ms":options.timeout_ms}))));
    }
    if output.status.success() { return Ok(("TESTED".into(),None)); }
    let failure = output.stderr
                .lines()
                .rev()
                .find_map(|line| serde_json::from_str::<Value>(line).ok())
                .unwrap_or_else(
                    || json!({"kind":"worker_failure","exit_code":output.status.code(),"message":output.stderr,"truncated":output.truncated}),
                );
    let state = match failure["kind"].as_str() {
        Some("hole_reached"|"permission_denied_net"|"permission_denied_stdout") => "BLOCKED",
        Some("allocation_failed"|"allocation_limit") => "UNKNOWN",
        _ => "FAILED",
    };
    Ok((state.into(),Some(failure)))
}
fn run_tests(
    source: &str,
    program: &Program,
    analysis: &Analysis,
    options: &TestOptions,
) -> Result<Value> {
    let selected:Vec<_> = program.tests.iter().enumerate().filter(|(_,test)|options.filter.as_ref().is_none_or(|filter| {
        if program.tests.iter().any(|t| &t.name==filter) { &test.name==filter } else { test.name.contains(filter) }
    })).collect();
    if options.replay.is_some() && (selected.len()!=1 || selected[0].1.generator.is_none()) {
        return Err("--value replay must select exactly one property; use --filter with its full name".into());
    }
    let temporary = Temp::new()?;
    let binary = temporary.0.join("tests");
    compile(program, analysis, true, &binary, None)?;
    let mut results = Vec::new();
    let deadline = Instant::now()+Duration::from_millis(options.budget_ms);
    for (index, test) in selected {
        if Instant::now()>=deadline { results.push(json!({"name":test.name,"status":"UNKNOWN","cases":0,"failure":{"kind":"suite_execution_limit","budget_ms":options.budget_ms}})); continue; }
        let mut bounded = options.clone();
        bounded.timeout_ms = options.timeout_ms.min(deadline.saturating_duration_since(Instant::now()).as_millis() as u64).max(1);
        if let (Some(v), Some((_, min, max))) = (options.replay, &test.generator)
            && (v < *min || v > *max)
        {
            return Err(format!(
                "replay value {v} is outside [{min}, {max}] for '{}'",
                test.name
            ));
        }
        let (status, mut failure) = run_worker(&binary, index, &bounded, options.replay)?;
        let mut attempts = 0;
        let mut shrunk = false;
        if options.shrink
            && status == "FAILED"
            && options.replay.is_none()
            && let (Some(f), Some((_, min, max))) = (&failure, &test.generator)
            && let Some(original) = f.get("value").and_then(Value::as_i64)
        {
            let mut best = original;
            let expected_kind = f["kind"].clone();
            let expected_offset = f["offset"].clone();
            let mut seen = BTreeSet::new();
            seen.insert(best);
            while attempts < 32 && Instant::now()<deadline {
                let candidates = [
                    0,
                    best / 2,
                    if best > 0 {
                        best - 1
                    } else {
                        best.saturating_add(1)
                    },
                    *min,
                    *max,
                ];
                let mut improved = false;
                for candidate in candidates {
                    if attempts >= 32 || Instant::now()>=deadline {
                        break;
                    }
                    if candidate < *min
                        || candidate > *max
                        || candidate.unsigned_abs() >= best.unsigned_abs()
                        || !seen.insert(candidate)
                    {
                        continue;
                    }
                    attempts += 1;
                    bounded.timeout_ms=options.timeout_ms.min(deadline.saturating_duration_since(Instant::now()).as_millis() as u64).max(1);
                    let (state, evidence) = run_worker(&binary, index, &bounded, Some(candidate))?;
                    if state == "FAILED"
                        && evidence.as_ref().is_some_and(|e| {
                            e["kind"] == expected_kind && e["offset"] == expected_offset
                        })
                    {
                        best = candidate;
                        failure = evidence;
                        shrunk = true;
                        improved = true;
                        break;
                    }
                }
                if !improved {
                    break;
                }
            }
        }
        let mut result = json!({"name":test.name,"status":status,"cases":if test.generator.is_some() { if options.replay.is_some(){1}else{options.cases} }else{1},"seed":options.seed,"case_count_is_budget":status!="TESTED"});
        if let Some(mut failure) = failure {
            if let Some(offset) = failure.get("offset").and_then(Value::as_u64) {
                let loc = Diagnostic::new(source, offset as usize, "", "");
                failure["line"] = json!(loc.line);
                failure["column"] = json!(loc.column);
            }
            failure["shrink_attempts"] = json!(attempts);
            failure["shrunk"] = json!(shrunk);
            if failure["has_value"] == true {
                failure["replay"] =
                    json!({"test":test.name,"value":failure["value"],"revision":revision(source)});
            }
            result["failure"] = failure;
        }
        results.push(result);
    }
    let state = if results.is_empty() {
        "UNKNOWN"
    } else if results.iter().any(|r| r["status"] == "FAILED") {
        "FAILED"
    } else if results.iter().any(|r| r["status"] == "UNKNOWN") {
        "UNKNOWN"
    } else if results.iter().any(|r| r["status"] == "BLOCKED") {
        "BLOCKED"
    } else {
        "TESTED"
    };
    Ok(
        json!({"status":state,"revision":revision(source),"tests":results,"assurance":"TESTED means only the recorded cases passed; contracts are ENFORCED at runtime, never PROVEN","holes":analysis.holes,"limits":{"suite_ms":options.budget_ms,"worker_ms":options.timeout_ms,"memory_mib":options.memory_mib,"memory_enforced":cfg!(target_os="linux") && options.memory_mib>0}}),
    )
}
fn test_engine(source:&str,program:&Program,analysis:&Analysis,args:&[String])->Result<Value> {
    let options=test_options(args)?;
    match option(args,"--engine")?.as_deref().unwrap_or("native") {
        "native"=>run_tests(source,program,analysis,&options),
        "reference"=>eval::run_tests(source,program,&options),
        "both"=>{
            let mut native=run_tests(source,program,analysis,&options)?;
            let reference=eval::run_tests(source,program,&options)?;
            let a=native["tests"].as_array().ok_or("invalid native test report")?;
            let b=reference["tests"].as_array().ok_or("invalid reference test report")?;
            let mut mismatches=Vec::new(); let mut uncertain=false;
            if a.len()!=b.len() {return Err("differential test selections differ".into());}
            for (native,reference) in a.iter().zip(b) {
                if native["status"]=="UNKNOWN" || reference["status"]=="UNKNOWN" {uncertain=true;continue;}
                let same=native["name"]==reference["name"] && native["status"]==reference["status"] && ["kind","offset","has_value","value"].iter().all(|field|native["failure"][field]==reference["failure"][field]);
                if !same {mismatches.push(json!({"test":native["name"],"native":native,"reference":reference}));}
            }
            let empty=a.is_empty();
            if !mismatches.is_empty() {native["status"]=json!("FAILED");}
            else if uncertain && native["status"]!="FAILED" {native["status"]=json!("UNKNOWN");}
            native["engine"]=json!("both");
            native["differential"]=json!({"status":if !mismatches.is_empty(){"FAILED"}else if uncertain || empty{"UNKNOWN"}else{"TESTED"},"mismatches":mismatches,"reference":reference,"assurance":"Agreement only on executed cases; not proof of language soundness"});
            Ok(native)
        },
        _=>Err("--engine must be native, reference, or both".into())
    }
}
fn option(args: &[String], name: &str) -> Result<Option<String>> {
    if let Some(i) = args.iter().position(|a| a == name) {
        return args
            .get(i + 1)
            .filter(|v| !v.starts_with("--"))
            .cloned()
            .map(Some)
            .ok_or_else(|| format!("{name} needs a value"));
    }
    Ok(None)
}
fn numeric<T: std::str::FromStr>(args: &[String], name: &str, default: T) -> Result<T> {
    match option(args, name)? {
        Some(v) => v.parse().map_err(|_| format!("invalid {name}: {v}")),
        None => Ok(default),
    }
}
fn test_options(args: &[String]) -> Result<TestOptions> {
    let options = TestOptions {
        cases: numeric(args, "--cases", 100)?,
        seed: numeric(args, "--seed", 1)?,
        timeout_ms: numeric(args, "--timeout-ms", 2000)?,
        filter: option(args, "--filter")?,
        replay: option(args, "--value")?
            .map(|v| v.parse().map_err(|_| "invalid --value".to_string()))
            .transpose()?,
        shrink: !args.iter().any(|a| a == "--no-shrink"),
        memory_mib: numeric(args,"--memory-mib",256)?,
        budget_ms: numeric(args,"--budget-ms",30_000)?,
    };
    if options.cases == 0
        || options.cases > 1_000_000
        || options.timeout_ms == 0
        || options.timeout_ms > 60_000
        || options.budget_ms==0 || options.budget_ms>300_000
        || options.memory_mib>8192
    {
        return Err("cases must be 1..1000000; timeout-ms must be 1..60000".into());
    }
    Ok(options)
}
fn inspect(source: &str, program: &Program, analysis: &Analysis, args: &[String]) -> Result<Value> {
    let selected = option(args, "--symbol")?;
    let limit = numeric(args, "--max-chars", 12_000_usize)?;
    let mut remaining = limit;
    let mut incomplete = false;
    let mut functions = Vec::new();
    for f in &program.functions {
        if selected.as_ref().is_some_and(|s| s != &f.name) {
            continue;
        }
        let full = &source[f.start..f.end];
        let snippet: String = full.chars().take(remaining).collect();
        remaining = remaining.saturating_sub(snippet.chars().count());
        incomplete |= snippet.len() < full.len();
        let dependencies=analysis.calls.get(&f.name).cloned().unwrap_or_default().into_iter().map(|name| {
            if let Some(target)=program.functions.iter().find(|f|f.name==name) { json!({"name":name,"interface":source[target.start..target.body_start].trim(),"effects":target.effects}) }
            else if let Some(b)=check::builtin(&name) { json!({"name":name,"parameters":b.params,"result":b.result,"effects":b.effects,"trusted_host_adapter":true}) }
            else { json!({"name":name}) }
        }).collect::<Vec<_>>();
        let callers = analysis
            .calls
            .iter()
            .filter(|(_, calls)| calls.contains(&f.name))
            .map(|(n, _)| n)
            .collect::<Vec<_>>();
        functions.push(json!({"target":format!("fn:{}",f.name),"name":f.name,"public":f.public,"parameters":f.params,"result":f.result,"effects":f.effects,"source":snippet,"dependencies":dependencies,"callers":callers,"contracts":{"requires":f.requires.len(),"ensures":f.ensures.len(),"assurance":"ENFORCED when executed"}}));
    }
    if functions.is_empty() && selected.is_some() {
        return Err("symbol not found".into());
    }
    Ok(
        json!({"revision":revision(source),"functions":functions,"holes":analysis.holes,"tests":program.tests.iter().map(|t|&t.name).collect::<Vec<_>>(),"incomplete":incomplete,"source_character_budget":limit,"resource_behavior":{"allocation":"Text clone, concat, conversion, response, and body allocate; transitive costs not analyzed","deep_copies":"only explicit library operations","external_behavior":"HTTP adapter is trusted native code"}}),
    )
}
fn edit(path: &Path, source: &str, program: &Program, args: &[String]) -> Result<Value> {
    edit_context(path,source,program,args,None)
}
fn edit_context(path: &Path, source: &str, program: &Program, args: &[String], context:Option<&project::Project>) -> Result<Value> {
    let request_path = option(args, "--request")?.ok_or("edit requires --request path.json")?;
    let request: Value =
        serde_json::from_str(&files::read(Path::new(&request_path),files::SOURCE_LIMIT)?)
            .map_err(|e| e.to_string())?;
    if request["base_revision"] != revision(source) {
        return Ok(
            json!({"status":"FAILED","revision":revision(source),"message":"stale_revision: inspect the current source and resubmit; no changes applied"}),
        );
    }
    let edits = if let Some(edits)=request.get("edits") {
        if request.get("operation").is_some() || request.get("source").is_some() || request.get("target").is_some() {return Err("use either a single edit or an edits array".into());}
        edits.as_array().ok_or("edits must be an array")?.clone()
    } else {vec![request.clone()]};
    if edits.is_empty() || edits.len()>128 {return Err("transaction needs 1..128 body edits".into());}
    let mut replacements=Vec::new(); let mut targets=BTreeSet::new(); let mut target_file:Option<PathBuf>=None; let mut file_base=0;
    for instruction in &edits {
    if instruction["operation"] != "replace_body" {
        return Err("edits support replace_body only; interfaces, contracts, and acceptance tests are protected".into());
    }
    let target = instruction["target"]
        .as_str()
        .and_then(|s| s.strip_prefix("fn:"))
        .ok_or("target must be fn:<name>")?;
    let function = program
        .functions
        .iter()
        .find(|f| f.name == target)
        .ok_or("edit target not found")?;
    if !targets.insert(target.to_string()) {return Err("transaction cannot edit a function twice".into());}
    let owner=context.and_then(|c|c.at(function.body_start));
    if let Some(owner)=owner {
        if owner.protected {return Err("acceptance_protected: declarations in manifest test files cannot be structurally edited".into());}
        if function.end>owner.end {return Err("declaration crosses a physical source file boundary".into());}
    }
    let owner_path=owner.map(|p|p.path.clone()).unwrap_or_else(||path.to_owned());
    if target_file.as_ref().is_some_and(|p|p!=&owner_path) {return Err("atomic transactions currently require targets in one physical source file".into());}
    target_file=Some(owner_path); file_base=owner.map(|p|p.start).unwrap_or(0);
    let body = instruction["source"]
        .as_str()
        .ok_or("source must contain a replacement braced body")?;
    if !body.trim_start().starts_with('{') {
        return Err(
            "replacement must start with '{'; changing the interface or contracts is not allowed"
                .into(),
        );
    }
    // Parse in isolation first: reject attempts to escape the body and inject declarations.
    let wrapper = format!("fn placeholder() {body}");
    let parsed =
        syntax::parse(&wrapper).map_err(|d| format!("invalid replacement body: {}", d.message))?;
    if parsed.functions.len() != 1
        || !parsed.tests.is_empty()
        || parsed.functions[0].end != wrapper.trim_end().len()
    {
        return Err("replacement must be exactly one braced body; appended declarations/comments are not accepted".into());
    }
    replacements.push((function.body_start,function.end,body.to_string()));
    }
    let run=request.get("run").and_then(Value::as_str).unwrap_or("check");
    if run!="check" && run!="affected_checks_and_tests" {return Err("run must be check or affected_checks_and_tests".into());}
    let target_file=target_file.unwrap();
    if fs::symlink_metadata(&target_file).is_ok_and(|m|m.file_type().is_symlink()) {return Err("structural edits require a regular source path, not a symbolic link".into());}
    let _lock=files::EditLock::acquire(&target_file)?;
    replacements.sort_by_key(|r|std::cmp::Reverse(r.0));
    let mut candidate=source.to_string();
    for (start,end,body) in &replacements {candidate.replace_range(*start..*end,body);}
    let (new_program, analysis) = match checked(&candidate) {
        Ok(v) => v,
        Err(v) => return Ok(json!({"status":"FAILED","applied":false,"candidate":v})),
    };
    let evidence = if run == "affected_checks_and_tests" {
        let mut options = test_options(args)?;
        options.filter = None;
        options.replay = None;
        let evidence = run_tests(&candidate, &new_program, &analysis, &options)?;
        if evidence["status"] != "TESTED" {
            return Ok(json!({"status":"FAILED","applied":false,"candidate":evidence}));
        }
        Some(evidence)
    } else {
        None
    };
    let original=if let Some(context)=context {project::Project::load(context.manifest.as_deref().unwrap_or(path))?.source}else{files::read(path,files::SOURCE_LIMIT)?};
    if original != source {
        return Err("source changed during validation; edit not applied".into());
    }
    let mut new_file=if let Some(part)=context.and_then(|c|c.at(file_base)) {part.source.clone()}else{source.to_string()};
    for (start,end,body) in &replacements {new_file.replace_range(start-file_base..end-file_base,body);}
    files::atomic_write(&target_file,new_file.as_bytes(),Some(fs::metadata(&target_file).map_err(|e|e.to_string())?.permissions()))?;
    Ok(
        json!({"status":"APPLIED","base_revision":revision(source),"revision":revision(&candidate),"target":request["target"],"targets":targets,"file":target_file,"evidence":evidence,"message":"Function bodies replaced atomically; interfaces, contracts, and test source preserved."}),
    )
}
fn review(source: &str, program: &Program, args: &[String]) -> Result<Value> {
    let baseline = option(args, "--against")?.ok_or("review requires --against baseline.keel")?;
    let old_source = project::Project::load(Path::new(&baseline))?.source;
    let old = syntax::parse(&old_source).map_err(|d| d.message)?;
    let old_functions: BTreeMap<_, _> = old.functions.iter().map(|f| (&f.name, f)).collect();
    let mut changes = Vec::new();
    for f in &program.functions {
        if let Some(previous) = old_functions.get(&f.name) {
            let interface_changed =
                old_source[previous.start..previous.body_start] != source[f.start..f.body_start];
            let body_changed =
                old_source[previous.body_start..previous.end] != source[f.body_start..f.end];
            if interface_changed || body_changed {
                changes.push(json!({"function":f.name,"interface_or_contract_changed":interface_changed,"body_changed":body_changed,"effects_before":previous.effects,"effects_after":f.effects,"source_offset":f.start}));
            }
        } else {
            changes.push(json!({"function":f.name,"added":true}));
        }
    }
    for f in &old.functions {
        if !program.functions.iter().any(|n| n.name == f.name) {
            changes.push(json!({"function":f.name,"removed":true}));
        }
    }
    fn test_sources<'a>(source:&'a str,p:&'a Program)->BTreeMap<&'a str,&'a str> {
        let starts:BTreeSet<_>=p.functions.iter().map(|f|f.start).chain(p.tests.iter().map(|t|t.at)).collect();
        p.tests.iter().map(|t| {let end=starts.range(t.at+1..).next().copied().unwrap_or(source.len());(t.name.as_str(),source[t.at..end].trim())}).collect()
    }
    let before=test_sources(&old_source,&old); let after=test_sources(source,program);
    let names:BTreeSet<_>=before.keys().chain(after.keys()).copied().collect();
    let test_changes:Vec<_>=names.into_iter().filter(|name|before.get(name)!=after.get(name)).map(|name|json!({"test":name,"added":!before.contains_key(name),"removed":!after.contains_key(name),"acceptance_review_required":true})).collect();
    Ok(json!({"revision":revision(source),"base_revision":revision(&old_source),"changes":changes,"test_changes":test_changes,"test_declarations_before":old.tests.len(),"test_declarations_after":program.tests.len(),"assurance":"UNKNOWN: review does not run tests or prove behavior","limitations":["test comparisons are textual, not semantic","resource estimates and transitive impact are not computed"]}))
}
fn usage() {
    println!(
        "Keel — experimental native language\n\n  keel init DIRECTORY [--json]\n  keel check FILE_OR_PROJECT [--json]\n  keel inspect FILE_OR_PROJECT [--symbol NAME] [--max-chars N] [--json]\n  keel build FILE_OR_PROJECT [-o BINARY] [--emit-c FILE] [--json]\n  keel run FILE_OR_PROJECT [--policy POLICY.json | --allow-net=127.0.0.1:PORT --allow-stdout]\n  keel test FILE_OR_PROJECT [--cases N] [--seed N] [--filter TEXT] [--value N]\n                 [--timeout-ms N] [--budget-ms N] [--memory-mib N] [--no-shrink] [--json]\n  keel edit FILE_OR_PROJECT --request EDIT.json [--json]\n  keel review FILE_OR_PROJECT --against BASELINE [--json]\n  keel explain FILE_OR_PROJECT --offset N [--json]\n  keel fmt FILE_OR_PROJECT [--check] [--json]\n  keel serve [--max-cache-mib N]\n  keel doctor [--json]\n  keel api BUILTIN [--json]\n\nBuilds reject holes. Tests reaching holes are BLOCKED. No network authority is granted by an effects declaration."
    );
}
fn execute() -> Result<i32> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() || args[0] == "--help" || args[0] == "help" {
        usage();
        return Ok(0);
    }
    if args[0] == "--version" {
        println!("keel 0.1.0");
        return Ok(0);
    }
    if args[0]=="agent" {
        let result=agent::execute(&args)?;let json_output=args.iter().any(|a|a=="--json");
        if !json_output && let Some(content)=result.get("content").and_then(Value::as_str) {println!("{content}");}else{report(&result,true);}
        return Ok(if result["status"]=="FAILED"{1}else{0});
    }
    if args[0]=="serve" {
        if args.len()!=1 && !(args.len()==3 && args[1]=="--max-cache-mib") {return Err("usage: keel serve [--max-cache-mib N]".into());}
        service::serve(numeric(&args,"--max-cache-mib",64_usize)?)?;return Ok(0);
    }
    if args[0]=="doctor" {
        if args.iter().skip(1).any(|a|a!="--json") {return Err("usage: keel doctor [--json]".into());}
        let temporary=Temp::new()?; let (program,analysis)=checked("fn main() {}").map_err(|v|v.to_string())?;
        let outcome=compile(&program,&analysis,false,&temporary.0.join("probe"),None);
        let value=json!({"status":if outcome.is_ok(){"READY"}else{"FAILED"},"native_compiler":env::var("CC").unwrap_or_else(|_|"cc".into()),"native_probe":outcome.err(),"os":env::consts::OS,"architecture":env::consts::ARCH,"worker_memory_limits":if cfg!(target_os="linux"){"RLIMIT_AS"}else{"not enforced on this platform"},"production_readiness":"NOT CERTIFIED; see docs/design-status.json"});
        report(&value,args.iter().any(|a|a=="--json"));return Ok(if value["status"]=="READY"{0}else{1});
    }
    if args[0]=="api" {
        if !(args.len()==2 || (args.len()==3 && args[2]=="--json")) {return Err("usage: keel api BUILTIN [--json]".into());}
        let name=&args[1]; let signature=check::builtin(name).ok_or("unknown builtin; see docs/features.md and docs/language.md")?;
        report(&json!({"name":name,"parameters":signature.params,"result":signature.result,"effects":signature.effects,"trusted_host_adapter":true}),args.iter().any(|a|a=="--json"));return Ok(0);
    }
    cli::validate(&args)?;
    let command = &args[0];
    let path = PathBuf::from(args.get(1).ok_or("expected source file")?);
    let json_output = args.iter().any(|a| a == "--json");
    if command=="init" {report(&project::init(&path)?,json_output);return Ok(0);}
    let project=project::Project::load(&path)?;
    let source = &project.source;
    let (program, analysis) = match checked(source) {
        Ok(v) => v,
        Err(mut v) => {
            project.annotate(&mut v);
            report(&v, json_output);
            return Ok(1);
        }
    };
    let mut value = match command.as_str() {
        "check" => {
            json!({"status":if analysis.holes.is_empty(){"CHECKED"}else{"INCOMPLETE"},"revision":revision(source),"functions":program.functions.len(),"tests":program.tests.len(),"holes":analysis.holes,"message":"Parsing, types, ownership, and declared effects checked. No behavioral proof or tests implied."})
        }
        "inspect" => inspect(source, &program, &analysis, &args)?,
        "lint" => lint::run(source,&program,args.iter().any(|a|a=="--deny-warnings")),
        "fmt" => {
            let mut changed=Vec::new();
            for part in &project.parts {
                let formatted=format::source(&part.source);
                syntax::parse(&formatted).map_err(|d|format!("formatter produced invalid syntax: {}",d.message))?;
                if formatted!=part.source {changed.push((part,formatted));}
            }
            let check_only=args.iter().any(|a|a=="--check");
            if !check_only { for (part,formatted) in &changed {
                let _lock=files::EditLock::acquire(&part.path)?;
                if files::read(&part.path,files::SOURCE_LIMIT)?!=part.source {return Err("source changed while formatting".into());}
                files::atomic_write(&part.path,formatted.as_bytes(),Some(fs::metadata(&part.path).map_err(|e|e.to_string())?.permissions()))?;
            }}
            json!({"status":if check_only && !changed.is_empty(){"FAILED"}else{"FORMATTED"},"changed":changed.iter().map(|(p,_)|&p.path).collect::<Vec<_>>(),"check_only":check_only,"message":"Conservative indentation/whitespace formatting; string bytes and comments preserved."})
        },
        "build" | "run" => {
            if !analysis.holes.is_empty() {
                report(
                    &json!({"status":"FAILED","holes":analysis.holes,"message":"Release builds reject unresolved holes."}),
                    json_output,
                );
                return Ok(1);
            }
            if !program.functions.iter().any(|f| f.name == "main") {
                return Err("executable requires fn main()".into());
            }
            let output = PathBuf::from(option(&args, "-o")?.unwrap_or_else(|| {
                format!("build/{}", project.name)
            }));
            let policy=option(&args,"--policy")?;
            let permissions=if let Some(policy)=&policy {project::policy(Path::new(policy))?}else{args.iter().filter(|a|a.starts_with("--allow-net=") || a.as_str()=="--allow-stdout").cloned().collect()};
            let mut protected_inputs=project.inputs.clone();if let Some(policy)=policy {protected_inputs.push(PathBuf::from(policy));}
            files::protect(&output,&protected_inputs)?;
            let emit_c=option(&args,"--emit-c")?;
            if let Some(c)=&emit_c {
                files::protect(Path::new(c),&protected_inputs)?;
                if files::aliases(&output,Path::new(c)) || files::normalized(&output)?==files::normalized(Path::new(c))? {return Err("binary and emitted C outputs must be distinct".into());}
            }
            compile(
                &program,
                &analysis,
                false,
                &output,
                emit_c.as_deref(),
            )?;
            if command == "run" {
                let status = Command::new(fs::canonicalize(&output).map_err(|e| e.to_string())?)
                    .args(permissions)
                    .status()
                    .map_err(|e| e.to_string())?;
                return Ok(status.code().unwrap_or(1));
            }
            json!({"status":"BUILT","revision":revision(source),"binary":output,"backend":"C11 → system native compiler","contracts":"ENFORCED"})
        }
        "test" => test_engine(source, &program, &analysis, &args)?,
        "edit" => {
            if project.manifest.is_none() && project.parts[0].protected {return Err("acceptance_protected: this file is listed in its containing project's acceptance tests".into());}
            if project.manifest.is_some() {edit_context(&path,source,&program,&args,Some(&project))?}else{edit(&path,source,&program,&args)?}
        },
        "review" => review(source, &program, &args)?,
        "explain" => {
            let offset = numeric(&args, "--offset", 0_usize)?;
            if offset > source.len() || !source.is_char_boundary(offset) {
                return Err("offset must be a UTF-8 byte boundary in the source".into());
            }
            let d = Diagnostic::new(source, offset, "source_location", "");
            let lines = source
                .lines()
                .enumerate()
                .filter(|(i, _)| i.abs_diff(d.line - 1) <= 2)
                .map(|(i, line)| json!({"line":i+1,"source":line}))
                .collect::<Vec<_>>();
            json!({"revision":revision(source),"offset":offset,"line":d.line,"column":d.column,"context":lines,"function":program.functions.iter().find(|f|offset>=f.start && offset<f.end).map(|f|&f.name),"message":"Source context only; v0 does not record expression traces."})
        }
        _ => return Err(format!("unknown command '{command}'; use keel --help")),
    };
    project.annotate(&mut value);
    let failed = matches!(
        value["status"].as_str(),
        Some("FAILED" | "BLOCKED" | "UNKNOWN" | "INCOMPLETE")
    );
    report(&value, json_output);
    Ok(if failed { 1 } else { 0 })
}
fn main() {
    match execute() {
        Ok(code) => std::process::exit(code),
        Err(message) => {
            if env::args().any(|a| a == "--json") {
                println!(
                    "{}",
                    json!({"status":"FAILED","kind":"tool_error","message":message})
                );
            } else {
                eprintln!("keel: {message}");
            }
            std::process::exit(2);
        }
    }
}
