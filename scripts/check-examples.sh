#!/usr/bin/env bash
#
# Regression guard for shipped examples and skills.
#
# Validates every top-level example plan and dry-run-expands every skill (which
# also validates the expanded plan). Dry-run does not spawn the helper or touch
# the GUI, so this is CI-safe. Examples under docs/examples/pending-*/ are
# intentionally excluded — they use step types not yet wired into the engine.
#
# Rule: no example or skill ships without passing this.
set -euo pipefail

BIN="${OPERATOR_BIN:-./target/debug/operator-jack}"
HELPER_STUB="/nonexistent/helper" # dry-run never spawns the helper
fail=0

if [[ ! -x "$BIN" ]]; then
  echo "error: binary not found at $BIN (run 'cargo build' or set OPERATOR_BIN)" >&2
  exit 1
fi

echo "==> Validating example plans"
for f in docs/examples/*.json; do
  [[ -e "$f" ]] || continue
  printf "    %-48s " "$(basename "$f")"
  if err=$("$BIN" plan validate --plan-file "$f" 2>&1); then
    echo "OK"
  else
    echo "FAIL"
    echo "$err" | sed 's/^/        /'
    fail=1
  fi
done

echo "==> Dry-running skills (expand + validate expanded plan)"
# skill-name followed by its representative --param args (required params only).
dry_run_skill() {
  local name="$1"
  shift
  printf "    %-48s " "$name"
  if err=$("$BIN" skill run "$name" --dry-run --helper-path "$HELPER_STUB" "$@" 2>&1); then
    echo "OK"
  else
    echo "FAIL"
    echo "$err" | sed 's/^/        /'
    fail=1
  fi
}

dry_run_skill open-and-type --param app=TextEdit --param text=hello
dry_run_skill create-note --param title=Test --param body=Body
dry_run_skill take-screenshot
dry_run_skill calendar-add-event --param summary=Standup

if [[ "$fail" -ne 0 ]]; then
  echo "==> FAILED: one or more examples/skills did not validate" >&2
  exit 1
fi
echo "==> All examples and skills valid."
