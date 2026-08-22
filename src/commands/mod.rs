use crate::core::{ByteView, Command, ExecutionContext, FusionInfo, Mutation, Pipeline};
use crate::engine::StructuralRegex;
use crate::engine::simd::{self, SimdLiteralMatcher};
use std::io::{self, Write};
use std::sync::Arc;

mod replacement;
pub use replacement::ReplacementTemplate;

mod tree;
pub use tree::{CollapseCommand, ExpandCommand};

/// `x/re/` — Extract: replace each view with the sub-views that match `re`.
pub struct ExtractCommand {
    pub re: StructuralRegex,
}

impl Command for ExtractCommand {
    fn apply<'a>(
        &'a self,
        views: Box<dyn Iterator<Item = ByteView<'a>> + 'a>,
        _ctx: &'a ExecutionContext,
    ) -> Box<dyn Iterator<Item = ByteView<'a>> + 'a> {
        // The match iterator borrows the mapped input, so matches can be yielded
        // without collecting the complete result set.
        let iter = views.flat_map(move |view| {
            self.re
                .find_iter(view.slice)
                .map(move |(start, end)| ByteView::new(&view.slice[start..end]))
        });
        Box::new(iter)
    }

    fn fusion_info(&self) -> FusionInfo {
        match line_split_newline_requirement(&self.re.pattern) {
            Some(require_newline) => FusionInfo::LineSplit { require_newline },
            None => FusionInfo::Other,
        }
    }
}

/// Recognize the whole-line extraction patterns the optimizer can fuse. Returns
/// `Some(require_newline)` for a line splitter (`.*\n` requires a terminating
/// newline; `.*\n?` does not), `None` for anything else. `.` and `[^\n]` are
/// equivalent here because `se` matches `.` in multi-line mode (no dot-all).
fn line_split_newline_requirement(pattern: &str) -> Option<bool> {
    match pattern {
        r".*\n" | r"[^\n]*\n" => Some(true),
        r".*\n?" | r"[^\n]*\n?" => Some(false),
        _ => None,
    }
}

/// `y/re/` and `z/re/` — Yank/split: replace each view with the gaps *between*
/// matches of `re` (the structural complement of Extract).
pub struct YankCommand {
    pub re: StructuralRegex,
}

impl Command for YankCommand {
    fn apply<'a>(
        &'a self,
        views: Box<dyn Iterator<Item = ByteView<'a>> + 'a>,
        _ctx: &'a ExecutionContext,
    ) -> Box<dyn Iterator<Item = ByteView<'a>> + 'a> {
        let iter = views.flat_map(move |view| {
            let mut matches = Vec::new();
            let mut last_end = 0;
            for (start, end) in self.re.find_iter(view.slice) {
                if start > last_end {
                    matches.push(ByteView::new(&view.slice[last_end..start]));
                }
                last_end = end;
            }
            if last_end < view.slice.len() {
                matches.push(ByteView::new(&view.slice[last_end..]));
            }
            matches.into_iter()
        });
        Box::new(iter)
    }
}

/// `g/re/` — Global keeper: keep only views that contain a match of `re`.
pub struct GlobalCommand {
    pub re: StructuralRegex,
}

impl Command for GlobalCommand {
    fn apply<'a>(
        &'a self,
        views: Box<dyn Iterator<Item = ByteView<'a>> + 'a>,
        _ctx: &'a ExecutionContext,
    ) -> Box<dyn Iterator<Item = ByteView<'a>> + 'a> {
        Box::new(views.filter(move |view| self.re.find_iter(view.slice).next().is_some()))
    }

    fn fusion_info(&self) -> FusionInfo {
        FusionInfo::LineFilter {
            re: self.re.clone(),
            invert: false,
        }
    }
}

/// `v/re/` — Global rejector: drop views that contain a match of `re`.
pub struct NotGlobalCommand {
    pub re: StructuralRegex,
}

