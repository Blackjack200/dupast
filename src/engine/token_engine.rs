//! Block-based similarity engine using frequency penalty
//!
//! Detects duplicated code blocks (functions, statements) across files
//! Uses bitmap filtering and O(log n) indexed lookup for fast similarity detection

use crate::config::Config;
use crate::parser::token_freq::{
    BlockSignature, BlockTokenizer, TokenizedBlock, block_similarity, block_similarity_fuzzy,
    blocks_share_tokens,
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

/// Indexed block entry for O(log n) binary search
#[derive(Debug, Clone)]
struct IndexedBlock {
    signature: BlockSignature,
    index: usize, // Index into the blocks array
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
    pub fn run(&self, files: Vec<std::path::PathBuf>) -> Result<Vec<BlockSimilarPair>, String> {
        if files.is_empty() {
            return Ok(Vec::new());
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
            return Ok(Vec::new());
        }

        // Find similar block pairs (cross-file only)
        Ok(self.find_similar_blocks(blocks))
    }

    /// Extract blocks from a single file
    fn extract_blocks_from_file(&self, path: &Path) -> Result<Vec<TokenizedBlock>, String> {
        let source_code =
            std::fs::read_to_string(path).map_err(|e| format!("Failed to read file: {}", e))?;

        let blocks = self
            .tokenizer
            .extract_blocks(&path.to_string_lossy(), &source_code);

        Ok(blocks)
    }

    /// Find similar block pairs across different files using O(log n) indexed lookup
    fn find_similar_blocks(&self, blocks: Vec<TokenizedBlock>) -> Vec<BlockSimilarPair> {
        let threshold = self.config.get_threshold();
        let mut pairs = Vec::new();

        tracing::info!(
            "Building signature index for {} blocks with O(log n) lookup",
            blocks.len()
        );

        // Build sorted signature index for O(log n) binary search
        let mut index: Vec<IndexedBlock> = blocks
            .iter()
            .enumerate()
            .map(|(i, block)| IndexedBlock {
                signature: block.signature,
                index: i,
            })
            .collect();

        // Sort by signature for binary search
        index.sort_by_key(|e| e.signature);

        tracing::info!("Comparing blocks using indexed O(log n) lookup");

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

                // O(log n): Binary search to find compatible candidates
                let candidates = self.find_compatible_candidates(block_a, &blocks, &index, i);

                // Only compare with pre-filtered candidates (much fewer than n)
                for indexed_block in candidates {
                    let block_b_index = indexed_block.index;

                    // Skip same block (already filtered by find_compatible_candidates)
                    if i == block_b_index {
                        continue;
                    }

                    let block_b_data = &blocks[block_b_index];

                    // Skip if both blocks are too small
                    let min_size = block_a.total_tokens.min(block_b_data.total_tokens);
                    if min_size < self.config.min_block_lines {
                        continue;
                    }

                    // SIZE-BASED FILTERING: Skip if size difference > 50%
                    let size_ratio = block_a.total_tokens as f64 / block_b_data.total_tokens as f64;
                    if !(0.5..=2.0).contains(&size_ratio) {
                        continue;
                    }

                    // SIMHASH FILTER: O(1) similarity estimation before expensive operations
                    // Skip if SimHash indicates blocks are too different (max 10 differing bits)
                    if !block_a.simhash.is_similar(block_b_data.simhash, 10) {
                        continue;
                    }

                    // Quick similarity check from SimHash - skip if estimated similarity is too low
                    let estimated_sim = block_a.simhash.estimated_similarity(block_b_data.simhash);
                    if estimated_sim < threshold * 0.3 {
                        continue;
                    }

                    // TOKEN INTERSECTION FILTER: Quick check for any shared tokens
                    if !blocks_share_tokens(block_a, block_b_data) {
                        continue;
                    }

                    // TWO-PASS APPROACH: First exact match (fast), then fuzzy (slow)
                    let sim = if self.config.fuzzy_identifiers {
                        // Step 1: Fast exact matching
                        let exact_sim =
                            block_similarity(block_a, block_b_data, self.config.frequency_penalty);

                        // Step 2: Only do expensive fuzzy matching if exact is moderate
                        // (suggests structural similarity but with renamed identifiers)
                        if exact_sim >= threshold * 0.5 && exact_sim < threshold {
                            if let Some(synonym_graph) = self.tokenizer.get_synonym_graph() {
                                block_similarity_fuzzy(
                                    block_a,
                                    block_b_data,
                                    self.config.frequency_penalty,
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
                        block_similarity(block_a, block_b_data, self.config.frequency_penalty)
                    };

                    if sim >= threshold {
                        local_pairs.push(BlockSimilarPair {
                            file_a: block_a.file_path.clone(),
                            line_start_a: block_a.start_line,
                            line_end_a: block_a.end_line,
                            file_b: block_b_data.file_path.clone(),
                            line_start_b: block_b_data.start_line,
                            line_end_b: block_b_data.end_line,
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

        // Sort by similarity (descending) and deduplicate
        pairs.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Remove duplicates (A-B and B-A)
        pairs.dedup_by(|a, b| {
            a.file_a == b.file_b
                && a.file_b == b.file_a
                && a.line_start_a == b.line_start_b
                && a.line_end_a == b.line_end_b
        });

        pairs
    }

    /// O(log n) binary search to find compatible candidates
    /// Returns IndexedBlock entries whose signatures are compatible with the query block
    fn find_compatible_candidates<'a>(
        &self,
        query: &TokenizedBlock,
        blocks: &'a [TokenizedBlock],
        index: &'a [IndexedBlock],
        query_index: usize,
    ) -> Vec<&'a IndexedBlock> {
        let mut candidates = Vec::new();

        // Binary search for compatible signatures
        // Since BlockSignature is Ord, we can find a range of compatible signatures
        let query_sig = query.signature;

        // Search for lower and upper bounds of compatible signatures
        // We use a tolerance range based on the signature's structural components
        let start = index.partition_point(|e| e.signature < query_sig);
        let end = index.partition_point(|e| {
            e.signature <= query_sig || query_sig.is_compatible(e.signature, 100)
        });

        // Collect candidates from the reduced range (typically much smaller than n)
        for entry in &index[start..end] {
            // Skip self
            if entry.index == query_index {
                continue;
            }

            let candidate = &blocks[entry.index];

            // Skip same file (already handled by outer loop, but double-check for safety)
            if candidate.file_path == query.file_path {
                continue;
            }

            // Verify actual signature compatibility before including
            if query_sig.is_compatible(candidate.signature, 50) {
                candidates.push(entry);
            }
        }

        candidates
    }

    /// Get pairs as engine SimilarPair format
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
