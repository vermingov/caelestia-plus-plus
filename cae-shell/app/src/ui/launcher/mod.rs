//! The launcher: one field, and whatever it finds.
//!
//! What it knows is kept for the life of the shell, because reading every
//! desktop entry, scheme and wallpaper is hundreds of milliseconds and a key
//! was just pressed. What it draws is not: the window exists while it is
//! open, and between opens the launcher is a list in memory and a socket.

mod pane;
mod rows;

use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixListener;
use std::sync::{Arc, Mutex};

use cae_core::launcher as backend;
use futures::StreamExt;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui::{
    App, AppContext, Bounds, Context, Entity, Global, Size, WindowBackgroundAppearance, WindowBounds, WindowHandle,
    WindowKind, WindowOptions, layer_shell::*, point, px,
};

use crate::feeds::Feeds;
use crate::ui::screen;
pub use pane::bind_keys;
use pane::Pane;

/// What the compositor's rules match on: the same name the old launcher had,
/// so the blur written for it is the blur this gets.
const NAMESPACE: &str = "caelestia-launcher";

/// What the launcher lists, shared between the thread that draws and the
/// threads that read disks on its behalf.
type Index = Arc<Mutex<backend::Launcher>>;

/// One thing asked of the launcher, by a keybind down the socket or by the
/// mark on the bar. The text is what the field should open holding, so a
/// keybind can drop straight into a mode: `>wallpaper ` for the picker.
enum Ask {
    Show(String),
    Hide,
    Toggle(String),
}

pub struct Launchers {
    index: Index,
    open: Option<WindowHandle<Pane>>,
}

/// Where the rest of the shell finds it.
struct Shared(Entity<Launchers>);

impl Global for Shared {}

/// What a key or the door asks of it: shown with a query, hidden, or
/// toggled. False when this shell has no launcher, which is one started to
/// be looked at beside another.
pub fn ask(how: Option<&str>, query: String, cx: &mut App) -> bool {
    let Some(launchers) = cx.try_global::<Shared>().map(|shared| shared.0.clone()) else { return false };
    let ask = match how {
        Some("show") => Ask::Show(query),
        Some("hide") => Ask::Hide,
        _ => Ask::Toggle(query),
    };
    launchers.update(cx, |launchers, cx| launchers.answer(ask, cx));
    true
}

/// Toggles this shell's launcher. False when there is none, which is a shell
/// that was started to be looked at beside another one.
pub fn toggle(cx: &mut App) -> bool {
    let Some(launchers) = cx.try_global::<Shared>().map(|shared| shared.0.clone()) else { return false };
    launchers.update(cx, |launchers, cx| launchers.answer(Ask::Toggle(String::new()), cx));
    true
}

impl Launchers {
    /// Reads what there is to launch, and then starts answering for it.
    ///
    /// The reading is every desktop entry, every scheme and every wallpaper,
    /// with an icon found for each application: most of a second, so it
    /// happens away from the thread that draws and the bar does not wait for
    /// it. Until it is done there is no launcher to open, which is the first
    /// moments of the session.
    pub fn start(cx: &mut App, feeds: &Feeds) {
        let feeds = feeds.clone();
        cx.spawn(async move |cx| {
            let index = cx
                .background_spawn(async {
                    let launcher = backend::Launcher::new();
                    launcher.warm();
                    Arc::new(Mutex::new(launcher))
                })
                .await;
            cx.update(|cx| Launchers::serve(index, &feeds, cx));
        })
        .detach();
    }

    /// Starts answering the socket, unless something already is: there is
    /// one socket, and taking it would take the launcher key from the shell
    /// that is in use.
    fn serve(index: Index, feeds: &Feeds, cx: &mut App) {
        let launchers = cx.new(|cx| {
            // Another workspace coming to the front is the person looking
            // away, and a launcher holding the keyboard cannot lose focus to
            // notice that for itself.
            let mut view = feeds.hypr.read(cx).value.view.clone();
            cx.observe(&feeds.hypr, move |launchers: &mut Launchers, hypr, cx| {
                let now = hypr.read(cx).value.view.clone();
                if now != view {
                    view = now;
                    launchers.answer(Ask::Hide, cx);
                }
            })
            .detach();
            Launchers { index: index.clone(), open: None }
        });
        cx.set_global(Shared(launchers.clone()));

        follow_installs(index);
        if backend::already_running() {
            eprintln!("cae: another launcher is answering the socket, which is left to it");
            return;
        }
        let (asks, mut asked) = unbounded::<Ask>();
        std::thread::spawn(move || listen(asks));
        cx.spawn(async move |cx| {
            while let Some(ask) = asked.next().await {
                launchers.update(cx, |launchers, cx| launchers.answer(ask, cx));
            }
        })
        .detach();
    }

