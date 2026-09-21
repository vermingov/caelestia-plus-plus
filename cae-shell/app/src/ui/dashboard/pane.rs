//! The pane: the glass, the pages along its top, and how it comes and goes.

use std::time::Duration;

use gpui::{
    AnyView, App, AppContext, Bounds, Context, Entity, IntoElement, Pixels, Render, ScrollWheelEvent, Size, Styled,
    Task, WeakEntity, Window, canvas, div, point, prelude::*, px,
};

use super::forecast::Forecast;
use super::{Dashboards, Tab, home, media, performance, weather};
use crate::ease::{Curve, Tween};
use crate::feeds::Feeds;
use crate::theme;
use crate::ui::glyph::glyph;
use crate::ui::{pointer, rsx};

pub const WIDTH: Pixels = px(900.);
pub const HEIGHT: Pixels = px(448.);
/// Room for the shadow it casts: to both sides and below. Nothing above,
/// because its top edge is the bar's bottom edge. In numbers, because pixels
/// cannot be added up in a constant.
const SHADOW: f32 = 48.;
/// How far down the bar reaches, stepped over rather than asked about.
const BAR: f32 = 50.;
pub const SURFACE: Size<Pixels> = Size { width: px(48. + 900. + 48.), height: px(50. + 448. + 48.) };

const HEAD: Pixels = px(46.);
const TAB: Pixels = px(132.);

const ENTER: Duration = Duration::from_millis(260);
const LEAVE: Duration = Duration::from_millis(170);
const SLIDE: Duration = Duration::from_millis(220);
/// Crossing from the edge of the screen onto the pane takes the pointer off
/// both for a moment, and that must not count as leaving.
const GRACE: Duration = Duration::from_millis(260);
/// How long one that the pointer has not reached yet waits for it: it was
/// opened by reaching for the edge, and the hand may have moved on.
const UNCLAIMED: Duration = Duration::from_millis(1400);

/// How things are said here, as the settings have it when the pane opens.
#[derive(Clone, Copy, Default)]
pub struct Units {
    pub fahrenheit: bool,
    pub twelve_hour: bool,
}

impl Units {
    fn read() -> Units {
        let shell = cae_core::config::read(cae_core::config::File::Shell);
        let flag = |path: &str| cae_core::config::lookup(&shell, path).and_then(serde_json::Value::as_bool).unwrap_or(false);
        Units { fahrenheit: flag("services.useFahrenheit"), twelve_hour: flag("services.useTwelveHourClock") }
    }

    /// A temperature, which is kept in Celsius, without its unit: `14°`.
    pub fn degrees(self, celsius: f64) -> String {
        let shown = if self.fahrenheit { celsius * 1.8 + 32. } else { celsius };
        format!("{}°", shown.round() as i64)
    }
}

/// What every page is handed.
#[derive(Clone)]
pub struct Reach {
    pub feeds: Feeds,
    pub forecast: Entity<Forecast>,
    pub units: Units,
}

pub struct Pane {
    reach: Reach,
    dashboards: WeakEntity<Dashboards>,
    tabs: Vec<Tab>,
    tab: Tab,
    /// The pages that have been looked at since it opened, kept so that going
    /// back to one is going back to it as it was left.
    pages: Vec<(Tab, AnyView)>,
    /// Whether the pointer decides when it goes: true for one opened by
    /// reaching for it, and for one opened by a key once it has been touched.
    follows_pointer: bool,
    closing: Option<Task<()>>,
    shown: Tween,
    marker: Tween,
    leaving: bool,
}

impl Pane {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tabs: Vec<Tab>,
        tab: Tab,
        by_hover: bool,
        feeds: &Feeds,
        forecast: Entity<Forecast>,
        dashboards: WeakEntity<Dashboards>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Pane {
        let mut shown = Tween::still(0.);
        shown.go(1., ENTER, Curve::Arrive);
        let at = tabs.iter().position(|known| *known == tab).unwrap_or(0) as f32;
        let mut pane = Pane {
            reach: Reach { feeds: feeds.clone(), forecast, units: Units::read() },
            dashboards,
            tabs,
            tab,
            pages: Vec::new(),
            follows_pointer: by_hover,
            closing: None,
            shown,
            marker: Tween::still(at),
            leaving: false,
        };
        if by_hover {
            pane.close_after(UNCLAIMED, window, cx);
        }
        pane
    }

    fn page(&mut self, cx: &mut Context<Self>) -> AnyView {
        if let Some((_, page)) = self.pages.iter().find(|(tab, _)| *tab == self.tab) {
            return page.clone();
        }
        let page: AnyView = match self.tab {
            Tab::Home => cx.new(|cx| home::Home::new(&self.reach, cx)).into(),
            Tab::Media => cx.new(|cx| media::Media::new(&self.reach, cx)).into(),
            Tab::Performance => cx.new(|cx| performance::Performance::new(&self.reach, cx)).into(),
            Tab::Weather => cx.new(|cx| weather::Weather::new(&self.reach, cx)).into(),
        };
        self.pages.push((self.tab, page.clone()));
        page
    }

    fn turn_to(&mut self, tab: Tab, cx: &mut Context<Self>) {
        let Some(at) = self.tabs.iter().position(|known| *known == tab) else { return };
        if tab != self.tab {
            self.tab = tab;
            self.marker.go(at as f32, SLIDE, Curve::Settle);
            cx.notify();
        }
    }

    fn turn_by(&mut self, by: isize, cx: &mut Context<Self>) {
        let at = self.tabs.iter().position(|known| *known == self.tab).unwrap_or(0) as isize;
        let to = (at + by).clamp(0, self.tabs.len() as isize - 1) as usize;
        self.turn_to(self.tabs[to], cx);
    }

