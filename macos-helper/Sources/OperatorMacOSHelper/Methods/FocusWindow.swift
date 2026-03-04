import AppKit
import ApplicationServices
import Foundation

/// Handles `ui.focusWindow` — brings a specific window to the foreground.
///
/// Params:
///   - `app` (string, required): Application name.
///   - `window` (object, required): One of `{ "index": <int> }` or `{ "title_contains": "<string>" }`.
///
/// Returns:
///   - `focused` (bool): Whether the window was raised.
///   - `window` (object): The targeted window's title and index.
func handleFocusWindow(_ params: [String: JSONValue]) throws -> [String: JSONValue] {
    try ensureAccessibility()
    let app = try extractApp(from: params)
    let appElement = axAppElement(for: app)

    // Parse window scope
    guard case .object(let winObj) = params["window"] else {
        throw HelperError(code: "INVALID_PARAMS", message: "Missing required 'window' parameter")
    }

    var scope = WindowScopeParams()
    if case .int(let v) = winObj["index"] { scope.index = v }
    if case .string(let v) = winObj["title_contains"] { scope.titleContains = v }

    let windowElement = try resolveWindow(appElement: appElement, scope: scope)

    // Raise the window and make it main
    AXUIElementPerformAction(windowElement, kAXRaiseAction as CFString)
    AXUIElementSetAttributeValue(windowElement, kAXMainAttribute as CFString, true as CFTypeRef)

    // Bring the app to foreground
    app.activate(options: .activateIgnoringOtherApps)

    // Read the window info for the response
    let title = axGetStringAttribute(windowElement, kAXTitleAttribute as String) ?? ""
    let windows = listWindows(appElement: appElement)
    let index = windows.firstIndex(where: { $0.title == title })?.description ?? "0"

    return [
        "focused": .bool(true),
        "window": .object([
            "title": .string(title),
            "index": .int(Int(index) ?? 0),
        ]),
    ]
}
