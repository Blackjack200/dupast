//! Configuration file parsing and management

use crate::error::{DupastError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Default configuration template as TOML
pub const DEFAULT_CONFIG_TEMPLATE: &str = include_str!("../../dupast.toml");

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Paths to scan when no CLI PATHS are provided
    #[serde(default)]
    pub paths: Vec<PathBuf>,

    /// Similarity threshold (0.0 - 1.0)
    pub threshold: Option<f64>,

    /// Minimum tokens to consider as duplicate block
    #[serde(default = "default_min_block_lines")]
    pub min_block_lines: usize,

    /// Ignore paths (glob patterns)
    #[serde(default)]
    pub ignore: Vec<String>,

    /// Check intra-file duplication
    #[serde(default = "default_check_intra_file")]
    pub check_intra_file: bool,

    /// Output format
    #[serde(default = "default_output_format")]
    pub output_format: String,

    /// File extensions to scan
    #[serde(default = "default_extensions")]
    pub extensions: Vec<String>,

    /// Maximum file size in bytes
    #[serde(default = "default_max_file_size")]
    pub max_file_size: u64,

    /// Number of parallel jobs (0 = CPU count)
    #[serde(default)]
    pub jobs: Option<usize>,

    /// Frequency penalty for token similarity (0.0 - 10.0)
    /// Higher values penalize common tokens more heavily
    #[serde(default = "default_frequency_penalty")]
    pub frequency_penalty: f64,

    /// Enable fuzzy identifier matching using semantic vectors
    #[serde(default = "default_fuzzy_identifiers")]
    pub fuzzy_identifiers: bool,

    /// Minimum similarity threshold for fuzzy identifier matching (0.0 - 1.0)
    #[serde(default = "default_fuzzy_threshold")]
    pub fuzzy_identifier_threshold: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            threshold: None,
            min_block_lines: default_min_block_lines(),
            ignore: Vec::new(),
            check_intra_file: default_check_intra_file(),
            output_format: default_output_format(),
            extensions: default_extensions(),
            max_file_size: default_max_file_size(),
            jobs: None,
            frequency_penalty: default_frequency_penalty(),
            fuzzy_identifiers: default_fuzzy_identifiers(),
            fuzzy_identifier_threshold: default_fuzzy_threshold(),
        }
    }
}

// Default value functions
fn default_min_block_lines() -> usize {
    3
}

fn default_check_intra_file() -> bool {
    true
}

fn default_output_format() -> String {
    "human".to_string()
}

fn default_extensions() -> Vec<String> {
    vec![
        "cpp".to_string(),
        "cc".to_string(),
        "cxx".to_string(),
        "hpp".to_string(),
        "h".to_string(),
        "hxx".to_string(),
        "rs".to_string(),
        "java".to_string(),
    ]
}

fn default_max_file_size() -> u64 {
    1_048_576 // 1MB
}

fn default_frequency_penalty() -> f64 {
    2.0
}

fn default_fuzzy_identifiers() -> bool {
    false
}

fn default_fuzzy_threshold() -> f32 {
    0.6
}

impl Config {
    /// Load configuration from a file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(&path).map_err(DupastError::Io)?;

        let config: Self = toml::from_str(&content)?;

        // Validate threshold value
        if let Some(threshold) = config.threshold
            && !(0.0..=1.0).contains(&threshold)
        {
            return Err(DupastError::InvalidThreshold(threshold));
        }

        Ok(config)
    }

    /// Generate default config file at the specified path
    pub fn generate_default<P: AsRef<Path>>(path: P) -> Result<()> {
        fs::write(path, DEFAULT_CONFIG_TEMPLATE)?;
        Ok(())
    }

    /// Merge CLI arguments into config (CLI args take precedence)
    pub fn merge_with_args(&mut self, args: &crate::cli::Args) {
        if let Some(threshold) = args.get_threshold() {
            self.threshold = Some(threshold);
        }

        if let Some(min_lines) = args.min_lines {
            self.min_block_lines = min_lines;
        }

        if let Some(frequency_penalty) = args.frequency_penalty {
            self.frequency_penalty = frequency_penalty;
        }

        if args.fuzzy_identifiers {
            self.fuzzy_identifiers = true;
        }

        if let Some(fuzzy_threshold) = args.fuzzy_threshold {
            self.fuzzy_identifier_threshold = fuzzy_threshold;
        }

        if let Some(ref output_format) = args.output_format {
            self.output_format = output_format.clone();
        }

        if args.no_intra_file {
            self.check_intra_file = false;
        }

        if let Some(jobs) = args.jobs {
            self.jobs = Some(jobs);
        }
    }

    /// Get effective threshold (returns default if not set)
    pub fn get_threshold(&self) -> f64 {
        self.threshold.unwrap_or(0.85)
    }

    /// Check if a path should be ignored based on ignore patterns
    pub fn should_ignore(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy().to_string();

        for pattern in &self.ignore {
            // Try full path match first
            if self.matches_glob(&path_str, pattern) {
                return true;
            }

            // Also check if path contains any ignored directory
            // Extract directory names from pattern
            if pattern.contains('/') {
                let dir_name = pattern.split('/').next().unwrap_or(pattern);
                // Check if path contains this directory
                if path_str.contains(&format!("/{}/", dir_name))
                    || path_str.contains(&format!("{}\\", dir_name))
                    || path_str.starts_with(&format!("{}/", dir_name))
                    || path_str.starts_with(&format!("{}\\", dir_name))
                {
                    return true;
                }
            }
        }

        false
    }

    /// Simple glob pattern matching
    fn matches_glob(&self, text: &str, pattern: &str) -> bool {
        // Convert glob pattern to regex
        // Handle **/ (match any number of directories)
        let regex_pattern = pattern
            .replace("**/", "(.*/)?") // **/ matches any path prefix
            .replace("**", ".*")
            .replace('*', "[^/]*")
            .replace('?', ".");

        if let Ok(re) = regex_matcher::Regex::new(&regex_pattern) {
            re.is_match(text)
        } else {
            false
        }
    }

    /// Check if a file extension is supported
    pub fn is_supported_extension(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| {
                self.extensions
                    .iter()
                    .any(|supported| supported.eq_ignore_ascii_case(ext))
            })
            .unwrap_or(false)
    }
}

/// Simple regex module for glob matching
mod regex_matcher {
    pub struct Regex {
        inner: String,
    }

    impl Regex {
        pub fn new(pattern: &str) -> Result<Self, Box<dyn std::error::Error>> {
            Ok(Regex {
                inner: format!("^{}$", pattern),
            })
        }

        pub fn is_match(&self, text: &str) -> bool {
            // Very simple glob matching without regex crate
            self.glob_match(&self.inner, text)
        }

        fn glob_match(&self, pattern: &str, text: &str) -> bool {
            let pattern = pattern.strip_prefix('^').unwrap_or(pattern);
            let pattern = pattern.strip_suffix('$').unwrap_or(pattern);

            if pattern == ".*" {
                return true;
            }

            // Simple wildcard matching
            let parts: Vec<&str> = pattern.split('*').collect();
            if parts.len() == 1 {
                return text == pattern;
            }

            let mut idx = 0;
            for (i, part) in parts.iter().enumerate() {
                if part.is_empty() {
                    continue;
                }

                if i == 0 {
                    if !text.starts_with(part) {
                        return false;
                    }
                    idx = part.len();
                } else if i == parts.len() - 1 {
                    if !text[idx..].ends_with(part) {
                        return false;
                    }
                } else if let Some(pos) = text[idx..].find(part) {
                    idx += pos + part.len();
                } else {
                    return false;
                }
            }

            true
        }
    }
}