    fn answer(&mut self, ask: Ask, cx: &mut Context<Self>) {
        match (ask, self.open.take()) {
            (Ask::Show(query), Some(window)) => {
                // Already up: a keybind for a mode still gets its mode.
                if !query.is_empty() {
                    let _ = window.update(cx, |pane, _, cx| pane.fill(query, cx));
                }
                self.open = Some(window);
            }
            (Ask::Show(query) | Ask::Toggle(query), None) => self.open = self.show(query, cx),
            (Ask::Hide | Ask::Toggle(_), Some(window)) => {
                let _ = window.update(cx, |_, window, _| window.remove_window());
                self.refresh(cx);
            }
            (Ask::Hide, None) => {}
        }
    }

    /// The pane has taken itself down: something was launched, or Escape. It
    /// does that itself because it is the one being updated at the time, and
    /// a window cannot be reached into from inside its own update.
    fn gone(&mut self, cx: &mut Context<Self>) {
        self.open = None;
        self.refresh(cx);
    }

    fn show(&mut self, query: String, cx: &mut Context<Self>) -> Option<WindowHandle<Pane>> {
        let display = screen::focused_display(cx);

        let options = WindowOptions {
            titlebar: None,
            display_id: display,
            // The whole output, whatever size that is, and never resized. The
            // pane grows and shrinks inside it.
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                point(px(0.), px(0.)),
                Size::new(screen::STRETCH, screen::STRETCH),
            ))),
            app_id: Some(NAMESPACE.to_string()),
            window_background: WindowBackgroundAppearance::Transparent,
            kind: WindowKind::LayerShell(LayerShellOptions {
                namespace: NAMESPACE.to_string(),
                layer: Layer::Overlay,
                // The surface is the whole output and never changes size. The
                // pane grows and shrinks inside it, which costs a repaint
                // where resizing a surface costs a round trip to the
                // compositor and a frame of the wrong size.
                anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
                // Exclusive, or what is typed goes to whatever had the
                // keyboard before.
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                ..Default::default()
            }),
            ..crate::ui::surface::options()
        };

        let (index, launchers) = (self.index.clone(), cx.weak_entity());
        match cx.open_window(options, move |window, cx| cx.new(|cx| Pane::new(index, launchers, query, window, cx))) {
            Ok(window) => Some(window),
            Err(error) => {
                eprintln!("cae: cannot open the launcher: {error}");
                None
            }
        }
    }

    /// Reads everything that can go stale, once the window is out of the way.
    ///
    /// On the way out rather than on the way in: it is hundreds of
    /// milliseconds, and between pressing the key and seeing anything is the
    /// one place those may not be spent. Here nobody is waiting, and what is
    /// listed is at most one open out of date.
    fn refresh(&self, cx: &mut Context<Self>) {
        let index = self.index.clone();
        cx.background_spawn(async move {
            // Built before the lock is taken, so that a launcher opened again
            // at once is searching the old list rather than waiting for this.
            let fresh = backend::Launcher::new();
            if let Ok(mut index) = index.lock() {
                fresh.inherit_icons(&index);
                *index = fresh;
            }
        })
        .detach();
    }
}

/// Keeps the list of applications in step with the disk, so that something
/// installed while the launcher is idle is there the next time it opens.
fn follow_installs(index: Index) {
    std::thread::spawn(move || {
        let reload = move || {
            let fresh = backend::Apps::load();
            if let Ok(mut index) = index.lock() {
                index.take_apps(fresh);
            }
        };
        if let Err(error) = backend::watch_applications(reload) {
            eprintln!("cae: not watching for new applications: {error}");
        }
    });
}

/// The socket a keybind talks to, so that opening the launcher is a connect
/// and a few bytes rather than a program started.
fn listen(asks: UnboundedSender<Ask>) {
    let path = backend::socket_path();
    // Left behind by a shell that was killed rather than stopped. Nothing is
    // listening on it: that was checked before this was called.
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(error) => return eprintln!("cae: cannot listen on {}: {error}", path.display()),
    };

    for stream in listener.incoming().flatten() {
        let asks = asks.clone();
        // One connection, any number of commands: a client that stays
        // connected pays for the socket once rather than once a keypress.
        std::thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                let (verb, query) = line.split_once(' ').unwrap_or((line.trim(), ""));
                let ask = match verb.trim() {
                    "show" => Ask::Show(query.to_string()),
                    "hide" => Ask::Hide,
                    _ => Ask::Toggle(query.to_string()),
                };
                if asks.unbounded_send(ask).is_err() {
                    return;
                }
            }
        });
    }
}
