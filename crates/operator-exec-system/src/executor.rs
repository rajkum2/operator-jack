use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use operator_core::types::StepType;
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum SystemExecError {
    #[error("Unsupported step type for system executor: {0}")]
    UnsupportedStep(String),
    #[error("Missing required parameter: {0}")]
    MissingParam(String),
    #[error("Execution failed: {0}")]
    ExecFailed(String),
    #[error("Timed out: {0}")]
    Timeout(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// 1 MiB capture cap for sys.exec stdout/stderr.
const CAPTURE_CAP: usize = 1_048_576;

// ---------------------------------------------------------------------------
// System step executor
// ---------------------------------------------------------------------------

/// Execute a system-lane step. Returns the output JSON on success.
pub fn execute_system_step(step_type: &StepType, params: &Value) -> Result<Value, SystemExecError> {
    execute_system_step_with_timeout(step_type, params, None)
}

/// Execute a system-lane step, enforcing `exec_timeout_ms` on `sys.exec` (the
/// only step that can run an arbitrary long-lived child). On timeout the child
/// process is killed, rather than being left to run orphaned after the engine's
/// worker thread gives up. `None` means no in-handler deadline.
pub fn execute_system_step_with_timeout(
    step_type: &StepType,
    params: &Value,
    exec_timeout_ms: Option<u64>,
) -> Result<Value, SystemExecError> {
    let start = Instant::now();

    let result = match step_type {
        StepType::SysOpenApp => exec_open_app(params),
        StepType::SysOpenUrl => exec_open_url(params),
        StepType::SysQuitApp => exec_quit_app(params),
        StepType::SysReadFile => exec_read_file(params),
        StepType::SysWriteFile => exec_write_file(params),
        StepType::SysAppendFile => exec_append_file(params),
        StepType::SysMkdir => exec_mkdir(params),
        StepType::SysMovePath => exec_move_path(params),
        StepType::SysCopyPath => exec_copy_path(params),
        StepType::SysDeletePath => exec_delete_path(params),
        StepType::SysExec => exec_command(params, exec_timeout_ms),
        StepType::SysClipboardGet => exec_clipboard_get(),
        StepType::SysClipboardSet => exec_clipboard_set(params),
        StepType::SysListDir => exec_list_dir(params),
        StepType::DataParseCsv => exec_parse_csv(params),
        StepType::DataParseJson => exec_parse_json(params),
        StepType::HttpRequest => exec_http_request(params),
        StepType::ScriptApplescript => exec_osascript(params, "AppleScript", exec_timeout_ms),
        StepType::ScriptJxa => exec_osascript(params, "JavaScript", exec_timeout_ms),
        StepType::ShortcutsRun => exec_shortcuts_run(params, exec_timeout_ms),
        _ => Err(SystemExecError::UnsupportedStep(step_type.to_string())),
    };

    // Inject duration_ms into successful outputs.
    match result {
        Ok(mut output) => {
            let duration_ms = start.elapsed().as_millis() as u64;
            if let Some(obj) = output.as_object_mut() {
                obj.insert("duration_ms".to_string(), json!(duration_ms));
            }
            Ok(output)
        }
        Err(e) => Err(e),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extracts a required string parameter or returns MissingParam error.
fn require_str<'a>(params: &'a Value, key: &str) -> Result<&'a str, SystemExecError> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| SystemExecError::MissingParam(key.to_string()))
}

/// Extracts an optional bool parameter, defaulting to the given value.
fn opt_bool(params: &Value, key: &str, default: bool) -> bool {
    params.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
}

/// Expands a leading `~` to the user's home directory.
fn expand_tilde(path: &str) -> String {
    if path.starts_with("~/") || path == "~" {
        if let Ok(home) = std::env::var("HOME") {
            return path.replacen('~', &home, 1);
        }
    }
    path.to_string()
}

// ---------------------------------------------------------------------------
// Step implementations
// ---------------------------------------------------------------------------

/// sys.open_app — `open -a "AppName"`
fn exec_open_app(params: &Value) -> Result<Value, SystemExecError> {
    let app = require_str(params, "app")?;
    tracing::info!(app = %app, "sys.open_app");

    let output = Command::new("open").args(["-a", app]).output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(SystemExecError::ExecFailed(format!(
            "open -a '{}' failed: {}",
            app,
            stderr.trim()
        )));
    }

    Ok(json!({ "app": app, "launched": true }))
}

/// sys.open_url — `open "url"`
fn exec_open_url(params: &Value) -> Result<Value, SystemExecError> {
    let url = require_str(params, "url")?;
    tracing::info!(url = %url, "sys.open_url");

    let output = Command::new("open").arg(url).output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(SystemExecError::ExecFailed(format!(
            "open '{}' failed: {}",
            url,
            stderr.trim()
        )));
    }

    Ok(json!({ "url": url }))
}

