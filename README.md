# dupast

`dupast` is a fast, tree-sitter based duplicate-code detector for C++, Rust, and Java.

It is built for day-to-day refactoring: find repeated blocks, decide what to extract, and keep noise manageable with tunable thresholds.

## Why this tool

- Block-level matching (not just whole-file heuristics)
- Language-aware parsing via tree-sitter
- Fast enough for local iteration and CI
- Rust-friendly CLI and output (`human`, `json`, `sarif`)

## Install

```bash
cargo install dupast
```

For local development:

```bash
cargo run -- --help
```

## Quick start

```bash
# Scan current project
dupast .

# Scan selected dirs
dupast src/ crates/

# Show more candidates (lower threshold)
dupast -t 80 src/

# Output machine-readable results
dupast -o json src/ > dupast.json
```

## Supported languages

- C++: `cpp`, `cc`, `cxx`, `hpp`, `h`, `hxx`
- Rust: `rs`
- Java: `java`

You can override extensions in `dupast.toml`.

## CLI

```text
dupast [OPTIONS] [PATHS]...
```

Common options:

- `-t, --threshold <PERCENT>`: similarity threshold (`0..=100`)
- `--min-lines <N>`: minimum token count per block
- `--frequency-penalty <PENALTY>`: penalize common tokens (`0.0..=10.0`)
- `--fuzzy-identifiers`: enable semantic-ish identifier matching
- `--fuzzy-threshold <THRESHOLD>`: fuzzy identifier threshold (`0.0..=1.0`)
- `-o, --output <FORMAT>`: `human | json | sarif`
- `-c, --config <FILE>`: explicit config path

See full options with:

```bash
dupast --help
```

## Configuration

Generate a starter config:

```bash
dupast --generate-config
```

Example defaults:

```toml
threshold = 0.90
min_block_lines = 25
check_intra_file = true
output_format = "human"
extensions = ["cpp", "cc", "cxx", "hpp", "h", "hxx", "rs", "java"]
max_file_size = 1_048_576
frequency_penalty = 2.0
fuzzy_identifiers = false
fuzzy_identifier_threshold = 0.6
```

## Tuning tips

- Too many results: increase `-t`, increase `--min-lines`, add `ignore` patterns.
- Too few results: lower `-t`, lower `--min-lines`, consider `--fuzzy-identifiers`.
- Slow runs: disable fuzzy matching first, then tune thresholds.

## How it works

1. Discover files by extension and ignore rules.
2. Parse ASTs with tree-sitter grammars.
3. Extract function/block nodes and tokenize identifiers.
4. Run filtered similarity comparisons.
5. Emit findings in selected output format.

## License

MIT OR Apache-2.0