impl Command for NotGlobalCommand {
    fn apply<'a>(
        &'a self,
        views: Box<dyn Iterator<Item = ByteView<'a>> + 'a>,
        _ctx: &'a ExecutionContext,
    ) -> Box<dyn Iterator<Item = ByteView<'a>> + 'a> {
        Box::new(views.filter(move |view| self.re.find_iter(view.slice).next().is_none()))
    }

    fn fusion_info(&self) -> FusionInfo {
        FusionInfo::LineFilter {
            re: self.re.clone(),
            invert: true,
        }
    }
}

/// `~k/pattern/` — Fuzzy keeper: keep only views containing a substring within
/// Levenshtein distance `k` of the literal `pattern` (agrep semantics). See
/// [`FuzzyMatcher`](crate::engine::FuzzyMatcher) for the NEON-prefiltered design.
pub struct FuzzyCommand {
    pub matcher: crate::engine::FuzzyMatcher,
}

impl Command for FuzzyCommand {
    fn apply<'a>(
        &'a self,
        views: Box<dyn Iterator<Item = ByteView<'a>> + 'a>,
        _ctx: &'a ExecutionContext,
    ) -> Box<dyn Iterator<Item = ByteView<'a>> + 'a> {
        Box::new(views.filter(move |view| self.matcher.is_match(view.slice)))
    }
}

/// `:sem:/concept/` — Semantic keeper: keep views mentioning any expansion of
/// `concept` from the built-in lexicon. See
/// [`SemanticMatcher`](crate::engine::SemanticMatcher) (lexicon-based, not
/// embedding-based).
pub struct SemanticCommand {
    pub matcher: crate::engine::SemanticMatcher,
}

impl Command for SemanticCommand {
    fn apply<'a>(
        &'a self,
        views: Box<dyn Iterator<Item = ByteView<'a>> + 'a>,
        _ctx: &'a ExecutionContext,
    ) -> Box<dyn Iterator<Item = ByteView<'a>> + 'a> {
        Box::new(views.filter(move |view| self.matcher.is_match(view.slice)))
    }
}

/// `@{ awk program }` — run an awk program with each incoming view as a record.
/// `BEGIN` runs before the first record, `END` after the last; views pass
/// through unchanged so the command composes. Output is via awk `print`/`printf`.
pub struct AwkProgramCommand {
    pub program: crate::awk::Program,
    pub state: std::sync::Mutex<crate::awk::Interp>,
}

impl Command for AwkProgramCommand {
    fn apply<'a>(
        &'a self,
        views: Box<dyn Iterator<Item = ByteView<'a>> + 'a>,
        ctx: &'a ExecutionContext,
    ) -> Box<dyn Iterator<Item = ByteView<'a>> + 'a> {
        Box::new(AwkRun {
            program: &self.program,
            state: &self.state,
            views,
            out: io::BufWriter::new(io::stdout()),
            ctx,
            began: false,
            ended: false,
            errored: false,
        })
    }
}

/// Stateful iterator owning the awk output buffer across BEGIN, the records, and
/// END, so all three phases write to the same stream in order.
struct AwkRun<'a> {
    program: &'a crate::awk::Program,
    state: &'a std::sync::Mutex<crate::awk::Interp>,
    views: Box<dyn Iterator<Item = ByteView<'a>> + 'a>,
    out: io::BufWriter<io::Stdout>,
    ctx: &'a ExecutionContext,
    began: bool,
    ended: bool,
    /// Once a phase raises a runtime error we record it and stop running the awk
    /// program (a fatal error aborts the whole run, like gawk), while still
    /// draining the view stream so the pipeline terminates.
    errored: bool,
}

impl<'a> Iterator for AwkRun<'a> {
    type Item = ByteView<'a>;

