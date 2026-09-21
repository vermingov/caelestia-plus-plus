//! A tray item's own menu, drawn here rather than by the application: a menu
//! window of the application's own has nowhere to go on a layer surface.

use cae_core::tray;
use gpui::{AppContext, Context, Div, EventEmitter, IntoElement, Render, SharedString, Styled, Window, div, prelude::*, px};

use super::Finished;
use super::pieces::trailing;
use crate::theme;
use crate::ui::glyph::glyph;
use crate::ui::rsx;

pub struct Menu {
    key: SharedString,
    /// Asked for when the menu opens: an application rebuilds its menu as it
    /// pleases, and the one from the last time it was open is nobody's.
    entries: Vec<tray::MenuEntry>,
    asked: bool,
}

impl EventEmitter<Finished> for Menu {}

impl Menu {
    pub fn new(key: SharedString, cx: &mut Context<Self>) -> Menu {
        let asking = key.clone();
        cx.spawn(async move |menu, cx| {
            let entries = cx.background_spawn(async move { tray::menu(&asking) }).await;
            let _ = menu.update(cx, |menu, cx| {
                (menu.entries, menu.asked) = (entries, true);
                cx.notify();
            });
        })
        .detach();
        Menu { key, entries: Vec::new(), asked: false }
    }

    fn choose(&mut self, id: i32, cx: &mut Context<Self>) {
        let key = self.key.clone();
        cx.background_spawn(async move { tray::click(&key, id) }).detach();
        cx.emit(Finished);
    }
}

/// One thing in a menu. A submenu's entries are already in hand, so they are
/// shown under their parent, indented, rather than in a second panel to the
/// side that has nowhere to go on a narrow screen.
fn entry(entry: &tray::MenuEntry, nested: bool) -> Div {
    let pressable = entry.enabled && entry.children.is_empty();
    rsx! {
        <div
            class="flex flex-none items-center gap-[10px] min-h-[30px] pr-[8px]"
            pl={px(if nested { 22. } else { 8. })}
            rounded={px(7.)}
            text_size={px(if nested { 11.5 } else { 12. })}
            text_color={if entry.checked { theme::text() } else { theme::text_dim() }}
            when={(!entry.enabled, |entry| entry.opacity(0.4))}
            when={(entry.checked, |entry| entry.bg(theme::white(0.09)).shadow(theme::edge(0.08)))}
            when={(pressable && !entry.checked, |entry| {
                entry.cursor_pointer().hover(|style| style.bg(theme::white(0.055)).text_color(theme::text()))
            })}
        >
            {...(entry.kind == "checkmark")
                .then(|| glyph(if entry.checked { "check_box" } else { "check_box_outline_blank" }, px(15.)))}
            <div class="flex-1 min-w-[0px] truncate">{SharedString::from(entry.label.clone())}</div>
            {...(!entry.children.is_empty()).then(|| trailing("›"))}
        </div>
    }
}

impl Render for Menu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Flattened, a submenu's entries straight after their parent.
        let rows = self.entries.iter().flat_map(|parent| {
            std::iter::once((parent, false)).chain(parent.children.iter().map(|child| (child, true)))
        });

        rsx! {
            <div class="flex flex-col gap-[1px] min-h-[0px]">
                {for (index, (item, nested)) in rows.enumerate() {
                    {if item.kind == "separator" {
                        rsx! { <div class="flex-none h-[1px] mx-[2px] my-[4px]" bg={theme::white(0.07)} /> }.into_any_element()
                    } else {
                        let (id, pressable) = (item.id, item.enabled && item.children.is_empty());
                        rsx! {
                            <div
                                base={entry(item, nested)}
                                id={("entry", index)}
                                onClick={cx.listener(move |menu, _, _, cx| if pressable { menu.choose(id, cx) })}
                            />
                        }
                        .into_any_element()
                    }}
                }}
                // An application with nothing to offer still gets an answer:
                // a menu that opens empty looks like one that failed to.
                {...(self.asked && self.entries.is_empty()).then(|| rsx! {
                    <div class="px-[8px] py-[6px]" text_size={px(12.)} text_color={theme::text_faint()}>{"No menu"}</div>
                })}
            </div>
        }
    }
}
