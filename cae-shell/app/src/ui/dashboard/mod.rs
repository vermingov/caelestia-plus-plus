//! The dashboard: what the day and the machine are doing, a reach up away.
//!
//! It comes down out of the middle of the bar, square where the two meet, so
//! that it reads as the bar opening rather than as a panel that happens to be
//! near it. The bar reserves its own height, so anchoring to the top puts
//! this immediately beneath it without either knowing the other's numbers.
//!
//! Like everything else here it is a window only while it is up. What is
//! always there is a strip two pixels tall under the bar, which is how
//! reaching for it is heard; between uses the dashboard is that strip and the
//! last forecast.

mod forecast;
mod home;
mod media;
mod pane;
mod performance;
mod weather;

use std::collections::HashMap;

use cae_core::{config, hypr, services};
use gpui::{
    AnyWindowHandle, App, AppContext, Bounds, Context, DisplayId, Entity, Global, IntoElement, Render, Size,
    Styled, WeakEntity, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions,
    canvas, div, layer_shell::*, point, prelude::*, px,
};

use crate::feeds::Feeds;
use crate::ours;
use crate::ui::screen::{self, Screens};
use crate::ui::{Ask, rsx};
use forecast::Forecast;
use pane::Pane;

/// What Quickshell is asked about before any of this is drawn.
const PIECE: &str = "dashboard";

/// Its own name, and not a panel's.
///
/// A panel is blurred behind and faded in by the compositor, and this must be
/// neither. The bar is deliberately not blurred — its surface is taller than
/// the pill and `ignore_alpha` does not keep the compositor off the
/// transparent part — so a blurred surface hanging off it shows a different
/// world through the same glass, and the join is visible however well the
/// two colours are matched. The fade is worse: it dissolves the thing in
/// rather than letting it open, over the top of whatever this draws itself.
const NAMESPACE: &str = "caelestia-drawer";

/// How far down the bar's own surface reaches. Both of this module's
/// surfaces step over it themselves rather than asking the compositor to
/// keep them clear of it: a layer surface that respects exclusive zones is
/// placed by the compositor, and the two that matter here do not agree about
/// where that puts a surface anchored on one edge only.
const BAR: gpui::Pixels = px(50.);

/// The strip under the middle of the bar that opens it: wide enough to be
/// reached for without aiming, and thin enough to cost the window below it
/// nothing but its top row of pixels. Its surface carries the bar's height
/// above that strip, which nothing is drawn in and which `reach` hands back
/// to whatever is underneath.
const EDGE: Size<gpui::Pixels> = Size { width: px(420.), height: px(50. + 2.) };

/// Which of its pages the dashboard is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Home,
    Media,
    Performance,
    Weather,
}

impl Tab {
    fn title(self) -> &'static str {
        match self {
            Tab::Home => "Dashboard",
            Tab::Media => "Media",
            Tab::Performance => "Performance",
            Tab::Weather => "Weather",
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            Tab::Home => "dashboard",
            Tab::Media => "queue_music",
            Tab::Performance => "speed",
            Tab::Weather => "cloud",
        }
    }

    /// The key in `shell.json` that takes this page away.
    fn shown_by(self) -> &'static str {
        match self {
            Tab::Home => "dashboard.showDashboard",
            Tab::Media => "dashboard.showMedia",
            Tab::Performance => "dashboard.showPerformance",
            Tab::Weather => "dashboard.showWeather",
        }
    }

    const ALL: [Tab; 4] = [Tab::Home, Tab::Media, Tab::Performance, Tab::Weather];
}

/// What the settings say about the dashboard, read when it matters: when the
/// edge is reached for, and when the pane opens.
struct Settings {
    enabled: bool,
    on_hover: bool,
    tabs: Vec<Tab>,
}

impl Settings {
    fn read() -> Settings {
        let shell = config::read(config::File::Shell);
        let flag = |path: &str| config::lookup(&shell, path).and_then(serde_json::Value::as_bool).unwrap_or(true);
        Settings {
            enabled: flag("dashboard.enabled"),
            on_hover: flag("dashboard.showOnHover"),
            tabs: Tab::ALL.into_iter().filter(|tab| flag(tab.shown_by())).collect(),
        }
    }
}

