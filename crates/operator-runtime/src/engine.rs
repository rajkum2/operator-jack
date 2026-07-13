use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use chrono::Utc;
use serde_json::json;

use operator_core::event::{Event, EventType};
use operator_core::interpolation::interpolate_params;
use operator_core::types::*;
use operator_exec_system::executor::execute_system_step_with_timeout;
use operator_ipc::client::HelperClient;
use operator_store::Store;

use crate::logging::RunLogger;
use crate::policy::{PolicyDecision, PolicyError, PolicyGate};
use crate::RuntimeError;

// ---------------------------------------------------------------------------
// EngineConfig
// ---------------------------------------------------------------------------

/// Configuration for the execution engine, typically populated from CLI flags
/// and environment variables.
pub struct EngineConfig {
    /// Execution mode (safe / unsafe).
    pub mode: Mode,
    /// If true, automatically approve all confirmation prompts.
    pub yes_to_all: bool,
    /// Whether the session has an interactive terminal.
    pub interactive: bool,
    /// If true, steps are logged but not actually executed.
    pub dry_run: bool,
    /// App allow-list from CLI flags.
    pub allow_apps: Vec<String>,
    /// Domain allow-list from CLI flags.
    pub allow_domains: Vec<String>,
    /// Directory for JSONL run logs.
    pub log_dir: std::path::PathBuf,
    /// Optional path to the macOS helper binary.
    pub helper_path: Option<String>,
    /// Default step timeout in milliseconds.
    pub default_timeout_ms: u64,
    /// Default retry count for failed steps.
    pub default_retries: u32,
    /// Default initial backoff in milliseconds between retries.
    pub default_backoff_ms: u64,
    /// Config-level task-scoped grants (M8). Intersected with the plan's.
    pub grants: Option<operator_core::grants::Grants>,
    /// Config-level run budget (M8). Intersected with the plan's (min wins).
    pub budget: Option<operator_core::grants::RunBudget>,
    /// Root dir for the undo journal (M15). `None` disables copy-before-mutate.
    /// When set, mutating `sys.*` steps back up prior state under
    /// `<undo_dir>/<run_id>/` so `operator-jack undo <run-id>` can reverse them.
    pub undo_dir: Option<std::path::PathBuf>,
}

// ---------------------------------------------------------------------------
// RunSummary
// ---------------------------------------------------------------------------

/// A summary of a completed (or aborted) run, returned by `Engine::execute_plan`.
pub struct RunSummary {
    pub run_id: String,
    pub plan_id: String,
    pub status: RunStatus,
    pub steps_total: usize,
    pub steps_succeeded: usize,
    pub steps_failed: usize,
    pub steps_skipped: usize,
    pub duration_ms: u64,
    /// True if a step was denied by the policy gate (allow-list, grant, or
    /// escalation). Drives the CLI's policy-denied exit code (3).
    pub policy_denied: bool,
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

/// The main plan execution engine. Drives each step through the policy gate,
/// dispatches to the appropriate executor (system or UI/IPC), records results
/// in the store, and writes JSONL event logs.
pub struct Engine {
    store: Store,
    config: EngineConfig,
    cancel_flag: Arc<AtomicBool>,
    /// Optional callback invoked with the run_id immediately after the run
    /// record is created (before steps begin). Used by the CLI to update the
    /// PID file with the correct run_id.
    #[allow(clippy::type_complexity)]
    on_run_created: Option<Box<dyn FnOnce(&str)>>,
    /// One-shot resume state (M11): consumed at the start of the next
    /// `execute_plan` to skip completed steps and grant an approved escalation.
    pending_resume: Option<ResumeState>,
}

/// State carried into a resumed plan run (M11).
#[derive(Debug, Clone)]
pub struct ResumeState {
    /// Steps with index `< cursor` are skipped (already ran in the prior run).
    pub cursor: usize,
    /// Variable map restored from the checkpoint.
    pub variables: HashMap<String, serde_json::Value>,
    /// Allow the first always-escalate action through once (approved out-of-band).
    pub grant_escalation: bool,
}

impl Engine {
    /// Creates a new engine with the given store and configuration.
    pub fn new(store: Store, config: EngineConfig) -> Self {
        Self {
            store,
            config,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            on_run_created: None,
            pending_resume: None,
        }
    }

    /// Arms a one-shot resume for the next `execute_plan` call (M11): the run
    /// skips steps before `cursor`, restores `variables`, and — if
    /// `grant_escalation` — lets the first always-escalate action through once.
    pub fn resume_next(&mut self, state: ResumeState) {
        self.pending_resume = Some(state);
    }

    /// Sets a callback that will be invoked with the run_id immediately after
    /// the run record is created (before step execution begins).
    pub fn set_on_run_created<F: FnOnce(&str) + 'static>(&mut self, f: F) {
        self.on_run_created = Some(Box::new(f));
    }

