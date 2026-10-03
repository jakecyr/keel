use crate::{Result, files};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    name: String,
    entry: String,
    #[serde(default)]
    sources: Vec<String>,
    #[serde(default)]
    tests: Vec<String>,
}
pub struct Part {
    pub path: PathBuf,
    pub source: String,
    pub start: usize,
    pub end: usize,
    pub protected: bool,
}
pub struct Project {
    pub source: String,
    pub parts: Vec<Part>,
    pub inputs: Vec<PathBuf>,
    pub name: String,
    pub manifest: Option<PathBuf>,
}
impl Project {
    pub fn load(path: &Path) -> Result<Self> {
        let manifest = if path.is_dir() {
            Some(path.join("keel.json"))
        } else if path.extension().is_some_and(|s| s == "json") {
            Some(path.to_owned())
        } else {
            None
        };
        let mut project = Self {
            source: String::new(),
            parts: Vec::new(),
            inputs: Vec::new(),
            name: path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            manifest: manifest.clone(),
        };
        let paths = if let Some(manifest) = manifest {
            if !manifest.exists() {
                return Err(format!(
                    "no Keel project found at {}; run `keel init` here, change to your project directory, or pass a .keel file",
                    manifest.display()
                ));
            }
            let config: Manifest = serde_json::from_str(&files::read(&manifest, 64 * 1024)?)
                .map_err(|e| format!("invalid project manifest: {e}"))?;
            if config.schema != 1
                || config.name.is_empty()
                || !config
                    .name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                return Err("manifest requires schema 1 and a simple nonempty project name".into());
            }
            project.name = config.name;
            project.inputs.push(manifest.clone());
            let root = fs::canonicalize(
                manifest
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new(".")),
            )
            .map_err(|e| e.to_string())?;
            let mut paths = Vec::new();
            let mut seen = BTreeSet::new();
            for (relative, protected) in std::iter::once((config.entry, false))
                .chain(config.sources.into_iter().map(|p| (p, false)))
                .chain(config.tests.into_iter().map(|p| (p, true)))
            {
                let p = Path::new(&relative);
                if p.is_absolute()
                    || p.components()
                        .any(|c| matches!(c, std::path::Component::ParentDir))
                {
                    return Err("project paths must stay inside the project directory".into());
                }
                let path =
                    fs::canonicalize(root.join(p)).map_err(|e| format!("{relative}: {e}"))?;
                if !path.starts_with(&root) || !seen.insert(path.clone()) {
                    return Err(format!("duplicate or escaping project path: {relative}"));
                }
                paths.push((path, protected));
            }
            paths
        } else {
            let path = fs::canonicalize(path).map_err(|e| e.to_string())?;
            let protected = protected_source(&path)?;
            vec![(path, protected)]
        };
        for (path, protected) in paths {
            let source = files::read(&path, files::SOURCE_LIMIT)?;
            if project.manifest.is_some() {
                crate::syntax::parse(&source).map_err(|d| {
                    format!(
                        "{}:{}:{}: each project file must parse independently: {}",
                        path.display(),
                        d.line,
                        d.column,
                        d.message
                    )
                })?;
            }
            let start = project.source.len();
            project.source.push_str(&source);
            let end = project.source.len();
            project.source.push('\n');
            if project.source.len() > files::SOURCE_LIMIT {
                return Err("project exceeds 4 MiB source limit".into());
            }
            project.inputs.push(path.clone());
            project.parts.push(Part {
                path,
                source,
                start,
                end,
                protected,
            });
        }
        // Preserve legacy single-file revisions byte-for-byte.
        if project.manifest.is_none() {
            project.source = project.parts[0].source.clone();
        }
        Ok(project)
    }
    pub fn at(&self, offset: usize) -> Option<&Part> {
        self.parts
            .iter()
            .find(|p| offset >= p.start && offset <= p.end)
    }
    pub fn annotate(&self, value: &mut Value) {
        match value {
            Value::Object(map) => {
                if let Some(offset) = map.get("offset").and_then(Value::as_u64)
                    && let Some(part) = self.at(offset as usize)
                {
                    let local = (offset as usize - part.start).min(part.source.len());
                    if part.source.is_char_boundary(local) {
                        let location = crate::syntax::Diagnostic::new(&part.source, local, "", "");
                        map.insert("file".into(), json!(part.path));
                        map.insert("file_offset".into(), json!(local));
                        map.insert("line".into(), json!(location.line));
                        map.insert("column".into(), json!(location.column));
                    }
                }
                for v in map.values_mut() {
                    self.annotate(v);
                }
            }
            Value::Array(items) => {
                for v in items {
                    self.annotate(v)
                }
            }
            _ => {}
        }
    }
}
fn protected_source(path: &Path) -> Result<bool> {
    for parent in path.ancestors().skip(1).take(16) {
        let manifest = parent.join("keel.json");
        if manifest.is_file() {
            let config: Manifest = serde_json::from_str(&files::read(&manifest, 64 * 1024)?)
                .map_err(|e| format!("invalid containing project manifest: {e}"))?;
            return Ok(config
                .tests
                .iter()
                .any(|test| files::aliases(path, &parent.join(test))));
        }
    }
    Ok(false)
}

