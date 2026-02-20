use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// EventType
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    RunStarted,
    StepStarted,
    StepRetryScheduled,
    StepFinished,
    StepFailed,
    RunFinished,
    RunCancelled,
}

// ---------------------------------------------------------------------------
// Event
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub run_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
    pub ts: DateTime<Utc>,
    pub event_type: EventType,
    pub payload: serde_json::Value,
}

impl Event {
    /// Creates a new event with an auto-generated ULID and the current UTC
    /// timestamp.
    pub fn new(
        run_id: impl Into<String>,
        step_id: Option<String>,
        event_type: EventType,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            id: ulid::Ulid::new().to_string(),
            run_id: run_id.into(),
            step_id,
            ts: Utc::now(),
            event_type,
            payload,
        }
    }
}
