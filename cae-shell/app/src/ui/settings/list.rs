//! The page that is only a list of settings, whichever list it is.

use gpui::{AnyElement, Context, Entity, Focusable, IntoElement, Render, Window, div, prelude::*, px};
use serde_json::Value;

use super::frame::{Commit, Reach};
use super::rows::{figure, figure_text, leads, page, pressable, row, rule, section_title, step, typed};
use super::schema::{Control, Item, Row, Section};
use super::store::Source;
use crate::ui::controls::{chip, switch, warning};
use crate::ui::field::Field;
use crate::ui::rsx;

pub struct ListPage {
    reach: Reach,
    sections: &'static [Section],
    /// Whether every row here is one of the compositor's, which a machine
    /// without the helper cannot show at all.
    compositor: bool,
    /// The box for each row that is typed into, by where the row keeps its
    /// value. Made once: a field is somewhere a caret lives.
    fields: Vec<(Source, Entity<Field>)>,
}

impl ListPage {
    pub fn new(sections: &'static [Section], reach: &Reach, window: &mut Window, cx: &mut Context<Self>) -> ListPage {
        cx.observe(&reach.store, |page: &mut ListPage, _, cx| {
            page.follow(cx);
            cx.notify();
        })
        .detach();

        let typed_rows = sections.iter().flat_map(|section| section.items).filter_map(|item| match item {
            Item::Row(Row { at, control: Control::Text { placeholder }, .. }) => Some((*at, *placeholder)),
            _ => None,
        });
        let fields: Vec<_> = typed_rows
            .map(|(at, placeholder)| {
                let field = cx.new(|cx| Field::new(placeholder, cx));
                // Leaving a box is saying what is in it. While it is being
                // typed into it says nothing: half a keybind is not one.
                cx.on_blur(&field.focus_handle(cx), window, move |page: &mut ListPage, _, cx| page.commit(at, cx)).detach();
                (at, field)
            })
            .collect();

        let compositor = sections
            .iter()
            .flat_map(|section| section.items)
            .any(|item| matches!(item, Item::Row(Row { at: Source::Knob(_), .. })));
        let mut page = ListPage { reach: reach.clone(), sections, compositor, fields };
        page.follow(cx);
        page
    }

    /// Puts what the store says in every box that is not being typed into.
    /// The store learns things late: the compositor's values arrive after
    /// the page is up, and the other shell writes the same files.
    fn follow(&mut self, cx: &mut Context<Self>) {
        for (at, field) in &self.fields {
            let kept = self.reach.store.read(cx).text(*at, "");
            field.update(cx, |field, cx| {
                if !field.is_focused() && field.text() != kept {
                    field.set_text(kept, cx);
                }
            });
        }
    }

    fn commit(&mut self, at: Source, cx: &mut Context<Self>) {
        let Some((_, field)) = self.fields.iter().find(|(source, _)| *source == at) else { return };
        let said = field.read(cx).text().trim().to_string();
        self.reach.store.update(cx, |store, cx| store.set(at, said.into(), cx));
    }

    fn set(&mut self, at: Source, value: Value, cx: &mut Context<Self>) {
        self.reach.store.update(cx, |store, cx| store.set(at, value, cx));
    }

    fn item(&self, index: usize, item: &'static Item, cx: &mut Context<Self>) -> AnyElement {
        let store = self.reach.store.read(cx);
        match item {
            Item::Leads { glyph, label, to, says } => {
                let nav = self.reach.nav.clone();
                rsx! {
                    <div
                        base={leads(glyph, *label, says(store))}
                        id={("leads", index)}
                        onClick={move |_, window, cx| nav.go(to(), window, cx)}
                    />
                }
                .into_any_element()
            }
            Item::Row(setting) => {
                let live = setting.needs.is_none_or(|(switch, otherwise)| store.flag(switch, otherwise));
                self.setting(index, setting, live, cx)
            }
        }
    }

