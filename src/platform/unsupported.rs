use anyhow::{Result, bail};
use std::path::Path;

#[derive(Debug)]
#[allow(dead_code)]
pub enum WatchEvent {
    FileChanged,
    FileDeleted,
    Interrupted,
}

pub fn run_watch_loop<P, F>(_path: P, _callback: F) -> Result<()>
where
    P: AsRef<Path>,
    F: FnMut(WatchEvent) -> Result<bool>,
{
    bail!("watch mode (-w) currently requires macOS and kqueue")
}
