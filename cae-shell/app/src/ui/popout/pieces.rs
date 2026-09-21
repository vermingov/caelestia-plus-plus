//! What panels are made of.
//!
//! A panel is a column of these and of the shell's controls, and nothing
//! else.

use gpui::{Div, SharedString, Styled, div, prelude::*, px};

pub use crate::ui::controls::{chip, entry, ghost, meter, meter_in, switch, toggle, trailing, warning, wide};
use crate::theme;
use crate::ui::rsx;

/// A panel's rows, top to bottom.
///
/// Nothing in one may have a negative bottom margin. GPUI lays a column with
/// one in it out at no height at all, and the panel comes up as a sliver
/// showing the top half of its first line. Negative side margins are fine,
/// and the lists use them.
pub fn column() -> Div {
    rsx! { <div class="flex flex-col gap-[11px] min-h-[0px]" /> }
}

/// What the panel is about, in one line.
pub fn headline(text: impl Into<SharedString>) -> Div {
    rsx! { <div text_size={px(14.)} text_color={theme::text()}>{text.into()}</div> }
}

/// The small print under it.
pub fn detail(text: impl Into<SharedString>) -> Div {
    rsx! { <div text_size={px(12.)} line_height={px(17.4)} text_color={theme::text_faint()}>{text.into()}</div> }
}

/// A quiet count or section label, between a headline and a detail.
pub fn caption(text: impl Into<SharedString>) -> Div {
    rsx! { <div class="mt-[2px]" text_size={px(11.5)} text_color={theme::text_faint()}>{text.into()}</div> }
}

/// A headline with something to press at the other end of it.
pub fn row() -> Div {
    rsx! { <div class="flex items-center justify-between gap-[16px]" /> }
}

/// Networks, paired devices, outputs. Capped rather than endless: a scan in a
/// block of flats is forty entries, and the panel has to stay a panel.
///
/// Scrolling is the caller's to ask for, with the `id` that scrolling needs:
/// what has an identity is said in the markup, where it can be seen.
pub fn list() -> Div {
    rsx! { <div class="flex flex-col flex-shrink-1 gap-[1px] mx-[-6px] max-h-[420px]" /> }
}
