# Contributing to Hypergrep

## Getting started

```bash
git clone https://github.com/marjoballabani/hypergrep.git
cd hypergrep
cargo build
cargo test
```

## Development workflow

1. Create a branch: `git checkout -b feature/my-feature`
2. Make changes
3. Run tests: `cargo test --all`
4. Run lints: `cargo clippy --all -- -D warnings`
5. Format: `cargo fmt --all`
6. Commit and open a PR

## Project structure

```
hypergrep/
  Cargo.toml                  # Workspace root
  crates/
    hypergrep-core/           # Library: index, search, graph, bloom, etc.
      src/
        index.rs              # Core index: build, search, persist
        trigram.rs             # Trigram extraction + regex decomposition
        posting.rs             # Posting list intersection (galloping)
        structure.rs           # Tree-sitter AST parsing (8 languages)
        graph.rs               # Call graph + impact analysis
        semantic.rs            # Semantic compression (L0/L1/L2)
        bloom.rs               # Bloom filter + concept detection
        mental_model.rs        # Codebase summary generation
        persist.rs             # Disk persistence (.hypergrep/index.bin)
        prefetch.rs            # Predictive query prefetch
        walker.rs              # Directory traversal (.gitignore aware)
      tests/
        production.rs          # Integration + edge case tests
    hypergrep-cli/            # Binary: CLI interface
      src/main.rs
    hypergrep-daemon/         # Binary: persistent daemon + fs watcher
      src/
        main.rs
        watcher.rs
```

## Architecture

```
User query
  |
  v
CLI (clap) --> Index::build() --> check disk cache (.hypergrep/index.bin)
                  |                   |
                  |                   +--> cache hit: load (5ms)
                  |                   +--> cache miss: build trigram index (70ms)
                  v
              Index::search()  --> trigram filter --> regex verify --> matches
                  |
                  +--> search_structural()  --> lazy tree-sitter parse --> function bodies
                  +--> search_semantic()    --> compress to L0/L1/L2 --> budget fit
                  +--> graph.callers_of()   --> reverse call graph lookup
                  +--> graph.impact()       --> BFS with severity classification
                  +--> bloom.might_contain()--> O(1) existence check
```

## Testing

```bash
# All tests
cargo test --all

# Just unit tests
cargo test -p hypergrep-core --lib

# Just integration tests
cargo test -p hypergrep-core --test production

# Specific test
cargo test test_structural_rust_function
```

## Adding a new language

1. Add the tree-sitter grammar crate to `hypergrep-core/Cargo.toml`
2. Add the variant to `Lang` enum in `structure.rs`
3. Add file extension mapping in `Lang::from_path()`
4. Add the tree-sitter language in `Lang::ts_language()`
5. Add symbol node types in `Lang::symbol_node_types()`
6. Add call expression types in `graph.rs::call_expression_types()`
7. Add call name extraction in `graph.rs::extract_call_name()`
8. Add import extraction in `graph.rs::extract_imports()`
9. Add tests for the new language in `structure.rs` and `tests/production.rs`

## Release process

1. Update version in `Cargo.toml` workspace
2. Update BENCHMARKS.md if performance changed
3. Commit: `git commit -m "release: v0.x.0"`
4. Tag: `git tag v0.x.0`
5. Push: `git push origin main --tags`
6. GitHub Actions builds binaries for all platforms and creates a release

## Code style

- `cargo fmt` for formatting
- `cargo clippy` for lints, treat warnings as errors
- No unsafe code without a comment explaining why
- Every public function has a doc comment
- Edge cases get tests in `tests/production.rs`
