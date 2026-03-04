import AppKit
import ApplicationServices
import Foundation

/// Handles `ui.listWindows` — enumerates windows for an application.
///
/// Params:
///   - `app` (string, required): Application name.
///
/// Returns:
///   - `windows` (array): List of window objects with `title`, `index`, `main`, `focused`.
func handleListWindows(_ params: [String: JSONValue]) throws -> [String: JSONValue] {
    try ensureAccessibility()
    let app = try extractApp(from: params)
    let appElement = axAppElement(for: app)
    let windows = listWindows(appElement: appElement)

    let windowValues: [JSONValue] = windows.map { win in
        .object([
            "title": .string(win.title),
            "index": .int(win.index),
            "main": .bool(win.isMain),
            "focused": .bool(win.isFocused),
        ])
    }

    return [
        "windows": .array(windowValues),
    ]
}