/// sys.quit_app — `osascript -e 'tell application "X" to quit'`
/// With force=true: `kill` the process.
fn exec_quit_app(params: &Value) -> Result<Value, SystemExecError> {
    let app = require_str(params, "app")?;
    let force = opt_bool(params, "force", false);
    tracing::info!(app = %app, force = force, "sys.quit_app");

    if force {
        // Force quit via pkill
        let output = Command::new("pkill").args(["-x", app]).output()?;

        // pkill returns 0 if matched, 1 if no match. Both are acceptable.
        let quit = output.status.success();
        Ok(json!({ "app": app, "quit": quit, "force": true }))
    } else {
        // Graceful quit via AppleScript
        let script = format!("tell application \"{}\" to quit", app);
        let output = Command::new("osascript").args(["-e", &script]).output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(SystemExecError::ExecFailed(format!(
                "quit '{}' failed: {}",
                app,
                stderr.trim()
            )));
        }

        Ok(json!({ "app": app, "quit": true, "force": false }))
    }
}

/// sys.read_file — `std::fs::read_to_string`
fn exec_read_file(params: &Value) -> Result<Value, SystemExecError> {
    let path_str = require_str(params, "path")?;
    let path = expand_tilde(path_str);
    tracing::info!(path = %path, "sys.read_file");

    let content = fs::read_to_string(&path)?;
    let size_bytes = content.len();

    Ok(json!({
        "path": path,
        "content": content,
        "size_bytes": size_bytes,
    }))
}

/// sys.write_file — `std::fs::write`
fn exec_write_file(params: &Value) -> Result<Value, SystemExecError> {
    let path_str = require_str(params, "path")?;
    let content = require_str(params, "content")?;
    let create_parent = opt_bool(params, "create_parent", false);
    let path = expand_tilde(path_str);
    tracing::info!(path = %path, bytes = content.len(), "sys.write_file");

    if create_parent {
        if let Some(parent) = Path::new(&path).parent() {
            fs::create_dir_all(parent)?;
        }
    }

    fs::write(&path, content)?;
    let bytes_written = content.len();

    Ok(json!({
        "path": path,
        "bytes_written": bytes_written,
    }))
}

/// sys.append_file — OpenOptions::append
fn exec_append_file(params: &Value) -> Result<Value, SystemExecError> {
    let path_str = require_str(params, "path")?;
    let content = require_str(params, "content")?;
    let create_parent = opt_bool(params, "create_parent", false);
    let path = expand_tilde(path_str);
    tracing::info!(path = %path, bytes = content.len(), "sys.append_file");

    if create_parent {
        if let Some(parent) = Path::new(&path).parent() {
            fs::create_dir_all(parent)?;
        }
    }

    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    file.write_all(content.as_bytes())?;

    Ok(json!({
        "path": path,
        "bytes_written": content.len(),
    }))
}

/// sys.mkdir — `std::fs::create_dir_all` or `create_dir`
fn exec_mkdir(params: &Value) -> Result<Value, SystemExecError> {
    let path_str = require_str(params, "path")?;
    let parents = opt_bool(params, "parents", true);
    let path = expand_tilde(path_str);
    tracing::info!(path = %path, parents = parents, "sys.mkdir");

    let already_exists = Path::new(&path).is_dir();

    if parents {
        fs::create_dir_all(&path)?;
    } else {
        fs::create_dir(&path)?;
    }

    Ok(json!({
        "path": path,
        "created": !already_exists,
    }))
}

