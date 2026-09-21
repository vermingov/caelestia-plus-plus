//! The three cards: keeping the machine awake, recording it, and the
//! switches that would otherwise be a panel each.

use std::path::PathBuf;
use std::time::Duration;

use cae_core::{gamemode, network, recorder, services, volume};
use gpui::{AnyElement, App, AppContext, Context, IntoElement, Render, SharedString, Styled, Window, div, prelude::*, px};

use crate::feeds::Feeds;
use crate::ui::controls::{chip, round, switch as flip};
use crate::ui::glyph::glyph;
use crate::ui::rsx;
use crate::{awake, clock, theme};

/// How often what the card shows is asked for again while it is up: a
/// recording's length, and whether one is still going.
const TICK: Duration = Duration::from_secs(1);

/// The shape every card has: a title, and what it is about under it.
fn card(children: Vec<AnyElement>) -> gpui::Div {
    rsx! {
        <div
            class="flex flex-col flex-none gap-[10px] p-[14px]"
            rounded={px(14.)}
            bg={theme::white(0.045)}
            shadow={theme::edge(0.05)}
        >
            {...children}
        </div>
    }
}

fn heading(text: impl Into<SharedString>) -> gpui::Div {
    rsx! { <div class="flex-none" text_size={px(12.5)} text_color={theme::text_dim()}>{text.into()}</div> }
}

/// The round mark a card leads with, lit while what it stands for is on.
fn mark(symbol: &'static str, lit: bool) -> gpui::Div {
    rsx! {
        <div
            class="flex flex-none items-center justify-center size-[38px] rounded-full"
            bg={if lit { theme::accent() } else { theme::white(0.08) }}
            text_color={if lit { theme::on_accent() } else { theme::text_dim() }}
        >
            {glyph(symbol, px(20.))}
        </div>
    }
}

/// What a card says about itself: what it is, and what it is doing.
fn saying(what: &'static str, doing: impl Into<SharedString>) -> gpui::Div {
    rsx! {
        <div class="flex flex-col flex-1 min-w-[0px] gap-[1px]">
            <div class="flex-none" text_size={px(13.5)}>{what}</div>
            <div class="flex-none" text_size={px(11.5)} text_color={theme::text_dim()}>{doing.into()}</div>
        </div>
    }
}

/// Keeping the machine awake: one switch, and since when.
pub struct KeepAwake;

impl KeepAwake {
    pub fn new(_: &mut Context<Self>) -> KeepAwake {
        KeepAwake
    }
}

impl Render for KeepAwake {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let since = awake::kept(cx);
        let on = since.is_some();
        let doing = match since {
            Some(since) => format!("Since {}", clock::at(since)),
            None => "Sleeping as usual".to_string(),
        };
        card(vec![
            rsx! {
                <div
                    id="keep-awake"
                    class="flex flex-none items-center gap-[12px] cursor-pointer"
                    onClick={cx.listener(move |_, _, _, cx| {
                        awake::keep(!on, cx);
                        cx.notify();
                    })}
                >
                    {mark("coffee", on)}
                    {saying("Keep awake", doing)}
                    {flip(on)}
                </div>
            }
            .into_any_element(),
        ])
    }
}

/// Recording the screen: what it is doing, how to start it, and what has
/// been made.
pub struct Recorder {
    now: recorder::Recording,
    made: Vec<recorder::Made>,
    /// What the next recording will be of.
    region: bool,
    sound: bool,
    listing: bool,
    /// The one whose bin has been pressed once, which asks again before it
    /// goes.
    asking: Option<PathBuf>,
}

impl Recorder {
    pub fn new(cx: &mut Context<Self>) -> Recorder {
        cx.spawn(async move |card, cx| {
            loop {
                let (now, made) = cx.background_spawn(async { (recorder::now(), recorder::made()) }).await;
                let landed = card.update(cx, |card: &mut Recorder, cx| {
                    if (&card.now, &card.made) != (&now, &made) {
                        (card.now, card.made) = (now, made);
                        cx.notify();
                    }
                });
                if landed.is_err() {
                    return;
                }
                cx.background_executor().timer(TICK).await;
            }
        })
        .detach();
        Recorder { now: recorder::now(), made: Vec::new(), region: false, sound: false, listing: false, asking: None }
    }

