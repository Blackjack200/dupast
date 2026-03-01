//! Tree-sitter based tokenizer for source code
//!
//! Extracts and tokenizes individual statements/functions for comparison

use crate::config::Config;
use crate::parser::synonym_graph::SynonymGraph;
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use tree_sitter::Parser;
use tree_sitter_cpp as TSCpp;
use tree_sitter_java as TSJava;
use tree_sitter_rust as TSRust;

const EMBEDDED_SYNONYMS_BIN: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/embedded_synonyms.wincode"));

/// 64-bit SimHash for O(1) Jaccard similarity estimation
/// Uses Locality Sensitive Hashing to estimate document similarity
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct SimHash(u64);

impl SimHash {
    /// Compute SimHash from a collection of tokens
    /// Similar documents will have similar hashes (small Hamming distance)
    pub fn from_tokens(tokens: &[String]) -> Self {
        let mut hash_vector: [i64; 64] = [0; 64];
        let mut hasher = DefaultHasher::new();

        for token in tokens {
            // Hash each token to 64 bits
            token.hash(&mut hasher);
            let hash = hasher.finish();
            hasher = DefaultHasher::new(); // Reset for next token

            // Update vector: +1 for bit=1, -1 for bit=0
            for i in 0..64 {
                if (hash >> i) & 1 == 1 {
                    hash_vector[i as usize] += 1;
                } else {
                    hash_vector[i as usize] -= 1;
                }
            }
        }

        // Final hash: 1 where vector[i] > 0, 0 otherwise
        let mut result = 0u64;
        for i in 0..64 {
            if hash_vector[i as usize] > 0 {
                result |= 1 << i;
            }
        }

        SimHash(result)
    }

    /// Estimate similarity via Hamming distance (0 = identical, 64 = completely different)
    /// Lower Hamming distance = higher similarity
    pub fn hamming_distance(&self, other: SimHash) -> u32 {
        (self.0 ^ other.0).count_ones()
    }

    /// Fast similarity check: true if hashes are similar enough (within threshold)
    pub fn is_similar(&self, other: SimHash, max_distance: u32) -> bool {
        self.hamming_distance(other) <= max_distance
    }

    /// Estimate Jaccard similarity from Hamming distance
    /// Returns 0.0 to 1.0 (higher = more similar)
    pub fn estimated_similarity(&self, other: SimHash) -> f64 {
        let distance = self.hamming_distance(other) as f64;
        // Hamming distance to Jaccard approximation: (64 - distance) / 64
        (64.0 - distance) / 64.0
    }
}

/// 64-bit bitmap signature for fast block similarity pre-filtering
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockSignature(u64);

impl BlockSignature {
    /// Create signature from block characteristics
    pub fn from_block(
        token_count: usize,
        unique_token_count: usize,
        first_token: Option<&str>,
        last_token: Option<&str>,
    ) -> Self {
        let mut sig = 0u64;

        // Token count bucket (5 bits)
        let count_bucket = match token_count {
            0..=10 => 0,
            11..=25 => 1,
            26..=50 => 2,
            51..=100 => 3,
            101..=200 => 4,
            201..=500 => 5,
            _ => 6,
        };
        sig |= (count_bucket as u64) & 0x1F;

        // Unique token ratio bucket (3 bits) - captures code density
        let ratio = if token_count > 0 {
            (unique_token_count as f64 / token_count as f64 * 10.0) as u64
        } else {
            0
        };
        sig |= ratio.min(7) << 5;

        // First character hash (8 bits)
        if let Some(token) = first_token {
            let mut hasher = DefaultHasher::new();
            token.hash(&mut hasher);
            sig |= (hasher.finish() & 0xFF) << 8;
        }

        // Last character hash (8 bits)
        if let Some(token) = last_token {
            let mut hasher = DefaultHasher::new();
            token.hash(&mut hasher);
            sig |= (hasher.finish() & 0xFF) << 16;
        }

        // Token count modulo for additional discrimination (16 bits)
        sig |= ((token_count as u64) & 0xFFFF) << 24;

        // Unique token count modulo for additional discrimination (16 bits)
        sig |= ((unique_token_count as u64) & 0xFFFF) << 40;

        BlockSignature(sig)
    }

