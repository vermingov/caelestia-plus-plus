//! Everything that is drawn.
//!
//! Written as markup. `rsx!` is gpui-rsx's macro: it turns tags into the GPUI
//! builder calls they stand for while the shell is compiled, so none of it is
//! in the binary and a tag costs exactly what the calls would have. Layout
//! that never changes goes in `class`; anything that is a token of the theme
//! or is worked out while drawing is an attribute, which becomes the builder
//! call of the same name.

pub mod bar;
pub mod controls;
pub mod dashboard;
pub mod dial;
pub mod eggs;
pub mod features;
pub mod field;
pub mod glyph;
pub mod guard;
pub mod launcher;
pub mod lock;
pub mod notifs;
pub mod osd;
pub mod picker;
pub mod pointer;
pub mod popout;
pub mod screen;
pub mod security;
pub mod session;
pub mod settings;
pub mod slider;
pub mod surface;
pub mod utilities;

/// What a key, or somebody at the door, asks of a panel that opens and
/// shuts. The launcher has one of its own: what it opens with is a query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    Show,
    Hide,
    Toggle,
}

impl Ask {
    pub fn named(word: Option<&str>) -> Ask {
        match word {
            Some("show") => Ask::Show,
            Some("hide") => Ask::Hide,
            _ => Ask::Toggle,
        }
    }
}

/// The strict form, for the whole shell. The plain one drops a class it does
/// not know and the style is simply never applied; this one refuses to
/// compile it.
pub(crate) use gpui_rsx::rsx_strict as rsx;