    /// Returns a clone of the cancellation flag. Signal handlers (e.g.
    /// Ctrl-C) should set this to `true` to request a graceful stop.
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancel_flag)
    }

    /// Executes a previously-saved plan identified by `plan_id`.
    ///
    /// This is the main entry point for running a plan. It:
    /// 1. Loads the plan from the store.
    /// 2. Creates a run record.
    /// 3. Iterates over every step, applying the policy gate and dispatching
    ///    execution.
    /// 4. Records step results and events.
    /// 5. Returns a `RunSummary`.
    pub fn execute_plan(&mut self, plan_id: &str) -> Result<RunSummary, RuntimeError> {
        let wall_start = Instant::now();

        // -- 1. Load plan -------------------------------------------------------
        let (_id, plan) = self.store.get_plan(plan_id)?;

        // One-shot resume state (M11): skip completed steps, restore variables,
        // and optionally allow one approved escalation.
        let resume = self.pending_resume.take();
        let resume_cursor = resume.as_ref().map(|r| r.cursor).unwrap_or(0);

        // -- 2. Determine effective mode ----------------------------------------
        // The configured (CLI/config) mode is a CEILING: a plan may make itself
        // MORE restrictive (safe) but must never ESCALATE to unsafe over a safe
        // config. This stops an untrusted or LLM-generated plan from disabling
        // the user's confirmations by declaring `mode: unsafe`.
        let effective_mode = match (&self.config.mode, &plan.mode) {
            (Mode::Safe, _) => Mode::Safe,
            (_, Some(Mode::Safe)) => Mode::Safe,
            (config_mode, _) => config_mode.clone(),
        };

        // -- 3. Create run (queued) ---------------------------------------------
        let run_id = self.store.create_run(plan_id, &effective_mode)?;
        tracing::info!(run_id = %run_id, plan_id = %plan_id, "created run");

        // Notify caller of the run_id (e.g. to write PID file with correct id).
        if let Some(cb) = self.on_run_created.take() {
            cb(&run_id);
        }

        // -- 4. Create RunLogger ------------------------------------------------
        let mut logger = RunLogger::new(&self.config.log_dir, &run_id)
            .map_err(|e| RuntimeError::Other(format!("Failed to create run logger: {}", e)))?;

        // -- 5. Build PolicyGate ------------------------------------------------
        // Intersect plan-level and CLI-level allow-lists.
        let allow_apps = intersect_lists(&self.config.allow_apps, &plan.allow_apps);
        let allow_domains = intersect_lists(&self.config.allow_domains, &plan.allow_domains);
        // Domains for the browser lane's own URL allow-list (belt & suspenders
        // with the policy gate); cloned before allow_domains is moved below.
        let browser_domains = allow_domains.clone();

        // Grant layers: config + plan. Each present layer must allow a step
        // (most restrictive wins). Absent grants impose no restriction.
        let mut grant_layers: Vec<operator_core::grants::Grants> = Vec::new();
        if let Some(ref g) = self.config.grants {
            grant_layers.push(g.clone());
        }
        if let Some(ref g) = plan.grants {
            grant_layers.push(g.clone());
        }

        let policy = PolicyGate::new(
            effective_mode.clone(),
            self.config.yes_to_all,
            self.config.interactive,
            self.config.dry_run,
            allow_apps,
            allow_domains,
            grant_layers,
        );
        // A resumed run may carry a one-shot approved-escalation allowance.
        if resume.as_ref().map(|r| r.grant_escalation).unwrap_or(false) {
            policy.grant_one_escalation();
        }

        // -- 6. Optionally create HelperClient (lazy connect on first UI step) --
        let mut helper: Option<HelperClient> =
            Some(HelperClient::new(self.config.helper_path.clone()));

        // Browser lane (M12): lazily spawns an isolated Chrome on first
        // browser.* step. Default profile isolation (never the user's Chrome).
        let mut browser = crate::browser_lane::BrowserHandle::new(
            browser_domains,
            crate::browser_lane::lane_options(true, false, None),
        );

        // -- 7. Update run to Running, log run_started --------------------------
        self.store
            .update_run_status(&run_id, &RunStatus::Running, None)?;

        let run_started_event = Event::new(
            &run_id,
            None,
            EventType::RunStarted,
            json!({
                "plan_id": plan_id,
                "mode": serde_json::to_value(&effective_mode).unwrap_or_default(),
                "dry_run": self.config.dry_run,
                "step_count": plan.steps.len(),
            }),
        );
        self.store.insert_event(&run_started_event)?;
        let _ = logger.log_event(&run_started_event);

        // -- 8. Initialise variables from plan.variables (+ restored on resume) -
        let mut variables: HashMap<String, serde_json::Value> =
            plan.variables.clone().unwrap_or_default();
        if let Some(ref r) = resume {
            variables.extend(r.variables.clone());
        }

        // -- 9. Step execution loop ---------------------------------------------
        let total_steps = plan.steps.len();
        let mut steps_succeeded: usize = 0;
        let mut steps_failed: usize = 0;
        let mut steps_skipped: usize = 0;
        let mut aborted = false;
        let mut policy_denied = false;
        let mut budget_exceeded: Option<String> = None;
        let mut bytes_written: u64 = 0;
        // M11: set when the run pauses (escalation / human_takeover) at step idx.
        // (step_index, reason, is_takeover).
        let mut paused: Option<(usize, String, bool)> = None;

        // Effective run budget = most restrictive of config + plan budgets.
        let budget = merge_budgets(self.config.budget.as_ref(), plan.budget.as_ref());

        for (idx, step) in plan.steps.iter().enumerate() {
            // -- 9a-resume. Skip steps already completed in the prior run (M11) --
            if idx < resume_cursor {
                continue;
            }

            // -- 9a0. Budget: step count / wall clock (checked before each step) -
            if let Some(ref b) = budget {
                if let Some(max) = b.max_steps {
                    if idx as u64 >= max {
                        budget_exceeded = Some(format!("max_steps ({max}) reached"));
                    }
                }
                if budget_exceeded.is_none() {
                    if let Some(max) = b.max_wall_ms {
                        if wall_start.elapsed().as_millis() as u64 >= max {
                            budget_exceeded = Some(format!("max_wall_ms ({max}) reached"));
                        }
                    }
                }
                if let Some(ref reason) = budget_exceeded {
                    tracing::warn!(run_id = %run_id, "budget exceeded: {reason}");
                    steps_skipped += total_steps - idx;
                    break;
                }
            }

            // -- 9a. Check cancel flag ------------------------------------------
            if self.cancel_flag.load(Ordering::Relaxed) {
                tracing::warn!(run_id = %run_id, "cancel requested, skipping remaining steps");
                steps_skipped += total_steps - idx;
                // Record skip results for remaining steps
                for skip_idx in idx..total_steps {
                    let skip_step = &plan.steps[skip_idx];
                    let sr = StepResult {
                        id: ulid::Ulid::new().to_string(),
                        run_id: run_id.clone(),
                        step_id: skip_step.id.clone(),
                        step_index: skip_idx as u32,
                        status: StepStatus::Cancelled,
                        attempt: 0,
                        started_at: Utc::now(),
                        ended_at: Some(Utc::now()),
                        input_json: skip_step.params.clone(),
                        output_json: None,
                        error_json: None,
                    };
                    let _ = self.store.insert_step_result(&sr);
                }
                aborted = true;
                break;
            }

            // -- 9b. Interpolate params -----------------------------------------
            let interpolated_params = match interpolate_params(&step.params, &variables) {
                Ok(p) => p,
                Err(e) => {
                    tracing::error!(step_id = %step.id, "interpolation failed: {}", e);
                    let sr = make_failed_step_result(
                        &run_id,
                        &step.id,
                        idx,
                        &step.params,
                        &format!("Interpolation failed: {}", e),
                    );
                    let _ = self.store.insert_step_result(&sr);
                    emit_step_failed(
                        &self.store,
                        &mut logger,
                        &run_id,
                        &step.id,
                        idx,
                        &format!("Interpolation failed: {}", e),
                    );
                    steps_failed += 1;

                    match handle_on_fail(&step.effective_on_fail(), self.config.interactive) {
                        OnFailAction::Abort => {
                            aborted = true;
                            steps_skipped += total_steps - idx - 1;
                            break;
                        }
                        OnFailAction::Continue => continue,
                        OnFailAction::Retry => {
                            // Cannot meaningfully retry an interpolation error,
                            // treat as continue.
                            continue;
                        }
                    }
                }
            };

            // -- 9b-takeover. ui.human_takeover: hand control to the human -------
            if step.step_type == StepType::UiHumanTakeover {
                let prompt = interpolated_params
                    .get("prompt")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Human takeover requested — complete the step, then continue.")
                    .to_string();
                if self.config.interactive {
                    // Block for the human, then treat as a completed step.
                    eprintln!("\n⏸  {prompt}");
                    eprint!("Press Enter when done (or Ctrl-C to abort)… ");
                    let _ = std::io::Write::flush(&mut std::io::stderr());
                    let mut line = String::new();
                    let _ = std::io::stdin().read_line(&mut line);
                    let sr = StepResult {
                        id: ulid::Ulid::new().to_string(),
                        run_id: run_id.clone(),
                        step_id: step.id.clone(),
                        step_index: idx as u32,
                        status: StepStatus::Succeeded,
                        attempt: 1,
                        started_at: Utc::now(),
                        ended_at: Some(Utc::now()),
                        input_json: interpolated_params.clone(),
                        output_json: Some(json!({ "human_takeover": true })),
                        error_json: None,
                    };
                    let _ = self.store.insert_step_result(&sr);
                    steps_succeeded += 1;
                    continue;
                } else {
                    // Non-interactive: pause and checkpoint for out-of-band resume.
                    paused = Some((idx, prompt, true));
                    steps_skipped += total_steps - idx;
                    break;
                }
            }

            // -- 9c. Policy gate ------------------------------------------------
            // Use interpolated params for policy checks so variable-derived
            // app names and URLs are properly validated.
            let policy_decision =
                match policy.check_step_with_params(step, &interpolated_params, idx, total_steps) {
                    Ok(d) => d,
                    // A non-interactive escalation is a pause point (M11): checkpoint
                    // and stop for out-of-band approval instead of hard-failing.
                    Err(PolicyError::EscalationRequired(reason)) if !self.config.interactive => {
                        paused = Some((idx, reason, false));
                        steps_skipped += total_steps - idx;
                        break;
                    }
                    // All other policy denials (app/domain allow-list, grant, or an
                    // interactive escalation) are hard failures handled uniformly
                    // and flag the run for the policy-denied exit code.
                    Err(policy_err) => {
                        policy_denied = true;
                        let msg = policy_err.to_string();
                        tracing::warn!(step_id = %step.id, "{}", msg);
                        let sr = make_failed_step_result(
                            &run_id,
                            &step.id,
                            idx,
                            &interpolated_params,
                            &msg,
                        );
                        let _ = self.store.insert_step_result(&sr);
                        emit_step_failed(&self.store, &mut logger, &run_id, &step.id, idx, &msg);
                        steps_failed += 1;

                        // Respect on_fail instead of unconditional abort.
                        match handle_on_fail(&step.effective_on_fail(), self.config.interactive) {
                            OnFailAction::Abort => {
                                aborted = true;
                                steps_skipped += total_steps - idx - 1;
                                break;
                            }
                            OnFailAction::Continue => continue,
                            OnFailAction::Retry => continue, // cannot retry a policy error
                        }
                    }
                };

            match policy_decision {
                PolicyDecision::DryRun => {
                    tracing::info!(
                        step_id = %step.id,
                        step_type = %step.step_type,
                        "[DRY-RUN] would execute step {}/{}",
                        idx + 1,
                        total_steps,
                    );
                    let sr = StepResult {
                        id: ulid::Ulid::new().to_string(),
                        run_id: run_id.clone(),
                        step_id: step.id.clone(),
                        step_index: idx as u32,
                        status: StepStatus::Skipped,
                        attempt: 0,
                        started_at: Utc::now(),
                        ended_at: Some(Utc::now()),
                        input_json: interpolated_params.clone(),
                        output_json: Some(json!({ "dry_run": true })),
                        error_json: None,
                    };
                    let _ = self.store.insert_step_result(&sr);
                    steps_skipped += 1;
                    continue;
                }
                PolicyDecision::Denied => {
                    tracing::info!(step_id = %step.id, "step denied by policy");
                    let sr = make_failed_step_result(
                        &run_id,
                        &step.id,
                        idx,
                        &interpolated_params,
                        "Step denied by user",
                    );
                    let _ = self.store.insert_step_result(&sr);
                    emit_step_failed(
                        &self.store,
                        &mut logger,
                        &run_id,
                        &step.id,
                        idx,
                        "Step denied by user",
                    );
                    steps_failed += 1;

                    match handle_on_fail(&step.effective_on_fail(), self.config.interactive) {
                        OnFailAction::Abort => {
                            aborted = true;
                            steps_skipped += total_steps - idx - 1;
                            break;
                        }
                        OnFailAction::Continue => continue,
                        OnFailAction::Retry => continue, // re-prompting would loop, just continue
                    }
                }
                PolicyDecision::ConfirmationRequired => {
                    tracing::warn!(
                        step_id = %step.id,
                        "confirmation required but session is not interactive"
                    );
                    let sr = make_failed_step_result(
                        &run_id,
                        &step.id,
                        idx,
                        &interpolated_params,
                        "Confirmation required but session is not interactive",
                    );
                    let _ = self.store.insert_step_result(&sr);
                    emit_step_failed(
                        &self.store,
                        &mut logger,
                        &run_id,
                        &step.id,
                        idx,
                        "Confirmation required but session is not interactive",
                    );
                    steps_failed += 1;

                    match handle_on_fail(&step.effective_on_fail(), self.config.interactive) {
                        OnFailAction::Abort => {
                            aborted = true;
                            steps_skipped += total_steps - idx - 1;
                            break;
                        }
                        OnFailAction::Continue => continue,
                        OnFailAction::Retry => continue,
                    }
                }
                PolicyDecision::Allowed => {
                    // Proceed to execution below.
                }
            }

            // -- 9c2. Resolve {{secret:name}} refs for DISPATCH ONLY. The audited
            //          params (input_json, above) keep the reference form, so the
            //          literal secret never enters the store, logs, or evidence.
            let dispatch_params = if operator_core::secrets::has_secret_refs(&interpolated_params) {
                match operator_core::secrets::resolve_secrets(&interpolated_params) {
                    Ok(p) => p,
                    Err(e) => {
                        let msg = format!("Secret resolution failed: {e}");
                        tracing::warn!(step_id = %step.id, "{}", msg);
                        let sr = make_failed_step_result(
                            &run_id,
                            &step.id,
                            idx,
                            &interpolated_params,
                            &msg,
                        );
                        let _ = self.store.insert_step_result(&sr);
                        emit_step_failed(&self.store, &mut logger, &run_id, &step.id, idx, &msg);
                        steps_failed += 1;
                        match handle_on_fail(&step.effective_on_fail(), self.config.interactive) {
                            OnFailAction::Abort => {
                                aborted = true;
                                steps_skipped += total_steps - idx - 1;
                                break;
                            }
                            OnFailAction::Continue => continue,
                            OnFailAction::Retry => continue,
                        }
                    }
                }
            } else {
                interpolated_params.clone()
            };

            // -- 9d. Execute step (with retries + timeout) -----------------------
            let max_retries = step.effective_retries(self.config.default_retries);
            let backoff_ms = step.effective_backoff_ms(self.config.default_backoff_ms);
            let timeout_ms = step.effective_timeout_ms(self.config.default_timeout_ms);

            let mut last_error: Option<String> = None;
            let mut step_succeeded = false;
            let mut last_error_retryable = true;

            for attempt in 0..=max_retries {
                if attempt > 0 {
                    // P2 fix: check retryability before retrying
                    if !last_error_retryable {
                        tracing::info!(
                            step_id = %step.id,
                            "error is not retryable, skipping remaining retries"
                        );
                        break;
                    }

                    // Apply exponential backoff before retry
                    let sleep_ms = backoff_ms * 2u64.saturating_pow(attempt - 1);
                    tracing::info!(
                        step_id = %step.id,
                        attempt = attempt + 1,
                        backoff_ms = sleep_ms,
                        "retrying step after backoff"
                    );

                    // Emit retry-scheduled event
                    let retry_event = Event::new(
                        &run_id,
                        Some(step.id.clone()),
                        EventType::StepRetryScheduled,
                        json!({
                            "step_index": idx,
                            "attempt": attempt + 1,
                            "backoff_ms": sleep_ms,
                        }),
                    );
                    let _ = self.store.insert_event(&retry_event);
                    let _ = logger.log_event(&retry_event);

                    thread::sleep(Duration::from_millis(sleep_ms));

                    // Check cancel flag between retries
                    if self.cancel_flag.load(Ordering::Relaxed) {
                        last_error = Some("Cancelled during retry".to_string());
                        break;
                    }
                }

                // Log step_started
                let started_event = Event::new(
                    &run_id,
                    Some(step.id.clone()),
                    EventType::StepStarted,
                    json!({
                        "step_index": idx,
                        "step_type": step.step_type.to_string(),
                        "attempt": attempt + 1,
                    }),
                );
                let _ = self.store.insert_event(&started_event);
                let _ = logger.log_event(&started_event);

                // Insert step result (running)
                let sr_id = ulid::Ulid::new().to_string();
                let step_start_time = Utc::now();
                let sr = StepResult {
                    id: sr_id.clone(),
                    run_id: run_id.clone(),
                    step_id: step.id.clone(),
                    step_index: idx as u32,
                    status: StepStatus::Running,
                    attempt: attempt + 1,
                    started_at: step_start_time,
                    ended_at: None,
                    input_json: interpolated_params.clone(),
                    output_json: None,
                    error_json: None,
                };
                let _ = self.store.insert_step_result(&sr);

                // P1 fix: Dispatch execution with timeout enforcement.
                // Uses dispatch_params (secrets resolved); the stored input_json
                // above keeps the {{secret:*}} reference form.
                // Undo journal (M15): copy-before-mutate for filesystem steps.
                if let Some(ref undo_dir) = self.config.undo_dir {
                    crate::undo::record(
                        undo_dir,
                        &run_id,
                        &step.step_type,
                        &dispatch_params,
                        idx as u32,
                    );
                }

                let exec_result = execute_with_timeout(
                    &step.step_type,
                    &dispatch_params,
                    &mut helper,
                    &mut browser,
                    timeout_ms,
                );

                // M3: Intercept ELEMENT_AMBIGUOUS for interactive disambiguation
                let exec_result = maybe_disambiguate(
                    exec_result,
                    self.config.interactive,
                    &step.step_type,
                    &dispatch_params,
                    &mut helper,
                    &mut browser,
                    timeout_ms,
                );

                match exec_result {
                    Ok(output) => {
                        // Update step result: succeeded
                        let sr_updated = StepResult {
                            status: StepStatus::Succeeded,
                            ended_at: Some(Utc::now()),
                            output_json: Some(output.clone()),
                            ..sr
                        };
                        let _ = self.store.update_step_result(&sr_updated);

                        // Log step_finished
                        let finished_event = Event::new(
                            &run_id,
                            Some(step.id.clone()),
                            EventType::StepFinished,
                            json!({
                                "step_index": idx,
                                "attempt": attempt + 1,
                                "status": "succeeded",
                            }),
                        );
                        let _ = self.store.insert_event(&finished_event);
                        let _ = logger.log_event(&finished_event);

                        // Store output in variables as step.<step_id>.output
                        // so later steps can reference ${step.<id>.output}
                        let step_var_key = "step".to_string();
                        let step_entry = json!({
                            "output": output,
                            "status": "succeeded",
                        });

                        merge_step_variable(&mut variables, &step_var_key, &step.id, step_entry);

                        step_succeeded = true;
                        break;
                    }
                    Err(e) => {
                        let err_str = e.to_string();
                        // Timeouts are retryable; most other errors are too.
                        // Non-retryable: validation/interpolation errors, policy
                        // errors (already handled above).
                        last_error_retryable = is_retryable_error(&e);

                        tracing::warn!(
                            step_id = %step.id,
                            attempt = attempt + 1,
                            retryable = last_error_retryable,
                            "step execution failed: {}",
                            err_str
                        );

                        // Update step result: failed
                        let sr_updated = StepResult {
                            status: StepStatus::Failed,
                            ended_at: Some(Utc::now()),
                            error_json: Some(json!({ "message": err_str })),
                            ..sr
                        };
                        let _ = self.store.update_step_result(&sr_updated);

                        // Log step_failed
                        let failed_event = Event::new(
                            &run_id,
                            Some(step.id.clone()),
                            EventType::StepFailed,
                            json!({
                                "step_index": idx,
                                "attempt": attempt + 1,
                                "error": err_str,
                                "retryable": last_error_retryable,
                            }),
                        );
                        let _ = self.store.insert_event(&failed_event);
                        let _ = logger.log_event(&failed_event);

                        last_error = Some(err_str);
                        // Continue to next retry attempt (if any remain)
                    }
                }
            }

            if step_succeeded {
                steps_succeeded += 1;

                // Budget: accumulate bytes written by fs-write steps and stop if
                // the run exceeds its write allowance.
                if matches!(
                    step.step_type,
                    StepType::SysWriteFile | StepType::SysAppendFile
                ) {
                    if let Some(content) =
                        interpolated_params.get("content").and_then(|v| v.as_str())
                    {
                        bytes_written += content.len() as u64;
                    }
                }
                if let Some(max) = budget.as_ref().and_then(|b| b.max_bytes_written) {
                    if bytes_written > max {
                        budget_exceeded = Some(format!("max_bytes_written ({max}) exceeded"));
                        steps_skipped += total_steps - idx - 1;
                        break;
                    }
                }
            } else {
                // All attempts exhausted (or cancelled during retries)
                steps_failed += 1;

                // Store the failed step's status in variables
                let step_var_key = "step".to_string();
                let step_entry = json!({
                    "output": null,
                    "status": "failed",
                });

                merge_step_variable(&mut variables, &step_var_key, &step.id, step_entry);

                // Handle on_fail strategy
                match handle_on_fail_with_ask(
                    &step.effective_on_fail(),
                    self.config.interactive,
                    &step.id,
                    last_error.as_deref().unwrap_or("unknown error"),
                ) {
                    OnFailAction::Abort => {
                        aborted = true;
                        steps_skipped += total_steps - idx - 1;
                        break;
                    }
                    OnFailAction::Continue => continue,
                    OnFailAction::Retry => {
                        // P2 fix: Actually re-execute the step once when user
                        // requests retry from the "ask" prompt.
                        tracing::info!(
                            step_id = %step.id,
                            "user requested retry, re-executing step once"
                        );

                        let retry_result = execute_with_timeout(
                            &step.step_type,
                            &interpolated_params,
                            &mut helper,
                            &mut browser,
                            timeout_ms,
                        );

                        // M3: Apply disambiguation on retry path too
                        let retry_result = maybe_disambiguate(
                            retry_result,
                            self.config.interactive,
                            &step.step_type,
                            &interpolated_params,
                            &mut helper,
                            &mut browser,
                            timeout_ms,
                        );

                        match retry_result {
                            Ok(output) => {
                                // Undo the failure count, count as success
                                steps_failed -= 1;
                                steps_succeeded += 1;

                                let sr_retry = StepResult {
                                    id: ulid::Ulid::new().to_string(),
                                    run_id: run_id.clone(),
                                    step_id: step.id.clone(),
                                    step_index: idx as u32,
                                    status: StepStatus::Succeeded,
                                    attempt: max_retries + 2, // after all retries + user retry
                                    started_at: Utc::now(),
                                    ended_at: Some(Utc::now()),
                                    input_json: interpolated_params.clone(),
                                    output_json: Some(output.clone()),
                                    error_json: None,
                                };
                                let _ = self.store.insert_step_result(&sr_retry);

                                let step_var_key2 = "step".to_string();
                                let step_entry2 = json!({
                                    "output": output,
                                    "status": "succeeded",
                                });
                                merge_step_variable(
                                    &mut variables,
                                    &step_var_key2,
                                    &step.id,
                                    step_entry2,
                                );
                            }
                            Err(e) => {
                                tracing::warn!(
                                    step_id = %step.id,
                                    "user-requested retry also failed: {}",
                                    e
                                );
                                // Failure count already incremented above
                            }
                        }
                        continue;
                    }
                }
            }
        }

        // -- 9z. Paused: persist a checkpoint + open an approval (M11) ----------
        if let Some((idx, ref reason, is_takeover)) = paused {
            // Resume cursor: a takeover was completed by the human, so continue
            // PAST it; an escalation still needs to RUN, so re-execute it (with
            // the one-shot approved-escalation grant).
            let resume_idx = if is_takeover { idx + 1 } else { idx };
            let checkpoint = Checkpoint {
                run_id: run_id.clone(),
                plan_id: plan_id.to_string(),
                kind: CheckpointKind::Plan,
                cursor: json!({ "step_index": resume_idx }),
                variables: Some(json!(variables)),
                transcript: None,
                goal: None,
                reason: reason.clone(),
            };
            let _ = self.store.save_checkpoint(&checkpoint);
            let kind = if is_takeover {
                ApprovalKind::Takeover
            } else {
                ApprovalKind::Escalation
            };
            let _ = self.store.create_approval(&run_id, kind, reason);
            tracing::warn!(run_id = %run_id, step = idx, "run paused: {reason}");
        }

        // -- 10. Determine terminal run status ----------------------------------
        // P2 fix: Correct terminal status classification.
        // - Paused if a human takeover / escalation suspended the run (M11)
        // - Cancelled if cancel flag was set (SIGINT/SIGTERM)
        // - Failed if aborted (on_fail=abort triggered)
        // - CompletedWithErrors if some steps failed but run was not aborted
        // - Succeeded if all steps passed
        let terminal_status = if paused.is_some() {
            RunStatus::Paused
        } else if self.cancel_flag.load(Ordering::Relaxed) {
            RunStatus::Cancelled
        } else if aborted {
            RunStatus::Failed
        } else if budget_exceeded.is_some() {
            // Stopped early for a non-error reason (budget) — preserve results.
            RunStatus::Partial
        } else if steps_failed > 0 {
            RunStatus::CompletedWithErrors
        } else {
            RunStatus::Succeeded
        };

        // -- 11. Update run, log run_finished -----------------------------------
        let error_payload = if steps_failed > 0 {
            Some(json!({
                "steps_failed": steps_failed,
                "steps_skipped": steps_skipped,
            }))
        } else {
            None
        };

        self.store
            .update_run_status(&run_id, &terminal_status, error_payload.as_ref())?;

        let event_type = if terminal_status == RunStatus::Cancelled {
            EventType::RunCancelled
        } else {
            EventType::RunFinished
        };

        let duration_ms = wall_start.elapsed().as_millis() as u64;
        let run_finished_event = Event::new(
            &run_id,
            None,
            event_type,
            json!({
                "status": serde_json::to_value(&terminal_status).unwrap_or_default(),
                "steps_succeeded": steps_succeeded,
                "steps_failed": steps_failed,
                "steps_skipped": steps_skipped,
                "duration_ms": duration_ms,
                "budget_exceeded": budget_exceeded,
                "policy_denied": policy_denied,
            }),
        );
        self.store.insert_event(&run_finished_event)?;
        let _ = logger.log_event(&run_finished_event);

        // Disconnect helper if it was connected.
        if let Some(ref mut h) = helper {
            h.disconnect();
        }
        // Tear down the browser lane (kills Chrome + removes ephemeral profile).
        browser.shutdown();

        tracing::info!(
            run_id = %run_id,
            status = ?terminal_status,
            succeeded = steps_succeeded,
            failed = steps_failed,
            skipped = steps_skipped,
            duration_ms = duration_ms,
            "run finished"
        );

        Ok(RunSummary {
            run_id,
            plan_id: plan_id.to_string(),
            status: terminal_status,
            steps_total: total_steps,
            steps_succeeded,
            steps_failed,
            steps_skipped,
            duration_ms,
            policy_denied,
        })
    }
}

