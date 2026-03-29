/// Structural code analysis via tree-sitter.
///
/// Parses source files into ASTs and extracts symbol boundaries (functions, classes,
/// methods, structs, traits, etc). Used to expand raw line matches into complete
/// syntactic units -- the core of the "context gain" contribution.
use std::path::Path;

use tree_sitter::{Language, Parser, Tree};

/// A code symbol extracted from the AST.
#[derive(Debug, Clone)]
pub struct Symbol {
    /// The kind of symbol (function, class, method, struct, etc.)
    pub kind: SymbolKind,
    /// The name of the symbol
    pub name: String,
    /// Byte range in the source file [start, end)
    pub byte_range: (usize, usize),
    /// Line range in the source file [start, end] (1-indexed)
    pub line_range: (usize, usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Struct,
    Trait,
    Interface,
    Enum,
    Module,
    Impl,
}

impl std::fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SymbolKind::Function => write!(f, "function"),
            SymbolKind::Method => write!(f, "method"),
            SymbolKind::Class => write!(f, "class"),
            SymbolKind::Struct => write!(f, "struct"),
            SymbolKind::Trait => write!(f, "trait"),
            SymbolKind::Interface => write!(f, "interface"),
            SymbolKind::Enum => write!(f, "enum"),
            SymbolKind::Module => write!(f, "module"),
            SymbolKind::Impl => write!(f, "impl"),
        }
    }
}

/// Supported languages and their tree-sitter configurations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Go,
    Java,
    C,
    Cpp,
}

impl Lang {
    /// Detect language from file extension.
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?;
        match ext {
            "rs" => Some(Lang::Rust),
            "py" | "pyi" => Some(Lang::Python),
            "js" | "jsx" | "mjs" | "cjs" => Some(Lang::JavaScript),
            "ts" | "tsx" | "mts" | "cts" => Some(Lang::TypeScript),
            "go" => Some(Lang::Go),
            "java" => Some(Lang::Java),
            "c" | "h" => Some(Lang::C),
            "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => Some(Lang::Cpp),
            _ => None,
        }
    }

    /// Get the tree-sitter Language for this lang.
    pub fn ts_language(&self) -> Language {
        match self {
            Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
            Lang::Python => tree_sitter_python::LANGUAGE.into(),
            Lang::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Lang::Go => tree_sitter_go::LANGUAGE.into(),
            Lang::Java => tree_sitter_java::LANGUAGE.into(),
            Lang::C => tree_sitter_c::LANGUAGE.into(),
            Lang::Cpp => tree_sitter_cpp::LANGUAGE.into(),
        }
    }

    /// Node types that represent symbol definitions for this language.
    /// Returns (node_type, symbol_kind, name_field).
    fn symbol_node_types(&self) -> &[(&str, SymbolKind, &str)] {
        match self {
            Lang::Rust => &[
                ("function_item", SymbolKind::Function, "name"),
                ("struct_item", SymbolKind::Struct, "name"),
                ("enum_item", SymbolKind::Enum, "name"),
                ("trait_item", SymbolKind::Trait, "name"),
                ("impl_item", SymbolKind::Impl, "type"),
                ("mod_item", SymbolKind::Module, "name"),
            ],
            Lang::Python => &[
                ("function_definition", SymbolKind::Function, "name"),
                ("class_definition", SymbolKind::Class, "name"),
            ],
            Lang::JavaScript | Lang::TypeScript => &[
                ("function_declaration", SymbolKind::Function, "name"),
                ("class_declaration", SymbolKind::Class, "name"),
                ("method_definition", SymbolKind::Method, "name"),
                ("arrow_function", SymbolKind::Function, ""),
                ("lexical_declaration", SymbolKind::Function, ""),
            ],
            Lang::Go => &[
                ("function_declaration", SymbolKind::Function, "name"),
                ("method_declaration", SymbolKind::Method, "name"),
                ("type_declaration", SymbolKind::Struct, ""),
            ],
            Lang::Java => &[
                ("method_declaration", SymbolKind::Method, "name"),
                ("class_declaration", SymbolKind::Class, "name"),
                ("interface_declaration", SymbolKind::Interface, "name"),
                ("enum_declaration", SymbolKind::Enum, "name"),
            ],
            Lang::C => &[
                ("function_definition", SymbolKind::Function, "declarator"),
                ("struct_specifier", SymbolKind::Struct, "name"),
                ("enum_specifier", SymbolKind::Enum, "name"),
            ],
            Lang::Cpp => &[
                ("function_definition", SymbolKind::Function, "declarator"),
                ("class_specifier", SymbolKind::Class, "name"),
                ("struct_specifier", SymbolKind::Struct, "name"),
                ("enum_specifier", SymbolKind::Enum, "name"),
            ],
        }
    }
}

