//! The right-hand side of the bar: dials, status glyphs, the clock, the
//! session button. Each is a function from what the feeds say to a pill.

use cae_core::volume::Volume;
use cae_core::{guards, hypr, logo, notifs, services, system};
use gpui::{
    App, FontWeight, IntoElement, ScrollWheelEvent, Styled, Window, div, prelude::*, px, rgba,
};

use super::pieces::{Tone, badge, pill, section, slot};
use crate::ui::glyph::glyph;
use crate::ui::dial::{Dial, dial};
use crate::ui::popout::{Hover, Kind};
use crate::ui::rsx;
use crate::{actions, theme};

fn stat(name: &'static str, percent: f64, kind: Kind, hover: &Hover) -> impl IntoElement + use<> {
    let dial = rsx! {
        <div base={slot(Tone::Plain)} id={name} class="gap-[7px]">
            {dial(percent, Dial::ring(px(18.), px(2.)))}
            <div class="font-semibold" text_size={px(11.)} text_color={theme::text()}>{name}</div>
            // Wide enough for "100%" and never narrower: a reading that grows
            // a digit must not push everything to its right along the bar.
            <div
                class="min-w-[34px]"
                text_size={px(11.5)}
                font_features={theme::tabular()}
                text_color={theme::text_faint()}
            >
                {format!("{}%", percent.round() as i64)}
            </div>
        </div>
    };
    hover.opens(kind, dial)
}

/// One section, up to three dials. Grouping them says they are the same kind
/// of thing, which is what stops this end of the bar reading as a row of
/// unrelated glyphs.
pub fn stats(snapshot: &system::Snapshot, metrics: &logo::Stats, hover: &Hover) -> impl IntoElement {
    rsx! {
        <div base={section()}>
            {...metrics.cpu.then(|| stat("CPU", snapshot.cpu, Kind::Cpu, hover))}
            {...metrics.ram.then(|| stat("RAM", snapshot.memory, Kind::Memory, hover))}
            // Only where the driver reports it: a dial stuck at zero is worse
            // than no dial.
            {...snapshot.gpu.filter(|_| metrics.gpu).map(|gpu| stat("GPU", gpu, Kind::Gpu, hover))}
        </div>
    }
}

fn network_glyph(network: &system::Network) -> &'static str {
    match network.kind.as_str() {
        "ethernet" => "lan",
        "wifi" if network.strength > 70 => "network_wifi",
        "wifi" if network.strength > 45 => "network_wifi_3_bar",
        "wifi" if network.strength > 20 => "network_wifi_2_bar",
        "wifi" => "network_wifi_1_bar",
        _ => "wifi_off",
    }
}

/// The battery slot. With no battery it shows the power profile instead,
/// which is what the shell's own bar did on a desktop.
fn battery(battery: Option<&system::Battery>, profile: &str) -> (&'static str, Tone) {
    let Some(battery) = battery else {
        let glyph = match profile {
            "power-saver" => "energy_savings_leaf",
            "performance" => "rocket_launch",
            _ => "balance",
        };
        return (glyph, Tone::Plain);
    };
    if battery.charging {
        return ("battery_charging_full", Tone::Plain);
    }
    // Plugged in but not taking charge: the level still matters, but the plug
    // is the thing to say, because nothing is draining. And a low battery on
    // mains is not an emergency.
    if battery.on_mains {
        return ("power", Tone::Plain);
    }
    let glyph = match battery.level {
        level if level > 90 => "battery_full",
        level if level > 60 => "battery_5_bar",
        level if level > 40 => "battery_3_bar",
        level if level > 20 => "battery_2_bar",
        _ => "battery_alert",
    };
    // Flat, not charging, and nobody has noticed: the one state here that
    // has earned a colour.
    let tone = match battery.level {
        level if level <= 10 => Tone::Alert,
        level if level <= 25 => Tone::Warn,
        _ => Tone::Plain,
    };
    (glyph, tone)
}

