//! The history on disk.
//!
//! The same file the shell's server used, so that moving the server did not
//! throw away what was already there — which means reading what that server
//! wrote, and it wrote its times as ISO-8601 strings and its entries without
//! an id.

use std::path::PathBuf;

use serde::Deserialize;

use super::{Notification, MAX_HISTORY};

/// A directory the history lives in.
#[derive(Clone, Debug)]
pub struct Store {
    dir: Option<PathBuf>,
}

impl Store {
    /// The real one, under `$XDG_STATE_HOME/caelestia`.
    pub fn xdg() -> Store {
        let dir = std::env::var("HOME").ok().map(|home| {
            let state = std::env::var("XDG_STATE_HOME").unwrap_or(format!("{home}/.local/state"));
            PathBuf::from(state).join("caelestia")
        });
        Store { dir }
    }

    /// One that keeps nothing, for tests: a test that wrote to the real
    /// directory would replace the history of the machine it ran on.
    #[cfg(test)]
    pub fn nowhere() -> Store {
        Store { dir: None }
    }

    fn path(&self, name: &str) -> Option<PathBuf> {
        Some(self.dir.as_ref()?.join(name))
    }

    pub fn load(&self) -> Vec<Notification> {
        let Some(path) = self.path("notifs.json") else { return Vec::new() };
        let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
        parse(&text)
    }

    pub fn save(&self, list: &[Notification]) {
        let Some(path) = self.path("notifs.json") else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let Ok(text) = serde_json::to_string(list) else { return };
        // Written beside and moved into place: a bar killed mid-write would
        // otherwise leave a truncated file, and the next start would find no
        // history at all.
        let temporary = path.with_extension("json.new");
        if std::fs::write(&temporary, text).is_ok() {
            let _ = std::fs::rename(&temporary, &path);
        }
    }

    pub fn load_dnd(&self) -> bool {
        self.path("notifs-dnd").is_some_and(|p| p.exists())
    }

