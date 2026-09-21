//! The clock on the desktop, for a desktop that is meant to be looked at.
//!
//! Off unless the settings ask for it. It is part of the background rather
//! than a panel of its own — it goes where the wallpaper goes, it is the
//! thing furthest back that is still read, and it answers for the same piece
//! Quickshell hands over.
//!
//! The surface is the clock's own rectangle and no larger. The QML one was
//! the whole screen with the clock drawn somewhere in it, which is a whole
//! screen of pointer events caught by something nobody can press.

use cae_core::config;
use gpui::{
    App, AppContext, Bounds, Context, Entity, FontWeight, IntoElement, Pixels, Render, Size, Styled, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions, div, layer_shell::*, point,
    prelude::*, px,
};
use serde_json::Value;

use crate::clock;
use crate::feeds::Feeds;
use crate::theme;
use crate::ui::rsx;
use crate::ui::screen;
use gpui::DisplayId;

/// The name the compositor already blurs behind, which is how the plate gets
/// its glass without a shader and without a rule of its own.
const NAMESPACE: &str = "caelestia-panel";

/// The rectangle the clock takes at scale 1, tuned to the type below: the
/// time at 96, the date column beside it, and the padding a plate wants.
/// Wide enough for the longest month — September, at 21 — with room left
/// either side of it, so the box is the same width all year.
const WIDTH: f32 = 512.;
const HEIGHT: f32 = 168.;
/// What a twelve-hour clock needs beyond that for its am or pm.
const HALF_DAY: f32 = 46.;

