//! The window's frame: the pages down the side, and the one that is open.

use std::time::Duration;

use gpui::{
    AnyView, App, AppContext, Context, Entity, FocusHandle, Focusable, FontWeight, IntoElement, KeyBinding, MouseButton,
    Pixels, Render, ScrollHandle, SharedString, Styled, WeakEntity, Window, actions, div, prelude::*, px,
};

use super::list::ListPage;
use super::store::Store;
use super::{Page, pages, schema};
use crate::ease::{Curve, Tween};
use crate::feeds::Feeds;
use crate::theme;
use crate::ui::controls::{ghost, warning};
use crate::ui::field::{Field, FieldEvent};
use crate::ui::glyph::glyph;
use crate::ui::{pointer, rsx};

actions!(settings, [Back, Commit, Find]);

/// What the keys do while the settings have the keyboard. Once, at startup.
pub fn bind_keys(cx: &mut App) {
    const CONTEXT: Option<&str> = Some("Settings");
    cx.bind_keys([
        KeyBinding::new("escape", Back, CONTEXT),
        KeyBinding::new("enter", Commit, CONTEXT),
        KeyBinding::new("ctrl-f", Find, CONTEXT),
    ]);
}

/// One of the pages down the side.
pub struct Root {
    pub page: Page,
    pub glyph: &'static str,
    /// What `cae-shell settings WORD` calls it.
    pub word: &'static str,
    /// What else somebody might type when looking for it.
    pub also: &'static str,
}

const fn root(page: Page, glyph: &'static str, word: &'static str, also: &'static str) -> Root {
    Root { page, glyph, word, also }
}

/// The pages down the side, in the groups they are drawn in.
pub const ROOTS: &[&[Root]] = &[
    &[root(Page::Look, "palette", "look", "wallpaper background colours scheme theme dark light transparency variant")],
    &[
        root(Page::Network, "wifi", "network", "wifi wireless ethernet wired internet ip address dns gateway"),
        root(Page::Bluetooth, "bluetooth", "bluetooth", "devices pair headphones keyboard mouse discoverable"),
        root(Page::Audio, "volume_up", "audio", "sound volume microphone speakers output input device app"),
    ],
    &[
        root(Page::Compositor, "grid_view", "compositor", "hyprland blur gaps shadows gestures keybinds monitors"),
        root(Page::Updates, "update", "updates", "release version upgrade"),
        root(Page::Checkup, "troubleshoot", "scan", "system check health diagnose repair fix missing packages broken"),
    ],
    &[
        root(Page::Panels, "dock_to_bottom", "panels", "bar launcher dashboard sidebar workspaces tray clock"),
        root(Page::Apps, "apps", "apps", "default terminal file manager favourite hidden launcher graphics card gpu"),
        root(Page::Services, "build", "services", "notifications toasts lyrics volume brightness visualiser gpu"),
        root(Page::Region, "schedule", "region", "celsius fahrenheit temperature clock 12 24 hour"),
    ],
    &[root(Page::About, "info", "about", "version hostname kernel system")],
];

/// What every page is handed: the settings themselves, what the desktop is
/// doing, and the way to the other pages.
#[derive(Clone)]
pub struct Reach {
    pub store: Entity<Store>,
    pub feeds: Feeds,
    pub nav: Nav,
}

/// The way from one page to another, for a page to keep.
#[derive(Clone)]
pub struct Nav(WeakEntity<Frame>);

impl Nav {
    pub fn go(&self, page: Page, window: &mut Window, cx: &mut App) {
        let _ = self.0.update(cx, |frame, cx| frame.go(page, window, cx));
    }

    pub fn back(&self, window: &mut Window, cx: &mut App) {
        let _ = self.0.update(cx, |frame, cx| frame.back(window, cx));
    }

    /// Takes the keyboard back from whatever box has it, which is how a box
    /// is told that what is in it is meant.
    pub fn settle(&self, window: &mut Window, cx: &mut App) {
        let _ = self.0.update(cx, |frame, cx| window.focus(&frame.focus, cx));
    }
}

const SIDE: Pixels = px(232.);
const HEAD: Pixels = px(58.);
const ROOT_ROW: Pixels = px(34.);
const ROOT_GAP: Pixels = px(2.);
const GROUP_GAP: Pixels = px(14.);
const TRAVEL: Duration = Duration::from_millis(210);
const ARRIVE: Duration = Duration::from_millis(190);
/// How far a page slides as it arrives, and from which side: forward comes
/// from the right, back from the left.
const SLIDE: f32 = 14.;

pub struct Frame {
    reach: Reach,
    /// From a page down the side to the one that is open, which is last.
    trail: Vec<Page>,
    view: AnyView,
    search: Entity<Field>,
    focus: FocusHandle,
    /// Where down the side the marker is, in pixels from the first row.
    marker: Tween,
    arriving: Tween,
    arrives_from: f32,
    scroll: ScrollHandle,
}

