//! Applications: which ones the shell opens things with, and what the
//! launcher makes of each.

use std::path::PathBuf;
use std::sync::Arc;

use cae_core::gpus::{self, Gpu};
use cae_core::launcher::apps::{self, App};
use cae_core::launcher::config::matches_any;
use cae_core::launcher::icons::Icons;
use gpui::{AnyElement, AppContext, Context, Entity, Focusable, FontWeight, IntoElement, ObjectFit, Render, Window, div, img, prelude::*, px};
use serde_json::Value;

use super::super::Page;
use super::super::frame::Reach;
use super::super::rows::{chosen_mark, fact, leads, nothing, page, pressable, row, rule, searched, section_title};
use super::super::store::{Source, shell};
use crate::theme;
use crate::ui::controls::{chip, switch};
use crate::ui::field::{Field, FieldEvent};
use crate::ui::glyph::glyph;
use crate::ui::rsx;

/// As many applications as a list shows at once. A machine has hundreds, and
/// each row is an icon to find and decode: the rest are a few letters away.
const MOST_SHOWN: usize = 40;

/// Something the shell opens with a program the person chooses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opens {
    Terminal,
    Audio,
    Playback,
    Files,
}

impl Opens {
    const ALL: [Opens; 4] = [Opens::Terminal, Opens::Audio, Opens::Playback, Opens::Files];

    pub fn title(self) -> &'static str {
        match self {
            Opens::Terminal => "Terminal",
            Opens::Audio => "Audio mixer",
            Opens::Playback => "Media player",
            Opens::Files => "File manager",
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            Opens::Terminal => "terminal",
            Opens::Audio => "volume_up",
            Opens::Playback => "play_circle",
            Opens::Files => "folder",
        }
    }

    /// Where it is kept, and the program it is when nothing is.
    fn kept(self) -> (Source, &'static str) {
        match self {
            Opens::Terminal => (shell("general.apps.terminal"), "foot"),
            Opens::Audio => (shell("general.apps.audio"), "pavucontrol"),
            Opens::Playback => (shell("general.apps.playback"), "mpv"),
            Opens::Files => (shell("general.apps.explorer"), "thunar"),
        }
    }
}

/// A desktop entry's command line as the words of it, which is how the
/// config keeps a command. Quotes keep what is between them together.
fn words(exec: &str) -> Vec<String> {
    let (mut words, mut word, mut quote, mut any) = (Vec::new(), String::new(), None, false);
    for character in exec.chars() {
        match (quote, character) {
            (None, '"' | '\'') => (quote, any) = (Some(character), true),
            (Some(open), close) if open == close => quote = None,
            (None, space) if space.is_whitespace() => {
                if any {
                    words.push(std::mem::take(&mut word));
                    any = false;
                }
            }
            (_, other) => {
                word.push(other);
                any = true;
            }
        }
    }
    if any {
        words.push(word);
    }
    words
}

pub struct Apps {
    reach: Reach,
    /// Known once the helper has answered: a page about graphics cards is
    /// only worth the way to it on a machine with a choice of them.
    cards: usize,
}

impl Apps {
    pub fn new(reach: &Reach, cx: &mut Context<Self>) -> Apps {
        cx.observe(&reach.store, |_, _, cx| cx.notify()).detach();
        cx.spawn(async move |page, cx| {
            let cards = cx.background_spawn(async { gpus::list().len() }).await;
            let _ = page.update(cx, |page: &mut Apps, cx| {
                page.cards = cards;
                cx.notify();
            });
        })
        .detach();
        Apps { reach: reach.clone(), cards: 0 }
    }
}

impl Render for Apps {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.reach.store.read(cx);
        let opened: Vec<AnyElement> = Opens::ALL
            .into_iter()
            .enumerate()
            .map(|(index, opens)| {
                let (at, otherwise) = opens.kept();
                let command = store.words(at).filter(|command| !command.is_empty()).map_or(otherwise.to_string(), |command| command.join(" "));
                let nav = self.reach.nav.clone();
                rsx! {
                    <div class="flex flex-col flex-none">
                        {...(index > 0).then(rule)}
                        <div
                            base={leads(opens.glyph(), opens.title(), command)}
                            id={("opens", index)}
                            onClick={move |_, window, cx| nav.go(Page::OpensWith(opens), window, cx)}
                        />
                    </div>
                }
                .into_any_element()
            })
            .collect();
        let (to_all, to_cards) = (self.reach.nav.clone(), self.reach.nav.clone());

