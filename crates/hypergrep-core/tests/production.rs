/// Production test suite: edge cases, error paths, correctness guarantees.
/// These tests cover the gaps identified in the coverage audit.
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

use hypergrep_core::index::Index;

// ============================================================================
// CORRECTNESS: Text search must match ripgrep exactly
// ============================================================================

fn make_dir(files: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new().unwrap();
    for (name, content) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
    }
    dir
}

#[test]
fn test_empty_directory() {
    let dir = TempDir::new().unwrap();
    let index = Index::build(dir.path()).unwrap();
    assert_eq!(index.file_count(), 0);
    assert!(index.search("anything").unwrap().is_empty());
}

#[test]
fn test_single_file_single_match() {
    let dir = make_dir(&[("a.txt", "hello world\n")]);
    let index = Index::build(dir.path()).unwrap();
    let m = index.search("hello").unwrap();
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].line_number, 1);
}

#[test]
fn test_multiple_matches_same_line() {
    let dir = make_dir(&[("a.txt", "foo foo foo\n")]);
    let index = Index::build(dir.path()).unwrap();
    // regex::find returns first match per line only
    let m = index.search("foo").unwrap();
    assert_eq!(m.len(), 1);
}

#[test]
fn test_match_on_every_line() {
    let dir = make_dir(&[("a.txt", "aaa\naaa\naaa\naaa\naaa\n")]);
    let index = Index::build(dir.path()).unwrap();
    let m = index.search("aaa").unwrap();
    assert_eq!(m.len(), 5);
}

#[test]
fn test_no_match_returns_empty() {
    let dir = make_dir(&[("a.txt", "hello world\n")]);
    let index = Index::build(dir.path()).unwrap();
    assert!(index.search("zzzznothere").unwrap().is_empty());
}

#[test]
fn test_regex_special_characters() {
    let dir = make_dir(&[("a.txt", "price is $100.00\nfoo(bar)\n[bracket]\n")]);
    let index = Index::build(dir.path()).unwrap();
    // Escaped special chars
    assert_eq!(index.search(r"\$100").unwrap().len(), 1);
    assert_eq!(index.search(r"foo\(bar\)").unwrap().len(), 1);
    assert_eq!(index.search(r"\[bracket\]").unwrap().len(), 1);
}

#[test]
fn test_regex_character_classes() {
    let dir = make_dir(&[("a.txt", "abc123\ndef456\nghi789\n")]);
    let index = Index::build(dir.path()).unwrap();
    assert_eq!(index.search("[0-9]{3}").unwrap().len(), 3);
    assert_eq!(index.search("[a-c]+").unwrap().len(), 1);
}

#[test]
fn test_regex_alternation() {
    let dir = make_dir(&[("a.txt", "cat\ndog\nbird\n")]);
    let index = Index::build(dir.path()).unwrap();
    assert_eq!(index.search("cat|dog").unwrap().len(), 2);
    assert_eq!(index.search("cat|dog|bird").unwrap().len(), 3);
}

#[test]
fn test_regex_anchors() {
    let dir = make_dir(&[("a.txt", "hello world\nworld hello\n")]);
    let index = Index::build(dir.path()).unwrap();
    assert_eq!(index.search("^hello").unwrap().len(), 1);
    assert_eq!(index.search("hello$").unwrap().len(), 1);
}

#[test]
fn test_case_sensitive_by_default() {
    let dir = make_dir(&[("a.txt", "Hello\nhello\nHELLO\n")]);
    let index = Index::build(dir.path()).unwrap();
    assert_eq!(index.search("hello").unwrap().len(), 1);
    assert_eq!(index.search("Hello").unwrap().len(), 1);
    assert_eq!(index.search("(?i)hello").unwrap().len(), 3);
}

#[test]
fn test_invalid_regex_returns_error() {
    let dir = make_dir(&[("a.txt", "hello\n")]);
    let index = Index::build(dir.path()).unwrap();
    assert!(index.search("[invalid").is_err());
    assert!(index.search("(unclosed").is_err());
}

