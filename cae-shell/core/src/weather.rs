//! The weather: where this machine is, and what the sky is doing there.
//!
//! The same three services the QML shell asked, in the same order. Where the
//! config names a place, that place. Where it does not, the address this
//! machine is seen from places it roughly, which is all a forecast needs. The
//! forecast is Open-Meteo's, which wants no key. There are three requests an
//! hour at most, each made the way `web` makes them.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::web;

/// How old a forecast may be before another is asked for.
pub const FRESH: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Place {
    pub latitude: f64,
    pub longitude: f64,
    pub city: String,
}

/// `55.68, 12.57`, as the config may give a place.
fn coordinates(text: &str) -> Option<(f64, f64)> {
    let (latitude, longitude) = text.split_once(',')?;
    Some((latitude.trim().parse().ok()?, longitude.trim().parse().ok()?))
}

/// Where the forecast is for. `configured` is `services.weatherLocation`:
/// coordinates, the name of a place, or nothing.
pub fn place(configured: &str) -> Option<Place> {
    let configured = configured.trim();
    if let Some((latitude, longitude)) = coordinates(configured) {
        return Some(Place { latitude, longitude, city: city_at(latitude, longitude).unwrap_or_default() });
    }
    if !configured.is_empty() {
        let name = web::encoded(configured);
        let found = web::json(&format!("https://geocoding-api.open-meteo.com/v1/search?name={name}&count=1&format=json"), None)?;
        let first = found.get("results")?.get(0)?;
        return Some(Place {
            latitude: first.get("latitude")?.as_f64()?,
            longitude: first.get("longitude")?.as_f64()?,
            city: first.get("name").and_then(Value::as_str).unwrap_or(configured).to_string(),
        });
    }
    let seen_from = web::json("https://ipinfo.io/json", None)?;
    let (latitude, longitude) = coordinates(seen_from.get("loc")?.as_str()?)?;
    Some(Place { latitude, longitude, city: seen_from.get("city").and_then(Value::as_str).unwrap_or_default().to_string() })
}

