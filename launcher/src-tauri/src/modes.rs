//! Which list the launcher is showing.
//!
//! A query starting with the action prefix selects a mode; anything else is
//! the application list. The prefixed modes mirror the shell's own — the same
//! words, so muscle memory carries over, and the same behaviour on Enter.

use crate::config::Config;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Apps,
    Actions,
    Calc,
    Scheme,
    Variant,
    Wallpaper,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Apps => "apps",
            Mode::Actions => "actions",
            Mode::Calc => "calc",
            Mode::Scheme => "scheme",
            Mode::Variant => "variant",
            Mode::Wallpaper => "wallpapers",
        }
    }

    /// The heading over the list, and what Enter does — the two things the
    /// chrome tells the user about the mode they are in.
    pub fn label(self) -> &'static str {
        match self {
            Mode::Apps => "Applications",
            Mode::Actions => "Commands",
            Mode::Calc => "Calculator",
            Mode::Scheme => "Colour schemes",
            Mode::Variant => "Variants",
            Mode::Wallpaper => "Wallpapers",
        }
    }

    pub fn action(self) -> &'static str {
        match self {
            Mode::Apps => "Open",
            Mode::Actions => "Run",
            Mode::Calc => "Copy",
            Mode::Scheme | Mode::Variant => "Apply",
            Mode::Wallpaper => "Set",
        }
    }
}

/// The mode a query selects, and what is left of the query once the prefix
/// and mode word are taken off.
pub fn parse<'a>(query: &'a str, config: &Config) -> (Mode, &'a str) {
    let prefix = &config.launcher.action_prefix;
    let Some(rest) = query.strip_prefix(prefix.as_str()) else {
        return (Mode::Apps, query);
    };

    // A mode word only counts once it is followed by a space: `>cal` is still
    // the command list being narrowed, `>calc ` is the calculator.
    for (word, mode) in [
        ("calc ", Mode::Calc),
        ("scheme ", Mode::Scheme),
        ("variant ", Mode::Variant),
        ("wallpaper ", Mode::Wallpaper),
    ] {
        if let Some(argument) = rest.strip_prefix(word) {
            return (mode, argument);
        }
    }
    (Mode::Actions, rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        let mut config = Config::load();
        config.launcher.action_prefix = ">".to_string();
        config
    }

    #[test]
    fn a_bare_query_is_the_application_list() {
        let config = config();
        assert_eq!(parse("firefox", &config), (Mode::Apps, "firefox"));
        assert_eq!(parse("", &config), (Mode::Apps, ""));
    }

    #[test]
    fn the_prefix_alone_is_the_command_list() {
        let config = config();
        assert_eq!(parse(">", &config), (Mode::Actions, ""));
        assert_eq!(parse(">loc", &config), (Mode::Actions, "loc"));
    }

    #[test]
    fn a_mode_word_needs_its_space() {
        let config = config();
        assert_eq!(parse(">calc", &config), (Mode::Actions, "calc"));
        assert_eq!(parse(">calc ", &config), (Mode::Calc, ""));
        assert_eq!(parse(">calc 2+2", &config), (Mode::Calc, "2+2"));
        assert_eq!(parse(">scheme mocha", &config), (Mode::Scheme, "mocha"));
        assert_eq!(parse(">variant vib", &config), (Mode::Variant, "vib"));
        assert_eq!(parse(">wallpaper sky", &config), (Mode::Wallpaper, "sky"));
    }

    #[test]
    fn the_prefix_is_whatever_the_config_says() {
        let mut config = config();
        config.launcher.action_prefix = "/".to_string();
        assert_eq!(parse("/calc 1+1", &config), (Mode::Calc, "1+1"));
        assert_eq!(parse(">calc 1+1", &config), (Mode::Apps, ">calc 1+1"));
    }
}
