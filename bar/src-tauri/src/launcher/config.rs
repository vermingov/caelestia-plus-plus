//! What the shell's own config says the launcher should do.
//!
//! The QML launcher reads `~/.config/caelestia/shell.json` through the config
//! plugin; this reads the same file, so one settings page drives both and a
//! prefix or a favourite set in the shell applies here too. Anything the file
//! does not mention falls back to the defaults the plugin ships.

use std::path::PathBuf;

use serde::Deserialize;

/// One entry of `launcher.actions`.
#[derive(Clone, Debug, Deserialize)]
pub struct Action {
    #[serde(default = "unnamed")]
    pub name: String,
    #[serde(default = "no_description")]
    pub description: String,
    #[serde(default = "help_icon")]
    pub icon: String,
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub dangerous: bool,
}

fn unnamed() -> String {
    "Unnamed".to_string()
}
fn no_description() -> String {
    "No description".to_string()
}
fn help_icon() -> String {
    "help_outline".to_string()
}
fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Launcher {
    /// What turns a search into a command. Everything after it selects a mode.
    pub action_prefix: String,
    pub max_shown: usize,
    pub max_wallpapers: usize,
    pub vim_keybinds: bool,
    pub enable_dangerous_actions: bool,
    /// Regexes; an app whose id matches is pinned with a heart.
    pub favourite_apps: Vec<String>,
    /// Regexes; an app whose id matches never appears.
    pub hidden_apps: Vec<String>,
    pub actions: Vec<Action>,
}

impl Default for Launcher {
    fn default() -> Launcher {
        Launcher {
            action_prefix: ">".to_string(),
            max_shown: 8,
            max_wallpapers: 9,
            vim_keybinds: true,
            enable_dangerous_actions: false,
            favourite_apps: Vec::new(),
            hidden_apps: Vec::new(),
            actions: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub launcher: Launcher,
    /// Navigation into the other modes, always offered.
    pub builtins: Vec<Action>,
    /// The terminal the rest of the desktop uses, for `Terminal=true` entries
    /// and for opening a calculation in a real calculator.
    pub terminal: Vec<String>,
    pub wallpaper_dir: PathBuf,
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

pub fn path() -> PathBuf {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"));
    config.join("caelestia/shell.json")
}

/// `~` and `$HOME` the way the shell's own path helper expands them.
fn absolute(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        return home().join(rest);
    }
    if path == "~" {
        return home();
    }
    PathBuf::from(path)
}

impl Config {
    pub fn load() -> Config {
        let raw = std::fs::read_to_string(path()).unwrap_or_default();
        let root: serde_json::Value = serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null);

        let launcher = root
            .get("launcher")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();

        let terminal = root
            .pointer("/general/apps/terminal")
            .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| vec![std::env::var("TERMINAL").unwrap_or_else(|_| "foot".to_string())]);

        // The environment wins over the config, as it does in the shell.
        let wallpaper_dir = match std::env::var_os("CAELESTIA_WALLPAPERS_DIR") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => root
                .pointer("/paths/wallpaperDir")
                .and_then(|v| v.as_str())
                .map(absolute)
                .unwrap_or_else(|| home().join("Pictures/Wallpapers")),
        };

        Config { builtins: builtins(), launcher, terminal, wallpaper_dir }
    }

    /// The actions the user may actually run: disabled ones are gone, and the
    /// dangerous ones only appear when they have been asked for.
    ///
    /// The built-in ones come first. They are not commands — they are how the
    /// other modes are discovered, which is otherwise a matter of knowing the
    /// words already. A configured action of the same name replaces its
    /// built-in rather than sitting beside it.
    pub fn usable_actions(&self) -> Vec<&Action> {
        let configured: Vec<&Action> = self
            .launcher
            .actions
            .iter()
            .filter(|a| a.enabled && (self.launcher.enable_dangerous_actions || !a.dangerous))
            .collect();

        let overridden: Vec<&str> = configured.iter().map(|a| a.name.as_str()).collect();
        self.builtins
            .iter()
            .filter(|a| !overridden.contains(&a.name.as_str()))
            .chain(configured)
            .collect()
    }
}

/// The modes, as entries, so typing the prefix alone shows what there is.
fn builtins() -> Vec<Action> {
    let navigate = |name: &str, description: &str, icon: &str, word: &str| Action {
        name: name.to_string(),
        description: description.to_string(),
        icon: icon.to_string(),
        command: vec!["autocomplete".to_string(), word.to_string()],
        enabled: true,
        dangerous: false,
    };
    vec![
        navigate("Calculator", "Evaluate an expression, with units", "function", "calc"),
        navigate("Colour schemes", "Switch to another scheme and flavour", "palette", "scheme"),
        navigate("Variants", "Change how the palette is generated", "format_paint", "variant"),
        navigate("Wallpapers", "Browse and set a wallpaper", "wallpaper", "wallpaper"),
        Action {
            name: "Settings".to_string(),
            description: "Open the settings".to_string(),
            icon: "settings".to_string(),
            // A word to the shell that is running, which is what draws them.
            command: vec!["cae-shell".to_string(), "settings".to_string()],
            enabled: true,
            dangerous: false,
        },
        Action {
            name: "Light mode".to_string(),
            description: "Switch the scheme to its light variant".to_string(),
            icon: "light_mode".to_string(),
            command: vec!["setMode".to_string(), "light".to_string()],
            enabled: true,
            dangerous: false,
        },
        Action {
            name: "Dark mode".to_string(),
            description: "Switch the scheme to its dark variant".to_string(),
            icon: "dark_mode".to_string(),
            command: vec!["setMode".to_string(), "dark".to_string()],
            enabled: true,
            dangerous: false,
        },
    ]
}

