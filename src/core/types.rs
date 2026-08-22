use crate::engine::StructuralRegex;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

/// A borrowed byte range in the mapped input.
#[derive(Clone, Copy, Debug)]
pub struct ByteView<'a> {
    pub slice: &'a [u8],
}

impl<'a> ByteView<'a> {
    /// Create a view from a byte slice.
    #[inline(always)]
    pub const fn new(slice: &'a [u8]) -> Self {
        Self { slice }
    }

    /// Return the view length in bytes.
    #[inline(always)]
    pub const fn len(&self) -> usize {
        self.slice.len()
    }

    /// Return whether the view has no bytes.
    #[allow(dead_code)]
    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.slice.is_empty()
    }

    /// Calculate the view's byte offset from the start of the input mapping.
    #[inline(always)]
    pub fn absolute_offset(&self, base_ptr: usize) -> usize {
        self.slice.as_ptr() as usize - base_ptr
    }

    /// Merge two input views into the range spanning both of them.
    ///
    /// Both views must borrow from `master_slice`.
    pub fn merge(self, other: ByteView<'a>, base_ptr: usize, master_slice: &'a [u8]) -> Self {
        let start1 = self.absolute_offset(base_ptr);
        let end1 = start1 + self.slice.len();

        let start2 = other.absolute_offset(base_ptr);
        let end2 = start2 + other.slice.len();

        let min_start = std::cmp::min(start1, start2);
        let max_end = std::cmp::max(end1, end2);

        ByteView::new(&master_slice[min_start..max_end])
    }
}

impl<'a> PartialEq for ByteView<'a> {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.slice == other.slice
    }
}

impl<'a> Eq for ByteView<'a> {}

impl<'a> Hash for ByteView<'a> {
    #[inline(always)]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.slice.hash(state);
    }
}

/// A pending in-place edit to the source document.
/// Owns its replacement bytes so it carries no arena lifetime — making
/// ExecutionContext lifetime-free and therefore Sync, which lets Rayon
/// share it across worker threads.
/// `replacement` is an `Arc<[u8]>` so recording one mutation per match (e.g. a
/// global substitution with millions of hits) is a refcount bump rather than a
/// fresh heap allocation per match.
#[derive(Debug, Clone)]
pub struct Mutation {
    pub start: usize,
    pub end: usize,
    pub replacement: Arc<[u8]>,
}

/// Shared state threaded through every command in a pipeline.
///
/// `source_ptr` is stored as `usize` (pointer-as-integer) so this struct is
/// `Sync` without an unsafe impl — raw `*const u8` would be `!Sync`.
/// Commands recover the pointer with `ctx.source_ptr as *const u8`.
pub struct ExecutionContext {
    /// Base address of the master mmap, stored as a plain integer for Sync.
    pub source_ptr: usize,
    pub master_len: usize,
    pub mutations: Mutex<Vec<Mutation>>,
    pub substitutions_made: AtomicBool,
    /// First runtime error raised by a command (e.g. an awk interpreter fault).
    /// Recorded out-of-band because `Command::apply` yields an infallible
    /// iterator; `drive` drains this after execution and turns it into a
    /// non-zero process exit.
    pub error: Mutex<Option<anyhow::Error>>,
}

impl ExecutionContext {
    pub fn new(source_ptr: *const u8, master_len: usize) -> Self {
        Self {
            source_ptr: source_ptr as usize,
            master_len,
            mutations: Mutex::new(Vec::new()),
            substitutions_made: AtomicBool::new(false),
            error: Mutex::new(None),
        }
    }

    /// Record the first runtime error; later errors are ignored (awk aborts on
    /// the first fault, like gawk).
    pub fn record_error(&self, err: anyhow::Error) {
        let mut slot = self.error.lock().unwrap();
        if slot.is_none() {
            *slot = Some(err);
        }
    }
}

// SAFETY: source_ptr is a read-only pointer into a live mmap whose lifetime
// is guaranteed by the caller to outlast all ExecutionContext uses.
unsafe impl Sync for ExecutionContext {}
unsafe impl Send for ExecutionContext {}

/// How a command participates in the line-filter peephole optimizer. The
/// optimizer fuses an adjacent `LineSplit` + `LineFilter` (i.e. `x/.*\n/ g/re/`)
/// into a single search-then-extend pass when the filter is a NEON literal.
pub enum FusionInfo {
    /// A whole-line extractor (`x/.*\n/`, `x/.*\n?/`). `require_newline` mirrors
    /// the pattern: `.*\n` requires a terminating newline, `.*\n?` does not.
    LineSplit { require_newline: bool },
    /// A per-view containment filter: `g/re/` (`invert=false`) or `v/re/`
    /// (`invert=true`).
    LineFilter { re: StructuralRegex, invert: bool },
    /// An awk binder whose action is exactly `{ p }` (or the bare `/re/` form):
    /// `/re/ { p }`. Unlike `LineFilter` it *passes every view through*, so it is
    /// only fusible to a line filter when it is the last command in the pipeline
    /// (nothing downstream depends on the pass-through stream).
    AwkPrint { re: StructuralRegex },
    /// The `p` print command.
    Print,
    /// Anything else — never participates in fusion.
    Other,
}

/// The core `Command` trait for structural editing operations.
/// All commands take a stream (Iterator) of ByteViews, and yield a new stream
/// sequentially or via parallel bridges. At this abstraction layer we rely exclusively
/// on standard lazy Iterators to enforce zero intermediate allocations.
pub trait Command: Sync + Send {
    fn apply<'a>(
        &'a self,
        views: Box<dyn Iterator<Item = ByteView<'a>> + 'a>,
        ctx: &'a ExecutionContext,
    ) -> Box<dyn Iterator<Item = ByteView<'a>> + 'a>;

    /// True if this command (or a nested block) records document mutations.
    ///
    /// A pipeline containing a mutator is in "rewrite mode": the program emits
    /// the document with edits stitched in, and emits it verbatim even when no
    /// match occurred. Pure selector/print pipelines return `false` and only
    /// emit what `p`/`=` produce.
    fn rewrites(&self) -> bool {
        false
    }

    /// Classify this command for the line-filter peephole optimizer. Defaults to
    /// `Other`; the line extractor, `g`/`v`, and `p` override it.
    fn fusion_info(&self) -> FusionInfo {
        FusionInfo::Other
    }
}

/// A pipeline encompasses multiple commands fused together.
pub struct Pipeline {
    pub commands: Vec<Box<dyn Command>>,
}

impl Pipeline {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    pub fn push(&mut self, cmd: Box<dyn Command>) {
        self.commands.push(cmd);
    }

    /// True if any command in this pipeline (including nested blocks) rewrites
    /// the document — see [`Command::rewrites`].
    pub fn has_mutator(&self) -> bool {
        self.commands.iter().any(|c| c.rewrites())
    }

    /// Execute the pipeline on a root view (e.g. the entire mapped file) using the context.
    pub fn execute<'a>(
        &'a self,
        initial_view: ByteView<'a>,
        ctx: &'a ExecutionContext,
    ) -> Box<dyn Iterator<Item = ByteView<'a>> + 'a> {
        let mut iter: Box<dyn Iterator<Item = ByteView<'a>> + 'a> =
            Box::new(std::iter::once(initial_view));

        for cmd in &self.commands {
            iter = cmd.apply(iter, ctx);
        }

        iter
    }
}
