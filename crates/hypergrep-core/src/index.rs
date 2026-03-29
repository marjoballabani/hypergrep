/// The core index: trigram posting lists + on-demand structural analysis.
///
/// Production design:
///   1. `build()` checks for cached index on disk, loads if valid (~5ms)
///   2. If no cache or stale, builds trigram index from scratch (~70ms for 200 files)
///   3. Saves to `.hypergrep/index.bin` for next run
///   4. Tree-sitter parsing happens LAZILY -- only for files that match a structural query
///   5. Full structural pass (`complete_index()`) available for graph queries
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use anyhow::Result;
use tracing::info;

use crate::bloom::BloomFilter;
use crate::graph::CodeGraph;
use crate::mental_model::MentalModel;
use crate::persist;
use crate::posting;
use crate::structure::{self, Lang, Symbol, SymbolKind};
use crate::trigram::{self, Trigram};
use crate::walker;

/// Metadata for an indexed file.
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub mtime: SystemTime,
    pub size: u64,
}

/// The in-memory index.
pub struct Index {
    /// doc_id -> file metadata
    pub files: Vec<FileEntry>,
    /// trigram -> sorted list of doc_ids
    pub posting_lists: HashMap<Trigram, Vec<u32>>,
    /// doc_id -> parsed symbols (lazily populated)
    pub symbols: Vec<Vec<Symbol>>,
    /// Tracks which files have been parsed by tree-sitter
    parsed: Vec<bool>,
    /// Code graph (built on demand via complete_index)
    pub graph: CodeGraph,
    /// Bloom filter for O(1) existence queries
    pub bloom: BloomFilter,
    /// Codebase mental model (built on demand)
    pub mental_model: MentalModel,
    /// Root directory
    pub root: PathBuf,
    /// Whether full structural pass has been completed
    pub structural_ready: bool,
}

/// A structural search match.
#[derive(Debug, Clone)]
pub struct StructuralMatch {
    pub path: PathBuf,
    pub symbol_name: String,
    pub symbol_kind: SymbolKind,
    pub line_range: (usize, usize),
    pub body: String,
    pub match_line_number: usize,
    pub match_line: String,
}

/// A single search match.
#[derive(Debug, Clone)]
pub struct SearchMatch {
    pub path: PathBuf,
    pub line_number: usize,
    pub line: String,
    pub match_start: usize,
    pub match_end: usize,
}

impl Index {
    /// Build index with disk caching. Fastest path:
    ///   - If `.hypergrep/index.bin` exists and is valid: load from disk (~5ms)
    ///   - Otherwise: build from scratch, save to disk for next time
    pub fn build(root: &Path) -> Result<Self> {
        let start = Instant::now();
        let root = std::fs::canonicalize(root)?;

        // Try loading cached index
        if let Some((files, posting_lists, bloom, stale)) = persist::load(&root) {
            let file_count = files.len();
            let trigram_count = posting_lists.len();
            let stale_count = stale.len();

            let mut index = Index {
                symbols: vec![Vec::new(); files.len()],
                parsed: vec![false; files.len()],
                files,
                posting_lists,
                graph: CodeGraph::build(&[]),
                bloom,
                mental_model: MentalModel::default(),
                root: root.clone(),
                structural_ready: false,
            };

            if stale_count == 0 {
                info!(
                    "Loaded cached index: {} files, {} trigrams in {:?}",
                    file_count,
                    trigram_count,
                    start.elapsed()
                );
            } else {
                // Incremental update: only re-index stale files
                for doc_id in &stale {
                    let path = index.files[*doc_id as usize].path.clone();
                    let _ = index.update_file(&path, &root);
                }

                // Check for new files not in the cache
                let indexed_paths: std::collections::HashSet<_> =
                    index.files.iter().map(|f| f.path.clone()).collect();
                let current_files = walker::walk_and_read(&root).unwrap_or_default();
                for file in &current_files {
                    if !indexed_paths.contains(&file.path) {
                        let _ = index.update_file(&file.path, &root);
                    }
                }

                // Save updated index
                let _ = persist::save(&root, &index.files, &index.posting_lists, &index.bloom);

                info!(
                    "Incremental update: {} stale of {} files in {:?}",
                    stale_count,
                    file_count,
                    start.elapsed()
                );
            }

            return Ok(index);
        }

        // No cache at all -- build from scratch
        let index = Self::build_fresh(&root)?;

        if let Err(e) = persist::save(&root, &index.files, &index.posting_lists, &index.bloom) {
            info!("Failed to save index cache: {}", e);
        }

        Ok(index)
    }

