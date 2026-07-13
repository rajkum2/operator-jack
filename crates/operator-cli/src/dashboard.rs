//! Local settings & control dashboard (M-dash): a self-contained web UI served
//! on `127.0.0.1` that lets the user *see* status/runs/approvals and *edit*
//! configuration without hand-writing TOML.
//!
//! Security posture (this endpoint can approve escalations, edit config, and
//! delete secrets, so it is locked down):
//!   * binds `127.0.0.1` only — never a routable interface;
//!   * a random per-session token (printed at startup, embedded in the served
//!     page) gates every `/api/*` call;
//!   * the `Host` header must be `127.0.0.1:PORT`/`localhost:PORT` (DNS-rebinding
//!     defense) and POSTs must be `Content-Type: application/json`;
//!   * no CORS headers are ever sent (default-deny for cross-origin fetches);
//!   * API keys are NEVER written to `config.toml` (the config schema has no
//!     api_key field — keys stay in env vars / the keychain) and secret VALUES
//!     are never returned over the API.

use std::io::{Cursor, Read};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use tiny_http::{Header, Method, Request, Response, Server};

use operator_core::config::OperatorConfig;
use operator_store::Store;

/// Shared dashboard state (one instance behind an Arc, cloned into each
/// per-request thread).
struct App {
    token: String,
    port: u16,
    store: Mutex<Store>,
    helper_path: Option<String>,
}

