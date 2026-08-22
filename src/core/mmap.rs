use anyhow::{Context, Result};
use memmap2::{Mmap, MmapOptions};
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use tempfile::tempfile;

#[cfg(target_os = "macos")]
use libc::{F_NOCACHE, MADV_SEQUENTIAL, MADV_WILLNEED, c_void, fcntl, madvise};
#[cfg(target_os = "macos")]
use std::os::unix::io::AsRawFd;

/// Keep this many stdin bytes in an anonymous mapping before spilling to disk.
const STDIN_MEMORY_LIMIT: usize = 4 * 1024 * 1024;

/// Size of each read from standard input.
const READ_CHUNK: usize = 65536;

/// Read-only mapped input.
pub struct MmapSource {
    mmap: Mmap,
    len: usize,
    _file: Option<File>,
}

impl MmapSource {
    /// Open a file and map it for sequential reads.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(&path)
            .with_context(|| format!("failed to open '{}'", path.as_ref().display()))?;
        Self::from_file(file)
    }

    /// Read stdin into an anonymous mapping, spilling larger input to a
    /// temporary file-backed mapping.
    pub fn from_stdin() -> Result<Self> {
        let mut buffer = MmapOptions::new()
            .len(STDIN_MEMORY_LIMIT)
            .map_anon()
            .context("failed to allocate stdin buffer")?;

        let stdin = io::stdin();
        let mut handle = stdin.lock();
        let mut write_pos = 0usize;
        let mut spill: Option<File> = None;
        let mut chunk = [0u8; READ_CHUNK];

        loop {
            let n = handle.read(&mut chunk).context("failed to read stdin")?;
            if n == 0 {
                break;
            }

            if spill.is_none() && write_pos + n <= STDIN_MEMORY_LIMIT {
                buffer[write_pos..write_pos + n].copy_from_slice(&chunk[..n]);
                write_pos += n;
            } else {
                if spill.is_none() {
                    let mut f = tempfile().context("failed to create stdin spill file")?;
                    io::Write::write_all(&mut f, &buffer[..write_pos])
                        .context("failed to write buffered stdin to spill file")?;
                    spill = Some(f);
                }
                io::Write::write_all(spill.as_mut().unwrap(), &chunk[..n])
                    .context("failed to write stdin to spill file")?;
            }
        }

        if let Some(file) = spill {
            Self::from_file(file)
        } else {
            let mmap = buffer
                .make_read_only()
                .context("failed to make stdin mapping read-only")?;
            Ok(Self {
                mmap,
                len: write_pos,
                _file: None,
            })
        }
    }

    fn from_file(file: File) -> Result<Self> {
        let len = usize::try_from(file.metadata()?.len()).context("file is too large to map")?;
        if len == 0 {
            // mmap(2) rejects a zero-length mapping. Keep a one-byte anonymous
            // map behind a logical length of zero so empty files behave like
            // empty stdin.
            let mmap = MmapOptions::new().len(1).map_anon()?.make_read_only()?;
            return Ok(Self {
                mmap,
                len: 0,
                _file: Some(file),
            });
        }

        // Bypass the macOS unified buffer cache for files we mmap directly,
        // preventing double-buffering between the UBC and our mmap pages.
        #[cfg(target_os = "macos")]
        unsafe {
            fcntl(file.as_raw_fd(), F_NOCACHE, 1);
        }

        let mmap = unsafe { MmapOptions::new().map(&file)? };
        #[cfg(target_os = "macos")]
        unsafe {
            madvise(
                mmap.as_ptr() as *mut c_void,
                mmap.len(),
                MADV_SEQUENTIAL | MADV_WILLNEED,
            );
        }

        Ok(Self {
            mmap,
            len,
            _file: Some(file),
        })
    }

    /// Return the valid input bytes.
    #[inline(always)]
    pub fn as_slice(&self) -> &[u8] {
        &self.mmap[..self.len]
    }

    /// Return the valid input length.
    pub fn len(&self) -> usize {
        self.len
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}
