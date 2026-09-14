//! `caelestia screenshot` — Print, and the region picker behind it.
//!
//! A fullscreen shot goes to the clipboard and to a cache file, and the
//! notification offers to open or keep it. A region shot either hands the
//! selection to the shell's own picker or crops with grim and opens the
//! editor.

use crate::{clock, hypr, paths, proc};

pub struct Args {
    /// `Some("slurp")` for the shell's picker, `Some(geometry)` for a region
    /// already chosen, `None` for the focused monitor.
    pub region: Option<String>,
    pub freeze: bool,
}

pub fn run(args: &Args) -> i32 {
    match args.region.as_deref() {
        Some("slurp") => open_picker(args.freeze),
        Some(region) => crop(region.trim()),
        None => fullscreen(),
    }
}

fn open_picker(freeze: bool) -> i32 {
    let action = if freeze { "openFreeze" } else { "open" };
    let ok = proc::run("qs", &["-c", "caelestia", "ipc", "call", "picker", action]);
    i32::from(!ok)
}

fn crop(region: &str) -> i32 {
    let Some(image) = proc::capture("grim", &["-l", "0", "-g", region, "-"]) else {
        eprintln!("caelestia: grim could not capture {region}");
        return 1;
    };
    proc::spawn_detached_with_input("swappy", &["-f", "-"], &image);
    0
}

fn fullscreen() -> i32 {
    let monitor = hypr::focused_monitor()
        .and_then(|m| m.str_field("name").map(str::to_string));
    let image = match &monitor {
        Some(name) => proc::capture("grim", &["-o", name, "-"]),
        None => proc::capture("grim", &["-"]),
    };
    let Some(image) = image else {
        eprintln!("caelestia: grim could not take the screenshot");
        return 1;
    };

    proc::pipe("wl-copy", &[], &image);

    let cache = paths::screenshots_cache_dir();
    if std::fs::create_dir_all(&cache).is_err() {
        eprintln!("caelestia: cannot write to {}", cache.display());
        return 1;
    }
    let saved = cache.join(clock::now().compact());
    if std::fs::write(&saved, &image).is_err() {
        eprintln!("caelestia: cannot write {}", saved.display());
        return 1;
    }

    let path = saved.display().to_string();
    let hint = format!("STRING:image-path:{path}");
    let body = format!("Screenshot stored in {path} and copied to clipboard");
    let action = proc::notify(&[
        "-i",
        "image-x-generic-symbolic",
        "-h",
        &hint,
        "--action=open=Open",
        "--action=save=Save",
        "Screenshot taken",
        &body,
    ]);

    match action.as_str() {
        "open" => {
            proc::spawn_detached("swappy", &["-f", &path]);
        }
        "save" => {
            let keep_dir = paths::screenshots_dir();
            let _ = std::fs::create_dir_all(&keep_dir);
            let kept = keep_dir.join(format!("{}.png", saved.file_name().unwrap().to_string_lossy()));
            if std::fs::rename(&saved, &kept).is_ok() {
                proc::notify(&["Screenshot saved", &format!("Saved to {}", kept.display())]);
            }
        }
        _ => {}
    }
    0
}
