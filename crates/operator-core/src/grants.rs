//! Task-scoped grants, run budgets, and the always-escalate classifier — the
//! M8 safety envelope (see SPEC_FREEZE_V0.8.md).
//!
//! Grants are *additive restrictions*: a capability with no grant is
//! unrestricted; once granted, values must match. The runtime enforces them on
//! interpolated params, checking against every present layer (config + plan) so
//! the most restrictive wins.

use std::path::{Component, Path, PathBuf};

use globset::{Glob, GlobSetBuilder};
use serde::{Deserialize, Serialize};

use crate::types::StepType;

/// Task-scoped capability grants. `None` for a field means "no restriction".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grants {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fs_read: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fs_write: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exec: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domains: Option<Vec<String>>,
}

impl Grants {
    /// True if no capability is restricted (an empty envelope).
    pub fn is_unrestricted(&self) -> bool {
        self.fs_read.is_none()
            && self.fs_write.is_none()
            && self.exec.is_none()
            && self.domains.is_none()
    }
}

/// Whole-run resource bounds. `None` fields are unbounded.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RunBudget {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_steps: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_wall_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_bytes_written: Option<u64>,
    /// Dormant until M10 (plans make no run-time LLM calls).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_llm_calls: Option<u64>,
    /// Dormant until M10.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_llm_cost_usd: Option<f64>,
}

/// A capability that a grant can gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    FsRead,
    FsWrite,
    Exec,
    Domain,
}

impl Capability {
    pub fn name(&self) -> &'static str {
        match self {
            Capability::FsRead => "fs_read",
            Capability::FsWrite => "fs_write",
            Capability::Exec => "exec",
            Capability::Domain => "domains",
        }
    }
}

/// A grant violation: the capability, the offending value, and a human message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantDenial {
    pub capability: String,
    pub value: String,
    pub message: String,
}

/// An action that must be escalated to a human regardless of mode/grants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Escalation {
    pub reason: String,
}

// ---------------------------------------------------------------------------
// Grant enforcement
// ---------------------------------------------------------------------------

/// Checks a step's interpolated params against every present grant layer.
/// Returns the first violation, or `Ok(())` if all layers allow it (or none
/// restrict the relevant capability).
pub fn check_grants(
    step_type: &StepType,
    params: &serde_json::Value,
    layers: &[&Grants],
) -> Result<(), GrantDenial> {
    for (cap, raw_value) in grant_checks_for_step(step_type, params) {
        let is_fs = matches!(cap, Capability::FsRead | Capability::FsWrite);
        for layer in layers {
            let patterns = match cap {
                Capability::FsRead => &layer.fs_read,
                Capability::FsWrite => &layer.fs_write,
                Capability::Exec => &layer.exec,
                Capability::Domain => &layer.domains,
            };
            let Some(patterns) = patterns else { continue };

            let allowed = match cap {
                Capability::Domain => {
                    let host = extract_host(&raw_value);
                    patterns.iter().any(|p| domain_matches(&host, p))
                }
                Capability::FsRead | Capability::FsWrite => {
                    let normalized = normalize_path(&raw_value);
                    glob_any(patterns, &normalized, true)
                }
                Capability::Exec => glob_any(patterns, &raw_value, false),
            };

            if !allowed {
                let shown = if is_fs {
                    normalize_path(&raw_value)
                } else {
                    raw_value.clone()
                };
                return Err(GrantDenial {
                    capability: cap.name().to_string(),
                    value: shown.clone(),
                    message: format!(
                        "'{}' is not permitted by the '{}' grant (value: {}). \
                         Add it to the run's grants to allow.",
                        cap.name(),
                        cap.name(),
                        shown
                    ),
                });
            }
        }
    }
    Ok(())
}

/// The (capability, value) pairs a step must satisfy, from its params.
fn grant_checks_for_step(
    step_type: &StepType,
    params: &serde_json::Value,
) -> Vec<(Capability, String)> {
    let s = |k: &str| params.get(k).and_then(|v| v.as_str()).map(String::from);
    match step_type {
        StepType::SysReadFile => s("path")
            .map(|p| vec![(Capability::FsRead, p)])
            .unwrap_or_default(),
        StepType::SysWriteFile
        | StepType::SysAppendFile
        | StepType::SysMkdir
        | StepType::SysDeletePath => s("path")
            .map(|p| vec![(Capability::FsWrite, p)])
            .unwrap_or_default(),
        StepType::SysMovePath => {
            let mut v = vec![];
            if let Some(f) = s("from") {
                v.push((Capability::FsWrite, f));
            }
            if let Some(t) = s("to") {
                v.push((Capability::FsWrite, t));
            }
            v
        }
        StepType::SysCopyPath => {
            let mut v = vec![];
            if let Some(f) = s("from") {
                v.push((Capability::FsRead, f));
            }
            if let Some(t) = s("to") {
                v.push((Capability::FsWrite, t));
            }
            v
        }
        StepType::SysExec => vec![(Capability::Exec, reconstruct_cmdline(params))],
        StepType::SysOpenUrl | StepType::BrowserNavigate => s("url")
            .map(|u| vec![(Capability::Domain, u)])
            .unwrap_or_default(),
        _ => vec![],
    }
}

