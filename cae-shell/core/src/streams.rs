//! What is playing, a program at a time.
//!
//! The default sink's level is `volume`'s. These are the streams into it:
//! the browser, the game, the call, each with a level of its own. Asked of
//! `pactl` when somebody is looking, and not watched: it is a page in the
//! settings, open for a minute a month.

use serde::Deserialize;

/// One program's sound, on its way to a sink.
#[derive(Clone, Debug, PartialEq)]
pub struct Stream {
    /// PipeWire's number for it, which is what changing it needs.
    pub index: u32,
    /// The program, as it calls itself.
    pub name: String,
    /// What it says it is playing, where that is more than a placeholder.
    pub playing: String,
    /// An icon name, from the few programs that give one.
    pub icon: String,
    /// 0 to 100, and beyond where it has been pushed.
    pub level: i64,
    pub muted: bool,
}

#[derive(Deserialize)]
struct Listed {
    index: u32,
    #[serde(default)]
    mute: bool,
    #[serde(default)]
    volume: std::collections::BTreeMap<String, Channel>,
    #[serde(default)]
    properties: std::collections::BTreeMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct Channel {
    /// "153%", which is the one form of it that needs no arithmetic.
    value_percent: String,
}

/// What a stream calls what it is playing when it has nothing to say.
const PLACEHOLDERS: [&str; 4] = ["Playback", "AudioStream", "audio stream", "playStream"];

fn parse(listing: &str) -> Vec<Stream> {
    let listed: Vec<Listed> = serde_json::from_str(listing).unwrap_or_default();
    let streams = listed.into_iter().map(|stream| {
        let said = |key: &str| stream.properties.get(key).and_then(|value| value.as_str()).unwrap_or_default().to_string();
        // The loudest channel is the level: it is what is heard, and the
        // level that is set is set on all of them.
        let level = stream
            .volume
            .values()
            .filter_map(|channel| channel.value_percent.trim_end_matches('%').trim().parse::<i64>().ok())
            .max()
            .unwrap_or(0);
        let name = [said("application.name"), said("node.name"), said("application.process.binary")]
            .into_iter()
            .find(|name| !name.is_empty())
            .unwrap_or_else(|| "Unknown".to_string());
        let playing = Some(said("media.name")).filter(|playing| !PLACEHOLDERS.contains(&playing.as_str()) && *playing != name);
        Stream {
            index: stream.index,
            name,
            playing: playing.unwrap_or_default(),
            icon: said("application.icon_name"),
            level,
            muted: stream.mute,
        }
    });
    let mut streams: Vec<Stream> = streams.collect();
    streams.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.index.cmp(&b.index)));
    streams
}

pub fn list() -> Vec<Stream> {
    let listing = std::process::Command::new("pactl").args(["-f", "json", "list", "sink-inputs"]).output();
    listing.map(|output| parse(&String::from_utf8_lossy(&output.stdout))).unwrap_or_default()
}

pub fn set_level(index: u32, percent: i64) {
    let level = format!("{}%", percent.max(0));
    let _ = std::process::Command::new("pactl").args(["set-sink-input-volume", &index.to_string(), &level]).status();
}

pub fn set_muted(index: u32, muted: bool) {
    let mute = if muted { "1" } else { "0" };
    let _ = std::process::Command::new("pactl").args(["set-sink-input-mute", &index.to_string(), mute]).status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listing_is_read_for_what_a_person_would_call_each_stream() {
        let listing = r#"[
            {"index": 1521, "mute": true, "volume": {"front-left": {"value": 65536, "value_percent": "100%"}},
             "properties": {"application.name": "Zen", "media.name": "AudioStream"}},
            {"index": 1054, "mute": false,
             "volume": {"front-left": {"value": 99957, "value_percent": "153%"}, "front-right": {"value": 1, "value_percent": "40%"}},
             "properties": {"application.name": "Fluxer Canary", "application.icon_name": "fluxer-canary", "media.name": "A call"}},
            {"index": 7, "volume": {}, "properties": {"application.process.binary": "mpv"}}
        ]"#;
        let streams = parse(listing);

        assert_eq!(streams.iter().map(|stream| stream.name.as_str()).collect::<Vec<_>>(), ["Fluxer Canary", "mpv", "Zen"]);
        assert_eq!(streams[0].level, 153, "the loudest channel is the level");
        assert_eq!(streams[0].playing, "A call");
        assert_eq!(streams[0].icon, "fluxer-canary");
        assert_eq!(streams[2].playing, "", "a placeholder is not a title");
        assert!(streams[2].muted);
    }

    #[test]
    fn a_listing_that_is_not_one_is_nothing_playing() {
        assert!(parse("").is_empty());
        assert!(parse("Connection failure: Connection refused").is_empty());
    }
}
