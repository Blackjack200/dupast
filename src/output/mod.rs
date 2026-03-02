//! Output formatting module

pub mod diagnostic;

use crate::Config;
use crate::engine::{IntraFileDuplication, SimilarPair};
use std::io::{self, Write};

pub use diagnostic::Formatter;

/// Write output to stdout
pub fn write_output(
    config: &Config,
    pairs: &[SimilarPair],
    intra_file: &[IntraFileDuplication],
) -> std::io::Result<()> {
    let formatter = Formatter::new(config.use_color());
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    match config.output_format.as_str() {
        "json" => {
            writeln!(stdout, "{}", formatter.format_json(pairs, intra_file))?;
        }
        "sarif" => {
            writeln!(stdout, "{}", formatter.format_sarif(pairs, intra_file))?;
        }
        _ => {
            // Human-readable output with Cargo-like formatting
            let count = pairs.len() + intra_file.len();

            // Summary header (like Cargo's "Compiling" messages)
            if config.quiet {
                // Quiet mode - only output the pairs
                for pair in pairs {
                    writeln!(stdout, "{}", formatter.format_pair(pair))?;
                    writeln!(stdout)?;
                }

                for dup in intra_file {
                    for output in formatter.format_intra_file(dup) {
                        writeln!(stdout, "{output}")?;
                        writeln!(stdout)?;
                    }
                }
            } else {
                if count > 0 {
                    writeln!(
                        stdout,
                        "    Found {} duplicate pair{}",
                        count,
                        if count == 1 { "" } else { "s" }
                    )?;
                    writeln!(stdout)?;
                }

                // Grouped findings by severity
                write_grouped_findings(pairs, intra_file, &mut stdout, &formatter)?;

                // Final summary (like Cargo's "Finished" message)
                write_finished_message(pairs, intra_file, &mut stdout)?;
            }
        }
    }

    Ok(())
}

/// Write grouped findings by severity level (Cargo-like hierarchy)
fn write_grouped_findings(
    pairs: &[SimilarPair],
    intra_file: &[IntraFileDuplication],
    writer: &mut impl std::io::Write,
    formatter: &Formatter,
) -> std::io::Result<()> {
    use std::collections::HashMap;

    // Group cross-file pairs by severity
    let mut groups: HashMap<&str, Vec<&SimilarPair>> = HashMap::new();
    for pair in pairs {
        let group = if pair.similarity >= 0.95 {
            "critical"
        } else if pair.similarity >= 0.85 {
            "high"
        } else if pair.similarity >= 0.75 {
            "medium"
        } else {
            "low"
        };
        groups.entry(group).or_default().push(pair);
    }

    // Output each group with header
    for (group, label) in [
        ("critical", "Critical (≥95%)"),
        ("high", "High (≥85%)"),
        ("medium", "Medium (≥75%)"),
        ("low", "Low"),
    ] {
        if let Some(pairs) = groups.get(group)
            && !pairs.is_empty()
        {
            writeln!(writer)?;
            writeln!(writer, "   {} ({}):", label, pairs.len())?;
            for pair in pairs {
                writeln!(writer, "{}", formatter.format_pair(pair))?;
                writeln!(writer)?;
            }
        }
    }

    // Intra-file duplications
    for dup in intra_file {
        for output in formatter.format_intra_file(dup) {
            writeln!(writer, "{output}")?;
            writeln!(writer)?;
        }
    }

    Ok(())
}

/// Write the "Finished" message (like Cargo)
fn write_finished_message(
    pairs: &[SimilarPair],
    intra_file: &[IntraFileDuplication],
    writer: &mut impl std::io::Write,
) -> std::io::Result<()> {
    writeln!(writer)?;

    let total_count = pairs.len() + intra_file.len();

    if total_count > 0 {
        // Use HashSet<String> - simpler and safer than dealing with lifetimes
        // The performance impact is minimal for typical file counts (<1000)
        let mut files = std::collections::HashSet::new();

        for pair in pairs {
            files.insert(pair.file_a.clone());
            files.insert(pair.file_b.clone());
        }

        for dup in intra_file {
            files.insert(dup.path.to_string_lossy().to_string());
        }

        writeln!(
            writer,
            "    Found {} pair{} in {} file{}",
            total_count,
            if total_count == 1 { "" } else { "s" },
            files.len(),
            if files.len() == 1 { "" } else { "s" }
        )?;
    } else {
        writeln!(writer, "    No code duplication found ✓")?;
    }

    Ok(())
}