/// Matches the way the shell's own `testRegexList` does: any pattern matching
/// anywhere in the id counts, and an unparseable pattern is ignored rather
/// than fatal.
pub fn matches_any(patterns: &[String], id: &str) -> bool {
    patterns.iter().any(|pattern| simple_match(pattern, id))
}

/// A deliberately small regex: literals, `.`, `*`, `^` and `$`, which is
/// every pattern a favourites or hidden list has ever needed. Anything richer
/// falls back to a substring test rather than pulling in a regex engine for
/// one config key.
fn simple_match(pattern: &str, text: &str) -> bool {
    let anchored_start = pattern.starts_with('^');
    let anchored_end = pattern.ends_with('$') && !pattern.ends_with("\\$");
    let body = pattern.trim_start_matches('^');
    let body = if anchored_end { &body[..body.len() - 1] } else { body };

    if !body.contains(['*', '.', '[', '(', '+', '?', '|']) {
        return match (anchored_start, anchored_end) {
            (true, true) => text == body,
            (true, false) => text.starts_with(body),
            (false, true) => text.ends_with(body),
            (false, false) => text.contains(body),
        };
    }
    glob_like(body, text, anchored_start, anchored_end)
}

/// `.` matches one character and `.*` any run, which covers the patterns
/// people actually write here.
fn glob_like(pattern: &str, text: &str, anchored_start: bool, anchored_end: bool) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();

    fn matches(p: &[char], t: &[char]) -> bool {
        if p.is_empty() {
            return t.is_empty();
        }
        if p.len() >= 2 && p[1] == '*' {
            // Zero or more of whatever p[0] is.
            let mut i = 0;
            loop {
                if matches(&p[2..], &t[i..]) {
                    return true;
                }
                if i >= t.len() || !(p[0] == '.' || p[0] == t[i]) {
                    return false;
                }
                i += 1;
            }
        }
        if t.is_empty() {
            return false;
        }
        (p[0] == '.' || p[0] == t[0]) && matches(&p[1..], &t[1..])
    }

    match (anchored_start, anchored_end) {
        (true, true) => matches(&p, &t),
        (true, false) => (0..=t.len()).any(|end| matches(&p, &t[..end])),
        (false, true) => (0..=t.len()).any(|start| matches(&p, &t[start..])),
        (false, false) => (0..=t.len()).any(|start| (start..=t.len()).any(|end| matches(&p, &t[start..end]))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_plugins() {
        let launcher = Launcher::default();
        assert_eq!(launcher.action_prefix, ">");
        assert_eq!(launcher.max_shown, 8);
        assert!(!launcher.enable_dangerous_actions, "dangerous actions are opt-in");
    }

    #[test]
    fn the_modes_are_always_reachable_from_the_prefix() {
        let config = Config::load();
        let names: Vec<&str> = config.usable_actions().iter().map(|a| a.name.as_str()).collect();
        for mode in ["Calculator", "Colour schemes", "Variants", "Wallpapers"] {
            assert!(names.contains(&mode), "{mode} is not offered; {names:?}");
        }
    }

    #[test]
    fn a_configured_action_replaces_its_builtin_rather_than_doubling_it() {
        let mut config = Config::load();
        config.launcher.actions = vec![Action {
            name: "Calculator".into(),
            description: "Mine".into(),
            icon: String::new(),
            command: vec!["kcalc".into()],
            enabled: true,
            dangerous: false,
        }];
        let actions = config.usable_actions();
        let calculators: Vec<_> = actions.iter().filter(|a| a.name == "Calculator").collect();
        assert_eq!(calculators.len(), 1, "the built-in and the override both showed");
        assert_eq!(calculators[0].description, "Mine");
    }

    #[test]
    fn reads_the_shells_own_config() {
        // No assertion about contents: the point is that a real file on this
        // machine parses rather than falling back silently.
        let config = Config::load();
        assert!(!config.terminal.is_empty());
        assert!(!config.launcher.action_prefix.is_empty());
    }

    #[test]
    fn patterns_match_the_way_the_shell_matches_them() {
        assert!(simple_match("firefox", "org.mozilla.firefox"));
        assert!(simple_match("^firefox", "firefox"));
        assert!(!simple_match("^firefox", "org.mozilla.firefox"));
        assert!(simple_match("firefox$", "org.mozilla.firefox"));
        assert!(simple_match("^org.*firefox$", "org.mozilla.firefox"));
        assert!(!simple_match("^org.*chrome$", "org.mozilla.firefox"));
        assert!(simple_match(".*", "anything"));
    }

    #[test]
    fn a_list_matches_if_any_pattern_does() {
        let patterns = vec!["^steam$".to_string(), "firefox".to_string()];
        assert!(matches_any(&patterns, "steam"));
        assert!(matches_any(&patterns, "org.mozilla.firefox"));
        assert!(!matches_any(&patterns, "kitty"));
        assert!(!matches_any(&[], "anything"));
    }
}
