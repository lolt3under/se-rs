// libc's kqueue constants already match the `kevent` field types on macOS, but
// the explicit casts are kept as defensive documentation of each field's type.
#![allow(clippy::unnecessary_cast)]

use anyhow::{Context, Result, anyhow};
use libc::{
    self, EV_ADD, EV_CLEAR, EV_ENABLE, EVFILT_SIGNAL, EVFILT_VNODE, NOTE_DELETE, NOTE_EXTEND,
    NOTE_WRITE, SIGINT, kevent,
};
use std::fs::File;
use std::os::unix::io::AsRawFd;
use std::path::Path;

/// Events delivered by the kqueue event loop.
#[derive(Debug)]
pub enum WatchEvent {
    /// NOTE_WRITE or NOTE_EXTEND fired — file content changed.
    FileChanged,
    /// NOTE_DELETE fired — file was removed (editor replace / rm).
    /// The loop will attempt to re-open and re-register the inode.
    FileDeleted,
    /// EVFILT_SIGNAL delivered SIGINT — user pressed Ctrl+C.
    Interrupted,
}

/// Registers `EVFILT_VNODE` (NOTE_WRITE | NOTE_EXTEND | NOTE_DELETE) and
/// `EVFILT_SIGNAL` (SIGINT) on a single kqueue and blocks in an event loop,
/// invoking `callback` on each event.
///
/// `callback` returns `Ok(true)` to continue waiting or `Ok(false)` to exit
/// the loop cleanly.
///
/// SIGINT is blocked via `pthread_sigmask` before registration so that the
/// kernel delivers it through kqueue instead of terminating the process.
pub fn run_watch_loop<P, F>(path: P, mut callback: F) -> Result<()>
where
    P: AsRef<Path>,
    F: FnMut(WatchEvent) -> Result<bool>,
{
    // Block SIGINT at the OS level so EVFILT_SIGNAL intercepts it via kqueue
    // rather than the default terminate-the-process action.
    unsafe {
        let mut mask: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut mask);
        libc::sigaddset(&mut mask, SIGINT);
        libc::pthread_sigmask(libc::SIG_BLOCK, &mask, std::ptr::null_mut());
    }

    let kq = unsafe { libc::kqueue() };
    if kq < 0 {
        return Err(anyhow!(
            "kqueue() failed: {}",
            std::io::Error::last_os_error()
        ));
    }

    let mut current_file = File::open(path.as_ref())
        .with_context(|| format!("Cannot open '{}' for kqueue watch", path.as_ref().display()))?;

    register_vnode(kq, current_file.as_raw_fd())?;
    register_signal(kq)?;

    eprintln!(
        "se: Watching '{}' (Ctrl+C to quit)...",
        path.as_ref().display()
    );

    loop {
        let mut ev: kevent = unsafe { std::mem::zeroed() };

        // Block indefinitely until any registered event fires.
        let n = unsafe { kevent(kq, std::ptr::null(), 0, &mut ev, 1, std::ptr::null()) };

        if n < 0 {
            let err = std::io::Error::last_os_error();
            // EINTR means a signal interrupted kevent() itself; restart the wait.
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            unsafe { libc::close(kq) };
            return Err(anyhow!("kevent() wait failed: {}", err));
        }

        if n == 0 {
            continue; // Spurious wakeup (shouldn't occur with infinite timeout)
        }

        if ev.filter == EVFILT_SIGNAL as i16 {
            let cont = callback(WatchEvent::Interrupted)?;
            if !cont {
                break;
            }
            continue;
        }

        // EVFILT_VNODE event
        if ev.fflags & (NOTE_DELETE as u32) != 0 {
            let cont = callback(WatchEvent::FileDeleted)?;
            if !cont {
                break;
            }

            // Attempt to re-open the file (editors like vim/sed -i replace-by-rename).
            // Retry for up to one second in 50 ms increments.
            let mut reopened = false;
            for _ in 0..20 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                if let Ok(f) = File::open(path.as_ref()) {
                    current_file = f;
                    if register_vnode(kq, current_file.as_raw_fd()).is_ok() {
                        reopened = true;
                        break;
                    }
                }
            }

            if !reopened {
                eprintln!(
                    "se: '{}' not recoverable after deletion — stopping watch.",
                    path.as_ref().display()
                );
                break;
            }
        } else {
            // NOTE_WRITE or NOTE_EXTEND: file data changed.
            let cont = callback(WatchEvent::FileChanged)?;
            if !cont {
                break;
            }
        }
    }

    unsafe { libc::close(kq) };
    Ok(())
}

fn register_vnode(kq: libc::c_int, fd: libc::c_int) -> Result<()> {
    let change = kevent {
        ident: fd as libc::uintptr_t,
        filter: EVFILT_VNODE as i16,
        flags: (EV_ADD | EV_ENABLE | EV_CLEAR) as u16,
        fflags: (NOTE_WRITE | NOTE_EXTEND | NOTE_DELETE) as u32,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    let ret = unsafe { kevent(kq, &change, 1, std::ptr::null_mut(), 0, std::ptr::null()) };
    if ret < 0 {
        return Err(anyhow!(
            "kevent vnode registration failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

fn register_signal(kq: libc::c_int) -> Result<()> {
    let change = kevent {
        ident: SIGINT as libc::uintptr_t,
        filter: EVFILT_SIGNAL as i16,
        flags: (EV_ADD | EV_ENABLE) as u16,
        fflags: 0,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    let ret = unsafe { kevent(kq, &change, 1, std::ptr::null_mut(), 0, std::ptr::null()) };
    if ret < 0 {
        return Err(anyhow!(
            "kevent EVFILT_SIGNAL registration failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}