/// Merges config + plan run budgets into the most restrictive combination
/// (the smaller of each present limit). Returns `None` if neither sets a budget.
fn merge_budgets(
    config: Option<&operator_core::grants::RunBudget>,
    plan: Option<&operator_core::grants::RunBudget>,
) -> Option<operator_core::grants::RunBudget> {
    match (config, plan) {
        (None, None) => None,
        (Some(b), None) | (None, Some(b)) => Some(b.clone()),
        (Some(a), Some(b)) => {
            fn min_opt<T: Ord + Copy>(x: Option<T>, y: Option<T>) -> Option<T> {
                match (x, y) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (v, None) | (None, v) => v,
                }
            }
            Some(operator_core::grants::RunBudget {
                max_steps: min_opt(a.max_steps, b.max_steps),
                max_wall_ms: min_opt(a.max_wall_ms, b.max_wall_ms),
                max_bytes_written: min_opt(a.max_bytes_written, b.max_bytes_written),
                max_llm_calls: min_opt(a.max_llm_calls, b.max_llm_calls),
                max_llm_cost_usd: match (a.max_llm_cost_usd, b.max_llm_cost_usd) {
                    (Some(x), Some(y)) => Some(x.min(y)),
                    (v, None) | (None, v) => v,
                },
            })
        }
    }
}

