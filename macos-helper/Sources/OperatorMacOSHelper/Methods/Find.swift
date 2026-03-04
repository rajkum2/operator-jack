import ApplicationServices
import AppKit
import Foundation

/// Handles `ui.find` — finds elements in the AX tree matching a selector.
///
/// Params:
///   - `app` (string, required): Application name.
///   - `selector` (object, required): Selector criteria.
///
/// Returns:
///   - `matches` (array): Matched elements with metadata.
///   - `count` (int): Number of matches.
func handleFind(_ params: [String: JSONValue]) throws -> [String: JSONValue] {
    try ensureAccessibility()
    let app = try extractApp(from: params)
    let appElement = axAppElement(for: app)

    guard let selector = SelectorParams.parse(from: params) else {
        throw HelperError(code: "INVALID_PARAMS", message: "Missing or invalid 'selector' parameter")
    }

    let windowElement = try resolveWindow(appElement: appElement, scope: selector.window)
    let maxDepth = selector.maxDepth ?? 12
    let results = findElements(root: windowElement, selector: selector, maxDepth: maxDepth, maxResults: 100)

    let appName: String
    if case .string(let n) = params["app"] {
        appName = n
    } else {
        appName = "unknown"
    }

    let refStore = ElementRefStore.shared
    let matches: [JSONValue] = results.map { el in
        var meta = el.metadata
        let refId = refStore.store(element: el.element, selector: selector, appName: appName)
        meta["element_ref"] = .string(refId)
        return .object(meta)
    }

    return [
        "matches": .array(matches),
        "count": .int(results.count),
    ]
}
