//! The native agent loop (M10): observe → propose one gated step → execute →
//! re-observe, until the model declares done or a budget is hit.
//!
//! Each turn the model is shown the goal and the running transcript (including
//! prior `snapshot` output) and asked for exactly ONE next action as JSON. The
//! action is executed through a [`RunSession`] — the same policy gate, secret
//! resolution, redaction, and audit as everything else — and its result (or an
//! `ELEMENT_AMBIGUOUS` candidate list) is appended for the model to react to.

use operator_core::types::{Step, StepType};
use operator_planner::provider::LlmProvider;
use operator_runtime::engine::{RunSession, SessionError};
use serde::Deserialize;
use serde_json::{json, Value};

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentStatus {
    /// The model declared the task complete.
    Done,
    /// The iteration budget was exhausted before completion.
    MaxIterations,
    /// The loop could not continue (e.g. the model stopped producing actions).
    Aborted,
}

/// The outcome of an agent run.
pub struct AgentOutcome {
    pub status: AgentStatus,
    pub summary: String,
    pub iterations: usize,
}

/// One action parsed from the model's response.
#[derive(Debug, Deserialize)]
struct Action {
    action: String,
    #[serde(default)]
    args: Value,
    #[serde(default)]
    summary: Option<String>,
}

/// Maps an agent action name to the step type it dispatches. `None` for control
/// actions like `done`.
fn action_step_type(name: &str) -> Option<StepType> {
    Some(match name {
        "snapshot" => StepType::UiSnapshot,
        "screenshot" => StepType::UiScreenshot,
        "click" => StepType::UiClick,
        "type_text" => StepType::UiTypeText,
        "key_press" => StepType::UiKeyPress,
        "set_value" => StepType::UiSetValue,
        "select_menu" => StepType::UiSelectMenu,
        "read_text" => StepType::UiReadText,
        "wait_for" => StepType::UiWaitFor,
        "list_apps" => StepType::UiListApps,
        "focus_app" => StepType::UiFocusApp,
        "open_app" => StepType::SysOpenApp,
        "read_file" => StepType::SysReadFile,
        "write_file" => StepType::SysWriteFile,
        "exec" => StepType::SysExec,
        _ => return None,
    })
}

