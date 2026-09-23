//! The row of readouts on the pill, laid out from the config's entry list.
//!
//! What the bar is made of and in what order is the person's choice, not an
//! order chosen here: a config that moves the clock moves it in this bar.
//! Anything the config names that there is no component for is skipped rather
//! than guessed at.

use std::time::Duration;

use cae_core::media;
use gpui::{
    AnyElement, App, Context, IntoElement, MouseButton, Render, ScrollWheelEvent, Styled, Window, div, prelude::*,
    px, svg,
};

use super::pieces::{Tone, pill, section, slot};
use super::workspaces::Marker;
use super::{readouts, tray, workspaces};
use crate::feeds::Feeds;
use crate::ui::glyph::glyph;
use crate::ui::popout::{Hover, Kind, Popouts};
use crate::ui::rsx;
use crate::{actions, clock, theme};

pub struct Strip {
    /// Which output this bar is on, for a config that wants each bar to show
    /// only its own screen's workspaces.
    output: String,
    feeds: Feeds,
    marker: Marker,
    icons: tray::Icons,
    hover: Hover,
    date: String,
    time: String,
    /// Whether a fullscreen window has this bar's screen, and the bar with
    /// it. Hyprland goes on asking a hidden bar for frames, so while it is
    /// hidden nothing is drawn: a readout redrawn every second for nobody is
    /// a frame's work a second, for as long as the game lasts. Coming back
    /// into view is itself a change, and draws everything as it is by then.
    covered: bool,
}

impl Strip {
    pub fn new(output: String, feeds: &Feeds, cx: &mut Context<Self>) -> Strip {
        // Everything the strip draws, except the spectrum: that one repaints
        // fifteen times a second and has a view of its own for it.
        cx.observe(&feeds.hypr, |strip: &mut Strip, hypr, cx| {
            strip.covered = hypr.read(cx).value.fullscreen.contains(&strip.output);
            strip.changed(cx);
        })
        .detach();
        cx.observe(&feeds.system, |strip: &mut Strip, _, cx| strip.changed(cx)).detach();
        cx.observe(&feeds.services, |strip: &mut Strip, _, cx| strip.changed(cx)).detach();
        cx.observe(&feeds.tray, |strip: &mut Strip, _, cx| strip.changed(cx)).detach();
        cx.observe(&feeds.media, |strip: &mut Strip, _, cx| strip.changed(cx)).detach();
        cx.observe(&feeds.settings, |strip: &mut Strip, _, cx| strip.changed(cx)).detach();
        cx.observe(&feeds.notifs, |strip: &mut Strip, _, cx| strip.changed(cx)).detach();

        // Aligned to the minute rather than ticking every second: the clock
        // shows minutes, so a second of work per second is fifty-nine wasted.
        cx.spawn(async move |strip, cx| {
            loop {
                let wait = clock::until_next_minute();
                cx.background_executor().timer(wait + Duration::from_millis(20)).await;
                let ticked = strip.update(cx, |strip, cx| {
                    (strip.date, strip.time) = clock::now();
                    strip.changed(cx);
                });
                if ticked.is_err() {
                    break;
                }
            }
        })
        .detach();

        let (date, time) = clock::now();
        let hover = Hover::new(cx.new(|_| Popouts::new(feeds)));
        let covered = feeds.hypr.read(cx).value.fullscreen.contains(&output);
        Strip {
            output,
            feeds: feeds.clone(),
            marker: Marker::default(),
            icons: tray::Icons::default(),
            hover,
            date,
            time,
            covered,
        }
    }

    /// Something the strip shows has changed: drawn, unless nobody can see it.
    fn changed(&mut self, cx: &mut Context<Self>) {
        if !self.covered {
            cx.notify();
        }
    }

