use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("failed to execute statement: {0}")]
    Execute(String),
    #[error("failed to load extension: {0}")]
    LoadExtension(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("path error: {0}")]
    Path(String),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("database error: {0}")]
    Database(String),
    #[error("row error: {0}")]
    Row(String),
    #[error("database lock poisoned")]
    Poisoned,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
