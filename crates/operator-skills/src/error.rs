//! Error types for the skills module.

use thiserror::Error;

/// Errors that can occur when working with skills.
#[derive(Debug, Error)]
pub enum SkillError {
    #[error("Skill not found: {0}")]
    NotFound(String),

    #[error("Failed to parse skill manifest: {0}")]
    ParseError(String),

    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("Missing required parameter: {0}")]
    MissingParameter(String),

    #[error("Invalid parameter value for '{name}': {reason}")]
    InvalidParameterValue { name: String, reason: String },

    #[error("Skill expansion failed: {0}")]
    ExpansionError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("YAML error: {0}")]
    YamlError(#[from] serde_yaml::Error),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
