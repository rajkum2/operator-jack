import ApplicationServices
import AppKit
import Foundation

// MARK: - Selector Params

/// Parsed selector parameters from the IPC request.
struct SelectorParams {
    var role: String?
    var subrole: String?
    var name: String?
    var nameContains: String?
    var description: String?
    var descriptionContains: String?
    var value: String?
    var valueContains: String?
    var identifier: String?
    var path: String?
    var index: Int?
    var maxDepth: Int?
    var window: WindowScopeParams?
    var anyOf: [SelectorParams]?

    static func parse(from params: [String: JSONValue]) -> SelectorParams? {
        guard case .object(let sel) = params["selector"] else { return nil }
        return parseSelector(sel)
    }

    static func parseSelector(_ sel: [String: JSONValue]) -> SelectorParams {
        var s = SelectorParams()
        if case .string(let v) = sel["role"] { s.role = v }
        if case .string(let v) = sel["subrole"] { s.subrole = v }
        if case .string(let v) = sel["name"] { s.name = v }
        if case .string(let v) = sel["name_contains"] { s.nameContains = v }
        if case .string(let v) = sel["description"] { s.description = v }
        if case .string(let v) = sel["description_contains"] { s.descriptionContains = v }
        if case .string(let v) = sel["value"] { s.value = v }
        if case .string(let v) = sel["value_contains"] { s.valueContains = v }
        if case .string(let v) = sel["identifier"] { s.identifier = v }
        if case .string(let v) = sel["path"] { s.path = v }
        if case .int(let v) = sel["index"] { s.index = v }
        if case .int(let v) = sel["max_depth"] { s.maxDepth = v }

        // Window scope
        if case .object(let w) = sel["window"] {
            var ws = WindowScopeParams()
            if case .int(let v) = w["index"] { ws.index = v }
            if case .string(let v) = w["title_contains"] { ws.titleContains = v }
            s.window = ws
        }

        // anyOf alternatives
        if case .array(let alternatives) = sel["any_of"] {
            var alts: [SelectorParams] = []
            for alt in alternatives {
                if case .object(let altObj) = alt {
                    alts.append(parseSelector(altObj))
                }
            }
            if !alts.isEmpty {
                s.anyOf = alts
            }
        }

        return s
    }

    /// True if the selector has at least one field the matcher actually checks.
    /// (`path` is a real matcher as of M7 — a suffix match on the computed
    /// traversal path.) `index`, `window`, and `maxDepth` are filters/scope, not
    /// content matchers; `anyOf` is resolved separately by the caller.
    var hasMatchableCriteria: Bool {
        role != nil || subrole != nil || name != nil || nameContains != nil
            || description != nil || descriptionContains != nil || value != nil
            || valueContains != nil || identifier != nil || path != nil
    }
}

/// Window scoping parameters.
struct WindowScopeParams {
    var index: Int?
    var titleContains: String?
}

// MARK: - Found Element

/// A matched element from AX tree traversal.
struct FoundElement {
    let element: AXUIElement
    let metadata: [String: JSONValue]
    let path: String
}

// MARK: - App Resolution

/// Finds a running application by name (case-insensitive).
func resolveApp(name: String) throws -> NSRunningApplication {
    let workspace = NSWorkspace.shared
    let candidates = workspace.runningApplications.filter { app in
        guard app.activationPolicy == .regular else { return false }
        guard let appName = app.localizedName else { return false }
        return appName.localizedCaseInsensitiveCompare(name) == .orderedSame
    }

    guard let app = candidates.first else {
        // Check if any app with this name exists but is terminated
        let allApps = workspace.runningApplications.filter { app in
            app.localizedName?.localizedCaseInsensitiveCompare(name) == .orderedSame
        }
        if !allApps.isEmpty {
            throw HelperError(code: "APP_NOT_RUNNING", message: "App '\(name)' is not running (may be background-only)", retryable: true)
        }
        throw HelperError(code: "APP_NOT_FOUND", message: "No running app found matching '\(name)'")
    }

    return app
}

// MARK: - AX Element Helpers

