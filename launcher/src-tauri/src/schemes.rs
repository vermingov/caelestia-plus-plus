//! The colour schemes the CLI knows about, with their palettes.
//!
//! `caelestia scheme list` returns every scheme and flavour with all of its
//! colours, which is what lets a row show real swatches rather than a name.
//! The list is fetched once and refreshed when the launcher opens, because
//! generating the dynamic scheme's palette is the slow part and it only
//! changes when the wallpaper does.

use std::process::Command;

use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct Scheme {
    pub name: String,
    pub flavour: String,
    /// The handful of colours a row shows as swatches, `rrggbb`.
    pub swatches: Vec<String>,
    #[serde(skip)]
    pub haystack: String,
}

/// The roles a swatch strip shows, in the order they read best: the accent
/// trio, then the surface it would sit on.
const SWATCH_ROLES: [&str; 5] = ["primary", "secondary", "tertiary", "error", "surface"];

pub fn load() -> Vec<Scheme> {
    let Ok(output) = Command::new("caelestia").args(["scheme", "list"]).output() else {
        return Vec::new();
    };
    let Ok(parsed) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
        return Vec::new();
    };
    let Some(by_name) = parsed.as_object() else { return Vec::new() };

    let mut schemes = Vec::new();
    for (name, flavours) in by_name {
        let Some(flavours) = flavours.as_object() else { continue };
        for (flavour, colours) in flavours {
            let swatches = SWATCH_ROLES
                .iter()
                .filter_map(|role| colours.get(*role).and_then(|c| c.as_str()).map(str::to_string))
                .collect();
            schemes.push(Scheme {
                haystack: format!("{} {}", name.to_lowercase(), flavour.to_lowercase()),
                name: name.clone(),
                flavour: flavour.clone(),
                swatches,
            });
        }
    }
    schemes.sort_by(|a, b| {
        format!("{}{}", a.name, a.flavour).cmp(&format!("{}{}", b.name, b.flavour))
    });
    schemes
}

/// Which scheme is set now, as `name flavour` and the variant, so the list
/// can mark it.
pub fn current() -> (String, String) {
    let Ok(output) = Command::new("caelestia").args(["scheme", "get", "-nfv"]).output() else {
        return (String::new(), String::new());
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = text.trim().lines();
    let name = lines.next().unwrap_or("").trim();
    let flavour = lines.next().unwrap_or("").trim();
    let variant = lines.next().unwrap_or("").trim();
    (format!("{name} {flavour}"), variant.to_string())
}

pub fn apply(name: &str, flavour: &str) {
    spawn(&["scheme", "set", "-n", name, "-f", flavour]);
}

pub fn apply_variant(variant: &str) {
    spawn(&["scheme", "set", "-v", variant]);
}

/// Detached: applying a scheme rewrites a dozen config files and restarts
/// nothing, but it takes long enough that waiting would hold the launcher
/// open past its close animation.
fn spawn(args: &[&str]) {
    let joined: Vec<String> = args.iter().map(|a| shell_quote(a)).collect();
    let _ = Command::new("sh")
        .args(["-c", &format!("setsid -f caelestia {} >/dev/null 2>&1", joined.join(" "))])
        .spawn();
}

fn shell_quote(word: &str) -> String {
    if word.chars().all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c)) {
        return word.to_string();
    }
    format!("'{}'", word.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_schemes_installed_on_this_machine() {
        let schemes = load();
        assert!(schemes.len() > 5, "found only {} schemes", schemes.len());
        assert!(schemes.iter().any(|s| s.name == "catppuccin"));
        // Every scheme should carry swatches, or rows render blank.
        assert!(schemes.iter().all(|s| !s.swatches.is_empty()));
    }

    #[test]
    fn knows_what_is_set_now() {
        let (scheme, variant) = current();
        assert!(!scheme.trim().is_empty(), "no current scheme reported");
        assert!(!variant.is_empty());
    }

    #[test]
    fn arguments_with_spaces_survive_the_shell() {
        assert_eq!(shell_quote("mocha"), "mocha");
        assert_eq!(shell_quote("two words"), "'two words'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }
}