#[test]
fn test_empty_pattern() {
    let dir = make_dir(&[("a.txt", "hello\nworld\n")]);
    let index = Index::build(dir.path()).unwrap();
    // Empty regex matches everything
    let m = index.search("").unwrap();
    assert!(m.len() >= 2);
}

// ============================================================================
// UNICODE
// ============================================================================

#[test]
fn test_unicode_content() {
    let dir = make_dir(&[("a.txt", "cafe\ncaf\u{00e9}\n\u{4f60}\u{597d}\n")]);
    let index = Index::build(dir.path()).unwrap();
    assert_eq!(index.search("caf\u{00e9}").unwrap().len(), 1);
    assert_eq!(index.search("\u{4f60}\u{597d}").unwrap().len(), 1);
}

#[test]
fn test_unicode_in_code() {
    let dir = make_dir(&[(
        "a.rs",
        "fn greet() -> &str { \"\u{4f60}\u{597d}\u{4e16}\u{754c}\" }\n",
    )]);
    let index = Index::build(dir.path()).unwrap();
    assert_eq!(index.search("greet").unwrap().len(), 1);
}

#[test]
fn test_long_lines() {
    let dir = TempDir::new().unwrap();
    let long_line = "a".repeat(100_000) + "needle" + &"b".repeat(100_000);
    fs::write(dir.path().join("a.txt"), format!("{}\n", long_line)).unwrap();
    let index = Index::build(dir.path()).unwrap();
    let m = index.search("needle").unwrap();
    assert_eq!(m.len(), 1);
}

// ============================================================================
// BINARY FILE HANDLING
// ============================================================================

#[test]
fn test_binary_files_skipped() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("text.txt"), "findme\n").unwrap();
    fs::write(dir.path().join("binary.bin"), b"findme\x00\x01\x02").unwrap();
    let index = Index::build(dir.path()).unwrap();
    let m = index.search("findme").unwrap();
    // Only text.txt should match, not binary.bin
    assert_eq!(m.len(), 1);
    assert!(m[0].path.ends_with("text.txt"));
}

#[test]
fn test_binary_detection_null_in_first_8k() {
    let dir = TempDir::new().unwrap();
    let mut content = vec![b'a'; 100];
    content[50] = 0; // null byte
    fs::write(dir.path().join("file.dat"), &content).unwrap();
    let index = Index::build(dir.path()).unwrap();
    // Should be skipped as binary
    assert_eq!(index.file_count(), 0);
}

// ============================================================================
// FILE SYSTEM EDGE CASES
// ============================================================================

#[test]
fn test_gitignore_respected() {
    let dir = TempDir::new().unwrap();
    // The `ignore` crate needs a .git dir to respect .gitignore
    fs::create_dir_all(dir.path().join(".git")).unwrap();
    fs::write(dir.path().join(".gitignore"), "ignored/\n").unwrap();
    fs::create_dir_all(dir.path().join("ignored")).unwrap();
    fs::write(dir.path().join("ignored/secret.txt"), "findme\n").unwrap();
    fs::write(dir.path().join("visible.txt"), "findme\n").unwrap();
    let index = Index::build(dir.path()).unwrap();
    let m = index.search("findme").unwrap();
    assert_eq!(m.len(), 1);
    assert!(m[0].path.ends_with("visible.txt"));
}

#[test]
fn test_nested_directories() {
    let dir = make_dir(&[
        ("a/b/c/d/deep.txt", "deep_match\n"),
        ("top.txt", "top_match\n"),
    ]);
    let index = Index::build(dir.path()).unwrap();
    assert_eq!(index.search("deep_match").unwrap().len(), 1);
    assert_eq!(index.search("top_match").unwrap().len(), 1);
}

#[test]
fn test_empty_files_handled() {
    let dir = make_dir(&[("empty.txt", ""), ("has_content.txt", "findme\n")]);
    let index = Index::build(dir.path()).unwrap();
    assert_eq!(index.search("findme").unwrap().len(), 1);
}

