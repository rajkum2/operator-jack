//! Deterministic control flow (M13): `control.foreach`, `control.if`,
//! `control.repeat_until`.
//!
//! Control flow is a recursive interpreter over a step list. Leaf steps are
//! executed through a [`LeafRunner`] (implemented by [`RunSession`] — the same
//! policy gate, secret resolution, redaction, and audit as every other step, so
//! each loop iteration is independently gated and recorded). Control steps carry
//! their body as sub-steps inside their `params`, so the flat `Step`/plan schema
//! is unchanged.
//!
//! Every loop has a **mandatory iteration cap** (`control.foreach` caps on the
//! item count and an optional `max_iterations`; `control.repeat_until` requires
//! `max_iterations`), and nesting depth is bounded, so a plan can't spin forever.

use std::collections::HashMap;

use operator_core::interpolation::{interpolate_params, resolve_variable};
use operator_core::types::{Step, StepType};
use serde::Deserialize;
use serde_json::Value;

/// Maximum control-flow nesting depth.
const MAX_DEPTH: usize = 16;
/// Hard ceiling on any single loop, even if a larger `max_iterations` is given.
const HARD_ITER_CAP: u64 = 100_000;

/// Run-scoped variable map.
pub type Vars = HashMap<String, Value>;

/// Executes a single leaf (non-control) step and returns its output. The
/// implementor supplies the real gate/dispatch/audit (e.g. [`RunSession`]).
pub trait LeafRunner {
    fn run_leaf(&mut self, step: &Step) -> Result<Value, String>;
}

/// An error from the control-flow interpreter.
#[derive(Debug, thiserror::Error)]
pub enum ControlError {
    #[error("control flow: {0}")]
    Malformed(String),
    #[error("{0}")]
    LeafFailed(String),
    #[error("control nesting exceeded {MAX_DEPTH} levels")]
    TooDeep,
}

// -- body schemas ------------------------------------------------------------

#[derive(Deserialize)]
struct ForeachBody {
    /// The collection to iterate: a literal array, or `"${var}"` naming a
    /// variable that holds an array (or a JSON-array string).
    items: Value,
    /// Loop variable name bound to each element.
    #[serde(rename = "as")]
    as_var: String,
    /// Body executed per element.
    steps: Vec<Step>,
    /// Optional cap on iterations (in addition to the item count).
    #[serde(default)]
    max_iterations: Option<u64>,
}

#[derive(Deserialize)]
struct IfBody {
    condition: Condition,
    #[serde(default)]
    then: Vec<Step>,
    #[serde(default, rename = "else")]
    otherwise: Vec<Step>,
}

#[derive(Deserialize)]
struct RepeatBody {
    steps: Vec<Step>,
    condition: Condition,
    /// Required safety cap.
    max_iterations: u64,
}

/// A comparison over interpolated operands: `left <op> right`.
#[derive(Deserialize, Clone)]
struct Condition {
    left: Value,
    op: String,
    #[serde(default)]
    right: Value,
}

// -- public entry ------------------------------------------------------------

/// Runs a program (a step list that may contain control steps) to completion.
pub fn run_program(
    runner: &mut dyn LeafRunner,
    steps: &[Step],
    vars: &mut Vars,
) -> Result<(), ControlError> {
    run_steps(runner, steps, vars, 0)
}

/// True if any step in the list is a control step (so the caller knows to use
/// the interpreter rather than the flat executor).
pub fn has_control_flow(steps: &[Step]) -> bool {
    steps.iter().any(|s| is_control(&s.step_type))
}

fn is_control(t: &StepType) -> bool {
    matches!(
        t,
        StepType::ControlForeach | StepType::ControlIf | StepType::ControlRepeatUntil
    )
}

// -- interpreter -------------------------------------------------------------

fn run_steps(
    runner: &mut dyn LeafRunner,
    steps: &[Step],
    vars: &mut Vars,
    depth: usize,
) -> Result<(), ControlError> {
    if depth > MAX_DEPTH {
        return Err(ControlError::TooDeep);
    }
    for step in steps {
        run_one(runner, step, vars, depth)?;
    }
    Ok(())
}

