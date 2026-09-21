//! The sky: what it is doing, what it will do for the rest of the day, and
//! the week after that.

use cae_core::weather::{self, Day, Hour};
use gpui::{AnyElement, Context, FontWeight, IntoElement, Render, Window, div, prelude::*, px, relative};

use super::pane::Reach;
use crate::ui::glyph::glyph;
use crate::ui::rsx;
use crate::{clock, theme};

/// How many of the hours to come there is room for.
const HOURS: usize = 12;

pub struct Weather {
    reach: Reach,
}

impl Weather {
    pub fn new(reach: &Reach, cx: &mut Context<Self>) -> Weather {
        cx.observe(&reach.forecast, |_, _, cx| cx.notify()).detach();
        reach.forecast.update(cx, |forecast, cx| forecast.refresh(cx));
        Weather { reach: reach.clone() }
    }

    fn fact(&self, symbol: &'static str, name: &'static str, value: String) -> gpui::Div {
        rsx! {
            <div class="flex items-center gap-[10px]" text_size={px(12.5)}>
                <div class="flex-none" text_color={theme::text_faint()}>{glyph(symbol, px(16.))}</div>
                <div class="flex-1" text_color={theme::text_dim()}>{name}</div>
                <div font_features={theme::tabular()}>{value}</div>
            </div>
        }
    }

    fn now(&self, weather: &weather::Weather) -> gpui::Div {
        let (now, units) = (&weather.now, self.reach.units);
        // Sunrise and sunset come as `06:52`, which a clock with twelve hours
        // on it says another way.
        let clock_time = |stamp: &str| {
            let (hour, minute) = stamp.split_once(':').unwrap_or((stamp, "00"));
            match (units.twelve_hour, hour.parse::<u32>()) {
                (true, Ok(hour)) => {
                    let (hour, half) = clock::of_twelve(hour);
                    format!("{hour}:{minute} {half}")
                }
                _ => stamp.to_string(),
            }
        };
        rsx! {
            <div class="flex flex-col flex-none justify-between w-[290px] h-full p-[24px]">
                <div class="flex flex-col gap-[4px]">
                    <div class="truncate" text_size={px(17.)} font_weight={FontWeight::MEDIUM}>
                        {if weather.place.city.is_empty() { "Here".to_string() } else { weather.place.city.clone() }}
                    </div>
                    <div text_size={px(12.5)} text_color={theme::text_faint()}>{clock::long_date()}</div>
                </div>
                <div class="flex items-center gap-[16px]">
                    <div class="flex-none" text_color={theme::text_dim()}>{glyph(weather::symbol(now.code, now.daylight), px(32.))}</div>
                    <div class="flex flex-col gap-[2px]">
                        <div text_size={px(40.)} line_height={px(44.)} font_weight={FontWeight::SEMIBOLD} font_features={theme::tabular()}>
                            {units.degrees(now.celsius)}
                        </div>
                        <div text_size={px(13.)} text_color={theme::text_dim()}>{weather::condition(now.code)}</div>
                    </div>
                </div>
                <div class="flex flex-col gap-[9px]">
                    {self.fact("thermostat", "Feels like", units.degrees(now.feels_like))}
                    {self.fact("water_drop", "Humidity", format!("{}%", now.humidity))}
                    {self.fact("air", "Wind", format!("{} km/h", now.wind.round() as i64))}
                    {self.fact("wb_twilight", "Sunrise", clock_time(&now.sunrise))}
                    {self.fact("bedtime", "Sunset", clock_time(&now.sunset))}
                </div>
            </div>
        }
    }