/// Creates an AXUIElement for an application by PID.
///
/// On creation we (best-effort) enable Chromium/Electron accessibility and bound
/// the per-message timeout so a busy target degrades instead of hanging every
/// attribute read. `AXManualAccessibility` is the documented Chromium flag that
/// makes web/Electron content (Chrome web areas, Slack, VS Code, Discord)
/// visible to assistive clients; without it those trees are empty.
func axAppElement(for app: NSRunningApplication) -> AXUIElement {
    let element = AXUIElementCreateApplication(app.processIdentifier)
    AXUIElementSetAttributeValue(element, "AXManualAccessibility" as CFString, kCFBooleanTrue)
    AXUIElementSetMessagingTimeout(element, 5.0)
    return element
}

/// Reads an AX attribute, returning nil on failure.
func axGetAttribute(_ element: AXUIElement, _ attribute: String) -> CFTypeRef? {
    var value: CFTypeRef?
    let result = AXUIElementCopyAttributeValue(element, attribute as CFString, &value)
    guard result == .success else { return nil }
    return value
}

/// Reads a string-typed AX attribute.
func axGetStringAttribute(_ element: AXUIElement, _ attr: String) -> String? {
    guard let value = axGetAttribute(element, attr) else { return nil }
    return value as? String
}

/// Reads a boolean-typed AX attribute.
func axGetBoolAttribute(_ element: AXUIElement, _ attr: String) -> Bool? {
    guard let value = axGetAttribute(element, attr) else { return nil }
    if let num = value as? NSNumber {
        return num.boolValue
    }
    return value as? Bool
}

/// Reads an array-typed AX attribute as [AXUIElement].
func axGetArrayAttribute(_ element: AXUIElement, _ attr: String) -> [AXUIElement] {
    var count: CFIndex = 0
    let countResult = AXUIElementGetAttributeValueCount(element, attr as CFString, &count)
    guard countResult == .success, count > 0 else { return [] }

    var values: CFArray?
    let result = AXUIElementCopyAttributeValues(element, attr as CFString, 0, count, &values)
    guard result == .success, let array = values as? [AXUIElement] else { return [] }
    return array
}

/// PIDs whose accessibility tree we have already waited to settle this session.
private var settledAxPids = Set<pid_t>()

/// Chromium/Electron apps build their a11y tree lazily after `AXManualAccessibility`
/// is set (see `axAppElement`), so the first read can be empty. On the first
/// observation of an app this session, wait until the shallow node count
/// stabilizes (or a timeout), so the first snapshot/find sees real content.
/// Native apps stabilize on the first check (~one poll interval); web apps take
/// up to the cap. No-op on subsequent calls for the same PID.
func settleAccessibilityTree(app: NSRunningApplication, root: AXUIElement) {
    let pid = app.processIdentifier
    if settledAxPids.contains(pid) { return }
    settledAxPids.insert(pid)

    func shallowCount(_ el: AXUIElement, _ depth: Int) -> Int {
        if depth <= 0 { return 1 }
        var n = 1
        for child in axGetArrayAttribute(el, kAXChildrenAttribute as String) {
            n += shallowCount(child, depth - 1)
        }
        return n
    }

    var last = shallowCount(root, 5)
    let deadline = Date().addingTimeInterval(2.0)
    while Date() < deadline {
        Thread.sleep(forTimeInterval: 0.3)
        let now = shallowCount(root, 5)
        if now == last { break }  // tree stopped growing
        last = now
    }
}

/// Returns the AX action names supported by an element (e.g. "AXPress").
func axActionNames(_ element: AXUIElement) -> [String] {
    var names: CFArray?
    let result = AXUIElementCopyActionNames(element, &names)
    guard result == .success, let arr = names as? [String] else { return [] }
    return arr
}

