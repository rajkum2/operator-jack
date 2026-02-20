use operator_core::types::StepType;
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum SystemExecError {
    #[error("Unsupported step type for system executor: {0}")]
    UnsupportedStep(String),
    #[error("Execution failed: {0}")]
    ExecFailed(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

// ---------------------------------------------------------------------------
// System step executor
// ---------------------------------------------------------------------------

/// Execute a system-lane step. Returns the output JSON on success.
///
/// In M0, **all system steps are stubs** that log what they would do and return
/// synthetic success results. The real implementations will replace each match
/// arm in M1.
pub fn execute_system_step(
    step_type: &StepType,
    params: &Value,
) -> Result<Value, SystemExecError> {
    match step_type {
        StepType::SysOpenApp => {
            let app = params
                .get("app")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            tracing::info!("[STUB] sys.open_app: {}", app);
            Ok(json!({ "app": app, "launched": true }))
        }
        StepType::SysOpenUrl => {
            let url = params
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            tracing::info!("[STUB] sys.open_url: {}", url);
            Ok(json!({ "url": url }))
        }
        StepType::SysReadFile => {
            let path = params
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            tracing::info!("[STUB] sys.read_file: {}", path);
            Ok(json!({ "path": path, "content": "[stub content]", "size_bytes": 14 }))
        }
        StepType::SysWriteFile => {
            let path = params
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            tracing::info!("[STUB] sys.write_file: {}", path);
            Ok(json!({ "path": path, "bytes_written": 0 }))
        }
        StepType::SysAppendFile => {
            let path = params
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            tracing::info!("[STUB] sys.append_file: {}", path);
            Ok(json!({ "path": path, "bytes_written": 0 }))
        }
        StepType::SysMkdir => {
            let path = params
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            tracing::info!("[STUB] sys.mkdir: {}", path);
            Ok(json!({ "path": path, "created": true }))
        }
        StepType::SysMovePath => {
            let from = params
                .get("from")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let to = params
                .get("to")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            tracing::info!("[STUB] sys.move_path: {} -> {}", from, to);
            Ok(json!({ "from": from, "to": to }))
        }
        StepType::SysCopyPath => {
            let from = params
                .get("from")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let to = params
                .get("to")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            tracing::info!("[STUB] sys.copy_path: {} -> {}", from, to);
            Ok(json!({ "from": from, "to": to }))
        }
        StepType::SysDeletePath => {
            let path = params
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            tracing::info!("[STUB] sys.delete_path: {}", path);
            Ok(json!({ "path": path, "deleted": true }))
        }
        StepType::SysExec => {
            let cmd = params
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            tracing::info!("[STUB] sys.exec: {}", cmd);
            Ok(json!({
                "command": cmd,
                "exit_code": 0,
                "stdout": "",
                "stderr": "",
                "stdout_bytes": 0,
                "stderr_bytes": 0,
                "truncated": false
            }))
        }
        StepType::SysQuitApp => {
            let app = params
                .get("app")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            tracing::info!("[STUB] sys.quit_app: {}", app);
            Ok(json!({ "app": app, "quit": true }))
        }
        StepType::SysClipboardGet => {
            tracing::info!("[STUB] sys.clipboard_get");
            Ok(json!({ "text": null, "types": [] }))
        }
        StepType::SysClipboardSet => {
            let text = params
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            tracing::info!("[STUB] sys.clipboard_set: {} chars", text.len());
            Ok(json!({ "set": true, "length": text.len() }))
        }
        _ => Err(SystemExecError::UnsupportedStep(step_type.to_string())),
    }
}
