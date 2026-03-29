# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-03-29

### Changed
- Disk persistence: index cached to `.hypergrep/index.bin`, loads in ~40ms on subsequent runs
- Progressive indexing: text search no longer triggers tree-sitter parsing
- Lazy tree-sitter: only files matching a structural query get parsed
- Memory: file contents no longer held in RAM, re-read on demand
- Bloom filter: now parses Cargo.toml, package.json, go.mod, requirements.txt for dependency detection
- Zero clippy warnings, cargo fmt clean
- Binary renamed from `hypergrep-cli` to `hypergrep`

### Added
- `--update` instructions in README
- Disk cache invalidation on file mtime/size change
- Corrupt cache recovery (auto-rebuild)
- `.gitignore` auto-updated with `.hypergrep/` on first index save
- 120 tests (62 unit + 58 production/integration)
- GitHub Actions CI (test + clippy + fmt on every push)
- GitHub Actions Release (4 platform binaries on tag)
- Platform installer script (`hypergrep-installer.sh`)
- LICENSE, CONTRIBUTING.md, CHANGELOG.md, SECURITY.md
- Issue templates, PR template

### Fixed
- Bloom filter false negative on "regex" in ripgrep source (now parses Cargo.toml deps)
- Memory usage: removed `contents: Vec<Vec<u8>>` from Index struct

## [0.1.0] - 2026-03-29

### Added
- Trigram-indexed text search with regex support
- Tree-sitter structural search returning full function/class bodies (8 languages)
- Call graph with `--callers`, `--callees`, and `--impact` analysis
- Semantic compression with 3 layers (L0: names, L1: signatures+calls, L2: full code)
- Token budget fitting (`--budget N`)
- JSON output for agent consumption (`--json`)
- Bloom filter for O(1) existence checks (`--exists`)
- Codebase mental model generation (`--model`)
- Disk persistence (`.hypergrep/index.bin`) -- 40ms cached loads
- Progressive indexing: text search skips tree-sitter, structural features parse lazily
- Predictive query prefetch engine
- Filesystem watching daemon (`hypergrep-daemon`)
- 120 tests (62 unit + 58 integration/production)

### Supported languages
- Rust, Python, JavaScript, TypeScript, Go, Java, C, C++

### Performance (208 files, 52K lines)
- Text search: 40ms cached, 4.4ms warm
- Structural search: 5-17ms warm
- Graph queries: 2.5us
- Bloom filter: 291ns
- Token savings: 87% vs ripgrep in agent scenarios
