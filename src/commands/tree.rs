//! Structural tree navigation over balanced delimiters `()`, `[]`, `{}`.
//!
//! In the spirit of Rob Pike's structural regular expressions, these treat the
//! file as a *tree* of bracketed regions and let a pipeline move between levels:
//!
//! - **`+` (expand)** replaces each view with the smallest bracketed region that
//!   strictly encloses it — i.e. "select the surrounding block". A view already
//!   at the top level is passed through unchanged.
//! - **`-` (collapse)** descends one level: it replaces each view with the
//!   content *inside* its first balanced bracket pair. A view containing no
//!   bracket is dropped (nothing to descend into).
//!
//! Composing them walks the structure, e.g. `x/foo/ +` selects the block that
//! contains `foo`, and a following `-` steps back into it.
//!
//! Delimiter matching is by nesting depth across all three bracket kinds (the
//! common "folding" behaviour); the bracket scan itself is the NEON
//! [`first_of`](crate::engine::simd::first_of) set scan.

use crate::core::{ByteView, Command, ExecutionContext};
use crate::engine::simd;

const OPENERS: &[u8] = b"([{";
const BRACKETS: &[u8] = b"()[]{}";

#[inline]
fn is_opener(b: u8) -> bool {
    matches!(b, b'(' | b'[' | b'{')
}

/// Index of the unmatched opener that encloses position `s` (scanning left), or
/// `None` if `s` is already at the top level. Reverse walk — inherently scalar.
fn enclosing_open(master: &[u8], s: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut i = s;
    while i > 0 {
        i -= 1;
        let b = master[i];
        if matches!(b, b')' | b']' | b'}') {
            depth += 1;
        } else if is_opener(b) {
            if depth == 0 {
                return Some(i);
            }
            depth -= 1;
        }
    }
    None
}

/// Index of the closer matching the level open just before `e` (scanning right
/// with the NEON bracket scan), or `None` if the region is unbalanced.
fn enclosing_close(master: &[u8], e: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut j = e;
    while let Some(rel) = simd::first_of(BRACKETS, &master[j..]) {
        let k = j + rel;
        if is_opener(master[k]) {
            depth += 1;
        } else if depth == 0 {
            return Some(k);
        } else {
            depth -= 1;
        }
        j = k + 1;
    }
    None
}

/// Index of the closer matching the opener at `open` within `v` (NEON scan).
fn matching_close(v: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut j = open;
    while let Some(rel) = simd::first_of(BRACKETS, &v[j..]) {
        let k = j + rel;
        if is_opener(v[k]) {
            depth += 1;
        } else {
            depth -= 1;
            if depth == 0 {
                return Some(k);
            }
        }
        j = k + 1;
    }
    None
}

/// `+` — Expand: widen each view to the bracketed block enclosing it.
pub struct ExpandCommand;

impl Command for ExpandCommand {
    fn apply<'a>(
        &'a self,
        views: Box<dyn Iterator<Item = ByteView<'a>> + 'a>,
        ctx: &'a ExecutionContext,
    ) -> Box<dyn Iterator<Item = ByteView<'a>> + 'a> {
        let base = ctx.source_ptr;
        // SAFETY: source_ptr/master_len describe the live mmap that outlives the
        // pipeline (guaranteed by run_pipeline), and we only read from it.
        let master: &'a [u8] =
            unsafe { std::slice::from_raw_parts(base as *const u8, ctx.master_len) };

        Box::new(views.map(move |view| {
            let s = view.absolute_offset(base);
            let e = s + view.len();
            if let (Some(o), Some(c)) = (enclosing_open(master, s), enclosing_close(master, e)) {
                ByteView::new(&master[o..=c])
            } else {
                view // already at the top level
            }
        }))
    }
}

/// `-` — Collapse: descend into each view's first balanced bracket pair.
pub struct CollapseCommand;

impl Command for CollapseCommand {
    fn apply<'a>(
        &'a self,
        views: Box<dyn Iterator<Item = ByteView<'a>> + 'a>,
        _ctx: &'a ExecutionContext,
    ) -> Box<dyn Iterator<Item = ByteView<'a>> + 'a> {
        Box::new(views.filter_map(move |view| {
            let v = view.slice;
            let open = simd::first_of(OPENERS, v)?;
            let inner = match matching_close(v, open) {
                Some(close) => &v[open + 1..close],
                None => &v[open + 1..], // unbalanced: take the rest
            };
            Some(ByteView::new(inner))
        }))
    }
}
