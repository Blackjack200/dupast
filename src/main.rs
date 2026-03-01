//! dupast: C++ copy-pasta and duplication detector
//!
//! Detects repetitive patterns in C++ codebases using statement-level
//! tokenization and frequency-weighted similarity.

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
use crate::parser::Parser as CppParser;
use clap::Parser as ClapParser;
use std::path::PathBuf;
use std::process::ExitCode;
use tracing::Level;

fn main() -> ExitCode {
    let args = Args::parse();

    // Set up logging based on verbosity
    let log_level = match args.verbose {
        0 => Level::WARN,
        1 => Level::INFO,
        2 => Level::DEBUG,
        _ => Level::TRACE,
    };

    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .with_target(false)
        .init();

    // Handle config generation
    if args.generate_config {
        match generate_config() {
            Ok(()) => {
                println!("Default configuration written to dupast.toml");
                return ExitCode::SUCCESS;
            }
            Err(e) => {
                eprintln!("Error generating config: {}", e);
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
            eprintln!("Error: {}", e);
            ExitCode::FAILURE
        }
    }
}

fn run(args: Args) -> Result<bool> {
    // Load or use default config
    let mut config = if let Some(ref config_path) = args.config {
        Config::from_file(config_path)?
    } else {
        // Try to load dupast.toml from current working directory
        find_and_load_config().unwrap_or_else(|_| Config::default())
    };

    // Merge CLI arguments (CLI takes precedence)
    config.merge_with_args(&args);

    tracing::info!("Starting dupast");
    tracing::info!("Threshold: {}", config.get_threshold());
    tracing::info!("Frequency penalty: {}", config.frequency_penalty);

    // Determine paths to check
    let paths = if !args.paths.is_empty() {
        args.paths
    } else if !config.paths.is_empty() {
        config.paths.clone()
    } else {
        vec![PathBuf::from(".")]
    };

    // Discover C++ files
    let parser = CppParser::new(config.clone());
    let files = parser.discover_files(paths.as_slice())?;

    tracing::info!("Found {} C++ file(s) to analyze", files.len());

    if files.is_empty() {
        eprintln!("No C++ files found in the specified paths");
        return Ok(false);
    }

    // Run the similarity engine using token-based approach
    let token_engine = TokenEngine::new(config.clone());
    let token_pairs = token_engine
        .run(files.clone())
        .map_err(|e| DupastError::Internal(format!("Token engine failed: {}", e)))?;
    let engine_pairs = TokenEngine::to_engine_pairs(token_pairs);

    // Run intra-file detection if enabled
    let intra_file_results = if config.check_intra_file {
        tracing::info!("Running intra-file duplication detection");
        Vec::new() // Token engine doesn't support intra-file yet
    } else {
        Vec::new()
    };

    // Write output
    crate::output::write_output(&config, &engine_pairs, &intra_file_results)?;

    // Return whether issues were found
    Ok(!engine_pairs.is_empty() || !intra_file_results.is_empty())
}

fn generate_config() -> Result<()> {
    Config::generate_default("dupast.toml")
}

fn find_and_load_config() -> Result<Config> {
    let config_path = std::env::current_dir()?.join("dupast.toml");
    if config_path.exists() {
        tracing::info!("Loading config from {}", config_path.display());
        return Config::from_file(config_path);
    }

    Err(DupastError::Internal(
        "No dupast.toml found in current working directory".to_string(),
    ))
}
