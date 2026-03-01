//! Similarity detection types and token-based engine

pub mod token_engine;

/// A match between two n-grams
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Match {
    pub gram: u64,
    pub range_a: (usize, usize),
    pub range_b: (usize, usize),
    pub similarity: f64,
}

/// A pair of similar files/regions
#[derive(Debug, Clone)]
pub struct SimilarPair {
    pub file_a: String,
    pub file_b: String,
    pub similarity: f64,
    pub matches: Vec<Match>,
}

/// Internal duplication within a file
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct IntraFileDuplication {
    pub path: std::path::PathBuf,
    pub pairs: Vec<InternalPair>,
}

/// A pair of similar regions within a file
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct InternalPair {
    pub region_a: (usize, usize),
    pub region_b: (usize, usize),
    pub similarity: f64,
    pub matches: Vec<Match>,
}
