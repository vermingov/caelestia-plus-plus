//! `caelestia screenshot` — Print, and the region picker behind it.
//!
//! A fullscreen shot goes to the clipboard and to a cache file, and the
//! notification offers to open or keep it. A region shot either hands the
//! selection to the shell's own picker or crops with grim and opens the
//! editor — or, asked for, copies it and says so.

use crate::cmd::shell;
use crate::{clock, hypr, paths, proc};

pub struct Args {
    /// `Some("slurp")` for the shell's picker, `Some(geometry)` for a region
    /// already chosen, `None` for the focused monitor.
    pub region: Option<String>,
    pub freeze: bool,
    /// To the clipboard rather than to the editor.
    pub clipboard: bool,
    /// A picture that has already been taken, to be used instead of taking
    /// one. The shell's picker holds the screen still by photographing it and
    /// drawing that; cropping the live screen afterwards throws that away and
    /// takes whatever is there a moment later, which is not what was chosen.
    pub from: Option<String>,
}

pub fn run(args: &Args) -> i32 {
    if let Some(path) = args.from.as_deref() {
        return already_taken(path, args.clipboard);
    }
    match args.region.as_deref() {
        Some("slurp") => open_picker(args),
        Some(region) => crop(region.trim(), args.clipboard),
        None => fullscreen(),
    }
}

fn open_picker(args: &Args) -> i32 {
    // The shell that is running: cae answers at its door, and the QML one
    // over its own IPC. Whichever is there is the one with a picker.
    let mut words = vec!["picker"];
    if args.freeze {
        words.push("freeze");
    }
    if args.clipboard {
        words.push("clip");
    }
    if shell::knock(&words) {
        return 0;
    }
    let action = match (args.freeze, args.clipboard) {
        (true, true) => "openFreezeClip",
        (true, false) => "openFreeze",
        (false, true) => "openClip",
        (false, false) => "open",
    };
    i32::from(!proc::run("qs", &["-c", "caelestia", "ipc", "call", "picker", action]))
}

/// Does the rest of what a screenshot is — the editor, or the clipboard and
/// a word about it — to a picture somebody else has already taken.
fn already_taken(path: &str, clipboard: bool) -> i32 {
    let Ok(image) = std::fs::read(path) else {
        eprintln!("caelestia: cannot read {path}");
        return 1;
    };
    let _ = std::fs::remove_file(path);
    finish(image, clipboard)
}

fn crop(region: &str, clipboard: bool) -> i32 {
    let Some(image) = proc::capture("grim", &["-l", "0", "-g", region, "-"]) else {
        eprintln!("caelestia: grim could not capture {region}");
        return 1;
    };
    finish(image, clipboard)
}

fn finish(image: Vec<u8>, clipboard: bool) -> i32 {
    if !clipboard {
        proc::spawn_detached_with_input("swappy", &["-f", "-"], &image);
        return 0;
    }
    proc::pipe("wl-copy", &["--type", "image/png"], &image);
    proc::spawn_detached(
        "notify-send",
        &["-a", "caelestia-cli", "Screenshot taken", "Screenshot copied to clipboard"],
    );
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
