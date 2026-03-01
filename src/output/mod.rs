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
    let formatter = Formatter::new(true);
    let mut stdout = io::stdout().lock();

    match config.output_format.as_str() {
        "json" => {
            writeln!(stdout, "{}", formatter.format_json(pairs, intra_file))?;
        }
        "sarif" => {
            writeln!(stdout, "{}", formatter.format_sarif(pairs, intra_file))?;
        }
        _ => {
            // Human-readable output
            for pair in pairs {
                writeln!(stdout, "{}", formatter.format_pair(pair))?;
                writeln!(stdout)?;
            }

            for dup in intra_file {
                for output in formatter.format_intra_file(dup) {
                    writeln!(stdout, "{}", output)?;
                    writeln!(stdout)?;
                }
            }

            write!(stdout, "{}", formatter.format_summary(pairs, intra_file))?;
        }
    }

    Ok(())
}