pub struct Dashboards {
    feeds: Feeds,
    forecast: Entity<Forecast>,
    open: Option<WindowHandle<Pane>>,
    edges: HashMap<DisplayId, AnyWindowHandle>,
    /// The page it was last on, which is the page it opens on.
    tab: Tab,
}

struct Shared(Entity<Dashboards>);

impl Global for Shared {}

/// Does what a key asked. Where the dashboard is still Quickshell's, the key
/// is passed along to it, so that the same words work on both sides of the
/// hand-over.
pub fn ask(ask: Ask, cx: &mut App) {
    let Some(dashboards) = cx.try_global::<Shared>().map(|shared| shared.0.clone()) else { return };
    ours::when_known(PIECE, cx, move |ours, cx| {
        if ours {
            return dashboards.update(cx, |dashboards, cx| dashboards.answer(ask, None, false, cx));
        }
        cx.background_spawn(async { drop(services::ipc("drawers", "toggle", &["dashboard"])) }).detach();
    });
}

/// Keeps the edge that opens it on every output for the life of the shell.
pub fn keep_on_every_output(cx: &mut App, screens: &Entity<Screens>, feeds: &Feeds) {
    let dashboards = cx.new(|cx| {
        cx.observe(screens, |dashboards: &mut Dashboards, _, cx| dashboards.edges(cx)).detach();
        Dashboards { feeds: feeds.clone(), forecast: cx.new(|_| Forecast::default()), open: None, edges: HashMap::new(), tab: Tab::Home }
    });
    dashboards.update(cx, |dashboards, cx| dashboards.edges(cx));
    cx.set_global(Shared(dashboards));
}

fn surface(display: Option<DisplayId>, size: Size<gpui::Pixels>) -> WindowOptions {
    WindowOptions {
        titlebar: None,
        focus: false,
        display_id: display,
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(0.), px(0.)), size))),
        app_id: Some(NAMESPACE.to_string()),
        window_background: WindowBackgroundAppearance::Transparent,
        kind: WindowKind::LayerShell(LayerShellOptions {
            namespace: NAMESPACE.to_string(),
            layer: Layer::Overlay,
            // The top, and neither side: anchored on one axis only, the
            // compositor centres it on the other.
            anchor: Anchor::TOP,
            // Placed at the very top and stepping over the bar itself. -1 is
            // "ignore every exclusive zone", which is the only placement both
            // compositors agree on; the height of the bar is then a number
            // this module holds rather than one it hopes for.
            exclusive_zone: Some(px(-1.)),
            // Never the keyboard: it opens because the pointer came near,
            // over whatever was being typed into.
            keyboard_interactivity: KeyboardInteractivity::None,
            ..Default::default()
        }),
        ..crate::ui::surface::options()
    }
}

impl Dashboards {
    /// An edge on every output there is, and none on one that has gone.
    fn edges(&mut self, cx: &mut Context<Self>) {
        let outputs = screen::outputs(cx);
        // Quickshell has a dashboard that rises out of the same corner, and
        // one started before it knew to stand down still draws it.
        if !ours::is_ours(PIECE, cx) {
            let this = cx.weak_entity();
            return ours::once_ours(PIECE, cx, move |cx| drop(this.update(cx, |this, cx| this.edges(cx))));
        }
        self.edges.retain(|display, edge| {
            let stays = outputs.iter().any(|(_, wanted)| wanted == display);
            if !stays {
                let _ = edge.update(cx, |_, window, _| window.remove_window());
            }
            stays
        });

        let dashboards = cx.weak_entity();
        for (name, display) in &outputs {
            if self.edges.contains_key(display) {
                continue;
            }
            let (dashboards, display) = (dashboards.clone(), *display);
            match cx.open_window(surface(Some(display), EDGE), move |_, cx| cx.new(|_| Edge { dashboards, display })) {
                Ok(edge) => drop(self.edges.insert(display, edge.into())),
                Err(error) => eprintln!("cae: cannot open the dashboard's edge on {name}: {error}"),
            }
        }
    }

