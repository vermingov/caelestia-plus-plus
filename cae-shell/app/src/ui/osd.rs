//! The on-screen display: how loud, and how bright, said when either changes.
//!
//! A key turns the volume down and something has to say that it did. This is
//! that: a small pane against the right-hand edge, up for a moment after a
//! change and gone again, or for as long as the pointer is on it, because it
//! can also be dragged. Between changes it is a strip two pixels wide, which
//! is how reaching for that edge is heard.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use cae_core::{config, hypr, levels, system, volume};
use gpui::{
    AnyElement, AnyWindowHandle, App, AppContext, AsyncApp, Bounds, Context, DispatchPhase, DisplayId, Entity, Global, IntoElement,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Render, ScrollWheelEvent, Size, Styled, Task, WeakEntity, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions, canvas, div, layer_shell::*, point, prelude::*, px,
    relative,
};

use crate::actions;
use crate::ease::{Curve, Tween};
use crate::feeds::Feeds;
use crate::ours;
use crate::theme;
use crate::ui::glyph::glyph;
use crate::ui::screen;
use crate::ui::{Ask, pointer, rsx};

const PIECE: &str = "osd";
const NAMESPACE: &str = "caelestia-panel";

/// The strip along the right-hand edge that opens it, half way down.
const EDGE: Size<Pixels> = Size { width: px(2.), height: px(260.) };

const BAR: Pixels = px(150.);
const COLUMN: Pixels = px(44.);
const FLOAT: Pixels = px(12.);
const SHADOW: Pixels = px(44.);
const TRAVEL: f32 = 14.;
const ENTER: Duration = Duration::from_millis(200);
const LEAVE: Duration = Duration::from_millis(160);
/// Crossing from the edge onto the pane takes the pointer off both for a
/// moment, and that must not count as leaving.
const GRACE: Duration = Duration::from_millis(240);

/// What it shows the level of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Of {
    Volume,
    Microphone,
    Brightness,
}

/// What the settings say about it, read when it is about to be shown.
struct Settings {
    enabled: bool,
    stays: Duration,
    brightness: bool,
    microphone: bool,
    loudest: i64,
    over_fullscreen: bool,
}

impl Settings {
    fn read() -> Settings {
        let shell = config::read(config::File::Shell);
        let flag = |path: &str, otherwise: bool| config::lookup(&shell, path).and_then(serde_json::Value::as_bool).unwrap_or(otherwise);
        let number = |path: &str, otherwise: f64| config::lookup(&shell, path).and_then(serde_json::Value::as_f64).unwrap_or(otherwise);
        Settings {
            enabled: flag("osd.enabled", true),
            stays: Duration::from_millis(number("osd.hideDelay", 2000.).clamp(300., 20_000.) as u64),
            brightness: flag("osd.enableBrightness", true),
            microphone: flag("osd.enableMicrophone", false),
            loudest: levels::Steps::said_by(&shell).loudest,
            over_fullscreen: flag("general.showOverFullscreen", false),
        }
    }

    /// Whether it has a bar for that, given a device to have a level.
    fn shows(&self, of: Of) -> bool {
        match of {
            Of::Volume => true,
            Of::Microphone => self.microphone,
            Of::Brightness => self.brightness,
        }
    }
}

/// The three levels, as they were when last looked at: a change in any of
/// them is what brings the display up.
#[derive(Clone, Debug, Default, PartialEq)]
struct Levels {
    volume: Option<(i64, bool)>,
    microphone: Option<(i64, bool)>,
    brightness: Option<i64>,
}

impl Levels {
    fn of(snapshot: &system::Snapshot) -> Levels {
        let level = |end: &Option<volume::Volume>| end.as_ref().map(|end| (end.level, end.muted));
        Levels { volume: level(&snapshot.volume), microphone: level(&snapshot.microphone), brightness: snapshot.brightness }
    }