    fn next(&mut self) -> Option<ByteView<'a>> {
        if !self.began {
            self.began = true;
            if !self.errored {
                let mut ip = self.state.lock().unwrap();
                if let Err(e) = self.program.run_begin(&mut ip, &mut self.out) {
                    self.errored = true;
                    self.ctx.record_error(e);
                }
            }
        }
        match self.views.next() {
            Some(view) => {
                if !self.errored {
                    let mut ip = self.state.lock().unwrap();
                    if let Err(e) = self.program.run_record(&mut ip, view.slice, &mut self.out) {
                        self.errored = true;
                        self.ctx.record_error(e);
                    }
                }
                Some(view)
            }
            None => {
                if !self.ended {
                    self.ended = true;
                    if !self.errored {
                        let mut ip = self.state.lock().unwrap();
                        if let Err(e) = self.program.run_end(&mut ip, &mut self.out) {
                            self.errored = true;
                            self.ctx.record_error(e);
                        }
                    }
                    let _ = self.out.flush();
                }
                None
            }
        }
    }
}

/// `{ ... }` — Group: run a sub-pipeline independently on each incoming view
/// and splice its output views back into the stream.
pub struct GroupCommand {
    pub pipeline: Pipeline,
}

impl Command for GroupCommand {
    fn apply<'a>(
        &'a self,
        views: Box<dyn Iterator<Item = ByteView<'a>> + 'a>,
        ctx: &'a ExecutionContext,
    ) -> Box<dyn Iterator<Item = ByteView<'a>> + 'a> {
        Box::new(views.flat_map(move |view| self.pipeline.execute(view, ctx)))
    }

    fn rewrites(&self) -> bool {
        self.pipeline.has_mutator()
    }
}

/// `c/text/` — Change: record an out-of-band mutation overwriting each view
/// with `text`. The mmap is never touched.
pub struct ChangeCommand {
    pub replacement: Vec<u8>,
}

impl Command for ChangeCommand {
    fn apply<'a>(
        &'a self,
        views: Box<dyn Iterator<Item = ByteView<'a>> + 'a>,
        ctx: &'a ExecutionContext,
    ) -> Box<dyn Iterator<Item = ByteView<'a>> + 'a> {
        let replacement: &'a [u8] = &self.replacement;
        let shared: std::sync::Arc<[u8]> = std::sync::Arc::from(replacement);
        let iter = views.map(move |view| {
            let start = view.absolute_offset(ctx.source_ptr);
            let end = start + view.len();
            ctx.mutations.lock().unwrap().push(Mutation {
                start,
                end,
                replacement: shared.clone(),
            });
            ByteView::new(replacement)
        });
        Box::new(iter)
    }

    fn rewrites(&self) -> bool {
        true
    }
}

/// `p` — Print: stream each view to stdout. A trailing newline is added only
/// when the view does not already end in one, so `x/.*\n/ ... p` (grep-style
/// line extraction) prints single-spaced.
pub struct PrintCommand;

impl Command for PrintCommand {
    fn apply<'a>(
        &'a self,
        views: Box<dyn Iterator<Item = ByteView<'a>> + 'a>,
        _ctx: &'a ExecutionContext,
    ) -> Box<dyn Iterator<Item = ByteView<'a>> + 'a> {
        // Buffer the stdout handle. Nested blocks may also print, so do not hold
        // a long-lived StdoutLock here.
        let mut w = io::BufWriter::new(io::stdout());
        let iter = views.inspect(move |view| {
            let _ = w.write_all(view.slice);
            if view.slice.last() != Some(&b'\n') {
                let _ = w.write_all(b"\n");
            }
        });
        Box::new(iter)
    }

    fn fusion_info(&self) -> FusionInfo {
        FusionInfo::Print
    }
}

/// `/pattern/ { ... }` — Awk binder: run the action pipeline on each view that
/// matches `pattern`, passing the view through unchanged to the outer stream.
pub struct AwkCommand {
    pub re: StructuralRegex,
    pub pipeline: Pipeline,
}

