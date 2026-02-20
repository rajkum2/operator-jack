use operator_core::policy::{requires_confirmation, risk_level, RiskLevel};
use operator_core::types::{Mode, Step, StepType};
use std::io::{self, BufRead, Write};

// ---------------------------------------------------------------------------
// PolicyDecision / PolicyError
// ---------------------------------------------------------------------------

/// The outcome of a policy gate check for a single step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    /// The step is allowed to execute.
    Allowed,
    /// The step was denied by the user or policy.
    Denied,
    /// Dry-run mode: the step should be logged but not executed.
    DryRun,
    /// Non-interactive session requires confirmation that cannot be obtained.
    ConfirmationRequired,
}

/// An error from the policy gate (app/domain allow-list violations).
#[derive(Debug, thiserror::Error)]
pub enum PolicyError {
    #[error("App not allowed: {0}")]
    AppNotAllowed(String),
    #[error("Domain not allowed: {0}")]
    DomainNotAllowed(String),
}

// ---------------------------------------------------------------------------
// PolicyGate
// ---------------------------------------------------------------------------

/// Enforces safety policy (allow-lists, risk-level confirmation, dry-run) for
/// each step before it is executed.
pub struct PolicyGate {
    mode: Mode,
    yes_to_all: bool,
    interactive: bool,
    dry_run: bool,
    allow_apps: Vec<String>,
    allow_domains: Vec<String>,
}

impl PolicyGate {
    /// Creates a new policy gate.
    ///
    /// * `mode` -- Safe or Unsafe execution mode.
    /// * `yes_to_all` -- if true, automatically approve all confirmation prompts.
    /// * `interactive` -- whether the session has an interactive terminal for prompts.
    /// * `dry_run` -- if true, all steps return `DryRun` without executing.
    /// * `allow_apps` -- app allow-list; empty means "allow all".
    /// * `allow_domains` -- domain allow-list for `sys.open_url`; empty means "allow all".
    pub fn new(
        mode: Mode,
        yes_to_all: bool,
        interactive: bool,
        dry_run: bool,
        allow_apps: Vec<String>,
        allow_domains: Vec<String>,
    ) -> Self {
        Self {
            mode,
            yes_to_all,
            interactive,
            dry_run,
            allow_apps,
            allow_domains,
        }
    }

    /// Checks whether a step is allowed to run under the current policy.
    ///
    /// Returns `Ok(PolicyDecision)` on success, or `Err(PolicyError)` if the
    /// step violates an allow-list constraint (app or domain).
    ///
    /// Note: This uses the step's original params. Prefer `check_step_with_params`
    /// which uses interpolated params for accurate allowlist checks.
    pub fn check_step(
        &self,
        step: &Step,
        step_index: usize,
        total_steps: usize,
    ) -> Result<PolicyDecision, PolicyError> {
        self.check_step_with_params(step, &step.params, step_index, total_steps)
    }