fn system_prompt() -> String {
    r#"You are Operator Jack, an autonomous macOS agent. You accomplish the user's goal by taking ONE action at a time and observing the result.

Respond with EXACTLY ONE JSON object and nothing else:
  {"thought": "<brief reasoning>", "action": "<name>", "args": { ... }}
or, when the goal is complete:
  {"action": "done", "summary": "<what you accomplished>"}

Available actions (args mirror the step's params):
- open_app        {"app": "Calculator"}                       launch/activate an app
- focus_app       {"app": "Notes"}                            bring an app to the front
- snapshot        {"app": "Calculator"}                       see the UI: a compact tree with element refs [e1], [e2]…  ALWAYS snapshot before clicking/typing
- click           {"app": "Calculator", "element_ref": "e5"}  click an element by its ref (or a selector)
- type_text       {"app": "Notes", "text": "hello"}           type into the focused element
- key_press       {"app": "Notes", "key": "Return"}           press a key (optional "modifiers": ["command"])
- set_value       {"app": "X", "element_ref": "e2", "value": "10"}
- select_menu     {"app": "Notes", "menu_path": ["File", "New Note"]}
- read_text       {"app": "Calculator", "element_ref": "e1"}  read an element's text/value
- wait_for        {"app": "X", "selector": {"role": "AXWindow"}, "timeout_ms": 5000}
- read_file       {"path": "~/x.txt"} / write_file {"path": "~/x.txt", "content": "..."} / exec {"command": "ls", "args": ["-la"]}

Rules:
- Take snapshot first to discover element refs, THEN act on a ref. Refs are only valid until the next snapshot.
- If an action reports the element is ambiguous, choose a specific ref or add an index and try again.
- If an action is denied by policy, do not retry it — adapt or finish.
- Never put secrets in plain text; reference them as {{secret:name}}.
- Emit only the single JSON object, no prose around it."#
        .to_string()
}

/// Extracts the first balanced-ish JSON object from a model response (first `{`
/// to last `}`), mirroring the planner's tolerant extraction.
fn extract_json(text: &str) -> &str {
    match (text.find('{'), text.rfind('}')) {
        (Some(s), Some(e)) if e >= s => &text[s..=e],
        _ => text.trim(),
    }
}

/// Runs the agent loop against `session`, using `provider` as the brain, for at
/// most `max_iters` turns.
pub fn run_agent(
    goal: &str,
    provider: &dyn LlmProvider,
    session: &mut RunSession,
    max_iters: usize,
) -> AgentOutcome {
    let system = system_prompt();
    let mut transcript: Vec<String> = Vec::new();

    for iter in 0..max_iters {
        // Build the user prompt: goal + recent transcript.
        let mut user = format!("GOAL: {goal}\n\n");
        if transcript.is_empty() {
            user.push_str("No actions taken yet. Decide your first action.\n");
        } else {
            user.push_str("Transcript so far:\n");
            // Keep the last ~12 turns to bound context.
            let start = transcript.len().saturating_sub(12);
            for line in &transcript[start..] {
                user.push_str(line);
                user.push('\n');
            }
        }
        user.push_str("\nYour next action (one JSON object):");

        let response = match provider.chat(&system, &user) {
            Ok(r) => r,
            Err(e) => {
                return AgentOutcome {
                    status: AgentStatus::Aborted,
                    summary: format!("provider error: {e}"),
                    iterations: iter,
                };
            }
        };

        let action: Action = match serde_json::from_str(extract_json(&response)) {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!("could not parse action: {e}; response: {response}");
                transcript.push(format!(
                    "Action: <unparseable>\n=> ERROR: your reply was not a single JSON action object ({e}). Reply with exactly one JSON object."
                ));
                continue;
            }
        };

        tracing::info!(iter, action = %action.action, "agent action");

        if action.action == "done" {
            return AgentOutcome {
                status: AgentStatus::Done,
                summary: action.summary.unwrap_or_else(|| "done".to_string()),
                iterations: iter + 1,
            };
        }

        let Some(step_type) = action_step_type(&action.action) else {
            transcript.push(format!(
                "Action: {}\n=> ERROR: unknown action '{}'. Use one of the listed actions or \"done\".",
                action.action, action.action
            ));
            continue;
        };

        let args = if action.args.is_object() {
            action.args.clone()
        } else {
            json!({})
        };
        let step = Step {
            id: ulid::Ulid::new().to_string(),
            step_type,
            params: args.clone(),
            timeout_ms: None,
            retries: None,
            retry_backoff_ms: None,
            on_fail: None,
        };

        let result = match session.execute_step(&step) {
            Ok(output) => summarize_output(&action.action, &output),
            Err(SessionError::Denied(m)) => format!("DENIED: {m}"),
            Err(SessionError::Failed(m)) => format!("ERROR: {m}"),
        };
        transcript.push(format!("Action: {} {}\n=> {}", action.action, args, result));
    }

    AgentOutcome {
        status: AgentStatus::MaxIterations,
        summary: format!("reached the {max_iters}-iteration limit without finishing"),
        iterations: max_iters,
    }
}

/// Renders a step output for the transcript. For snapshots, return the snapshot
/// text (the model needs to see refs); for others, a compact summary.
fn summarize_output(action: &str, output: &Value) -> String {
    if action == "snapshot" {
        if let Some(s) = output.get("snapshot").and_then(|v| v.as_str()) {
            return format!(
                "snapshot ({} refs):\n{}",
                output
                    .get("ref_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                s
            );
        }
    }
    if let Some(text) = output.get("text").and_then(|v| v.as_str()) {
        return format!("text: {text}");
    }
    // Compact: drop verbose _evidence to keep context small.
    let mut trimmed = output.clone();
    if let Some(obj) = trimmed.as_object_mut() {
        obj.remove("_evidence");
    }
    serde_json::to_string(&trimmed).unwrap_or_else(|_| "ok".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_action_from_fenced_or_prose_response() {
        // Model wraps its JSON in prose / fences — extract and parse it.
        let resp = "Sure! Here's my next step:\n```json\n{\"thought\":\"look first\",\"action\":\"snapshot\",\"args\":{\"app\":\"Calculator\"}}\n```";
        let a: Action = serde_json::from_str(extract_json(resp)).unwrap();
        assert_eq!(a.action, "snapshot");
        assert_eq!(a.args["app"], "Calculator");
    }

    #[test]
    fn parses_done_action() {
        let a: Action = serde_json::from_str(extract_json(
            "{\"action\":\"done\",\"summary\":\"computed 3\"}",
        ))
        .unwrap();
        assert_eq!(a.action, "done");
        assert_eq!(a.summary.as_deref(), Some("computed 3"));
    }

    #[test]
    fn action_names_map_to_step_types() {
        assert_eq!(action_step_type("snapshot"), Some(StepType::UiSnapshot));
        assert_eq!(action_step_type("click"), Some(StepType::UiClick));
        assert_eq!(action_step_type("exec"), Some(StepType::SysExec));
        assert_eq!(action_step_type("done"), None);
        assert_eq!(action_step_type("nonsense"), None);
    }
}