impl Command for AwkCommand {
    fn apply<'a>(
        &'a self,
        views: Box<dyn Iterator<Item = ByteView<'a>> + 'a>,
        ctx: &'a ExecutionContext,
    ) -> Box<dyn Iterator<Item = ByteView<'a>> + 'a> {
        let iter = views.inspect(move |view| {
            if self.re.find_iter(view.slice).next().is_some() {
                let _ = self.pipeline.execute(*view, ctx).count();
            }
        });
        Box::new(iter)
    }

    fn rewrites(&self) -> bool {
        self.pipeline.has_mutator()
    }

    fn fusion_info(&self) -> FusionInfo {
        // Only `/re/ { p }` (or bare `/re/`, which desugars to `{ p }`) can fuse:
        // a single print action with no mutation. Anything richer keeps the
        // general awk path.
        if self.pipeline.commands.len() == 1
            && matches!(self.pipeline.commands[0].fusion_info(), FusionInfo::Print)
        {
            FusionInfo::AwkPrint {
                re: self.re.clone(),
            }
        } else {
            FusionInfo::Other
        }
    }
}

/// `=` — Print the byte offsets and length of each view as `start,end,length`.
pub struct PrintLineCommand;

impl Command for PrintLineCommand {
    fn apply<'a>(
        &'a self,
        views: Box<dyn Iterator<Item = ByteView<'a>> + 'a>,
        ctx: &'a ExecutionContext,
    ) -> Box<dyn Iterator<Item = ByteView<'a>> + 'a> {
        let mut w = io::BufWriter::new(io::stdout());
        let iter = views.inspect(move |view| {
            let start = view.absolute_offset(ctx.source_ptr);
            let len = view.slice.len();
            let _ = writeln!(w, "{},{},{}", start, start + len, len);
        });
        Box::new(iter)
    }
}

/// `s/re/repl/flags` — Substitute: record out-of-band mutations replacing
/// matches of `re` with `repl`. With `g`, every match in a view is replaced;
/// otherwise only the first. `repl` may reference capture groups (`$1`,
/// `${name}`, `\1`) — see [`ReplacementTemplate`].
pub struct SubstituteCommand {
    pub re: StructuralRegex,
    pub template: ReplacementTemplate,
    pub global: bool,
    /// 1-based index of the first match to replace (GNU sed `s///N`). Defaults to
    /// 1. With `global`, every match from this one onward is replaced (`s///Ng`);
    ///    otherwise only this single match.
    pub occurrence: usize,
}

impl Command for SubstituteCommand {
    fn apply<'a>(
        &'a self,
        views: Box<dyn Iterator<Item = ByteView<'a>> + 'a>,
        ctx: &'a ExecutionContext,
    ) -> Box<dyn Iterator<Item = ByteView<'a>> + 'a> {
        // Fast path: a literal replacement (no capture references) shares one
        // `Arc<[u8]>` across every match, so each push is a refcount bump, not
        // an allocation — this is what makes dense `s///g` fast.
        if let Some(lit) = self.template.as_literal() {
            let shared: Arc<[u8]> = Arc::from(lit);
            let iter = views.inspect(move |view| {
                let view_abs = view.absolute_offset(ctx.source_ptr);
                let mut any = false;
                let mut nth = 0usize;
                let mut lock = ctx.mutations.lock().unwrap();
                for (start, end) in self.re.find_iter_sub(view.slice) {
                    nth += 1;
                    if nth < self.occurrence {
                        continue; // skip matches before the Nth (GNU sed `s///N`)
                    }
                    lock.push(Mutation {
                        start: view_abs + start,
                        end: view_abs + end,
                        replacement: shared.clone(),
                    });
                    any = true;
                    if !self.global {
                        break;
                    }
                }
                drop(lock);
                if any {
                    ctx.substitutions_made
                        .store(true, std::sync::atomic::Ordering::SeqCst);
                }
            });
            return Box::new(iter);
        }

        // Capture path: the replacement references groups, so it is rendered per
        // match (one allocation each — only when the user actually asks for it).
        let iter = views.inspect(move |view| {
            let view_abs = view.absolute_offset(ctx.source_ptr);
            let mut any = false;
            let mut nth = 0usize;
            let mut lock = ctx.mutations.lock().unwrap();
            for caps in self.re.captures_iter(view.slice) {
                nth += 1;
                if nth < self.occurrence {
                    continue; // skip matches before the Nth (GNU sed `s///N`)
                }
                let (start, end) = caps.overall();
                let mut rendered = Vec::new();
                self.template.render(view.slice, &caps, &mut rendered);
                lock.push(Mutation {
                    start: view_abs + start,
                    end: view_abs + end,
                    replacement: Arc::from(rendered),
                });
                any = true;
                if !self.global {
                    break;
                }
            }
            drop(lock);
            if any {
                ctx.substitutions_made
                    .store(true, std::sync::atomic::Ordering::SeqCst);
            }
        });
        Box::new(iter)
    }

    fn rewrites(&self) -> bool {
        true
    }
}

