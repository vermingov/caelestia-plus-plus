//! Material Symbols, by name.
//!
//! The font maps a name to its glyph with a ligature, so "wifi" set in it is
//! the wifi symbol. That is all an icon is here: a short string in a
//! different font, which costs what text costs and takes the colour text does.

use gpui::{Div, Pixels, SharedString, Styled, div, prelude::*};

use crate::theme;
use crate::ui::rsx;

pub fn glyph(name: impl Into<SharedString>, size: Pixels) -> Div {
    // The symbol's box is its em square; anything taller is the text line's
    // leading, which would push a glyph off the middle of a pill.
    rsx! {
        <div class="flex-none" font_family={theme::SYMBOLS} text_size={size} line_height={size}>
            {name.into()}
        </div>
    }
}