    /// Check compatibility with another signature (lower = more compatible)
    pub fn is_compatible(&self, other: BlockSignature, max_diff: u64) -> bool {
        // Compare structural components (count bucket, ratio, first/last hashes)
        let self_struct = self.0 & 0xFFFF;
        let other_struct = other.0 & 0xFFFF;

        // Count differences in structural components
        let diff = self_struct.abs_diff(other_struct);

        diff <= max_diff
    }
}

/// A tokenized code block (function, statement, etc.)
#[derive(Debug, Clone)]
pub struct TokenizedBlock {
    pub file_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub tokens: Vec<String>,
    pub frequencies: FxHashMap<String, f64>,
    pub total_tokens: usize,
    /// Bitmap signature for fast similarity pre-filtering
    pub signature: BlockSignature,
    /// SimHash for O(1) similarity estimation
    pub simhash: SimHash,
}

/// Tree-sitter based tokenizer that extracts code blocks
pub struct BlockTokenizer {
    ignore_set: HashSet<String>,
    min_block_tokens: usize,
    synonym_graph: Option<SynonymGraph>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceLanguage {
    Cpp,
    Rust,
    Java,
}

impl BlockTokenizer {
    pub fn new(config: &Config) -> Self {
        let ignore_set = [
            // Keywords
            "auto",
            "const",
            "constexpr",
            "inline",
            "namespace",
            "using",
            "typedef",
            "public",
            "private",
            "protected",
            "class",
            "struct",
            "enum",
            "union",
            "virtual",
            "override",
            "final",
            "mutable",
            "static",
            "extern",
            "void",
            "bool",
            "char",
            "int",
            "float",
            "double",
            "unsigned",
            "signed",
            "size_t",
            "uint8_t",
            "uint16_t",
            "uint32_t",
            "uint64_t",
            "int8_t",
            "int16_t",
            "int32_t",
            "int64_t",
            "return",
            "if",
            "else",
            "for",
            "while",
            "do",
            "switch",
            "case",
            "default",
            "break",
            "continue",
            "goto",
            "try",
            "catch",
            "throw",
            "noexcept",
            "nullptr",
            "template",
            "typename",
            "concept",
            "requires",
            "true",
            "false",
            // Operators and punctuation
            "{",
            "}",
            "(",
            ")",
            "[",
            "]",
            ";",
            ":",
            ",",
            ".",
            "<",
            ">",
            "+",
            "-",
            "*",
            "/",
            "%",
            "^",
            "&",
            "|",
            "!",
            "~",
            "=",
            "+=",
            "-=",
            "*=",
            "/=",
            "%=",
            "^=",
            "&=",
            "|=",
            "=",
            "==",
            "!=",
            "<=",
            ">=",
            "&&",
            "||",
            "++",
            "--",
            "->",
            ".*",
            "?",
            "::",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        // Load synonym graph if available
        let synonym_graph = Self::load_synonym_graph();

        Self {
            ignore_set,
            min_block_tokens: config.min_block_lines,
            synonym_graph,
        }
    }

    /// Get reference to the synonym graph (for fuzzy matching)
    pub fn get_synonym_graph(&self) -> Option<&SynonymGraph> {
        self.synonym_graph.as_ref()
    }

    fn load_synonym_graph() -> Option<SynonymGraph> {
        match SynonymGraph::from_serialized_bytes(EMBEDDED_SYNONYMS_BIN) {
            Ok(graph) => {
                tracing::info!(
                    "Loaded embedded synonym graph from binary with {} unique words",
                    graph.graph.len()
                );
                Some(graph)
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to load embedded synonym graph binary: {} (exact matching only)",
                    e
                );
                None
            }
        }
    }