/// How far off the screen's edge it sits, and how far below a bar that is
/// there whether or not this shell drew it.
const MARGIN: f32 = 28.;
fn under_the_bar() -> Pixels {
    theme::PILL + theme::FLOAT
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Down {
    Top,
    Middle,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Across {
    Left,
    Centre,
    Right,
}

/// Where on the screen it sits, as the settings' `top-left` … `bottom-right`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Spot(Down, Across);

impl Spot {
    fn named(said: &str) -> Spot {
        let (down, across) = said.split_once('-').unwrap_or(("bottom", "right"));
        Spot(
            match down {
                "top" => Down::Top,
                "middle" => Down::Middle,
                _ => Down::Bottom,
            },
            match across {
                "left" => Across::Left,
                "center" | "centre" => Across::Centre,
                _ => Across::Right,
            },
        )
    }

    /// An edge it is held to, per axis. Held to neither edge of an axis, the
    /// compositor centres it along that one, which is what the middle and
    /// the centre are.
    fn anchor(self) -> Anchor {
        let down = match self.0 {
            Down::Top => Anchor::TOP,
            Down::Middle => Anchor::empty(),
            Down::Bottom => Anchor::BOTTOM,
        };
        let across = match self.1 {
            Across::Left => Anchor::LEFT,
            Across::Centre => Anchor::empty(),
            Across::Right => Anchor::RIGHT,
        };
        down | across
    }

    /// Top, right, bottom, left, as the protocol takes them. Only the edges
    /// it is held to matter, and the top one also clears the bar.
    fn margins(self) -> (Pixels, Pixels, Pixels, Pixels) {
        let top = if self.0 == Down::Top { px(MARGIN) + under_the_bar() } else { px(MARGIN) };
        (top, px(MARGIN), px(MARGIN), px(MARGIN))
    }
}

/// What the settings say about it.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    on: bool,
    /// Everything is drawn at this multiple, the rectangle included.
    scale: f32,
    at: Spot,
    /// How solid the plate behind it is, or none for no plate.
    plate: Option<f32>,
    /// Depth under the plate. A box shadow wants a box, so it is drawn only
    /// where there is one: GPUI has no shadow for glyphs alone.
    shadow: bool,
    twelve_hour: bool,
}

impl Settings {
    pub fn read() -> Settings {
        Settings::said_by(&config::read(config::File::Shell))
    }

    fn said_by(shell: &Value) -> Settings {
        let at = |path: &str| config::lookup(shell, path);
        let flag = |path: &str, otherwise: bool| at(path).and_then(Value::as_bool).unwrap_or(otherwise);
        let number = |path: &str, otherwise: f64| at(path).and_then(Value::as_f64).unwrap_or(otherwise);
        Settings {
            on: flag("background.desktopClock.enabled", false) && flag("background.enabled", true),
            // A clock at no size is not a clock, and one the size of a wall
            // is a mistake somebody wants to see rather than a crash.
            scale: number("background.desktopClock.scale", 1.).clamp(0.4, 4.) as f32,
            at: Spot::named(at("background.desktopClock.position").and_then(Value::as_str).unwrap_or("bottom-right")),
            // On unless it is switched off, where upstream has it off unless
            // switched on. Upstream's clock carried a shadow on the glyphs
            // themselves, which kept it readable over anything; GPUI has no
            // such shadow, so without the plate a bright wallpaper simply
            // eats the type.
            plate: flag("background.desktopClock.background.enabled", true)
                .then(|| number("background.desktopClock.background.opacity", 0.7).clamp(0., 1.) as f32),
            shadow: flag("background.desktopClock.shadow.enabled", true),
            twelve_hour: flag("services.useTwelveHourClock", false),
        }
    }

    fn size(&self) -> Size<Pixels> {
        let width = if self.twelve_hour { WIDTH + HALF_DAY } else { WIDTH };
        Size::new(px(width * self.scale), px(HEIGHT * self.scale))
    }
}

/// The face, and the windows it is drawn in — one per output.
pub struct Clocks {
    settings: Settings,
    open: Vec<WindowHandle<Face>>,
    /// What the open windows were made for, so that nothing is remade while
    /// nothing has changed.
    showing: Option<(Settings, Vec<DisplayId>)>,
}

impl Clocks {
    pub fn new(cx: &mut Context<Self>) -> Clocks {
        // The minute turns whether or not anything else happens.
        cx.spawn(async move |clocks, cx| {
            loop {
                cx.background_executor().timer(clock::until_next_minute()).await;
                if clocks.update(cx, |clocks: &mut Clocks, cx| clocks.retime(cx)).is_err() {
                    return;
                }
            }
        })
        .detach();
        Clocks { settings: Settings::read(), open: Vec::new(), showing: None }
    }

    /// Redraws what is already up, which is all a new minute needs.
    fn retime(&mut self, cx: &mut Context<Self>) {
        for window in &self.open {
            let _ = window.update(cx, |_, _, cx| cx.notify());
        }
    }

    /// Opens, closes and moves the windows so that they are what the
    /// settings and the outputs ask for now.
    pub fn follow(&mut self, ours: bool, cx: &mut Context<Self>) {
        let wanted = self.settings.on && ours;
        let displays: Vec<DisplayId> = if wanted { screen::outputs(cx).into_iter().map(|(_, display)| display).collect() } else { Vec::new() };
        if self.showing.as_ref() == Some(&(self.settings.clone(), displays.clone())) {
            return;
        }
        for window in self.open.drain(..) {
            let _ = window.update(cx, |_, window, _| window.remove_window());
        }
        for display in &displays {
            let settings = self.settings.clone();
            let opened = cx.open_window(surface(*display, &settings), |_, cx| cx.new(|_| Face { settings }));
            match opened {
                Ok(window) => self.open.push(window),
                Err(error) => eprintln!("cae: cannot draw the desktop clock: {error}"),
            }
        }
        self.showing = Some((self.settings.clone(), displays));
    }

    /// Reads the settings again. The windows follow on the next look.
    pub fn reread(&mut self) {
        self.settings = Settings::read();
    }
}

fn surface(display: DisplayId, settings: &Settings) -> WindowOptions {
    WindowOptions {
        titlebar: None,
        focus: false,
        show: true,
        display_id: Some(display),
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(0.), px(0.)), settings.size()))),
        app_id: Some(NAMESPACE.to_string()),
        window_background: WindowBackgroundAppearance::Transparent,
        kind: WindowKind::LayerShell(LayerShellOptions {
            namespace: NAMESPACE.to_string(),
            // Over the wallpaper, under everything anybody works in.
            layer: Layer::Bottom,
            anchor: settings.at.anchor(),
            margin: Some(settings.at.margins()),
            // It reserves nothing: windows tile over it, as over a wallpaper.
            exclusive_zone: Some(px(-1.)),
            keyboard_interactivity: KeyboardInteractivity::None,
            ..Default::default()
        }),
        ..crate::ui::surface::options()
    }
}

/// One clock on one screen.
struct Face {
    settings: Settings,
}

impl Render for Face {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let scale = self.settings.scale;
        let big = |at: f32| px(at * scale);
        let (hour, minute, half) = clock::hour_and_minute(self.settings.twelve_hour);
        let (month, day, weekday) = clock::today_worded();

