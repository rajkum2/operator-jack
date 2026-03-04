use std::collections::HashMap;

use crate::error::{CoreError, OperatorError};

/// Recursively interpolates variable references within a JSON value.
///
/// In strings, `$var_name` and `${var.dotted.path}` patterns are resolved
/// against the provided variables map.
///
/// - If the entire string is a single variable reference, the replacement
///   preserves the JSON type of the resolved value.
/// - If the string contains mixed text and variable references (template),
///   each referenced variable must resolve to a JSON string; otherwise an
///   `INTERPOLATION_TYPE_ERROR` is raised.
/// - Missing variables cause an `INTERPOLATION_MISSING` error.
pub fn interpolate_params(
    params: &serde_json::Value,
    variables: &HashMap<String, serde_json::Value>,
) -> Result<serde_json::Value, CoreError> {
    match params {
        serde_json::Value::String(s) => interpolate_string(s, variables),
        serde_json::Value::Array(arr) => {
            let mut result = Vec::with_capacity(arr.len());
            for item in arr {
                result.push(interpolate_params(item, variables)?);
            }
            Ok(serde_json::Value::Array(result))
        }
        serde_json::Value::Object(map) => {
            let mut result = serde_json::Map::new();
            for (key, value) in map {
                result.insert(key.clone(), interpolate_params(value, variables)?);
            }
            Ok(serde_json::Value::Object(result))
        }
        // Numbers, booleans, nulls pass through unchanged.
        other => Ok(other.clone()),
    }
}

/// Resolves a (possibly dotted) variable name against the variables map.
///
/// Supports dotted paths: `"step.x.output"` will look up key `"step"` in
/// the map, then descend into the resulting JSON object via `"x"` then
/// `"output"`.
pub fn resolve_variable(
    name: &str,
    variables: &HashMap<String, serde_json::Value>,
) -> Option<serde_json::Value> {
    let parts: Vec<&str> = name.splitn(2, '.').collect();
    let root_key = parts[0];

    let root_value = variables.get(root_key)?;

    if parts.len() == 1 {
        return Some(root_value.clone());
    }

    // Traverse the remaining dotted path
    let remaining = parts[1];
    let segments: Vec<&str> = remaining.split('.').collect();
    let mut current = root_value;

    for segment in &segments {
        match current {
            serde_json::Value::Object(map) => {
                current = map.get(*segment)?;
            }
            _ => return None,
        }
    }

    Some(current.clone())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Identifies all variable references in a string and determines whether the
/// whole string is a single reference or a template with mixed content.
fn interpolate_string(
    s: &str,
    variables: &HashMap<String, serde_json::Value>,
) -> Result<serde_json::Value, CoreError> {
    // Quick path: no dollar sign means no interpolation needed.
    if !s.contains('$') {
        return Ok(serde_json::Value::String(s.to_string()));
    }

    // Check if the entire string is a single variable reference.
    if let Some(var_name) = is_single_variable_ref(s) {
        return match resolve_variable(var_name, variables) {
            Some(val) => Ok(val),
            None => Err(CoreError::Operator(OperatorError::interpolation_missing(
                var_name,
            ))),
        };
    }

    // Template mode: build the string by replacing each variable reference.
    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' {
            let var_name = parse_variable_name(&mut chars);
            if var_name.is_empty() {
                // Bare '$' at end of string or followed by non-identifier char
                result.push('$');
                continue;
            }
            match resolve_variable(&var_name, variables) {
                Some(serde_json::Value::String(val)) => {
                    result.push_str(&val);
                }
                Some(_) => {
                    return Err(CoreError::Operator(
                        OperatorError::interpolation_type_error(&var_name, "a string"),
                    ));
                }
                None => {
                    return Err(CoreError::Operator(OperatorError::interpolation_missing(
                        &var_name,
                    )));
                }
            }
        } else {
            result.push(ch);
        }
    }

    Ok(serde_json::Value::String(result))
}

/// If the entire string is exactly one variable reference (`$foo` or
/// `${foo.bar}`), returns the variable name. Otherwise returns `None`.
fn is_single_variable_ref(s: &str) -> Option<&str> {
    let s = s.trim();
    if s.starts_with("${") && s.ends_with('}') {
        let inner = &s[2..s.len() - 1];
        // Verify the inner portion is a valid dotted variable name with no
        // extra braces or whitespace.
        if !inner.is_empty() && is_valid_var_path(inner) {
            return Some(inner);
        }
    } else if let Some(name) = s.strip_prefix('$') {
        if !name.is_empty() && is_valid_simple_var(name) {
            return Some(name);
        }
    }
    None
}