    fn entry(&mut self, name: &str, cx: &mut Context<Self>) -> Option<AnyElement> {
        let hypr = &self.feeds.hypr.read(cx).value;
        let settings = &self.feeds.settings.read(cx).value;
        let system = &self.feeds.system.read(cx).value;
        let services = &self.feeds.services.read(cx).value;
        let entries = &settings.layout.entries;

        Some(match name {
            "spacer" => rsx! { <div class="flex-1 min-w-[0px]" /> }.into_any_element(),
            "logo" => logo(&settings.logo)?,
            "workspaces" => {
                // With per-monitor workspaces on, each bar shows only what
                // lives on its own screen; otherwise every bar shows the same
                // row.
                // Only where the compositor says which screen each is on: a
                // name of this shell's own making matches none of them.
                let known = hypr.workspaces.iter().any(|w| w.monitor == self.output);
                let own = settings.workspaces.per_monitor && known;
                let shown: Vec<_> =
                    hypr.workspaces.iter().filter(|w| !own || w.monitor == self.output).collect();
                self.marker.aim(workspaces::focused_slot(&shown, &settings.workspaces));
                workspaces::row(&shown, &settings.workspaces, self.marker).into_any_element()
            }
            "specials" => specials(&hypr.specials)?,
            "activeWindow" => active_window(&hypr.active, &self.hover),
            "media" => now_playing(self.feeds.media.read(cx).value.as_ref(), &self.hover)?,
            "sysStats" => readouts::stats(system, &settings.layout.stats, &self.hover).into_any_element(),
            // The shield carries the wrench with it unless the config gives
            // the wrench a place of its own.
            "firewall" => rsx! {
                <div base={section()} class="gap-[0px]">
                    {readouts::shield(&services.guards, &self.hover)}
                    {...(!entries.iter().any(|e| e == "features"))
                        .then(|| readouts::wrench(&services.features, &self.hover))}
                </div>
            }
            .into_any_element(),
            "features" => rsx! {
                <div base={section()} class="gap-[0px]">{readouts::wrench(&services.features, &self.hover)}</div>
            }
            .into_any_element(),
            "tray" => {
                let items = self.feeds.tray.read(cx).value.clone();
                tray::row(&items, &mut self.icons, &self.hover)?.into_any_element()
            }
            "statusIcons" => rsx! {
                <div class="flex flex-none items-center" gap={theme::GAP}>
                    {...readouts::locks(&hypr.keyboard, &settings.layout.status)}
                    {...readouts::layout(&hypr.keyboard, &settings.layout.status)}
                    {readouts::status(
                        system,
                        services,
                        &self.feeds.notifs.read(cx).value,
                        &settings.layout.status,
                        &self.output,
                        &self.hover,
                    )}
                </div>
            }
            .into_any_element(),
            "clock" => readouts::clock(&self.date, &self.time, &self.hover).into_any_element(),
            "power" => readouts::power().into_any_element(),
            _ => return None,
        })
    }
}

impl Render for Strip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entries = self.feeds.settings.read(cx).value.layout.entries.clone();
        let row: Vec<AnyElement> = entries.iter().filter_map(|name| self.entry(name, cx)).collect();

        rsx! {
            <div
                id="strip"
                class="size-full flex items-center"
                gap={theme::GAP}
                // The right end needs a full pill radius of clearance or the
                // last control sits against the curve. The left starts with
                // the mark, which is round itself and sits happily much
                // closer to the edge.
                pl={px(8.)}
                pr={px(22.)}
                text_color={theme::text()}
                // Scrolling the bar itself: volume over the left half,
                // brightness over the right. Anything with a wheel of its own
                // stops the event before it gets here.
                onScrollWheel={|event: &ScrollWheelEvent, window: &mut Window, cx: &mut App| {
                    let up = event.delta.pixel_delta(window.line_height()).y > px(0.);
                    if event.position.x < window.viewport_size().width / 2. {
                        actions::volume(cx, up);
                    } else {
                        actions::brightness(cx, up);
                    }
                }}
            >
                {...row}
            </div>
        }
    }
}

