//! One line of text that can be typed into.
//!
//! GPUI has the parts of a text field and not the field, so this is one: the
//! launcher's search box, and the password a network asks for. Built the way
//! GPUI's own input example is, around its input handler rather than around
//! key presses, because a key press is not a character. A dead key and the
//! letter after it are two presses and one é, an input method is a dozen
//! presses and one word, and only the input handler is told how those come
//! out.

use std::ops::Range;
use std::time::Duration;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId, InspectorElementId, IntoElement,
    KeyBinding, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render,
    ShapedLine, SharedString, Style, Styled, TextRun, UTF16Selection, UnderlineStyle, Window, actions, div, fill,
    point, prelude::*, px, relative, size,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::theme;
use crate::ui::rsx;

actions!(field, [Backspace, Delete, DeleteWord, Left, Right, SelectLeft, SelectRight, SelectAll, Home, End, Paste, Cut, Copy]);

/// What the keys do while a field has the keyboard. Once, at startup.
pub fn bind_keys(cx: &mut App) {
    const CONTEXT: Option<&str> = Some("Field");
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, CONTEXT),
        KeyBinding::new("delete", Delete, CONTEXT),
        KeyBinding::new("ctrl-backspace", DeleteWord, CONTEXT),
        KeyBinding::new("ctrl-w", DeleteWord, CONTEXT),
        KeyBinding::new("left", Left, CONTEXT),
        KeyBinding::new("right", Right, CONTEXT),
        KeyBinding::new("shift-left", SelectLeft, CONTEXT),
        KeyBinding::new("shift-right", SelectRight, CONTEXT),
        KeyBinding::new("ctrl-a", SelectAll, CONTEXT),
        KeyBinding::new("home", Home, CONTEXT),
        KeyBinding::new("end", End, CONTEXT),
        KeyBinding::new("ctrl-v", Paste, CONTEXT),
        KeyBinding::new("ctrl-x", Cut, CONTEXT),
        KeyBinding::new("ctrl-c", Copy, CONTEXT),
    ]);
}

/// What a field tells whoever is holding it.
pub enum FieldEvent {
    /// The text is different from what it was.
    Changed,
}

/// How often the caret changes its mind about being visible.
const BLINK: Duration = Duration::from_millis(530);

pub struct Field {
    focus: FocusHandle,
    text: String,
    placeholder: SharedString,
    /// Drawn as dots, and never put on the clipboard.
    secret: bool,
    /// Left and right are somebody else's while this is set: a strip of
    /// wallpapers is walked with the same keys a caret is, and the strip is
    /// what is being looked at.
    pub arrows_navigate: bool,
    selection: Range<usize>,
    /// Which end of the selection moves.
    reversed: bool,
    /// What an input method is still composing, which is in the text but is
    /// not settled.
    marked: Option<Range<usize>>,
    selecting: bool,
    /// Whether it had the keyboard when it was last drawn.
    focused: bool,
    caret_shown: bool,
    /// What was drawn last, which is what a click is measured against.
    line: Option<ShapedLine>,
    bounds: Option<Bounds<Pixels>>,
}

impl EventEmitter<FieldEvent> for Field {}

impl Focusable for Field {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Field {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Field {
        cx.spawn(async move |field, cx| {
            loop {
                cx.background_executor().timer(BLINK).await;
                let blinked = field.update(cx, |field, cx| {
                    field.caret_shown = !field.caret_shown;
                    // A caret nobody can see has nothing to redraw, and a
                    // page of thirty boxes is thirty of those.
                    if field.focused {
                        cx.notify();
                    }
                });
                if blinked.is_err() {
                    break;
                }
            }
        })
        .detach();

        Field {
            focus: cx.focus_handle(),
            text: String::new(),
            placeholder: placeholder.into(),
            secret: false,
            arrows_navigate: false,
            selection: 0..0,
            reversed: false,
            marked: None,
            selecting: false,
            focused: false,
            caret_shown: true,
            line: None,
            bounds: None,
        }
    }

