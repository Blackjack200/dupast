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

    #[error("No source files found\n\n{help}")]
    NoFilesFound { help: String },

    #[allow(dead_code)] // Infrastructure for future validation features
    #[error("Threshold must be between 0 and 100, got {value}\n\n{help}")]
    InvalidThreshold { value: f64, help: String },

    #[allow(dead_code)] // Infrastructure for future validation features
    #[error("Frequency penalty must be between 0.0 and 10.0, got {value}\n\n{help}")]
    InvalidFrequencyPenalty { value: f64, help: String },

    #[error("Invalid output format '{format}'\n\n{help}")]
    InvalidOutputFormat { format: String, help: String },

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Result type alias for dupast
pub type Result<T> = std::result::Result<T, DupastError>;
