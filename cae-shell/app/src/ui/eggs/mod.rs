//! The desktop egg: seven seconds of something silly along the bottom edge.
//!
//! It was the QML shell's, triggered by an evdev watcher that ships beside
//! the shell and knocks when somebody types the word for it. Nothing about
//! it is useful, which is the point; what matters here is that it is the
//! last thing in `modules/` that was not Quickshell's own plumbing, so it
//! comes across with everything else.
//!
//! One surface, no input region at all — clicks go through it to whatever is
//! underneath — and one clock. Every part of the scene is read off the time
//! since it started rather than kept as state, so there is nothing to get
//! out of step and nothing to reset.

pub mod cinema;
mod paint;
mod sticker;

use std::time::{Duration, Instant};

use gpui::{
    App, AppContext, Bounds, Context, ContentMask, Global, IntoElement, Pixels, Render, Size, Styled, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, canvas, div, layer_shell::*, point, prelude::*,
    px, rgba,
};

use crate::ui::rsx;
use crate::ui::screen;
use sticker::{PARTNER, Placed, RISER};

/// Its own name, so a compositor rule can leave it alone: it is not a panel
/// and must not be blurred or animated like one.
const NAMESPACE: &str = "caelestia-easteregg";

/// How tall a slice of the screen it plays in.
const STAGE: Pixels = px(520.);

/// Roughly sixty a second, which is what the drawing was written for.
const FRAME: Duration = Duration::from_millis(16);

/// The beats, in milliseconds from the start. The scene is over at `SINK`
/// plus the time the two take to go back down.
const RISES: u128 = 0;
const JOINS: u128 = 1200;
const NEARS: u128 = 2200;
const MOVES: u128 = 2900;
const BURSTS: u128 = 4400;
const AFTER: u128 = 5400;
const SINK: u128 = 7000;
const OVER: u128 = 7700;

/// How far the riser leans once it is alongside, in full turns.
const LEAN: f32 = 62. / 360.;

#[derive(Default)]
struct Open(Option<WindowHandle<Scene>>);

impl Global for Open {}

/// What Quickshell is asked about before anything is drawn: two of these at
/// once would be one too many, as with every other piece.
const PIECE: &str = "egg";

/// Plays it, unless it is already playing — or asks the old shell to, while
/// the old shell is still the one that draws it.
pub fn pop(cx: &mut App) {
    crate::ours::when_known(PIECE, cx, |ours, cx| {
        if !ours {
            return cx.background_spawn(async { drop(cae_core::services::ipc("easterEgg", "pop", &[])) }).detach();
        }
        play(cx);
    });
}

fn play(cx: &mut App) {
    if cx.default_global::<Open>().0.is_some_and(|open| open.update(cx, |_, _, _| ()).is_ok()) {
        return;
    }
    let display = screen::focused_display(cx).or_else(|| screen::outputs(cx).first().map(|(_, display)| *display));
    let options = gpui::WindowOptions {
        titlebar: None,
        focus: false,
        display_id: display,
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(0.), px(0.)), Size::new(screen::STRETCH, STAGE)))),
        app_id: Some(NAMESPACE.to_string()),
        window_background: WindowBackgroundAppearance::Transparent,
        kind: WindowKind::LayerShell(LayerShellOptions {
            namespace: NAMESPACE.to_string(),
            layer: Layer::Overlay,
            anchor: Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
            exclusive_zone: Some(px(-1.)),
            keyboard_interactivity: KeyboardInteractivity::None,
            ..Default::default()
        }),
        ..crate::ui::surface::options()
    };
    let opened = cx.open_window(options, |window, cx| cx.new(|cx| Scene::new(window, cx)));
    match opened {
        Ok(window) => cx.set_global(Open(Some(window))),
        Err(error) => eprintln!("cae: cannot play the egg: {error}"),
    }
}