    /// Build from scratch (no cache). Used internally and for testing.
    pub fn build_fresh(root: &Path) -> Result<Self> {
        let start = Instant::now();
        let root = std::fs::canonicalize(root)?;

        let files = walker::walk_and_read(&root)?;
        let file_count = files.len();

        info!("Read {} files in {:?}", file_count, start.elapsed());

        let mut entries = Vec::with_capacity(files.len());
        let mut posting_lists: HashMap<Trigram, Vec<u32>> = HashMap::new();
        let mut bloom_input: Vec<(PathBuf, Vec<u8>)> = Vec::with_capacity(files.len());

        for file in &files {
            let metadata = std::fs::metadata(&file.path).ok();

            entries.push(FileEntry {
                path: file.path.clone(),
                mtime: metadata
                    .as_ref()
                    .and_then(|m| m.modified().ok())
                    .unwrap_or(SystemTime::UNIX_EPOCH),
                size: metadata.as_ref().map_or(0, |m| m.len()),
            });

            let trigrams = trigram::extract(&file.content);
            for t in trigrams {
                posting_lists.entry(t).or_default().push(file.doc_id);
            }

            bloom_input.push((file.path.clone(), file.content.clone()));
        }

        for list in posting_lists.values_mut() {
            list.sort_unstable();
            list.dedup();
        }

        let bloom_refs: Vec<_> = bloom_input
            .iter()
            .map(|(p, c)| (p.clone(), c.as_slice()))
            .collect();
        let bloom = crate::bloom::build_concept_filter(&bloom_refs);

        let fc = entries.len();
        let index = Index {
            symbols: vec![Vec::new(); fc],
            parsed: vec![false; fc],
            files: entries,
            posting_lists,
            graph: CodeGraph::build(&[]),
            bloom,
            mental_model: MentalModel::default(),
            root: root.clone(),
            structural_ready: false,
        };

        info!(
            "Index built: {} files, {} trigrams, {} concepts in {:?}",
            fc,
            index.posting_lists.len(),
            index.bloom.len(),
            start.elapsed()
        );

        Ok(index)
    }

