import ApplicationServices
import AppKit
import Foundation

/// Handles `ui.typeText` — types text into the focused app via CGEvent keystrokes.
///
/// Fallback ladder:
///   1. Focus the app
///   2. If selector provided: find element and focus it
///   3. Type via CGEvent unicode keystrokes
///
/// Params:
///   - `app` (string, required): Application name.
///   - `text` (string, required): Text to type.
///   - `selector` (object, optional): Element to focus before typing.
///   - `implicit_wait_ms` (int, optional): Wait time for element (default 2000).
///
/// Returns:
///   - `typed` (bool): Whether text was typed.
///   - `chars` (int): Number of characters typed.
func handleTypeText(_ params: [String: JSONValue]) throws -> [String: JSONValue] {
    try ensureAccessibility()
    let app = try extractApp(from: params)

    guard case .string(let text) = params["text"] else {
        throw HelperError(code: "INVALID_PARAMS", message: "Missing required 'text' parameter")
    }

    // Bring app to foreground
    app.activate(options: .activateIgnoringOtherApps)
    Thread.sleep(forTimeInterval: 0.1) // Brief pause for activation

    // If a selector is provided, find and focus the element
    if let selector = SelectorParams.parse(from: params) {
        let appElement = axAppElement(for: app)
        let waitMs = extractImplicitWaitMs(from: params)
        let windowElement = try resolveWindow(appElement: appElement, scope: selector.window)
        let results = try implicitWaitForElement(root: windowElement, selector: selector, waitMs: waitMs)
        let match = try disambiguate(results: results, index: selector.index)

        // Focus the element
        AXUIElementSetAttributeValue(match.element, kAXFocusedAttribute as CFString, true as CFTypeRef)
        Thread.sleep(forTimeInterval: 0.05)
    }

    // Type via CGEvent unicode keystrokes
    var charCount = 0
    for character in text {
        if character == "\n" || character == "\r" {
            // Return key
            typeVirtualKey(keyCode: 36)
        } else if character == "\t" {
            // Tab key
            typeVirtualKey(keyCode: 48)
        } else {
            // Unicode character via CGEvent
            var chars = Array(String(character).utf16)
            guard let keyDown = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: true) else {
                throw HelperError(code: "INPUT_BLOCKED", message: "Failed to create CGEvent — secure input may be active")
            }
            keyDown.keyboardSetUnicodeString(stringLength: chars.count, unicodeString: &chars)
            keyDown.post(tap: .cghidEventTap)

            guard let keyUp = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: false) else { continue }
            keyUp.keyboardSetUnicodeString(stringLength: chars.count, unicodeString: &chars)
            keyUp.post(tap: .cghidEventTap)
        }
        charCount += 1
        usleep(5000) // 5ms between keystrokes
    }

    return [
        "typed": .bool(true),
        "chars": .int(charCount),
        "_evidence": .object(gatherEvidence(app: app)),
    ]
}

/// Types a single virtual key code (key down + key up).
private func typeVirtualKey(keyCode: CGKeyCode, flags: CGEventFlags = []) {
    if let keyDown = CGEvent(keyboardEventSource: nil, virtualKey: keyCode, keyDown: true) {
        keyDown.flags = flags
        keyDown.post(tap: .cghidEventTap)
    }
    if let keyUp = CGEvent(keyboardEventSource: nil, virtualKey: keyCode, keyDown: false) {
        keyUp.flags = flags
        keyUp.post(tap: .cghidEventTap)
    }
}
