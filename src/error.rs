use std::path::PathBuf;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, ProfileError>;

#[derive(Debug, Error)]
pub enum ProfileError {
    #[error("stdin is not supported in v0.1.0; pass a JSON or JSONL file path")]
    StdinUnsupported,

    #[error("input path is not a file: {0}")]
    InputNotFile(PathBuf),

    #[error("refs sqlite path is not a file: {0}")]
    RefsNotFile(PathBuf),

    #[error("invalid config: {0}")]
    InvalidConfig(String),

    #[error("failed to read config {path}: {source}")]
    ConfigRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse config {path}: {source}")]
    ConfigParse {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },

    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("json parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
