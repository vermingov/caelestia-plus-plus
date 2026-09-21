//! cae: the Caelestia++ shell.
//!
//! One process and no web engine, and nothing of anybody else's shell under
//! it. The bar is a layer surface fifty pixels tall; everything else it shows
//! is a window of its own that exists only while it is on screen.

mod actions;
mod assets;
mod awake;
mod background;
mod clock;
mod door;
mod ease;
mod battery;
mod feeds;
mod idle;
mod ours;
mod say;
mod setup;
mod theme;
mod ui;

use gpui::App;
use gpui_platform::application;

/// Has glibc give memory back when the shell is done with it.
///
/// It makes up to eight heap arenas per core and each keeps its own cache of
/// freed pages, so a process with a handful of busy threads on a sixteen-core
/// machine retains far more than it uses: two is plenty.
///
/// And it learns. Anything large is given pages of its own, which go back to
/// the kernel when it is freed; but free one such and glibc raises what it
/// counts as large, up to 32 MB, and the next picture opened is carved out of
/// the heap instead, which does not shrink again. A wallpaper opened up is
/// fifty megabytes for a quarter of a second, and the shell was that much
/// bigger for the rest of the day. Saying what large is stops it learning.
fn tame_the_heap() {
    const LARGE: libc::c_int = 256 * 1024;
    // SAFETY: `mallopt` is thread-safe and this runs before any thread is
    // started; a failed call changes nothing.
    unsafe {
        libc::mallopt(libc::M_ARENA_MAX, 2);
        libc::mallopt(libc::M_MMAP_THRESHOLD, LARGE);
    }
}

/// Asks the kernel to end this process when the shell that started it dies.
///
/// While cae is taking Quickshell's place a piece at a time, Quickshell is
/// what starts it, and a bar left on screen with nothing behind it is worse
/// than no bar: half of what it opens belongs to a shell that is not there.
/// Quickshell ending its children covers the orderly cases. This covers the
/// rest, being killed outright included, because the signal comes from the
/// kernel rather than from anything the dying process still has to run.
///
/// Only when the shell says it started this. Run by hand from a terminal the
/// parent is that terminal, and dying with it would be a surprise.
fn die_with_the_shell() {
    if std::env::var_os("CAELESTIA_SHELL_MANAGED").is_none() {
        return;
    }
    // SAFETY: `prctl` with PR_SET_PDEATHSIG only sets a flag on this process,
    // and a failed call leaves it unset, which is how it was before.
    unsafe {
        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
    }
    // The parent may already have died between forking and here, in which
    // case the signal has been and gone and nothing will send another.
    if unsafe { libc::getppid() } == 1 {
        std::process::exit(0);
    }
}

/// Makes sure this is the only cae serving the desktop, and exits if not.
///
/// Two of them is two bars reserving two strips, and the second taking the
/// sockets the first is listening on: the launcher key and the notification
/// bridge would go to whichever started last, and the first would never know.
/// The lock is the kernel's, so it goes when the process does, however it
/// went, and there is never a stale one to clear.
///
/// The exit is a failure on purpose. Whatever started this restarts a shell
/// that exits cleanly, and a second one turned away politely would be started
/// again for ever.
fn be_the_only_one() -> std::fs::File {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map_or_else(|| "/tmp".into(), std::path::PathBuf::from);
    let path = runtime.join("cae-shell.lock");
    let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path);
    let Ok(lock) = lock else {
        eprintln!("cae: cannot open {}, so cannot tell whether another is running", path.display());
        std::process::exit(3);
    };
    // SAFETY: `flock` on a descriptor this function owns, which outlives it.
    let taken = unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&lock), libc::LOCK_EX | libc::LOCK_NB) };
    if taken != 0 {
        eprintln!("cae: another cae-shell is already this desktop's shell");
        std::process::exit(3);
    }
    lock
}

fn main() {
    // Words for the shell that is running, rather than a shell to start.
    let words: Vec<String> = std::env::args().skip(1).collect();
    if door::is_knock(&words) {
        if door::knock(&words) {
            return;
        }
        eprintln!("cae: no shell is running to hear that");
        std::process::exit(1);
    }

    say::start();
    tame_the_heap();
    die_with_the_shell();

    // `--preview` draws the bar under the one that is in use, over everything
    // and reserving nothing, and leaves the notification service alone: a way
    // to look at a build next to the shell that is actually running.
    let preview = words.iter().any(|arg| arg == "--preview");
    // Held for as long as the process lives. A preview is there to be run
    // beside the real one, so it neither asks nor holds.
    let _only = (!preview).then(be_the_only_one);

    application().with_assets(assets::Assets).run(move |cx: &mut App| {
        assets::install_fonts(cx);
        ui::field::bind_keys(cx);
        ui::launcher::bind_keys(cx);
        ui::popout::bind_keys(cx);
        ui::settings::bind_keys(cx);
        ui::session::bind_keys(cx);
        ui::picker::bind_keys(cx);
        ui::guard::bind_keys(cx);
        ui::security::bind_keys(cx);
        ui::features::bind_keys(cx);
        ui::lock::bind_keys(cx);
        setup::bind_keys(cx);

        let feeds = feeds::Feeds::start(cx, !preview);
        cx.set_global(feeds.clone());
        cx.set_global(actions::Desk { server: feeds.server.clone(), serving: feeds.serving });
        ui::launcher::Launchers::start(cx, &feeds);
        let answers = door::open(cx);
        ui::bar::keep_on_every_output(cx, &feeds, preview);
        // A shell that is only being looked at has no notifications of its
        // own to show: they are the other one's, on the other one's surfaces.
        if !preview {
            ui::notifs::keep_on_every_output(cx, &feeds);
        }
        // What opens by itself, when an edge is reached for or a level
        // changes, belongs to the shell that answers for the desktop: one
        // started beside it to be looked at would open a second of each.
        if answers {
            ui::dashboard::keep_on_every_output(cx, &feeds);
            ui::osd::keep_on_every_output(cx, &feeds);
            ui::utilities::keep(cx, &feeds);
            background::keep(cx, &feeds);
            idle::keep(cx, &feeds);
            battery::keep(cx, &feeds);
            setup::keep(cx);
            ui::guard::keep(cx, &feeds);
            ui::security::keep(cx, &feeds);
            ui::features::keep(cx);
            // What was on when the shell last ran: the lid holder is a
            // process, and a process does not survive a restart.
            std::thread::spawn(cae_core::features::resume);
            // The watcher that knocks when somebody types the word for it.
            ui::eggs::watch();
        }
    });
}