/// Whether an element is actionable: it supports AXPress (buttons, checkboxes,
/// links, menu items, popups) or its value is settable (text fields, sliders).
/// These are the elements that get a `ref` in a snapshot.
func isInteractiveElement(_ element: AXUIElement) -> Bool {
    if axActionNames(element).contains(kAXPressAction as String) {
        return true
    }
    var settable: DarwinBoolean = false
    let r = AXUIElementIsAttributeSettable(element, kAXValueAttribute as CFString, &settable)
    return r == .success && settable.boolValue
}

/// Shortens an AX role for compact snapshots: strips the `AX` prefix and
/// lowercases (`AXButton` -> `button`, `AXStaticText` -> `statictext`).
func shortRole(_ role: String) -> String {
    let stripped = role.hasPrefix("AX") ? String(role.dropFirst(2)) : role
    return stripped.lowercased()
}

// MARK: - Permission Check

/// Ensures accessibility permission is granted. Throws PERMISSION_DENIED if not.
func ensureAccessibility() throws {
    guard AXIsProcessTrusted() else {
        throw HelperError(
            code: "PERMISSION_DENIED",
            message: "Accessibility permission not granted. Grant access in System Settings > Privacy & Security > Accessibility."
        )
    }
}

// MARK: - Window Enumeration

/// Window metadata from AX enumeration.
struct WindowInfo {
    let element: AXUIElement
    let title: String
    let index: Int
    let isMain: Bool
    let isFocused: Bool
}

/// Lists all windows for an app's AX element.
func listWindows(appElement: AXUIElement) -> [WindowInfo] {
    let windows = axGetArrayAttribute(appElement, kAXWindowsAttribute as String)
    var result: [WindowInfo] = []

    for (i, win) in windows.enumerated() {
        let title = axGetStringAttribute(win, kAXTitleAttribute as String) ?? ""
        let isMain = axGetBoolAttribute(win, kAXMainAttribute as String) ?? false
        let isFocused = axGetBoolAttribute(win, kAXFocusedAttribute as String) ?? false
        result.append(WindowInfo(element: win, title: title, index: i, isMain: isMain, isFocused: isFocused))
    }

    return result
}

/// Resolves a specific window by scope, or returns the frontmost window.
func resolveWindow(appElement: AXUIElement, scope: WindowScopeParams?) throws -> AXUIElement {
    let windows = listWindows(appElement: appElement)

    guard !windows.isEmpty else {
        throw HelperError(code: "WINDOW_NOT_FOUND", message: "App has no open windows")
    }

    guard let scope = scope else {
        // Default: return the first (frontmost) window
        return windows[0].element
    }

    if let idx = scope.index {
        guard idx >= 0 && idx < windows.count else {
            throw HelperError(code: "WINDOW_NOT_FOUND", message: "Window index \(idx) out of range (app has \(windows.count) windows)")
        }
        return windows[idx].element
    }

    if let titleContains = scope.titleContains {
        guard let win = windows.first(where: { $0.title.localizedCaseInsensitiveContains(titleContains) }) else {
            let titles = windows.map { $0.title }.joined(separator: ", ")
            throw HelperError(code: "WINDOW_NOT_FOUND", message: "No window with title containing '\(titleContains)'. Available: [\(titles)]")
        }
        return win.element
    }

    // Fallback
    return windows[0].element
}

// MARK: - Element Serialization

/// Serializes an AX element's key attributes to a JSONValue dictionary.
func serializeElement(_ element: AXUIElement) -> [String: JSONValue] {
    var dict: [String: JSONValue] = [:]

    if let role = axGetStringAttribute(element, kAXRoleAttribute as String) {
        dict["role"] = .string(role)
    }
    if let subrole = axGetStringAttribute(element, kAXSubroleAttribute as String) {
        dict["subrole"] = .string(subrole)
    }
    if let name = axGetStringAttribute(element, kAXTitleAttribute as String) {
        dict["name"] = .string(name)
    }
    if let desc = axGetStringAttribute(element, kAXDescriptionAttribute as String) {
        dict["description"] = .string(desc)
    }
    if let value = axGetStringAttribute(element, kAXValueAttribute as String) {
        dict["value"] = .string(value)
    }
    if let identifier = axGetStringAttribute(element, kAXIdentifierAttribute as String) {
        dict["identifier"] = .string(identifier)
    }
    if let enabled = axGetBoolAttribute(element, kAXEnabledAttribute as String) {
        dict["enabled"] = .bool(enabled)
    }
    if let focused = axGetBoolAttribute(element, kAXFocusedAttribute as String) {
        dict["focused"] = .bool(focused)
    }

    let children = axGetArrayAttribute(element, kAXChildrenAttribute as String)
    dict["children_count"] = .int(children.count)

    return dict
}

