/// Semantic compression: convert code into compact representations for AI agents.
///
/// Instead of returning raw source code, return structured metadata that
/// captures the information agents need in 5-10x fewer tokens.
///
/// Layers:
///   0 - File path + symbol name + kind (~15 tokens)
///   1 - Signature + calls + called_by + raises (~80-120 tokens)
///   2 - Full source code of enclosing function (~200-800 tokens)
use std::path::{Path, PathBuf};

use crate::graph::CodeGraph;
use crate::structure::{Symbol, SymbolKind};

/// A compressed semantic representation of a search result.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SemanticResult {
    pub file: String,
    pub name: String,
    pub kind: String,
    pub line_range: (usize, usize),
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calls: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub called_by: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Estimated token count of this result
    pub tokens: usize,
}

/// Output layer controlling how much detail to include.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// File path + symbol name + kind only
    L0,
    /// Signature + calls + called_by
    L1,
    /// Full source code
    L2,
}

impl Layer {
    pub fn from_u8(n: u8) -> Self {
        match n {
            0 => Layer::L0,
            1 => Layer::L1,
            _ => Layer::L2,
        }
    }
}

/// Compress a symbol into a semantic result at the given layer.
pub fn compress(
    symbol: &Symbol,
    content: &[u8],
    file: &Path,
    layer: Layer,
    graph: &CodeGraph,
) -> SemanticResult {
    let kind_str = format!("{}", symbol.kind);
    let body_bytes = &content[symbol.byte_range.0..symbol.byte_range.1];
    let body_text = String::from_utf8_lossy(body_bytes);

    match layer {
        Layer::L0 => {
            let tokens = estimate_tokens(&format!(
                "{}:{}:{} {}",
                file.display(),
                symbol.line_range.0,
                kind_str,
                symbol.name
            ));
            SemanticResult {
                file: file.display().to_string(),
                name: symbol.name.clone(),
                kind: kind_str,
                line_range: symbol.line_range,
                signature: None,
                calls: None,
                called_by: None,
                body: None,
                tokens,
            }
        }
        Layer::L1 => {
            let sig = extract_signature(&body_text, symbol.kind);
            let calls: Vec<String> = graph
                .callees_of(&symbol.name)
                .into_iter()
                .map(|s| s.name.clone())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();
            let called_by: Vec<String> = graph
                .callers_of(&symbol.name)
                .into_iter()
                .map(|s| s.name.clone())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();

            let token_text = format!(
                "{}:{}:{} {} sig:{} calls:{:?} called_by:{:?}",
                file.display(),
                symbol.line_range.0,
                kind_str,
                symbol.name,
                sig,
                calls,
                called_by,
            );

            let tokens = estimate_tokens(&token_text);

            SemanticResult {
                file: file.display().to_string(),
                name: symbol.name.clone(),
                kind: kind_str,
                line_range: symbol.line_range,
                signature: Some(sig),
                calls: if calls.is_empty() { None } else { Some(calls) },
                called_by: if called_by.is_empty() {
                    None
                } else {
                    Some(called_by)
                },
                body: None,
                tokens,
            }
        }
        Layer::L2 => {
            let tokens = estimate_tokens(&body_text);
            SemanticResult {
                file: file.display().to_string(),
                name: symbol.name.clone(),
                kind: kind_str,
                line_range: symbol.line_range,
                signature: None,
                calls: None,
                called_by: None,
                body: Some(body_text.into_owned()),
                tokens,
            }
        }
    }
}

/// Select results that fit within a token budget, preferring higher-ranked results.
/// Returns results and the total tokens consumed.
pub fn fit_budget(results: &[SemanticResult], budget: usize) -> (Vec<&SemanticResult>, usize) {
    let mut selected = Vec::new();
    let mut total = 0;

    for r in results {
        if total + r.tokens <= budget {
            selected.push(r);
            total += r.tokens;
        } else if selected.is_empty() {
            // Always include at least one result, even if over budget
            selected.push(r);
            total += r.tokens;
            break;
        } else {
            break;
        }
    }

    (selected, total)
}

/// Upgrade the highest-ranked result to a deeper layer if budget allows.
/// Takes L1 results and upgrades the first one to L2 if it fits.
pub fn upgrade_top_result(
    results: &mut Vec<SemanticResult>,
    symbols: &[(Symbol, Vec<u8>, PathBuf)],
    graph: &CodeGraph,
    budget: usize,
) {
    if results.is_empty() || symbols.is_empty() {
        return;
    }

    let current_total: usize = results.iter().map(|r| r.tokens).sum();
    if current_total >= budget {
        return;
    }

    let remaining = budget - current_total;

    // Try to upgrade first result to L2
    let (sym, content, file) = &symbols[0];
    let l2 = compress(sym, content, file, Layer::L2, graph);
    let extra_tokens = l2.tokens.saturating_sub(results[0].tokens);

    if extra_tokens <= remaining {
        results[0] = l2;
    }
}

