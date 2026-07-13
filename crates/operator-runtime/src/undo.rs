//! The undo journal (M15): copy-before-mutate for filesystem `sys.*` steps so a
//! whole run can be reversed with `operator-jack undo <run-id>`.
//!
//! Before a mutating step runs, [`record`] snapshots the prior state under
//! `<undo_dir>/<run_id>/` and appends a line to that run's `manifest.jsonl`.
//! [`restore`] replays the manifest in reverse: backups are copied back, files
//! the run created are removed, and moves are moved back. Reversal is
//! best-effort — each entry reports its own outcome, and one failure does not
//! abort the rest.

use std::path::{Path, PathBuf};

use operator_core::types::StepType;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One reversible mutation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op")]
enum UndoEntry {
    /// A file was created or overwritten at `path`. If `backup` is set, restore
    /// it; otherwise the file didn't exist before, so delete it on undo.
    Write {
        path: String,
        backup: Option<String>,
    },
    /// A directory was created at `path` (recorded only if it didn't exist).
    Mkdir { path: String },
    /// `from` was moved to `to`; undo moves it back.
    Move { from: String, to: String },
    /// A path was deleted; a copy was saved to `backup` first (None if the copy
    /// could not be made — then the delete is not reversible).
    Delete {
        path: String,
        backup: Option<String>,
    },
}

fn run_dir(undo_dir: &Path, run_id: &str) -> PathBuf {
    undo_dir.join(run_id)
}

fn manifest_path(undo_dir: &Path, run_id: &str) -> PathBuf {
    run_dir(undo_dir, run_id).join("manifest.jsonl")
}

/// Expands a leading `~`.
fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

/// Records the pre-mutation state of a step into the run's undo journal. A
/// no-op for non-mutating step types. `seq` disambiguates backup filenames.
pub fn record(undo_dir: &Path, run_id: &str, step_type: &StepType, params: &Value, seq: u32) {
    let s = |k: &str| params.get(k).and_then(|v| v.as_str());

    let entry = match step_type {
        StepType::SysWriteFile | StepType::SysAppendFile | StepType::SysCopyPath => {
            // write/append target is `path`; copy target is `to`.
            let key = if *step_type == StepType::SysCopyPath {
                "to"
            } else {
                "path"
            };
            let Some(p) = s(key) else { return };
            let target = expand_tilde(p);
            let backup = backup_if_exists(undo_dir, run_id, &target, seq);
            UndoEntry::Write {
                path: target.to_string_lossy().to_string(),
                backup,
            }
        }
        StepType::SysMkdir => {
            let Some(p) = s("path") else { return };
            let target = expand_tilde(p);
            if target.exists() {
                return; // already existed → nothing to undo
            }
            UndoEntry::Mkdir {
                path: target.to_string_lossy().to_string(),
            }
        }
        StepType::SysMovePath => {
            let (Some(from), Some(to)) = (s("from"), s("to")) else {
                return;
            };
            UndoEntry::Move {
                from: expand_tilde(from).to_string_lossy().to_string(),
                to: expand_tilde(to).to_string_lossy().to_string(),
            }
        }
        StepType::SysDeletePath => {
            let Some(p) = s("path") else { return };
            let target = expand_tilde(p);
            if !target.exists() {
                return;
            }
            // Copy the target aside so the delete is reversible.
            let backup = backup_if_exists(undo_dir, run_id, &target, seq);
            UndoEntry::Delete {
                path: target.to_string_lossy().to_string(),
                backup,
            }
        }
        _ => return,
    };

    if let Err(e) = append_entry(undo_dir, run_id, &entry) {
        tracing::warn!("undo: failed to record entry: {e}");
    }
}

/// Copies `target` (if it exists) into the run's undo dir and returns the backup
/// path, or `None` if it didn't exist / the copy failed.
fn backup_if_exists(undo_dir: &Path, run_id: &str, target: &Path, seq: u32) -> Option<String> {
    if !target.exists() {
        return None;
    }
    let backups = run_dir(undo_dir, run_id).join("backups");
    if std::fs::create_dir_all(&backups).is_err() {
        return None;
    }
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let dest = backups.join(format!("{seq}__{name}"));
    let copy_res = if target.is_dir() {
        copy_dir_recursive(target, &dest)
    } else {
        std::fs::copy(target, &dest).map(|_| ())
    };
    match copy_res {
        Ok(()) => Some(dest.to_string_lossy().to_string()),
        Err(e) => {
            tracing::warn!("undo: backup of {} failed: {e}", target.display());
            None
        }
    }
}

fn append_entry(undo_dir: &Path, run_id: &str, entry: &UndoEntry) -> std::io::Result<()> {
    use std::io::Write;
    let dir = run_dir(undo_dir, run_id);
    std::fs::create_dir_all(&dir)?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(manifest_path(undo_dir, run_id))?;
    let line = serde_json::to_string(entry).unwrap_or_default();
    writeln!(f, "{line}")
}

