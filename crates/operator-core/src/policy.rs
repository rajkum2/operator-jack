use serde::{Deserialize, Serialize};

use crate::types::{Mode, StepType};

// ---------------------------------------------------------------------------
// RiskLevel
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

/// Returns the risk level for a given step type according to the spec.
pub fn risk_level(step_type: &StepType) -> RiskLevel {
    match step_type {
        // Low risk -- read-only / informational
        StepType::SysReadFile
        | StepType::SysClipboardGet
        | StepType::SysListDir
        | StepType::DataParseCsv
        | StepType::DataParseJson
        | StepType::UiListApps
        | StepType::UiFind
        | StepType::UiReadText
        | StepType::UiWaitFor
        | StepType::UiCheckAccessibilityPermission
        | StepType::UiListWindows
        | StepType::UiSnapshot
        | StepType::UiScreenshot
        // Document extraction is read-only.
        | StepType::DocPdfText
        | StepType::DocOcr
        // Control: orchestration only; the body's leaf steps are each gated on
        // their own risk when executed.
        | StepType::UiHumanTakeover
        | StepType::ControlForeach
        | StepType::ControlIf
        | StepType::ControlRepeatUntil => RiskLevel::Low,

        // High risk -- filesystem mutations, arbitrary command/script execution
        StepType::SysWriteFile
        | StepType::SysAppendFile
        | StepType::SysMkdir
        | StepType::SysMovePath
        | StepType::SysCopyPath
        | StepType::SysDeletePath
        | StepType::SysExec
        // Scripts drive first-party apps with app-level authority — exec-grade.
        | StepType::ScriptApplescript
        | StepType::ScriptJxa
        | StepType::ShortcutsRun => RiskLevel::High,

        // Medium risk -- everything else
        StepType::SysOpenApp
        | StepType::SysOpenUrl
        | StepType::SysQuitApp
        | StepType::SysClipboardSet
        // Network: gated by domain grants/allow-list, like sys.open_url.
        | StepType::HttpRequest
        | StepType::UiFocusApp
        | StepType::UiClick
        | StepType::UiSetValue
        | StepType::UiTypeText
        | StepType::UiKeyPress
        | StepType::UiSelectMenu
        | StepType::UiScroll
        | StepType::UiFocusWindow
        // Browser lane
        | StepType::BrowserNavigate
        | StepType::BrowserClick
        | StepType::BrowserType
        | StepType::BrowserGetText
        | StepType::BrowserGetAttribute
        | StepType::BrowserWaitFor
        | StepType::BrowserScroll
        | StepType::BrowserGetTextAll
        | StepType::BrowserGetAttributesAll
        | StepType::BrowserDownload => RiskLevel::Medium,

        // Browser high risk
        StepType::BrowserExecuteJs => RiskLevel::High,

        // Browser low risk (read-only)
        StepType::BrowserScreenshot | StepType::BrowserSnapshot => RiskLevel::Low,
    }
}

/// Returns whether the given step type requires user confirmation under the
/// specified mode.
///
/// - In `Unsafe` mode, confirmation is never required.
/// - In `Safe` mode, confirmation is required for medium and high risk steps.
pub fn requires_confirmation(step_type: &StepType, mode: &Mode) -> bool {
    match mode {
        Mode::Unsafe => false,
        Mode::Safe => {
            let level = risk_level(step_type);
            matches!(level, RiskLevel::Medium | RiskLevel::High)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_file_is_low_risk() {
        assert_eq!(risk_level(&StepType::SysReadFile), RiskLevel::Low);
    }

    #[test]
    fn test_write_file_is_high_risk() {
        assert_eq!(risk_level(&StepType::SysWriteFile), RiskLevel::High);
    }

    #[test]
    fn test_exec_is_high_risk() {
        assert_eq!(risk_level(&StepType::SysExec), RiskLevel::High);
    }

    #[test]
    fn test_open_app_is_medium_risk() {
        assert_eq!(risk_level(&StepType::SysOpenApp), RiskLevel::Medium);
    }

    #[test]
    fn test_clipboard_get_is_low_risk() {
        assert_eq!(risk_level(&StepType::SysClipboardGet), RiskLevel::Low);
    }

    #[test]
    fn test_safe_mode_requires_confirmation_for_medium() {
        assert!(requires_confirmation(&StepType::SysOpenApp, &Mode::Safe));
    }

    #[test]
    fn test_safe_mode_requires_confirmation_for_high() {
        assert!(requires_confirmation(&StepType::SysExec, &Mode::Safe));
    }

    #[test]
    fn test_safe_mode_no_confirmation_for_low() {
        assert!(!requires_confirmation(&StepType::SysReadFile, &Mode::Safe));
    }

    #[test]
    fn test_unsafe_mode_never_requires_confirmation() {
        assert!(!requires_confirmation(&StepType::SysExec, &Mode::Unsafe));
        assert!(!requires_confirmation(
            &StepType::SysDeletePath,
            &Mode::Unsafe
        ));
    }

    #[test]
    fn test_list_windows_is_low_risk() {
        assert_eq!(risk_level(&StepType::UiListWindows), RiskLevel::Low);
        assert!(!requires_confirmation(
            &StepType::UiListWindows,
            &Mode::Safe
        ));
    }

    #[test]
    fn test_focus_window_is_medium_risk() {
        assert_eq!(risk_level(&StepType::UiFocusWindow), RiskLevel::Medium);
        assert!(requires_confirmation(&StepType::UiFocusWindow, &Mode::Safe));
    }
}
