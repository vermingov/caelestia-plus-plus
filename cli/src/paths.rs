//! Where Caelestia keeps things, mirroring `caelestia.utils.paths`.
//!
//! The Python CLI owns these paths and writes most of them; this front-end
//! only has to agree, or the two would disagree about where a screenshot or
//! the scheme lives.

use std::path::PathBuf;

fn env_dir(var: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(var) {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => home().join(fallback),
    }
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

pub fn config_dir() -> PathBuf {
    env_dir("XDG_CONFIG_HOME", ".config")
}

pub fn state_dir() -> PathBuf {
    env_dir("XDG_STATE_HOME", ".local/state")
}

pub fn cache_dir() -> PathBuf {
    env_dir("XDG_CACHE_HOME", ".cache")
}

pub fn pictures_dir() -> PathBuf {
    env_dir("XDG_PICTURES_DIR", "Pictures")
}

pub fn caelestia_config_dir() -> PathBuf {
    config_dir().join("caelestia")
}

pub fn caelestia_state_dir() -> PathBuf {
    state_dir().join("caelestia")
}

pub fn caelestia_cache_dir() -> PathBuf {
    cache_dir().join("caelestia")
}

/// The user's CLI config: toggles, recorder arguments.
pub fn user_config_path() -> PathBuf {
    caelestia_config_dir().join("cli.json")
}

pub fn screenshots_dir() -> PathBuf {
    match std::env::var_os("CAELESTIA_SCREENSHOTS_DIR") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => pictures_dir().join("Screenshots"),
    }
}

pub fn screenshots_cache_dir() -> PathBuf {
    caelestia_cache_dir().join("screenshots")
}

/// The emoji and glyph list the picker reads. It ships inside the Python
/// package, so the file lives wherever that package was installed — found by
/// looking rather than by asking Python, which would cost more than the whole
/// rest of this program.
pub fn emoji_data_path() -> Option<PathBuf> {
    let local = caelestia_state_dir().join("emojis.txt");
    if local.is_file() {
        return Some(local); // a user-refreshed copy wins
    }
    for root in ["/usr/lib", "/usr/local/lib"] {
        let Ok(entries) = std::fs::read_dir(root) else { continue };
        for entry in entries.flatten() {
            let candidate = entry
                .path()
                .join("site-packages/caelestia/data/emojis.txt");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}
