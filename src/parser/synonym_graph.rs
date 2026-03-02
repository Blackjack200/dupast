//! Graph-based semantic vectorizer using synonym dictionary
//!
//! Builds a synonym graph and calculates similarity via graph traversal
//! Uses bitmap filtering and inverted indexing for fast pre-filtering
//! Persists to disk for fast subsequent loads
//! Lock-free design: read-only graph + `DashMap` for zero-cost concurrent access

use dashmap::DashMap;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// 64-bit bitmap signature for fast similarity pre-filtering
/// Each identifier gets a compact signature that captures:
/// - First letter (6 bits)
/// - Length bucket (3 bits: 0-3, 4-6, 7-10, 11-15, 16+)
/// - Word count (3 bits: 0, 1, 2, 3, 4+)
/// - Character class flags (4 bits: `has_digit`, `has_underscore`, `is_uppercase_start`, `is_camel_case`)
/// - Quick hash bits (48 bits from character hash)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
struct BitmapSignature(u64);

impl BitmapSignature {
    const HAS_DIGIT: u64 = 1 << 12;
    const HAS_UNDERSCORE: u64 = 1 << 13;
    const UPPERCASE_START: u64 = 1 << 14;
    const CAMEL_CASE: u64 = 1 << 15;

    #[allow(clippy::cast_sign_loss)]
    fn new(identifier: &str) -> Self {
        let mut sig = 0u64;

        if identifier.is_empty() {
            return BitmapSignature(sig);
        }

        let bytes = identifier.as_bytes();
        let first = bytes[0];

        // First letter bucket (6 bits, fold a-z into 0-25)
        let first_lower = first.to_ascii_lowercase();
        if first_lower.is_ascii_alphabetic() {
            sig |= (u64::from(first_lower) - u64::from(b'a')) & 0x3F;
        }

        // Length bucket (3 bits)
        let len = identifier.len();
        let len_bucket = match len {
            0..=3 => 0,
            4..=6 => 1,
            7..=10 => 2,
            11..=15 => 3,
            _ => 4,
        };
        sig |= (len_bucket as u64) << 6;

        // Word count approximation (3 bits) - count uppercase + underscores
        let word_count = identifier
            .chars()
            .filter(|c| c.is_uppercase() || *c == '_')
            .count()
            .min(7);
        sig |= (word_count as u64) << 9;

        // Character class flags
        if identifier.chars().any(|c| c.is_ascii_digit()) {
            sig |= Self::HAS_DIGIT;
        }
        if identifier.contains('_') {
            sig |= Self::HAS_UNDERSCORE;
        }
        if first.is_ascii_uppercase() {
            sig |= Self::UPPERCASE_START;
        }

        // Detect camelCase: lowercase followed by uppercase (not at start)
        let chars = identifier.chars().peekable();
        let mut prev_is_lower = false;
        for c in chars {
            if prev_is_lower && c.is_uppercase() {
                sig |= Self::CAMEL_CASE;
                break;
            }
            prev_is_lower = c.is_lowercase();
        }

        BitmapSignature(sig)
    }

    /// Fast compatibility check using XOR popcount
    /// Lower score = more similar (0 = identical)
    fn compatibility(self, other: BitmapSignature) -> u32 {
        let diff = self.0 ^ other.0;
        diff.count_ones()
    }

    /// Quick check if signatures are compatible enough for full comparison
    fn is_compatible(self, other: BitmapSignature, threshold: u32) -> bool {
        self.compatibility(other) <= threshold
    }
}

/// Sorted index entry for O(log n) binary search
#[derive(Debug, Clone, Serialize, Deserialize)]
struct IndexEntry {
    identifier: String,
    signature: BitmapSignature,
}

/// Serializable representation of `SynonymGraph` (without `RwLock` caches)
#[derive(Debug, Clone, Serialize, Deserialize, wincode::SchemaWrite, wincode::SchemaRead)]
struct SerializedSynonymGraph {
    graph: FxHashMap<String, Vec<(String, f32)>>,
}