    pub fn save_dnd(&self, on: bool) {
        let Some(path) = self.path("notifs-dnd") else { return };
        if on {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(path, "");
        } else {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// A history file as a list, newest first.
///
/// Entry by entry: a single one this version cannot read must not cost the
/// rest of the history, which is what parsing straight into a `Vec` would do
/// — and the next save would then write the empty list over all of it.
fn parse(text: &str) -> Vec<Notification> {
    let entries: Vec<serde_json::Value> = serde_json::from_str(text).unwrap_or_default();
    let mut list: Vec<Notification> =
        entries.into_iter().filter_map(|entry| serde_json::from_value(entry).ok()).collect();

    // Restored entries are history, never toasts: coming back from a restart
    // is not a reason to throw three hundred popups at somebody.
    list.iter_mut().for_each(|notif| notif.popup = false);
    list.sort_by(|a, b| b.time.cmp(&a.time));
    list.truncate(MAX_HISTORY);

    // The QML server never wrote an id. Every entry needs one that nothing
    // else has, or closing one from the list would close them all.
    let mut next = list.iter().map(|n| n.id).max().unwrap_or(0);
    for notif in list.iter_mut().filter(|n| n.id == 0) {
        next += 1;
        notif.id = next;
    }
    list
}

/// A time from either server: milliseconds, or the ISO-8601 string the QML
/// one wrote.
pub fn epoch_ms<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::Number(number) => Ok(number.as_f64().unwrap_or_default() as i64),
        serde_json::Value::String(text) => Ok(parse_iso8601(&text).unwrap_or_default()),
        _ => Ok(0),
    }
}

/// `2026-09-20T12:47:07.628Z`, and the same with a `+02:00` style offset.
///
/// Hand-written rather than pulled in: this is the only date this program
/// ever parses, and it is always one this desktop wrote itself.
fn parse_iso8601(text: &str) -> Option<i64> {
    let (date, rest) = text.split_once('T')?;
    let mut parts = date.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;

    // Whatever follows the time is the zone. A negative offset cannot be
    // found by searching for `Z` or `+`, so it is looked for separately —
    // the time itself contains no minus sign, only the offset does.
    let (clock, zone_minutes) = match rest.find(['Z', '+']).or_else(|| rest.rfind('-')) {
        Some(at) if rest.as_bytes()[at] == b'Z' => (&rest[..at], 0),
        Some(at) => (&rest[..at], offset(&rest[at..])?),
        None => (rest, 0),
    };

    let mut clock = clock.split(':');
    let hour: i64 = clock.next()?.parse().ok()?;
    let minute: i64 = clock.next()?.parse().ok()?;
    let seconds: f64 = clock.next().unwrap_or("0").parse().ok()?;

    let days = days_from_civil(year, month, day);
    let ms = ((days * 86_400 + hour * 3600 + minute * 60) * 1000) as f64 + seconds * 1000.0;
    // The clock read is local to that zone, so getting back to UTC means
    // taking the offset off — both ways round.
    Some(ms as i64 - zone_minutes * 60_000)
}

/// `+02:00` or `-05:30` as minutes.
fn offset(text: &str) -> Option<i64> {
    let sign = if text.starts_with('-') { -1 } else { 1 };
    let (hours, minutes) = text[1..].split_once(':')?;
    Some(sign * (hours.parse::<i64>().ok()? * 60 + minutes.parse::<i64>().ok()?))
}

/// Days from 1970-01-01, by Howard Hinnant's civil calendar algorithm. Shifts
/// the year to start in March so that the leap day is the last day of it and
/// the month lengths fall into a repeating pattern.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The history this machine already had, written by the QML server.
    ///
    /// The shapes differ in two ways that both used to be fatal: `time` is an
    /// ISO-8601 string rather than a number, and there is no `id` at all.
    /// Parsing straight into the struct failed, `unwrap_or_default` turned
    /// that into an empty list, and the next save would have written the
    /// empty list over three hundred real notifications.
    #[test]
    fn the_history_the_qml_server_wrote_is_still_readable() {
        let list = parse(include_str!("../../tests/data/qml-history.json"));

        assert_eq!(list.len(), 6, "entries the QML server wrote were dropped");
        assert!(list.iter().all(|n| n.time > 0), "an ISO-8601 time did not survive");
        assert!(list.iter().any(|n| !n.summary.is_empty()));
        assert!(list.windows(2).all(|pair| pair[0].time >= pair[1].time), "not newest first");
    }

    #[test]
    fn entries_with_no_id_are_given_one_each() {
        let list = parse(include_str!("../../tests/data/qml-history.json"));
        let mut ids: Vec<u32> = list.iter().map(|n| n.id).collect();
        assert!(ids.iter().all(|id| *id > 0), "an entry kept the id zero");
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), list.len(), "two entries share an id");
    }

    #[test]
    fn one_unreadable_entry_does_not_cost_the_rest() {
        let text = r#"[{"summary": "fine", "time": 5}, "not an object", {"summary": "also fine", "time": 9}]"#;
        let list = parse(text);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].summary, "also fine");
    }

    #[test]
    fn iso8601_times_land_where_they_should() {
        // The epoch itself, and a day the calendar arithmetic has to get
        // right: 2000 is a leap year, 1900 was not.
        assert_eq!(parse_iso8601("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(parse_iso8601("2000-03-01T00:00:00Z"), Some(951_868_800_000));
        // Milliseconds are kept.
        assert_eq!(parse_iso8601("1970-01-01T00:00:00.250Z"), Some(250));
        // An offset is applied, not ignored: 01:00+01:00 is midnight UTC.
        assert_eq!(parse_iso8601("1970-01-01T01:00:00+01:00"), Some(0));
        assert_eq!(parse_iso8601("1969-12-31T23:00:00-01:00"), Some(0));
        // Nonsense is nothing, not a panic.
        assert_eq!(parse_iso8601("not a date"), None);
    }
}
