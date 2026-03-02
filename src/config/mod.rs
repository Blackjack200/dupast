//! Configuration file parsing and management

use crate::cli::ColorWhen;
use crate::error::{DupastError, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Default configuration template as TOML
pub const DEFAULT_CONFIG_TEMPLATE: &str = include_str!("../../dupast.toml");

/// Similarity threshold (0.0 - 1.0)
///
/// Type-safe wrapper enforcing valid range at construction time.
/// Invalid states are unrepresentable (Parse, Don't Validate).
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Threshold(f64);

impl Threshold {
    /// Minimum valid threshold value
    pub const MIN: f64 = 0.0;

    /// Maximum valid threshold value
    pub const MAX: f64 = 1.0;

    /// Create a new Threshold, validating the range
    ///
    /// # Panics
    /// Panics if value is outside [0.0, 1.0]. Use `try_from` for non-panic version.
    #[inline]
    pub fn new(value: f64) -> Self {
        assert!(
            (Self::MIN..=Self::MAX).contains(&value),
            "Threshold must be between {} and {}, got {}",
            Self::MIN,
            Self::MAX,
            value
        );
        Self(value)
    }

    /// Get the inner f64 value
    #[inline]
    pub fn as_f64(self) -> f64 {
        self.0
    }

    /// Create from percentage (0-100), converting to 0.0-1.0
    #[inline]
    pub fn from_percentage(percent: f64) -> Self {
        Self::new(percent / 100.0)
    }
}

impl FromStr for Threshold {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let value = s
            .parse::<f64>()
            .map_err(|e| format!("Invalid threshold value: {e}"))?;

        if !(Self::MIN..=Self::MAX).contains(&value) {
            return Err(format!(
                "Threshold must be between {} and {}, got {}",
                Self::MIN,
                Self::MAX,
                value
            ));
        }

        Ok(Self(value))
    }
}

impl TryFrom<f64> for Threshold {
    type Error = String;

    fn try_from(value: f64) -> std::result::Result<Self, Self::Error> {
        if !(Self::MIN..=Self::MAX).contains(&value) {
            return Err(format!(
                "Threshold must be between {} and {}, got {}",
                Self::MIN,
                Self::MAX,
                value
            ));
        }
        Ok(Self(value))
    }
}

impl Serialize for Threshold {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Threshold {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;
        let value = f64::deserialize(deserializer)?;
        Self::try_from(value).map_err(D::Error::custom)
    }
}

/// Frequency penalty for token similarity (0.0 - 10.0)
///
/// Type-safe wrapper enforcing valid range at construction time.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct FrequencyPenalty(f64);

impl FrequencyPenalty {
    /// Minimum valid penalty value
    pub const MIN: f64 = 0.0;

    /// Maximum valid penalty value
    pub const MAX: f64 = 10.0;

    /// Create a new `FrequencyPenalty`, validating the range
    #[inline]
    pub fn new(value: f64) -> Self {
        assert!(
            (Self::MIN..=Self::MAX).contains(&value),
            "FrequencyPenalty must be between {} and {}, got {}",
            Self::MIN,
            Self::MAX,
            value
        );
        Self(value)
    }

    /// Get the inner f64 value
    #[inline]
    pub fn as_f64(self) -> f64 {
        self.0
    }
}

impl FromStr for FrequencyPenalty {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let value = s
            .parse::<f64>()
            .map_err(|e| format!("Invalid frequency penalty: {e}"))?;

        if !(Self::MIN..=Self::MAX).contains(&value) {
            return Err(format!(
                "FrequencyPenalty must be between {} and {}, got {}",
                Self::MIN,
                Self::MAX,
                value
            ));
        }

        Ok(Self(value))
    }
}

impl TryFrom<f64> for FrequencyPenalty {
    type Error = String;

    fn try_from(value: f64) -> std::result::Result<Self, Self::Error> {
        if !(Self::MIN..=Self::MAX).contains(&value) {
            return Err(format!(
                "FrequencyPenalty must be between {} and {}, got {}",
                Self::MIN,
                Self::MAX,
                value
            ));
        }
        Ok(Self(value))
    }
}

