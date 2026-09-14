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

    #[test]
    fn the_ordered_reader_keeps_the_file_order() {
        let text = r#"{"zebra": "ffffff", "apple": "000000", "mid": "112233"}"#;
        let pairs = ordered_object(text);
        assert_eq!(
            pairs,
            vec![
                ("zebra".to_string(), "ffffff".to_string()),
                ("apple".to_string(), "000000".to_string()),
                ("mid".to_string(), "112233".to_string()),
            ],
            "a sorting parser would have put apple first"
        );
    }

    #[test]
    fn the_ordered_reader_survives_escapes_and_whitespace() {
        let text = "{\n  \"a\\\"b\": \"one\\\\two\",\n  \"c\": \"three\"\n}";
        assert_eq!(
            ordered_object(text),
            vec![
                ("a\"b".to_string(), "one\\two".to_string()),
                ("c".to_string(), "three".to_string()),
            ]
        );
    }

    #[test]
    fn dumping_round_trips_through_the_reader() {
        let pairs: Colours = vec![
            ("primary".to_string(), "aabbcc".to_string()),
            ("odd\"name".to_string(), "ddeeff".to_string()),
        ];
        assert_eq!(ordered_object(&dump_object(&pairs)), pairs);
    }

    /// Regenerates every palette the Python already cached for the wallpaper
    /// on this machine, and checks the two agree name for name, in order.
    #[test]
    fn generated_palettes_match_the_ones_python_cached() {
        let Some(hash) = sha256::file_hex(&paths::wallpaper_thumbnail_path()) else { return };
        let base = paths::scheme_cache_dir().join(hash);
        let Ok(variants) = std::fs::read_dir(&base) else { return };

        let mut checked = 0usize;
        for variant in variants.flatten().filter(|e| e.path().is_dir()) {
            let Ok(flavours) = std::fs::read_dir(variant.path()) else { continue };
            for flavour in flavours.flatten().filter(|e| e.path().is_dir()) {
                let Ok(modes) = std::fs::read_dir(flavour.path()) else { continue };
                for mode in modes.flatten().filter(|e| e.path().is_file()) {
                    let path = mode.path();
                    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
                    let (variant, flavour) = (
                        variant.file_name().to_string_lossy().into_owned(),
                        flavour.file_name().to_string_lossy().into_owned(),
                    );
                    let Ok(text) = std::fs::read_to_string(&path) else { continue };
                    let theirs = ordered_object(&text);
                    if theirs.is_empty() {
                        continue;
                    }

                    let seed = seed_for(&paths::wallpaper_thumbnail_path(), &base)
                        .unwrap_or_else(|e| panic!("{e}"));
                    let ours = crate::material::generator::gen_scheme(&variant, &flavour, stem, seed);
                    assert_eq!(
                        ours.len(),
                        theirs.len(),
                        "{variant}/{flavour}/{stem}: colour count"
                    );
                    for (ours, theirs) in ours.iter().zip(&theirs) {
                        assert_eq!(ours, theirs, "{variant}/{flavour}/{stem}");
                    }
                    checked += 1;
                }
            }
        }
        assert!(checked > 0, "no cached palettes to check against");
    }
}

/// A palette in the order it was written, because the generated Hyprland and
/// SCSS files list every colour and a reordered file is a spurious diff.
pub type Colours = Vec<(String, String)>;

/// Reads a flat `{"name": "value"}` object keeping the file's own key order.
/// The shared JSON parser sorts its keys, which is right for the daemons and
/// wrong here: these colours are written out as a list, and a reordered list
/// is a diff in every themed config file.
fn ordered_object(text: &str) -> Colours {
    let bytes = text.as_bytes();
    let mut i = text.find('{').map_or(bytes.len(), |at| at + 1);
    let mut out = Vec::new();
    loop {
        let Some(key) = next_string(text, &mut i) else { break };
        let Some(value) = next_string(text, &mut i) else { break };
        out.push((key, value));
    }
    out
}