#[test]
fn test_file_deleted_after_indexing() {
    let dir = make_dir(&[("a.txt", "hello\n"), ("b.txt", "world\n")]);
    let index = Index::build(dir.path()).unwrap();

    // Delete a file after indexing
    fs::remove_file(dir.path().join("a.txt")).unwrap();

    // Search should still work (skip missing files gracefully)
    let m = index.search("world").unwrap();
    assert_eq!(m.len(), 1);

    // Search for deleted file's content should return empty (file can't be re-read)
    let m = index.search("hello").unwrap();
    assert_eq!(m.len(), 0);
}

#[test]
fn test_file_modified_after_indexing() {
    let dir = make_dir(&[("a.txt", "original_content\n")]);
    let mut index = Index::build(dir.path()).unwrap();

    // Modify the file and update the index
    fs::write(dir.path().join("a.txt"), "modified_content\n").unwrap();
    index
        .update_file(&dir.path().join("a.txt"), dir.path())
        .unwrap();

    // Search finds the NEW content
    let m = index.search("modified_content").unwrap();
    assert_eq!(m.len(), 1);

    // Original content no longer found (trigrams updated)
    let m = index.search("original_content").unwrap();
    assert_eq!(m.len(), 0);
}

#[test]
fn test_stale_index_still_correct() {
    // If file changes WITHOUT update_file, trigram index is stale.
    // Search for old content: file is re-read, old content is gone -> 0 matches.
    // Search for new content: trigrams don't match -> 0 matches (correct: no false negatives, but missing new content).
    // This is expected behavior: the index needs update_file() or rebuild to see changes.
    let dir = make_dir(&[("a.txt", "original_content\n")]);
    let index = Index::build(dir.path()).unwrap();

    fs::write(dir.path().join("a.txt"), "modified_content\n").unwrap();

    // Stale index: old content returns 0 (re-read shows new content, regex doesn't match)
    let m = index.search("original_content").unwrap();
    assert_eq!(m.len(), 0);

    // New content also returns 0 (trigrams block it -- stale but no false positives)
    let m = index.search("modified_content").unwrap();
    assert_eq!(m.len(), 0);
}

#[test]
fn test_large_file() {
    let dir = TempDir::new().unwrap();
    let mut content = String::new();
    for i in 0..10_000 {
        content.push_str(&format!("line {} with some content\n", i));
    }
    content.push_str("unique_needle_xyz\n");
    fs::write(dir.path().join("large.txt"), &content).unwrap();
    let index = Index::build(dir.path()).unwrap();
    let m = index.search("unique_needle_xyz").unwrap();
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].line_number, 10001);
}

#[test]
fn test_many_files() {
    let dir = TempDir::new().unwrap();
    for i in 0..500 {
        fs::write(
            dir.path().join(format!("file_{}.txt", i)),
            format!("content of file {}\n", i),
        )
        .unwrap();
    }
    fs::write(dir.path().join("target.txt"), "unique_target_abc\n").unwrap();
    let index = Index::build(dir.path()).unwrap();
    assert_eq!(index.file_count(), 501);
    let m = index.search("unique_target_abc").unwrap();
    assert_eq!(m.len(), 1);
}

// ============================================================================
// DISK PERSISTENCE
// ============================================================================

#[test]
fn test_persist_save_and_load() {
    let dir = make_dir(&[("a.rs", "fn hello() {}\n"), ("b.txt", "world\n")]);
    let index = Index::build(dir.path()).unwrap();
    index.save().unwrap();

    // Load from cache
    let index2 = Index::build(dir.path()).unwrap();
    assert_eq!(index.file_count(), index2.file_count());
    assert_eq!(index.trigram_count(), index2.trigram_count());

    // Search works on cached index
    assert_eq!(index2.search("hello").unwrap().len(), 1);
    assert_eq!(index2.search("world").unwrap().len(), 1);
}

#[test]
fn test_persist_detects_stale_files() {
    let dir = make_dir(&[("a.txt", "original\n")]);
    let index = Index::build(dir.path()).unwrap();
    index.save().unwrap();

    // Modify the file (changes mtime)
    std::thread::sleep(std::time::Duration::from_millis(100));
    fs::write(dir.path().join("a.txt"), "modified\n").unwrap();

    // Rebuild should detect staleness and rebuild
    let index2 = Index::build(dir.path()).unwrap();
    let m = index2.search("modified").unwrap();
    assert_eq!(m.len(), 1);
}

