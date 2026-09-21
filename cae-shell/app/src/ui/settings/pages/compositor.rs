//! What the compositor's settings have that is more than a list of knobs:
//! keybinds of the person's own, the arrangement of the screens, and every
//! option Hyprland has, for whatever nobody thought to make a knob of.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use cae_core::hyprmod::{self, HyprOption, Overrides};
use cae_core::monitors::{self, Monitor, Placed};
use gpui::{
    AnyElement, AppContext, Bounds, Context, Entity, Focusable, FontWeight, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    Pixels, Render, SharedString, Window, canvas, div, prelude::*, px,
};
use serde_json::{Value, json};

use super::super::frame::{Commit, Reach};
use super::super::rows::{
    button, chosen_mark, figure, figure_text, nothing, page, pick, press, press_twice, pressable, row, rule, searched, section_title, step, typed,
};
use crate::theme;
use crate::ui::controls::{chip, switch, warning};
use crate::ui::field::{Field, FieldEvent};
use crate::ui::rsx;

/// The helper writes the config and reloads what it must; what it wrote is
/// there to be read a moment after it returns.
const SETTLED: Duration = Duration::from_millis(500);
const ARMED: Duration = Duration::from_secs(4);

// ---- keybinds of the person's own ---------------------------------------

/// What a keybind does, as the helper names the three kinds.
const KINDS: [(&str, &str, &str); 3] = [
    ("exec", "Runs a command", "firefox"),
    ("global", "A shell action", "caelestia:launcher"),
    ("lua", "Lua", "hl.dsp.togglefloating()"),
];

pub struct CustomKeys {
    reach: Reach,
    binds: Vec<hyprmod::Bind>,
    keys: Entity<Field>,
    does: Entity<Field>,
    kind: usize,
    removing: Option<usize>,
}

impl CustomKeys {
    pub fn new(reach: &Reach, cx: &mut Context<Self>) -> CustomKeys {
        let field = |placeholder: &'static str, cx: &mut Context<Self>| {
            let field = cx.new(|cx| Field::new(placeholder, cx));
            cx.observe(&field, |_, _, cx| cx.notify()).detach();
            field
        };
        let mut page =
            CustomKeys { reach: reach.clone(), binds: Vec::new(), keys: field("SUPER + B", cx), does: field(KINDS[0].2, cx), kind: 0, removing: None };
        page.look(Duration::ZERO, cx);
        page
    }

    fn look(&mut self, after: Duration, cx: &mut Context<Self>) {
        cx.spawn(async move |page, cx| {
            cx.background_executor().timer(after).await;
            let binds = cx.background_spawn(async { hyprmod::overrides().binds }).await;
            let _ = page.update(cx, |page: &mut CustomKeys, cx| {
                page.binds = binds;
                cx.notify();
            });
        })
        .detach();
    }

    fn add(&mut self, cx: &mut Context<Self>) {
        let (keys, does) = (self.keys.read(cx).text().trim().to_string(), self.does.read(cx).text().trim().to_string());
        if keys.is_empty() || does.is_empty() {
            return;
        }
        let kind = KINDS[self.kind].0;
        self.reach.store.update(cx, |store, _| store.run("bind added", move || hyprmod::add_bind(&keys, kind, &does)));
        self.keys.update(cx, |field, cx| field.set_text("", cx));
        self.does.update(cx, |field, cx| field.set_text("", cx));
        self.look(SETTLED, cx);
    }

    fn remove(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.removing != Some(index) {
            self.removing = Some(index);
            cx.notify();
            cx.spawn(async move |page, cx| {
                cx.background_executor().timer(ARMED).await;
                let _ = page.update(cx, |page: &mut CustomKeys, cx| {
                    page.removing = None;
                    cx.notify();
                });
            })
            .detach();
            return;
        }
        self.removing = None;
        self.binds.remove(index);
        self.reach.store.update(cx, |store, _| store.run("bind removed", move || hyprmod::remove_bind(index)));
        self.look(SETTLED, cx);
        cx.notify();
    }
}

