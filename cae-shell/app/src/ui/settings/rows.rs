//! What a page of settings is made of.
//!
//! A page is sections, a section is rows, and a row is what something is
//! called on the left and the thing that changes it on the right. No boxes:
//! rows are told apart by a hairline and sections by the space above them.

use gpui::{Div, Entity, FontWeight, IntoElement, SharedString, Styled, div, prelude::*, px};

use crate::theme;
use crate::ui::field::Field;
use crate::ui::glyph::glyph;
use crate::ui::rsx;

/// As wide as a page's column of rows gets. Wider, and the eye loses the
/// line between a label and the switch that belongs to it.
pub const COLUMN: gpui::Pixels = px(660.);

/// A page's sections, top to bottom.
pub fn page() -> Div {
    rsx! { <div class="flex flex-col w-full pb-[28px]" max_w={COLUMN} /> }
}

/// What a group of rows is about. Sits closer to the rows under it than to
/// the ones above, which is what makes it theirs.
pub fn section_title(text: impl Into<SharedString>, first: bool) -> Div {
    rsx! {
        <div
            class="pb-[6px]"
            pt={px(if first { 2. } else { 26. })}
            text_size={px(12.)}
            text_color={theme::text_faint()}
        >
            {text.into()}
        </div>
    }
}

/// The line between two rows.
pub fn rule() -> Div {
    rsx! { <div class="flex-none h-[1px]" bg={theme::white(0.05)} /> }
}

/// A row, with what it is called already in it. Whatever changes it is the
/// caller's to add, and goes at the far end.
///
/// What it is called keeps a floor of its own width: a control with a dozen
/// things in it would otherwise squeeze the label to one letter a line, and
/// a control that wide is the one that should be wrapping.
pub fn row(label: impl Into<SharedString>, note: impl Into<SharedString>, live: bool) -> Div {
    let note: SharedString = note.into();
    rsx! {
        <div
            class="flex flex-none items-center justify-between gap-[20px] min-h-[46px] py-[8px]"
            when={(!live, |row| row.opacity(0.38))}
        >
            <div class="flex flex-col flex-1 gap-[3px]" min_w={px(112.)}>
                <div text_size={px(13.)} text_color={theme::text()}>{label.into()}</div>
                {...(!note.is_empty()).then(|| rsx! {
                    <div text_size={px(11.5)} line_height={px(15.5)} text_color={theme::text_faint()}>{note}</div>
                })}
            </div>
        </div>
    }
}

/// A row that is pressed as a whole: one with a switch at the end of it, one
/// that leads somewhere. It answers to the pointer along its whole length,
/// a little past the text at each end.
pub fn pressable(row: Div) -> Div {
    row.mx(px(-10.)).px(px(10.)).rounded(px(9.)).cursor_pointer().hover(|style| style.bg(theme::white(0.035)))
}

/// A row that leads to another page: a glyph, where it goes, what is there.
pub fn leads(symbol: &'static str, label: impl Into<SharedString>, says: impl Into<SharedString>) -> Div {
    let says: SharedString = says.into();
    rsx! {
        <div class="flex flex-none items-center gap-[14px] min-h-[52px] py-[8px]" base={pressable(div())}>
            <div
                class="flex flex-none items-center justify-center size-[32px] rounded-full"
                bg={theme::white(0.06)}
                text_color={theme::text_dim()}
            >
                {glyph(symbol, px(17.))}
            </div>
            <div class="flex flex-col flex-1 gap-[3px] min-w-[0px]">
                <div text_size={px(13.)} text_color={theme::text()}>{label.into()}</div>
                {...(!says.is_empty()).then(|| rsx! {
                    <div class="truncate" text_size={px(11.5)} text_color={theme::text_faint()}>{says}</div>
                })}
            </div>
            <div class="flex-none" text_color={theme::text_faint()}>{glyph("chevron_right", px(18.))}</div>
        </div>
    }
}