    /// Which of them are not what they were in `before`. A device arriving
    /// or leaving is not its level moving.
    fn moved_since(&self, before: &Levels) -> Vec<Of> {
        fn moved<Level: PartialEq>(before: &Option<Level>, now: &Option<Level>) -> bool {
            before.is_some() && now.is_some() && before != now
        }
        [
            moved(&before.volume, &self.volume).then_some(Of::Volume),
            moved(&before.microphone, &self.microphone).then_some(Of::Microphone),
            moved(&before.brightness, &self.brightness).then_some(Of::Brightness),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

pub struct Osds {
    feeds: Feeds,
    pane: Option<WindowHandle<Pane>>,
    /// On its way to being opened, once the compositor has said what is in
    /// front. An edge says it was reached for with every move along it, and
    /// one question at a time is enough.
    opening: bool,
    edges: HashMap<DisplayId, AnyWindowHandle>,
    seen: Option<Levels>,
}

struct Shared(Entity<Osds>);

impl Global for Shared {}

/// Shows it, or takes it away: what a key asks for, since nothing else here
/// is a key's to ask.
pub fn ask(ask: Ask, cx: &mut App) {
    let Some(osds) = cx.try_global::<Shared>().map(|shared| shared.0.clone()) else { return };
    osds.update(cx, |osds, cx| match ask {
        Ask::Show => osds.show(None, None, cx),
        Ask::Hide => osds.hide(cx),
        Ask::Toggle if osds.pane.is_some() => osds.hide(cx),
        Ask::Toggle => osds.show(None, None, cx),
    });
}

/// Keeps the edge that opens it on every output, and watches the levels, for
/// the life of the shell.
pub fn keep_on_every_output(cx: &mut App, feeds: &Feeds) {
    let osds = cx.new(|cx| {
        cx.observe(&feeds.system, |osds: &mut Osds, _, cx| osds.levels_moved(cx)).detach();
        Osds { feeds: feeds.clone(), pane: None, opening: false, edges: HashMap::new(), seen: None }
    });
    cx.set_global(Shared(osds.clone()));

    cx.spawn(async move |cx: &mut AsyncApp| {
        let mut looks = 0_u32;
        loop {
            let found = osds.update(cx, |osds, cx| osds.edges(cx));
            looks += 1;
            let wait = if found == 0 && looks < 200 { 25 } else { 2000 };
            cx.background_executor().timer(Duration::from_millis(wait)).await;
        }
    })
    .detach();
}

fn surface(display: Option<DisplayId>, size: Size<Pixels>) -> WindowOptions {
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
            // The right-hand edge only, which leaves the compositor to put
            // it half way down.
            anchor: Anchor::RIGHT,
            keyboard_interactivity: KeyboardInteractivity::None,
            ..Default::default()
        }),
        ..Default::default()
    }
}

impl Osds {
    fn edges(&mut self, cx: &mut Context<Self>) -> usize {
        let outputs = screen::outputs(cx);
        // Quickshell has one of these on the same edge, and one started
        // before it knew to stand down still draws it.
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
        let osds = cx.weak_entity();
        for (name, display) in &outputs {
            if self.edges.contains_key(display) {
                continue;
            }
            let (osds, display) = (osds.clone(), *display);
            match cx.open_window(surface(Some(display), EDGE), move |_, cx| cx.new(|_| Edge { osds, display })) {
                Ok(edge) => drop(self.edges.insert(display, edge.into())),
                Err(error) => eprintln!("cae: cannot open the on-screen display's edge on {name}: {error}"),
            }
        }
        outputs.len()
    }

    /// The readouts have been read again. If one of the three levels is not
    /// what it was, that is said.
    fn levels_moved(&mut self, cx: &mut Context<Self>) {
        let now = Levels::of(&self.feeds.system.read(cx).value);
        // The first reading is not a change.
        let Some(before) = self.seen.replace(now.clone()) else { return };
        let moved = now.moved_since(&before);
        if !moved.is_empty() && ours::is_ours(PIECE, cx) {
            self.show(None, Some(&moved), cx);
        }
    }

    /// Brings it up, or keeps it up. `moved` is the levels that changed, when
    /// that is why, and nothing when it was reached for.
    fn show(&mut self, display: Option<DisplayId>, moved: Option<&[Of]>, cx: &mut Context<Self>) {
        // A level it has no bar for is not something to come up and say: a
        // call that rides the microphone's gain would have it up all evening.
        let worth_saying = |settings: &Settings| moved.is_none_or(|moved| moved.iter().any(|of| settings.shows(*of)));
        let stay = |pane: &mut Pane, window: &mut Window, cx: &mut Context<Pane>| {
            if worth_saying(&pane.settings) {
                pane.stay(window, cx);
            }
        };
        if let Some(pane) = &self.pane
            && pane.update(cx, stay).is_ok()
        {
            return;
        }
        if self.opening {
            return;
        }
        let settings = Settings::read();
        if !settings.enabled || !worth_saying(&settings) {
            return;
        }
        // Over a fullscreen window only where the settings say so, and never
        // because its edge was reached for: the edge of a game is part of
        // the game.
        let by_hover = moved.is_none();
        let may_cover = settings.over_fullscreen && !by_hover;
        self.opening = true;
        cx.spawn(async move |osds, cx| {
            let covered = !may_cover && cx.background_spawn(async { hypr::fullscreen_focused() }).await;
            let _ = osds.update(cx, |osds, cx| {
                osds.opening = false;
                if !covered {
                    osds.open(display, settings, by_hover, cx);
                }
            });
        })
        .detach();
    }