fn run_one(
    runner: &mut dyn LeafRunner,
    step: &Step,
    vars: &mut Vars,
    depth: usize,
) -> Result<(), ControlError> {
    match step.step_type {
        StepType::ControlForeach => {
            let body: ForeachBody = parse_body(step)?;
            let items = resolve_collection(&body.items, vars)?;
            let cap = body
                .max_iterations
                .unwrap_or(HARD_ITER_CAP)
                .min(HARD_ITER_CAP);
            for (i, item) in items.into_iter().enumerate() {
                if i as u64 >= cap {
                    tracing::warn!("control.foreach hit iteration cap {cap}");
                    break;
                }
                vars.insert(body.as_var.clone(), item);
                vars.insert("__index".to_string(), Value::from(i as u64));
                run_steps(runner, &body.steps, vars, depth + 1)?;
            }
            Ok(())
        }
        StepType::ControlIf => {
            let body: IfBody = parse_body(step)?;
            let branch = if eval_condition(&body.condition, vars)? {
                &body.then
            } else {
                &body.otherwise
            };
            run_steps(runner, branch, vars, depth + 1)
        }
        StepType::ControlRepeatUntil => {
            let body: RepeatBody = parse_body(step)?;
            let cap = body.max_iterations.min(HARD_ITER_CAP);
            if cap == 0 {
                return Err(ControlError::Malformed(
                    "control.repeat_until requires max_iterations >= 1".to_string(),
                ));
            }
            for i in 0..cap {
                vars.insert("__iteration".to_string(), Value::from(i));
                run_steps(runner, &body.steps, vars, depth + 1)?;
                if eval_condition(&body.condition, vars)? {
                    break;
                }
            }
            Ok(())
        }
        // Leaf: interpolate params with the current vars, run through the gate,
        // and (optionally) bind the output to a variable via `save_as`.
        _ => {
            let interpolated = interpolate_params(&step.params, vars)
                .map_err(|e| ControlError::Malformed(format!("interpolation: {e}")))?;
            let leaf = Step {
                params: interpolated,
                ..step.clone()
            };
            let out = runner.run_leaf(&leaf).map_err(ControlError::LeafFailed)?;
            if let Some(name) = step.params.get("save_as").and_then(|v| v.as_str()) {
                vars.insert(name.to_string(), out);
            }
            Ok(())
        }
    }
}

fn parse_body<T: for<'de> Deserialize<'de>>(step: &Step) -> Result<T, ControlError> {
    serde_json::from_value(step.params.clone())
        .map_err(|e| ControlError::Malformed(format!("{} params: {e}", step.step_type.as_str())))
}

/// Resolves a `foreach.items` value to a concrete array. A string of exactly
/// `${name}` (or `$name`) is looked up in `vars` (preserving its type — arrays
/// stay arrays); a JSON-array string is parsed; a literal array is used as-is.
fn resolve_collection(items: &Value, vars: &Vars) -> Result<Vec<Value>, ControlError> {
    let resolved = match items {
        Value::String(s) => {
            if let Some(name) = single_var_ref(s) {
                resolve_variable(name, vars).unwrap_or(Value::Null)
            } else if let Ok(v) = serde_json::from_str::<Value>(s) {
                v
            } else {
                Value::String(s.clone())
            }
        }
        other => other.clone(),
    };
    match resolved {
        Value::Array(a) => Ok(a),
        // A `{count, rows}` shape (data.parse_csv / get_text_all) → iterate rows/items.
        Value::Object(ref o) => {
            if let Some(Value::Array(a)) = o.get("rows").or_else(|| o.get("items")) {
                Ok(a.clone())
            } else {
                Err(ControlError::Malformed(
                    "foreach.items object has no 'rows'/'items' array".to_string(),
                ))
            }
        }
        Value::Null => Ok(vec![]),
        _ => Err(ControlError::Malformed(
            "foreach.items did not resolve to an array".to_string(),
        )),
    }
}