/// `t { ... }` — Test/branch: run the block only if a substitution was made
/// since the last test (consuming the flag), mirroring sed's `t`.
pub struct TestCommand {
    pub pipeline: Pipeline,
}

impl Command for TestCommand {
    fn apply<'a>(
        &'a self,
        views: Box<dyn Iterator<Item = ByteView<'a>> + 'a>,
        ctx: &'a ExecutionContext,
    ) -> Box<dyn Iterator<Item = ByteView<'a>> + 'a> {
        let iter = views.flat_map(move |view| {
            if ctx
                .substitutions_made
                .swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                let v: Vec<_> = self.pipeline.execute(view, ctx).collect();
                Box::new(v.into_iter()) as Box<dyn Iterator<Item = ByteView<'a>> + 'a>
            } else {
                Box::new(std::iter::once(view)) as Box<dyn Iterator<Item = ByteView<'a>> + 'a>
            }
        });
        Box::new(iter)
    }

    fn rewrites(&self) -> bool {
        self.pipeline.has_mutator()
    }
}

/// `N` — Next joiner: merge each adjacent pair of views into one view spanning
/// `min(start)..max(end)` over the master slice.
pub struct NextCommand;

impl Command for NextCommand {
    fn apply<'a>(
        &'a self,
        views: Box<dyn Iterator<Item = ByteView<'a>> + 'a>,
        ctx: &'a ExecutionContext,
    ) -> Box<dyn Iterator<Item = ByteView<'a>> + 'a> {
        let mut peekable = views.peekable();
        // SAFETY: source_ptr/master_len describe the live mmap that outlives the
        // pipeline (guaranteed by run_pipeline), and we only read from it.
        let master_slice: &'a [u8] =
            unsafe { std::slice::from_raw_parts(ctx.source_ptr as *const u8, ctx.master_len) };

        let iter = std::iter::from_fn(move || {
            let v1 = peekable.next()?;
            match peekable.next() {
                Some(v2) => Some(v1.merge(v2, ctx.source_ptr, master_slice)),
                None => Some(v1),
            }
        });
        Box::new(iter)
    }
}

/// `m/re/ { ... }` — Map: extract matches of `re` and run the block on each, in
/// order. Equivalent to `x/re/ { ... }`. Sequential execution keeps output in
/// input order, including when a block prints.
pub struct MapCommand {
    pub re: StructuralRegex,
    pub pipeline: Pipeline,
}

