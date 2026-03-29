/// Trigram extraction and regex-to-trigram decomposition.
///
/// A trigram is three consecutive bytes packed into a u32.
/// We use trigrams as index keys to map to posting lists of file IDs.
pub type Trigram = u32;

/// Pack three bytes into a trigram.
#[inline]
pub fn pack(a: u8, b: u8, c: u8) -> Trigram {
    (a as u32) << 16 | (b as u32) << 8 | c as u32
}

/// Unpack a trigram into three bytes.
#[inline]
pub fn unpack(t: Trigram) -> (u8, u8, u8) {
    ((t >> 16) as u8, (t >> 8) as u8, t as u8)
}

/// Extract all unique trigrams from a byte slice.
/// Returns a sorted, deduplicated vec.
pub fn extract(content: &[u8]) -> Vec<Trigram> {
    if content.len() < 3 {
        return Vec::new();
    }

    let mut trigrams: Vec<Trigram> = Vec::with_capacity(content.len());

    for window in content.windows(3) {
        trigrams.push(pack(window[0], window[1], window[2]));
    }

    trigrams.sort_unstable();
    trigrams.dedup();
    trigrams
}

/// Returns true if the content appears to be a binary file.
/// Heuristic: any null byte in the first 8KB.
pub fn is_binary(content: &[u8]) -> bool {
    let check_len = content.len().min(8192);
    content[..check_len].contains(&0)
}

/// Boolean expression over trigrams, used for query planning.
#[derive(Debug, Clone)]
pub enum TrigramQuery {
    /// All trigrams must be present (intersection).
    And(Vec<TrigramQuery>),
    /// At least one branch must match (union).
    Or(Vec<TrigramQuery>),
    /// A single trigram that must be present.
    Literal(Trigram),
    /// No trigram constraint -- must scan all files.
    All,
}

impl TrigramQuery {
    /// Simplify the query tree: flatten nested And/Or, remove All from And, etc.
    pub fn simplify(self) -> Self {
        match self {
            TrigramQuery::And(children) => {
                let mut simplified: Vec<TrigramQuery> = Vec::new();
                for child in children {
                    let child = child.simplify();
                    match child {
                        TrigramQuery::All => {} // AND with All is a no-op
                        TrigramQuery::And(inner) => simplified.extend(inner),
                        other => simplified.push(other),
                    }
                }
                match simplified.len() {
                    0 => TrigramQuery::All,
                    1 => simplified.into_iter().next().unwrap(),
                    _ => TrigramQuery::And(simplified),
                }
            }
            TrigramQuery::Or(children) => {
                let mut simplified: Vec<TrigramQuery> = Vec::new();
                for child in children {
                    let child = child.simplify();
                    match child {
                        TrigramQuery::All => return TrigramQuery::All, // OR with All = All
                        TrigramQuery::Or(inner) => simplified.extend(inner),
                        other => simplified.push(other),
                    }
                }
                match simplified.len() {
                    0 => TrigramQuery::All,
                    1 => simplified.into_iter().next().unwrap(),
                    _ => TrigramQuery::Or(simplified),
                }
            }
            other => other,
        }
    }
}

/// Extract trigrams from a literal string and AND them together.
pub fn trigrams_from_literal(s: &[u8]) -> TrigramQuery {
    if s.len() < 3 {
        return TrigramQuery::All;
    }

    let trigrams: Vec<TrigramQuery> = s
        .windows(3)
        .map(|w| TrigramQuery::Literal(pack(w[0], w[1], w[2])))
        .collect();

    TrigramQuery::And(trigrams).simplify()
}

/// Decompose a regex pattern into a TrigramQuery using regex-syntax AST.
pub fn trigrams_from_regex(pattern: &str) -> TrigramQuery {
    use regex_syntax::hir::{Hir, HirKind, Literal};

    let hir = match regex_syntax::parse(pattern) {
        Ok(hir) => hir,
        Err(_) => return TrigramQuery::All,
    };

    fn walk(hir: &Hir) -> TrigramQuery {
        match hir.kind() {
            HirKind::Literal(Literal(bytes)) => trigrams_from_literal(bytes),
            HirKind::Concat(subs) => {
                // Collect consecutive literal bytes, then extract trigrams
                let mut all_bytes: Vec<u8> = Vec::new();
                let mut parts: Vec<TrigramQuery> = Vec::new();

                for sub in subs {
                    if let HirKind::Literal(Literal(bytes)) = sub.kind() {
                        all_bytes.extend_from_slice(bytes);
                    } else {
                        // Flush accumulated literal bytes
                        if !all_bytes.is_empty() {
                            parts.push(trigrams_from_literal(&all_bytes));
                            all_bytes.clear();
                        }
                        parts.push(walk(sub));
                    }
                }

                if !all_bytes.is_empty() {
                    parts.push(trigrams_from_literal(&all_bytes));
                }

                TrigramQuery::And(parts).simplify()
            }
            HirKind::Alternation(subs) => {
                let parts: Vec<TrigramQuery> = subs.iter().map(walk).collect();
                TrigramQuery::Or(parts).simplify()
            }
            HirKind::Capture(cap) => walk(&cap.sub),
            // Repetition, class, look-around, empty -- no trigram constraint
            _ => TrigramQuery::All,
        }
    }

    walk(&hir).simplify()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pack_unpack() {
        let t = pack(b'a', b'b', b'c');
        assert_eq!(unpack(t), (b'a', b'b', b'c'));
    }

    #[test]
    fn test_extract_basic() {
        let content = b"hello";
        let trigrams = extract(content);
        assert!(trigrams.contains(&pack(b'h', b'e', b'l')));
        assert!(trigrams.contains(&pack(b'e', b'l', b'l')));
        assert!(trigrams.contains(&pack(b'l', b'l', b'o')));
        assert_eq!(trigrams.len(), 3);
    }

    #[test]
    fn test_extract_short() {
        assert!(extract(b"ab").is_empty());
        assert!(extract(b"").is_empty());
    }

    #[test]
    fn test_extract_dedup() {
        let content = b"aaaa"; // trigrams: aaa, aaa -> deduplicated to 1
        let trigrams = extract(content);
        assert_eq!(trigrams.len(), 1);
    }

    #[test]
    fn test_is_binary() {
        assert!(is_binary(b"hello\x00world"));
        assert!(!is_binary(b"hello world"));
    }

    #[test]
    fn test_trigrams_from_literal() {
        match trigrams_from_literal(b"auth") {
            TrigramQuery::And(parts) => {
                assert_eq!(parts.len(), 2); // "aut" and "uth"
            }
            _ => panic!("expected And"),
        }
    }

    #[test]
    fn test_trigrams_from_literal_short() {
        assert!(matches!(trigrams_from_literal(b"ab"), TrigramQuery::All));
    }

    #[test]
    fn test_trigrams_from_regex_literal() {
        let q = trigrams_from_regex("authenticate");
        // Should produce AND of 10 trigrams
        match q {
            TrigramQuery::And(parts) => {
                assert!(parts.len() >= 3);
            }
            _ => panic!("expected And for literal pattern"),
        }
    }

    #[test]
    fn test_trigrams_from_regex_alternation() {
        let q = trigrams_from_regex("foo|bar");
        assert!(matches!(q, TrigramQuery::Or(_)));
    }

    #[test]
    fn test_trigrams_from_regex_wildcard() {
        let q = trigrams_from_regex("f.*o");
        // f and o are too short for trigrams, should be All
        assert!(matches!(q, TrigramQuery::All));
    }
}