/// Parses a variable name starting after the '$'. Handles both `${...}` and
/// bare `$name` forms. Consumes characters from the iterator.
fn parse_variable_name(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    if chars.peek() == Some(&'{') {
        // Consume the opening brace
        chars.next();
        let mut name = String::new();
        for ch in chars.by_ref() {
            if ch == '}' {
                break;
            }
            name.push(ch);
        }
        name
    } else {
        // Bare $name: consume identifier characters [a-zA-Z0-9_]
        // Note: hyphens are only allowed inside ${...} braces to avoid
        // ambiguity with subtraction in bare $name context.
        let mut name = String::new();
        while let Some(&ch) = chars.peek() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                name.push(ch);
                chars.next();
            } else {
                break;
            }
        }
        name
    }
}

/// Returns true if `s` is a valid dotted variable path (e.g. "step.x.output").
fn is_valid_var_path(s: &str) -> bool {
    s.split('.')
        .all(|part| !part.is_empty() && is_valid_simple_var(part))
}

/// Returns true if `s` consists only of `[a-zA-Z0-9_-]` characters.
/// Hyphens are allowed since step IDs commonly use them (e.g. "step-1").
fn is_valid_simple_var(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn vars() -> HashMap<String, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("name".to_string(), json!("Alice"));
        m.insert("count".to_string(), json!(42));
        m.insert(
            "step".to_string(),
            json!({
                "step-1": { "output": { "text": "hello" }, "status": "succeeded" },
                "open_app": { "output": { "pid": 123 }, "status": "succeeded" }
            }),
        );
        m
    }

    #[test]
    fn test_simple_var() {
        let v = vars();
        let result = interpolate_params(&json!("$name"), &v).unwrap();
        assert_eq!(result, json!("Alice"));
    }

    #[test]
    fn test_braced_var() {
        let v = vars();
        let result = interpolate_params(&json!("${name}"), &v).unwrap();
        assert_eq!(result, json!("Alice"));
    }

    #[test]
    fn test_numeric_var_preserves_type() {
        let v = vars();
        let result = interpolate_params(&json!("$count"), &v).unwrap();
        assert_eq!(result, json!(42));
    }

    #[test]
    fn test_template_mode() {
        let v = vars();
        let result = interpolate_params(&json!("Hello $name!"), &v).unwrap();
        assert_eq!(result, json!("Hello Alice!"));
    }

    #[test]
    fn test_dotted_path() {
        let v = vars();
        let result = interpolate_params(&json!("${step.open_app.output.pid}"), &v).unwrap();
        assert_eq!(result, json!(123));
    }

    #[test]
    fn test_hyphenated_step_id() {
        let v = vars();
        // This was broken before the P3 fix.
        let result = interpolate_params(&json!("${step.step-1.output.text}"), &v).unwrap();
        assert_eq!(result, json!("hello"));
    }

    #[test]
    fn test_missing_var_error() {
        let v = vars();
        let result = interpolate_params(&json!("$nonexistent"), &v);
        assert!(result.is_err());
    }

    #[test]
    fn test_no_interpolation_needed() {
        let v = vars();
        let result = interpolate_params(&json!("plain text"), &v).unwrap();
        assert_eq!(result, json!("plain text"));
    }

    #[test]
    fn test_object_recursion() {
        let v = vars();
        let input = json!({"greeting": "Hello $name", "nested": {"val": "$count"}});
        let result = interpolate_params(&input, &v).unwrap();
        assert_eq!(result["greeting"], json!("Hello Alice"));
        assert_eq!(result["nested"]["val"], json!(42));
    }

    #[test]
    fn test_array_recursion() {
        let v = vars();
        let input = json!(["$name", "$count", "literal"]);
        let result = interpolate_params(&input, &v).unwrap();
        assert_eq!(result, json!(["Alice", 42, "literal"]));
    }

    #[test]
    fn test_template_with_non_string_var_errors() {
        let v = vars();
        // In template mode (mixed text + var), non-string vars must error.
        let result = interpolate_params(&json!("count is $count items"), &v);
        assert!(result.is_err());
    }

    #[test]
    fn test_valid_var_path_with_hyphens() {
        assert!(is_valid_var_path("step.step-1.output"));
        assert!(is_valid_var_path("my-var"));
        assert!(is_valid_var_path("a.b.c"));
        assert!(!is_valid_var_path(""));
        assert!(!is_valid_var_path("."));
        assert!(!is_valid_var_path("a..b"));
    }
}
