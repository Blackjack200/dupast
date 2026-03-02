# CLAUDE.md

Guidance for Claude Code working on this repository.

## Project Overview

dupast is a fast multi-language code duplication detector with Cargo-like ergonomics. Supports C, C++, Go, Java, JavaScript, PHP, Rust, and TypeScript. Detects `calculateSum(a, b)` ≈ `computeTotal(x, y)` using a 13,000+ word thesaurus.

**Cargo-like features**: Smart project discovery, environment variables, shell completion, verbosity levels, color control.

## Build and Run

```bash
cargo build --release
./target/release/dupast [OPTIONS] [PATHS...]

# Common usage
./target/release/dupast src/
./target/release/dupast -t 80 --fuzzy-identifiers src/
DUPAST_THRESHOLD=75 ./target/release/dupast src/
./target/release/dupast --completion bash > completions/dupast
```

## Architecture

Pipeline: `CLI Args → Config → Project Discovery → Parser → Tokenizer → Similarity Engine → Output`

- **main.rs**: Entry point, logging setup, shell completion, project discovery, env var application
- **cli/**: clap derive API, short options (`-C`, `-F`, `-L`, `-q`, `-v`), color control, threshold divided by 100
- **config/**: Type-safe config with newtypes, smart discovery, env var support, glob ignore patterns
- **parser/**: Multi-language tree-sitter, lazy-loaded synonym graph, tokenization, similarity calculation
- **engine/**: Parallel processing (rayon), three-stage filtering, progress bars
- **output/**: Cargo-like grouped output, severity levels, JSON/SARIF formats
- **error.rs**: Helpful error messages with suggestions

## Key Design Decisions

### Type-Safe Configuration

`Threshold` and `FrequencyPenalty` are newtypes that enforce valid ranges at construction time ("Parse, Don't Validate"). Invalid states are unrepresentable.

```rust
pub struct Threshold(f64);  // Always 0.0-1.0
pub struct FrequencyPenalty(f64);  // Always 0.0-10.0
```

### Lazy Loading

Synonym graph (27KB) is ONLY loaded when `fuzzy_identifiers=true`. Default mode: fast startup, minimal memory.

### Three-Stage Filtering

Reduces fuzzy matching by ~300-400x:
1. Size filtering (~65% filtered)
2. Token intersection (~85% of remaining)
3. Two-pass similarity (~95% reduction in fuzzy calls)

### Project Discovery

Scans upward for `dupast.toml` or project markers (`.git`, `Cargo.toml`, `package.json`, etc.). Falls back to XDG config: `~/.config/dupast/config.toml`.

## Configuration Precedence

CLI args > ENV vars > Config file > Defaults

## Environment Variables

All options support `DUPAST_*` prefix: `DUPAST_THRESHOLD`, `DUPAST_FUZZY_IDENTIFIERS`, `DUPAST_JOBS`, `DUPAST_OUTPUT_FORMAT`, `DUPAST_MIN_LINES`, `DUPAST_FREQUENCY_PENALTY`, `DUPAST_QUIET`, `DUPAST_COLOR`.

## Two-Threshold System

1. `-t/--threshold`: Block similarity (0-100%, default 90)
2. `--fuzzy-threshold`: Identifier similarity (0.0-1.0, default 0.6)

## Supported Languages

C, C++, Go, Java, JavaScript, PHP, Rust, TypeScript (via tree-sitter)

## Exit Codes

0 = no issues, 1 = issues found

## Development Notes

- Maintain Cargo-like UX (grouped output, progress bars, helpful errors)
- Respect precedence (CLI > ENV > Config > Defaults)
- Keep lazy loading (synonym graph only when needed)
- Validate config with helpful error messages
- Use `par_iter()` for expensive operations
- Test with `-vv` for debugging

## Performance

Small (<10K LOC): <1s, Medium (10K-100K LOC): 1-5s, Large (100K+ LOC): 5-10s with fuzzy matching

## Dependencies

tree-sitter, rayon, clap (derive), serde/toml, rustc-hash, indicatif, similar, owo-colors, dirs, thiserror, tracing
