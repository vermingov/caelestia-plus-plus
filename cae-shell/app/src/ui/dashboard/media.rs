//! What is playing: its cover, where in it the player is, and the words to
//! it as they are sung.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use cae_core::{config, lyrics, media, web};
use gpui::{
    AnyElement, AppContext, Context, Entity, FontWeight, IntoElement, ObjectFit, Render, RetainAllImageCache, ScrollHandle, SharedString, Window,
    div, img, point, prelude::*, px,
};

use super::pane::Reach;
use crate::ease::{Curve, Tween};
use crate::theme;
use crate::ui::glyph::glyph;
use crate::ui::rsx;
use crate::ui::slider::{Slide, slider};

/// How tall a line of the words is, which is what lets the one being sung be
/// kept in the middle by arithmetic.
const LINE: f32 = 34.;
const FOLLOW: Duration = Duration::from_millis(420);

/// Where a player said it was, and when it said so. Between sayings the
/// position is that, plus however long it has been playing since.
#[derive(Clone, Copy)]
struct Heard {
    position: i64,
    at: Instant,
}

enum Words {
    Looking,
    Found(Vec<lyrics::Line>),
    None,
}

pub struct Media {
    players: Vec<media::NowPlaying>,
    /// The player being shown, by its name on the bus. Whichever is playing,
    /// until somebody chooses.
    chosen: String,
    heard: Option<Heard>,
    /// The track the cover and the words are of: they are looked for again
    /// only when this changes.
    track: (String, String),
    cover: Option<PathBuf>,
    words: Words,
    sung: Option<usize>,
    scrolled: Tween,
    list: ScrollHandle,
    seek: Slide,
    pictures: Entity<RetainAllImageCache>,
}

fn minutes(microseconds: i64) -> String {
    let seconds = (microseconds / 1_000_000).max(0);
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// A cover as a file: a player's own file as it is, one on the web fetched
/// once into the cache.
fn cover_of(art: &str) -> Option<PathBuf> {
    if let Some(path) = art.strip_prefix("file://") {
        return Some(PathBuf::from(path)).filter(|path| path.is_file());
    }
    if !art.starts_with("http") {
        return None;
    }
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let cache = std::env::var_os("XDG_CACHE_HOME").map_or_else(|| home.join(".cache"), PathBuf::from).join("caelestia/covers");
    let name: u64 = art.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3));
    let kept = cache.join(format!("{name:016x}"));
    if kept.is_file() {
        return Some(kept);
    }
    std::fs::create_dir_all(&cache).ok()?;
    web::download(art, &kept).then_some(kept)
}