    /// `display` is where the pointer reached for it, for an ask that came
    /// from an edge; a key does not say where it was pressed.
    fn answer(&mut self, ask: Ask, display: Option<DisplayId>, by_hover: bool, cx: &mut Context<Self>) {
        match (ask, self.open.take()) {
            (Ask::Show, Some(pane)) => self.open = Some(pane),
            (Ask::Show | Ask::Toggle, None) => self.open = self.show(display, by_hover, cx),
            (Ask::Hide | Ask::Toggle, Some(pane)) => {
                let _ = pane.update(cx, |pane, window, cx| pane.leave(window, cx));
            }
            (Ask::Hide, None) => {}
        }
    }

    fn show(&mut self, display: Option<DisplayId>, by_hover: bool, cx: &mut Context<Self>) -> Option<WindowHandle<Pane>> {
        let settings = Settings::read();
        if !settings.enabled || settings.tabs.is_empty() || (by_hover && !settings.on_hover) {
            return None;
        }
        let tab = if settings.tabs.contains(&self.tab) { self.tab } else { settings.tabs[0] };
        // Where the person is looking, or where no compositor says, the
        // first screen there is: never left to the compositor to choose.
        let display = display.or_else(|| screen::focused_display(cx)).or_else(|| screen::outputs(cx).first().map(|(_, display)| *display));
        let (feeds, forecast, dashboards) = (self.feeds.clone(), self.forecast.clone(), cx.weak_entity());

        let opened = cx.open_window(surface(display, pane::SURFACE), move |window, cx| {
            cx.new(|cx| Pane::new(settings.tabs, tab, by_hover, &feeds, forecast, dashboards, window, cx))
        });
        opened.map_err(|error| eprintln!("cae: cannot open the dashboard: {error}")).ok()
    }

    /// The pane has taken itself down, and says which page it was on.
    fn gone(&mut self, tab: Tab) {
        (self.open, self.tab) = (None, tab);
    }
}

/// The strip along the foot of one screen.
struct Edge {
    dashboards: WeakEntity<Dashboards>,
    display: DisplayId,
}

/// Tells the compositor that the edge is the strip and nothing else.
///
/// The surface stands the bar's height taller than the strip, to put the
/// strip under the bar without asking the compositor where the bar ends. A
/// surface takes the pointer everywhere by default, so that height is a band
/// of the screen that swallows clicks. Under the bar nobody notices, because
/// the bar is there to be clicked. Over a fullscreen window it is the top of
/// somebody's game: the bar is on the top layer and Hyprland stops offering
/// it the pointer, this is on the overlay layer and it does not, and the
/// clicks land on a surface that has been drawing nothing for as long as the
/// game has been up.
fn reach(window: &Window) {
    window.set_input_region(Some(&[Bounds::new(point(px(0.), BAR), Size::new(EDGE.width, EDGE.height - BAR))]));
}

impl Render for Edge {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (dashboards, display) = (self.dashboards.clone(), self.display);
        rsx! {
            <div
                id="edge"
                class="absolute w-full"
                top={BAR}
                bottom={px(0.)}
                // Any movement in it, rather than arriving in it: a window is
                // born believing the pointer is at its corner, which for one
                // this thin is inside it.
                onMouseMove={move |_: &gpui::MouseMoveEvent, _: &mut Window, cx: &mut App| {
                    let dashboards = dashboards.clone();
                    // Not over a fullscreen window: the foot of a game is
                    // part of the game. Asked of the compositor away from the
                    // thread that draws.
                    cx.spawn(async move |cx| {
                        if cx.background_spawn(async { hypr::fullscreen_focused() }).await {
                            return;
                        }
                        let _ = dashboards.update(cx, |dashboards, cx| dashboards.answer(Ask::Show, Some(display), true, cx));
                    })
                    .detach();
                }}
            >
                <canvas class="absolute size-full" prepaint={|_, window: &mut Window, _: &mut App| reach(window)} paint={|_, _, _, _| ()} />
            </div>
        }
    }
}
