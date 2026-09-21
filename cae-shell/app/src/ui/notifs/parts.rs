//! What a notification is drawn with, wherever it is drawn: its face, its
//! body, its buttons, and how long ago it was. A toast, a row in the centre
//! and a group's heading are arrangements of these.

use std::path::PathBuf;

use cae_core::notifs::Notification;
use gpui::{
    AnyElement, App, Bounds, Div, FontStyle, FontWeight, HighlightStyle, InteractiveText, IntoElement, ObjectFit,
    PathBuilder, PathStyle, Pixels, SharedString, StrokeOptions, StyledImage, StyledText, Styled, UnderlineStyle,
    Window, canvas, div, img, point, prelude::*, px, rgba, svg,
};
use lyon::tessellation::LineCap;

use super::markup;
use crate::theme;
use crate::ui::glyph::glyph;
use crate::ui::rsx;

pub const LOW: u8 = 0;
pub const CRITICAL: u8 = 2;

/// "now", "4m", "2h", "3d": how long ago something happened, as short as the
/// old shell said it.
pub fn ago(time: i64, now: i64) -> String {
    let minutes = (now - time).max(0) / 60_000;
    match (minutes / 1440, minutes / 60) {
        _ if minutes < 1 => "now".to_string(),
        (days, _) if days > 0 => format!("{days}d"),
        (_, hours) if hours > 0 => format!("{hours}h"),
        _ => format!("{minutes}m"),
    }
}

/// The glyph for a notification that brought no picture of its own, chosen
/// from what it says. The old shell's list, kept word for word: these are
/// the notifications this desktop sends itself, and they were the ones it
/// was written for.
fn fallback_glyph(summary: &str, urgency: u8) -> &'static str {
    const BY_WORD: [(&str, &str); 12] = [
        ("reboot", "restart_alt"),
        ("recording", "screen_record"),
        ("battery", "power"),
        ("screenshot", "screenshot_monitor"),
        ("welcome", "waving_hand"),
        ("time", "schedule"),
        ("a break", "schedule"),
        ("installed", "download"),
        ("update", "update"),
        ("unable to", "deployed_code_alert"),
        ("profile", "person"),
        ("file", "folder_copy"),
    ];
    let said = summary.to_lowercase();
    BY_WORD
        .iter()
        .find(|(word, _)| said.contains(word))
        .map_or(if urgency == CRITICAL { "release_alert" } else { "chat" }, |(_, glyph)| glyph)
}

/// An application's icon at `size`. A symbolic one is drawn in black and
/// meant to be recoloured by whatever shows it; left alone, on a dark pane it
/// vanishes, so the vector ones are drawn in the text's colour instead.
fn icon(path: &str, size: Pixels, otherwise: impl Fn() -> AnyElement + 'static) -> AnyElement {
    if path.contains("symbolic") && path.ends_with(".svg") {
        return rsx! { <svg external_path={SharedString::from(path.to_string())} class="flex-none" size={size} text_color={theme::white(0.85)} /> }
            .into_any_element();
    }
    rsx! { <img src={PathBuf::from(path)} class="flex-none" size={size} object_fit={ObjectFit::Contain} with_fallback={otherwise} /> }
        .into_any_element()
}

/// How far round a notification that reports progress has got, as a ring
/// outside its face.
fn ring(progress: i32, across: Pixels) -> impl IntoElement {
    let turn = (progress.clamp(0, 100) as f32 / 100.).min(0.9999);
    rsx! {
        <canvas
            class="absolute"
            top={px(-3.)}
            left={px(-3.)}
            size={across + px(6.)}
            prepaint={|_, _, _| ()}
            paint={move |bounds: Bounds<Pixels>, _, window: &mut Window, _: &mut App| {
                if turn <= 0.004 {
                    return;
                }
                let (centre, radius) = (bounds.center(), bounds.size.width / 2. - px(1.));
                let at = |turn: f32| {
                    let angle = (turn - 0.25) * std::f32::consts::TAU;
                    point(centre.x + radius * angle.cos(), centre.y + radius * angle.sin())
                };
                let round = StrokeOptions::default().with_line_width(2.).with_line_cap(LineCap::Round);
                let mut path = PathBuilder::stroke(px(2.)).with_style(PathStyle::Stroke(round));
                path.move_to(at(0.));
                // More than half a turn is two arcs: one cannot say which way
                // round it means.
                path.arc_to(point(radius, radius), px(0.), false, true, at(turn / 2.));
                path.arc_to(point(radius, radius), px(0.), false, true, at(turn));
                if let Ok(path) = path.build() {
                    window.paint_path(path, theme::text());
                }
            }}
        />
    }
}

