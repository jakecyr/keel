//! Local, vendor-neutral JSON-lines protocol. Stdout contains protocol responses only.
use crate::{Analysis, Program, Result, files};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io::{BufRead, Write},
    rc::Rc,
};

struct Snapshot {
    source: String,
    data: std::result::Result<(Program, Analysis), Value>,
    estimated_bytes: usize,
}
pub struct Service {
    cache: VecDeque<Rc<Snapshot>>,
    max_bytes: usize,
    bytes: usize,
    hits: u64,
    misses: u64,
}
impl Service {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            cache: VecDeque::new(),
            max_bytes,
            bytes: 0,
            hits: 0,
            misses: 0,
        }
    }
    fn snapshot(&mut self, source: String) -> Rc<Snapshot> {
        if let Some(index) = self.cache.iter().position(|s| s.source == source) {
            self.hits += 1;
            let snapshot = self.cache.remove(index).unwrap();
            self.cache.push_back(snapshot.clone());
            return snapshot;
        }
        self.misses += 1;
        let estimated_bytes = source.len().saturating_mul(128).saturating_add(8192);
        let snapshot = Rc::new(Snapshot {
            data: crate::checked(&source),
            source,
            estimated_bytes,
        });
        if estimated_bytes <= self.max_bytes {
            while self.bytes + estimated_bytes > self.max_bytes {
                if let Some(old) = self.cache.pop_front() {
                    self.bytes -= old.estimated_bytes;
                } else {
                    break;
                }
            }
            self.bytes += estimated_bytes;
            self.cache.push_back(snapshot.clone());
        }
        snapshot
    }
    pub fn request(&mut self, request: Value) -> Value {
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        match self.handle(&request) {
            Ok(value) => json!({"id":id,"result":value}),
            Err(message) => json!({"id":id,"error":{"kind":"protocol_error","message":message}}),
        }
    }
    fn handle(&mut self, request: &Value) -> Result<Value> {
        let object = request.as_object().ok_or("request must be an object")?;
        for key in object.keys() {
            if !["id", "method", "source", "path", "args"].contains(&key.as_str()) {
                return Err(format!("unknown request field '{key}'"));
            }
        }
        let method = request["method"]
            .as_str()
            .ok_or("method must be a string")?;
        if method == "stats" {
            return Ok(
                json!({"cache_entries":self.cache.len(),"cache_estimated_bytes":self.bytes,"cache_limit_bytes":self.max_bytes,"hits":self.hits,"misses":self.misses,"incrementality":"whole-source snapshots; no declaration-level invalidation yet"}),
            );
        }
        if method == "shutdown" {
            return Ok(json!({"status":"STOPPED"}));
        }
        if !["check", "inspect", "test", "format", "lint"].contains(&method) {
            return Err(format!("unknown service method '{method}'"));
        }
        if request.get("source").is_some() && request.get("path").is_some() {
            return Err("provide source or path, not both".into());
        }
        let project = if let Some(path) = request.get("path") {
            Some(crate::project::Project::load(std::path::Path::new(
                path.as_str().ok_or("path must be a string")?,
            ))?)
        } else {
            None
        };
        let source = match &project {
            Some(p) => p.source.clone(),
            None => request["source"]
                .as_str()
                .ok_or("provide source or path")?
                .to_string(),
        };
        if source.len() > files::SOURCE_LIMIT {
            return Err("source exceeds 4 MiB limit".into());
        }
        let mut args = vec![
            if method == "format" {
                "fmt".into()
            } else {
                method.into()
            },
            "<service>".into(),
        ];
        if let Some(values) = request.get("args") {
            for value in values
                .as_array()
                .ok_or("args must be an array of strings")?
            {
                args.push(
                    value
                        .as_str()
                        .ok_or("args must contain strings")?
                        .to_string(),
                );
            }
        }
        crate::cli::validate(&args)?;
        let snapshot = self.snapshot(source);
        let mut result = match &snapshot.data {
            Err(diagnostic) => diagnostic.clone(),
            Ok((program, analysis)) => match method {
                "check" => {
                    json!({"status":if analysis.holes.is_empty(){"CHECKED"}else{"INCOMPLETE"},"revision":crate::revision(&snapshot.source),"functions":program.functions.len(),"tests":program.tests.len(),"holes":analysis.holes})
                }
                "inspect" => crate::inspect(&snapshot.source, program, analysis, &args)?,
                "lint" => crate::lint::run(
                    &snapshot.source,
                    program,
                    args.iter().any(|a| a == "--deny-warnings"),
                ),
                "test" => crate::test_engine(&snapshot.source, program, analysis, &args)?,
                "format" => {
                    json!({"source":crate::format::source(&snapshot.source),"revision":crate::revision(&snapshot.source)})
                }
                _ => unreachable!(),
            },
        };
        if let Some(project) = project {
            project.annotate(&mut result);
        }
        Ok(result)
    }
}
pub fn serve(max_mib: usize) -> Result<()> {
    if max_mib == 0 || max_mib > 1024 {
        return Err("--max-cache-mib must be 1..1024".into());
    }
    let mut service = Service::new(max_mib * 1024 * 1024);
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    loop {
        let mut bytes = Vec::new();
        let mut oversized = false;
        let mut seen = false;
        loop {
            let chunk = input.fill_buf().map_err(|e| e.to_string())?;
            if chunk.is_empty() {
                break;
            }
            seen = true;
            let end = chunk.iter().position(|b| *b == b'\n').map(|i| i + 1);
            let n = end.unwrap_or(chunk.len());
            if bytes.len() + n <= 8 * 1024 * 1024 {
                bytes.extend_from_slice(&chunk[..n]);
            } else {
                oversized = true;
            }
            input.consume(n);
            if end.is_some() {
                break;
            }
        }
        if !seen {
            break;
        }
        let request = if oversized {
            Err("request exceeds 8 MiB limit".to_string())
        } else {
            serde_json::from_slice::<Value>(&bytes).map_err(|e| e.to_string())
        };
        let shutdown = request.as_ref().is_ok_and(|r| r["method"] == "shutdown");
        let response = match request {
            Ok(r) => service.request(r),
            Err(message) => json!({"id":null,"error":{"kind":"protocol_error","message":message}}),
        };
        serde_json::to_writer(&mut output, &response).map_err(|e| e.to_string())?;
        output.write_all(b"\n").map_err(|e| e.to_string())?;
        output.flush().map_err(|e| e.to_string())?;
        if shutdown {
            break;
        }
    }
    Ok(())
}
