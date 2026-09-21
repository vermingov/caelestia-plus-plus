//! The battery's panel, which is the power panel: how much is left, which
//! profile the machine is on, and the modes that change how it behaves. On a
//! desktop there is no battery and the rest is the whole of it.

use cae_core::{services, system};
use gpui::{AppContext, Context, Div, IntoElement, Render, Styled, Window, div, prelude::*, px, rgba};

use super::pieces::{column, detail, headline, meter_in, switch, warning};
use super::simple::feature_name;
use crate::feeds::{self, Feeds};
use crate::theme;
use crate::ui::glyph::glyph;
use crate::ui::rsx;

pub struct Power {
    feeds: Feeds,
    /// Whichever mode the pointer is over, so its description is the one
    /// shown.
    focused: String,
}

impl Power {
    pub fn new(feeds: &Feeds, cx: &mut Context<Self>) -> Power {
        cx.observe(&feeds.system, |_, _, cx| cx.notify()).detach();
        cx.observe(&feeds.services, |_, _, cx| cx.notify()).detach();
        Power { feeds: feeds.clone(), focused: String::new() }
    }

    /// Picking a profile by hand takes it back from the auto-switcher: the
    /// two cannot both be driving.
    fn choose(&mut self, profile: String, cx: &mut Context<Self>) {
        let dynamic = self.feeds.services.read(cx).value.power.dynamic;
        feeds::act(cx, move || {
            if dynamic {
                services::set_dynamic(false);
            }
            services::set_power_profile(&profile);
        });
    }
}

fn profile_name(profile: &str) -> &str {
    match profile {
        "power-saver" => "Power saver",
        "balanced" => "Balanced",
        "performance" => "Performance",
        other => other,
    }
}

fn profile_glyph(profile: &str) -> &'static str {
    match profile {
        "power-saver" => "energy_savings_leaf",
        "performance" => "rocket_launch",
        _ => "balance",
    }
}

/// A mode's glyph, and the sentence that says what it actually does: a switch
/// with no explanation is a switch nobody touches.
fn describe(id: &str) -> (&'static str, &'static str) {
    match id {
        "maxPerf" => ("bolt", "Pins CPU and GPU at their limits, tuned to this machine"),
        "antiHeat" => ("ac_unit", "Runs cooler at full speed: undervolt and early fans, never power caps"),
        "lidStay" => ("laptop", "The lid stops suspending the machine; the screen still turns off"),
        "caffeine" => ("coffee", "Nothing idles, blanks or locks while this is on"),
        "gameMode" => ("sports_esports", "Quietens the desktop and keeps the compositor out of the way"),
        "bedMode" => (
            "bed",
            "Much more sensitive fan curve for restricted airflow, e.g. on a bed. Fans only — your power profile is untouched",
        ),
        _ => ("tune", "Hover a mode to see what it does"),
    }
}

/// The time the battery has left, said the way a person would.
fn remaining(battery: &system::Battery) -> Option<String> {
    let minutes = battery.minutes?;
    let (hours, rest) = (minutes / 60, minutes % 60);
    let spell = if hours > 0 {
        format!("{hours} hr{} {rest} mins", if hours == 1 { "" } else { "s" })
    } else {
        format!("{rest} mins")
    };
    Some(if battery.charging { format!("Time until charged: {spell}") } else { format!("Time remaining: {spell}") })
}

/// Plugged in and not charging is its own state and has to be said out loud.
/// A laptop held at a charge threshold sits there for hours, and "On battery"
/// is simply untrue while the cable is in.
fn state(battery: &system::Battery) -> &'static str {
    match (battery.charging, battery.on_mains) {
        (true, _) => "Charging",
        (false, true) => "Plugged in, not charging",
        (false, false) => "On battery",
    }
}

/// One of the row of circles. The chosen one is the only filled thing in the
/// panel, which is what makes it readable at a glance.
fn dial(symbol: &'static str, on: bool) -> Div {
    rsx! {
        <div
            class="flex flex-1 items-center justify-center h-[34px] rounded-full cursor-pointer"
            bg={if on { theme::accent() } else { theme::white(0.) }}
            text_color={if on { theme::on_accent() } else { theme::text_dim() }}
            when={(!on, |dial| dial.hover(|style| style.bg(theme::white(0.07)).text_color(theme::text())))}
        >
            {glyph(symbol, px(19.))}
        </div>
    }
}

