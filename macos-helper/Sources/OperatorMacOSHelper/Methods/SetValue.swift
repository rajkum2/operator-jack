import ApplicationServices
import AppKit
import Foundation

/// Handles `ui.setValue` — sets the value of a UI element via AX API.
///
/// Params:
///   - `app` (string, required): Application name.
///   - `selector` (object, required): Selector criteria.
///   - `value` (string, required): Value to set.
///   - `implicit_wait_ms` (int, optional): Wait time for element (default 2000).
///
/// Returns:
///   - `set` (bool): Whether the value was set.
///   - `value` (string): The value that was set.
///   - `verified` (bool): Whether the value was verified after setting.
///   - `target` (object): The matched element's metadata.
func handleSetValue(_ params: [String: JSONValue]) throws -> [String: JSONValue] {
    try ensureAccessibility()
    let app = try extractApp(from: params)
    let appElement = axAppElement(for: app)

    guard let selector = SelectorParams.parse(from: params) else {
        throw HelperError(code: "INVALID_PARAMS", message: "Missing or invalid 'selector' parameter")
    }

    guard case .string(let newValue) = params["value"] else {
        throw HelperError(code: "INVALID_PARAMS", message: "Missing required 'value' parameter")
    }

    let waitMs = extractImplicitWaitMs(from: params)
    let windowElement = try resolveWindow(appElement: appElement, scope: selector.window)
    let results = try implicitWaitForElement(root: windowElement, selector: selector, waitMs: waitMs)
    let match = try disambiguate(results: results, index: selector.index)

    // Check if the value attribute is settable
    var settable: DarwinBoolean = false
    let settableResult = AXUIElementIsAttributeSettable(match.element, kAXValueAttribute as CFString, &settable)
    guard settableResult == .success && settable.boolValue else {
        throw HelperError(
            code: "ELEMENT_NOT_ACTIONABLE",
            message: "Element's value attribute is not settable"
        )
    }

    // Set the value
    let setResult = AXUIElementSetAttributeValue(match.element, kAXValueAttribute as CFString, newValue as CFTypeRef)
    guard setResult == .success else {
        throw HelperError(
            code: "ELEMENT_NOT_ACTIONABLE",
            message: "Failed to set value (error \(setResult.rawValue))",
            retryable: true
        )
    }

    // Verify by reading back
    let readBack = axGetStringAttribute(match.element, kAXValueAttribute as String)
    let verified = readBack == newValue

    return [
        "set": .bool(true),
        "value": .string(newValue),
        "verified": .bool(verified),
        "target": .object(match.metadata),
        "_evidence": .object(gatherEvidence(app: app, element: match.element)),
    ]
}
