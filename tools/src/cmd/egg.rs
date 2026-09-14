//! `caelestia-tools egg-watch` — the desktop easter eggs.
//!
//! Typing a trigger word with nothing focused pops a surprise through the
//! shell's IPC. It reads keyboard event devices directly, which is why it
//! needs membership of the `input` group (or an ACL grant), and why it is
//! careful about what it keeps: only the last few letter keycodes, in memory,
//! never written anywhere.
//!
//! This one runs for the life of the session, so it used to be a Python
//! interpreter resident the whole time for the sake of a joke.

use std::io::Read;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// struct input_event: a timeval, then type, code, value.
const EVENT_SIZE: usize = 24;
const EV_KEY: u16 = 1;
const KEY_DOWN: i32 = 1;

/// The qwerty letter rows: KEY_Q..KEY_P, KEY_A..KEY_L, KEY_Z..KEY_M.
fn is_letter(code: u16) -> bool {
    (16..=25).contains(&code) || (30..=38).contains(&code) || (44..=50).contains(&code)
}

/// Trigger words as keycodes, and the shell target each one pops.
const SEQUENCES: [(&[u16], &str); 2] = [
    (&[25, 18, 49, 23, 31], "easterEgg"),        // p-e-n-i-s
    (&[23, 31, 19, 30, 18, 38], "israelEgg"),    // i-s-r-a-e-l
];

const RESCAN: Duration = Duration::from_secs(15);
const COOLDOWN: Duration = Duration::from_secs(5);

pub fn run(args: &[String]) -> i32 {
    let test = args.iter().any(|a| a == "--test");

    // In test mode there is no lock and nothing pops: it runs alongside the
    // live watcher and prints what it sees, which is how "I type it and
    // nothing happens" gets diagnosed.
    let _lock = if test { None } else { Some(acquire_lock()) };

    let mut keyboards = open_keyboards(test);
    if test {
        println!(
            "[test] {} device(s) open; every keypress prints below. Ctrl-C to quit",
            keyboards.len()
        );
    }

    let longest = SEQUENCES.iter().map(|(seq, _)| seq.len()).max().unwrap_or(0);
    let mut recent: Vec<u16> = Vec::with_capacity(longest);
    let mut last_scan = Instant::now();
    let mut last_pop: Option<Instant> = None;

    loop {
        if last_scan.elapsed() > RESCAN {
            // Reopen only when the set of devices actually changed, so a
            // hotplug is picked up without churning file descriptors.
            let current: Vec<PathBuf> = keyboard_paths();
            let open: Vec<PathBuf> = keyboards.iter().map(|k| k.path.clone()).collect();
            if current != open {
                keyboards = open_keyboards(test);
            }
            last_scan = Instant::now();
        }

        if keyboards.is_empty() {
            std::thread::sleep(RESCAN);
            last_scan = Instant::now() - RESCAN;
            continue;
        }

        let ready = wait_for_input(&keyboards, RESCAN);
        for index in ready {
            let mut buffer = [0u8; EVENT_SIZE * 64];
            let read = match keyboards[index].file.read(&mut buffer) {
                Ok(0) => continue,
                Ok(n) => n,
                Err(_) => {
                    keyboards.remove(index);
                    break; // indices past this one have shifted
                }
            };

            for chunk in buffer[..read].chunks_exact(EVENT_SIZE) {
                let kind = u16::from_ne_bytes([chunk[16], chunk[17]]);
                let code = u16::from_ne_bytes([chunk[18], chunk[19]]);
                let value = i32::from_ne_bytes([chunk[20], chunk[21], chunk[22], chunk[23]]);
                if kind != EV_KEY || value != KEY_DOWN {
                    continue;
                }
                if test {
                    println!(
                        "[test] keydown code={code} {} from {}",
                        if is_letter(code) { "letter" } else { "other" },
                        keyboards[index].path.display()
                    );
                }
                if !is_letter(code) {
                    recent.clear();
                    continue;
                }

                recent.push(code);
                if recent.len() > longest {
                    let excess = recent.len() - longest;
                    recent.drain(..excess);
                }

                let Some(target) = matched(&recent) else { continue };
                if test {
                    println!(
                        "[test] SEQUENCE DETECTED ({target}); gate desktop_is_focused() = {}",
                        desktop_is_focused()
                    );
                    recent.clear();
                    continue;
                }
                if last_pop.is_some_and(|at| at.elapsed() <= COOLDOWN) {
                    continue;
                }
                recent.clear();
                if desktop_is_focused() {
                    last_pop = Some(Instant::now());
                    pop(target);
                }
            }
        }
    }
}

fn matched(recent: &[u16]) -> Option<&'static str> {
    SEQUENCES.iter().find_map(|(sequence, target)| {
        (recent.len() >= sequence.len() && &recent[recent.len() - sequence.len()..] == *sequence)
            .then_some(*target)
    })
}