/// One of several, of which one is chosen: an audio device, a network, a
/// monitor's mode. The chosen one is lit, and `chosen_mark` goes at the far
/// end of it, after whatever else the caller puts there.
pub fn pick(symbol: &'static str, label: impl Into<SharedString>, note: impl Into<SharedString>, chosen: bool) -> Div {
    let note: SharedString = note.into();
    rsx! {
        <div class="flex flex-none items-center gap-[12px] min-h-[42px] py-[7px]" base={pressable(div())}>
            <div class="flex-none" text_color={if chosen { theme::text() } else { theme::text_faint() }}>{glyph(symbol, px(17.))}</div>
            <div class="flex flex-col flex-1 gap-[2px] min-w-[0px]">
                <div class="truncate" text_size={px(13.)} text_color={if chosen { theme::text() } else { theme::text_dim() }}>
                    {label.into()}
                </div>
                {...(!note.is_empty()).then(|| rsx! {
                    <div class="truncate" text_size={px(11.5)} text_color={theme::text_faint()}>{note}</div>
                })}
            </div>
        </div>
    }
}

pub fn chosen_mark() -> Div {
    glyph("check", px(17.)).text_color(theme::accent())
}

/// What a slider is the level of, and what that level is, over the slider
/// itself: the glyph is pressed to mute, so it is the caller's to make.
pub fn level(symbol: impl IntoElement, label: impl Into<SharedString>, says: impl Into<SharedString>) -> Div {
    rsx! {
        <div class="flex flex-none items-center gap-[12px] pt-[10px] pb-[4px]">
            {symbol}
            <div class="flex-1 min-w-[0px] truncate" text_size={px(13.)} text_color={theme::text()}>{label.into()}</div>
            <div class="flex-none" text_size={px(13.)} text_color={theme::text_dim()} font_features={theme::tabular()}>
                {says.into()}
            </div>
        </div>
    }
}

/// A press that cannot be taken back, asked twice: the first press arms it,
/// and it says so by turning the colour a warning is.
pub fn press_twice(symbol: &'static str, armed: bool, says: &'static str) -> Div {
    if !armed {
        return press(symbol, false);
    }
    rsx! {
        <div
            class="flex flex-none items-center gap-[6px] h-[30px] pl-[9px] pr-[12px] rounded-full cursor-pointer"
            text_size={px(12.)}
            bg={gpui::rgba(0xff8a8030)}
            text_color={theme::alert()}
            hover={|style| style.bg(gpui::rgba(0xff8a8048))}
        >
            {glyph(symbol, px(16.))}
            {says}
        </div>
    }
}

/// A glyph that is pressed: mute, forget, remove.
pub fn press(symbol: &'static str, lit: bool) -> Div {
    rsx! {
        <div
            class="flex flex-none items-center justify-center size-[30px] rounded-full cursor-pointer"
            bg={theme::white(if lit { 0.14 } else { 0.06 })}
            text_color={if lit { theme::text() } else { theme::text_dim() }}
            hover={|style| style.bg(theme::white(0.16)).text_color(theme::text())}
        >
            {glyph(symbol, px(17.))}
        </div>
    }
}

/// One end of a stepper.
pub fn step(symbol: &'static str, live: bool) -> Div {
    rsx! {
        <div
            class="flex flex-none items-center justify-center size-[26px] rounded-full"
            bg={theme::white(0.06)}
            text_color={if live { theme::text_dim() } else { theme::text_faint() }}
            when={(live, |step| step.cursor_pointer().hover(|style| style.bg(theme::white(0.12)).text_color(theme::text())))}
            when={(!live, |step| step.opacity(0.45))}
        >
            {glyph(symbol, px(15.))}
        </div>
    }
}

/// The number between a stepper's two ends. One width whatever it says, in
/// figures that are all one width too, so that stepping from 9 to 10 moves
/// nothing.
pub fn figure(text: impl Into<SharedString>) -> Div {
    rsx! {
        <div
            class="flex flex-none justify-center min-w-[62px]"
            text_size={px(13.)}
            text_color={theme::text()}
            font_features={theme::tabular()}
        >
            {text.into()}
        </div>
    }
}

