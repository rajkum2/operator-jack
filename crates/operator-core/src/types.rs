use std::collections::HashMap;
use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// StepType
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StepType {
    // System lane
    #[serde(rename = "sys.open_app")]
    SysOpenApp,
    #[serde(rename = "sys.open_url")]
    SysOpenUrl,
    #[serde(rename = "sys.read_file")]
    SysReadFile,
    #[serde(rename = "sys.write_file")]
    SysWriteFile,
    #[serde(rename = "sys.append_file")]
    SysAppendFile,
    #[serde(rename = "sys.mkdir")]
    SysMkdir,
    #[serde(rename = "sys.move_path")]
    SysMovePath,
    #[serde(rename = "sys.copy_path")]
    SysCopyPath,
    #[serde(rename = "sys.delete_path")]
    SysDeletePath,
    #[serde(rename = "sys.exec")]
    SysExec,
    #[serde(rename = "sys.quit_app")]
    SysQuitApp,
    #[serde(rename = "sys.clipboard_get")]
    SysClipboardGet,
    #[serde(rename = "sys.clipboard_set")]
    SysClipboardSet,

    // UI lane
    #[serde(rename = "ui.check_accessibility_permission")]
    UiCheckAccessibilityPermission,
    #[serde(rename = "ui.list_apps")]
    UiListApps,
    #[serde(rename = "ui.focus_app")]
    UiFocusApp,
    #[serde(rename = "ui.find")]
    UiFind,
    #[serde(rename = "ui.click")]
    UiClick,
    #[serde(rename = "ui.set_value")]
    UiSetValue,
    #[serde(rename = "ui.type_text")]
    UiTypeText,
    #[serde(rename = "ui.key_press")]
    UiKeyPress,
    #[serde(rename = "ui.read_text")]
    UiReadText,
    #[serde(rename = "ui.wait_for")]
    UiWaitFor,
    #[serde(rename = "ui.select_menu")]
    UiSelectMenu,
}

impl StepType {
    /// Returns the lane this step type belongs to: "system" or "ui".
    pub fn lane(&self) -> &str {
        match self {
            StepType::SysOpenApp
            | StepType::SysOpenUrl
            | StepType::SysReadFile
            | StepType::SysWriteFile
            | StepType::SysAppendFile
            | StepType::SysMkdir
            | StepType::SysMovePath
            | StepType::SysCopyPath
            | StepType::SysDeletePath
            | StepType::SysExec
            | StepType::SysQuitApp
            | StepType::SysClipboardGet
            | StepType::SysClipboardSet => "system",

            StepType::UiCheckAccessibilityPermission
            | StepType::UiListApps
            | StepType::UiFocusApp
            | StepType::UiFind
            | StepType::UiClick
            | StepType::UiSetValue
            | StepType::UiTypeText
            | StepType::UiKeyPress
            | StepType::UiReadText
            | StepType::UiWaitFor
            | StepType::UiSelectMenu => "ui",
        }
    }

    /// Returns the serde serialized name (e.g. "sys.open_app").
    pub fn as_str(&self) -> &'static str {
        match self {
            StepType::SysOpenApp => "sys.open_app",
            StepType::SysOpenUrl => "sys.open_url",
            StepType::SysReadFile => "sys.read_file",
            StepType::SysWriteFile => "sys.write_file",
            StepType::SysAppendFile => "sys.append_file",
            StepType::SysMkdir => "sys.mkdir",
            StepType::SysMovePath => "sys.move_path",
            StepType::SysCopyPath => "sys.copy_path",
            StepType::SysDeletePath => "sys.delete_path",
            StepType::SysExec => "sys.exec",
            StepType::SysQuitApp => "sys.quit_app",
            StepType::SysClipboardGet => "sys.clipboard_get",
            StepType::SysClipboardSet => "sys.clipboard_set",
            StepType::UiCheckAccessibilityPermission => "ui.check_accessibility_permission",
            StepType::UiListApps => "ui.list_apps",
            StepType::UiFocusApp => "ui.focus_app",
            StepType::UiFind => "ui.find",
            StepType::UiClick => "ui.click",
            StepType::UiSetValue => "ui.set_value",
            StepType::UiTypeText => "ui.type_text",
            StepType::UiKeyPress => "ui.key_press",
            StepType::UiReadText => "ui.read_text",
            StepType::UiWaitFor => "ui.wait_for",
            StepType::UiSelectMenu => "ui.select_menu",
        }
    }
}

impl fmt::Display for StepType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Mode
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Safe,
    Unsafe,
}

// ---------------------------------------------------------------------------
// OnFail
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnFail {
    Abort,
    Continue,
    Ask,
}

// ---------------------------------------------------------------------------
// RunStatus
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Running,
    Succeeded,
    CompletedWithErrors,
    Failed,
    Cancelled,
}

// ---------------------------------------------------------------------------
// StepStatus
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    TimedOut,
    Skipped,
    Cancelled,
}

// ---------------------------------------------------------------------------
// Plan
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub schema_version: u32,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<Mode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_apps: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_domains: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variables: Option<HashMap<String, serde_json::Value>>,
    pub steps: Vec<Step>,
}

// ---------------------------------------------------------------------------
// Step
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub id: String,
    #[serde(rename = "type")]
    pub step_type: StepType,
    pub params: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retries: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_backoff_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_fail: Option<OnFail>,
}

impl Step {
    /// Returns the effective timeout in milliseconds, falling back to `default`
    /// when no per-step override is set.
    pub fn effective_timeout_ms(&self, default: u64) -> u64 {
        self.timeout_ms.unwrap_or(default)
    }

    /// Returns the effective retry count, falling back to `default`.
    pub fn effective_retries(&self, default: u32) -> u32 {
        self.retries.unwrap_or(default)
    }

    /// Returns the effective retry backoff in milliseconds, falling back to `default`.
    pub fn effective_backoff_ms(&self, default: u64) -> u64 {
        self.retry_backoff_ms.unwrap_or(default)
    }

    /// Returns the effective on_fail strategy, defaulting to `Abort`.
    pub fn effective_on_fail(&self) -> OnFail {
        self.on_fail.clone().unwrap_or(OnFail::Abort)
    }
}

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub plan_id: String,
    pub status: RunStatus,
    pub mode: Mode,
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// StepResult
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub id: String,
    pub run_id: String,
    pub step_id: String,
    pub step_index: u32,
    pub status: StepStatus,
    pub attempt: u32,
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<DateTime<Utc>>,
    pub input_json: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_json: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_json: Option<serde_json::Value>,
}
