# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

dupast is a fast C++ code duplication detector inspired by Rust's copy-pasta detection. It detects copy-pasted code using token-based similarity matching with support for semantic identifier detection via graph-based synonym matching. Key innovation: detects `calculateSum(a, b)` as similar to `computeTotal(x, y)` using a 13,000+ word thesaurus.

## Build and Run Commands

```bash
# Build the project (debug)
cargo build

# Build release (optimized, with LTO)
cargo build --release

# Run directly with cargo
cargo run -- --help

# Run the built binary
./target/release/dupast [OPTIONS] [PATHS...]

# Run with custom config
./target/release/dupast -c dupast.toml /path/to/cpp/src/

# Run with verbose output (-v, -vv, -vvv)
./target/release/dupast -vv /path/to/cpp/src/

# Generate default config file
./target/release/dupast --generate-config

# Run tests
cargo test

# Check for compilation errors without building
cargo check

# Format code
cargo fmt
```

## Architecture

The codebase follows a pipeline architecture with clear separation of concerns:

### Pipeline Flow

```
CLI Args → Config → Parser → Tokenizer → Similarity Engine → Output
   ↓         ↓         ↓           ↓              ↓              ↓
 clap    TOML    tree-sitter   Blocks        Parallel       Formatter
                      (C++)      (functions)    (rayon)      (JSON/SARIF)
                                                  ↓
                                          Synonym Graph
                                          (semantic match)
```

### Module Responsibilities

**`src/main.rs`**: Entry point, sets up logging with tracing-subscriber, orchestrates the pipeline

**`src/cli/`**: Command-line argument parsing using clap derive API
- Defines `Args` struct with all CLI options
- Threshold from CLI is divided by 100 (takes 0-100, stores as 0.0-1.0)

**`src/config/`**: Configuration management
- Loads from `dupast.toml` (searches current dir, then parent dirs)
- CLI arguments override config file values
- Contains glob pattern matching for ignore rules
- Key settings: `threshold`, `min_block_lines`, `frequency_penalty`, `fuzzy_identifiers`

**`src/parser/`**: C++ parsing and tokenization
- `mod.rs`: `Parser` struct for file discovery and orchestration using walkdir
- `token_freq.rs`: Core tokenization and similarity logic
  - `BlockTokenizer`: Extracts code blocks (functions, compound statements)
  - `TokenizedBlock`: Stores tokens, frequencies, line ranges
  - `block_similarity()`: Frequency-penalized cosine similarity
  - `block_similarity_fuzzy()`: Semantic matching via synonym graph
  - `blocks_share_tokens()`: Fast pre-filter for token intersection
- `synonym_graph.rs`: Graph-based semantic identifier matching
  - `SynonymGraph`: Adjacency list with weighted edges, BFS traversal
  - `identifier_similarity()`: Cosine similarity on expanded vocabularies
  - `identifier_similarity_fast()`: Quick check for root word sharing or direct synonyms
  - Caching: `split_cache` for identifier splitting, `expand_cache` for synonym expansion

**`src/engine/`**: Similarity detection engine
- `mod.rs`: Exports `SimilarPair` for output formatting
- `token_engine.rs`: `TokenEngine::run()` - parallel processing pipeline
  - Extracts blocks from all files in parallel (rayon)
  - `find_similar_blocks()`: Cross-file comparison with three-stage filtering:
    1. Size-based filtering (skip if size difference > 50%)
    2. Token intersection check (skip if no shared tokens)
    3. Two-pass similarity: exact match (fast) → fuzzy match (slow, only if exact is moderate)
  - Only does expensive fuzzy matching on ~0.26% of comparisons

**`src/output/`**: Result formatting
- `mod.rs`: Output dispatch based on format (human, json, sarif)
- `diagnostic.rs`: rustc-style warnings, JSON, SARIF 2.1.0 with annotate-snippets

**`src/error.rs`**: Error types using thiserror

## Key Algorithms

### Tokenization

Extracts identifiers from C++ code using tree-sitter-cpp:
- Keywords and operators are filtered out (large ignore set in `BlockTokenizer::new()`)
- Only meaningful identifiers remain (function names, variable names, etc.)
- Each code block (function, compound statement) gets a list of tokens

### Frequency-Penalized Cosine Similarity

Main similarity metric in `block_similarity()`:
```rust
weight = 1.0 / (1.0 + penalty * avg_freq)
```
- Common tokens (high frequency) get lower weight
- Rare tokens get higher weight
- Helps distinguish structural similarity from common boilerplate

### Semantic Identifier Matching

Graph-based approach in `synonym_graph.rs`:
1. **Identifier splitting**: `calculateSum` → `["calculate", "sum"]`
2. **Graph expansion**: Each word expands to synonyms via BFS (2 hops max, 0.9 decay per hop)
3. **Cosine similarity**: Compare expanded vocabularies

