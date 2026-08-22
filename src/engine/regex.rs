use crate::engine::simd::SimdLiteralMatcher;
use anyhow::{Result, anyhow};
use regex_automata::util::captures::Captures;
use regex_automata::{Input, PatternID, meta::Regex};
use std::sync::Arc;

/// Compilation flags parsed from the trailing characters of a pattern
/// (e.g. the `gi` in `s/foo/bar/gi`, or the `i` in `g/foo/i`).
#[derive(Debug, Clone, Copy, Default)]
pub struct Flags {
    /// `i` — ASCII/Unicode case-insensitive matching.
    pub case_insensitive: bool,
    /// `s` — let `.` match newlines (dot-all).
    pub dot_all: bool,
    /// `g` — apply to every match in a view, not just the first
    /// (only meaningful for `s///`; selectors are always global).
    pub global: bool,
    /// A numeric occurrence selector in `s///N` (GNU sed): replace starting at
    /// the `N`th match (1-based). `None` means the first match. Only consumed by
    /// `s///`; regex compilation ignores it.
    pub occurrence: Option<usize>,
}

pub enum EngineBackend {
    Simd(Arc<SimdLiteralMatcher>),
    Meta(Arc<Regex>),
}

/// The unified structural regular expression engine.
///
/// Routes plain ASCII literals to the Aarch64 SIMD scanner and everything else
/// (metacharacters, case-insensitivity, alternation, …) to `regex-automata`'s
/// meta engine. Both backends yield byte-offset `(start, end)` match pairs.
#[derive(Clone)]
pub struct StructuralRegex {
    backend: EngineBackend,
    pub pattern: String,
}

impl StructuralRegex {
    /// Compile with default flags (case-sensitive). Convenience for callers
    /// that build patterns internally (e.g. the implicit line splitter).
    pub fn compile(pattern: &str) -> Result<Self> {
        Self::compile_with(pattern, Flags::default())
    }

    /// Compile `pattern` honouring `flags`.
    pub fn compile_with(pattern: &str, flags: Flags) -> Result<Self> {
        // The SIMD literal fast-path is only valid for case-sensitive,
        // non-empty patterns containing no regex metacharacters.
        let is_literal = is_literal_pattern(pattern);

        if is_literal && !flags.case_insensitive {
            return Ok(Self {
                backend: EngineBackend::Simd(Arc::new(SimdLiteralMatcher::new(pattern.as_bytes()))),
                pattern: pattern.to_owned(),
            });
        }

        let regex = regex_automata::meta::Builder::new()
            .syntax(
                regex_automata::util::syntax::Config::new()
                    .case_insensitive(flags.case_insensitive)
                    .dot_matches_new_line(flags.dot_all)
                    // se treats the whole input as multi-line: `^`/`$` anchor
                    // to line boundaries, which is what structural selectors want.
                    .multi_line(true),
            )
            .build(pattern)
            .map_err(|e| anyhow!("Invalid regex '{}': {}", pattern, e))?;

        Ok(Self {
            backend: EngineBackend::Meta(Arc::new(regex)),
            pattern: pattern.to_owned(),
        })
    }