    pub fn secret(mut self) -> Field {
        self.secret = true;
        self
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// Replaces the lot and puts the caret at the end, as something that put
    /// text here on the person's behalf would want: a completion, a mode a
    /// keybind asked for.
    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        self.text = text.into();
        self.selection = self.text.len()..self.text.len();
        self.marked = None;
        self.stir(cx);
        cx.emit(FieldEvent::Changed);
    }

    /// The caret is solid while something is happening to it, and only
    /// blinks once it has been left alone.
    fn stir(&mut self, cx: &mut Context<Self>) {
        self.caret_shown = true;
        cx.notify();
    }

    fn caret(&self) -> usize {
        if self.reversed { self.selection.start } else { self.selection.end }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selection = offset..offset;
        self.reversed = false;
        self.stir(cx);
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.reversed {
            self.selection.start = offset;
        } else {
            self.selection.end = offset;
        }
        if self.selection.end < self.selection.start {
            self.reversed = !self.reversed;
            self.selection = self.selection.end..self.selection.start;
        }
        self.stir(cx);
    }

    fn before(&self, offset: usize) -> usize {
        self.text.grapheme_indices(true).rev().find_map(|(at, _)| (at < offset).then_some(at)).unwrap_or(0)
    }

    fn after(&self, offset: usize) -> usize {
        self.text.grapheme_indices(true).find_map(|(at, _)| (at > offset).then_some(at)).unwrap_or(self.text.len())
    }

    /// Where the word the caret is in, or has just left, begins.
    fn word_start(&self, offset: usize) -> usize {
        self.text[..offset].unicode_word_indices().next_back().map_or(0, |(at, _)| at)
    }

    fn index_at(&self, position: Point<Pixels>) -> usize {
        let (Some(bounds), Some(line)) = (self.bounds.as_ref(), self.line.as_ref()) else { return 0 };
        if self.text.is_empty() {
            return 0;
        }
        let shown = line.closest_index_for_x(position.x - bounds.left());
        if self.secret { self.text_offset(shown) } else { shown }
    }

    /// What is drawn for the text: itself, or a dot for each character of a
    /// secret. The two are different lengths in bytes, so a place in one has
    /// to be translated to reach the same place in the other.
    fn shown(&self) -> String {
        if self.secret { "•".repeat(self.text.graphemes(true).count()) } else { self.text.clone() }
    }

    fn shown_offset(&self, offset: usize) -> usize {
        if !self.secret {
            return offset;
        }
        self.text[..offset].graphemes(true).count() * '•'.len_utf8()
    }

    fn text_offset(&self, shown: usize) -> usize {
        let count = shown / '•'.len_utf8();
        self.text.grapheme_indices(true).nth(count).map_or(self.text.len(), |(at, _)| at)
    }

    fn replace(&mut self, range: Range<usize>, with: &str, cx: &mut Context<Self>) {
        self.text.replace_range(range.clone(), with);
        let caret = range.start + with.len();
        self.selection = caret..caret;
        self.reversed = false;
        self.marked = None;
        self.stir(cx);
        cx.emit(FieldEvent::Changed);
    }

