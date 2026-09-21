//! Speaking up about the battery, and putting the machine down before it
//! runs out.
//!
//! Reading the charge is the system sampler's job; this is only what to say
//! about a reading and when. The levels are the user's, in
//! `general.battery.warnLevels`, each with its own words: one warning per
//! crossing, so a machine sitting at 19% says "low battery" once rather than
//! every time the sampler looks.

use serde_json::Value;

use crate::{config, system::Battery};

/// One level worth mentioning, and what to say at it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Level {
    /// The percentage at or below which it is said.
    pub at: i64,
    pub title: String,
    pub message: String,
    pub glyph: String,
    /// Said loudly. The last one or two usually are.
    pub critical: bool,
}

/// Everything the settings say about the battery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Smallest first, which is what makes the first crossing found the
    /// worst one crossed.
    pub levels: Vec<Level>,
    /// Below this the machine goes down rather than dies.
    pub critical: i64,
    /// Whether plugging a charger in is worth a word.
    pub say_charging: bool,
}

/// Something to say, or to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Say {
    /// A level was crossed on the way down.
    Warn(Level),
    Unplugged,
    Plugged,
    /// Nothing is left: say so, then go down after the grace below.
    Hibernate,
}

/// The levels upstream ships, which are what a file that says nothing means.
fn by_default() -> Vec<Level> {
    let level = |at, title: &str, message: &str, glyph: &str, critical| Level {
        at,
        title: title.to_string(),
        message: message.to_string(),
        glyph: glyph.to_string(),
        critical,
    };
    vec![
        level(5, "Critical battery level", "PLUG THE CHARGER RIGHT NOW!!", "battery_android_alert", true),
        level(10, "Did you see the previous message?", "You should probably plug in a charger <b>now</b>", "battery_android_frame_1", false),
        level(20, "Low battery", "You might want to plug in a charger", "battery_android_frame_2", false),
    ]
}

impl Settings {
    pub fn read() -> Settings {
        Settings::said_by(&config::read(config::File::Shell))
    }

    fn said_by(shell: &Value) -> Settings {
        let at = |path: &str| config::lookup(shell, path);
        let mut levels: Vec<Level> = match at("general.battery.warnLevels").and_then(Value::as_array) {
            Some(listed) => listed.iter().filter_map(Level::said).collect(),
            None => by_default(),
        };
        levels.sort_by_key(|level| level.at);
        Settings {
            levels,
            critical: at("general.battery.criticalLevel").and_then(Value::as_i64).unwrap_or(3),
            say_charging: at("utilities.toasts.chargingChanged").and_then(Value::as_bool).unwrap_or(true),
        }
    }
}

impl Level {
    fn said(value: &Value) -> Option<Level> {
        let word = |name: &str, otherwise: &str| {
            value.get(name).and_then(Value::as_str).unwrap_or(otherwise).to_string()
        };
        Some(Level {
            at: value.get("level")?.as_i64()?,
            title: word("title", "Battery warning"),
            message: word("message", "Battery level is low"),
            glyph: word("icon", "battery_android_alert"),
            critical: value.get("critical").and_then(Value::as_bool).unwrap_or(false),
        })
    }
}

/// What the last reading was, so that a crossing can be told from a level
/// simply being low.
#[derive(Debug, Default)]
pub struct Watch {
    /// The charge at the previous reading. A charger resets it to full, so
    /// that everything crossed on the way down is said again on the next
    /// discharge.
    was: Option<i64>,
    /// Whether a charger was plugged in at the previous reading.
    were_on_mains: Option<bool>,
    /// Set once the machine has been told to go down, so it is told once.
    going_down: bool,
}

impl Watch {
    /// What this reading is worth saying, in the order it should be said.
    /// Nothing at all on a machine with no battery.
    pub fn reading(&mut self, battery: Option<&Battery>, settings: &Settings) -> Vec<Say> {
        let Some(battery) = battery else { return Vec::new() };
        let mut saying = Vec::new();
        let plugged = battery.on_mains;

        if self.were_on_mains.is_some_and(|were| were != plugged) && settings.say_charging {
            saying.push(if plugged { Say::Plugged } else { Say::Unplugged });
        }
        self.were_on_mains = Some(plugged);

        // A charger is the end of the emergency, whether or not the machine
        // is taking charge from it yet: the fall has stopped.
        if plugged {
            self.was = Some(100);
            self.going_down = false;
            return saying;
        }

        let was = self.was.replace(battery.level);
        if let Some(was) = was {
            // Smallest first, so the first level crossed is the worst one.
            if let Some(level) = settings.levels.iter().find(|level| battery.level <= level.at && was > level.at) {
                saying.push(Say::Warn(level.clone()));
            }
        }
        if !self.going_down && battery.level <= settings.critical {
            self.going_down = true;
            saying.push(Say::Hibernate);
        }
        saying
    }

