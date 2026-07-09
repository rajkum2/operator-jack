# Operator CLI Spec Freeze v0.10

Status: Frozen for implementation
Version: `0.11.0`
Date: `2026-07-09`
Predecessor: Spec Freeze v0.9

Additive delta to v0.9. Covers **M10 — the native agent loop**: Operator Jack
drives *itself*, using an LLM as the brain and its own `RunSession` as the
hands, in an observe → propose-one-step → execute → re-observe cycle.

---

## 1) Provider `chat`

`LlmProvider` gains `chat(system, user) -> Result<String>` — a single-turn
completion returning the model's raw text. Implemented for all four providers
(Kimi/OpenAI/Anthropic/Ollama) by sharing the HTTP path with `generate_plan`
(which is now `chat` + plan extraction). The agent loop uses `chat` to request
one action per turn.

---

## 2) `operator-agent` crate & the loop

`operator_agent::run_agent(goal, provider, &mut RunSession, max_iters)` runs:

1. Show the model the goal and the running transcript (including prior
   `snapshot` output, so element refs are in view).
2. The model replies with ONE JSON action:
   `{"thought": "...", "action": "<name>", "args": { ... }}` or
   `{"action": "done", "summary": "..."}`.
3. The action maps to a step type and executes through the `RunSession` — same
   policy gate (grants, escalation), secret resolution, redaction, and audit as
   everything else. Its result (or an `ELEMENT_AMBIGUOUS` candidate list, or a
   policy `DENIED` message) is appended to the transcript for the model to react
   to.
4. Repeat until the model emits `done`, the `max_iters` budget is hit
   (`AgentStatus::MaxIterations`), or the provider fails (`Aborted`).

Actions: `open_app`, `focus_app`, `snapshot`, `click`, `type_text`, `key_press`,
`set_value`, `select_menu`, `read_text`, `wait_for`, `read_file`, `write_file`,
`exec`. `click`/`read_text`/`set_value` use an `element_ref` from the latest
`snapshot`. Malformed replies and unknown actions are fed back as errors so the
model self-corrects; `ELEMENT_AMBIGUOUS` is a one-turn recovery (the model picks
a ref/index).

---

## 3) `operator-jack agent "<goal>"`

```
operator-jack agent "open Calculator and compute 2 + 3" [--max-iters N] [--provider P] [--grants <file>]
```

Like MCP mode, the agent is autonomous and non-interactive: it auto-approves
confirmations and relies on the grant envelope + escalation classifier +
secure-field refusal for safety (the effective envelope is printed first). The
whole run is one session-run in the store; every executed action is audited.

---

## 4) Acceptance

- The loop drives real, gated actions and re-observes between them, with element
  refs surviving across turns via the persistent helper. *(Verified live with a
  local Ollama model: open_app → snapshot → click → snapshot → click against
  Calculator, 6 audited steps under one run.)*
- A denied action is reported to the model (not retried blindly); an ambiguous
  selector is a one-turn recovery.
- Task-completion quality tracks the model; a stronger provider (Claude via API)
  completes goals a small local model may not.