/// How far down the side a page's row is, and nothing for a page that is
/// not there.
fn root_offset(page: &Page) -> Option<f32> {
    let mut offset = 0.;
    for group in ROOTS {
        for root in *group {
            if root.page == *page {
                return Some(offset);
            }
            offset += f32::from(ROOT_ROW + ROOT_GAP);
        }
        // The gap after a group is in place of the one after a row, not as
        // well as it.
        offset += f32::from(GROUP_GAP - ROOT_GAP);
    }
    None
}

impl Frame {
    pub fn new(start: Page, feeds: &Feeds, window: &mut Window, cx: &mut Context<Self>) -> Frame {
        let store = cx.new(|cx| Store::new(feeds, cx));
        cx.observe(&store, |_, _, cx| cx.notify()).detach();
        let search = cx.new(|cx| Field::new("Search", cx));
        cx.subscribe(&search, |_, _, _: &FieldEvent, cx| cx.notify()).detach();

        let reach = Reach { store, feeds: feeds.clone(), nav: Nav(cx.weak_entity()) };
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        // What a window list and a task switcher call it.
        window.set_window_title("Settings");

        let trail = start.trail();
        let view = build(trail.last().expect("a trail ends in its page"), &reach, window, cx);
        let marker = Tween::still(root_offset(&trail[0]).unwrap_or(0.));
        Frame {
            reach,
            trail,
            view,
            search,
            focus,
            marker,
            arriving: Tween::still(1.),
            arrives_from: 1.,
            scroll: ScrollHandle::new(),
        }
    }