    fn setting(&self, index: usize, setting: &'static Row, live: bool, cx: &mut Context<Self>) -> AnyElement {
        let store = self.reach.store.read(cx);
        let at = setting.at;
        let line = row(setting.label, setting.note, live);

        match setting.control {
            Control::Switch { otherwise } => {
                let on = store.flag(at, otherwise);
                rsx! {
                    <div
                        base={if live { pressable(line) } else { line }}
                        id={("switch", index)}
                        when={(live, |row| row.on_click(cx.listener(move |page, _, _, cx| page.set(at, (!on).into(), cx))))}
                    >
                        {switch(on)}
                    </div>
                }
                .into_any_element()
            }
            Control::Stepper { otherwise, from, to, step: by, shown_times, unit } => {
                let shown = store.number(at, otherwise) * shown_times;
                // Kept as what it is, not as what it is shown as: a count
                // stays a whole number in the file, a fraction a fraction.
                let moved = move |towards: f64| {
                    let next = (shown + towards * by).clamp(from, to) / shown_times;
                    if shown_times == 1. && by.fract() == 0. { Value::from(next.round() as i64) } else { Value::from(next) }
                };
                let (lower, raise) = (live && shown > from, live && shown < to);
                rsx! {
                    <div base={line}>
                        <div class="flex flex-none items-center gap-[2px]">
                            <div
                                base={step("remove", lower)}
                                id={("lower", index)}
                                when={(lower, |end| end.on_click(cx.listener(move |page, _, _, cx| page.set(at, moved(-1.), cx))))}
                            />
                            {figure(figure_text(shown, by, unit))}
                            <div
                                base={step("add", raise)}
                                id={("raise", index)}
                                when={(raise, |end| end.on_click(cx.listener(move |page, _, _, cx| page.set(at, moved(1.), cx))))}
                            />
                        </div>
                    </div>
                }
                .into_any_element()
            }
            Control::Choice { otherwise, options } => {
                let kept = store.get(at);
                let chosen = options.iter().position(|(_, pick)| Some(&pick.to_json()) == kept).unwrap_or(otherwise);
                rsx! {
                    <div base={line}>
                        <div class="flex flex-wrap justify-end items-center gap-[4px] min-w-[0px]">
                            {for (option, (label, pick)) in options.iter().enumerate() {
                                <div
                                    base={chip(*label, option == chosen)}
                                    id={("choice", index * 16 + option)}
                                    when={(live, |chip| chip.on_click(cx.listener(move |page, _, _, cx| page.set(at, pick.to_json(), cx))))}
                                />
                            }}
                        </div>
                    </div>
                }
                .into_any_element()
            }
            Control::Text { .. } => {
                let field = self.fields.iter().find(|(source, _)| *source == at).map(|(_, field)| field);
                let Some(field) = field else { return line.into_any_element() };
                let focused = field.read(cx).is_focused();
                rsx! { <div base={line}>{typed(field, focused)}</div> }.into_any_element()
            }
        }
    }
}

impl Render for ListPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.compositor && self.reach.store.read(cx).knows_compositor() == Some(false) {
            return rsx! {
                <div base={page()}>
                    {warning("These are settings of the Hyprland config Caelestia++ installs, and this machine's is not one it knows its way around.")}
                </div>
            };
        }

        let mut index = 0;
        let mut body: Vec<AnyElement> = Vec::new();
        for (position, section) in self.sections.iter().enumerate() {
            if !section.title.is_empty() {
                body.push(section_title(section.title, position == 0).into_any_element());
            } else if position > 0 {
                body.push(rsx! { <div class="flex-none h-[18px]" /> }.into_any_element());
            }
            for (place, item) in section.items.iter().enumerate() {
                if place > 0 {
                    body.push(rule().into_any_element());
                }
                body.push(self.item(index, item, cx));
                index += 1;
            }
        }

        rsx! {
            <div
                base={page()}
                // Enter says what is in the box being typed into, by
                // leaving it: the one way a box is ever committed.
                on_action={cx.listener(|page, _: &Commit, window, cx| page.reach.nav.settle(window, cx))}
            >
                {...body}
            </div>
        }
    }
}