struct Keyboard {
    path: PathBuf,
    file: std::fs::File,
}

/// Devices with a kbd handler. Anything else there never emits letters.
fn keyboard_paths() -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string("/proc/bus/input/devices") else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    for line in text.lines() {
        if !line.starts_with("H: Handlers=") || !line.contains("kbd") {
            continue;
        }
        for token in line.split_whitespace() {
            if token.starts_with("event") {
                paths.push(PathBuf::from("/dev/input").join(token));
            }
        }
    }
    paths
}

fn open_keyboards(test: bool) -> Vec<Keyboard> {
    let mut keyboards = Vec::new();
    for path in keyboard_paths() {
        match std::fs::File::open(&path) {
            Ok(file) => {
                if test {
                    println!("[test] opened {}", path.display());
                }
                keyboards.push(Keyboard { path, file });
            }
            Err(e) => {
                // Not readable yet — a later rescan picks it up once the ACL
                // or group membership lands.
                if test {
                    println!("[test] FAILED to open {}: {e}", path.display());
                }
            }
        }
    }
    keyboards
}

/// Indices of the devices with something to read, or none after `timeout`.
fn wait_for_input(keyboards: &[Keyboard], timeout: Duration) -> Vec<usize> {
    #[repr(C)]
    struct PollFd {
        fd: i32,
        events: i16,
        revents: i16,
    }
    const POLLIN: i16 = 1;

    let mut fds: Vec<PollFd> = keyboards
        .iter()
        .map(|k| PollFd {
            fd: k.file.as_raw_fd(),
            events: POLLIN,
            revents: 0,
        })
        .collect();

    let ready = unsafe {
        poll(
            fds.as_mut_ptr() as *mut u8,
            fds.len() as u64,
            timeout.as_millis() as i32,
        )
    };
    if ready <= 0 {
        return Vec::new();
    }
    fds.iter()
        .enumerate()
        .filter(|(_, fd)| fd.revents & POLLIN != 0)
        .map(|(i, _)| i)
        .collect()
}

/// True when nothing is focused, or when the workspace under the cursor is
/// empty — on more than one monitor, a window elsewhere can hold focus while
/// the user types at a bare desktop.
fn desktop_is_focused() -> bool {
    let Some(active) = hyprctl(&["activewindow"]) else { return false };
    if !active.contains("\"address\"") {
        // hyprctl prints "Invalid" rather than JSON when nothing is focused
        return true;
    }
    cursor_workspace_is_empty()
}

fn cursor_workspace_is_empty() -> bool {
    use redcommon::json::Json;

    let Some(cursor) = hyprctl(&["cursorpos"]).and_then(|t| redcommon::json::parse(t.trim()))
    else {
        return false;
    };
    let (Some(cx), Some(cy)) = (number(&cursor, "x"), number(&cursor, "y")) else {
        return false;
    };

    let Some(Json::Arr(monitors)) =
        hyprctl(&["monitors"]).and_then(|t| redcommon::json::parse(t.trim()))
    else {
        return false;
    };

    for monitor in monitors {
        let (Some(mx), Some(my), Some(width), Some(height)) = (
            number(&monitor, "x"),
            number(&monitor, "y"),
            number(&monitor, "width"),
            number(&monitor, "height"),
        ) else {
            continue;
        };
        let scale = number(&monitor, "scale").filter(|s| *s != 0.0).unwrap_or(1.0);
        if cx < mx || cx >= mx + width / scale || cy < my || cy >= my + height / scale {
            continue;
        }

        let Some(active) = monitor.get("activeWorkspace").and_then(|w| number(w, "id")) else {
            return false;
        };
        let Some(Json::Arr(workspaces)) =
            hyprctl(&["workspaces"]).and_then(|t| redcommon::json::parse(t.trim()))
        else {
            return false;
        };
        return workspaces.iter().any(|w| {
            number(w, "id") == Some(active) && number(w, "windows").unwrap_or(1.0) == 0.0
        });
    }
    false
}

fn number(value: &redcommon::json::Json, key: &str) -> Option<f64> {
    match value.get(key) {
        Some(redcommon::json::Json::Num(n)) => Some(*n),
        _ => None,
    }
}

fn hyprctl(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("hyprctl")
        .args(args)
        .arg("-j")
        .output()
        .ok()?;
    String::from_utf8(output.stdout).ok()
}

