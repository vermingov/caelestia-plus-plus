//! What the shell does about a desktop nobody is at.
//!
//! The lengths of quiet and what each one is for are the user's, in
//! `general.idle.timeouts`: lock after three minutes, the screens off after
//! five, the machine down after ten. Noticing the quiet is a Wayland
//! protocol and belongs to whatever is drawing; this is only what the
//! settings say and how each thing is done.

use std::time::Duration;

use serde_json::Value;

use crate::{config, hypr, services, session};

/// What to do when a length of quiet passes, or when somebody comes back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Do {
    Lock,
    Unlock,
    /// The screens off, or on again.
    Screens(bool),
    /// A command, which may be one of the words the session menu knows.
    Run(Vec<String>),
}

impl Do {
    /// What the settings put there: a word, a list of words, or nothing.
    fn said(value: Option<&Value>) -> Option<Do> {
        match value? {
            Value::String(word) => Some(match word.as_str() {
                "lock" => Do::Lock,
                "unlock" => Do::Unlock,
                "dpms off" => Do::Screens(false),
                "dpms on" => Do::Screens(true),
                other => Do::Run(other.split_whitespace().map(str::to_string).collect()),
            }),
            Value::Array(words) => {
                let words: Vec<String> = words.iter().filter_map(|word| word.as_str().map(str::to_string)).collect();
                (!words.is_empty()).then_some(Do::Run(words))
            }
            _ => None,
        }
    }
}

/// One length of quiet, and what it is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Timeout {
    pub after: Duration,
    pub idle: Option<Do>,
    pub back: Option<Do>,
    pub enabled: bool,
    /// Not counted while something is playing, or while the machine is on
    /// the mains. Each length says for itself, and the settings say for all
    /// of them.
    pub not_while_playing: bool,
    pub not_while_charging: bool,
    /// Whether something asking to be left alone stops it counting.
    pub respect_inhibitors: bool,
}

/// Everything the settings say about idleness.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    pub timeouts: Vec<Timeout>,
    pub lock_before_sleep: bool,
}

/// The lengths upstream ships, which are what a file that says nothing means.
fn by_default() -> Vec<Timeout> {
    let timeout = |seconds, idle, back| Timeout {
        after: Duration::from_secs(seconds),
        idle,
        back,
        enabled: true,
        not_while_playing: false,
        not_while_charging: false,
        respect_inhibitors: true,
    };
    vec![
        timeout(180, Some(Do::Lock), None),
        timeout(300, Some(Do::Screens(false)), Some(Do::Screens(true))),
        timeout(600, Some(Do::Run(vec!["suspendThenHibernate".to_string()])), None),
    ]
}

impl Settings {
    pub fn read() -> Settings {
        Settings::said_by(&config::read(config::File::Shell))
    }

    fn said_by(shell: &Value) -> Settings {
        let at = |path: &str| config::lookup(shell, path);
        let flag = |path: &str, otherwise: bool| at(path).and_then(Value::as_bool).unwrap_or(otherwise);
        let listed = at("general.idle.timeouts").and_then(Value::as_array);
        let all_playing = flag("general.idle.inhibitWhenAudio", true);
        let all_charging = flag("general.idle.inhibitWhenCharging", false);

        let mut timeouts: Vec<Timeout> = match listed {
            Some(listed) => listed.iter().filter_map(Timeout::said).collect(),
            None => by_default(),
        };
        // The two that hold for every length, which each length may also ask
        // for on its own.
        for timeout in &mut timeouts {
            timeout.not_while_playing |= all_playing;
            timeout.not_while_charging |= all_charging;
        }
        Settings { timeouts, lock_before_sleep: flag("general.idle.lockBeforeSleep", true) }
    }
}

impl Timeout {
    fn said(value: &Value) -> Option<Timeout> {
        let flag = |name: &str| value.get(name).and_then(Value::as_bool);
        let seconds = value.get("timeout")?.as_f64()?;
        (seconds > 0.).then_some(Timeout {
            after: Duration::from_secs_f64(seconds),
            idle: Do::said(value.get("idleAction")),
            back: Do::said(value.get("returnAction")),
            enabled: flag("enabled").unwrap_or(true),
            not_while_playing: flag("inhibitWhenAudio").unwrap_or(false),
            not_while_charging: flag("inhibitWhenCharging").unwrap_or(false),
            respect_inhibitors: flag("respectInhibitors").unwrap_or(true),
        })
    }
}

/// Does it. Blocking: every one of these is a program or a socket.
pub fn run(what: &Do) {
    match what {
        // Whoever calls this locks the screen itself where the lock is
        // theirs to draw; these are for while it is still the old shell's.
        Do::Lock => drop(services::ipc("lock", "lock", &[])),
        Do::Unlock => drop(services::ipc("lock", "unlock", &[])),
        Do::Screens(on) => hypr::dispatch(if *on { "dpms on" } else { "dpms off" }),
        Do::Run(words) => session::run(words),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_file_that_says_nothing_means_what_upstream_ships() {
        let settings = Settings::said_by(&json!(null));
        assert_eq!(settings.timeouts.len(), 3);
        assert_eq!(settings.timeouts[0].idle, Some(Do::Lock));
        assert_eq!(settings.timeouts[1].back, Some(Do::Screens(true)));
        assert!(settings.lock_before_sleep);
        // The default for all of them is on, so every length carries it.
        assert!(settings.timeouts.iter().all(|timeout| timeout.not_while_playing));
    }

    #[test]
    fn a_length_is_read_with_what_it_is_for() {
        let shell = json!({ "general": { "idle": { "inhibitWhenAudio": false, "timeouts": [
            { "timeout": 90, "idleAction": "dpms off", "returnAction": "dpms on", "respectInhibitors": false },
            { "timeout": 120, "idleAction": ["systemctl", "suspend"], "enabled": false },
            { "timeout": 0, "idleAction": "lock" }
        ] } } });
        let settings = Settings::said_by(&shell);
        assert_eq!(settings.timeouts.len(), 2, "a length of nothing is not a length");
        assert_eq!(settings.timeouts[0].after, Duration::from_secs(90));
        assert_eq!(settings.timeouts[0].idle, Some(Do::Screens(false)));
        assert!(!settings.timeouts[0].respect_inhibitors);
        assert!(!settings.timeouts[0].not_while_playing);
        assert_eq!(settings.timeouts[1].idle, Some(Do::Run(vec!["systemctl".to_string(), "suspend".to_string()])));
        assert!(!settings.timeouts[1].enabled);
    }
}