    /// Checks whether a step is allowed to run under the current policy,
    /// using the provided (interpolated) params for allowlist checks.
    ///
    /// This ensures that variable-derived app names and URLs are properly
    /// validated against the allow-lists.
    pub fn check_step_with_params(
        &self,
        step: &Step,
        params: &serde_json::Value,
        step_index: usize,
        total_steps: usize,
    ) -> Result<PolicyDecision, PolicyError> {
        // -- 1. Allow-apps check (using interpolated params) --------------------
        if !self.allow_apps.is_empty() {
            if let Some(app) = extract_app_from_params(&step.step_type, params) {
                let allowed = self
                    .allow_apps
                    .iter()
                    .any(|a| a.eq_ignore_ascii_case(&app));
                if !allowed {
                    return Err(PolicyError::AppNotAllowed(app));
                }
            }
        }

        // -- 2. Allow-domains check (sys.open_url only, using interpolated params)
        if !self.allow_domains.is_empty() {
            if step.step_type == StepType::SysOpenUrl {
                if let Some(url) = params.get("url").and_then(|v| v.as_str()) {
                    let domain = extract_domain(url);
                    if !domain.is_empty() {
                        let allowed = self
                            .allow_domains
                            .iter()
                            .any(|d| d.eq_ignore_ascii_case(&domain));
                        if !allowed {
                            return Err(PolicyError::DomainNotAllowed(domain));
                        }
                    }
                }
            }
        }

        // -- 3. Dry-run ---------------------------------------------------------
        if self.dry_run {
            return Ok(PolicyDecision::DryRun);
        }

        // -- 4. Risk-level / confirmation logic ---------------------------------
        if !requires_confirmation(&step.step_type, &self.mode) {
            return Ok(PolicyDecision::Allowed);
        }

        // Confirmation is required.
        if self.yes_to_all {
            return Ok(PolicyDecision::Allowed);
        }

        if self.interactive {
            let risk = risk_level(&step.step_type);
            let risk_str = match risk {
                RiskLevel::Low => "low",
                RiskLevel::Medium => "medium",
                RiskLevel::High => "high",
            };

            let prompt = format!(
                "Step {}/{} [{}] is {}-risk in safe mode. Approve? [y/N]: ",
                step_index + 1,
                total_steps,
                step.step_type,
                risk_str,
            );

            let approved = prompt_yes_no(&prompt);
            if approved {
                return Ok(PolicyDecision::Allowed);
            } else {
                return Ok(PolicyDecision::Denied);
            }
        }

        // Non-interactive, not yes_to_all: cannot obtain confirmation.
        Ok(PolicyDecision::ConfirmationRequired)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extracts the `app` parameter from given params and step type.
/// This covers both UI-lane steps (which typically have an `app`
/// param) and the system-lane `open_app`, `quit_app` equivalents.
fn extract_app_from_params(step_type: &StepType, params: &serde_json::Value) -> Option<String> {
    // UI steps that carry an "app" param
    let is_ui_with_app = step_type.lane() == "ui"
        && !matches!(
            step_type,
            StepType::UiCheckAccessibilityPermission | StepType::UiListApps
        );

    // System steps that reference an app by name
    let is_sys_app_step = matches!(
        step_type,
        StepType::SysOpenApp | StepType::SysQuitApp
    );

    if is_ui_with_app || is_sys_app_step {
        params
            .get("app")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    } else {
        None
    }
}

/// Extracts the domain (host) portion of a URL by finding the text between
/// `://` and the next `/` (or `:` for port), stripping any port number.
fn extract_domain(url: &str) -> String {
    let after_scheme = if let Some(idx) = url.find("://") {
        &url[idx + 3..]
    } else {
        url
    };

    // Take everything up to the next '/' or end-of-string
    let host_and_port = if let Some(idx) = after_scheme.find('/') {
        &after_scheme[..idx]
    } else {
        after_scheme
    };

    // Strip optional port (:8080, etc.)
    if let Some(idx) = host_and_port.rfind(':') {
        // Make sure this colon is actually a port separator and not part of an
        // IPv6 address (which would contain multiple colons).
        let potential_port = &host_and_port[idx + 1..];
        if potential_port.chars().all(|c| c.is_ascii_digit()) {
            return host_and_port[..idx].to_string();
        }
    }

    host_and_port.to_string()
}

/// Prompts the user on stderr/stdin and returns true if they type 'y' or 'Y'.
fn prompt_yes_no(prompt: &str) -> bool {
    let stderr = io::stderr();
    let mut stderr_lock = stderr.lock();
    let _ = write!(stderr_lock, "{}", prompt);
    let _ = stderr_lock.flush();

    let stdin = io::stdin();
    let mut line = String::new();
    if stdin.lock().read_line(&mut line).is_ok() {
        let trimmed = line.trim();
        trimmed.eq_ignore_ascii_case("y") || trimmed.eq_ignore_ascii_case("yes")
    } else {
        false
    }
}
