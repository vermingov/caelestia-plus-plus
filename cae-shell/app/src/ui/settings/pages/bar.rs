//! The bar: what it carries.
//!
//! Most of a bar's settings are keys like any other and are listed like any
//! other. What it carries is not: it is one list in the config, in the order
//! the bar is drawn in, and switching something off is editing that list.

use cae_core::logo;
use gpui::{Context, Entity, IntoElement, Render, Window, div, prelude::*, px};

use super::super::frame::Reach;
use super::super::list::ListPage;
use super::super::rows::{page, pressable, row, rule, section_title};
use super::super::schema;
use super::super::store::shell;
use crate::ui::controls::switch;
use crate::ui::rsx;

/// The entries that can be switched, by what the config calls them, in the
/// order they sit on the bar. The mark, the window title and the monitor are
/// not here: those are switched by preferences of their own, below.
pub const ENTRIES: [(&str, &str, &str); 10] = [
    ("workspaces", "Workspaces", ""),
    ("specials", "Special workspaces", "Shown while one has something in it"),
    ("media", "Now playing", "Shown while something is playing"),
    ("visualiser", "Visualiser", "The spectrum along the foot of the bar"),
    ("firewall", "Security", "The shield, and what is waiting for a verdict"),
    ("features", "Features", "The wrench, and which modes are on"),
    ("tray", "Tray", ""),
    ("statusIcons", "Status icons", ""),
    ("clock", "Clock", ""),
    ("power", "Power", ""),
];

pub struct Bar {
    reach: Reach,
    rest: Entity<ListPage>,
}

impl Bar {
    pub fn new(reach: &Reach, window: &mut Window, cx: &mut Context<Self>) -> Bar {
        cx.observe(&reach.store, |_, _, cx| cx.notify()).detach();
        let rest = cx.new(|cx| ListPage::new(schema::BAR, reach, window, cx));
        Bar { reach: reach.clone(), rest }
    }

    fn carry(&mut self, id: &'static str, carried: bool, cx: &mut Context<Self>) {
        self.reach.store.update(cx, |store, cx| {
            let entries = logo::entries_with(store.get(shell("bar.entries")), id, carried);
            store.set(shell("bar.entries"), entries, cx);
        });
    }
}

impl Render for Bar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let listed = logo::entries_of(self.reach.store.read(cx).get(shell("bar.entries")));
        let carried = |id: &str| listed.iter().any(|(entry, enabled)| entry == id && *enabled);

        rsx! {
            <div base={page()} class="pb-[0px]">
                {section_title("On the bar", true)}
                {for (index, (id, label, note)) in ENTRIES.into_iter().enumerate() {
                    <div class="flex flex-col" key={index}>
                        {...(index > 0).then(rule)}
                        <div
                            base={pressable(row(label, note, true))}
                            id={("carries", index)}
                            onClick={cx.listener({
                                let on = carried(id);
                                move |bar, _, _, cx| bar.carry(id, !on, cx)
                            })}
                        >
                            {switch(carried(id))}
                        </div>
                    </div>
                }}
                <div class="flex-none h-[26px]" />
                {self.rest.clone()}
            </div>
        }
    }
}
