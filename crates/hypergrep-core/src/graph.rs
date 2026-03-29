/// Code graph: call edges, import edges, and impact analysis.
///
/// Built from tree-sitter ASTs during indexing. Enables queries like:
/// - "What functions call authenticate()?"
/// - "What breaks if I change this function's signature?"
/// - "What modules import this module?"
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use tree_sitter::Parser;

use crate::structure::Lang;

/// A unique identifier for a symbol in the codebase.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct SymbolId {
    /// File path (relative or absolute)
    pub file: PathBuf,
    /// Symbol name
    pub name: String,
}

impl std::fmt::Display for SymbolId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.file.display(), self.name)
    }
}

/// An edge in the code graph.
#[derive(Debug, Clone)]
pub struct Edge {
    pub from: SymbolId,
    pub to: SymbolId,
    pub kind: EdgeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// Function A calls function B
    Calls,
    /// File A imports from file/module B
    Imports,
}

/// Impact depth classification.
#[derive(Debug, Clone)]
pub struct ImpactResult {
    pub symbol: SymbolId,
    pub depth: usize,
    pub severity: ImpactSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImpactSeverity {
    /// Direct caller -- will break
    WillBreak,
    /// Caller of caller -- may break
    MayBreak,
    /// Transitive -- review
    Review,
}

impl std::fmt::Display for ImpactSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImpactSeverity::WillBreak => write!(f, "WILL BREAK"),
            ImpactSeverity::MayBreak => write!(f, "MAY BREAK"),
            ImpactSeverity::Review => write!(f, "REVIEW"),
        }
    }
}

/// The code graph: nodes are symbols, edges are call/import relationships.
pub struct CodeGraph {
    /// All edges in the graph
    edges: Vec<Edge>,
    /// Forward index: symbol -> symbols it calls
    callees: HashMap<SymbolId, Vec<SymbolId>>,
    /// Reverse index: symbol -> symbols that call it
    callers: HashMap<SymbolId, Vec<SymbolId>>,
    /// Import edges: file -> files it imports from
    #[allow(dead_code)]
    imports: HashMap<PathBuf, Vec<PathBuf>>,
    /// Reverse imports: file -> files that import it
    imported_by: HashMap<PathBuf, Vec<PathBuf>>,
}

impl CodeGraph {
    /// Build a code graph from indexed files.
    pub fn build(files: &[(PathBuf, &[u8])]) -> Self {
        let mut edges = Vec::new();
        let mut callees: HashMap<SymbolId, Vec<SymbolId>> = HashMap::new();
        let mut callers: HashMap<SymbolId, Vec<SymbolId>> = HashMap::new();
        let mut imports: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
        let mut imported_by: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();

        // Collect all defined symbol names per file for resolution
        let mut defined_symbols: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for (path, content) in files {
            if let Some(lang) = Lang::from_path(path) {
                let syms = crate::structure::parse_symbols(content, lang);
                for sym in &syms {
                    defined_symbols
                        .entry(sym.name.clone())
                        .or_default()
                        .push(path.clone());
                }
            }
        }

        // Extract call edges and imports from each file
        for (path, content) in files {
            let lang = match Lang::from_path(path) {
                Some(l) => l,
                None => continue,
            };

            let file_symbols = crate::structure::parse_symbols(content, lang);
            let call_refs = extract_calls(content, lang);
            let import_refs = extract_imports(content, lang);

            // For each call reference, find which defined symbol it's inside,
            // and resolve the callee to a defined symbol
            for call in &call_refs {
                // Find enclosing symbol for this call
                let caller_sym = file_symbols
                    .iter()
                    .filter(|s| {
                        call.byte_offset >= s.byte_range.0 && call.byte_offset < s.byte_range.1
                    })
                    .min_by_key(|s| s.byte_range.1 - s.byte_range.0);

                let caller_name = caller_sym
                    .map(|s| s.name.clone())
                    .unwrap_or_else(|| "<module>".to_string());

                let from = SymbolId {
                    file: path.clone(),
                    name: caller_name,
                };

                // Resolve callee: look for a defined symbol with this name
                if let Some(target_files) = defined_symbols.get(&call.name) {
                    for target_file in target_files {
                        let to = SymbolId {
                            file: target_file.clone(),
                            name: call.name.clone(),
                        };

                        edges.push(Edge {
                            from: from.clone(),
                            to: to.clone(),
                            kind: EdgeKind::Calls,
                        });

                        callees.entry(from.clone()).or_default().push(to.clone());
                        callers.entry(to).or_default().push(from.clone());
                    }
                }
            }

            // Import edges
            for imp in &import_refs {
                imports.entry(path.clone()).or_default().push(imp.clone());
                imported_by
                    .entry(imp.clone())
                    .or_default()
                    .push(path.clone());
            }
        }

        CodeGraph {
            edges,
            callees,
            callers,
            imports,
            imported_by,
        }
    }