/// A hung or missing shell must never take the watcher down with it: nothing
/// restarts this.
fn pop(target: &str) {
    let _ = std::process::Command::new("qs")
        .args(["-c", "caelestia", "ipc", "call", target, "pop"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// One watcher at a time.
///
/// The shell spawns this at startup and a user may autostart a copy too. A
/// duplicate of the same program stands down; a holder that is some *other*
/// copy is evicted, because the one the shell just spawned is the newest and
/// carries the current triggers.
fn acquire_lock() -> std::fs::File {
    const LOCK_EX_NB: i32 = 2 | 4;
    const SIGTERM: i32 = 15;

    let dir = state_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("egg-watch.lock");
    let file = match std::fs::File::create(&path) {
        Ok(file) => file,
        Err(_) => std::process::exit(0),
    };

    for _ in 0..12 {
        if unsafe { flock(file.as_raw_fd(), LOCK_EX_NB) } == 0 {
            return file;
        }
        let Some(holder) = lock_holder(&path) else {
            std::thread::sleep(Duration::from_millis(250));
            continue;
        };
        if is_same_program(holder) || !looks_like_watcher(holder) {
            std::process::exit(0); // a duplicate, or not ours to evict
        }
        if unsafe { kill(holder, SIGTERM) } != 0 {
            std::process::exit(0); // someone else's process; yield to it
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    std::process::exit(0);
}

fn state_dir() -> PathBuf {
    let state = match std::env::var_os("XDG_STATE_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"),
    };
    state.join("caelestia")
}

/// Who holds the flock, found through /proc/locks by the lock file's inode.
/// Device numbers are deliberately not compared: on btrfs, stat gives the
/// subvolume's anonymous device while /proc/locks shows the superblock's, so
/// they legitimately differ.
fn lock_holder(path: &Path) -> Option<i32> {
    use std::os::unix::fs::MetadataExt;

    let inode = std::fs::metadata(path).ok()?.ino();
    let locks = std::fs::read_to_string("/proc/locks").ok()?;
    for line in locks.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if !fields.contains(&"FLOCK") || fields.len() < 4 {
            continue;
        }
        // "… WRITE <pid> <maj>:<min>:<inode> 0 EOF"
        let location = fields[fields.len() - 3];
        let Some(found) = location.split(':').nth(2).and_then(|i| i.parse::<u64>().ok()) else {
            continue;
        };
        if found == inode {
            return fields[fields.len() - 4].parse().ok();
        }
    }
    None
}

fn is_same_program(pid: i32) -> bool {
    let (Ok(theirs), Ok(ours)) = (
        std::fs::read_link(format!("/proc/{pid}/exe")),
        std::env::current_exe(),
    ) else {
        return false;
    };
    theirs == ours
}

/// Inode matching is filesystem-local, so check the holder really is a
/// watcher of some kind before signalling it.
fn looks_like_watcher(pid: i32) -> bool {
    if let Ok(comm) = std::fs::read_to_string(format!("/proc/{pid}/comm")) {
        let comm = comm.trim();
        if comm.starts_with("python") || comm.starts_with("penis-egg") || comm.starts_with("caelestia-tools")
        {
            return true;
        }
    }
    std::fs::read(format!("/proc/{pid}/cmdline"))
        .map(|raw| String::from_utf8_lossy(&raw).contains("egg-watch"))
        .unwrap_or(false)
}

extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
    fn poll(fds: *mut u8, nfds: u64, timeout: i32) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_letter_rows_count() {
        for code in [16, 25, 30, 38, 44, 50] {
            assert!(is_letter(code), "{code} is a letter key");
        }
        for code in [1, 15, 26, 29, 39, 43, 51, 57] {
            assert!(!is_letter(code), "{code} is not a letter key");
        }
    }

    #[test]
    fn a_trigger_is_matched_at_the_end_of_what_was_typed() {
        let penis = [25u16, 18, 49, 23, 31];
        assert_eq!(matched(&penis), Some("easterEgg"));
        // Typing more before it changes nothing
        assert_eq!(matched(&[30, 30, 25, 18, 49, 23, 31]), Some("easterEgg"));
        // Typing more after it does
        assert_eq!(matched(&[25, 18, 49, 23, 31, 30]), None);
        assert_eq!(matched(&[23, 31, 19, 30, 18, 38]), Some("israelEgg"));
        assert_eq!(matched(&[]), None);
        assert_eq!(matched(&[25, 18]), None);
    }

    #[test]
    fn an_event_is_read_out_of_the_kernel_layout() {
        // type=EV_KEY, code=25 (p), value=1 (down), after a 16-byte timeval
        let mut event = [0u8; EVENT_SIZE];
        event[16..18].copy_from_slice(&EV_KEY.to_ne_bytes());
        event[18..20].copy_from_slice(&25u16.to_ne_bytes());
        event[20..24].copy_from_slice(&KEY_DOWN.to_ne_bytes());

        assert_eq!(u16::from_ne_bytes([event[16], event[17]]), EV_KEY);
        assert_eq!(u16::from_ne_bytes([event[18], event[19]]), 25);
        assert_eq!(
            i32::from_ne_bytes([event[20], event[21], event[22], event[23]]),
            KEY_DOWN
        );
    }

    #[test]
    fn keyboards_are_found_by_their_handler() {
        // Whatever this machine has, every path must be an event device.
        for path in keyboard_paths() {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            assert!(name.starts_with("event"), "{name}");
            assert!(path.starts_with("/dev/input"));
        }
    }
}
