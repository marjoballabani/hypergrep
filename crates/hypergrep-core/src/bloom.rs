/// Negative index: bloom filter for O(1) existence queries.
///
/// "Does this codebase use Redis?" answered in microseconds instead of
/// scanning every file for zero results.
///
/// Zero false negatives: if the filter says "no", it's definitely not there.
/// ~1% false positives: if it says "maybe", fall back to real search.

/// A simple bloom filter for concept/technology detection.
pub struct BloomFilter {
    bits: Vec<u64>,
    num_bits: usize,
    num_hashes: u32,
    items: usize,
}

impl BloomFilter {
    /// Create a new bloom filter sized for `expected_items` with target false positive rate.
    pub fn new(expected_items: usize, fp_rate: f64) -> Self {
        // Optimal number of bits: -n * ln(p) / (ln(2))^2
        let num_bits =
            (-(expected_items as f64) * fp_rate.ln() / (2.0_f64.ln().powi(2))).ceil() as usize;
        let num_bits = num_bits.max(64);

        // Optimal number of hashes: (m/n) * ln(2)
        let num_hashes = ((num_bits as f64 / expected_items as f64) * 2.0_f64.ln()).ceil() as u32;
        let num_hashes = num_hashes.max(1).min(16);

        let words = num_bits.div_ceil(64);

        BloomFilter {
            bits: vec![0u64; words],
            num_bits,
            num_hashes,
            items: 0,
        }
    }

    /// Insert a string into the filter.
    pub fn insert(&mut self, item: &str) {
        let item_lower = item.to_lowercase();
        for i in 0..self.num_hashes {
            let idx = self.hash(&item_lower, i);
            let word = idx / 64;
            let bit = idx % 64;
            self.bits[word] |= 1 << bit;
        }
        self.items += 1;
    }

    /// Check if an item might be in the filter.
    /// Returns false = definitely not present, true = possibly present.
    pub fn might_contain(&self, item: &str) -> bool {
        let item_lower = item.to_lowercase();
        for i in 0..self.num_hashes {
            let idx = self.hash(&item_lower, i);
            let word = idx / 64;
            let bit = idx % 64;
            if self.bits[word] & (1 << bit) == 0 {
                return false;
            }
        }
        true
    }

    /// Number of items inserted.
    pub fn len(&self) -> usize {
        self.items
    }

    /// Whether the filter is empty.
    pub fn is_empty(&self) -> bool {
        self.items == 0
    }

    /// Size in bytes.
    pub fn size_bytes(&self) -> usize {
        self.bits.len() * 8
    }

    /// Reconstruct from raw parts (for deserialization).
    pub fn from_raw(bits: Vec<u64>, num_bits: usize, num_hashes: u32, items: usize) -> Self {
        BloomFilter {
            bits,
            num_bits,
            num_hashes,
            items,
        }
    }

    /// Access raw bits (for serialization).
    pub fn bits(&self) -> &[u64] {
        &self.bits
    }

    /// Number of bits in the filter.
    pub fn num_bits(&self) -> usize {
        self.num_bits
    }

    /// Number of hash functions.
    pub fn num_hashes(&self) -> u32 {
        self.num_hashes
    }

    /// Double-hashing scheme: h(item, i) = (h1 + i * h2) mod m
    fn hash(&self, item: &str, i: u32) -> usize {
        let h1 = fnv1a(item.as_bytes());
        let h2 = fnv1a_seed(item.as_bytes(), 0x517cc1b727220a95);
        ((h1 as u128 + i as u128 * h2 as u128) % self.num_bits as u128) as usize
    }
}

/// FNV-1a hash.
fn fnv1a(data: &[u8]) -> u64 {
    fnv1a_seed(data, 0xcbf29ce484222325)
}

fn fnv1a_seed(data: &[u8], seed: u64) -> u64 {
    let mut hash = seed;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Build a bloom filter from indexed file contents.
/// Indexes: import names, file extensions, technology keywords,
/// framework patterns, library names.
pub fn build_concept_filter(files: &[(std::path::PathBuf, &[u8])]) -> BloomFilter {
    let mut filter = BloomFilter::new(10_000, 0.01);

    for (path, content) in files {
        // File extension
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            filter.insert(ext);
        }

        // File name
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            filter.insert(name);
        }

        // Extract technology indicators from content
        let text = String::from_utf8_lossy(content);
        extract_concepts(&text, &mut filter);

        // Parse package manifests for dependency names
        extract_manifest_deps(path, &text, &mut filter);
    }

    filter
}

