mod awk;
mod cli;
mod commands;
mod core;
mod engine;
mod parser;
mod platform;

use crate::core::MmapSource;
use crate::core::{ByteView, ExecutionContext, Mutation};
use clap::Parser as ClapParser;
use std::io::{self, Write};
use std::sync::Arc;

fn main() -> anyhow::Result<()> {
    let args = cli::Cli::parse();

    let mut base_pipeline = parser::parse(&args.program)?;

    // Implicit Awk line-splitting: a program beginning with `/` operates one
    // line at a time, so prepend a line extractor that feeds the predicates.
    if args.program.trim_start().starts_with('/') {
        let mut awk_pipeline = crate::core::Pipeline::new();
        let re = crate::engine::StructuralRegex::compile(r".*\n?")?;
        awk_pipeline.push(Box::new(crate::commands::ExtractCommand { re }));
        for cmd in base_pipeline.commands {
            awk_pipeline.push(cmd);
        }
        base_pipeline = awk_pipeline;
    }

    // Fuse the grep idiom (`x/.*\n/ g/lit/`) into a single search-then-extend
    // pass before execution. Behaviour-preserving; see `parser::optimize`.
    let base_pipeline = parser::optimize(base_pipeline);

    let pipeline = Arc::new(base_pipeline);
    let backup_ext = args.i.clone().flatten();
    let in_place = args.i.is_some();

    if args.files.is_empty() {
        if in_place {
            return Err(anyhow::anyhow!(
                "in-place edit (-i) requires at least one file"
            ));
        }
        let source = MmapSource::from_stdin()?;
        let stdout = io::stdout();
        let mut out = io::BufWriter::new(stdout.lock());
        run_pipeline(&pipeline, &source, &mut out)?;
        out.flush()?;
        return Ok(());
    }

    // Process files sequentially so stdout stays deterministic.
    for file_path in &args.files {
        if args.watch {
            watch_file(&pipeline, file_path)?;
        } else if in_place {
            run_in_place(&pipeline, file_path, backup_ext.as_deref())?;
        } else {
            let source = MmapSource::open(file_path)?;
            let stdout = io::stdout();
            let mut out = io::BufWriter::new(stdout.lock());
            run_pipeline(&pipeline, &source, &mut out)?;
            out.flush()?;
        }
    }

    Ok(())
}

/// Drive the pipeline over `source`, then write the result to `out`: either the
/// original bytes with out-of-band mutations stitched in, or nothing when the
/// program only produced `p`/`=` side effects.
fn run_pipeline<W: Write>(
    pipeline: &crate::core::Pipeline,
    source: &MmapSource,
    out: &mut W,
) -> anyhow::Result<()> {
    let mutations = drive(pipeline, source)?;
    if mutations.is_empty() {
        // Rewrite-mode pipelines (containing s///, c//, r//) emit the document
        // verbatim even when nothing matched, so they behave as stream filters.
        if pipeline.has_mutator() {
            out.write_all(source.as_slice())?;
        }
    } else {
        stitch(source.as_slice(), mutations, out)?;
    }
    Ok(())
}

/// In-place edit: apply the pipeline and atomically rewrite the file. A file
/// with no mutations is left untouched. With `backup_ext`, the original is
/// copied to `<path><ext>` first.
fn run_in_place(
    pipeline: &crate::core::Pipeline,
    path: &str,
    backup_ext: Option<&str>,
) -> anyhow::Result<()> {
    let source = MmapSource::open(path)?;
    let mutations = drive(pipeline, &source)?;
    if mutations.is_empty() {
        return Ok(()); // no edits — leave the file untouched
    }

    let mut buf = Vec::with_capacity(source.len());
    stitch(source.as_slice(), mutations, &mut buf)?;

    if let Some(ext) = backup_ext.filter(|e| !e.is_empty()) {
        std::fs::copy(path, format!("{path}{ext}"))
            .map_err(|e| anyhow::anyhow!("failed writing backup for '{}': {}", path, e))?;
    }

    atomic_write(path, &buf)?;
    Ok(())
}

