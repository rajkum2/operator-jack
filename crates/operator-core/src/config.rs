//! Configuration file loading for Operator Jack.
//!
//! Loads settings from `~/.config/operator-jack/config.toml` with overrides
//! from environment variables. CLI flags take highest precedence (handled
//! in operator-cli).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// All settings that can appear in `config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OperatorConfig {
    /// Path to the macOS helper binary.
    pub helper_path: Option<String>,

    /// Default execution mode: "safe" or "unsafe".
    pub default_mode: String,

    /// Whether to default to interactive mode.
    pub interactive: bool,

    /// Default per-step timeout in milliseconds.
    pub default_step_timeout_ms: u64,

    /// Default retry count for failed steps.
    pub default_retries: u32,

    /// Default retry backoff in milliseconds.
    pub default_retry_backoff_ms: u64,

    /// Default application allowlist.
    #[serde(default)]
    pub allow_apps: Vec<String>,

    /// Default domain allowlist.
    #[serde(default)]
    pub allow_domains: Vec<String>,

    /// Custom log directory.
    pub log_dir: Option<String>,

    /// Custom database path.
    pub db_path: Option<String>,
}

impl Default for OperatorConfig {
    fn default() -> Self {
        Self {
            helper_path: None,
            default_mode: "safe".to_string(),
            interactive: true,
            default_step_timeout_ms: 30_000,
            default_retries: 0,
            default_retry_backoff_ms: 1_000,
            allow_apps: Vec::new(),
            allow_domains: Vec::new(),
            log_dir: None,
            db_path: None,
        }
    }
}

impl OperatorConfig {
    /// Returns the default config file path: `~/.config/operator-jack/config.toml`.
    pub fn default_path() -> Option<PathBuf> {
        dirs_config_dir().map(|d| d.join("operator-jack").join("config.toml"))
    }

    /// Loads config from the given path. Returns `Ok(default)` if the file
    /// does not exist; returns `Err` only on parse failures.
    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::ReadFailed(path.display().to_string(), e.to_string()))?;
        let config: Self = toml::from_str(&contents)
            .map_err(|e| ConfigError::ParseFailed(path.display().to_string(), e.to_string()))?;
        Ok(config)
    }

    /// Loads config from the default path, or env var `OPERATOR_CONFIG_PATH`.
    pub fn load() -> Result<Self, ConfigError> {
        let path = if let Ok(p) = std::env::var("OPERATOR_CONFIG_PATH") {
            PathBuf::from(p)
        } else if let Some(p) = Self::default_path() {
            p
        } else {
            return Ok(Self::default());
        };
        let mut config = Self::load_from(&path)?;
        config.apply_env_overrides();
        Ok(config)
    }

    /// Apply environment variable overrides (second precedence after file,
    /// before CLI flags).
    pub fn apply_env_overrides(&mut self) {
        if let Ok(v) = std::env::var("OPERATOR_HELPER_PATH") {
            self.helper_path = Some(v);
        }
        if let Ok(v) = std::env::var("OPERATOR_MODE") {
            self.default_mode = v;
        }
        if let Ok(v) = std::env::var("OPERATOR_DB_PATH") {
            self.db_path = Some(v);
        }
        if let Ok(v) = std::env::var("OPERATOR_LOG_DIR") {
            self.log_dir = Some(v);
        }
        if let Ok(v) = std::env::var("OPERATOR_TIMEOUT_MS") {
            if let Ok(n) = v.parse() {
                self.default_step_timeout_ms = n;
            }
        }
    }

    /// Generates the default config file content with comments.
    pub fn default_toml() -> &'static str {
        r#"# Operator Jack configuration
# Location: ~/.config/operator-jack/config.toml

# Path to the macOS helper binary (auto-detected if omitted)
# helper_path = "/usr/local/bin/operator-macos-helper"

# Default execution mode: "safe" or "unsafe"
default_mode = "safe"

# Enable interactive prompts (disable for CI/scripted use)
interactive = true

# Default per-step timeout in milliseconds
default_step_timeout_ms = 30000

# Default retry count for failed steps
default_retries = 0

# Default retry backoff in milliseconds
default_retry_backoff_ms = 1000

# Default application allowlist (empty = allow all)
# allow_apps = ["TextEdit", "Notes", "Calculator"]

# Default domain allowlist (empty = allow all)
# allow_domains = ["github.com", "google.com"]

# Custom log directory (default: ~/Library/Application Support/operator-jack/logs/)
# log_dir = "/path/to/logs"

# Custom database path (default: ~/Library/Application Support/operator-jack/operator-jack.db)
# db_path = "/path/to/operator-jack.db"
"#
    }
}

/// Config file system directory (XDG on Linux, ~/Library/Preferences on macOS).
fn dirs_config_dir() -> Option<PathBuf> {
    // On macOS: ~/Library/Application Support (via dirs::config_dir)
    // We use ~/.config/ for cross-platform consistency.
    dirs::home_dir().map(|h| h.join(".config"))
}

/// Errors from config file loading.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config file '{0}': {1}")]
    ReadFailed(String, String),
    #[error("failed to parse config file '{0}': {1}")]
    ParseFailed(String, String),
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_safe_mode() {
        let cfg = OperatorConfig::default();
        assert_eq!(cfg.default_mode, "safe");
        assert_eq!(cfg.default_step_timeout_ms, 30_000);
        assert_eq!(cfg.default_retries, 0);
        assert!(cfg.allow_apps.is_empty());
    }

    #[test]
    fn parse_minimal_toml() {
        let toml_str = r#"
            default_mode = "unsafe"
            default_step_timeout_ms = 5000
        "#;
        let cfg: OperatorConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.default_mode, "unsafe");
        assert_eq!(cfg.default_step_timeout_ms, 5000);
        // Defaults for omitted fields
        assert_eq!(cfg.default_retries, 0);
        assert!(cfg.helper_path.is_none());
    }

    #[test]
    fn parse_full_toml() {
        let toml_str = r#"
            helper_path = "/usr/local/bin/operator-macos-helper"
            default_mode = "unsafe"
            interactive = false
            default_step_timeout_ms = 10000
            default_retries = 3
            default_retry_backoff_ms = 2000
            allow_apps = ["TextEdit", "Notes"]
            allow_domains = ["github.com"]
            log_dir = "/tmp/logs"
            db_path = "/tmp/operator.db"
        "#;
        let cfg: OperatorConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(
            cfg.helper_path.as_deref(),
            Some("/usr/local/bin/operator-macos-helper")
        );
        assert_eq!(cfg.default_mode, "unsafe");
        assert!(!cfg.interactive);
        assert_eq!(cfg.default_step_timeout_ms, 10_000);
        assert_eq!(cfg.default_retries, 3);
        assert_eq!(cfg.allow_apps, vec!["TextEdit", "Notes"]);
        assert_eq!(cfg.allow_domains, vec!["github.com"]);
    }

    #[test]
    fn load_nonexistent_file_returns_default() {
        let cfg = OperatorConfig::load_from(Path::new("/nonexistent/config.toml")).unwrap();
        assert_eq!(cfg.default_mode, "safe");
    }

    #[test]
    fn default_toml_is_valid() {
        // The default template should parse successfully
        let _: OperatorConfig = toml::from_str(OperatorConfig::default_toml()).unwrap();
    }
}