    /// Opens `page` as somewhere new, reached from the side of the window.
    pub fn show(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |search, cx| search.set_text("", cx));
        self.replace(page.trail(), 1., window, cx);
    }

    /// Opens `page` as somewhere this page leads.
    fn go(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        let mut trail = self.trail.clone();
        trail.push(page);
        self.replace(trail, 1., window, cx);
    }

    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.trail.len() > 1 {
            let trail = self.trail[..self.trail.len() - 1].to_vec();
            self.replace(trail, -1., window, cx);
        }
    }

    /// Back as far as the page at `depth` along the trail.
    fn back_to(&mut self, depth: usize, window: &mut Window, cx: &mut Context<Self>) {
        if depth + 1 < self.trail.len() {
            self.replace(self.trail[..=depth].to_vec(), -1., window, cx);
        }
    }

    fn replace(&mut self, trail: Vec<Page>, from: f32, window: &mut Window, cx: &mut Context<Self>) {
        if trail == self.trail {
            return;
        }
        // The box that was being typed into is about to go, and with it
        // anything that would have heard it being left.
        window.focus(&self.focus, cx);
        self.view = build(trail.last().expect("a trail ends in its page"), &self.reach, window, cx);
        if let Some(offset) = root_offset(&trail[0]) {
            self.marker.go(offset, TRAVEL, Curve::Settle);
        }
        self.trail = trail;
        self.arrives_from = from;
        self.arriving.jump(0.);
        self.arriving.go(1., ARRIVE, Curve::Arrive);
        self.scroll.set_offset(gpui::point(px(0.), px(0.)));
        cx.notify();
    }

    fn side(&self, query: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let open = &self.trail[0];
        let found = (!query.is_empty()).then(|| find(query));

        let pages = rsx! {
            <div class="relative flex flex-col" gap={GROUP_GAP}>
                <div
                    class="absolute w-full"
                    top={px(self.marker.value())}
                    h={ROOT_ROW}
                    rounded={px(9.)}
                    bg={theme::highlight()}
                    shadow={theme::highlight_shadows()}
                />
                {for group in ROOTS.iter() {
                    <div class="flex flex-col" gap={ROOT_GAP}>
                        {for root in group.iter() {
                            <div
                                id={root.word}
                                class="relative flex flex-none items-center gap-[11px] px-[10px] cursor-pointer"
                                h={ROOT_ROW}
                                rounded={px(9.)}
                                text_size={px(13.)}
                                text_color={if root.page == *open { theme::text() } else { theme::text_dim() }}
                                when={(root.page != *open, |row| row.hover(|style| style.text_color(theme::text())))}
                                onClick={cx.listener(|frame, _, window, cx| frame.show(root.page.clone(), window, cx))}
                            >
                                {glyph(root.glyph, px(17.))}
                                {root.page.title()}
                            </div>
                        }}
                    </div>
                }}
            </div>
        };

        rsx! {
            <div class="flex flex-col flex-none h-full px-[12px] pb-[12px]" w={SIDE}>
                <div class="flex flex-none items-center gap-[9px] px-[10px]" h={HEAD} text_size={px(13.)}>
                    {glyph("search", px(17.)).text_color(theme::text_faint())}
                    {self.search.clone()}
                </div>
                <div id="side" class="flex flex-col flex-1 min-h-[0px] overflow-y-scroll">
                    {match found {
                        None => pages.into_any_element(),
                        Some(found) if found.is_empty() => rsx! {
                            <div class="px-[10px] pt-[6px]" text_size={px(12.5)} text_color={theme::text_faint()}>
                                {"Nothing by that name"}
                            </div>
                        }
                        .into_any_element(),
                        Some(found) => rsx! {
                            <div class="flex flex-col" gap={ROOT_GAP}>
                                {for (index, hit) in found.into_iter().enumerate() {
                                    <div
                                        id={("found", index)}
                                        class="flex flex-none items-center gap-[11px] px-[10px] py-[7px] cursor-pointer"
                                        min_h={ROOT_ROW}
                                        rounded={px(9.)}
                                        text_color={theme::text_dim()}
                                        hover={|style| style.bg(theme::white(0.05)).text_color(theme::text())}
                                        onClick={cx.listener(move |frame, _, window, cx| frame.show(hit.page.clone(), window, cx))}
                                    >
                                        {...hit.glyph.map(|symbol| glyph(symbol, px(17.)))}
                                        <div class="flex flex-col flex-1 gap-[2px] min-w-[0px]">
                                            <div class="truncate" text_size={px(13.)}>{hit.label}</div>
                                            {...(!hit.place.is_empty()).then(|| rsx! {
                                                <div class="truncate" text_size={px(11.5)} text_color={theme::text_faint()}>{hit.place}</div>
                                            })}
                                        </div>
                                    </div>
                                }}
                            </div>
                        }
                        .into_any_element(),
                    }}
                </div>
            </div>
        }
    }

    /// Where this is, as the way here: every page on the way is a way back.
    fn head(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let last = self.trail.len() - 1;
        rsx! {
            <div
                class="flex flex-none items-center gap-[6px] pl-[30px] pr-[14px]"
                h={HEAD}
                // The window has no title bar, so the top of it is one.
                onMouseDown={(MouseButton::Left, |_, window: &mut Window, _: &mut App| window.start_window_move())}
            >
                {for (depth, page) in self.trail.iter().enumerate() {
                    <div class="flex flex-none items-center gap-[6px]" key={depth}>
                        {...(depth > 0).then(|| glyph("chevron_right", px(16.)).text_color(theme::text_faint()))}
                        <div
                            id={("crumb", depth)}
                            class="flex-none"
                            text_size={px(17.)}
                            font_weight={FontWeight::MEDIUM}
                            text_color={if depth == last { theme::text() } else { theme::text_faint() }}
                            when={(depth != last, |crumb| crumb.cursor_pointer().hover(|style| style.text_color(theme::text_dim())))}
                            // A press here is a way back, not the start of a
                            // drag: it goes no further up.
                            onMouseDown={(MouseButton::Left, |_, _: &mut Window, cx: &mut App| cx.stop_propagation())}
                            onClick={cx.listener(move |frame, _, window, cx| frame.back_to(depth, window, cx))}
                        >
                            {page.title()}
                        </div>
                    </div>
                }}
                <div class="flex-1" />
                <div
                    base={ghost("close")}
                    id="close"
                    class="size-[26px]"
                    onMouseDown={(MouseButton::Left, |_, _: &mut Window, cx: &mut App| cx.stop_propagation())}
                    onClick={|_, window, _| window.remove_window()}
                />
            </div>
        }
    }
}