        rsx! {
            <div base={page()}>
                {section_title("What the shell opens things with", true)}
                {...opened}
                <div class="flex-none h-[22px]" />
                <div base={leads("apps", "All apps", "Favourites, and what the launcher hides")} id="all" onClick={move |_, window, cx| to_all.go(Page::AllApps, window, cx)} />
                {...(self.cards > 1).then(|| rsx! {
                    <div class="flex flex-col flex-none">
                        {rule()}
                        <div base={leads("memory", "Graphics cards", "Which card each app starts on")} id="cards" onClick={move |_, window, cx| to_cards.go(Page::AppGpus, window, cx)} />
                    </div>
                })}
            </div>
        }
    }
}

/// What a list of applications is for.
#[derive(Clone, Copy, PartialEq)]
pub enum Purpose {
    /// Looking at one: a row leads to the page about it.
    Browse,
    /// Choosing the program something opens with.
    Choose(Opens),
    /// Giving each a graphics card.
    Cards,
}

pub struct AppList {
    reach: Reach,
    purpose: Purpose,
    search: Entity<Field>,
    apps: Arc<Vec<App>>,
    /// Icon files by icon name, found for the rows that have been shown.
    icons: Vec<(String, Option<PathBuf>)>,
    cards: Vec<Gpu>,
    assigned: std::collections::BTreeMap<String, String>,
    looked: bool,
}

impl AppList {
    pub fn new(purpose: Purpose, reach: &Reach, window: &mut Window, cx: &mut Context<Self>) -> AppList {
        cx.observe(&reach.store, |_, _, cx| cx.notify()).detach();
        let search = cx.new(|cx| Field::new("Search the apps on this machine", cx));
        cx.subscribe(&search, |list: &mut AppList, _, _: &FieldEvent, cx| list.find_icons(cx)).detach();
        window.focus(&search.focus_handle(cx), cx);

        cx.spawn(async move |list, cx| {
            let found = cx
                .background_spawn(async move {
                    let cards = if purpose == Purpose::Cards { (gpus::list(), gpus::assignments()) } else { Default::default() };
                    (apps::load(), cards)
                })
                .await;
            let _ = list.update(cx, |list: &mut AppList, cx| {
                let (apps, (cards, assigned)) = found;
                (list.apps, list.cards, list.assigned, list.looked) = (Arc::new(apps), cards, assigned, true);
                list.find_icons(cx);
            });
        })
        .detach();

        AppList {
            reach: reach.clone(),
            purpose,
            search,
            apps: Arc::default(),
            icons: Vec::new(),
            cards: Vec::new(),
            assigned: Default::default(),
            looked: false,
        }
    }

    /// The applications the search leaves, best first: the ones the launcher
    /// pins, then by name.
    fn shown(&self, cx: &Context<Self>) -> Vec<&App> {
        let query = self.search.read(cx).text().trim().to_lowercase();
        let favourites = self.reach.store.read(cx).words(shell("launcher.favouriteApps")).unwrap_or_default();
        let mut shown: Vec<&App> = self.apps.iter().filter(|app| query.is_empty() || app.haystack.contains(&query)).collect();
        shown.sort_by_key(|app| (!matches_any(&favourites, &app.id), !self.assigned.contains_key(&app.id)));
        shown.truncate(MOST_SHOWN);
        shown
    }

    /// Finds the icon files for what is shown, away from the thread that
    /// draws: each is a walk of the icon theme's directories.
    fn find_icons(&mut self, cx: &mut Context<Self>) {
        cx.notify();
        let wanted: Vec<String> = self
            .shown(cx)
            .iter()
            .map(|app| app.icon.clone())
            .filter(|icon| !self.icons.iter().any(|(known, _)| known == icon))
            .collect();
        if wanted.is_empty() {
            return;
        }
        cx.spawn(async move |list, cx| {
            let found = cx
                .background_spawn(async move {
                    let icons = Icons::new();
                    wanted.into_iter().map(|icon| (icon.clone(), icons.resolve(&icon).map(PathBuf::from))).collect::<Vec<_>>()
                })
                .await;
            let _ = list.update(cx, |list: &mut AppList, cx| {
                list.icons.extend(found);
                cx.notify();
            });
        })
        .detach();
    }

    fn pressed(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(app) = self.apps.iter().find(|app| app.id == id) else { return };
        match self.purpose {
            Purpose::Browse => self.reach.nav.go(Page::App { id, name: app.name.clone() }, window, cx),
            Purpose::Choose(opens) => {
                let command: Vec<Value> = words(&app.exec).into_iter().map(Value::from).collect();
                self.reach.store.update(cx, |store, cx| store.set(opens.kept().0, command.into(), cx));
                self.reach.nav.back(window, cx);
            }
            Purpose::Cards => {}
        }
    }

