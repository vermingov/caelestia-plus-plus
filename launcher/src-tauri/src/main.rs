// The window is drawn by the webview; a console window would be a second,
// empty one on platforms that have them.
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

fn main() {
    cap_heap_arenas();
    let argv: Vec<String> = std::env::args().skip(1).collect();
    // An optional starting query follows the verb, so a keybind can open
    // straight into a mode: `--show '>wallpaper '`.
    let query = argv.get(1).cloned().unwrap_or_default();
    let word = match argv.first().map(String::as_str) {
        Some("--toggle") => Some(format!("toggle {query}")),
        Some("--show") => Some(format!("show {query}")),
        Some("--hide") => Some("hide".to_string()),
        Some(other) => {
            eprintln!("caelestia-launcher: unknown option {other}");
            eprintln!("usage: caelestia-launcher [--toggle | --show | --hide] [QUERY]");
            std::process::exit(2);
        }
        None => None,
    };

    // Asked to toggle and one is already running: hand it over and stop. Only
    // when nothing answers does this process become the launcher — and then
    // it shows itself, because someone asked for it.
    if let Some(word) = &word {
        if caelestia_launcher_lib::send_control(word) {
            return;
        }
        if word == "hide" {
            return; // nothing running, nothing to hide
        }
    }

    caelestia_launcher_lib::run();
}