/// The town at some coordinates, from OpenStreetMap, whose usage policy asks
/// for a request that says who is asking.
fn city_at(latitude: f64, longitude: f64) -> Option<String> {
    let url = format!("https://nominatim.openstreetmap.org/reverse?lat={latitude}&lon={longitude}&format=geocodejson");
    let found = web::json(&url, None)?;
    let about = found.get("features")?.get(0)?.get("properties")?.get("geocoding")?;
    let town = if about.get("type").and_then(Value::as_str) == Some("city") { about.get("name") } else { about.get("city") };
    town.and_then(Value::as_str).map(str::to_string)
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Now {
    pub code: i64,
    pub celsius: f64,
    pub feels_like: f64,
    pub humidity: i64,
    /// Kilometres an hour.
    pub wind: f64,
    pub daylight: bool,
    /// `06:41`, local to the place.
    pub sunrise: String,
    pub sunset: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Hour {
    /// `2026-09-20T14:00`, local to the place.
    pub at: String,
    pub code: i64,
    pub celsius: f64,
    /// The chance of rain, as a percentage.
    pub wet: i64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Day {
    /// `2026-09-20`.
    pub date: String,
    pub code: i64,
    pub high: f64,
    pub low: f64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Weather {
    pub place: Place,
    pub now: Now,
    /// From the hour that is current, onwards.
    pub hours: Vec<Hour>,
    pub days: Vec<Day>,
    /// Seconds since the epoch at which this was fetched.
    pub fetched: u64,
}

impl Weather {
    pub fn is_fresh(&self) -> bool {
        let fetched = SystemTime::UNIX_EPOCH + Duration::from_secs(self.fetched);
        SystemTime::now().duration_since(fetched).is_ok_and(|age| age < FRESH)
    }
}

/// Open-Meteo's answer as a forecast. `current_hour` is the place's own
/// `2026-09-20T14`, which is where the hours worth showing begin.
fn parse(answer: &Value, place: Place, fetched: u64) -> Option<Weather> {
    let (current, hourly, daily) = (answer.get("current")?, answer.get("hourly")?, answer.get("daily")?);
    let number = |of: &Value, key: &str| of.get(key).and_then(Value::as_f64);
    let column = |of: &Value, key: &str| of.get(key).and_then(Value::as_array).cloned().unwrap_or_default();
    let clock = |of: &Value, key: &str| {
        let stamp = of.get(key).and_then(|days| days.get(0)).and_then(Value::as_str).unwrap_or_default();
        stamp.split_once('T').map_or(String::new(), |(_, time)| time.to_string())
    };

    let now = Now {
        code: number(current, "weather_code")? as i64,
        celsius: number(current, "temperature_2m")?,
        feels_like: number(current, "apparent_temperature").unwrap_or_default(),
        humidity: number(current, "relative_humidity_2m").unwrap_or_default() as i64,
        wind: number(current, "wind_speed_10m").unwrap_or_default(),
        daylight: number(current, "is_day").unwrap_or(1.) != 0.,
        sunrise: clock(daily, "sunrise"),
        sunset: clock(daily, "sunset"),
    };

    // The hours are the whole week's, from midnight today. What has already
    // happened is not a forecast: they begin at the hour it is there now.
    let this_hour = current.get("time").and_then(Value::as_str).map(|time| time.get(..13).unwrap_or(time).to_string()).unwrap_or_default();
    let (times, codes, temperatures, chances) =
        (column(hourly, "time"), column(hourly, "weather_code"), column(hourly, "temperature_2m"), column(hourly, "precipitation_probability"));
    let hours = times.iter().enumerate().filter_map(|(index, time)| {
        let at = time.as_str()?;
        (at.get(..13).unwrap_or(at) >= this_hour.as_str()).then(|| Hour {
            at: at.to_string(),
            code: codes.get(index).and_then(Value::as_i64).unwrap_or_default(),
            celsius: temperatures.get(index).and_then(Value::as_f64).unwrap_or_default(),
            wet: chances.get(index).and_then(Value::as_i64).unwrap_or_default(),
        })
    });

    let (dates, codes, highs, lows) =
        (column(daily, "time"), column(daily, "weather_code"), column(daily, "temperature_2m_max"), column(daily, "temperature_2m_min"));
    let days = dates.iter().enumerate().filter_map(|(index, date)| {
        Some(Day {
            date: date.as_str()?.to_string(),
            code: codes.get(index).and_then(Value::as_i64).unwrap_or_default(),
            high: highs.get(index).and_then(Value::as_f64).unwrap_or_default(),
            low: lows.get(index).and_then(Value::as_f64).unwrap_or_default(),
        })
    });

    Some(Weather { place, now, hours: hours.take(24).collect(), days: days.collect(), fetched })
}

fn kept_at() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let cache = std::env::var_os("XDG_CACHE_HOME").map_or_else(|| home.join(".cache"), PathBuf::from);
    Some(cache.join("caelestia/weather.json"))
}

/// The last forecast fetched, however old, so that there is something to
/// show at once and the network is only gone to for something newer.
pub fn kept() -> Option<Weather> {
    serde_json::from_str(&std::fs::read_to_string(kept_at()?).ok()?).ok()
}

/// Fetches a forecast and keeps it. Slow, and may fail: a machine is not
/// always on a network.
pub fn fetch(configured: &str) -> Option<Weather> {
    let place = place(configured)?;
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&hourly=weather_code,temperature_2m,precipitation_probability\
         &daily=weather_code,temperature_2m_max,temperature_2m_min,sunrise,sunset\
         &current=temperature_2m,relative_humidity_2m,apparent_temperature,is_day,weather_code,wind_speed_10m&timezone=auto&forecast_days=7",
        place.latitude, place.longitude
    );
    let fetched = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |since| since.as_secs());
    let weather = parse(&web::json(&url, None)?, place, fetched)?;

    if let (Some(path), Ok(text)) = (kept_at(), serde_json::to_string(&weather)) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, text);
    }
    Some(weather)
}

/// What a weather code is called. WMO's codes, as Open-Meteo gives them.
pub fn condition(code: i64) -> &'static str {
    match code {
        0 | 1 => "Clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Fog",
        51 | 53 | 55 => "Drizzle",
        56 | 57 => "Freezing drizzle",
        61 | 66 | 80 => "Light rain",
        63 | 81 => "Rain",
        65 | 67 | 82 => "Heavy rain",
        71 => "Light snow",
        73 | 77 => "Snow",
        75 => "Heavy snow",
        85 => "Light snow showers",
        86 => "Heavy snow showers",
        95 => "Thunderstorm",
        96 | 99 => "Thunderstorm with hail",
        _ => "Unknown",
    }
}

