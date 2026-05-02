use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GitregError {
    #[error("could not determine config directory")]
    NoConfigDir,

    #[error("path not found: {0}")]
    PathNotFound(PathBuf),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("gitreg already initialized in {0}")]
    AlreadyInitialized(PathBuf),

    #[error("network error: {0}")]
    Network(String),

    #[error("upgrade failed: {0}")]
    Upgrade(String),

    #[error("could not resolve executable path: {0}")]
    ExePath(std::io::Error),

    #[error("no repository found matching '{0}'")]
    NotFound(String),

    #[error("invalid format: {0}")]
    InvalidFormat(String),
}

pub type Result<T> = std::result::Result<T, GitregError>;
