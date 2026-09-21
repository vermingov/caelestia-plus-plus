//! The things a panel or a page is pressed, dragged and read through.
//!
//! One of each: one switch shape for a radio and a mode, one row for a
//! network and a paired device and an audio output, one bar for every
//! quantity. They were the popouts' alone until the settings wanted the same
//! ones, and a switch that looked different in the two would be two switches.

use gpui::{Div, Hsla, IntoElement, SharedString, Styled, div, prelude::*, px, relative, rgba};

use crate::theme;
use crate::ui::glyph::glyph;
use crate::ui::rsx;

/// One shape for every quantity a popout shows: signal, charge, load, memory.
pub fn meter(percent: f64) -> impl IntoElement {
    meter_in(percent, theme::white(0.45))
}

pub fn meter_in(percent: f64, fill: Hsla) -> impl IntoElement {
    let filled = (percent.clamp(0., 100.) / 100.) as f32;
    rsx! {
        <div class="flex-none h-[4px] rounded-full overflow-hidden" bg={theme::white(0.07)}>
            <div class="h-full rounded-full" w={relative(filled)} bg={fill} />
        </div>
    }
}

/// A small pill that is pressed: filled when it is on, or when it is the
/// thing to press.
pub fn chip(label: impl Into<SharedString>, lit: bool) -> Div {
    let (fill, ink) = if lit { (theme::white(0.16), theme::text()) } else { (theme::white(0.07), theme::text_dim()) };
    rsx! {
        <div
            class="flex flex-none items-center h-[22px] px-[10px] rounded-full cursor-pointer"
            text_size={px(11.)}
            bg={fill}
            text_color={ink}
            hover={move |style| if lit { style } else { style.bg(theme::white(0.11)).text_color(theme::text()) }}
        >
            {label.into()}
        </div>
    }
}

/// One switch shape, for the wifi radio and the bluetooth adapter. It says
/// what it is rather than showing a knob, because there is a word for both
/// of its states and the word is shorter.
pub fn toggle(on: bool) -> Div {
    chip(if on { "On" } else { "Off" }, on)
}

/// One thing in a list: a glyph, what it is called, and whatever goes at the
/// far end. The one in use is filled; the rest answer to the pointer.
pub fn entry(symbol: &'static str, label: impl Into<SharedString>, current: bool) -> Div {
    rsx! {
        <div
            class="flex flex-none items-center gap-[10px] min-h-[30px] px-[8px] cursor-pointer"
            rounded={px(7.)}
            text_size={px(12.)}
            text_color={if current { theme::text() } else { theme::text_dim() }}
            when={(current, |entry| entry.bg(theme::white(0.09)).shadow(theme::edge(0.08)))}
            when={(!current, |entry| entry.hover(|style| style.bg(theme::white(0.055)).text_color(theme::text())))}
        >
            {glyph(symbol, px(15.))}
            <div class="flex-1 min-w-[0px] truncate">{label.into()}</div>
        </div>
    }
}

/// What goes at the far end of an entry when it is words: a strength, a state.
pub fn trailing(text: impl Into<SharedString>) -> Div {
    rsx! { <div class="flex-none" text_size={px(11.)} text_color={theme::text_faint()}>{text.into()}</div> }
}

/// A full-width action, for the one thing a panel does rather than shows.
pub fn wide(symbol: &'static str, label: impl Into<SharedString>) -> Div {
    rsx! {
        <div
            class="flex flex-none items-center justify-center gap-[8px] h-[30px] cursor-pointer"
            rounded={px(9.)}
            text_size={px(12.)}
            bg={theme::white(0.07)}
            text_color={theme::text_dim()}
            hover={|style| style.bg(theme::white(0.12)).text_color(theme::text())}
        >
            {glyph(symbol, px(16.))}
            {label.into()}
        </div>
    }
}

/// A destructive action inside a row: present, but never the thing the eye
/// lands on first.
pub fn ghost(symbol: &'static str) -> Div {
    rsx! {
        <div
            class="flex flex-none items-center justify-center size-[20px] rounded-full cursor-pointer"
            text_color={theme::text_faint()}
            hover={|style| style.bg(rgba(0xff8a802e)).text_color(theme::alert())}
        >
            {glyph(symbol, px(14.))}
        </div>
    }
}

/// A round button with a glyph in it.
pub fn round(symbol: &'static str, lit: bool) -> Div {
    rsx! {
        <div
            class="flex flex-none items-center justify-center size-[28px] rounded-full cursor-pointer"
            bg={theme::white(if lit { 0.16 } else { 0.07 })}
            text_color={if lit { theme::text() } else { theme::text_dim() }}
            hover={|style| style.bg(theme::white(0.13)).text_color(theme::text())}
        >
            {glyph(symbol, px(16.))}
        </div>
    }
}

/// The one thing in a popout that is allowed to be loud, because it is the
/// one thing the machine is telling you rather than showing you.
pub fn warning(text: impl Into<SharedString>) -> impl IntoElement {
    rsx! {
        <div
            class="flex flex-none items-center gap-[9px] py-[9px] px-[11px]"
            rounded={px(10.)}
            bg={rgba(0xff8a8026)}
            shadow={theme::edge_in(rgba(0xff8a8040).into())}
            text_color={theme::alert()}
            text_size={px(12.)}
            line_height={px(16.2)}
        >
            {glyph("warning", px(17.))}
            <div class="min-w-[0px]">{text.into()}</div>
        </div>
    }
}

/// A real switch, because a mode that changes how the machine behaves should
/// look like something you flip rather than something you read.
pub fn switch(on: bool) -> impl IntoElement {
    rsx! {
        <div
            class="relative flex-none w-[42px] h-[24px] rounded-full"
            bg={if on { theme::accent() } else { theme::white(0.09) }}
            when={(!on, |track| track.shadow(theme::edge(0.07)))}
        >
            <div
                class="absolute flex items-center justify-center size-[18px] rounded-full"
                top={px(3.)}
                left={px(if on { 21. } else { 3. })}
                bg={if on { theme::white(1.) } else { theme::white(0.55) }}
                text_color={rgba(0x15151aff)}
            >
                {glyph(if on { "check" } else { "close" }, px(13.))}
            </div>
        </div>
    }
}
