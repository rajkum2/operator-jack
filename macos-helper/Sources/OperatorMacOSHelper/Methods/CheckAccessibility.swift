import ApplicationServices
import Foundation

/// Handles `ui.checkAccessibilityPermission` — checks if the process is trusted
/// for accessibility access.
///
/// Params:
///   - `prompt` (bool, optional): if true, show the system prompt dialog.
///
/// Returns:
///   - `trusted` (bool): whether accessibility access is currently granted.
func handleCheckAccessibility(_ params: [String: JSONValue]) throws -> [String: JSONValue] {
    // Determine whether to show the system accessibility prompt.
    let shouldPrompt: Bool
    if case .bool(let p) = params["prompt"] {
        shouldPrompt = p
    } else {
        shouldPrompt = false
    }

    let options: NSDictionary
    if shouldPrompt {
        options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue(): true]
    } else {
        options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue(): false]
    }

    let trusted = AXIsProcessTrustedWithOptions(options)

    return [
        "trusted": .bool(trusted),
    ]
}
