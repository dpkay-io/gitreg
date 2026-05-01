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

    #[cfg(not(windows))]
    #[error("gitreg already initialized in {0}")]
    AlreadyInitialized(PathBuf),

    #[cfg(windows)]
    #[error("`gitreg init` is not supported on Windows; use WSL or Git Bash")]
    UnsupportedPlatform,

    #[error("network error: {0}")]
    Network(String),

    #[error("upgrade failed: {0}")]
    Upgrade(String),

    #[error("could not resolve executable path: {0}")]
    ExePath(std::io::Error),
}

pub type Result<T> = std::result::Result<T, GitregError>;
