//! `caelestia record` — start, pause or stop a screen recording.
//!
//! Three keybinds land here, and the timing matters more than anywhere else:
//! the stop keybind has to reach the recorder before the moment worth keeping
//! has passed, and an interpreter start in front of it is dead time.
//!
//! The recorder itself is gpu-screen-recorder; this decides what to hand it,
//! where the file goes afterwards, and what the notification offers.

use std::time::{Duration, Instant};

use crate::{clock, config, hypr, paths, proc};

const RECORDER: &str = "gpu-screen-recorder";

pub struct Args {
    pub region: Option<String>,
    pub sound: bool,
    pub pause: bool,
    pub clipboard: bool,
}

pub fn run(args: &Args) -> i32 {
    if args.pause {
        // The recorder toggles its own pause on USR2; there is nothing for
        // us to track.
        proc::run("pkill", &["-USR2", "-f", RECORDER]);
        return 0;
    }
    if recording() {
        stop(args)
    } else {
        start(args)
    }
}

fn recording() -> bool {
    proc::run("pidof", &[RECORDER])
}

fn recording_path() -> std::path::PathBuf {
    paths::caelestia_state_dir().join("record/recording.mp4")
}

fn notification_id_path() -> std::path::PathBuf {
    paths::caelestia_state_dir().join("record/notifid.txt")
}

fn recordings_dir() -> std::path::PathBuf {
    match std::env::var_os("CAELESTIA_RECORDINGS_DIR") {
        Some(v) if !v.is_empty() => std::path::PathBuf::from(v),
        _ => match std::env::var_os("XDG_VIDEOS_DIR") {
            Some(v) if !v.is_empty() => std::path::PathBuf::from(v).join("Recordings"),
            _ => paths::home().join("Videos/Recordings"),
        },
    }
}

fn start(args: &Args) -> i32 {
    let monitors = hypr::monitors();
    let mut recorder_args: Vec<String> = vec!["-w".to_string()];

    match &args.region {
        Some(region) => {
            let region = if region == "slurp" {
                match proc::capture_text("slurp", &["-f", "%wx%h+%x+%y"]) {
                    Some(r) => r.trim().to_string(),
                    None => return 0, // selection cancelled
                }
            } else {
                region.trim().to_string()
            };

            let Some(rect) = parse_region(&region) else {
                eprintln!("caelestia: invalid region: {region}");
                return 1;
            };
            recorder_args.push("region".to_string());
            recorder_args.push("-region".to_string());
            recorder_args.push(region);
            // A region can straddle monitors; record at the fastest one it
            // touches so the smoother screen is not the one that suffers.
            recorder_args.push("-f".to_string());
            recorder_args.push(fastest_rate_over(&monitors, rect).to_string());
        }
        None => {
            let Some(monitor) = monitors.iter().find(|m| m.bool_field("focused", false)) else {
                eprintln!("caelestia: no focused monitor to record");
                return 1;
            };
            let Some(name) = monitor.str_field("name") else { return 1 };
            recorder_args.push(name.to_string());
            recorder_args.push("-f".to_string());
            recorder_args.push(refresh_rate(monitor).to_string());
        }
    }

    if args.sound {
        recorder_args.push("-a".to_string());
        recorder_args.push("default_output".to_string());
    }
    recorder_args.extend(extra_args());

    let output = recording_path();
    if let Some(dir) = output.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    recorder_args.push("-o".to_string());
    recorder_args.push(output.display().to_string());

    let borrowed: Vec<&str> = recorder_args.iter().map(String::as_str).collect();
    let Some(mut child) = proc::spawn_detached(RECORDER, &borrowed) else {
        eprintln!("caelestia: cannot start {RECORDER}");
        return 1;
    };

    let notif = proc::notify(&["-p", "Recording started", "Recording..."]);
    let _ = std::fs::write(notification_id_path(), &notif);

    // A recorder that dies immediately — no permission, a bad argument — must
    // not leave a "Recording..." notification sitting there forever.
    if let Some(status) = wait_briefly(&mut child, Duration::from_secs(1)) {
        if !status.success() {
            proc::close_notification(&notif);
            proc::notify(&[
                "Recording failed",
                &format!("{RECORDER} exited with {status}"),
            ]);
            return 1;
        }
    }
    0
}

fn stop(args: &Args) -> i32 {
    proc::run("pkill", &["-f", RECORDER]);

    // The file is only complete once the recorder has finished writing its
    // trailer; moving it early produces a video nothing will play.
    let deadline = Instant::now() + Duration::from_secs(30);
    while recording() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }

    let dir = recordings_dir();
    let _ = std::fs::create_dir_all(&dir);
    let saved = dir.join(format!("recording_{}.mp4", clock::now().recording()));
    if let Err(e) = move_file(&recording_path(), &saved) {
        eprintln!("caelestia: cannot save the recording: {e}");
        return 1;
    }

    if let Ok(id) = std::fs::read_to_string(notification_id_path()) {
        proc::close_notification(id.trim());
    }

    if args.clipboard {
        let uri = format!("{}\n", file_uri(&saved));
        proc::pipe("wl-copy", &["--type", "text/uri-list"], uri.as_bytes());
    }

    let path = saved.display().to_string();
    let action = proc::notify(&[
        "--action=watch=Watch",
        "--action=open=Open",
        "--action=delete=Delete",
        "Recording stopped",
        &format!("Recording saved in {path}"),
    ]);

    match action.as_str() {
        "watch" => {
            proc::spawn_detached("xdg-open", &[&path]);
        }
        "open" => show_in_file_manager(&saved),
        "delete" => {
            let _ = std::fs::remove_file(&saved);
        }
        _ => {}
    }
    0
}

