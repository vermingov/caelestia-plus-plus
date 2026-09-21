//! The settings: an ordinary window, which is the one thing here that is.
//!
//! Everything else the shell draws is a layer surface or a popup hung from
//! one. This is a toplevel like any application's, because it is used like
//! one: left open beside the thing being adjusted, moved, tiled, closed with
//! the key that closes windows. It exists while it is open and costs nothing
//! otherwise, like the rest.

mod frame;
mod list;
mod pages;
mod rows;
mod schema;
mod store;

use gpui::{
    App, AppContext, Bounds, Global, SharedString, Size, WindowBackgroundAppearance, WindowBounds, WindowDecorations, WindowHandle,
    WindowOptions, px,
};

use crate::feeds::Feeds;
use crate::ui::screen;
pub use frame::bind_keys;
/// The one page something outside the settings has business with: a fix
/// offered elsewhere is read and run here.
pub use pages::checkup::hand_over;
use frame::Frame;

/// What the compositor's rules know the window by.
const APP_ID: &str = "caelestia-settings";

/// Somewhere in the settings. The ones along the side of the window are
/// `Page::roots`; the rest are reached from one of those, and `parent` says
/// which, so that a page opened by name still knows the way back.
#[derive(Clone, Debug, PartialEq)]
pub enum Page {
    Look,
    Wallpapers,
    /// A folder under the wallpaper directory, by its path from there.
    WallpaperFolder(String),
    Colours,
    Network,
    /// One wired device, by the kernel's name for it and the profile it is on.
    Ethernet { interface: String, connection: String },
    Bluetooth,
    Device { address: String, name: String },
    Pairing,
    Audio,
    AppVolumes,
    Updates,
    Apps,
    /// Choosing the program the shell opens one kind of thing with.
    OpensWith(pages::apps::Opens),
    AllApps,
    App { id: String, name: String },
    AppGpus,
    About,
    Panels,
    Bar,
    BarWorkspaces,
    BarStatus,
    Launcher,
    Dashboard,
    Sidebar,
    Services,
    Notifications,
    Region,
    Compositor,
    Checkup,
    WindowLooks,
    Gaps,
    Input,
    LaunchedApps,
    Keys,
    CustomKeys,
    Monitors,
    AllOptions,
}

impl Page {
    pub fn title(&self) -> SharedString {
        let fixed = match self {
            Page::Ethernet { interface, connection } => {
                return if connection.is_empty() { interface.clone() } else { connection.clone() }.into();
            }
            Page::Device { name, .. } | Page::App { name, .. } => return name.clone().into(),
            Page::OpensWith(opens) => opens.title(),
            Page::Apps => "Apps",
            Page::AllApps => "All apps",
            Page::AppGpus => "Graphics cards",
            Page::WallpaperFolder(folder) => return folder.rsplit('/').next().unwrap_or_default().to_string().into(),
            Page::Look => "Wallpaper and colours",
            Page::Wallpapers => "Wallpapers",
            Page::Colours => "Colours",
            Page::Network => "Network",
            Page::Bluetooth => "Bluetooth",
            Page::Pairing => "Pair a new device",
            Page::Audio => "Audio",
            Page::AppVolumes => "App volumes",
            Page::Updates => "Updates",
            Page::Checkup => "System scan",
            Page::About => "About",
            Page::Panels => "Panels",
            Page::Bar => "Bar",
            Page::BarWorkspaces => "Workspaces",
            Page::BarStatus => "Status icons",
            Page::Launcher => "Launcher",
            Page::Dashboard => "Dashboard",
            Page::Sidebar => "Sidebar",
            Page::Services => "Services",
            Page::Notifications => "Notifications",
            Page::Region => "Units and time",
            Page::Compositor => "Compositor",
            Page::WindowLooks => "Blur, shadows and windows",
            Page::Gaps => "Gaps",
            Page::Input => "Input and gestures",
            Page::LaunchedApps => "Apps it starts",
            Page::Keys => "Keybinds",
            Page::CustomKeys => "Your own keybinds",
            Page::Monitors => "Screens",
            Page::AllOptions => "Every option",
        };
        fixed.into()
    }

    pub fn parent(&self) -> Option<Page> {
        Some(match self {
            Page::Wallpapers | Page::Colours => Page::Look,
            Page::WallpaperFolder(folder) => match folder.rsplit_once('/') {
                Some((above, _)) => Page::WallpaperFolder(above.to_string()),
                None => Page::Wallpapers,
            },
            Page::Ethernet { .. } => Page::Network,
            Page::Device { .. } | Page::Pairing => Page::Bluetooth,
            Page::AppVolumes => Page::Audio,
            Page::OpensWith(_) | Page::AllApps | Page::AppGpus => Page::Apps,
            Page::App { .. } => Page::AllApps,
            Page::Bar | Page::Launcher | Page::Dashboard | Page::Sidebar => Page::Panels,
            Page::BarWorkspaces | Page::BarStatus => Page::Bar,
            Page::Notifications => Page::Services,
            Page::WindowLooks | Page::Gaps | Page::Input | Page::LaunchedApps | Page::Keys => Page::Compositor,
            Page::CustomKeys | Page::Monitors | Page::AllOptions => Page::Compositor,
            _ => return None,
        })
    }

