//! Agent-facing TypeScript documentation derived from the tool-owned schema.
//! Constraints that TypeScript cannot express remain field comments.
use serde_json::Value;

pub(super) fn describe(schema: &Value) -> String {
    let mut output = comments(schema, 0);
    output.push_str(&format!("type Arguments = {};", render(schema, schema, 0)));
    output
}

fn comments(schema: &Value, depth: usize) -> String {
    let mut notes = Vec::new();
    if let Some(description) = schema["description"].as_str() {
        notes.extend(description.lines().map(str::to_owned));
    }
    if schema["type"] == "integer" {
        notes.push("Integer.".into());
    }
    for (key, label) in [
        ("minimum", "Minimum"),
        ("maximum", "Maximum"),
        ("exclusiveMinimum", "Greater than"),
        ("exclusiveMaximum", "Less than"),
        ("multipleOf", "Multiple of"),
        ("minLength", "Min length"),
        ("maxLength", "Max length"),
        ("minItems", "Min items"),
        ("maxItems", "Max items"),
        ("pattern", "Pattern"),
        ("format", "Format"),
        ("default", "Default"),
    ] {
        if let Some(value) = schema.get(key) {
            notes.push(format!("{label}: {value}."));
        }
    }
    if schema["uniqueItems"] == true {
        notes.push("Items must be unique.".into());
    }
    let indent = "  ".repeat(depth);
    notes
        .into_iter()
        .map(|line| {
            format!(
                "{indent}// {}\n",
                line.replace(['\r', '\u{2028}', '\u{2029}'], " ")
            )
        })
        .collect()
}

fn render(schema: &Value, root: &Value, depth: usize) -> String {
    if depth > 24 {
        return "unknown /* recursive value */".into();
    }
    if schema == &Value::Bool(false) {
        return "never".into();
    }
    if let Some(reference) = schema["$ref"].as_str() {
        if let Some(target) = reference.strip_prefix('#').and_then(|p| root.pointer(p)) {
            return render(target, root, depth + 1);
        }
        return "unknown /* external reference */".into();
    }
    if let Some(value) = schema.get("const") {
        return value.to_string();
    }
    if let Some(values) = schema["enum"].as_array() {
        return if values.is_empty() {
            "never".into()
        } else {
            values
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join(" | ")
        };
    }
    for (key, join) in [("oneOf", " | "), ("anyOf", " | "), ("allOf", " & ")] {
        if let Some(branches) = schema[key].as_array() {
            return branches
                .iter()
                .map(|v| format!("({})", render(v, root, depth)))
                .collect::<Vec<_>>()
                .join(join);
        }
    }
    if let Some(types) = schema["type"].as_array() {
        return types
            .iter()
            .map(|kind| {
                let mut branch = schema.clone();
                branch["type"] = kind.clone();
                render(&branch, root, depth)
            })
            .collect::<Vec<_>>()
            .join(" | ");
    }
    match schema["type"].as_str() {
        Some("string") => "string".into(),
        Some("number" | "integer") => "number".into(),
        Some("boolean") => "boolean".into(),
        Some("null") => "null".into(),
        Some("array") => format!("Array<{}>", render(&schema["items"], root, depth)),
        Some("object") => object(schema, root, depth),
        _ if schema.get("properties").is_some() => object(schema, root, depth),
        _ => "unknown".into(),
    }
}

fn object(schema: &Value, root: &Value, depth: usize) -> String {
    let properties = schema["properties"].as_object();
    let extra = schema.get("additionalProperties");
    if properties.is_none_or(|p| p.is_empty()) {
        let value = match extra {
            Some(Value::Bool(false)) => "never".into(),
            Some(value) if value.is_object() => render(value, root, depth),
            _ => "unknown".into(),
        };
        return format!("Record<string, {value}>");
    }
    let required = schema["required"].as_array();
    let indent = "  ".repeat(depth + 1);
    let mut output = String::from("{\n");
    for (name, field) in properties.unwrap() {
        output.push_str(&comments(field, depth + 1));
        let required =
            required.is_some_and(|fields| fields.iter().any(|v| v.as_str() == Some(name)));
        let optional = if required { "" } else { "?" };
        let key = if !name.is_empty()
            && name.bytes().enumerate().all(|(i, b)| {
                b.is_ascii_alphabetic() || b == b'_' || b == b'$' || (i > 0 && b.is_ascii_digit())
            }) {
            name.clone()
        } else {
            serde_json::to_string(name).expect("property name")
        };
        output.push_str(&format!(
            "{indent}{key}{optional}: {};\n",
            render(field, root, depth + 1)
        ));
    }
    if extra != Some(&Value::Bool(false)) {
        if let Some(value) = extra.filter(|v| v.is_object()) {
            output.push_str(&format!(
                "{indent}// Additional keys: {}.\n",
                render(value, root, depth + 1).replace('\n', " ")
            ));
        }
        output.push_str(&format!("{indent}[key: string]: unknown;\n"));
    }
    output.push_str(&format!("{}}}", "  ".repeat(depth)));
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn types_explain_required_fields_grant_unions_and_limits() {
        let text = describe(
            &json!({"type":"object","required":["grant"],"additionalProperties":false,"properties":{
                "grant":{"oneOf":[
                    {"type":"object","properties":{"scope":{"const":"mesh"}},"required":["scope"],"additionalProperties":false},
                    {"type":"object","properties":{"scope":{"const":"selected"},"subjects":{"type":"array","items":{"type":"string"}}},"required":["scope","subjects"],"additionalProperties":false}
                ]},
                "timeout_seconds":{"type":"integer","minimum":1,"maximum":86400,"description":"Execution deadline."},
                "secret_env":{"type":"object","additionalProperties":{"type":"string"},"description":"Environment variable names, never secret values."},
                "allowlist":{"type":["array","null"],"items":{"type":"string"}}
            }}),
        );
        assert!(text.contains("grant: ({"));
        assert!(text.contains("scope: \"mesh\";"));
        assert!(text.contains("scope: \"selected\";"));
        assert!(text.contains("subjects: Array<string>;"));
        assert!(text.contains(
            "// Integer.\n  // Minimum: 1.\n  // Maximum: 86400.\n  timeout_seconds?: number;"
        ));
        assert!(text.contains("secret_env?: Record<string, string>;"));
        assert!(text.contains("allowlist?: Array<string> | null;"));
        assert!(!text.contains("additionalProperties"));
    }

    #[test]
    fn references_quoted_names_and_multiline_comments_remain_typescript() {
        let text = describe(
            &json!({"type":"object","additionalProperties":false,"$defs":{"text":{"type":"string"}},"properties":{
                "header-name":{"$ref":"#/$defs/text","description":"First line.\nSecond line."}
            }}),
        );
        assert!(text.contains("// First line.\n  // Second line.\n  \"header-name\"?: string;"));
        assert_eq!(
            describe(&json!({"type":"object","additionalProperties":false})),
            "type Arguments = Record<string, never>;"
        );
    }
}
