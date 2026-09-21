//! What one result looks like: a row in the list, or a tile in the strip of
//! wallpapers. One shape of row covers every mode, drawn from whichever of an
//! icon file, a glyph or a palette the result came with.

use std::path::PathBuf;

use cae_core::launcher::Entry;
use gpui::{
    Div, FontWeight, Hsla, IntoElement, ObjectFit, Pixels, SharedString, Styled, div, img, prelude::*, px, relative, rgba,
};

use crate::theme;
use crate::ui::glyph::glyph;
use crate::ui::rsx;

/// A row's height, which the list's height and the highlight's place are
/// both counted in.
pub const ROW: Pixels = px(44.);

/// `rrggbb`, as a scheme lists its colours.
fn colour(hex: &str) -> Hsla {
    let value = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0x808080);
    rgba(value << 8 | 0xff).into()
}

/// What stands at the head of a row.
fn picture(entry: &Entry) -> impl IntoElement + use<> {
    if !entry.icon.is_empty() {
        return rsx! { <img src={PathBuf::from(&entry.icon)} class="flex-none size-[22px]" object_fit={ObjectFit::Contain} /> }
            .into_any_element();
    }
    if !entry.glyph.is_empty() {
        return rsx! {
            <div class="flex flex-none items-center justify-center size-[22px]" text_color={theme::text_dim()}>
                {glyph(entry.glyph.clone(), px(20.))}
            </div>
        }
        .into_any_element();
    }
    if !entry.swatches.is_empty() {
        // A scheme's palette, as the four colours that identify it.
        return rsx! {
            <div class="flex flex-none flex-wrap size-[22px] overflow-hidden" rounded={px(6.)}>
                {for swatch in entry.swatches.iter().take(4) {
                    <div class="size-[11px]" bg={colour(swatch)} />
                }}
            </div>
        }
        .into_any_element();
    }
    rsx! { <div class="flex-none size-[22px]" rounded={px(6.)} bg={theme::white(0.07)} /> }.into_any_element()
}

/// The heart on a favourite, the tick on whatever is already in use.
fn mark(entry: &Entry) -> Option<impl IntoElement + use<>> {
    let symbol = match (entry.marked, entry.trailing.as_str()) {
        (false, _) | (true, "Calculator") => return None,
        (true, "Application") => "favorite",
        (true, _) => "check",
    };
    Some(glyph(symbol, px(16.)).text_color(theme::accent()))
}

pub fn row(entry: &Entry) -> Div {
    // The calculator says an expression is wrong by marking its answer.
    let failed = entry.marked && entry.trailing == "Calculator";
    rsx! {
        <div class="relative flex flex-none items-center gap-[12px] px-[10px] cursor-pointer" h={ROW} rounded={px(9.)}>
            {picture(entry)}
            <div
                class="flex-none truncate"
                max_w={relative(0.44)}
                text_size={px(14.)}
                text_color={if failed { theme::alert() } else { theme::text() }}
            >
                {SharedString::from(entry.name.clone())}
            </div>
            <div class="flex-1 min-w-[0px] truncate" text_size={px(13.)} text_color={theme::text_dim()}>
                {SharedString::from(entry.comment.clone())}
            </div>
            {...mark(entry)}
            <div class="flex-none" text_size={px(13.)} text_color={theme::text_faint()}>
                {SharedString::from(entry.trailing.clone())}
            </div>
        </div>
    }
}

/// How tall a tile is for a given width: a 16:10 face, and a line of name
/// under it.
pub fn tile_height(width: Pixels) -> Pixels {
    width * 10. / 16. + px(31.)
}

/// One wallpaper. The chosen one lifts rather than gaining a border: a ring
/// round a photograph reads as a crop mark.
pub fn tile(entry: &Entry, width: Pixels, current: bool) -> Div {
    rsx! {
        <div
            class="relative flex flex-col flex-none overflow-hidden cursor-pointer"
            w={width}
            h={tile_height(width)}
            rounded={px(12.)}
            bg={theme::white(0.04)}
            shadow={if current { theme::tile_lifted() } else { theme::edge(0.05) }}
            when={(current, |tile| tile.mt(px(-3.)).mb(px(3.)))}
        >
            {...(!entry.preview.is_empty()).then(|| rsx! {
                <img
                    src={PathBuf::from(&entry.preview)}
                    class="flex-none w-full"
                    h={width * 10. / 16.}
                    object_fit={ObjectFit::Cover}
                />
            })}
            <div class="flex-none truncate px-[10px] pt-[7px] font-medium" text_size={px(12.)}>
                {SharedString::from(entry.name.clone())}
            </div>
            {...entry.marked.then(|| rsx! {
                <div
                    class="absolute flex items-center justify-center size-[22px] rounded-full"
                    top={px(7.)}
                    right={px(7.)}
                    bg={rgba(0x0000008c)}
                    text_color={theme::white(1.)}
                >
                    {glyph("check", px(15.))}
                </div>
            })}
        </div>
    }
}

/// A key, named in the footer and the hint.
pub fn key(name: &'static str) -> impl IntoElement {
    rsx! {
        <div
            class="flex flex-none items-center justify-center min-w-[20px] h-[20px] px-[5px]"
            rounded={px(6.)}
            bg={theme::white(0.07)}
            shadow={theme::edge(0.05)}
            text_size={px(11.)}
            text_color={theme::text_dim()}
        >
            {name}
        </div>
    }
}
