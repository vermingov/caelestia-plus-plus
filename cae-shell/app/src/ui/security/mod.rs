//! The security centre: what the two guards are doing, what they remember,
//! and what starts itself when the machine does.
//!
//! The shield in the bar opens it. It is one window with four pages, and
//! unlike the prompts it is nothing urgent: the guards go on guarding
//! whether or not it is open.

mod pages;
mod pane;

pub use pane::bind_keys;

use gpui::{
    App, AppContext, Bounds, Context, Entity, Global, Size, WindowBackgroundAppearance, WindowBounds, WindowHandle,
    WindowKind, WindowOptions, layer_shell::*, point, px,
};

use crate::feeds::Feeds;
use crate::ours;
use crate::ui::screen::{self, STRETCH};
use pane::Pane;

/// What Quickshell is asked about before this is drawn.
const PIECE: &str = "security";

const NAMESPACE: &str = "caelestia-panel";

/// Which page it is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Protection,
    Firewall,
    Startup,
}

impl Tab {
    pub fn named(word: Option<&str>) -> Tab {
        match word {
            Some("protection") => Tab::Protection,
            Some("firewall") => Tab::Firewall,
            Some("startup") => Tab::Startup,
            _ => Tab::Overview,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Protection => "Protection",
            Tab::Firewall => "Firewall",
            Tab::Startup => "Startup",
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            Tab::Overview => "space_dashboard",
            Tab::Protection => "security",
            Tab::Firewall => "gpp_good",
            Tab::Startup => "rocket_launch",
        }
    }

    /// Which daemon the page is about, if it is about one.
    fn daemon(self) -> Option<&'static str> {
        match self {
            Tab::Protection => Some("protection"),
            Tab::Firewall => Some("firewall"),
            _ => None,
        }
    }

    const ALL: [Tab; 4] = [Tab::Overview, Tab::Protection, Tab::Firewall, Tab::Startup];
}

struct Centre {
    feeds: Feeds,
    open: Option<WindowHandle<Pane>>,
}

struct Shared(Entity<Centre>);

impl Global for Shared {}

/// Keeps it for the life of the shell. Nothing is drawn until it is asked
/// for.
pub fn keep(cx: &mut App, feeds: &Feeds) {
    let centre = cx.new(|_| Centre { feeds: feeds.clone(), open: None });
    cx.set_global(Shared(centre));
}

/// Opens it on a page, or shuts it if that page is already the one showing.
pub fn ask(tab: Tab, cx: &mut App) {
    let Some(shared) = cx.try_global::<Shared>().map(|shared| shared.0.clone()) else { return };
    ours::when_known(PIECE, cx, move |ours, cx| {
        if !ours {
            let word = match tab {
                Tab::Overview => "overview",
                Tab::Protection => "protection",
                Tab::Firewall => "firewall",
                Tab::Startup => "startup",
            };
            return cx.background_spawn(async move { drop(cae_core::services::ipc("security", "openTab", &[word])) }).detach();
        }
        shared.update(cx, |centre, cx| centre.answer(tab, cx));
    });
}

impl Centre {
    fn answer(&mut self, tab: Tab, cx: &mut Context<Self>) {
        if let Some(open) = self.open.take() {
            // Open already: the ask is either a different page or a second
            // press of the same one, which shuts it.
            let showing = open.update(cx, |pane, window, cx| pane.turn_to(tab, window, cx));
            match showing {
                Ok(true) => return self.open = Some(open),
                Ok(false) => return,
                Err(_) => {}
            }
        }
        let (feeds, centre) = (self.feeds.clone(), cx.weak_entity());
        let display = screen::focused_display(cx).or_else(|| screen::outputs(cx).first().map(|(_, display)| *display));
        let options = WindowOptions {
            titlebar: None,
            display_id: display,
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(0.), px(0.)), Size::new(STRETCH, STRETCH)))),
            app_id: Some(NAMESPACE.to_string()),
            window_background: WindowBackgroundAppearance::Transparent,
            kind: WindowKind::LayerShell(LayerShellOptions {
                namespace: NAMESPACE.to_string(),
                layer: Layer::Overlay,
                anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
                exclusive_zone: Some(px(-1.)),
                keyboard_interactivity: KeyboardInteractivity::OnDemand,
                ..Default::default()
            }),
            ..crate::ui::surface::options()
        };
        let opened = cx.open_window(options, move |window, cx| cx.new(|cx| Pane::new(tab, &feeds, centre, window, cx)));
        self.open = opened.map_err(|error| eprintln!("cae: cannot open the security centre: {error}")).ok();
        // Anything frozen and waiting stays in front of it.
        cx.defer(crate::ui::guard::raise);
    }

    fn gone(&mut self) {
        self.open = None;
    }
}
