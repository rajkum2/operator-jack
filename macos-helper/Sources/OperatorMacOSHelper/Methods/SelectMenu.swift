import ApplicationServices
import AppKit
import Foundation

/// Handles `ui.selectMenu` — navigates and selects a menu item by path.
///
/// Params:
///   - `app` (string, required): Application name.
///   - `menu_path` (string[], required): Menu path (e.g. ["File", "Save As..."]).
///
/// Returns:
///   - `selected` (bool): Whether the menu item was selected.
///   - `menu_path` (array): The menu path that was navigated.
func handleSelectMenu(_ params: [String: JSONValue]) throws -> [String: JSONValue] {
    try ensureAccessibility()
    let app = try extractApp(from: params)

    guard case .array(let pathValues) = params["menu_path"] else {
        throw HelperError(code: "INVALID_PARAMS", message: "Missing required 'menu_path' parameter (must be string array)")
    }

    var menuPath: [String] = []
    for val in pathValues {
        guard case .string(let s) = val else {
            throw HelperError(code: "INVALID_PARAMS", message: "Each element in 'menu_path' must be a string")
        }
        menuPath.append(s)
    }

    guard !menuPath.isEmpty else {
        throw HelperError(code: "INVALID_PARAMS", message: "'menu_path' must not be empty")
    }

    // Bring app to foreground
    app.activate(options: .activateIgnoringOtherApps)
    Thread.sleep(forTimeInterval: 0.1)

    let appElement = axAppElement(for: app)

    // Get the menu bar
    guard let menuBar = axGetAttribute(appElement, kAXMenuBarAttribute as String) else {
        throw HelperError(code: "ELEMENT_NOT_FOUND", message: "Could not access menu bar for app '\(app.localizedName ?? "unknown")'")
    }
    let menuBarElement = menuBar as! AXUIElement

    // Walk the menu path
    var currentElement = menuBarElement

    for (i, segment) in menuPath.enumerated() {
        let isLast = (i == menuPath.count - 1)

        // Get children of current menu level
        let children = axGetArrayAttribute(currentElement, kAXChildrenAttribute as String)

        // Find the menu item matching this segment by title
        var foundItem: AXUIElement? = nil
        for child in children {
            let title = axGetStringAttribute(child, kAXTitleAttribute as String) ?? ""
            if title == segment {
                foundItem = child
                break
            }
            // Also check children of each child (menu bar items have a nested structure)
            let subChildren = axGetArrayAttribute(child, kAXChildrenAttribute as String)
            for subChild in subChildren {
                let subTitle = axGetStringAttribute(subChild, kAXTitleAttribute as String) ?? ""
                if subTitle == segment {
                    foundItem = subChild
                    break
                }
            }
            if foundItem != nil { break }
        }

        guard let item = foundItem else {
            let available = children.compactMap { axGetStringAttribute($0, kAXTitleAttribute as String) }
            throw HelperError(
                code: "ELEMENT_NOT_FOUND",
                message: "Menu item '\(segment)' not found at level \(i). Available: [\(available.joined(separator: ", "))]"
            )
        }

        if isLast {
            // Select the final menu item
            let pressResult = AXUIElementPerformAction(item, kAXPressAction as CFString)
            guard pressResult == .success else {
                throw HelperError(
                    code: "ELEMENT_NOT_ACTIONABLE",
                    message: "Failed to press menu item '\(segment)' (error \(pressResult.rawValue))",
                    retryable: true
                )
            }
        } else {
            // Open the submenu by pressing, then navigate into it
            let pressResult = AXUIElementPerformAction(item, kAXPressAction as CFString)
            guard pressResult == .success else {
                throw HelperError(
                    code: "ELEMENT_NOT_ACTIONABLE",
                    message: "Failed to open menu '\(segment)' (error \(pressResult.rawValue))",
                    retryable: true
                )
            }

            // Wait for submenu to appear
            Thread.sleep(forTimeInterval: 0.1)

            // Get the submenu children — the pressed item should now have children
            let submenus = axGetArrayAttribute(item, kAXChildrenAttribute as String)
            if let submenu = submenus.first {
                currentElement = submenu
            } else {
                // Try polling briefly for the submenu to appear
                var found = false
                for _ in 0..<5 {
                    Thread.sleep(forTimeInterval: 0.1)
                    let retrySubmenus = axGetArrayAttribute(item, kAXChildrenAttribute as String)
                    if let submenu = retrySubmenus.first {
                        currentElement = submenu
                        found = true
                        break
                    }
                }
                if !found {
                    throw HelperError(
                        code: "ELEMENT_NOT_FOUND",
                        message: "Submenu for '\(segment)' did not appear within timeout"
                    )
                }
            }
        }
    }

    let resultPath: [JSONValue] = menuPath.map { .string($0) }
    return [
        "selected": .bool(true),
        "menu_path": .array(resultPath),
        "_evidence": .object(gatherEvidence(app: app)),
    ]
}
