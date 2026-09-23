//! Ending the session: logging out, and the four ways of putting the machine
//! down.
//!
//! The config gives each a command, and out of the box each command is one
//! word: `logout`, `poweroff`, `hibernate`, `reboot`. Those words are not
//! programs. The QML shell took them as names for what it asked logind to do,
//! and they mean the same here, asked through the two tools that come with
//! logind. Anything else in the config is a program, and is run as written.

/// What is actually run for a command from the config. `session` is asked
/// only for logging out, the one that has to name the session it ends.
pub fn argv(command: &[String], session: impl FnOnce() -> String) -> Vec<String> {
    let Some(first) = command.first() else { return Vec::new() };
    // `systemctl poweroff` and `loginctl hibernate` name the same things, and
    // so does `power-off`: what is matched is the word with its dashes out.
    let named = match (first.as_str(), command.len()) {
        ("systemctl" | "loginctl", 2) => command[1].as_str(),
        (_, 1) => first.as_str(),
        _ => "",
    };
    let word: String = named.chars().filter(|c| *c != '-' && *c != '_').collect::<String>().to_lowercase();
    let run = |words: &[&str]| words.iter().map(|word| word.to_string()).collect();
    match word.as_str() {
        "logout" => run(&["loginctl", "terminate-session", &session()]),
        "poweroff" => run(&["systemctl", "poweroff"]),
        "reboot" => run(&["systemctl", "reboot"]),
        "hibernate" => run(&["systemctl", "hibernate"]),
        "suspend" => run(&["systemctl", "suspend"]),
        "suspendthenhibernate" => run(&["systemctl", "suspend-then-hibernate"]),
        _ => command.to_vec(),
    }
}

/// Runs it, out of this process's tree and out of its service: what it
/// starts has to outlive the shell it is about to end.
pub fn run(command: &[String]) {
    let argv = argv(command, || crate::logind::session_id().unwrap_or_default());
    let Some(program) = argv.first() else { return };
    let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
    crate::children::launch(program, &argv);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(command: &[&str]) -> Vec<String> {
        command.iter().map(|word| word.to_string()).collect()
    }

    fn session_three() -> String {
        "3".to_string()
    }

    #[test]
    fn the_configs_words_are_names_for_what_logind_does() {
        assert_eq!(argv(&words(&["poweroff"]), session_three), words(&["systemctl", "poweroff"]));
        assert_eq!(argv(&words(&["reboot"]), session_three), words(&["systemctl", "reboot"]));
        assert_eq!(argv(&words(&["hibernate"]), session_three), words(&["systemctl", "hibernate"]));
        assert_eq!(argv(&words(&["logout"]), session_three), words(&["loginctl", "terminate-session", "3"]));
        assert_eq!(argv(&words(&["suspend-then-hibernate"]), session_three), words(&["systemctl", "suspend-then-hibernate"]));
        assert_eq!(argv(&words(&["systemctl", "Power-Off"]), session_three), words(&["systemctl", "poweroff"]));
    }

    #[test]
    fn anything_else_is_a_program_and_is_run_as_written() {
        assert_eq!(argv(&words(&["hyprctl", "dispatch", "exit"]), session_three), words(&["hyprctl", "dispatch", "exit"]));
        assert_eq!(argv(&words(&["wlogout"]), session_three), words(&["wlogout"]));
        // Three words are not the two-word form, whatever the first two are.
        assert_eq!(argv(&words(&["systemctl", "reboot", "--firmware-setup"]), session_three), words(&["systemctl", "reboot", "--firmware-setup"]));
        assert!(argv(&[], session_three).is_empty());
    }
}