// MARK: - AX Tree Traversal & Selector Matching

/// Traverses the AX tree in pre-order depth-first order, matching elements
/// against the selector. Returns up to `maxResults` matches.
func findElements(
    root: AXUIElement,
    selector: SelectorParams,
    maxDepth: Int,
    maxResults: Int
) -> [FoundElement] {
    var results: [FoundElement] = []
    traverseTree(
        element: root,
        selector: selector,
        currentDepth: 0,
        maxDepth: maxDepth,
        maxResults: maxResults,
        pathPrefix: "",
        siblingCounts: [:],
        results: &results
    )

    // Apply index filter if specified (post-filter on the full result set)
    if let idx = selector.index {
        if idx >= 0 && idx < results.count {
            return [results[idx]]
        }
        return []
    }

    return results
}

private func traverseTree(
    element: AXUIElement,
    selector: SelectorParams,
    currentDepth: Int,
    maxDepth: Int,
    maxResults: Int,
    pathPrefix: String,
    siblingCounts: [String: Int],
    results: inout [FoundElement]
) {
    guard results.count < maxResults else { return }
    guard currentDepth <= maxDepth else { return }

    let role = axGetStringAttribute(element, kAXRoleAttribute as String) ?? "Unknown"
    let currentPath = pathPrefix.isEmpty ? role : "\(pathPrefix)/\(role)"

    // Check if this element matches the selector — including the root itself, so
    // a selector like {role: "AXWindow"} against a window root can match. (Before,
    // the root was skipped, making window-targeting selectors silently unmatchable.)
    if matchesSelector(element: element, selector: selector, role: role, path: currentPath) {
        let metadata = serializeElement(element)
        var metaWithPath = metadata
        metaWithPath["path"] = .string(currentPath)
        results.append(FoundElement(element: element, metadata: metaWithPath, path: currentPath))
    }

    guard results.count < maxResults else { return }
    guard currentDepth < maxDepth else { return }

    // Recurse into children
    let children = axGetArrayAttribute(element, kAXChildrenAttribute as String)
    var childRoleCounts: [String: Int] = [:]

    for child in children {
        guard results.count < maxResults else { return }

        let childRole = axGetStringAttribute(child, kAXRoleAttribute as String) ?? "Unknown"
        let roleIndex = childRoleCounts[childRole, default: 0]
        childRoleCounts[childRole] = roleIndex + 1

        let childPath = "\(currentPath)/\(childRole)[\(roleIndex)]"

        traverseTree(
            element: child,
            selector: selector,
            currentDepth: currentDepth + 1,
            maxDepth: maxDepth,
            maxResults: maxResults,
            pathPrefix: childPath.isEmpty ? childRole : childPath,
            siblingCounts: childRoleCounts,
            results: &results
        )
    }
}