    /// Find all symbols that call the given symbol (reverse call graph).
    pub fn callers_of(&self, name: &str) -> Vec<&SymbolId> {
        let mut result = Vec::new();
        for (sym, caller_list) in &self.callers {
            if sym.name == name {
                result.extend(caller_list.iter());
            }
        }
        result
    }

    /// Find all symbols that the given symbol calls (forward call graph).
    pub fn callees_of(&self, name: &str) -> Vec<&SymbolId> {
        let mut result = Vec::new();
        for (sym, callee_list) in &self.callees {
            if sym.name == name {
                result.extend(callee_list.iter());
            }
        }
        result
    }

    /// Impact analysis: BFS upstream through call graph from a target symbol.
    /// Returns all symbols that would be affected if the target changes.
    pub fn impact(&self, name: &str, max_depth: usize) -> Vec<ImpactResult> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut results = Vec::new();

        // Find all SymbolIds matching this name
        for sym in self.callers.keys() {
            if sym.name == name {
                queue.push_back((sym.clone(), 0usize));
                visited.insert(sym.clone());
            }
        }

        // Also check symbols that have no callers but match the name
        for edge in &self.edges {
            if edge.to.name == name && !visited.contains(&edge.to) {
                queue.push_back((edge.to.clone(), 0));
                visited.insert(edge.to.clone());
            }
        }

        while let Some((current, depth)) = queue.pop_front() {
            if depth > 0 {
                let severity = match depth {
                    1 => ImpactSeverity::WillBreak,
                    2 => ImpactSeverity::MayBreak,
                    _ => ImpactSeverity::Review,
                };

                results.push(ImpactResult {
                    symbol: current.clone(),
                    depth,
                    severity,
                });
            }

            if depth < max_depth {
                // Find callers of current symbol
                if let Some(caller_list) = self.callers.get(&current) {
                    for caller in caller_list {
                        if !visited.contains(caller) {
                            visited.insert(caller.clone());
                            queue.push_back((caller.clone(), depth + 1));
                        }
                    }
                }
            }
        }

        // Sort by depth, then by file, then by name
        results.sort_by(|a, b| {
            a.depth
                .cmp(&b.depth)
                .then_with(|| a.symbol.file.cmp(&b.symbol.file))
                .then_with(|| a.symbol.name.cmp(&b.symbol.name))
        });

        results
    }

    /// Total number of edges in the graph.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Get files that import the given file.
    pub fn imported_by(&self, path: &Path) -> Vec<&PathBuf> {
        self.imported_by
            .get(path)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }
}

/// A call reference found in source code.
struct CallRef {
    name: String,
    byte_offset: usize,
}

/// Extract function/method call references from source code using tree-sitter.
fn extract_calls(content: &[u8], lang: Lang) -> Vec<CallRef> {
    let mut parser = Parser::new();
    parser
        .set_language(&lang.ts_language())
        .expect("failed to set language");

    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => return Vec::new(),
    };

    let mut calls = Vec::new();
    let call_node_types = call_expression_types(lang);

    collect_calls(
        &mut tree.walk(),
        content,
        &call_node_types,
        lang,
        &mut calls,
    );
    calls
}

/// Node types that represent function calls for each language.
fn call_expression_types(lang: Lang) -> Vec<&'static str> {
    match lang {
        Lang::Rust => vec!["call_expression", "macro_invocation"],
        Lang::Python => vec!["call"],
        Lang::JavaScript | Lang::TypeScript => vec!["call_expression"],
        Lang::Go => vec!["call_expression"],
        Lang::Java => vec!["method_invocation"],
        Lang::C | Lang::Cpp => vec!["call_expression"],
    }
}

