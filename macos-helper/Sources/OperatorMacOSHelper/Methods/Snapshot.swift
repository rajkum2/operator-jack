import ApplicationServices
import AppKit
import Foundation

/// Handles `ui.snapshot` — a compact, LLM-oriented serialization of an app's
/// accessibility tree with actionable element refs (`e1`, `e2`, …).
/// See SPEC_FREEZE_V0.7.md §2.
///
/// Params:
///   - `app` (string, required)
///   - `window` (object, optional): window scope (index | title_contains)
///   - `max_depth` (int, optional): default 12
///   - `interactive_only` (bool, optional): default true
///
/// Returns: `snapshot` (string), `ref_count` (int), `truncated` (bool),
/// `token_estimate` (int, chars/4).
func handleSnapshot(_ params: [String: JSONValue]) throws -> [String: JSONValue] {
    try ensureAccessibility()
    let app = try extractApp(from: params)
    let appName: String
    if case .string(let n) = params["app"] {
        appName = n
    } else {
        appName = app.localizedName ?? "unknown"
    }
    let appElement = axAppElement(for: app)

    var maxDepth = 12
    if case .int(let d) = params["max_depth"] { maxDepth = d }
    var interactiveOnly = true
    if case .bool(let b) = params["interactive_only"] { interactiveOnly = b }

    var windowScope: WindowScopeParams? = nil
    if case .object(let w) = params["window"] {
        var ws = WindowScopeParams()
        if case .int(let v) = w["index"] { ws.index = v }
        if case .string(let v) = w["title_contains"] { ws.titleContains = v }
        windowScope = ws
    }
    let root = try resolveWindow(appElement: appElement, scope: windowScope)

    // On first observation of a Chromium/Electron app this session, wait for its
    // lazily-built a11y tree to populate so the first snapshot isn't empty.
    settleAccessibilityTree(app: app, root: root)

    // Per-snapshot ref invalidation: clear, then mint fresh e1..eN in pre-order.
    let refStore = ElementRefStore.shared
    refStore.clear()

    var refCounter = 0
    var truncated = false
    let entries = collectSnapshot(
        element: root,
        depth: 0,
        maxDepth: maxDepth,
        interactiveOnly: interactiveOnly,
        appName: appName,
        refStore: refStore,
        refCounter: &refCounter,
        truncated: &truncated
    )

    let lines = entries.map { String(repeating: "  ", count: $0.0) + $0.1 }
    let snapshot = lines.joined(separator: "\n")
    let tokenEstimate = (snapshot.count + 3) / 4

    return [
        "snapshot": .string(snapshot),
        "ref_count": .int(refCounter),
        "truncated": .bool(truncated),
        "token_estimate": .int(tokenEstimate),
    ]
}

/// Returns `(relativeDepth, lineText)` entries for the subtree. Relative depth
/// counts only INCLUDED ancestors, so pruned non-interactive containers do not
/// leave misleading indentation.
private func collectSnapshot(
    element: AXUIElement,
    depth: Int,
    maxDepth: Int,
    interactiveOnly: Bool,
    appName: String,
    refStore: ElementRefStore,
    refCounter: inout Int,
    truncated: inout Bool
) -> [(Int, String)] {
    let interactive = isInteractiveElement(element)

    var ref: String? = nil
    if interactive {
        refCounter += 1
        let r = "e\(refCounter)"
        ref = r
        refStore.setRef(r, element: element, selector: snapshotSelector(for: element), appName: appName)
    }
    let line = describeSnapshotElement(element, ref: ref)

    // A labeled non-interactive element (e.g. the Calculator display, a status
    // label) carries state a model needs, so keep it even in interactive_only
    // mode. Only empty structural containers get pruned.
    let hasLabel = snapshotElementHasLabel(element)

    var childEntries: [(Int, String)] = []
    let children = axGetArrayAttribute(element, kAXChildrenAttribute as String)
    if depth < maxDepth {
        for child in children {
            childEntries += collectSnapshot(
                element: child,
                depth: depth + 1,
                maxDepth: maxDepth,
                interactiveOnly: interactiveOnly,
                appName: appName,
                refStore: refStore,
                refCounter: &refCounter,
                truncated: &truncated
            )
        }
    } else if !children.isEmpty {
        truncated = true
    }

    // Include self if: not filtering, OR interactive, OR it carries a label,
    // OR it has included children (a needed structural ancestor).
    let includeSelf = !interactiveOnly || interactive || hasLabel || !childEntries.isEmpty
    if includeSelf {
        var result: [(Int, String)] = [(0, line)]
        result.append(contentsOf: childEntries.map { ($0.0 + 1, $0.1) })
        return result
    }
    return childEntries
}

/// Renders one element as a snapshot line (without indentation).
private func describeSnapshotElement(_ element: AXUIElement, ref: String?) -> String {
    let role = shortRole(axGetStringAttribute(element, kAXRoleAttribute as String) ?? "Unknown")
    let title = axGetStringAttribute(element, kAXTitleAttribute as String)
    let desc = axGetStringAttribute(element, kAXDescriptionAttribute as String)
    let value = axGetStringAttribute(element, kAXValueAttribute as String)
    let label = [title, desc, value].compactMap { $0 }.first(where: { !$0.isEmpty })

    var parts: [String] = [role]
    if let label = label, !label.isEmpty {
        parts.append("\"\(snapshotLabel(label))\"")
    }
    if let ref = ref {
        parts.append("[\(ref)]")
    }

    var attrs: [String] = []
    if let id = axGetStringAttribute(element, kAXIdentifierAttribute as String), !id.isEmpty {
        attrs.append("id=\(id)")
    }
    if let enabled = axGetBoolAttribute(element, kAXEnabledAttribute as String), !enabled {
        attrs.append("disabled")
    }
    if let focused = axGetBoolAttribute(element, kAXFocusedAttribute as String), focused {
        attrs.append("focused")
    }
    // Show value only when it adds information beyond the label (e.g. a text
    // field whose title is a placeholder and whose value is the typed contents).
    if let value = value, !value.isEmpty, value != label {
        attrs.append("value=\"\(snapshotLabel(value))\"")
    }
    if !attrs.isEmpty {
        parts.append("{\(attrs.joined(separator: " "))}")
    }
    return parts.joined(separator: " ")
}

/// True if the element carries a non-empty title, description, or value.
private func snapshotElementHasLabel(_ element: AXUIElement) -> Bool {
    for attr in [kAXTitleAttribute, kAXDescriptionAttribute, kAXValueAttribute] {
        if let s = axGetStringAttribute(element, attr as String), !s.isEmpty {
            return true
        }
    }
    return false
}

/// Collapses newlines and truncates a label to 80 characters (char-safe).
private func snapshotLabel(_ s: String) -> String {
    let oneLine = s.replacingOccurrences(of: "\n", with: " ").replacingOccurrences(of: "\r", with: " ")
    if oneLine.count > 80 {
        return String(oneLine.prefix(80)) + "…"
    }
    return oneLine
}

/// Builds a best-effort selector for an element so a ref can be re-resolved if
/// the cached AXUIElement goes stale.
private func snapshotSelector(for element: AXUIElement) -> SelectorParams {
    var s = SelectorParams()
    s.role = axGetStringAttribute(element, kAXRoleAttribute as String)
    if let id = axGetStringAttribute(element, kAXIdentifierAttribute as String), !id.isEmpty {
        s.identifier = id
    } else if let name = axGetStringAttribute(element, kAXTitleAttribute as String), !name.isEmpty {
        s.name = name
    }
    return s
}
