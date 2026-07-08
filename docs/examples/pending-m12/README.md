# Pending examples (not yet runnable)

These example plans use step types that are **not yet wired into the plan
execution engine**, so they intentionally fail `plan validate` today and are
excluded from `make check-examples`.

- `browser-navigate.json` — uses `browser.*` steps. The browser lane exists as a
  CDP crate + `operator-jack browser` CLI subcommands, but plan-lane integration
  (validation + engine dispatch) lands in **M12** (see `MASTER_PLAN.md`). Once
  M12 ships, this file moves back up into `docs/examples/`.
