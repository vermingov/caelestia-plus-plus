//! Keeping the app list level with the disk.
//!
//! The list used to be rebuilt only when the launcher was closed, which means
//! anything installed while it sat idle was invisible for the whole of the
//! next open — and for every open after that, if the launcher never closed
//! the long way. A package manager writes a desktop entry the moment it
//! installs an app, so that write is the signal worth listening to.
//!
//! Watching costs one inotify descriptor per directory and wakes nothing up
//! until a file is written, so it is cheaper than the rebuild-on-close it
//! replaces. Asked of the kernel directly, for the four things that change
//! which entries there are and nothing else: the watcher this replaced also
//! heard every time anything so much as opened an entry to read it.

use std::fs::File;
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
#[cfg(feature = "tauri-ui")]
use std::sync::Mutex;
use std::time::Duration;

#[cfg(feature = "tauri-ui")]
use tauri::{AppHandle, Manager};

#[cfg(feature = "tauri-ui")]
use super::apps;
#[cfg(feature = "tauri-ui")]
use super::Launcher;

/// An install writes a directory's worth of files, and a `pacman -Syu` writes
/// several directories' worth. Collecting the burst and reloading once at the
/// end of it is the difference between one rebuild and two hundred.
const SETTLE: Duration = Duration::from_millis(250);

/// A file appearing, being written, going, or being renamed in or out.
const CHANGES: u32 =
    libc::IN_CREATE | libc::IN_CLOSE_WRITE | libc::IN_DELETE | libc::IN_MOVED_FROM | libc::IN_MOVED_TO;

/// The fixed part of an `inotify_event`; the name follows it, `len` bytes.
const HEADER: usize = std::mem::size_of::<libc::inotify_event>();

/// Whether a buffer of events says anything about a desktop entry. A plain
/// write to an entry already listed still counts — an update can rename one
/// or hide it — and a queue that overflowed may have said anything.
fn touches_entries(events: &[u8]) -> bool {
    let mut at = 0;
    while at + HEADER <= events.len() {
        // SAFETY: the kernel writes whole events, and the header is read
        // unaligned from within the bounds just checked.
        let event = unsafe { std::ptr::read_unaligned(events[at..].as_ptr().cast::<libc::inotify_event>()) };
        let name = &events[at + HEADER..(at + HEADER + event.len as usize).min(events.len())];
        let name = &name[..name.iter().position(|byte| *byte == 0).unwrap_or(name.len())];
        if event.mask & libc::IN_Q_OVERFLOW != 0 || name.ends_with(b".desktop") {
            return true;
        }
        at += HEADER + event.len as usize;
    }
    false
}

/// Calls `on_change` once per burst of desktop-entry changes under `dirs`,
/// forever. Returns only when watching could not be set up at all.
///
/// Split out from the thread below so the debouncing and the filtering can be
/// tested against a real directory rather than reasoned about.
pub fn watch_dirs(dirs: Vec<PathBuf>, mut on_change: impl FnMut()) -> Result<(), String> {
    // SAFETY: a plain system call; the descriptor is owned from here on.
    let descriptor = unsafe { libc::inotify_init1(libc::IN_CLOEXEC) };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    // SAFETY: a descriptor just made and owned by nothing else.
    let mut inotify = unsafe { File::from_raw_fd(descriptor) };

    // A directory that does not exist yet is not an error: plenty of machines
    // have no `~/.local/share/applications` until something writes one. The
    // rest are still watched.
    let watched = dirs
        .iter()
        .filter_map(|dir| std::ffi::CString::new(dir.as_os_str().as_bytes()).ok())
        // SAFETY: a valid descriptor and a NUL-terminated path.
        .filter(|dir| unsafe { libc::inotify_add_watch(inotify.as_raw_fd(), dir.as_ptr(), CHANGES) } >= 0)
        .count();
    if watched == 0 {
        return Err("no application directory could be watched".to_string());
    }

    // Room for a few dozen events with long names at a time.
    let mut events = vec![0u8; 16 * 1024];
    loop {
        let read = read_events(&mut inotify, &mut events)?;
        if !touches_entries(&events[..read]) {
            continue;
        }
        // Drain the rest of the burst before reading anything, so a
        // multi-package install rebuilds the list once.
        while crate::children::readable(&inotify, Some(SETTLE)) {
            read_events(&mut inotify, &mut events)?;
        }
        on_change();
    }
}

