// The bar is drawn by the webview; a console window would be a second, empty
// one on platforms that have them.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// Caps how many heap arenas glibc creates, here and in every child.
///
/// glibc makes up to eight arenas per core and each keeps its own cache of
/// freed pages, so a process with a handful of busy threads on a sixteen-core
/// machine retains far more than it is using.
///
/// Both halves are needed, and for different processes. `mallopt` applies to
/// arenas this process has yet to create, so it has to run before the threads
/// that would create them — but it does not survive `exec`, and the webview
/// runs in separate `WebKitWebProcess` and `WebKitNetworkProcess` binaries
/// that hold most of the memory between them. The environment variable is
/// what reaches those: glibc reads it before `main` in each one.
///
/// Measured across the bar and its two web processes: 304 MiB with `mallopt`
/// alone, which is what the parent gets, against 223 MiB with both.
fn cap_heap_arenas() {
    // SAFETY: `mallopt` is thread-safe and this runs before any thread is
    // started; a failed call changes nothing and is not worth reacting to.
    unsafe {
        libc::mallopt(libc::M_ARENA_MAX, 2);
    }

    // First statement of `main`, so no other thread can be reading the
    // environment while this writes it. Anything the caller set wins.
    if std::env::var_os("MALLOC_ARENA_MAX").is_none() {
        std::env::set_var("MALLOC_ARENA_MAX", "2");
    }
}

/// Asks the kernel to kill this process when the shell that started it dies.
///
/// The bar is the shell's bar, and a bar left on screen with nothing behind it
/// is worse than no bar: its popouts call a shell that is not there. Quickshell
/// terminating its children covers the orderly cases; this covers the rest,
/// including the shell being killed outright, because the signal comes from the
/// kernel rather than from anything the dying process still has to run.
///
/// Only when the shell says it started us. Run by hand from a terminal the
/// parent is that terminal, and dying with it would be surprising.
fn die_with_the_shell() {
    if std::env::var_os("CAELESTIA_SHELL_MANAGED").is_none() {
        return;
    }
    // SAFETY: `prctl` with PR_SET_PDEATHSIG only sets a flag on this process,
    // and a failed call leaves it unset, which is the behaviour we had before.
    unsafe {
        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
    }
    // The parent can already have died between being forked and getting here,
    // in which case the signal has been and gone and nothing will send another.
    if unsafe { libc::getppid() } == 1 {
        std::process::exit(0);
    }
}

fn main() {
    cap_heap_arenas();
    die_with_the_shell();
    caelestia_bar_lib::start();
}