impl Render for CustomKeys {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ready = !self.keys.read(cx).text().trim().is_empty() && !self.does.read(cx).text().trim().is_empty();
        let binds: Vec<AnyElement> = self
            .binds
            .iter()
            .enumerate()
            .map(|(index, bind)| {
                let does = KINDS.iter().find(|(kind, ..)| *kind == bind.kind).map_or("Lua", |(_, does, _)| does);
                rsx! {
                    <div class="flex flex-col flex-none">
                        {...(index > 0).then(rule)}
                        <div base={row(bind.combo.clone(), format!("{does}: {}", bind.value), true)}>
                            <div
                                base={press_twice("delete", self.removing == Some(index), "Remove")}
                                id={("remove", index)}
                                onClick={cx.listener(move |page, _, _, cx| page.remove(index, cx))}
                            />
                        </div>
                    </div>
                }
                .into_any_element()
            })
            .collect();

        rsx! {
            <div base={page()} on_action={cx.listener(|page, _: &Commit, _, cx| page.add(cx))}>
                {...binds}
                {...self.binds.is_empty().then(|| nothing("keyboard", "No keybinds of your own yet"))}

                {section_title("A new one", false)}
                <div base={row("Keys", "As Hyprland writes them", true)}>{typed(&self.keys, self.keys.read(cx).is_focused())}</div>
                {rule()}
                <div base={row("It", "", true)}>
                    <div class="flex flex-none items-center gap-[4px]">
                        {for (index, (_, label, _)) in KINDS.iter().enumerate() {
                            <div
                                base={chip(*label, index == self.kind)}
                                id={("kind", index)}
                                onClick={cx.listener(move |page, _, _, cx| {
                                    page.kind = index;
                                    cx.notify();
                                })}
                            />
                        }}
                    </div>
                </div>
                {rule()}
                <div base={row("Which", KINDS[self.kind].2, true)}>{typed(&self.does, self.does.read(cx).is_focused())}</div>
                <div class="flex flex-none justify-end pt-[12px]">
                    <div
                        base={button("add", "Add the keybind", true)}
                        id="add"
                        when={(!ready, |button| button.opacity(0.45))}
                        onClick={cx.listener(|page, _, _, cx| page.add(cx))}
                    />
                </div>
            </div>
        }
    }
}

// ---- the screens ---------------------------------------------------------

const CANVAS: Pixels = px(250.);
const CANVAS_PAD: f32 = 22.;

/// A monitor being dragged: which, where the pointer took hold of it, and
/// where it is now, in the compositor's units.
#[derive(Clone)]
struct Drag {
    name: String,
    /// From the monitor's corner to the pointer, in those units.
    held_at: (f32, f32),
    start: (i64, i64),
    now: (i64, i64),
}

pub struct Monitors {
    reach: Reach,
    monitors: Vec<Monitor>,
    saved: Overrides,
    selected: String,
    drag: Option<Drag>,
    /// How the picture maps to the compositor's units, kept from the last
    /// frame drawn: a pointer is turned back into a position with it.
    view: Rc<Cell<(Bounds<Pixels>, f32, (f32, f32))>>,
    modes_open: bool,
    looked: bool,
}

impl Monitors {
    pub fn new(reach: &Reach, cx: &mut Context<Self>) -> Monitors {
        let mut page = Monitors {
            reach: reach.clone(),
            monitors: Vec::new(),
            saved: Overrides::default(),
            selected: String::new(),
            drag: None,
            view: Rc::default(),
            modes_open: false,
            looked: false,
        };
        page.look(Duration::ZERO, cx);
        page
    }

