import AppKit
import Foundation

/// Handles `ui.listApps` — lists running applications with activation policy `.regular`.
///
/// Returns:
///   - `apps` (array): list of app objects with `name`, `bundle_id`, `pid`, `active`.
func handleListApps(_ params: [String: JSONValue]) throws -> [String: JSONValue] {
    let workspace = NSWorkspace.shared
    let runningApps = workspace.runningApplications

    var apps: [JSONValue] = []
    for app in runningApps {
        // Only include regular (dock-visible) apps.
        guard app.activationPolicy == .regular else { continue }

        let name: JSONValue = app.localizedName.map { .string($0) } ?? .null
        let bundleId: JSONValue = app.bundleIdentifier.map { .string($0) } ?? .null
        let pid: JSONValue = .int(Int(app.processIdentifier))
        let active: JSONValue = .bool(app.isActive)

        apps.append(.object([
            "name": name,
            "bundle_id": bundleId,
            "pid": pid,
            "active": active,
        ]))
    }

    return [
        "apps": .array(apps),
    ]
}