/// The symbol for a weather code, by its name in Material Symbols. A clear
/// sky at night is a moon.
pub fn symbol(code: i64, daylight: bool) -> &'static str {
    match (code, daylight) {
        (0 | 1, true) => "clear_day",
        (0 | 1, false) => "clear_night",
        (2, true) => "partly_cloudy_day",
        (2, false) => "partly_cloudy_night",
        (3, _) => "cloud",
        (45 | 48, _) => "foggy",
        (51..=67 | 80..=82, _) => "rainy",
        (71 | 73 | 77 | 85, _) => "cloudy_snowing",
        (75 | 86, _) => "snowing_heavy",
        (95..=99, _) => "thunderstorm",
        _ => "air",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANSWER: &str = r#"{
        "current": {"time": "2026-09-20T14:15", "temperature_2m": 16.4, "relative_humidity_2m": 71, "apparent_temperature": 15.1,
                    "is_day": 1, "weather_code": 61, "wind_speed_10m": 18.2},
        "hourly": {"time": ["2026-09-20T12:00", "2026-09-20T13:00", "2026-09-20T14:00", "2026-09-20T15:00"],
                   "weather_code": [3, 3, 61, 63], "temperature_2m": [15.0, 15.8, 16.4, 16.1], "precipitation_probability": [10, 20, 65, 80]},
        "daily": {"time": ["2026-09-20", "2026-09-21"], "weather_code": [63, 2], "temperature_2m_max": [17.2, 19.0],
                  "temperature_2m_min": [11.3, 10.1], "sunrise": ["2026-09-20T06:52", "2026-09-21T06:54"], "sunset": ["2026-09-20T19:14", "2026-09-21T19:11"]}
    }"#;

    #[test]
    fn an_answer_is_read_as_now_the_hours_to_come_and_the_days() {
        let weather = parse(&serde_json::from_str(ANSWER).unwrap(), Place::default(), 0).unwrap();
        assert_eq!((weather.now.code, weather.now.celsius, weather.now.humidity), (61, 16.4, 71));
        assert_eq!((weather.now.sunrise.as_str(), weather.now.sunset.as_str()), ("06:52", "19:14"));
        assert!(weather.now.daylight);

        let hours: Vec<&str> = weather.hours.iter().map(|hour| hour.at.as_str()).collect();
        assert_eq!(hours, ["2026-09-20T14:00", "2026-09-20T15:00"], "the hours that have been are not a forecast");
        assert_eq!(weather.hours[1].wet, 80);
        assert_eq!(weather.days.len(), 2);
        assert_eq!((weather.days[1].high, weather.days[1].low), (19.0, 10.1));
    }

    #[test]
    fn an_answer_with_no_weather_in_it_is_no_forecast() {
        assert!(parse(&serde_json::json!({"error": true, "reason": "Latitude must be in range"}), Place::default(), 0).is_none());
    }

    #[test]
    fn a_place_given_as_coordinates_is_told_from_one_given_by_name() {
        assert_eq!(coordinates("55.68, 12.57"), Some((55.68, 12.57)));
        assert_eq!(coordinates("Copenhagen"), None);
        assert_eq!(coordinates("Frederiksberg, Denmark"), None);
    }

    #[test]
    fn every_code_has_a_name_and_a_symbol() {
        for code in [0, 1, 2, 3, 45, 48, 51, 53, 55, 56, 57, 61, 63, 65, 66, 67, 71, 73, 75, 77, 80, 81, 82, 85, 86, 95, 96, 99] {
            assert_ne!(condition(code), "Unknown", "{code} has no name");
            assert_ne!(symbol(code, true), "air", "{code} has no symbol");
        }
        assert_eq!(symbol(0, false), "clear_night");
    }

    #[test]
    fn a_forecast_goes_stale() {
        let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();
        assert!(Weather { fetched: now - 60, ..Weather::default() }.is_fresh());
        assert!(!Weather { fetched: now - 3 * 3600, ..Weather::default() }.is_fresh());
    }
}

/// Not a test of anything but the network and three services being there:
/// run by hand, with `--ignored --nocapture`, to see what they say today.
#[cfg(test)]
mod probe {
    #[test]
    #[ignore = "diagnostic: asks the real services where this machine is and what the sky is doing"]
    fn fetch() {
        let weather = super::fetch("").expect("no forecast: is the network there?");
        println!("{} ({:.2}, {:.2})", weather.place.city, weather.place.latitude, weather.place.longitude);
        println!("now: {} {:.1}°C, feels {:.1}°C, wind {:.0} km/h", super::condition(weather.now.code), weather.now.celsius, weather.now.feels_like, weather.now.wind);
        println!("{} hours, {} days; sun {} to {}", weather.hours.len(), weather.days.len(), weather.now.sunrise, weather.now.sunset);
    }
}