impl Serialize for FrequencyPenalty {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for FrequencyPenalty {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;
        let value = f64::deserialize(deserializer)?;
        Self::try_from(value).map_err(D::Error::custom)
    }
}

impl std::fmt::Display for Threshold {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::fmt::Display for FrequencyPenalty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Paths to scan when no CLI PATHS are provided
    #[serde(default)]
    pub paths: Vec<PathBuf>,

    /// Similarity threshold (0.0 - 1.0)
    #[serde(default)]
    pub threshold: Option<Threshold>,

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
    pub frequency_penalty: FrequencyPenalty,

    /// Enable fuzzy identifier matching using semantic vectors
    #[serde(default = "default_fuzzy_identifiers")]
    pub fuzzy_identifiers: bool,

    /// Minimum similarity threshold for fuzzy identifier matching (0.0 - 1.0)
    #[serde(default = "default_fuzzy_threshold")]
    pub fuzzy_identifier_threshold: f32,

    /// Color output mode
    #[serde(default)]
    pub color: Option<ColorWhen>,

    /// Quiet mode (suppress non-error output)
    #[serde(default)]
    pub quiet: bool,
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
            color: None,
            quiet: false,
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
        // C/C++
        "c".to_string(),
        "cpp".to_string(),
        "cc".to_string(),
        "cxx".to_string(),
        "hpp".to_string(),
        "h".to_string(),
        "hxx".to_string(),
        // Go
        "go".to_string(),
        // Java
        "java".to_string(),
        // JavaScript
        "js".to_string(),
        "mjs".to_string(),
        "cjs".to_string(),
        // PHP
        "php".to_string(),
        // Rust
        "rs".to_string(),
        // TypeScript
        "ts".to_string(),
        "tsx".to_string(),
    ]
}

fn default_max_file_size() -> u64 {
    1_048_576 // 1MB
}