    /// Whether the machine is still on its way down. Asked after the grace
    /// period, because a charger in the meantime calls it off.
    pub fn still_going_down(&self) -> bool {
        self.going_down
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn battery(level: i64, on_mains: bool) -> Battery {
        Battery { level, charging: on_mains, on_mains, minutes: None }
    }

    #[test]
    fn a_file_that_says_nothing_means_what_upstream_ships() {
        let settings = Settings::said_by(&json!(null));
        assert_eq!(settings.levels.len(), 3);
        assert_eq!(settings.levels[0].at, 5, "smallest first");
        assert_eq!(settings.critical, 3);
        assert!(settings.say_charging);
    }

    #[test]
    fn a_level_is_said_on_the_crossing_and_not_again() {
        let settings = Settings::said_by(&json!(null));
        let mut watch = Watch::default();
        assert_eq!(watch.reading(Some(&battery(50, false)), &settings), vec![], "nothing to compare against yet");
        assert_eq!(watch.reading(Some(&battery(21, false)), &settings), vec![]);
        let crossed = watch.reading(Some(&battery(19, false)), &settings);
        assert_eq!(crossed.len(), 1);
        assert!(matches!(&crossed[0], Say::Warn(level) if level.at == 20));
        assert_eq!(watch.reading(Some(&battery(18, false)), &settings), vec![], "still low is not newly low");
    }

    #[test]
    fn falling_past_two_levels_at_once_says_the_worse_one() {
        let settings = Settings::said_by(&json!(null));
        let mut watch = Watch::default();
        watch.reading(Some(&battery(30, false)), &settings);
        let crossed = watch.reading(Some(&battery(8, false)), &settings);
        assert!(matches!(&crossed[0], Say::Warn(level) if level.at == 10));
    }

    #[test]
    fn a_charger_resets_the_warnings_and_calls_off_the_hibernate() {
        let settings = Settings::said_by(&json!(null));
        let mut watch = Watch::default();
        watch.reading(Some(&battery(30, false)), &settings);
        assert!(watch.reading(Some(&battery(2, false)), &settings).contains(&Say::Hibernate));
        assert!(watch.still_going_down());

        assert_eq!(watch.reading(Some(&battery(2, true)), &settings), vec![Say::Plugged]);
        assert!(!watch.still_going_down());

        // Unplugged again at 19%: the charger left the watch at full, so
        // everything the fall passes is said again.
        let said = watch.reading(Some(&battery(19, false)), &settings);
        assert_eq!(said[0], Say::Unplugged);
        assert!(matches!(&said[1], Say::Warn(level) if level.at == 20));
        assert_eq!(watch.reading(Some(&battery(18, false)), &settings), vec![]);
    }

    #[test]
    fn the_machine_is_only_told_to_go_down_once() {
        let settings = Settings::said_by(&json!(null));
        let mut watch = Watch::default();
        watch.reading(Some(&battery(30, false)), &settings);
        assert!(watch.reading(Some(&battery(3, false)), &settings).contains(&Say::Hibernate));
        assert!(!watch.reading(Some(&battery(2, false)), &settings).contains(&Say::Hibernate));
    }

    #[test]
    fn a_machine_with_no_battery_has_nothing_to_say() {
        let settings = Settings::said_by(&json!(null));
        let mut watch = Watch::default();
        assert_eq!(watch.reading(None, &settings), vec![]);
    }

    #[test]
    fn the_settings_own_levels_replace_the_shipped_ones() {
        let shell = json!({ "general": { "battery": { "criticalLevel": 1, "warnLevels": [
            { "level": 15, "title": "Getting low", "message": "Plug in", "icon": "bolt", "critical": true },
            { "level": 40 }
        ] } }, "utilities": { "toasts": { "chargingChanged": false } } });
        let settings = Settings::said_by(&shell);
        assert_eq!(settings.levels.len(), 2);
        assert_eq!(settings.levels[0].at, 15);
        assert_eq!(settings.levels[1].title, "Battery warning", "a level with no words gets the plain ones");
        assert_eq!(settings.critical, 1);
        assert!(!settings.say_charging);

        let mut watch = Watch::default();
        watch.reading(Some(&battery(50, false)), &settings);
        assert_eq!(watch.reading(Some(&battery(45, true)), &settings), vec![], "asked not to mention the charger");
    }
}