/// The bell. It counts what has arrived since the notification centre was
/// last open, not how long the history is.
fn bell(feed: &notifs::Feed, output: &str) -> impl IntoElement {
    let summary = notifs::Summary::of(feed);
    let open = !summary.centre.is_empty() && summary.centre == output;
    let tone = if open {
        Tone::Lit
    } else if summary.dnd {
        Tone::Dim
    } else {
        Tone::Plain
    };
    let count = if summary.unseen > 99 { "99+".to_string() } else { summary.unseen.to_string() };

    let screen = output.to_string();
    rsx! {
        <div base={slot(tone)} id="bell" onClick={move |_, _, cx| actions::toggle_centre(cx, screen.clone())}>
            {glyph(if summary.dnd { "notifications_off" } else { "notifications" }, px(17.))}
            {...(summary.unseen > 0 && !open).then(|| badge(count))}
        </div>
    }
}

fn microphone(microphone: &Volume, hover: &Hover) -> impl IntoElement + use<> {
    let slot = rsx! {
        <div
            base={slot(if microphone.muted { Tone::Lit } else { Tone::Plain })}
            id="microphone"
            onClick={|_, _, cx| actions::mute_microphone(cx)}
        >
            {glyph(if microphone.muted { "mic_off" } else { "mic" }, px(17.))}
        </div>
    };
    hover.opens(Kind::Microphone, slot)
}

fn volume(volume: &Volume, hover: &Hover) -> impl IntoElement + use<> {
    let name = match volume.level {
        _ if volume.muted => "volume_off",
        level if level > 55 => "volume_up",
        level if level > 0 => "volume_down",
        _ => "volume_mute",
    };
    let reading = if volume.muted { "—".to_string() } else { format!("{}%", volume.level) };

    let slot = rsx! {
        <div
            base={slot(Tone::Plain)}
            id="volume"
            onClick={|_, _, cx| actions::mute(cx)}
            onScrollWheel={|event: &ScrollWheelEvent, window: &mut Window, cx: &mut App| {
                actions::volume(cx, event.delta.pixel_delta(window.line_height()).y > px(0.));
                cx.stop_propagation();
            }}
        >
            {glyph(name, px(17.))}
            <div class="min-w-[34px]" text_size={px(11.5)} font_features={theme::tabular()}>{reading}</div>
        </div>
    };
    hover.opens(Kind::Volume, slot)
}

/// The readouts, in the order the shell's own row has them.
pub fn status(
    snapshot: &system::Snapshot,
    services: &services::Snapshot,
    feed: &notifs::Feed,
    show: &logo::Status,
    output: &str,
    hover: &Hover,
) -> impl IntoElement {
    let bluetooth_glyph = match (&services.bluetooth.powered, services.bluetooth.connected) {
        (false, _) => "bluetooth_disabled",
        (true, connected) if connected > 0 => "bluetooth_connected",
        (true, _) => "bluetooth",
    };
    let bluetooth_tone = if services.bluetooth.powered { Tone::Plain } else { Tone::Dim };
    let (battery_glyph, battery_tone) = battery(snapshot.battery.as_ref(), &services.power.profile);

    rsx! {
        <div base={section()}>
            {bell(feed, output)}
            {...snapshot.microphone.as_ref().filter(|_| show.show_microphone).map(|level| microphone(level, hover))}
            {...snapshot.volume.as_ref().filter(|_| show.show_audio).map(|level| volume(level, hover))}
            {...show.show_network.then(|| hover.opens(Kind::Network, rsx! {
                <div base={slot(Tone::Plain)} id="network">{glyph(network_glyph(&snapshot.network), px(17.))}</div>
            }))}
            {...show.show_bluetooth.then(|| hover.opens(Kind::Bluetooth, rsx! {
                <div base={slot(bluetooth_tone)} id="bluetooth">{glyph(bluetooth_glyph, px(17.))}</div>
            }))}
            {...show.show_battery.then(|| hover.opens(Kind::Battery, rsx! {
                <div base={slot(battery_tone)} id="battery">{glyph(battery_glyph, px(17.))}</div>
            }))}
        </div>
    }
}

