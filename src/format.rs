//! Conservative formatter: preserve token spelling, comments, line structure, and string bytes.
pub fn source(input: &str) -> String {
    let mut output = String::new();
    let mut depth = 0_usize;
    let mut quoted = false;
    let mut escaped = false;
    let mut blank = false;
    for original in input.split_inclusive('\n') {
        let line = original.strip_suffix('\n').unwrap_or(original);
        let started_quoted = quoted;
        let text = if started_quoted {
            line
        } else {
            line.trim_start()
        };
        let mut leading_close = 0;
        if !started_quoted {
            for c in text.chars() {
                if c == '}' {
                    leading_close += 1
                } else if !c.is_whitespace() {
                    break;
                }
            }
        }
        let indent = depth.saturating_sub(leading_close);
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if quoted {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    quoted = false;
                }
            } else if c == '/' && chars.peek() == Some(&'/') {
                break;
            } else if c == '"' {
                quoted = true;
            } else if c == '{' {
                depth += 1;
            } else if c == '}' {
                depth = depth.saturating_sub(1);
            }
        }
        let rendered = if quoted { text } else { text.trim_end() };
        if rendered.is_empty() && !started_quoted && !quoted {
            if !blank && !output.is_empty() {
                output.push('\n');
            }
            blank = true;
            continue;
        }
        blank = false;
        if !started_quoted {
            output.push_str(&"    ".repeat(indent));
        }
        output.push_str(rendered);
        if original.ends_with('\n') || !quoted {
            output.push('\n');
        }
    }
    if !quoted {
        while output.ends_with("\n\n") {
            output.pop();
        }
    }
    output
}

#[cfg(test)]
mod tests {
    #[test]
    fn idempotent_and_preserves_multiline_strings_and_comments() {
        let input = "// { comment\nfn main() {\n let x = \"a\n  b\"\n if true {\nassert x == \"a\n  b\" // }\n}\n}\n";
        let formatted = super::source(input);
        assert_eq!(super::source(&formatted), formatted);
        assert!(formatted.contains("    let x = \"a\n  b\""));
        crate::syntax::parse(&formatted).unwrap();
        assert!(formatted.contains("        assert x"));
    }
}
