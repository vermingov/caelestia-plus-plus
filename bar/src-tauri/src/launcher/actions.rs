//! The commands the user has put in their config, and what running one means.
//!
//! Two of them do not start a program: `autocomplete` rewrites the search box
//! so a command can lead into a mode, and `setMode` flips the scheme between
//! light and dark. The rest are argv, run detached.

use super::config::{Action, Config};

/// What activating an action asks the front end to do.
pub enum Outcome {
    /// Replace the query with this and keep the launcher open.
    Autocomplete(String),
    /// Done; close.
    Ran,
    /// The action has no command at all.
    Nothing,
}

pub fn run(action: &Action, config: &Config) -> Outcome {
    let Some(verb) = action.command.first() else { return Outcome::Nothing };
    let rest = &action.command[1..];

    match verb.as_str() {
        "autocomplete" if !rest.is_empty() => {
            Outcome::Autocomplete(format!("{}{} ", config.launcher.action_prefix, rest[0]))
        }
        "setMode" if !rest.is_empty() => {
            spawn(&["caelestia", "scheme", "set", "-m", &rest[0]]);
            Outcome::Ran
        }
        _ => {
            let argv: Vec<&str> = action.command.iter().map(String::as_str).collect();
            spawn(&argv);
            Outcome::Ran
        }
    }
}

/// Out of this process's tree and out of its service, so nothing it starts
/// dies with the launcher. As argv, with no shell in between to re-read it.
fn spawn(argv: &[&str]) {
    crate::children::launch(argv[0], argv);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(command: &[&str]) -> Action {
        Action {
            name: "Test".into(),
            description: String::new(),
            icon: String::new(),
            command: command.iter().map(|s| s.to_string()).collect(),
            enabled: true,
            dangerous: false,
        }
    }

    #[test]
    fn autocomplete_rewrites_the_query_rather_than_running_anything() {
        let config = Config::load();
        match run(&action(&["autocomplete", "scheme"]), &config) {
            Outcome::Autocomplete(text) => {
                assert_eq!(text, format!("{}scheme ", config.launcher.action_prefix));
            }
            _ => panic!("should have autocompleted"),
        }
    }

    #[test]
    fn an_action_with_no_command_does_nothing() {
        let config = Config::load();
        assert!(matches!(run(&action(&[]), &config), Outcome::Nothing));
    }

    #[test]
    fn dangerous_actions_are_hidden_unless_asked_for() {
        let mut config = Config::load();
        let mut risky = action(&["poweroff"]);
        risky.name = "Risky".into();
        risky.dangerous = true;
        let mut normal = action(&["firefox"]);
        normal.name = "Firefox".into();
        config.launcher.actions = vec![risky, normal];

        // Counted by name: the list also carries the built-in mode entries.
        let offers = |config: &Config, name: &str| {
            config.usable_actions().iter().any(|a| a.name == name)
        };

        config.launcher.enable_dangerous_actions = false;
        assert!(!offers(&config, "Risky"), "a dangerous action showed unasked");
        assert!(offers(&config, "Firefox"));

        config.launcher.enable_dangerous_actions = true;
        assert!(offers(&config, "Risky"), "a dangerous action stayed hidden when asked for");
    }
}
