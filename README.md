# Hypergrep

[![CI](https://github.com/marjoballabani/hypergrep/actions/workflows/ci.yml/badge.svg)](https://github.com/marjoballabani/hypergrep/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-120%20passing-brightgreen.svg)]()

**A codebase intelligence engine for AI coding agents.**

AI agents waste 60-80% of their tokens on navigation -- grep returns raw lines, the agent reads files to understand context, repeats 50+ times per session. Hypergrep returns structural answers: function bodies, call graphs, impact analysis, and codebase summaries in 87% fewer tokens.

### Key numbers (measured, not projected)

| Metric | ripgrep | Hypergrep | |
|--------|---------|-----------|--|
| Warm search latency | 31ms | **4.4ms** | 7x faster |
| 50-query session | 1,550ms | **220ms** | 7x faster |
| Tokens per 3-query task | 20,580 | **2,814** | 87% less |
| "Who calls this?" | impossible | **2.5us** | new capability |
| "Does this use Redis?" | 31ms (full scan) | **291ns** | 100,000x faster |
| Codebase summary | N/A | **699 tokens** | loaded once |

> Benchmarked on ripgrep's own source (208 files, 52K lines). See [BENCHMARKS.md](BENCHMARKS.md) for full methodology.

### Why not just ripgrep?

ripgrep is the best text search tool. Use it for one-off greps. But AI agents don't do one-off greps -- they do 50-200 searches per session, and every result is raw text that needs follow-up file reads to understand.

Hypergrep answers the questions agents actually ask:

| Agent needs | ripgrep gives | Hypergrep gives |
|-------------|---------------|-----------------|
| "Find the auth handler" | 47 matching lines | The function body + signature + call graph |
| "What calls this?" | nothing | `--callers`: reverse call graph in 2.5us |
| "What breaks if I change this?" | nothing | `--impact`: blast radius with severity |
| "Does this project use Redis?" | Full scan, 0 results | `--exists`: YES/NO in 291ns |
| "How is this codebase structured?" | nothing | `--model`: structural summary in 699 tokens |
| "Give me the best results in 500 tokens" | not possible | `--budget 500`: budget-fitted results |

### Status

**v0.1.0** -- Production-ready for small/medium codebases (<1K files). 120 tests. 8 languages. Disk-cached index. Zero false negatives guaranteed.

| Component | Status |
|-----------|--------|
| Text search (trigram index) | Stable |
| Structural search (tree-sitter, 8 langs) | Stable |
| Call graph + impact analysis | Stable |
| Semantic compression (L0/L1/L2 + budget) | Stable |
| Bloom filter (existence checks) | Stable |
| Mental model (codebase summary) | Stable |
| Disk persistence (.hypergrep/index.bin) | Stable |
| Daemon mode (persistent index + fs watcher) | Beta |
| Predictive query prefetch | Experimental |

## Install

### Pre-built binary (fastest)

macOS and Linux -- downloads the right binary for your platform:

```bash
curl -sSfL https://github.com/marjoballabani/hypergrep/releases/latest/download/hypergrep-installer.sh | sh
```

Or download manually from the [Releases page](https://github.com/marjoballabani/hypergrep/releases).

| Platform | Binary |
|----------|--------|
| macOS Apple Silicon (M1/M2/M3/M4) | `hypergrep-aarch64-apple-darwin.tar.gz` |
| macOS Intel | `hypergrep-x86_64-apple-darwin.tar.gz` |
| Linux x86_64 | `hypergrep-x86_64-unknown-linux-gnu.tar.gz` |
| Linux ARM64 | `hypergrep-aarch64-unknown-linux-gnu.tar.gz` |

### From source

```bash
git clone https://github.com/marjoballabani/hypergrep.git
cd hypergrep
./install.sh
```

Or manually:

```bash
cargo build --release
cp target/release/hypergrep ~/.cargo/bin/   # or /usr/local/bin/
```

Requires Rust 1.75+ and a C compiler (for tree-sitter grammars).

### Verify

```bash
hypergrep --version
hypergrep --help
```

## Quick start

```bash
# Search (ripgrep-compatible)
hypergrep "authenticate" src/

# Structural search (return full function bodies)
hypergrep -s "authenticate" src/

# Semantic compression (signatures + call graph, 500 token budget)
hypergrep --layer 1 --budget 500 "authenticate" src/

# JSON output for agent consumption
hypergrep --layer 1 --json "authenticate" src/

# Impact analysis (what breaks if this changes?)
hypergrep --impact "authenticate" src/

# Codebase mental model (load once, skip orientation)
hypergrep --model "" src/

# Existence check (O(1) bloom filter)
hypergrep --exists "redis" src/
```

## Search modes

### Text search (default)

Ripgrep-compatible output. Builds a trigram index internally for fast repeated searches.

```
hypergrep "pattern" dir
hypergrep -c "pattern" dir            # count only
hypergrep -l "pattern" dir            # file names only
```

### Structural search (`-s`)

Returns complete enclosing functions/classes instead of raw lines. If a pattern matches 5 lines inside one function, the function is returned once (deduplicated).

```
hypergrep -s "authenticate" src/
```

Output:
```
src/auth.rs:1-8 function authenticate
fn authenticate(user: &str, pass: &str) -> bool {
    let hashed = hash_password(pass);
    check_db(user, hashed)
}
---
```

### Semantic compression (`--layer`)

Three levels of detail, each using fewer tokens:

| Layer | Content | Tokens/result |
|-------|---------|---------------|
| `--layer 0` | File path + symbol name + kind | ~15 |
| `--layer 1` | Signature + calls + called_by | ~80-120 |
| `--layer 2` | Full source code of enclosing function | ~200-800 |

```bash
# Layer 1: signatures + call graph context
hypergrep --layer 1 "search" src/
```

Output:
```
src/index.rs:function search (~65 tokens)
  sig: pub fn search(&self, pattern: &str) -> Result<Vec<SearchMatch>>
  calls: trigrams_from_regex, resolve_query
  called_by: search_structural, search_semantic, test_search_literal
```

### Token budget (`--budget`)

Tell Hypergrep how many tokens you can afford. It selects the best results that fit.

```bash
# Best results in 500 tokens
hypergrep --layer 1 --budget 500 "authenticate" src/
```

### JSON output (`--json`)

For programmatic agent consumption. Works with `--layer`, `--model`, and `--exists`.

```bash
hypergrep --layer 1 --json "authenticate" src/
```

```json
[
  {
    "file": "src/auth.rs",
    "name": "authenticate",
    "kind": "function",
    "line_range": [1, 8],
    "signature": "fn authenticate(user: &str, pass: &str) -> bool",
    "calls": ["hash_password", "check_db"],
    "called_by": ["login_handler", "api_key_verify"],
    "tokens": 85
  }
]
```

## Graph queries

### Callers (`--callers`)

Reverse call graph: who calls this symbol?

```bash
hypergrep --callers "authenticate" src/
```

### Callees (`--callees`)

Forward call graph: what does this symbol call?

```bash
hypergrep --callees "authenticate" src/
```

### Impact analysis (`--impact`)

What breaks if you change this symbol? BFS upstream through the call graph with severity classification:

```bash
hypergrep --impact "hash_password" src/
```

Output:
```
Impact analysis for 'hash_password' (depth 3):

  [depth 1] WILL BREAK   src/auth.rs:authenticate
  [depth 2] MAY BREAK    src/api.rs:login_handler
  [depth 3] REVIEW        src/main.rs:setup_routes
```

Severity levels:
- **WILL BREAK** (depth 1) -- direct callers
- **MAY BREAK** (depth 2) -- callers of callers
- **REVIEW** (depth 3+) -- transitive dependents

## Codebase intelligence

### Mental model (`--model`)

A compressed structural summary (~300-500 tokens) of the entire codebase. Load this once at agent session start to skip 80% of exploratory searches.

```bash
hypergrep --model "" src/
```

Output:
```
# Codebase Mental Model

## Languages
- Rust: 14 files
- TypeScript: 8 files

## Structure
- src/auth/ (3 files) -- 5 functions, 2 structs
- src/api/ (6 files) -- 12 functions, 3 structs
- src/db/ (4 files) -- 8 functions, 1 struct

## Key Abstractions
- function authenticate (src/auth/handler.rs) -- 8 callers, 3 callees
- struct UserService (src/auth/service.rs) -- 5 callers, 4 callees

## Entry Points
- src/main.rs

## Hot Spots (most complex)
- src/api/handlers.rs (15 symbols, 340 lines)
- src/auth/handler.rs (8 symbols, 180 lines)
```

### Existence check (`--exists`)

Does this codebase use a specific technology? Answered in microseconds via bloom filter.

```bash
hypergrep --exists "redis" src/        # YES or NO
hypergrep --exists "graphql" src/
hypergrep --exists "kubernetes" src/
```

- **NO** = definitely not present (zero false negatives, guaranteed)
- **YES** = likely present (~1% false positive rate)

### Stats (`--stats`)

```bash
hypergrep --stats "" src/
```

```
Files indexed: 17
Unique trigrams: 8113
Symbols parsed: 214
Graph edges: 305
Bloom filter: 173 concepts, 11984 bytes
Mental model: 474 tokens
Index build time: 94ms
```

## Supported languages

Tree-sitter grammars for structural parsing and call graph extraction:

| Language | Structural search | Call graph | Import tracking |
|----------|------------------|------------|-----------------|
| Rust | Functions, structs, enums, traits, impls, modules | Yes | Yes |
| Python | Functions, classes | Yes | Yes |
| JavaScript | Functions, classes, methods, arrow functions | Yes | Yes |
| TypeScript | Functions, classes, methods, arrow functions | Yes | Yes |
| Go | Functions, methods, type declarations | Yes | Yes |
| Java | Methods, classes, interfaces, enums | Yes | Partial |
| C | Functions, structs, enums | Yes | No |
| C++ | Functions, classes, structs, enums | Yes | No |

Unsupported languages fall back to line-level text search (same as ripgrep).

## Daemon mode

For persistent indexing across multiple queries (used by agent integrations):

```bash
# Start daemon
hypergrep-daemon /path/to/project

# Queries go through Unix socket -- sub-millisecond after first index build
# Index updates incrementally via filesystem watching
```

## Architecture

```
Agent (Claude Code, Cursor, etc.)
  |
  v
Hypergrep Daemon
  |
  +-- Query Router (text / structural / graph / existence)
  +-- Prefetch Engine (predict next 3-5 queries, cache speculatively)
  +-- Result Compiler (layer selection, budget fitting, dedup)
  |
  +-- Unified Index
  |     +-- Text Index (trigram posting lists, galloping intersection)
  |     +-- Code Graph (call/import/type edges, BFS impact analysis)
  |     +-- AST Cache (tree-sitter symbol boundaries per file)
  |     +-- Bloom Filter (concept vocabulary, ~12KB)
  |     +-- Mental Model (derived structural summary)
  |
  +-- Index Manager (fs watcher, incremental re-index, git state tracking)
```

## How it works

1. **Index build** (~100ms for medium codebases): Walk directory, extract trigrams from every file, parse ASTs with tree-sitter, build call graph from call expressions, populate bloom filter from imports/patterns.

2. **Text search**: Decompose regex into required trigrams. Intersect posting lists (galloping merge). Run regex verification only on candidate files. Zero false negatives guaranteed.

3. **Structural search**: After text match, look up the enclosing AST node (function, class, method). Return the complete symbol body. Deduplicate: multiple matches in one symbol return it once.

4. **Graph queries**: BFS traversal of the call graph. Callers = reverse edges. Impact = multi-depth BFS with severity classification.

5. **Semantic compression**: Convert symbols to compact JSON representations. Layer 0 = name. Layer 1 = signature + call graph. Layer 2 = full code. Budget fitting = greedy selection of top results within token limit.

## Performance

| Scenario | Latency | Notes |
|----------|---------|-------|
| Cold start (no cache) | ~800ms | Builds trigram index + saves to disk |
| Cached start | **40ms** | Loads from `.hypergrep/index.bin` |
| Warm text search | **3-7ms** | Daemon mode, index in memory |
| Warm structural search | **5-17ms** | Lazy tree-sitter, parses only matched files |
| Graph queries | **2-7us** | In-memory adjacency list traversal |
| Bloom filter | **291ns** | Single hash lookup |
| 50-query agent session | **220ms** | 4.4ms/query average |

Tested on 208 files / 52K lines (ripgrep source). See [BENCHMARKS.md](BENCHMARKS.md) for full numbers with methodology.

## Limitations

- **Cold start is slower than ripgrep** (800ms vs 31ms). The index pays for itself after ~40 queries. Use the daemon for agent workloads.
- **Call graph is static analysis only.** Dynamic dispatch, reflection, callbacks, and macros are not resolved. Impact results may be incomplete.
- **Bloom filter has ~2% false positives.** "YES" means "probably" -- confirm with a real search. "NO" is always correct.
- **Large codebases (>10K files)** need daemon mode. CLI cold start is too slow.
- **Memory**: ~17 MB for text index, ~54 MB with full structural pass (208 files). Scales linearly.

## Research

See [RESEARCH.md](RESEARCH.md) for the full theoretical foundations, prior art analysis (42 references), and quantitative projections behind Hypergrep.

## License

[MIT](LICENSE)

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, project structure, and how to add new languages.