    fn assign(&mut self, id: String, slot: Option<String>, cx: &mut Context<Self>) {
        match &slot {
            Some(slot) => self.assigned.insert(id.clone(), slot.clone()),
            None => self.assigned.remove(&id),
        };
        self.reach.store.update(cx, |store, _| store.run(format!("card {id}"), move || gpus::assign(&id, slot.as_deref())));
        cx.notify();
    }

    fn icon(&self, app: &App) -> AnyElement {
        let file = self.icons.iter().find(|(name, _)| *name == app.icon).and_then(|(_, file)| file.clone());
        match file {
            Some(file) => rsx! { <img src={file} class="flex-none size-[26px]" object_fit={ObjectFit::Contain} /> }.into_any_element(),
            None => rsx! { <div class="flex-none size-[26px]" /> }.into_any_element(),
        }
    }
}

impl Render for AppList {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.reach.store.read(cx);
        let favourites = store.words(shell("launcher.favouriteApps")).unwrap_or_default();
        let chosen = match self.purpose {
            Purpose::Choose(opens) => store.words(opens.kept().0).filter(|command| !command.is_empty()).unwrap_or_else(|| vec![opens.kept().1.to_string()]),
            _ => Vec::new(),
        };
        let (shown, all) = (self.shown(cx), self.apps.len());
        let more = self.looked && shown.len() == MOST_SHOWN && all > MOST_SHOWN;

        let rows: Vec<AnyElement> = shown
            .iter()
            .enumerate()
            .map(|(index, app)| {
                let id = app.id.clone();
                let is_chosen = !chosen.is_empty() && words(&app.exec) == chosen;
                let line = rsx! {
                    <div class="flex flex-none items-center gap-[13px] min-h-[48px] py-[7px]">
                        {self.icon(app)}
                        <div class="flex flex-col flex-1 gap-[2px] min-w-[0px]">
                            <div class="truncate" text_size={px(13.)}>{app.name.clone()}</div>
                            {...(!app.comment.is_empty()).then(|| rsx! {
                                <div class="truncate" text_size={px(11.5)} text_color={theme::text_faint()}>{app.comment.clone()}</div>
                            })}
                        </div>
                    </div>
                };
                let trailing: Vec<AnyElement> = match self.purpose {
                    Purpose::Cards => {
                        let given = self.assigned.get(&app.id);
                        let default = {
                            let id = app.id.clone();
                            rsx! { <div base={chip("Default", given.is_none())} id={("card", index * 8)} onClick={cx.listener(move |list, _, _, cx| list.assign(id.clone(), None, cx))} /> }
                        };
                        let cards = self.cards.iter().enumerate().map(|(card, gpu)| {
                            let (id, slot) = (app.id.clone(), gpu.slot.clone());
                            rsx! {
                                <div
                                    base={chip(gpu.name.clone(), given == Some(&gpu.slot))}
                                    id={("card", index * 8 + card + 1)}
                                    onClick={cx.listener(move |list, _, _, cx| list.assign(id.clone(), Some(slot.clone()), cx))}
                                />
                            }
                            .into_any_element()
                        });
                        std::iter::once(default.into_any_element()).chain(cards).collect()
                    }
                    _ => [
                        matches_any(&favourites, &app.id).then(|| glyph("favorite", px(16.)).text_color(theme::accent()).into_any_element()),
                        is_chosen.then(|| chosen_mark().into_any_element()),
                        (self.purpose == Purpose::Browse).then(|| glyph("chevron_right", px(18.)).text_color(theme::text_faint()).into_any_element()),
                    ]
                    .into_iter()
                    .flatten()
                    .collect(),
                };
                rsx! {
                    <div class="flex flex-col flex-none">
                        {...(index > 0).then(rule)}
                        <div
                            base={if self.purpose == Purpose::Cards { line } else { pressable(line) }}
                            id={("app", index)}
                            onClick={cx.listener(move |list, _, window, cx| list.pressed(id.clone(), window, cx))}
                        >
                            <div class="flex flex-none items-center gap-[6px]">{...trailing}</div>
                        </div>
                    </div>
                }
                .into_any_element()
            })
            .collect();
        let nothing_found = self.looked && rows.is_empty();

        rsx! {
            <div base={page()}>
                {searched(&self.search, self.search.read(cx).is_focused())}
                {...rows}
                {...nothing_found.then(|| nothing("search_off", "No app by that name"))}
                {...more.then(|| rsx! {
                    <div class="pt-[12px]" text_size={px(12.)} text_color={theme::text_faint()}>
                        {format!("The first {MOST_SHOWN} of {all}. A few letters in the search finds the rest")}
                    </div>
                })}
            </div>
        }
    }
}

