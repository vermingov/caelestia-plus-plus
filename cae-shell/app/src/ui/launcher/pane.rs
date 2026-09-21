//! The pane: the field, what it found, and which of those is chosen.

use std::time::Duration;

use cae_core::launcher as backend;
use gpui::{
    App, AppContext, Context, Entity, Focusable, FontWeight, IntoElement, KeyBinding,
    MouseButton, Pixels,
    Render, RetainAllImageCache, ScrollHandle, SharedString, Size, Styled, Task, WeakEntity, Window, actions, div, prelude::*, px,
};

use super::rows::{self, ROW};
use super::{Index, Launchers};
use crate::ui::field::{self, Field, FieldEvent};
use crate::ui::glyph::glyph;
use crate::ui::{pointer, rsx};
use crate::ease::{Curve, Tween};
use crate::theme;

actions!(launcher, [Dismiss, Next, Previous, Run, RunElsewhere, Complete]);

/// What the keys do while the launcher is open. Once, at startup.
pub fn bind_keys(cx: &mut App) {
    const CONTEXT: Option<&str> = Some("Launcher");
    cx.bind_keys([
        KeyBinding::new("escape", Dismiss, CONTEXT),
        KeyBinding::new("down", Next, CONTEXT),
        KeyBinding::new("up", Previous, CONTEXT),
        // The vim keys the old launcher answered to, so that muscle memory
        // carries over.
        KeyBinding::new("ctrl-j", Next, CONTEXT),
        KeyBinding::new("ctrl-n", Next, CONTEXT),
        KeyBinding::new("ctrl-k", Previous, CONTEXT),
        KeyBinding::new("ctrl-p", Previous, CONTEXT),
        KeyBinding::new("enter", Run, CONTEXT),
        KeyBinding::new("ctrl-enter", RunElsewhere, CONTEXT),
        KeyBinding::new("tab", Complete, CONTEXT),
    ]);
}

// The pane's parts are fixed heights, so that how tall it should be is
// arithmetic rather than something measured a frame late.
const SEARCH: Pixels = px(58.);
const RULE: Pixels = px(1.);
const SECTION: Pixels = px(31.);
const LIST_FOOT: Pixels = px(6.);
const EMPTY: Pixels = px(106.);
const FOOTER: Pixels = px(40.);
const PAD: Pixels = px(16.);

const RESIZE: Duration = Duration::from_millis(190);
const TRAVEL: Duration = Duration::from_millis(210);
/// The calculator is another program, so it waits for a pause in the typing
/// rather than being started once a character.
const CALCULATOR_PAUSE: Duration = Duration::from_millis(90);

pub struct Pane {
    index: Index,
    launchers: WeakEntity<Launchers>,
    field: Entity<Field>,
    found: backend::Results,
    /// The query `found` is the answer to, which is not always the one in the
    /// field: the calculator may still be on its way back.
    answered: String,
    selected: usize,
    /// The selection, as one pane that travels rather than a fill that swaps
    /// between rows: a highlight that moves reads as one thing sliding, where
    /// a background changing rows reads as two rows blinking.
    highlight: Tween,
    height: Tween,
    list: ScrollHandle,
    calculating: Option<Task<()>>,
    /// Nothing has been drawn yet, so whatever comes first arrives rather
    /// than resizes.
    arriving: bool,
    /// The wallpapers the strip has shown, which go when the pane does. The
    /// application's own cache would keep every one of them, decoded, until
    /// the shell was restarted. Icons are left to it: there are only so many,
    /// and they are wanted again the next time a key is pressed.
    wallpapers: Entity<RetainAllImageCache>,
}