#[test]
fn test_persist_corrupt_cache_rebuilds() {
    let dir = make_dir(&[("a.txt", "hello\n")]);
    let index = Index::build(dir.path()).unwrap();
    index.save().unwrap();

    // Corrupt the cache file
    fs::write(dir.path().join(".hypergrep/index.bin"), b"garbage").unwrap();

    // Should rebuild from scratch, not crash
    let index2 = Index::build(dir.path()).unwrap();
    assert_eq!(index2.search("hello").unwrap().len(), 1);
}

#[test]
fn test_persist_missing_cache_rebuilds() {
    let dir = make_dir(&[("a.txt", "hello\n")]);
    let index = Index::build(dir.path()).unwrap();
    assert_eq!(index.search("hello").unwrap().len(), 1);
    // No save -- no cache exists. Should just build fresh.
}

#[test]
fn test_persist_creates_gitignore() {
    let dir = make_dir(&[("a.txt", "hello\n")]);
    let index = Index::build(dir.path()).unwrap();
    index.save().unwrap();

    let gitignore = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert!(gitignore.contains(".hypergrep/"));
}

// ============================================================================
// STRUCTURAL SEARCH
// ============================================================================

#[test]
fn test_structural_rust_function() {
    let dir = make_dir(&[(
        "lib.rs",
        "fn process(data: &[u8]) -> Vec<u8> {\n    let x = transform(data);\n    x\n}\n",
    )]);
    let mut index = Index::build(dir.path()).unwrap();
    let m = index.search_structural("transform").unwrap();
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].symbol_name, "process");
    assert!(m[0].body.contains("fn process"));
    assert!(m[0].body.contains("transform"));
}

#[test]
fn test_structural_python_class() {
    let dir = make_dir(&[(
        "app.py",
        "class UserService:\n    def get_user(self, uid):\n        return self.db.find(uid)\n\n    def delete_user(self, uid):\n        self.db.remove(uid)\n",
    )]);
    let mut index = Index::build(dir.path()).unwrap();
    let m = index.search_structural("find").unwrap();
    assert!(m.iter().any(|s| s.symbol_name == "get_user"));
}

#[test]
fn test_structural_unsupported_language_fallback() {
    let dir = make_dir(&[("data.csv", "name,value\nfoo,123\nbar,456\n")]);
    let mut index = Index::build(dir.path()).unwrap();
    let m = index.search_structural("foo").unwrap();
    // Falls back to line-level match (no tree-sitter for .csv)
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].symbol_name, "<module>");
}

#[test]
fn test_structural_dedup_multiple_matches_in_function() {
    let dir = make_dir(&[(
        "lib.rs",
        "fn repeat(x: i32) -> i32 {\n    let a = x;\n    let b = x;\n    let c = x;\n    a + b + c\n}\n",
    )]);
    let mut index = Index::build(dir.path()).unwrap();
    // "x" appears on 4 lines inside repeat(), should return function only once
    let m = index.search_structural("x").unwrap();
    let repeat_matches: Vec<_> = m.iter().filter(|s| s.symbol_name == "repeat").collect();
    assert_eq!(repeat_matches.len(), 1);
}

#[test]
fn test_structural_empty_file() {
    let dir = make_dir(&[("empty.rs", ""), ("real.rs", "fn hello() {}\n")]);
    let mut index = Index::build(dir.path()).unwrap();
    let m = index.search_structural("hello").unwrap();
    assert_eq!(m.len(), 1);
}

// ============================================================================
// GRAPH QUERIES
// ============================================================================

#[test]
fn test_graph_simple_call_chain() {
    let dir = make_dir(&[("chain.rs", "fn a() { b() }\nfn b() { c() }\nfn c() {}\n")]);
    let mut index = Index::build(dir.path()).unwrap();
    index.complete_index();

    let callers = index.graph.callers_of("c");
    assert!(callers.iter().any(|s| s.name == "b"));

    let impact = index.graph.impact("c", 3);
    let names: Vec<&str> = impact.iter().map(|r| r.symbol.name.as_str()).collect();
    assert!(names.contains(&"b")); // depth 1
    assert!(names.contains(&"a")); // depth 2
}

