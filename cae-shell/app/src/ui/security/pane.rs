//! The window: the pages along the top, and the one being looked at.

use std::time::Duration;

use cae_core::startup;
use gpui::{
    AnyElement, App, AppContext, Context, Entity, FocusHandle, Focusable, IntoElement, KeyBinding, MouseButton, Render,
    Styled, Task, WeakEntity, Window, actions, div, prelude::*, px,
};

use super::{Centre, Tab, pages};
use crate::ease::{Curve, Tween};
use crate::feeds::Feeds;
use crate::theme;
use crate::ui::glyph::glyph;
use crate::ui::{pointer, rsx};

actions!(security, [Close]);

const WIDTH: gpui::Pixels = px(880.);
const HEIGHT: gpui::Pixels = px(560.);
const ENTER: Duration = Duration::from_millis(220);
const LEAVE: Duration = Duration::from_millis(150);
/// How often the list of what starts itself is read again while it is shown:
/// it is a scan of two directories and a systemctl, and nothing else changes
/// it while the panel is open.
const RESCAN: Duration = Duration::from_secs(4);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", Close, Some("Security"))]);
}

/// What every page is handed.
#[derive(Clone)]
pub struct Reach {
    pub feeds: Feeds,
    /// What starts itself, as last read. Kept here rather than in the page,
    /// so that turning to it and back does not blank the list.
    pub starts: Entity<Vec<startup::Entry>>,
}

pub struct Pane {
    reach: Reach,
    centre: WeakEntity<Centre>,
    tab: Tab,
    shown: Tween,
    leaving: bool,
    focus: FocusHandle,
    _scanning: Task<()>,
}

impl Pane {
    pub fn new(tab: Tab, feeds: &Feeds, centre: WeakEntity<Centre>, window: &mut Window, cx: &mut Context<Self>) -> Pane {
        cx.observe(&feeds.guards, |_, _, cx| cx.notify()).detach();
        let mut shown = Tween::still(0.);
        shown.go(1., ENTER, Curve::Arrive);
        let focus = cx.focus_handle();
        window.focus(&focus, cx);

        let starts = cx.new(|_| Vec::new());
        cx.observe(&starts, |_, _, cx| cx.notify()).detach();
        let scanning = cx.spawn({
            let starts = starts.clone();
            async move |_, cx| {
                loop {
                    let read = cx.background_spawn(async { startup::scan() }).await;
                    starts.update(cx, |kept, cx| {
                        if *kept != read {
                            *kept = read;
                            cx.notify();
                        }
                    });
                    cx.background_executor().timer(RESCAN).await;
                }
            }
        });

        Pane { reach: Reach { feeds: feeds.clone(), starts }, centre, tab, shown, leaving: false, focus, _scanning: scanning }
    }

    /// Turns to a page, which is what the overview's own rows do.
    pub fn turn(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.tab = tab;
        cx.notify();
    }

    /// Turns to a page. Says whether the window is staying: asked for the
    /// page it is already on, it goes instead.
    pub fn turn_to(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.tab == tab {
            self.leave(window, cx);
            return false;
        }
        self.tab = tab;
        cx.notify();
        true
    }

    fn leave(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.leaving, true) {
            return;
        }
        self.shown.go(0., LEAVE, Curve::In);
        cx.notify();
        let centre = self.centre.clone();
        cx.spawn_in(window, async move |_, cx| {
            cx.background_executor().timer(LEAVE).await;
            let _ = cx.update(|window, cx| {
                window.remove_window();
                let _ = centre.update(cx, |centre, _| centre.gone());
            });
        })
        .detach();
    }
}

impl Focusable for Pane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Pane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.shown.done() {
            window.request_animation_frame();
        }
        let shown = self.shown.value();
        let tab = self.tab;

        let tabs: Vec<AnyElement> = Tab::ALL
            .into_iter()
            .map(|which| {
                let here = which == tab;
                rsx! {
                    <div
                        id={which.title()}
                        class="flex flex-none items-center gap-[7px] h-[32px] px-[13px] rounded-full cursor-pointer"
                        bg={if here { theme::white(0.12) } else { theme::white(0.0) }}
                        text_color={if here { theme::text() } else { theme::text_dim() }}
                        text_size={px(12.5)}
                        hover={|style| style.text_color(theme::text())}
                        onClick={cx.listener(move |pane: &mut Pane, _, _, cx| {
                            pane.tab = which;
                            cx.notify();
                        })}
                    >
                        {glyph(which.glyph(), px(16.))}
                        {which.title()}
                    </div>
                }
                .into_any_element()
            })
            .collect();

        let page = match tab {
            Tab::Overview => pages::overview(&self.reach, cx),
            Tab::Protection | Tab::Firewall => pages::guard(tab, &self.reach, cx),
            Tab::Startup => pages::startup(&self.reach, cx),
        };

        rsx! {
            <div
                id="security"
                class="relative size-full flex items-center justify-center"
                key_context="Security"
                track_focus={&self.focus}
                font_family={theme::FONT}
                bg={theme::black(0.4 * shown)}
                on_action={cx.listener(|pane: &mut Pane, _: &Close, window, cx| pane.leave(window, cx))}
                // A press on the dark around it is a way out, as it is
                // everywhere else in the shell.
                onMouseDown={(MouseButton::Left, cx.listener(|pane: &mut Pane, _, window, cx| pane.leave(window, cx)))}
            >
                <div
                    id="pane"
                    class="flex flex-col flex-none overflow-hidden"
                    w={WIDTH}
                    h={HEIGHT}
                    opacity={shown}
                    rounded={px(22.)}
                    bg={theme::pane()}
                    shadow={theme::pane_shadows()}
                    text_color={theme::text()}
                    // Nothing pressed inside it is a press on the outside.
                    onMouseDown={(MouseButton::Left, |_, _: &mut Window, cx: &mut App| cx.stop_propagation())}
                >
                    <div class="flex flex-none items-center gap-[6px] px-[16px] py-[12px]">
                        {...tabs}
                        <div class="flex-1" />
                        <div
                            base={crate::ui::controls::round("close", false)}
                            id="close"
                            onClick={cx.listener(|pane: &mut Pane, _, window, cx| pane.leave(window, cx))}
                        />
                    </div>
                    <div class="flex flex-col flex-1 min-h-[0px] px-[16px] pb-[16px]">{page}</div>
                </div>
                {pointer::see_out()}
            </div>
        }
    }
}