impl Pane {
    pub fn new(
        index: Index,
        launchers: WeakEntity<Launchers>,
        query: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Pane {
        let field = cx.new(|cx| Field::new("Search for apps and commands…", cx));
        cx.subscribe(&field, |pane: &mut Pane, _, _: &FieldEvent, cx| pane.search(cx)).detach();
        window.focus(&field.focus_handle(cx), cx);

        let mut pane = Pane {
            index,
            launchers,
            field,
            found: empty(),
            answered: String::new(),
            selected: 0,
            highlight: Tween::still(0.),
            height: Tween::still(0.),
            list: ScrollHandle::new(),
            calculating: None,
            arriving: true,
            wallpapers: RetainAllImageCache::new(cx),
        };
        if query.is_empty() { pane.search(cx) } else { pane.fill(query, cx) }
        pane
    }

    /// Puts text in the field as though it had been typed: a mode a keybind
    /// asked for, a completion, somewhere an action leads.
    pub fn fill(&mut self, text: String, cx: &mut Context<Self>) {
        self.field.update(cx, |field, cx| field.set_text(text, cx));
    }

    fn query(&self, cx: &App) -> String {
        self.field.read(cx).text().to_string()
    }

    /// Ranks again, on every keystroke and at once: ranking is microseconds,
    /// and a launcher that lags the keyboard is the one thing it may not do.
    fn search(&mut self, cx: &mut Context<Self>) {
        let asked = self.query(cx);
        if !asked.contains("calc ") {
            self.calculating = None;
            let found = self.index.lock().map(|index| index.search(&asked)).unwrap_or_else(|_| empty());
            return self.show(asked, found, cx);
        }

        let index = self.index.clone();
        self.calculating = Some(cx.spawn(async move |pane, cx| {
            cx.background_executor().timer(CALCULATOR_PAUSE).await;
            let (query, searching) = (asked.clone(), index.clone());
            let found = cx
                .background_spawn(async move { searching.lock().map(|index| index.search(&query)).ok() })
                .await;
            let _ = pane.update(cx, |pane, cx| {
                // A slower answer to an earlier question is no answer.
                if let Some(found) = found.filter(|_| pane.query(cx) == asked) {
                    pane.show(asked, found, cx);
                }
            });
        }));
    }

    fn show(&mut self, asked: String, found: backend::Results, cx: &mut Context<Self>) {
        self.found = found;
        self.answered = asked;
        // A new list is read from the top. The old place means nothing in
        // it: every keystroke re-ranks, row three is a different thing now,
        // and the best match is the first.
        self.selected = 0;
        self.highlight.jump(0.);
        self.list.set_offset(gpui::point(px(0.), px(0.)));

        let strip = self.found.mode == "wallpapers";
        self.field.update(cx, |field, _| field.arrows_navigate = strip);
        cx.notify();
    }

    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        let count = self.found.entries.len() as isize;
        if count > 0 {
            // Wraps, so holding a key walks the whole list and comes round.
            self.select((self.selected as isize + by).rem_euclid(count) as usize, cx);
            self.list.scroll_to_item(self.selected);
        }
    }

    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        if index != self.selected && index < self.found.entries.len() {
            self.selected = index;
            self.highlight.go(index as f32, TRAVEL, Curve::Settle);
            cx.notify();
        }
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.remove_window();
        let _ = self.launchers.update(cx, |launchers, cx| launchers.gone(cx));
    }

    /// What Enter does to the chosen row.
    fn run(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Enter can outrun the search it follows. Typed quickly, it arrives
        // while the list is still the answer to an earlier keystroke, and the
        // top of that list is not what was asked for.
        let query = self.query(cx);
        if self.answered != query {
            let found = self.index.lock().map(|index| index.search(&query)).unwrap_or_else(|_| empty());
            self.show(query.clone(), found, cx);
        }
        let Some(id) = self.found.entries.get(self.selected).map(|entry| entry.id.clone()) else { return };

        // Off this thread: most of what a row does is start a program, but a
        // scheme or a wallpaper is another program run to its end.
        let index = self.index.clone();
        cx.spawn_in(window, async move |pane, cx| {
            let next = cx
                .background_spawn(async move { index.lock().map(|mut index| index.activate(&query, &id)).unwrap_or_default() })
                .await;
            let _ = pane.update_in(cx, |pane, window, cx| {
                // Empty is done. Anything else is where the row leads: an
                // action that completes, a sum with nothing to copy yet.
                if next.is_empty() { pane.close(window, cx) } else { pane.fill(next, cx) }
            });
        })
        .detach();
    }

    /// A sum opened in a real calculator, which is the one thing a one-line
    /// answer cannot do. Anywhere else it is Enter.
    fn run_elsewhere(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.found.mode != "calc" {
            return self.run(window, cx);
        }
        let sum = self.found.entries.get(self.selected).map(|entry| entry.comment.clone()).unwrap_or_default();
        if self.index.lock().is_ok_and(|index| index.open_in_calculator(&sum)) {
            self.close(window, cx);
        }
    }

    /// What Tab would put in the field: the chosen row rather than running
    /// it, which is how to say "this one, but let me keep typing".
    fn completion(&self, cx: &App) -> Option<String> {
        let entry = self.found.entries.get(self.selected).filter(|_| self.found.mode != "calc")?;
        let completion = if entry.completion.is_empty() { &entry.name } else { &entry.completion };
        (!completion.is_empty() && *completion != self.query(cx)).then(|| completion.clone())
    }

    /// How tall the pane should be for what it is showing.
    fn fitted(&self, screen: Size<Pixels>) -> Pixels {
        let body = if self.found.entries.is_empty() {
            EMPTY
        } else if self.found.mode == "wallpapers" {
            SECTION + px(16.) + rows::tile_height(tile_width(screen))
        } else {
            SECTION + self.list_height(screen) + LIST_FOOT
        };
        SEARCH + RULE + body + RULE + FOOTER
    }

    /// As many rows as the config allows, or as many as fit the screen,
    /// whichever is fewer. After that it scrolls.
    fn list_height(&self, screen: Size<Pixels>) -> Pixels {
        let rows = self.found.entries.len().min(self.found.max_shown) as f32;
        (ROW * rows).min(screen.height * 0.44)
    }
}