/// Extract the function/class signature (first line up to the opening brace/colon).
fn extract_signature(body: &str, kind: SymbolKind) -> String {
    let first_line = body.lines().next().unwrap_or("");

    match kind {
        SymbolKind::Function | SymbolKind::Method => {
            // Take everything up to and including the opening brace or colon
            if let Some(pos) = first_line.find('{') {
                first_line[..pos].trim().to_string()
            } else if first_line.ends_with(':') {
                // Python-style
                first_line.trim().to_string()
            } else {
                first_line.trim().to_string()
            }
        }
        SymbolKind::Struct | SymbolKind::Class | SymbolKind::Trait | SymbolKind::Interface => {
            if let Some(pos) = first_line.find('{') {
                first_line[..pos].trim().to_string()
            } else if first_line.ends_with(':') {
                first_line.trim().to_string()
            } else {
                first_line.trim().to_string()
            }
        }
        _ => first_line.trim().to_string(),
    }
}

/// Rough token estimation: ~4 characters per token (GPT/Claude average).
fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4) // ceiling division
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::CodeGraph;

    fn make_symbol(
        name: &str,
        kind: SymbolKind,
        start: usize,
        end: usize,
        line_start: usize,
        line_end: usize,
    ) -> Symbol {
        Symbol {
            name: name.to_string(),
            kind,
            byte_range: (start, end),
            line_range: (line_start, line_end),
        }
    }

    #[test]
    fn test_layer0_minimal() {
        let code = b"fn hello() {\n    42\n}\n";
        let sym = make_symbol("hello", SymbolKind::Function, 0, 21, 1, 3);
        let graph = CodeGraph::build(&[]);

        let result = compress(&sym, code, Path::new("test.rs"), Layer::L0, &graph);
        assert_eq!(result.name, "hello");
        assert_eq!(result.kind, "function");
        assert!(result.signature.is_none());
        assert!(result.body.is_none());
        assert!(result.tokens < 30);
    }

    #[test]
    fn test_layer1_with_signature() {
        let code = b"fn authenticate(user: &str, pass: &str) -> bool {\n    check(user)\n}\n";
        let sym = make_symbol("authenticate", SymbolKind::Function, 0, 68, 1, 3);
        let graph = CodeGraph::build(&[]);

        let result = compress(&sym, code, Path::new("auth.rs"), Layer::L1, &graph);
        assert_eq!(result.name, "authenticate");
        assert!(result.signature.is_some());
        let sig = result.signature.unwrap();
        assert!(sig.contains("fn authenticate"));
        assert!(sig.contains("bool"));
        assert!(result.body.is_none());
    }

    #[test]
    fn test_layer2_full_body() {
        let code = b"fn hello() {\n    println!(\"hi\");\n}\n";
        let sym = make_symbol("hello", SymbolKind::Function, 0, 34, 1, 3);
        let graph = CodeGraph::build(&[]);

        let result = compress(&sym, code, Path::new("test.rs"), Layer::L2, &graph);
        assert!(result.body.is_some());
        assert!(result.body.unwrap().contains("println!"));
    }

    #[test]
    fn test_layer1_fewer_tokens_than_layer2() {
        let code = b"fn process(data: Vec<u8>) -> Result<(), Error> {\n    let a = validate(data);\n    let b = transform(a);\n    let c = save(b);\n    Ok(c)\n}\n";
        let sym = make_symbol("process", SymbolKind::Function, 0, code.len(), 1, 6);
        let graph = CodeGraph::build(&[]);

        let l1 = compress(&sym, code, Path::new("svc.rs"), Layer::L1, &graph);
        let l2 = compress(&sym, code, Path::new("svc.rs"), Layer::L2, &graph);

        assert!(
            l1.tokens < l2.tokens,
            "L1 ({}) should use fewer tokens than L2 ({})",
            l1.tokens,
            l2.tokens
        );
    }

    #[test]
    fn test_fit_budget() {
        let results = vec![
            SemanticResult {
                file: "a.rs".into(),
                name: "a".into(),
                kind: "function".into(),
                line_range: (1, 5),
                signature: Some("fn a()".into()),
                calls: None,
                called_by: None,
                body: None,
                tokens: 50,
            },
            SemanticResult {
                file: "b.rs".into(),
                name: "b".into(),
                kind: "function".into(),
                line_range: (1, 10),
                signature: Some("fn b()".into()),
                calls: None,
                called_by: None,
                body: None,
                tokens: 80,
            },
            SemanticResult {
                file: "c.rs".into(),
                name: "c".into(),
                kind: "function".into(),
                line_range: (1, 20),
                signature: Some("fn c()".into()),
                calls: None,
                called_by: None,
                body: None,
                tokens: 200,
            },
        ];

        // Budget 150: fits a(50) + b(80) = 130, not c(200)
        let (selected, total) = fit_budget(&results, 150);
        assert_eq!(selected.len(), 2);
        assert_eq!(total, 130);

        // Budget 50: fits only a
        let (selected, total) = fit_budget(&results, 50);
        assert_eq!(selected.len(), 1);
        assert_eq!(total, 50);

        // Budget 10: still returns at least one result
        let (selected, _) = fit_budget(&results, 10);
        assert_eq!(selected.len(), 1);
    }

    #[test]
    fn test_python_signature() {
        let sig = extract_signature(
            "def authenticate(username, password):",
            SymbolKind::Function,
        );
        assert_eq!(sig, "def authenticate(username, password):");
    }

    #[test]
    fn test_rust_signature() {
        let sig = extract_signature(
            "fn search(&self, pattern: &str) -> Result<Vec<Match>> {",
            SymbolKind::Function,
        );
        assert_eq!(sig, "fn search(&self, pattern: &str) -> Result<Vec<Match>>");
    }
}