    // What the keys are bound to.

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if self.selection.is_empty() {
            self.select_to(self.before(self.caret()), cx);
        }
        self.replace(self.selection.clone(), "", cx);
    }

    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        if self.selection.is_empty() {
            self.select_to(self.after(self.caret()), cx);
        }
        self.replace(self.selection.clone(), "", cx);
    }

    fn delete_word(&mut self, _: &DeleteWord, _: &mut Window, cx: &mut Context<Self>) {
        if self.selection.is_empty() {
            self.select_to(self.word_start(self.caret()), cx);
        }
        self.replace(self.selection.clone(), "", cx);
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.arrows_navigate {
            return cx.propagate();
        }
        let to = if self.selection.is_empty() { self.before(self.caret()) } else { self.selection.start };
        self.move_to(to, cx);
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.arrows_navigate {
            return cx.propagate();
        }
        let to = if self.selection.is_empty() { self.after(self.caret()) } else { self.selection.end };
        self.move_to(to, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.before(self.caret()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.after(self.caret()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.text.len(), cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.text.len(), cx);
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            // One line is all there is room for.
            self.replace(self.selection.clone(), &text.replace(['\n', '\r'], " "), cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.secret && !self.selection.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.text[self.selection.clone()].to_string()));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.secret && !self.selection.is_empty() {
            self.copy(&Copy, window, cx);
            self.replace(self.selection.clone(), "", cx);
        }
    }

    fn pressed(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.selecting = true;
        let at = self.index_at(event.position);
        if event.modifiers.shift { self.select_to(at, cx) } else { self.move_to(at, cx) }
    }

    fn dragged(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.selecting {
            self.select_to(self.index_at(event.position), cx);
        }
    }

    fn released(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.selecting = false;
    }

    // The input handler counts in UTF-16, because the platforms it was
    // written for do. The text here is UTF-8.

    fn utf16_at(&self, offset: usize) -> usize {
        self.text[..offset].encode_utf16().count()
    }

    fn offset_at_utf16(&self, offset: usize) -> usize {
        let mut units = 0;
        for (at, character) in self.text.char_indices() {
            if units >= offset {
                return at;
            }
            units += character.len_utf16();
        }
        self.text.len()
    }

    fn utf16_range(&self, range: &Range<usize>) -> Range<usize> {
        self.utf16_at(range.start)..self.utf16_at(range.end)
    }

    fn range_at_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_at_utf16(range.start)..self.offset_at_utf16(range.end)
    }

    /// Where an edit the input handler describes lands: the range it names,
    /// or what is being composed, or the selection.
    fn edited(&self, range_utf16: Option<Range<usize>>) -> Range<usize> {
        range_utf16
            .map(|range| self.range_at_utf16(&range))
            .or(self.marked.clone())
            .unwrap_or(self.selection.clone())
    }
}

impl EntityInputHandler for Field {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_at_utf16(&range_utf16);
        actual.replace(self.utf16_range(&range));
        Some(self.text[range].to_string())
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        Some(UTF16Selection { range: self.utf16_range(&self.selection), reversed: self.reversed })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|range| self.utf16_range(range))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace(self.edited(range_utf16), text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        selected_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.edited(range_utf16);
        self.text.replace_range(range.clone(), text);
        self.marked = (!text.is_empty()).then(|| range.start..range.start + text.len());
        self.selection = selected_utf16
            .map(|selected| self.range_at_utf16(&selected))
            .map(|selected| selected.start + range.start..selected.end + range.start)
            .unwrap_or(range.start + text.len()..range.start + text.len());
        self.stir(cx);
        cx.emit(FieldEvent::Changed);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let line = self.line.as_ref()?;
        let range = self.range_at_utf16(&range_utf16);
        let (from, to) = (self.shown_offset(range.start), self.shown_offset(range.end));
        Some(Bounds::from_corners(
            point(bounds.left() + line.x_for_index(from), bounds.top()),
            point(bounds.left() + line.x_for_index(to), bounds.bottom()),
        ))
    }

    fn character_index_for_point(&mut self, at: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        let local = self.bounds?.localize(&at)?;
        let shown = self.line.as_ref()?.index_for_x(local.x)?;
        Some(self.utf16_at(if self.secret { self.text_offset(shown) } else { shown }))
    }
}

