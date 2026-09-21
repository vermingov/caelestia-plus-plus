//! The two marks the bar can draw itself.
//!
//! Compiled in rather than read off the disk: GPUI tints an SVG with the text
//! colour, which is how the mark wears the accent, and that only works for
//! assets it loads through here.

use std::borrow::Cow;

use gpui::{App, AssetSource, Result, SharedString};

/// Rubik, at the three weights the shell sets text in.
///
/// Carried rather than expected: the copy most systems have is a variable
/// font, and the text stack GPUI uses on Linux picks a face by weight without
/// driving a variable font's weight axis, so every weight came out regular.
/// These are static instances cut from that same font (SIL OFL 1.1; the
/// licence is beside them in assets/fonts).
pub fn install_fonts(cx: &App) {
    let fonts = vec![
        Cow::Borrowed(&include_bytes!("../assets/fonts/Rubik-400.ttf")[..]),
        Cow::Borrowed(&include_bytes!("../assets/fonts/Rubik-500.ttf")[..]),
        Cow::Borrowed(&include_bytes!("../assets/fonts/Rubik-600.ttf")[..]),
    ];
    if let Err(error) = cx.text_system().add_fonts(fonts) {
        eprintln!("cae: could not load the bundled fonts: {error}");
    }
}

pub struct Assets;

const MARKS: [(&str, &[u8]); 2] = [
    ("marks/caelestia.svg", include_bytes!("../assets/logo.svg")),
    ("marks/cachyos.svg", include_bytes!("../assets/cachyos-rounded.svg")),
];

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(MARKS.iter().find(|(name, _)| *name == path).map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(MARKS.iter().filter(|(name, _)| name.starts_with(path)).map(|(name, _)| (*name).into()).collect())
    }
}