/// The outcome of an undo.
#[derive(Debug, Default, Serialize)]
pub struct UndoReport {
    pub reversed: usize,
    pub failed: usize,
    pub messages: Vec<String>,
}

/// Reverses a run's recorded mutations (most-recent first). Returns a report;
/// individual failures are collected, not fatal.
pub fn restore(undo_dir: &Path, run_id: &str) -> Result<UndoReport, String> {
    let manifest = manifest_path(undo_dir, run_id);
    let contents = std::fs::read_to_string(&manifest)
        .map_err(|e| format!("no undo journal for run {run_id} ({e})"))?;

    let entries: Vec<UndoEntry> = contents
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();

    let mut report = UndoReport::default();
    for entry in entries.iter().rev() {
        match reverse_one(entry) {
            Ok(msg) => {
                report.reversed += 1;
                report.messages.push(msg);
            }
            Err(e) => {
                report.failed += 1;
                report.messages.push(format!("FAILED: {e}"));
            }
        }
    }
    Ok(report)
}

fn reverse_one(entry: &UndoEntry) -> Result<String, String> {
    match entry {
        UndoEntry::Write { path, backup } | UndoEntry::Delete { path, backup } => {
            let target = PathBuf::from(path);
            match backup {
                Some(b) => {
                    let bp = PathBuf::from(b);
                    if bp.is_dir() {
                        let _ = std::fs::remove_dir_all(&target);
                        copy_dir_recursive(&bp, &target).map_err(|e| e.to_string())?;
                    } else {
                        if let Some(parent) = target.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        std::fs::copy(&bp, &target).map_err(|e| e.to_string())?;
                    }
                    Ok(format!("restored {path}"))
                }
                None => {
                    // No prior state: the file was newly created — remove it.
                    if target.is_dir() {
                        std::fs::remove_dir_all(&target).map_err(|e| e.to_string())?;
                    } else if target.exists() {
                        std::fs::remove_file(&target).map_err(|e| e.to_string())?;
                    }
                    Ok(format!("removed newly-created {path}"))
                }
            }
        }
        UndoEntry::Mkdir { path } => {
            let target = PathBuf::from(path);
            if target.exists() {
                std::fs::remove_dir_all(&target).map_err(|e| e.to_string())?;
            }
            Ok(format!("removed created dir {path}"))
        }
        UndoEntry::Move { from, to } => {
            let from = PathBuf::from(from);
            let to = PathBuf::from(to);
            if to.exists() {
                if let Some(parent) = from.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                std::fs::rename(&to, &from).map_err(|e| e.to_string())?;
            }
            Ok(format!("moved back to {}", from.display()))
        }
    }
}

/// Recursively copies a directory tree.
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tmp() -> PathBuf {
        std::env::temp_dir().join(format!("oj_undo_{}", ulid::Ulid::new()))
    }

    #[test]
    fn undo_restores_overwrite_and_removes_new_file() {
        let base = tmp();
        std::fs::create_dir_all(&base).unwrap();
        let undo_dir = base.join("undo");
        let run = "RUN1";

        let existing = base.join("keep.txt");
        std::fs::write(&existing, "original").unwrap();
        let created = base.join("new.txt");

        record(
            &undo_dir,
            run,
            &StepType::SysWriteFile,
            &json!({ "path": existing.to_str().unwrap() }),
            0,
        );
        std::fs::write(&existing, "modified").unwrap();

        record(
            &undo_dir,
            run,
            &StepType::SysWriteFile,
            &json!({ "path": created.to_str().unwrap() }),
            1,
        );
        std::fs::write(&created, "brand new").unwrap();

        let report = restore(&undo_dir, run).unwrap();
        assert_eq!(report.failed, 0, "{:?}", report.messages);
        assert_eq!(report.reversed, 2);
        assert_eq!(std::fs::read_to_string(&existing).unwrap(), "original");
        assert!(!created.exists(), "newly-created file should be removed");
    }

    #[test]
    fn undo_restores_deleted_file() {
        let base = tmp();
        std::fs::create_dir_all(&base).unwrap();
        let undo_dir = base.join("undo");
        let run = "RUN2";

        let victim = base.join("victim.txt");
        std::fs::write(&victim, "precious").unwrap();

        // Record (copies aside), then the engine performs the real delete.
        record(
            &undo_dir,
            run,
            &StepType::SysDeletePath,
            &json!({ "path": victim.to_str().unwrap() }),
            0,
        );
        std::fs::remove_file(&victim).unwrap();
        assert!(!victim.exists());

        let report = restore(&undo_dir, run).unwrap();
        assert_eq!(report.failed, 0, "{:?}", report.messages);
        assert!(victim.exists(), "deleted file should be restored");
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "precious");
    }
}