/// Extract dependency names from package manifest files.
fn extract_manifest_deps(path: &std::path::Path, content: &str, filter: &mut BloomFilter) {
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    match filename {
        "Cargo.toml" => extract_cargo_deps(content, filter),
        "package.json" => extract_npm_deps(content, filter),
        "go.mod" => extract_go_deps(content, filter),
        "requirements.txt" => extract_pip_deps(content, filter),
        "pyproject.toml" => extract_pyproject_deps(content, filter),
        "Gemfile" => extract_gemfile_deps(content, filter),
        _ => {}
    }
}

/// Parse Cargo.toml dependencies (line-based, no TOML parser needed).
fn extract_cargo_deps(content: &str, filter: &mut BloomFilter) {
    let mut in_deps = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_deps = trimmed.contains("dependencies");
            continue;
        }
        if in_deps {
            // Lines like: regex = "1.0" or serde = { version = "1", features = [...] }
            if let Some(name) = trimmed.split('=').next() {
                let name = name.trim();
                if !name.is_empty() && !name.starts_with('#') {
                    filter.insert(name);
                    // Also insert with underscores replaced by hyphens and vice versa
                    filter.insert(&name.replace('-', "_"));
                    filter.insert(&name.replace('_', "-"));
                }
            }
        }
    }
}

/// Parse package.json dependencies.
fn extract_npm_deps(content: &str, filter: &mut BloomFilter) {
    // Simple approach: find all "name": "version" patterns
    // between "dependencies" or "devDependencies" sections
    let mut in_deps = false;
    let mut brace_depth = 0;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.contains("\"dependencies\"")
            || trimmed.contains("\"devDependencies\"")
            || trimmed.contains("\"peerDependencies\"")
        {
            in_deps = true;
            if trimmed.contains('{') {
                brace_depth = 1;
            }
            continue;
        }
        if in_deps {
            if trimmed.contains('{') {
                brace_depth += 1;
            }
            if trimmed.contains('}') {
                brace_depth -= 1;
                if brace_depth <= 0 {
                    in_deps = false;
                    continue;
                }
            }
            // Extract package name from "name": "version"
            if let Some(name) = trimmed.strip_prefix('"')
                && let Some(end) = name.find('"')
            {
                let pkg = &name[..end];
                if !pkg.is_empty() {
                    filter.insert(pkg);
                    // Also insert the unscoped name for @scope/name packages
                    if let Some(unscoped) = pkg.strip_prefix('@')
                        && let Some(name_part) = unscoped.split('/').nth(1)
                    {
                        filter.insert(name_part);
                    }
                }
            }
        }
    }
}

/// Parse go.mod dependencies.
fn extract_go_deps(content: &str, filter: &mut BloomFilter) {
    let mut in_require = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "require (" {
            in_require = true;
            continue;
        }
        if trimmed == ")" {
            in_require = false;
            continue;
        }
        if in_require {
            // Lines like: github.com/foo/bar v1.2.3
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if let Some(module) = parts.first() {
                filter.insert(module);
                // Also insert the last path segment
                if let Some(name) = module.rsplit('/').next() {
                    filter.insert(name);
                }
            }
        }
        // Single-line: require github.com/foo/bar v1.2.3
        if trimmed.starts_with("require ") && !trimmed.contains('(') {
            let rest = &trimmed[8..];
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if let Some(module) = parts.first() {
                filter.insert(module);
                if let Some(name) = module.rsplit('/').next() {
                    filter.insert(name);
                }
            }
        }
    }
}

/// Parse requirements.txt dependencies.
fn extract_pip_deps(content: &str, filter: &mut BloomFilter) {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('-') {
            continue;
        }
        // Strip version specifiers: package>=1.0, package==2.0, package[extra]>=1.0
        let name = trimmed
            .split(['>', '<', '=', '!', '~', '['])
            .next()
            .unwrap_or(trimmed)
            .trim();
        if !name.is_empty() {
            filter.insert(name);
            filter.insert(&name.replace('-', "_"));
            filter.insert(&name.replace('_', "-"));
        }
    }
}

/// Parse pyproject.toml dependencies.
fn extract_pyproject_deps(content: &str, filter: &mut BloomFilter) {
    let mut in_deps = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.contains("[project.dependencies]")
            || trimmed.contains("[tool.poetry.dependencies]")
        {
            in_deps = true;
            continue;
        }
        if trimmed.starts_with('[') && in_deps {
            in_deps = false;
            continue;
        }
        if in_deps {
            // "package>=1.0" or package = "^1.0"
            let name = trimmed
                .split(['>', '<', '=', '!', '~', '"', ' '])
                .next()
                .unwrap_or(trimmed)
                .trim_matches('"')
                .trim();
            if !name.is_empty() && !name.starts_with('#') {
                filter.insert(name);
            }
        }
    }
}