/// Serves the dashboard until Ctrl-C. `helper_path` is the already-resolved
/// macOS helper binary path (for the on-demand permissions check).
pub fn serve(port: u16, no_open: bool, helper_path: Option<String>) -> Result<()> {
    let addr = format!("127.0.0.1:{port}");
    let server = Server::http(&addr).map_err(|e| {
        anyhow!("failed to bind {addr}: {e}. Is the port already in use? Try --port <N>.")
    })?;

    // A dedicated store connection with a busy timeout so dashboard writes
    // (approve/deny/prune) wait for a live run's transaction instead of failing.
    let store = crate::open_store()?;
    let _ = store.set_busy_timeout(3000);

    let token = ulid::Ulid::new().to_string();
    let app = Arc::new(App {
        token: token.clone(),
        port,
        store: Mutex::new(store),
        helper_path,
    });

    let url = format!("http://127.0.0.1:{port}/?token={token}");
    eprintln!("operator-jack dashboard");
    eprintln!("  → {url}");
    eprintln!("  (bound to 127.0.0.1 only; the token gates every action — keep this URL private)");
    eprintln!("  Ctrl-C to stop.");
    if !no_open {
        let _ = std::process::Command::new("open").arg(&url).status();
    }

    for request in server.incoming_requests() {
        let app = Arc::clone(&app);
        // One thread per request keeps a slow permissions check from blocking
        // the UI; the store is shared behind a Mutex.
        std::thread::spawn(move || {
            if let Err(e) = handle(&app, request) {
                tracing::warn!("dashboard request error: {e}");
            }
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Request handling
// ---------------------------------------------------------------------------

fn handle(app: &App, mut request: Request) -> Result<()> {
    // DNS-rebinding defense: only accept requests addressed to our loopback host.
    if !host_ok(&request, app.port) {
        return respond(
            request,
            403,
            json!({ "error": "forbidden: bad Host header" }),
        );
    }

    let raw_url = request.url().to_string();
    let (path, query) = split_query(&raw_url);
    let method = request.method().clone();

    // The page itself (token embedded) is the only non-/api GET we serve.
    if path == "/" && method == Method::Get {
        let html = include_str!("dashboard.html").replace("__TOKEN__", &app.token);
        return respond_html(request, &html);
    }

    if !path.starts_with("/api/") {
        return respond(request, 404, json!({ "error": "not found" }));
    }

    // Every API call must carry the session token.
    if !token_ok(&request, query, &app.token) {
        return respond(
            request,
            403,
            json!({ "error": "forbidden: missing or invalid token" }),
        );
    }

    // Read a JSON body for mutating requests.
    let body = if method == Method::Post {
        if !content_type_json(&request) {
            return respond(
                request,
                415,
                json!({ "error": "POST requires Content-Type: application/json" }),
            );
        }
        read_body(&mut request)?
    } else {
        String::new()
    };

    let result = route(app, &method, path, query, &body);
    match result {
        Ok((code, val)) => respond(request, code, val),
        Err(e) => respond(request, 500, json!({ "error": e.to_string() })),
    }
}

/// Dispatches an `/api/*` request to a handler. Returns `(status, json)`.
fn route(app: &App, method: &Method, path: &str, query: &str, body: &str) -> Result<(u16, Value)> {
    let seg: Vec<&str> = path.trim_matches('/').split('/').collect();
    // seg[0] == "api"
    match (method, seg.as_slice()) {
        (Method::Get, ["api", "status"]) => api_status(app),
        (Method::Get, ["api", "permissions"]) => Ok((200, api_permissions(app))),
        (Method::Get, ["api", "config"]) => api_get_config(),
        (Method::Post, ["api", "config"]) => api_save_config(body),
        (Method::Get, ["api", "runs"]) => api_list_runs(app, query),
        (Method::Get, ["api", "runs", id]) => api_run_detail(app, id),
        (Method::Get, ["api", "approvals"]) => api_list_approvals(app, query),
        (Method::Post, ["api", "approvals", id, "decide"]) => api_decide_approval(app, id, body),
        (Method::Get, ["api", "secrets"]) => Ok((
            200,
            json!({ "names": operator_core::secrets::list_secrets() }),
        )),
        (Method::Post, ["api", "secrets"]) => api_set_secret(body),
        (Method::Post, ["api", "secrets", name, "delete"]) => api_delete_secret(name),
        (Method::Get, ["api", "skills"]) => api_list_skills(),
        (Method::Post, ["api", "prune"]) => api_prune(app, body),
        _ => Ok((404, json!({ "error": "no such endpoint" }))),
    }
}

// ---------------------------------------------------------------------------
// Endpoints
// ---------------------------------------------------------------------------

/// Cheap, instant status — safe to call on page load (no helper spawn, no dialog).
fn api_status(app: &App) -> Result<(u16, Value)> {
    let db_path = crate::resolve_db_path();
    let sqlite_ok = app.store.lock().is_ok();
    let cfg_path = config_path();
    Ok((
        200,
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "sqlite_ok": sqlite_ok,
            "data_dir": crate::resolve_data_dir().display().to_string(),
            "db_path": db_path.display().to_string(),
            "log_dir": crate::resolve_log_dir().display().to_string(),
            "helper_path": app.helper_path,
            "helper_found": app.helper_path.as_ref().map(|p| PathBuf::from(p).exists()).unwrap_or(false),
            "config_path": cfg_path.display().to_string(),
            "config_exists": cfg_path.exists(),
            "full_disk_access": crate::full_disk_access_probe(),
        }),
    ))
}

/// On-demand permission probes — may spawn the helper + take a moment. Uses
/// `prompt: false` so it never pops a system dialog.
fn api_permissions(app: &App) -> Value {
    use operator_ipc::client::HelperClient;
    let mut accessibility = json!("unknown");
    let mut screen_recording = json!("unknown");

    let mut helper = HelperClient::new(app.helper_path.clone());
    if helper.connect().is_ok() {
        if let Ok(r) = helper.send_with_timeout(
            "ui.check_accessibility_permission",
            json!({ "prompt": false }),
            Duration::from_secs(6),
        ) {
            accessibility = r;
        }
        if let Ok(r) = helper.send_with_timeout(
            "ui.check_screen_recording_permission",
            json!({}),
            Duration::from_secs(6),
        ) {
            screen_recording = r;
        }
        helper.disconnect();
    }

    // Provider readiness (cheap: env-key presence; ollama is local so left to `doctor`).
    let providers = json!({
        "kimi": { "api_key_set": std::env::var("KIMI_API_KEY").is_ok() },
        "openai": { "api_key_set": std::env::var("OPENAI_API_KEY").is_ok() },
        "anthropic": { "api_key_set": std::env::var("ANTHROPIC_API_KEY").is_ok() },
        "ollama": { "note": "local — run `operator-jack doctor` to verify the daemon/model" },
    });

    json!({
        "accessibility": accessibility,
        "screen_recording": screen_recording,
        "full_disk_access": crate::full_disk_access_probe(),
        "providers": providers,
        "note": "Automation (AppleEvents) consent is prompted per-app on first script.* use.",
    })
}

/// Returns the on-disk config (loaded WITHOUT env overrides, so saving is safe).
fn api_get_config() -> Result<(u16, Value)> {
    let path = config_path();
    let cfg = load_config_no_env(&path)?;
    let value = serde_json::to_value(&cfg)?;
    Ok((
        200,
        json!({
            "path": path.display().to_string(),
            "exists": path.exists(),
            "config": value,
        }),
    ))
}

/// Saves an edited config. The client posts the full config object (as returned
/// by GET, edited); we deserialize it (unknown fields — e.g. any smuggled
/// api_key — are ignored by serde), lightly validate, and atomically write TOML.
fn api_save_config(body: &str) -> Result<(u16, Value)> {
    let incoming: Value = serde_json::from_str(body).context("body is not valid JSON")?;
    // Accept either {config: {...}} or the bare config object.
    let cfg_val = incoming.get("config").cloned().unwrap_or(incoming);

    let cfg: OperatorConfig =
        serde_json::from_value(cfg_val).context("does not match the config schema")?;

    // Light validation before persisting.
    if cfg.default_mode != "safe" && cfg.default_mode != "unsafe" {
        return Ok((
            400,
            json!({ "error": format!("default_mode must be \"safe\" or \"unsafe\", got {:?}", cfg.default_mode) }),
        ));
    }
    if !["kimi", "openai", "anthropic", "ollama"].contains(&cfg.default_provider.as_str()) {
        return Ok((
            400,
            json!({ "error": format!("default_provider must be kimi|openai|anthropic|ollama, got {:?}", cfg.default_provider) }),
        ));
    }

    let toml_text = toml::to_string_pretty(&cfg).context("failed to serialize config to TOML")?;
    let path = config_path();
    atomic_write(&path, &toml_text)?;

    Ok((
        200,
        json!({
            "saved": true,
            "path": path.display().to_string(),
            "note": "Comments from `init` are not preserved on save. In-flight runs are unaffected (config is read at command start).",
        }),
    ))
}

fn api_list_runs(app: &App, query: &str) -> Result<(u16, Value)> {
    let limit = query_param(query, "limit")
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(50)
        .min(500);
    let store = app
        .store
        .lock()
        .map_err(|_| anyhow!("store lock poisoned"))?;
    let runs = store.list_runs(limit)?;
    Ok((200, json!({ "runs": runs })))
}

fn api_run_detail(app: &App, id: &str) -> Result<(u16, Value)> {
    let store = app
        .store
        .lock()
        .map_err(|_| anyhow!("store lock poisoned"))?;
    let run = match store.get_run(id) {
        Ok(r) => r,
        Err(_) => return Ok((404, json!({ "error": "run not found" }))),
    };
    let steps = store.get_step_results(id).unwrap_or_default();
    let events = store.get_events(id).unwrap_or_default();
    let checkpoint = store.load_checkpoint(id).ok().flatten();
    Ok((
        200,
        json!({ "run": run, "steps": steps, "events": events, "checkpoint": checkpoint }),
    ))
}

fn api_list_approvals(app: &App, query: &str) -> Result<(u16, Value)> {
    use operator_core::types::ApprovalStatus;
    let all = query_param(query, "all")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false);
    let filter = if all {
        None
    } else {
        Some(ApprovalStatus::Pending)
    };
    let store = app
        .store
        .lock()
        .map_err(|_| anyhow!("store lock poisoned"))?;
    let approvals = store.list_approvals(filter)?;
    Ok((200, json!({ "approvals": approvals })))
}

fn api_decide_approval(app: &App, id: &str, body: &str) -> Result<(u16, Value)> {
    use operator_core::types::ApprovalStatus;
    let v: Value = serde_json::from_str(body).unwrap_or(json!({}));
    let decision = v.get("status").and_then(|s| s.as_str()).unwrap_or("");
    let status = match decision {
        "approved" | "approve" => ApprovalStatus::Approved,
        "denied" | "deny" => ApprovalStatus::Denied,
        other => {
            return Ok((
                400,
                json!({ "error": format!("status must be approved|denied, got {other:?}") }),
            ))
        }
    };

    let store = app
        .store
        .lock()
        .map_err(|_| anyhow!("store lock poisoned"))?;
    let approval = match store.get_approval(id)? {
        Some(a) => a,
        None => return Ok((404, json!({ "error": "approval not found" }))),
    };
    if approval.status != ApprovalStatus::Pending {
        return Ok((
            409,
            json!({ "error": format!("approval already {:?}", approval.status) }),
        ));
    }
    store.decide_approval(id, status)?;

    // Approving flips the row; the paused run still resumes via the CLI. Surface
    // the exact command so the UI isn't misleading.
    let resume_cmd = if status == ApprovalStatus::Approved {
        // A takeover checkpoint is a plan/agent run; we can't be sure which
        // resume verb without the checkpoint, so show the run-id both ways.
        Some(format!(
            "operator-jack run --resume {rid}   (or: agent --resume {rid})",
            rid = approval.run_id
        ))
    } else {
        None
    };
    Ok((
        200,
        json!({ "decided": decision, "run_id": approval.run_id, "resume_command": resume_cmd }),
    ))
}

fn api_set_secret(body: &str) -> Result<(u16, Value)> {
    let v: Value = serde_json::from_str(body).context("invalid JSON")?;
    let name = v.get("name").and_then(|s| s.as_str()).unwrap_or("").trim();
    let value = v.get("value").and_then(|s| s.as_str()).unwrap_or("");
    if name.is_empty() {
        return Ok((400, json!({ "error": "name is required" })));
    }
    if value.is_empty() {
        return Ok((400, json!({ "error": "value must not be empty" })));
    }
    // Validate name against the {{secret:NAME}} charset.
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
    {
        return Ok((
            400,
            json!({ "error": "name may contain only letters, digits, _ . -" }),
        ));
    }
    operator_core::secrets::set_secret(name, value).map_err(|e| anyhow!("{e}"))?;
    Ok((
        200,
        json!({ "set": name, "reference": format!("{{{{secret:{name}}}}}") }),
    ))
}

fn api_delete_secret(name: &str) -> Result<(u16, Value)> {
    let name = percent_decode(name);
    operator_core::secrets::delete_secret(&name).map_err(|e| anyhow!("{e}"))?;
    Ok((200, json!({ "removed": name })))
}

fn api_list_skills() -> Result<(u16, Value)> {
    match operator_skills::list_skills() {
        Ok(skills) => {
            let out: Vec<Value> = skills
                .iter()
                .map(|s| json!({ "name": s.name, "description": s.description, "version": s.version }))
                .collect();
            Ok((200, json!({ "skills": out })))
        }
        Err(e) => Ok((200, json!({ "skills": [], "error": e.to_string() }))),
    }
}

fn api_prune(app: &App, body: &str) -> Result<(u16, Value)> {
    let v: Value = serde_json::from_str(body).unwrap_or(json!({}));
    let days = v
        .get("older_than_days")
        .and_then(|d| d.as_i64())
        .unwrap_or(30)
        .max(0);
    let keep = v.get("keep").and_then(|d| d.as_u64()).unwrap_or(20) as usize;
    let cutoff = chrono::Utc::now() - chrono::Duration::days(days);
    let store = app
        .store
        .lock()
        .map_err(|_| anyhow!("store lock poisoned"))?;
    let pruned = store.prune_runs(cutoff, keep)?;
    Ok((
        200,
        json!({ "pruned": pruned, "older_than_days": days, "kept": keep }),
    ))
}

// ---------------------------------------------------------------------------
// Config helpers
// ---------------------------------------------------------------------------

/// Resolves the config path the same way the CLI does (env override first).
fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("OPERATOR_CONFIG_PATH") {
        return PathBuf::from(p);
    }
    OperatorConfig::default_path().unwrap_or_else(|| PathBuf::from("config.toml"))
}