    fn start(&self, cx: &mut App) {
        let how: Vec<&str> = match (self.region, self.sound) {
            (true, true) => vec!["-sr"],
            (true, false) => vec!["-r"],
            (false, true) => vec!["-s"],
            (false, false) => Vec::new(),
        };
        cx.background_spawn(async move { recorder::start(&how) }).detach();
    }

    /// One recording in the list: when it was made, and what can be done
    /// with it.
    fn row(&self, at: usize, made: &recorder::Made, cx: &mut Context<Self>) -> AnyElement {
        let path = made.path.clone();
        let asking = self.asking.as_deref() == Some(path.as_path());
        let playing = path.clone();
        let (bin, forget) = (path.clone(), path.clone());
        rsx! {
            <div class="flex flex-none items-center gap-[8px] h-[30px]">
                <div class="flex-1 min-w-[0px] truncate" text_size={px(12.)} text_color={theme::text_dim()}>
                    {clock::when_recorded(&made.name)}
                </div>
                <div
                    base={round("play_arrow", false)}
                    id={("play", at)}
                    onClick={move |_, _, cx: &mut App| {
                        let playing = playing.clone();
                        cx.background_spawn(async move { recorder::play(&playing) }).detach();
                    }}
                />
                <div
                    base={round(if asking { "delete_forever" } else { "delete" }, asking)}
                    id={("delete", at)}
                    onClick={cx.listener(move |card: &mut Recorder, _, _, cx| {
                        if card.asking.as_deref() == Some(bin.as_path()) {
                            let going = forget.clone();
                            cx.background_spawn({
                                let going = going.clone();
                                async move { recorder::delete(&going) }
                            })
                            .detach();
                            card.made.retain(|made| made.path != going);
                            card.asking = None;
                        } else {
                            card.asking = Some(bin.clone());
                        }
                        cx.notify();
                    })}
                />
            </div>
        }
        .into_any_element()
    }
}

impl Render for Recorder {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let running = self.now.running;
        let doing = if running { format!("Recording {}", clock::counted(self.now.elapsed)) } else { "Ready".to_string() };
        let head = rsx! {
            <div class="flex flex-none items-center gap-[12px]">
                {mark("screen_record", running)}
                {saying("Screen recorder", doing)}
                {...(!running).then(|| rsx! {
                    <div
                        base={chip("Record", true)}
                        id="record"
                        onClick={cx.listener(|card: &mut Recorder, _, _, cx| card.start(cx))}
                    />
                })}
                {...running.then(|| rsx! {
                    <div class="flex flex-none items-center gap-[6px]">
                        <div
                            base={round("pause", false)}
                            id="pause"
                            onClick={|_, _, cx: &mut App| cx.background_spawn(async { recorder::toggle_pause() }).detach()}
                        />
                        <div
                            base={round("stop", true)}
                            id="stop"
                            onClick={|_, _, cx: &mut App| cx.background_spawn(async { recorder::stop() }).detach()}
                        />
                    </div>
                })}
            </div>
        };

        let (region, sound, listing) = (self.region, self.sound, self.listing);
        let choices = rsx! {
            <div class="flex flex-none items-center gap-[8px]">
                <div
                    base={chip("Region", region)}
                    id="region"
                    onClick={cx.listener(|card: &mut Recorder, _, _, cx| {
                        card.region = !card.region;
                        cx.notify();
                    })}
                />
                <div
                    base={chip("Sound", sound)}
                    id="sound"
                    onClick={cx.listener(|card: &mut Recorder, _, _, cx| {
                        card.sound = !card.sound;
                        cx.notify();
                    })}
                />
                <div class="flex-1" />
                <div
                    class="flex flex-none items-center gap-[4px] cursor-pointer"
                    id="listing"
                    text_size={px(11.5)}
                    text_color={theme::text_dim()}
                    hover={|style| style.text_color(theme::text())}
                    onClick={cx.listener(|card: &mut Recorder, _, _, cx| {
                        card.listing = !card.listing;
                        card.asking = None;
                        cx.notify();
                    })}
                >
                    {format!("{} recording{}", self.made.len(), if self.made.len() == 1 { "" } else { "s" })}
                    {glyph(if listing { "unfold_less" } else { "unfold_more" }, px(15.))}
                </div>
            </div>
        };

