//! Independent reference implementations of pure standard-library operations.
use serde_json::value::RawValue;
use std::collections::BTreeMap;
const LIMIT: usize = 1024 * 1024;
pub type TextResult = Result<String, String>;
struct RawMembers<'a>(Vec<(String, &'a RawValue)>);
impl<'de> serde::Deserialize<'de> for RawMembers<'de> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct MembersVisitor;
        impl<'de> serde::de::Visitor<'de> for MembersVisitor {
            type Value = RawMembers<'de>;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("JSON object")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<Self::Value, M::Error> {
                let mut members = Vec::new();
                while let Some(member) = map.next_entry()? {
                    members.push(member);
                }
                Ok(RawMembers(members))
            }
        }
        deserializer.deserialize_map(MembersVisitor)
    }
}
fn raw(source: &str, depth: usize) -> Result<&RawValue, String> {
    if source.len() > LIMIT || depth > 64 {
        return Err("invalid JSON".into());
    }
    let value: &RawValue = serde_json::from_str(source).map_err(|_| "invalid JSON")?;
    match value.get().as_bytes().first() {
        Some(b'{') => {
            let members: RawMembers<'_> =
                serde_json::from_str(source).map_err(|_| "invalid JSON")?;
            for (_, child) in members.0 {
                raw(child.get(), depth + 1)?;
            }
        }
        Some(b'[') => {
            let children: Vec<&RawValue> =
                serde_json::from_str(source).map_err(|_| "invalid JSON")?;
            for child in children {
                raw(child.get(), depth + 1)?;
            }
        }
        Some(b'"') => {
            let _: String = serde_json::from_str(source).map_err(|_| "invalid JSON")?;
        }
        _ => {}
    }
    Ok(value)
}
pub fn json_parse(source: &str) -> TextResult {
    raw(source, 0)?;
    Ok(source.into())
}
pub fn json_get(source: &str, pointer: &str) -> TextResult {
    let mut value = raw(source, 0)?;
    if pointer.len() > LIMIT || (!pointer.is_empty() && !pointer.starts_with('/')) {
        return Err("invalid JSON pointer".into());
    }
    for part in pointer.split('/').skip(1) {
        let mut key = String::new();
        let mut chars = part.chars();
        while let Some(c) = chars.next() {
            key.push(if c == '~' {
                match chars.next() {
                    Some('0') => '~',
                    Some('1') => '/',
                    _ => return Err("invalid JSON pointer".into()),
                }
            } else {
                c
            });
        }
        value = match value.get().as_bytes().first() {
            Some(b'{') => {
                let object: BTreeMap<String, &RawValue> =
                    serde_json::from_str(value.get()).map_err(|_| "invalid JSON")?;
                object.get(&key).copied()
            }
            Some(b'[') => {
                let array: Vec<&RawValue> =
                    serde_json::from_str(value.get()).map_err(|_| "invalid JSON")?;
                if key.is_empty() || key.starts_with('+') || (key.len() > 1 && key.starts_with('0'))
                {
                    None
                } else {
                    key.parse::<usize>()
                        .ok()
                        .and_then(|n| array.get(n).copied())
                }
            }
            _ => None,
        }
        .ok_or("JSON path not found")?;
    }
    Ok(value.get().into())
}
pub fn json_text(source: &str, path: &str) -> TextResult {
    serde_json::from_str::<String>(&json_get(source, path)?)
        .map_err(|_| "expected JSON string".into())
}
pub fn json_quote(source: &str) -> String {
    let mut out = String::from("\"");
    for c in source.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
pub fn csv_get(source: &str, row: i64, column: i64) -> TextResult {
    if source.len() > LIMIT {
        return Err("invalid CSV".into());
    }
    if row < 0 || column < 0 {
        return Err("CSV cell not found".into());
    }
    let bytes = source.as_bytes();
    let mut p = 0;
    let mut rows = Vec::new();
    let mut record = Vec::new();
    while p < bytes.len() {
        let mut field = Vec::new();
        if bytes[p] == b'"' {
            p += 1;
            loop {
                let Some(&b) = bytes.get(p) else {
                    return Err("invalid CSV".into());
                };
                p += 1;
                if b == b'"' {
                    if bytes.get(p) == Some(&b'"') {
                        field.push(b);
                        p += 1;
                    } else {
                        break;
                    }
                } else {
                    field.push(b);
                }
            }
        } else {
            while p < bytes.len() && !b",\r\n".contains(&bytes[p]) {
                if bytes[p] == b'"' {
                    return Err("invalid CSV".into());
                }
                field.push(bytes[p]);
                p += 1;
            }
        }
        record.push(String::from_utf8(field).map_err(|_| "invalid CSV")?);
        match bytes.get(p) {
            None => {}
            Some(b',') => {
                p += 1;
                if p == bytes.len() {
                    record.push(String::new());
                }
                continue;
            }
            Some(b'\n') => p += 1,
            Some(b'\r') => {
                p += 1;
                if bytes.get(p) != Some(&b'\n') {
                    return Err("invalid CSV".into());
                }
                p += 1;
            }
            _ => return Err("invalid CSV".into()),
        }
        rows.push(std::mem::take(&mut record));
    }
    if !record.is_empty() {
        rows.push(record);
    }
    rows.get(row as usize)
        .and_then(|r| r.get(column as usize))
        .cloned()
        .ok_or("CSV cell not found".into())
}
pub fn sse_data(source: &str, wanted: i64) -> TextResult {
    if source.len() > LIMIT {
        return Err("invalid SSE".into());
    }
    if wanted < 0 {
        return Err("SSE event not found".into());
    }
    let source = source
        .strip_prefix('\u{feff}')
        .unwrap_or(source)
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let mut data = Vec::new();
    let mut events = 0;
    for line in source.split_terminator('\n') {
        if line.is_empty() {
            if !data.is_empty() {
                if events == wanted {
                    return Ok(data.join("\n"));
                }
                events += 1;
                data.clear();
            }
        } else if line == "data" {
            data.push("");
        } else if let Some(rest) = line.strip_prefix("data:") {
            data.push(rest.strip_prefix(' ').unwrap_or(rest));
        }
    }
    Err("SSE event not found".into())
}
pub fn xml_text(source: &str, path: &str) -> TextResult {
    if source.len() > LIMIT || path.len() > LIMIT || source.contains('\0') || path.contains('\0') {
        return Err("invalid XML".into());
    }
    if source.contains("<!DOCTYPE") {
        return Err("XML DTDs are disabled".into());
    }
    let doc = roxmltree::Document::parse(source).map_err(|_| "invalid XML")?;
    if doc
        .descendants()
        .filter(|n| n.is_element())
        .any(|n| n.ancestors().filter(|a| a.is_element()).count() > 64)
    {
        return Err("invalid XML".into());
    }
    if !path.starts_with('/') {
        return Err("invalid XML path".into());
    }
    let mut node = doc.root();
    for name in path.split('/').skip(1) {
        if name.is_empty() {
            return Err("invalid XML path".into());
        }
        node = node
            .children()
            .find(|n| {
                n.is_element() && n.tag_name().namespace().is_none() && n.tag_name().name() == name
            })
            .ok_or("XML path not found")?;
    }
    Ok(node
        .descendants()
        .filter(|n| n.is_text())
        .filter_map(|n| n.text())
        .collect())
}
pub fn dotenv_get(source: &str, name: &str) -> TextResult {
    if source.len() > LIMIT {
        return Err("invalid dotenv".into());
    }
    let mut found = None;
    for line in source.lines() {
        let line = line.trim_matches([' ', '\t', '\r']);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err("invalid dotenv".into());
        };
        let key = key.trim_matches([' ', '\t']);
        if key.is_empty()
            || !key
                .bytes()
                .enumerate()
                .all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || (i > 0 && b.is_ascii_digit()))
        {
            return Err("invalid dotenv".into());
        }
        let mut value = value.trim_matches([' ', '\t']);
        if value.starts_with(['\'', '"']) {
            if value.len() < 2 || value.as_bytes().last() != value.as_bytes().first() {
                return Err("invalid dotenv".into());
            }
            value = &value[1..value.len() - 1];
        }
        if key == name {
            found = Some(value.to_string());
        }
    }
    found.ok_or("dotenv key not found".into())
}