    /// Run the full structural pass: parse ALL files with tree-sitter,
    /// build call graph, generate mental model. Required for --callers, --impact, --model.
    pub fn complete_index(&mut self) {
        if self.structural_ready {
            return;
        }

        let start = Instant::now();

        let mut graph_input: Vec<(PathBuf, Vec<u8>)> = Vec::with_capacity(self.files.len());
        let mut mm_files: Vec<(PathBuf, usize)> = Vec::with_capacity(self.files.len());

        for (i, entry) in self.files.iter().enumerate() {
            let content = match std::fs::read(&entry.path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            if !self.parsed[i] {
                let symbols = Lang::from_path(&entry.path)
                    .map(|lang| structure::parse_symbols(&content, lang))
                    .unwrap_or_default();
                self.symbols[i] = symbols;
                self.parsed[i] = true;
            }

            let lines = content.iter().filter(|&&b| b == b'\n').count() + 1;
            mm_files.push((entry.path.clone(), lines));
            graph_input.push((entry.path.clone(), content));
        }

        let graph_refs: Vec<_> = graph_input
            .iter()
            .map(|(p, c)| (p.clone(), c.as_slice()))
            .collect();
        self.graph = CodeGraph::build(&graph_refs);

        let mm_symbols: Vec<_> = self
            .files
            .iter()
            .zip(self.symbols.iter())
            .map(|(e, s)| (e.path.clone(), s.clone()))
            .collect();
        self.mental_model =
            crate::mental_model::generate(&mm_files, &mm_symbols, &self.graph, &self.root);

        self.structural_ready = true;

        info!(
            "Structural pass: {} symbols, {} edges in {:?}",
            self.symbol_count(),
            self.graph.edge_count(),
            start.elapsed()
        );
    }

    /// Ensure a specific file has been parsed by tree-sitter (lazy parsing).
    fn ensure_parsed(&mut self, doc_id: usize) {
        if doc_id >= self.files.len() || self.parsed[doc_id] {
            return;
        }

        let content = match std::fs::read(&self.files[doc_id].path) {
            Ok(c) => c,
            Err(_) => return,
        };

        let symbols = Lang::from_path(&self.files[doc_id].path)
            .map(|lang| structure::parse_symbols(&content, lang))
            .unwrap_or_default();
        self.symbols[doc_id] = symbols;
        self.parsed[doc_id] = true;
    }

    fn read_content(&self, doc_id: usize) -> Option<Vec<u8>> {
        if doc_id >= self.files.len() {
            return None;
        }
        std::fs::read(&self.files[doc_id].path).ok()
    }

    /// Text search. Works immediately after build (no tree-sitter needed).
    pub fn search(&self, pattern: &str) -> Result<Vec<SearchMatch>> {
        let compiled = regex::Regex::new(pattern)?;
        let query = trigram::trigrams_from_regex(pattern);

        let empty: Vec<u32> = Vec::new();
        let candidates =
            posting::resolve_query(&query, self.files.len() as u32, &|t: Trigram| -> &[u32] {
                self.posting_lists.get(&t).map_or(&empty, |v| v.as_slice())
            });

        let candidate_ratio = candidates.len() as f64 / self.files.len().max(1) as f64;
        let scan_ids: Vec<u32> = if candidate_ratio > 0.5 {
            (0..self.files.len() as u32).collect()
        } else {
            candidates
        };

        let mut matches = Vec::new();
        for doc_id in scan_ids {
            let idx = doc_id as usize;
            let content = match self.read_content(idx) {
                Some(c) => c,
                None => continue,
            };
            let path = &self.files[idx].path;

            for (line_num, line) in content.split(|&b| b == b'\n').enumerate() {
                let line_str = String::from_utf8_lossy(line).into_owned();
                if let Some(m) = compiled.find(&line_str) {
                    matches.push(SearchMatch {
                        path: path.clone(),
                        line_number: line_num + 1,
                        match_start: m.start(),
                        match_end: m.end(),
                        line: line_str,
                    });
                }
            }
        }

        Ok(matches)
    }

    /// Structural search with LAZY tree-sitter parsing.
    /// Only parses the files that actually match -- not all 208 files.
    pub fn search_structural(&mut self, pattern: &str) -> Result<Vec<StructuralMatch>> {
        let line_matches = self.search(pattern)?;

        // Collect unique doc_ids that matched
        let matched_doc_ids: std::collections::HashSet<usize> = line_matches
            .iter()
            .filter_map(|m| self.files.iter().position(|f| f.path == m.path))
            .collect();

        // Lazily parse only the files that matched
        for &doc_id in &matched_doc_ids {
            self.ensure_parsed(doc_id);
        }

        let mut seen = std::collections::HashSet::new();
        let mut structural_matches = Vec::new();

        for m in &line_matches {
            let doc_id = match self.files.iter().position(|f| f.path == m.path) {
                Some(id) => id,
                None => continue,
            };

            let content = match self.read_content(doc_id) {
                Some(c) => c,
                None => continue,
            };
            let symbols = &self.symbols[doc_id];
            let byte_offset = line_byte_offset(&content, m.line_number);

            if let Some(sym) = structure::enclosing_symbol(symbols, byte_offset) {
                let key = (doc_id, sym.byte_range.0, sym.byte_range.1);
                if seen.contains(&key) {
                    continue;
                }
                seen.insert(key);

                let body_bytes = structure::symbol_text(sym, &content);
                let body = String::from_utf8_lossy(body_bytes).into_owned();

                structural_matches.push(StructuralMatch {
                    path: m.path.clone(),
                    symbol_name: sym.name.clone(),
                    symbol_kind: sym.kind,
                    line_range: sym.line_range,
                    body,
                    match_line_number: m.line_number,
                    match_line: m.line.clone(),
                });
            } else {
                structural_matches.push(StructuralMatch {
                    path: m.path.clone(),
                    symbol_name: "<module>".to_string(),
                    symbol_kind: SymbolKind::Module,
                    line_range: (m.line_number, m.line_number),
                    body: m.line.clone(),
                    match_line_number: m.line_number,
                    match_line: m.line.clone(),
                });
            }
        }

        Ok(structural_matches)
    }

    /// Semantic search with lazy parsing.
    pub fn search_semantic(
        &mut self,
        pattern: &str,
        layer: crate::semantic::Layer,
        budget: Option<usize>,
    ) -> Result<Vec<crate::semantic::SemanticResult>> {
        let line_matches = self.search(pattern)?;

        let matched_doc_ids: std::collections::HashSet<usize> = line_matches
            .iter()
            .filter_map(|m| self.files.iter().position(|f| f.path == m.path))
            .collect();

        for &doc_id in &matched_doc_ids {
            self.ensure_parsed(doc_id);
        }

        let mut seen = std::collections::HashSet::new();
        let mut results = Vec::new();

        for m in &line_matches {
            let doc_id = match self.files.iter().position(|f| f.path == m.path) {
                Some(id) => id,
                None => continue,
            };

            let content = match self.read_content(doc_id) {
                Some(c) => c,
                None => continue,
            };
            let symbols = &self.symbols[doc_id];
            let byte_offset = line_byte_offset(&content, m.line_number);

            if let Some(sym) = structure::enclosing_symbol(symbols, byte_offset) {
                let key = (doc_id, sym.byte_range.0, sym.byte_range.1);
                if seen.contains(&key) {
                    continue;
                }
                seen.insert(key);

                results.push(crate::semantic::compress(
                    sym,
                    &content,
                    &self.files[doc_id].path,
                    layer,
                    &self.graph,
                ));
            }
        }

        if let Some(budget) = budget {
            let (selected, _) = crate::semantic::fit_budget(&results, budget);
            results = selected.into_iter().cloned().collect();
        }

        Ok(results)
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    pub fn trigram_count(&self) -> usize {
        self.posting_lists.len()
    }

    pub fn symbol_count(&self) -> usize {
        self.symbols.iter().map(|s| s.len()).sum()
    }

    /// Number of files that have been tree-sitter parsed.
    pub fn parsed_count(&self) -> usize {
        self.parsed.iter().filter(|&&p| p).count()
    }

    pub fn update_file(&mut self, path: &Path, _root: &Path) -> Result<bool> {
        let existing_doc_id = self
            .files
            .iter()
            .position(|f| f.path == path)
            .map(|i| i as u32);

        let content = match std::fs::read(path) {
            Ok(c) => c,
            Err(_) => {
                if let Some(doc_id) = existing_doc_id {
                    self.remove_doc(doc_id);
                }
                return Ok(existing_doc_id.is_some());
            }
        };

        if trigram::is_binary(&content) {
            if let Some(doc_id) = existing_doc_id {
                self.remove_doc(doc_id);
            }
            return Ok(false);
        }

        let metadata = std::fs::metadata(path).ok();
        let new_trigrams = trigram::extract(&content);

        if let Some(doc_id) = existing_doc_id {
            let old_content = std::fs::read(&self.files[doc_id as usize].path).unwrap_or_default();
            let old_trigrams = trigram::extract(&old_content);
            for t in &old_trigrams {
                if let Some(list) = self.posting_lists.get_mut(t) {
                    list.retain(|&id| id != doc_id);
                }
            }

            self.files[doc_id as usize] = FileEntry {
                path: path.to_path_buf(),
                mtime: metadata
                    .as_ref()
                    .and_then(|m| m.modified().ok())
                    .unwrap_or(SystemTime::UNIX_EPOCH),
                size: metadata.as_ref().map_or(0, |m| m.len()),
            };

            // Mark as unparsed so tree-sitter re-parses on next structural query
            if (doc_id as usize) < self.parsed.len() {
                self.parsed[doc_id as usize] = false;
                self.symbols[doc_id as usize] = Vec::new();
            }

            for t in new_trigrams {
                self.posting_lists.entry(t).or_default().push(doc_id);
                if let Some(list) = self.posting_lists.get_mut(&t) {
                    list.sort_unstable();
                    list.dedup();
                }
            }
        } else {
            let doc_id = self.files.len() as u32;
            self.files.push(FileEntry {
                path: path.to_path_buf(),
                mtime: metadata
                    .as_ref()
                    .and_then(|m| m.modified().ok())
                    .unwrap_or(SystemTime::UNIX_EPOCH),
                size: metadata.as_ref().map_or(0, |m| m.len()),
            });
            self.symbols.push(Vec::new());
            self.parsed.push(false);

            for t in new_trigrams {
                self.posting_lists.entry(t).or_default().push(doc_id);
            }
        }

        Ok(true)
    }

    fn remove_doc(&mut self, doc_id: u32) {
        for list in self.posting_lists.values_mut() {
            list.retain(|&id| id != doc_id);
        }
    }

    /// Save the current index to disk.
    pub fn save(&self) -> Result<()> {
        persist::save(&self.root, &self.files, &self.posting_lists, &self.bloom)
    }
}

fn line_byte_offset(content: &[u8], line_number: usize) -> usize {
    if line_number <= 1 {
        return 0;
    }
    let mut current_line = 1;
    for (i, &byte) in content.iter().enumerate() {
        if byte == b'\n' {
            current_line += 1;
            if current_line == line_number {
                return i + 1;
            }
        }
    }
    content.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("hello.txt"), "hello world\nfoo bar\n").unwrap();
        fs::write(
            dir.path().join("auth.rs"),
            "fn authenticate(user: &str) {\n    println!(\"auth\");\n}\n",
        )
        .unwrap();
        fs::write(dir.path().join("readme.md"), "# My Project\nSome text\n").unwrap();
        dir
    }

