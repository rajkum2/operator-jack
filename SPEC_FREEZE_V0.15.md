# Operator CLI Spec Freeze v0.15

Status: Frozen for implementation
Version: `0.16.0`
Date: `2026-07-09`
Predecessor: Spec Freeze v0.14

Additive delta to v0.14. Covers **M15 — trust, residency & measurement**: what
makes a user run this unattended on their real machine — reversibility (undo),
unattended triggers (watch), a measured success rate (eval), and a signed
install.

---

## 1) Undo journal (reversibility)

Copy-before-mutate for filesystem `sys.*` steps. When `EngineConfig.undo_dir` is
set (the CLI sets it to `<data_dir>/undo` for every real — non-`--dry-run` —
run), the engine snapshots prior state **before** each mutating step:

- `sys.write_file` / `sys.append_file` / `sys.copy_path`: back up the target
  file if it exists, else record that it was newly created.
- `sys.delete_path`: copy the target aside before it is deleted.
- `sys.move_path`: record `from`→`to`.
- `sys.mkdir`: record the created directory (only if it didn't exist).

Each mutation appends a line to `<undo_dir>/<run_id>/manifest.jsonl`; backups
live under `<undo_dir>/<run_id>/backups/`. Applies to both plan runs and
`RunSession` (agent/MCP) runs.

**`operator-jack undo <run-id>`** replays the manifest in reverse: restores
backups, removes files the run created, moves moves back, restores deleted
files from their copy. Each entry reports its own outcome; one failure doesn't
abort the rest.

---

## 2) Watch / daemon mode (unattended triggers)

**`operator-jack watch --plan-file <p> [--interval N] [--path <watch>]
[--install-launchd]`**:

- Interval-only (`--interval`, default 60s): fires the plan every tick.
- File trigger (`--path`): fires when the path's mtime changes (for a directory,
  the latest mtime among its entries), baselined at startup so it doesn't fire
  immediately.
- **Concurrency model:** a single-instance run mutex (the PID file + a live-PID
  check) serializes runs; a trigger that fires while a run holds the mutex is
  **queued** (a `pending` flag), not dropped and not run concurrently. True
  concurrent runs are out of scope — one agent, one desktop.
- **`--install-launchd`** writes a `~/Library/LaunchAgents/com.operator-jack.watch.plist`
  (RunAtLoad + KeepAlive) that runs the watch at login, and prints the
  `launchctl load/unload` commands.

---

## 3) Eval harness (measurement)

**`operator-jack eval <suite-dir>`** runs every `*.json` plan in the directory
and scores each **pass** (`Succeeded`, zero failed steps) / **fail**, printing a
per-task table and an overall pass rate (`--json` for machine-readable output).
Exits non-zero if any task fails (a CI signal). This is the seed of the
"mini-OSWorld-for-macOS" suite; a self-hosted macOS runner (AX needs a real
session) is the CI target.

---

## 4) Exec sandboxing (stretch) — deferred

A seatbelt profile generated from the run's fs grants wrapping `sys.exec` was a
stretch goal. It is **deferred**: the default posture (accept-and-audit + the
M8 grant/escalation gate + the M15 undo journal + run budgets) already bounds
`sys.exec`, and `sandbox-exec` is deprecated. Documented, not implemented.

---

## 5) Signing & notarization

`release.yml` gains a **Sign & notarize** step: it imports a Developer ID cert
from secrets into a temporary keychain, `codesign`s the binaries (hardened
runtime, timestamped), and notarizes each release tarball via `notarytool`. The
step **skips cleanly if the signing secrets are unset**, so releases keep
building (unsigned) until an Apple Developer ID cert is configured. Required
secrets: `MACOS_CERT_P12`, `MACOS_CERT_PASSWORD`, `MACOS_SIGN_IDENTITY`,
`AC_API_KEY_ID`, `AC_API_ISSUER_ID`, `AC_API_KEY_P8`.

**Status:** scaffolding complete; not exercised here (no Apple Developer ID
certificate is available in this environment). A signed, notarized build that
passes Gatekeeper requires that cert.

---

## 6) Acceptance

- Verified live: a run that overwrote an existing file, created a new file, and
  deleted a file was fully reversed by `operator-jack undo <run-id>` (3
  reversed, 0 failed — original restored, created removed, deleted restored).
- Verified live: `operator-jack watch --path <file>` fired the plan when the
  file changed (a new audited run appeared) and stopped cleanly;
  `--install-launchd` wrote a valid plist.
- Verified live: `operator-jack eval <suite>` ran a 3-plan suite → 3/3 (100%),
  exit 0.
- Unit tests: undo restores overwrite + removes newly-created + restores
  deleted. 166 Rust tests, 10 Swift tests.
- Full gate green: fmt, clippy `-D warnings`, 166 Rust tests, 10 Swift tests,
  check-examples all valid.

Known-partial (documented): exec sandboxing deferred (stretch); signing/
notarization is scaffolded but requires an Apple Developer ID cert (unavailable
here), so no Gatekeeper-passing build was produced; watch user-activity-pause
(defer a run while the human is active) is not implemented; the eval CI runner
is not wired (needs a self-hosted macOS session).
