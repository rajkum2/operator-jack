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
use operator_exec_system::executor::execute_system_step;
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
    on_run_created: Option<Box<dyn FnOnce(&str)>>,
}

impl Engine {
    /// Creates a new engine with the given store and configuration.
    pub fn new(store: Store, config: EngineConfig) -> Self {
        Self {
            store,
            config,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            on_run_created: None,
        }
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

        // -- 2. Determine effective mode ----------------------------------------
        let effective_mode = plan.mode.clone().unwrap_or_else(|| self.config.mode.clone());

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

        let policy = PolicyGate::new(
            effective_mode.clone(),
            self.config.yes_to_all,
            self.config.interactive,
            self.config.dry_run,
            allow_apps,
            allow_domains,
        );

        // -- 6. Optionally create HelperClient (lazy connect on first UI step) --
        let mut helper: Option<HelperClient> =
            Some(HelperClient::new(self.config.helper_path.clone()));

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

        // -- 8. Initialise variables from plan.variables ------------------------
        let mut variables: HashMap<String, serde_json::Value> = plan
            .variables
            .clone()
            .unwrap_or_default();

        // -- 9. Step execution loop ---------------------------------------------
        let total_steps = plan.steps.len();
        let mut steps_succeeded: usize = 0;
        let mut steps_failed: usize = 0;
        let mut steps_skipped: usize = 0;
        let mut aborted = false;

        for (idx, step) in plan.steps.iter().enumerate() {
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

                    match handle_on_fail(
                        &step.effective_on_fail(),
                        self.config.interactive,
                    ) {
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

            // -- 9c. Policy gate ------------------------------------------------
            // Use interpolated params for policy checks so variable-derived
            // app names and URLs are properly validated.
            let policy_decision = match policy.check_step_with_params(step, &interpolated_params, idx, total_steps) {
                Ok(d) => d,
                Err(PolicyError::AppNotAllowed(app)) => {
                    let msg = format!("App not allowed: {}", app);
                    tracing::warn!(step_id = %step.id, "{}", msg);
                    let sr = make_failed_step_result(
                        &run_id, &step.id, idx, &interpolated_params, &msg,
                    );
                    let _ = self.store.insert_step_result(&sr);
                    emit_step_failed(
                        &self.store, &mut logger, &run_id, &step.id, idx, &msg,
                    );
                    steps_failed += 1;

                    // Respect on_fail instead of unconditional abort.
                    match handle_on_fail(
                        &step.effective_on_fail(),
                        self.config.interactive,
                    ) {
                        OnFailAction::Abort => {
                            aborted = true;
                            steps_skipped += total_steps - idx - 1;
                            break;
                        }
                        OnFailAction::Continue => continue,
                        OnFailAction::Retry => continue, // cannot retry a policy error
                    }
                }
                Err(PolicyError::DomainNotAllowed(domain)) => {
                    let msg = format!("Domain not allowed: {}", domain);
                    tracing::warn!(step_id = %step.id, "{}", msg);
                    let sr = make_failed_step_result(
                        &run_id, &step.id, idx, &interpolated_params, &msg,
                    );
                    let _ = self.store.insert_step_result(&sr);
                    emit_step_failed(
                        &self.store, &mut logger, &run_id, &step.id, idx, &msg,
                    );
                    steps_failed += 1;

                    // Respect on_fail instead of unconditional abort.
                    match handle_on_fail(
                        &step.effective_on_fail(),
                        self.config.interactive,
                    ) {
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

                    match handle_on_fail(
                        &step.effective_on_fail(),
                        self.config.interactive,
                    ) {
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

                    match handle_on_fail(
                        &step.effective_on_fail(),
                        self.config.interactive,
                    ) {
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

                // P1 fix: Dispatch execution with timeout enforcement
                let exec_result = execute_with_timeout(
                    &step.step_type,
                    &interpolated_params,
                    &mut helper,
                    timeout_ms,
                );

                // M3: Intercept ELEMENT_AMBIGUOUS for interactive disambiguation
                let exec_result = maybe_disambiguate(
                    exec_result,
                    self.config.interactive,
                    &step.step_type,
                    &interpolated_params,
                    &mut helper,
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

                        merge_step_variable(
                            &mut variables,
                            &step_var_key,
                            &step.id,
                            step_entry,
                        );

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
            } else {
                // All attempts exhausted (or cancelled during retries)
                steps_failed += 1;

                // Store the failed step's status in variables
                let step_var_key = "step".to_string();
                let step_entry = json!({
                    "output": null,
                    "status": "failed",
                });

                merge_step_variable(
                    &mut variables,
                    &step_var_key,
                    &step.id,
                    step_entry,
                );

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
                            timeout_ms,
                        );

                        // M3: Apply disambiguation on retry path too
                        let retry_result = maybe_disambiguate(
                            retry_result,
                            self.config.interactive,
                            &step.step_type,
                            &interpolated_params,
                            &mut helper,
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

        // -- 10. Determine terminal run status ----------------------------------
        // P2 fix: Correct terminal status classification.
        // - Cancelled if cancel flag was set (SIGINT/SIGTERM)
        // - Failed if aborted (on_fail=abort triggered)
        // - CompletedWithErrors if some steps failed but run was not aborted
        // - Succeeded if all steps passed
        let terminal_status = if self.cancel_flag.load(Ordering::Relaxed) {
            RunStatus::Cancelled
        } else if aborted {
            RunStatus::Failed
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

        self.store.update_run_status(
            &run_id,
            &terminal_status,
            error_payload.as_ref(),
        )?;

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
            }),
        );
        self.store.insert_event(&run_finished_event)?;
        let _ = logger.log_event(&run_finished_event);

        // Disconnect helper if it was connected.
        if let Some(ref mut h) = helper {
            h.disconnect();
        }

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
        })
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
    timeout_ms: u64,
) -> Result<serde_json::Value, RuntimeError> {
    let lane = step_type.lane();
    let step_type_clone = step_type.clone();
    let params_clone = params.clone();

    match lane {
        "system" => {
            // Run system step on a worker thread with timeout.
            let (tx, rx) = mpsc::channel();
            thread::spawn(move || {
                let result = execute_system_step(&step_type_clone, &params_clone);
                let _ = tx.send(result.map_err(RuntimeError::from));
            });

            match rx.recv_timeout(Duration::from_millis(timeout_ms)) {
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
            let h = helper.as_mut().ok_or_else(|| {
                RuntimeError::Other("Helper client not initialised".to_string())
            })?;

            if !h.is_connected() {
                h.connect()?;
            }

            // For UI steps, the timeout is enforced by the IPC protocol
            // (the helper should respect timeout_ms). For now, use the same
            // channel-based approach.
            let result = h.send(step_type.as_str(), params.clone())?;
            Ok(result)
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
    timeout_ms: u64,
) -> Result<serde_json::Value, RuntimeError> {
    let err = match result {
        Ok(v) => return Ok(v),
        Err(e) => e,
    };

    // Check if this is specifically an ELEMENT_AMBIGUOUS helper error
    let (code, details) = match &err {
        RuntimeError::Ipc(operator_ipc::IpcError::HelperError {
            code,
            details,
            ..
        }) => (code.as_str(), details.clone()),
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
    eprintln!(
        "Multiple UI matches for selector in app \"{}\":",
        app_name
    );

    for (i, candidate) in candidates.iter().enumerate() {
        let role = candidate
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let name = candidate
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let path = candidate
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let name_display = if name.is_empty() {
            String::new()
        } else {
            format!(" name=\"{}\"", name)
        };

        eprintln!(
            "  {}) {}{} path=\"{}\"",
            i + 1,
            role,
            name_display,
            path
        );
    }

    eprint!(
        "Choose [1-{}] or q to abort: ",
        candidates.len()
    );

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

    execute_with_timeout(step_type, &new_params, helper, timeout_ms)
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
        .filter(|c| {
            plan_list
                .iter()
                .any(|p| p.eq_ignore_ascii_case(c.as_str()))
        })
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
