//! The words to what is playing, timed to it.
//!
//! Looked for where the QML shell looked, in the order it looked: a `.lrc`
//! beside the person's music, then LRCLIB, then NetEase, unless the settings
//! say to ask one of them only. What is found on the web is kept under the
//! cache directory, because a song is played more than once and its words do
//! not change. All of it is slow and none of it is sure, so it is asked for
//! away from the thread that draws, and a song with no words found is a song
//! with no words.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::web;

/// One line, and how far into the track it is sung, in milliseconds.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub at: i64,
    pub text: String,
}

/// What is being looked for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// In seconds, where the player says.
    pub length: i64,
}

/// Where to look, as `services.lyricsBackend` names them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum From {
    #[default]
    Anywhere,
    Local,
    Lrclib,
    NetEase,
}

impl From {
    pub fn named(name: &str) -> From {
        match name.to_lowercase().as_str() {
            "local" => From::Local,
            "lrclib" => From::Lrclib,
            "netease" => From::NetEase,
            _ => From::Anywhere,
        }
    }
}

/// `[01:23.45]` and `[01:23]` and `[1:23.456]`, as milliseconds. Nothing for
/// a tag that is not a time: `[ar: somebody]` is a fact about the file.
fn stamp(tag: &str) -> Option<i64> {
    let (minutes, seconds) = tag.split_once(':')?;
    let minutes: i64 = minutes.trim().parse().ok()?;
    let seconds: f64 = seconds.trim().parse().ok()?;
    Some(minutes * 60_000 + (seconds * 1000.).round() as i64)
}

/// An LRC file as its lines, in the order they are sung. A line may carry
/// several times, for a chorus written once; one that carries none is not a
/// lyric. Lines with nothing in them are kept: they are the gaps between
/// verses, and what is showing during one is nothing.
pub fn parse(lrc: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    for raw in lrc.lines() {
        let mut rest = raw.trim();
        let mut times = Vec::new();
        while let Some(tagged) = rest.strip_prefix('[') {
            let Some((tag, after)) = tagged.split_once(']') else { break };
            let Some(at) = stamp(tag) else { break };
            times.push(at);
            rest = after.trim_start();
        }
        lines.extend(times.into_iter().map(|at| Line { at, text: rest.trim().to_string() }));
    }
    lines.sort_by_key(|line| line.at);
    lines
}

/// Which line is being sung at `position` milliseconds: the last one that
/// has started.
pub fn current(lines: &[Line], position: i64) -> Option<usize> {
    lines.iter().rposition(|line| line.at <= position)
}

fn contains(haystack: &str, needle: &str) -> bool {
    !needle.is_empty() && haystack.to_lowercase().contains(&needle.to_lowercase())
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

/// A `.lrc` under `dir` whose name has the artist and the title in it.
fn beside_the_music(dir: &Path, track: &Track, depth: usize) -> Option<PathBuf> {
    let mut folders = Vec::new();
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            folders.push(path);
            continue;
        }
        let name = path.file_name()?.to_string_lossy().into_owned();
        let is_lrc = path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("lrc"));
        if is_lrc && contains(&name, &track.title) && (track.artist.is_empty() || contains(&name, &track.artist)) {
            return Some(path);
        }
    }
    // A collection is folders of folders, and not endlessly so.
    (depth > 0).then(|| folders.iter().find_map(|folder| beside_the_music(folder, track, depth - 1))).flatten()
}

fn local(track: &Track, dir: &Path) -> Option<Vec<Line>> {
    let file = beside_the_music(dir, track, 4)?;
    Some(parse(&std::fs::read_to_string(file).ok()?)).filter(|lines| !lines.is_empty())
}

fn lrclib(track: &Track) -> Option<String> {
    let named = format!("track_name={}&artist_name={}", web::encoded(&track.title), web::encoded(&track.artist));
    let mut exactly = format!("https://lrclib.net/api/get?{named}");
    if !track.album.is_empty() {
        exactly += &format!("&album_name={}", web::encoded(&track.album));
    }
    if track.length > 0 {
        exactly += &format!("&duration={}", track.length);
    }
    let synced = |song: &Value| song.get("syncedLyrics")?.as_str().filter(|lrc| !lrc.is_empty()).map(str::to_string);

    web::json(&exactly, None).as_ref().and_then(synced).or_else(|| {
        // Asked for exactly, a song that is there under a slightly different
        // album or length is not found. Asked for loosely, it is.
        let found = web::json(&format!("https://lrclib.net/api/search?{named}"), None)?;
        found.as_array()?.iter().find_map(synced)
    })
}

fn netease(track: &Track) -> Option<String> {
    // It answers nothing that does not look as if it came from its own pages.
    const SITE: &str = "https://music.163.com/";
    let query = web::encoded(&format!("{} {}", track.title, track.artist));
    let found = web::json(&format!("https://music.163.com/api/search/get?s={query}&type=1&limit=5"), Some(SITE))?;
    let songs = found.get("result")?.get("songs")?.as_array()?;
    // The first whose artist is the one playing: the search is by words, and
    // a cover of the same song is the same words.
    let song = songs.iter().find(|song| {
        let artist = song.get("artists").and_then(|artists| artists.get(0)).and_then(|artist| artist.get("name")).and_then(Value::as_str);
        artist.is_some_and(|artist| contains(artist, &track.artist) || contains(&track.artist, artist))
    })?;
    let id = song.get("id")?.as_i64()?;
    let lyric = web::json(&format!("https://music.163.com/api/song/lyric?id={id}&lv=1"), Some(SITE))?;
    lyric.get("lrc")?.get("lyric")?.as_str().map(str::to_string)
}