/// Loads config from `path` WITHOUT applying env overrides (so a save doesn't
/// bake transient OPERATOR_* values into the file). Missing file → defaults.
fn load_config_no_env(path: &std::path::Path) -> Result<OperatorConfig> {
    if path.exists() {
        OperatorConfig::load_from(path).map_err(|e| anyhow!("failed to read config: {e}"))
    } else {
        Ok(OperatorConfig::default())
    }
}

/// Atomic write: temp file in the same dir, then rename over the target.
fn atomic_write(path: &std::path::Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// HTTP plumbing
// ---------------------------------------------------------------------------

fn split_query(url: &str) -> (&str, &str) {
    match url.split_once('?') {
        Some((p, q)) => (p, q),
        None => (url, ""),
    }
}

fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        if k == key {
            Some(percent_decode(v))
        } else {
            None
        }
    })
}

fn header_value(request: &Request, name: &str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str().to_string())
}

fn host_ok(request: &Request, port: u16) -> bool {
    match header_value(request, "Host") {
        Some(h) => h == format!("127.0.0.1:{port}") || h == format!("localhost:{port}"),
        // HTTP/1.0 without a Host header from a local client — allow.
        None => true,
    }
}

fn content_type_json(request: &Request) -> bool {
    header_value(request, "Content-Type")
        .map(|c| c.to_ascii_lowercase().starts_with("application/json"))
        .unwrap_or(false)
}