/// The first thing in the row. It is a button because it is the one thing on
/// the bar everybody tries to click: it opens the launcher.
fn logo(logo: &cae_core::logo::Logo) -> Option<AnyElement> {
    if !logo.show {
        return None;
    }
    let mark = match logo.kind.as_str() {
        "cachyos" => "marks/cachyos.svg",
        _ => "marks/caelestia.svg",
    };
    Some(
        rsx! {
            <div
                id="logo"
                class="flex flex-none items-center justify-center w-[32px] h-[26px]"
                rounded={theme::PILL_RADIUS}
                onClick={|_, _, cx| actions::toggle_launcher(cx)}
            >
                // The mark wears the accent, which is the one place a colour
                // is used for identity rather than for state.
                <svg src={mark} class="size-[19px]" text_color={theme::accent()} />
            </div>
        }
        .into_any_element(),
    )
}

fn special_symbol(name: &str) -> &'static str {
    match name {
        "sysmon" => "monitor_heart",
        "music" => "music_note",
        "communication" => "forum",
        "todo" => "checklist",
        _ => "layers",
    }
}

fn special(index: usize, special: &cae_core::hypr::Special) -> impl IntoElement + use<> {
    let name = special.name.clone();
    rsx! {
        <div
            base={slot(if special.open { Tone::Lit } else { Tone::Plain })}
            id={("special", index)}
            onClick={move |_, _, cx| actions::toggle_special(cx, name.clone())}
        >
            {glyph(special_symbol(&special.name), px(17.))}
        </div>
    }
}

/// Named workspaces, drawn only while something is on one: an empty
/// scratchpad is not a thing to navigate to, and a permanent row of dead
/// pills is how a bar fills up with noise.
fn specials(specials: &[cae_core::hypr::Special]) -> Option<AnyElement> {
    if specials.is_empty() {
        return None;
    }
    Some(
        rsx! {
            <div base={section()} class="gap-[0px] px-[3px]">
                {for (index, workspace) in specials.iter().enumerate() {
                    {special(index, workspace)}
                }}
            </div>
        }
        .into_any_element(),
    )
}

/// The class is the application, the title is what it is doing. Both, in that
/// order, is more useful than either, and an empty desktop says so rather
/// than leaving a hole in the row.
fn active_window(active: &cae_core::hypr::Active, hover: &Hover) -> AnyElement {
    let name = if active.class.is_empty() { "Desktop".to_string() } else { active.class.clone() };
    let detail = (active.title != active.class && !active.title.is_empty()).then(|| active.title.clone());

    let window = rsx! {
        <div
            id="active-window"
            class="flex flex-shrink-1 items-baseline gap-[8px] min-w-[0px] h-[26px] px-[4px]"
            rounded={theme::PILL_RADIUS}
            text_size={px(12.)}
            hover={|style| style.bg(theme::white(0.045))}
        >
            <div class="flex-none" text_color={theme::text()}>{name}</div>
            {...detail.map(|title| rsx! {
                <div
                    class="min-w-[0px] flex-shrink-1 truncate"
                    text_size={px(11.5)}
                    text_color={theme::text_faint()}
                >
                    {title}
                </div>
            })}
        </div>
    };
    hover.opens(Kind::Active, window).into_any_element()
}

/// Title first, artist after: on a bar with one line, the title is the thing
/// being recognised.
fn now_playing(playing: Option<&media::NowPlaying>, hover: &Hover) -> Option<AnyElement> {
    let playing = playing?;
    let label = if playing.title.is_empty() { playing.identity.clone() } else { playing.title.clone() };
    if label.is_empty() {
        return None;
    }
    let pill = rsx! {
        <div
            base={pill(Tone::Plain)}
            id="media"
            class="flex-shrink-1 min-w-[0px] max-w-[260px] gap-[8px]"
            onClick={|_, _, cx| actions::media(cx, "PlayPause")}
            onMouseDown={(MouseButton::Middle, |_, _, cx| actions::media(cx, "Next"))}
            onScrollWheel={|event: &ScrollWheelEvent, window: &mut Window, cx: &mut App| {
                let down = event.delta.pixel_delta(window.line_height()).y < px(0.);
                actions::media(cx, if down { "Next" } else { "Previous" });
                cx.stop_propagation();
            }}
        >
            {glyph(if playing.playing { "graphic_eq" } else { "pause" }, px(16.))}
            <div class="min-w-[0px] truncate">{label}</div>
        </div>
    };
    Some(hover.opens(Kind::Media, pill).into_any_element())
}
