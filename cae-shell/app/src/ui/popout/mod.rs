//! Popouts: what an item on the bar says when the pointer rests on it.
//!
//! Each is a window of its own and exists only while it is open: a popup the
//! compositor hangs from the bar's surface, under the item that opened it.
//! Nothing is kept warm for one, so a bar nobody is pointing at is fifty
//! pixels of surface and that is all.

mod audio;
mod bluetooth;
mod join;
mod media;
mod menu;
mod network;
mod pieces;
mod power;
mod simple;

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyView, App, AppContext, Bounds, Context, Div, Entity, IntoElement, MouseButton, Pixels,
    Render, SharedString, Size, Stateful, Styled, Task, WeakEntity, Window, WindowBackgroundAppearance, WindowBounds,
    WindowHandle, WindowKind, WindowOptions, canvas, div, point,
    popup::{PopupAnchor, PopupConstraintAdjustment, PopupGravity, PopupOptions},
    prelude::*,
    px,
};

use crate::feeds::Feeds;
use crate::ui::pointer;
use crate::ui::rsx;
use crate::{ease, theme};

pub use join::bind_keys;

/// Where on a screen a panel is, for a body that opens something in its
/// place: the password a network asks for takes over from the panel the
/// network was chosen in.
#[derive(Clone, Copy)]
pub struct Place {
    /// The panel's top left corner, from the top left of the screen.
    at: gpui::Point<Pixels>,
    display: Option<gpui::DisplayId>,
}

/// A body has finished with its panel: it has handed over to something that
/// takes its place, or what it was opened for has been chosen. Said out loud
/// because nothing else will make the panel go. A popup is drawn over every
/// other surface there is, the one replacing it included, so the pointer never
/// leaves it and it never closes by itself.
pub struct Finished;

/// Which popout, named for the item that opens it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Active,
    Media,
    Cpu,
    Memory,
    Gpu,
    Guards,
    Features,
    Clock,
    Network,
    Bluetooth,
    Volume,
    Microphone,
    Battery,
    /// A tray item's own menu, by the item's key.
    Tray(SharedString),
}

impl Kind {
    /// How long the pointer has to rest on the item before it opens. Nothing
    /// for a panel, which only says something. A menu waits just long enough
    /// that sweeping along the tray does not open four of them on the way
    /// past, and no longer: a menu that takes a beat to appear feels like the
    /// bar is thinking about it.
    fn hold(&self) -> Duration {
        Duration::from_millis(if matches!(self, Kind::Tray(_)) { 70 } else { 0 })
    }

    fn is_menu(&self) -> bool {
        matches!(self, Kind::Tray(_))
    }

    /// What goes in the panel: a view of its own for each that has anything
    /// to remember, and one between them for those that only say something.
    fn body(&self, feeds: &Feeds, place: Place, cx: &mut Context<Panel>) -> AnyView {
        use simple::Says;
        let says = |says: Says, cx: &mut Context<Panel>| cx.new(|cx| simple::Simple::new(says, feeds, cx)).into();
        match self {
            Kind::Cpu => says(Says::Cpu, cx),
            Kind::Memory => says(Says::Memory, cx),
            Kind::Gpu => says(Says::Gpu, cx),
            Kind::Active => says(Says::Active, cx),
            Kind::Guards => says(Says::Guards, cx),
            Kind::Features => says(Says::Features, cx),
            Kind::Clock => says(Says::Clock, cx),
            Kind::Media => cx.new(|cx| media::Media::new(feeds, cx)).into(),
            Kind::Network => {
                let network = cx.new(|cx| network::Network::new(feeds, place, cx));
                cx.subscribe(&network, |panel, _, _: &Finished, cx| panel.step_aside(cx)).detach();
                network.into()
            }
            Kind::Tray(key) => {
                let menu = cx.new(|cx| menu::Menu::new(key.clone(), cx));
                cx.subscribe(&menu, |panel, _, _: &Finished, cx| panel.step_aside(cx)).detach();
                menu.into()
            }
            Kind::Bluetooth => cx.new(|cx| bluetooth::Bluetooth::new(feeds, cx)).into(),
            Kind::Volume => cx.new(|cx| audio::Audio::new(audio::End::Output, feeds, cx)).into(),
            Kind::Microphone => cx.new(|cx| audio::Audio::new(audio::End::Input, feeds, cx)).into(),
            Kind::Battery => cx.new(|cx| power::Power::new(feeds, cx)).into(),
        }
    }
}

/// "Open settings", on the page about what the panel is about. After the
/// press has been dealt with: a window cannot be opened from inside the
/// update of another.
fn open_settings(page: &'static str, cx: &mut App) {
    cx.defer(move |cx| crate::ui::settings::open(Some(page), cx));
}