/// One application: whether the launcher pins it or hides it, and what it
/// is on disk.
pub struct AppInfo {
    reach: Reach,
    id: String,
    app: Option<App>,
    icon: Option<PathBuf>,
}

impl AppInfo {
    pub fn new(id: String, reach: &Reach, cx: &mut Context<Self>) -> AppInfo {
        cx.observe(&reach.store, |_, _, cx| cx.notify()).detach();
        let wanted = id.clone();
        cx.spawn(async move |page, cx| {
            let found = cx
                .background_spawn(async move {
                    let app = apps::load().into_iter().find(|app| app.id == wanted)?;
                    let icon = Icons::new().resolve(&app.icon).map(PathBuf::from);
                    Some((app, icon))
                })
                .await;
            let _ = page.update(cx, |page: &mut AppInfo, cx| {
                if let Some((app, icon)) = found {
                    (page.app, page.icon) = (Some(app), icon);
                }
                cx.notify();
            });
        })
        .detach();
        AppInfo { reach: reach.clone(), id, app: None, icon: None }
    }

    /// Adds this application to one of the launcher's lists, or takes it out.
    fn list_in(&mut self, at: Source, listed: bool, cx: &mut Context<Self>) {
        let id = self.id.clone();
        self.reach.store.update(cx, |store, cx| {
            let mut list = store.words(at).unwrap_or_default();
            list.retain(|entry| *entry != id);
            if listed {
                list.push(id);
            }
            store.set(at, list.into_iter().map(Value::from).collect::<Vec<_>>().into(), cx);
        });
    }

    /// One of the two switches. A list can hold patterns as well as names,
    /// and an application a pattern matches cannot be taken out by name: the
    /// switch says so rather than appearing to do nothing.
    fn listed(&self, label: &'static str, note: &'static str, id: &'static str, at: Source, cx: &mut Context<Self>) -> AnyElement {
        let list = self.reach.store.read(cx).words(at).unwrap_or_default();
        let on = matches_any(&list, &self.id);
        let by_pattern = on && !list.contains(&self.id);
        let note = if by_pattern { "Matched by a pattern in the config file, which is where to change it" } else { note };
        rsx! {
            <div
                base={if by_pattern { row(label, note, false) } else { pressable(row(label, note, true)) }}
                id={id}
                when={(!by_pattern, |row| row.on_click(cx.listener(move |page, _, _, cx| page.list_in(at, !on, cx))))}
            >
                {switch(on)}
            </div>
        }
        .into_any_element()
    }
}

impl Render for AppInfo {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(app) = self.app.clone() else { return rsx! { <div base={page()} /> } };
        rsx! {
            <div base={page()}>
                <div class="flex flex-none items-center gap-[16px] pt-[4px] pb-[14px]">
                    {...self.icon.clone().map(|icon| rsx! { <img src={icon} class="flex-none size-[44px]" object_fit={ObjectFit::Contain} /> })}
                    <div class="flex flex-col gap-[3px] min-w-[0px]">
                        <div class="truncate" text_size={px(17.)} font_weight={FontWeight::MEDIUM}>{app.name.clone()}</div>
                        {...(!app.comment.is_empty()).then(|| rsx! {
                            <div class="truncate" text_size={px(12.)} text_color={theme::text_faint()}>{app.comment.clone()}</div>
                        })}
                    </div>
                </div>

                {section_title("In the launcher", true)}
                {self.listed("Favourite", "Kept at the top of the results", "favourite", shell("launcher.favouriteApps"), cx)}
                {rule()}
                {self.listed("Hidden", "Never in the results", "hidden", shell("launcher.hiddenApps"), cx)}

                {section_title("On disk", false)}
                {fact("Desktop entry", app.id.clone())}
                {rule()}
                {fact("Runs", app.exec.clone())}
            </div>
        }
    }
}

#[cfg(test)]
mod tests {
    use super::words;

    #[test]
    fn a_command_line_is_split_where_a_shell_would_split_it() {
        assert_eq!(words("kitty"), ["kitty"]);
        assert_eq!(words("  foot   --server  "), ["foot", "--server"]);
        assert_eq!(words(r#"sh -c "echo two words""#), ["sh", "-c", "echo two words"]);
        assert_eq!(words("env NAME='a b' run"), ["env", "NAME=a b", "run"]);
        assert_eq!(words(r#"open """#), ["open", ""], "an empty argument is still one");
        assert!(words("").is_empty());
    }
}