/// A notification's face: the picture it brought, or its sender's icon, or a
/// glyph chosen from what it says. With a picture in the slot the sender's
/// icon rides on its corner, so that it is still clear who is speaking.
pub fn avatar(notif: &Notification, image: &str, app_icon: &str, urgency: u8, small: bool) -> impl IntoElement + use<> {
    let across = px(if small { 28. } else { 38. });
    let (ground, ink) = match urgency {
        CRITICAL => (rgba(0xff544929).into(), theme::alert()),
        LOW => (theme::white(0.045), theme::text_faint()),
        _ => (theme::white(0.08), theme::text_dim()),
    };
    let symbol = fallback_glyph(&notif.summary, urgency);
    let drawn = move || glyph(symbol, px(if small { 15. } else { 19. })).into_any_element();

    // A picture that will not load is treated as one that was never sent, so
    // the slot falls through to the next best thing rather than showing a
    // torn image.
    let face = if !image.is_empty() {
        let (behind, size) = (app_icon.to_string(), across * 0.6);
        rsx! {
            <img
                src={PathBuf::from(image)}
                class="size-full rounded-full"
                object_fit={ObjectFit::Cover}
                with_fallback={move || if behind.is_empty() { drawn() } else { icon(&behind, size, drawn) }}
            />
        }
        .into_any_element()
    } else if !app_icon.is_empty() {
        icon(app_icon, across * 0.6, drawn)
    } else {
        drawn()
    };

    rsx! {
        <div class="relative flex flex-none items-center justify-center rounded-full" size={across} bg={ground} text_color={ink}>
            {face}
            {...(!image.is_empty() && !app_icon.is_empty()).then(|| rsx! {
                <div
                    class="absolute flex items-center justify-center size-[17px] rounded-full"
                    right={px(-3.)}
                    bottom={px(-3.)}
                    bg={rgba(0x1b1b1fff)}
                    shadow={theme::ring(rgba(0x101013f2).into(), px(1.5))}
                >
                    {icon(app_icon, px(11.), || div().into_any_element())}
                </div>
            })}
            {...notif.progress.filter(|_| !small).map(|progress| ring(progress, across))}
        </div>
    }
}

/// One of the sender's own buttons.
pub fn action(label: &str) -> Div {
    let label = if label.trim().is_empty() { "Open" } else { label.trim() };
    rsx! {
        <div
            class="flex flex-1 items-center justify-center min-w-[0px] h-[28px] px-[12px] rounded-full cursor-pointer"
            bg={theme::white(0.07)}
            text_size={px(12.)}
            text_color={theme::text_dim()}
            hover={|style| style.bg(theme::white(0.13)).text_color(theme::text())}
        >
            <div class="min-w-[0px] truncate">{SharedString::from(label.to_string())}</div>
        </div>
    }
}

/// When something happened, in the corner of whatever it happened to.
pub fn time(when: i64, now: i64) -> Div {
    rsx! {
        <div class="flex-none" text_size={px(11.)} font_features={theme::tabular()} text_color={theme::text_faint()}>
            {ago(when, now)}
        </div>
    }
}

/// A body in full: its styling kept, its links pressable, and none of it
/// markup. `id` tells one body's links from another's.
pub fn body(id: u32, text: &str) -> impl IntoElement + use<> {
    let runs = markup::runs(text);
    let whole: String = runs.iter().map(|run| run.text.as_str()).collect();

    let mut at = 0;
    let (mut styles, mut links, mut addresses) = (Vec::new(), Vec::new(), Vec::new());
    for run in &runs {
        let range = at..at + run.text.len();
        at = range.end;
        let linked = !run.href.is_empty();
        if linked {
            links.push(range.clone());
            addresses.push(run.href.clone());
        }
        if run.bold || run.italic || run.underline || linked {
            styles.push((
                range,
                HighlightStyle {
                    color: (run.bold || linked).then(theme::text),
                    font_weight: run.bold.then_some(FontWeight::SEMIBOLD),
                    font_style: run.italic.then_some(FontStyle::Italic),
                    underline: (run.underline || linked).then(|| UnderlineStyle {
                        thickness: px(1.),
                        color: Some(theme::white(if linked { 0.3 } else { 0.56 })),
                        wavy: false,
                    }),
                    ..Default::default()
                },
            ));
        }
    }

    let text = InteractiveText::new(("body", id as usize), StyledText::new(whole).with_highlights(styles))
        .on_click(links, move |index, _, cx| open_link(addresses[index].clone(), cx));
    rsx! {
        <div
            id={("body-scroll", id as usize)}
            class="pt-[5px] max-h-[220px] overflow-y-scroll"
            text_size={px(12.)}
            line_height={px(17.4)}
            text_color={theme::text_dim()}
        >
            {text}
        </div>
    }
}

/// Opens a link from a body in the browser.
///
/// The address was written by whoever sent the notification, so it is never
/// handed to a shell: only to `xdg-open`, as one argument, and only if it is
/// the kind of link a notification has any business containing. Waited for,
/// away from the thread that draws, because a child that is never waited on
/// stays in the process table for as long as the shell runs.
fn open_link(address: String, cx: &App) {
    let lower = address.to_ascii_lowercase();
    if !["https://", "http://", "mailto:"].iter().any(|scheme| lower.starts_with(scheme)) {
        return;
    }
    cx.background_spawn(async move { drop(std::process::Command::new("xdg-open").arg(address).status()) }).detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn how_long_ago_is_said_in_the_largest_unit_that_fits() {
        let minute = 60_000;
        assert_eq!(ago(0, 59_000), "now");
        assert_eq!(ago(0, 4 * minute), "4m");
        assert_eq!(ago(0, 125 * minute), "2h");
        assert_eq!(ago(0, 3 * 1440 * minute + 5), "3d");
        // A clock that went backwards is not a notification from the future.
        assert_eq!(ago(10 * minute, 0), "now");
    }

    #[test]
    fn a_notification_without_a_picture_gets_a_glyph_from_what_it_says() {
        assert_eq!(fallback_glyph("Screenshot taken", 1), "screenshot_monitor");
        assert_eq!(fallback_glyph("Battery low", CRITICAL), "power");
        assert_eq!(fallback_glyph("Something else", CRITICAL), "release_alert");
        assert_eq!(fallback_glyph("Hello", 1), "chat");
    }
}
