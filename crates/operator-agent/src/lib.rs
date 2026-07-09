//! The native agent loop (M10): observe → propose one gated step → execute →
//! re-observe, until the model declares done or a budget is hit. M11 adds
//! long-horizon supervision: the loop pauses and checkpoints at an escalation
//! or a `ui.human_takeover`, can be steered mid-run, and compacts a long
//! transcript so runs survive past the context window.
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
    /// Suspended awaiting out-of-band approval or human takeover (M11). A
    /// checkpoint was persisted; the run resumes with `agent --resume`.
    Paused,
}

/// The outcome of an agent run.
pub struct AgentOutcome {
    pub status: AgentStatus,
    pub summary: String,
    pub iterations: usize,
    /// The transcript at the point the loop returned — persisted on pause so a
    /// resume can re-seed the model's context.
    pub transcript: Vec<String>,
}

/// Why the loop paused, so the supervisor can label the approval request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseKind {
    /// The always-escalate classifier flagged a high-consequence action.
    Escalation,
    /// A `ui.human_takeover` step (login/CAPTCHA).
    Takeover,
}

/// A snapshot of agent progress, handed to the supervisor to persist on pause.
pub struct AgentCheckpoint<'a> {
    pub goal: &'a str,
    pub transcript: &'a [String],
    pub iteration: usize,
    pub reason: &'a str,
}

/// The host's supervision hooks: persistence (checkpoint + approval), live
/// steering, and user notification. Kept as a trait so the agent crate stays
/// store-agnostic; the CLI supplies a store-backed implementation.
pub trait AgentSupervisor {
    /// Persist a pause checkpoint and open a pending approval; return the
    /// approval id (for messaging the user).
    fn pause(&mut self, run_id: &str, kind: PauseKind, cp: &AgentCheckpoint) -> String;
    /// Drain any steering messages injected by `operator-jack steer <run-id>`.
    fn poll_steer(&mut self, run_id: &str) -> Vec<String>;
    /// Notify the user out-of-band (e.g. a macOS notification).
    fn notify(&self, _title: &str, _body: &str) {}
}

/// A supervisor that persists nothing and never steers — used by tests and by
/// callers that don't need long-horizon features.
pub struct NoopSupervisor;

