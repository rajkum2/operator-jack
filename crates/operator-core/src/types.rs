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
    /// List a directory (optionally glob-filtered) → array of entries (M13).
    #[serde(rename = "sys.list_dir")]
    SysListDir,

    // Data lane (M13) — pure transforms, no side effects.
    /// Parse CSV text (or a file) → array of row objects/arrays.
    #[serde(rename = "data.parse_csv")]
    DataParseCsv,
    /// Parse JSON text (or a file), optionally at a JSON pointer.
    #[serde(rename = "data.parse_json")]
    DataParseJson,

    // Network lane (M13) — HTTP under domain grants.
    /// Make an HTTP request → {status, headers, body}.
    #[serde(rename = "http.request")]
    HttpRequest,

    // Script lane (M14) — first-party app automation via osascript/shortcuts.
    /// Run an AppleScript (High risk; exec-grade grant; body policy-scanned).
    #[serde(rename = "script.applescript")]
    ScriptApplescript,
    /// Run a JavaScript-for-Automation (JXA) script (same gating as AppleScript).
    #[serde(rename = "script.jxa")]
    ScriptJxa,
    /// Run a macOS Shortcut by name.
    #[serde(rename = "shortcuts.run")]
    ShortcutsRun,

    // Document lane (M14) — text extraction via the Swift helper.
    /// Extract text from a PDF (PDFKit).
    #[serde(rename = "doc.pdf_text")]
    DocPdfText,
    /// OCR an image / scanned document (Vision.framework, on-device).
    #[serde(rename = "doc.ocr")]
    DocOcr,

    // Control lane (M13) — deterministic control flow; bodies carry sub-steps.
    /// Iterate a collection, running the body per element.
    #[serde(rename = "control.foreach")]
    ControlForeach,
    /// Run one of two branches based on a condition.
    #[serde(rename = "control.if")]
    ControlIf,
    /// Run the body until a condition holds or `max_iterations` is hit.
    #[serde(rename = "control.repeat_until")]
    ControlRepeatUntil,

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
    #[serde(rename = "ui.list_windows")]
    UiListWindows,
    #[serde(rename = "ui.focus_window")]
    UiFocusWindow,
    #[serde(rename = "ui.snapshot")]
    UiSnapshot,
    #[serde(rename = "ui.screenshot")]
    UiScreenshot,
    /// Scroll a scrollable element (or the focused window) by a delta or to an
    /// edge (M13). No scroll verb existed in the UI lane before.
    #[serde(rename = "ui.scroll")]
    UiScroll,
    /// Pause the run and hand control to the human (login walls, CAPTCHAs). The
    /// agent never types credentials or solves CAPTCHAs itself (M11).
    #[serde(rename = "ui.human_takeover")]
    UiHumanTakeover,

    // Browser lane (M5)
    #[serde(rename = "browser.navigate")]
    BrowserNavigate,
    #[serde(rename = "browser.click")]
    BrowserClick,
    #[serde(rename = "browser.type")]
    BrowserType,
    #[serde(rename = "browser.get_text")]
    BrowserGetText,
    #[serde(rename = "browser.get_attribute")]
    BrowserGetAttribute,
    #[serde(rename = "browser.execute_js")]
    BrowserExecuteJs,
    #[serde(rename = "browser.screenshot")]
    BrowserScreenshot,
    #[serde(rename = "browser.wait_for")]
    BrowserWaitFor,
    #[serde(rename = "browser.scroll")]
    BrowserScroll,
    /// Extract text from ALL elements matching a selector (querySelectorAll → array).
    #[serde(rename = "browser.get_text_all")]
    BrowserGetTextAll,
    /// Extract one attribute from ALL matching elements (→ array).
    #[serde(rename = "browser.get_attributes_all")]
    BrowserGetAttributesAll,
    /// Compact snapshot of the page's interactive elements with ref ids (M12) —
    /// the browser-lane analogue of `ui.snapshot`, so the agent loop is
    /// lane-agnostic.
    #[serde(rename = "browser.snapshot")]
    BrowserSnapshot,
    /// Download a file (via a URL or by clicking a link) to a target dir.
    #[serde(rename = "browser.download")]
    BrowserDownload,
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
            | StepType::SysClipboardSet
            | StepType::SysListDir
            | StepType::DataParseCsv
            | StepType::DataParseJson
            | StepType::HttpRequest
            | StepType::ScriptApplescript
            | StepType::ScriptJxa
            | StepType::ShortcutsRun => "system",

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
            | StepType::UiSelectMenu
            | StepType::UiListWindows
            | StepType::UiFocusWindow
            | StepType::UiSnapshot
            | StepType::UiScreenshot
            | StepType::UiScroll
            // Document steps run in the Swift helper (PDFKit / Vision).
            | StepType::DocPdfText
            | StepType::DocOcr => "ui",

            // Control lane: handled by the engine/session itself, not dispatched.
            StepType::UiHumanTakeover
            | StepType::ControlForeach
            | StepType::ControlIf
            | StepType::ControlRepeatUntil => "control",

            // Browser lane
            StepType::BrowserNavigate
            | StepType::BrowserClick
            | StepType::BrowserType
            | StepType::BrowserGetText
            | StepType::BrowserGetAttribute
            | StepType::BrowserExecuteJs
            | StepType::BrowserScreenshot
            | StepType::BrowserWaitFor
            | StepType::BrowserScroll
            | StepType::BrowserGetTextAll
            | StepType::BrowserGetAttributesAll
            | StepType::BrowserSnapshot
            | StepType::BrowserDownload => "browser",
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
            StepType::SysListDir => "sys.list_dir",
            StepType::DataParseCsv => "data.parse_csv",
            StepType::DataParseJson => "data.parse_json",
            StepType::HttpRequest => "http.request",
            StepType::ScriptApplescript => "script.applescript",
            StepType::ScriptJxa => "script.jxa",
            StepType::ShortcutsRun => "shortcuts.run",
            StepType::DocPdfText => "doc.pdf_text",
            StepType::DocOcr => "doc.ocr",
            StepType::ControlForeach => "control.foreach",
            StepType::ControlIf => "control.if",
            StepType::ControlRepeatUntil => "control.repeat_until",
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
            StepType::UiListWindows => "ui.list_windows",
            StepType::UiFocusWindow => "ui.focus_window",
            StepType::UiSnapshot => "ui.snapshot",
            StepType::UiScreenshot => "ui.screenshot",
            StepType::UiScroll => "ui.scroll",
            StepType::UiHumanTakeover => "ui.human_takeover",
            // Browser lane
            StepType::BrowserNavigate => "browser.navigate",
            StepType::BrowserClick => "browser.click",
            StepType::BrowserType => "browser.type",
            StepType::BrowserGetText => "browser.get_text",
            StepType::BrowserGetAttribute => "browser.get_attribute",
            StepType::BrowserExecuteJs => "browser.execute_js",
            StepType::BrowserScreenshot => "browser.screenshot",
            StepType::BrowserWaitFor => "browser.wait_for",
            StepType::BrowserScroll => "browser.scroll",
            StepType::BrowserGetTextAll => "browser.get_text_all",
            StepType::BrowserGetAttributesAll => "browser.get_attributes_all",
            StepType::BrowserSnapshot => "browser.snapshot",
            StepType::BrowserDownload => "browser.download",
        }
    }
}

