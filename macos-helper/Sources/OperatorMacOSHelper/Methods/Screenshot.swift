import AppKit
import ApplicationServices
import CoreGraphics
import Foundation

/// Handles `ui.screenshot` — captures a downscaled PNG of a window or the main
/// display and writes it to disk, returning only the path (never image bytes).
/// See SPEC_FREEZE_V0.7.md §3.
///
/// Params:
///   - `app` (string, optional): capture this app's frontmost window; omit for the main display.
///   - `window` (object, optional): reserved for future per-window scoping.
///   - `output` (string, optional): absolute output path. Default: a UUID png under the data dir.
///   - `max_width` (int, optional): long-edge downscale target. Default 1370.
///
/// Returns: `path` (string), `width` (int), `height` (int).
func handleScreenshot(_ params: [String: JSONValue]) throws -> [String: JSONValue] {
    // Screenshots need Screen Recording permission (not Accessibility), so we do
    // not call ensureAccessibility() here.
    var maxWidth = 1370
    if case .int(let w) = params["max_width"], w > 0 { maxWidth = w }

    let outputPath: String
    if case .string(let p) = params["output"], !p.isEmpty {
        outputPath = (p as NSString).expandingTildeInPath
    } else {
        let dir = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/operator-jack/screenshots")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        outputPath = dir.appendingPathComponent("\(UUID().uuidString).png").path
    }

    // Build screencapture args: a specific window if `app` resolves to one,
    // otherwise the whole (main) display. -x = no sound, -t png = format.
    var args = ["-x", "-t", "png"]
    if case .string(let appName) = params["app"] {
        let app = try resolveApp(name: appName)
        if let windowID = frontmostWindowID(for: app) {
            args += ["-l", String(windowID)]
        }
    }

    let tmp = NSTemporaryDirectory() + "opjack-\(UUID().uuidString).png"
    let capStatus = runCommand("/usr/sbin/screencapture", args + [tmp])
    guard capStatus == 0, FileManager.default.fileExists(atPath: tmp) else {
        throw HelperError(
            code: "SCREENSHOT_FAILED",
            message:
                "screencapture failed (Screen Recording permission may not be granted — check `operator-jack doctor`)"
        )
    }

    // Downscale so the longest edge is <= maxWidth (sips -Z), writing the final PNG.
    let sipsStatus = runCommand("/usr/bin/sips", ["-Z", String(maxWidth), tmp, "--out", outputPath])
    try? FileManager.default.removeItem(atPath: tmp)
    guard sipsStatus == 0, FileManager.default.fileExists(atPath: outputPath) else {
        throw HelperError(code: "SCREENSHOT_FAILED", message: "failed to write \(outputPath)")
    }

    let (width, height) = imageDimensions(outputPath) ?? (0, 0)
    return [
        "path": .string(outputPath),
        "width": .int(width),
        "height": .int(height),
    ]
}

/// Handles `ui.checkScreenRecordingPermission` — reports whether the process may
/// capture the screen (used by `doctor`).
func handleCheckScreenRecording(_ params: [String: JSONValue]) throws -> [String: JSONValue] {
    ["granted": .bool(CGPreflightScreenCaptureAccess())]
}

// MARK: - Helpers

/// Finds the CGWindowID of an app's frontmost normal (layer 0) window, choosing
/// the largest by area. Reads only ids/bounds/pids, which do not require Screen
/// Recording permission (window titles and pixel capture do).
private func frontmostWindowID(for app: NSRunningApplication) -> CGWindowID? {
    let pid = app.processIdentifier
    let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
    guard let infoList = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]]
    else { return nil }

    var best: (id: CGWindowID, area: CGFloat)? = nil
    for info in infoList {
        guard let ownerPid = info[kCGWindowOwnerPID as String] as? pid_t, ownerPid == pid else {
            continue
        }
        let layer = info[kCGWindowLayer as String] as? Int ?? 0
        guard layer == 0 else { continue }
        guard let wid = info[kCGWindowNumber as String] as? CGWindowID else { continue }
        var area: CGFloat = 0
        if let bounds = info[kCGWindowBounds as String] as? [String: CGFloat] {
            area = (bounds["Width"] ?? 0) * (bounds["Height"] ?? 0)
        }
        if best == nil || area > best!.area { best = (wid, area) }
    }
    return best?.id
}

/// Reads a PNG's pixel dimensions without decoding the whole image.
private func imageDimensions(_ path: String) -> (Int, Int)? {
    guard
        let src = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil),
        let props = CGImageSourceCopyPropertiesAtIndex(src, 0, nil) as? [CFString: Any]
    else { return nil }
    let w = props[kCGImagePropertyPixelWidth] as? Int ?? 0
    let h = props[kCGImagePropertyPixelHeight] as? Int ?? 0
    return (w, h)
}

/// Runs a command to completion, returning its exit status (-1 on spawn failure).
private func runCommand(_ path: String, _ args: [String]) -> Int32 {
    let process = Process()
    process.executableURL = URL(fileURLWithPath: path)
    process.arguments = args
    process.standardOutput = FileHandle.nullDevice
    process.standardError = FileHandle.nullDevice
    do {
        try process.run()
        process.waitUntilExit()
        return process.terminationStatus
    } catch {
        return -1
    }
}