/// Starts the watcher that knocks when the word is typed.
///
/// It flocks itself, so a second copy — one the old shell started, or a
/// compositor autostart — is a no-op, and it needs to read `/dev/input` to
/// do anything at all. The helper binary carries it where it is installed;
/// the script it was before is still there for a checkout without one, and
/// an old binary that has never heard of the tool falls through to it.
pub fn watch() {
    let script = cae_core::about::checkout().join("assets/penis-egg-watch.py");
    std::thread::spawn(move || {
        if watching(std::process::Command::new("caelestia-tools").arg("egg-watch")) {
            return;
        }
        watching(std::process::Command::new("python3").arg(script));
    });
}

/// Runs a watcher until it stops, and says whether it was one. It must not
/// outlive the shell: the kernel signals it on the way out, which covers the
/// deaths an orderly exit does not.
fn watching(watcher: &mut std::process::Command) -> bool {
    unsafe {
        use std::os::unix::process::CommandExt;
        watcher.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    watcher.status().is_ok_and(|status| status.success())
}

/// What the scene is made of that cannot be worked out from the clock: where
/// it happens to stand this time, and which way each droplet happens to go.
#[derive(Clone, Copy)]
struct Roll {
    spawn: f32,
    droplets: [(f32, f32); 9],
}

impl Roll {
    /// A throw of the dice with nothing to seed it but the clock. Good
    /// enough for a joke, and one fewer thing in the lockfile.
    fn new() -> Roll {
        let mut state = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0x2545F491, |since| since.subsec_nanos() as u64 | 1);
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % 10_000) as f32 / 10_000.
        };
        let spawn = 0.1 + next() * 0.4;
        let droplets = std::array::from_fn(|_| ((next() - 0.35) * 0.9, 0.55 + next() * 0.65));
        Roll { spawn, droplets }
    }
}

struct Scene {
    from: Instant,
    roll: Roll,
}

impl Scene {
    fn new(_: &mut Window, cx: &mut Context<Self>) -> Scene {
        cx.spawn(async move |scene, cx| {
            loop {
                cx.background_executor().timer(FRAME).await;
                let going = scene.update(cx, |scene: &mut Scene, cx| {
                    if scene.at() >= OVER {
                        cx.defer(away);
                        return false;
                    }
                    cx.notify();
                    true
                });
                if !matches!(going, Ok(true)) {
                    return;
                }
            }
        })
        .detach();
        Scene { from: Instant::now(), roll: Roll::new() }
    }

    fn at(&self) -> u128 {
        self.from.elapsed().as_millis()
    }
}

fn away(cx: &mut App) {
    let Some(open) = cx.default_global::<Open>().0.take() else { return };
    let _ = open.update(cx, |_, window, _| window.remove_window());
}

// -- The curves the beats are ridden on ------------------------------------

/// How far through something that started at `from` and lasts `over`.
fn through(now: u128, from: u128, over: u128) -> f32 {
    if now <= from {
        return 0.;
    }
    (((now - from) as f32) / over as f32).min(1.)
}

fn in_out(at: f32) -> f32 {
    if at < 0.5 { 2. * at * at } else { 1. - (-2. * at + 2.).powi(2) / 2. }
}

fn in_out_sine(at: f32) -> f32 {
    (1. - (at * std::f32::consts::PI).cos()) / 2.
}

/// Overshoots and comes back, which is what makes the rise land rather than
/// arrive. The overshoot the QML asked for was 2.5.
fn back(at: f32) -> f32 {
    const OVERSHOOT: f32 = 2.5;
    let at = at - 1.;
    at * at * ((OVERSHOOT + 1.) * at + OVERSHOOT) + 1.
}

/// A loop that rises over `up` and falls over `down`, forever.
fn beat(since: u128, up: u128, down: u128, rise: fn(f32) -> f32, fall: fn(f32) -> f32) -> f32 {
    let round = since % (up + down);
    if round < up { rise(round as f32 / up as f32) } else { 1. - fall((round - up) as f32 / down as f32) }
}

fn in_quad(at: f32) -> f32 {
    at * at
}

fn out_quad(at: f32) -> f32 {
    1. - (1. - at) * (1. - at)
}

