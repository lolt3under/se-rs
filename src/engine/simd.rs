//! Exact-literal scanning.
//!
//! Single-byte search uses an Aarch64 NEON `vceqq_u8` loop (16 bytes per
//! iteration) with a SWAR `u64` fallback. Multi-byte search reuses the NEON
//! byte scan to locate candidate positions of the needle's first byte and then
//! verifies the full needle. Other architectures use the SWAR path.

#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::*;

/// Exact literal scanner with an AArch64 NEON implementation.
pub struct SimdLiteralMatcher {
    literal: Vec<u8>,
}

impl SimdLiteralMatcher {
    pub fn new(literal: &[u8]) -> Self {
        Self {
            literal: literal.to_vec(),
        }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.literal.len()
    }

    /// The stored needle bytes. Used by the line-filter fast path to verify the
    /// literal contains no newline (so search-then-extend stays line-safe).
    #[inline(always)]
    pub fn as_bytes(&self) -> &[u8] {
        &self.literal
    }

    /// Finds the first occurrence of the stored literal in `haystack`.
    #[inline]
    pub fn find(&self, haystack: &[u8]) -> Option<usize> {
        let needle = &self.literal;
        if needle.is_empty() {
            return Some(0);
        }
        if needle.len() > haystack.len() {
            return None;
        }

        if needle.len() == 1 {
            return Self::find_byte(needle[0], haystack);
        }

        Self::find_substring(needle, haystack)
    }

    /// Locate `needle` (len >= 2) by scanning for its first byte with NEON and
    /// verifying the remainder. The first-byte scan is the hot loop and is
    /// vectorised; verification is a cheap `starts_with` on the candidate.
    #[inline]
    fn find_substring(needle: &[u8], haystack: &[u8]) -> Option<usize> {
        debug_assert!(needle.len() >= 2);
        let first = needle[0];
        // Last legal start index for the needle inside the haystack.
        let last_start = haystack.len() - needle.len();
        let mut pos = 0;

        while pos <= last_start {
            // Vectorised search for the next occurrence of the first byte.
            let rel = Self::find_byte(first, &haystack[pos..])?;
            let cand = pos + rel;
            if cand > last_start {
                return None;
            }
            // SAFETY-free verify: bounds guaranteed by `cand <= last_start`.
            if haystack[cand..cand + needle.len()] == *needle {
                return Some(cand);
            }
            pos = cand + 1;
        }
        None
    }

    /// Dispatches single-byte search to NEON when available, else SWAR.
    #[inline]
    pub(crate) fn find_byte(byte: u8, haystack: &[u8]) -> Option<usize> {
        #[cfg(target_arch = "aarch64")]
        {
            if std::arch::is_aarch64_feature_detected!("neon") {
                // SAFETY: guarded by a runtime NEON feature check.
                return unsafe { Self::find_byte_neon(byte, haystack) };
            }
        }

        Self::find_byte_swar(byte, haystack)
    }

    /// NEON single-byte scan, 16 bytes per iteration.
    ///
    /// # Safety
    /// Caller must ensure the target CPU supports NEON (checked in `find_byte`).
    /// All pointer reads stay within `haystack`'s bounds: the vector loop only
    /// loads while `ptr + 16 <= end`, and the scalar tail loop while `ptr < end`.
    #[cfg(target_arch = "aarch64")]
    #[target_feature(enable = "neon")]
    unsafe fn find_byte_neon(byte: u8, haystack: &[u8]) -> Option<usize> {
        unsafe {
            let base = haystack.as_ptr();
            let mut ptr = base;
            let end = base.add(haystack.len());
            let target = vdupq_n_u8(byte);

            while ptr.add(16) <= end {
                let chunk = vld1q_u8(ptr);
                let cmp = vceqq_u8(chunk, target);

                // Cheap "any lane set?" reduction across the 128-bit compare mask.
                if vmaxvq_u8(cmp) != 0 {
                    for i in 0..16 {
                        if *ptr.add(i) == byte {
                            return Some(ptr.add(i).offset_from(base) as usize);
                        }
                    }
                }
                ptr = ptr.add(16);
            }

            while ptr < end {
                if *ptr == byte {
                    return Some(ptr.offset_from(base) as usize);
                }
                ptr = ptr.add(1);
            }

            None
        }
    }

    /// SWAR (SIMD Within A Register) single-byte scan over `u64` lanes.
    /// Portable fallback used when NEON is unavailable.
    fn find_byte_swar(byte: u8, haystack: &[u8]) -> Option<usize> {
        const LO: u64 = 0x0101_0101_0101_0101;
        const HI: u64 = 0x8080_8080_8080_8080;
        let mask = LO.wrapping_mul(byte as u64);

        let mut i = 0;
        while i + 8 <= haystack.len() {
            // SAFETY: `i + 8 <= len` guarantees 8 readable bytes at `i`.
            let chunk = unsafe { std::ptr::read_unaligned(haystack.as_ptr().add(i) as *const u64) };
            let xor = chunk ^ mask;
            let found = xor.wrapping_sub(LO) & !xor & HI;
            if found != 0 {
                return Some(i + (found.trailing_zeros() / 8) as usize);
            }
            i += 8;
        }

        while i < haystack.len() {
            if haystack[i] == byte {
                return Some(i);
            }
            i += 1;
        }

        None
    }
}

/// Forward single-byte search (NEON-accelerated). Returns the index of the
/// first `byte` in `haystack`. This is the line-boundary scanner used by the
/// fused line-filter path to find the newline terminating a matched line.
#[inline]
pub fn memchr(byte: u8, haystack: &[u8]) -> Option<usize> {
    SimdLiteralMatcher::find_byte(byte, haystack)
}