impl Command for MapCommand {
    fn apply<'a>(
        &'a self,
        views: Box<dyn Iterator<Item = ByteView<'a>> + 'a>,
        ctx: &'a ExecutionContext,
    ) -> Box<dyn Iterator<Item = ByteView<'a>> + 'a> {
        let iter = views.flat_map(move |view| {
            self.re.find_iter(view.slice).flat_map(move |(start, end)| {
                self.pipeline
                    .execute(ByteView::new(&view.slice[start..end]), ctx)
            })
        });
        Box::new(iter)
    }

    fn rewrites(&self) -> bool {
        self.pipeline.has_mutator()
    }
}

/// `r/sep/` — Reduce fold: collapse all current views into a single new view
/// whose bytes are the views joined by `sep`. This is a value-producing fold,
/// not a document edit — follow it with `p` to emit the result.
pub struct ReduceCommand {
    pub separator: Vec<u8>,
}

impl Command for ReduceCommand {
    fn apply<'a>(
        &'a self,
        views: Box<dyn Iterator<Item = ByteView<'a>> + 'a>,
        _ctx: &'a ExecutionContext,
    ) -> Box<dyn Iterator<Item = ByteView<'a>> + 'a> {
        let views_vec: Vec<ByteView<'a>> = views.collect();
        if views_vec.is_empty() {
            return Box::new(std::iter::empty());
        }

        let mut content: Vec<u8> = Vec::new();
        for (i, view) in views_vec.iter().enumerate() {
            if i > 0 {
                content.extend_from_slice(&self.separator);
            }
            content.extend_from_slice(view.slice);
        }

        // Leak the folded bytes to 'static so the resulting ByteView satisfies
        // any 'a. Acceptable for a short-lived, single-shot CLI process.
        let static_slice: &'static [u8] = Box::leak(content.into_boxed_slice());
        Box::new(std::iter::once(ByteView::new(static_slice)))
    }
}

/// Fused `x/.*\n/ g/lit/` (or `v/lit/`) — emit whole lines selected by a NEON
/// literal, *grep's way*: search the buffer for the literal, then extend each
/// hit to its line boundaries, instead of splitting every line into a view and
/// filtering it. Work is proportional to the number of matches, not the number
/// of lines, so a sparse pattern over a huge file costs almost nothing.
///
/// Produced only by the peephole optimizer (`parser::optimize`) when the filter
/// is a newline-free literal, so it is always behaviourally identical to the
/// `x/.*\n/ g/…/` pipeline it replaces — see `line_split_newline_requirement`.
pub struct LineFilterCommand {
    pub matcher: Arc<SimdLiteralMatcher>,
    /// `true` for `v/…/` (keep non-matching lines), `false` for `g/…/`.
    pub invert: bool,
    /// Mirror the line splitter: when `true`, a trailing line with no newline is
    /// not a "line" (as under `.*\n`) and is dropped.
    pub require_newline: bool,
}

