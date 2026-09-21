//! The wall clock, in the words the old bar used.
//!
//! That bar was a browser, and a browser words dates from `LANG`: English
//! names here, with the Danish dot between hours and minutes, where a plain
//! `strftime` under this machine's `LC_TIME` would say "søn" and use a colon.

use std::ffi::{CStr, c_char};
use std::time::Duration;

fn local() -> libc::tm {
    // SAFETY: `time` and `localtime_r` only write through the pointers given,
    // and both point at values that live for the call.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut parts: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut parts);
        parts
    }
}

pub fn until_next_minute() -> Duration {
    Duration::from_secs(60 - local().tm_sec.clamp(0, 59) as u64)
}

/// The separator CLDR gives the territory in `LANG`. A few write the time
/// with a dot, and a clock that suddenly uses a colon reads as a different
/// clock.
fn separator() -> char {
    let lang = std::env::var("LANG").unwrap_or_default();
    let territory = lang.split(['_', '.']).nth(1).unwrap_or_default();
    if matches!(territory, "DK" | "FI" | "ID") { '.' } else { ':' }
}

fn speaks_english() -> bool {
    std::env::var("LANG").unwrap_or_default().starts_with("en")
}

/// One field of the date as the C library words it, which is in the language
/// of `LC_TIME`.
fn worded(parts: &libc::tm, format: &CStr) -> String {
    let mut buffer = [0 as c_char; 64];
    // SAFETY: the buffer is as long as it is said to be, and the format is
    // NUL-terminated by its type.
    unsafe {
        libc::strftime(buffer.as_mut_ptr(), buffer.len(), format.as_ptr(), parts);
        CStr::from_ptr(buffer.as_ptr()).to_string_lossy().into_owned()
    }
}

const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November",
    "December",
];

/// The weekday in the language of `LANG`, which is not always the language of
/// `LC_TIME`.
fn weekday(parts: &libc::tm) -> String {
    if speaks_english() { DAYS[parts.tm_wday.clamp(0, 6) as usize].to_string() } else { worded(parts, c"%A") }
}

fn month(parts: &libc::tm) -> String {
    if speaks_english() { MONTHS[parts.tm_mon.clamp(0, 11) as usize].to_string() } else { worded(parts, c"%B") }
}

/// What the bar has room for: "Sun 20", and "17.42".
pub fn now() -> (String, String) {
    let parts = local();
    let day: String = weekday(&parts).chars().take(3).collect();
    (format!("{day} {}", parts.tm_mday), format!("{:02}{}{:02}", parts.tm_hour, separator(), parts.tm_min))
}

/// Today in the three parts the desktop clock sets one under the other: the
/// month, the day of it, and the weekday.
pub fn today_worded() -> (String, String, String) {
    let parts = local();
    (month(&parts), format!("{:02}", parts.tm_mday), weekday(&parts))
}

/// Spelled out in full, for the popout: "Sunday, 20 September 2026".
pub fn long_date() -> String {
    let parts = local();
    format!("{}, {} {} {}", weekday(&parts), parts.tm_mday, month(&parts), parts.tm_year + 1900)
}

/// The ISO week, which is the one a calendar with a week number on it means.
pub fn week() -> u32 {
    worded(&local(), c"%V").trim().parse().unwrap_or_default()
}

/// Today, as a year, a month from 1 and a day from 1.
pub fn today() -> (i32, u32, u32) {
    let parts = local();
    (parts.tm_year + 1900, parts.tm_mon as u32 + 1, parts.tm_mday as u32)
}

/// An hour of the day as a clock with twelve hours on it says it.
pub fn of_twelve(hour: u32) -> (u32, &'static str) {
    (if hour % 12 == 0 { 12 } else { hour % 12 }, if hour < 12 { "am" } else { "pm" })
}

/// The hour and the minute as two figures each, for a clock set large
/// enough that each is a thing of its own, and which half of the day it is
/// for a clock with only twelve hours on it.
pub fn hour_and_minute(twelve: bool) -> (String, String, &'static str) {
    let parts = local();
    let (hour, half) = if twelve { of_twelve(parts.tm_hour as u32) } else { (parts.tm_hour as u32, "") };
    (format!("{hour:02}"), format!("{:02}", parts.tm_min), half)
}

/// `14:00` as a clock here says it: `14.00`, or `2 pm`.
pub fn hour_said(hour: u32, twelve: bool) -> String {
    if twelve {
        let (hour, half) = of_twelve(hour);
        return format!("{hour} {half}");
    }
    format!("{hour:02}{}00", separator())
}

/// A day of the week by its number from Monday, in three letters.
pub fn weekday_short(from_monday: u32) -> String {
    let mut parts = local();
    parts.tm_wday = ((from_monday + 1) % 7) as i32;
    weekday(&parts).chars().take(3).collect()
}

/// A month by its number from 1, in the language of `LANG`.
pub fn month_name(month: u32) -> String {
    let mut parts = local();
    parts.tm_mon = month.clamp(1, 12) as i32 - 1;
    self::month(&parts)
}

pub fn days_in(year: i32, month: u32) -> u32 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => 31,
    }
}