impl fmt::Display for StepType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for StepType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "sys.open_app" => Ok(StepType::SysOpenApp),
            "sys.open_url" => Ok(StepType::SysOpenUrl),
            "sys.read_file" => Ok(StepType::SysReadFile),
            "sys.write_file" => Ok(StepType::SysWriteFile),
            "sys.append_file" => Ok(StepType::SysAppendFile),
            "sys.mkdir" => Ok(StepType::SysMkdir),
            "sys.move_path" => Ok(StepType::SysMovePath),
            "sys.copy_path" => Ok(StepType::SysCopyPath),
            "sys.delete_path" => Ok(StepType::SysDeletePath),
            "sys.exec" => Ok(StepType::SysExec),
            "sys.quit_app" => Ok(StepType::SysQuitApp),
            "sys.clipboard_get" => Ok(StepType::SysClipboardGet),
            "sys.clipboard_set" => Ok(StepType::SysClipboardSet),
            "sys.list_dir" => Ok(StepType::SysListDir),
            "data.parse_csv" => Ok(StepType::DataParseCsv),
            "data.parse_json" => Ok(StepType::DataParseJson),
            "http.request" => Ok(StepType::HttpRequest),
            "script.applescript" => Ok(StepType::ScriptApplescript),
            "script.jxa" => Ok(StepType::ScriptJxa),
            "shortcuts.run" => Ok(StepType::ShortcutsRun),
            "doc.pdf_text" => Ok(StepType::DocPdfText),
            "doc.ocr" => Ok(StepType::DocOcr),
            "control.foreach" => Ok(StepType::ControlForeach),
            "control.if" => Ok(StepType::ControlIf),
            "control.repeat_until" => Ok(StepType::ControlRepeatUntil),
            "ui.check_accessibility_permission" => Ok(StepType::UiCheckAccessibilityPermission),
            "ui.list_apps" => Ok(StepType::UiListApps),
            "ui.focus_app" => Ok(StepType::UiFocusApp),
            "ui.find" => Ok(StepType::UiFind),
            "ui.click" => Ok(StepType::UiClick),
            "ui.set_value" => Ok(StepType::UiSetValue),
            "ui.type_text" => Ok(StepType::UiTypeText),
            "ui.key_press" => Ok(StepType::UiKeyPress),
            "ui.read_text" => Ok(StepType::UiReadText),
            "ui.wait_for" => Ok(StepType::UiWaitFor),
            "ui.select_menu" => Ok(StepType::UiSelectMenu),
            "ui.list_windows" => Ok(StepType::UiListWindows),
            "ui.focus_window" => Ok(StepType::UiFocusWindow),
            "ui.snapshot" => Ok(StepType::UiSnapshot),
            "ui.screenshot" => Ok(StepType::UiScreenshot),
            "ui.scroll" => Ok(StepType::UiScroll),
            "ui.human_takeover" => Ok(StepType::UiHumanTakeover),
            // Browser lane
            "browser.navigate" => Ok(StepType::BrowserNavigate),
            "browser.click" => Ok(StepType::BrowserClick),
            "browser.type" => Ok(StepType::BrowserType),
            "browser.get_text" => Ok(StepType::BrowserGetText),
            "browser.get_attribute" => Ok(StepType::BrowserGetAttribute),
            "browser.execute_js" => Ok(StepType::BrowserExecuteJs),
            "browser.screenshot" => Ok(StepType::BrowserScreenshot),
            "browser.wait_for" => Ok(StepType::BrowserWaitFor),
            "browser.scroll" => Ok(StepType::BrowserScroll),
            "browser.get_text_all" => Ok(StepType::BrowserGetTextAll),
            "browser.get_attributes_all" => Ok(StepType::BrowserGetAttributesAll),
            "browser.snapshot" => Ok(StepType::BrowserSnapshot),
            "browser.download" => Ok(StepType::BrowserDownload),
            _ => Err(format!("Unknown step type: {}", s)),
        }
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
    /// Stopped early for a non-error reason (budget exhausted). Completed step
    /// results are preserved. Terminal.
    Partial,
    /// Suspended mid-run awaiting out-of-band approval or human takeover (M11).
    /// A checkpoint is persisted; the run can be resumed with `--resume` once an
    /// approval is granted. Non-terminal (it becomes a fresh run on resume).
    Paused,
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
// Checkpoint / Approval (M11 — long-horizon supervision)
// ---------------------------------------------------------------------------

