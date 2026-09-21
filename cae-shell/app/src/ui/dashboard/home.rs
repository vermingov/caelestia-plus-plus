//! The first page: the time and the sky, the month, and the machine and what
//! it is playing at a glance.

use std::path::PathBuf;
use std::time::Duration;

use cae_core::{machine, weather};
use gpui::{AnyElement, AppContext, Context, FontWeight, IntoElement, ObjectFit, Render, ScrollWheelEvent, Window, div, img, prelude::*, px};

use super::pane::Reach;
use crate::ui::dial::{Dial, dial};
use crate::ui::glyph::glyph;
use crate::ui::rsx;
use crate::{actions, clock, theme};

pub struct Home {
    reach: Reach,
    /// The month the calendar is turned to.
    month: (i32, u32),
    disk: f64,
    uptime: Duration,
    face: Option<PathBuf>,
}

fn rule_down() -> gpui::Div {
    rsx! { <div class="flex-none w-[1px] h-full" bg={theme::white(0.06)} /> }
}

fn rule_across() -> gpui::Div {
    rsx! { <div class="flex-none h-[1px] w-full" bg={theme::white(0.06)} /> }
}

/// `3 h 12 min`, to the two largest units there are: nobody wants the
/// seconds of a machine that has been up a fortnight.
fn said(uptime: Duration) -> String {
    let minutes = uptime.as_secs() / 60;
    let (days, hours, minutes) = (minutes / 1440, minutes / 60 % 24, minutes % 60);
    match (days, hours) {
        (0, 0) => format!("{minutes} min"),
        (0, _) => format!("{hours} h {minutes} min"),
        _ => format!("{days} d {hours} h"),
    }
}

