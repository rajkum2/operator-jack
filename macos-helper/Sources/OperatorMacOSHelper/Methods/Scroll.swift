import ApplicationServices
import AppKit
import Foundation

/// Handles `ui.scroll` (M13) — scrolls the focused window/element of an app by
/// synthesizing scroll-wheel events. No scroll verb existed in the UI lane
/// before M13.
///
/// Params:
///   - `app` (string, required): Application name (brought to the front first).
///   - `dy` (int, optional): Vertical lines per click. NEGATIVE scrolls the
///     content DOWN (the natural macOS wheel convention); default -5.
///   - `dx` (int, optional): Horizontal lines per click; default 0.
///   - `clicks` (int, optional): Number of wheel clicks to post; default 1.
///   - `to` (string, optional): "top" or "bottom" — a large repeated scroll to
///     the respective edge (overrides dy/clicks).
///
/// Returns:
///   - `scrolled` (bool), and the effective `dx`, `dy`, `clicks`.
func handleScroll(_ params: [String: JSONValue]) throws -> [String: JSONValue] {
    try ensureAccessibility()
    let app = try extractApp(from: params)

    // Bring the app forward so wheel events land on its focused element.
    app.activate(options: .activateIgnoringOtherApps)
    for _ in 0..<10 {
        if app.isActive { break }
        Thread.sleep(forTimeInterval: 0.1)
    }

    var dy = -5
    var dx = 0
    var clicks = 1
    if case .int(let v) = params["dy"] { dy = v }
    if case .int(let v) = params["dx"] { dx = v }
    if case .int(let v) = params["clicks"] { clicks = v }

    if case .string(let to) = params["to"] {
        switch to.lowercased() {
        case "bottom": dy = -40; dx = 0; clicks = 25
        case "top": dy = 40; dx = 0; clicks = 25
        default: break
        }
    }

    clicks = max(1, min(clicks, 200)) // bound the burst

    for _ in 0..<clicks {
        guard
            let ev = CGEvent(
                scrollWheelEvent2Source: nil,
                units: .line,
                wheelCount: 2,
                wheel1: Int32(dy),
                wheel2: Int32(dx),
                wheel3: 0
            )
        else {
            throw HelperError(
                code: "INPUT_BLOCKED",
                message: "failed to synthesize scroll event",
                retryable: true
            )
        }
        ev.post(tap: .cghidEventTap)
        Thread.sleep(forTimeInterval: 0.02)
    }

    return [
        "scrolled": .bool(true),
        "dx": .int(dx),
        "dy": .int(dy),
        "clicks": .int(clicks),
    ]
}
