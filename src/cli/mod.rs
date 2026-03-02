//! Command-line interface definitions using clap

use clap::Parser;
use std::path::PathBuf;

/// Multi-language copy-pasta and duplication detector
#[derive(Parser, Debug, Clone)]
#[command(name = "dupast")]
#[command(author = "dupast contributors")]
#[command(version = "0.1.0")]
#[command(
    about = "Detect code duplication across multiple programming languages",
    long_about = "Fast code duplication detector supporting C, C++, Go, Java, JavaScript, PHP, Rust, and TypeScript",
    after_help = EXAMPLES
)]
// Allow multiple bools - CLI flags are appropriate as individual bools
// State machine would overcomplicate the API for simple flag options
#[allow(clippy::struct_excessive_bools)]
pub struct Args {
    /// Source files/directories to check (default: ".")
    #[arg(value_name = "PATHS")]
    pub paths: Vec<PathBuf>,

    /// Similarity threshold percentage (0-100)
    ///
    /// NOTE: This value is divided by 100 internally to get the 0.0-1.0 range
    /// used by the similarity engine. For example, -t 85 becomes 0.85.
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
    #[arg(short = 'L', long = "min-lines", value_name = "N")]
    pub min_lines: Option<usize>,

    /// Frequency penalty for token similarity (0.0 - 10.0)
    #[arg(short = 'f', long = "frequency-penalty", value_name = "PENALTY")]
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

    /// Quiet mode (errors only, suppresses warnings)
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Disable intra-file duplication detection
    #[arg(long = "no-intra-file")]
    pub no_intra_file: bool,

    /// Color output (always, never, auto)
    #[arg(long = "color", value_name = "WHEN")]
    pub color: Option<ColorWhen>,

    /// Generate shell completion for specified shell
    #[arg(long = "completion", value_name = "SHELL", hide = true)]
    pub generate_completion: Option<String>,

    /// Generate default config file
    #[arg(long = "generate-config")]
    pub generate_config: bool,
}

/// When to use colors in output
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ColorWhen {
    Auto,
    Always,
    Never,
}

impl std::str::FromStr for ColorWhen {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "always" => Ok(ColorWhen::Always),
            "never" => Ok(ColorWhen::Never),
            "auto" => Ok(ColorWhen::Auto),
            _ => Err(format!("invalid color value: {s}")),
        }
    }
}

impl std::fmt::Display for ColorWhen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ColorWhen::Always => write!(f, "always"),
            ColorWhen::Never => write!(f, "never"),
            ColorWhen::Auto => write!(f, "auto"),
        }
    }
}

const EXAMPLES: &str = r"EXAMPLES:
    # Scan current directory
    $ dupast

    # Scan with lower threshold
    $ dupast -t 80 src/

    # Enable fuzzy semantic matching
    $ dupast --fuzzy-identifiers

    # JSON output for CI
    $ dupast -o json

    # Generate shell completions
    $ dupast --completion bash > ~/.local/share/bash-completion/completions/dupast

ENVIRONMENT VARIABLES:
    DUPAST_THRESHOLD         Similarity threshold (0-100)
    DUPAST_FUZZY_IDENTIFIERS Enable fuzzy matching (1/true)
    DUPAST_FUZZY_THRESHOLD   Fuzzy identifier threshold (0.0-1.0)
    DUPAST_JOBS             Parallelism (0 = CPU count)
    DUPAST_OUTPUT_FORMAT    Output format (human/json/sarif)
    DUPAST_MIN_LINES        Minimum block lines
    DUPAST_FREQUENCY_PENALTY Token frequency penalty (0.0-10.0)
    DUPAST_QUIET            Quiet mode (1/true)
    DUPAST_COLOR            Color output (always/never/auto)";

impl Args {
    pub fn get_threshold(&self) -> Option<f64> {
        self.threshold.map(|t| t / 100.0)
    }
}