    /// Extract and tokenize all code blocks from a source file
    pub fn extract_blocks(&self, file_path: &str, source_code: &str) -> Vec<TokenizedBlock> {
        let Some(language) = detect_language(file_path) else {
            tracing::debug!("Skipping file with unsupported extension: {}", file_path);
            return vec![];
        };

        let mut parser = Parser::new();
        let language_set_result = match language {
            SourceLanguage::Cpp => parser.set_language(&TSCpp::LANGUAGE.into()),
            SourceLanguage::Rust => parser.set_language(&TSRust::LANGUAGE.into()),
            SourceLanguage::Java => parser.set_language(&TSJava::LANGUAGE.into()),
        };
        if let Err(e) = language_set_result {
            tracing::warn!("Failed to set parser language for {}: {}", file_path, e);
            return vec![];
        }

        let tree = match parser.parse(source_code, None) {
            Some(t) => t,
            None => {
                tracing::warn!("Failed to parse file: {}", file_path);
                return vec![];
            }
        };

        let mut blocks = Vec::new();
        let root = tree.root_node();

        // Find function definitions and other top-level declarations
        self.extract_blocks_from_node(&root, source_code, file_path, language, &mut blocks);

        blocks
    }

    /// Recursively extract code blocks from AST nodes
    fn extract_blocks_from_node(
        &self,
        node: &tree_sitter::Node,
        source: &str,
        file_path: &str,
        language: SourceLanguage,
        blocks: &mut Vec<TokenizedBlock>,
    ) {
        let kind = node.kind();

        if is_function_like_node(language, kind) || is_block_node(language, kind) {
            let min_lines = if is_block_node(language, kind) { 5 } else { 0 };
            let line_count = node.end_position().row - node.start_position().row;
            if line_count >= min_lines
                && let Some(block) = self.tokenize_block(node, source, file_path, language)
                && block.total_tokens >= self.min_block_tokens
            {
                blocks.push(block);
            }
        }

        // Recurse into children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract_blocks_from_node(&child, source, file_path, language, blocks);
        }
    }

    /// Tokenize a single AST node into a TokenizedBlock
    fn tokenize_block(
        &self,
        node: &tree_sitter::Node,
        source: &str,
        file_path: &str,
        language: SourceLanguage,
    ) -> Option<TokenizedBlock> {
        let mut tokens = Vec::new();
        let mut frequency: FxHashMap<String, usize> = FxHashMap::default();

        // Extract tokens from this node
        self.extract_tokens_from_node(node, source, language, &mut tokens, &mut frequency);

        if tokens.is_empty() {
            return None;
        }

        let total_tokens = tokens.len();

        // Calculate frequencies
        let total = tokens.len() as f64;
        let frequencies: FxHashMap<String, f64> = frequency
            .into_iter()
            .map(|(token, count)| (token, count as f64 / total))
            .collect();

        // Compute bitmap signature for fast similarity pre-filtering
        let first_token = tokens.first().map(|s| s.as_str());
        let last_token = tokens.last().map(|s| s.as_str());
        let signature =
            BlockSignature::from_block(tokens.len(), frequencies.len(), first_token, last_token);

        // Compute SimHash for O(1) similarity estimation
        let simhash = SimHash::from_tokens(&tokens);

        Some(TokenizedBlock {
            file_path: file_path.to_string(),
            start_line: node.start_position().row + 1,
            end_line: node.end_position().row + 1,
            tokens,
            frequencies,
            total_tokens,
            signature,
            simhash,
        })
    }

    /// Extract tokens from a node recursively
    fn extract_tokens_from_node(
        &self,
        node: &tree_sitter::Node,
        source: &str,
        language: SourceLanguage,
        tokens: &mut Vec<String>,
        frequency: &mut FxHashMap<String, usize>,
    ) {
        let kind = node.kind();

        // Skip certain node types
        if should_skip_node_kind(kind) {
            return;
        }

        // Extract identifier tokens
        if is_identifier_kind(language, kind)
            && let Ok(text) = node.utf8_text(source.as_bytes())
        {
            let trimmed = text.trim();
            if !trimmed.is_empty()
                && !self.ignore_set.contains(trimmed)
                && trimmed.len() >= 2
                && !trimmed.chars().all(|c| c.is_ascii_digit())
            {
                frequency
                    .entry(trimmed.to_string())
                    .and_modify(|c| *c += 1)
                    .or_insert(1);
                tokens.push(trimmed.to_string());
            }
        }

        // Recurse into children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract_tokens_from_node(&child, source, language, tokens, frequency);
        }
    }
}