/// The next quoted string at or after `i`, or None if the object ends first.
fn next_string(text: &str, i: &mut usize) -> Option<String> {
    let bytes = text.as_bytes();
    while *i < bytes.len() && bytes[*i] != b'"' {
        if bytes[*i] == b'}' {
            return None;
        }
        *i += 1;
    }
    if *i >= bytes.len() {
        return None;
    }
    *i += 1;

    let mut value = String::new();
    let mut plain = *i;
    while *i < bytes.len() && bytes[*i] != b'"' {
        if bytes[*i] != b'\\' {
            *i += 1;
            continue;
        }
        value.push_str(text.get(plain..*i)?);
        *i += 1;
        // Only the escapes a colour file can hold; anything else is the
        // character itself, which is what JSON says for \" and \\.
        value.push(match bytes.get(*i) {
            Some(b'n') => '\n',
            Some(b't') => '\t',
            Some(other) => *other as char,
            None => return None,
        });
        *i += 1;
        plain = *i;
    }
    value.push_str(text.get(plain..*i)?);
    *i += 1;
    Some(value)
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The same spacing Python's `json.dumps` produces, so the state file does
/// not churn when the two CLIs take turns writing it.
fn dump_object(pairs: &Colours) -> String {
    let body: Vec<String> =
        pairs.iter().map(|(k, v)| format!("\"{}\": \"{}\"", escape(k), escape(v))).collect();
    format!("{{{}}}", body.join(", "))
}

/// A shipped scheme's colours, in file order.
pub fn file_colours_ordered(name: &str, flavour: &str, mode: &str) -> Option<Colours> {
    let path = paths::scheme_data_dir()?.join(name).join(flavour).join(format!("{mode}.txt"));
    let text = std::fs::read_to_string(path).ok()?;
    let colours: Colours = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| line.split_once(' '))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    (!colours.is_empty()).then_some(colours)
}

/// Which scheme is in use, colours and all, in the order it was stored.
pub struct Scheme {
    pub name: String,
    pub flavour: String,
    pub mode: String,
    pub variant: String,
    pub colours: Colours,
}

impl Scheme {
    /// The stored scheme, or the default one if nothing has been set yet.
    pub fn load() -> Result<Scheme, String> {
        if let Some(scheme) = Scheme::stored() {
            return Ok(scheme);
        }
        let mut scheme = Scheme {
            name: "catppuccin".to_string(),
            flavour: "mocha".to_string(),
            mode: "dark".to_string(),
            variant: "tonalspot".to_string(),
            colours: Vec::new(),
        };
        scheme.colours = file_colours_ordered(&scheme.name, &scheme.flavour, &scheme.mode)
            .ok_or_else(|| "the default scheme is not installed".to_string())?;
        scheme.save();
        Ok(scheme)
    }

    fn stored() -> Option<Scheme> {
        let text = std::fs::read_to_string(paths::scheme_state_path()).ok()?;
        let parsed = json::parse(&text)?;
        let colours_text = text.find("\"colours\"").map(|at| &text[at..]).unwrap_or("");
        Some(Scheme {
            name: parsed.str_field("name")?.to_string(),
            flavour: parsed.str_field("flavour")?.to_string(),
            mode: parsed.str_field("mode")?.to_string(),
            variant: parsed.str_field("variant")?.to_string(),
            colours: ordered_object(colours_text),
        })
    }

    pub fn save(&self) {
        let body = format!(
            "{{\"name\": \"{}\", \"flavour\": \"{}\", \"mode\": \"{}\", \"variant\": \"{}\", \"colours\": {}}}",
            escape(&self.name),
            escape(&self.flavour),
            escape(&self.mode),
            escape(&self.variant),
            dump_object(&self.colours)
        );
        let path = paths::scheme_state_path();
        if let Err(e) = paths::atomic_write(&path, &body) {
            eprintln!("caelestia: cannot write {}: {e}", path.display());
        }
    }

    /// Changing the name can invalidate the flavour and the mode, so both are
    /// pulled back to something the new scheme actually has.
    pub fn set_name(&mut self, name: &str) -> Result<(), String> {
        if name == self.name {
            return Ok(());
        }
        if !names().iter().any(|n| n == name) {
            return Err(format!("\"{name}\" is not a valid scheme.\nValid schemes are: {:?}", names()));
        }
        self.name = name.to_string();

        let flavours = flavours(&self.name);
        if !flavours.iter().any(|f| *f == self.flavour) {
            self.flavour = flavours.first().cloned().unwrap_or_default();
        }
        let modes = modes(&self.name, &self.flavour);
        if !modes.iter().any(|m| *m == self.mode) {
            self.mode = modes.first().cloned().unwrap_or_default();
        }
        self.update_colours()
    }

