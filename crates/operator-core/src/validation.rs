use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

use crate::error::OperatorError;
use crate::selector::Selector;
use crate::types::{Plan, StepType};

/// Regex for validating step IDs: must start with a letter, followed by up to
/// 63 alphanumeric, underscore, or hyphen characters.
static STEP_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z][a-zA-Z0-9_-]{0,63}$").unwrap());

/// Validates a plan, returning a list of all validation errors found.
///
/// The function performs multiple checks and accumulates errors rather than
/// failing on the first one.
pub fn validate_plan(plan: &Plan) -> Result<(), Vec<OperatorError>> {
    let mut errors: Vec<OperatorError> = Vec::new();

    // 1. schema_version must be 1
    if plan.schema_version != 1 {
        errors.push(OperatorError::validation_error(format!(
            "schema_version must be 1, got {}",
            plan.schema_version
        )));
    }

    // 2. name must be non-empty
    if plan.name.trim().is_empty() {
        errors.push(OperatorError::validation_error(
            "plan name must not be empty",
        ));
    }

    // 3. steps must be non-empty
    if plan.steps.is_empty() {
        errors.push(OperatorError::validation_error(
            "plan must have at least one step",
        ));
    }

    // 4. Validate step IDs are unique and well-formed
    let mut seen_ids: HashSet<String> = HashSet::new();
    // Track which step IDs have been "completed" (available for forward-ref check)
    let mut completed_step_ids: HashSet<String> = HashSet::new();

    for (index, step) in plan.steps.iter().enumerate() {
        // 4a. Step ID format
        if !STEP_ID_RE.is_match(&step.id) {
            errors.push(OperatorError::validation_error(format!(
                "step[{}] id '{}' does not match required pattern ^[a-zA-Z][a-zA-Z0-9_-]{{0,63}}$",
                index, step.id
            )));
        }

        // 4b. Step ID uniqueness
        if !seen_ids.insert(step.id.clone()) {
            errors.push(OperatorError::validation_error(format!(
                "step[{}] duplicate id '{}'",
                index, step.id
            )));
        }

        // 5. Check for unsupported browser.* types (this is handled by serde
        //    deserialization, but we note it for completeness). Since StepType
        //    is an enum, unknown types would have failed to parse. We validate
        //    the step_type is known by checking it serializes correctly.
        let type_str = step.step_type.to_string();
        if type_str.starts_with("browser.") {
            errors.push(OperatorError::unsupported_step_type(&type_str));
        }

        // 6. Validate required params per step type
        if let Err(msg) = validate_step_params(&step.step_type, &step.params) {
            errors.push(OperatorError::validation_error(format!(
                "step[{}] '{}': {}",
                index, step.id, msg
            )));
        }

        // 7. Validate selectors in UI step params
        if step.step_type.lane() == "ui" {
            if let Some(selector_val) = step.params.get("selector") {
                match serde_json::from_value::<Selector>(selector_val.clone()) {
                    Ok(sel) => {
                        if let Err(e) = sel.validate() {
                            errors.push(OperatorError::validation_error(format!(
                                "step[{}] '{}' selector: {}",
                                index, step.id, e
                            )));
                        }
                    }
                    Err(e) => {
                        errors.push(OperatorError::validation_error(format!(
                            "step[{}] '{}' selector parse error: {}",
                            index, step.id, e
                        )));
                    }
                }
            }
        }

        // 8. Check variable references don't forward-reference future step outputs
        check_forward_references(
            &step.params,
            &completed_step_ids,
            index,
            &step.id,
            &mut errors,
        );

        // Mark this step as completed for subsequent forward-reference checks
        completed_step_ids.insert(step.id.clone());
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Validates that the required parameters are present for a given step type.
pub fn validate_step_params(
    step_type: &StepType,
    params: &serde_json::Value,
) -> Result<(), String> {
    match step_type {
        StepType::SysOpenApp => require_string_param(params, "app"),
        StepType::SysOpenUrl => require_string_param(params, "url"),
        StepType::SysReadFile => require_string_param(params, "path"),
        StepType::SysWriteFile => {
            require_string_param(params, "path")?;
            require_string_param(params, "content")
        }
        StepType::SysAppendFile => {
            require_string_param(params, "path")?;
            require_string_param(params, "content")
        }
        StepType::SysMkdir => require_string_param(params, "path"),
        StepType::SysMovePath => {
            require_string_param(params, "from")?;
            require_string_param(params, "to")
        }
        StepType::SysCopyPath => {
            require_string_param(params, "from")?;
            require_string_param(params, "to")
        }
        StepType::SysDeletePath => require_string_param(params, "path"),
        StepType::SysExec => require_string_param(params, "command"),
        StepType::SysQuitApp => require_string_param(params, "app"),
        StepType::SysClipboardGet => Ok(()),
        StepType::SysClipboardSet => require_string_param(params, "text"),
        StepType::UiCheckAccessibilityPermission => Ok(()),
        StepType::UiListApps => Ok(()),
        StepType::UiFocusApp => require_string_param(params, "app"),
        StepType::UiFind => {
            require_string_param(params, "app")?;
            require_object_param(params, "selector")
        }
        StepType::UiClick => {
            require_string_param(params, "app")?;
            require_selector_or_element_ref(params)
        }
        StepType::UiSetValue => {
            require_string_param(params, "app")?;
            require_selector_or_element_ref(params)?;
            require_string_param(params, "value")
        }
        StepType::UiTypeText => {
            require_string_param(params, "app")?;
            require_string_param(params, "text")
        }
        StepType::UiKeyPress => {
            require_string_param(params, "app")?;
            require_string_param(params, "key")
        }
        StepType::UiReadText => {
            require_string_param(params, "app")?;
            require_selector_or_element_ref(params)
        }
        StepType::UiWaitFor => {
            require_string_param(params, "app")?;
            require_object_param(params, "selector")
        }
        StepType::UiSelectMenu => {
            require_string_param(params, "app")?;
            require_array_param(params, "menu_path")
        }
        StepType::UiListWindows => require_string_param(params, "app"),
        StepType::UiFocusWindow => {
            require_string_param(params, "app")?;
            require_object_param(params, "window")
        }
    }
}

// ---------------------------------------------------------------------------
// Parameter requirement helpers
// ---------------------------------------------------------------------------

fn require_string_param(params: &serde_json::Value, key: &str) -> Result<(), String> {
    match params.get(key) {
        Some(serde_json::Value::String(_)) => Ok(()),
        Some(_) => Err(format!("param '{}' must be a string", key)),
        None => Err(format!("missing required param '{}'", key)),
    }
}

fn require_object_param(params: &serde_json::Value, key: &str) -> Result<(), String> {
    match params.get(key) {
        Some(serde_json::Value::Object(_)) => Ok(()),
        Some(_) => Err(format!("param '{}' must be an object", key)),
        None => Err(format!("missing required param '{}'", key)),
    }
}

/// Requires either a `selector` object or an `element_ref` string param.
fn require_selector_or_element_ref(params: &serde_json::Value) -> Result<(), String> {
    let has_selector = matches!(params.get("selector"), Some(serde_json::Value::Object(_)));
    let has_element_ref = matches!(
        params.get("element_ref"),
        Some(serde_json::Value::String(_))
    );

    if has_selector || has_element_ref {
        Ok(())
    } else {
        Err("missing required 'selector' (object) or 'element_ref' (string) param".to_string())
    }
}

fn require_array_param(params: &serde_json::Value, key: &str) -> Result<(), String> {
    match params.get(key) {
        Some(serde_json::Value::Array(_)) => Ok(()),
        Some(_) => Err(format!("param '{}' must be an array", key)),
        None => Err(format!("missing required param '{}'", key)),
    }
}

// ---------------------------------------------------------------------------
// Forward-reference checking
// ---------------------------------------------------------------------------

/// Recursively scans a JSON value for variable references of the form
/// `$step.<id>.output...` and checks that the referenced step ID has already
/// been completed (appears earlier in the plan).
fn check_forward_references(
    value: &serde_json::Value,
    completed_ids: &HashSet<String>,
    step_index: usize,
    step_id: &str,
    errors: &mut Vec<OperatorError>,
) {
    match value {
        serde_json::Value::String(s) => {
            // Look for $step.<id> or ${step.<id>...} references
            let refs = extract_step_references(s);
            for referenced_id in refs {
                if !completed_ids.contains(&referenced_id) {
                    errors.push(OperatorError::validation_error(format!(
                        "step[{}] '{}' references future step output 'step.{}'",
                        step_index, step_id, referenced_id
                    )));
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                check_forward_references(item, completed_ids, step_index, step_id, errors);
            }
        }
        serde_json::Value::Object(map) => {
            for val in map.values() {
                check_forward_references(val, completed_ids, step_index, step_id, errors);
            }
        }
        _ => {}
    }
}

/// Extracts step IDs referenced via `$step.<id>` or `${step.<id>...}` patterns
/// in a string.
fn extract_step_references(s: &str) -> Vec<String> {
    let mut refs = Vec::new();
    let mut remaining = s;

    while let Some(dollar_pos) = remaining.find('$') {
        let after_dollar = &remaining[dollar_pos + 1..];

        if after_dollar.starts_with('{') {
            // ${...} form
            if let Some(close) = after_dollar.find('}') {
                let inner = &after_dollar[1..close];
                if let Some(step_id) = extract_step_id_from_path(inner) {
                    refs.push(step_id);
                }
                remaining = &after_dollar[close + 1..];
            } else {
                break;
            }
        } else {
            // $name form - consume identifier chars
            let name: String = after_dollar
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                // For bare $step references, we can't get the step ID since
                // bare names don't support dots. Only ${step.x.output} form
                // can reference step outputs. But we also handle `$step_x`
                // which is just a plain variable, not a step reference.
                // Step references require dotted paths like step.<id>.
            }
            remaining = &after_dollar[name.len()..];
        }
    }

    refs
}

/// Given a dotted path like "step.myStep.output.field", extracts "myStep" if
/// the path starts with "step.".
fn extract_step_id_from_path(path: &str) -> Option<String> {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.len() >= 2 && parts[0] == "step" {
        Some(parts[1].to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_list_windows_requires_app() {
        let result = validate_step_params(&StepType::UiListWindows, &json!({}));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("app"));

        let result = validate_step_params(&StepType::UiListWindows, &json!({"app": "Finder"}));
        assert!(result.is_ok());
    }

    #[test]
    fn test_focus_window_requires_app_and_window() {
        // Missing both
        let result = validate_step_params(&StepType::UiFocusWindow, &json!({}));
        assert!(result.is_err());

        // Missing window
        let result = validate_step_params(&StepType::UiFocusWindow, &json!({"app": "Finder"}));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("window"));

        // Valid
        let result = validate_step_params(
            &StepType::UiFocusWindow,
            &json!({"app": "Finder", "window": {"index": 0}}),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_focus_window_window_must_be_object() {
        let result = validate_step_params(
            &StepType::UiFocusWindow,
            &json!({"app": "Finder", "window": "main"}),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("object"));
    }

    #[test]
    fn test_ui_click_requires_app_and_selector() {
        let result = validate_step_params(&StepType::UiClick, &json!({"app": "Finder"}));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("selector"));

        let result = validate_step_params(
            &StepType::UiClick,
            &json!({"app": "Finder", "selector": {"role": "AXButton"}}),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_element_ref_accepted_instead_of_selector() {
        // click with element_ref instead of selector
        let result = validate_step_params(
            &StepType::UiClick,
            &json!({"app": "Finder", "element_ref": "01234567890ABCDEF"}),
        );
        assert!(result.is_ok());

        // readText with element_ref
        let result = validate_step_params(
            &StepType::UiReadText,
            &json!({"app": "Finder", "element_ref": "01234567890ABCDEF"}),
        );
        assert!(result.is_ok());

        // setValue with element_ref + value
        let result = validate_step_params(
            &StepType::UiSetValue,
            &json!({"app": "Finder", "element_ref": "01234567890ABCDEF", "value": "hello"}),
        );
        assert!(result.is_ok());

        // click with neither selector nor element_ref should fail
        let result = validate_step_params(&StepType::UiClick, &json!({"app": "Finder"}));
        assert!(result.is_err());
    }
}