/// Checks if a single AX element matches all specified selector criteria.
private func matchesSelector(
    element: AXUIElement, selector: SelectorParams, role: String, path: String
) -> Bool {
    // A selector with no matchable criteria must match NOTHING, not everything.
    // (Rust validation rejects such selectors before they reach here; this is
    // defense-in-depth for direct-driven callers and tests.)
    guard selector.hasMatchableCriteria else { return false }

    // Path: the element's computed traversal path must END WITH the given path
    // (e.g. selector "AXButton[2]" matches ".../AXGroup[0]/AXButton[2]").
    if let sPath = selector.path {
        guard path.hasSuffix(sPath) else { return false }
    }

    // Role: exact match
    if let sRole = selector.role {
        guard role == sRole else { return false }
    }

    // Subrole: exact match
    if let sSubrole = selector.subrole {
        let subrole = axGetStringAttribute(element, kAXSubroleAttribute as String)
        guard subrole == sSubrole else { return false }
    }

    // Name: exact match
    if let sName = selector.name {
        let name = axGetStringAttribute(element, kAXTitleAttribute as String)
        guard name == sName else { return false }
    }

    // Name contains: substring
    if let sNameContains = selector.nameContains {
        let name = axGetStringAttribute(element, kAXTitleAttribute as String) ?? ""
        guard name.localizedCaseInsensitiveContains(sNameContains) else { return false }
    }

    // Description: exact match
    if let sDesc = selector.description {
        let desc = axGetStringAttribute(element, kAXDescriptionAttribute as String)
        guard desc == sDesc else { return false }
    }

    // Description contains: substring
    if let sDescContains = selector.descriptionContains {
        let desc = axGetStringAttribute(element, kAXDescriptionAttribute as String) ?? ""
        guard desc.localizedCaseInsensitiveContains(sDescContains) else { return false }
    }

    // Value: exact match
    if let sValue = selector.value {
        let value = axGetStringAttribute(element, kAXValueAttribute as String)
        guard value == sValue else { return false }
    }

    // Value contains: substring
    if let sValueContains = selector.valueContains {
        let value = axGetStringAttribute(element, kAXValueAttribute as String) ?? ""
        guard value.localizedCaseInsensitiveContains(sValueContains) else { return false }
    }

    // Identifier: exact match
    if let sIdentifier = selector.identifier {
        let identifier = axGetStringAttribute(element, kAXIdentifierAttribute as String)
        guard identifier == sIdentifier else { return false }
    }

    return true
}

// MARK: - anyOf Resolution

/// Resolves a selector that may contain an `anyOf` array. Tries each alternative
/// in order; the first strategy returning exactly 1 match wins. If none match
/// exactly 1, falls back to the first strategy that returned any matches.
func resolveAnyOf(
    root: AXUIElement,
    alternatives: [SelectorParams],
    maxResults: Int = 100
) -> [FoundElement] {
    var firstNonEmpty: [FoundElement]? = nil

    for alt in alternatives {
        let maxDepth = alt.maxDepth ?? 12
        let results = findElements(root: root, selector: alt, maxDepth: maxDepth, maxResults: maxResults)
        if results.count == 1 {
            return results
        }
        if firstNonEmpty == nil && !results.isEmpty {
            firstNonEmpty = results
        }
    }

    // No exact single match — return the first non-empty result set for disambiguation
    return firstNonEmpty ?? []
}

// MARK: - Implicit Wait

/// Polls for elements matching the selector until found or timeout.
/// Handles `anyOf` selectors transparently.
/// Returns matched elements on success, throws ELEMENT_NOT_FOUND on timeout.
func implicitWaitForElement(
    root: AXUIElement,
    selector: SelectorParams,
    waitMs: Int,
    maxResults: Int = 100
) throws -> [FoundElement] {
    let pollIntervalMs = 200
    let maxIterations = max(1, waitMs / pollIntervalMs)
    let hasAnyOf = selector.anyOf != nil && !(selector.anyOf!.isEmpty)

    for i in 0..<maxIterations {
        let results: [FoundElement]
        if hasAnyOf {
            results = resolveAnyOf(root: root, alternatives: selector.anyOf!, maxResults: maxResults)
        } else {
            let maxDepth = selector.maxDepth ?? 12
            results = findElements(root: root, selector: selector, maxDepth: maxDepth, maxResults: maxResults)
        }
        if !results.isEmpty {
            return results
        }
        if i < maxIterations - 1 {
            Thread.sleep(forTimeInterval: Double(pollIntervalMs) / 1000.0)
        }
    }

    // Final attempt
    let finalResults: [FoundElement]
    if hasAnyOf {
        finalResults = resolveAnyOf(root: root, alternatives: selector.anyOf!, maxResults: maxResults)
    } else {
        let maxDepth = selector.maxDepth ?? 12
        finalResults = findElements(root: root, selector: selector, maxDepth: maxDepth, maxResults: maxResults)
    }
    if !finalResults.isEmpty {
        return finalResults
    }

    throw HelperError(
        code: "ELEMENT_NOT_FOUND",
        message: "No element matching selector found within \(waitMs)ms",
        retryable: true
    )
}

