//! The right-hand column of one screen: the toasts, or the centre they
//! collect in. A window that exists while either has something to show, and
//! accepts the pointer only where that something is; the rest of it is not
//! there as far as any window underneath is concerned.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

use cae_core::logo;
use cae_core::notifs::{Notification, reason};
use gpui::{
    AnyElement, App, Bounds, ClipboardItem, Context, DispatchPhase, DisplayId, Entity, IntoElement, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, Render, RetainAllImageCache, Styled, Task, WeakEntity, Window, canvas, div,
    prelude::*, px,
};

use super::{Surfaces, markup};
use crate::ease::{Curve, Tween};
use crate::feeds::Feeds;
use crate::theme;
use crate::ui::{pointer, rsx};

/// More than this and the newest few are shown with a count of the rest: a
/// column of toasts the height of the screen is not being read by anybody.
const MOST_TOASTS: usize = 5;
/// A toast's width, and the centre's. The surface is wider than either, by
/// the room their shadows need to fall off in.
pub(super) const TOAST: Pixels = px(400.);
pub(super) const CENTRE: Pixels = px(420.);
pub const SURFACE: Pixels = px(460.);
pub(super) const GAP: f32 = 10.;
/// How far aside something goes to be off the screen: its own width, and
/// its shadow's.
pub(super) const ASIDE: f32 = 428.;

const GROW: Duration = Duration::from_millis(380);
const FOLD: Duration = Duration::from_millis(320);
const ARRIVE: Duration = Duration::from_millis(420);
const LEAVE: Duration = Duration::from_millis(260);
/// How long the centre outlasts a pointer that has been in it and gone. Long
/// enough to come back from an overshoot, short enough to feel like leaving.
const LINGER: Duration = Duration::from_millis(600);

/// Something in the column that can be dragged: sideways to throw it away,
/// up or down to fold it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum What {
    Toast(u32),
    Row(u32),
    Group(String),
}

/// A drag in progress.
struct Drag {
    what: What,
    /// Where the pointer was at its first move rather than at the press, so
    /// that the first frame of a drag asks for exactly the place the thing
    /// already has.
    grab: Option<Point<Pixels>>,
    width: f32,
    pull: f32,
    folded: bool,
    /// A press that never travelled is a click.
    travelled: bool,
}

pub(super) struct Toast {
    /// As last heard of. Kept after the notification has gone, because its
    /// toast is still on its way out and has to be drawn as it was.
    pub(super) notif: Notification,
    pub(super) expanded: bool,
    pub(super) leaving: bool,
    /// Off to the left rather than the right: the way it was thrown.
    pub(super) leftwards: bool,
    /// How tall its content would like to be, as last laid out.
    pub(super) natural: Rc<Cell<f32>>,
    pub(super) height: Tween,
    /// Nothing at rest, one when it is off the screen.
    pub(super) aside: Tween,
    pub(super) shown: Tween,
}

impl Toast {
    fn arriving(notif: &Notification, expanded: bool) -> Toast {
        let (mut aside, mut shown) = (Tween::still(1.), Tween::still(0.));
        aside.go(0., ARRIVE, Curve::Arrive);
        shown.go(1., Duration::from_millis(220), Curve::Out);
        Toast {
            notif: notif.clone(),
            expanded,
            leaving: false,
            leftwards: false,
            natural: Rc::default(),
            // Nothing until it has been laid out once and says how tall it
            // is: a frame in which it is there and takes no room.
            height: Tween::still(0.),
            aside,
            shown,
        }
    }

    fn leave(&mut self) {
        if !std::mem::replace(&mut self.leaving, true) {
            self.aside.go(1., LEAVE, Curve::Leave);
            self.shown.go(0., Duration::from_millis(200), Curve::In);
            self.height.go(0., GROW, Curve::Settle);
        }
    }

    fn still_moving(&self) -> bool {
        !(self.height.done() && self.aside.done() && self.shown.done())
    }
}

#[derive(Default)]
pub(super) struct Centre {
    /// Asked for, which is not the same as drawn: it is drawn until it has
    /// finished leaving.
    pub(super) open: bool,
    pub(super) drawn: bool,
    pub(super) aside: Option<Tween>,
    pub(super) shown: Option<Tween>,
    /// Which groups are unfolded. Kept here rather than with each group, so
    /// that one redrawn because its notifications changed stays as it was.
    pub(super) unfolded: HashSet<String>,
    /// The pointer has been in it. Opened from a key, the pointer may be the
    /// other side of the screen, and a centre that shut because the pointer
    /// was not already on it would never be seen.
    pub(super) entered: bool,
    pub(super) shutting: Option<Task<()>>,
}

