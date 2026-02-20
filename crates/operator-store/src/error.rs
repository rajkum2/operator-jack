#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Migration error: {0}")]
    Migration(String),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Not found: {entity} with id {id}")]
    NotFound { entity: String, id: String },
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
