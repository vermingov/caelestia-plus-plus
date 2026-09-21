//! The workspace row: one track with a travelling marker under the numbers,
//! rather than a filled box that jumps from one to the next. The thing that
//! moves should look like one thing moving.

use std::time::Duration;

use cae_core::{hypr, logo};
use gpui::{
    Animation, AnimationExt, App, ElementId, FontWeight, IntoElement, ScrollWheelEvent, Styled, Window, div,
    prelude::*, px,
};

use crate::ui::rsx;
use crate::{actions, ease, theme};

/// A pip's width, and the distance from one pip's left edge to the next.
/// Fixed rather than measured: a marker placed from a count of pips cannot
/// drift out of register, and two digits fit in a width meant for one.
const PIP: f32 = 22.;
const PITCH: f32 = 24.;
/// The track's own padding, which is where the first pip starts.
const INSET: f32 = 3.;

/// Where the marker was, where it is going, and which journey this is.
///
/// The journey number names the animation, so a change of destination is a
/// new animation from wherever the last one was heading rather than the old
/// one carrying on to a place nobody is looking at any more.
#[derive(Clone, Copy, Default, PartialEq)]
pub struct Marker {
    from: usize,
    to: usize,
    journey: usize,
}

impl Marker {
    /// Points the marker at a pip, starting a journey if that is news.
    pub fn aim(&mut self, slot: usize) {
        if slot != self.to {
            self.from = self.to;
            self.to = slot;
            self.journey += 1;
        }
    }
}

/// One pip, as drawn.
struct Pip {
    id: i64,
    label: String,
    occupied: bool,
    focused: bool,
    windows: i64,
}

/// The group of pips the focused workspace falls in: 1–5, then 6–10, and so
/// on. A fixed group that pages, rather than a row whose length changes under
/// the pointer every time a workspace empties.
fn pips(workspaces: &[&hypr::Workspace], options: &logo::BarConfig) -> (Vec<Pip>, usize) {
    let shown = options.shown.max(1);
    let focused = workspaces.iter().find(|w| w.focused).map_or(1, |w| w.id);
    let offset = (focused - 1).div_euclid(shown) * shown;

    let pips = (0..shown)
        .map(|index| {
            let id = offset + index + 1;
            let workspace = workspaces.iter().find(|w| w.id == id);
            let windows = workspace.map_or(0, |w| w.windows);
            let occupied = windows > 0;
            let is_focused = id == focused;

            // The config can put a character in place of the number: one for
            // every pip, and separate ones for the occupied and focused.
            let label = [
                (is_focused, &options.active_label),
                (occupied, &options.occupied_label),
                (true, &options.label),
            ]
            .into_iter()
            .find(|(applies, label)| *applies && !label.is_empty())
            .map_or_else(|| id.to_string(), |(_, label)| label.clone());

            Pip { id, label, occupied, focused: is_focused, windows }
        })
        .collect();

    (pips, (focused - 1 - offset).max(0) as usize)
}

/// Which slot the marker belongs under, for the strip to aim it before it
/// draws.
pub fn focused_slot(workspaces: &[&hypr::Workspace], options: &logo::BarConfig) -> usize {
    pips(workspaces, options).1
}

pub fn row(workspaces: &[&hypr::Workspace], options: &logo::BarConfig, marker: Marker) -> impl IntoElement {
    let (pips, _) = pips(workspaces, options);

    // The run of workspaces that have something on them: it says which part
    // of the row is in use without marking each one.
    let first = pips.iter().position(|pip| pip.occupied);
    let last = pips.iter().rposition(|pip| pip.occupied);
    let occupied_run = options.occupied_bg.then_some(first.zip(last)).flatten().map(|(first, last)| {
        rsx! {
            <div
                class="absolute h-[20px]"
                top={px(3.)}
                left={px(INSET + first as f32 * PITCH)}
                w={px((last - first + 1) as f32 * PITCH - 2.)}
                rounded={px(6.)}
                bg={theme::white(0.05)}
            />
        }
    });

    let show_windows = options.show_windows;

    rsx! {
        <div
            id="workspaces"
            class="relative flex flex-none items-center h-[26px]"
            gap={px(PITCH - PIP)}
            px={px(INSET)}
            rounded={theme::PILL_RADIUS}
            bg={theme::white(0.03)}
            // Scrolling the row walks the workspaces, and stops there: the
            // bar behind it would take the same scroll for the volume.
            onScrollWheel={|event: &ScrollWheelEvent, window: &mut Window, cx: &mut App| {
                let down = event.delta.pixel_delta(window.line_height()).y < px(0.);
                actions::cycle_workspace(cx, down);
                cx.stop_propagation();
            }}
        >
            {...occupied_run}
            {travelling(marker, options.active_trail)}
            {for pip in pips {
                {number(pip, show_windows)}
            }}
        </div>
    }
}

/// One workspace's number, and how many windows are on it when the config
/// asks for that.
fn number(pip: Pip, show_windows: bool) -> impl IntoElement {
    let id = pip.id;
    let colour = match (pip.focused, pip.occupied) {
        (true, _) => theme::on_accent(),
        (false, true) => theme::text_dim(),
        (false, false) => theme::text_faint(),
    };

    rsx! {
        <div
            id={("workspace", id as usize)}
            class="relative flex items-center justify-center h-[20px]"
            w={px(PIP)}
            rounded={px(6.)}
            text_size={px(11.5)}
            font_features={theme::tabular()}
            text_color={colour}
            when={(pip.focused, |pip| pip.font_weight(FontWeight::SEMIBOLD))}
            when={(!pip.focused, |pip| pip.hover(|style| style.text_color(theme::text_dim())))}
            onClick={move |_, _, cx| actions::focus_workspace(cx, id)}
        >
            {pip.label}
            {...(show_windows && pip.windows > 0).then(|| rsx! {
                <div
                    class="absolute"
                    right={px(1.)}
                    bottom={px(0.)}
                    text_size={px(8.)}
                    line_height={px(8.)}
                    opacity={0.65}
                >
                    {pip.windows.to_string()}
                </div>
            })}
        </div>
    }
}

/// The marker on its way from one pip to another.
///
/// While it travels it stretches to cover the ground between where it was and
/// where it is going, then settles: the edge in front runs ahead and the one
/// behind catches up. Without that, a jump between distant workspaces reads as
/// a teleport.
fn travelling(marker: Marker, trail: bool) -> impl IntoElement {
    let edge = |slot: usize| INSET + slot as f32 * PITCH;
    let (from, to) = (edge(marker.from), edge(marker.to));

    let marker_shape = rsx! { <div class="absolute h-[20px]" top={px(3.)} rounded={px(6.)} bg={theme::accent()} /> };

    marker_shape.with_animation(
        ElementId::NamedInteger("workspace-marker".into(), marker.journey as u64),
        Animation::new(Duration::from_millis(260)),
        move |marker, progress| {
            let settle = ease::settle();
            let leading = settle((progress * 1.5).min(1.));
            let trailing = if trail { settle(((progress - 0.2) / 0.8).max(0.)) } else { leading };

            // Moving right, the right edge leads; moving left, the left one.
            let (left, right) = if to >= from { (trailing, leading) } else { (leading, trailing) };
            let left_edge = from + (to - from) * left;
            let right_edge = from + PIP + (to - from) * right;
            marker.left(px(left_edge)).w(px(right_edge - left_edge))
        },
    )
}