impl From<SerializedSynonymGraph> for SynonymGraph {
    fn from(value: SerializedSynonymGraph) -> Self {
        Self {
            graph: value.graph,
            split_cache: DashMap::new(),
            expand_cache: DashMap::new(),
            signature_cache: DashMap::new(),
            word_index: DashMap::new(),
            signature_index: Mutex::new(Vec::new()),
        }
    }
}

impl From<&SynonymGraph> for SerializedSynonymGraph {
    fn from(graph: &SynonymGraph) -> Self {
        Self {
            graph: graph.graph.clone(),
        }
    }
}

/// Synonym graph built from dictionary
/// Lock-free: uses `DashMap` for concurrent cache access without `RwLock` overhead
#[allow(unused)]
pub struct SynonymGraph {
    /// Adjacency list: word -> [(synonym, weight)]
    /// Read-only after construction, safe for concurrent reads
    pub graph: FxHashMap<String, Vec<(String, f32)>>,
    /// Cache for split identifiers: identifier -> Vec<word>
    /// `DashMap` provides lock-free concurrent access
    split_cache: DashMap<String, Vec<String>>,
    /// Cache for expanded identifiers: word -> expanded vocabulary
    expand_cache: DashMap<String, FxHashMap<String, f32>>,
    /// Bitmap signature cache for fast pre-filtering
    signature_cache: DashMap<String, BitmapSignature>,
    /// Inverted index: word -> Vec<identifiers that contain this word
    /// Used for fast candidate retrieval
    word_index: DashMap<String, Vec<String>>,
    /// Sorted index of identifiers by signature for O(log n) binary search
    /// Mutex protects the rare mutations during build
    signature_index: Mutex<Vec<IndexEntry>>,
}

impl SynonymGraph {
    /// Load from simple format: word|syn1,syn2,syn3
    pub fn from_simple_format(data: &str) -> Self {
        let mut graph: FxHashMap<String, Vec<(String, f32)>> = FxHashMap::default();

        for line in data.lines() {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() != 2 {
                continue;
            }

            let word = parts[0].trim().to_lowercase();
            let synonyms_str = parts[1].trim();

            if word.is_empty() {
                continue;
            }

            // Parse synonyms
            let synonyms: Vec<String> = synonyms_str
                .split(',')
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty() && s != &word)
                .collect();

            if synonyms.is_empty() {
                continue;
            }

            // Add direct synonym relationships (weight = 1.0)
            // Collect pairs first to avoid borrow checker issues
            let mut synonym_pairs: Vec<(String, (String, f32))> = Vec::new();
            for syn in &synonyms {
                synonym_pairs.push((word.clone(), (syn.clone(), 1.0)));
                synonym_pairs.push((syn.clone(), (word.clone(), 1.0)));
            }

            // Add all pairs to graph
            for (key, value) in synonym_pairs {
                graph.entry(key).or_default().push(value);
            }
        }

