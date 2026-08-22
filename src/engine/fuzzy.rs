//! Levenshtein-bounded substring matching for `~k/pattern/`.
//!
//! The matcher splits the pattern into `k + 1` pieces and searches each piece
//! exactly. Any substring within `k` edits must contain at least one undamaged
//! piece. A view with no exact piece can therefore be rejected before running
//! the dynamic-programming check.
//!
//! The confirming pass uses `O(pattern length)` memory and
//! `O(pattern length * text length)` time.

use crate::engine::simd::SimdLiteralMatcher;

/// A literal pattern, maximum edit distance, and its exact prefilter pieces.
pub struct FuzzyMatcher {
    pattern: Vec<u8>,
    k: usize,
    pieces: Vec<SimdLiteralMatcher>,
}

impl FuzzyMatcher {
    pub fn new(pattern: &[u8], k: usize) -> Self {
        let m = pattern.len();
        // Pieces are only useful (and the pigeonhole only holds) when k < m.
        let pieces = if m > 0 && k < m {
            split_pieces(pattern, k + 1)
        } else {
            Vec::new()
        };
        Self {
            pattern: pattern.to_vec(),
            k,
            pieces,
        }
    }

    /// True if `text` contains a substring within edit distance `k` of the
    /// pattern.
    #[inline]
    pub fn is_match(&self, text: &[u8]) -> bool {
        let m = self.pattern.len();
        // An empty pattern matches everything; if k >= m the pattern is within
        // distance k of the empty substring, so every text matches.
        if m == 0 || self.k >= m {
            return true;
        }
        // NEON prefilter: at least one disjoint piece must occur exactly.
        if !self.pieces.iter().any(|p| p.find(text).is_some()) {
            return false;
        }
        within_k(&self.pattern, text, self.k)
    }
}

/// Partition `pattern` into `parts` contiguous, non-empty pieces (caller
/// guarantees `parts <= pattern.len()`), each a NEON literal matcher.
fn split_pieces(pattern: &[u8], parts: usize) -> Vec<SimdLiteralMatcher> {
    let m = pattern.len();
    let base = m / parts;
    let rem = m % parts;
    let mut pieces = Vec::with_capacity(parts);
    let mut start = 0;
    for i in 0..parts {
        let len = base + usize::from(i < rem);
        pieces.push(SimdLiteralMatcher::new(&pattern[start..start + len]));
        start += len;
    }
    pieces
}

/// Approximate-substring DP: true iff some substring of `text` is within
/// Levenshtein distance `k` of `pattern`. O(n·m) time, O(m) space.
///
/// `D[i][j]` is the min edits to align `pattern[..i]` against a substring of
/// `text` *ending* at `j`. Row 0 is all zeros (the match may start anywhere);
/// column 0 is `i` (aligning `i` pattern chars against nothing costs `i`
/// deletions). A match exists iff `D[m][j] <= k` for some `j`.
fn within_k(pattern: &[u8], text: &[u8], k: usize) -> bool {
    let m = pattern.len();
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut cur = vec![0usize; m + 1];

    for &tc in text {
        cur[0] = 0;
        for i in 1..=m {
            let sub = prev[i - 1] + usize::from(pattern[i - 1] != tc);
            let del = prev[i] + 1;
            let ins = cur[i - 1] + 1;
            cur[i] = sub.min(del).min(ins);
        }
        if cur[m] <= k {
            return true;
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::FuzzyMatcher;

    fn m(pat: &str, k: usize, text: &str) -> bool {
        FuzzyMatcher::new(pat.as_bytes(), k).is_match(text.as_bytes())
    }

    #[test]
    fn exact_within_zero() {
        assert!(m("hello", 0, "well hello there"));
        assert!(!m("hello", 0, "well helo there"));
    }

    #[test]
    fn one_substitution() {
        assert!(m("hello", 1, "say helXo"));
        assert!(!m("hello", 1, "say hXXXo"));
    }

    #[test]
    fn one_insertion_and_deletion() {
        assert!(m("hello", 1, "helo")); // one deletion from pattern's view
        assert!(m("hello", 1, "helllo")); // one insertion
        assert!(!m("hello", 1, "hel"));
    }

    #[test]
    fn distance_two() {
        assert!(m("kitten", 2, "the sitten cat")); // k->s, distance 1
        assert!(m("kitten", 2, "saw a sittin bird")); // sittin: k->s,e->i = 2
        assert!(!m("kitten", 1, "sittin"));
    }

    #[test]
    fn k_ge_len_matches_anything() {
        assert!(m("ab", 2, "zzz"));
        assert!(m("", 0, "anything"));
    }

    #[test]
    fn prefilter_rejects_cleanly() {
        // No piece of "abcdef" (split into "abc","def" for k=1) appears.
        assert!(!m("abcdef", 1, "xyz qrs tuv"));
    }
}
