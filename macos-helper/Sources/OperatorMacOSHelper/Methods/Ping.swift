import Foundation

/// Handles `ui.ping` — health check / handshake.
/// Returns protocol version and helper version for the Rust client to validate.
func handlePing(_ params: [String: JSONValue]) throws -> [String: JSONValue] {
    return [
        "protocol_version": .string("1"),
        "helper_version": .string("0.1.0"),
    ]
}
