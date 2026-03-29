/// Predictive query prefetch: speculatively execute likely next queries
/// while the LLM is generating its response.
///
/// Agent search patterns are structurally predictable:
/// - Function definition search -> callers (~70% likely)
/// - Error message search -> handler (~60% likely)
/// - Type/interface search -> implementations (~70% likely)
/// - Test file search -> source file (~75% likely)
///
/// The prefetch engine predicts the next 3-5 queries based on the current
/// query and caches results. When the prediction hits, perceived latency is zero.
use std::collections::HashMap;

use crate::index::{Index, SearchMatch, StructuralMatch};
use crate::semantic::{Layer, SemanticResult};

/// A predicted query with its confidence score.
#[derive(Debug, Clone)]
pub struct Prediction {
    pub query: PredictedQuery,
    pub confidence: f64,
    pub reason: &'static str,
}

/// Types of predicted queries.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum PredictedQuery {
    /// Search for callers of a symbol
    Callers(String),
    /// Search for callees of a symbol
    Callees(String),
    /// Text search for a related pattern
    Search(String),
    /// Impact analysis
    Impact(String),
}

/// Cached prefetch results.
pub struct PrefetchCache {
    /// Cached search results keyed by predicted query
    pub entries: HashMap<PredictedQuery, CachedResult>,
    /// Total predictions made
    pub total_predictions: usize,
    /// Predictions that were actually used (hits)
    pub hits: usize,
}

/// A cached search result.
#[derive(Debug, Clone)]
pub enum CachedResult {
    Search(Vec<SearchMatch>),
    Structural(Vec<StructuralMatch>),
    Semantic(Vec<SemanticResult>),
    Callers(Vec<String>),
    Impact(Vec<String>),
}

impl Default for PrefetchCache {
    fn default() -> Self {
        Self::new()
    }
}

impl PrefetchCache {
    pub fn new() -> Self {
        PrefetchCache {
            entries: HashMap::new(),
            total_predictions: 0,
            hits: 0,
        }
    }

    /// Check if a query has been prefetched.
    pub fn get(&mut self, query: &PredictedQuery) -> Option<&CachedResult> {
        if self.entries.contains_key(query) {
            self.hits += 1;
            self.entries.get(query)
        } else {
            None
        }
    }

    /// Hit rate as a percentage.
    pub fn hit_rate(&self) -> f64 {
        if self.total_predictions == 0 {
            return 0.0;
        }
        self.hits as f64 / self.total_predictions as f64 * 100.0
    }