/// The lock keys, which are not there at all until one is on. Not there,
/// rather than there and empty: an empty box in a row still has a gap on
/// each side of it, and the row would sit six pixels wider for nothing.
pub fn locks(keyboard: &hypr::Keyboard, show: &logo::Status) -> Option<impl IntoElement + use<>> {
    (show.show_lock_status && (keyboard.caps_lock || keyboard.num_lock)).then(|| {
        rsx! {
            <div class="flex flex-none items-center" text_color={theme::text_dim()}>
                {...keyboard.caps_lock.then(|| glyph("keyboard_capslock_badge", px(16.)).px(px(3.)))}
                {...keyboard.num_lock.then(|| glyph("looks_one", px(16.)).px(px(3.)))}
            </div>
        }
    })
}

/// The keyboard layout, when the config asks for it and there is one to name.
pub fn layout(keyboard: &hypr::Keyboard, show: &logo::Status) -> Option<impl IntoElement + use<>> {
    (show.show_kb_layout && !keyboard.layout.is_empty())
        .then(|| rsx! { <div base={pill(Tone::Plain)} class="px-[8px]">{keyboard.layout.to_uppercase()}</div> })
}

/// The shield: neither guard up is a struck one, up with nothing waiting is a
/// calm one, and something waiting is the one that asks to be looked at.
pub fn shield(guards: &guards::Guards, hover: &Hover) -> impl IntoElement {
    let (name, tone) = match (guards.connected, guards.pending > 0) {
        (false, _) => ("gpp_bad", Tone::Dim),
        (true, true) => ("gpp_maybe", Tone::Lit),
        (true, false) => ("gpp_good", Tone::Plain),
    };
    let waiting = guards.pending > 0;
    let shield = rsx! {
        <div base={slot(tone)} id="shield" onClick={move |_, _, cx| actions::security(cx, waiting)}>
            {glyph(name, px(17.))}
            {...(guards.pending > 0).then(|| badge(guards.pending.to_string()))}
        </div>
    };
    hover.opens(Kind::Guards, shield)
}

/// The wrench, lit while any mode it reaches is on.
pub fn wrench(features: &[services::Feature], hover: &Hover) -> impl IntoElement {
    let active = features.iter().filter(|feature| feature.enabled).count();
    let wrench = rsx! {
        <div
            base={slot(if active > 0 { Tone::Lit } else { Tone::Plain })}
            id="wrench"
            onClick={|_, _, cx| actions::features_menu(cx)}
        >
            {glyph("build", px(17.))}
            {...(active > 0).then(|| badge(active.to_string()))}
        </div>
    };
    hover.opens(Kind::Features, wrench)
}

/// The date and the time, which a hairline makes read as one thing said twice
/// rather than two readouts crowding each other.
///
/// The figures are asked for on the two texts and not on the section: a style
/// set there is inherited by the calendar glyph too, and a symbol font asked
/// for a second set of features is a second font loaded.
pub fn clock(date: &str, time: &str, hover: &Hover) -> impl IntoElement {
    let clock = rsx! {
        <div base={section()} id="clock" class="gap-[8px] px-[11px]" text_size={px(12.5)} text_color={theme::text()}>
            {glyph("calendar_month", px(16.)).text_color(theme::accent())}
            <div text_size={px(12.)} font_features={theme::tabular()} text_color={theme::text_dim()}>
                {date.to_string()}
            </div>
            <div class="w-[1px] h-[12px]" bg={theme::white(0.14)} />
            <div class="font-medium" font_features={theme::tabular()}>{time.to_string()}</div>
        </div>
    };
    hover.opens(Kind::Clock, clock)
}

/// The one control on the bar that ends the session, so it is the one thing
/// drawn in the alert colour and given no popout to linger over.
pub fn power() -> impl IntoElement {
    rsx! {
        <div
            base={pill(Tone::Alert)}
            id="power"
            class="px-[7px]"
            hover={|style| style.bg(rgba(0xff8a8024))}
            onClick={|_, _, cx| actions::session(cx)}
        >
            {glyph("power_settings_new", px(17.))}
        </div>
    }
}