/// Where the two are, and what they are doing, at one moment.
struct Frame {
    riser: Placed,
    partner: Placed,
    /// Everything right of this is not drawn, which is what makes the tip
    /// read as going in rather than standing in front.
    clip: Option<Pixels>,
    junction: (f32, f32),
    ground: f32,
    spray: f32,
    drips: f32,
    puddle: f32,
}

/// Where everything is at one moment. A free function on purpose: it is
/// worked out while the window is being painted, and a window may not be
/// read from inside its own paint.
fn frame_at(now: u128, roll: &Roll, size: Size<Pixels>) -> Frame {
    {
        let (width, height) = (f32::from(size.width), f32::from(size.height));

        // Up out of the edge, and back down at the end.
        let risen = |from: u128| {
            let up = back(through(now, from, 600));
            let down = in_out(through(now, SINK, 600));
            (up - down).clamp(0., 1.)
        };
        let (riser_up, partner_up) = (risen(RISES), risen(JOINS));

        let partner_x = (roll.spawn * width + 400.).min(width - PARTNER.across - 30.);
        let partner_y = height - PARTNER.down * partner_up;
        let junction = (partner_x + 120., partner_y + 92.);

        // Alongside from the moment it nears until it draws back.
        let near = in_out_sine(through(now, NEARS, 300)) - in_out_sine(through(now, AFTER, 300));
        let lean = LEAN * near;
        let (tip_x, tip_y) = {
            let angle = LEAN * std::f32::consts::TAU;
            (RISER.down * angle.sin(), RISER.down * angle.cos())
        };

        let thrust = if (MOVES..AFTER).contains(&now) { beat(now - MOVES, 260, 240, in_quad, out_quad) } else { 0. };
        let throb = if (NEARS..AFTER).contains(&now) { beat(now - NEARS, 160, 340, out_quad, in_quad) } else { 0. };
        let wobble = beat(now, 350, 350, in_out_sine, in_out_sine) * 2. - 1.;

        let alongside = junction.0 - RISER.across / 2. - tip_x + 30.;
        let withdrawn = partner_x - 210.;
        let rest = if now < NEARS {
            roll.spawn * width
        } else if now < AFTER {
            let to = in_out(through(now, NEARS, 600));
            roll.spawn * width + (alongside - roll.spawn * width) * to
        } else {
            let to = in_out(through(now, AFTER, 600));
            alongside + (withdrawn - alongside) * to
        };
        let dip = (partner_y + 118. + tip_y - height) * near;

        let angle = LEAN * std::f32::consts::TAU;
        let riser = Placed {
            // Both are placed by their foot, which is also what they turn
            // about: risen all the way, the foot sits on the screen's edge.
            at: point(
                px(rest + RISER.across / 2. + thrust * 20. * angle.sin()),
                px(height + RISER.down * (1. - riser_up) + dip - thrust * 20. * angle.cos()),
            ),
            about: (RISER.across / 2., RISER.down),
            turn: lean + wobble * (6. / 360.) * if now < NEARS { 1. } else { 0.2 },
            alpha: 1.,
            scale: 1. + throb * if (BURSTS..AFTER).contains(&now) { 0.09 } else { 0.045 },
        };
        let partner = Placed {
            at: point(px(partner_x + PARTNER.across / 2.), px(height + PARTNER.down * (1. - partner_up))),
            about: (PARTNER.across / 2., PARTNER.down),
            turn: thrust * (3. / 360.),
            alpha: 1.,
            scale: 1. - thrust * 0.04,
        };

        Frame {
            riser,
            partner,
            clip: (now >= NEARS).then(|| px(junction.0)),
            junction,
            ground: height - 12.,
            spray: through(now, BURSTS, 1000),
            drips: through(now, AFTER, 1600),
            puddle: through(now, BURSTS, 2600),
        }
    }
}

/// The colour of everything that is thrown, dripped or left behind.
const SPILT: u32 = 0xf7f7f0;
const SPILT_EDGE: u32 = 0xd8d8cc;