    /// The way to a page from the side of the window, the page itself last.
    pub fn trail(self) -> Vec<Page> {
        let mut trail = vec![self];
        while let Some(parent) = trail[0].parent() {
            trail.insert(0, parent);
        }
        trail
    }

    /// The page a word on a command line means: `cae-shell settings audio`.
    fn named(word: &str) -> Option<Page> {
        frame::ROOTS.iter().flat_map(|group| group.iter()).find(|root| root.word == word).map(|root| root.page.clone())
    }
}

/// The one window, while there is one.
#[derive(Default)]
struct Open(Option<WindowHandle<Frame>>);

impl Global for Open {}

/// Opens the settings, or brings them forward if they are open, on the page
/// `word` names if it names one.
pub fn open(word: Option<&str>, cx: &mut App) {
    let page = word.and_then(Page::named);
    if let Some(window) = cx.default_global::<Open>().0 {
        let shown = window.update(cx, |frame, window, cx| {
            window.activate_window();
            if let Some(page) = page.clone() {
                frame.show(page, window, cx);
            }
        });
        // Closed by the compositor, which tells nobody: the handle is what is
        // left of it, and the only way to find out is to try it.
        if shown.is_ok() {
            return;
        }
    }

    let display = screen::focused_display(cx);
    let screen = cx
        .displays()
        .into_iter()
        .find(|candidate| Some(candidate.id()) == display)
        .or_else(|| cx.primary_display())
        .map_or(Size::new(px(1920.), px(1080.)), |display| display.bounds().size);
    let height = (screen.height * 0.72).clamp(px(520.), px(880.));
    let size = Size::new((height * 1.4).min(screen.width * 0.92), height);

    let options = WindowOptions {
        titlebar: None,
        display_id: display,
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(display, size, cx))),
        window_min_size: Some(Size::new(px(760.), px(480.))),
        app_id: Some(APP_ID.to_string()),
        window_background: WindowBackgroundAppearance::Transparent,
        // The compositor's, which on Hyprland is a border and rounded
        // corners and on anything else is whatever that desktop's windows
        // have. Drawing our own would be a second frame inside the first.
        window_decorations: Some(WindowDecorations::Server),
        ..Default::default()
    };

    let feeds = cx.global::<Feeds>().clone();
    let start = page.unwrap_or_else(|| frame::ROOTS[0][0].page.clone());
    match cx.open_window(options, move |window, cx| cx.new(|cx| Frame::new(start, &feeds, window, cx))) {
        Ok(window) => cx.set_global(Open(Some(window))),
        Err(error) => eprintln!("cae: cannot open the settings: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every page that is named by itself. The others are named by
    /// something found while running, and lead back the same way.
    fn named_by_themselves() -> Vec<Page> {
        use Page::*;
        vec![
            Look, Wallpapers, Colours, Network, Bluetooth, Pairing, Audio, AppVolumes, Updates, Apps, AllApps, AppGpus, About, Panels, Bar,
            BarWorkspaces, BarStatus, Launcher, Dashboard, Sidebar, Services, Notifications, Region, Compositor, WindowLooks, Gaps, Input,
            LaunchedApps, Keys, CustomKeys, Monitors, AllOptions,
        ]
    }

    /// The marker down the side and the way back along the top both follow
    /// the trail, so a page whose trail starts anywhere but at the side of
    /// the window has neither.
    #[test]
    fn every_page_leads_back_to_the_side_of_the_window() {
        let at_the_side = |page: &Page| frame::ROOTS.iter().flat_map(|group| group.iter()).any(|root| root.page == *page);
        let found_while_running = [
            Page::WallpaperFolder("City/Night".into()),
            Page::Ethernet { interface: "enp3s0".into(), connection: String::new() },
            Page::Device { address: "AA:BB".into(), name: "Headphones".into() },
            Page::App { id: "firefox".into(), name: "Firefox".into() },
            Page::OpensWith(pages::apps::Opens::Terminal),
        ];
        for page in named_by_themselves().into_iter().chain(found_while_running) {
            let trail = page.clone().trail();
            assert!(at_the_side(&trail[0]), "{page:?} leads back to {:?}, which is not at the side", trail[0]);
            assert!(!page.title().is_empty(), "{page:?} has no title");
        }
        assert_eq!(Page::WallpaperFolder("City/Night".into()).trail().len(), 4, "a folder in a folder is reached through it");
    }

    #[test]
    fn a_page_can_be_asked_for_by_name() {
        assert_eq!(Page::named("audio"), Some(Page::Audio));
        assert_eq!(Page::named("compositor"), Some(Page::Compositor));
        assert_eq!(Page::named("nowhere"), None);
    }
}
