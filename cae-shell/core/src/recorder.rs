//! Recording the screen, which `caelestia record` does and this asks for.
//!
//! Nothing here holds the recording: the CLI starts gpu-screen-recorder
//! detached and moves the file when it stops, the same way whichever keybind
//! or panel asked. What is kept is only how to ask, and how to see what is
//! going on from outside.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// What is recording, as far as anyone outside it can tell.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recording {
    pub running: bool,
    /// How long it has been going, from when the file it is writing was made.
    /// Nothing is counted here, so it is right after a restart of the shell
    /// as well.
    pub elapsed: Duration,
}

/// One that has been made, newest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Made {
    pub path: PathBuf,
    /// As the name says it: "recording_20260715_16-34-52" is this one's.
    pub name: String,
    pub when: Option<SystemTime>,
}

const RECORDER: &str = "gpu-screen-recorder";

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

/// Where the CLI writes while it records, before it moves the file.
fn in_progress() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME").map_or_else(|| home().join(".local/state"), PathBuf::from);
    state.join("caelestia/record/recording.mp4")
}

/// Where finished recordings go, by the same rules the CLI uses.
pub fn folder() -> PathBuf {
    let said = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
    said("CAELESTIA_RECORDINGS_DIR")
        .or_else(|| said("XDG_VIDEOS_DIR").map(|videos| videos.join("Recordings")))
        .unwrap_or_else(|| home().join("Videos/Recordings"))
}

pub fn now() -> Recording {
    let running = std::process::Command::new("pidof")
        .arg(RECORDER)
        .stdout(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    let elapsed = std::fs::metadata(in_progress())
        .ok()
        .and_then(|file| file.created().ok())
        .and_then(|made| made.elapsed().ok())
        .filter(|_| running)
        .unwrap_or_default();
    Recording { running, elapsed }
}

/// Starts one. `how` is the CLI's own words for it: `-r` for a region, `-s`
/// for sound, `-sr` for both.
pub fn start(how: &[&str]) {
    ask(how);
}

/// Stops the one that is running: the CLI's `record` with nothing else said
/// is a stop while one is going.
pub fn stop() {
    ask(&[]);
}

pub fn toggle_pause() {
    ask(&["-p"]);
}

/// In a scope of its own, as an app is: a recording must not end because
/// the shell restarted in the middle of it.
fn ask(how: &[&str]) {
    let argv: Vec<&str> = ["caelestia", "record"].into_iter().chain(how.iter().copied()).collect();
    crate::children::launch("recorder", &argv);
}

/// Plays one, with whatever the settings say plays video, or with whatever
/// the desktop has for it when they say nothing.
pub fn play(recording: &Path) {
    let shell = crate::config::read(crate::config::File::Shell);
    let said = crate::config::lookup(&shell, "general.apps.playback").and_then(serde_json::Value::as_array);
    let mut words: Vec<String> =
        said.map(|words| words.iter().filter_map(|word| word.as_str().map(str::to_string)).collect()).unwrap_or_default();
    if words.is_empty() {
        words.push("xdg-open".to_string());
    }
    let recording = recording.to_string_lossy();
    let argv: Vec<&str> = words.iter().map(String::as_str).chain([recording.as_ref()]).collect();
    crate::children::launch(argv[0], &argv);
}

/// Every recording there is, newest first.
pub fn made() -> Vec<Made> {
    let Ok(files) = std::fs::read_dir(folder()) else { return Vec::new() };
    let mut made: Vec<Made> = files
        .filter_map(Result::ok)
        .filter_map(|file| {
            let path = file.path();
            let name = path.file_stem()?.to_str()?.to_string();
            if !name.starts_with("recording_") || path.extension()? != "mp4" {
                return None;
            }
            Some(Made { when: file.metadata().ok().and_then(|file| file.modified().ok()), name, path })
        })
        .collect();
    // By name, which is the time it was made, and is right whatever has
    // since touched the files.
    made.sort_by(|one, other| other.name.cmp(&one.name));
    made
}

/// Throws one away. Only ever a recording, whatever it is handed.
pub fn delete(recording: &Path) {
    if recording.starts_with(folder()) && recording.extension().is_some_and(|kind| kind == "mp4") {
        let _ = std::fs::remove_file(recording);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_outside_the_recordings_folder_is_deleted() {
        let kept = std::env::temp_dir().join("cae-not-a-recording.mp4");
        std::fs::write(&kept, b"").expect("a file to try it on");
        delete(&kept);
        assert!(kept.exists(), "a path outside the folder must be left alone");
        std::fs::remove_file(&kept).ok();
    }
}