/// One width for every panel. A panel whose width follows its content moves
/// sideways under the pointer every time anything in it changes.
const WIDTH: Pixels = px(340.);
/// As tall as a panel may get. The surface is this tall whatever is in it,
/// because a popup cannot be told to grow: only the part with a panel on it
/// takes the pointer, and the rest is glass.
const MAX_HEIGHT: Pixels = px(560.);
/// Room around the panel, on the same surface, for the shadow it casts.
const SHADOW_SIDE: Pixels = px(32.);
const SHADOW_BELOW: Pixels = px(52.);
/// How far above its resting place a panel starts, and so how much surface
/// there has to be above it for that part of the journey to be drawn on.
const TRAVEL: Pixels = px(8.);
/// The gap between the bar's lower edge and a panel at rest.
const GAP: Pixels = px(2.);
/// How near an edge of the screen a panel may sit.
const EDGE: Pixels = px(8.);

/// Crossing from an item to the panel it opened takes the pointer off both
/// for a moment, and that must not count as leaving.
const GRACE: Duration = Duration::from_millis(130);
const ENTER: Duration = Duration::from_millis(200);
const LEAVE: Duration = Duration::from_millis(150);

/// The popouts of one bar: at most one open, and the timer that closes it.
///
/// Kept as where the pointer is rather than as what it last did. The bar and
/// the panel are two windows, and word that the pointer has left one can
/// arrive after word that it has reached the other: acting on each as it came
/// closed panels with the pointer sitting on them.
pub struct Popouts {
    feeds: Feeds,
    open: Option<(Kind, WindowHandle<Panel>)>,
    /// The item the pointer is on, if it is on one.
    item: Option<Kind>,
    on_panel: bool,
    /// What is waiting out its hold before it opens.
    opening: Option<Task<()>>,
    /// Dropping a task cancels it, which is how the pointer coming back stops
    /// a close that was already on its way.
    closing: Option<Task<()>>,
}

impl Popouts {
    pub fn new(feeds: &Feeds) -> Popouts {
        Popouts { feeds: feeds.clone(), open: None, item: None, on_panel: false, opening: None, closing: None }
    }

    /// The pointer is on an item, whose box in the bar's window is `item`.
    fn item_entered(&mut self, kind: Kind, item: Bounds<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        self.item = Some(kind.clone());
        self.settle(cx);
        if self.open.as_ref().is_some_and(|(open, _)| *open == kind) {
            return;
        }

        // Where it goes is worked out now, while there is a window to ask.
        let (options, place) = placement(item, window, cx);
        if kind.hold().is_zero() {
            return self.show(kind, options, place, cx);
        }
        self.opening = Some(cx.spawn(async move |popouts, cx| {
            cx.background_executor().timer(kind.hold()).await;
            let _ = popouts.update(cx, |popouts, cx| {
                // Still here: the pointer was not just passing.
                if popouts.item.as_ref() == Some(&kind) {
                    popouts.show(kind, options, place, cx);
                }
            });
        }));
    }

    /// Only the item the pointer is believed to be on can be left: going from
    /// one item to the next, the first may say so after the second has
    /// already been reached.
    fn item_left(&mut self, kind: &Kind, cx: &mut Context<Self>) {
        if self.item.as_ref() == Some(kind) {
            self.item = None;
            self.opening = None;
        }
        self.settle(cx);
    }

    /// A press that means "the menu": opens it without the wait, and closes
    /// it if it is the one that is open.
    fn item_pressed(&mut self, kind: Kind, item: Bounds<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        self.opening = None;
        if self.open.as_ref().is_some_and(|(open, _)| *open == kind) {
            return self.dismiss(cx);
        }
        let (options, place) = placement(item, window, cx);
        self.show(kind, options, place, cx);
    }