pub fn init(path: &Path) -> Result<Value> {
    if path.exists() && !path.is_dir() {
        return Err("init target must be a directory".into());
    }
    let root = files::normalized(path)?;
    let raw_name = root
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("cannot initialize a filesystem root")?;
    let name: String = raw_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let already = root.join("keel.json").exists();
    let mut writes: Vec<(PathBuf, String)> = Vec::new();
    if already {
        let manifest: Manifest =
            serde_json::from_str(&files::read(&root.join("keel.json"), 64 * 1024)?)
                .map_err(|e| format!("invalid existing keel.json: {e}"))?;
        if manifest.schema != 1 {
            return Err("unsupported existing manifest schema".into());
        }
    } else {
        let manifest = json!({"schema":1,"name":name,"entry":"src/main.keel","sources":[],"tests":["tests/acceptance.keel"]});
        let templates=[
            ("keel.json",format!("{}\n",serde_json::to_string_pretty(&manifest).unwrap())),
            ("src/main.keel","pub fn greet() -> Text {\n    return \"Hello from Keel!\"\n}\n\nfn main() effects { io.stdout } {\n    io.println(greet())\n}\n".into()),
            ("tests/acceptance.keel","test \"greeting\" {\n    assert greet() == \"Hello from Keel!\"\n}\n".into()),
            ("keel.policy.json","{\n  \"schema\": 1,\n  \"stdout\": false,\n  \"listen\": []\n}\n".into()),
        ];
        for (name, contents) in templates {
            let target = root.join(name);
            if target.symlink_metadata().is_ok() {
                return Err(format!(
                    "init would overwrite {}; no project files were changed",
                    target.display()
                ));
            }
            writes.push((target, contents));
        }
    }
    for name in ["AGENTS.md", "CLAUDE.md"] {
        let file = root.join(name);
        files::protect(&file, &[])?;
        let old = if file.exists() {
            files::read(&file, 128 * 1024)?
        } else {
            String::new()
        };
        let new = crate::agent::merge_instructions(&old)?;
        if new != old {
            writes.push((file, new));
        }
    }
    let ignore = root.join(".gitignore");
    files::protect(&ignore, &[])?;
    let old = if ignore.exists() {
        files::read(&ignore, 128 * 1024)?
    } else {
        String::new()
    };
    let mut new = old.clone();
    for pattern in ["/build/", "*.keel-lock"] {
        if !old.lines().any(|line| line == pattern) {
            if !new.is_empty() && !new.ends_with('\n') {
                new.push('\n');
            }
            new.push_str(pattern);
            new.push('\n');
        }
    }
    if old != new {
        writes.push((ignore, new));
    }
    for (file, _) in &writes {
        if !files::normalized(file)?.starts_with(&root) {
            return Err("init destination escapes the project through a symbolic link".into());
        }
    }
    for (file, contents) in &writes {
        let permissions = fs::metadata(file).ok().map(|m| m.permissions());
        files::atomic_write(file, contents.as_bytes(), permissions)?;
    }
    Ok(
        json!({"status":if already{"INITIALIZED"}else{"CREATED"},"project":root,"working_directory":root,"manifest":root.join("keel.json"),"changed":writes.iter().map(|(p,_)|p).collect::<Vec<_>>(),"next":["keel agent context . --json","keel check .","keel test . --engine both","keel run . --allow-stdout"]}),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    schema: u32,
    #[serde(default)]
    stdout: bool,
    #[serde(default)]
    listen: Vec<String>,
}
pub fn policy(path: &Path) -> Result<Vec<String>> {
    let policy: Policy = serde_json::from_str(&files::read(path, 16 * 1024)?)
        .map_err(|e| format!("invalid runtime policy: {e}"))?;
    if policy.schema != 1 || policy.listen.len() > 1 {
        return Err("policy requires schema 1 and at most one listening endpoint".into());
    }
    let mut args = Vec::new();
    if policy.stdout {
        args.push("--allow-stdout".into());
    }
    for endpoint in policy.listen {
        let port = endpoint
            .strip_prefix("127.0.0.1:")
            .and_then(|p| p.parse::<u16>().ok())
            .filter(|p| *p > 0)
            .ok_or("policy listen must be 127.0.0.1:PORT (1..65535)")?;
        args.push(format!("--allow-net=127.0.0.1:{port}"));
    }
    Ok(args)
}
