use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use operator_core::event::Event;
use operator_core::redaction::redact_value;
use operator_core::types::{Mode, Plan, Run, RunStatus, StepResult, StepStatus};

use crate::error::StoreError;
use crate::migrations::all_migrations;

// ---------------------------------------------------------------------------
// PlanSummary
// ---------------------------------------------------------------------------

/// A lightweight summary of a stored plan, returned by `list_plans`.
#[derive(Debug, Clone, Serialize)]
pub struct PlanSummary {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub step_count: usize,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Serializes a serde-serializable enum value to a plain string suitable for
/// SQLite TEXT columns. For enums with `rename_all = "snake_case"` or
/// `rename_all = "lowercase"`, `serde_json::to_string` produces a
/// JSON-quoted string like `"queued"`. This helper strips the surrounding
/// quotes so we store just `queued`.
fn to_db_string<T: Serialize>(val: &T) -> Result<String, StoreError> {
    let json = serde_json::to_string(val)?;
    // Strip surrounding quotes from JSON string values
    Ok(json.trim_matches('"').to_string())
}

/// Deserializes a plain string from a SQLite TEXT column back into a typed
/// enum. Re-wraps the value in quotes so `serde_json::from_str` can parse it.
fn from_db_string<T: serde::de::DeserializeOwned>(s: &str) -> Result<T, StoreError> {
    let quoted = format!("\"{}\"", s);
    Ok(serde_json::from_str(&quoted)?)
}

/// Returns true if the given `RunStatus` is a terminal state (the run has
/// finished).
fn is_terminal_run_status(status: &RunStatus) -> bool {
    matches!(
        status,
        RunStatus::Succeeded
            | RunStatus::CompletedWithErrors
            | RunStatus::Failed
            | RunStatus::Cancelled
    )
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

pub struct Store {
    conn: Connection,
}

impl Store {
    // -- constructors -------------------------------------------------------

    /// Opens (or creates) a SQLite database at `db_path`, enables WAL mode,
    /// and runs all pending migrations.
    pub fn open(db_path: &Path) -> Result<Self, StoreError> {
        // Ensure parent directory exists.
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut conn = Connection::open(db_path)?;
        Self::init(&mut conn)?;
        Ok(Self { conn })
    }

    /// Creates an in-memory SQLite database, useful for tests.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let mut conn = Connection::open_in_memory()?;
        Self::init(&mut conn)?;
        Ok(Self { conn })
    }

    /// Shared initialisation: WAL mode, foreign keys, migrations.
    fn init(conn: &mut Connection) -> Result<(), StoreError> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;

        let migrations = all_migrations();
        migrations
            .to_latest(conn)
            .map_err(|e| StoreError::Migration(e.to_string()))?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Plans CRUD
    // -----------------------------------------------------------------------

    /// Persists a `Plan`, generating a new ULID as the primary key.
    /// Returns the generated plan id.
    pub fn save_plan(&self, plan: &Plan) -> Result<String, StoreError> {
        let plan_id = ulid::Ulid::new().to_string();
        let plan_json = serde_json::to_string(plan)?;
        let now = Utc::now().to_rfc3339();

        let mode_str: Option<String> = plan.mode.as_ref().map(to_db_string).transpose()?;
        let allow_apps_json: Option<String> = plan
            .allow_apps
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let allow_domains_json: Option<String> = plan
            .allow_domains
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;

        self.conn.execute(
            "INSERT INTO plans (id, schema_version, name, description, plan_json, parent_plan_id, mode, allow_apps_json, allow_domains_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                plan_id,
                plan.schema_version,
                plan.name,
                plan.description,
                plan_json,
                Option::<String>::None, // parent_plan_id
                mode_str,
                allow_apps_json,
                allow_domains_json,
                now,
            ],
        )?;

        tracing::debug!(plan_id = %plan_id, "saved plan");
        Ok(plan_id)
    }

    /// Retrieves a plan by its id. Returns `(plan_id, Plan)`.
    /// Errors with `StoreError::NotFound` if the row does not exist.
    pub fn get_plan(&self, plan_id: &str) -> Result<(String, Plan), StoreError> {
        let row = self
            .conn
            .query_row(
                "SELECT id, plan_json FROM plans WHERE id = ?1",
                params![plan_id],
                |row| {
                    let id: String = row.get(0)?;
                    let json: String = row.get(1)?;
                    Ok((id, json))
                },
            )
            .optional()?;

        match row {
            Some((id, json)) => {
                let plan: Plan = serde_json::from_str(&json)?;
                Ok((id, plan))
            }
            None => Err(StoreError::NotFound {
                entity: "Plan".to_string(),
                id: plan_id.to_string(),
            }),
        }
    }

