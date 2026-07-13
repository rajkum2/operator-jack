# Operator CLI Spec Freeze v0.14

Status: Frozen for implementation
Version: `0.15.0`
Date: `2026-07-09`
Predecessor: Spec Freeze v0.13

Additive delta to v0.13. Covers **M14 — the native macOS integration lane**:
structured access to first-party apps (AppleScript/JXA/Shortcuts) where one
call returns data that would take fifty fragile AX clicks, plus document text
extraction (PDF, OCR).

---

## 1) Script lane (system lane, High risk)

- **`script.applescript {script}`** / **`script.jxa {script}`** — run a script
  through `osascript` (`-l AppleScript` / `-l JavaScript`), reusing the
  `sys.exec` timeout + output-capture path. Returns `{stdout, stderr,
  exit_code, language}`.
- **`shortcuts.run {name, input_path?}`** — run a macOS Shortcut by name via the
  `shortcuts` CLI.

All three are **High risk** and **exec-grade**: they map to the `exec` grant
capability (the script body / `shortcuts run <name>` is the match target, so a
grant like `exec: ["osascript*"]` scopes them), and the always-escalate
classifier **scans script bodies** — `do shell script`, `administrator
privileges`, `sudo`, `rm -rf`, and shelling out escalate to a human, exactly
like dangerous `sys.exec` commands.

---

## 2) Document lane (helper / ui lane, Low risk)

Both run in the Swift helper (macOS-native frameworks) and are read-only:

- **`doc.pdf_text {path, max_pages?}`** — extract text via **PDFKit**; returns
  `{text, page_count, chars}`.
- **`doc.ocr {path, fast?}`** — on-device OCR via **Vision**
  (`VNRecognizeTextRequest`, `.accurate` by default) → `{text, line_count}`.
  Free, offline, no external service; doubles as the perception fallback for
  scanned/image-only content.

New Swift handlers `handlePdfText` / `handleOcr` (`Methods/Document.swift`),
registered as `doc.pdf_text` / `doc.ocr` (pass-through method names).

---

## 3) TCC / permissions in `doctor`

`operator-jack doctor` gains:
- a **Full Disk Access** probe (attempts to list `~/Library/Mail`; reports
  granted / not-granted — needed for `script.*` against Mail/Messages data), and
- an **Automation (AppleEvents)** note explaining that the first `script.*` to a
  given app triggers a one-time macOS consent dialog (per-target-app TCC).

---

## 4) Recipe skills

`skills/calendar-add-event.yaml` — a recipe skill that creates a Calendar event
via `script.applescript` (params: `summary`, `calendar`, `duration_minutes`),
demonstrating the "one script call vs. a click chain" pattern. It expands and
validates under `check-examples.sh`.

---

## 5) i18n guidance

Selector guidance (docs/prompts): **prefer `identifier`-based selectors** —
name-based selectors break on non-English macOS. (The `KeyPress.swift` keycode
table's US-layout assumptions are noted for a future audit.)

---

## 6) Acceptance

- Verified live: `script.applescript "return 2 + 3"` → `5`; `script.jxa "6*7"` →
  `42`; `script.applescript` reading `current date` → the real year;
  `doc.pdf_text` extracted the full text of a real (cupsfilter-generated) PDF;
  `doc.ocr` OCR'd a rendered PNG → `"INVOICE 42"` via Vision.
- Unit test: dangerous scripts (`do shell script`, `administrator privileges`)
  escalate; a benign script does not; scripts map to the `exec` grant capability.
- `skills/calendar-add-event.yaml` validates; 164 Rust tests, 10 Swift tests.
- Full gate green: fmt, clippy `-D warnings`, 164 Rust tests, 10 Swift tests,
  check-examples all valid.

Known-partial (documented): the "save all PDF attachments from unread invoices"
end-to-end acceptance recipe and OCR-to-agent data-entry flow are enabled by
these primitives but were not assembled/exercised as a single scripted run here
(they need Mail data + Automation + Full Disk Access consent on the host). The
Mail/Finder recipe skills beyond Calendar are deferred. `shortcuts.run` is
wired but not exercised (no user Shortcuts on this host). The i18n keycode-table
audit is documented, not yet applied.
