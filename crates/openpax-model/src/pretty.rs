use serde_json::Value;

/// Longest object that is still written on a single line.
const INLINE_WIDTH: usize = 88;

/// Formats JSON the way model files are usually written: nested structure on separate lines,
/// but number lists and small leaf objects (a face, a rotation) kept on one line.
pub fn to_pretty_json(value: &Value) -> String {
    let mut out = String::new();
    write(value, 0, &mut out);
    out.push('\n');
    out
}

fn is_scalar(value: &Value) -> bool {
    !matches!(value, Value::Array(_) | Value::Object(_))
}

fn is_scalar_list(value: &Value) -> bool {
    matches!(value, Value::Array(items) if items.iter().all(is_scalar))
}

fn scalar(value: &Value) -> String {
    serde_json::to_string(value).expect("scalars are always serialisable")
}

fn inline_list(items: &[Value]) -> String {
    format!(
        "[{}]",
        items.iter().map(scalar).collect::<Vec<_>>().join(", ")
    )
}

/// The single-line form of an object whose values are all scalars or scalar lists.
fn inline_object(map: &serde_json::Map<String, Value>) -> Option<String> {
    let mut parts = Vec::with_capacity(map.len());
    for (key, value) in map {
        let rendered = match value {
            Value::Array(items) if is_scalar_list(value) => inline_list(items),
            _ if is_scalar(value) => scalar(value),
            _ => return None,
        };
        parts.push(format!(
            "{}: {rendered}",
            scalar(&Value::String(key.clone()))
        ));
    }
    Some(format!("{{{}}}", parts.join(", ")))
}

fn write(value: &Value, depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth + 1);
    let closing = "  ".repeat(depth);
    match value {
        Value::Array(items) if items.is_empty() => out.push_str("[]"),
        Value::Array(items) if is_scalar_list(value) => out.push_str(&inline_list(items)),
        Value::Array(items) => {
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                out.push_str(&indent);
                write(item, depth + 1, out);
                out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
            }
            out.push_str(&closing);
            out.push(']');
        }
        Value::Object(map) if map.is_empty() => out.push_str("{}"),
        Value::Object(map) => {
            if depth > 0
                && let Some(inline) = inline_object(map)
                && indent.len() + inline.len() <= INLINE_WIDTH
            {
                out.push_str(&inline);
                return;
            }
            out.push_str("{\n");
            for (i, (key, item)) in map.iter().enumerate() {
                out.push_str(&indent);
                out.push_str(&scalar(&Value::String(key.clone())));
                out.push_str(": ");
                write(item, depth + 1, out);
                out.push_str(if i + 1 < map.len() { ",\n" } else { "\n" });
            }
            out.push_str(&closing);
            out.push('}');
        }
        other => out.push_str(&scalar(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn layout() {
        let value = json!({
            "parent": "block/cube",
            "textures": { "0": "block/stone" },
            "elements": [
                { "from": [0, 0, 0], "to": [16, 16, 16], "faces": { "up": { "uv": [0, 0, 16, 16], "texture": "#0" } } }
            ],
            "empty": [],
            "none": {}
        });
        let expected = r##"{
  "parent": "block/cube",
  "textures": {"0": "block/stone"},
  "elements": [
    {
      "from": [0, 0, 0],
      "to": [16, 16, 16],
      "faces": {
        "up": {"uv": [0, 0, 16, 16], "texture": "#0"}
      }
    }
  ],
  "empty": [],
  "none": {}
}
"##;
        assert_eq!(to_pretty_json(&value), expected);
    }

    #[test]
    fn output_is_valid_json_with_escapes() {
        let value = json!({ "a\"b": "line\nbreak", "list": [1, 2.5, true, null, "x"] });
        let parsed: Value = serde_json::from_str(&to_pretty_json(&value)).unwrap();
        assert_eq!(parsed, value);
    }

    #[test]
    fn long_objects_wrap() {
        let long = "x".repeat(90);
        let text = to_pretty_json(&json!({ "outer": { "key": long } }));
        assert!(text.contains("\"outer\": {\n"), "{text}");
    }
}