        let rows: Vec<AnyElement> = self.made.iter().take(24).enumerate().map(|(at, made)| self.row(at, made, cx)).collect();
        let mut parts = vec![head.into_any_element(), choices.into_any_element()];
        if listing && !rows.is_empty() {
            parts.push(
                rsx! {
                    <div id="recordings" class="flex flex-col flex-none gap-[2px] overflow-y-scroll" max_h={px(132.)}>{...rows}</div>
                }
                .into_any_element(),
            );
        }
        card(parts)
    }
}

/// The switches: the radios, the microphone, the modes.
pub struct Toggles {
    feeds: Feeds,
    wifi: bool,
    playing: bool,
}

impl Toggles {
    pub fn new(feeds: &Feeds, cx: &mut Context<Self>) -> Toggles {
        cx.observe(&feeds.system, |_, _, cx| cx.notify()).detach();
        cx.observe(&feeds.services, |_, _, cx| cx.notify()).detach();
        cx.observe(&feeds.notifs, |_, _, cx| cx.notify()).detach();
        let toggles = Toggles { feeds: feeds.clone(), wifi: false, playing: false };
        toggles.look(cx);
        toggles
    }

    /// The two that no feed carries, asked of the machine itself. A moment
    /// later, because this is also how a switch that was just pressed learns
    /// whether it took.
    fn look(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |card, cx| {
            cx.background_executor().timer(Duration::from_millis(600)).await;
            let (wifi, playing) = cx.background_spawn(async { (network::wifi_enabled(), gamemode::enabled()) }).await;
            let _ = card.update(cx, |card: &mut Toggles, cx| {
                (card.wifi, card.playing) = (wifi, playing);
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for Toggles {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let system = &self.feeds.system.read(cx).value;
        let muted = system.microphone.as_ref().is_some_and(|microphone| microphone.muted);
        let bluetooth = self.feeds.services.read(cx).value.bluetooth.powered;
        let quiet = self.feeds.notifs.read(cx).value.dnd;
        let (wifi, playing) = (self.wifi, self.playing);
        let quietening = self.feeds.server.clone();

        let switches: Vec<AnyElement> = vec![
            switch("wifi", "wifi", wifi, cx.listener(move |card: &mut Toggles, _, _, cx| {
                cx.background_spawn(async move { services::set_wifi(!wifi) }).detach();
                // Shown at once, and asked about again in a moment: a radio
                // takes its time, and may refuse.
                card.wifi = !wifi;
                card.look(cx);
                cx.notify();
            })),
            switch("bluetooth", "bluetooth", bluetooth, move |_, _, cx: &mut App| {
                crate::feeds::act(cx, move || services::set_bluetooth(!bluetooth));
            }),
            switch("mic", if muted { "mic_off" } else { "mic" }, !muted, |_, _, cx: &mut App| {
                cx.background_spawn(async { volume::toggle_microphone() }).detach();
            }),
            switch("game", "sports_esports", playing, cx.listener(move |card: &mut Toggles, _, _, cx| {
                cx.background_spawn(async move { gamemode::set(!playing) }).detach();
                card.playing = !playing;
                card.look(cx);
                cx.notify();
            })),
            switch("quiet", "notifications_off", quiet, move |_, _, _: &mut App| quietening.set_dnd(!quiet)),
            switch("settings", "settings", false, |_, _, cx: &mut App| {
                cx.defer(|cx| crate::ui::settings::open(None, cx));
            }),
        ];

        card(vec![
            heading("Switches").into_any_element(),
            rsx! { <div class="flex flex-none items-center gap-[8px]">{...switches}</div> }.into_any_element(),
        ])
    }
}

/// One switch: a round button that says by its colour whether it is on.
fn switch(
    id: &'static str,
    symbol: &'static str,
    on: bool,
    pressed: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    rsx! {
        <div
            id={id}
            class="flex flex-1 items-center justify-center h-[44px] cursor-pointer"
            rounded={px(12.)}
            bg={if on { theme::accent() } else { theme::white(0.07) }}
            text_color={if on { theme::on_accent() } else { theme::text_dim() }}
            hover={|style| style.text_color(if on { theme::on_accent() } else { theme::text() })}
            onClick={pressed}
        >
            {glyph(symbol, px(20.))}
        </div>
    }
    .into_any_element()
}
