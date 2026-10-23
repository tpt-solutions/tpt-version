//! Three-way merge of sequences, used for text fields with
//! [`FieldStrategy::TextLines`](crate::merge::FieldStrategy::TextLines) (lines)
//! and lists with [`ListStrategy::Ordered`](crate::merge::ListStrategy::Ordered)
//! (items).
//!
//! Each side's changes are found as hunks against the base, using a longest
//! common subsequence. Hunks from the two sides merge when their base regions
//! are apart. Hunks that touch or overlap are a conflict, unless both sides made
//! the identical change.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::ops::Range;

/// A run of base items replaced by `items`.
#[derive(Debug, PartialEq, Eq)]
struct Hunk<T> {
    base: Range<usize>,
    items: Vec<T>,
}

/// The changes that turn `base` into `side`, in base order.
fn hunks<T: PartialEq + Clone>(base: &[T], side: &[T]) -> Vec<Hunk<T>> {
    let (n, m) = (base.len(), side.len());
    // lcs[i][j] is the length of the longest common subsequence of base[i..] and side[j..].
    let mut lcs = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if base[i] == side[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }

    let mut found = Vec::new();
    let (mut i, mut j) = (0, 0);
    // Where the current hunk started, in base and side coordinates.
    let mut open: Option<(usize, usize)> = None;
    while i < n || j < m {
        if i < n && j < m && base[i] == side[j] {
            if let Some((base_start, side_start)) = open.take() {
                found.push(Hunk {
                    base: base_start..i,
                    items: side[side_start..j].to_vec(),
                });
            }
            i += 1;
            j += 1;
            continue;
        }
        open.get_or_insert((i, j));
        // Step along an optimal path: drop a base item, or add a side item.
        if i < n && (j == m || lcs[i + 1][j] >= lcs[i][j + 1]) {
            i += 1;
        } else {
            j += 1;
        }
    }
    if let Some((base_start, side_start)) = open {
        found.push(Hunk {
            base: base_start..n,
            items: side[side_start..m].to_vec(),
        });
    }
    found
}

/// Merges two edits of the sequence `base`. Returns `None` on a conflict.
pub(crate) fn merge_sequence<T: PartialEq + Clone>(
    base: &[T],
    ours: &[T],
    theirs: &[T],
) -> Option<Vec<T>> {
    let mut combined: Vec<Hunk<T>> = hunks(base, ours);
    for hunk in hunks(base, theirs) {
        match combined
            .iter()
            .find(|other| touches(&other.base, &hunk.base))
        {
            Some(other) if *other == hunk => {}
            Some(_) => return None,
            None => combined.push(hunk),
        }
    }
    combined.sort_by_key(|hunk| (hunk.base.start, hunk.base.end));

    let mut merged = Vec::new();
    let mut position = 0;
    for hunk in combined {
        merged.extend_from_slice(&base[position..hunk.base.start]);
        merged.extend(hunk.items);
        position = hunk.base.end;
    }
    merged.extend_from_slice(&base[position..]);
    Some(merged)
}

/// Merges two line-level edits of `base`. Returns `None` on a conflict.
pub(crate) fn merge_lines(base: &str, ours: &str, theirs: &str) -> Option<String> {
    let base_lines: Vec<&str> = base.split('\n').collect();
    let our_lines: Vec<&str> = ours.split('\n').collect();
    let their_lines: Vec<&str> = theirs.split('\n').collect();
    merge_sequence(&base_lines, &our_lines, &their_lines).map(|lines| lines.join("\n"))
}

/// Whether two base regions overlap or touch. Touching counts, so that edits
/// on adjacent items are a conflict rather than an ambiguous interleaving.
fn touches(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start <= b.end && b.start <= a.end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_on_distant_lines_merge() {
        let base = "one\ntwo\nthree\nfour";
        let ours = "ONE\ntwo\nthree\nfour";
        let theirs = "one\ntwo\nthree\nFOUR";
        assert_eq!(
            merge_lines(base, ours, theirs).as_deref(),
            Some("ONE\ntwo\nthree\nFOUR")
        );
    }

    #[test]
    fn insertion_at_the_end_merges_with_an_edit_above() {
        let base = "a\nb\nc";
        let ours = "a\nB\nc";
        let theirs = "a\nb\nc\nd";
        assert_eq!(
            merge_lines(base, ours, theirs).as_deref(),
            Some("a\nB\nc\nd")
        );
    }

    #[test]
    fn edits_to_the_same_line_conflict() {
        assert_eq!(merge_lines("a\nb", "a\nX", "a\nY"), None);
    }

    #[test]
    fn identical_edits_collapse() {
        assert_eq!(merge_lines("a\nb", "a\nX", "a\nX").as_deref(), Some("a\nX"));
    }

    #[test]
    fn adjacent_edits_conflict_rather_than_interleave() {
        assert_eq!(merge_lines("a\nb\nc", "A\nb\nc", "a\nB\nc"), None);
    }

    #[test]
    fn a_removal_and_a_distant_addition_both_apply() {
        let merged = merge_sequence(&[1, 2, 3, 4], &[2, 3, 4], &[1, 2, 3, 4, 5]);
        assert_eq!(merged, Some(alloc::vec![2, 3, 4, 5]));
    }
}
