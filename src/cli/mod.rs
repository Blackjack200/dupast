//! Command-line interface definitions using clap

use clap::Parser;
use std::path::PathBuf;

/// C++ copy-pasta and duplication detector
#[derive(Parser, Debug, Clone)]
#[command(name = "dupast")]
#[command(author = "dupast contributors")]
#[command(version = "0.1.0")]
#[command(about = "Detect code duplication in C++ codebases", long_about = None)]
pub struct Args {
    /// C++ files/directories to check (default: ".")
    #[arg(value_name = "PATHS")]
    pub paths: Vec<PathBuf>,

    /// Similarity threshold percentage (0-100)
    #[arg(short = 't', long = "threshold", value_name = "PERCENT")]
    pub threshold: Option<f64>,

    /// Config file path
    #[arg(short = 'c', long = "config", value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Number of parallel jobs (0 = CPU count)
    #[arg(short = 'j', long = "jobs", value_name = "N")]
    pub jobs: Option<usize>,

    /// Output format (human, json, sarif)
    #[arg(short = 'o', long = "output", value_name = "FORMAT")]
    pub output_format: Option<String>,

    /// Minimum tokens to consider as duplicate block
    #[arg(long = "min-lines", value_name = "N")]
    pub min_lines: Option<usize>,

    /// Frequency penalty for token similarity (0.0 - 10.0)
    #[arg(long = "frequency-penalty", value_name = "PENALTY")]
    pub frequency_penalty: Option<f64>,

    /// Enable fuzzy identifier matching using semantic vectors
    #[arg(long = "fuzzy-identifiers")]
    pub fuzzy_identifiers: bool,

    /// Minimum similarity for fuzzy identifier matching (0.0 - 1.0)
    #[arg(long = "fuzzy-threshold", value_name = "THRESHOLD")]
    pub fuzzy_threshold: Option<f32>,

    /// Verbose mode (-v, -vv, -vvv)
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Disable intra-file duplication detection
    #[arg(long = "no-intra-file")]
    pub no_intra_file: bool,

    /// Generate default config file
    #[arg(long = "generate-config")]
    pub generate_config: bool,
}

impl Args {
    pub fn get_threshold(&self) -> Option<f64> {
        self.threshold.map(|t| t / 100.0)
    }
}
