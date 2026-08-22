//! Compiled `s///` replacement templates with capture-group support.
//!
//! A template is a sequence of literal byte runs and capture-group references.
//! Supported reference syntaxes mirror the common tools:
//!
//! - `$1` … `$9` (and multi-digit `$12`) — numbered groups, like Perl/`regex`.
//! - `${name}` / `$name` — named groups declared `(?P<name>…)` or `(?<name>…)`.
//! - `${0}` / `$0` / `&` / `\0` — the whole match. `&` and `\0` are sed's
//!   spellings; `\&` is a literal `&`.
//! - `\1` … `\9` — numbered groups, sed-style.
//! - `$$` — a literal `$` (ripgrep dialect). se does not treat a bare `$` as a
//!   literal, so sed's plain-`$` semantics (where `$$` renders as `$$`) are
//!   intentionally not adopted — the `$`-based group syntax takes precedence.
//! - `\n \t \r \\`, `\xNN` (a raw byte, e.g. `\x00` for NUL), and `\<char>` —
//!   the same escapes the rest of `se` uses.
//!
//! A reference to a group that does not exist (or did not participate in the
//! match) renders as the empty string. When a template has no references at all
//! it keeps a flattened literal so `SubstituteCommand` can take its
//! allocation-free shared-`Arc` fast path.

use crate::engine::{MatchCaps, StructuralRegex};

/// Sentinel group index that never participates, used for unresolved names so
/// they render as empty without a separate enum variant.
const ABSENT: usize = usize::MAX;

/// One piece of a replacement template.
enum Segment {
    /// Literal bytes (escapes already expanded).
    Literal(Vec<u8>),
    /// Reference to capture group `n` (0 = whole match).
    Group(usize),
}

/// A parsed `s///` replacement.
pub struct ReplacementTemplate {
    segments: Vec<Segment>,
    /// `Some` iff the template is pure literal (no group references): the
    /// flattened bytes, enabling the substitute fast path.
    literal: Option<Vec<u8>>,
}

impl ReplacementTemplate {
    /// Compile `raw` (the unescaped-on-the-wire replacement text) against the
    /// pattern `re`, resolving named groups to indices up front.
    pub fn compile(raw: &str, re: &StructuralRegex) -> Self {
        let bytes = raw.as_bytes();
        let mut segments: Vec<Segment> = Vec::new();
        let mut cur: Vec<u8> = Vec::new();
        let mut i = 0;

        while i < bytes.len() {
            let b = bytes[i];
            if b == b'\\' && i + 1 < bytes.len() {
                let n = bytes[i + 1];
                match n {
                    b'n' => {
                        cur.push(b'\n');
                        i += 2;
                    }
                    b't' => {
                        cur.push(b'\t');
                        i += 2;
                    }
                    b'r' => {
                        cur.push(b'\r');
                        i += 2;
                    }
                    b'\\' => {
                        cur.push(b'\\');
                        i += 2;
                    }
                    // `\0`..`\9` are capture-group backrefs (sed-style); `\0` is
                    // the whole match. A NUL byte is written `\x00`, not `\0`.
                    b'0'..=b'9' => {
                        flush(&mut segments, &mut cur);
                        segments.push(Segment::Group((n - b'0') as usize));
                        i += 2;
                    }
                    b'x' => match hex_byte(bytes.get(i + 2), bytes.get(i + 3)) {
                        Some(byte) => {
                            cur.push(byte);
                            i += 4;
                        }
                        None => {
                            cur.push(b'x');
                            i += 2;
                        } // malformed → literal `x`
                    },
                    other => {
                        cur.push(other);
                        i += 2;
                    }
                }
            } else if b == b'&' {
                // Unescaped `&` is the whole match (sed); `\&` above is a literal.
                flush(&mut segments, &mut cur);
                segments.push(Segment::Group(0));
                i += 1;
            } else if b == b'$' && i + 1 < bytes.len() {
                let n = bytes[i + 1];
                if n == b'$' {
                    cur.push(b'$');
                    i += 2;
                } else if n == b'{' {
                    match bytes[i + 2..].iter().position(|&c| c == b'}') {
                        Some(rel) => {
                            let close = i + 2 + rel;
                            flush(&mut segments, &mut cur);
                            segments.push(resolve_braced(&bytes[i + 2..close], re));
                            i = close + 1;
                        }
                        None => {
                            cur.push(b'$'); // unterminated `${` → literal `$`
                            i += 1;
                        }
                    }
                } else if n.is_ascii_digit() {
                    let mut j = i + 1;
                    let mut num = 0usize;
                    while j < bytes.len() && bytes[j].is_ascii_digit() {
                        num = num
                            .saturating_mul(10)
                            .saturating_add((bytes[j] - b'0') as usize);
                        j += 1;
                    }
                    flush(&mut segments, &mut cur);
                    segments.push(Segment::Group(num));
                    i = j;
                } else if n == b'_' || n.is_ascii_alphabetic() {
                    let mut j = i + 1;
                    while j < bytes.len() && (bytes[j] == b'_' || bytes[j].is_ascii_alphanumeric())
                    {
                        j += 1;
                    }
                    let name = std::str::from_utf8(&bytes[i + 1..j]).unwrap_or("");
                    flush(&mut segments, &mut cur);
                    segments.push(resolve_name(name, re));
                    i = j;
                } else {
                    cur.push(b'$'); // `$` before a non-reference char → literal
                    i += 1;
                }
            } else {
                cur.push(b);
                i += 1;
            }
        }
        flush(&mut segments, &mut cur);

        let literal = if segments.iter().all(|s| matches!(s, Segment::Literal(_))) {
            let mut flat = Vec::new();
            for s in &segments {
                if let Segment::Literal(b) = s {
                    flat.extend_from_slice(b);
                }
            }
            Some(flat)
        } else {
            None
        };

        Self { segments, literal }
    }

