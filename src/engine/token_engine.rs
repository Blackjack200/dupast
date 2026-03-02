//! Block-based similarity engine using frequency penalty
//!
//! Detects duplicated code blocks (functions, statements) across files
//! Uses bitmap filtering and O(n²) comparison for fast similarity detection

use crate::config::Config;
use crate::parser::token_freq::{
    BlockTokenizer, TokenizedBlock, block_similarity, block_similarity_fuzzy, blocks_share_tokens,
};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::path::Path;

/// Internal similar block pair representation
#[derive(Debug, Clone)]
pub struct BlockSimilarPair {
    pub file_a: String,
    pub line_start_a: usize,
    pub line_end_a: usize,
    pub file_b: String,
    pub line_start_b: usize,
    pub line_end_b: usize,
    pub similarity: f64,
}

/// Block-based similarity engine
pub struct TokenEngine {
    config: Config,
    tokenizer: BlockTokenizer,
}

impl TokenEngine {
    pub fn new(config: Config) -> Self {
        let tokenizer = BlockTokenizer::new(&config);
        Self { config, tokenizer }
    }

    /// Run block-based similarity detection
    /// Note: Returns Vec directly since this function never fails
    /// Error handling during extraction is done via logging, not Result
    pub fn run(&self, files: &[std::path::PathBuf]) -> Vec<BlockSimilarPair> {
        if files.is_empty() {
            return Vec::new();
        }

        tracing::info!("Extracting code blocks from {} files", files.len());

        let extract_progress = ProgressBar::new(files.len() as u64);
        extract_progress.set_style(
            ProgressStyle::with_template("{prefix:>12.bold} [{pos:>4}/{len:4}] {msg}").unwrap(),
        );
        extract_progress.set_prefix("Extracting");
        extract_progress.set_message("token blocks");

        // Extract blocks from all files
        let extract_progress_for_threads = extract_progress.clone();
        let blocks: Vec<TokenizedBlock> = files
            .par_iter()
            .flat_map(|path| {
                let result = match self.extract_blocks_from_file(path) {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::warn!("Failed to extract blocks from {}: {}", path.display(), e);
                        Vec::new()
                    }
                };
                extract_progress_for_threads.inc(1);
                result
            })
            .collect();
        extract_progress.finish_with_message("block extraction complete");

        tracing::info!("Extracted {} code blocks", blocks.len());

        if blocks.is_empty() {
            return Vec::new();
        }

