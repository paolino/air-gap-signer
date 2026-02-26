use serde_json::Value;

/// A line in the display layout.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayLine {
    pub indent: usize,
    pub key: Option<String>,
    pub value: String,
}

/// Convert a JSON value into a flat list of display lines
/// suitable for rendering on a simple framebuffer.
pub fn json_to_lines(value: &Value) -> Vec<DisplayLine> {
    let mut lines = Vec::new();
    flatten(value, 0, None, &mut lines);
    lines
}

fn flatten(value: &Value, indent: usize, key: Option<&str>, out: &mut Vec<DisplayLine>) {
    match value {
        Value::Object(map) => {
            if let Some(k) = key {
                out.push(DisplayLine {
                    indent,
                    key: Some(k.into()),
                    value: String::new(),
                });
            }
            for (k, v) in map {
                flatten(v, indent + 1, Some(k), out);
            }
        }
        Value::Array(arr) => {
            if let Some(k) = key {
                out.push(DisplayLine {
                    indent,
                    key: Some(k.into()),
                    value: format!("[{} items]", arr.len()),
                });
            }
            for (i, v) in arr.iter().enumerate() {
                flatten(v, indent + 1, Some(&format!("[{i}]")), out);
            }
        }
        _ => {
            let text = match value {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                Value::Null => "null".into(),
                _ => unreachable!(),
            };
            out.push(DisplayLine {
                indent,
                key: key.map(Into::into),
                value: text,
            });
        }
    }
}

/// Render display lines to a plain-text string (for terminal / testing).
pub fn render_text(lines: &[DisplayLine]) -> String {
    let mut out = String::new();
    for line in lines {
        let pad = "  ".repeat(line.indent);
        match &line.key {
            Some(k) if line.value.is_empty() => out.push_str(&format!("{pad}{k}:\n")),
            Some(k) => out.push_str(&format!("{pad}{k}: {}\n", line.value)),
            None => out.push_str(&format!("{pad}{}\n", line.value)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn simple_object() {
        let val = json!({"to": "addr1...", "amount": 42});
        let lines = json_to_lines(&val);
        assert!(lines.iter().any(|l| l.key.as_deref() == Some("to")));
        assert!(lines.iter().any(|l| l.key.as_deref() == Some("amount")));
    }

    #[test]
    fn nested_object() {
        let val = json!({"tx": {"to": "addr1", "value": "5 ADA"}});
        let lines = json_to_lines(&val);
        let text = render_text(&lines);
        assert!(text.contains("tx:"));
        assert!(text.contains("  to: addr1"));
    }

    #[test]
    fn array_values() {
        let val = json!({"outputs": [{"addr": "a"}, {"addr": "b"}]});
        let lines = json_to_lines(&val);
        let text = render_text(&lines);
        assert!(text.contains("[2 items]"));
    }
}
