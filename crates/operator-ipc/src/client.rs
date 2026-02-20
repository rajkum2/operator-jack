use std::io::BufReader;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::thread;
use std::time::Duration;

use crate::framing::{read_ndjson_line, write_ndjson_line};
use crate::protocol::{IpcRequest, IpcResponse};
use crate::IpcError;

/// A client for communicating with the macOS accessibility helper process.
///
/// Spawns the helper binary as a child process, communicates over NDJSON on
/// stdin/stdout, and validates the handshake on connect.
pub struct HelperClient {
    helper_path: Option<String>,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    reader: Option<BufReader<ChildStdout>>,
}

impl HelperClient {
    /// Creates a new helper client. `helper_path` is the optional path to the
    /// macOS helper binary. The binary is not spawned until `connect()` is called.
    pub fn new(helper_path: Option<String>) -> Self {
        Self {
            helper_path,
            child: None,
            stdin: None,
            reader: None,
        }
    }

    /// Spawns the helper process and performs a handshake (ui.ping).
    ///
    /// The handshake validates that the helper responds with `protocol_version == "1"`.
    /// Returns `IpcError::HelperNotFound` if no helper path is configured or the
    /// binary does not exist, `IpcError::ProtocolMismatch` if the version is wrong.
    pub fn connect(&mut self) -> Result<(), IpcError> {
        let path = self.helper_path.as_ref().ok_or_else(|| {
            IpcError::HelperNotFound("No helper path configured".into())
        })?;

        if !std::path::Path::new(path).exists() {
            return Err(IpcError::HelperNotFound(format!(
                "Helper binary not found at: {}",
                path
            )));
        }

        tracing::info!(helper_path = %path, "spawning macOS helper");

        let mut child = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit()) // helper diagnostics go to our stderr
            .spawn()
            .map_err(|e| IpcError::SpawnFailed(format!("{}: {}", path, e)))?;

        let stdin = child.stdin.take().ok_or_else(|| {
            IpcError::SpawnFailed("Failed to capture helper stdin".into())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            IpcError::SpawnFailed("Failed to capture helper stdout".into())
        })?;

        self.child = Some(child);
        self.stdin = Some(stdin);
        self.reader = Some(BufReader::new(stdout));

        // Perform handshake: send ui.ping, validate protocol_version.
        let ping_result = self.send_raw("ui.ping", serde_json::json!({}))?;

        let protocol_version = ping_result
            .get("protocol_version")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if protocol_version != "1" {
            self.disconnect();
            return Err(IpcError::ProtocolMismatch {
                expected: "1".into(),
                got: protocol_version.to_string(),
            });
        }

        tracing::info!(
            helper_version = ping_result.get("helper_version").and_then(|v| v.as_str()).unwrap_or("unknown"),
            "helper connected and handshake passed"
        );

        Ok(())
    }

    /// Returns whether the helper process is currently connected and alive.
    pub fn is_connected(&self) -> bool {
        self.child.is_some()
    }

    /// Sends a request to the helper process and returns the result.
    ///
    /// The `method` should be the snake_case step type string (e.g.
    /// `ui.check_accessibility_permission`); it will be translated to the
    /// camelCase IPC method name before sending.
    pub fn send(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, IpcError> {
        let ipc_method = translate_method_name(method);
        self.send_raw(&ipc_method, params)
    }

    /// Sends a raw IPC request (method name already in IPC format).
    fn send_raw(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, IpcError> {
        // Check if helper process is still alive.
        self.check_alive()?;

        let request = IpcRequest::new(method, params);
        let request_id = request.id.clone();

        let stdin = self.stdin.as_mut().ok_or_else(|| {
            IpcError::HelperCrashed("Helper stdin not available".into())
        })?;
        write_ndjson_line(stdin, &request)?;

        let reader = self.reader.as_mut().ok_or_else(|| {
            IpcError::HelperCrashed("Helper stdout not available".into())
        })?;
        let response: IpcResponse = read_ndjson_line(reader)?;

        // Validate response id matches request id.
        if response.id != request_id {
            return Err(IpcError::InvalidResponse(format!(
                "Response id mismatch: expected {}, got {}",
                request_id, response.id
            )));
        }

        if response.ok {
            Ok(response.result.unwrap_or(serde_json::json!({})))
        } else if let Some(err) = response.error {
            Err(IpcError::HelperError {
                code: err.code,
                message: err.message,
            })
        } else {
            Err(IpcError::InvalidResponse(
                "Response has ok=false but no error payload".into(),
            ))
        }
    }

    /// Checks if the child process is still alive. Returns an error if it has
    /// exited unexpectedly.
    fn check_alive(&mut self) -> Result<(), IpcError> {
        if let Some(ref mut child) = self.child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    // Child has exited — clean up.
                    self.child = None;
                    self.stdin = None;
                    self.reader = None;
                    Err(IpcError::HelperCrashed(format!(
                        "Helper process exited with status: {}",
                        status
                    )))
                }
                Ok(None) => Ok(()), // Still running.
                Err(e) => Err(IpcError::HelperCrashed(format!(
                    "Failed to check helper status: {}",
                    e
                ))),
            }
        } else {
            Err(IpcError::HelperCrashed("Helper process not running".into()))
        }
    }

    /// Disconnects from the helper process. Drops stdin to signal EOF, waits
    /// up to 2 seconds for graceful exit, then kills if still alive.
    pub fn disconnect(&mut self) {
        // Drop stdin to signal EOF to the helper.
        self.stdin = None;
        self.reader = None;

        if let Some(mut child) = self.child.take() {
            // Wait up to 2 seconds for the helper to exit.
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => {
                        tracing::debug!("helper exited gracefully");
                        return;
                    }
                    Ok(None) => {
                        if std::time::Instant::now() >= deadline {
                            break;
                        }
                        thread::sleep(Duration::from_millis(50));
                    }
                    Err(e) => {
                        tracing::warn!("error waiting for helper: {}", e);
                        break;
                    }
                }
            }

            // Still alive after 2s — force kill.
            tracing::warn!("helper did not exit gracefully, killing");
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for HelperClient {
    fn drop(&mut self) {
        self.disconnect();
    }
}

