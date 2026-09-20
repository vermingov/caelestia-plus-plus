//! Keeping the app list level with the disk.
//!
//! The list used to be rebuilt only when the launcher was closed, which means
//! anything installed while it sat idle was invisible for the whole of the
//! next open — and for every open after that, if the launcher never closed
//! the long way. A package manager writes a desktop entry the moment it
//! installs an app, so that write is the signal worth listening to.
//!
//! Watching costs one inotify descriptor per directory and wakes nothing up
//! until a file appears, so it is cheaper than the rebuild-on-close it
//! replaces.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use notify::{Event, EventKind, RecursiveMode, Watcher};
use tauri::{AppHandle, Manager};

use crate::apps;
use crate::Launcher;

/// An install writes a directory's worth of files, and a `pacman -Syu` writes
/// several directories' worth. Collecting the burst and reloading once at the
/// end of it is the difference between one rebuild and two hundred.
const SETTLE: Duration = Duration::from_millis(250);

/// Whether an event can change which apps exist. A plain write to an entry
/// already listed still counts — an update can rename one or hide it.
fn touches_entries(event: &Event) -> bool {
    if !matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)) {
        return false;
    }
    event.paths.iter().any(|p| p.extension().and_then(|e| e.to_str()) == Some("desktop"))
}

/// Calls `on_change` once per burst of desktop-entry changes under `dirs`,
/// forever. Returns only when watching could not be set up at all.
///
/// Split out from the thread below so the debouncing and the filtering can be
/// tested against a real directory rather than reasoned about.
fn watch_dirs(dirs: Vec<PathBuf>, mut on_change: impl FnMut()) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = notify::recommended_watcher(tx).map_err(|e| e.to_string())?;

    // A directory that does not exist yet is not an error: plenty of machines
    // have no `~/.local/share/applications` until something writes one. The
    // rest are still watched.
    let watched = dirs.iter().filter(|dir| watcher.watch(dir, RecursiveMode::NonRecursive).is_ok()).count();
    if watched == 0 {
        return Err("no application directory could be watched".to_string());
    }

    while let Ok(event) = rx.recv() {
        if !event.map(|e| touches_entries(&e)).unwrap_or(false) {
            continue;
        }
        // Drain the rest of the burst before reading anything, so a
        // multi-package install rebuilds the list once.
        while rx.recv_timeout(SETTLE).is_ok() {}
        on_change();
    }
    Ok(())
}

/// Watches every directory the app list is built from and swaps in a fresh
/// list whenever one of them changes.
///
/// Nothing here can fail in a way worth surfacing: a launcher whose watcher
/// did not start is the launcher as it behaved before, not a broken one, so
/// the failure is a line on stderr and the caller carries on.
pub fn applications(app: AppHandle) {
    std::thread::spawn(move || {
        let reload = || {
            // Read before the lock is taken, so a keystroke is never waiting
            // behind a directory walk.
            let fresh = apps::load();
            let Some(state) = app.try_state::<Mutex<Launcher>>() else { return };
            let Ok(mut launcher) = state.lock() else { return };
            // A new app brings new icons with it, and a name that resolved to
            // nothing before is exactly the name that has a file now, so the
            // negative results cached against it have to go as well.
            launcher.icons.forget();
            launcher.apps = fresh;
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