/// Which execution lane produced a checkpoint, so `--resume` knows how to pick
/// the run back up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointKind {
    /// A deterministic plan run: resume replays from `cursor` (step index).
    Plan,
    /// A native agent run: resume re-seeds the transcript and continues.
    Agent,
}

/// A persisted snapshot of a paused run, enough to resume it after the process
/// exits. One (latest) checkpoint per run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub run_id: String,
    pub plan_id: String,
    pub kind: CheckpointKind,
    /// Resume cursor. For plans, `{ "step_index": N }`; for agents, the goal and
    /// iteration are carried alongside in `transcript`/`goal`.
    pub cursor: serde_json::Value,
    /// Run-scoped variable map at pause time (control-flow vars, M13).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variables: Option<serde_json::Value>,
    /// Agent transcript lines (agent runs only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript: Option<Vec<String>>,
    /// The agent goal (agent runs only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
    /// Why the run paused (escalation reason / takeover prompt).
    pub reason: String,
}

/// The disposition of a pending approval request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Denied,
}

/// What kind of human decision a paused run is waiting on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalKind {
    /// A high-consequence action the always-escalate classifier flagged.
    Escalation,
    /// A `ui.human_takeover` step (login/CAPTCHA); the human drives, then resumes.
    Takeover,
}

/// A request for out-of-band human approval that gates resuming a paused run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Approval {
    pub id: String,
    pub run_id: String,
    pub kind: ApprovalKind,
    pub summary: String,
    pub status: ApprovalStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<chrono::DateTime<chrono::Utc>>,
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
    /// Task-scoped capability grants (M8). None = unrestricted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grants: Option<crate::grants::Grants>,
    /// Whole-run resource budget (M8). None = unbounded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget: Option<crate::grants::RunBudget>,
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
