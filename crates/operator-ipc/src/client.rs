use crate::IpcError;

/// A client for communicating with the macOS accessibility helper process.
///
/// In M0 (milestone 0), this is a **stub**: the helper is never actually
/// spawned or connected. All calls that require the helper return an error
/// indicating it is not available.
///
/// In M2+, this will spawn the helper binary, negotiate the NDJSON protocol,
/// and relay UI-lane step requests.
pub struct HelperClient {
    helper_path: Option<String>,
    connected: bool,
}

impl HelperClient {
    /// Creates a new helper client. `helper_path` is the optional path to the
    /// macOS helper binary. In M0, the binary is never actually launched.
    pub fn new(helper_path: Option<String>) -> Self {
        Self {
            helper_path,
            connected: false,
        }
    }

    /// Attempts to connect to the helper process.
    ///
    /// In M0, this is a stub. It checks whether the helper binary exists on
    /// disk but does not actually spawn or connect. Always returns an error.
    pub fn connect(&mut self) -> Result<(), IpcError> {
        if let Some(ref path) = self.helper_path {
            if std::path::Path::new(path).exists() {
                tracing::info!(
                    "Helper binary found at {}, but connection not implemented in M0",
                    path
                );
            }
        }
        tracing::warn!("Helper client is stub in M0 - UI steps will fail");
        Err(IpcError::HelperNotFound(
            "macOS helper not available (M0 stub). UI steps require M2+ implementation.".into(),
        ))
    }

    /// Returns whether the helper process is currently connected.
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Sends a request to the helper process and returns the result.
    ///
    /// In M0, this always returns an error because the helper is never
    /// connected.
    pub fn send(
        &mut self,
        method: &str,
        _params: serde_json::Value,
    ) -> Result<serde_json::Value, IpcError> {
        if !self.connected {
            return Err(IpcError::HelperNotFound(format!(
                "Cannot call {} - helper not connected",
                method
            )));
        }
        // M2+ will implement actual NDJSON communication here.
        // In M0 this path is unreachable because `connected` is always false.
        Err(IpcError::HelperNotFound(
            "Helper communication not implemented in M0".into(),
        ))
    }

    /// Disconnects from / kills the helper process.
    ///
    /// In M0, this simply resets the connected flag.
    pub fn disconnect(&mut self) {
        self.connected = false;
        tracing::debug!("Helper client disconnected (stub)");
    }
}
