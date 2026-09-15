//! Asks the running bar to show, hide or toggle the launcher.
//!
//! The launcher itself lives in the bar's process. This is all that is left
//! of the binary that used to be the launcher: a client that connects to the
//! socket and writes one word. Keybinds and scripts that called
//! `caelestia-launcher --toggle` keep working unchanged, and now cost a
//! connect instead of mapping GTK and WebKit into a process that only wanted
//! to send eight bytes.

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    // An optional starting query follows the verb, so a keybind can open
    // straight into a mode: `--show '>wallpaper '`.
    let query = argv.get(1).cloned().unwrap_or_default();

    let word = match argv.first().map(String::as_str) {
        Some("--toggle") => format!("toggle {query}"),
        Some("--show") => format!("show {query}"),
        Some("--hide") => "hide".to_string(),
        // Bare, this used to start the launcher daemon. The bar starts it
        // now, so there is nothing to do — and doing the obvious thing
        // instead, toggling, would pop the launcher open at login for
        // anything still autostarting it.
        None => {
            eprintln!("caelestia-launcher: the launcher runs inside caelestia-bar; nothing to start");
            return;
        }
        Some(other) => {
            eprintln!("caelestia-launcher: unknown option {other}");
            eprintln!("usage: caelestia-launcher [--toggle | --show | --hide] [QUERY]");
            std::process::exit(2);
        }
    };

    if !caelestia_bar_lib::launcher::send_control(&word) {
        eprintln!("caelestia-launcher: nothing is listening; is caelestia-bar running?");
        std::process::exit(1);
    }
}