impl Home {
    pub fn new(reach: &Reach, cx: &mut Context<Self>) -> Home {
        cx.observe(&reach.feeds.system, |_, _, cx| cx.notify()).detach();
        cx.observe(&reach.feeds.media, |_, _, cx| cx.notify()).detach();
        cx.observe(&reach.forecast, |_, _, cx| cx.notify()).detach();
        reach.forecast.update(cx, |forecast, cx| forecast.refresh(cx));

        // The minute turns while it is open.
        cx.spawn(async move |home, cx| {
            loop {
                cx.background_executor().timer(clock::until_next_minute()).await;
                if home.update(cx, |home: &mut Home, cx| {
                    home.uptime = machine::uptime();
                    cx.notify();
                }).is_err() {
                    break;
                }
            }
        })
        .detach();

        cx.spawn(async move |home, cx| {
            let disk = cx.background_spawn(async { machine::disks().first().map_or(0., machine::Disk::percent) }).await;
            let _ = home.update(cx, |home: &mut Home, cx| {
                home.disk = disk;
                cx.notify();
            });
        })
        .detach();

        let (year, month, _) = clock::today();
        let face = std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".face")).filter(|face| face.is_file());
        Home { reach: reach.clone(), month: (year, month), disk: 0., uptime: machine::uptime(), face }
    }

    fn turn(&mut self, by: i32, cx: &mut Context<Self>) {
        self.month = clock::month_after(self.month.0, self.month.1, by);
        cx.notify();
    }

    fn now(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (hour, minute, half) = clock::hour_and_minute(self.reach.units.twelve_hour);
        let forecast = self.reach.forecast.read(cx).weather.clone();
        let sky = forecast.map(|weather| {
            let now = &weather.now;
            rsx! {
                <div class="flex flex-col gap-[6px]">
                    <div class="flex items-center gap-[12px]">
                        <div text_color={theme::text_dim()}>{glyph(weather::symbol(now.code, now.daylight), px(30.))}</div>
                        <div text_size={px(26.)} font_weight={FontWeight::MEDIUM} font_features={theme::tabular()}>
                            {self.reach.units.degrees(now.celsius)}
                        </div>
                    </div>
                    <div text_size={px(13.)} text_color={theme::text_dim()}>{weather::condition(now.code)}</div>
                    <div class="truncate" text_size={px(12.)} text_color={theme::text_faint()}>{weather.place.city.clone()}</div>
                </div>
            }
        });

        rsx! {
            <div class="flex flex-col flex-none justify-between w-[220px] h-full p-[24px]">
                <div class="flex flex-col gap-[4px]">
                    <div class="flex items-baseline" text_size={px(58.)} line_height={px(62.)} font_weight={FontWeight::SEMIBOLD} font_features={theme::tabular()}>
                        {hour}
                        <div text_color={theme::accent()}>{":"}</div>
                        {minute}
                        {...(!half.is_empty()).then(|| rsx! {
                            <div class="pl-[6px]" text_size={px(16.)} font_weight={FontWeight::MEDIUM} text_color={theme::text_dim()}>{half}</div>
                        })}
                    </div>
                    <div text_size={px(13.)} text_color={theme::text_dim()}>{clock::long_date()}</div>
                </div>
                {...sky}
            </div>
        }
    }

    fn calendar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (year, month) = self.month;
        let today = clock::today();
        let monday_first = clock::week_starts_on_monday();
        let first = clock::weekday_of(year, month, 1);
        let lead = if monday_first { first } else { (first + 1) % 7 };
        let days = clock::days_in(year, month);
        let is_this_month = (year, month) == (today.0, today.1);

        let cells: Vec<AnyElement> = (0..42)
            .map(|cell| {
                let day = cell as i64 - lead as i64 + 1;
                if day < 1 || day > days as i64 {
                    return rsx! { <div class="flex-none size-[34px]" /> }.into_any_element();
                }
                let column = cell % 7;
                let weekend = if monday_first { column >= 5 } else { column == 0 || column == 6 };
                let is_today = is_this_month && day as u32 == today.2;
                rsx! {
                    <div
                        class="flex flex-none items-center justify-center size-[34px] rounded-full"
                        text_size={px(12.5)}
                        font_features={theme::tabular()}
                        text_color={if is_today { theme::on_accent() } else if weekend { theme::text_faint() } else { theme::text_dim() }}
                        when={(is_today, |day| day.bg(theme::accent()).font_weight(FontWeight::SEMIBOLD))}
                    >
                        {day.to_string()}
                    </div>
                }
                .into_any_element()
            })
            .collect();

        let turner = |symbol: &'static str, id: &'static str, by: i32, cx: &mut Context<Self>| {
            rsx! {
                <div
                    id={id}
                    class="flex flex-none items-center justify-center size-[28px] rounded-full cursor-pointer"
                    text_color={theme::text_dim()}
                    hover={|style| style.bg(theme::white(0.08)).text_color(theme::text())}
                    onClick={cx.listener(move |home, _, _, cx| home.turn(by, cx))}
                >
                    {glyph(symbol, px(18.))}
                </div>
            }
        };

        rsx! {
            <div
                id="calendar"
                class="flex flex-col flex-none items-center h-full px-[26px] py-[20px]"
                onScrollWheel={cx.listener(|home, event: &ScrollWheelEvent, _, cx| {
                    let down = f32::from(event.delta.pixel_delta(px(20.)).y) < 0.;
                    home.turn(if down { 1 } else { -1 }, cx);
                })}
            >
                <div class="flex flex-none items-center justify-between w-full pb-[12px]">
                    {turner("chevron_left", "earlier", -1, cx)}
                    <div
                        id="this-month"
                        class="flex-none px-[10px] py-[4px] cursor-pointer"
                        rounded={px(8.)}
                        text_size={px(14.)}
                        font_weight={FontWeight::MEDIUM}
                        hover={|style| style.bg(theme::white(0.06))}
                        onClick={cx.listener(|home, _, _, cx| {
                            let (year, month, _) = clock::today();
                            home.month = (year, month);
                            cx.notify();
                        })}
                    >
                        {format!("{} {year}", clock::month_name(month))}
                    </div>
                    {turner("chevron_right", "later", 1, cx)}
                </div>
                <div class="flex flex-none w-[238px]">
                    {for initial in clock::weekday_initials() {
                        <div class="flex flex-none items-center justify-center w-[34px] h-[24px]" text_size={px(11.)} text_color={theme::text_faint()}>
                            {initial}
                        </div>
                    }}
                </div>
                <div class="flex flex-wrap flex-none w-[238px]">{...cells}</div>
            </div>
        }
    }

    fn somebody(&self) -> impl IntoElement {
        let name = std::env::var("USER").unwrap_or_default();
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        rsx! {
            <div class="flex flex-none items-center gap-[16px] px-[24px] py-[20px]">
                {match &self.face {
                    Some(face) => rsx! { <img src={face.clone()} class="flex-none size-[52px]" rounded={px(26.)} object_fit={ObjectFit::Cover} /> }
                        .into_any_element(),
                    None => rsx! {
                        <div class="flex flex-none items-center justify-center size-[52px] rounded-full" bg={theme::white(0.07)} text_color={theme::text_dim()}>
                            {glyph("person", px(26.))}
                        </div>
                    }
                    .into_any_element(),
                }}
                <div class="flex flex-col flex-1 gap-[3px] min-w-[0px]">
                    <div class="truncate" text_size={px(15.)} font_weight={FontWeight::MEDIUM}>{name}</div>
                    <div class="truncate" text_size={px(12.)} text_color={theme::text_faint()}>
                        {if desktop.is_empty() { format!("Up {}", said(self.uptime)) } else { format!("{desktop}, up {}", said(self.uptime)) }}
                    </div>
                </div>
            </div>
        }
    }

    fn machine(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let system = &self.reach.feeds.system.read(cx).value;
        let reading = |symbol: &'static str, name: &'static str, percent: f64| {
            rsx! {
                <div class="flex flex-col flex-1 items-center gap-[8px]">
                    <div class="relative flex flex-none items-center justify-center size-[64px]">
                        <div class="absolute">{dial(percent, Dial::gauge(px(64.), px(5.)))}</div>
                        <div text_color={theme::text_dim()}>{glyph(symbol, px(20.))}</div>
                    </div>
                    <div class="flex items-baseline gap-[5px]" text_size={px(12.)}>
                        <div text_color={theme::text_dim()}>{name}</div>
                        <div text_color={theme::text_faint()} font_features={theme::tabular()}>{format!("{}%", percent.round() as i64)}</div>
                    </div>
                </div>
            }
        };
        rsx! {
            <div class="flex flex-none items-center px-[18px] py-[18px]">
                {reading("memory", "CPU", system.cpu)}
                {reading("memory_alt", "Memory", system.memory)}
                {reading("hard_drive", "Disk", self.disk)}
            </div>
        }
    }

    fn playing(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let playing = self.reach.feeds.media.read(cx).value.clone();
        let (title, artist, is_playing) = match &playing {
            Some(now) => (if now.title.is_empty() { now.identity.clone() } else { now.title.clone() }, now.artist.clone(), now.playing),
            None => ("Nothing is playing".to_string(), String::new(), false),
        };
        let there = playing.is_some();
        let press = |symbol: &'static str, id: &'static str, action: &'static str| {
            rsx! {
                <div
                    id={id}
                    class="flex flex-none items-center justify-center size-[32px] rounded-full"
                    text_color={if there { theme::text_dim() } else { theme::text_faint() }}
                    when={(there, |button| button.cursor_pointer().hover(|style| style.bg(theme::white(0.09)).text_color(theme::text())))}
                    onClick={move |_, _, cx| if there { actions::media(cx, action) }}
                >
                    {glyph(symbol, px(20.))}
                </div>
            }
        };
        rsx! {
            <div class="flex flex-1 items-center gap-[12px] min-h-[0px] px-[24px]">
                <div class="flex-none" text_color={theme::text_faint()}>{glyph("music_note", px(20.))}</div>
                <div class="flex flex-col flex-1 gap-[2px] min-w-[0px]">
                    <div class="truncate" text_size={px(13.)} text_color={if there { theme::text() } else { theme::text_dim() }}>{title}</div>
                    {...(!artist.is_empty()).then(|| rsx! {
                        <div class="truncate" text_size={px(12.)} text_color={theme::text_faint()}>{artist}</div>
                    })}
                </div>
                <div class="flex flex-none items-center gap-[2px]">
                    {press("skip_previous", "previous", "Previous")}
                    {press(if is_playing { "pause" } else { "play_arrow" }, "play", "PlayPause")}
                    {press("skip_next", "next", "Next")}
                </div>
            </div>
        }
    }
}

impl Render for Home {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        rsx! {
            <div class="flex size-full">
                {self.now(cx)}
                {rule_down()}
                {self.calendar(cx)}
                {rule_down()}
                <div class="flex flex-col flex-1 min-w-[0px] h-full">
                    {self.somebody()}
                    {rule_across()}
                    {self.machine(cx)}
                    {rule_across()}
                    {self.playing(cx)}
                </div>
            </div>
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn how_long_it_has_been_up_is_said_in_the_two_largest_units() {
        assert_eq!(said(Duration::from_secs(45 * 60)), "45 min");
        assert_eq!(said(Duration::from_secs(3 * 3600 + 12 * 60 + 40)), "3 h 12 min");
        assert_eq!(said(Duration::from_secs(16 * 86_400 + 5 * 3600 + 59 * 60)), "16 d 5 h");
    }
}