fn token_ok(request: &Request, query: &str, token: &str) -> bool {
    if let Some(t) = header_value(request, "X-Dashboard-Token") {
        if constant_eq(&t, token) {
            return true;
        }
    }
    if let Some(t) = query_param(query, "token") {
        if constant_eq(&t, token) {
            return true;
        }
    }
    false
}

/// Length-checked, non-short-circuiting compare (avoids trivial timing leaks).
fn constant_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

fn read_body(request: &mut Request) -> Result<String> {
    let mut buf = String::new();
    // Cap the body at 1 MiB — config/secret payloads are tiny.
    request
        .as_reader()
        .take(1_048_576)
        .read_to_string(&mut buf)?;
    Ok(buf)
}

fn respond(request: Request, code: u16, val: Value) -> Result<()> {
    let body = serde_json::to_string(&val).unwrap_or_else(|_| "{}".to_string());
    let response = Response::from_string(body)
        .with_status_code(code)
        .with_header(json_header())
        .with_header(nosniff_header());
    request.respond(response).map_err(|e| anyhow!("{e}"))
}

fn respond_html(request: Request, html: &str) -> Result<()> {
    let response: Response<Cursor<Vec<u8>>> = Response::from_string(html.to_string())
        .with_header(
            Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap(),
        )
        .with_header(nosniff_header());
    request.respond(response).map_err(|e| anyhow!("{e}"))
}