/// sys.move_path — `std::fs::rename`
fn exec_move_path(params: &Value) -> Result<Value, SystemExecError> {
    let from_str = require_str(params, "from")?;
    let to_str = require_str(params, "to")?;
    let overwrite = opt_bool(params, "overwrite", false);
    let from = expand_tilde(from_str);
    let to = expand_tilde(to_str);
    tracing::info!(from = %from, to = %to, "sys.move_path");

    if !overwrite && Path::new(&to).exists() {
        return Err(SystemExecError::ExecFailed(format!(
            "Destination '{}' already exists and overwrite=false",
            to
        )));
    }

    fs::rename(&from, &to)?;

    Ok(json!({ "from": from, "to": to }))
}

/// sys.copy_path — `std::fs::copy` for files, recursive copy for dirs
fn exec_copy_path(params: &Value) -> Result<Value, SystemExecError> {
    let from_str = require_str(params, "from")?;
    let to_str = require_str(params, "to")?;
    let overwrite = opt_bool(params, "overwrite", false);
    let from = expand_tilde(from_str);
    let to = expand_tilde(to_str);
    tracing::info!(from = %from, to = %to, "sys.copy_path");

    if !overwrite && Path::new(&to).exists() {
        return Err(SystemExecError::ExecFailed(format!(
            "Destination '{}' already exists and overwrite=false",
            to
        )));
    }

    let from_path = Path::new(&from);
    if from_path.is_dir() {
        copy_dir_recursive(from_path, Path::new(&to))?;
    } else {
        // Ensure parent directory exists for the destination.
        if let Some(parent) = Path::new(&to).parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::copy(&from, &to)?;
    }

    Ok(json!({ "from": from, "to": to }))
}

/// Recursively copies a directory tree.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), SystemExecError> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let entry_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if entry_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// sys.delete_path — `std::fs::remove_file` or `remove_dir_all`
fn exec_delete_path(params: &Value) -> Result<Value, SystemExecError> {
    let path_str = require_str(params, "path")?;
    let recursive = opt_bool(params, "recursive", false);
    let path = expand_tilde(path_str);
    tracing::info!(path = %path, recursive = recursive, "sys.delete_path");

    let p = Path::new(&path);
    if !p.exists() {
        return Err(SystemExecError::ExecFailed(format!(
            "Path '{}' does not exist",
            path
        )));
    }

    if p.is_dir() {
        if recursive {
            fs::remove_dir_all(&path)?;
        } else {
            // remove_dir only works on empty directories
            fs::remove_dir(&path)?;
        }
    } else {
        fs::remove_file(&path)?;
    }

    Ok(json!({ "path": path, "deleted": true }))
}