    fn open(&mut self, display: Option<DisplayId>, settings: Settings, by_hover: bool, cx: &mut Context<Self>) {
        let display = display.or_else(|| screen::focused_display(cx)).or_else(|| screen::outputs(cx).first().map(|(_, display)| *display));
        let (feeds, osds) = (self.feeds.clone(), cx.weak_entity());
        let size = Size::new(Pane::width(&settings, &self.feeds, cx) + FLOAT + SHADOW, Pane::HEIGHT + SHADOW * 2.);
        let opened = cx.open_window(surface(display, size), move |window, cx| cx.new(|cx| Pane::new(settings, by_hover, &feeds, osds, window, cx)));
        self.pane = opened.map_err(|error| eprintln!("cae: cannot open the on-screen display: {error}")).ok();
    }

    fn hide(&mut self, cx: &mut Context<Self>) {
        if let Some(pane) = &self.pane {
            let _ = pane.update(cx, |pane, window, cx| pane.leave(window, cx));
        }
    }

    fn gone(&mut self) {
        self.pane = None;
    }
}

/// The strip along the right-hand edge of one screen.
struct Edge {
    osds: WeakEntity<Osds>,
    display: DisplayId,
}

impl Render for Edge {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (osds, display) = (self.osds.clone(), self.display);
        rsx! {
            <div
                id="edge"
                class="size-full"
                onMouseMove={move |_: &MouseMoveEvent, _: &mut Window, cx: &mut App| {
                    let _ = osds.update(cx, |osds, cx| osds.show(Some(display), None, cx));
                }}
            />
        }
    }
}

/// A level being dragged: where it has been put, until the machine says the
/// same. Setting a volume is a program started and an event waited for, and a
/// bar that trails the pointer by that long feels broken.
#[derive(Clone, Default)]
struct Held {
    dragging: Rc<Cell<bool>>,
    at: Rc<Cell<Option<i64>>>,
}

pub struct Pane {
    settings: Settings,
    feeds: Feeds,
    osds: WeakEntity<Osds>,
    shown: Tween,
    hovered: bool,
    hiding: Option<Task<()>>,
    /// On its way out, and what takes the surface away when it has gone.
    leaving: Option<Task<()>>,
    held: [Held; 3],
}

impl Pane {
    const HEIGHT: Pixels = px(232.);

    fn columns(settings: &Settings, feeds: &Feeds, cx: &App) -> Vec<Of> {
        let system = &feeds.system.read(cx).value;
        let has_a_level = |of: Of| match of {
            Of::Volume => system.volume.is_some(),
            Of::Microphone => system.microphone.is_some(),
            Of::Brightness => system.brightness.is_some(),
        };
        [Of::Volume, Of::Microphone, Of::Brightness].into_iter().filter(|of| settings.shows(*of) && has_a_level(*of)).collect()
    }

    fn width(settings: &Settings, feeds: &Feeds, cx: &App) -> Pixels {
        COLUMN * Pane::columns(settings, feeds, cx).len().max(1) as f32 + px(20.)
    }

    fn new(settings: Settings, by_hover: bool, feeds: &Feeds, osds: WeakEntity<Osds>, window: &mut Window, cx: &mut Context<Self>) -> Pane {
        cx.observe(&feeds.system, |_, _, cx| cx.notify()).detach();
        let mut shown = Tween::still(0.);
        shown.go(1., ENTER, Curve::Arrive);
        let mut pane =
            Pane { settings, feeds: feeds.clone(), osds, shown, hovered: by_hover, hiding: None, leaving: None, held: Default::default() };
        pane.stay(window, cx);
        pane
    }

    /// One that was on its way out comes back: what moved as it left is
    /// still worth saying, and a pointer that caught it is still on it.
    fn come_back(&mut self, cx: &mut Context<Self>) {
        if self.leaving.take().is_some() {
            self.shown.go(1., ENTER, Curve::Arrive);
            cx.notify();
        }
    }

