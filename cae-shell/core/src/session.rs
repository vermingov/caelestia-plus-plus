//! Ending the session: logging out, and the four ways of putting the machine
//! down.
//!
//! The config gives each a command, and out of the box each command is one
//! word: `logout`, `poweroff`, `hibernate`, `reboot`. Those words are not
//! programs. The QML shell took them as names for what it asked logind to do,
//! and they mean the same here, asked through the two tools that come with
//! logind. Anything else in the config is a program, and is run as written.

/// What is actually run for a command from the config.
pub fn argv(command: &[String], session: &str) -> Vec<String> {
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
        "logout" => run(&["loginctl", "terminate-session", session]),
        "poweroff" => run(&["systemctl", "poweroff"]),
        "reboot" => run(&["systemctl", "reboot"]),
        "hibernate" => run(&["systemctl", "hibernate"]),
        "suspend" => run(&["systemctl", "suspend"]),
        "suspendthenhibernate" => run(&["systemctl", "suspend-then-hibernate"]),
        _ => command.to_vec(),
    }
}

/// Runs it, out of this process's tree: what it starts has to outlive the
/// shell it is about to end.
pub fn run(command: &[String]) {
    let session = std::env::var("XDG_SESSION_ID").unwrap_or_default();
    let argv = argv(command, &session);
    let Some((program, args)) = argv.split_first() else { return };
    let _ = std::process::Command::new("setsid").arg("-f").arg(program).args(args).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(command: &[&str]) -> Vec<String> {
        command.iter().map(|word| word.to_string()).collect()
    }

    #[test]
    fn the_configs_words_are_names_for_what_logind_does() {
        assert_eq!(argv(&words(&["poweroff"]), "3"), words(&["systemctl", "poweroff"]));
        assert_eq!(argv(&words(&["reboot"]), "3"), words(&["systemctl", "reboot"]));
        assert_eq!(argv(&words(&["hibernate"]), "3"), words(&["systemctl", "hibernate"]));
        assert_eq!(argv(&words(&["logout"]), "3"), words(&["loginctl", "terminate-session", "3"]));
        assert_eq!(argv(&words(&["suspend-then-hibernate"]), "3"), words(&["systemctl", "suspend-then-hibernate"]));
        assert_eq!(argv(&words(&["systemctl", "Power-Off"]), "3"), words(&["systemctl", "poweroff"]));
    }

    #[test]
    fn anything_else_is_a_program_and_is_run_as_written() {
        assert_eq!(argv(&words(&["hyprctl", "dispatch", "exit"]), "3"), words(&["hyprctl", "dispatch", "exit"]));
        assert_eq!(argv(&words(&["wlogout"]), "3"), words(&["wlogout"]));
        // Three words are not the two-word form, whatever the first two are.
        assert_eq!(argv(&words(&["systemctl", "reboot", "--firmware-setup"]), "3"), words(&["systemctl", "reboot", "--firmware-setup"]));
        assert!(argv(&[], "3").is_empty());
    }
}