#[test]
fn test_graph_no_callers() {
    let dir = make_dir(&[("lib.rs", "fn orphan() {}\n")]);
    let mut index = Index::build(dir.path()).unwrap();
    index.complete_index();
    assert!(index.graph.callers_of("orphan").is_empty());
    assert!(index.graph.impact("orphan", 3).is_empty());
}

#[test]
fn test_graph_recursive_function() {
    let dir = make_dir(&[(
        "rec.rs",
        "fn factorial(n: u64) -> u64 {\n    if n <= 1 { 1 } else { n * factorial(n - 1) }\n}\n",
    )]);
    let mut index = Index::build(dir.path()).unwrap();
    index.complete_index();
    // factorial calls itself -- should not infinite loop in impact analysis
    let impact = index.graph.impact("factorial", 3);
    // Should complete without hanging
    assert!(impact.len() <= 10); // bounded
}

#[test]
fn test_graph_cross_file() {
    let dir = make_dir(&[
        ("auth.rs", "fn login() { validate() }\n"),
        ("validator.rs", "fn validate() {}\n"),
    ]);
    let mut index = Index::build(dir.path()).unwrap();
    index.complete_index();

    let callers = index.graph.callers_of("validate");
    assert!(callers.iter().any(|s| s.name == "login"));
}

// ============================================================================
// SEMANTIC COMPRESSION
// ============================================================================

#[test]
fn test_semantic_layer0_minimal() {
    let dir = make_dir(&[("lib.rs", "fn target() { secret() }\nfn secret() {}\n")]);
    let mut index = Index::build(dir.path()).unwrap();
    let results = index
        .search_semantic("target", hypergrep_core::semantic::Layer::L0, None)
        .unwrap();
    assert!(!results.is_empty());
    assert!(results[0].body.is_none());
    assert!(results[0].signature.is_none());
}

#[test]
fn test_semantic_layer1_has_signature() {
    let dir = make_dir(&[(
        "lib.rs",
        "fn authenticate(user: &str) -> bool {\n    true\n}\n",
    )]);
    let mut index = Index::build(dir.path()).unwrap();
    let results = index
        .search_semantic("authenticate", hypergrep_core::semantic::Layer::L1, None)
        .unwrap();
    assert!(!results.is_empty());
    assert!(results[0].signature.is_some());
    let sig = results[0].signature.as_ref().unwrap();
    assert!(sig.contains("fn authenticate"));
}

#[test]
fn test_semantic_layer2_has_body() {
    let dir = make_dir(&[("lib.rs", "fn hello() {\n    println!(\"hi\");\n}\n")]);
    let mut index = Index::build(dir.path()).unwrap();
    let results = index
        .search_semantic("hello", hypergrep_core::semantic::Layer::L2, None)
        .unwrap();
    assert!(!results.is_empty());
    assert!(results[0].body.is_some());
    assert!(results[0].body.as_ref().unwrap().contains("println!"));
}

#[test]
fn test_semantic_budget_limits_results() {
    let dir = make_dir(&[(
        "lib.rs",
        "fn aaa() {}\nfn bbb() {}\nfn ccc() {}\nfn ddd() {}\nfn eee() {}\n",
    )]);
    let mut index = Index::build(dir.path()).unwrap();
    let all = index
        .search_semantic("fn", hypergrep_core::semantic::Layer::L1, None)
        .unwrap();
    let budgeted = index
        .search_semantic("fn", hypergrep_core::semantic::Layer::L1, Some(100))
        .unwrap();
    assert!(budgeted.len() <= all.len());
    let total_tokens: usize = budgeted.iter().map(|r| r.tokens).sum();
    // Should respect budget (with at-least-one guarantee)
    assert!(total_tokens <= 200); // some slack for at-least-one rule
}

// ============================================================================
// BLOOM FILTER
// ============================================================================

