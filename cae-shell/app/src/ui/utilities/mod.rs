//! The utilities: the three things about the machine that are switched
//! rather than read.
//!
//! Keeping it awake, recording what is on it, and the handful of switches
//! that would otherwise be a panel each. It rises from the foot of the
//! screen at the right, where the QML shell's did, and is a window only
//! while it is up.

mod cards;
mod pane;

use std::collections::HashMap;
use std::time::Duration;

use gpui::{
    AnyWindowHandle, App, AppContext, AsyncApp, Bounds, Context, DisplayId, Entity, Global, Size,
    IntoElement, MouseMoveEvent, Render, WeakEntity, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle,
    WindowKind, WindowOptions, div, layer_shell::*, point, prelude::*, px,
};

use crate::feeds::Feeds;
use crate::ours;
use crate::ui::screen;
use crate::ui::rsx;
use crate::ui::Ask;
use pane::Pane;

/// What Quickshell is asked about before any of this is drawn.
const PIECE: &str = "utilities";

/// The name the compositor already blurs behind and fades in: a panel's.
const NAMESPACE: &str = "caelestia-panel";

/// The strip along the foot of the screen, at the right-hand end, that opens
/// it. The QML shell rose this panel when the corner was reached for, and a
/// panel nothing can reach for is a panel nobody opens.
const EDGE: Size<gpui::Pixels> = Size { width: px(420.), height: px(2.) };

pub struct Utilities {
    feeds: Feeds,
    open: Option<WindowHandle<Pane>>,
    /// One strip per screen, remade as screens come and go.
    edges: HashMap<DisplayId, AnyWindowHandle>,
}

struct Shared(Entity<Utilities>);

impl Global for Shared {}

/// Keeps it for the life of the shell: the strip that opens it on every
/// screen, and nothing else until it is asked for.
pub fn keep(cx: &mut App, feeds: &Feeds) {
    let utilities = cx.new(|_| Utilities { feeds: feeds.clone(), open: None, edges: HashMap::new() });
    cx.set_global(Shared(utilities.clone()));

    // Outputs come and go, and GPUI does not say when. Looked at on a slow
    // tick, quickly at first: at startup the displays arrive a few
    // milliseconds after the application does.
    cx.spawn(async move |cx: &mut AsyncApp| {
        let mut looks = 0_u32;
        loop {
            let found = utilities.update(cx, |utilities, cx| utilities.edges(cx));
            looks += 1;
            let wait = if found == 0 && looks < 200 { 25 } else { 2000 };
            cx.background_executor().timer(Duration::from_millis(wait)).await;
        }
    })
    .detach();
}

/// What a key or the door asks for.
pub fn answer(ask: Ask, cx: &mut App) {
    let Some(shared) = cx.try_global::<Shared>().map(|shared| shared.0.clone()) else { return };
    // Asked afresh: the kept answer may be a no from before Quickshell had
    // stood its own down, and a key is somebody waiting.
    ours::when_known(PIECE, cx, move |ours, cx| {
        if !ours {
            // Still the old shell's, so it is the old shell that opens it.
            return cx.background_spawn(async { drop(cae_core::services::ipc("drawers", "toggle", &["utilities"])) }).detach();
        }
        shared.update(cx, |utilities, cx| utilities.answer(ask, None, false, cx));
    });
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
            anchor: Anchor::BOTTOM | Anchor::RIGHT,
            keyboard_interactivity: KeyboardInteractivity::None,
            ..Default::default()
        }),
        ..crate::ui::surface::options()
    }
}

impl Utilities {
    /// A strip on every output there is, and none on one that has gone.
    /// Says how many outputs there are.
    fn edges(&mut self, cx: &mut Context<Self>) -> usize {
        let outputs = screen::outputs(cx);
        // Quickshell rises one out of the same corner, and one started
        // before it knew to stand down still does.
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

        let utilities = cx.weak_entity();
        for (name, display) in &outputs {
            if self.edges.contains_key(display) {
                continue;
            }
            let (utilities, display) = (utilities.clone(), *display);
            match cx.open_window(surface(Some(display), EDGE), move |_, cx| cx.new(|_| Edge { utilities, display })) {
                Ok(edge) => drop(self.edges.insert(display, edge.into())),
                Err(error) => eprintln!("cae: cannot open the utilities' edge on {name}: {error}"),
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
        if !enabled() {
            return None;
        }
        // Where the person is looking, or where no compositor says, the
        // first screen there is: never left to the compositor to choose.
        let display = display.or_else(|| screen::focused_display(cx)).or_else(|| screen::outputs(cx).first().map(|(_, display)| *display));
        let (feeds, utilities) = (self.feeds.clone(), cx.weak_entity());
        let opened = cx.open_window(surface(display, pane::SURFACE), move |window, cx| {
            cx.new(|cx| Pane::new(by_hover, &feeds, utilities, window, cx))
        });
        opened.map_err(|error| eprintln!("cae: cannot open the utilities: {error}")).ok()
    }

    fn gone(&mut self) {
        self.open = None;
    }
}

/// Whether the settings want it at all.
fn enabled() -> bool {
    let shell = cae_core::config::read(cae_core::config::File::Shell);
    cae_core::config::lookup(&shell, "utilities.enabled").and_then(serde_json::Value::as_bool).unwrap_or(true)
}

/// The strip along the foot of one screen, at the right-hand end.
struct Edge {
    utilities: WeakEntity<Utilities>,
    display: DisplayId,
}

impl Render for Edge {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (utilities, display) = (self.utilities.clone(), self.display);
        rsx! {
            <div
                id="edge"
                class="size-full"
                // Any movement in it, rather than arriving in it: a window is
                // born believing the pointer is at its corner, which for one
                // this thin is inside it.
                onMouseMove={move |_: &MouseMoveEvent, _: &mut Window, cx: &mut App| {
                    let utilities = utilities.clone();
                    // Not over a fullscreen window: the foot of a game is
                    // part of the game. Asked of the compositor away from the
                    // thread that draws.
                    cx.spawn(async move |cx| {
                        if cx.background_spawn(async { cae_core::hypr::fullscreen_focused() }).await {
                            return;
                        }
                        let _ = utilities.update(cx, |utilities, cx| utilities.answer(Ask::Show, Some(display), true, cx));
                    })
                    .detach();
                }}
            />
        }
    }
}