    fn panel_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        self.on_panel = hovered;
        self.settle(cx);
    }

    fn show(&mut self, kind: Kind, options: WindowOptions, place: Place, cx: &mut Context<Self>) {
        // Whatever was open goes at once, without its exit: the next one is
        // already arriving, and two panels crossing is a smear. It will not
        // be saying that the pointer has left it, so that is assumed here.
        if let Some((_, panel)) = self.open.take() {
            let _ = panel.update(cx, |_, window, _| window.remove_window());
        }
        self.on_panel = false;

        let (feeds, popouts, opening) = (self.feeds.clone(), cx.weak_entity(), kind.clone());
        let opened = cx.open_window(options, move |_, cx| cx.new(|cx| Panel::new(&opening, &feeds, place, popouts, cx)));
        match opened {
            Ok(panel) => self.open = Some((kind, panel)),
            Err(error) => eprintln!("cae: cannot open the {kind:?} popout: {error}"),
        }
    }

    /// Takes down whatever is open, now and without its exit.
    ///
    /// After whatever is going on rather than during it: this is asked for
    /// from inside the panel's own window, and a window cannot be reached
    /// into while it is the one being updated.
    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        (self.closing, self.opening, self.on_panel) = (None, None, false);
        if let Some((_, panel)) = self.open.take() {
            cx.defer(move |cx| drop(panel.update(cx, |_, window, _| window.remove_window())));
        }
    }

    /// Closes what is open a beat after the pointer is on neither the item
    /// nor the panel, and calls that off the moment it is on either.
    fn settle(&mut self, cx: &mut Context<Self>) {
        if self.item.is_some() || self.on_panel {
            self.closing = None;
            return;
        }
        if self.open.is_none() || self.closing.is_some() {
            return;
        }
        self.closing = Some(cx.spawn(async move |popouts, cx| {
            cx.background_executor().timer(GRACE).await;
            let _ = popouts.update(cx, |popouts, cx| {
                popouts.closing = None;
                if let Some((_, panel)) = popouts.open.take() {
                    let _ = panel.update(cx, |panel, window, cx| panel.leave(window, cx));
                }
            });
        }));
    }
}

/// Where a panel goes: centred under the item, and kept on the screen.
///
/// Worked out here rather than left to the compositor, which would keep the
/// whole surface on the screen, shadow and all, and so could never let a
/// panel nearer an edge than its shadow is wide.
fn placement(item: Bounds<Pixels>, bar: &Window, cx: &App) -> (WindowOptions, Place) {
    let strip = bar.viewport_size();
    let half = WIDTH / 2.;
    let centre = item.center().x.clamp(EDGE + half, strip.width - EDGE - half);

    let place = Place {
        at: point(centre - half, strip.height + GAP),
        display: bar.display(cx).map(|display| display.id()),
    };

    let popup = PopupOptions {
        parent: bar.window_handle(),
        // A sliver the height of the bar, under which the popup hangs.
        anchor_rect: Bounds::new(point(centre, px(0.)), Size::new(px(1.), strip.height)),
        anchor: PopupAnchor::Bottom,
        gravity: PopupGravity::Bottom,
        constraint_adjustment: PopupConstraintAdjustment::empty(),
        offset: point(px(0.), GAP - TRAVEL),
        grab: false,
    };
    let options = WindowOptions {
        titlebar: None,
        focus: false,
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(px(0.), px(0.)),
            Size::new(WIDTH + SHADOW_SIDE * 2., TRAVEL + MAX_HEIGHT + SHADOW_BELOW),
        ))),
        window_background: WindowBackgroundAppearance::Transparent,
        kind: WindowKind::AnchoredPopup(popup),
        ..crate::ui::surface::options()
    };
    (options, place)
}

/// What an item is given so that it can open a popout: handed down the strip
/// to whatever draws one.
#[derive(Clone)]
pub struct Hover {
    popouts: Entity<Popouts>,
}

impl Hover {
    pub fn new(popouts: Entity<Popouts>) -> Hover {
        Hover { popouts }
    }

    /// Makes `item` open a popout while the pointer is on it.
    ///
    /// The popout hangs under the item's own box, which only layout knows, so
    /// an invisible child the size of the item notes it down as it is placed.
    pub fn opens(&self, kind: Kind, item: Stateful<Div>) -> Stateful<Div> {
        self.wire(kind, item, false)
    }

    /// The same for an item whose popout is its menu, which a right click
    /// opens at once and closes again.
    pub fn menu(&self, kind: Kind, item: Stateful<Div>) -> Stateful<Div> {
        self.wire(kind, item, true)
    }

    /// Closes whatever is open: for an item that has just been used, and
    /// whose menu would otherwise be left hanging over what it did.
    pub fn dismiss(&self, cx: &mut App) {
        self.popouts.update(cx, |popouts, cx| popouts.dismiss(cx));
    }

    fn wire(&self, kind: Kind, item: Stateful<Div>, pressable: bool) -> Stateful<Div> {
        let placed = Rc::new(Cell::new(Bounds::default()));
        let noted = placed.clone();
        let marker = rsx! {
            <canvas class="absolute size-full" prepaint={move |bounds, _, _| noted.set(bounds)} paint={|_, _, _, _| ()} />
        };

        let (popouts, hovering, at) = (self.popouts.clone(), kind.clone(), placed.clone());
        let item = item.child(marker).on_hover(move |hovered, window, cx| {
            let (hovered, kind, item) = (*hovered, hovering.clone(), at.get());
            popouts.update(cx, |popouts, cx| {
                if hovered { popouts.item_entered(kind, item, window, cx) } else { popouts.item_left(&kind, cx) }
            });
        });
        if !pressable {
            return item;
        }
        let popouts = self.popouts.clone();
        item.on_mouse_down(MouseButton::Right, move |_, window, cx| {
            let (kind, item) = (kind.clone(), placed.get());
            popouts.update(cx, |popouts, cx| popouts.item_pressed(kind, item, window, cx));
        })
    }
}