/// The next events there are, waiting for them. A read cut short by a
/// signal is read again rather than taken for the end of watching.
fn read_events(inotify: &mut File, events: &mut [u8]) -> Result<usize, String> {
    loop {
        match inotify.read(events) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            read => return read.map_err(|error| error.to_string()),
        }
    }
}

/// Watches every directory the app list is built from and swaps in a fresh
/// list whenever one of them changes.
///
/// Nothing here can fail in a way worth surfacing: a launcher whose watcher
/// did not start is the launcher as it behaved before, not a broken one, so
/// the failure is a line on stderr and the bar carries on.
#[cfg(feature = "tauri-ui")]
pub fn applications(app: AppHandle) {
    std::thread::spawn(move || {
        let reload = || {
            // Read before the lock is taken, so a keystroke is never waiting
            // behind a directory walk.
            let fresh = super::Apps::load();
            let Some(state) = app.try_state::<Mutex<Launcher>>() else { return };
            let Ok(mut launcher) = state.lock() else { return };
            launcher.take_apps(fresh);
        };

        if let Err(e) = watch_dirs(apps::application_dirs(), reload) {
            eprintln!("caelestia-launcher: not watching for new apps: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::mpsc::RecvTimeoutError;

    /// A directory of our own, removed when the test ends. Writing one is
    /// less code than taking a dependency for it.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("caelestia-watch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the scratch directory can be made");
        dir
    }

    #[test]
    fn a_new_desktop_entry_wakes_the_watcher_and_an_unrelated_file_does_not() {
        let dir = scratch("entries");
        let (tx, rx) = std::sync::mpsc::channel();

        let watched = dir.clone();
        std::thread::spawn(move || {
            let _ = watch_dirs(vec![watched], move || {
                let _ = tx.send(());
            });
        });
        // The watch is set up on that thread; nothing written before it lands
        // is seen.
        std::thread::sleep(Duration::from_millis(300));

        std::fs::write(dir.join("notes.txt"), "not an app").unwrap();
        assert_eq!(
            rx.recv_timeout(Duration::from_millis(600)),
            Err(RecvTimeoutError::Timeout),
            "a file that is not a desktop entry rebuilt the app list"
        );

        std::fs::write(dir.join("brand-new.desktop"), "[Desktop Entry]\nType=Application\nName=New\nExec=new\n").unwrap();
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(3)),
            Ok(()),
            "installing an app did not rebuild the list"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_burst_of_entries_rebuilds_the_list_once() {
        let dir = scratch("burst");
        let (tx, rx) = std::sync::mpsc::channel();

        let watched = dir.clone();
        std::thread::spawn(move || {
            let _ = watch_dirs(vec![watched], move || {
                let _ = tx.send(());
            });
        });
        std::thread::sleep(Duration::from_millis(300));

        // What a multi-package install looks like from here.
        for i in 0..40 {
            std::fs::write(
                dir.join(format!("app-{i}.desktop")),
                format!("[Desktop Entry]\nType=Application\nName=App {i}\nExec=app{i}\n"),
            )
            .unwrap();
        }

        assert_eq!(rx.recv_timeout(Duration::from_secs(3)), Ok(()), "the burst never rebuilt the list");
        // Everything after the first is the same burst, so there is no second.
        assert_eq!(
            rx.recv_timeout(Duration::from_millis(600)),
            Err(RecvTimeoutError::Timeout),
            "forty entries rebuilt the list more than once"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