        // Find similar block pairs (cross-file only)
        self.find_similar_blocks(&blocks)
    }

    /// Extract blocks from a single file
    fn extract_blocks_from_file(&self, path: &Path) -> Result<Vec<TokenizedBlock>, String> {
        let source_code =
            std::fs::read_to_string(path).map_err(|e| format!("Failed to read file: {e}"))?;

        let blocks = self
            .tokenizer
            .extract_blocks(&path.to_string_lossy(), &source_code);

        Ok(blocks)
    }

    /// Find similar block pairs across different files using O(n²) comparison
    fn find_similar_blocks(&self, blocks: &[TokenizedBlock]) -> Vec<BlockSimilarPair> {
        let threshold = self.config.get_threshold();
        let mut pairs = Vec::new();

        tracing::info!("Comparing {} blocks for similarity", blocks.len());

        tracing::info!("Comparing blocks using O(n²) scan");

        let compare_progress = ProgressBar::new(blocks.len() as u64);
        compare_progress.set_style(
            ProgressStyle::with_template("{prefix:>12.bold} [{pos:>4}/{len:4}] {msg}").unwrap(),
        );
        compare_progress.set_prefix("Comparing");
        compare_progress.set_message("candidate blocks");

        // Compare each block with compatible candidates from index
        let compare_progress_for_threads = compare_progress.clone();
        let comparisons: Vec<Vec<BlockSimilarPair>> = blocks
            .par_iter()
            .enumerate()
            .map(|(i, block_a)| {
                let mut local_pairs = Vec::new();

                // Simple O(n) scan: compare with all blocks from different files
                // For N~300, this is ~90K comparisons which is fast enough
                for (j, block_b) in blocks.iter().enumerate() {
                    // Skip same block
                    if i == j {
                        continue;
                    }

                    // Skip same file (we only want cross-file comparisons)
                    if block_a.file_path == block_b.file_path {
                        continue;
                    }

                    // Skip if both blocks are too small
                    let min_size = block_a.total_tokens.min(block_b.total_tokens);
                    if min_size < self.config.min_block_lines {
                        continue;
                    }

                    // SIZE-BASED FILTERING: Skip if size difference > 50%
                    // Use f64 for ratio calculation; precision loss is acceptable for this comparison
                    #[allow(clippy::cast_precision_loss)] // usize > f64 mantissa is acceptable here
                    let size_ratio = block_a.total_tokens as f64 / block_b.total_tokens as f64;
                    if !(0.5..=2.0).contains(&size_ratio) {
                        continue;
                    }

                    // SIMHASH FILTER: O(1) similarity estimation before expensive operations
                    // Skip if SimHash indicates blocks are too different (max 10 differing bits)
                    if !block_a.simhash.is_similar(block_b.simhash, 10) {
                        continue;
                    }

                    // Quick similarity check from SimHash - skip if estimated similarity is too low
                    let estimated_sim = block_a.simhash.estimated_similarity(block_b.simhash);
                    if estimated_sim < threshold * 0.3 {
                        continue;
                    }

                    // TOKEN INTERSECTION FILTER: Quick check for any shared tokens
                    if !blocks_share_tokens(block_a, block_b) {
                        continue;
                    }

                    // TWO-PASS APPROACH: First exact match (fast), then fuzzy (slow)
                    let sim = if self.config.fuzzy_identifiers {
                        // Step 1: Fast exact matching
                        let exact_sim = block_similarity(
                            block_a,
                            block_b,
                            self.config.frequency_penalty.as_f64(),
                        );

                        // Step 2: Only do expensive fuzzy matching if exact is moderate
                        // (suggests structural similarity but with renamed identifiers)
                        if exact_sim >= threshold * 0.5 && exact_sim < threshold {
                            if let Some(synonym_graph) = self.tokenizer.get_synonym_graph() {
                                block_similarity_fuzzy(
                                    block_a,
                                    block_b,
                                    self.config.frequency_penalty.as_f64(),
                                    synonym_graph,
                                    self.config.fuzzy_identifier_threshold,
                                )
                            } else {
                                exact_sim
                            }
                        } else {
                            exact_sim
                        }
                    } else {
                        block_similarity(block_a, block_b, self.config.frequency_penalty.as_f64())
                    };

                    // Log similarity for debugging (temporarily disabled)
                    /*
                    if i == 0 {
                        if sim >= threshold {
                            tracing::debug!("  Block 0 vs {} ({:?}): SIMILAR! sim={:.2}",
                                j, block_b.file_path, sim);
                        } else {
                            tracing::debug!("  Block 0 vs {} ({:?}): sim={:.2} < {:.2}",
                                j, block_b.file_path, sim, threshold);
                        }
                    }
                    */

                    if sim >= threshold {
                        local_pairs.push(BlockSimilarPair {
                            file_a: block_a.file_path.clone(),
                            line_start_a: block_a.start_line,
                            line_end_a: block_a.end_line,
                            file_b: block_b.file_path.clone(),
                            line_start_b: block_b.start_line,
                            line_end_b: block_b.end_line,
                            similarity: sim,
                        });
                    }
                }

                compare_progress_for_threads.inc(1);
                local_pairs
            })
            .collect();
        compare_progress.finish_with_message("block comparison complete");

        for mut local_pairs in comparisons {
            pairs.append(&mut local_pairs);
        }

        // Sort by similarity (descending)
        pairs.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Remove duplicates (exact and inverse pairs)
        // Use HashSet with &str keys - we clone file paths only for deduplication key
        let mut seen_file_pairs = std::collections::HashSet::new();
        let mut unique_pairs = Vec::new();

        for pair in pairs {
            // Clone only for the dedup key (these are small strings compared to full pairs)
            let key1 = (pair.file_a.clone(), pair.file_b.clone());
            let key2 = (pair.file_b.clone(), pair.file_a.clone());

            if seen_file_pairs.contains(&key1) || seen_file_pairs.contains(&key2) {
                continue; // Already seen this pair or its inverse
            }

            seen_file_pairs.insert(key1);
            unique_pairs.push(pair);
        }

        unique_pairs
    }

    /// Get pairs as engine `SimilarPair` format
    pub fn to_engine_pairs(pairs: Vec<BlockSimilarPair>) -> Vec<crate::engine::SimilarPair> {
        pairs
            .into_iter()
            .map(|p| crate::engine::SimilarPair {
                file_a: p.file_a.clone(),
                file_b: p.file_b.clone(),
                similarity: p.similarity,
                matches: vec![crate::engine::Match {
                    gram: 0,
                    range_a: (p.line_start_a, p.line_end_a),
                    range_b: (p.line_start_b, p.line_end_b),
                    similarity: p.similarity,
                }],
            })
            .collect()
    }
}