// MARK: - Disambiguation

/// Resolves a single element from a list of matches.
/// If exactly 1 match, returns it. If index is specified, returns that index.
/// Otherwise throws ELEMENT_AMBIGUOUS with candidates in details.
func disambiguate(results: [FoundElement], index: Int?) throws -> FoundElement {
    if results.count == 1 {
        return results[0]
    }

    if let idx = index {
        guard idx >= 0 && idx < results.count else {
            throw HelperError(
                code: "ELEMENT_NOT_FOUND",
                message: "Index \(idx) out of range (found \(results.count) matches)"
            )
        }
        return results[idx]
    }

    // Multiple matches, no index — ambiguous
    let candidates: [JSONValue] = results.enumerated().map { (i, el) in
        var dict = el.metadata
        dict["index"] = .int(i)
        return .object(dict)
    }

    throw HelperError(
        code: "ELEMENT_AMBIGUOUS",
        message: "Selector matched \(results.count) elements. Specify 'index' or refine selector.",
        details: ["candidates": .array(candidates)]
    )
}

// MARK: - Common Param Extraction

/// Extracts the `app` string param and resolves the running application.
func extractApp(from params: [String: JSONValue]) throws -> NSRunningApplication {
    guard case .string(let appName) = params["app"] else {
        throw HelperError(code: "INVALID_PARAMS", message: "Missing required 'app' parameter")
    }
    return try resolveApp(name: appName)
}

/// Extracts implicit_wait_ms from params, defaulting to 2000.
func extractImplicitWaitMs(from params: [String: JSONValue]) -> Int {
    if case .int(let v) = params["implicit_wait_ms"] {
        return v
    }
    return 2000
}

// MARK: - Evidence Gathering

/// Gathers contextual evidence about the current UI state for audit/debugging.
/// Returns a dictionary with: active app, active window title, and optionally
/// a shallow subtree of the target element (2 levels).
func gatherEvidence(app: NSRunningApplication, element: AXUIElement? = nil) -> [String: JSONValue] {
    var evidence: [String: JSONValue] = [:]

    // Active app info
    evidence["active_app"] = .string(app.localizedName ?? "unknown")
    evidence["active_app_pid"] = .int(Int(app.processIdentifier))
    evidence["app_is_active"] = .bool(app.isActive)

    // Active window title
    let appElement = axAppElement(for: app)
    let windows = listWindows(appElement: appElement)
    if let mainWindow = windows.first(where: { $0.isMain }) ?? windows.first {
        evidence["active_window"] = .string(mainWindow.title)
    }

    // Element subtree (2 levels) if provided
    if let el = element {
        var subtree = serializeElement(el)
        let children = axGetArrayAttribute(el, kAXChildrenAttribute as String)
        if !children.isEmpty {
            let childNodes: [JSONValue] = children.prefix(10).map { child in
                .object(serializeElement(child))
            }
            subtree["children"] = .array(childNodes)
        }
        evidence["element_subtree"] = .object(subtree)
    }

    return evidence
}

// MARK: - Element Ref Store

/// Stores element references (ULIDs → AXUIElement) for reuse across actions.
/// Best-effort: if the element becomes stale, callers fall back to re-resolution.
final class ElementRefStore {
    static let shared = ElementRefStore()

    private var store: [String: (element: AXUIElement, selector: SelectorParams, appName: String)] = [:]

    private init() {}

    /// Generates a ULID-style unique ID for an element reference.
    func generateRef() -> String {
        // Simple time-based unique ID (not full ULID, but unique enough for in-process use)
        let time = UInt64(Date().timeIntervalSince1970 * 1000)
        let random = UInt32.random(in: 0...UInt32.max)
        return String(format: "%016llX%08X", time, random)
    }