/// Parse a source file and extract all symbols.
pub fn parse_symbols(content: &[u8], lang: Lang) -> Vec<Symbol> {
    let mut parser = Parser::new();
    parser
        .set_language(&lang.ts_language())
        .expect("failed to set language");

    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => return Vec::new(),
    };

    let mut symbols = Vec::new();
    let node_types = lang.symbol_node_types();

    collect_symbols(&tree, content, node_types, &mut symbols);
    symbols
}

/// Recursively walk the AST and collect symbols.
fn collect_symbols(
    tree: &Tree,
    source: &[u8],
    node_types: &[(&str, SymbolKind, &str)],
    symbols: &mut Vec<Symbol>,
) {
    let mut cursor = tree.walk();
    walk_node(&mut cursor, source, node_types, symbols);
}

fn walk_node(
    cursor: &mut tree_sitter::TreeCursor,
    source: &[u8],
    node_types: &[(&str, SymbolKind, &str)],
    symbols: &mut Vec<Symbol>,
) {
    let node = cursor.node();
    let node_type = node.kind();

    // Check if this node is a symbol definition
    for &(type_name, kind, name_field) in node_types {
        if node_type == type_name {
            let name = if !name_field.is_empty() {
                node.child_by_field_name(name_field)
                    .map(|n| {
                        // For nested declarators (C/C++ function pointers), get the innermost name
                        extract_name(n, source)
                    })
                    .unwrap_or_else(|| "<anonymous>".to_string())
            } else {
                // Try to extract name from variable declarator parent or first identifier child
                extract_first_identifier(&node, source).unwrap_or_else(|| "<anonymous>".to_string())
            };

            // Validate: skip if node is suspiciously large (likely a parse error)
            let line_count = node.end_position().row - node.start_position().row + 1;
            if line_count > 500 {
                // Likely a parse error -- skip this symbol
                break;
            }

            symbols.push(Symbol {
                kind,
                name,
                byte_range: (node.start_byte(), node.end_byte()),
                line_range: (node.start_position().row + 1, node.end_position().row + 1),
            });
            break;
        }
    }

    // Recurse into children
    if cursor.goto_first_child() {
        loop {
            walk_node(cursor, source, node_types, symbols);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}

/// Extract the name text from a name node (handles nested declarators).
fn extract_name(node: tree_sitter::Node, source: &[u8]) -> String {
    // For simple identifiers, just return the text
    if node.kind() == "identifier" || node.kind() == "type_identifier" {
        return node_text(node, source);
    }

    // For nested nodes (e.g. C/C++ function_declarator), find the first identifier
    find_first_identifier_text(&node, source).unwrap_or_else(|| node_text(node, source))
}

/// Find the text of the first identifier descendant of a node.
fn find_first_identifier_text(node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
    if node.kind() == "identifier" || node.kind() == "type_identifier" {
        return Some(node_text(*node, source));
    }

    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if let Some(text) = find_first_identifier_text(&child, source) {
                return Some(text);
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }

    None
}

/// Extract the first identifier from a node's children.
fn extract_first_identifier(node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
    find_first_identifier_text(node, source)
}

/// Get the text content of a node.
fn node_text(node: tree_sitter::Node, source: &[u8]) -> String {
    let bytes = &source[node.start_byte()..node.end_byte()];
    String::from_utf8_lossy(bytes).into_owned()
}

/// Given a byte offset in a file, find the smallest enclosing symbol.
/// Returns the symbol and the full source text of that symbol.
pub fn enclosing_symbol(symbols: &[Symbol], byte_offset: usize) -> Option<&Symbol> {
    let mut best: Option<&Symbol> = None;

    for sym in symbols {
        if byte_offset >= sym.byte_range.0 && byte_offset < sym.byte_range.1 {
            match best {
                None => best = Some(sym),
                Some(current) => {
                    // Prefer the smaller (more specific) enclosing symbol
                    let current_size = current.byte_range.1 - current.byte_range.0;
                    let sym_size = sym.byte_range.1 - sym.byte_range.0;
                    if sym_size < current_size {
                        best = Some(sym);
                    }
                }
            }
        }
    }

    best
}

/// Extract the source text for a symbol from the file content.
pub fn symbol_text<'a>(symbol: &Symbol, content: &'a [u8]) -> &'a [u8] {
    &content[symbol.byte_range.0..symbol.byte_range.1]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rust_functions() {
        let code = br#"
fn hello() {
    println!("hello");
}

fn world(x: i32) -> i32 {
    x + 1
}

struct Foo {
    bar: i32,
}
"#;

        let symbols = parse_symbols(code, Lang::Rust);

        let fns: Vec<_> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Function)
            .collect();
        assert_eq!(fns.len(), 2);
        assert_eq!(fns[0].name, "hello");
        assert_eq!(fns[1].name, "world");

        let structs: Vec<_> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Struct)
            .collect();
        assert_eq!(structs.len(), 1);
        assert_eq!(structs[0].name, "Foo");
    }

    #[test]
    fn test_parse_python_functions() {
        let code = br#"
def authenticate(username, password):
    if check_password(username, password):
        return create_session(username)
    return None

class UserService:
    def get_user(self, user_id):
        return self.db.find(user_id)
"#;

        let symbols = parse_symbols(code, Lang::Python);

        let fns: Vec<_> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Function)
            .collect();
        assert_eq!(fns.len(), 2); // authenticate + get_user
        assert_eq!(fns[0].name, "authenticate");
        assert_eq!(fns[1].name, "get_user");

        let classes: Vec<_> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Class)
            .collect();
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].name, "UserService");
    }

    #[test]
    fn test_parse_javascript_functions() {
        let code = br#"
function handleRequest(req, res) {
    res.send("ok");
}

class Router {
    get(path, handler) {
        this.routes.push({ path, handler });
    }
}
"#;

        let symbols = parse_symbols(code, Lang::JavaScript);

        let fns: Vec<_> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Function)
            .collect();
        assert!(fns.len() >= 1);
        assert_eq!(fns[0].name, "handleRequest");

        let classes: Vec<_> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Class)
            .collect();
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].name, "Router");
    }

    #[test]
    fn test_enclosing_symbol() {
        let code = br#"
fn outer() {
    let x = 1;
    let y = 2;
}

fn inner() {
    let z = 3;
}
"#;

        let symbols = parse_symbols(code, Lang::Rust);

        // Byte offset inside "outer" function
        let offset = code.windows(5).position(|w| w == b"let x").unwrap();
        let enclosing = enclosing_symbol(&symbols, offset);
        assert!(enclosing.is_some());
        assert_eq!(enclosing.unwrap().name, "outer");

        // Byte offset inside "inner" function
        let offset = code.windows(5).position(|w| w == b"let z").unwrap();
        let enclosing = enclosing_symbol(&symbols, offset);
        assert!(enclosing.is_some());
        assert_eq!(enclosing.unwrap().name, "inner");
    }

    #[test]
    fn test_symbol_text() {
        let code = br#"
fn greet(name: &str) {
    println!("Hello, {}", name);
}
"#;

        let symbols = parse_symbols(code, Lang::Rust);
        assert_eq!(symbols.len(), 1);

        let text = symbol_text(&symbols[0], code);
        let text_str = std::str::from_utf8(text).unwrap();
        assert!(text_str.contains("fn greet"));
        assert!(text_str.contains("println!"));
    }

    #[test]
    fn test_lang_detection() {
        assert_eq!(Lang::from_path(Path::new("foo.rs")), Some(Lang::Rust));
        assert_eq!(Lang::from_path(Path::new("bar.py")), Some(Lang::Python));
        assert_eq!(Lang::from_path(Path::new("baz.ts")), Some(Lang::TypeScript));
        assert_eq!(Lang::from_path(Path::new("qux.go")), Some(Lang::Go));
        assert_eq!(Lang::from_path(Path::new("main.java")), Some(Lang::Java));
        assert_eq!(Lang::from_path(Path::new("data.csv")), None);
    }

    #[test]
    fn test_go_functions() {
        let code = br#"
package main

func main() {
    fmt.Println("hello")
}

func (s *Server) handleRequest(w http.ResponseWriter, r *http.Request) {
    w.Write([]byte("ok"))
}
"#;

        let symbols = parse_symbols(code, Lang::Go);
        let fns: Vec<_> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Function || s.kind == SymbolKind::Method)
            .collect();
        assert!(fns.len() >= 2);
    }
}