fn mode(symbol: &'static str, name: String, on: bool) -> Div {
    rsx! {
        <div
            class="flex flex-none items-center gap-[11px] h-[38px] px-[10px] cursor-pointer"
            rounded={px(10.)}
            text_size={px(12.5)}
            text_color={theme::text()}
            hover={|style| style.bg(theme::white(0.05))}
        >
            {glyph(symbol, px(18.)).text_color(theme::text_dim())}
            <div class="flex-1 min-w-[0px] truncate">{name}</div>
            {switch(on)}
        </div>
    }
}

impl Render for Power {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let battery = self.feeds.system.read(cx).value.battery.clone();
        let services = self.feeds.services.read(cx).value.clone();
        let power = &services.power;
        let dynamic = power.dynamic;

        let tier = match power.dynamic_tier.as_str() {
            "yield" => "paused — Max performance on",
            "" => "starting…",
            tier => profile_name(tier),
        };

        rsx! {
            <div base={column()}>
                {...battery.as_ref().map(|battery| headline(remaining(battery).unwrap_or(format!("{}%", battery.level))))}
                {...battery.as_ref().map(|battery| {
                    meter_in(battery.level as f64, if battery.charging { rgba(0x9fd6a0ff).into() } else { theme::white(0.45) })
                })}
                {...battery.as_ref().map(|battery| detail(format!("{}% · {}", battery.level, state(battery))))}

                // The machine is not giving what the profile asks for, and
                // saying so is the difference between a slow laptop and a
                // broken one.
                {...(!power.degraded.is_empty())
                    .then(|| warning(format!("Performance degraded — {}", power.degraded.replace('-', " "))))}

                // One choice, so one row of circles rather than a list of
                // rows: the shape says "pick one of these".
                {...(!power.available.is_empty()).then(|| rsx! {
                    <div
                        class="flex flex-none items-center justify-between gap-[6px] p-[4px] rounded-full"
                        bg={theme::white(0.045)}
                        shadow={theme::edge(0.05)}
                    >
                        {for (index, profile) in power.available.iter().enumerate() {
                            <div
                                base={dial(profile_glyph(profile), *profile == power.profile && !dynamic)}
                                id={("profile", index)}
                                onClick={cx.listener({
                                    let profile = profile.clone();
                                    move |power, _, _, cx| power.choose(profile.clone(), cx)
                                })}
                            />
                        }}
                        // The auto-switcher takes the choice over rather than
                        // being one of the choices: it is on top of a
                        // profile, not instead of one.
                        <div
                            base={dial("auto_mode", dynamic)}
                            id="auto"
                            onClick={move |_, _, cx| feeds::act(cx, move || services::set_dynamic(!dynamic))}
                        />
                    </div>
                })}
                {...dynamic.then(|| detail(format!("Auto-switching by load — now: {tier}")))}

                // Wider than the column by the rows' own padding, so that their
                // text lines up with everything above while their highlight
                // runs nearly to the panel's edge.
                <div class="flex flex-col flex-none gap-[2px] mx-[-8px]">
                    {...services.bed_mode.map(|on| rsx! {
                        <div
                            base={mode("bed", "Bed mode".to_string(), on)}
                            id="bed-mode"
                            onHover={cx.listener(|power, hovered: &bool, _, cx| {
                                if *hovered {
                                    power.focused = "bedMode".to_string();
                                    cx.notify();
                                }
                            })}
                            onClick={|_, _, cx| feeds::act(cx, services::toggle_bed_mode)}
                        />
                    })}
                    {for (index, feature) in services.features.iter().enumerate() {
                        <div
                            base={mode(describe(&feature.id).0, feature_name(&feature.id).to_string(), feature.enabled)}
                            id={("mode", index)}
                            onHover={cx.listener({
                                let id = feature.id.clone();
                                move |power, hovered: &bool, _, cx| {
                                    if *hovered {
                                        power.focused = id.clone();
                                        cx.notify();
                                    }
                                }
                            })}
                            onClick={{
                                let id = feature.id.clone();
                                move |_, _, cx| {
                                    let id = id.clone();
                                    feeds::act(cx, move || drop(services::ipc("features", "toggle", &[&id])));
                                }
                            }}
                        />
                    }}
                    // Kept its height whether or not anything is hovered, so
                    // that moving between switches does not resize the panel
                    // under the pointer.
                    <div base={detail(describe(&self.focused).1)} class="min-h-[34px] px-[10px]" line_height={px(16.2)} />
                </div>
            </div>
        }
    }
}