    /// The flattened bytes if this template references no capture groups.
    #[inline]
    pub fn as_literal(&self) -> Option<&[u8]> {
        self.literal.as_deref()
    }

    /// Render the template for one match into `into`. `buf` is the slice the
    /// capture spans in `caps` are relative to (the matched view).
    pub fn render(&self, buf: &[u8], caps: &MatchCaps, into: &mut Vec<u8>) {
        for seg in &self.segments {
            match seg {
                Segment::Literal(b) => into.extend_from_slice(b),
                Segment::Group(i) => {
                    if let Some((s, e)) = caps.group(*i) {
                        into.extend_from_slice(&buf[s..e]);
                    }
                }
            }
        }
    }
}

/// Resolve the body of a `${…}` reference: digits → numbered group, otherwise a
/// (possibly named) group.
fn resolve_braced(inner: &[u8], re: &StructuralRegex) -> Segment {
    if !inner.is_empty() && inner.iter().all(|c| c.is_ascii_digit()) {
        let mut num = 0usize;
        for &c in inner {
            num = num.saturating_mul(10).saturating_add((c - b'0') as usize);
        }
        Segment::Group(num)
    } else {
        let name = std::str::from_utf8(inner).unwrap_or("");
        resolve_name(name, re)
    }
}

/// Resolve a named group to a `Group` segment, falling back to the never-present
/// `ABSENT` index (renders empty) when the name is unknown.
fn resolve_name(name: &str, re: &StructuralRegex) -> Segment {
    Segment::Group(re.group_index(name).unwrap_or(ABSENT))
}

/// Parse two ASCII hex digits into a byte, for `\xNN` replacement escapes.
fn hex_byte(hi: Option<&u8>, lo: Option<&u8>) -> Option<u8> {
    let h = (*hi? as char).to_digit(16)?;
    let l = (*lo? as char).to_digit(16)?;
    Some((h * 16 + l) as u8)
}

#[inline]
fn flush(segments: &mut Vec<Segment>, cur: &mut Vec<u8>) {
    if !cur.is_empty() {
        segments.push(Segment::Literal(std::mem::take(cur)));
    }
}