// ---------------------------------------------------------------------------
// RunSession — ad-hoc, one-step-at-a-time gated execution
// ---------------------------------------------------------------------------

/// The outcome of a failed session step, so callers (MCP tools, the agent loop)
/// can distinguish a policy/grant/escalation denial from an execution error.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// Denied by the policy gate (allow-list, grant, or confirmation).
    #[error("policy denied: {0}")]
    Denied(String),
    /// The always-escalate classifier flagged this action; a human must approve
    /// out-of-band before it can run (M11). The driver should pause + checkpoint.
    #[error("escalation required: {0}")]
    Escalation(String),
    /// A `ui.human_takeover` control step: pause and let the human drive, then
    /// resume (M11). The payload is the prompt to show the human.
    #[error("human takeover: {0}")]
    Takeover(String),
    /// The step ran but errored (execution/IPC/timeout).
    #[error("{0}")]
    Failed(String),
}

/// A live execution session for **ad-hoc, one-step-at-a-time** gated execution —
/// the shared substrate for MCP server mode (M9) and the native agent loop (M10).
///
/// Unlike [`Engine::execute_plan`] (which runs a fixed plan), a session executes
/// steps supplied one at a time (e.g. per MCP tool call). It holds a *persistent*
/// helper connection, so element refs minted by `ui.snapshot` survive across
/// calls, and a single run in the store, so every step is audited under one
/// session. Each step goes through the **same** policy gate, secret resolution,
/// redaction, and dispatch path as plan execution (shared helpers), so grants,
/// budgets, the escalation classifier, and audit apply identically.
pub struct RunSession {
    store: Store,
    config: EngineConfig,
    run_id: String,
    plan_id: String,
    policy: PolicyGate,
    helper: Option<HelperClient>,
    browser: crate::browser_lane::BrowserHandle,
    logger: RunLogger,
    step_index: u32,
    steps_succeeded: usize,
    steps_failed: usize,
    policy_denied: bool,
    wall_start: Instant,
}