/// sys.exec — direct exec via std::process::Command (no shell)
fn exec_command(params: &Value, timeout_ms: Option<u64>) -> Result<Value, SystemExecError> {
    let command = require_str(params, "command")?;
    let args: Vec<String> = params
        .get("args")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    // Default cwd is $HOME, not process cwd.
    let default_home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let cwd = params
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(expand_tilde)
        .unwrap_or(default_home);

    let env_clean = opt_bool(params, "env_clean", false);

    tracing::info!(
        command = %command,
        args = ?args,
        cwd = %cwd,
        env_clean = env_clean,
        "sys.exec"
    );

    let mut cmd = Command::new(command);
    cmd.args(&args);
    cmd.current_dir(&cwd);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    // Environment handling
    if env_clean {
        cmd.env_clear();
    }
    if let Some(env_obj) = params.get("env").and_then(|v| v.as_object()) {
        for (key, val) in env_obj {
            if let Some(val_str) = val.as_str() {
                cmd.env(key, val_str);
            }
        }
    }

    let mut child = cmd.spawn()?;

    // Drain stdout/stderr on separate threads so a chatty child cannot deadlock
    // by filling a pipe buffer while we are waiting for it to exit.
    let mut out_pipe = child.stdout.take();
    let mut err_pipe = child.stderr.take();
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(ref mut p) = out_pipe {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(ref mut p) = err_pipe {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });

    // Wait for exit, enforcing the deadline if one was given. On timeout the
    // child is killed so it does not keep running after the step is abandoned.
    let status = match timeout_ms {
        Some(ms) => {
            let deadline = Instant::now() + Duration::from_millis(ms);
            loop {
                match child.try_wait()? {
                    Some(status) => break status,
                    None => {
                        if Instant::now() >= deadline {
                            let _ = child.kill();
                            let _ = child.wait();
                            // Killing closes the pipes, so the reader threads finish.
                            let _ = out_reader.join();
                            let _ = err_reader.join();
                            return Err(SystemExecError::Timeout(format!(
                                "command '{command}' did not complete within {ms}ms and was killed"
                            )));
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                }
            }
        }
        None => child.wait()?,
    };

    let stdout_vec = out_reader.join().unwrap_or_default();
    let stderr_vec = err_reader.join().unwrap_or_default();
    let exit_code = status.code().unwrap_or(-1);

    let stdout_bytes = stdout_vec.len();
    let stderr_bytes = stderr_vec.len();

    // Apply 1 MiB capture cap.
    let truncated = stdout_bytes > CAPTURE_CAP || stderr_bytes > CAPTURE_CAP;

    let stdout_raw = &stdout_vec[..stdout_bytes.min(CAPTURE_CAP)];
    let stderr_raw = &stderr_vec[..stderr_bytes.min(CAPTURE_CAP)];

    let mut stdout_str = String::from_utf8_lossy(stdout_raw).to_string();
    let mut stderr_str = String::from_utf8_lossy(stderr_raw).to_string();

    if stdout_bytes > CAPTURE_CAP {
        stdout_str.push_str(&format!("\n[TRUNCATED after {} bytes]", CAPTURE_CAP));
    }
    if stderr_bytes > CAPTURE_CAP {
        stderr_str.push_str(&format!("\n[TRUNCATED after {} bytes]", CAPTURE_CAP));
    }

    if !status.success() {
        tracing::warn!(
            command = %command,
            exit_code = exit_code,
            "sys.exec returned non-zero exit code"
        );
    }

    Ok(json!({
        "command": command,
        "exit_code": exit_code,
        "stdout": stdout_str,
        "stderr": stderr_str,
        "stdout_bytes": stdout_bytes,
        "stderr_bytes": stderr_bytes,
        "truncated": truncated,
    }))
}

/// sys.clipboard_get — `pbpaste`
fn exec_clipboard_get() -> Result<Value, SystemExecError> {
    tracing::info!("sys.clipboard_get");

    let output = Command::new("pbpaste").output()?;
    let text = String::from_utf8_lossy(&output.stdout).to_string();

    // If pbpaste returns empty, treat as null clipboard.
    let text_val = if text.is_empty() {
        Value::Null
    } else {
        Value::String(text)
    };

    Ok(json!({
        "text": text_val,
        "types": ["public.utf8-plain-text"],
    }))
}

/// sys.clipboard_set — pipe into `pbcopy`
fn exec_clipboard_set(params: &Value) -> Result<Value, SystemExecError> {
    let text = require_str(params, "text")?;
    tracing::info!(length = text.len(), "sys.clipboard_set");

    let mut child = Command::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes())?;
    }

    let status = child.wait()?;
    if !status.success() {
        return Err(SystemExecError::ExecFailed(
            "pbcopy exited with non-zero status".to_string(),
        ));
    }

    Ok(json!({
        "set": true,
        "length": text.len(),
    }))
}

// ---------------------------------------------------------------------------
// M13: data / network / list_dir handlers
// ---------------------------------------------------------------------------

/// Reads the input for a data.* step from either an inline `text` param or a
/// `path` param (file). Exactly one is expected.
fn read_text_or_file(params: &Value) -> Result<String, SystemExecError> {
    if let Some(text) = params.get("text").and_then(|v| v.as_str()) {
        return Ok(text.to_string());
    }
    if let Some(path) = params.get("path").and_then(|v| v.as_str()) {
        let path = expand_tilde(path);
        return std::fs::read_to_string(&path).map_err(SystemExecError::Io);
    }
    Err(SystemExecError::MissingParam("text or path".to_string()))
}