    /// Returns the most recent plans (up to `limit`).
    pub fn list_plans(&self, limit: u32) -> Result<Vec<PlanSummary>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, created_at, plan_json FROM plans ORDER BY created_at DESC LIMIT ?1",
        )?;

        let rows = stmt.query_map(params![limit], |row| {
            let id: String = row.get(0)?;
            let name: String = row.get(1)?;
            let created_at: String = row.get(2)?;
            let plan_json: String = row.get(3)?;
            Ok((id, name, created_at, plan_json))
        })?;

        let mut plans = Vec::new();
        for row in rows {
            let (id, name, created_at, plan_json) = row?;
            let step_count = match serde_json::from_str::<Plan>(&plan_json) {
                Ok(p) => p.steps.len(),
                Err(_) => 0,
            };
            plans.push(PlanSummary {
                id,
                name,
                created_at,
                step_count,
            });
        }
        Ok(plans)
    }

    // -----------------------------------------------------------------------
    // Runs CRUD
    // -----------------------------------------------------------------------

    /// Creates a new run for the given plan, with status `Queued`.
    /// Returns the generated run id.
    pub fn create_run(&self, plan_id: &str, mode: &Mode) -> Result<String, StoreError> {
        let run_id = ulid::Ulid::new().to_string();
        let status_str = to_db_string(&RunStatus::Queued)?;
        let mode_str = to_db_string(mode)?;
        let now = Utc::now().to_rfc3339();

        self.conn.execute(
            "INSERT INTO runs (id, plan_id, status, mode, started_at, ended_at, error_json)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL)",
            params![run_id, plan_id, status_str, mode_str, now],
        )?;

        tracing::debug!(run_id = %run_id, plan_id = %plan_id, "created run");
        Ok(run_id)
    }

    /// Updates the status of an existing run. If the new status is terminal,
    /// `ended_at` is set to the current time. An optional error payload may
    /// be stored alongside.
    pub fn update_run_status(
        &self,
        run_id: &str,
        status: &RunStatus,
        error: Option<&serde_json::Value>,
    ) -> Result<(), StoreError> {
        let status_str = to_db_string(status)?;
        let ended_at: Option<String> = if is_terminal_run_status(status) {
            Some(Utc::now().to_rfc3339())
        } else {
            None
        };
        // P1 fix: Apply redaction to error payload before SQLite writes.
        let error_json: Option<String> = error.map(|e| redact_value(e).to_string());

        let affected = self.conn.execute(
            "UPDATE runs SET status = ?1, ended_at = COALESCE(?2, ended_at), error_json = ?3 WHERE id = ?4",
            params![status_str, ended_at, error_json, run_id],
        )?;

        if affected == 0 {
            return Err(StoreError::NotFound {
                entity: "Run".to_string(),
                id: run_id.to_string(),
            });
        }

        tracing::debug!(run_id = %run_id, status = %status_str, "updated run status");
        Ok(())
    }

    /// Retrieves a run by its id.
    pub fn get_run(&self, run_id: &str) -> Result<Run, StoreError> {
        let row = self
            .conn
            .query_row(
                "SELECT id, plan_id, status, mode, started_at, ended_at, error_json FROM runs WHERE id = ?1",
                params![run_id],
                |row| {
                    Ok(RunRow {
                        id: row.get(0)?,
                        plan_id: row.get(1)?,
                        status: row.get(2)?,
                        mode: row.get(3)?,
                        started_at: row.get(4)?,
                        ended_at: row.get(5)?,
                        error_json: row.get(6)?,
                    })
                },
            )
            .optional()?;

        match row {
            Some(r) => run_from_row(r),
            None => Err(StoreError::NotFound {
                entity: "Run".to_string(),
                id: run_id.to_string(),
            }),
        }
    }

    /// Lists the most recent runs (up to `limit`), ordered by `started_at`
    /// descending.
    pub fn list_runs(&self, limit: u32) -> Result<Vec<Run>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, plan_id, status, mode, started_at, ended_at, error_json
             FROM runs ORDER BY started_at DESC LIMIT ?1",
        )?;

        let rows = stmt.query_map(params![limit], |row| {
            Ok(RunRow {
                id: row.get(0)?,
                plan_id: row.get(1)?,
                status: row.get(2)?,
                mode: row.get(3)?,
                started_at: row.get(4)?,
                ended_at: row.get(5)?,
                error_json: row.get(6)?,
            })
        })?;

        let mut runs = Vec::new();
        for row in rows {
            runs.push(run_from_row(row?)?);
        }
        Ok(runs)
    }

    // -----------------------------------------------------------------------
    // Step Results CRUD
    // -----------------------------------------------------------------------

    /// Inserts a new step result row. JSON fields are redacted before storage.
    pub fn insert_step_result(&self, result: &StepResult) -> Result<(), StoreError> {
        let status_str = to_db_string(&result.status)?;
        let started_at = result.started_at.to_rfc3339();
        let ended_at: Option<String> = result.ended_at.map(|dt| dt.to_rfc3339());
        // P1 fix: Apply redaction to JSON fields before SQLite writes.
        let input_json = redact_value(&result.input_json).to_string();
        let output_json: Option<String> = result
            .output_json
            .as_ref()
            .map(|v| redact_value(v).to_string());
        let error_json: Option<String> = result
            .error_json
            .as_ref()
            .map(|v| redact_value(v).to_string());

        self.conn.execute(
            "INSERT INTO step_results (id, run_id, step_id, step_index, status, attempt, started_at, ended_at, input_json, output_json, error_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                result.id,
                result.run_id,
                result.step_id,
                result.step_index,
                status_str,
                result.attempt,
                started_at,
                ended_at,
                input_json,
                output_json,
                error_json,
            ],
        )?;

        tracing::debug!(step_result_id = %result.id, run_id = %result.run_id, "inserted step result");
        Ok(())
    }

    /// Updates an existing step result (status, ended_at, output_json,
    /// error_json). JSON fields are redacted before storage.
    pub fn update_step_result(&self, result: &StepResult) -> Result<(), StoreError> {
        let status_str = to_db_string(&result.status)?;
        let ended_at: Option<String> = result.ended_at.map(|dt| dt.to_rfc3339());
        // P1 fix: Apply redaction to JSON fields before SQLite writes.
        let output_json: Option<String> = result
            .output_json
            .as_ref()
            .map(|v| redact_value(v).to_string());
        let error_json: Option<String> = result
            .error_json
            .as_ref()
            .map(|v| redact_value(v).to_string());

        let affected = self.conn.execute(
            "UPDATE step_results SET status = ?1, ended_at = ?2, output_json = ?3, error_json = ?4 WHERE id = ?5",
            params![status_str, ended_at, output_json, error_json, result.id],
        )?;

        if affected == 0 {
            return Err(StoreError::NotFound {
                entity: "StepResult".to_string(),
                id: result.id.clone(),
            });
        }

        tracing::debug!(step_result_id = %result.id, "updated step result");
        Ok(())
    }

    /// Returns all step results for a run, ordered by step_index then attempt.
    pub fn get_step_results(&self, run_id: &str) -> Result<Vec<StepResult>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, run_id, step_id, step_index, status, attempt, started_at, ended_at, input_json, output_json, error_json
             FROM step_results WHERE run_id = ?1 ORDER BY step_index, attempt",
        )?;

        let rows = stmt.query_map(params![run_id], |row| {
            Ok(StepResultRow {
                id: row.get(0)?,
                run_id: row.get(1)?,
                step_id: row.get(2)?,
                step_index: row.get(3)?,
                status: row.get(4)?,
                attempt: row.get(5)?,
                started_at: row.get(6)?,
                ended_at: row.get(7)?,
                input_json: row.get(8)?,
                output_json: row.get(9)?,
                error_json: row.get(10)?,
            })
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(step_result_from_row(row?)?);
        }
        Ok(results)
    }

    // -----------------------------------------------------------------------
    // Events
    // -----------------------------------------------------------------------

    /// Inserts an event row. Payload is redacted before storage.
    pub fn insert_event(&self, event: &Event) -> Result<(), StoreError> {
        let ts = event.ts.to_rfc3339();
        let event_type_str = to_db_string(&event.event_type)?;
        // P1 fix: Apply redaction to event payload before SQLite writes.
        let payload_json = redact_value(&event.payload).to_string();

        self.conn.execute(
            "INSERT INTO events (id, run_id, step_id, ts, event_type, payload_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                event.id,
                event.run_id,
                event.step_id,
                ts,
                event_type_str,
                payload_json,
            ],
        )?;

        tracing::debug!(event_id = %event.id, run_id = %event.run_id, "inserted event");
        Ok(())
    }

    /// Returns all events for a run, ordered by timestamp.
    pub fn get_events(&self, run_id: &str) -> Result<Vec<Event>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, run_id, step_id, ts, event_type, payload_json
             FROM events WHERE run_id = ?1 ORDER BY ts",
        )?;

        let rows = stmt.query_map(params![run_id], |row| {
            Ok(EventRow {
                id: row.get(0)?,
                run_id: row.get(1)?,
                step_id: row.get(2)?,
                ts: row.get(3)?,
                event_type: row.get(4)?,
                payload_json: row.get(5)?,
            })
        })?;

        let mut events = Vec::new();
        for row in rows {
            events.push(event_from_row(row?)?);
        }
        Ok(events)
    }
}