impl Render for Field {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        rsx! {
            <div
                class="flex flex-1 items-center min-w-[0px] h-full overflow-hidden"
                key_context="Field"
                track_focus={&self.focus}
                cursor={CursorStyle::IBeam}
                on_action={cx.listener(Self::backspace)}
                on_action={cx.listener(Self::delete)}
                on_action={cx.listener(Self::delete_word)}
                on_action={cx.listener(Self::left)}
                on_action={cx.listener(Self::right)}
                on_action={cx.listener(Self::select_left)}
                on_action={cx.listener(Self::select_right)}
                on_action={cx.listener(Self::select_all)}
                on_action={cx.listener(Self::home)}
                on_action={cx.listener(Self::end)}
                on_action={cx.listener(Self::paste)}
                on_action={cx.listener(Self::cut)}
                on_action={cx.listener(Self::copy)}
                onMouseDown={(MouseButton::Left, cx.listener(Self::pressed))}
                onMouseUp={(MouseButton::Left, cx.listener(Self::released))}
                onMouseUpOut={(MouseButton::Left, cx.listener(Self::released))}
                onMouseMove={cx.listener(Self::dragged)}
            >
                {Line { field: cx.entity() }}
            </div>
        }
    }
}

/// The text itself, the selection behind it and the caret in it: drawn by
/// hand, because where the caret goes is a question for the shaped line and
/// nothing else has one.
struct Line {
    field: Entity<Field>,
}

struct Shaped {
    line: ShapedLine,
    selection: Option<Bounds<Pixels>>,
    caret: Option<Bounds<Pixels>>,
}

impl IntoElement for Line {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Line {
    type RequestLayoutState = ();
    type PrepaintState = Shaped;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Shaped {
        let field = self.field.read(cx);
        let style = window.text_style();
        let empty = field.text.is_empty();
        let shown: SharedString = if empty { field.placeholder.clone() } else { field.shown().into() };

        let run = TextRun {
            len: shown.len(),
            font: style.font(),
            color: if empty { theme::text_faint() } else { style.color },
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        // What is still being composed is underlined, which is how every
        // other text field on the desktop says so.
        let runs = match field.marked.as_ref().filter(|_| !empty) {
            Some(marked) => {
                let (from, to) = (field.shown_offset(marked.start), field.shown_offset(marked.end));
                let composing = UnderlineStyle { color: Some(run.color), thickness: px(1.), wavy: false };
                vec![
                    TextRun { len: from, ..run.clone() },
                    TextRun { len: to - from, underline: Some(composing), ..run.clone() },
                    TextRun { len: shown.len() - to, ..run },
                ]
                .into_iter()
                .filter(|run| run.len > 0)
                .collect()
            }
            None => vec![run],
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window.text_system().shape_line(shown, font_size, &runs, None);

        let x = |offset: usize| bounds.left() + if empty { px(0.) } else { line.x_for_index(field.shown_offset(offset)) };
        let (selection, caret) = if field.selection.is_empty() {
            let caret = Bounds::new(point(x(field.caret()), bounds.top()), size(px(1.5), bounds.size.height));
            (None, field.caret_shown.then_some(caret))
        } else {
            let selected = Bounds::from_corners(
                point(x(field.selection.start), bounds.top()),
                point(x(field.selection.end), bounds.bottom()),
            );
            (Some(selected), None)
        };
        Shaped { line, selection, caret }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        shaped: &mut Shaped,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.field.read(cx).focus.clone();
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.field.clone()), cx);

        if let Some(selection) = shaped.selection.take() {
            window.paint_quad(fill(selection, theme::white(0.16)));
        }
        let _ = shaped.line.paint(bounds.origin, window.line_height(), gpui::TextAlign::Left, None, window, cx);
        if focus.is_focused(window)
            && let Some(caret) = shaped.caret.take()
        {
            window.paint_quad(fill(caret, theme::accent()));
        }

        let (line, focused) = (shaped.line.clone(), focus.is_focused(window));
        self.field.update(cx, |field, _| {
            field.line = Some(line);
            field.bounds = Some(bounds);
            field.focused = focused;
        });
    }
}