impl Command for LineFilterCommand {
    fn apply<'a>(
        &'a self,
        views: Box<dyn Iterator<Item = ByteView<'a>> + 'a>,
        _ctx: &'a ExecutionContext,
    ) -> Box<dyn Iterator<Item = ByteView<'a>> + 'a> {
        if self.invert {
            // Rejecting lines means we must look at every line, so enumerate
            // lines (NEON newline scan) and keep those with no literal hit.
            Box::new(views.flat_map(move |view| {
                let buf = view.slice;
                let require_newline = self.require_newline;
                let matcher = &self.matcher;
                let mut pos = 0usize;
                std::iter::from_fn(move || {
                    loop {
                        if pos >= buf.len() {
                            return None;
                        }
                        let (line, next, has_nl) = match simd::memchr(b'\n', &buf[pos..]) {
                            Some(nl) => (&buf[pos..pos + nl + 1], pos + nl + 1, true),
                            None => (&buf[pos..], buf.len(), false),
                        };
                        pos = next;
                        if !has_nl && require_newline {
                            return None; // newline-less tail isn't a line under `.*\n`
                        }
                        if matcher.find(line).is_none() {
                            return Some(ByteView::new(line));
                        }
                        // line contains the literal — reject and continue
                    }
                })
            }))
        } else {
            // Keeping matching lines: jump match-to-match with NEON and extend
            // each hit to its enclosing line. Sparse patterns skip whole regions.
            Box::new(views.flat_map(move |view| {
                let buf = view.slice;
                let require_newline = self.require_newline;
                let matcher = &self.matcher;
                let mut cursor = 0usize;
                std::iter::from_fn(move || {
                    if cursor >= buf.len() {
                        return None;
                    }
                    let rel = matcher.find(&buf[cursor..])?;
                    let mpos = cursor + rel;
                    // Line start: just past the previous newline within the gap
                    // since `cursor` (a line boundary), else `cursor` itself.
                    let line_start = match simd::memrchr(b'\n', &buf[cursor..mpos]) {
                        Some(p) => cursor + p + 1,
                        None => cursor,
                    };
                    match simd::memchr(b'\n', &buf[mpos..]) {
                        Some(nl) => {
                            let line_end = mpos + nl + 1;
                            cursor = line_end;
                            Some(ByteView::new(&buf[line_start..line_end]))
                        }
                        None => {
                            // Match sits in the newline-less tail.
                            cursor = buf.len();
                            if require_newline {
                                None
                            } else {
                                Some(ByteView::new(&buf[line_start..]))
                            }
                        }
                    }
                })
            }))
        }
    }
}

/// Fused `x/.*\n/ g/re/` for a newline-free literal that compiled to the *meta*
/// engine — chiefly the case-insensitive `g/lit/i` idiom. Same whole-buffer
/// search-then-extend strategy as [`LineFilterCommand`], but driven by the meta
/// regex so Unicode case folding (e.g. `é`/`É`, `k`/`K`) is handled correctly
/// instead of a hand-rolled byte fold. Only `g` (non-invert) is fused; `v/lit/i`
/// keeps the per-line path. Produced by `parser::optimize`; the literal being
/// newline-free (`StructuralRegex::is_plain_literal`) guarantees no match ever
/// crosses a line, so this is byte-identical to the pipeline it replaces — just
/// proportional to the match count instead of the line count.
pub struct RegexLineFilterCommand {
    pub re: StructuralRegex,
    /// Mirror the line splitter: when `true` (`.*\n`), a trailing line with no
    /// newline is not a "line" and is dropped.
    pub require_newline: bool,
}

impl Command for RegexLineFilterCommand {
    fn apply<'a>(
        &'a self,
        views: Box<dyn Iterator<Item = ByteView<'a>> + 'a>,
        _ctx: &'a ExecutionContext,
    ) -> Box<dyn Iterator<Item = ByteView<'a>> + 'a> {
        Box::new(views.flat_map(move |view| {
            let buf = view.slice;
            let require_newline = self.require_newline;
            let re = &self.re;
            // As in LineFilterCommand, resume after an emitted line. Repeated
            // matches on that line are not enumerated. The cursor remains at a
            // line boundary, so a leading `^` still anchors correctly.
            let mut cursor = 0usize;
            std::iter::from_fn(move || {
                if cursor >= buf.len() {
                    return None;
                }
                let (rel_start, _) = re.find_iter(&buf[cursor..]).next()?;
                let mpos = cursor + rel_start;
                let line_start = match simd::memrchr(b'\n', &buf[cursor..mpos]) {
                    Some(p) => cursor + p + 1,
                    None => cursor,
                };
                match simd::memchr(b'\n', &buf[mpos..]) {
                    Some(nl) => {
                        let line_end = mpos + nl + 1;
                        cursor = line_end;
                        Some(ByteView::new(&buf[line_start..line_end]))
                    }
                    None => {
                        cursor = buf.len();
                        if require_newline {
                            None // newline-less tail isn't a line under `.*\n`
                        } else {
                            Some(ByteView::new(&buf[line_start..]))
                        }
                    }
                }
            })
        }))
    }
}