    fn hours(&self, hours: &[Hour], sunrise: &str, sunset: &str) -> gpui::Div {
        let units = self.reach.units;
        let columns: Vec<AnyElement> = hours
            .iter()
            .take(HOURS)
            .map(|hour| {
                // `2026-09-20T14:00`: the hour is what is between T and the colon.
                let of_day = hour.at.split_once('T').map_or("", |(_, time)| time);
                let number: u32 = of_day.get(..2).and_then(|hour| hour.parse().ok()).unwrap_or(0);
                let daylight = of_day >= sunrise && of_day < sunset;
                rsx! {
                    <div class="flex flex-col flex-1 items-center gap-[9px] min-w-[0px]">
                        <div text_size={px(11.)} text_color={theme::text_faint()} font_features={theme::tabular()}>
                            {clock::hour_said(number, units.twelve_hour)}
                        </div>
                        <div text_color={theme::text_dim()}>{glyph(weather::symbol(hour.code, daylight), px(20.))}</div>
                        <div text_size={px(13.)} font_features={theme::tabular()}>{units.degrees(hour.celsius)}</div>
                        // The chance of rain, said only when there is one
                        // worth carrying an umbrella for.
                        <div text_size={px(11.)} text_color={theme::text_faint()} font_features={theme::tabular()}>
                            {if hour.wet >= 20 { format!("{}%", hour.wet) } else { String::new() }}
                        </div>
                    </div>
                }
                .into_any_element()
            })
            .collect();
        rsx! { <div class="flex flex-none items-start px-[18px] pt-[20px] pb-[14px] h-[128px]">{...columns}</div> }
    }

    /// The week, a day a row: each day's range as a stretch of the week's.
    fn days(&self, days: &[Day]) -> gpui::Div {
        let units = self.reach.units;
        let coldest = days.iter().map(|day| day.low).fold(f64::INFINITY, f64::min);
        let warmest = days.iter().map(|day| day.high).fold(f64::NEG_INFINITY, f64::max);
        let span = (warmest - coldest).max(1.);

        let rows: Vec<AnyElement> = days
            .iter()
            .enumerate()
            .map(|(index, day)| {
                let mut parts = day.date.split('-').filter_map(|part| part.parse::<u32>().ok());
                let (year, month, date) = (parts.next().unwrap_or(1970) as i32, parts.next().unwrap_or(1), parts.next().unwrap_or(1));
                let name = if index == 0 { "Today".to_string() } else { clock::weekday_short(clock::weekday_of(year, month, date)) };
                let (from, to) = (((day.low - coldest) / span) as f32, ((day.high - coldest) / span) as f32);
                rsx! {
                    <div class="flex flex-1 items-center gap-[14px] min-h-[0px]" text_size={px(12.5)}>
                        <div class="flex-none w-[46px]" text_color={if index == 0 { theme::text() } else { theme::text_dim() }}>{name}</div>
                        <div class="flex-none" text_color={theme::text_dim()}>{glyph(weather::symbol(day.code, true), px(18.))}</div>
                        <div class="flex-none w-[34px] text-right" text_color={theme::text_faint()} font_features={theme::tabular()}>
                            {units.degrees(day.low)}
                        </div>
                        <div class="relative flex-1 h-[4px] rounded-full" bg={theme::white(0.07)}>
                            <div class="absolute h-full rounded-full" left={relative(from)} w={relative((to - from).max(0.04))} bg={theme::accent()} />
                        </div>
                        <div class="flex-none w-[34px]" font_features={theme::tabular()}>{units.degrees(day.high)}</div>
                    </div>
                }
                .into_any_element()
            })
            .collect();
        rsx! { <div class="flex flex-col flex-1 min-h-[0px] px-[24px] py-[14px]">{...rows}</div> }
    }
}

impl Render for Weather {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(weather) = self.reach.forecast.read(cx).weather.clone() else {
            return rsx! {
                <div class="flex flex-col size-full items-center justify-center gap-[8px]" text_color={theme::text_faint()}>
                    {glyph("cloud_off", px(26.))}
                    <div text_size={px(13.)}>{"No forecast yet. It needs the network"}</div>
                </div>
            };
        };
        rsx! {
            <div class="flex size-full">
                {self.now(&weather)}
                <div class="flex-none w-[1px] h-full" bg={theme::white(0.06)} />
                <div class="flex flex-col flex-1 min-w-[0px] h-full">
                    {self.hours(&weather.hours, &weather.now.sunrise, &weather.now.sunset)}
                    <div class="flex-none h-[1px] w-full" bg={theme::white(0.06)} />
                    {self.days(&weather.days)}
                </div>
            </div>
        }
    }
}