        // The plate, where the settings ask for one. Without it the clock is
        // the type alone on the wallpaper, which is what the QML one did
        // too once its shader grab is taken away.
        let mut plate = rsx! {
            <div class="flex size-full items-center justify-center" font_family={theme::FONT} rounded={big(26.)} text_color={theme::text()} />
        };
        if let Some(solidity) = self.settings.plate {
            plate = plate.bg(theme::black(solidity));
            if self.settings.shadow {
                plate = plate.shadow(theme::pane_shadows());
            }
        }

        rsx! {
            <div base={plate}>
                <div class="flex items-baseline" text_size={big(96.)} line_height={big(104.)} font_weight={FontWeight::SEMIBOLD} font_features={theme::tabular()}>
                    {hour}
                    <div text_color={theme::accent()}>{":"}</div>
                    {minute}
                    {...(!half.is_empty()).then(|| rsx! {
                        <div class="self-start" pl={big(8.)} pt={big(14.)} text_size={big(22.)} font_weight={FontWeight::MEDIUM} text_color={theme::text_dim()}>
                            {half}
                        </div>
                    })}
                </div>

                <div class="flex-none" w={big(3.)} h={big(96.)} mx={big(26.)} rounded={big(2.)} bg={theme::white(0.22)} />

                <div class="flex flex-col justify-center">
                    <div text_size={big(21.)} font_weight={FontWeight::MEDIUM} text_color={theme::text_dim()}>{month.to_uppercase()}</div>
                    <div text_size={big(44.)} line_height={big(50.)} font_weight={FontWeight::SEMIBOLD} font_features={theme::tabular()}>{day}</div>
                    <div text_size={big(15.)} text_color={theme::text_faint()}>{weekday}</div>
                </div>
            </div>
        }
    }
}

/// Kept by the background, which is what holds it for the life of the shell.
pub fn keep(cx: &mut App, feeds: &Feeds) -> Entity<Clocks> {
    cx.new(|cx: &mut Context<Clocks>| {
        cx.observe(&feeds.settings, |clocks: &mut Clocks, _, _| clocks.reread()).detach();
        Clocks::new(cx)
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_file_that_says_nothing_leaves_the_desktop_bare() {
        let settings = Settings::said_by(&json!(null));
        assert!(!settings.on);
        assert_eq!(settings.at, Spot(Down::Bottom, Across::Right));
        assert_eq!(settings.plate, Some(0.7), "the plate is what keeps it readable, so it is on unless switched off");
        assert!(settings.shadow);
    }

    #[test]
    fn a_position_names_the_edges_it_is_held_to() {
        assert_eq!(Spot::named("top-left").anchor(), Anchor::TOP | Anchor::LEFT);
        assert_eq!(Spot::named("bottom-center").anchor(), Anchor::BOTTOM);
        assert_eq!(Spot::named("middle-center").anchor(), Anchor::empty(), "held to nothing is centred on both");
        assert_eq!(Spot::named("middle-right").anchor(), Anchor::RIGHT);
        assert_eq!(Spot::named("nonsense").anchor(), Anchor::BOTTOM | Anchor::RIGHT, "where it is by default");
    }

    #[test]
    fn only_the_top_of_the_screen_has_a_bar_to_clear() {
        let (top, ..) = Spot::named("top-left").margins();
        assert!(top > px(MARGIN));
        assert_eq!(Spot::named("bottom-left").margins().0, px(MARGIN));
    }

    #[test]
    fn the_settings_are_read_and_kept_within_reason() {
        let shell = json!({ "background": { "desktopClock": {
            "enabled": true, "scale": 99., "position": "top-center",
            "background": { "enabled": true, "opacity": 3. },
            "shadow": { "enabled": false }
        } } });
        let settings = Settings::said_by(&shell);
        assert!(settings.on);
        assert_eq!(settings.scale, 4., "a clock the size of a wall is still a clock");
        assert_eq!(settings.at, Spot(Down::Top, Across::Centre));
        assert_eq!(settings.plate, Some(1.));
        assert!(!settings.shadow);
    }

    #[test]
    fn a_plate_switched_off_stays_off() {
        let shell = json!({ "background": { "desktopClock": { "background": { "enabled": false } } } });
        assert_eq!(Settings::said_by(&shell).plate, None);
    }

    #[test]
    fn a_clock_the_background_is_switched_off_for_is_off_too() {
        let shell = json!({ "background": { "enabled": false, "desktopClock": { "enabled": true } } });
        assert!(!Settings::said_by(&shell).on);
    }

    #[test]
    fn a_twelve_hour_clock_is_given_room_for_its_half() {
        let wider = Settings { twelve_hour: true, ..Settings::said_by(&json!(null)) };
        assert!(wider.size().width > Settings::said_by(&json!(null)).size().width);
    }
}