/// The day of the week a date falls on, Monday being 0. Sakamoto's method:
/// the months are offsets from a year that starts in March, so that the leap
/// day comes last and changes nothing before it.
pub fn weekday_of(year: i32, month: u32, day: u32) -> u32 {
    const OFFSETS: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let year = if month < 3 { year - 1 } else { year };
    let sunday_first = (year + year / 4 - year / 100 + year / 400 + OFFSETS[month as usize - 1] + day as i32).rem_euclid(7);
    ((sunday_first + 6) % 7) as u32
}

/// Whether a week here begins on a Monday, which in most of the world it
/// does. The territory in `LANG` says, for the few where it begins on Sunday.
pub fn week_starts_on_monday() -> bool {
    let lang = std::env::var("LANG").unwrap_or_default();
    let territory = lang.split(['_', '.']).nth(1).unwrap_or_default();
    !matches!(territory, "US" | "CA" | "MX" | "BR" | "JP" | "KR" | "TW" | "IL" | "IN" | "PH")
}

/// The month after `(year, month)` by `by` months, which may be negative.
pub fn month_after(year: i32, month: u32, by: i32) -> (i32, u32) {
    let months = year * 12 + month as i32 - 1 + by;
    (months.div_euclid(12), months.rem_euclid(12) as u32 + 1)
}

/// The initials of the days of the week, in the order a week here runs.
pub fn weekday_initials() -> [&'static str; 7] {
    if week_starts_on_monday() { ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"] } else { ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"] }
}

/// What time it was, as a clock on the wall would have said: "17.42".
pub fn at(moment: std::time::SystemTime) -> String {
    let seconds = moment.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as libc::time_t;
    // SAFETY: `localtime_r` only writes through the pointer given, which
    // points at a value that lives for the call.
    let parts = unsafe {
        let mut parts: libc::tm = std::mem::zeroed();
        libc::localtime_r(&seconds, &mut parts);
        parts
    };
    format!("{:02}{}{:02}", parts.tm_hour, separator(), parts.tm_min)
}

/// A length as a stopwatch shows it: `0.42`, `12.03`, `1:04.07`.
pub fn counted(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    let separator = separator();
    match hours {
        0 => format!("{minutes}{separator}{seconds:02}"),
        _ => format!("{hours}:{minutes:02}{separator}{seconds:02}"),
    }
}

/// When a recording was made, from the name the CLI gave it:
/// "recording_20260715_16-34-52" is "15 July at 16.34".
pub fn when_recorded(name: &str) -> String {
    let Some(stamp) = name.strip_prefix("recording_") else { return name.to_string() };
    let number = |at: std::ops::Range<usize>| stamp.get(at).and_then(|digits| digits.parse::<u32>().ok());
    let (Some(month), Some(day), Some(hour), Some(minute)) = (number(4..6), number(6..8), number(9..11), number(12..14)) else {
        return name.to_string();
    };
    format!("{day} {} at {hour:02}{}{minute:02}", month_name(month), separator())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_fall_on_the_days_they_fell_on() {
        assert_eq!(weekday_of(2026, 9, 20), 6, "20 September 2026 was a Sunday");
        assert_eq!(weekday_of(2000, 2, 29), 1, "the leap day of 2000 was a Tuesday");
        assert_eq!(weekday_of(1970, 1, 1), 3, "the epoch began on a Thursday");
        assert_eq!(weekday_of(2024, 1, 1), 0);
    }

    #[test]
    fn a_clock_with_twelve_hours_on_it_has_no_hour_nought() {
        assert_eq!(of_twelve(0), (12, "am"));
        assert_eq!(of_twelve(12), (12, "pm"));
        assert_eq!(of_twelve(15), (3, "pm"));
        assert_eq!(hour_said(9, true), "9 am");
    }

    #[test]
    fn months_are_as_long_as_they_are() {
        assert_eq!(days_in(2026, 2), 28);
        assert_eq!(days_in(2024, 2), 29);
        assert_eq!(days_in(1900, 2), 28, "a century is a leap year only every fourth time");
        assert_eq!(days_in(2000, 2), 29);
        assert_eq!((days_in(2026, 9), days_in(2026, 12)), (30, 31));
    }

    #[test]
    fn the_month_after_december_is_next_years_january() {
        assert_eq!(month_after(2026, 12, 1), (2027, 1));
        assert_eq!(month_after(2026, 1, -1), (2025, 12));
        assert_eq!(month_after(2026, 9, -21), (2024, 12));
    }

    #[test]
    fn a_length_is_counted_the_way_a_stopwatch_shows_it() {
        let said = |seconds| counted(Duration::from_secs(seconds)).replace('.', ":");
        assert_eq!(said(7), "0:07");
        assert_eq!(said(723), "12:03");
        assert_eq!(said(3847), "1:04:07");
    }

    #[test]
    fn a_recordings_name_says_when_it_was_made() {
        assert_eq!(when_recorded("recording_20260715_16-34-52").replace('.', ":"), "15 July at 16:34");
        // Anything else is shown as it is rather than guessed at.
        assert_eq!(when_recorded("something-else"), "something-else");
        assert_eq!(when_recorded("recording_short"), "recording_short");
    }
}