    pub fn set_flavour(&mut self, flavour: &str) -> Result<(), String> {
        if flavour == self.flavour {
            return Ok(());
        }
        let valid = flavours(&self.name);
        if !valid.iter().any(|f| f == flavour) {
            return Err(format!(
                "\"{flavour}\" is not a valid flavour of scheme \"{}\".\nValid flavours are: {valid:?}",
                self.name
            ));
        }
        self.flavour = flavour.to_string();

        let modes = modes(&self.name, &self.flavour);
        if !modes.iter().any(|m| *m == self.mode) {
            self.mode = modes.first().cloned().unwrap_or_default();
        }
        self.update_colours()
    }

    pub fn set_mode(&mut self, mode: &str) -> Result<(), String> {
        if mode == self.mode {
            return Ok(());
        }
        let valid = modes(&self.name, &self.flavour);
        if !valid.iter().any(|m| m == mode) {
            return Err(format!(
                "scheme \"{} {}\" does not have a {mode} mode.\nValid modes: {valid:?}",
                self.name, self.flavour
            ));
        }
        self.mode = mode.to_string();
        self.update_colours()
    }

    pub fn set_variant(&mut self, variant: &str) -> Result<(), String> {
        if variant == self.variant {
            return Ok(());
        }
        self.variant = variant.to_string();
        self.update_colours()
    }

    /// A scheme picked at random from everything installed.
    pub fn set_random(&mut self) -> Result<(), String> {
        let names = names();
        self.name = pick(&names).ok_or("no schemes installed")?;
        let flavours = flavours(&self.name);
        self.flavour = pick(&flavours).ok_or("scheme has no flavours")?;
        let modes = modes(&self.name, &self.flavour);
        self.mode = pick(&modes).ok_or("scheme has no modes")?;
        self.update_colours()
    }

    pub fn update_colours(&mut self) -> Result<(), String> {
        self.colours = if self.name == DYNAMIC {
            dynamic_colours(&self.variant, &self.flavour, &self.mode)?
        } else {
            file_colours_ordered(&self.name, &self.flavour, &self.mode)
                .ok_or_else(|| format!("no colours for {}/{}/{}", self.name, self.flavour, self.mode))?
        };
        self.save();
        Ok(())
    }
}

/// Enough randomness to pick a scheme; the clock is the only entropy this
/// needs and the only one available without a dependency.
fn pick(options: &[String]) -> Option<String> {
    if options.is_empty() {
        return None;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize)
        .unwrap_or(0);
    Some(options[nanos % options.len()].clone())
}

/// The generated palette for the current wallpaper, from the cache if it is
/// there and generated if it is not.
pub fn dynamic_colours(variant: &str, flavour: &str, mode: &str) -> Result<Colours, String> {
    let thumbnail = paths::wallpaper_thumbnail_path();
    let hash = sha256::file_hex(&thumbnail).ok_or(
        "no wallpaper set. Set one with `caelestia wallpaper` before setting a dynamic scheme.",
    )?;

    let base = paths::scheme_cache_dir().join(&hash);
    let cached = base.join(variant).join(flavour).join(format!("{mode}.json"));
    if let Ok(text) = std::fs::read_to_string(&cached) {
        let colours = ordered_object(&text);
        if !colours.is_empty() {
            return Ok(colours);
        }
    }

    let seed = seed_for(&thumbnail, &base)?;
    let colours = crate::material::generator::gen_scheme(variant, flavour, mode, seed);
    if let Err(e) = paths::atomic_write(&cached, &dump_object(&colours)) {
        eprintln!("caelestia: cannot cache {}: {e}", cached.display());
    }
    Ok(colours)
}

/// The wallpaper's dominant colour. Quantising an image is the expensive half
/// of generating a scheme, so the answer is kept beside the palettes — the
/// Python writes this file too, but reads it back in a way that always fails,
/// so it re-quantises on every change.
fn seed_for(thumbnail: &std::path::Path, base: &std::path::Path) -> Result<crate::material::hct::Hct, String> {
    let cache = base.join("score.json");
    if let Some(argb) = std::fs::read_to_string(&cache).ok().and_then(|t| t.trim().parse::<u32>().ok()) {
        return Ok(crate::material::hct::Hct::from_int(argb));
    }
    let bytes = std::fs::read(thumbnail).map_err(|e| format!("cannot read the wallpaper thumbnail: {e}"))?;
    let seed = crate::material::score::score_image(&bytes)?;
    if let Err(e) = paths::atomic_write(&cache, &seed.to_int().to_string()) {
        eprintln!("caelestia: cannot cache {}: {e}", cache.display());
    }
    Ok(seed)
}