// ---------------------------------------------------------------------------
// Internal row types + converters
// ---------------------------------------------------------------------------

/// Intermediate struct for reading a `runs` row from SQLite before
/// deserialising the typed fields.
struct RunRow {
    id: String,
    plan_id: String,
    status: String,
    mode: String,
    started_at: String,
    ended_at: Option<String>,
    error_json: Option<String>,
}

fn run_from_row(r: RunRow) -> Result<Run, StoreError> {
    let status: RunStatus = from_db_string(&r.status)?;
    let mode: Mode = from_db_string(&r.mode)?;
    let started_at = parse_datetime(&r.started_at)?;
    let ended_at = r.ended_at.as_deref().map(parse_datetime).transpose()?;
    let error: Option<serde_json::Value> = r
        .error_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?;

    Ok(Run {
        id: r.id,
        plan_id: r.plan_id,
        status,
        mode,
        started_at,
        ended_at,
        error,
    })
}

struct StepResultRow {
    id: String,
    run_id: String,
    step_id: String,
    step_index: u32,
    status: String,
    attempt: u32,
    started_at: String,
    ended_at: Option<String>,
    input_json: String,
    output_json: Option<String>,
    error_json: Option<String>,
}

fn step_result_from_row(r: StepResultRow) -> Result<StepResult, StoreError> {
    let status: StepStatus = from_db_string(&r.status)?;
    let started_at = parse_datetime(&r.started_at)?;
    let ended_at = r.ended_at.as_deref().map(parse_datetime).transpose()?;
    let input_json: serde_json::Value = serde_json::from_str(&r.input_json)?;
    let output_json: Option<serde_json::Value> = r
        .output_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?;
    let error_json: Option<serde_json::Value> = r
        .error_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?;

    Ok(StepResult {
        id: r.id,
        run_id: r.run_id,
        step_id: r.step_id,
        step_index: r.step_index,
        status,
        attempt: r.attempt,
        started_at,
        ended_at,
        input_json,
        output_json,
        error_json,
    })
}

