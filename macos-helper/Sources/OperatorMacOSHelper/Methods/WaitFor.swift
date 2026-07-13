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

    let maxDepth = selector.maxDepth ?? 12
    let hasAnyOf = selector.anyOf != nil && !(selector.anyOf!.isEmpty)
    let pollIntervalMs = 200
    let maxIterations = max(1, timeoutMs / pollIntervalMs)
    let startTime = DispatchTime.now()

    for i in 0..<maxIterations {
        // Resolve the window inside the loop: the app may not have opened its
        // window yet, and wait_for should poll for it rather than fail instantly
        // with WINDOW_NOT_FOUND on the first attempt.
        let windowElement = try? resolveWindow(appElement: appElement, scope: selector.window)

        // Gather matches. When an index is requested we must collect enough
        // candidates for findElements' index post-filter (it applies index AFTER
        // truncating to maxResults), so a maxResults of 1 would make index>=1
        // never match.
        var results: [FoundElement] = []
        if let windowElement = windowElement {
            if hasAnyOf {
                results = resolveAnyOf(root: windowElement, alternatives: selector.anyOf!, maxResults: 100)
            } else {
                let maxResults = selector.index != nil ? 100 : 1
                results = findElements(
                    root: windowElement, selector: selector, maxDepth: maxDepth, maxResults: maxResults)
            }
        }

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