/// Parse Gemfile dependencies.
fn extract_gemfile_deps(content: &str, filter: &mut BloomFilter) {
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("gem ") {
            // gem 'name', '~> 1.0'
            let name = rest
                .trim()
                .trim_start_matches(['\'', '"'])
                .split(['\'', '"'])
                .next()
                .unwrap_or("");
            if !name.is_empty() {
                filter.insert(name);
            }
        }
    }
}

/// Extract technology concepts from source code text.
fn extract_concepts(text: &str, filter: &mut BloomFilter) {
    // Common patterns that indicate technology usage
    let patterns = [
        // Databases
        ("redis", "redis"),
        ("postgres", "postgresql"),
        ("mysql", "mysql"),
        ("mongodb", "mongodb"),
        ("sqlite", "sqlite"),
        ("dynamodb", "dynamodb"),
        // Frameworks
        ("express", "express"),
        ("fastapi", "fastapi"),
        ("django", "django"),
        ("flask", "flask"),
        ("spring", "spring"),
        ("actix", "actix"),
        ("axum", "axum"),
        ("gin", "gin"),
        ("echo", "echo"),
        ("nextjs", "next.js"),
        ("react", "react"),
        ("vue", "vue"),
        ("angular", "angular"),
        ("svelte", "svelte"),
        // Protocols
        ("graphql", "graphql"),
        ("grpc", "grpc"),
        ("websocket", "websocket"),
        ("rest", "rest-api"),
        // Infrastructure
        ("docker", "docker"),
        ("kubernetes", "kubernetes"),
        ("terraform", "terraform"),
        ("aws", "aws"),
        ("gcp", "gcp"),
        ("azure", "azure"),
        // Testing
        ("jest", "jest"),
        ("pytest", "pytest"),
        ("vitest", "vitest"),
        ("mocha", "mocha"),
        // Auth
        ("oauth", "oauth"),
        ("jwt", "jwt"),
        ("bcrypt", "bcrypt"),
        // Queues
        ("rabbitmq", "rabbitmq"),
        ("kafka", "kafka"),
        ("celery", "celery"),
        ("bull", "bull"),
        // Languages (in imports/usage)
        ("typescript", "typescript"),
        ("python", "python"),
    ];

    let text_lower = text.to_lowercase();
    for (pattern, concept) in &patterns {
        if text_lower.contains(pattern) {
            filter.insert(concept);
        }
    }

    // Extract import/require/use names
    for line in text.lines() {
        let trimmed = line.trim();
        // Python: import X, from X import Y
        if trimmed.starts_with("import ") || trimmed.starts_with("from ") {
            for word in trimmed.split_whitespace() {
                if word != "import" && word != "from" && word != "as" {
                    let clean =
                        word.trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-');
                    if clean.len() >= 2 {
                        filter.insert(clean);
                    }
                }
            }
        }
        // JS/TS: import ... from 'X', require('X')
        if trimmed.contains("require(") || trimmed.contains("from '") || trimmed.contains("from \"")
        {
            for word in trimmed.split(['\'', '"']) {
                let clean = word.trim();
                if !clean.is_empty() && !clean.contains(' ') && clean.len() >= 2 {
                    filter.insert(clean);
                    // Also insert the package name (first segment)
                    if let Some(pkg) = clean.split('/').next()
                        && pkg.len() >= 2
                    {
                        filter.insert(pkg);
                    }
                }
            }
        }
        // Rust: use X::Y
        if let Some(rest) = trimmed.strip_prefix("use ") {
            if let Some(crate_name) = rest.split("::").next() {
                let clean = crate_name.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
                if clean.len() >= 2 {
                    filter.insert(clean);
                }
            }
        }
        // Go: import "X"
        if trimmed.starts_with("\"") && trimmed.ends_with("\"") {
            let clean = trimmed.trim_matches('"');
            if clean.len() >= 2 {
                filter.insert(clean);
                if let Some(last) = clean.rsplit('/').next() {
                    filter.insert(last);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_bloom_basic() {
        let mut filter = BloomFilter::new(100, 0.01);
        filter.insert("hello");
        filter.insert("world");

        assert!(filter.might_contain("hello"));
        assert!(filter.might_contain("world"));
        assert!(!filter.might_contain("nonexistent_unique_string_xyz"));
    }

    #[test]
    fn test_bloom_case_insensitive() {
        let mut filter = BloomFilter::new(100, 0.01);
        filter.insert("Redis");

        assert!(filter.might_contain("redis"));
        assert!(filter.might_contain("REDIS"));
    }

    #[test]
    fn test_bloom_false_positive_rate() {
        let n = 1000;
        let mut filter = BloomFilter::new(n, 0.01);

        for i in 0..n {
            filter.insert(&format!("item_{}", i));
        }

        // Check false positive rate on items NOT inserted
        let test_count = 10_000;
        let mut false_positives = 0;
        for i in 0..test_count {
            if filter.might_contain(&format!("nonexistent_{}", i)) {
                false_positives += 1;
            }
        }

        let fp_rate = false_positives as f64 / test_count as f64;
        // Should be around 1% (allow up to 3% for statistical variance)
        assert!(
            fp_rate < 0.03,
            "False positive rate too high: {:.2}%",
            fp_rate * 100.0
        );
    }

    #[test]
    fn test_bloom_zero_false_negatives() {
        let mut filter = BloomFilter::new(100, 0.01);

        let items: Vec<String> = (0..100).map(|i| format!("item_{}", i)).collect();
        for item in &items {
            filter.insert(item);
        }

        // Every inserted item must be found -- zero false negatives
        for item in &items {
            assert!(filter.might_contain(item), "False negative for '{}'", item);
        }
    }

    #[test]
    fn test_concept_detection() {
        let files = vec![(
            PathBuf::from("app.py"),
            b"import redis\nfrom flask import Flask\nimport pytest\n" as &[u8],
        )];

        let filter = build_concept_filter(&files);

        assert!(filter.might_contain("redis"));
        assert!(filter.might_contain("flask"));
        assert!(filter.might_contain("pytest"));
        assert!(!filter.might_contain("kubernetes"));
        assert!(!filter.might_contain("graphql"));
    }

    #[test]
    fn test_concept_detection_rust() {
        let files = vec![(
            PathBuf::from("main.rs"),
            b"use tokio;\nuse serde::Serialize;\nuse axum::Router;\n" as &[u8],
        )];

        let filter = build_concept_filter(&files);

        assert!(filter.might_contain("tokio"));
        assert!(filter.might_contain("serde"));
        assert!(filter.might_contain("axum"));
    }

    #[test]
    fn test_bloom_size() {
        let filter = BloomFilter::new(10_000, 0.01);
        // Should be around 12KB for 10K items at 1% FPR
        assert!(
            filter.size_bytes() < 20_000,
            "Filter too large: {} bytes",
            filter.size_bytes()
        );
    }

    #[test]
    fn test_cargo_toml_deps() {
        let cargo = br#"
[package]
name = "myapp"
version = "0.1.0"

[dependencies]
regex = "1"
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
tree-sitter = "0.24"

[dev-dependencies]
tempfile = "3"
"#;
        let files = vec![(PathBuf::from("Cargo.toml"), cargo.as_slice())];
        let filter = build_concept_filter(&files);

        assert!(filter.might_contain("regex"));
        assert!(filter.might_contain("serde"));
        assert!(filter.might_contain("tokio"));
        assert!(filter.might_contain("tree-sitter"));
        assert!(filter.might_contain("tree_sitter")); // underscore variant
        assert!(filter.might_contain("tempfile"));
    }

    #[test]
    fn test_package_json_deps() {
        let pkg = br#"{
  "name": "myapp",
  "dependencies": {
    "express": "^4.18.0",
    "react": "^18.0.0",
    "@types/node": "^20.0.0"
  },
  "devDependencies": {
    "jest": "^29.0.0"
  }
}"#;
        let files = vec![(PathBuf::from("package.json"), pkg.as_slice())];
        let filter = build_concept_filter(&files);

        assert!(filter.might_contain("express"));
        assert!(filter.might_contain("react"));
        assert!(filter.might_contain("jest"));
        assert!(filter.might_contain("node")); // unscoped from @types/node
    }

    #[test]
    fn test_requirements_txt_deps() {
        let reqs = b"flask>=2.0\nredis==4.0.0\ncelery[redis]>=5.0\npytest\n";
        let files = vec![(PathBuf::from("requirements.txt"), reqs.as_slice())];
        let filter = build_concept_filter(&files);

        assert!(filter.might_contain("flask"));
        assert!(filter.might_contain("redis"));
        assert!(filter.might_contain("celery"));
        assert!(filter.might_contain("pytest"));
    }

    #[test]
    fn test_go_mod_deps() {
        let gomod = br#"module github.com/myorg/myapp

go 1.21

require (
	github.com/gin-gonic/gin v1.9.1
	github.com/redis/go-redis/v9 v9.0.0
)
"#;
        let files = vec![(PathBuf::from("go.mod"), gomod.as_slice())];
        let filter = build_concept_filter(&files);

        assert!(filter.might_contain("gin"));
        assert!(filter.might_contain("github.com/redis/go-redis/v9"));
    }
}
