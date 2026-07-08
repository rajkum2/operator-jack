import ApplicationServices
import AppKit
import Foundation

/// Handles `ui.readText` — reads text/value from a UI element.
///
/// Reads attributes in priority order: kAXValueAttribute, kAXTitleAttribute, kAXDescriptionAttribute.
///
/// Params:
///   - `app` (string, required): Application name.
///   - `selector` (object, required): Selector criteria.
///   - `implicit_wait_ms` (int, optional): Wait time for element (default 2000).
///
/// Returns:
///   - `text` (string): The element's text value.
///   - `source_attribute` (string): Which AX attribute the value came from.
///   - `target` (object): The matched element's metadata.
func handleReadText(_ params: [String: JSONValue]) throws -> [String: JSONValue] {
    try ensureAccessibility()
    let app = try extractApp(from: params)

    // Resolve target via element_ref (preferred) or selector.
    let match = try resolveActionTarget(params: params, app: app)

    // Read text with priority: value > title > description
    let attributePriority: [(attr: String, label: String)] = [
        (kAXValueAttribute as String, "kAXValueAttribute"),
        (kAXTitleAttribute as String, "kAXTitleAttribute"),
        (kAXDescriptionAttribute as String, "kAXDescriptionAttribute"),
    ]

    let evidence = gatherEvidence(app: app, element: match.element)

    for (attr, label) in attributePriority {
        if let text = axGetStringAttribute(match.element, attr) {
            return [
                "text": .string(text),
                "source_attribute": .string(label),
                "target": .object(match.metadata),
                "_evidence": .object(evidence),
            ]
        }
    }

    // No text found — return empty string
    return [
        "text": .string(""),
        "source_attribute": .string("none"),
        "target": .object(match.metadata),
        "_evidence": .object(evidence),
    ]
}
