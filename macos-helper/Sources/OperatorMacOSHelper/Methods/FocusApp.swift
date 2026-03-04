import AppKit
import Foundation

/// Handles `ui.focusApp` — brings an application to the foreground.
///
/// Params:
///   - `app` (string, required): Application name.
///
/// Returns:
///   - `app` (string): App name.
///   - `focused` (bool): Whether the app became active.
///   - `pid` (int): Process ID.
func handleFocusApp(_ params: [String: JSONValue]) throws -> [String: JSONValue] {
    try ensureAccessibility()
    let app = try extractApp(from: params)

    app.activate(options: .activateIgnoringOtherApps)

    // Poll up to 2 seconds for app to become active
    var focused = app.isActive
    if !focused {
        for _ in 0..<10 {
            Thread.sleep(forTimeInterval: 0.2)
            if app.isActive {
                focused = true
                break
            }
        }
    }

    return [
        "app": .string(app.localizedName ?? "unknown"),
        "focused": .bool(focused),
        "pid": .int(Int(app.processIdentifier)),
    ]
}