fn detect_language(file_path: &str) -> Option<SourceLanguage> {
    let ext = std::path::Path::new(file_path)
        .extension()
        .and_then(|s| s.to_str())?
        .to_ascii_lowercase();

    match ext.as_str() {
        "cpp" | "cc" | "cxx" | "hpp" | "h" | "hxx" => Some(SourceLanguage::Cpp),
        "rs" => Some(SourceLanguage::Rust),
        "java" => Some(SourceLanguage::Java),
        _ => None,
    }
}

fn is_function_like_node(language: SourceLanguage, kind: &str) -> bool {
    match language {
        SourceLanguage::Cpp => matches!(kind, "function_definition" | "function_declaration"),
        SourceLanguage::Rust => matches!(kind, "function_item"),
        SourceLanguage::Java => matches!(kind, "method_declaration" | "constructor_declaration"),
    }
}

fn is_block_node(language: SourceLanguage, kind: &str) -> bool {
    match language {
        SourceLanguage::Cpp => kind == "compound_statement",
        SourceLanguage::Rust => kind == "block",
        SourceLanguage::Java => kind == "block",
    }
}

fn is_identifier_kind(_language: SourceLanguage, kind: &str) -> bool {
    matches!(
        kind,
        "identifier" | "type_identifier" | "field_identifier" | "scoped_identifier"
    )
}

/// Check if a node kind should be skipped
fn should_skip_node_kind(kind: &str) -> bool {
    matches!(
        kind,
        "comment"
            | "line_comment"
            | "block_comment"
            | "string_literal"
            | "system_lib_string_literal"
            | "number_literal"
            | "preproc_include"
            | "preproc_def"
            | "preproc_if"
            | "preproc_elif"
            | "preproc_else"
            | "preproc_endif"
            | "preproc_undef"
            | "preproc_params"
    )
}

/// Check if two blocks share any tokens (fast pre-filter)
/// Optimized to use FxHashSet and early exit
pub fn blocks_share_tokens(block_a: &TokenizedBlock, block_b: &TokenizedBlock) -> bool {
    // Early exit if either is empty
    if block_a.tokens.is_empty() || block_b.tokens.is_empty() {
        return false;
    }

    // Use the smaller token set for iteration
    let (smaller, larger) = if block_a.tokens.len() < block_b.tokens.len() {
        (&block_a.tokens, &block_b.tokens)
    } else {
        (&block_b.tokens, &block_a.tokens)
    };

    // Only check first min(20, len) tokens for early exit
    // If they share any tokens in the first 20, they're worth comparing
    let check_limit = smaller.len().min(20);

    // Convert larger to FxHashSet for O(1) lookup
    let larger_set: FxHashSet<_> = larger.iter().collect();

    // Check if any token from smaller exists in larger (early exit)
    for token in smaller.iter().take(check_limit) {
        if larger_set.contains(token) {
            return true;
        }
    }

    false
}

