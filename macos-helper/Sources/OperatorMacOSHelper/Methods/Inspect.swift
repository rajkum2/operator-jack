import ApplicationServices
import AppKit
import Foundation

/// Handles `ui.inspect` — dumps the accessibility tree for debugging.
///
/// Params:
///   - `app` (string, required): Application name.
///   - `depth` (int, optional): Maximum tree depth (default 5).
///   - `window` (object, optional): Window scope (index or title_contains).
///
/// Returns:
///   - `app` (string): Application name.
///   - `tree` (object): Recursive AX tree structure.
///   - `node_count` (int): Total number of nodes in the tree.
func handleInspect(_ params: [String: JSONValue]) throws -> [String: JSONValue] {
    try ensureAccessibility()
    let app = try extractApp(from: params)
    let appElement = axAppElement(for: app)

    let maxDepth: Int
    if case .int(let d) = params["depth"] {
        maxDepth = d
    } else {
        maxDepth = 5
    }

    // Parse optional window scope
    var windowScope: WindowScopeParams? = nil
    if case .object(let w) = params["window"] {
        var ws = WindowScopeParams()
        if case .int(let v) = w["index"] { ws.index = v }
        if case .string(let v) = w["title_contains"] { ws.titleContains = v }
        windowScope = ws
    }

    let windowElement = try resolveWindow(appElement: appElement, scope: windowScope)

    var nodeCount = 0
    let tree = inspectTree(element: windowElement, currentDepth: 0, maxDepth: maxDepth, nodeCount: &nodeCount)

    return [
        "app": .string(app.localizedName ?? "unknown"),
        "tree": tree,
        "node_count": .int(nodeCount),
    ]
}

/// Recursively builds a JSONValue tree representation of the AX hierarchy.
private func inspectTree(element: AXUIElement, currentDepth: Int, maxDepth: Int, nodeCount: inout Int) -> JSONValue {
    nodeCount += 1

    var dict: [String: JSONValue] = serializeElement(element)
    dict["depth"] = .int(currentDepth)

    if currentDepth < maxDepth {
        let children = axGetArrayAttribute(element, kAXChildrenAttribute as String)
        if !children.isEmpty {
            let childNodes: [JSONValue] = children.map { child in
                inspectTree(element: child, currentDepth: currentDepth + 1, maxDepth: maxDepth, nodeCount: &nodeCount)
            }
            dict["children"] = .array(childNodes)
        }
    }

    return .object(dict)
}