/// `sys.list_dir`: list a directory, optionally filtered by a glob `pattern`
/// (matched against entry names), with per-entry metadata.
fn exec_list_dir(params: &Value) -> Result<Value, SystemExecError> {
    let path = expand_tilde(require_str(params, "path")?);
    let pattern = params
        .get("pattern")
        .and_then(|v| v.as_str())
        .map(glob::Pattern::new)
        .transpose()
        .map_err(|e| SystemExecError::ExecFailed(format!("invalid glob pattern: {e}")))?;

    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&path)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(ref pat) = pattern {
            if !pat.matches(&name) {
                continue;
            }
        }
        let meta = entry.metadata().ok();
        let (is_dir, size, modified) = match meta {
            Some(m) => {
                let modified = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs());
                (m.is_dir(), m.len(), modified)
            }
            None => (false, 0, None),
        };
        entries.push(json!({
            "name": name,
            "path": entry.path().to_string_lossy(),
            "is_dir": is_dir,
            "size": size,
            "modified_epoch": modified,
        }));
    }
    // Deterministic order (readdir order is unspecified).
    entries.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });
    tracing::info!(path = %path, count = entries.len(), "sys.list_dir");
    Ok(json!({ "count": entries.len(), "entries": entries }))
}

/// `data.parse_csv`: parse CSV → array of row objects (keyed by header) when
/// `headers` (default true), else array of arrays. Handles quoted fields,
/// embedded commas/quotes ("" escape), and CRLF.
fn exec_parse_csv(params: &Value) -> Result<Value, SystemExecError> {
    let text = read_text_or_file(params)?;
    let with_headers = params
        .get("headers")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let rows = parse_csv(&text);
    if rows.is_empty() {
        return Ok(json!({ "count": 0, "rows": [] }));
    }

    let out = if with_headers {
        let header = &rows[0];
        let records: Vec<Value> = rows[1..]
            .iter()
            .map(|row| {
                let obj: serde_json::Map<String, Value> = header
                    .iter()
                    .enumerate()
                    .map(|(i, key)| {
                        (
                            key.clone(),
                            Value::String(row.get(i).cloned().unwrap_or_default()),
                        )
                    })
                    .collect();
                Value::Object(obj)
            })
            .collect();
        json!({ "count": records.len(), "headers": header, "rows": records })
    } else {
        let records: Vec<Value> = rows
            .iter()
            .map(|row| Value::Array(row.iter().map(|c| Value::String(c.clone())).collect()))
            .collect();
        json!({ "count": records.len(), "rows": records })
    };
    Ok(out)
}

/// A small, dependency-free RFC-4180-ish CSV parser (quotes, "" escape, CRLF).
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => in_quotes = false,
                _ => field.push(c),
            }
        } else {
            match c {
                '"' => in_quotes = true,
                ',' => {
                    row.push(std::mem::take(&mut field));
                }
                '\r' => { /* swallow; \n ends the row */ }
                '\n' => {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                }
                _ => field.push(c),
            }
        }
    }
    // Final field/row if the text doesn't end in a newline.
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    // Drop a trailing empty row (e.g. file ending in a newline).
    if rows
        .last()
        .map(|r| r.len() == 1 && r[0].is_empty())
        .unwrap_or(false)
    {
        rows.pop();
    }
    rows
}

/// `data.parse_json`: parse JSON, optionally selecting a sub-value via a JSON
/// pointer (RFC 6901, e.g. "/results/0/name").
fn exec_parse_json(params: &Value) -> Result<Value, SystemExecError> {
    let text = read_text_or_file(params)?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|e| SystemExecError::ExecFailed(format!("invalid JSON: {e}")))?;
    let selected = match params.get("pointer").and_then(|v| v.as_str()) {
        Some(ptr) if !ptr.is_empty() => value
            .pointer(ptr)
            .cloned()
            .ok_or_else(|| SystemExecError::ExecFailed(format!("JSON pointer not found: {ptr}")))?,
        _ => value,
    };
    Ok(json!({ "value": selected }))
}