    /// Stores an element and returns its reference ID.
    func store(element: AXUIElement, selector: SelectorParams, appName: String) -> String {
        let ref_id = generateRef()
        store[ref_id] = (element: element, selector: selector, appName: appName)
        evictIfNeeded()
        return ref_id
    }

    /// Stores an element under a caller-chosen reference id (e.g. a snapshot's
    /// short `e1`/`e2` handles).
    func setRef(_ refId: String, element: AXUIElement, selector: SelectorParams, appName: String) {
        store[refId] = (element: element, selector: selector, appName: appName)
    }

    /// Bounds the store so long-lived sessions (many `ui.find` calls) cannot grow
    /// it without limit. Snapshots additionally `clear()` before minting.
    private func evictIfNeeded() {
        let cap = 1000
        if store.count > cap {
            // Drop an arbitrary chunk; refs are best-effort and callers re-resolve.
            for key in store.keys.prefix(store.count - cap) {
                store.removeValue(forKey: key)
            }
        }
    }

    /// Retrieves an element by reference ID. Returns nil if not found.
    func get(_ ref_id: String) -> (element: AXUIElement, selector: SelectorParams, appName: String)? {
        return store[ref_id]
    }

    /// Checks if a stored element is still valid by attempting to read its role.
    func isValid(_ ref_id: String) -> Bool {
        guard let entry = store[ref_id] else { return false }
        return axGetStringAttribute(entry.element, kAXRoleAttribute as String) != nil
    }

    /// Clears all stored references.
    func clear() {
        store.removeAll()
    }
}

/// Resolves an element either from an `element_ref` or by selector lookup.
/// If `element_ref` is provided and still valid, returns it directly.
/// If stale or not found, falls back to selector-based resolution.
func resolveElementRef(
    params: [String: JSONValue],
    appElement: AXUIElement,
    windowScope: WindowScopeParams?
) throws -> FoundElement? {
    guard case .string(let refId) = params["element_ref"] else {
        return nil // No element_ref provided, caller should use selector
    }

    let refStore = ElementRefStore.shared

    // Try to use the cached element
    if refStore.isValid(refId), let entry = refStore.get(refId) {
        let metadata = serializeElement(entry.element)
        return FoundElement(element: entry.element, metadata: metadata, path: "")
    }

    // Element is stale — fall back to selector if available
    if let selector = SelectorParams.parse(from: params) {
        let windowElement = try resolveWindow(appElement: appElement, scope: windowScope ?? selector.window)
        let waitMs = extractImplicitWaitMs(from: params)
        let results = try implicitWaitForElement(root: windowElement, selector: selector, waitMs: waitMs)
        return try disambiguate(results: results, index: selector.index)
    }

    throw HelperError(
        code: "ELEMENT_NOT_FOUND",
        message: "Element ref '\(refId)' is stale and no selector provided for fallback"
    )
}

/// Resolves the target element for an action handler (click/readText/setValue).
/// Prefers `element_ref` (cached AXUIElement, with selector fallback when stale),
/// otherwise resolves via `selector` with an implicit wait + disambiguation.
/// Throws INVALID_PARAMS if neither an element_ref nor a selector is provided.
func resolveActionTarget(
    params: [String: JSONValue],
    app: NSRunningApplication
) throws -> FoundElement {
    let appElement = axAppElement(for: app)

    // element_ref path (returns nil only when no element_ref key is present).
    if let viaRef = try resolveElementRef(params: params, appElement: appElement, windowScope: nil) {
        return viaRef
    }

    // Selector path.
    guard let selector = SelectorParams.parse(from: params) else {
        throw HelperError(
            code: "INVALID_PARAMS",
            message: "Missing required 'selector' or 'element_ref' parameter"
        )
    }
    let waitMs = extractImplicitWaitMs(from: params)
    let windowElement = try resolveWindow(appElement: appElement, scope: selector.window)
    let results = try implicitWaitForElement(root: windowElement, selector: selector, waitMs: waitMs)
    return try disambiguate(results: results, index: selector.index)
}