/// If `s` is exactly a single variable reference — `${name}` (dotted paths
/// allowed) or `$name` — returns the variable name; otherwise `None`. Matches
/// the interpolation engine's `$`/`${}` syntax so types are preserved.
fn single_var_ref(s: &str) -> Option<&str> {
    let t = s.trim();
    if let Some(inner) = t.strip_prefix("${").and_then(|x| x.strip_suffix('}')) {
        if !inner.is_empty() && !inner.contains(['$', '{', '}']) {
            return Some(inner);
        }
        return None;
    }
    if let Some(name) = t.strip_prefix('$') {
        if !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
        {
            return Some(name);
        }
    }
    None
}

/// Evaluates a condition. Operands are resolved against `vars`
/// (`${var}`-aware), then compared. Numeric comparisons apply when both sides
/// parse as numbers; otherwise string comparison is used.
fn eval_condition(cond: &Condition, vars: &Vars) -> Result<bool, ControlError> {
    let left = resolve_operand(&cond.left, vars);
    let right = resolve_operand(&cond.right, vars);

    let result = match cond.op.as_str() {
        "exists" | "truthy" => is_truthy(&left),
        "empty" => !is_truthy(&left),
        "==" | "eq" => values_equal(&left, &right),
        "!=" | "ne" => !values_equal(&left, &right),
        "contains" => left
            .as_str()
            .zip(right.as_str())
            .map(|(l, r)| l.contains(r))
            .unwrap_or(false),
        ">" | "<" | ">=" | "<=" | "gt" | "lt" | "gte" | "lte" => {
            match (as_number(&left), as_number(&right)) {
                (Some(l), Some(r)) => match cond.op.as_str() {
                    ">" | "gt" => l > r,
                    "<" | "lt" => l < r,
                    ">=" | "gte" => l >= r,
                    "<=" | "lte" => l <= r,
                    _ => unreachable!(),
                },
                _ => {
                    return Err(ControlError::Malformed(format!(
                        "numeric op '{}' needs numeric operands",
                        cond.op
                    )))
                }
            }
        }
        other => return Err(ControlError::Malformed(format!("unknown op '{other}'"))),
    };
    Ok(result)
}

/// Resolves a condition operand: a single `${var}`/`$var` → the variable's
/// value (type preserved); any other string is interpolated; non-strings pass
/// through.
fn resolve_operand(v: &Value, vars: &Vars) -> Value {
    match v {
        Value::String(s) => {
            if let Some(name) = single_var_ref(s) {
                resolve_variable(name, vars).unwrap_or(Value::Null)
            } else {
                let params = serde_json::json!({ "v": s });
                interpolate_params(&params, vars)
                    .ok()
                    .and_then(|p| p.get("v").cloned())
                    .unwrap_or_else(|| v.clone())
            }
        }
        other => other.clone(),
    }
}

fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Value::String(s) => !s.is_empty() && s != "false" && s != "0",
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn values_equal(a: &Value, b: &Value) -> bool {
    if let (Some(x), Some(y)) = (as_number(a), as_number(b)) {
        return x == y;
    }
    // Compare as strings so 3 == "3".
    operand_to_string(a) == operand_to_string(b)
}