fn json_header() -> Header {
    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap()
}

fn nosniff_header() -> Header {
    Header::from_bytes(&b"X-Content-Type-Options"[..], &b"nosniff"[..]).unwrap()
}

/// Minimal percent-decoding for path/query segments (%XX + '+' → space).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                if let (Some(h), Some(l)) = (hi, lo) {
                    out.push((h * 16 + l) as u8);
                    i += 3;
                    continue;
                }
                out.push(bytes[i]);
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_compare_is_length_and_content_checked() {
        assert!(constant_eq("abc123", "abc123"));
        assert!(!constant_eq("abc123", "abc124"));
        assert!(!constant_eq("abc", "abc123")); // length mismatch
        assert!(!constant_eq("", "x"));
    }

    #[test]
    fn query_parsing_and_decoding() {
        assert_eq!(
            split_query("/api/runs?limit=10&all=1"),
            ("/api/runs", "limit=10&all=1")
        );
        assert_eq!(split_query("/api/status"), ("/api/status", ""));
        assert_eq!(
            query_param("limit=10&token=abc", "token").as_deref(),
            Some("abc")
        );
        assert_eq!(query_param("limit=10", "token"), None);
        assert_eq!(percent_decode("a%20b%2Fc"), "a b/c");
        assert_eq!(percent_decode("name+here"), "name here");
        assert_eq!(percent_decode("plain"), "plain");
    }

    #[test]
    fn config_toml_round_trips() {
        // The save path relies on toml round-tripping a full config incl. grants/budget.
        let cfg = OperatorConfig {
            default_mode: "unsafe".to_string(),
            allow_apps: vec!["Notes".to_string()],
            ..OperatorConfig::default()
        };
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back: OperatorConfig = toml::from_str(&text).unwrap();
        assert_eq!(back.default_mode, "unsafe");
        assert_eq!(back.allow_apps, vec!["Notes".to_string()]);
    }
}