// ---------------------------------------------------------------------------
// Always-escalate classifier
// ---------------------------------------------------------------------------

/// Apps for which send/purchase-like clicks always escalate.
const SENSITIVE_APPS: &[&str] = &["mail", "messages", "slack", "outlook", "spark"];
/// Control-name fragments that indicate an outbound / purchase action.
const SENSITIVE_ACTIONS: &[&str] = &[
    "send",
    "buy",
    "pay",
    "checkout",
    "place order",
    "confirm purchase",
    "purchase",
];

/// Returns an escalation if the step is a high-consequence action that must
/// reach a human regardless of mode or grants. (Bulk-delete counting and
/// secure-field refusal are enforced in the runtime and helper respectively.)
pub fn check_escalation(step_type: &StepType, params: &serde_json::Value) -> Option<Escalation> {
    match step_type {
        StepType::SysExec => {
            let cmd = reconstruct_cmdline(params);
            let lc = cmd.to_lowercase();
            let first = lc.split_whitespace().next().unwrap_or("");
            let pipes_to_shell = lc.contains("| sh")
                || lc.contains("|sh")
                || lc.contains("| bash")
                || lc.contains("|bash")
                || lc.contains("| zsh");
            if first == "sudo" || lc.contains(" sudo ") || lc.contains("rm -rf") || pipes_to_shell {
                Some(Escalation {
                    reason: format!("sys.exec runs a high-consequence command: `{cmd}`"),
                })
            } else {
                None
            }
        }
        StepType::UiClick | StepType::UiKeyPress => {
            let app = params
                .get("app")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_lowercase();
            if !SENSITIVE_APPS.iter().any(|a| app == *a) {
                return None;
            }
            // Look at the selector name / key for a send/purchase action.
            let mut label = String::new();
            if let Some(sel) = params.get("selector") {
                for key in ["name", "name_contains", "identifier", "description"] {
                    if let Some(v) = sel.get(key).and_then(|v| v.as_str()) {
                        label.push_str(&v.to_lowercase());
                        label.push(' ');
                    }
                }
            }
            if let Some(k) = params.get("key").and_then(|v| v.as_str()) {
                label.push_str(&k.to_lowercase());
            }
            if SENSITIVE_ACTIONS.iter().any(|a| label.contains(a)) {
                Some(Escalation {
                    reason: format!(
                        "action in '{app}' looks like an outbound/purchase action ('{}')",
                        label.trim()
                    ),
                })
            } else {
                None
            }
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Reconstructs a shell-ish command line from `sys.exec` params for matching.
fn reconstruct_cmdline(params: &serde_json::Value) -> String {
    let cmd = params.get("command").and_then(|v| v.as_str()).unwrap_or("");
    let args: Vec<String> = params
        .get("args")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if args.is_empty() {
        cmd.to_string()
    } else {
        format!("{} {}", cmd, args.join(" "))
    }
}

/// Expands a leading `~` and collapses `.`/`..` lexically so a path cannot use
/// `..` to escape a grant (`~/scraped/../secrets` -> `<home>/secrets`).
pub fn normalize_path(path: &str) -> String {
    let expanded = if let Some(rest) = path.strip_prefix("~/") {
        dirs::home_dir()
            .unwrap_or_default()
            .join(rest)
            .to_string_lossy()
            .to_string()
    } else if path == "~" {
        dirs::home_dir()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    } else {
        path.to_string()
    };

    let mut out = PathBuf::new();
    for comp in Path::new(&expanded).components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out.to_string_lossy().to_string()
}

/// True if `candidate` matches any pattern. For fs patterns, `~` is expanded so
/// grants can be written with `~`; `**` crosses `/`.
fn glob_any(patterns: &[String], candidate: &str, expand_tilde: bool) -> bool {
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        let pat = if expand_tilde {
            expand_tilde_str(p)
        } else {
            p.clone()
        };
        if let Ok(g) = Glob::new(&pat) {
            builder.add(g);
        }
    }
    match builder.build() {
        Ok(set) => set.is_match(candidate),
        Err(_) => false,
    }
}

fn expand_tilde_str(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("~/") {
        format!(
            "{}/{}",
            dirs::home_dir().unwrap_or_default().to_string_lossy(),
            rest
        )
    } else if s == "~" {
        dirs::home_dir()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    } else {
        s.to_string()
    }
}

/// Extracts the lowercased host from a URL (or returns the input if not a URL).
pub fn extract_host(url: &str) -> String {
    let after_scheme = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let host_port = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(after_scheme);
    let host = host_port
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(host_port);
    // Strip a numeric port.
    let host = match host.rsplit_once(':') {
        Some((h, port)) if port.chars().all(|c| c.is_ascii_digit()) => h,
        _ => host,
    };
    host.to_lowercase()
}

/// Domain match: exact, dot-suffix (`example.com` matches `www.example.com`), or
/// glob (`*.example.com`).
fn domain_matches(host: &str, pattern: &str) -> bool {
    let pat = pattern.to_lowercase();
    if pat.contains('*') {
        return Glob::new(&pat)
            .map(|g| g.compile_matcher().is_match(host))
            .unwrap_or(false);
    }
    host == pat || host.ends_with(&format!(".{pat}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn grants(fs_write: &[&str], exec: &[&str], domains: &[&str]) -> Grants {
        Grants {
            fs_read: None,
            fs_write: if fs_write.is_empty() {
                None
            } else {
                Some(fs_write.iter().map(|s| s.to_string()).collect())
            },
            exec: if exec.is_empty() {
                None
            } else {
                Some(exec.iter().map(|s| s.to_string()).collect())
            },
            domains: if domains.is_empty() {
                None
            } else {
                Some(domains.iter().map(|s| s.to_string()).collect())
            },
        }
    }

    #[test]
    fn no_grants_allows_everything() {
        let g = Grants::default();
        let p = json!({"path": "/etc/passwd", "content": "x"});
        assert!(check_grants(&StepType::SysWriteFile, &p, &[&g]).is_ok());
    }

    #[test]
    fn fs_write_grant_allows_matching_and_denies_outside() {
        let g = grants(&["~/scraped/**"], &[], &[]);
        let ok = json!({"path": "~/scraped/out.csv", "content": "x"});
        assert!(check_grants(&StepType::SysWriteFile, &ok, &[&g]).is_ok());

        let bad = json!({"path": "/etc/hosts", "content": "x"});
        let err = check_grants(&StepType::SysWriteFile, &bad, &[&g]).unwrap_err();
        assert_eq!(err.capability, "fs_write");
    }

    #[test]
    fn dotdot_cannot_escape_fs_grant() {
        let g = grants(&["~/scraped/**"], &[], &[]);
        let escape = json!({"path": "~/scraped/../secrets/key", "content": "x"});
        let err = check_grants(&StepType::SysWriteFile, &escape, &[&g]).unwrap_err();
        assert_eq!(err.capability, "fs_write");
        assert!(
            !err.value.contains(".."),
            "path should be normalized: {}",
            err.value
        );
    }

    #[test]
    fn exec_grant_matches_command_line() {
        let g = grants(&[], &["pip install *", "python *"], &[]);
        let ok = json!({"command": "pip", "args": ["install", "requests"]});
        assert!(check_grants(&StepType::SysExec, &ok, &[&g]).is_ok());

        let bad = json!({"command": "curl", "args": ["http://x"]});
        assert!(check_grants(&StepType::SysExec, &bad, &[&g]).is_err());
    }

    #[test]
    fn domain_grant_suffix_and_glob() {
        let g = grants(&[], &[], &["example.com", "*.wikipedia.org"]);
        for url in [
            "https://example.com/x",
            "https://www.example.com",
            "https://en.wikipedia.org/wiki/Y",
        ] {
            let p = json!({ "url": url });
            assert!(
                check_grants(&StepType::SysOpenUrl, &p, &[&g]).is_ok(),
                "{url}"
            );
        }
        let bad = json!({ "url": "https://evil.com" });
        assert!(check_grants(&StepType::SysOpenUrl, &bad, &[&g]).is_err());
    }

    #[test]
    fn both_layers_must_allow() {
        let config = grants(&["~/**"], &[], &[]);
        let plan = grants(&["~/scraped/**"], &[], &[]);
        // Allowed by both.
        let ok = json!({"path": "~/scraped/a", "content": "x"});
        assert!(check_grants(&StepType::SysWriteFile, &ok, &[&config, &plan]).is_ok());
        // Allowed by config, denied by plan.
        let bad = json!({"path": "~/other/a", "content": "x"});
        assert!(check_grants(&StepType::SysWriteFile, &bad, &[&config, &plan]).is_err());
    }

    #[test]
    fn escalation_flags_sudo_and_pipe_to_shell() {
        assert!(check_escalation(
            &StepType::SysExec,
            &json!({"command": "sudo", "args": ["rm", "x"]})
        )
        .is_some());
        assert!(check_escalation(
            &StepType::SysExec,
            &json!({"command": "rm", "args": ["-rf", "/"]})
        )
        .is_some());
        assert!(check_escalation(
            &StepType::SysExec,
            &json!({"command": "sh", "args": ["-c", "curl x | sh"]})
        )
        .is_some());
        assert!(check_escalation(
            &StepType::SysExec,
            &json!({"command": "ls", "args": ["-la"]})
        )
        .is_none());
    }

    #[test]
    fn escalation_flags_send_in_messaging_app() {
        let send = json!({"app": "Messages", "selector": {"name": "Send"}});
        assert!(check_escalation(&StepType::UiClick, &send).is_some());
        // Same action in a non-sensitive app does not escalate.
        let calc = json!({"app": "Calculator", "selector": {"name": "Send"}});
        assert!(check_escalation(&StepType::UiClick, &calc).is_none());
    }
}