Example: `calculateSum` vs `computeTotal`
- `calculate` expands to `[calculate(1.0), create(0.9), make(0.81), ...]`
- `Sum` expands to `[sum(1.0), total(0.9), aggregate(0.81), ...]`
- Overlap in expanded vocabularies → semantic similarity detected

### Performance Optimizations

Three-stage filtering reduces fuzzy matching by ~300-400x:
1. **Size filtering**: Skip if block sizes differ by >50% (~65% filtered)
2. **Token intersection**: Skip if no shared tokens (~85% of remaining)
3. **Two-pass similarity**:
   - Fast exact match first
   - Expensive fuzzy only if exact similarity is moderate (20-50% of threshold)
   - ~95% reduction in fuzzy matching calls

### Caching

`SynonymGraph` uses `RwLock<FxHashMap>` for thread-safe caching:
- `split_cache`: Identifier → words (camelCase/snake_case parsing)
- `expand_cache`: Word → expanded vocabulary with similarity weights

## Important Implementation Details

### Two-Threshold System

1. **`-t` / `--threshold`**: Overall block similarity threshold (0-100%)
   - Default: 90% (from config)
   - Report pairs ≥ this threshold

2. **`--fuzzy-threshold`**: Identifier similarity threshold (0.0-1.0)
   - Default: 0.6
   - Only used when `--fuzzy-identifiers` is enabled
   - Filters which identifier pairs undergo expensive expansion

### Synonym Dictionary

- Loaded from `synonyms.txt` in working directory (if present)
- Format: `word|syn1,syn2,syn3`
- Default: 13,212 words from Oxford American Writer's Thesaurus
- Custom dictionaries supported by placing `synonyms.txt` in project root

### Parser: tree-sitter-cpp Only

Uses tree-sitter-cpp (not clangd, not AST normalization):
- Fast, no preprocessing required
- Handles modern C++ (C++11-C++20)
- Limitations: Complex macros may cause parse warnings (still processes)
- Directly extracts identifiers, no normalization to "dry code"

### Parallel Processing

Rayon is used for all expensive operations:
- File parsing: `par_iter()`
- Block extraction: `par_iter()`
- Block comparison: `par_iter()` with `flat_map()`

### Exit Codes

- `0`: No issues found
- `1`: Issues found (for CI/CD integration)

## Configuration Search Path

1. Current directory
2. Parent directories (upwards)
3. Home directory

## Testing Strategy

Unit tests exist for synonym graph:
```bash
cargo test
```

Manual integration testing with real C++ projects:
```bash
# Test with small threshold to see more results
./target/release/dupast -t 30 --fuzzy-identifiers --fuzzy-threshold 0.1 /path/to/project
```

## Dependencies

Key crates:
- `tree-sitter` + `tree-sitter-cpp`: C++ parsing (0.24, 0.23)
- `rayon`: Parallel processing (1.10)
- `rustc-hash`: Fast hashing for token frequencies (2.0)
- `clap`: CLI argument parsing with derive (4.5)
- `serde`/`toml`: Config file handling (1.0, 0.8)
- `tracing`/`tracing-subscriber`: Structured logging (0.1, 0.3)
- `termcolor`: Terminal colors (1.4)
- `annotate-snippets`: Rustc-like diagnostic formatting (0.11)

## Performance Characteristics

- Small projects (<10K LOC): <1 second
- Medium projects (10K-100K LOC): 1-5 seconds
- Large projects (100K+ LOC, 6901 files, 154K blocks): 5-10 seconds with fuzzy matching

## Known Limitations

- Intra-file detection only works if `check_intra_file = true` in config
- False positives on: `= delete`/`= default`, simple getters/setters
- Fuzzy matching is expensive (only used when `--fuzzy-identifiers` is enabled)
- Token-based: misses structural similarities if identifiers are completely different and semantic matching is disabled

## Common Issues

### "No C++ files found"
- Check file extensions (default: cpp, cc, cxx, hpp, h, hxx)
- Check ignore patterns in config
- Use `-vv` to see which files are being skipped

### Parse errors but tool still works
- tree-sitter is tolerant of syntax errors
- Warnings logged but processing continues
- Files with severe errors are skipped with a warning

### Too many false positives
- Increase threshold: `-t 85` (or 90, 95)
- Increase `min_block_lines` in config (default: 25)
- Add patterns to `ignore` list in config
- Reduce `frequency_penalty` to penalize common tokens more

### Too slow with fuzzy matching
- Increase `--fuzzy-threshold` (0.4-0.6) to reduce fuzzy match attempts
- Increase `-t` to reduce overall comparisons
- Increase `--min-lines` to ignore small blocks
- Disable fuzzy matching entirely (remove `--fuzzy-identifiers`)