    /// Stays up for as long as the settings say from now, and longer if the
    /// pointer is on it when that is up.
    fn stay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.come_back(cx);
        let wait = self.settings.stays;
        self.hiding = Some(cx.spawn_in(window, async move |pane, cx| {
            cx.background_executor().timer(wait).await;
            let _ = pane.update_in(cx, |pane, window, cx| {
                if !pane.hovered && !pane.held.iter().any(|held| held.dragging.get()) {
                    pane.leave(window, cx);
                }
            });
        }));
    }

    fn pointer(&mut self, hovered: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.hovered = hovered;
        if hovered {
            self.hiding = None;
            return self.come_back(cx);
        }
        self.hiding = Some(cx.spawn_in(window, async move |pane, cx| {
            cx.background_executor().timer(GRACE).await;
            let _ = pane.update_in(cx, |pane, window, cx| {
                if !pane.hovered {
                    pane.leave(window, cx);
                }
            });
        }));
    }

    fn leave(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.leaving.is_some() {
            return;
        }
        self.shown.go(0., LEAVE, Curve::In);
        cx.notify();
        let osds = self.osds.clone();
        self.leaving = Some(cx.spawn_in(window, async move |_, cx| {
            cx.background_executor().timer(LEAVE).await;
            let _ = cx.update(|window, cx| {
                window.remove_window();
                let _ = osds.update(cx, |osds, _| osds.gone());
            });
        }));
    }

    /// One level: a bar that fills from its foot, and under it what it is
    /// the level of.
    fn column(&self, index: usize, of: Of, cx: &mut Context<Self>) -> AnyElement {
        let system = &self.feeds.system.read(cx).value;
        let (level, muted, top) = match of {
            Of::Volume => system.volume.as_ref().map_or((0, false, 100), |end| (end.level, end.muted, self.settings.loudest)),
            Of::Microphone => system.microphone.as_ref().map_or((0, false, 100), |end| (end.level, end.muted, 100)),
            Of::Brightness => (system.brightness.unwrap_or(0), false, 100),
        };
        let held = self.held[index].clone();
        let shown = held.at.get().unwrap_or(level).clamp(0, top);
        let symbol = match of {
            Of::Volume if muted || shown == 0 => "volume_off".to_string(),
            Of::Volume if shown <= 40 => "volume_down".to_string(),
            Of::Volume => "volume_up".to_string(),
            Of::Microphone if muted => "mic_off".to_string(),
            Of::Microphone => "mic".to_string(),
            Of::Brightness => format!("brightness_{}", (shown as f32 / 100. * 6.).round() as i64 + 1),
        };
        let set = move |value: i64, cx: &mut App| {
            cx.background_spawn(async move {
                match of {
                    Of::Volume => volume::set(value, top),
                    Of::Microphone => volume::set_microphone(value),
                    Of::Brightness => system::set_brightness(value.max(1)),
                }
            })
            .detach();
        };
        let listen = move |track: Bounds<Pixels>, window: &mut Window| {
            let value_at = move |y: Pixels| (((track.bottom() - y) / track.size.height).clamp(0., 1.) * top as f32).round() as i64;
            let put = {
                let (held, set) = (held.clone(), set.clone());
                move |y: Pixels, window: &mut Window, cx: &mut App| {
                    let value = value_at(y);
                    if held.at.replace(Some(value)) != Some(value) {
                        set(value, cx);
                        window.refresh();
                    }
                }
            };
            window.on_mouse_event({
                let (held, put) = (held.clone(), put.clone());
                move |event: &MouseDownEvent, phase, window, cx| {
                    let on_bar = track.dilate(px(10.)).contains(&event.position);
                    if phase == DispatchPhase::Bubble && event.button == MouseButton::Left && on_bar {
                        held.dragging.set(true);
                        put(event.position.y, window, cx);
                    }
                }
            });
            window.on_mouse_event({
                let held = held.clone();
                move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble && held.dragging.get() && event.dragging() {
                        put(event.position.y, window, cx);
                    }
                }
            });
            window.on_mouse_event({
                let held = held.clone();
                move |_: &MouseUpEvent, phase, window, cx| {
                    if phase != DispatchPhase::Bubble || !held.dragging.replace(false) {
                        return;
                    }
                    // Let go of once the real value has had time to arrive.
                    let (held, handle) = (held.clone(), window.window_handle());
                    cx.spawn(async move |cx| {
                        cx.background_executor().timer(Duration::from_millis(400)).await;
                        if !held.dragging.get() {
                            held.at.set(None);
                            let _ = handle.update(cx, |_, window, _| window.refresh());
                        }
                    })
                    .detach();
                }
            });
        };

        let part = (shown as f32 / top as f32).clamp(0., 1.);
        rsx! {
            <div
                id={("level", index)}
                class="flex flex-col flex-none items-center gap-[10px] cursor-pointer"
                w={COLUMN}
                onScrollWheel={move |event: &ScrollWheelEvent, _: &mut Window, cx: &mut App| {
                    let up = f32::from(event.delta.pixel_delta(px(20.)).y) > 0.;
                    match of {
                        Of::Volume => actions::volume(cx, up),
                        Of::Microphone => actions::microphone(cx, up),
                        Of::Brightness => actions::brightness(cx, up),
                    }
                }}
            >
                <div class="flex-none" text_size={px(11.5)} text_color={theme::text_dim()} font_features={theme::tabular()}>
                    {if muted { "Muted".to_string() } else { shown.to_string() }}
                </div>
                <div class="relative flex flex-col flex-none justify-end w-[8px] rounded-full overflow-hidden" h={BAR} bg={theme::white(0.09)}>
                    <canvas class="absolute size-full" prepaint={|_, _, _| ()} paint={move |track, _, window, _| listen(track, window)} />
                    <div class="w-full rounded-full" h={relative(part)} bg={if muted { theme::white(0.3) } else { theme::text() }} />
                </div>
                <div class="flex-none" text_color={if muted { theme::text_faint() } else { theme::text_dim() }}>{glyph(symbol, px(18.))}</div>
            </div>
        }
        .into_any_element()
    }
}

