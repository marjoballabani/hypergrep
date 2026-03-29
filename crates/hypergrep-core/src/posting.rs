/// Posting list construction and intersection.
///
/// A posting list is a sorted array of document IDs (u32) associated with a trigram.
/// Intersection finds doc IDs present in all posting lists (AND) or any (OR).
use crate::trigram::{Trigram, TrigramQuery};

/// Intersect two sorted slices using galloping (exponential search).
/// Returns a sorted vec of elements present in both.
pub fn intersect_sorted(a: &[u32], b: &[u32]) -> Vec<u32> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }

    // Always iterate over the shorter list
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };

    let mut result = Vec::with_capacity(short.len().min(long.len()));
    let mut long_idx = 0;

    for &val in short {
        // Galloping search in long list
        long_idx = gallop(long, long_idx, val);
        if long_idx >= long.len() {
            break;
        }
        if long[long_idx] == val {
            result.push(val);
        }
    }

    result
}

/// Union two sorted slices. Returns a sorted, deduplicated vec.
pub fn union_sorted(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut result = Vec::with_capacity(a.len() + b.len());
    let (mut i, mut j) = (0, 0);

    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => {
                result.push(a[i]);
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                result.push(b[j]);
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                result.push(a[i]);
                i += 1;
                j += 1;
            }
        }
    }

    result.extend_from_slice(&a[i..]);
    result.extend_from_slice(&b[j..]);
    result
}

/// Galloping search: find the first index in `list[start..]` where `list[idx] >= target`.
fn gallop(list: &[u32], start: usize, target: u32) -> usize {
    if start >= list.len() {
        return list.len();
    }

    // Exponential search forward
    let mut bound = 1;
    let idx = start;

    while idx + bound < list.len() && list[idx + bound] < target {
        bound *= 2;
    }

    // Binary search in the narrowed range
    let lo = idx + bound / 2;
    let hi = (idx + bound).min(list.len() - 1);

    match list[lo..=hi].binary_search(&target) {
        Ok(pos) => lo + pos,
        Err(pos) => lo + pos,
    }
}

/// Resolve a TrigramQuery against a posting list lookup function.
/// Returns sorted candidate document IDs.
pub fn resolve_query<'a, F>(query: &TrigramQuery, total_docs: u32, lookup: &'a F) -> Vec<u32>
where
    F: Fn(Trigram) -> &'a [u32],
{
    match query {
        TrigramQuery::Literal(t) => lookup(*t).to_vec(),
        TrigramQuery::And(children) => {
            let mut lists: Vec<Vec<u32>> = children
                .iter()
                .map(|c| resolve_query(c, total_docs, lookup))
                .collect();

            if lists.is_empty() {
                return all_docs(total_docs);
            }

            // Sort by length -- intersect shortest first
            lists.sort_by_key(|l| l.len());

            let mut result = lists.swap_remove(0);
            for list in &lists {
                result = intersect_sorted(&result, list);
                if result.is_empty() {
                    break;
                }
            }
            result
        }
        TrigramQuery::Or(children) => {
            let mut result: Vec<u32> = Vec::new();
            for child in children {
                let child_result = resolve_query(child, total_docs, lookup);
                result = union_sorted(&result, &child_result);
            }
            result
        }
        TrigramQuery::All => all_docs(total_docs),
    }
}

fn all_docs(total: u32) -> Vec<u32> {
    (0..total).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intersect_basic() {
        assert_eq!(intersect_sorted(&[1, 3, 5, 7], &[2, 3, 5, 8]), vec![3, 5]);
    }

    #[test]
    fn test_intersect_empty() {
        assert!(intersect_sorted(&[], &[1, 2, 3]).is_empty());
        assert!(intersect_sorted(&[1, 2], &[]).is_empty());
    }

    #[test]
    fn test_intersect_no_overlap() {
        assert!(intersect_sorted(&[1, 3, 5], &[2, 4, 6]).is_empty());
    }

    #[test]
    fn test_intersect_identical() {
        assert_eq!(intersect_sorted(&[1, 2, 3], &[1, 2, 3]), vec![1, 2, 3]);
    }

    #[test]
    fn test_union_basic() {
        assert_eq!(union_sorted(&[1, 3, 5], &[2, 3, 6]), vec![1, 2, 3, 5, 6]);
    }

    #[test]
    fn test_gallop_finds_target() {
        let list = vec![1, 3, 5, 7, 9, 11, 13];
        assert_eq!(gallop(&list, 0, 7), 3);
    }
}