        Self {
            graph,
            split_cache: DashMap::new(),
            expand_cache: DashMap::new(),
            signature_cache: DashMap::new(),
            word_index: DashMap::new(),
            signature_index: Mutex::new(Vec::new()),
        }
    }

    /// Get the cache file path for the synonym graph
    #[allow(dead_code, clippy::map_unwrap_or)] // Caching infrastructure kept for future use
    fn get_cache_path(source_path: &Path) -> std::path::PathBuf {
        let source_name = source_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("synonyms");

        // Use XDG cache directory or current directory
        let cache_dir = std::env::var("XDG_CACHE_HOME")
            .map(|p| PathBuf::from(p).join("dupast"))
            .unwrap_or_else(|_| {
                let mut path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                path.push(".cache");
                path.push("dupast");
                path
            });

        // Create cache directory if it doesn't exist
        fs::create_dir_all(&cache_dir).ok();

        cache_dir.join(format!("{source_name}.wincode"))
    }

    /// Get file modification time for cache invalidation
    #[allow(dead_code)] // Caching infrastructure kept for future use
    fn get_mtime(path: &Path) -> Option<std::time::SystemTime> {
        fs::metadata(path).ok()?.modified().ok()
    }

    /// Load from cached binary file if available and valid, otherwise build from source
    #[allow(dead_code)] // Caching infrastructure kept for future use
    pub fn load_or_build(source_path: &Path) -> Result<Self, String> {
        let cache_path = Self::get_cache_path(source_path);

        // Check if cache exists and is newer than source
        if cache_path.exists() {
            let source_mtime = Self::get_mtime(source_path);
            let cache_mtime = Self::get_mtime(cache_path.as_path());

            // Cache is valid if it's newer than source file
            if source_mtime.is_none() || (cache_mtime.is_some() && cache_mtime >= source_mtime) {
                tracing::info!("Loading cached synonym graph from: {:?}", cache_path);

                match Self::load_from_cache(cache_path.as_path()) {
                    Ok(graph) => {
                        tracing::info!(
                            "Successfully loaded cached graph with {} words",
                            graph.graph.len()
                        );
                        return Ok(graph);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to load cache (will rebuild): {}", e);
                    }
                }
            } else {
                tracing::info!("Cache is stale, will rebuild graph");
            }
        }

        // Build from source file
        tracing::info!("Building synonym graph from: {:?}", source_path);
        let data = fs::read_to_string(source_path)
            .map_err(|e| format!("Failed to read synonyms file: {e}"))?;

        let graph = Self::from_simple_format(&data);

        // Save to cache for next time
        tracing::info!("Saving graph to cache: {:?}", cache_path);
        if let Err(e) = graph.save_to_cache(cache_path.as_path()) {
            tracing::warn!("Failed to save cache (will rebuild next time): {}", e);
        }

        Ok(graph)
    }

    /// Load graph from cached binary file
    #[allow(dead_code)] // Caching infrastructure kept for future use
    fn load_from_cache(path: &Path) -> Result<Self, String> {
        let bytes = fs::read(path).map_err(|e| format!("Failed to read cache file: {e}"))?;
        Self::from_serialized_bytes(&bytes)
    }

    /// Load graph from serialized wincode bytes.
    pub fn from_serialized_bytes(bytes: &[u8]) -> Result<Self, String> {
        let serialized: SerializedSynonymGraph =
            wincode::deserialize(bytes).map_err(|e| format!("Failed to deserialize cache: {e}"))?;

        Ok(SynonymGraph::from(serialized))
    }

    /// Save graph to cached binary file
    #[allow(dead_code)] // Caching infrastructure kept for future use
    fn save_to_cache(&self, path: &Path) -> Result<(), String> {
        let serialized = SerializedSynonymGraph::from(self);

        let bytes = wincode::serialize(&serialized)
            .map_err(|e| format!("Failed to serialize graph: {e}"))?;

        fs::write(path, bytes).map_err(|e| format!("Failed to write cache file: {e}"))?;

        Ok(())
    }

    /// Calculate identifier similarity by comparing expanded vocabularies
    /// Returns all semantically related words with their similarity scores
    ///
    /// PERFORMANCE: Clones only on cache miss. Cache hits return cloned data
    /// but clone is amortized across all subsequent calls.
    pub fn expand_identifier(&self, identifier: &str) -> FxHashMap<String, f32> {
        // Check cache first (DashMap provides lock-free concurrent access)
        let key = identifier.to_lowercase();
        if let Some(cached) = self.expand_cache.get(&key) {
            // Clone on cache hit (amortized O(1) across all calls)
            return cached.value().clone();
        }

        let mut expanded = FxHashMap::default();

        // Split identifier into words (camelCase, snake_case)
        for word in self.split_identifier(identifier) {
            let word_lower = word.to_lowercase();

            // Add the original word with full weight
            expanded.insert(word.clone(), 1.0);

            // Traverse graph to find related words (with limit for performance)
            let mut visited: HashSet<String> = HashSet::new();
            let mut queue: VecDeque<(String, f32)> = VecDeque::new();

            queue.push_back((word_lower.clone(), 1.0));
            visited.insert(word_lower.clone());
            let mut hops = 0;
            let max_hops = 2; // Limit to 2 hops for performance

            while let Some((current, sim)) = queue.pop_front() {
                if hops >= max_hops {
                    hops += 1;
                    continue;
                }

                if let Some(neighbors) = self.graph.get(&current) {
                    for (neighbor, edge_weight) in neighbors {
                        if !visited.contains(neighbor) && *edge_weight > 0.5 {
                            // Word is a direct synonym, add it
                            let new_sim = sim * edge_weight * 0.9;
                            if new_sim >= 0.4 {
                                // Higher threshold for performance
                                expanded.insert(neighbor.clone(), new_sim);
                                visited.insert(neighbor.clone());
                                // Only continue BFS for high-similarity words
                                if new_sim > 0.7 {
                                    queue.push_back((neighbor.clone(), new_sim));
                                }
                            }
                        }
                    }
                }
                hops += 1;
            }
        }

        // Cache result (DashMap is lock-free for concurrent access)
        // Clone once here for insertion; subsequent reads are also cloned but amortized
        self.expand_cache.insert(key, expanded.clone());

        expanded
    }

    /// Zero-copy variant of `expand_identifier` using callback pattern
    /// Use this for hot paths to avoid any allocation when cache hits
    ///
    /// PERFORMANCE: Zero-copy on cache hit, allocates only on cache miss
    #[allow(dead_code)] // Kept for potential future optimizations
    pub fn with_expanded<F, R>(&self, identifier: &str, f: F) -> R
    where
        F: FnOnce(&FxHashMap<String, f32>) -> R,
    {
        let key = identifier.to_lowercase();
        if let Some(cached) = self.expand_cache.get(&key) {
            return f(cached.value());
        }

        // Compute expanded vocabulary
        let mut expanded = FxHashMap::default();
        for word in self.split_identifier(identifier) {
            let word_lower = word.to_lowercase();
            expanded.insert(word.clone(), 1.0);

            let mut visited: HashSet<String> = HashSet::new();
            let mut queue: VecDeque<(String, f32)> = VecDeque::new();
            queue.push_back((word_lower.clone(), 1.0));
            visited.insert(word_lower.clone());
            let mut hops = 0;
            let max_hops = 2;

            while let Some((current, sim)) = queue.pop_front() {
                if hops >= max_hops {
                    hops += 1;
                    continue;
                }

                if let Some(neighbors) = self.graph.get(&current) {
                    for (neighbor, edge_weight) in neighbors {
                        if !visited.contains(neighbor) && *edge_weight > 0.5 {
                            let new_sim = sim * edge_weight * 0.9;
                            if new_sim >= 0.4 {
                                expanded.insert(neighbor.clone(), new_sim);
                                visited.insert(neighbor.clone());
                                if new_sim > 0.7 {
                                    queue.push_back((neighbor.clone(), new_sim));
                                }
                            }
                        }
                    }
                }
                hops += 1;
            }
        }

        // Cache before invoking callback
        self.expand_cache.insert(key, expanded.clone());

        f(&expanded)
    }

    /// Fast identifier similarity check (for early filtering)
    /// Uses bitmap signature pre-filtering before expensive graph traversal
    /// Returns true if identifiers might be similar, false if definitely not
    pub fn identifier_similarity_fast(&self, id1: &str, id2: &str) -> bool {
        // Exact match
        if id1.eq_ignore_ascii_case(id2) {
            return true;
        }

        // Bitmap signature pre-filter (very fast)
        let sig1 = self.get_signature(id1);
        let sig2 = self.get_signature(id2);

        // If signatures are too different, skip graph lookup entirely
        // Threshold of 8 different bits = very different structure
        if !sig1.is_compatible(sig2, 8) {
            return false;
        }

        // Check if they share any root words
        let words1 = self.split_identifier(id1);
        let words2 = self.split_identifier(id2);

        // Early exit if word counts are very different
        let count_diff = words1.len().abs_diff(words2.len());
        if count_diff > 2 {
            return false;
        }

        for w1 in &words1 {
            for w2 in &words2 {
                if w1 == w2 {
                    return true; // Share at least one root word
                }
                // Check if they're direct synonyms (fast path using graph)
                if let Some(neighbors) = self.graph.get(w1) {
                    for (neighbor, _) in neighbors {
                        if neighbor == w2 {
                            return true;
                        }
                    }
                }
            }
        }

        false
    }

    /// Get or compute bitmap signature for an identifier
    fn get_signature(&self, identifier: &str) -> BitmapSignature {
        let key = identifier.to_lowercase();
        // Check cache first (DashMap provides lock-free concurrent access)
        if let Some(sig_ref) = self.signature_cache.get(&key) {
            return *sig_ref;
        }
        // Compute and cache
        let sig = BitmapSignature::new(identifier);
        self.signature_cache.insert(key, sig);
        sig
    }

    /// Calculate identifier similarity by comparing expanded vocabularies
    /// Uses bitmap pre-filtering to avoid expensive expansion for clearly different identifiers
    pub fn identifier_similarity(&self, id1: &str, id2: &str) -> f32 {
        // Fast bitmap check first - skip expensive work if clearly different
        let sig1 = self.get_signature(id1);
        let sig2 = self.get_signature(id2);

        // If signatures are too different (12+ bit differences), skip entirely
        if !sig1.is_compatible(sig2, 12) {
            return 0.0;
        }

        let exp1 = self.expand_identifier(id1);
        let exp2 = self.expand_identifier(id2);

        if exp1.is_empty() || exp2.is_empty() {
            return 0.0;
        }

        // Collect all unique expanded words
        let mut all_words: HashSet<String> = HashSet::new();
        for word in exp1.keys() {
            all_words.insert(word.clone());
        }
        for word in exp2.keys() {
            all_words.insert(word.clone());
        }

        if all_words.is_empty() {
            return 0.0;
        }

        // Build weighted vectors
        let mut vector_a = Vec::with_capacity(all_words.len());
        let mut vector_b = Vec::with_capacity(all_words.len());

        for word in &all_words {
            let w1 = exp1.get(word).copied().unwrap_or(0.0);
            let w2 = exp2.get(word).copied().unwrap_or(0.0);

            // Average the weights for bi-directional similarity
            vector_a.push(w1);
            vector_b.push(w2);
        }

        // Calculate cosine similarity
        let dot_product: f32 = vector_a
            .iter()
            .zip(vector_b.iter())
            .map(|(a, b)| a * b)
            .sum();

        let norm_a: f32 = vector_a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = vector_b.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            return 0.0;
        }

        dot_product / (norm_a * norm_b)
    }

    /// Build inverted word index from a set of identifiers
    /// This enables O(1) lookup of which identifiers contain a given word
    #[allow(unused)]
    pub fn build_word_index(&self, identifiers: &[String]) {
        self.word_index.clear();

        for identifier in identifiers {
            let words = self.split_identifier(identifier);
            for word in words {
                let word_lower = word.to_lowercase();
                self.word_index
                    .entry(word_lower)
                    .or_default()
                    .push(identifier.clone());
            }
        }
    }

    /// Query the inverted index to find candidate identifiers that share words
    /// Returns a `HashSet` of unique identifiers that share at least one word with the query
    #[allow(unused)]
    pub fn find_candidates_by_words(&self, identifier: &str) -> HashSet<String> {
        let mut candidates = HashSet::new();
        let words = self.split_identifier(identifier);

        for word in words {
            let word_lower = word.to_lowercase();
            if let Some(matching_ids) = self.word_index.get(&word_lower) {
                for id in matching_ids.value() {
                    candidates.insert(id.clone());
                }
            }
        }

        candidates
    }

    /// Build sorted signature index for O(log n) binary search queries
    /// Call this once with all identifiers to enable fast lookups
    #[allow(unused)]
    pub fn build_signature_index(&self, identifiers: &[String]) {
        let mut index = self
            .signature_index
            .lock()
            .expect("signature_index mutex poisoned (thread panicked while holding lock)");
        index.clear();

        // Build entries
        for identifier in identifiers {
            let sig = self.get_signature(identifier);
            index.push(IndexEntry {
                identifier: identifier.clone(),
                signature: sig,
            });
        }

        // Sort by signature for binary search
        index.sort_by_key(|e| e.signature);
    }

    /// O(log n) query to find similar identifiers using binary search
    /// Returns candidates that have compatible signatures (within threshold bits)
    /// This is much faster than checking all pairs when no cache is available
    #[allow(unused)]
    pub fn find_similar_logn(&self, query: &str, threshold_bits: u32) -> Vec<String> {
        let query_sig = self.get_signature(query);
        let index = self
            .signature_index
            .lock()
            .expect("signature_index mutex poisoned (thread panicked while holding lock)");

        // Binary search for compatible signatures
        // Since signatures are u64, we search for a range around query_sig
        let lower = BitmapSignature(query_sig.0.saturating_sub(1 << threshold_bits));
        let upper = BitmapSignature(query_sig.0.saturating_add(1 << threshold_bits));

        let start = index.partition_point(|e| e.signature < lower);
        let end = index.partition_point(|e| e.signature <= upper);

        // Collect candidates from the reduced range
        let mut candidates = Vec::new();
        for entry in &index[start..end] {
            // Verify actual compatibility before including
            if query_sig.is_compatible(entry.signature, threshold_bits) {
                candidates.push(entry.identifier.clone());
            }
        }

        candidates
    }

    /// Split identifier into words (handles camelCase, `snake_case`)
    fn split_identifier(&self, identifier: &str) -> Vec<String> {
        // Check cache first (DashMap provides lock-free concurrent access)
        let key = identifier.to_lowercase();
        if let Some(cached) = self.split_cache.get(&key) {
            return cached.clone();
        }

        // Compute split
        let mut words = Vec::new();
        let mut current_word = String::new();
        let chars = identifier.chars().peekable();

        for c in chars {
            if c.is_uppercase() {
                if !current_word.is_empty() {
                    words.push(current_word.clone());
                }
                current_word = c.to_lowercase().to_string();
            } else if c == '_' {
                if !current_word.is_empty() {
                    words.push(current_word.clone());
                }
                current_word = String::new();
            } else {
                current_word.push(c);
            }
        }

        if !current_word.is_empty() {
            words.push(current_word);
        }

        // Cache result (DashMap is lock-free for concurrent access)
        self.split_cache.insert(key.clone(), words.clone());

        words
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identifier_similarity() {
        let data = "get|fetch,retrieve,obtain\n\
                    make|create,build,construct\n\
                    sum|total,aggregate";

        let graph = SynonymGraph::from_simple_format(data);

        // calculateSum ≈ computeTotal (semantic match)
        let sim = graph.identifier_similarity("calculateSum", "obtainTotal");
        assert!(sim > 0.0); // Should have some similarity
    }

    #[test]
    fn test_identifier_splitting() {
        let data = "get|fetch,retrieve";
        let graph = SynonymGraph::from_simple_format(data);

        assert_eq!(
            graph.split_identifier("calculateSumValue"),
            vec!["calculate", "sum", "value"]
        );
        assert_eq!(
            graph.split_identifier("get_user_data"),
            vec!["get", "user", "data"]
        );
    }

    #[test]
    fn test_fast_similarity_check() {
        let data = "get|fetch,retrieve";
        let graph = SynonymGraph::from_simple_format(data);

        // Exact match should pass fast check
        assert!(graph.identifier_similarity_fast("getItem", "getItem"));

        // Same root word should pass fast check
        assert!(graph.identifier_similarity_fast("getItem", "fetchItem"));

        // No relation should fail fast check
        assert!(!graph.identifier_similarity_fast("getItem", "destroyElement"));
    }

    #[test]
    fn test_bitmap_signature() {
        // Test that similar identifiers have compatible signatures
        let sig1 = BitmapSignature::new("calculateSum");
        let sig2 = BitmapSignature::new("computeTotal");

        // Similar identifiers should have compatible signatures (low XOR popcount)
        let compatibility = sig1.compatibility(sig2);
        assert!(
            compatibility < 16,
            "Similar identifiers should have compatible signatures"
        );

        // Very different identifiers should have higher XOR popcount
        let sig3 = BitmapSignature::new("x");
        let sig4 = BitmapSignature::new("getVeryLongUserNameWithNumbers123");
        assert!(sig3.compatibility(sig4) > compatibility);
    }

    #[test]
    fn test_bitmap_filtering_rejection() {
        let data = "get|fetch,retrieve\nmake|create,build";
        let graph = SynonymGraph::from_simple_format(data);

        // Bitmap filter should reject clearly different pairs
        // Single char vs long identifier with very different structure
        let sig_short = graph.get_signature("x");
        let sig_long = graph.get_signature("getVeryLongUserName");
        assert!(
            !sig_short.is_compatible(sig_long, 4),
            "Clearly different identifiers should be rejected by bitmap filter"
        );
    }

    #[test]
    fn test_inverted_word_index() {
        let data = "get|fetch,retrieve\nset|update,modify";
        let graph = SynonymGraph::from_simple_format(data);

        let identifiers = vec![
            "getItem".to_string(),
            "fetchItem".to_string(),
            "setValue".to_string(),
            "updateValue".to_string(),
        ];

        graph.build_word_index(&identifiers);

        // Find candidates that share words with "getItem"
        let candidates = graph.find_candidates_by_words("getItem");

        // Should contain getItem itself and fetchItem (shares "item")
        assert!(candidates.contains("getItem"));
        assert!(candidates.contains("fetchItem"));

        // Should NOT contain setValue or updateValue (no shared words)
        assert!(!candidates.contains("setValue"));
    }

    #[test]
    fn test_logn_query() {
        let data = "get|fetch,retrieve\nset|update,modify\nsum|total,aggregate";
        let graph = SynonymGraph::from_simple_format(data);

        // Build a larger index to test O(log n) query
        let identifiers: Vec<String> = [
            "getItem",
            "fetchItem",
            "retrieveItem",
            "setValue",
            "updateValue",
            "modifyValue",
            "calculateSum",
            "computeTotal",
            "aggregateAmount",
            "x",
            "y",
            "z", // Very different ones
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        graph.build_signature_index(&identifiers);

        // Query for similar identifiers to "calculateSum"
        let candidates = graph.find_similar_logn("calculateSum", 12);

        // Should find semantically similar identifiers
        assert!(
            candidates.contains(&"calculateSum".to_string()),
            "Should find itself"
        );

        // Should NOT find very different identifiers like "x", "y", "z"
        assert!(
            !candidates.contains(&"x".to_string()),
            "Should not find very different identifiers"
        );

        // Query for "setValue" should find similar ones
        let candidates2 = graph.find_similar_logn("setValue", 12);
        assert!(candidates2.contains(&"setValue".to_string()));
        assert!(
            candidates2.contains(&"updateValue".to_string())
                || candidates2.contains(&"modifyValue".to_string()),
            "Should find semantically related identifiers"
        );
    }

    #[test]
    fn test_cache_roundtrip() {
        let data = "get|fetch,retrieve\nset|update,modify\nsum|total,aggregate";
        let original = SynonymGraph::from_simple_format(data);

        // Test serialization and deserialization
        let serialized: SerializedSynonymGraph = (&original).into();
        let bytes = wincode::serialize(&serialized).expect("Failed to serialize");
        assert!(bytes.len() > 0, "Serialized data should not be empty");

        let deserialized: SerializedSynonymGraph =
            wincode::deserialize(&bytes).expect("Failed to deserialize");
        let restored = SynonymGraph::from(deserialized);

        // Verify the restored graph has the same data
        assert_eq!(restored.graph.len(), original.graph.len());

        // Test that similarity calculations work the same
        let sim_original = original.identifier_similarity("calculateSum", "obtainTotal");
        let sim_restored = restored.identifier_similarity("calculateSum", "obtainTotal");
        assert_eq!(
            sim_original, sim_restored,
            "Similarity should match after roundtrip"
        );
    }
}
