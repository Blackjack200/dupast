//! Error types for dupast

use std::path::PathBuf;
use thiserror::Error;

/// Main error type for dupast
#[derive(Error, Debug)]
pub enum DupastError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to parse config file: {0}")]
    ConfigParse(#[from] toml::de::Error),

    #[error("File not found: {0}")]
    FileNotFound(PathBuf),

    #[error("No C++ files found in the specified paths")]
    NoFilesFound,

    #[error("Invalid threshold value: {0}. Must be between 0.0 and 1.0")]
    InvalidThreshold(f64),

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Result type alias for dupast
pub type Result<T> = std::result::Result<T, DupastError>;
