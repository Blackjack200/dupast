//! Source parsing module

pub mod synonym_graph;
pub mod token_freq;

use crate::Config;
use crate::error::{DupastError, Result};
use std::path::PathBuf;
use walkdir::{DirEntry, WalkDir};

/// Orchestrator for source file discovery
pub struct Parser {
    config: Config,
}

impl Parser {
    /// Create a new parser with the given configuration
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Discover all supported source files in the given paths
    pub fn discover_files(&self, paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();

        for path in paths {
            if !path.exists() {
                return Err(DupastError::FileNotFound(path.clone()));
            }

            if path.is_file() {
                if self.config.is_supported_extension(path.as_path()) {
                    files.push(path.to_path_buf());
                }
            } else {
                // Walk directory
                let walker = WalkDir::new(path)
                    .follow_links(false)
                    .into_iter()
                    .filter_entry(|e| !self.is_hidden(e));

                for entry in walker {
                    let entry = entry.map_err(|e| match e.into_io_error() {
                        Some(io_err) => DupastError::Io(io_err),
                        None => DupastError::Internal("walkdir error".to_string()),
                    })?;
                    if entry.file_type().is_file() {
                        let path = entry.path();
                        if self.config.is_supported_extension(path)
                            && !self.config.should_ignore(path)
                        {
                            // Check file size
                            if let Ok(metadata) = entry.metadata()
                                && metadata.len() > self.config.max_file_size
                            {
                                tracing::warn!(
                                    "Skipping large file: {} ({} bytes)",
                                    path.display(),
                                    metadata.len()
                                );
                                continue;
                            }
                            files.push(path.to_path_buf());
                        }
                    }
                }
            }
        }

        if files.is_empty() {
            return Err(DupastError::NoFilesFound);
        }

        Ok(files)
    }

    /// Check if a directory entry is hidden
    fn is_hidden(&self, entry: &DirEntry) -> bool {
        entry
            .file_name()
            .to_str()
            .map(|s| s.starts_with('.'))
            .unwrap_or(false)
    }
}
