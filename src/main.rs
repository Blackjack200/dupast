//! dupast: source code copy-pasta and duplication detector
//!
//! Detects repetitive patterns in C++/Rust/Java codebases using statement-level
//! tokenization and frequency-weighted similarity.

// Clippy lints for code quality
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![warn(clippy::cargo)]
// Allow certain pedantic lints that don't make sense for this project
#![allow(clippy::too_many_lines)] // Complex functions are sometimes necessary
#![allow(clippy::module_name_repetitions)] // Output modules are intentionally named
#![allow(clippy::must_use_candidate)]
// We use explicit returns for clarity
// Allow dependency version conflicts (tree-sitter ecosystem issue)
#![allow(clippy::multiple_crate_versions)]

mod cli;
mod config;
mod engine;
mod error;
mod output;
mod parser;

use crate::cli::Args;
use crate::config::Config;
use crate::engine::token_engine::TokenEngine;
use crate::error::{DupastError, Result};
use crate::parser::Parser as SourceParser;
use clap::{CommandFactory, Parser as ClapParser};
use clap_complete::{generate, shells::Shell};
use std::io::stdout;
use std::path::PathBuf;
use std::process::ExitCode;
use tracing_subscriber::EnvFilter;

fn main() -> ExitCode {
    let args = Args::parse();

    // Handle shell completion generation
    if let Some(shell_name) = args.generate_completion {
        return generate_completion(&shell_name);
    }

    // Set up logging based on verbosity and quiet mode
    setup_logging(&args);

    // Handle config generation
    if args.generate_config {
        match generate_config() {
            Ok(()) => {
                println!("Default configuration written to dupast.toml");
                return ExitCode::SUCCESS;
            }
            Err(e) => {
                eprintln!("Error generating config: {e}");
                return ExitCode::FAILURE;
            }
        }
    }

    // Run the checker
    match run(args) {
        Ok(has_issues) => {
            if has_issues {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Setup logging with Cargo-like verbosity levels
fn setup_logging(args: &Args) {
    // Check quiet mode (from CLI or env)
    let is_quiet = args.quiet
        || std::env::var("DUPAST_QUIET")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

    let filter = if is_quiet {
        EnvFilter::new("error")
    } else {
        match args.verbose {
            0 => EnvFilter::new("warn"),
            1 => EnvFilter::new("info"),
            2 => EnvFilter::new("debug"),
            _ => EnvFilter::new("trace"),
        }
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();
}

/// Generate shell completion for the specified shell
fn generate_completion(shell_name: &str) -> ExitCode {
    let mut cmd = Args::command();
    let shell_name = shell_name.to_lowercase();

    let shell = match shell_name.as_str() {
        "bash" => Shell::Bash,
        "zsh" => Shell::Zsh,
        "fish" => Shell::Fish,
        "elvish" => Shell::Elvish,
        "powershell" | "pwsh" => Shell::PowerShell,
        _ => {
            eprintln!("Error: Unknown shell '{shell_name}'");
            eprintln!("Supported shells: bash, zsh, fish, elvish, powershell");
            return ExitCode::FAILURE;
        }
    };

    generate(shell, &mut cmd, "dupast", &mut stdout());
    ExitCode::SUCCESS
}

fn run(args: Args) -> Result<bool> {
    let start_time = std::time::Instant::now();

    // Load or use default config
    let mut config = if let Some(ref config_path) = args.config {
        Config::from_file(config_path)?
    } else {
        // Try to find dupast.toml using smart project discovery
        find_and_load_config().unwrap_or_else(|_| Config::default())
    };

    // Apply environment variables (before CLI args, so CLI takes precedence)
    config.apply_env_vars();

    // Merge CLI arguments (CLI takes precedence)
    config.merge_with_args(&args);

    // Validate configuration
    config.validate()?;

    tracing::info!("Starting dupast");
    tracing::info!("Threshold: {}", config.get_threshold());
    tracing::info!("Frequency penalty: {}", config.frequency_penalty);
    if config.fuzzy_identifiers {
        tracing::info!("Fuzzy identifier matching enabled");
    }

    // Determine paths to check
    let paths = if !args.paths.is_empty() {
        args.paths
    } else if !config.paths.is_empty() {
        // Use reference instead of clone - Vec is Copy for iteration
        // We need to clone here because config.paths is used later
        config.paths.clone()
    } else {
        vec![PathBuf::from(".")]
    };

    // Discover source files
    let parser = SourceParser::new(config.clone());
    let files = parser.discover_files(paths.as_slice())?;

    tracing::info!("Found {} source file(s) to analyze", files.len());

    if files.is_empty() {
        return Err(DupastError::NoFilesFound {
            help: format!(
                "Supported extensions: {}\n\
                 Check ignore patterns in config file\n\
                 Try: dupast -vv to see what's being skipped",
                config.extensions.join(", ")
            ),
        });
    }

    // Run the similarity engine using token-based approach
    let token_engine = TokenEngine::new(config.clone());
    let token_pairs = token_engine.run(&files);
    let engine_pairs = TokenEngine::to_engine_pairs(token_pairs);

    // Run intra-file detection if enabled
    let intra_file_results = if config.check_intra_file {
        tracing::info!("Running intra-file duplication detection");
        Vec::new() // Token engine doesn't support intra-file yet
    } else {
        Vec::new()
    };

    // Track timing
    let elapsed = start_time.elapsed();

    // Log timing in verbose mode
    if args.verbose > 0 {
        tracing::info!("Analysis completed in {:.2}s", elapsed.as_secs_f64());
    }

    // Write output
    crate::output::write_output(&config, &engine_pairs, &intra_file_results)?;

    // Return whether issues were found
    Ok(!engine_pairs.is_empty() || !intra_file_results.is_empty())
}

fn generate_config() -> Result<()> {
    Config::generate_default("dupast.toml")
}

/// Find and load config using smart project discovery
/// Searches upward for dupast.toml or common project markers
fn find_and_load_config() -> Result<Config> {
    let project_root = find_project_root();

    if let Some(root) = project_root {
        // Check for dupast.toml in project root
        let config_path = root.join("dupast.toml");
        if config_path.exists() {
            tracing::info!("Loading config from {}", config_path.display());
            return Config::from_file(config_path);
        }
    }

    // Check XDG config location
    if let Some(home) = dirs::home_dir() {
        let xdg_config = home.join(".config/dupast/config.toml");
        if xdg_config.exists() {
            tracing::info!("Loading config from {}", xdg_config.display());
            return Config::from_file(xdg_config);
        }

        // Fallback to ~/.dupast.toml
        let home_config = home.join(".dupast.toml");
        if home_config.exists() {
            tracing::info!("Loading config from {}", home_config.display());
            return Config::from_file(home_config);
        }
    }

    Err(DupastError::Internal(
        "No dupast.toml found in project root or config directories".to_string(),
    ))
}

/// Find project root by scanning upward for markers
/// Like Cargo, scans upward looking for Cargo.toml, .git, etc.
fn find_project_root() -> Option<PathBuf> {
    let current_dir = std::env::current_dir().ok()?;

    // Scan upward looking for project markers
    for ancestor in current_dir.ancestors() {
        // Check for dupast config
        if ancestor.join("dupast.toml").exists() {
            tracing::debug!("Found dupast.toml at {}", ancestor.display());
            return Some(ancestor.to_path_buf());
        }

        // Check for common project markers (like Cargo does)
        let markers = [
            ".git",
            "Cargo.toml",
            "package.json",
            "pom.xml",
            "go.mod",
            "Gemfile",
            "pyproject.toml",
            "setup.py",
            "composer.json",
        ];
        for marker in markers {
            if ancestor.join(marker).exists() {
                tracing::debug!(
                    "Found project marker '{}' at {}",
                    marker,
                    ancestor.display()
                );
                return Some(ancestor.to_path_buf());
            }
        }
    }

    None
}