impl RunSession {
    /// Opens a session: persists a synthetic session plan (so the run has a
    /// plan row and records the granted envelope), creates the run, builds the
    /// policy gate from config + session grants, and prepares a lazily-connected
    /// helper.
    pub fn open(
        store: Store,
        config: EngineConfig,
        session_grants: Option<operator_core::grants::Grants>,
        session_budget: Option<operator_core::grants::RunBudget>,
    ) -> Result<Self, RuntimeError> {
        let mode = config.mode.clone();
        let session_plan = Plan {
            schema_version: 1,
            name: "mcp-session".to_string(),
            description: Some("Ad-hoc session (MCP / agent loop)".to_string()),
            mode: Some(mode.clone()),
            allow_apps: (!config.allow_apps.is_empty()).then(|| config.allow_apps.clone()),
            allow_domains: (!config.allow_domains.is_empty()).then(|| config.allow_domains.clone()),
            grants: session_grants.clone(),
            budget: session_budget,
            variables: None,
            steps: vec![],
        };
        let plan_id = store
            .save_plan(&session_plan)
            .map_err(|e| RuntimeError::Other(format!("failed to save session plan: {e}")))?;
        let run_id = store.create_run(&plan_id, &mode)?;

        let mut logger = RunLogger::new(&config.log_dir, &run_id)
            .map_err(|e| RuntimeError::Other(format!("failed to create run logger: {e}")))?;

        let mut grant_layers: Vec<operator_core::grants::Grants> = Vec::new();
        if let Some(ref g) = config.grants {
            grant_layers.push(g.clone());
        }
        if let Some(g) = session_grants {
            grant_layers.push(g);
        }
        let policy = PolicyGate::new(
            mode.clone(),
            config.yes_to_all,
            config.interactive,
            config.dry_run,
            config.allow_apps.clone(),
            config.allow_domains.clone(),
            grant_layers,
        );

        store.update_run_status(&run_id, &RunStatus::Running, None)?;
        let started = Event::new(
            &run_id,
            None,
            EventType::RunStarted,
            json!({ "plan_id": plan_id, "session": true }),
        );
        let _ = store.insert_event(&started);
        let _ = logger.log_event(&started);

        Ok(RunSession {
            helper: Some(HelperClient::new(config.helper_path.clone())),
            browser: crate::browser_lane::BrowserHandle::new(
                config.allow_domains.clone(),
                crate::browser_lane::lane_options(true, false, None),
            ),
            store,
            config,
            run_id,
            plan_id,
            policy,
            logger,
            step_index: 0,
            steps_succeeded: 0,
            steps_failed: 0,
            policy_denied: false,
            wall_start: Instant::now(),
        })
    }

