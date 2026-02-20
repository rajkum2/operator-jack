use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

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
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// 1 MiB capture cap for sys.exec stdout/stderr.
const CAPTURE_CAP: usize = 1_048_576;

// ---------------------------------------------------------------------------
// System step executor
// ---------------------------------------------------------------------------

/// Execute a system-lane step. Returns the output JSON on success.
pub fn execute_system_step(
    step_type: &StepType,
    params: &Value,
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
        StepType::SysExec => exec_command(params),
        StepType::SysClipboardGet => exec_clipboard_get(),
        StepType::SysClipboardSet => exec_clipboard_set(params),
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
    params
        .get(key)
        .and_then(|v| v.as_bool())
        .unwrap_or(default)
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

    let output = Command::new("open")
        .args(["-a", app])
        .output()?;

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

    let output = Command::new("open")
        .arg(url)
        .output()?;

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
        let output = Command::new("pkill")
            .args(["-x", app])
            .output()?;

        // pkill returns 0 if matched, 1 if no match. Both are acceptable.
        let quit = output.status.success();
        Ok(json!({ "app": app, "quit": quit, "force": true }))
    } else {
        // Graceful quit via AppleScript
        let script = format!("tell application \"{}\" to quit", app);
        let output = Command::new("osascript")
            .args(["-e", &script])
            .output()?;

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

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
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
fn exec_command(params: &Value) -> Result<Value, SystemExecError> {
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
        .map(|s| expand_tilde(s))
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

    let output = cmd.output()?;
    let exit_code = output.status.code().unwrap_or(-1);

    let stdout_bytes = output.stdout.len();
    let stderr_bytes = output.stderr.len();

    // Apply 1 MiB capture cap.
    let truncated = stdout_bytes > CAPTURE_CAP || stderr_bytes > CAPTURE_CAP;

    let stdout_raw = &output.stdout[..stdout_bytes.min(CAPTURE_CAP)];
    let stderr_raw = &output.stderr[..stderr_bytes.min(CAPTURE_CAP)];

    let mut stdout_str = String::from_utf8_lossy(stdout_raw).to_string();
    let mut stderr_str = String::from_utf8_lossy(stderr_raw).to_string();

    if stdout_bytes > CAPTURE_CAP {
        stdout_str.push_str(&format!("\n[TRUNCATED after {} bytes]", CAPTURE_CAP));
    }
    if stderr_bytes > CAPTURE_CAP {
        stderr_str.push_str(&format!("\n[TRUNCATED after {} bytes]", CAPTURE_CAP));
    }

    if !output.status.success() {
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
        let result = exec_command(&params).unwrap();

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
        let result = exec_command(&params).unwrap();

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
        let result = exec_command(&params).unwrap();

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
        let result = exec_command(&params).unwrap();

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
        let result = exec_command(&params).unwrap();

        assert_eq!(result["exit_code"], 42);
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