    #[test]
    fn test_build_index() {
        let dir = create_test_dir();
        let index = Index::build(dir.path()).unwrap();
        assert_eq!(index.file_count(), 3);
        assert!(index.trigram_count() > 0);
    }

    #[test]
    fn test_search_literal() {
        let dir = create_test_dir();
        let index = Index::build(dir.path()).unwrap();
        let matches = index.search("authenticate").unwrap();
        assert_eq!(matches.len(), 1);
        assert!(matches[0].path.ends_with("auth.rs"));
    }

    #[test]
    fn test_search_regex() {
        let dir = create_test_dir();
        let index = Index::build(dir.path()).unwrap();
        let matches = index.search("hel+o").unwrap();
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_search_no_match() {
        let dir = create_test_dir();
        let index = Index::build(dir.path()).unwrap();
        assert!(index.search("nonexistent_xyz_123").unwrap().is_empty());
    }

    #[test]
    fn test_incremental_update() {
        let dir = create_test_dir();
        let mut index = Index::build(dir.path()).unwrap();
        assert!(index.search("newcontent_xyz").unwrap().is_empty());

        let new_file = dir.path().join("new.txt");
        fs::write(&new_file, "this has newcontent_xyz in it\n").unwrap();
        index.update_file(&new_file, dir.path()).unwrap();

        assert_eq!(index.search("newcontent_xyz").unwrap().len(), 1);
    }

    #[test]
    fn test_disk_persistence() {
        let dir = create_test_dir();

        // Build and save
        let index1 = Index::build(dir.path()).unwrap();
        index1.save().unwrap();

        // Load from cache
        let index2 = Index::build(dir.path()).unwrap();
        assert_eq!(index1.file_count(), index2.file_count());
        assert_eq!(index1.trigram_count(), index2.trigram_count());

        // Search works on loaded index
        let matches = index2.search("authenticate").unwrap();
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_lazy_structural_search() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("auth.rs"),
            "fn authenticate(user: &str, pass: &str) -> bool {\n    check(user)\n}\nfn other() {}\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("unrelated.rs"),
            "fn unrelated() {\n    println!(\"hello\");\n}\n",
        )
        .unwrap();

        let mut index = Index::build(dir.path()).unwrap();
        assert_eq!(index.parsed_count(), 0); // nothing parsed yet

        let matches = index.search_structural("authenticate").unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].symbol_name, "authenticate");

        // Only auth.rs should have been parsed, not unrelated.rs
        assert_eq!(index.parsed_count(), 1);
    }

    #[test]
    fn test_structural_dedup() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("service.py"),
            "def process(data):\n    validate(data)\n    transform(data)\n    save(data)\n    return data\n",
        )
        .unwrap();

        let mut index = Index::build(dir.path()).unwrap();
        let matches = index.search_structural("data").unwrap();
        let process_matches: Vec<_> = matches
            .iter()
            .filter(|m| m.symbol_name == "process")
            .collect();
        assert_eq!(process_matches.len(), 1);
    }

    #[test]
    fn test_symbol_count_after_complete() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("lib.rs"),
            "fn foo() {}\nfn bar() {}\nstruct Baz {}\n",
        )
        .unwrap();

        let mut index = Index::build(dir.path()).unwrap();
        assert_eq!(index.symbol_count(), 0); // lazy, not parsed yet

        index.complete_index();
        assert_eq!(index.symbol_count(), 3);
    }
}