    fn look(&mut self, after: Duration, cx: &mut Context<Self>) {
        cx.spawn(async move |page, cx| {
            cx.background_executor().timer(after).await;
            let found = cx.background_spawn(async { (monitors::list(), hyprmod::overrides()) }).await;
            let _ = page.update(cx, |page: &mut Monitors, cx| {
                (page.monitors, page.saved) = found;
                page.looked = true;
                if !page.monitors.iter().any(|monitor| monitor.name == page.selected) {
                    page.selected = page.monitors.first().map(|monitor| monitor.name.clone()).unwrap_or_default();
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn selected(&self) -> Option<&Monitor> {
        self.monitors.iter().find(|monitor| monitor.name == self.selected)
    }

    /// Saves `changes` for a monitor, over what was saved for it before and
    /// what it is doing now for whatever was never saved.
    fn save(&mut self, name: &str, changes: Value, cx: &mut Context<Self>) {
        let Some(live) = self.monitors.iter().find(|monitor| monitor.name == name) else { return };
        let saved = self.saved.monitors.get(name);
        let kept = |key: &str, otherwise: Value| saved.and_then(|saved| saved.get(key)).cloned().unwrap_or(otherwise);
        let mut spec = json!({
            "mode": kept("mode", live.mode().into()),
            "position": kept("position", format!("{}x{}", live.x, live.y).into()),
            "scale": kept("scale", live.scale.into()),
        });
        for (key, value) in changes.as_object().into_iter().flatten() {
            spec[key] = value.clone();
        }
        self.saved.monitors.insert(name.to_string(), spec.clone());
        let monitor = name.to_string();
        self.reach.store.update(cx, |store, _| store.run(format!("monitor {monitor}"), move || hyprmod::set_monitor(&monitor, &spec)));
        self.look(SETTLED, cx);
        cx.notify();
    }

    fn pressed(&mut self, name: String, event: &MouseDownEvent, cx: &mut Context<Self>) {
        self.selected = name.clone();
        self.modes_open = false;
        let Some(monitor) = self.monitors.iter().find(|monitor| monitor.name == name && !monitor.disabled) else { return cx.notify() };
        let (picture, scale, origin) = self.view.get();
        let pointer = (f32::from(event.position.x - picture.left()), f32::from(event.position.y - picture.top()));
        let at = ((pointer.0 - origin.0) / scale, (pointer.1 - origin.1) / scale);
        self.drag = Some(Drag {
            name,
            held_at: (at.0 - monitor.x as f32, at.1 - monitor.y as f32),
            start: (monitor.x, monitor.y),
            now: (monitor.x, monitor.y),
        });
        cx.notify();
    }

    fn moved(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.clone().filter(|_| event.dragging()) else { return };
        let Some(dragged) = self.monitors.iter().find(|monitor| monitor.name == drag.name) else { return };
        let (picture, scale, origin) = self.view.get();
        let pointer = (f32::from(event.position.x - picture.left()), f32::from(event.position.y - picture.top()));
        let wanted = (((pointer.0 - origin.0) / scale - drag.held_at.0) as i64, ((pointer.1 - origin.1) / scale - drag.held_at.1) as i64);

        let others: Vec<Placed> =
            self.monitors.iter().filter(|monitor| monitor.name != drag.name && !monitor.disabled).map(Monitor::placed).collect();
        let now = monitors::settle(dragged.placed(), drag.start, wanted, &others);
        if now != drag.now {
            self.drag = Some(Drag { now, ..drag });
            cx.notify();
        }
    }

    fn released(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else { return };
        if drag.now != drag.start {
            // Shown where it was let go at once: the compositor takes a
            // moment to agree, and until it does the picture would jump back.
            if let Some(monitor) = self.monitors.iter_mut().find(|monitor| monitor.name == drag.name) {
                (monitor.x, monitor.y) = drag.now;
            }
            self.save(&drag.name, json!({ "position": format!("{}x{}", drag.now.0, drag.now.1) }), cx);
        }
        cx.notify();
    }

    /// The picture of the screens: each where it is, to scale, and the one
    /// being dragged where the drag has it.
    fn picture(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let placed: Vec<(&Monitor, Placed)> = self
            .monitors
            .iter()
            .map(|monitor| {
                let mut place = monitor.placed();
                if let Some(drag) = self.drag.as_ref().filter(|drag| drag.name == monitor.name) {
                    (place.x, place.y) = drag.now;
                }
                (monitor, place)
            })
            .collect();

        // What the picture has to fit is fixed while something is dragged,
        // or the scale would change under the pointer as the drag went on.
        let fitted: Vec<Placed> = self.monitors.iter().map(Monitor::placed).collect();
        let left = fitted.iter().map(|place| place.x).min().unwrap_or(0) as f32;
        let top = fitted.iter().map(|place| place.y).min().unwrap_or(0) as f32;
        let across = fitted.iter().map(|place| place.x + place.w).max().unwrap_or(1) as f32 - left;
        let down = fitted.iter().map(|place| place.y + place.h).max().unwrap_or(1) as f32 - top;

        let width = f32::from(super::super::rows::COLUMN);
        let scale = ((width - CANVAS_PAD * 2.) / across).min((f32::from(CANVAS) - CANVAS_PAD * 2.) / down).min(0.16);
        let origin = ((width - across * scale) / 2. - left * scale, (f32::from(CANVAS) - down * scale) / 2. - top * scale);
        let view = self.view.clone();

        rsx! {
            <div
                id="screens"
                class="relative flex-none w-full overflow-hidden"
                h={CANVAS}
                rounded={px(10.)}
                bg={theme::white(0.03)}
                shadow={theme::edge(0.05)}
                onMouseMove={cx.listener(|page, event, _, cx| page.moved(event, cx))}
                onMouseUp={(MouseButton::Left, cx.listener(|page, _, _, cx| page.released(cx)))}
                onMouseUpOut={(MouseButton::Left, cx.listener(|page, _, _, cx| page.released(cx)))}
            >
                <canvas
                    class="absolute size-full"
                    prepaint={move |bounds, _, _| view.set((bounds, scale, origin))}
                    paint={|_, _, _, _| ()}
                />
                {for (index, (monitor, place)) in placed.into_iter().enumerate() {
                    <div
                        id={("screen", index)}
                        class="absolute flex flex-col items-center justify-center gap-[2px] overflow-hidden"
                        left={px(origin.0 + place.x as f32 * scale)}
                        top={px(origin.1 + place.y as f32 * scale)}
                        w={px(place.w as f32 * scale)}
                        h={px(place.h as f32 * scale)}
                        rounded={px(6.)}
                        bg={theme::white(if monitor.name == self.selected { 0.13 } else { 0.07 })}
                        shadow={if monitor.name == self.selected { theme::edge_in(theme::accent().opacity(0.8)) } else { theme::edge(0.1) }}
                        when={(monitor.disabled, |screen| screen.opacity(0.4))}
                        when={(!monitor.disabled, |screen| screen.cursor_grab())}
                        onMouseDown={(MouseButton::Left, cx.listener({
                            let name = monitor.name.clone();
                            move |page, event, _, cx| page.pressed(name.clone(), event, cx)
                        }))}
                    >
                        <div class="truncate" text_size={px(12.5)} font_weight={FontWeight::MEDIUM}>{monitor.name.clone()}</div>
                        <div class="truncate" text_size={px(11.)} text_color={theme::text_faint()} font_features={theme::tabular()}>
                            {monitor.mode()}
                        </div>
                    </div>
                }}
            </div>
        }
    }
}

impl Render for Monitors {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.looked && self.monitors.is_empty() {
            return rsx! { <div base={page()}>{warning("Hyprland is not saying what screens there are.")}</div> };
        }
        let Some(monitor) = self.selected().cloned() else { return rsx! { <div base={page()} /> } };
        let name = monitor.name.clone();
        let primary = self.saved.primary == name;
        let saved = self.saved.monitors.contains_key(&name);
        let several = self.monitors.len() > 1;

        let scale = monitor.scale;
        // In quarters, and kept to them: a scale the compositor cannot divide
        // the panel's pixels by evenly is one it refuses.
        let scale_to = move |towards: f64| ((scale + towards * 0.25).clamp(0.5, 3.) * 4.).round() / 4.;
        let (lower, raise) = (monitor.scale > 0.5, monitor.scale < 3.);
        let (to_lower, to_raise, to_forget, to_primary) = (name.clone(), name.clone(), name.clone(), name.clone());

        let modes: Vec<AnyElement> = monitor
            .available_modes
            .iter()
            .enumerate()
            .map(|(index, listed)| {
                let mode = monitors::mode_from(listed);
                let (chosen, to) = (mode == monitor.mode(), name.clone());
                rsx! {
                    <div
                        base={pick("aspect_ratio", listed.trim_end_matches("Hz").replace('@', " at ") + " Hz", "", chosen)}
                        id={("mode", index)}
                        onClick={cx.listener(move |page, _, _, cx| {
                            page.modes_open = false;
                            page.save(&to, json!({ "mode": mode.clone() }), cx);
                        })}
                    >
                        {...chosen.then(chosen_mark)}
                    </div>
                }
                .into_any_element()
            })
            .collect();

        rsx! {
            <div base={page()}>
                {self.picture(cx)}
                {...several.then(|| rsx! {
                    <div class="pt-[8px]" text_size={px(11.5)} text_color={theme::text_faint()}>
                        {"Drag a screen to where it stands. It applies as it is let go"}
                    </div>
                })}

                {section_title(SharedString::from(if monitor.description.is_empty() { name.clone() } else { format!("{name}, {}", monitor.description) }), false)}
                <div
                    base={pressable(row("Resolution and refresh rate", "", true))}
                    id="modes"
                    onClick={cx.listener(|page, _, _, cx| {
                        page.modes_open = !page.modes_open;
                        cx.notify();
                    })}
                >
                    <div class="flex flex-none items-center gap-[6px]" text_size={px(13.)} text_color={theme::text_dim()} font_features={theme::tabular()}>
                        {monitor.mode().replace('@', " at ") + " Hz"}
                        {press(if self.modes_open { "expand_less" } else { "expand_more" }, self.modes_open)}
                    </div>
                </div>
                {...self.modes_open.then(|| rsx! {
                    <div id="mode-list" class="flex flex-col flex-none max-h-[264px] overflow-y-scroll pl-[14px]">{...modes}</div>
                })}
                {rule()}
                <div base={row("Scale", "How large everything is drawn on it", true)}>
                    <div class="flex flex-none items-center gap-[2px]">
                        <div
                            base={step("remove", lower)}
                            id="scale-down"
                            when={(lower, |end| end.on_click(cx.listener(move |page, _, _, cx| page.save(&to_lower, json!({ "scale": scale_to(-1.) }), cx))))}
                        />
                        {figure(figure_text(monitor.scale * 100., 1., "%"))}
                        <div
                            base={step("add", raise)}
                            id="scale-up"
                            when={(raise, |end| end.on_click(cx.listener(move |page, _, _, cx| page.save(&to_raise, json!({ "scale": scale_to(1.) }), cx))))}
                        />
                    </div>
                </div>
                {...several.then(|| rsx! {
                    <div class="flex flex-col flex-none">
                        {rule()}
                        <div
                            base={pressable(row("Primary", "Where workspace 1 lives after a restart", true))}
                            id="primary"
                            onClick={cx.listener(move |page, _, _, cx| {
                                let name = if primary { String::new() } else { to_primary.clone() };
                                page.saved.primary = name.clone();
                                page.reach.store.update(cx, |store, _| store.run("primary monitor", move || hyprmod::set_primary(&name)));
                                cx.notify();
                            })}
                        >
                            {switch(primary)}
                        </div>
                    </div>
                })}
                {...saved.then(|| rsx! {
                    <div class="flex flex-none justify-end pt-[14px]">
                        <div
                            base={button("restart_alt", "Forget what was saved for it", false)}
                            id="forget"
                            onClick={cx.listener(move |page, _, _, cx| {
                                let name = to_forget.clone();
                                page.saved.monitors.remove(&name);
                                page.reach.store.update(cx, |store, _| store.run(format!("monitor {name}"), move || hyprmod::forget_monitor(&name)));
                                page.look(SETTLED, cx);
                            })}
                        />
                    </div>
                })}
            </div>
        }
    }
}

// ---- every option --------------------------------------------------------

/// As many options as are shown at once: there are several hundred, and the
/// search is how any one of them is reached.
const MOST_SHOWN: usize = 40;

pub struct Options {
    reach: Reach,
    search: Entity<Field>,
    options: Vec<HyprOption>,
    overridden: serde_json::Map<String, Value>,
    /// The box for each option that is typed rather than switched, made as
    /// the option first comes into view.
    fields: HashMap<String, Entity<Field>>,
    looked: bool,
}

impl Options {
    pub fn new(reach: &Reach, window: &mut Window, cx: &mut Context<Self>) -> Options {
        let search = cx.new(|cx| Field::new("Search Hyprland's options", cx));
        cx.subscribe_in(&search, window, |page: &mut Options, _, _: &FieldEvent, window, cx| page.make_fields(window, cx)).detach();
        window.focus(&search.focus_handle(cx), cx);

        cx.spawn_in(window, async move |page, cx| {
            let found = cx.background_spawn(async { (hyprmod::schema(), hyprmod::overrides().options) }).await;
            let _ = page.update_in(cx, |page: &mut Options, window, cx| {
                (page.options, page.overridden) = found;
                page.looked = true;
                page.make_fields(window, cx);
            });
        })
        .detach();

        Options { reach: reach.clone(), search, options: Vec::new(), overridden: Default::default(), fields: HashMap::new(), looked: false }
    }

    fn shown(&self, cx: &Context<Self>) -> Vec<&HyprOption> {
        let query = self.search.read(cx).text().trim().to_lowercase();
        let matches = |option: &&HyprOption| query.is_empty() || option.name.contains(&query) || option.description.to_lowercase().contains(&query);
        let mut shown: Vec<&HyprOption> = self.options.iter().filter(matches).collect();
        // What has been changed first: it is what somebody comes back for.
        shown.sort_by_key(|option| !self.overridden.contains_key(&option.name));
        shown.truncate(MOST_SHOWN);
        shown
    }

    fn value(&self, option: &HyprOption) -> Value {
        self.overridden.get(&option.name).cloned().unwrap_or_else(|| if option.current.is_null() { option.default.clone() } else { option.current.clone() })
    }

    fn make_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let wanted: Vec<(String, String)> = self
            .shown(cx)
            .into_iter()
            .filter(|option| !option.default.is_boolean() && !self.fields.contains_key(&option.name))
            .map(|option| (option.name.clone(), as_typed(&self.value(option))))
            .collect();
        for (name, value) in wanted {
            let field = cx.new(|cx| Field::new("", cx));
            field.update(cx, |field, cx| field.set_text(value, cx));
            let option = name.clone();
            cx.on_blur(&field.focus_handle(cx), window, move |page: &mut Options, _, cx| page.commit(&option, cx)).detach();
            self.fields.insert(name, field);
        }
        cx.notify();
    }

    fn commit(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(said) = self.fields.get(name).map(|field| field.read(cx).text().trim().to_string()) else { return };
        let Some(option) = self.options.iter().find(|option| option.name == name) else { return };
        if said.is_empty() || said == as_typed(&self.value(option)) {
            return;
        }
        self.set(name.to_string(), said.into(), cx);
    }

    fn set(&mut self, name: String, value: Value, cx: &mut Context<Self>) {
        self.overridden.insert(name.clone(), value.clone());
        let said = as_typed(&value);
        self.reach.store.update(cx, |store, _| store.run(format!("option {name}"), move || hyprmod::set_option(&name, &said)));
        cx.notify();
    }

    fn reset(&mut self, name: String, cx: &mut Context<Self>) {
        self.overridden.remove(&name);
        if let (Some(field), Some(option)) = (self.fields.get(&name), self.options.iter().find(|option| option.name == name)) {
            let value = as_typed(&self.value(option));
            field.update(cx, |field, cx| field.set_text(value, cx));
        }
        self.reach.store.update(cx, |store, _| store.run(format!("option {name}"), move || hyprmod::unset_option(&name)));
        cx.notify();
    }
}

/// A value the way it is typed: a word without its quotes.
fn as_typed(value: &Value) -> String {
    value.as_str().map_or_else(|| value.to_string(), str::to_string)
}

impl Render for Options {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (shown, all) = (self.shown(cx), self.options.len());
        let more = shown.len() == MOST_SHOWN && all > MOST_SHOWN;
        let rows: Vec<AnyElement> = shown
            .iter()
            .enumerate()
            .map(|(index, option)| {
                let (name, changed) = (option.name.clone(), self.overridden.contains_key(&option.name));
                let range = match (option.min, option.max) {
                    (Some(min), Some(max)) => format!(" From {min} to {max}"),
                    _ => String::new(),
                };
                let control = match self.fields.get(&option.name) {
                    Some(field) => typed(field, field.read(cx).is_focused()).w(px(170.)).into_any_element(),
                    None => {
                        let on = self.value(option).as_bool().unwrap_or_default();
                        let name = name.clone();
                        rsx! { <div id={("flip", index)} class="cursor-pointer" onClick={cx.listener(move |page, _, _, cx| page.set(name.clone(), (!on).into(), cx))}>{switch(on)}</div> }
                            .into_any_element()
                    }
                };
                rsx! {
                    <div class="flex flex-col flex-none">
                        {...(index > 0).then(rule)}
                        <div base={row(option.name.clone(), format!("{}{range}", option.description), true)}>
                            {...changed.then(|| rsx! {
                                <div base={press("history", false)} id={("reset", index)} onClick={cx.listener(move |page, _, _, cx| page.reset(name.clone(), cx))} />
                            })}
                            {control}
                        </div>
                    </div>
                }
                .into_any_element()
            })
            .collect();
        let nothing_found = self.looked && rows.is_empty();

        rsx! {
            <div base={page()} on_action={cx.listener(|page, _: &Commit, window, cx| page.reach.nav.settle(window, cx))}>
                {searched(&self.search, self.search.read(cx).is_focused())}
                {...rows}
                {...nothing_found.then(|| nothing("search_off", "No option by that name"))}
                {...more.then(|| rsx! {
                    <div class="pt-[12px]" text_size={px(12.)} text_color={theme::text_faint()}>
                        {format!("The first {MOST_SHOWN} of {all}. A few letters in the search finds the rest")}
                    </div>
                })}
            </div>
        }
    }
}
