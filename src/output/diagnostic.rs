//! rustc-like diagnostic output formatting

use crate::engine::{IntraFileDuplication, SimilarPair};
use owo_colors::OwoColorize;
use similar::TextDiff;
use std::fmt::Write;

/// Output formatter
pub struct Formatter {
    use_color: bool,
}

impl Formatter {
    /// Create a new formatter
    pub fn new(use_color: bool) -> Self {
        Self { use_color }
    }

    /// Format a single similar pair as a rustc-like warning
    pub fn format_pair(&self, pair: &SimilarPair) -> String {
        let percent = (pair.similarity * 100.0) as u32;
        let mut output = String::new();

        // Main warning line
        writeln!(
            output,
            "{}: code duplication detected ({}% similarity)",
            self.style_warning_label(),
            percent
        )
        .ok();

        // Location in file_a
        if let Some(first_match) = pair.matches.first() {
            writeln!(
                output,
                "   {} {}:{}:1",
                self.style_arrow(),
                pair.file_a,
                first_match.range_a.0
            )
            .ok();
        }

        output.push_str("    |\n");

        // Show rustc-like location and diff snippet
        if let Some(first_match) = pair.matches.first() {
            let source_a =
                get_source_lines(&pair.file_a, first_match.range_a.0, first_match.range_a.1);
            let source_b =
                get_source_lines(&pair.file_b, first_match.range_b.0, first_match.range_b.1);
            let diff_lines = build_unified_diff_lines(
                &pair.file_a,
                first_match.range_a,
                &source_a,
                &pair.file_b,
                first_match.range_b,
                &source_b,
            );
            for line in diff_lines {
                output.push_str("    | ");
                output.push_str(&self.style_diff_line(&line));
                output.push('\n');
            }
        }

        output.push_str("    |\n");

        // Notes
        if let Some(first_match) = pair.matches.first() {
            writeln!(
                output,
                "    = note: similar to {}:{}",
                pair.file_b, first_match.range_b.0
            )
            .ok();
        }

        output.push_str("    = help: consider extracting to shared utility function\n");

        output
    }

    /// Format intra-file duplication
    pub fn format_intra_file(&self, dup: &IntraFileDuplication) -> Vec<String> {
        let mut outputs = Vec::new();

        for pair in &dup.pairs {
            let percent = (pair.similarity * 100.0) as u32;
            let mut output = String::new();

            writeln!(
                output,
                "{}: intra-file duplication detected ({}% similarity)",
                self.style_warning_label(),
                percent
            )
            .ok();

            writeln!(
                output,
                "   {} {}:{}:1",
                self.style_arrow(),
                dup.path.display(),
                pair.region_a.0
            )
            .ok();

            output.push_str("    |\n");

            // Show rustc-like location and diff snippet
            let source_a = get_source_lines(
                &dup.path.to_string_lossy(),
                pair.region_a.0,
                pair.region_a.1,
            );
            let source_b = get_source_lines(
                &dup.path.to_string_lossy(),
                pair.region_b.0,
                pair.region_b.1,
            );
            let diff_lines = build_unified_diff_lines(
                &dup.path.to_string_lossy(),
                pair.region_a,
                &source_a,
                &dup.path.to_string_lossy(),
                pair.region_b,
                &source_b,
            );
            for line in diff_lines {
                output.push_str("    | ");
                output.push_str(&self.style_diff_line(&line));
                output.push('\n');
            }

            output.push_str("    |\n");
            writeln!(
                output,
                "    = note: similar to lines {}-{}",
                pair.region_b.0, pair.region_b.1
            )
            .ok();
            output.push_str("    = help: consider extracting to a separate function\n");

            outputs.push(output);
        }

        outputs
    }

    /// Format a summary
    pub fn format_summary(
        &self,
        pairs: &[SimilarPair],
        intra_file: &[IntraFileDuplication],
    ) -> String {
        let count = pairs.len() + intra_file.len();
        if count == 0 {
            if self.use_color {
                format!("{}\n", "✓ No code duplication found!".green())
            } else {
                "No code duplication found!\n".to_string()
            }
        } else {
            let summary = if self.use_color {
                format!("{}", "Summary:".bold())
            } else {
                "Summary:".to_string()
            };
            format!(
                "\n{} Found {} issue(s)\n  - {} cross-file pair(s)\n  - {} intra-file case(s)\n",
                summary,
                count,
                pairs.len(),
                intra_file.len()
            )
        }
    }

    fn style_warning_label(&self) -> String {
        if self.use_color {
            format!("{}", "warning".yellow().bold())
        } else {
            "warning".to_string()
        }
    }

    fn style_arrow(&self) -> String {
        if self.use_color {
            format!("{}", "-->".bold())
        } else {
            "-->".to_string()
        }
    }

    fn style_diff_line(&self, line: &str) -> String {
        if !self.use_color {
            return line.to_string();
        }

        if line.starts_with("--- ") || line.starts_with("+++ ") {
            format!("{}", line.cyan().bold())
        } else if line.starts_with("@@") {
            format!("{}", line.blue().bold())
        } else if line.starts_with('+') && !line.starts_with("+++") {
            format!("{}", line.green())
        } else if line.starts_with('-') && !line.starts_with("---") {
            format!("{}", line.red())
        } else {
            line.to_string()
        }
    }