#[test]
fn test_bloom_no_false_negatives() {
    let mut filter = hypergrep_core::bloom::BloomFilter::new(1000, 0.01);
    let items: Vec<String> = (0..1000).map(|i| format!("item_{}", i)).collect();
    for item in &items {
        filter.insert(item);
    }
    for item in &items {
        assert!(filter.might_contain(item), "False negative: {}", item);
    }
}

#[test]
fn test_bloom_reasonable_false_positive_rate() {
    let mut filter = hypergrep_core::bloom::BloomFilter::new(1000, 0.01);
    for i in 0..1000 {
        filter.insert(&format!("item_{}", i));
    }
    let mut fp = 0;
    for i in 0..10000 {
        if filter.might_contain(&format!("absent_{}", i)) {
            fp += 1;
        }
    }
    let rate = fp as f64 / 10000.0;
    assert!(rate < 0.05, "FP rate too high: {:.2}%", rate * 100.0);
}

#[test]
fn test_bloom_cargo_toml_parsing() {
    let dir = make_dir(&[(
        "Cargo.toml",
        "[package]\nname = \"myapp\"\n\n[dependencies]\nregex = \"1\"\nserde = { version = \"1\" }\ntokio = \"1\"\n",
    )]);
    let index = Index::build(dir.path()).unwrap();
    assert!(index.bloom.might_contain("regex"));
    assert!(index.bloom.might_contain("serde"));
    assert!(index.bloom.might_contain("tokio"));
    assert!(!index.bloom.might_contain("django"));
}

#[test]
fn test_bloom_package_json_parsing() {
    let dir = make_dir(&[(
        "package.json",
        "{\n  \"dependencies\": {\n    \"express\": \"^4.0\",\n    \"react\": \"^18.0\"\n  }\n}\n",
    )]);
    let index = Index::build(dir.path()).unwrap();
    assert!(index.bloom.might_contain("express"));
    assert!(index.bloom.might_contain("react"));
}

#[test]
fn test_bloom_requirements_txt_parsing() {
    let dir = make_dir(&[("requirements.txt", "flask>=2.0\nredis==4.0\npytest\n")]);
    let index = Index::build(dir.path()).unwrap();
    assert!(index.bloom.might_contain("flask"));
    assert!(index.bloom.might_contain("redis"));
    assert!(index.bloom.might_contain("pytest"));
}

// ============================================================================
// INCREMENTAL UPDATES
// ============================================================================

#[test]
fn test_update_add_new_file() {
    let dir = make_dir(&[("a.txt", "hello\n")]);
    let mut index = Index::build(dir.path()).unwrap();
    assert_eq!(index.file_count(), 1);

    fs::write(dir.path().join("b.txt"), "world\n").unwrap();
    index
        .update_file(&dir.path().join("b.txt"), dir.path())
        .unwrap();

    assert_eq!(index.file_count(), 2);
    assert_eq!(index.search("world").unwrap().len(), 1);
}

#[test]
fn test_update_modify_existing_file() {
    let dir = make_dir(&[("a.txt", "old_content\n")]);
    let mut index = Index::build(dir.path()).unwrap();
    assert_eq!(index.search("old_content").unwrap().len(), 1);

    fs::write(dir.path().join("a.txt"), "new_content\n").unwrap();
    index
        .update_file(&dir.path().join("a.txt"), dir.path())
        .unwrap();

    // Old content gone from trigram index (but file re-read will show new content)
    assert_eq!(index.search("new_content").unwrap().len(), 1);
}

#[test]
fn test_update_delete_file() {
    let dir = make_dir(&[("a.txt", "findme\n")]);
    let mut index = Index::build(dir.path()).unwrap();

    fs::remove_file(dir.path().join("a.txt")).unwrap();
    index
        .update_file(&dir.path().join("a.txt"), dir.path())
        .unwrap();

    assert!(index.search("findme").unwrap().is_empty());
}

// ============================================================================
// MENTAL MODEL
// ============================================================================

#[test]
fn test_mental_model_generated() {
    let dir = make_dir(&[
        ("src/main.rs", "fn main() {}\n"),
        ("src/lib.rs", "pub fn hello() {}\npub struct Config {}\n"),
        ("tests/test.rs", "fn test_hello() {}\n"),
    ]);
    let mut index = Index::build(dir.path()).unwrap();
    index.complete_index();

    let text = hypergrep_core::mental_model::format_text(&index.mental_model);
    assert!(text.contains("Codebase Mental Model"));
    assert!(text.len() > 50);
    assert!(index.mental_model.tokens > 0);
    assert!(index.mental_model.tokens < 2000);
}

