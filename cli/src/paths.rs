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

/// The installed Python package's directory. Its `data/` subtree holds the
/// emoji list and every built-in colour scheme, and those files belong to
/// whichever copy of the CLI is installed — so they are found by looking,
/// not by asking Python, which would cost more than the rest of this program.
pub fn python_package_dir() -> Option<PathBuf> {
    for root in ["/usr/lib", "/usr/local/lib"] {
        let Ok(entries) = std::fs::read_dir(root) else { continue };
        for entry in entries.flatten() {
            let candidate = entry.path().join("site-packages/caelestia");
            if candidate.is_dir() {
                return Some(candidate);
            }
        }
    }
    None
}

/// The emoji and glyph list the picker reads.
pub fn emoji_data_path() -> Option<PathBuf> {
    let local = caelestia_state_dir().join("emojis.txt");
    if local.is_file() {
        return Some(local); // a user-refreshed copy wins
    }
    let packaged = python_package_dir()?.join("data/emojis.txt");
    packaged.is_file().then_some(packaged)
}

/// The built-in colour schemes: <package>/data/schemes/<name>/<flavour>/<mode>.txt
pub fn scheme_data_dir() -> Option<PathBuf> {
    let dir = python_package_dir()?.join("data/schemes");
    dir.is_dir().then_some(dir)
}

/// Which scheme is in use, colours and all.
pub fn scheme_state_path() -> PathBuf {
    caelestia_state_dir().join("scheme.json")
}

/// Generated palettes, keyed by the hash of the wallpaper thumbnail.
pub fn scheme_cache_dir() -> PathBuf {
    caelestia_cache_dir().join("schemes")
}

pub fn wallpaper_thumbnail_path() -> PathBuf {
    caelestia_state_dir().join("wallpaper/thumbnail.jpg")
}

pub fn data_dir() -> PathBuf {
    env_dir("XDG_DATA_HOME", ".local/share")
}

/// The templates the theme layer fills in: one per application it themes.
/// They belong to the installed CLI package, so a template fix reaches us the
/// moment the package updates.
pub fn templates_dir() -> Option<PathBuf> {
    let dir = python_package_dir()?.join("data/templates");
    dir.is_dir().then_some(dir)
}

/// Templates the user dropped in themselves, rendered alongside ours.
pub fn user_templates_dir() -> PathBuf {
    caelestia_config_dir().join("templates")
}

/// Where rendered user templates land.
pub fn theme_dir() -> PathBuf {
    caelestia_state_dir().join("theme")
}

/// Write-then-rename, so a reader never sees half a theme file.
pub fn atomic_write(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    use std::io::Write;

    let parent = path.parent().unwrap_or(std::path::Path::new("."));
    std::fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("theme"),
        std::process::id()
    ));
    let result = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}
