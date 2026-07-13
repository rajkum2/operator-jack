import AppKit
import Foundation
import PDFKit
import Vision

/// Handles `doc.pdf_text` (M14) — extracts text from a PDF via PDFKit.
///
/// Params:
///   - `path` (string, required): PDF file path (`~` expanded).
///   - `max_pages` (int, optional): cap on pages read.
///
/// Returns: `text`, `page_count`, `chars`.
func handlePdfText(_ params: [String: JSONValue]) throws -> [String: JSONValue] {
    guard case .string(let rawPath)? = params["path"] else {
        throw HelperError(code: "MISSING_PARAM", message: "doc.pdf_text requires 'path'")
    }
    let path = (rawPath as NSString).expandingTildeInPath
    let url = URL(fileURLWithPath: path)
    guard FileManager.default.fileExists(atPath: path) else {
        throw HelperError(code: "DOC_NOT_FOUND", message: "no file at \(path)")
    }
    guard let doc = PDFDocument(url: url) else {
        throw HelperError(code: "DOC_READ_FAILED", message: "could not open PDF at \(path)")
    }

    var maxPages = doc.pageCount
    if case .int(let m)? = params["max_pages"] { maxPages = min(maxPages, max(0, m)) }

    var text = ""
    for i in 0..<maxPages {
        if let page = doc.page(at: i), let s = page.string {
            text += s
            if i < maxPages - 1 { text += "\n" }
        }
    }

    return [
        "text": .string(text),
        "page_count": .int(doc.pageCount),
        "chars": .int(text.count),
    ]
}

/// Handles `doc.ocr` (M14) — on-device OCR of an image / scanned document via
/// Vision's `VNRecognizeTextRequest`. Free, offline, no external service. Also
/// serves as the perception fallback for image-only screen content.
///
/// Params:
///   - `path` (string, required): image file path (`~` expanded).
///   - `fast` (bool, optional): use the fast recognition level (default false).
///
/// Returns: `text` (newline-joined lines), `line_count`.
func handleOcr(_ params: [String: JSONValue]) throws -> [String: JSONValue] {
    guard case .string(let rawPath)? = params["path"] else {
        throw HelperError(code: "MISSING_PARAM", message: "doc.ocr requires 'path'")
    }
    let path = (rawPath as NSString).expandingTildeInPath
    guard FileManager.default.fileExists(atPath: path) else {
        throw HelperError(code: "DOC_NOT_FOUND", message: "no file at \(path)")
    }
    guard let image = NSImage(contentsOfFile: path),
        let cgImage = image.cgImage(forProposedRect: nil, context: nil, hints: nil)
    else {
        throw HelperError(code: "DOC_READ_FAILED", message: "could not load image at \(path)")
    }

    var fast = false
    if case .bool(let f)? = params["fast"] { fast = f }

    let request = VNRecognizeTextRequest()
    request.recognitionLevel = fast ? .fast : .accurate
    request.usesLanguageCorrection = true

    let handler = VNImageRequestHandler(cgImage: cgImage, options: [:])
    do {
        try handler.perform([request])
    } catch {
        throw HelperError(
            code: "OCR_FAILED",
            message: "Vision text recognition failed: \(error.localizedDescription)",
            retryable: true
        )
    }

    let lines: [String] =
        request.results?.compactMap { $0.topCandidates(1).first?.string } ?? []

    return [
        "text": .string(lines.joined(separator: "\n")),
        "line_count": .int(lines.count),
    ]
}