fn blob(window: &mut Window, x: f32, y: f32, across: f32, down: f32, alpha: f32) {
    let put = |x: f32, y: f32| point(px(x), px(y));
    for (colour, width) in [(SPILT, None), (SPILT_EDGE, Some(1.))] {
        let mut path = match width {
            None => gpui::PathBuilder::fill(),
            Some(width) => gpui::PathBuilder::stroke(px(width)),
        };
        sticker::oval(&mut path, x, y, across, down, 0., &put);
        if let Ok(built) = path.build() {
            let shade: gpui::Hsla = rgba((colour << 8) | (alpha.clamp(0., 1.) * 255.) as u32).into();
            window.paint_path(built, shade);
        }
    }
}

impl Render for Scene {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // Purely decorative: whatever is under it is what the pointer finds.
        window.set_input_region(Some(&[]));
        let (now, roll) = (self.at(), self.roll);
        let droplets = roll.droplets;

        rsx! {
            <div class="relative size-full">
                <canvas
                    class="absolute size-full"
                    prepaint={|_, _, _| ()}
                    paint={move |bounds: Bounds<Pixels>, _, window: &mut Window, _: &mut App| {
                        let frame = frame_at(now, &roll, bounds.size);
                        let base = bounds.origin;
                        let cam = crate::ui::eggs::paint::Cam::still(base);

                        sticker::paint(&PARTNER, frame.partner, &cam, window);

                        match frame.clip {
                            Some(edge) => {
                                let mask = ContentMask { bounds: Bounds::new(base, gpui::size(edge, bounds.size.height)) };
                                window.with_content_mask(Some(mask), |window| sticker::paint(&RISER, frame.riser, &cam, window));
                            }
                            None => sticker::paint(&RISER, frame.riser, &cam, window),
                        }

                        // Thrown at the moment it bursts, each on its own arc.
                        if frame.spray > 0. && frame.spray < 1. {
                            for (index, (across, up)) in droplets.into_iter().enumerate() {
                                let gone = ((frame.spray - index as f32 * 0.045) / 0.55).clamp(0., 1.);
                                if gone <= 0. || gone >= 1. {
                                    continue;
                                }
                                let size = 4.5 + (index % 3) as f32 * 2.;
                                let (x, y) = (frame.junction.0 + across * 260. * gone, frame.junction.1 - up * 260. * gone + 330. * gone * gone);
                                if y < frame.ground - size {
                                    blob(window, f32::from(base.x) + x, f32::from(base.y) + y, size, size, 1.);
                                }
                            }
                        }

                        // What hangs afterwards, stretching until it lets go.
                        if frame.drips > 0. && frame.drips < 1. {
                            let hanging = [(0.35, 120., 160.), (0.6, 106., 150.), (0.5, 34., 44.)];
                            for (index, (until, dx, dy)) in hanging.into_iter().enumerate() {
                                let (anchor_x, anchor_y) = if index == 2 {
                                    (f32::from(frame.riser.at.x) + dx, f32::from(frame.riser.at.y) + dy)
                                } else {
                                    (frame.junction.0 - 120. + dx, frame.junction.1 - 92. + dy)
                                };
                                if frame.drips < until {
                                    let stretch = 5. + 13. * (frame.drips / until);
                                    blob(window, f32::from(base.x) + anchor_x, f32::from(base.y) + anchor_y, 4.5, stretch, 1.);
                                } else {
                                    let fall = ((frame.drips - until) / 0.3).min(1.);
                                    if fall < 1. {
                                        let y = anchor_y + (frame.ground - anchor_y) * fall * fall;
                                        blob(window, f32::from(base.x) + anchor_x, f32::from(base.y) + y, 4.5, 11., 1.);
                                    }
                                }
                            }
                        }

                        // And what lands has to end up somewhere.
                        if now >= BURSTS && frame.puddle > 0. {
                            let width = 20. + 70. * frame.puddle;
                            blob(window, f32::from(base.x) + frame.junction.0, f32::from(base.y) + frame.ground, width, 7., (frame.puddle * 4.).min(1.) * 0.9);
                        }
                    }}
                />
            </div>
        }
    }
}