/// Rename where possible, copy when the recording and the library are on
/// different filesystems — a home on one disk and videos on another is
/// ordinary, and rename cannot cross that line.
fn move_file(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            std::fs::copy(from, to)?;
            std::fs::remove_file(from)
        }
    }
}

fn show_in_file_manager(path: &std::path::Path) {
    let uri = format!("array:string:{}", file_uri(path));
    let shown = proc::run(
        "dbus-send",
        &[
            "--session",
            "--dest=org.freedesktop.FileManager1",
            "--type=method_call",
            "/org/freedesktop/FileManager1",
            "org.freedesktop.FileManager1.ShowItems",
            &uri,
            "string:",
        ],
    );
    if !shown {
        if let Some(parent) = path.parent() {
            proc::spawn_detached("xdg-open", &[&parent.display().to_string()]);
        }
    }
}

fn extra_args() -> Vec<String> {
    let config = config::user_config();
    match config.get("record").and_then(|r| r.get("extraArgs")) {
        Some(redcommon::json::Json::Arr(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

fn refresh_rate(monitor: &redcommon::json::Json) -> i64 {
    monitor
        .get("refreshRate")
        .and_then(|r| match r {
            redcommon::json::Json::Num(n) => Some(n.round() as i64),
            _ => None,
        })
        .unwrap_or(60)
}

/// `WIDTHxHEIGHT+X+Y`, the format slurp was asked for.
fn parse_region(region: &str) -> Option<(i64, i64, i64, i64)> {
    let (size, offset) = region.split_once('+')?;
    let (width, height) = size.split_once('x')?;
    let (x, y) = offset.split_once('+')?;
    Some((
        x.trim().parse().ok()?,
        y.trim().parse().ok()?,
        width.trim().parse().ok()?,
        height.trim().parse().ok()?,
    ))
}

fn fastest_rate_over(monitors: &[redcommon::json::Json], rect: (i64, i64, i64, i64)) -> i64 {
    let (x, y, w, h) = rect;
    let mut fastest = 0;
    for monitor in monitors {
        let mx = monitor.get("x").and_then(number).unwrap_or(0);
        let my = monitor.get("y").and_then(number).unwrap_or(0);
        let mw = monitor.get("width").and_then(number).unwrap_or(0);
        let mh = monitor.get("height").and_then(number).unwrap_or(0);
        if x < mx + mw && x + w > mx && y < my + mh && y + h > my {
            fastest = fastest.max(refresh_rate(monitor));
        }
    }
    if fastest == 0 {
        60
    } else {
        fastest
    }
}

fn number(v: &redcommon::json::Json) -> Option<i64> {
    match v {
        redcommon::json::Json::Num(n) => Some(*n as i64),
        _ => None,
    }
}

/// `file:///path`, percent-encoded the way a URI has to be.
fn file_uri(path: &std::path::Path) -> String {
    let absolute = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut uri = String::from("file://");
    for byte in absolute.display().to_string().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                uri.push(byte as char)
            }
            other => uri.push_str(&format!("%{other:02X}")),
        }
    }
    uri
}

/// Wait a moment for a process that should still be running, without
/// blocking on one that is.
fn wait_briefly(child: &mut std::process::Child, limit: Duration) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => return None,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use redcommon::json;

    #[test]
    fn reads_a_slurp_region_back() {
        assert_eq!(parse_region("1920x1080+0+0"), Some((0, 0, 1920, 1080)));
        assert_eq!(parse_region(" 800x600+100+50 "), Some((100, 50, 800, 600)));
        assert_eq!(parse_region("nonsense"), None);
        assert_eq!(parse_region("800x600"), None);
    }

    #[test]
    fn a_region_records_at_the_fastest_screen_it_touches() {
        let monitors = vec![
            json::obj([
                ("x", json::n(0u32)),
                ("y", json::n(0u32)),
                ("width", json::n(1920u32)),
                ("height", json::n(1080u32)),
                ("refreshRate", json::n(59.997)),
            ]),
            json::obj([
                ("x", json::n(1920u32)),
                ("y", json::n(0u32)),
                ("width", json::n(2560u32)),
                ("height", json::n(1440u32)),
                ("refreshRate", json::n(143.998)),
            ]),
        ];
        // Wholly on the slow screen
        assert_eq!(fastest_rate_over(&monitors, (0, 0, 800, 600)), 60);
        // Straddling both
        assert_eq!(fastest_rate_over(&monitors, (1800, 0, 400, 400)), 144);
        // Off every screen: a sane default rather than zero frames a second
        assert_eq!(fastest_rate_over(&monitors, (9000, 9000, 10, 10)), 60);
    }

    #[test]
    fn a_path_with_spaces_still_makes_a_valid_uri() {
        let uri = file_uri(std::path::Path::new("/tmp/a b/c#d.mp4"));
        assert!(uri.starts_with("file:///"));
        assert!(uri.contains("%20"), "{uri}");
        assert!(uri.contains("%23"), "{uri}");
        assert!(!uri.contains(' '));
    }
}