fn collect_calls(
    cursor: &mut tree_sitter::TreeCursor,
    source: &[u8],
    call_types: &[&str],
    lang: Lang,
    calls: &mut Vec<CallRef>,
) {
    let node = cursor.node();

    if call_types.contains(&node.kind())
        && let Some(name) = extract_call_name(&node, source, lang)
    {
        calls.push(CallRef {
            name,
            byte_offset: node.start_byte(),
        });
    }

    if cursor.goto_first_child() {
        loop {
            collect_calls(cursor, source, call_types, lang, calls);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}

/// Extract the function name from a call expression node.
fn extract_call_name(node: &tree_sitter::Node, source: &[u8], lang: Lang) -> Option<String> {
    match lang {
        Lang::Rust => {
            // call_expression: function field is the callee
            if let Some(func) = node.child_by_field_name("function") {
                let text = node_text(func, source);
                // For method calls like foo.bar(), extract "bar"
                // For simple calls like bar(), extract "bar"
                Some(text.rsplit('.').next()?.to_string())
            } else if node.kind() == "macro_invocation" {
                node.child_by_field_name("macro")
                    .map(|n| node_text(n, source))
            } else {
                None
            }
        }
        Lang::Python => {
            if let Some(func) = node.child_by_field_name("function") {
                let text = node_text(func, source);
                Some(text.rsplit('.').next()?.to_string())
            } else {
                None
            }
        }
        Lang::JavaScript | Lang::TypeScript => {
            if let Some(func) = node.child_by_field_name("function") {
                let text = node_text(func, source);
                Some(text.rsplit('.').next()?.to_string())
            } else {
                None
            }
        }
        Lang::Go => {
            if let Some(func) = node.child_by_field_name("function") {
                let text = node_text(func, source);
                Some(text.rsplit('.').next()?.to_string())
            } else {
                None
            }
        }
        Lang::Java => node
            .child_by_field_name("name")
            .map(|name| node_text(name, source)),
        Lang::C | Lang::Cpp => {
            if let Some(func) = node.child_by_field_name("function") {
                let text = node_text(func, source);
                Some(text.rsplit("::").next()?.to_string())
            } else {
                None
            }
        }
    }
}

/// Extract import references from source code.
fn extract_imports(content: &[u8], lang: Lang) -> Vec<PathBuf> {
    let mut parser = Parser::new();
    parser
        .set_language(&lang.ts_language())
        .expect("failed to set language");

    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => return Vec::new(),
    };

    let mut imports = Vec::new();
    collect_imports(&mut tree.walk(), content, lang, &mut imports);
    imports
}

fn collect_imports(
    cursor: &mut tree_sitter::TreeCursor,
    source: &[u8],
    lang: Lang,
    imports: &mut Vec<PathBuf>,
) {
    let node = cursor.node();

    match lang {
        Lang::Python => {
            if node.kind() == "import_from_statement"
                && let Some(module) = node.child_by_field_name("module_name")
            {
                let text = node_text(module, source);
                imports.push(PathBuf::from(text.replace('.', "/") + ".py"));
            }
        }
        Lang::JavaScript | Lang::TypeScript => {
            if node.kind() == "import_statement"
                && let Some(src) = node.child_by_field_name("source")
            {
                let text = node_text(src, source);
                let clean = text.trim_matches(|c| c == '\'' || c == '"');
                imports.push(PathBuf::from(clean));
            }
        }
        Lang::Go => {
            if node.kind() == "import_spec"
                && let Some(path) = node.child_by_field_name("path")
            {
                let text = node_text(path, source);
                let clean = text.trim_matches('"');
                imports.push(PathBuf::from(clean));
            }
        }
        Lang::Rust => {
            if node.kind() == "use_declaration" {
                // Simplified: just record the use statement text
                let text = node_text(node, source);
                imports.push(PathBuf::from(text));
            }
        }
        _ => {}
    }

    if cursor.goto_first_child() {
        loop {
            collect_imports(cursor, source, lang, imports);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}

fn node_text(node: tree_sitter::Node, source: &[u8]) -> String {
    String::from_utf8_lossy(&source[node.start_byte()..node.end_byte()]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_call_extraction_rust() {
        let code = br#"
fn authenticate(user: &str) -> bool {
    let hashed = hash_password(user);
    check_db(hashed)
}

fn hash_password(input: &str) -> String {
    bcrypt::hash(input)
}

fn check_db(hash: String) -> bool {
    true
}
"#;

        let files = vec![(PathBuf::from("auth.rs"), code.as_slice())];
        let graph = CodeGraph::build(&files);

        // authenticate calls hash_password and check_db
        let callees = graph.callees_of("authenticate");
        let names: Vec<&str> = callees.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"hash_password"));
        assert!(names.contains(&"check_db"));

        // hash_password is called by authenticate
        let callers = graph.callers_of("hash_password");
        let names: Vec<&str> = callers.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"authenticate"));
    }

    #[test]
    fn test_call_extraction_python() {
        let code = br#"
def process(data):
    validated = validate(data)
    transformed = transform(validated)
    save(transformed)

def validate(data):
    return data

def transform(data):
    return data

def save(data):
    pass
"#;

        let files = vec![(PathBuf::from("service.py"), code.as_slice())];
        let graph = CodeGraph::build(&files);

        let callees = graph.callees_of("process");
        let names: Vec<&str> = callees.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"validate"));
        assert!(names.contains(&"transform"));
        assert!(names.contains(&"save"));
    }

    #[test]
    fn test_impact_analysis() {
        let code = br#"
fn handler() {
    authenticate()
}

fn authenticate() {
    hash_password()
}

fn hash_password() {
}

fn unrelated() {
}
"#;

        let files = vec![(PathBuf::from("auth.rs"), code.as_slice())];
        let graph = CodeGraph::build(&files);

        // If hash_password changes, what's affected?
        let impact = graph.impact("hash_password", 3);

        // Depth 1: authenticate (directly calls hash_password)
        let depth1: Vec<&str> = impact
            .iter()
            .filter(|r| r.depth == 1)
            .map(|r| r.symbol.name.as_str())
            .collect();
        assert!(depth1.contains(&"authenticate"));

        // Depth 2: handler (calls authenticate)
        let depth2: Vec<&str> = impact
            .iter()
            .filter(|r| r.depth == 2)
            .map(|r| r.symbol.name.as_str())
            .collect();
        assert!(depth2.contains(&"handler"));

        // unrelated should NOT appear
        let all_names: Vec<&str> = impact.iter().map(|r| r.symbol.name.as_str()).collect();
        assert!(!all_names.contains(&"unrelated"));
    }

    #[test]
    fn test_impact_severity() {
        let code = br#"
fn a() { b() }
fn b() { c() }
fn c() { d() }
fn d() {}
"#;

        let files = vec![(PathBuf::from("chain.rs"), code.as_slice())];
        let graph = CodeGraph::build(&files);

        let impact = graph.impact("d", 5);

        for r in &impact {
            match r.depth {
                1 => assert_eq!(r.severity, ImpactSeverity::WillBreak),
                2 => assert_eq!(r.severity, ImpactSeverity::MayBreak),
                _ => assert_eq!(r.severity, ImpactSeverity::Review),
            }
        }
    }

    #[test]
    fn test_cross_file_calls() {
        let auth_code = br#"
fn authenticate(user: &str) -> bool {
    validate(user)
}
"#;
        let validator_code = br#"
fn validate(input: &str) -> bool {
    true
}
"#;

        let files = vec![
            (PathBuf::from("auth.rs"), auth_code.as_slice()),
            (PathBuf::from("validator.rs"), validator_code.as_slice()),
        ];
        let graph = CodeGraph::build(&files);

        // validate is called by authenticate (cross-file)
        let callers = graph.callers_of("validate");
        assert!(!callers.is_empty());
        assert_eq!(callers[0].name, "authenticate");

        // Impact: if validate changes, authenticate breaks
        let impact = graph.impact("validate", 2);
        let names: Vec<&str> = impact.iter().map(|r| r.symbol.name.as_str()).collect();
        assert!(names.contains(&"authenticate"));
    }
}