fn empty() -> backend::Results {
    backend::Results {
        mode: "apps".into(),
        max_shown: 8,
        label: "Applications".into(),
        action: "Open".into(),
        entries: Vec::new(),
    }
}

// Sized from the output rather than fixed. The short side rather than the
// width is what keeps the proportions on an ultrawide, where a share of the
// width would stretch the pane into a letterbox; the share of the width is
// only a guard for an output that is genuinely narrow.

fn pane_width(screen: Size<Pixels>, wide: bool) -> Pixels {
    let short = screen.width.min(screen.height);
    if wide { (short * 0.96).min(screen.width * 0.94) } else { (short * 0.66).min(screen.width * 0.92) }
}

/// A fifth of the way down, wherever the eye already is.
fn pane_top(screen: Size<Pixels>) -> Pixels {
    (screen.height * 0.2).clamp(px(56.), px(280.))
}

/// Tiles scale with the pane, so the strip shows about five of them whatever
/// the output is.
fn tile_width(screen: Size<Pixels>) -> Pixels {
    (screen.width.min(screen.height) * 0.18).clamp(px(150.), px(260.))
}

impl Render for Pane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let screen = window.viewport_size();
        let strip = self.found.mode == "wallpapers";
        let count = self.found.entries.len();

        // The pane's height is what moves, not the window's: a list that
        // grows by a row is a repaint, where a surface that grows by a row is
        // a round trip to the compositor and a frame at the wrong size.
        let fitted = f32::from(self.fitted(screen));
        if std::mem::take(&mut self.arriving) {
            self.height.jump(fitted);
        } else if fitted != self.height.target() {
            self.height.go(fitted, RESIZE, Curve::Settle);
        }
        if !(self.height.done() && self.highlight.done()) {
            window.request_animation_frame();
        }
        let (height, highlight) = (self.height.value(), self.highlight.value());

        let tally = if count == 1 { "1 result".to_string() } else { format!("{count} results") };
        let calculated = self.found.mode == "calc"
            && self.found.entries.get(self.selected).is_some_and(|entry| !entry.id.is_empty());

        let results = if count == 0 {
            rsx! {
                <div class="flex flex-col flex-none items-center justify-center gap-[4px]" h={EMPTY}>
                    <div class="font-medium" text_size={px(14.)} text_color={theme::text_dim()}>{"No results"}</div>
                    <div text_size={px(12.)} text_color={theme::text_faint()}>{"Try a different search"}</div>
                </div>
            }
            .into_any_element()
        } else if strip {
            let width = tile_width(screen);
            rsx! {
                // A box of its own for the cache: a box with an id cannot carry one.
                <div class="flex flex-col flex-none" image_cache={self.wallpapers.clone()}>
                    <div
                        id="wallpapers"
                        class="flex flex-none gap-[10px] px-[12px] pt-[4px] pb-[12px] overflow-x-scroll"
                        track_scroll={&self.list}
                    >
                        {for (index, entry) in self.found.entries.iter().enumerate() {
                            <div
                                base={rows::tile(entry, width, index == self.selected)}
                                id={("tile", index)}
                                onMouseMove={cx.listener(move |pane, _, _, cx| pane.select(index, cx))}
                                onClick={cx.listener(move |pane, _, window, cx| {
                                    pane.select(index, cx);
                                    pane.run(window, cx);
                                })}
                            />
                        }}
                    </div>
                </div>
            }
            .into_any_element()
        } else {
            let travelling = rsx! {
                <div
                    class="absolute"
                    top={ROW * highlight}
                    left={px(10.)}
                    right={px(10.)}
                    h={ROW}
                    rounded={px(9.)}
                    bg={theme::highlight()}
                    shadow={theme::highlight_shadows()}
                />
            };

            rsx! {
                <div
                    id="results"
                    class="relative flex flex-col flex-none px-[6px] overflow-y-scroll"
                    h={self.list_height(screen) + LIST_FOOT}
                    track_scroll={&self.list}
                >
                    {travelling}
                    {for (index, entry) in self.found.entries.iter().enumerate() {
                        <div
                            base={rows::row(entry)}
                            id={("row", index)}
                            // Only a pointer that moved chooses a row. One
                            // that is standing still while the list changes
                            // under it has not chosen anything.
                            onMouseMove={cx.listener(move |pane, _, _, cx| pane.select(index, cx))}
                            onClick={cx.listener(move |pane, _, window, cx| {
                                pane.select(index, cx);
                                pane.run(window, cx);
                            })}
                        />
                    }}
                </div>
            }
            .into_any_element()
        };

        let glass = rsx! {
            <div
                id="glass"
                class="relative flex flex-col flex-none overflow-hidden"
                // Nothing under the pane hears the pointer, the stage
                // included: a click on the pane is not a click outside it.
                occlude
                w={pane_width(screen, strip)}
                h={px(height)}
                rounded={px(15.)}
                bg={theme::pane()}
                shadow={theme::pane_shadows()}
                text_color={theme::text()}
            >
                // The specular: a short streak just inside the top edge, a
                // third of the way across, gone well before the corners.
                <div class="absolute flex h-[1px]" top={px(1.)} left={gpui::relative(0.12)} w={gpui::relative(0.46)}>
                    <div class="flex-1 h-full" bg={theme::streak(true)} />
                    <div class="flex-1 h-full" bg={theme::streak(false)} />
                </div>

                <div class="flex flex-none items-center gap-[12px]" h={SEARCH} px={PAD} text_size={px(16.)}>
                    {glyph("search", px(20.)).opacity(0.5)}
                    {self.field.clone()}
                    // What Tab would do, named rather than guessed at.
                    {...self.completion(cx).map(|_| rsx! {
                        <div class="flex flex-none items-center gap-[7px]" text_size={px(12.)} text_color={theme::text_faint()}>
                            {"Autocomplete"}
                            {rows::key("Tab")}
                        </div>
                    })}
                </div>
                <div class="flex-none" h={RULE} bg={theme::white(0.06)} />

                {...(count > 0).then(|| rsx! {
                    <div class="flex flex-none items-end pb-[5px]" h={SECTION} px={PAD} text_size={px(12.)} text_color={theme::text_faint()}>
                        {SharedString::from(self.found.label.clone())}
                    </div>
                })}
                {results}

                <div class="flex-none" h={RULE} bg={theme::white(0.06)} />
                <div class="flex flex-none items-center justify-between" h={FOOTER} px={PAD} text_size={px(11.)} text_color={theme::text_faint()}>
                    {if calculated {
                        rsx! {
                            <div class="flex items-center gap-[4px]">
                                {rows::key("Ctrl")}
                                {rows::key("↵")}
                                <div class="pl-[4px]">{"to open in a calculator"}</div>
                            </div>
                        }
                    } else {
                        rsx! { <div class="flex items-center">{tally}</div> }
                    }}
                    <div class="flex items-center gap-[8px]" text_color={theme::text_dim()}>
                        {SharedString::from(self.found.action.clone())}
                        {rows::key("↵")}
                    </div>
                </div>
            </div>
        };

        rsx! {
            <div
                id="stage"
                class="relative size-full flex flex-col items-center"
                key_context="Launcher"
                font_family={theme::FONT}
                pt={pane_top(screen)}
                // Anywhere outside the pane closes it. The surface covers the
                // screen so that the pane can change size without it, which
                // makes the empty space around the pane ours to answer for.
                onMouseDown={(MouseButton::Left, cx.listener(|pane, _, window, cx| pane.close(window, cx)))}
                on_action={cx.listener(|pane, _: &Dismiss, window, cx| pane.close(window, cx))}
                on_action={cx.listener(|pane, _: &Next, _, cx| pane.step(1, cx))}
                on_action={cx.listener(|pane, _: &Previous, _, cx| pane.step(-1, cx))}
                on_action={cx.listener(|pane, _: &Run, window, cx| pane.run(window, cx))}
                on_action={cx.listener(|pane, _: &RunElsewhere, window, cx| pane.run_elsewhere(window, cx))}
                on_action={cx.listener(|pane, _: &Complete, _, cx| {
                    if let Some(completion) = pane.completion(cx) {
                        pane.fill(completion, cx);
                    }
                })}
                // A strip of wallpapers is walked sideways, with the keys a
                // caret would otherwise have: the field passes them up.
                on_action={cx.listener(|pane, _: &field::Right, _, cx| pane.step(1, cx))}
                on_action={cx.listener(|pane, _: &field::Left, _, cx| pane.step(-1, cx))}
            >
                {glass}
                {pointer::see_out()}
            </div>
        }
    }
}