impl Render for Pane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.shown.done() {
            window.request_animation_frame();
        }
        let shown = self.shown.value();
        let columns = Pane::columns(&self.settings, &self.feeds, cx);
        let width = COLUMN * columns.len().max(1) as f32 + px(20.);
        let bars: Vec<AnyElement> = columns.into_iter().enumerate().map(|(index, of)| self.column(index, of, cx)).collect();

        rsx! {
            <div class="relative size-full" font_family={theme::FONT}>
                // The pane and the gap between it and the edge, as one thing
                // to be on or off: a pointer resting on the edge it was
                // reached for from is on it.
                <div
                    id="osd"
                    class="absolute flex items-center"
                    right={px(0.)}
                    top={SHADOW}
                    w={width + FLOAT}
                    h={Pane::HEIGHT}
                    onHover={cx.listener(|pane, hovered: &bool, window, cx| pane.pointer(*hovered, window, cx))}
                >
                    <div
                        class="flex flex-none items-center justify-center h-full"
                        w={width}
                        opacity={shown}
                        ml={px(TRAVEL * (1. - shown))}
                        rounded={px(15.)}
                        bg={theme::pane()}
                        shadow={theme::pane_shadows()}
                        text_color={theme::text()}
                    >
                        {...bars}
                    </div>
                </div>
                <canvas
                    class="absolute size-full"
                    prepaint={move |_, window: &mut Window, _: &mut App| {
                        let surface = window.viewport_size();
                        let region = Bounds::new(point(surface.width - width - FLOAT, SHADOW), Size::new(width + FLOAT, Pane::HEIGHT));
                        window.set_input_region(Some(&[region]));
                    }}
                    paint={|_, _, _, _| ()}
                />
                {pointer::see_out()}
            </div>
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Levels, Of};

    #[test]
    fn a_level_that_was_one_thing_and_is_another_has_moved() {
        let before = Levels { volume: Some((40, false)), microphone: Some((80, false)), brightness: Some(70) };
        assert_eq!(before.moved_since(&before), []);
        let louder = Levels { volume: Some((45, false)), ..before.clone() };
        assert_eq!(louder.moved_since(&before), [Of::Volume]);
        // Muting moves nothing and is still something to say.
        let muted = Levels { microphone: Some((80, true)), brightness: Some(60), ..before.clone() };
        assert_eq!(muted.moved_since(&before), [Of::Microphone, Of::Brightness]);
    }

    #[test]
    fn a_device_arriving_or_leaving_is_not_its_level_moving() {
        let without = Levels { volume: Some((40, false)), microphone: None, brightness: None };
        let with = Levels { volume: Some((40, false)), microphone: Some((80, false)), brightness: Some(70) };
        assert_eq!(with.moved_since(&without), []);
        assert_eq!(without.moved_since(&with), []);
    }
}