/// `http.request`: a blocking HTTP request. Domain gating is enforced upstream by
/// the policy gate (this handler assumes the URL is already allowed).
fn exec_http_request(params: &Value) -> Result<Value, SystemExecError> {
    let url = require_str(params, "url")?;
    let method = params
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("GET")
        .to_uppercase();

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("operator-jack")
        .build()
        .map_err(|e| SystemExecError::ExecFailed(format!("http client: {e}")))?;

    let m = reqwest::Method::from_bytes(method.as_bytes())
        .map_err(|e| SystemExecError::ExecFailed(format!("bad method: {e}")))?;
    let mut req = client.request(m, url);

    if let Some(headers) = params.get("headers").and_then(|v| v.as_object()) {
        for (k, v) in headers {
            if let Some(vs) = v.as_str() {
                req = req.header(k.as_str(), vs);
            }
        }
    }
    if let Some(body) = params.get("body") {
        match body {
            Value::String(s) => req = req.body(s.clone()),
            other => req = req.json(other),
        }
    }

    let resp = req
        .send()
        .map_err(|e| SystemExecError::ExecFailed(format!("http request failed: {e}")))?;
    let status = resp.status().as_u16();
    let resp_headers: serde_json::Map<String, Value> = resp
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.to_string(),
                Value::String(v.to_str().unwrap_or("").to_string()),
            )
        })
        .collect();
    // Cap the captured body to avoid unbounded memory / audit bloat.
    let text = resp.text().unwrap_or_default();
    let capped: String = text.chars().take(CAPTURE_CAP).collect();
    tracing::info!(url = %url, status, "http.request");
    Ok(json!({
        "status": status,
        "ok": (200..300).contains(&status),
        "headers": resp_headers,
        "body": capped,
        "truncated": text.len() > capped.len(),
    }))
}

// ---------------------------------------------------------------------------
// M14: script lane (AppleScript / JXA / Shortcuts) — via osascript/shortcuts
// ---------------------------------------------------------------------------

/// `script.applescript` / `script.jxa`: run a script through `osascript`,
/// reusing `exec_command`'s timeout + output capture. `language` is
/// "AppleScript" or "JavaScript" (JXA). The script body is passed as a single
/// `-e` argument (multi-line scripts are fine).
fn exec_osascript(
    params: &Value,
    language: &str,
    timeout_ms: Option<u64>,
) -> Result<Value, SystemExecError> {
    let script = require_str(params, "script")?;
    tracing::info!(language = %language, "script.{}", language.to_lowercase());
    let exec_params = json!({
        "command": "osascript",
        "args": ["-l", language, "-e", script],
    });
    let mut out = exec_command(&exec_params, timeout_ms)?;
    if let Some(obj) = out.as_object_mut() {
        obj.insert("language".to_string(), json!(language));
    }
    Ok(out)
}