/// Translates a Rust step type string (snake_case) to the IPC method name
/// (camelCase) expected by the Swift helper.
///
/// Most `ui.*` step types use snake_case in the Rust types but camelCase in
/// the IPC protocol. A few (ui.find, ui.click, ui.ping) are identical.
fn translate_method_name(step_type_str: &str) -> String {
    match step_type_str {
        "ui.check_accessibility_permission" => "ui.checkAccessibilityPermission".to_string(),
        "ui.list_apps" => "ui.listApps".to_string(),
        "ui.focus_app" => "ui.focusApp".to_string(),
        "ui.set_value" => "ui.setValue".to_string(),
        "ui.type_text" => "ui.typeText".to_string(),
        "ui.key_press" => "ui.keyPress".to_string(),
        "ui.read_text" => "ui.readText".to_string(),
        "ui.wait_for" => "ui.waitFor".to_string(),
        "ui.select_menu" => "ui.selectMenu".to_string(),
        // These are already the same in both conventions.
        "ui.find" | "ui.click" | "ui.ping" => step_type_str.to_string(),
        // Unknown method — pass through as-is.
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_name_translation() {
        // Snake_case → camelCase
        assert_eq!(
            translate_method_name("ui.check_accessibility_permission"),
            "ui.checkAccessibilityPermission"
        );
        assert_eq!(translate_method_name("ui.list_apps"), "ui.listApps");
        assert_eq!(translate_method_name("ui.focus_app"), "ui.focusApp");
        assert_eq!(translate_method_name("ui.set_value"), "ui.setValue");
        assert_eq!(translate_method_name("ui.type_text"), "ui.typeText");
        assert_eq!(translate_method_name("ui.key_press"), "ui.keyPress");
        assert_eq!(translate_method_name("ui.read_text"), "ui.readText");
        assert_eq!(translate_method_name("ui.wait_for"), "ui.waitFor");
        assert_eq!(translate_method_name("ui.select_menu"), "ui.selectMenu");

        // Already camelCase / no translation needed
        assert_eq!(translate_method_name("ui.find"), "ui.find");
        assert_eq!(translate_method_name("ui.click"), "ui.click");
        assert_eq!(translate_method_name("ui.ping"), "ui.ping");

        // Unknown method — pass through
        assert_eq!(translate_method_name("custom.method"), "custom.method");
    }

    #[test]
    fn test_new_client_not_connected() {
        let client = HelperClient::new(None);
        assert!(!client.is_connected());
    }

    #[test]
    fn test_connect_no_path() {
        let mut client = HelperClient::new(None);
        let result = client.connect();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), IpcError::HelperNotFound(_)));
    }

    #[test]
    fn test_connect_nonexistent_path() {
        let mut client = HelperClient::new(Some("/nonexistent/path/helper".into()));
        let result = client.connect();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), IpcError::HelperNotFound(_)));
    }
}
