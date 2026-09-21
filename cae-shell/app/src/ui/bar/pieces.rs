//! The shapes everything on the bar is made of.
//!
//! Everything on it is one of these: a pill with an optional glyph and an
//! optional number, sometimes grouped in a trough with others of its kind.
//! Buttons, readouts and the clock differ in what they say, not in how they
//! sit.

use gpui::{Div, FontWeight, Hsla, IntoElement, SharedString, StyleRefinement, Styled, div, prelude::*, px, rgba};

use crate::theme;
use crate::ui::rsx;

/// What a pill is saying by its colour, when it is saying anything.
#[derive(Clone, Copy, PartialEq)]
pub enum Tone {
    Plain,
    /// Something is switched on: a mode armed, the microphone muted.
    Lit,
    /// Nothing is answering, so it is drawn as unavailable rather than
    /// hidden: a slot that disappears is a slot you stop checking.
    Dim,
    Warn,
    Alert,
}

impl Tone {
    pub fn colour(self) -> Hsla {
        match self {
            Tone::Plain => theme::text_dim(),
            Tone::Lit => theme::accent(),
            Tone::Dim => theme::text_faint(),
            Tone::Warn => theme::warn(),
            Tone::Alert => theme::alert(),
        }
    }
}

/// What a pill does under the pointer: a wash of white, and a plain one
/// brightens. One that is saying something keeps saying it.
fn hovered(tone: Tone, wash: f32) -> impl Fn(StyleRefinement) -> StyleRefinement {
    move |style| {
        let style = style.bg(theme::white(wash));
        if tone == Tone::Plain { style.text_color(theme::text()) } else { style }
    }
}

/// A pill on its own in the row.
pub fn pill(tone: Tone) -> Div {
    rsx! {
        <div
            class="relative flex flex-none items-center gap-[7px] h-[26px] px-[9px]"
            rounded={theme::PILL_RADIUS}
            text_size={px(12.)}
            text_color={tone.colour()}
            hover={hovered(tone, 0.055)}
        />
    }
}

/// A pill inside a section. It loses its own trough, because two nested
/// rounded fills is one too many, and takes the section's shape: a squarish
/// highlight inside a fully rounded trough leaves a sliver showing at each
/// corner.
pub fn slot(tone: Tone) -> Div {
    rsx! { <div base={pill(tone)} class="h-[22px] px-[7px] rounded-full" hover={hovered(tone, 0.07)} /> }
}

/// A trough that groups related readouts. Three dials in one, the tray in
/// another, the date in a third: the grouping is what turns a row of glyphs
/// into a handful of things.
pub fn section() -> Div {
    rsx! {
        <div
            class="flex flex-none items-center gap-[2px] h-[26px] px-[4px] rounded-full"
            bg={theme::white(0.04)}
            shadow={theme::section_shadows()}
        />
    }
}

/// A count on the corner of a pill, where it cannot be missed and only while
/// there is one.
pub fn badge(count: impl Into<SharedString>) -> impl IntoElement {
    rsx! {
        <div
            class="absolute flex items-center justify-center min-w-[14px] h-[14px] px-[3px] rounded-full font-semibold"
            top={px(-1.)}
            right={px(-1.)}
            bg={theme::text()}
            text_color={rgba(0x101013ff)}
            text_size={px(9.5)}
            line_height={px(14.)}
        >
            {count.into()}
        </div>
    }
}
