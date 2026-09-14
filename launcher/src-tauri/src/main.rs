// The window is drawn by the webview; a console window would be a second,
// empty one on platforms that have them.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let word = match argv.first().map(String::as_str) {
        Some("--toggle") => Some("toggle"),
        Some("--show") => Some("show"),
        Some("--hide") => Some("hide"),
        Some(other) => {
            eprintln!("caelestia-launcher: unknown option {other}");
            eprintln!("usage: caelestia-launcher [--toggle | --show | --hide]");
            std::process::exit(2);
        }
        None => None,
    };

    // Asked to toggle and one is already running: hand it over and stop. Only
    // when nothing answers does this process become the launcher — and then
    // it shows itself, because someone asked for it.
    if let Some(word) = word {
        if caelestia_launcher_lib::send_control(word) {
            return;
        }
        if word == "hide" {
            return; // nothing running, nothing to hide
        }
    }

    caelestia_launcher_lib::run();
}
