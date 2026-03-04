import ApplicationServices
import AppKit
import Foundation

/// Handles `ui.click` — clicks a UI element matched by selector.
///
/// Params:
///   - `app` (string, required): Application name.
///   - `selector` (object, required): Selector criteria.
///   - `implicit_wait_ms` (int, optional): Wait time for element (default 2000).
///
/// Returns:
///   - `clicked` (bool): Whether the click was performed.
///   - `target` (object): The clicked element's metadata.
func handleClick(_ params: [String: JSONValue]) throws -> [String: JSONValue] {
    try ensureAccessibility()
    let app = try extractApp(from: params)
    let appElement = axAppElement(for: app)

    guard let selector = SelectorParams.parse(from: params) else {
        throw HelperError(code: "INVALID_PARAMS", message: "Missing or invalid 'selector' parameter")
    }

    let waitMs = extractImplicitWaitMs(from: params)
    let windowElement = try resolveWindow(appElement: appElement, scope: selector.window)

    // Implicit wait for element
    let results = try implicitWaitForElement(
        root: windowElement,
        selector: selector,
        waitMs: waitMs
    )

    // Disambiguate
    let match = try disambiguate(results: results, index: selector.index)

    // Check actionability
    if let enabled = axGetBoolAttribute(match.element, kAXEnabledAttribute as String), !enabled {
        throw HelperError(
            code: "ELEMENT_NOT_ACTIONABLE",
            message: "Element is disabled and cannot be clicked"
        )
    }

    // Perform the click
    let axResult = AXUIElementPerformAction(match.element, kAXPressAction as CFString)
    guard axResult == .success else {
        throw HelperError(
            code: "ELEMENT_NOT_ACTIONABLE",
            message: "AXPress action failed with error code \(axResult.rawValue)",
            retryable: true
        )
    }

    return [
        "clicked": .bool(true),
        "target": .object(match.metadata),
        "_evidence": .object(gatherEvidence(app: app, element: match.element)),
    ]
}