/// The view for a page: the one view for every page that is a list, and a
/// view of its own for each that is more.
fn build(page: &Page, reach: &Reach, window: &mut Window, cx: &mut Context<Frame>) -> AnyView {
    match page {
        Page::Look => cx.new(|cx| pages::look::Look::new(reach, window, cx)).into(),
        Page::Wallpapers => cx.new(|cx| pages::look::Wallpapers::new(String::new(), reach, cx)).into(),
        Page::WallpaperFolder(folder) => cx.new(|cx| pages::look::Wallpapers::new(folder.clone(), reach, cx)).into(),
        Page::Colours => cx.new(pages::look::Colours::new).into(),
        Page::Network => cx.new(|cx| pages::network::Network::new(reach, cx)).into(),
        Page::Ethernet { interface, connection } => {
            cx.new(|cx| pages::network::Ethernet::new(interface.clone(), connection.clone(), cx)).into()
        }
        Page::Bluetooth => cx.new(|cx| pages::bluetooth::Bluetooth::new(reach, cx)).into(),
        Page::Device { address, name } => cx.new(|cx| pages::bluetooth::Device::new(address.clone(), name.clone(), reach, cx)).into(),
        Page::Pairing => cx.new(|cx| pages::bluetooth::Pairing::new(reach, cx)).into(),
        Page::Audio => cx.new(|cx| pages::audio::Audio::new(reach, cx)).into(),
        Page::AppVolumes => cx.new(pages::audio::AppVolumes::new).into(),
        Page::Updates => cx.new(|cx| pages::updates::Updates::new(reach, cx)).into(),
        Page::Apps => cx.new(|cx| pages::apps::Apps::new(reach, cx)).into(),
        Page::OpensWith(opens) => cx.new(|cx| pages::apps::AppList::new(pages::apps::Purpose::Choose(*opens), reach, window, cx)).into(),
        Page::AllApps => cx.new(|cx| pages::apps::AppList::new(pages::apps::Purpose::Browse, reach, window, cx)).into(),
        Page::AppGpus => cx.new(|cx| pages::apps::AppList::new(pages::apps::Purpose::Cards, reach, window, cx)).into(),
        Page::App { id, .. } => cx.new(|cx| pages::apps::AppInfo::new(id.clone(), reach, cx)).into(),
        Page::CustomKeys => cx.new(|cx| pages::compositor::CustomKeys::new(reach, cx)).into(),
        Page::Monitors => cx.new(|cx| pages::compositor::Monitors::new(reach, cx)).into(),
        Page::AllOptions => cx.new(|cx| pages::compositor::Options::new(reach, window, cx)).into(),
        Page::Checkup => cx.new(pages::checkup::Checkup::new).into(),
        Page::About => cx.new(pages::about::About::new).into(),
        Page::Bar => cx.new(|cx| pages::bar::Bar::new(reach, window, cx)).into(),
        // A page with no sections is one nobody has written, which is a
        // mistake to find in a test and not a reason to take the bar down.
        listed => cx.new(|cx| ListPage::new(schema::sections(listed).unwrap_or_default(), reach, window, cx)).into(),
    }
}

/// Something the search found: what it is called, where it is, and the page
/// to open for it.
struct Found {
    /// A page's own glyph, for a page. A row has none.
    glyph: Option<&'static str>,
    label: SharedString,
    place: SharedString,
    page: Page,
}

/// How many the side of the window has room to show.
const MOST_FOUND: usize = 14;

fn find(query: &str) -> Vec<Found> {
    let query = query.trim().to_lowercase();
    let says = |text: &str| text.to_lowercase().contains(&query);

    let pages = ROOTS.iter().flat_map(|group| group.iter()).filter(|root| says(&root.page.title()) || says(root.also));
    let pages = pages.map(|root| Found { glyph: Some(root.glyph), label: root.page.title(), place: "".into(), page: root.page.clone() });

    let rows = schema::every_row().filter(|listed| says(listed.row.label) || says(listed.row.note) || says(listed.under));
    let rows = rows.map(|listed| {
        let mut way: Vec<String> = listed.page.clone().trail().iter().map(|page| page.title().to_string()).collect();
        if !listed.under.is_empty() {
            way.push(listed.under.to_string());
        }
        Found { glyph: None, label: listed.row.label.into(), place: way.join(" › ").into(), page: listed.page }
    });

    pages.chain(rows).take(MOST_FOUND).collect()
}

impl Focusable for Frame {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Frame {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !(self.marker.done() && self.arriving.done()) {
            window.request_animation_frame();
        }
        let query = self.search.read(cx).text().to_string();
        let arrived = self.arriving.value();
        let failure = self.reach.store.read(cx).failure.clone();

        rsx! {
            <div
                id="settings"
                class="relative size-full flex"
                key_context="Settings"
                track_focus={&self.focus}
                font_family={theme::FONT}
                bg={theme::window()}
                text_color={theme::text()}
                on_action={cx.listener(|frame, _: &Back, window, cx| {
                    if frame.search.read(cx).text().is_empty() {
                        frame.back(window, cx);
                    } else {
                        frame.search.update(cx, |search, cx| search.set_text("", cx));
                    }
                })}
                on_action={cx.listener(|frame, _: &Find, window, cx| window.focus(&frame.search.focus_handle(cx), cx))}
            >
                {self.side(&query, cx)}
                <div class="flex-none w-[1px] h-full" bg={theme::white(0.05)} />
                <div class="flex flex-col flex-1 min-w-[0px] h-full">
                    {self.head(cx)}
                    <div
                        id="page"
                        class="flex flex-col flex-1 min-h-[0px] pl-[30px] pr-[30px] overflow-y-scroll"
                        track_scroll={&self.scroll}
                        opacity={arrived}
                        ml={px((1. - arrived) * SLIDE * self.arrives_from)}
                    >
                        {...failure.map(|failure| rsx! { <div class="flex-none pb-[14px]" max_w={super::rows::COLUMN}>{warning(failure)}</div> })}
                        {self.view.clone()}
                    </div>
                </div>
                {pointer::see_out()}
            </div>
        }
    }
}