fn as_number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn operand_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Records the (type, params) of each leaf step it runs.
    struct RecordingRunner {
        seen: Vec<(String, Value)>,
    }
    impl LeafRunner for RecordingRunner {
        fn run_leaf(&mut self, step: &Step) -> Result<Value, String> {
            self.seen
                .push((step.step_type.as_str().to_string(), step.params.clone()));
            Ok(json!({ "ok": true }))
        }
    }

    fn leaf(id: &str, ty: &str, params: Value) -> Step {
        Step {
            id: id.to_string(),
            step_type: ty.parse().unwrap(),
            params,
            timeout_ms: None,
            retries: None,
            retry_backoff_ms: None,
            on_fail: None,
        }
    }

    #[test]
    fn foreach_iterates_and_binds_loop_var() {
        let foreach = leaf(
            "loop",
            "control.foreach",
            json!({
                "items": ["a", "b", "c"],
                "as": "item",
                "steps": [ { "id": "w", "type": "sys.clipboard_set", "params": { "text": "${item}" } } ]
            }),
        );
        let mut r = RecordingRunner { seen: vec![] };
        let mut vars = Vars::new();
        run_program(&mut r, &[foreach], &mut vars).unwrap();
        let texts: Vec<String> = r
            .seen
            .iter()
            .map(|(_, p)| p["text"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(texts, vec!["a", "b", "c"]);
    }

    #[test]
    fn foreach_over_var_array_and_object_rows() {
        // items = "${rows}" where rows is the {count, rows:[...]} shape.
        let foreach = leaf(
            "loop",
            "control.foreach",
            json!({
                "items": "${data}",
                "as": "row",
                "steps": [ { "id": "w", "type": "sys.clipboard_set", "params": { "text": "${row}" } } ]
            }),
        );
        let mut r = RecordingRunner { seen: vec![] };
        let mut vars = Vars::new();
        vars.insert("data".into(), json!({ "count": 2, "rows": ["x", "y"] }));
        run_program(&mut r, &[foreach], &mut vars).unwrap();
        assert_eq!(r.seen.len(), 2);
        assert_eq!(r.seen[1].1["text"], "y");
    }

    #[test]
    fn if_runs_correct_branch() {
        let if_step = leaf(
            "cond",
            "control.if",
            json!({
                "condition": { "left": "${n}", "op": ">=", "right": 5 },
                "then": [ { "id": "t", "type": "sys.clipboard_set", "params": { "text": "big" } } ],
                "else": [ { "id": "e", "type": "sys.clipboard_set", "params": { "text": "small" } } ]
            }),
        );
        let mut r = RecordingRunner { seen: vec![] };
        let mut vars = Vars::new();
        vars.insert("n".into(), json!(7));
        run_program(&mut r, &[if_step], &mut vars).unwrap();
        assert_eq!(r.seen.len(), 1);
        assert_eq!(r.seen[0].1["text"], "big");
    }

    #[test]
    fn repeat_until_respects_cap_and_condition() {
        // Never-satisfied condition → stops at max_iterations.
        let rep = leaf(
            "rep",
            "control.repeat_until",
            json!({
                "steps": [ { "id": "s", "type": "sys.clipboard_set", "params": { "text": "x" } } ],
                "condition": { "left": "false", "op": "truthy" },
                "max_iterations": 3
            }),
        );
        let mut r = RecordingRunner { seen: vec![] };
        let mut vars = Vars::new();
        run_program(&mut r, &[rep], &mut vars).unwrap();
        assert_eq!(r.seen.len(), 3);
    }

    #[test]
    fn condition_ops() {
        let vars = Vars::new();
        let c = |left: Value, op: &str, right: Value| Condition {
            left,
            op: op.to_string(),
            right,
        };
        assert!(eval_condition(&c(json!("3"), "==", json!(3)), &vars).unwrap());
        assert!(eval_condition(&c(json!(10), ">", json!(2)), &vars).unwrap());
        assert!(
            eval_condition(&c(json!("hello world"), "contains", json!("world")), &vars).unwrap()
        );
        assert!(eval_condition(&c(json!(""), "empty", json!(null)), &vars).unwrap());
        assert!(!eval_condition(&c(json!("x"), "empty", json!(null)), &vars).unwrap());
    }

    #[test]
    fn has_control_flow_detects_control_steps() {
        let flat = vec![leaf("a", "sys.clipboard_get", json!({}))];
        assert!(!has_control_flow(&flat));
        let ctrl = vec![leaf(
            "l",
            "control.foreach",
            json!({"items":[],"as":"x","steps":[]}),
        )];
        assert!(has_control_flow(&ctrl));
    }
}
