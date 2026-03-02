# dupast

A fast code duplication detector with Cargo-like ergonomics.

Supports C, C++, Go, Java, JavaScript, PHP, Rust, and TypeScript.

## Installation

```
cargo install dupast
```

## Quick start

```
# Scan current directory
dupast

# Scan specific paths
dupast src/ crates/

# Lower threshold for more candidates
dupast -t 80 src/

# Enable semantic identifier matching
dupast --fuzzy-identifiers src/
```

## Features

- **Multi-language support**: C, C++, Go, Java, JavaScript, PHP, Rust, TypeScript
- **Block-level matching**: Detects duplicate functions and code blocks
- **Semantic matching**: Finds `calculateSum(a, b)` ≈ `computeTotal(x, y)` via synonym graph
- **Cargo-like UX**: Smart project discovery, shell completion, environment variables
- **CI/CD ready**: JSON and SARIF output with proper exit codes

## Usage

```
dupast [OPTIONS] [PATHS]...
```

### Common options

- `-t, --threshold <PERCENT>` - Similarity threshold (0-100, default: 90)
- `-L, --min-lines <N>` - Minimum tokens per block (default: 3)
- `-F, --frequency-penalty <N>` - Token frequency penalty (0.0-10.0, default: 2.0)
- `--fuzzy-identifiers` - Enable semantic identifier matching
- `-o, --output <FORMAT>` - Output format: human, json, sarif (default: human)
- `-j, --jobs <N>` - Parallelism (0 = CPU count)
- `-v, --verbose` - Increase verbosity (-v, -vv, -vvv)
- `-q, --quiet` - Quiet mode (errors only)
- `--color <WHEN>` - Color output: always, never, auto

For full documentation, run `dupast --help`.

## Configuration

Create a `dupast.toml` in your project root:

```toml
threshold = 0.90
paths = ["src"]
min_block_lines = 3
ignore = ["vendor/**", "build/**", "target/**"]
check_intra_file = true
output_format = "human"
extensions = ["rs", "go", "java", "js", "ts"]
frequency_penalty = 2.0
fuzzy_identifiers = false
fuzzy_identifier_threshold = 0.6
```

Generate a default config:

```
dupast --generate-config
```

## Cargo-like features

### Smart project discovery

Like Cargo, `dupast` scans upward for `dupast.toml` or common project markers (`.git`, `Cargo.toml`, `package.json`, etc.). Run from any subdirectory and it will find your config.

### Environment variables

All options support `DUPAST_*` environment variables:

```
export DUPAST_THRESHOLD=75
export DUPAST_FUZZY_IDENTIFIERS=1
export DUPAST_JOBS=8
```

Precedence: CLI args > ENV vars > Config file > Defaults

### Shell completion

```
# Bash
dupast --completion bash > ~/.local/share/bash-completion/completions/dupast

# Zsh
dupast --completion zsh > ~/.zsh/completions/_dupast

# Fish
dupast --completion fish > ~/.config/fish/completions/dupast.fish
```

## Output

### Human output (default)

```
    Found 2 duplicate pairs

   Critical (≥95%) (1):
warning: code duplication detected (100% similarity)
   --> src/foo.rs:10:1
    |
    | --- src/foo.rs:10-25
    | +++ src/bar.rs:15-30
    | @@ -1,10 +1,10 @@
    |  fn calculate_sum(a: i32, b: i32) -> i32 {
    | -    a + b
    | +    x + y
    |  }
    |
    = note: similar to src/bar.rs:15
    = help: consider extracting to shared utility function

    Found 2 pairs in 3 files
```

### JSON / SARIF

```
dupast -o json src/ > results.json
dupast -o sarif src/ > results.sarif
```

## Semantic identifier matching

When `--fuzzy-identifiers` is enabled, `dupast` uses a 13,000+ word thesaurus to detect semantically similar identifiers:

- `calculateSum(a, b)` ≈ `computeTotal(x, y)`
- `getUserInfo()` ≈ `fetchUserData()`
- `createInstance()` ≈ `makeObject()`

The synonym graph (27KB) is only loaded when fuzzy matching is enabled.

## Supported languages

| Language | Extensions |
|----------|------------|
| C | `c`, `h` |
| C++ | `cpp`, `cc`, `cxx`, `hpp`, `hxx` |
| Go | `go` |
| Java | `java` |
| JavaScript | `js`, `mjs`, `cjs` |
| PHP | `php` |
| Rust | `rs` |
| TypeScript | `ts`, `tsx` |

## CI/CD integration

Exit codes: `0` (no issues), `1` (issues found).

Example for GitHub Actions:

```yaml
- name: Install dupast
  run: cargo install dupast

- name: Check for duplication
  run: dupast -o sarif src/ > results.sarif
  continue-on-error: true

- name: Upload SARIF
  uses: github/codeql-action/upload-sarif@v2
  with:
    sarif_file: results.sarif
```

## Performance

- Small projects (<10K LOC): <1s
- Medium projects (10K-100K LOC): 1-5s
- Large projects (100K+ LOC): 5-10s with fuzzy matching

Three-stage filtering reduces fuzzy matching by ~300-400x:
1. Size-based filtering (~65% filtered)
2. Token intersection (~85% of remaining)
3. Two-pass similarity (~95% reduction in fuzzy calls)

## License

MIT OR Apache-2.0