/// Compare two tokenized blocks using frequency-penalized similarity
pub fn block_similarity(block_a: &TokenizedBlock, block_b: &TokenizedBlock, penalty: f64) -> f64 {
    if block_a.tokens.is_empty() || block_b.tokens.is_empty() {
        return 0.0;
    }

    let freq_a = &block_a.frequencies;
    let freq_b = &block_b.frequencies;

    // Collect all unique tokens
    let mut all_tokens: HashSet<String> = HashSet::new();
    for token in &block_a.tokens {
        all_tokens.insert(token.clone());
    }
    for token in &block_b.tokens {
        all_tokens.insert(token.clone());
    }

    if all_tokens.is_empty() {
        return 0.0;
    }

    // Build weighted vectors
    let mut vector_a = Vec::new();
    let mut vector_b = Vec::new();

    for token in &all_tokens {
        let f_a = freq_a.get(token).copied().unwrap_or(0.0);
        let f_b = freq_b.get(token).copied().unwrap_or(0.0);

        let avg_freq = (f_a * f_b).sqrt();
        let weight = 1.0 / (1.0 + penalty * avg_freq);

        vector_a.push(f_a * weight);
        vector_b.push(f_b * weight);
    }

    // Calculate cosine similarity
    let dot_product: f64 = vector_a
        .iter()
        .zip(vector_b.iter())
        .map(|(a, b)| a * b)
        .sum();
    let norm_a: f64 = vector_a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let norm_b: f64 = vector_b.iter().map(|x| x * x).sum::<f64>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot_product / (norm_a * norm_b)
}

/// Compare two tokenized blocks using fuzzy identifier matching with synonym graph
pub fn block_similarity_fuzzy(
    block_a: &TokenizedBlock,
    block_b: &TokenizedBlock,
    penalty: f64,
    synonym_graph: &SynonymGraph,
    fuzzy_threshold: f32,
) -> f64 {
    if block_a.tokens.is_empty() || block_b.tokens.is_empty() {
        return 0.0;
    }

    let freq_a = &block_a.frequencies;
    let freq_b = &block_b.frequencies;

    // Build similarity matrix using fuzzy matching
    let mut matched_a: HashSet<usize> = HashSet::new();
    let mut matched_b: HashSet<usize> = HashSet::new();

    let mut vector_a = Vec::new();
    let mut vector_b = Vec::new();

    // Find fuzzy matches between tokens using synonym graph
    for (i, token_a) in block_a.tokens.iter().enumerate() {
        let mut best_match_idx = None;
        let mut best_match_score = 0.0f32;

        for (j, token_b) in block_b.tokens.iter().enumerate() {
            // Check exact match first
            if token_a == token_b {
                best_match_idx = Some(j);
                break;
            }

            // Fast similarity check (early exit)
            if !synonym_graph.identifier_similarity_fast(token_a, token_b) {
                continue;
            }

            // Then fuzzy match via synonym graph (only if fast check passed)
            let sim = synonym_graph.identifier_similarity(token_a, token_b);
            if sim > best_match_score && sim >= fuzzy_threshold {
                best_match_score = sim;
                best_match_idx = Some(j);
            }
        }

        if let Some(j) = best_match_idx {
            matched_a.insert(i);
            matched_b.insert(j);

            let f_a = freq_a.get(token_a).copied().unwrap_or(0.0);
            let f_b = freq_b.get(&block_b.tokens[j]).copied().unwrap_or(0.0);

            let avg_freq = (f_a * f_b).sqrt();
            let weight = 1.0 / (1.0 + penalty * avg_freq);

            vector_a.push(f_a * weight);
            vector_b.push(f_b * weight);
        }
    }

    // Add unmatched tokens
    for (i, token_a) in block_a.tokens.iter().enumerate() {
        if !matched_a.contains(&i) {
            let f_a = freq_a.get(token_a).copied().unwrap_or(0.0);
            vector_a.push(f_a);
            vector_b.push(0.0);
        }
    }

    for (j, token_b) in block_b.tokens.iter().enumerate() {
        if !matched_b.contains(&j) {
            let f_b = freq_b.get(token_b).copied().unwrap_or(0.0);
            vector_a.push(0.0);
            vector_b.push(f_b);
        }
    }

    // Calculate cosine similarity
    let dot_product: f64 = vector_a
        .iter()
        .zip(vector_b.iter())
        .map(|(a, b)| a * b)
        .sum();
    let norm_a: f64 = vector_a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let norm_b: f64 = vector_b.iter().map(|x| x * x).sum::<f64>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot_product / (norm_a * norm_b)
}