struct EventRow {
    id: String,
    run_id: String,
    step_id: Option<String>,
    ts: String,
    event_type: String,
    payload_json: String,
}

fn event_from_row(r: EventRow) -> Result<Event, StoreError> {
    let ts = parse_datetime(&r.ts)?;
    let event_type = from_db_string(&r.event_type)?;
    let payload: serde_json::Value = serde_json::from_str(&r.payload_json)?;

    Ok(Event {
        id: r.id,
        run_id: r.run_id,
        step_id: r.step_id,
        ts,
        event_type,
        payload,
    })
}

/// Parses an RFC 3339 timestamp string into `DateTime<Utc>`.
fn parse_datetime(s: &str) -> Result<DateTime<Utc>, StoreError> {
    let dt = DateTime::parse_from_rfc3339(s)
        .map_err(|e| StoreError::Migration(format!("invalid datetime '{}': {}", s, e)))?;
    Ok(dt.with_timezone(&Utc))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use operator_core::event::{Event, EventType};
    use operator_core::types::{Mode, Plan, RunStatus, Step, StepResult, StepStatus, StepType};
    use serde_json::json;

    fn sample_plan() -> Plan {
        Plan {
            schema_version: 1,
            name: "Test plan".to_string(),
            description: Some("A plan for testing".to_string()),
            mode: Some(Mode::Safe),
            allow_apps: Some(vec!["Safari".to_string()]),
            allow_domains: Some(vec!["example.com".to_string()]),
            variables: None,
            steps: vec![Step {
                id: "step-1".to_string(),
                step_type: StepType::SysOpenUrl,
                params: json!({"url": "https://example.com"}),
                timeout_ms: None,
                retries: None,
                retry_backoff_ms: None,
                on_fail: None,
            }],
        }
    }

    #[test]
    fn test_save_and_get_plan() {
        let store = Store::open_in_memory().expect("open in-memory store");
        let plan = sample_plan();

        let plan_id = store.save_plan(&plan).expect("save plan");
        assert!(!plan_id.is_empty());

        let (retrieved_id, retrieved_plan) = store.get_plan(&plan_id).expect("get plan");
        assert_eq!(retrieved_id, plan_id);
        assert_eq!(retrieved_plan.name, "Test plan");
        assert_eq!(retrieved_plan.steps.len(), 1);
    }

    #[test]
    fn test_get_plan_not_found() {
        let store = Store::open_in_memory().expect("open in-memory store");
        let result = store.get_plan("nonexistent");
        assert!(result.is_err());
        match result.unwrap_err() {
            StoreError::NotFound { entity, id } => {
                assert_eq!(entity, "Plan");
                assert_eq!(id, "nonexistent");
            }
            other => panic!("expected NotFound, got: {:?}", other),
        }
    }

    #[test]
    fn test_list_plans() {
        let store = Store::open_in_memory().expect("open in-memory store");
        let plan = sample_plan();

        store.save_plan(&plan).expect("save plan 1");
        store.save_plan(&plan).expect("save plan 2");

        let summaries = store.list_plans(10).expect("list plans");
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].name, "Test plan");
        assert_eq!(summaries[0].step_count, 1);
    }

    #[test]
    fn test_create_and_get_run() {
        let store = Store::open_in_memory().expect("open in-memory store");
        let plan = sample_plan();
        let plan_id = store.save_plan(&plan).expect("save plan");

        let run_id = store.create_run(&plan_id, &Mode::Safe).expect("create run");
        assert!(!run_id.is_empty());

        let run = store.get_run(&run_id).expect("get run");
        assert_eq!(run.id, run_id);
        assert_eq!(run.plan_id, plan_id);
        assert_eq!(run.status, RunStatus::Queued);
        assert_eq!(run.mode, Mode::Safe);
        assert!(run.ended_at.is_none());
    }

    #[test]
    fn test_update_run_status() {
        let store = Store::open_in_memory().expect("open in-memory store");
        let plan = sample_plan();
        let plan_id = store.save_plan(&plan).expect("save plan");
        let run_id = store.create_run(&plan_id, &Mode::Safe).expect("create run");

        // Transition to Running (non-terminal)
        store
            .update_run_status(&run_id, &RunStatus::Running, None)
            .expect("update to running");
        let run = store.get_run(&run_id).expect("get run after running");
        assert_eq!(run.status, RunStatus::Running);
        assert!(run.ended_at.is_none());

        // Transition to Succeeded (terminal)
        store
            .update_run_status(&run_id, &RunStatus::Succeeded, None)
            .expect("update to succeeded");
        let run = store.get_run(&run_id).expect("get run after succeeded");
        assert_eq!(run.status, RunStatus::Succeeded);
        assert!(run.ended_at.is_some());
    }

    #[test]
    fn test_update_run_status_with_error() {
        let store = Store::open_in_memory().expect("open in-memory store");
        let plan = sample_plan();
        let plan_id = store.save_plan(&plan).expect("save plan");
        let run_id = store
            .create_run(&plan_id, &Mode::Unsafe)
            .expect("create run");

        let err = json!({"code": "EXEC_FAILED", "message": "oops"});
        store
            .update_run_status(&run_id, &RunStatus::Failed, Some(&err))
            .expect("update to failed");

        let run = store.get_run(&run_id).expect("get run");
        assert_eq!(run.status, RunStatus::Failed);
        assert!(run.ended_at.is_some());
        assert_eq!(run.error.unwrap()["code"], "EXEC_FAILED");
    }

    #[test]
    fn test_list_runs() {
        let store = Store::open_in_memory().expect("open in-memory store");
        let plan = sample_plan();
        let plan_id = store.save_plan(&plan).expect("save plan");

        store
            .create_run(&plan_id, &Mode::Safe)
            .expect("create run 1");
        store
            .create_run(&plan_id, &Mode::Unsafe)
            .expect("create run 2");

        let runs = store.list_runs(10).expect("list runs");
        assert_eq!(runs.len(), 2);
    }

    #[test]
    fn test_insert_and_get_step_results() {
        let store = Store::open_in_memory().expect("open in-memory store");
        let plan = sample_plan();
        let plan_id = store.save_plan(&plan).expect("save plan");
        let run_id = store.create_run(&plan_id, &Mode::Safe).expect("create run");

        let sr = StepResult {
            id: ulid::Ulid::new().to_string(),
            run_id: run_id.clone(),
            step_id: "step-1".to_string(),
            step_index: 0,
            status: StepStatus::Running,
            attempt: 1,
            started_at: Utc::now(),
            ended_at: None,
            input_json: json!({"url": "https://example.com"}),
            output_json: None,
            error_json: None,
        };
        store.insert_step_result(&sr).expect("insert step result");

        let results = store.get_step_results(&run_id).expect("get step results");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].step_id, "step-1");
        assert_eq!(results[0].status, StepStatus::Running);
    }

    #[test]
    fn test_update_step_result() {
        let store = Store::open_in_memory().expect("open in-memory store");
        let plan = sample_plan();
        let plan_id = store.save_plan(&plan).expect("save plan");
        let run_id = store.create_run(&plan_id, &Mode::Safe).expect("create run");

        let sr_id = ulid::Ulid::new().to_string();
        let sr = StepResult {
            id: sr_id.clone(),
            run_id: run_id.clone(),
            step_id: "step-1".to_string(),
            step_index: 0,
            status: StepStatus::Running,
            attempt: 1,
            started_at: Utc::now(),
            ended_at: None,
            input_json: json!({"url": "https://example.com"}),
            output_json: None,
            error_json: None,
        };
        store.insert_step_result(&sr).expect("insert step result");

        // Now update it
        let updated = StepResult {
            status: StepStatus::Succeeded,
            ended_at: Some(Utc::now()),
            output_json: Some(json!({"ok": true})),
            ..sr
        };
        store
            .update_step_result(&updated)
            .expect("update step result");

        let results = store.get_step_results(&run_id).expect("get step results");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, StepStatus::Succeeded);
        assert!(results[0].ended_at.is_some());
        assert_eq!(results[0].output_json.as_ref().unwrap()["ok"], true);
    }

    #[test]
    fn test_insert_and_get_events() {
        let store = Store::open_in_memory().expect("open in-memory store");
        let plan = sample_plan();
        let plan_id = store.save_plan(&plan).expect("save plan");
        let run_id = store.create_run(&plan_id, &Mode::Safe).expect("create run");

        let event = Event::new(
            run_id.clone(),
            None,
            EventType::RunStarted,
            json!({"plan_id": plan_id}),
        );
        store.insert_event(&event).expect("insert event");

        let event2 = Event::new(
            run_id.clone(),
            Some("step-1".to_string()),
            EventType::StepStarted,
            json!({"step_index": 0}),
        );
        store.insert_event(&event2).expect("insert event 2");

        let events = store.get_events(&run_id).expect("get events");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, EventType::RunStarted);
        assert_eq!(events[1].event_type, EventType::StepStarted);
        assert_eq!(events[1].step_id.as_deref(), Some("step-1"));
    }

    #[test]
    fn test_update_run_not_found() {
        let store = Store::open_in_memory().expect("open in-memory store");
        let result = store.update_run_status("nonexistent", &RunStatus::Running, None);
        assert!(result.is_err());
        match result.unwrap_err() {
            StoreError::NotFound { entity, id } => {
                assert_eq!(entity, "Run");
                assert_eq!(id, "nonexistent");
            }
            other => panic!("expected NotFound, got: {:?}", other),
        }
    }

    #[test]
    fn test_update_step_result_not_found() {
        let store = Store::open_in_memory().expect("open in-memory store");
        let sr = StepResult {
            id: "nonexistent".to_string(),
            run_id: "r".to_string(),
            step_id: "s".to_string(),
            step_index: 0,
            status: StepStatus::Succeeded,
            attempt: 1,
            started_at: Utc::now(),
            ended_at: Some(Utc::now()),
            input_json: json!({}),
            output_json: None,
            error_json: None,
        };
        let result = store.update_step_result(&sr);
        assert!(result.is_err());
        match result.unwrap_err() {
            StoreError::NotFound { entity, id } => {
                assert_eq!(entity, "StepResult");
                assert_eq!(id, "nonexistent");
            }
            other => panic!("expected NotFound, got: {:?}", other),
        }
    }
}