/// Execute the pipeline (firing side effects) and return its recorded mutations.
/// A runtime error raised by a command (e.g. an awk interpreter fault) is
/// surfaced here so the process exits non-zero instead of silently continuing.
fn drive(pipeline: &crate::core::Pipeline, source: &MmapSource) -> anyhow::Result<Vec<Mutation>> {
    let ctx = ExecutionContext::new(source.as_slice().as_ptr(), source.as_slice().len());
    let initial_view = ByteView::new(source.as_slice());
    // Exhaust the lazy iterator so every side effect (print, mutation) fires.
    let _ = pipeline.execute(initial_view, &ctx).count();
    if let Some(err) = ctx.error.lock().unwrap().take() {
        return Err(err);
    }
    Ok(std::mem::take(&mut *ctx.mutations.lock().unwrap()))
}

/// Write `original` to `out`, replacing each non-overlapping mutated span with
/// its replacement bytes (mutations sorted by start; later overlaps dropped).
/// Returns `true` if anything was written (i.e. there were mutations).
fn stitch<W: Write>(
    original: &[u8],
    mut mutations: Vec<Mutation>,
    out: &mut W,
) -> io::Result<bool> {
    if mutations.is_empty() {
        return Ok(false);
    }
    // A single top-level s///g already yields matches left-to-right, so skip the
    // O(n log n) sort when the mutations are already ordered.
    if !mutations.is_sorted_by_key(|m| m.start) {
        mutations.sort_by_key(|m| m.start);
    }

    let mut cursor = 0;
    for m in mutations {
        if m.start < cursor {
            continue; // skip overlapping / out-of-order mutation
        }
        out.write_all(&original[cursor..m.start])?;
        out.write_all(&m.replacement)?;
        cursor = m.end;
    }
    if cursor < original.len() {
        out.write_all(&original[cursor..])?;
    }
    Ok(true)
}

/// Atomically replace `path`'s contents with `bytes` via a sibling tempfile and
/// rename, so a crash mid-write can never truncate the original.
fn atomic_write(path: &str, bytes: &[u8]) -> anyhow::Result<()> {
    let permissions = std::fs::metadata(path)
        .map_err(|e| anyhow::anyhow!("failed reading metadata for '{}': {}", path, e))?
        .permissions();
    let dir = std::path::Path::new(path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let mut tmp = tempfile::NamedTempFile::new_in(&dir)
        .map_err(|e| anyhow::anyhow!("failed creating tempfile in '{}': {}", dir.display(), e))?;
    tmp.write_all(bytes)
        .map_err(|e| anyhow::anyhow!("failed writing tempfile: {}", e))?;
    tmp.as_file()
        .set_permissions(permissions)
        .map_err(|e| anyhow::anyhow!("failed preserving file permissions: {}", e))?;
    tmp.as_file()
        .sync_all()
        .map_err(|e| anyhow::anyhow!("failed syncing tempfile: {}", e))?;
    tmp.persist(path)
        .map_err(|e| anyhow::anyhow!("failed replacing '{}': {}", path, e))?;
    if let Ok(directory) = std::fs::File::open(&dir) {
        directory
            .sync_all()
            .map_err(|e| anyhow::anyhow!("failed syncing '{}': {}", dir.display(), e))?;
    }
    Ok(())
}

/// Watch `path` with kqueue and re-run the pipeline to stdout on every change.
fn watch_file(pipeline: &Arc<crate::core::Pipeline>, file_path: &str) -> anyhow::Result<()> {
    let watch_path = file_path.to_string();
    let pipeline_ref = Arc::clone(pipeline);

    platform::run_watch_loop(file_path, move |event| {
        match event {
            platform::WatchEvent::Interrupted => {
                eprintln!("\nse: Interrupted, exiting watch.");
                return Ok(false);
            }
            platform::WatchEvent::FileDeleted => {}
            platform::WatchEvent::FileChanged => {}
        }

        match MmapSource::open(&watch_path) {
            Ok(source) => {
                let stdout = io::stdout();
                let mut out = io::BufWriter::new(stdout.lock());
                if let Err(e) = run_pipeline(&pipeline_ref, &source, &mut out) {
                    eprintln!("se: pipeline error: {}", e);
                }
                let _ = out.flush();
            }
            Err(e) => eprintln!("se: failed to re-read '{}': {}", watch_path, e),
        }
        Ok(true)
    })
}
