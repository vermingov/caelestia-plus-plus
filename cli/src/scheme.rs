//! Reading the colour schemes: which one is in use, which exist, and what
//! colours each one holds.
//!
//! All of it is files the Python CLI wrote, read back the same way it reads
//! them. Nothing here generates a palette — that is the Material pipeline,
//! which still belongs to the other CLI. When a generated palette is not in
//! the cache yet, everything here reports that it cannot answer and the
//! caller hands the whole command over rather than guessing.

use std::collections::BTreeMap;
use std::path::PathBuf;

use redcommon::json::{self, Json};

use crate::{paths, sha256};

/// The nine Material variants, in the order the CLI lists them.
pub const VARIANTS: [&str; 9] = [
    "tonalspot",
    "vibrant",
    "expressive",
    "fidelity",
    "fruitsalad",
    "monochrome",
    "neutral",
    "rainbow",
    "content",
];

/// The scheme generated from the wallpaper rather than shipped as a file.
pub const DYNAMIC: &str = "dynamic";

#[derive(Debug, Clone)]
pub struct Current {
    pub name: String,
    pub flavour: String,
    pub mode: String,
    pub variant: String,
}

pub fn current() -> Option<Current> {
    let text = std::fs::read_to_string(paths::scheme_state_path()).ok()?;
    let parsed = json::parse(&text)?;
    Some(Current {
        name: parsed.str_field("name")?.to_string(),
        flavour: parsed.str_field("flavour")?.to_string(),
        mode: parsed.str_field("mode")?.to_string(),
        variant: parsed.str_field("variant")?.to_string(),
    })
}

/// Every scheme name, sorted, with the generated one last — the same shape
/// the Python CLI prints, which lists the data directory and appends it.
pub fn names() -> Vec<String> {
    let mut names = match paths::scheme_data_dir() {
        Some(dir) => subdirectories(&dir),
        None => Vec::new(),
    };
    names.push(DYNAMIC.to_string());
    names
}

pub fn flavours(name: &str) -> Vec<String> {
    if name == DYNAMIC {
        return vec!["default".to_string(), "hard".to_string()];
    }
    match paths::scheme_data_dir() {
        Some(dir) => subdirectories(&dir.join(name)),
        None => Vec::new(),
    }
}

pub fn modes(name: &str, flavour: &str) -> Vec<String> {
    if name == DYNAMIC {
        return vec!["dark".to_string(), "light".to_string()];
    }
    let Some(dir) = paths::scheme_data_dir() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(dir.join(name).join(flavour)) else {
        return Vec::new();
    };
    let mut modes: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            e.path()
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .collect();
    modes.sort();
    modes
}

/// A shipped scheme's colours: one `name value` pair per line.
pub fn file_colours(name: &str, flavour: &str, mode: &str) -> Option<Json> {
    let path = paths::scheme_data_dir()?
        .join(name)
        .join(flavour)
        .join(format!("{mode}.txt"));
    let text = std::fs::read_to_string(path).ok()?;

    let mut colours = BTreeMap::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let Some((key, value)) = line.split_once(' ') else { continue };
        colours.insert(key.trim().to_string(), json::s(value.trim()));
    }
    (!colours.is_empty()).then_some(Json::Obj(colours))
}

/// A generated palette, if it is already in the cache. The cache is keyed by
/// the hash of the wallpaper thumbnail, so it is warm for as long as the
/// wallpaper has not changed — which is nearly always.
pub fn cached_dynamic_colours(variant: &str, flavour: &str, mode: &str) -> Option<Json> {
    let hash = sha256::file_hex(&paths::wallpaper_thumbnail_path())?;
    let path: PathBuf = paths::scheme_cache_dir()
        .join(hash)
        .join(variant)
        .join(flavour)
        .join(format!("{mode}.json"));
    let text = std::fs::read_to_string(path).ok()?;
    match json::parse(&text) {
        Some(Json::Obj(map)) if !map.is_empty() => Some(Json::Obj(map)),
        _ => None,
    }
}

/// Every scheme and flavour with its colours, as the launcher's picker wants
/// it. None when any part is missing — a cold dynamic cache, no data
/// directory — because a partial answer here is a picker with holes in it.
pub fn all_colours() -> Option<Json> {
    let current = current()?;

    // The generated scheme decides whether this is answerable at all, so it
    // is checked first: handing the command over after reading thirty files
    // would cost more than never having tried.
    let mut dynamic = BTreeMap::new();
    for flavour in flavours(DYNAMIC) {
        let modes = modes(DYNAMIC, &flavour);
        let mode = if modes.iter().any(|m| *m == current.mode) {
            current.mode.clone()
        } else {
            modes.first()?.clone()
        };
        dynamic.insert(flavour.clone(), cached_dynamic_colours(&current.variant, &flavour, &mode)?);
    }

    let mut schemes = BTreeMap::new();

    for name in names() {
        let mut per_flavour = BTreeMap::new();
        for flavour in flavours(&name) {
            let modes = modes(&name, &flavour);
            if modes.is_empty() {
                continue;
            }
            // The current mode where the scheme has one, its first otherwise.
            let mode = if modes.iter().any(|m| *m == current.mode) {
                current.mode.clone()
            } else {
                modes[0].clone()
            };

            let colours = match dynamic.get(&flavour) {
                Some(generated) if name == DYNAMIC => generated.clone(),
                _ => file_colours(&name, &flavour, &mode)?,
            };
            per_flavour.insert(flavour, colours);
        }
        if !per_flavour.is_empty() {
            schemes.insert(name, Json::Obj(per_flavour));
        }
    }

    (!schemes.is_empty()).then_some(Json::Obj(schemes))
}

fn subdirectories(dir: &std::path::Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_generated_scheme_is_always_offered() {
        let names = names();
        assert_eq!(names.last().map(String::as_str), Some(DYNAMIC));
        assert_eq!(flavours(DYNAMIC), ["default", "hard"]);
        assert_eq!(modes(DYNAMIC, "default"), ["dark", "light"]);
    }

    #[test]
    fn reads_the_schemes_installed_on_this_machine() {
        let Some(dir) = paths::scheme_data_dir() else { return };
        let names = names();
        assert!(names.len() > 1, "found only {names:?} in {}", dir.display());

        // Every shipped scheme must have at least one flavour and one mode,
        // and that mode's file must parse into colours.
        for name in names.iter().filter(|n| *n != DYNAMIC) {
            let flavours = flavours(name);
            assert!(!flavours.is_empty(), "{name} has no flavours");
            for flavour in &flavours {
                let modes = modes(name, flavour);
                assert!(!modes.is_empty(), "{name}/{flavour} has no modes");
                let colours = file_colours(name, flavour, &modes[0])
                    .unwrap_or_else(|| panic!("{name}/{flavour}/{} unreadable", modes[0]));
                let Json::Obj(map) = &colours else { panic!("not an object") };
                assert!(map.len() > 50, "{name}/{flavour} has only {} colours", map.len());
                assert!(map.contains_key("background"));
            }
        }
    }

    #[test]
    fn the_state_file_says_what_is_in_use() {
        let Some(current) = current() else { return };
        assert!(!current.name.is_empty());
        assert!(["dark", "light"].contains(&current.mode.as_str()));
        assert!(VARIANTS.contains(&current.variant.as_str()), "{}", current.variant);
    }
}