/// `shortcuts.run`: run a macOS Shortcut by name via the `shortcuts` CLI. An
/// `input_path` (file) is passed through if given; inline text input is out of
/// scope (most shortcuts take input from the clipboard/selection).
fn exec_shortcuts_run(params: &Value, timeout_ms: Option<u64>) -> Result<Value, SystemExecError> {
    let name = require_str(params, "name")?;
    tracing::info!(shortcut = %name, "shortcuts.run");
    let mut args = vec![json!("run"), json!(name)];
    if let Some(input_path) = params.get("input_path").and_then(|v| v.as_str()) {
        args.push(json!("--input-path"));
        args.push(json!(expand_tilde(input_path)));
    }
    let exec_params = json!({ "command": "shortcuts", "args": args });
    exec_command(&exec_params, timeout_ms)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Helper to create a temp directory for tests.
    fn test_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("operator_test_{}", ulid::Ulid::new()));
        fs::create_dir_all(&dir).expect("create test dir");
        dir
    }

    #[test]
    fn test_parse_csv_quoting_and_headers() {
        // Quoted field with an embedded comma, and a "" escaped quote.
        let text = "name,note\nAlpha,\"a, b\"\nBeta,\"say \"\"hi\"\"\"\n";
        let params = json!({ "text": text });
        let out = exec_parse_csv(&params).unwrap();
        assert_eq!(out["count"], 2);
        assert_eq!(out["rows"][0]["name"], "Alpha");
        assert_eq!(out["rows"][0]["note"], "a, b");
        assert_eq!(out["rows"][1]["note"], "say \"hi\"");
    }

    #[test]
    fn test_parse_json_pointer() {
        let params = json!({ "text": "{\"a\":{\"b\":[10,20]}}", "pointer": "/a/b/1" });
        let out = exec_parse_json(&params).unwrap();
        assert_eq!(out["value"], 20);
    }

    #[test]
    fn test_read_file() {
        let dir = test_dir();
        let file = dir.join("test.txt");
        fs::write(&file, "hello world").unwrap();

        let params = json!({ "path": file.to_str().unwrap() });
        let result = exec_read_file(&params).unwrap();

        assert_eq!(result["content"], "hello world");
        assert_eq!(result["size_bytes"], 11);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_read_file_not_found() {
        let params = json!({ "path": "/tmp/nonexistent_operator_test_file_xyz" });
        let result = exec_read_file(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_write_file() {
        let dir = test_dir();
        let file = dir.join("output.txt");

        let params = json!({
            "path": file.to_str().unwrap(),
            "content": "test content"
        });
        let result = exec_write_file(&params).unwrap();

        assert_eq!(result["bytes_written"], 12);
        assert_eq!(fs::read_to_string(&file).unwrap(), "test content");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_write_file_create_parent() {
        let dir = test_dir();
        let file = dir.join("sub").join("deep").join("file.txt");

        let params = json!({
            "path": file.to_str().unwrap(),
            "content": "nested",
            "create_parent": true
        });
        let result = exec_write_file(&params).unwrap();

        assert_eq!(result["bytes_written"], 6);
        assert_eq!(fs::read_to_string(&file).unwrap(), "nested");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_append_file() {
        let dir = test_dir();
        let file = dir.join("append.txt");
        fs::write(&file, "first").unwrap();

        let params = json!({
            "path": file.to_str().unwrap(),
            "content": " second"
        });
        exec_append_file(&params).unwrap();

        assert_eq!(fs::read_to_string(&file).unwrap(), "first second");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_mkdir() {
        let dir = test_dir();
        let new_dir = dir.join("a").join("b").join("c");

        let params = json!({
            "path": new_dir.to_str().unwrap(),
            "parents": true
        });
        let result = exec_mkdir(&params).unwrap();

        assert_eq!(result["created"], true);
        assert!(new_dir.is_dir());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_move_path() {
        let dir = test_dir();
        let src = dir.join("src.txt");
        let dst = dir.join("dst.txt");
        fs::write(&src, "move me").unwrap();

        let params = json!({
            "from": src.to_str().unwrap(),
            "to": dst.to_str().unwrap()
        });
        exec_move_path(&params).unwrap();

        assert!(!src.exists());
        assert_eq!(fs::read_to_string(&dst).unwrap(), "move me");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_move_path_no_overwrite() {
        let dir = test_dir();
        let src = dir.join("src.txt");
        let dst = dir.join("dst.txt");
        fs::write(&src, "src").unwrap();
        fs::write(&dst, "dst").unwrap();

        let params = json!({
            "from": src.to_str().unwrap(),
            "to": dst.to_str().unwrap(),
            "overwrite": false
        });
        let result = exec_move_path(&params);
        assert!(result.is_err());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_copy_path_file() {
        let dir = test_dir();
        let src = dir.join("orig.txt");
        let dst = dir.join("copy.txt");
        fs::write(&src, "copy me").unwrap();

        let params = json!({
            "from": src.to_str().unwrap(),
            "to": dst.to_str().unwrap()
        });
        exec_copy_path(&params).unwrap();

        assert!(src.exists()); // source still exists
        assert_eq!(fs::read_to_string(&dst).unwrap(), "copy me");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_copy_path_dir() {
        let dir = test_dir();
        let src_dir = dir.join("src_dir");
        fs::create_dir_all(src_dir.join("sub")).unwrap();
        fs::write(src_dir.join("a.txt"), "aaa").unwrap();
        fs::write(src_dir.join("sub").join("b.txt"), "bbb").unwrap();

        let dst_dir = dir.join("dst_dir");

        let params = json!({
            "from": src_dir.to_str().unwrap(),
            "to": dst_dir.to_str().unwrap()
        });
        exec_copy_path(&params).unwrap();

        assert_eq!(fs::read_to_string(dst_dir.join("a.txt")).unwrap(), "aaa");
        assert_eq!(
            fs::read_to_string(dst_dir.join("sub").join("b.txt")).unwrap(),
            "bbb"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_delete_file() {
        let dir = test_dir();
        let file = dir.join("delete_me.txt");
        fs::write(&file, "bye").unwrap();

        let params = json!({ "path": file.to_str().unwrap() });
        let result = exec_delete_path(&params).unwrap();

        assert_eq!(result["deleted"], true);
        assert!(!file.exists());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_delete_dir_recursive() {
        let dir = test_dir();
        let sub = dir.join("to_delete");
        fs::create_dir_all(sub.join("inner")).unwrap();
        fs::write(sub.join("inner").join("file.txt"), "x").unwrap();

        let params = json!({
            "path": sub.to_str().unwrap(),
            "recursive": true
        });
        let result = exec_delete_path(&params).unwrap();

        assert_eq!(result["deleted"], true);
        assert!(!sub.exists());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_delete_nonempty_dir_fails_without_recursive() {
        let dir = test_dir();
        let sub = dir.join("nonempty");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("file.txt"), "x").unwrap();

        let params = json!({
            "path": sub.to_str().unwrap(),
            "recursive": false
        });
        let result = exec_delete_path(&params);
        assert!(result.is_err());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_exec_echo() {
        let params = json!({
            "command": "echo",
            "args": ["hello", "world"]
        });
        let result = exec_command(&params, None).unwrap();

        assert_eq!(result["exit_code"], 0);
        assert_eq!(result["stdout"].as_str().unwrap().trim(), "hello world");
        assert_eq!(result["truncated"], false);
    }

    #[test]
    fn test_exec_with_cwd() {
        let params = json!({
            "command": "pwd",
            "cwd": "/tmp"
        });
        let result = exec_command(&params, None).unwrap();

        // /tmp may resolve to /private/tmp on macOS
        let stdout = result["stdout"].as_str().unwrap().trim();
        assert!(
            stdout == "/tmp" || stdout == "/private/tmp",
            "unexpected pwd: {}",
            stdout
        );
    }

    #[test]
    fn test_exec_with_env() {
        let params = json!({
            "command": "sh",
            "args": ["-c", "echo $MY_TEST_VAR"],
            "env": { "MY_TEST_VAR": "operator_test_value" }
        });
        let result = exec_command(&params, None).unwrap();

        assert_eq!(
            result["stdout"].as_str().unwrap().trim(),
            "operator_test_value"
        );
    }

    #[test]
    fn test_exec_env_clean() {
        let params = json!({
            "command": "env",
            "env_clean": true,
            "env": { "ONLY_THIS": "yes" }
        });
        let result = exec_command(&params, None).unwrap();

        let stdout = result["stdout"].as_str().unwrap();
        assert!(stdout.contains("ONLY_THIS=yes"));
        // In a clean env, PATH is not set so very few vars
        assert!(!stdout.contains("HOME="));
    }

    #[test]
    fn test_exec_nonzero_exit() {
        let params = json!({
            "command": "sh",
            "args": ["-c", "exit 42"]
        });
        let result = exec_command(&params, None).unwrap();

        assert_eq!(result["exit_code"], 42);
    }

    #[test]
    fn test_exec_times_out_and_kills_child() {
        // A 10s sleep with a 300ms deadline must return a Timeout error promptly
        // (the child is killed), not block for the full sleep duration.
        let params = json!({ "command": "sleep", "args": ["10"] });
        let start = Instant::now();
        let result = exec_command(&params, Some(300));
        let elapsed = start.elapsed();

        assert!(
            matches!(result, Err(SystemExecError::Timeout(_))),
            "expected Timeout, got {result:?}"
        );
        assert!(
            elapsed < Duration::from_secs(3),
            "should return near the 300ms deadline, took {elapsed:?}"
        );
    }

    #[test]
    fn test_clipboard_roundtrip() {
        let test_text = "operator_clipboard_test_value";

        // Set clipboard
        let set_params = json!({ "text": test_text });
        let set_result = exec_clipboard_set(&set_params).unwrap();
        assert_eq!(set_result["set"], true);
        assert_eq!(set_result["length"], json!(test_text.len()));

        // Get clipboard
        let get_result = exec_clipboard_get().unwrap();
        assert_eq!(get_result["text"].as_str().unwrap(), test_text);
    }

    #[test]
    fn test_missing_param_error() {
        let params = json!({});
        let result = exec_read_file(&params);
        assert!(matches!(result, Err(SystemExecError::MissingParam(_))));
    }

    #[test]
    fn test_expand_tilde() {
        let expanded = expand_tilde("~/Documents");
        assert!(!expanded.starts_with('~'));
        assert!(expanded.contains("Documents"));

        // Non-tilde path unchanged
        assert_eq!(expand_tilde("/tmp/foo"), "/tmp/foo");
    }
}
