import ApplicationServices
import AppKit
import Foundation

/// Handles `ui.waitFor` — polls until an element matching the selector appears
/// or a condition is met.
///
/// Params:
///   - `app` (string, required): Application name.
///   - `selector` (object, required): Selector criteria.
///   - `timeout_ms` (int, optional): Timeout in ms (default 5000).
///   - `condition` (string, optional): One of "exists" (default), "text_equals", "enabled", "visible".
///   - `expected_value` (string, optional): Expected text for "text_equals" condition.
///
/// Returns:
///   - `found` (bool): Whether the condition was met.
///   - `target` (object, optional): The matched element metadata.
///   - `waited_ms` (int): Actual time waited.
func handleWaitFor(_ params: [String: JSONValue]) throws -> [String: JSONValue] {
    try ensureAccessibility()
    let app = try extractApp(from: params)
    let appElement = axAppElement(for: app)

    guard let selector = SelectorParams.parse(from: params) else {
        throw HelperError(code: "INVALID_PARAMS", message: "Missing or invalid 'selector' parameter")
    }

    let timeoutMs: Int
    if case .int(let t) = params["timeout_ms"] {
        timeoutMs = t
    } else {
        timeoutMs = 5000
    }

    let condition: String
    if case .string(let c) = params["condition"] {
        condition = c
    } else {
        condition = "exists"
    }

    let expectedValue: String?
    if case .string(let v) = params["expected_value"] {
        expectedValue = v
    } else {
        expectedValue = nil
    }

    let windowElement = try resolveWindow(appElement: appElement, scope: selector.window)
    let maxDepth = selector.maxDepth ?? 12
    let pollIntervalMs = 200
    let maxIterations = max(1, timeoutMs / pollIntervalMs)
    let startTime = DispatchTime.now()

    for i in 0..<maxIterations {
        let results = findElements(root: windowElement, selector: selector, maxDepth: maxDepth, maxResults: 1)

        if let match = results.first {
            let conditionMet: Bool
            switch condition {
            case "exists":
                conditionMet = true
            case "text_equals":
                let value = axGetStringAttribute(match.element, kAXValueAttribute as String)
                    ?? axGetStringAttribute(match.element, kAXTitleAttribute as String)
                    ?? ""
                conditionMet = (value == expectedValue)
            case "enabled":
                conditionMet = axGetBoolAttribute(match.element, kAXEnabledAttribute as String) ?? false
            case "visible":
                let hidden = axGetBoolAttribute(match.element, "AXHidden") ?? false
                conditionMet = !hidden
            default:
                conditionMet = true // unknown condition, treat as "exists"
            }

            if conditionMet {
                let elapsed = DispatchTime.now().uptimeNanoseconds - startTime.uptimeNanoseconds
                let elapsedMs = Int(elapsed / 1_000_000)
                return [
                    "found": .bool(true),
                    "target": .object(match.metadata),
                    "waited_ms": .int(elapsedMs),
                ]
            }
        }

        if i < maxIterations - 1 {
            Thread.sleep(forTimeInterval: Double(pollIntervalMs) / 1000.0)
        }
    }

    throw HelperError(
        code: "TIMEOUT",
        message: "Condition '\(condition)' not met within \(timeoutMs)ms",
        retryable: true
    )
}
