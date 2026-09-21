//! What every window the shell opens has in common.
//!
//! GPUI is built for an editor, where a window nobody is typing into is a
//! window nobody is watching, and holding it to 30fps saves a laptop's
//! battery for the one in front. A shell inverts that: the bar, the OSD and
//! the notification column are watched constantly and focused never. They are
//! layer surfaces, and GPUI only ever marks a window active on a
//! `wl_keyboard::Enter` — which a surface that declines the keyboard cannot
//! receive — so `is_active()` is false for their whole life and the saving
//! throttle applies to every frame of every animation they play. A 260ms
//! marker gets eight frames instead of sixteen, and reads as a stutter.
//!
//! Nothing here idles in the sense the throttle is guarding against: a
//! surface asks for a frame only while something is moving, and asks for none
//! once it has settled. So the cap comes off and the compositor's frame
//! callback does the pacing, which is the refresh rate and nothing faster.

use gpui::WindowOptions;

/// The shell's window defaults. Stands in for `Default::default()` in every
/// `WindowOptions` the shell builds.
pub fn options() -> WindowOptions {
    WindowOptions { inactive_frame_interval: None, ..Default::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shell_is_not_held_to_the_inactive_frame_rate() {
        assert!(
            options().inactive_frame_interval.is_none(),
            "a layer surface is never active, so a cap on inactive windows is a cap on everything"
        );
    }

    #[test]
    fn nothing_else_is_moved_off_gpuis_defaults() {
        let (ours, theirs) = (options(), WindowOptions::default());
        assert_eq!(ours.kind, theirs.kind);
        assert_eq!(ours.focus, theirs.focus);
        assert_eq!(ours.show, theirs.show);
        assert_eq!(ours.is_resizable, theirs.is_resizable);
    }
}