    /// Iterate over non-overlapping leftmost matches as `(start, end)` byte
    /// offsets relative to `slice`. Record-consumer semantics: a zero-width match
    /// at end-of-slice is the synthetic EOF record and is always dropped.
    pub fn find_iter<'a>(&self, slice: &'a [u8]) -> MatchIterator<'a> {
        MatchIterator {
            backend: self.backend.clone(),
            slice,
            offset: 0,
            done: false,
            newline_only_eof: false,
        }
    }

    /// Like [`find_iter`](Self::find_iter) but with substitution semantics: a
    /// zero-width match at true end-of-content is a real edit site, so only a
    /// phantom match *after a newline terminator* is dropped.
    pub fn find_iter_sub<'a>(&self, slice: &'a [u8]) -> MatchIterator<'a> {
        MatchIterator {
            backend: self.backend.clone(),
            slice,
            offset: 0,
            done: false,
            newline_only_eof: true,
        }
    }

    /// True when the source pattern is a plain literal (no regex metacharacters)
    /// AND contains no newline byte, so it can never match across a line
    /// boundary. Unlike [`as_literal`](Self::as_literal) this is independent of
    /// the backend, so it also holds for a case-insensitive literal that
    /// compiled to the meta engine — which lets the line-filter optimizer run a
    /// whole-buffer search for `g/lit/i` safely. See [`is_literal_pattern`].
    pub fn is_plain_literal(&self) -> bool {
        is_literal_pattern(&self.pattern) && !self.pattern.as_bytes().contains(&b'\n')
    }

    /// If this pattern compiled to the NEON literal fast path, hand back its
    /// matcher (a cheap `Arc` clone). Returns `None` for regex patterns. The
    /// line-filter optimizer uses this to decide whether the grep fast path
    /// applies.
    pub fn as_literal(&self) -> Option<Arc<SimdLiteralMatcher>> {
        match &self.backend {
            EngineBackend::Simd(m) => Some(m.clone()),
            EngineBackend::Meta(_) => None,
        }
    }

    /// Resolve a named capture group to its index, or `None` if there is no such
    /// group (or this is a literal pattern with no groups).
    pub fn group_index(&self, name: &str) -> Option<usize> {
        match &self.backend {
            EngineBackend::Meta(re) => re
                .create_captures()
                .group_info()
                .to_index(PatternID::ZERO, name),
            EngineBackend::Simd(_) => None,
        }
    }

    /// Iterate over matches together with their capture-group spans. Used by
    /// `s///` when the replacement references groups (`$1`, `${name}`, `\1`).
    pub fn captures_iter<'a>(&self, slice: &'a [u8]) -> CapturesIter<'a> {
        let inner = match &self.backend {
            EngineBackend::Simd(m) => CapturesInner::Simd(m.clone()),
            EngineBackend::Meta(re) => CapturesInner::Meta {
                regex: re.clone(),
                caps: re.create_captures(),
            },
        };
        // `captures_iter` is only used by `s///` (capture-referencing
        // replacements), so it uses substitution EOF semantics.
        CapturesIter {
            inner,
            slice,
            offset: 0,
            done: false,
            newline_only_eof: true,
        }
    }
}

/// One match plus the byte spans of its capture groups, indexed by group number
/// (group 0 is the whole match). A `None` entry is a group that did not
/// participate in the match.
pub struct MatchCaps {
    groups: Vec<Option<(usize, usize)>>,
}

impl MatchCaps {
    /// Span of the whole match (group 0).
    #[inline]
    pub fn overall(&self) -> (usize, usize) {
        self.groups[0].expect("group 0 always participates")
    }

    /// Span of capture group `i`, if it participated.
    #[inline]
    pub fn group(&self, i: usize) -> Option<(usize, usize)> {
        self.groups.get(i).copied().flatten()
    }
}

enum CapturesInner {
    Simd(Arc<SimdLiteralMatcher>),
    Meta { regex: Arc<Regex>, caps: Captures },
}

pub struct CapturesIter<'a> {
    inner: CapturesInner,
    slice: &'a [u8],
    offset: usize,
    done: bool,
    newline_only_eof: bool,
}

impl<'a> Iterator for CapturesIter<'a> {
    type Item = MatchCaps;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done || self.offset > self.slice.len() {
            return None;
        }
        match &mut self.inner {
            CapturesInner::Simd(matcher) => {
                let idx = matcher.find(&self.slice[self.offset..])?;
                let start = self.offset + idx;
                let end = start + matcher.len();
                if is_synthetic_eof(start, end, self.slice, self.newline_only_eof) {
                    self.done = true;
                    return None;
                }
                advance(
                    &mut self.offset,
                    &mut self.done,
                    self.slice.len(),
                    end,
                    start,
                );
                Some(MatchCaps {
                    groups: vec![Some((start, end))],
                })
            }
            CapturesInner::Meta { regex, caps } => {
                let input = Input::new(self.slice).range(self.offset..);
                regex.search_captures(&input, caps);
                let m = caps.get_match()?;
                let (start, end) = (m.start(), m.end());
                if is_synthetic_eof(start, end, self.slice, self.newline_only_eof) {
                    self.done = true;
                    return None;
                }
                let n = caps.group_len();
                let mut groups = Vec::with_capacity(n);
                for i in 0..n {
                    groups.push(caps.get_group(i).map(|sp| (sp.start, sp.end)));
                }
                advance(
                    &mut self.offset,
                    &mut self.done,
                    self.slice.len(),
                    end,
                    start,
                );
                Some(MatchCaps { groups })
            }
        }
    }
}

