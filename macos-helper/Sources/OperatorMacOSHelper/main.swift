import Foundation

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

let maxLineBytes = 1_048_576  // 1 MiB

// ---------------------------------------------------------------------------
// Register methods
// ---------------------------------------------------------------------------

var dispatcher = MethodDispatcher()
dispatcher.register("ui.ping", handler: handlePing)
dispatcher.register("ui.checkAccessibilityPermission", handler: handleCheckAccessibility)
dispatcher.register("ui.listApps", handler: handleListApps)

// ---------------------------------------------------------------------------
// JSON encoder/decoder
// ---------------------------------------------------------------------------

let decoder = JSONDecoder()
let encoder = JSONEncoder()
encoder.outputFormatting = [.sortedKeys]  // deterministic output, no pretty-print

// ---------------------------------------------------------------------------
// NDJSON server loop: read stdin line-by-line, dispatch, write to stdout
// ---------------------------------------------------------------------------

/// Write a single NDJSON line to stdout. Uses FileHandle to bypass Swift
/// buffering over pipes.
func writeResponse(_ response: IpcResponse) {
    do {
        let data = try encoder.encode(response)
        var output = data
        output.append(0x0A)  // newline
        FileHandle.standardOutput.write(output)
    } catch {
        // Last resort: write a minimal error JSON manually.
        let fallback = "{\"id\":\"\",\"ok\":false,\"error\":{\"code\":\"ENCODE_ERROR\",\"message\":\"Failed to encode response\",\"retryable\":false}}\n"
        if let fallbackData = fallback.data(using: .utf8) {
            FileHandle.standardOutput.write(fallbackData)
        }
    }
}

// Log startup to stderr (diagnostics never go to stdout).
FileHandle.standardError.write("operator-macos-helper: started, waiting for requests on stdin\n".data(using: .utf8)!)

while let line = readLine(strippingNewline: true) {
    // Guard against oversized lines.
    if line.utf8.count > maxLineBytes {
        let errorResponse = IpcResponse.failure(
            id: "",
            code: "LINE_TOO_LONG",
            message: "Request line exceeds 1 MiB limit"
        )
        writeResponse(errorResponse)
        continue
    }

    // Skip empty lines.
    if line.isEmpty {
        continue
    }

    // Decode request.
    guard let lineData = line.data(using: .utf8) else {
        let errorResponse = IpcResponse.failure(
            id: "",
            code: "INVALID_UTF8",
            message: "Request line is not valid UTF-8"
        )
        writeResponse(errorResponse)
        continue
    }

    let request: IpcRequest
    do {
        request = try decoder.decode(IpcRequest.self, from: lineData)
    } catch {
        let errorResponse = IpcResponse.failure(
            id: "",
            code: "INVALID_JSON",
            message: "Failed to parse request: \(error.localizedDescription)"
        )
        writeResponse(errorResponse)
        continue
    }

    // Dispatch and respond.
    let response = dispatcher.dispatch(request: request)
    writeResponse(response)
}

// stdin EOF — clean exit.
FileHandle.standardError.write("operator-macos-helper: stdin closed, exiting\n".data(using: .utf8)!)
Foundation.exit(0)
