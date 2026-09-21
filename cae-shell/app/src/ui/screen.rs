//! Which of GPUI's displays is which output.

use gpui::{App, DisplayId};
use uuid::Uuid;

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
/// is.
///
/// Hyprland names them, because its name is what a config means by "this
/// screen". Under any other compositor nothing answers, and each display GPUI
/// knows is given a name of its own making: everything draws the same, and
/// only per-monitor workspaces have nothing to go on. Never empty, because
/// "the centre is open on no screen" is said with an empty name.
pub fn outputs(cx: &App) -> Vec<(String, DisplayId)> {
    let named = cae_core::hypr::monitors();
    if named.is_empty() {
        return cx
            .displays()
            .into_iter()
            .enumerate()
            .map(|(index, display)| (format!("screen-{index}"), display.id()))
            .collect();
    }
    // Hyprland can name an output a moment before GPUI has heard of it. The
    // next look picks it up.
    named.into_iter().filter_map(|(name, ..)| Some((name.clone(), display_named(cx, &name)?))).collect()
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