/// Whether a zero-width match landing exactly at end-of-slice is the synthetic
/// end-of-input record that should be dropped.
///
/// For record consumers (`x`/`g`/`v`/`y`) it always is: the empty logical line
/// after the final terminator (or past the last byte) is never a real record, so
/// they pass `newline_only = false` and it is dropped even for empty input.
///
/// Substitution (`s///`) passes `newline_only = true`: a zero-width match at true
/// end-of-content is a legitimate edit site (`s/$/X/` on `"abc"` → `"abcX"`;
/// `s/x*/-/g` on `"ab"` → `"-a-b-"`), so only the phantom record *after* a
/// newline terminator is dropped (which still fixes `s/$/X/gm` on `"a\nb\n"`).
#[inline]
fn is_synthetic_eof(start: usize, end: usize, slice: &[u8], newline_only: bool) -> bool {
    if start != end || end != slice.len() {
        return false;
    }
    !newline_only || slice.last() == Some(&b'\n')
}

/// Shared cursor-advance logic for the match iterators: forces forward progress
/// on zero-width matches so iteration always terminates.
#[inline]
fn advance(offset: &mut usize, done: &mut bool, slice_len: usize, end: usize, start: usize) {
    if end > start {
        *offset = end;
    } else {
        if *offset >= slice_len {
            *done = true;
        }
        *offset = end + 1;
    }
}

pub struct MatchIterator<'a> {
    backend: EngineBackend,
    slice: &'a [u8],
    offset: usize,
    done: bool,
    newline_only_eof: bool,
}

impl<'a> Iterator for MatchIterator<'a> {
    type Item = (usize, usize);

    fn next(&mut self) -> Option<Self::Item> {
        if self.done || self.offset > self.slice.len() {
            return None;
        }

        match &self.backend {
            EngineBackend::Simd(matcher) => {
                let remainder = &self.slice[self.offset..];
                let idx = matcher.find(remainder)?;
                let start = self.offset + idx;
                let end = start + matcher.len();
                if is_synthetic_eof(start, end, self.slice, self.newline_only_eof) {
                    self.done = true;
                    return None;
                }
                self.advance(end, start);
                Some((start, end))
            }
            EngineBackend::Meta(regex) => {
                let input = Input::new(self.slice).range(self.offset..);
                let mat = regex.find(input)?;
                let (start, end) = (mat.start(), mat.end());
                if is_synthetic_eof(start, end, self.slice, self.newline_only_eof) {
                    self.done = true;
                    return None;
                }
                self.advance(end, start);
                Some((start, end))
            }
        }
    }
}

impl<'a> MatchIterator<'a> {
    /// Advance the scan cursor past a match, guaranteeing forward progress on
    /// zero-width matches (e.g. `^`, `$`, empty pattern) so iteration always
    /// terminates instead of looping on the same position.
    #[inline]
    fn advance(&mut self, end: usize, start: usize) {
        if end > start {
            self.offset = end;
        } else {
            // Zero-width match: step one byte forward. If that lands past the
            // end, mark done so we still emit one match at the final position.
            if self.offset >= self.slice.len() {
                self.done = true;
            }
            self.offset = end + 1;
        }
    }
}

/// A pattern is a plain literal when it is non-empty and contains none of the
/// ERE metacharacters — so it matches exactly its own bytes. Shared by the SIMD
/// fast-path routing and the line-filter optimizer's newline-safety check.
fn is_literal_pattern(pattern: &str) -> bool {
    const META: &[char] = &[
        '.', '*', '+', '?', '^', '$', '(', ')', '[', ']', '{', '}', '|', '\\',
    ];
    !pattern.is_empty() && !pattern.chars().any(|c| META.contains(&c))
}

impl Clone for EngineBackend {
    fn clone(&self) -> Self {
        match self {
            Self::Simd(m) => Self::Simd(m.clone()),
            Self::Meta(v) => Self::Meta(v.clone()),
        }
    }
}
