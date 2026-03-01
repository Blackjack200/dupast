# dupast

A fast C++ code duplication detector built for copy-pasta detection. Detects copy-pasted code patterns using token-based similarity matching with support for semantic identifier matching via synonym graphs.

## Features

- **🚀 Fast**: Analyzes 150K+ code blocks in seconds with intelligent filtering
- **🎯 Semantic Matching**: Detects renamed identifiers using graph-based synonym matching
  - `calculateSum(a, b)` ≈ `computeTotal(x, y)`
  - Uses 13,000+ word thesaurus for semantic similarity
- **📊 Block-Level Detection**: Extracts individual functions/statements, not just file-level
- **⚡ Multi-Threaded**: Parallel processing with Rayon
- **🎨 Rustc-Style Output**: Clean, colored diagnostics familiar to Rust developers
- **📝 Multiple Formats**: Human, JSON, and SARIF 2.1.0 output

## Installation

```bash
cargo install --path .
```

## Quick Start

```bash
# Basic usage
dupast /path/to/project/src/

# With semantic matching (detects renamed identifiers)
dupast --fuzzy-identifiers -t 40 /path/to/project/src/

# Generate default config
dupast --generate-config > dupast.toml
```

## Usage

```bash
dupast [OPTIONS] [PATHS]...

ARGS:
    <PATHS>...    C++ files/directories to check (default: ".")

OPTIONS:
    -t, --threshold <PERCENT>          Similarity threshold [0-100, default: 90]
    -c, --config <FILE>                Config file path
    -j, --jobs <N>                     Parallel jobs [default: CPU count]
    -o, --output <FORMAT>              Output: human, json, sarif
        --min-lines <N>                Minimum block size [default: 25]
        --frequency-penalty <PENALTY>  Token frequency penalty [0.0-10.0, default: 2.0]
        --fuzzy-identifiers            Enable semantic identifier matching
        --fuzzy-threshold <THRESHOLD>  Fuzzy match threshold [0.0-1.0, default: 0.6]
        --no-intra-file                Disable intra-file detection
        --generate-config              Generate default config file
    -v, --verbose...                   Verbosity level (-v, -vv, -vvv)
    -h, --help                         Print help
```

## Configuration

Create `dupast.toml` in your project root:

```toml
# Similarity threshold (0.0 - 1.0)
threshold = 0.90

# Minimum tokens to consider as duplicate block
min_block_lines = 25

# Frequency penalty for token similarity (0.0 - 10.0)
# Higher values penalize common tokens more heavily
frequency_penalty = 2.0

# Enable fuzzy identifier matching using semantic vectors
fuzzy_identifiers = false

# Minimum similarity threshold for fuzzy identifier matching
fuzzy_identifier_threshold = 0.6

# Ignore paths (glob patterns)
ignore = [
    "vendor/**",
    "build/**",
    "third_party/**",
    "**/generated/*.cpp",
]

# Check intra-file duplication
check_intra_file = true

# Output format: "human", "json", "sarif"
output_format = "human"

# File extensions to scan
extensions = ["cpp", "cc", "cxx", "hpp", "h", "hxx"]

# Maximum file size to process in bytes (default: 1MB)
max_file_size = 1_048_576
```

## Semantic Matching

The tool can detect semantically similar code even when identifiers are renamed:

```cpp
// File A
void calculateSum(int a, int b) {
    return a + b;
}

// File B
int computeTotal(int x, int y) {
    return x + y;
}
```

With `--fuzzy-identifiers`, these are detected as similar (≈85-100%) because:
- `calculate ≈ compute` (synonyms)
- `Sum ≈ Total` (synonyms)

### Custom Synonym Dictionary

Place `synonyms.txt` in your project root with format:
```
word|syn1,syn2,syn3
get|fetch,retrieve,obtain
make|create,build,construct
```

A default 13,000+ word dictionary is included from the Oxford American Writer's Thesaurus.

## Performance

Optimized for large codebases with intelligent filtering:

| Optimization | Impact |
|--------------|--------|
| Size-based filtering | ~65% of comparisons filtered |
| Token intersection check | ~85% of remaining filtered |
| Two-pass similarity (exact → fuzzy) | ~95% fuzzy reduction |
| **Total** | **~300-400x faster** |

**Benchmarks** (6,901 files, 154,877 code blocks):
- Exact matching: ~2-3 seconds
- With fuzzy matching: ~5-10 seconds

## Recommendations

### For Large Projects (100K+ LOC)

```bash
dupast \
  -t 40 \                          # 40% threshold
  --fuzzy-identifiers \            # Enable semantic matching
  --fuzzy-threshold 0.2 \          # Permissive fuzzy threshold
  --min-lines 25 \                 # Ignore small blocks
  -j 0 \                           # Use all CPU cores
  /path/to/project/src/
```

### For CI/CD

```bash
# Use JSON output for parsing
dupast -o json -t 80 src/ > results.json

# Use SARIF for GitHub integration
dupast -o sarif -t 80 src/ > results.sarif
```

## Output Examples

### Human Format

```
warning: code duplication detected (92% similarity)
  --> src/utils.cpp:15:1
   |
15 | / int calculateSum(int a, int b) {
16 | |     return a + b;
17 | | }
   | |_- original definition
...
   = note: similar to src/helpers.cpp:23:1
   = help: consider extracting to shared utility function
```

### JSON Format

```json
{
  "pairs": [
    {
      "file_a": "src/utils.cpp",
      "file_b": "src/helpers.cpp",
      "similarity": 0.92,
      "matches": [
        {
          "range_a": [15, 17],
          "range_b": [23, 25],
          "similarity": 0.92
        }
      ]
    }
  ]
}
```

## How It Works

1. **Parsing**: Uses tree-sitter-cpp for fast syntax tree generation
2. **Tokenization**: Extracts identifiers, filters keywords/operators
3. **Block Extraction**: Identifies individual functions and compound statements
4. **Similarity Calculation**: Frequency-penalized cosine similarity
5. **Fuzzy Matching**: Graph-based BFS for semantic identifier matching
6. **Parallel Comparison**: Multi-threaded cross-file comparison

## Architecture

```
CLI Args → Config → Parser → Tokenizer → Similarity Engine → Output
   ↓         ↓         ↓           ↓              ↓              ↓
 clap    TOML    tree-sitter   Blocks       Parallel       Formatter
                      (C++)    (functions)    (rayon)
```

## License

MIT OR Apache-2.0
