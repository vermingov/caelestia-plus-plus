//! Which of GPUI's displays is which output, kept current for everything
//! that puts a surface on every screen.

use std::time::Duration;

use gpui::{App, AppContext, AsyncApp, DisplayId, Entity, Global};
use uuid::Uuid;

use crate::feeds::Feeds;

/// Which GPUI display is the output Hyprland calls by this name.
///
/// GPUI keeps a Wayland output's name to itself, but the UUID it reports is
/// made from that name and nothing else, so making the same UUID from
/// Hyprland's name finds it. Better than pairing by geometry, which two
/// mirrored outputs share.
pub fn display_named(cx: &App, output: &str) -> Option<DisplayId> {
    let wanted = Uuid::new_v5(&Uuid::NAMESPACE_DNS, output.as_bytes());
    cx.displays().into_iter().find(|display| display.uuid().ok() == Some(wanted)).map(|display| display.id())
}

/// Every output there is: what it is called, and which of GPUI's displays it
/// is. As last looked, where the shell keeps the list (see `keep`), and
/// looked up now where it does not yet.
pub fn outputs(cx: &App) -> Vec<(String, DisplayId)> {
    match cx.try_global::<Kept>() {
        Some(kept) if !kept.0.read(cx).list.is_empty() => kept.0.read(cx).list.clone(),
        _ => look(cx, &cae_core::hypr::monitors().into_iter().map(|(name, ..)| name).collect::<Vec<_>>()),
    }
}

/// The outputs `named`, each with its display.
///
/// Hyprland names them, because its name is what a config means by "this
/// screen". Under any other compositor nothing answers, and each display GPUI
/// knows is given a name of its own making: everything draws the same, and
/// only per-monitor workspaces have nothing to go on. Never empty, because
/// "the centre is open on no screen" is said with an empty name.
fn look(cx: &App, named: &[String]) -> Vec<(String, DisplayId)> {
    if named.is_empty() {
        return cx
            .displays()
            .into_iter()
            .enumerate()
            .map(|(index, display)| (format!("screen-{index}"), display.id()))
            .collect();
    }
    named.iter().filter_map(|name| Some((name.clone(), display_named(cx, name)?))).collect()
}

/// The outputs, as last looked. Everything that keeps a surface on every
/// screen watches this rather than looking for itself.
pub struct Screens {
    pub list: Vec<(String, DisplayId)>,
    /// Whether a look is already under way, so that a second is not started
    /// beside it.
    looking: bool,
}

struct Kept(Entity<Screens>);

impl Global for Kept {}

/// How long GPUI may take to hear of an output Hyprland has named, and how
/// often it is looked at meanwhile: at startup the displays arrive a few
/// milliseconds after the application does, and so does a monitor plugged in.
const CATCHING_UP: Duration = Duration::from_millis(25);
const PATIENCE: u32 = 200;

/// Where no compositor names its outputs, nothing says when one comes or
/// goes either, and the displays are looked at on this slow tick instead.
const UNNAMED_TICK: Duration = Duration::from_secs(5);

/// Keeps the list of outputs for the life of the shell, and returns it.
///
/// Looked at again when Hyprland says an output came or went, which it says
/// in the state the shell already follows — rather than each piece asking it
/// every two seconds, which was five questions to the compositor every two
/// seconds for the life of the session.
pub fn keep(cx: &mut App, feeds: &Feeds) -> Entity<Screens> {
    let screens = cx.new(|_| Screens { list: Vec::new(), looking: false });
    cx.set_global(Kept(screens.clone()));

    let hypr = feeds.hypr.clone();
    let watched = screens.clone();
    cx.observe(&hypr, move |hypr, cx| {
        let named = hypr.read(cx).value.outputs.clone();
        let screens = watched.read(cx);
        let settled = screens.list.iter().map(|(name, _)| name).eq(named.iter());
        if !settled && !screens.looking {
            catch_up(watched.clone(), hypr, cx);
        }
    })
    .detach();
    catch_up(screens.clone(), feeds.hypr.clone(), cx);
    screens
}

/// Looks until every output Hyprland names has a display, or it has waited
/// long enough; and, without Hyprland, on a slow tick for good.
fn catch_up(screens: Entity<Screens>, hypr: Entity<crate::feeds::Feed<cae_core::hypr::State>>, cx: &mut App) {
    screens.update(cx, |screens, _| screens.looking = true);
    cx.spawn(async move |cx: &mut AsyncApp| {
        let mut looks = 0;
        loop {
            let complete = cx.update(|cx| {
                let named = hypr.read(cx).value.outputs.clone();
                let list = look(cx, &named);
                let complete = !list.is_empty() && (named.is_empty() || list.len() == named.len());
                screens.update(cx, |screens, cx| {
                    if screens.list != list {
                        screens.list = list;
                        cx.notify();
                    }
                });
                (complete, named.is_empty())
            });
            looks += 1;
            let wait = match complete {
                (false, _) if looks < PATIENCE => CATCHING_UP,
                (_, true) => UNNAMED_TICK,
                _ => return cx.update(|cx| screens.update(cx, |screens, _| screens.looking = false)),
            };
            cx.background_executor().timer(wait).await;
        }
    })
    .detach();
}

/// The display the person is looking at, which is where anything that opens
/// because a key was pressed belongs: a keybind does not say where it was
/// pressed. Nothing when the compositor is not one that says, and whatever
/// opens then goes wherever the compositor puts it.
pub fn focused_display(cx: &App) -> Option<DisplayId> {
    display_named(cx, &cae_core::hypr::focused_monitor()?)
}

/// The size to ask for an axis a layer surface is anchored across: none.
///
/// Anchoring a surface to two opposite edges does not stretch it between
/// them. Asking for no size along that axis does, and any size that is asked
/// for is taken at its word: the surface is made that big and centred between
/// its anchors. A bar asked for at 1920 wide is the right width on the screen
/// it was written on and on no other.
pub const STRETCH: gpui::Pixels = gpui::px(0.);