    fn hovered(&mut self, hovered: bool, window: &mut Window, cx: &mut Context<Self>) {
        if hovered {
            // Touched, so from here on it goes when the pointer does,
            // however it came.
            (self.follows_pointer, self.closing) = (true, None);
        } else if self.follows_pointer {
            self.close_after(GRACE, window, cx);
        }
    }

    fn close_after(&mut self, wait: Duration, window: &mut Window, cx: &mut Context<Self>) {
        self.closing = Some(cx.spawn_in(window, async move |pane, cx| {
            cx.background_executor().timer(wait).await;
            let _ = pane.update_in(cx, |pane, window, cx| pane.leave(window, cx));
        }));
    }

    /// Plays the exit, and only then goes.
    pub fn leave(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.leaving, true) {
            return;
        }
        self.shown.go(0., LEAVE, Curve::In);
        cx.notify();
        let (dashboards, tab) = (self.dashboards.clone(), self.tab);
        cx.spawn_in(window, async move |_, cx| {
            cx.background_executor().timer(LEAVE).await;
            let _ = cx.update(|window, cx| {
                window.remove_window();
                let _ = dashboards.update(cx, |dashboards, _| dashboards.gone(tab));
            });
        })
        .detach();
    }
}

/// Tells the compositor which part of the surface is the pane: it, and the
/// gap between it and the two edges of the screen it stands off. With the
/// gap left out, a pointer resting on the edge it was opened from would be
/// on nothing, and the pane would close under it.
/// Only the pane takes the pointer; the shadow's room around it does not, or
/// the dashboard would hold a hover a hand's width away from itself.
fn reach(window: &Window) {
    let region = Bounds::new(point(px(SHADOW), px(BAR)), Size::new(WIDTH, HEIGHT));
    window.set_input_region(Some(&[region]));
}

impl Render for Pane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !(self.shown.done() && self.marker.done()) {
            window.request_animation_frame();
        }
        let (shown, marker) = (self.shown.value(), self.marker.value());
        let page = self.page(cx);
        let several = self.tabs.len() > 1;

        let head = rsx! {
            <div
                id="pages"
                class="relative flex flex-none items-center px-[14px]"
                h={HEAD}
                onScrollWheel={cx.listener(|pane, event: &ScrollWheelEvent, _, cx| {
                    let down = f32::from(event.delta.pixel_delta(px(20.)).y) < 0.;
                    pane.turn_by(if down { 1 } else { -1 }, cx);
                })}
            >
                {...several.then(|| rsx! {
                    <div
                        class="absolute h-[30px]"
                        top={px(8.)}
                        left={px(14.) + TAB * marker}
                        w={TAB}
                        rounded={px(9.)}
                        bg={theme::highlight()}
                        shadow={theme::highlight_shadows()}
                    />
                })}
                {for (index, tab) in self.tabs.clone().into_iter().enumerate() {
                    <div
                        id={("page", index)}
                        class="relative flex flex-none items-center justify-center gap-[8px] h-[30px] cursor-pointer"
                        w={TAB}
                        text_size={px(12.5)}
                        text_color={if tab == self.tab { theme::text() } else { theme::text_dim() }}
                        when={(tab != self.tab, |tab| tab.hover(|style| style.text_color(theme::text())))}
                        onClick={cx.listener(move |pane, _, _, cx| pane.turn_to(tab, cx))}
                    >
                        {glyph(tab.glyph(), px(16.))}
                        {tab.title()}
                    </div>
                }}
            </div>
        };

        rsx! {
            <div class="relative size-full" font_family={theme::FONT}>
                // The pane and the gap round it, as one thing to be on or off.
                <div
                    id="dashboard"
                    class="absolute flex"
                    left={px(SHADOW)}
                    top={px(BAR)}
                    w={WIDTH}
                    h={HEIGHT}
                    onHover={cx.listener(|pane, hovered: &bool, window, cx| pane.hovered(*hovered, window, cx))}
                >
                    <div
                        id="glass"
                        // Not occluding: a box that occludes takes the pointer
                        // from the one it is in, and the one it is in is what
                        // knows whether the pointer is here at all.
                        class="absolute overflow-hidden"
                        left={px(0.)}
                        // Up one pixel, over the hairline the bar draws inside
                        // its own bottom edge. That line is the bar saying
                        // where it ends, and this is meant to be where it does
                        // not.
                        top={px(-1.)}
                        w={WIDTH}
                        // It grows downward out of the bar rather than
                        // appearing and fading up: the height is the
                        // animation, and what is inside keeps its own so the
                        // contents are uncovered rather than squashed.
                        h={px(1.) + HEIGHT * shown}
                        // Square where it meets the bar, round where it ends:
                        // one shape continuing out of another, rather than a
                        // second shape parked under the first.
                        rounded_b={px(15.)}
                        bg={theme::pane()}
                        shadow={theme::hanging_shadows()}
                        text_color={theme::text()}
                    >
                        // The bar's last colour, fading out into the glass:
                        // under the contents, so it darkens the material and
                        // not what is written on it.
                        <div class="absolute w-full" left={px(0.)} top={px(0.)} h={theme::JOIN} bg={theme::join()} />
                        <div class="absolute flex flex-col" left={px(0.)} top={px(1.)} w={WIDTH} h={HEIGHT}>
                            {head}
                            <div class="flex-none h-[1px]" bg={theme::white(0.06)} />
                            <div class="flex flex-1 min-h-[0px]">{page}</div>
                        </div>
                    </div>
                </div>
                <canvas class="absolute size-full" prepaint={|_, window: &mut Window, _: &mut App| reach(window)} paint={|_, _, _, _| ()} />
                {pointer::see_out()}
            </div>
        }
    }
}
