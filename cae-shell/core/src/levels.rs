//! What the settings say about moving a level: how far one notch of a wheel
//! takes the volume or the brightness, and how loud is as loud as it gets.

use serde_json::Value;

use crate::config;

/// All in percent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Steps {
    /// A notch, for the speakers and the microphone alike.
    pub volume: i64,
    pub brightness: i64,
    /// What is above 100 is headroom, and how much of it there is is the
    /// user's to say.
    pub loudest: i64,
}

impl Steps {
    pub fn read() -> Steps {
        Steps::said_by(&config::read(config::File::Shell))
    }

    /// The file keeps them as parts of one, which is how upstream's shell
    /// reads them; the bounds are the ones its settings page offers.
    pub fn said_by(shell: &Value) -> Steps {
        let percent = |path: &str, otherwise: f64| (config::lookup(shell, path).and_then(Value::as_f64).unwrap_or(otherwise) * 100.).round() as i64;
        Steps {
            volume: percent("services.audioIncrement", 0.1).clamp(1, 50),
            brightness: percent("services.brightnessIncrement", 0.1).clamp(1, 50),
            loudest: percent("services.maxVolume", 1.).clamp(50, 200),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::Steps;

    #[test]
    fn a_file_that_says_nothing_means_a_tenth_at_a_time_and_no_headroom() {
        assert_eq!(Steps::said_by(&json!(null)), Steps { volume: 10, brightness: 10, loudest: 100 });
    }

    #[test]
    fn what_the_file_says_is_read_as_percent_and_kept_within_what_the_settings_offer() {
        let shell = json!({ "services": { "audioIncrement": 0.02, "brightnessIncrement": 0.9, "maxVolume": 2.0 } });
        assert_eq!(Steps::said_by(&shell), Steps { volume: 2, brightness: 50, loudest: 200 });
        let silly = json!({ "services": { "audioIncrement": 0, "maxVolume": 9 } });
        assert_eq!(Steps::said_by(&silly), Steps { volume: 1, brightness: 10, loudest: 200 });
    }
}