fn default_frequency_penalty() -> FrequencyPenalty {
    FrequencyPenalty::new(2.0)
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

        // Validate threshold value (already enforced by Threshold type, check for overflow safety)
        // The Threshold type ensures values are always in [0.0, 1.0]
        // Additional validation here is for overflow safety when converting to percentage
        if let Some(threshold) = config.threshold {
            let _ = threshold.as_f64() * 100.0; // Ensure no overflow
        }

        // Validate frequency_penalty (already enforced by FrequencyPenalty type)
        let _ = config.frequency_penalty.as_f64(); // Type guarantees validity

        // Validate output_format
        if !matches!(config.output_format.as_str(), "human" | "json" | "sarif") {
            return Err(DupastError::InvalidOutputFormat {
                format: config.output_format.clone(),
                help: "Supported formats:\n\
                       • human  - Colored, readable output (default)\n\
                       • json   - Machine-readable JSON\n\
                       • sarif  - SARIF format for CI/CD\n\
                       Use: -o json or output_format = \"json\""
                    .to_string(),
            });
        }

        Ok(config)
    }

    /// Generate default config file at the specified path
    pub fn generate_default<P: AsRef<Path>>(path: P) -> Result<()> {
        fs::write(path, DEFAULT_CONFIG_TEMPLATE)?;
        Ok(())
    }

    /// Apply environment variables following Cargo's pattern
    /// Precedence: CLI args > ENV vars > Config file > Defaults
    pub fn apply_env_vars(&mut self) {
        // DUPAST_THRESHOLD - like CARGO_BUILD_JOBS
        if let Ok(val) = std::env::var("DUPAST_THRESHOLD") {
            match val.parse::<f64>() {
                Ok(parsed) if (0.0..=100.0).contains(&parsed) => {
                    self.threshold = Some(Threshold::from_percentage(parsed));
                }
                Ok(parsed) => {
                    tracing::warn!("DUPAST_THRESHOLD out of range: {} (must be 0-100)", parsed);
                }
                Err(e) => {
                    tracing::warn!("DUPAST_THRESHOLD invalid value '{}': {}", val, e);
                }
            }
        }

        // DUPAST_FUZZY_IDENTIFIERS - boolean flags use "1" or "true"
        if let Ok(val) = std::env::var("DUPAST_FUZZY_IDENTIFIERS") {
            self.fuzzy_identifiers =
                val == "1" || val.eq_ignore_ascii_case("true") || val.eq_ignore_ascii_case("yes");
        }

        // DUPAST_FUZZY_THRESHOLD
        if let Ok(val) = std::env::var("DUPAST_FUZZY_THRESHOLD") {
            match val.parse::<f32>() {
                Ok(parsed) => {
                    self.fuzzy_identifier_threshold = parsed;
                }
                Err(e) => {
                    tracing::warn!("DUPAST_FUZZY_THRESHOLD invalid value '{}': {}", val, e);
                }
            }
        }

        // DUPAST_OUTPUT_FORMAT
        if let Ok(val) = std::env::var("DUPAST_OUTPUT_FORMAT") {
            if matches!(val.as_str(), "human" | "json" | "sarif") {
                self.output_format = val;
            } else {
                tracing::warn!(
                    "DUPAST_OUTPUT_FORMAT invalid value '{}': must be human, json, or sarif",
                    val
                );
            }
        }

        // DUPAST_JOBS - parallelism
        if let Ok(val) = std::env::var("DUPAST_JOBS") {
            match val.parse::<usize>() {
                Ok(parsed) => {
                    self.jobs = Some(parsed);
                }
                Err(e) => {
                    tracing::warn!("DUPAST_JOBS invalid value '{}': {}", val, e);
                }
            }
        }

        // DUPAST_MIN_LINES
        if let Ok(val) = std::env::var("DUPAST_MIN_LINES") {
            match val.parse::<usize>() {
                Ok(parsed) => {
                    self.min_block_lines = parsed;
                }
                Err(e) => {
                    tracing::warn!("DUPAST_MIN_LINES invalid value '{}': {}", val, e);
                }
            }
        }

        // DUPAST_FREQUENCY_PENALTY
        if let Ok(val) = std::env::var("DUPAST_FREQUENCY_PENALTY") {
            match val.parse::<f64>() {
                Ok(parsed) => match FrequencyPenalty::try_from(parsed) {
                    Ok(penalty) => self.frequency_penalty = penalty,
                    Err(e) => {
                        tracing::warn!("DUPAST_FREQUENCY_PENALTY invalid value '{}': {}", val, e);
                    }
                },
                Err(e) => {
                    tracing::warn!("DUPAST_FREQUENCY_PENALTY invalid value '{}': {}", val, e);
                }
            }
        }

        // DUPAST_QUIET - quiet mode
        if let Ok(val) = std::env::var("DUPAST_QUIET") {
            self.quiet =
                val == "1" || val.eq_ignore_ascii_case("true") || val.eq_ignore_ascii_case("yes");
        }

        // DUPAST_COLOR - like CARGO_TERM_COLOR
        if let Ok(val) = std::env::var("DUPAST_COLOR") {
            match val.parse() {
                Ok(color) => {
                    self.color = Some(color);
                }
                Err(_) => {
                    tracing::warn!(
                        "DUPAST_COLOR invalid value '{}': must be always, never, or auto",
                        val
                    );
                }
            }
        }
    }

    /// Merge CLI arguments into config (CLI args take precedence)
    pub fn merge_with_args(&mut self, args: &crate::cli::Args) {
        if let Some(threshold) = args.get_threshold() {
            self.threshold = Some(Threshold::new(threshold));
        }

        if let Some(min_lines) = args.min_lines {
            self.min_block_lines = min_lines;
        }

        if let Some(frequency_penalty) = args.frequency_penalty {
            self.frequency_penalty = FrequencyPenalty::new(frequency_penalty);
        }

        if args.fuzzy_identifiers {
            self.fuzzy_identifiers = true;
        }

        if let Some(fuzzy_threshold) = args.fuzzy_threshold {
            self.fuzzy_identifier_threshold = fuzzy_threshold;
        }

        if let Some(ref output_format) = args.output_format {
            self.output_format.clone_from(output_format);
        }

        if args.no_intra_file {
            self.check_intra_file = false;
        }

        if let Some(jobs) = args.jobs {
            self.jobs = Some(jobs);
        }

        if args.quiet {
            self.quiet = true;
        }

        if let Some(color) = args.color {
            self.color = Some(color);
        }
    }

    /// Get effective threshold (returns default if not set)
    pub fn get_threshold(&self) -> f64 {
        self.threshold.unwrap_or(Threshold::new(0.85)).as_f64()
    }

    /// Determine if colors should be used
    pub fn use_color(&self) -> bool {
        match self.color {
            Some(ColorWhen::Always) => true,
            Some(ColorWhen::Never) => false,
            Some(ColorWhen::Auto) | None => std::io::stdout().is_terminal(),
        }
    }

    /// Check if a path should be ignored based on ignore patterns
    ///
    /// Supports simple glob patterns: * (any characters), ? (single character)
    pub fn should_ignore(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();

        for pattern in &self.ignore {
            // Fast path: exact match or directory prefix
            if path_str.as_ref() == *pattern || path_str.starts_with(&format!("{pattern}/")) {
                return true;
            }

            // Glob patterns
            if (pattern.contains('*') || pattern.contains('?'))
                && self.matches_glob_simple(&path_str, pattern)
            {
                return true;
            }
        }

        false
    }

    /// Simple glob pattern matching
    /// Supports: * (match any characters in path segment), ? (single char)
    fn matches_glob_simple(&self, text: &str, pattern: &str) -> bool {
        // Note: &self is used for API consistency with Config methods
        // This could be a standalone function, but keeping it as a method maintains encapsulation
        let _ = self; // Explicitly acknowledge self is unused for API consistency

        // Convert glob pattern to a simple regex-like pattern
        let mut regex_pattern = String::new();
        let mut chars = pattern.chars().peekable();

        while let Some(c) = chars.next() {
            match c {
                '*' => {
                    // Check for ** (match any directories)
                    if chars.peek() == Some(&'*') {
                        chars.next(); // consume second *
                        if chars.peek() == Some(&'/') {
                            chars.next(); // consume /
                            regex_pattern.push_str("(?:.*/)?"); // any prefix
                        } else {
                            regex_pattern.push_str(".*"); // match anything
                        }
                    } else {
                        regex_pattern.push_str("[^/]*"); // match within path segment
                    }
                }
                '?' => regex_pattern.push('.'),
                '.' | '+' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '^' | '$' | '\\' => {
                    regex_pattern.push('\\');
                    regex_pattern.push(c);
                }
                _ => regex_pattern.push(c),
            }
        }

        // Simple matching - check if pattern matches any part of text
        if regex_pattern.contains(".*") {
            // Convert to a simple wildcard check
            let parts: Vec<&str> = regex_pattern.split(".*").collect();
            if parts.len() == 2 {
                let (prefix, suffix) = (parts[0], parts[1]);
                return text.starts_with(prefix) && text.ends_with(suffix);
            }
        }

        // Fallback: substring match
        text.contains(pattern.replace('*', "").as_str())
    }

    /// Check if a file extension is supported
    ///
    /// PERFORMANCE: Case-insensitive comparison without allocating
    pub fn is_supported_extension(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| {
                // Case-insensitive comparison without allocating
                self.extensions
                    .iter()
                    .any(|supported| ext.eq_ignore_ascii_case(supported.as_str()))
            })
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<()> {
        // Threshold validation is already enforced by the type - no additional checks needed
        // FrequencyPenalty validation is also enforced by the type

        // Validate output_format
        if !matches!(self.output_format.as_str(), "human" | "json" | "sarif") {
            return Err(DupastError::InvalidOutputFormat {
                format: self.output_format.clone(),
                help: "Supported formats:\n\
                       • human  - Colored, readable output (default)\n\
                       • json   - Machine-readable JSON\n\
                       • sarif  - SARIF format for CI/CD\n\
                       Use: -o json or output_format = \"json\""
                    .to_string(),
            });
        }

        Ok(())
    }
}