/// Reverse single-byte search: index of the *last* `byte` in `haystack`.
///
/// Used to walk back from a match to the start of its line. The line-filter
/// only ever calls this over the gap since the previous line boundary, so the
/// backward scans are disjoint and cost O(n) total across the whole input.
#[inline]
pub fn memrchr(byte: u8, haystack: &[u8]) -> Option<usize> {
    haystack.iter().rposition(|&b| b == byte)
}

/// Forward scan for the first byte equal to *any* member of `set`
/// (NEON-accelerated). `set` is expected to be small (≤ 8 bytes): the awk
/// field-splitter passes the whitespace set, and the structural tree-navigator
/// passes the bracket set. Each lane is compared against every target with
/// `vceqq_u8` and OR-reduced, so the hot loop stays vectorised.
#[inline]
pub fn first_of(set: &[u8], haystack: &[u8]) -> Option<usize> {
    if set.is_empty() {
        return None;
    }
    #[cfg(target_arch = "aarch64")]
    {
        if set.len() <= 8 && std::arch::is_aarch64_feature_detected!("neon") {
            // SAFETY: guarded by a runtime NEON check; `set` fits the 8 splats.
            return unsafe { first_of_neon(set, haystack) };
        }
    }
    haystack.iter().position(|b| set.contains(b))
}

/// NEON implementation of [`first_of`]: 16 bytes per iteration, OR-reducing a
/// `vceqq_u8` per target byte.
///
/// # Safety
/// Caller must ensure NEON is available and `set.len() <= 8` (both checked in
/// `first_of`). Pointer reads stay within `haystack` (vector loop guarded by
/// `ptr + 16 <= end`, scalar tail by `ptr < end`).
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn first_of_neon(set: &[u8], haystack: &[u8]) -> Option<usize> {
    unsafe {
        let base = haystack.as_ptr();
        let mut ptr = base;
        let end = base.add(haystack.len());
        let n = set.len();
        let mut targets = [vdupq_n_u8(0); 8];
        for (i, &b) in set.iter().enumerate() {
            targets[i] = vdupq_n_u8(b);
        }

        while ptr.add(16) <= end {
            let chunk = vld1q_u8(ptr);
            let mut acc = vceqq_u8(chunk, targets[0]);
            for t in &targets[1..n] {
                acc = vorrq_u8(acc, vceqq_u8(chunk, *t));
            }
            if vmaxvq_u8(acc) != 0 {
                for i in 0..16 {
                    if set.contains(&*ptr.add(i)) {
                        return Some(ptr.add(i).offset_from(base) as usize);
                    }
                }
            }
            ptr = ptr.add(16);
        }
        while ptr < end {
            if set.contains(&*ptr) {
                return Some(ptr.offset_from(base) as usize);
            }
            ptr = ptr.add(1);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{SimdLiteralMatcher, first_of, memchr, memrchr};

    fn find(needle: &str, hay: &str) -> Option<usize> {
        SimdLiteralMatcher::new(needle.as_bytes()).find(hay.as_bytes())
    }

    #[test]
    fn single_byte() {
        assert_eq!(find("c", "abcdef"), Some(2));
        assert_eq!(find("z", "abcdef"), None);
        assert_eq!(find("a", "a"), Some(0));
    }

    #[test]
    fn multi_byte() {
        assert_eq!(find("cd", "abcdef"), Some(2));
        assert_eq!(find("ef", "abcdef"), Some(4));
        assert_eq!(find("xy", "abcdef"), None);
        // Needle longer than haystack.
        assert_eq!(find("abcdefg", "abc"), None);
    }

    #[test]
    fn repeated_first_byte_requires_verify() {
        // First byte 'a' appears many times before the real match.
        assert_eq!(find("aab", "aaaaaab"), Some(4));
        assert_eq!(find("ab", "aaaaab"), Some(4));
    }

    #[test]
    fn spans_neon_block_boundary() {
        let hay = format!("{}needle{}", "x".repeat(40), "y".repeat(40));
        assert_eq!(find("needle", &hay), Some(40));
    }

    #[test]
    fn empty_needle() {
        assert_eq!(find("", "abc"), Some(0));
    }

    #[test]
    fn memchr_forward() {
        assert_eq!(memchr(b'\n', b"abc\ndef"), Some(3));
        assert_eq!(memchr(b'\n', b"abcdef"), None);
        // Spans past a NEON block boundary.
        let hay: Vec<u8> = std::iter::repeat_n(b'x', 40).chain(*b"\n").collect();
        assert_eq!(memchr(b'\n', &hay), Some(40));
    }

    #[test]
    fn memrchr_reverse() {
        assert_eq!(memrchr(b'\n', b"a\nb\nc"), Some(3));
        assert_eq!(memrchr(b'\n', b"abc"), None);
        assert_eq!(memrchr(b'\n', b""), None);
    }

    #[test]
    fn first_of_set() {
        // Whitespace set (field splitting).
        assert_eq!(first_of(b" \t", b"abc def"), Some(3));
        assert_eq!(first_of(b" \t", b"abc\tdef"), Some(3));
        assert_eq!(first_of(b" \t", b"abcdef"), None);
        // Bracket set (tree nav), past a NEON block boundary.
        let mut hay = vec![b'a'; 40];
        hay.push(b'}');
        assert_eq!(first_of(b"()[]{}", &hay), Some(40));
        // First of several candidates wins.
        assert_eq!(first_of(b"xyz", b"...y..x"), Some(3));
        assert_eq!(first_of(b"", b"abc"), None);
    }
}