    /// The session's run id (a run in the store; audit rows accrue under it).
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// The session's plan id (the synthetic session plan row).
    pub fn plan_id(&self) -> &str {
        &self.plan_id
    }

    /// Grants one out-of-band-approved escalation before resuming (M11): the
    /// next always-escalate action is allowed through exactly once.
    pub fn grant_one_escalation(&self) {
        self.policy.grant_one_escalation();
    }

    /// Abandons the session WITHOUT writing a terminal run status — used when the
    /// run paused (the supervisor has set status `Paused` and persisted a
    /// checkpoint). Disconnects the helper so no child is left running.
    pub fn abandon(mut self) {
        if let Some(mut h) = self.helper.take() {
            h.disconnect();
        }
        self.browser.shutdown();
    }

    /// Executes one step through the full gate + secret + audit + dispatch path.
    /// Returns the step's output JSON, or a [`SessionError`] distinguishing a
    /// policy denial from an execution failure. Does not retry — a driver (the
    /// model) decides whether to try again.
    pub fn execute_step(&mut self, step: &Step) -> Result<serde_json::Value, SessionError> {
        let idx = self.step_index as usize;
        self.step_index += 1;
        let params = step.params.clone();

        // -- Control lane: ui.human_takeover is a pause request, not a dispatch.
        if step.step_type == StepType::UiHumanTakeover {
            let prompt = params
                .get("prompt")
                .and_then(|v| v.as_str())
                .unwrap_or("Human takeover requested — complete the step, then resume.")
                .to_string();
            return Err(SessionError::Takeover(prompt));
        }

        // -- Policy gate (grants / escalation / allow-lists) ------------------
        let decision = match self
            .policy
            .check_step_with_params(step, &params, idx, idx + 1)
        {
            Ok(d) => d,
            // An always-escalate action is a pause point, not a failure: don't
            // record a failed step result, surface it so the driver checkpoints.
            Err(PolicyError::EscalationRequired(reason)) => {
                return Err(SessionError::Escalation(reason));
            }
            Err(e) => {
                let msg = e.to_string();
                self.record_failure(&step.id, idx, &params, &msg);
                self.policy_denied = true;
                return Err(SessionError::Denied(msg));
            }
        };
        match decision {
            PolicyDecision::Allowed => {}
            PolicyDecision::DryRun => return Ok(json!({ "dry_run": true })),
            PolicyDecision::Denied => {
                let msg = "step denied by policy".to_string();
                self.record_failure(&step.id, idx, &params, &msg);
                self.policy_denied = true;
                return Err(SessionError::Denied(msg));
            }
            PolicyDecision::ConfirmationRequired => {
                let msg = "confirmation required but session is non-interactive".to_string();
                self.record_failure(&step.id, idx, &params, &msg);
                self.policy_denied = true;
                return Err(SessionError::Denied(msg));
            }
        }

        // -- Resolve {{secret:*}} for dispatch only ---------------------------
        let dispatch = if operator_core::secrets::has_secret_refs(&params) {
            match operator_core::secrets::resolve_secrets(&params) {
                Ok(p) => p,
                Err(e) => {
                    let msg = format!("Secret resolution failed: {e}");
                    self.record_failure(&step.id, idx, &params, &msg);
                    return Err(SessionError::Failed(msg));
                }
            }
        } else {
            params.clone()
        };

        // -- Record running, dispatch, record result -------------------------
        let sr = StepResult {
            id: ulid::Ulid::new().to_string(),
            run_id: self.run_id.clone(),
            step_id: step.id.clone(),
            step_index: idx as u32,
            status: StepStatus::Running,
            attempt: 1,
            started_at: Utc::now(),
            ended_at: None,
            input_json: params.clone(), // keeps {{secret:*}} reference form
            output_json: None,
            error_json: None,
        };
        let _ = self.store.insert_step_result(&sr);

        let timeout = self.config.default_timeout_ms;
        // Undo journal (M15): copy-before-mutate for filesystem steps.
        if let Some(ref undo_dir) = self.config.undo_dir {
            crate::undo::record(
                undo_dir,
                &self.run_id,
                &step.step_type,
                &dispatch,
                idx as u32,
            );
        }

        let result = execute_with_timeout(
            &step.step_type,
            &dispatch,
            &mut self.helper,
            &mut self.browser,
            timeout,
        );
        // Non-interactive: ELEMENT_AMBIGUOUS surfaces to the caller as an error
        // (the model refines its selector), so no interactive disambiguation.
        match result {
            Ok(output) => {
                let done = StepResult {
                    status: StepStatus::Succeeded,
                    ended_at: Some(Utc::now()),
                    output_json: Some(output.clone()),
                    ..sr
                };
                let _ = self.store.update_step_result(&done);
                let ev = Event::new(
                    &self.run_id,
                    Some(step.id.clone()),
                    EventType::StepFinished,
                    json!({ "step_index": idx, "status": "succeeded" }),
                );
                let _ = self.store.insert_event(&ev);
                let _ = self.logger.log_event(&ev);
                self.steps_succeeded += 1;
                Ok(output)
            }
            Err(e) => {
                let msg = e.to_string();
                let done = StepResult {
                    status: StepStatus::Failed,
                    ended_at: Some(Utc::now()),
                    error_json: Some(json!({ "message": msg })),
                    ..sr
                };
                let _ = self.store.update_step_result(&done);
                emit_step_failed(
                    &self.store,
                    &mut self.logger,
                    &self.run_id,
                    &step.id,
                    idx,
                    &msg,
                );
                self.steps_failed += 1;
                Err(SessionError::Failed(msg))
            }
        }
    }

    fn record_failure(&mut self, step_id: &str, idx: usize, params: &serde_json::Value, msg: &str) {
        tracing::warn!(step_id = %step_id, "{}", msg);
        let sr = make_failed_step_result(&self.run_id, step_id, idx, params, msg);
        let _ = self.store.insert_step_result(&sr);
        emit_step_failed(
            &self.store,
            &mut self.logger,
            &self.run_id,
            step_id,
            idx,
            msg,
        );
        self.steps_failed += 1;
    }

    /// Finalizes the session: marks the run terminal, emits `run_finished`, and
    /// disconnects the helper. Returns a summary of the session's steps.
    /// Runs a program (a step list that may contain `control.*` steps) through
    /// the deterministic control-flow interpreter (M13). Leaf steps execute via
    /// [`execute_step`](Self::execute_step) — same gate/audit as everything else.
    /// `variables` seeds the interpolation/condition scope.
    pub fn run_program(
        &mut self,
        steps: &[Step],
        variables: std::collections::HashMap<String, serde_json::Value>,
    ) -> Result<(), crate::control::ControlError> {
        let mut vars = variables;
        crate::control::run_program(self, steps, &mut vars)
    }

    pub fn finish(mut self) -> RunSummary {
        let status = if self.steps_failed > 0 {
            RunStatus::CompletedWithErrors
        } else {
            RunStatus::Succeeded
        };
        let _ = self.store.update_run_status(&self.run_id, &status, None);
        let duration_ms = self.wall_start.elapsed().as_millis() as u64;
        let ev = Event::new(
            &self.run_id,
            None,
            EventType::RunFinished,
            json!({
                "status": serde_json::to_value(&status).unwrap_or_default(),
                "steps_succeeded": self.steps_succeeded,
                "steps_failed": self.steps_failed,
                "duration_ms": duration_ms,
                "session": true,
            }),
        );
        let _ = self.store.insert_event(&ev);
        let _ = self.logger.log_event(&ev);
        if let Some(mut h) = self.helper.take() {
            h.disconnect();
        }
        self.browser.shutdown();
        RunSummary {
            run_id: self.run_id.clone(),
            plan_id: self.plan_id.clone(),
            status,
            steps_total: self.steps_succeeded + self.steps_failed,
            steps_succeeded: self.steps_succeeded,
            steps_failed: self.steps_failed,
            steps_skipped: 0,
            duration_ms,
            policy_denied: self.policy_denied,
        }
    }
}