/// FNV-1a, for a cache file's name: the same song must be the same name the
/// next time the shell is built.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3))
}

fn kept_at(track: &Track) -> PathBuf {
    let cache = std::env::var_os("XDG_CACHE_HOME").map_or_else(|| home().join(".cache"), PathBuf::from);
    let key = format!("{}\n{}", track.artist.to_lowercase(), track.title.to_lowercase());
    cache.join(format!("caelestia/lyrics/{:016x}.lrc", fnv(key.as_bytes())))
}

/// The words to `track`, from `from`, or nothing. `music` is where the
/// person keeps `.lrc` files of their own.
pub fn find(track: &Track, from: From, music: &Path) -> Option<Vec<Line>> {
    if track.title.is_empty() {
        return None;
    }
    if matches!(from, From::Anywhere | From::Local)
        && let Some(lines) = local(track, music)
    {
        return Some(lines);
    }
    if from == From::Local {
        return None;
    }

    let kept = kept_at(track);
    if let Ok(lrc) = std::fs::read_to_string(&kept) {
        // An empty file is a song that was looked for and not found, which
        // is worth remembering too: the web is not asked again every play.
        return Some(parse(&lrc)).filter(|lines| !lines.is_empty());
    }
    let lrc = match from {
        From::Lrclib => lrclib(track),
        From::NetEase => netease(track),
        _ => lrclib(track).or_else(|| netease(track)),
    };
    if let Some(parent) = kept.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&kept, lrc.as_deref().unwrap_or_default());
    lrc.map(|lrc| parse(&lrc)).filter(|lines| !lines.is_empty())
}

/// Where `.lrc` files are kept when the config does not say.
pub fn default_folder() -> PathBuf {
    home().join("Music/Lyrics")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_lrc_file_is_read_in_the_order_it_is_sung() {
        let lrc = "[ar: Somebody]\n[ti: A song]\n[00:12.50] First line\n[01:02.00][00:40.10] The chorus\n[00:30] \n[bad] not a lyric\nNeither is this\n";
        let lines = parse(lrc);
        let said: Vec<(i64, &str)> = lines.iter().map(|line| (line.at, line.text.as_str())).collect();
        assert_eq!(said, [(12_500, "First line"), (30_000, ""), (40_100, "The chorus"), (62_000, "The chorus")]);
    }

    #[test]
    fn the_line_being_sung_is_the_last_one_that_has_started() {
        let lines = parse("[00:10.00] one\n[00:20.00] two\n[00:30.00] three\n");
        assert_eq!(current(&lines, 0), None, "nothing is sung before the first line");
        assert_eq!(current(&lines, 10_000), Some(0));
        assert_eq!(current(&lines, 29_999), Some(1));
        assert_eq!(current(&lines, 600_000), Some(2));
    }

    #[test]
    fn a_backend_is_known_by_the_name_the_settings_write() {
        assert_eq!(From::named("LRCLIB"), From::Lrclib);
        assert_eq!(From::named("NetEase"), From::NetEase);
        assert_eq!(From::named("Local"), From::Local);
        assert_eq!(From::named("Auto"), From::Anywhere);
        assert_eq!(From::named(""), From::Anywhere);
    }

    #[test]
    fn a_file_beside_the_music_is_found_by_what_it_is_called() {
        let dir = std::env::temp_dir().join(format!("cae-lyrics-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Albums/First")).unwrap();
        std::fs::write(dir.join("Albums/First/Somebody - A Song.lrc"), "[00:01.00] la\n").unwrap();
        std::fs::write(dir.join("Albums/First/notes.txt"), "A Song by Somebody").unwrap();

        let track = Track { title: "a song".into(), artist: "somebody".into(), ..Track::default() };
        assert_eq!(local(&track, &dir), Some(vec![Line { at: 1000, text: "la".into() }]));
        let other = Track { title: "another".into(), artist: "somebody".into(), ..Track::default() };
        assert_eq!(local(&other, &dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_same_song_is_the_same_cache_file_however_it_is_capitalised() {
        let a = Track { title: "A Song".into(), artist: "Somebody".into(), ..Track::default() };
        let b = Track { title: "a song".into(), artist: "SOMEBODY".into(), album: "Whatever".into(), length: 200 };
        assert_eq!(kept_at(&a), kept_at(&b));
    }
}

/// Run by hand, with `--ignored --nocapture`: whether the two services still
/// answer the way this reads them.
#[cfg(test)]
mod probe {
    use super::*;

    #[test]
    #[ignore = "diagnostic: asks LRCLIB and NetEase for the words to a well-known song"]
    fn a_well_known_song() {
        let track = Track { title: "Bohemian Rhapsody".into(), artist: "Queen".into(), ..Track::default() };
        for (name, found) in [("lrclib", lrclib(&track)), ("netease", netease(&track))] {
            let lines = found.map(|lrc| parse(&lrc)).unwrap_or_default();
            println!("{name}: {} lines; first: {:?}", lines.len(), lines.iter().find(|line| !line.text.is_empty()));
        }
    }
}
