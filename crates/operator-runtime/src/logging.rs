use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use operator_core::event::Event;
use operator_core::redaction::redact_value;

/// Writes events as redacted JSONL (one JSON object per line) to a run-specific
/// log file.
pub struct RunLogger {
    file: File,
    log_path: PathBuf,
}

impl RunLogger {
    /// Creates a new run logger that writes to `<log_dir>/<run_id>.jsonl`.
    ///
    /// The log directory is created (recursively) if it does not exist.
    pub fn new(log_dir: &Path, run_id: &str) -> Result<Self, std::io::Error> {
        std::fs::create_dir_all(log_dir)?;
        let log_path = log_dir.join(format!("{}.jsonl", run_id));
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        Ok(Self { file, log_path })
    }

    /// Returns the path to the JSONL log file.
    pub fn log_path(&self) -> &Path {
        &self.log_path
    }

    /// Serializes `event` to JSON, applies redaction to the payload, and writes
    /// one line to the log file (followed by a newline). The file is flushed
    /// after every event to ensure durability.
    pub fn log_event(&mut self, event: &Event) -> Result<(), std::io::Error> {
        // Serialize the whole event to a Value so we can redact it.
        let raw = serde_json::to_value(event)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let redacted = redact_value(&raw);
        let line = serde_json::to_string(&redacted)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        writeln!(self.file, "{}", line)?;
        self.file.flush()?;
        Ok(())
    }
}
