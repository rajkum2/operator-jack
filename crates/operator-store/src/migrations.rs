use rusqlite_migration::{Migrations, M};

/// Returns all schema migrations for the operator store database.
pub fn all_migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(
            r#"
CREATE TABLE IF NOT EXISTS plans (
    id TEXT PRIMARY KEY,
    schema_version INTEGER NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    plan_json TEXT NOT NULL,
    parent_plan_id TEXT,
    mode TEXT,
    allow_apps_json TEXT,
    allow_domains_json TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS runs (
    id TEXT PRIMARY KEY,
    plan_id TEXT NOT NULL,
    status TEXT NOT NULL,
    mode TEXT NOT NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    error_json TEXT,
    FOREIGN KEY (plan_id) REFERENCES plans(id)
);

CREATE TABLE IF NOT EXISTS step_results (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    step_id TEXT NOT NULL,
    step_index INTEGER NOT NULL,
    status TEXT NOT NULL,
    attempt INTEGER NOT NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    input_json TEXT NOT NULL,
    output_json TEXT,
    error_json TEXT,
    FOREIGN KEY (run_id) REFERENCES runs(id)
);

CREATE TABLE IF NOT EXISTS events (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    step_id TEXT,
    ts TEXT NOT NULL,
    event_type TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    FOREIGN KEY (run_id) REFERENCES runs(id)
);

CREATE INDEX IF NOT EXISTS idx_runs_plan_started ON runs(plan_id, started_at);
CREATE INDEX IF NOT EXISTS idx_step_results_run ON step_results(run_id, step_index, attempt);
CREATE INDEX IF NOT EXISTS idx_events_run_ts ON events(run_id, ts);
"#,
        ),
        // M11 — long-horizon supervision: checkpoint + approval tables.
        M::up(
            r#"
CREATE TABLE IF NOT EXISTS checkpoints (
    run_id TEXT PRIMARY KEY,
    plan_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    cursor_json TEXT NOT NULL,
    variables_json TEXT,
    transcript_json TEXT,
    goal TEXT,
    reason TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (run_id) REFERENCES runs(id)
);

CREATE TABLE IF NOT EXISTS approvals (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    summary TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    decided_at TEXT,
    FOREIGN KEY (run_id) REFERENCES runs(id)
);

CREATE INDEX IF NOT EXISTS idx_approvals_status ON approvals(status, created_at);
CREATE INDEX IF NOT EXISTS idx_approvals_run ON approvals(run_id, created_at);
"#,
        ),
    ])
}