#[test]
fn test_mental_model_empty_project() {
    let dir = TempDir::new().unwrap();
    let mut index = Index::build(dir.path()).unwrap();
    index.complete_index();
    // Should not crash, should produce minimal model
    let text = hypergrep_core::mental_model::format_text(&index.mental_model);
    assert!(text.contains("Codebase Mental Model"));
}

// ============================================================================
// LANGUAGE DETECTION
// ============================================================================

#[test]
fn test_all_supported_languages() {
    use hypergrep_core::structure::Lang;
    assert_eq!(Lang::from_path(&PathBuf::from("a.rs")), Some(Lang::Rust));
    assert_eq!(Lang::from_path(&PathBuf::from("a.py")), Some(Lang::Python));
    assert_eq!(
        Lang::from_path(&PathBuf::from("a.js")),
        Some(Lang::JavaScript)
    );
    assert_eq!(
        Lang::from_path(&PathBuf::from("a.jsx")),
        Some(Lang::JavaScript)
    );
    assert_eq!(
        Lang::from_path(&PathBuf::from("a.ts")),
        Some(Lang::TypeScript)
    );
    assert_eq!(
        Lang::from_path(&PathBuf::from("a.tsx")),
        Some(Lang::TypeScript)
    );
    assert_eq!(Lang::from_path(&PathBuf::from("a.go")), Some(Lang::Go));
    assert_eq!(Lang::from_path(&PathBuf::from("a.java")), Some(Lang::Java));
    assert_eq!(Lang::from_path(&PathBuf::from("a.c")), Some(Lang::C));
    assert_eq!(Lang::from_path(&PathBuf::from("a.h")), Some(Lang::C));
    assert_eq!(Lang::from_path(&PathBuf::from("a.cpp")), Some(Lang::Cpp));
    assert_eq!(Lang::from_path(&PathBuf::from("a.hpp")), Some(Lang::Cpp));
    assert_eq!(Lang::from_path(&PathBuf::from("a.txt")), None);
    assert_eq!(Lang::from_path(&PathBuf::from("a.csv")), None);
    assert_eq!(Lang::from_path(&PathBuf::from("a")), None);
}

// ============================================================================
// MULTI-LANGUAGE STRUCTURAL PARSING
// ============================================================================

#[test]
fn test_parse_typescript() {
    let dir = make_dir(&[(
        "app.ts",
        "function handleRequest(req: Request): Response {\n    return new Response(\"ok\");\n}\n\nclass Router {\n    get(path: string) {}\n}\n",
    )]);
    let mut index = Index::build(dir.path()).unwrap();
    let m = index.search_structural("handleRequest").unwrap();
    assert!(!m.is_empty());
}

#[test]
fn test_parse_go() {
    let dir = make_dir(&[(
        "main.go",
        "package main\n\nfunc main() {\n    fmt.Println(\"hello\")\n}\n\nfunc helper() int {\n    return 42\n}\n",
    )]);
    let mut index = Index::build(dir.path()).unwrap();
    let m = index.search_structural("Println").unwrap();
    assert!(!m.is_empty());
}

#[test]
fn test_parse_java() {
    let dir = make_dir(&[(
        "App.java",
        "public class App {\n    public void run() {\n        System.out.println(\"hello\");\n    }\n}\n",
    )]);
    let mut index = Index::build(dir.path()).unwrap();
    let m = index.search_structural("println").unwrap();
    assert!(!m.is_empty());
}

#[test]
fn test_parse_c() {
    let dir = make_dir(&[(
        "main.c",
        "int add(int a, int b) {\n    return a + b;\n}\n\nint main() {\n    return add(1, 2);\n}\n",
    )]);
    let mut index = Index::build(dir.path()).unwrap();
    let m = index.search_structural("add").unwrap();
    assert!(!m.is_empty());
}