    /// Format as JSON
    pub fn format_json(
        &self,
        pairs: &[SimilarPair],
        intra_file: &[IntraFileDuplication],
    ) -> String {
        use serde_json::json;

        let cross_file: Vec<_> = pairs
            .iter()
            .map(|p| {
                let matches: Vec<_> = p
                    .matches
                    .iter()
                    .map(|m| {
                        json!({
                            "file_a_line_start": m.range_a.0,
                            "file_a_line_end": m.range_a.1,
                            "file_b_line_start": m.range_b.0,
                            "file_b_line_end": m.range_b.1,
                        })
                    })
                    .collect();

                json!({
                    "file_a": p.file_a,
                    "file_b": p.file_b,
                    "similarity": p.similarity,
                    "matches": matches,
                })
            })
            .collect();

        let intra: Vec<_> = intra_file
            .iter()
            .map(|d| {
                let pairs: Vec<_> = d
                    .pairs
                    .iter()
                    .map(|p| {
                        json!({
                            "region_a": [p.region_a.0, p.region_a.1],
                            "region_b": [p.region_b.0, p.region_b.1],
                            "similarity": p.similarity,
                        })
                    })
                    .collect();

                json!({
                    "file": d.path.to_string_lossy(),
                    "pairs": pairs,
                })
            })
            .collect();

        json!({
            "cross_file": cross_file,
            "intra_file": intra,
            "total_issues": cross_file.len() + intra.len()
        })
        .to_string()
    }

    /// Format as SARIF (Static Analysis Results Interchange Format)
    pub fn format_sarif(
        &self,
        pairs: &[SimilarPair],
        intra_file: &[IntraFileDuplication],
    ) -> String {
        use serde_json::json;

        let mut results = Vec::new();

        // Cross-file pairs
        for pair in pairs {
            results.push(json!({
                "ruleId": "code-duplication",
                "level": "warning",
                "message": {
                    "text": format!("Code duplication detected ({}% similarity)", (pair.similarity * 100.0) as u32)
                },
                "locations": [
                    {
                        "physicalLocation": {
                            "artifactLocation": {
                                "uri": pair.file_a
                            },
                            "region": {
                                "startLine": pair.matches.first().map_or(1, |m| m.range_a.0)
                            }
                        }
                    },
                    {
                        "physicalLocation": {
                            "artifactLocation": {
                                "uri": pair.file_b
                            },
                            "region": {
                                "startLine": pair.matches.first().map_or(1, |m| m.range_b.0)
                            }
                        }
                    }
                ]
            }));
        }

        // Intra-file duplications
        for dup in intra_file {
            for pair in &dup.pairs {
                results.push(json!({
                    "ruleId": "intra-file-duplication",
                    "level": "warning",
                    "message": {
                        "text": format!("Intra-file duplication detected ({}% similarity)", (pair.similarity * 100.0) as u32)
                    },
                    "locations": [
                        {
                            "physicalLocation": {
                                "artifactLocation": {
                                    "uri": dup.path.to_string_lossy()
                                },
                                "region": {
                                    "startLine": pair.region_a.0,
                                    "endLine": pair.region_a.1
                                }
                            }
                        },
                        {
                            "physicalLocation": {
                                "artifactLocation": {
                                    "uri": dup.path.to_string_lossy()
                                },
                                "region": {
                                    "startLine": pair.region_b.0,
                                    "endLine": pair.region_b.1
                                }
                            }
                        }
                    ]
                }));
            }
        }

        json!({
            "version": "2.1.0",
            "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
            "runs": [{
                "tool": {
                    "driver": {
                        "name": "dupast",
                        "version": "0.1.0",
                        "informationUri": "https://github.com/user/dupast"
                    }
                },
                "results": results
            }]
        })
        .to_string()
    }
}

/// Build a unified-style diff body for two source regions.
fn build_unified_diff_lines(
    file_a: &str,
    range_a: (usize, usize),
    source_a: &str,
    file_b: &str,
    range_b: (usize, usize),
    source_b: &str,
) -> Vec<String> {
    if source_a == "// (source not available)" || source_b == "// (source not available)" {
        return vec!["(source not available for diff)".to_string()];
    }

    let header_a = format!("{}:{}-{}", file_a, range_a.0, range_a.1);
    let header_b = format!("{}:{}-{}", file_b, range_b.0, range_b.1);
    let diff = TextDiff::from_lines(source_a, source_b);
    let unified = diff
        .unified_diff()
        .context_radius(3)
        .header(&header_a, &header_b)
        .to_string();
    let mut out: Vec<String> = unified.lines().map(ToString::to_string).collect();

    const MAX_DIFF_LINES: usize = 120;
    if out.len() > MAX_DIFF_LINES {
        out.truncate(MAX_DIFF_LINES);
        out.push("... (diff truncated)".to_string());
    }

    out
}

/// Get source lines for a file
fn get_source_lines(path: &str, start: usize, end: usize) -> String {
    // Try to read the file
    if let Ok(content) = std::fs::read_to_string(path) {
        let lines: Vec<&str> = content.lines().collect();
        // Ensure indices are valid
        let start_idx = start.saturating_sub(1);
        let end_idx = end.min(lines.len());

        if start_idx < end_idx && end_idx <= lines.len() {
            return lines[start_idx..end_idx].join("\n");
        }
    }
    "// (source not available)".to_string()
}