pub struct Column {
    pub(super) output: String,
    pub(super) display: DisplayId,
    pub(super) feeds: Feeds,
    pub(super) surfaces: WeakEntity<Surfaces>,
    pub(super) config: logo::NotifsConfig,
    pub(super) toasts: Vec<Toast>,
    pub(super) waiting: usize,
    pub(super) centre: Centre,
    drag: Option<Drag>,
    /// Things on their way back to where they were, or being carried off.
    pub(super) pulls: HashMap<What, Tween>,
    /// Which bodies were copied, and when: the button says so for a moment.
    pub(super) copied: HashMap<u32, Instant>,
    pub(super) now: i64,
    /// Where things are, for the compositor to be told where the pointer may
    /// land, and what it was last told.
    pub(super) boxes: Rc<RefCell<Vec<Bounds<Pixels>>>>,
    pub(super) told: Rc<RefCell<Vec<Bounds<Pixels>>>>,
    /// The pictures this column has drawn, which go when it does. Left to
    /// the application's own cache they would be kept, decoded, for as long
    /// as the shell runs: every avatar of every message of the day.
    pictures: Entity<RetainAllImageCache>,
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_millis() as i64)
}

impl Column {
    pub fn new(
        output: String,
        display: DisplayId,
        feeds: &Feeds,
        surfaces: WeakEntity<Surfaces>,
        cx: &mut Context<Self>,
    ) -> Column {
        cx.observe(&feeds.notifs, |column, _, cx| column.hear(cx)).detach();
        cx.observe(&feeds.settings, |column, _, cx| {
            column.config = logo::notifs_config();
            cx.notify();
        })
        .detach();

        // One clock for every "4m" in the column. Nothing here is finer than
        // a minute, so one reading shared by all of them is all it takes.
        cx.spawn(async move |column, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(20)).await;
                let ticked = column.update(cx, |column, cx| {
                    column.now = now();
                    cx.notify();
                });
                if ticked.is_err() {
                    break;
                }
            }
        })
        .detach();

        let mut column = Column {
            output,
            display,
            pictures: RetainAllImageCache::new(cx),
            feeds: feeds.clone(),
            surfaces,
            config: logo::notifs_config(),
            toasts: Vec::new(),
            waiting: 0,
            centre: Centre::default(),
            drag: None,
            pulls: HashMap::new(),
            copied: HashMap::new(),
            now: now(),
            boxes: Rc::default(),
            told: Rc::default(),
        };
        column.hear(cx);
        column
    }

    /// Brings what is drawn into line with what the server says there is.
    fn hear(&mut self, cx: &mut Context<Self>) {
        let feed = self.feeds.notifs.read(cx).value.clone();
        let here = !feed.centre.is_empty() && feed.centre == self.output;

        // Nothing pops up beside an open centre. The pane is glass, and
        // toasts leaving behind it would show through it.
        let popups: Vec<&Notification> = feed.list.iter().filter(|notif| notif.popup && !here).collect();
        self.waiting = popups.len().saturating_sub(MOST_TOASTS);
        let shown = &popups[..popups.len().min(MOST_TOASTS)];

        for toast in &mut self.toasts {
            match shown.iter().find(|notif| notif.id == toast.notif.id) {
                Some(notif) if !toast.leaving => toast.notif = (*notif).clone(),
                _ => toast.leave(),
            }
        }
        // Each newcomer goes where the server's order puts it among the ones
        // that are staying, which for the newest is the top.
        let mut place = 0;
        for notif in shown {
            match self.toasts.iter().position(|toast| toast.notif.id == notif.id && !toast.leaving) {
                Some(at) => place = at + 1,
                None => {
                    self.toasts.insert(place, Toast::arriving(notif, self.config.open_expanded));
                    place += 1;
                }
            }
        }

        if here != self.centre.open {
            self.centre.open = here;
            let (mut aside, mut fade) = (
                self.centre.aside.unwrap_or(Tween::still(1.)),
                self.centre.shown.unwrap_or(Tween::still(0.)),
            );
            if here {
                self.centre.drawn = true;
                self.centre.entered = false;
                aside.go(0., Duration::from_millis(400), Curve::Arrive);
                fade.go(1., Duration::from_millis(200), Curve::Out);
            } else {
                self.centre.shutting = None;
                aside.go(1., Duration::from_millis(240), Curve::Leave);
                fade.go(0., Duration::from_millis(180), Curve::In);
            }
            (self.centre.aside, self.centre.shown) = (Some(aside), Some(fade));
        }
        cx.notify();
    }

    /// Drops what has finished leaving, and says whether anything is left.
    fn sweep(&mut self) -> bool {
        self.toasts.retain(|toast| !(toast.leaving && !toast.still_moving()));
        if !self.centre.open && self.centre.aside.is_some_and(|aside| aside.done()) {
            self.centre.drawn = false;
        }
        self.pulls.retain(|_, pull| !pull.done() || pull.target() != 0.);
        self.copied.retain(|_, when| when.elapsed() < Duration::from_secs(3));
        !self.toasts.is_empty() || self.centre.drawn
    }

    fn moving(&self) -> bool {
        self.toasts.iter().any(Toast::still_moving)
            || self.pulls.values().any(|pull| !pull.done())
            || self.centre.aside.is_some_and(|aside| !aside.done())
            || self.centre.shown.is_some_and(|shown| !shown.done())
    }

    /// How far sideways something is, from being dragged or from settling
    /// back after it.
    pub(super) fn pulled(&self, what: &What) -> f32 {
        match &self.drag {
            Some(drag) if drag.what == *what => drag.pull,
            _ => self.pulls.get(what).map_or(0., Tween::value),
        }
    }

    // Dragging. Followed on the window rather than on the thing, because for
    // most of a swipe the pointer is nowhere near it, and a release has to
    // end the drag wherever it lands.

    pub(super) fn press(&mut self, what: What, width: f32) {
        self.pulls.remove(&what);
        self.drag = Some(Drag { what, grab: None, width, pull: 0., folded: false, travelled: false });
    }

    fn dragged(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let fold_after = self.config.expand_threshold as f32;
        let Some(drag) = &mut self.drag else { return };
        let Some(grab) = drag.grab else {
            drag.grab = Some(at);
            return;
        };
        let (across, down) = (f32::from(at.x - grab.x), f32::from(at.y - grab.y));
        drag.travelled |= across.abs() > 4. || down.abs() > 4.;

        // Mostly up or down, and far enough: fold or unfold, once a gesture.
        let mut fold = None;
        if !drag.folded && down.abs() > fold_after && down.abs() > across.abs() * 1.5 {
            drag.folded = true;
            fold = Some((drag.what.clone(), down > 0.));
        }
        drag.pull = if drag.folded { 0. } else { across };
        if let Some((what, open)) = fold {
            self.fold(&what, open);
        }
        cx.notify();
    }

    fn released(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else { return };
        let thrown = drag.pull.abs() > drag.width * self.config.clear_threshold as f32;

        let mut pull = Tween::still(drag.pull);
        if thrown {
            // Carried off the way it was going, and then reported.
            pull.go(drag.pull.signum() * drag.width * 1.4, LEAVE, Curve::Leave);
            self.throw(&drag.what, drag.pull < 0.);
        } else {
            pull.go(0., FOLD, Curve::Settle);
            if !drag.travelled {
                self.clicked(&drag.what);
            }
        }
        self.pulls.insert(drag.what, pull);
        cx.notify();
    }

    fn fold(&mut self, what: &What, open: bool) {
        match what {
            What::Toast(id) => {
                if let Some(toast) = self.toasts.iter_mut().find(|toast| toast.notif.id == *id) {
                    toast.expanded = open;
                }
            }
            What::Group(app) => {
                if open {
                    self.centre.unfolded.insert(app.clone());
                } else {
                    self.centre.unfolded.remove(app);
                }
            }
            What::Row(_) => {}
        }
    }

    fn throw(&mut self, what: &What, leftwards: bool) {
        let server = &self.feeds.server;
        match what {
            // Off the screen, and into the list: it has been seen, not dealt
            // with.
            What::Toast(id) => {
                if let Some(toast) = self.toasts.iter_mut().find(|toast| toast.notif.id == *id) {
                    toast.leftwards = leftwards;
                }
                server.dismiss_popup(*id);
            }
            What::Row(id) => server.close(*id, reason::DISMISSED),
            What::Group(app) => server.close_app(app),
        }
    }

    /// A press that went nowhere. On a toast with exactly one thing it could
    /// mean, and a config that says so, it means that.
    fn clicked(&mut self, what: &What) {
        let What::Toast(id) = what else { return };
        let Some(toast) = self.toasts.iter().find(|toast| toast.notif.id == *id) else { return };
        if let (true, [only]) = (self.config.action_on_click, toast.notif.actions.as_slice()) {
            self.feeds.server.invoke(*id, &only.identifier);
        }
    }

    pub(super) fn copy(&mut self, notif: &Notification, cx: &mut Context<Self>) {
        let text = if notif.body.is_empty() { notif.summary.clone() } else { markup::plain(&notif.body) };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.copied.insert(notif.id, Instant::now());
        cx.notify();

        // The button says it worked, and then stops saying so.
        cx.spawn(async move |column, cx| {
            cx.background_executor().timer(Duration::from_millis(3050)).await;
            let _ = column.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    /// The pointer is on the centre, or has left it.
    pub(super) fn centre_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if hovered {
            (self.centre.entered, self.centre.shutting) = (true, None);
        } else if self.centre.entered && self.centre.open {
            let server = self.feeds.server.clone();
            self.centre.shutting = Some(cx.spawn(async move |_, cx| {
                cx.background_executor().timer(LINGER).await;
                server.set_centre("");
            }));
        }
    }

    // What it is drawn with. The toasts and the centre are in files of
    // their own.

    /// A box the pointer may land in: noted as it is laid out, and told to
    /// the compositor once the frame's worth is in.
    pub(super) fn reachable(&self) -> impl IntoElement + use<> {
        let boxes = self.boxes.clone();
        rsx! {
            <canvas class="absolute size-full" prepaint={move |bounds, _, _| boxes.borrow_mut().push(bounds)} paint={|_, _, _, _| ()} />
        }
    }

}

impl Render for Column {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.sweep() {
            // Nothing left to draw and nothing on its way out. The window
            // takes itself down, because it is the one being updated and
            // cannot be reached into from outside while it is.
            window.remove_window();
            let display = self.display;
            let _ = self.surfaces.update(cx, |surfaces, _| surfaces.gone(display));
            return div().into_any_element();
        }

        // What each toast is given follows what it last asked for.
        for toast in self.toasts.iter_mut().filter(|toast| !toast.leaving) {
            let natural = toast.natural.get();
            if natural > 0. && (natural - toast.height.target()).abs() > 0.5 {
                let first = toast.height.target() == 0.;
                toast.height.go(natural, if first { GROW } else { FOLD }, Curve::Settle);
            }
        }
        if self.moving() {
            window.request_animation_frame();
        }

        self.boxes.borrow_mut().clear();
        let (boxes, told) = (self.boxes.clone(), self.told.clone());
        let column = cx.weak_entity();
        let (moved, let_go) = (column.clone(), column);

        let toasts: Vec<AnyElement> = self.toasts.iter().map(|toast| self.toast(toast, cx)).collect();
        let waiting = self.waiting;

        rsx! {
            <div class="relative size-full" font_family={theme::FONT} image_cache={self.pictures.clone()}>
                <div class="absolute flex flex-col" top={px(6.)} right={px(12.)} w={TOAST}>
                    {...toasts}
                    {...(waiting > 0).then(|| rsx! {
                        <div class="flex flex-none justify-end">
                            <div
                                class="flex-none py-[4px] px-[11px] rounded-full"
                                bg={gpui::rgba(0x0d0d0fe0)}
                                shadow={theme::edge(0.06)}
                                text_size={px(11.5)}
                                text_color={theme::text_dim()}
                            >
                                {format!("{waiting} more waiting")}
                            </div>
                        </div>
                    })}
                </div>
                {...self.centre.drawn.then(|| self.centre(cx))}
                {pointer::see_out()}
                // Last, so that everything above has said where it is.
                <canvas
                    class="absolute size-full"
                    prepaint={move |_, window: &mut Window, _: &mut App| {
                        let boxes = boxes.borrow();
                        if *boxes != *told.borrow() {
                            window.set_input_region(Some(&boxes));
                            *told.borrow_mut() = boxes.clone();
                        }
                    }}
                    paint={move |_, _, window: &mut Window, _: &mut App| {
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                            if phase == DispatchPhase::Bubble && event.dragging() {
                                let _ = moved.update(cx, |column, cx| column.dragged(event.position, cx));
                            }
                        });
                        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                            if phase == DispatchPhase::Bubble {
                                let _ = let_go.update(cx, |column, cx| column.released(cx));
                            }
                        });
                    }}
                />
            </div>
        }
        .into_any_element()
    }
}