impl Media {
    pub fn new(reach: &Reach, cx: &mut Context<Self>) -> Media {
        let shell = config::read(config::File::Shell);
        let every = config::lookup(&shell, "dashboard.mediaUpdateInterval").and_then(serde_json::Value::as_u64).unwrap_or(500);

        // The bar already hears when what is playing changes. The position is
        // asked for, because no player announces it.
        cx.observe(&reach.feeds.media, |page: &mut Media, _, cx| page.look(cx)).detach();
        cx.spawn(async move |page, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(every.clamp(100, 5000))).await;
                if page.update(cx, |page: &mut Media, cx| page.listen(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();

        let mut page = Media {
            players: Vec::new(),
            chosen: String::new(),
            heard: None,
            track: Default::default(),
            cover: None,
            words: Words::None,
            sung: None,
            scrolled: Tween::still(0.),
            list: ScrollHandle::new(),
            seek: Slide::default(),
            pictures: RetainAllImageCache::new(cx),
        };
        page.look(cx);
        page
    }

    fn shown(&self) -> Option<&media::NowPlaying> {
        self.players.iter().find(|player| player.bus == self.chosen).or_else(|| self.players.first())
    }

    /// Reads the players again, and if the track has changed, finds its
    /// cover and its words.
    fn look(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |page, cx| {
            let players = cx.background_spawn(async { media::all() }).await;
            let _ = page.update(cx, |page: &mut Media, cx| {
                page.players = players;
                page.follow_track(cx);
                page.listen(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn follow_track(&mut self, cx: &mut Context<Self>) {
        let Some(now) = self.shown().cloned() else {
            (self.track, self.cover, self.words, self.sung) = (Default::default(), None, Words::None, None);
            return;
        };
        let track = (now.title.clone(), now.artist.clone());
        if track == self.track {
            return;
        }
        (self.track, self.cover, self.words, self.sung, self.heard) = (track.clone(), None, Words::Looking, None, None);
        self.scrolled.jump(0.);

        cx.spawn(async move |page, cx| {
            let wanted = track.clone();
            let found = cx
                .background_spawn(async move {
                    let shell = config::read(config::File::Shell);
                    let said = |path: &str| config::lookup(&shell, path).and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
                    let folder = Some(said("paths.lyricsDir")).filter(|folder| !folder.is_empty()).map_or_else(lyrics::default_folder, PathBuf::from);
                    let asked = lyrics::Track { title: now.title.clone(), artist: now.artist.clone(), album: now.album.clone(), length: now.length / 1_000_000 };
                    (cover_of(&now.art), lyrics::find(&asked, lyrics::From::named(&said("services.lyricsBackend")), &folder))
                })
                .await;
            let _ = page.update(cx, |page: &mut Media, cx| {
                // A slower answer about an earlier song is no answer.
                if page.track == wanted {
                    page.cover = found.0;
                    page.words = found.1.map_or(Words::None, Words::Found);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Asks the player where it is.
    fn listen(&mut self, cx: &mut Context<Self>) {
        let Some(bus) = self.shown().map(|now| now.bus.clone()) else { return };
        cx.spawn(async move |page, cx| {
            let position = cx.background_spawn(async move { media::position(&bus) }).await;
            let _ = page.update(cx, |page: &mut Media, cx| {
                page.heard = position.map(|position| Heard { position, at: Instant::now() });
                cx.notify();
            });
        })
        .detach();
    }

    /// Where the player is now, in microseconds.
    fn position(&self) -> i64 {
        let (Some(heard), Some(now)) = (self.heard, self.shown()) else { return 0 };
        let since = if now.playing { heard.at.elapsed().as_micros() as i64 } else { 0 };
        let position = heard.position + since;
        if now.length > 0 { position.min(now.length) } else { position }
    }

    fn control(&mut self, action: &'static str, cx: &mut Context<Self>) {
        let Some(bus) = self.shown().map(|now| now.bus.clone()) else { return };
        cx.spawn(async move |page, cx| {
            cx.background_spawn(async move { media::control_on(&bus, action) }).await;
            let _ = page.update(cx, |page: &mut Media, cx| page.look(cx));
        })
        .detach();
    }

    fn transport(&self, cx: &mut Context<Self>) -> gpui::Div {
        let now = self.shown();
        let (there, playing) = (now.is_some(), now.is_some_and(|now| now.playing));
        let (back, forward) = (now.is_some_and(|now| now.can_go_previous), now.is_some_and(|now| now.can_go_next));
        let press = |symbol: &'static str, id: &'static str, action: &'static str, live: bool, large: bool, cx: &mut Context<Self>| {
            rsx! {
                <div
                    id={id}
                    class="flex flex-none items-center justify-center rounded-full"
                    size={px(if large { 44. } else { 36. })}
                    text_color={if live { theme::text() } else { theme::text_faint() }}
                    when={(large, |button| button.bg(theme::white(0.09)))}
                    when={(live, |button| button.cursor_pointer().hover(|style| style.bg(theme::white(0.15))))}
                    onClick={cx.listener(move |page, _, _, cx| if live { page.control(action, cx) })}
                >
                    {glyph(symbol, px(if large { 26. } else { 22. }))}
                </div>
            }
        };
        rsx! {
            <div class="flex flex-none items-center justify-center gap-[10px]">
                {press("skip_previous", "previous", "Previous", back, false, cx)}
                {press(if playing { "pause" } else { "play_arrow" }, "play", "PlayPause", there, true, cx)}
                {press("skip_next", "next", "Next", forward, false, cx)}
            </div>
        }
    }

    fn words(&mut self, position: i64) -> AnyElement {
        let lines = match &self.words {
            Words::Found(lines) => lines,
            Words::Looking => return quiet("lyrics", "Looking for the words"),
            Words::None => return quiet("music_off", if self.shown().is_some() { "No words found for this one" } else { "Nothing is playing" }),
        };
        let sung = lyrics::current(lines, position / 1000);
        if sung != self.sung {
            self.sung = sung;
            self.scrolled.go(sung.unwrap_or(0) as f32, FOLLOW, Curve::Settle);
        }
        // The line being sung is kept four lines down: what has been sung is
        // above it, and more of what is to come is below.
        let lead = 4.;
        self.list.set_offset(point(px(0.), px(-(self.scrolled.value() - lead).max(0.) * LINE)));

        rsx! {
            <div id="words" class="flex flex-col flex-1 min-h-[0px] px-[28px] py-[14px] overflow-y-scroll" track_scroll={&self.list}>
                {for (index, line) in lines.iter().enumerate() {
                    <div
                        key={index}
                        class="flex flex-none items-center truncate"
                        h={px(LINE)}
                        text_size={px(if Some(index) == sung { 16. } else { 14. })}
                        font_weight={if Some(index) == sung { FontWeight::SEMIBOLD } else { FontWeight::NORMAL }}
                        text_color={match sung {
                            Some(sung) if index == sung => theme::text(),
                            Some(sung) if index < sung => theme::text_faint(),
                            _ => theme::text_dim(),
                        }}
                    >
                        {SharedString::from(line.text.clone())}
                    </div>
                }}
                // Room under the last line for it to be sung in the same
                // place the others were.
                <div class="flex-none" h={px(LINE * 6.)} />
            </div>
        }
        .into_any_element()
    }
}

fn quiet(symbol: &'static str, says: &'static str) -> AnyElement {
    rsx! {
        <div class="flex flex-col flex-1 items-center justify-center gap-[8px]" text_color={theme::text_faint()}>
            {glyph(symbol, px(26.))}
            <div text_size={px(13.)}>{says}</div>
        </div>
    }
    .into_any_element()
}

impl Render for Media {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = self.shown().cloned();
        let position = self.position();
        // The words move with the song, so while it plays this is drawn
        // again as the screen is: nothing else on the page needs it to be.
        let following = matches!(self.words, Words::Found(_)) && now.as_ref().is_some_and(|now| now.playing);
        if following || !self.scrolled.done() {
            window.request_animation_frame();
        }

        let (title, artist, album, length, can_seek) = match &now {
            Some(now) => (
                if now.title.is_empty() { now.identity.clone() } else { now.title.clone() },
                now.artist.clone(),
                now.album.clone(),
                now.length,
                now.can_seek && now.length > 0 && !now.track.is_empty(),
            ),
            None => ("Nothing is playing".to_string(), String::new(), String::new(), 0, false),
        };
        let by = [artist, album].into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" · ");

        let seek_to = {
            let target = now.as_ref().map(|now| (now.bus.clone(), now.track.clone()));
            move |seconds: i64, cx: &mut gpui::App| {
                let Some((bus, track)) = target.clone() else { return };
                cx.background_spawn(async move { media::set_position(&bus, &track, seconds * 1_000_000) }).detach();
            }
        };
        let shown_at = self.seek.showing(position / 1_000_000) * 1_000_000;

        let others: Vec<AnyElement> = self
            .players
            .iter()
            .enumerate()
            .map(|(index, player)| {
                let (bus, is_shown) = (player.bus.clone(), now.as_ref().is_some_and(|now| now.bus == player.bus));
                rsx! {
                    <div
                        id={("player", index)}
                        class="flex flex-none items-center h-[22px] px-[10px] rounded-full cursor-pointer max-w-[120px]"
                        text_size={px(11.)}
                        bg={theme::white(if is_shown { 0.16 } else { 0.06 })}
                        text_color={if is_shown { theme::text() } else { theme::text_dim() }}
                        onClick={cx.listener(move |page, _, _, cx| {
                            page.chosen = bus.clone();
                            page.follow_track(cx);
                            page.listen(cx);
                            cx.notify();
                        })}
                    >
                        <div class="truncate">{player.identity.clone()}</div>
                    </div>
                }
                .into_any_element()
            })
            .collect();

        rsx! {
            <div class="flex size-full" image_cache={self.pictures.clone()}>
                <div class="flex flex-col flex-none justify-between w-[340px] h-full p-[24px]">
                    <div class="flex items-center gap-[18px]">
                        <div class="flex flex-none items-center justify-center size-[112px]" rounded={px(12.)} bg={theme::white(0.05)} shadow={theme::edge(0.06)} text_color={theme::text_faint()}>
                            {match &self.cover {
                                Some(cover) => rsx! { <img src={cover.clone()} class="flex-none size-full" rounded={px(12.)} object_fit={ObjectFit::Cover} /> }
                                    .into_any_element(),
                                None => glyph("music_note", px(30.)).into_any_element(),
                            }}
                        </div>
                        <div class="flex flex-col flex-1 gap-[5px] min-w-[0px]">
                            <div class="truncate" text_size={px(16.)} font_weight={FontWeight::MEDIUM}>{title}</div>
                            <div class="truncate" text_size={px(12.5)} text_color={theme::text_dim()}>{by}</div>
                        </div>
                    </div>
                    <div class="flex flex-col gap-[6px]">
                        {if can_seek {
                            slider(&self.seek, position / 1_000_000, (0, length / 1_000_000), seek_to).into_any_element()
                        } else {
                            rsx! { <div class="flex-none h-[4px] my-[6px] rounded-full" bg={theme::white(0.07)} /> }.into_any_element()
                        }}
                        <div class="flex justify-between" text_size={px(11.5)} text_color={theme::text_faint()} font_features={theme::tabular()}>
                            <div>{minutes(shown_at)}</div>
                            <div>{if length > 0 { minutes(length) } else { String::new() }}</div>
                        </div>
                    </div>
                    {self.transport(cx)}
                    <div class="flex flex-wrap justify-center gap-[4px] min-h-[22px]">{...(self.players.len() > 1).then_some(others).into_iter().flatten()}</div>
                </div>
                <div class="flex-none w-[1px] h-full" bg={theme::white(0.06)} />
                <div class="flex flex-col flex-1 min-w-[0px] h-full">{self.words(position)}</div>
            </div>
        }
    }
}

#[cfg(test)]
mod tests {
    use super::minutes;

    #[test]
    fn a_position_is_said_as_minutes_and_seconds() {
        assert_eq!(minutes(0), "0:00");
        assert_eq!(minutes(65_000_000), "1:05");
        assert_eq!(minutes(3_599_999_999), "59:59");
        assert_eq!(minutes(-5), "0:00");
    }
}
