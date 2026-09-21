//! The dashboard: what the day and the machine are doing, a reach into the
//! corner away.
//!
//! It rises from the foot of the screen, at the left, where the QML shell's
//! did. Like everything else here it is a window only while it is up. What
//! is always there is a strip two pixels tall along the edge under it, which
//! is how reaching for it is heard; between uses the dashboard is that strip
//! and the last forecast.

mod forecast;
mod home;
mod media;
mod pane;
mod performance;
mod weather;

use std::collections::HashMap;
use std::time::Duration;

use cae_core::{config, hypr, services};
use gpui::{
    AnyWindowHandle, App, AppContext, AsyncApp, Bounds, Context, DisplayId, Entity, Global, IntoElement, Render, Size,
    Styled, WeakEntity, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions, div,
    layer_shell::*, point, prelude::*, px,
};

use crate::feeds::Feeds;
use crate::ours;
use crate::ui::screen;
use crate::ui::{Ask, rsx};
use forecast::Forecast;
use pane::Pane;

/// What Quickshell is asked about before any of this is drawn.
const PIECE: &str = "dashboard";

/// The name the compositor already blurs behind and fades in: a panel's.
const NAMESPACE: &str = "caelestia-panel";

/// The strip along the foot of the screen that opens it: as wide as the
/// reach for a corner is, and thin enough to cost the window above it
/// nothing but its bottom row of pixels.
const EDGE: Size<gpui::Pixels> = Size { width: px(420.), height: px(2.) };

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
pub fn keep_on_every_output(cx: &mut App, feeds: &Feeds) {
    let dashboards = cx.new(|cx| Dashboards {
        feeds: feeds.clone(),
        forecast: cx.new(|_| Forecast::default()),
        open: None,
        edges: HashMap::new(),
        tab: Tab::Home,
    });
    cx.set_global(Shared(dashboards.clone()));

    // Outputs come and go, and GPUI does not say when. Looked at on a slow
    // tick, quickly at first: at startup the displays arrive a few
    // milliseconds after the application does.
    cx.spawn(async move |cx: &mut AsyncApp| {
        let mut looks = 0_u32;
        loop {
            let found = dashboards.update(cx, |dashboards, cx| dashboards.edges(cx));
            looks += 1;
            let wait = if found == 0 && looks < 200 { 25 } else { 2000 };
            cx.background_executor().timer(Duration::from_millis(wait)).await;
        }
    })
    .detach();
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
            anchor: Anchor::BOTTOM | Anchor::LEFT,
            // Never the keyboard: it opens because the pointer came near,
            // over whatever was being typed into.
            keyboard_interactivity: KeyboardInteractivity::None,
            ..Default::default()
        }),
        ..Default::default()
    }
}

impl Dashboards {
    /// An edge on every output there is, and none on one that has gone. Says
    /// how many outputs there are.
    fn edges(&mut self, cx: &mut Context<Self>) -> usize {
        let outputs = screen::outputs(cx);
        // Quickshell has a dashboard that rises out of the same corner, and
        // one started before it knew to stand down still draws it.
        if !ours::is_ours(PIECE, cx) {
            return outputs.len();
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
        outputs.len()
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

impl Render for Edge {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (dashboards, display) = (self.dashboards.clone(), self.display);
        rsx! {
            <div
                id="edge"
                class="size-full"
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
            />
        }
    }
}