    /// Clear the cache.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Predict the next 3-5 queries based on the current query and its results.
pub fn predict(current_pattern: &str, results: &[SearchMatch], index: &Index) -> Vec<Prediction> {
    let mut predictions = Vec::new();

    // Strategy 1: If the pattern looks like a function name and was found,
    // predict the user will want callers
    if is_likely_symbol_name(current_pattern) {
        predictions.push(Prediction {
            query: PredictedQuery::Callers(current_pattern.to_string()),
            confidence: 0.70,
            reason: "symbol search -> callers",
        });

        predictions.push(Prediction {
            query: PredictedQuery::Impact(current_pattern.to_string()),
            confidence: 0.40,
            reason: "symbol search -> impact analysis",
        });
    }

    // Strategy 2: If results are in a specific file, predict searches
    // for related symbols in the same file
    if let Some(first) = results.first() {
        let doc_id = index.files.iter().position(|f| f.path == first.path);

        if let Some(doc_id) = doc_id {
            let symbols = &index.symbols[doc_id];
            // Find the enclosing symbol and predict searches for its callees
            for sym in symbols {
                if sym.name != current_pattern && sym.name.len() > 2 {
                    let callees = index.graph.callees_of(&sym.name);
                    for callee in callees.iter().take(2) {
                        if callee.name != current_pattern {
                            predictions.push(Prediction {
                                query: PredictedQuery::Search(callee.name.clone()),
                                confidence: 0.35,
                                reason: "related symbol in same file",
                            });
                        }
                    }
                }
            }
        }
    }

    // Strategy 3: If the pattern contains "test", predict the source file
    if current_pattern.contains("test") || current_pattern.contains("Test") {
        let source_name = current_pattern
            .replace("test_", "")
            .replace("Test", "")
            .replace("test", "");
        if source_name.len() >= 3 {
            predictions.push(Prediction {
                query: PredictedQuery::Search(source_name),
                confidence: 0.75,
                reason: "test search -> source",
            });
        }
    }

    // Strategy 4: If the pattern contains "error" or "Error", predict handler search
    if current_pattern.to_lowercase().contains("error") {
        predictions.push(Prediction {
            query: PredictedQuery::Search("handle".to_string()),
            confidence: 0.60,
            reason: "error search -> handler",
        });
        predictions.push(Prediction {
            query: PredictedQuery::Search("catch".to_string()),
            confidence: 0.45,
            reason: "error search -> catch block",
        });
    }

    // Sort by confidence descending, take top 5
    predictions.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
    predictions.truncate(5);
    predictions
}

/// Execute predictions and cache results.
pub fn execute_predictions(
    predictions: &[Prediction],
    index: &mut Index,
    cache: &mut PrefetchCache,
) {
    for pred in predictions {
        cache.total_predictions += 1;

        match &pred.query {
            PredictedQuery::Callers(name) => {
                let callers: Vec<String> = index
                    .graph
                    .callers_of(name)
                    .into_iter()
                    .map(|s| format!("{}:{}", s.file.display(), s.name))
                    .collect();
                cache
                    .entries
                    .insert(pred.query.clone(), CachedResult::Callers(callers));
            }
            PredictedQuery::Search(pattern) => {
                if let Ok(results) = index.search_semantic(pattern, Layer::L1, Some(500)) {
                    cache
                        .entries
                        .insert(pred.query.clone(), CachedResult::Semantic(results));
                }
            }
            PredictedQuery::Impact(name) => {
                let impact: Vec<String> = index
                    .graph
                    .impact(name, 3)
                    .into_iter()
                    .map(|r| {
                        format!(
                            "[depth {}] {} {}:{}",
                            r.depth,
                            r.severity,
                            r.symbol.file.display(),
                            r.symbol.name
                        )
                    })
                    .collect();
                cache
                    .entries
                    .insert(pred.query.clone(), CachedResult::Impact(impact));
            }
            PredictedQuery::Callees(name) => {
                let callees: Vec<String> = index
                    .graph
                    .callees_of(name)
                    .into_iter()
                    .map(|s| format!("{}:{}", s.file.display(), s.name))
                    .collect();
                cache
                    .entries
                    .insert(pred.query.clone(), CachedResult::Callers(callees));
            }
        }
    }
}

/// Check if a string looks like a symbol name (identifier-like).
fn is_likely_symbol_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() >= 3
        && !s.contains(' ')
        && !s.contains('.')
        && !s.contains('*')
        && !s.contains('|')
        && !s.contains('[')
        && s.chars().all(|c| c.is_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn test_predict_callers_for_symbol() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("auth.rs"),
            "fn authenticate() { check() }\nfn check() {}\n",
        )
        .unwrap();

        let index = Index::build(dir.path()).unwrap();
        let results = index.search("authenticate").unwrap();
        let predictions = predict("authenticate", &results, &index);

        // Should predict callers with high confidence
        let has_callers = predictions
            .iter()
            .any(|p| matches!(&p.query, PredictedQuery::Callers(n) if n == "authenticate"));
        assert!(has_callers, "Should predict callers for symbol search");
    }

    #[test]
    fn test_predict_source_for_test() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.rs"), "fn test_authenticate() {}\n").unwrap();

        let index = Index::build(dir.path()).unwrap();
        let results = index.search("test_authenticate").unwrap();
        let predictions = predict("test_authenticate", &results, &index);

        // Should predict searching for "authenticate" (the source)
        let has_source = predictions
            .iter()
            .any(|p| matches!(&p.query, PredictedQuery::Search(n) if n == "authenticate"));
        assert!(has_source, "Should predict source for test search");
    }

    #[test]
    fn test_predict_handler_for_error() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.rs"), "fn handle_error() {}\n").unwrap();

        let index = Index::build(dir.path()).unwrap();
        let results = index.search("error").unwrap();
        let predictions = predict("error", &results, &index);

        let has_handler = predictions
            .iter()
            .any(|p| matches!(&p.query, PredictedQuery::Search(n) if n == "handle"));
        assert!(has_handler, "Should predict handler for error search");
    }

    #[test]
    fn test_prefetch_cache() {
        let mut cache = PrefetchCache::new();
        let query = PredictedQuery::Callers("foo".to_string());

        cache.entries.insert(
            query.clone(),
            CachedResult::Callers(vec!["bar".to_string()]),
        );
        cache.total_predictions = 1;

        assert!(cache.get(&query).is_some());
        assert_eq!(cache.hits, 1);
        assert_eq!(cache.hit_rate(), 100.0);

        let miss = PredictedQuery::Callers("baz".to_string());
        assert!(cache.get(&miss).is_none());
    }

    #[test]
    fn test_is_symbol_name() {
        assert!(is_likely_symbol_name("authenticate"));
        assert!(is_likely_symbol_name("hash_password"));
        assert!(!is_likely_symbol_name("f.*o"));
        assert!(!is_likely_symbol_name("a|b"));
        assert!(!is_likely_symbol_name("ab")); // too short
    }
}