/// What a number reads as: whole when its steps are whole, and with its unit
/// closed up against it the way the unit is written.
pub fn figure_text(shown: f64, step: f64, unit: &str) -> String {
    let number = if step.fract() == 0. { format!("{shown:.0}") } else { format!("{shown:.1}") };
    match unit {
        "" => number,
        "%" => format!("{number}%"),
        unit => format!("{number} {unit}"),
    }
}

/// The box a line of text is typed into.
pub fn typed(field: &Entity<Field>, focused: bool) -> Div {
    rsx! {
        <div
            class="flex flex-none items-center w-[230px] h-[30px] px-[10px]"
            rounded={px(8.)}
            bg={theme::white(if focused { 0.075 } else { 0.05 })}
            shadow={if focused { theme::edge_in(theme::accent().opacity(0.55)) } else { theme::edge(0.06) }}
            text_size={px(12.5)}
            text_color={theme::text()}
        >
            {field.clone()}
        </div>
    }
}

/// The box a page's own search is typed into, across the whole column.
pub fn searched(field: &Entity<Field>, focused: bool) -> Div {
    rsx! {
        <div
            class="flex flex-none items-center gap-[9px] h-[34px] px-[11px] mb-[8px]"
            rounded={px(9.)}
            bg={theme::white(if focused { 0.075 } else { 0.05 })}
            shadow={if focused { theme::edge_in(theme::accent().opacity(0.55)) } else { theme::edge(0.06) }}
            text_size={px(13.)}
            text_color={theme::text()}
        >
            {glyph("search", px(17.)).text_color(theme::text_faint())}
            {field.clone()}
        </div>
    }
}

/// Something a page does rather than sets: check for updates, forget a
/// device. `strong` is for the one thing on a page that is the point of it.
pub fn button(symbol: &'static str, label: impl Into<SharedString>, strong: bool) -> Div {
    let (fill, ink) = if strong { (theme::accent(), theme::on_accent()) } else { (theme::white(0.07), theme::text_dim()) };
    rsx! {
        <div
            class="flex flex-none items-center gap-[8px] h-[32px] pl-[12px] pr-[14px] cursor-pointer"
            rounded={px(9.)}
            text_size={px(12.5)}
            bg={fill}
            text_color={ink}
            when={(strong, |button| button.font_weight(FontWeight::MEDIUM).hover(|style| style.opacity(0.88)))}
            when={(!strong, |button| button.hover(|style| style.bg(theme::white(0.12)).text_color(theme::text())))}
        >
            {glyph(symbol, px(16.))}
            {label.into()}
        </div>
    }
}

/// A fact, in a row of its own: what it is on the left, what it says on the
/// right, where it can be selected by nobody because it is not text.
pub fn fact(label: impl Into<SharedString>, value: impl Into<SharedString>) -> Div {
    rsx! {
        <div class="flex flex-none items-center justify-between gap-[20px] min-h-[40px] py-[6px]">
            <div class="flex-none" text_size={px(13.)} text_color={theme::text_dim()}>{label.into()}</div>
            <div class="min-w-[0px] truncate" text_size={px(13.)} text_color={theme::text()}>{value.into()}</div>
        </div>
    }
}

/// What a page says when it has nothing to list.
pub fn nothing(symbol: &'static str, text: impl Into<SharedString>) -> Div {
    rsx! {
        <div class="flex flex-col flex-none items-center gap-[8px] py-[34px]" text_color={theme::text_faint()}>
            {glyph(symbol, px(26.))}
            <div text_size={px(12.5)}>{text.into()}</div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::figure_text;

    #[test]
    fn a_number_reads_the_way_its_unit_is_written() {
        assert_eq!(figure_text(10., 1., "%"), "10%");
        assert_eq!(figure_text(500., 50., "ms"), "500 ms");
        assert_eq!(figure_text(1.5, 0.5, "s"), "1.5 s");
        assert_eq!(figure_text(7., 1., ""), "7");
        // Whole steps never show a fraction, whatever arithmetic left behind.
        assert_eq!(figure_text(29.999_999, 1., "px"), "30 px");
    }
}