/// Lets the control-flow interpreter (M13) drive a `RunSession`: each leaf step
/// runs through the full gate/secret/audit path, so loop bodies are as governed
/// as any other step.
impl crate::control::LeafRunner for RunSession {
    fn run_leaf(&mut self, step: &Step) -> Result<serde_json::Value, String> {
        self.execute_step(step).map_err(|e| e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Timeout wrapper
// ---------------------------------------------------------------------------

/// Executes a step with a timeout. Spawns the actual execution on a worker
/// thread and waits up to `timeout_ms` milliseconds for it to complete.
/// Returns `RuntimeError::Other("Step timed out ...")` on timeout.
fn execute_with_timeout(
    step_type: &StepType,
    params: &serde_json::Value,
    helper: &mut Option<HelperClient>,
    browser: &mut crate::browser_lane::BrowserHandle,
    timeout_ms: u64,
) -> Result<serde_json::Value, RuntimeError> {
    let lane = step_type.lane();
    let step_type_clone = step_type.clone();
    let params_clone = params.clone();

    match lane {
        "system" => {
            // Run the system step on a worker thread. sys.exec enforces the
            // deadline itself and KILLS its child on timeout (so it isn't left
            // orphaned); the channel recv is a backstop with extra grace so the
            // in-handler timeout fires first and returns a clean result.
            let (tx, rx) = mpsc::channel();
            thread::spawn(move || {
                let result = execute_system_step_with_timeout(
                    &step_type_clone,
                    &params_clone,
                    Some(timeout_ms),
                );
                let _ = tx.send(result.map_err(RuntimeError::from));
            });

            let backstop = Duration::from_millis(timeout_ms.saturating_add(5_000));
            match rx.recv_timeout(backstop) {
                Ok(result) => result,
                Err(mpsc::RecvTimeoutError::Timeout) => Err(RuntimeError::Other(format!(
                    "Step timed out after {}ms",
                    timeout_ms
                ))),
                Err(mpsc::RecvTimeoutError::Disconnected) => Err(RuntimeError::Other(
                    "Step execution thread panicked".to_string(),
                )),
            }
        }
        "ui" => {
            // UI steps go through the helper IPC client.
            let h = helper
                .as_mut()
                .ok_or_else(|| RuntimeError::Other("Helper client not initialised".to_string()))?;

            if !h.is_connected() {
                h.connect()?;
            }

            // Bound the IPC read so a hung helper cannot block the run forever.
            // The helper runs its own implicit waits (e.g. wait_for polling up to
            // its timeout_ms), so pad the read deadline generously — it should
            // only trip when the helper is genuinely stuck, not merely waiting.
            let read_timeout = Duration::from_millis(timeout_ms.saturating_add(10_000));
            let result = h.send_with_timeout(step_type.as_str(), params.clone(), read_timeout)?;
            Ok(result)
        }
        "browser" => {
            // Browser steps run on the per-run browser lane (own thread +
            // tokio runtime); Chrome launches lazily in an isolated profile.
            let step = Step {
                id: ulid::Ulid::new().to_string(),
                step_type: step_type.clone(),
                params: params.clone(),
                timeout_ms: None,
                retries: None,
                retry_backoff_ms: None,
                on_fail: None,
            };
            browser.execute(&step)
        }
        other => Err(RuntimeError::Other(format!(
            "Unknown lane '{}' for step type {}",
            other, step_type
        ))),
    }
}

// ---------------------------------------------------------------------------
// Interactive disambiguation for ELEMENT_AMBIGUOUS
// ---------------------------------------------------------------------------

/// If the result is an `ELEMENT_AMBIGUOUS` error and the session is interactive,
/// display the candidates to the user on stderr and let them choose. Then re-send
/// the IPC call with `selector.index` set to the chosen candidate.
///
/// In non-interactive mode the error is returned as-is.
fn maybe_disambiguate(
    result: Result<serde_json::Value, RuntimeError>,
    interactive: bool,
    step_type: &StepType,
    params: &serde_json::Value,
    helper: &mut Option<HelperClient>,
    browser: &mut crate::browser_lane::BrowserHandle,
    timeout_ms: u64,
) -> Result<serde_json::Value, RuntimeError> {
    let err = match result {
        Ok(v) => return Ok(v),
        Err(e) => e,
    };

    // Check if this is specifically an ELEMENT_AMBIGUOUS helper error
    let (code, details) = match &err {
        RuntimeError::Ipc(operator_ipc::IpcError::HelperError { code, details, .. }) => {
            (code.as_str(), details.clone())
        }
        _ => return Err(err),
    };

    if code != "ELEMENT_AMBIGUOUS" {
        return Err(err);
    }

    if !interactive {
        return Err(err);
    }

    // Extract candidates from error details
    let candidates = details
        .as_ref()
        .and_then(|d| d.get("candidates"))
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();

    if candidates.is_empty() {
        return Err(err);
    }

    // Display disambiguation prompt on stderr
    let app_name = params
        .get("app")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown app");

    eprintln!();
    eprintln!("Multiple UI matches for selector in app \"{}\":", app_name);

    for (i, candidate) in candidates.iter().enumerate() {
        let role = candidate
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let name = candidate.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let path = candidate.get("path").and_then(|v| v.as_str()).unwrap_or("");

        let name_display = if name.is_empty() {
            String::new()
        } else {
            format!(" name=\"{}\"", name)
        };

        eprintln!("  {}) {}{} path=\"{}\"", i + 1, role, name_display, path);
    }

    eprint!("Choose [1-{}] or q to abort: ", candidates.len());

    // Read user choice from stdin
    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_err() {
        return Err(err);
    }

    let input = input.trim();
    if input.eq_ignore_ascii_case("q") || input.eq_ignore_ascii_case("quit") {
        return Err(RuntimeError::Cancelled);
    }

    let chosen: usize = match input.parse::<usize>() {
        Ok(n) if n >= 1 && n <= candidates.len() => n - 1,
        _ => {
            eprintln!("Invalid choice, aborting disambiguation.");
            return Err(err);
        }
    };

    // Re-send with selector.index set to the chosen candidate index
    let mut new_params = params.clone();
    if let Some(selector) = new_params.get_mut("selector") {
        if let Some(obj) = selector.as_object_mut() {
            obj.insert("index".to_string(), json!(chosen));
        }
    }

    eprintln!("Re-sending with index {}...", chosen);

    execute_with_timeout(step_type, &new_params, helper, browser, timeout_ms)
}

/// Determines whether an error is retryable. Validation errors,
/// interpolation errors, and policy errors are not retryable.
/// Execution errors (timeouts, process failures, IPC errors) are retryable.
/// ELEMENT_AMBIGUOUS is not retryable (requires disambiguation, not retry).
fn is_retryable_error(err: &RuntimeError) -> bool {
    match err {
        RuntimeError::Validation(_) => false,
        RuntimeError::Core(_) => false, // interpolation/validation errors
        RuntimeError::PolicyDenied(_) => false,
        RuntimeError::Store(_) => false,
        RuntimeError::SystemExec(_) => true,
        RuntimeError::Ipc(ipc_err) => {
            // ELEMENT_AMBIGUOUS needs user disambiguation, not blind retry
            if let operator_ipc::IpcError::HelperError { code, .. } = ipc_err {
                code != "ELEMENT_AMBIGUOUS"
            } else {
                true
            }
        }
        RuntimeError::Cancelled => false,
        RuntimeError::Other(msg) => {
            // Timeouts are retryable
            msg.contains("timed out") || !msg.contains("not allowed")
        }
    }
}

// ---------------------------------------------------------------------------
// on_fail handling
// ---------------------------------------------------------------------------

/// The action to take after a step failure.
enum OnFailAction {
    Abort,
    Continue,
    Retry,
}

/// Determines the action to take based on the on_fail policy, without
/// interactive "ask" prompt.
fn handle_on_fail(on_fail: &OnFail, _interactive: bool) -> OnFailAction {
    match on_fail {
        OnFail::Abort => OnFailAction::Abort,
        OnFail::Continue => OnFailAction::Continue,
        OnFail::Ask => {
            // Ask requires interactivity, handled by the _with_ask variant.
            // Fallback: abort.
            OnFailAction::Abort
        }
    }
}

/// Determines the action to take based on the on_fail policy, with support
/// for the interactive "ask" prompt.
fn handle_on_fail_with_ask(
    on_fail: &OnFail,
    interactive: bool,
    step_id: &str,
    error_msg: &str,
) -> OnFailAction {
    match on_fail {
        OnFail::Abort => OnFailAction::Abort,
        OnFail::Continue => OnFailAction::Continue,
        OnFail::Ask => {
            if !interactive {
                tracing::warn!(
                    step_id = %step_id,
                    "on_fail=ask but session is not interactive, aborting"
                );
                return OnFailAction::Abort;
            }

            let prompt = format!(
                "Step '{}' failed: {}\n[c]ontinue, [a]bort, [r]etry-now: ",
                step_id, error_msg,
            );

            let stderr = std::io::stderr();
            let mut stderr_lock = stderr.lock();
            let _ = std::io::Write::write_all(&mut stderr_lock, prompt.as_bytes());
            let _ = std::io::Write::flush(&mut stderr_lock);

            let stdin = std::io::stdin();
            let mut line = String::new();
            if std::io::BufRead::read_line(&mut stdin.lock(), &mut line).is_ok() {
                match line.trim().to_ascii_lowercase().as_str() {
                    "c" | "continue" => OnFailAction::Continue,
                    "r" | "retry" | "retry-now" => OnFailAction::Retry,
                    _ => OnFailAction::Abort, // 'a', 'abort', or anything else
                }
            } else {
                OnFailAction::Abort
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Intersects a CLI-level allow-list with an optional plan-level allow-list.
///
/// - If the CLI list is empty, use the plan list (or empty).
/// - If the plan list is None or empty, use the CLI list.
/// - Otherwise, return only entries present in **both** lists (case-insensitive).
fn intersect_lists(cli: &[String], plan: &Option<Vec<String>>) -> Vec<String> {
    let plan_list = match plan {
        Some(p) if !p.is_empty() => p,
        _ => return cli.to_vec(),
    };

    if cli.is_empty() {
        return plan_list.clone();
    }

    // Both non-empty: intersect (case-insensitive).
    cli.iter()
        .filter(|c| plan_list.iter().any(|p| p.eq_ignore_ascii_case(c.as_str())))
        .cloned()
        .collect()
}

/// Creates a `StepResult` in the `Failed` state with the given error message.
fn make_failed_step_result(
    run_id: &str,
    step_id: &str,
    step_index: usize,
    input: &serde_json::Value,
    error_msg: &str,
) -> StepResult {
    StepResult {
        id: ulid::Ulid::new().to_string(),
        run_id: run_id.to_string(),
        step_id: step_id.to_string(),
        step_index: step_index as u32,
        status: StepStatus::Failed,
        attempt: 1,
        started_at: Utc::now(),
        ended_at: Some(Utc::now()),
        input_json: input.clone(),
        output_json: None,
        error_json: Some(json!({ "message": error_msg })),
    }
}

/// Merges a step's result entry into the `step` variable in the variables map.
///
/// The `step` variable is a JSON object keyed by step id, e.g.:
/// ```json
/// { "myStep": { "output": ..., "status": "succeeded" } }
/// ```
fn merge_step_variable(
    variables: &mut HashMap<String, serde_json::Value>,
    var_key: &str,
    step_id: &str,
    entry: serde_json::Value,
) {
    if let Some(existing) = variables.get(var_key) {
        if let Some(existing_obj) = existing.as_object() {
            let mut merged = existing_obj.clone();
            merged.insert(step_id.to_string(), entry);
            variables.insert(var_key.to_string(), serde_json::Value::Object(merged));
        } else {
            // Existing value is not an object, replace it.
            let mut obj = serde_json::Map::new();
            obj.insert(step_id.to_string(), entry);
            variables.insert(var_key.to_string(), serde_json::Value::Object(obj));
        }
    } else {
        let mut obj = serde_json::Map::new();
        obj.insert(step_id.to_string(), entry);
        variables.insert(var_key.to_string(), serde_json::Value::Object(obj));
    }
}

/// Emits a `StepFailed` event into both the store and the logger.
fn emit_step_failed(
    store: &Store,
    logger: &mut RunLogger,
    run_id: &str,
    step_id: &str,
    step_index: usize,
    error_msg: &str,
) {
    let event = Event::new(
        run_id,
        Some(step_id.to_string()),
        EventType::StepFailed,
        json!({
            "step_index": step_index,
            "error": error_msg,
        }),
    );
    let _ = store.insert_event(&event);
    let _ = logger.log_event(&event);
}

#[cfg(test)]
mod session_tests {
    use super::*;
    use operator_core::grants::Grants;
    use operator_store::Store;

    fn test_config(log_dir: std::path::PathBuf, grants: Option<Grants>) -> EngineConfig {
        EngineConfig {
            mode: Mode::Unsafe,
            yes_to_all: true,
            interactive: false,
            dry_run: false,
            allow_apps: vec![],
            allow_domains: vec![],
            log_dir,
            helper_path: None,
            default_timeout_ms: 5000,
            default_retries: 0,
            default_backoff_ms: 0,
            grants,
            budget: None,
            undo_dir: None,
        }
    }

    fn adhoc_step(id: &str, step_type: StepType, params: serde_json::Value) -> Step {
        Step {
            id: id.to_string(),
            step_type,
            params,
            timeout_ms: None,
            retries: None,
            retry_backoff_ms: None,
            on_fail: None,
        }
    }

    #[test]
    fn session_enforces_grants_and_audits_each_step() {
        let dir = std::env::temp_dir().join(format!("opjack-sess-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&dir).unwrap();
        let grant = Grants {
            fs_write: Some(vec![format!("{}/**", dir.display())]),
            ..Default::default()
        };
        let store = Store::open_in_memory().unwrap();
        let mut session =
            RunSession::open(store, test_config(dir.clone(), Some(grant)), None, None)
                .expect("open session");

        // Inside the fs_write grant -> allowed.
        let inside = adhoc_step(
            "ok",
            StepType::SysWriteFile,
            json!({ "path": format!("{}/ok.txt", dir.display()), "content": "hi" }),
        );
        assert!(session.execute_step(&inside).is_ok());
        assert!(dir.join("ok.txt").exists());

        // Outside the grant -> denied (a policy error, not an execution error).
        let outside = adhoc_step(
            "deny",
            StepType::SysWriteFile,
            json!({ "path": "/tmp/opjack-should-be-denied-xyz.txt", "content": "x" }),
        );
        match session.execute_step(&outside) {
            Err(SessionError::Denied(msg)) => assert!(msg.contains("fs_write")),
            other => panic!("expected Denied, got {other:?}"),
        }

        let summary = session.finish();
        assert_eq!(summary.steps_succeeded, 1);
        assert_eq!(summary.steps_failed, 1);
        assert!(summary.policy_denied);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