impl AgentSupervisor for NoopSupervisor {
    fn pause(&mut self, _run_id: &str, _kind: PauseKind, _cp: &AgentCheckpoint) -> String {
        String::new()
    }
    fn poll_steer(&mut self, _run_id: &str) -> Vec<String> {
        Vec::new()
    }
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
/// actions like `done` and `human_takeover`.
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
- human_takeover  {"prompt": "log in, then I'll continue"}     hand control to the human for logins/CAPTCHAs, then resume

Rules:
- Take snapshot first to discover element refs, THEN act on a ref. Refs are only valid until the next snapshot.
- If an action reports the element is ambiguous, choose a specific ref or add an index and try again.
- If an action is denied by policy, do not retry it — adapt or finish.
- For logins, paywalls, or CAPTCHAs, use human_takeover — never type credentials or solve CAPTCHAs yourself.
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

/// Rough token estimate (~4 chars/token) of a transcript's rendered size.
fn transcript_chars(transcript: &[String]) -> usize {
    transcript.iter().map(|l| l.len() + 1).sum()
}

/// Compacts a transcript that has grown past `max_chars` so long runs survive
/// the context window: keeps the most recent `keep_recent` turns verbatim and
/// replaces everything older with a single summary line noting how many turns
/// and which actions were folded away. Idempotent once under budget. Returns
/// true if it compacted.
fn compact_transcript(transcript: &mut Vec<String>, max_chars: usize, keep_recent: usize) -> bool {
    if transcript_chars(transcript) <= max_chars || transcript.len() <= keep_recent + 1 {
        return false;
    }
    let split = transcript.len() - keep_recent;
    let older = &transcript[..split];
    // Pull the action names out of the folded turns for a breadcrumb.
    let mut actions: Vec<String> = older
        .iter()
        .filter_map(|line| {
            line.strip_prefix("Action: ")
                .map(|rest| rest.split_whitespace().next().unwrap_or("?").to_string())
        })
        .collect();
    let folded = actions.len();
    actions.dedup();
    let preview: Vec<String> = actions.into_iter().take(12).collect();
    let summary = format!(
        "[... {} earlier turns compacted to save context; actions taken: {} ...]",
        older.len().max(folded),
        preview.join(", ")
    );
    let mut compacted = Vec::with_capacity(keep_recent + 1);
    compacted.push(summary);
    compacted.extend_from_slice(&transcript[split..]);
    *transcript = compacted;
    true
}

/// Transcript compaction threshold (chars). ~24k chars ≈ 6k tokens of history.
const COMPACT_MAX_CHARS: usize = 24_000;
/// Turns kept verbatim after a compaction.
const COMPACT_KEEP_RECENT: usize = 8;

/// Runs the agent loop with a no-op supervisor (no pause/steer/notify). Kept for
/// simple callers and tests.
pub fn run_agent(
    goal: &str,
    provider: &dyn LlmProvider,
    session: &mut RunSession,
    max_iters: usize,
) -> AgentOutcome {
    let mut sup = NoopSupervisor;
    run_agent_supervised(goal, provider, session, max_iters, &mut sup, Vec::new())
}

/// Runs the agent loop against `session`, using `provider` as the brain, for at
/// most `max_iters` turns, under `supervisor` (pause/steer/notify). Pass a
/// non-empty `initial_transcript` to resume a paused run.
pub fn run_agent_supervised(
    goal: &str,
    provider: &dyn LlmProvider,
    session: &mut RunSession,
    max_iters: usize,
    supervisor: &mut dyn AgentSupervisor,
    initial_transcript: Vec<String>,
) -> AgentOutcome {
    let system = system_prompt();
    let run_id = session.run_id().to_string();
    let resuming = !initial_transcript.is_empty();
    let mut transcript: Vec<String> = initial_transcript;

    for iter in 0..max_iters {
        // Fold in any live steering before deciding the next action.
        for msg in supervisor.poll_steer(&run_id) {
            tracing::info!(%run_id, "steer injected: {msg}");
            transcript.push(format!("USER STEER: {msg}"));
        }
        // Keep the transcript within the context budget.
        if compact_transcript(&mut transcript, COMPACT_MAX_CHARS, COMPACT_KEEP_RECENT) {
            tracing::info!(%run_id, "compacted agent transcript");
        }

        // Build the user prompt: goal + transcript.
        let mut user = format!("GOAL: {goal}\n\n");
        if transcript.is_empty() {
            user.push_str("No actions taken yet. Decide your first action.\n");
        } else {
            if resuming && iter == 0 {
                user.push_str(
                    "You are RESUMING a paused run after a human approved/completed the pending step. Continue toward the goal.\n\n",
                );
            }
            user.push_str("Transcript so far:\n");
            for line in &transcript {
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
                    transcript,
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
                transcript,
            };
        }

        // human_takeover is a control action: pause for the human, don't dispatch.
        if action.action == "human_takeover" {
            let reason = action
                .args
                .get("prompt")
                .and_then(|v| v.as_str())
                .or(action.summary.as_deref())
                .unwrap_or("Human takeover requested (login/CAPTCHA).")
                .to_string();
            transcript.push(format!("Action: human_takeover {}\n=> PAUSED", action.args));
            return pause(
                supervisor,
                &run_id,
                PauseKind::Takeover,
                goal,
                &transcript,
                iter + 1,
                &reason,
            );
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
            // Escalation / takeover are pause points: checkpoint and stop.
            Err(SessionError::Escalation(reason)) => {
                transcript.push(format!(
                    "Action: {} {}\n=> ESCALATION: {}",
                    action.action, args, reason
                ));
                return pause(
                    supervisor,
                    &run_id,
                    PauseKind::Escalation,
                    goal,
                    &transcript,
                    iter + 1,
                    &reason,
                );
            }
            Err(SessionError::Takeover(reason)) => {
                transcript.push(format!("Action: {} {}\n=> PAUSED", action.action, args));
                return pause(
                    supervisor,
                    &run_id,
                    PauseKind::Takeover,
                    goal,
                    &transcript,
                    iter + 1,
                    &reason,
                );
            }
        };
        transcript.push(format!("Action: {} {}\n=> {}", action.action, args, result));
    }

    AgentOutcome {
        status: AgentStatus::MaxIterations,
        summary: format!("reached the {max_iters}-iteration limit without finishing"),
        iterations: max_iters,
        transcript,
    }
}

/// Persists a pause checkpoint + approval via the supervisor, notifies the user,
/// and returns a `Paused` outcome.
#[allow(clippy::too_many_arguments)]
fn pause(
    supervisor: &mut dyn AgentSupervisor,
    run_id: &str,
    kind: PauseKind,
    goal: &str,
    transcript: &[String],
    iteration: usize,
    reason: &str,
) -> AgentOutcome {
    let cp = AgentCheckpoint {
        goal,
        transcript,
        iteration,
        reason,
    };
    let approval_id = supervisor.pause(run_id, kind, &cp);
    let (title, verb) = match kind {
        PauseKind::Escalation => ("Operator Jack: approval needed", "needs approval for"),
        PauseKind::Takeover => ("Operator Jack: your turn", "needs you to handle"),
    };
    supervisor.notify(title, &format!("Run {verb}: {reason}"));
    let summary = if approval_id.is_empty() {
        format!("paused ({reason})")
    } else {
        format!(
            "paused ({reason}). Approve with: operator-jack approvals approve {approval_id}, then: operator-jack agent --resume {run_id}"
        )
    };
    AgentOutcome {
        status: AgentStatus::Paused,
        summary,
        iterations: iteration,
        transcript: transcript.to_vec(),
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
        assert_eq!(action_step_type("human_takeover"), None);
        assert_eq!(action_step_type("nonsense"), None);
    }

    #[test]
    fn compaction_folds_old_turns_and_keeps_recent() {
        let mut t: Vec<String> = (0..50)
            .map(|_i| format!("Action: snapshot {{\"app\":\"A\"}}\n=> {}", "x".repeat(600)))
            .collect();
        // Force distinct action names so the breadcrumb has variety.
        t[0] = format!("Action: open_app {{}}\n=> {}", "y".repeat(600));
        let before = t.len();
        let did = compact_transcript(&mut t, COMPACT_MAX_CHARS, COMPACT_KEEP_RECENT);
        assert!(did, "should compact an over-budget transcript");
        assert!(t.len() < before);
        assert_eq!(t.len(), COMPACT_KEEP_RECENT + 1);
        assert!(t[0].contains("compacted"));
        // The most recent turns are preserved verbatim.
        assert_eq!(
            t.last(),
            Some(&format!(
                "Action: snapshot {{\"app\":\"A\"}}\n=> {}",
                "x".repeat(600)
            ))
        );
    }

    #[test]
    fn compaction_noop_when_under_budget() {
        let mut t = vec!["Action: snapshot {}\n=> ok".to_string(); 3];
        let did = compact_transcript(&mut t, COMPACT_MAX_CHARS, COMPACT_KEEP_RECENT);
        assert!(!did);
        assert_eq!(t.len(), 3);
    }
}