/// One open popout: the window's root, which is the glass and how it comes
/// and goes. What is on the glass is `body`.
pub struct Panel {
    body: AnyView,
    popouts: WeakEntity<Popouts>,
    leaving: bool,
    /// A menu is as wide as what is in it, and padded like a list.
    menu: bool,
    /// The panel's box when the compositor was last told where it may be
    /// reached.
    reach: Rc<Cell<Bounds<Pixels>>>,
}

impl Panel {
    fn new(kind: &Kind, feeds: &Feeds, place: Place, popouts: WeakEntity<Popouts>, cx: &mut Context<Self>) -> Panel {
        Panel { body: kind.body(feeds, place, cx), popouts, leaving: false, menu: kind.is_menu(), reach: Rc::default() }
    }

    /// Goes at once, because something is taking its place.
    fn step_aside(&mut self, cx: &mut Context<Self>) {
        let _ = self.popouts.update(cx, |popouts, cx| popouts.dismiss(cx));
    }

    /// Plays the exit, and only then goes.
    fn leave(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.leaving = true;
        cx.notify();
        cx.spawn_in(window, async move |_, cx| {
            cx.background_executor().timer(LEAVE).await;
            let _ = cx.update(|window, _| window.remove_window());
        })
        .detach();
    }
}

/// Tells the compositor which part of the surface is the panel. Everything
/// else on it is glass: the shadow's room, and however much of the full
/// size this panel does not use.
///
/// Stretched up to the top of the surface, which overlaps the foot of the
/// bar, so that there is no strip between the two that belongs to neither.
/// Where the panel comes to rest, not where it is on its way there: it is
/// measured while it travels, and the region would follow it down otherwise.
fn reach(panel: Bounds<Pixels>, told: &Cell<Bounds<Pixels>>, window: &Window) {
    let resting = Bounds::new(point(panel.origin.x, px(0.)), Size::new(panel.size.width, TRAVEL + panel.size.height));
    if told.replace(resting) != resting {
        window.set_input_region(Some(&[resting]));
    }
}

impl Render for Panel {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let (told, popouts, leaving) = (self.reach.clone(), self.popouts.clone(), self.leaving);

        let menu = self.menu;
        let panel = rsx! {
            <div
                id="panel"
                class="relative flex flex-col overflow-hidden"
                // A panel is one width, whatever is in it. A menu is as wide
                // as its longest entry, within reason both ways.
                when={(!menu, |panel| panel.w(WIDTH).py(px(15.)).px(px(17.)))}
                when={(menu, |panel| panel.min_w(px(200.)).max_w(WIDTH).p(px(6.)))}
                max_h={MAX_HEIGHT}
                rounded={theme::RADIUS}
                bg={theme::panel()}
                shadow={theme::panel_shadows()}
                text_color={theme::text()}
                onHover={move |hovered: &bool, _: &mut Window, cx: &mut App| {
                    let hovered = *hovered;
                    let _ = popouts.update(cx, |popouts, cx| popouts.panel_hovered(hovered, cx));
                }}
            >
                <canvas
                    class="absolute size-full"
                    prepaint={move |bounds, window, _| reach(bounds, &told, window)}
                    paint={|_, _, _, _| ()}
                />
                {self.body.clone()}
            </div>
        };

        // Opens downward out of the bar and closes back into it.
        let (name, length) = if leaving { ("popout-leave", LEAVE) } else { ("popout-enter", ENTER) };
        let moving = panel.with_animation(name, Animation::new(length), move |panel, progress| {
            let (shown, travelled) = if leaving {
                (1. - ease::cubic_bezier(0.4, 0., 1., 1.)(progress), 1. - ease::cubic_bezier(0.4, 0., 1., 1.)(progress))
            } else {
                // The fade is the shorter of the two: seen before it has
                // finished arriving.
                (ease::cubic_bezier(0., 0., 0.58, 1.)((progress / 0.7).min(1.)), ease::settle()(progress))
            };
            panel.opacity(shown).mt(TRAVEL * travelled)
        });

        rsx! {
            <div class="relative size-full flex flex-col items-center" font_family={theme::FONT}>
                {moving}
                {pointer::see_out()}
            </div>
        }
    }
}
