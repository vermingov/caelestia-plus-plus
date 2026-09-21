//! The audio visualiser's numbers.
//!
//! The shell gets these from a compiled cava provider. There is no `cava`
//! binary on this machine and a Tauri app has no Quickshell plugin to borrow,
//! so the bar does the whole thing itself: record the default sink's monitor
//! with `pw-record`, window it, transform it, and fold the result into the
//! handful of bars a 38px strip can actually show.
//!
//! The arithmetic is deliberately plain. An FFT of 1024 points at 45 frames a
//! second is a few hundred microseconds of work, and a dependency that pulls
//! in a linear algebra stack to do it would cost more to build than it saves
//! to run.

use std::io::{ErrorKind, Read};
use std::process::{ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use crate::children;

/// Power of two, and the only one that matters: 1024 samples at 44.1kHz is a
/// 23ms window, which is short enough to feel immediate and long enough to
/// resolve the bass.
const WINDOW: usize = 1024;
const RATE: f32 = 44_100.0;

/// How many bars the bar draws. Enough to read as a spectrum, few enough that
/// each one is still a few pixels wide.
pub const BARS: usize = 28;

/// Anything below this is silence as far as the visualiser is concerned, and
/// silence is what lets it stop working entirely.
const FLOOR: f32 = 0.0016;

/// The most frames a second the front end is given.
///
/// The analyser produces one per 1024 samples, which is 43 a second, and each
/// one is an IPC message and a repaint of every band. Past about thirty the
/// difference is not visible and the cost is: this alone is most of what the
/// visualiser charges for being on.
/// Fifteen is where the bars still read as motion rather than as a slideshow,
/// and each frame past it is a full repaint of a full-width layer surface.
const MAX_FPS: u64 = 15;

/// Rises fast, falls slowly: a spectrum that decays at the rate it attacks
/// reads as flickering rather than as sound.
///
/// Not too fast, though. At 0.45 a bar was most of the way to a new level in
/// one frame, which is not reacting to the music so much as following the
/// analyser's own noise — every window of a real signal differs from the last
/// whether or not anything audible changed, and at that rate all of it shows.
const ATTACK: f32 = 0.26;
const DECAY: f32 = 0.10;

/// The quietest a band's own reference may fall to.
///
/// Without a floor, a band with nothing in it divides by nothing and silence
/// comes out as a full-height bar of hiss.
const LEAST: f32 = 0.34;

/// How fast a band's reference follows what it is hearing. Up within a bar
/// of music, down over several seconds, so a loud passage does not
/// permanently flatten the quiet one after it.
const LOUDER: f32 = 0.08;
const QUIETER: f32 = 0.004;

/// How much of each bar is its neighbours.
///
/// The envelope above smooths each bar through time and leaves it deaf to the
/// ones beside it, so a band whose peak lands a bin to the left of another's
/// jumps while its neighbour does not, and the row reads as a picket fence
/// rattling rather than a shape moving. A little of each side turns the
/// same numbers into a curve. Little: at much more than this a kick drum
/// lifts the whole row and the thing stops meaning anything.
const NEIGHBOURS: f32 = 0.22;

/// How long the recorder may write nothing before whatever was playing is
/// taken to have stopped. Its buffers are 20ms apart, so this is several
/// missing in a row rather than one arriving late.
const STALL: Duration = Duration::from_millis(120);

/// How long one window lasts. Once the recorder has stalled, silence is fed
/// to the analyser at this pace, so the bars fall exactly as fast as they
/// would have if the silence had been recorded.
const WINDOW_TIME: Duration = Duration::from_micros(WINDOW as u64 * 1_000_000 / RATE as u64);

/// The twiddle factors for one window size, computed once.
///
/// They depend only on the window length, and the transform needs a thousand
/// of them per call — recomputing a sine and cosine for each, forty-three
/// times a second, was most of what the analyser cost.
fn twiddles() -> &'static [(f32, f32)] {
    static TWIDDLES: std::sync::OnceLock<Vec<(f32, f32)>> = std::sync::OnceLock::new();
    TWIDDLES.get_or_init(|| {
        let mut factors = Vec::with_capacity(WINDOW);
        let mut span = 1;
        while span < WINDOW {
            let angle = std::f32::consts::PI / span as f32;
            for group in 0..span {
                factors.push((-angle * group as f32).sin_cos());
            }
            span <<= 1;
        }
        factors
    })
}

/// One in-place iterative radix-2 FFT, on interleaved real/imaginary pairs.
///
/// Iterative rather than recursive because the recursion would allocate a
/// vector per level, forty-five times a second, forever.
fn fft(real: &mut [f32], imaginary: &mut [f32]) {
    let n = real.len();
    debug_assert!(n.is_power_of_two());

    // Bit-reversal permutation, which is what makes the butterflies below
    // read consecutive memory instead of chasing the recursion's order.
    let mut target = 0usize;
    for position in 0..n {
        if target > position {
            real.swap(target, position);
            imaginary.swap(target, position);
        }
        let mut mask = n >> 1;
        while target & mask != 0 {
            target &= !mask;
            mask >>= 1;
        }
        target |= mask;
    }

    let factors = twiddles();
    let mut factor = 0;
    let mut span = 1;
    while span < n {
        let step = span << 1;
        for group in 0..span {
            let (sin, cos) = factors[factor + group];
            let mut index = group;
            while index < n {
                let pair = index + span;
                let re = cos * real[pair] - sin * imaginary[pair];
                let im = cos * imaginary[pair] + sin * real[pair];
                real[pair] = real[index] - re;
                imaginary[pair] = imaginary[index] - im;
                real[index] += re;
                imaginary[index] += im;
                index += step;
            }
        }
        factor += span;
        span = step;
    }
}

/// Where each bar's slice of the spectrum starts, in bins.
///
/// Logarithmic, because hearing is: a linear split gives twenty bars of treble
/// nobody can hear and one bar containing every note anybody is playing.
fn band_edges() -> Vec<usize> {
    const LOW: f32 = 40.0;
    const HIGH: f32 = 12_000.0;
    let bins = WINDOW / 2;

    (0..=BARS)
        .map(|bar| {
            let fraction = bar as f32 / BARS as f32;
            let frequency = LOW * (HIGH / LOW).powf(fraction);
            ((frequency / (RATE / WINDOW as f32)) as usize).min(bins - 1)
        })
        .collect()
}

/// Turns one window of samples into one frame of bars.
struct Analyser {
    window: Vec<f32>,
    edges: Vec<usize>,
    levels: [f32; BARS],
    /// What each band has been reaching lately.
    ///
    /// A fixed tilt cannot even this row out. How much energy sits in a band
    /// depends on the music, not only on the frequency: a track with a busy
    /// bass and nothing at 4kHz drew a mountain at one end and a flat line
    /// at the other, and the same tilt that fixed that track ruined the
    /// next. Each band is measured against what it has been doing instead,
    /// so a band that is quiet in this song still has somewhere to move.
    loudest: [f32; BARS],
}

impl Analyser {
    fn new() -> Analyser {
        // Hann, precomputed: it never changes and a cosine per sample per
        // frame is the one avoidable cost in here.
        let window = (0..WINDOW)
            .map(|i| {
                let phase = std::f32::consts::TAU * i as f32 / (WINDOW - 1) as f32;
                0.5 - 0.5 * phase.cos()
            })
            .collect();
        Analyser { window, edges: band_edges(), levels: [0.0; BARS], loudest: [LEAST; BARS] }
    }

    fn frame(&mut self, samples: &[f32]) -> [f32; BARS] {
        let mut real: Vec<f32> = samples
            .iter()
            .zip(&self.window)
            .map(|(sample, window)| sample * window)
            .collect();
        let mut imaginary = vec![0.0; WINDOW];
        fft(&mut real, &mut imaginary);

        let mut heard = [0.0f32; BARS];
        for bar in 0..BARS {
            let (from, to) = (self.edges[bar], self.edges[bar + 1].max(self.edges[bar] + 1));
            let mut peak = 0.0f32;
            for bin in from..to.min(WINDOW / 2) {
                let magnitude = (real[bin] * real[bin] + imaginary[bin] * imaginary[bin]).sqrt();
                peak = peak.max(magnitude);
            }

            // Decibels, then normalised: amplitude is a ratio, and a linear
            // bar of a ratio is a bar that only ever moves at the very top.
            let decibels = 20.0 * (peak / WINDOW as f32 * 2.0 + 1e-9).log10();
            let level = ((decibels + 62.0) / 62.0).clamp(0.0, 1.0);

            // Treble carries far less energy than bass and would otherwise
            // never leave the floor. A gentler tilt than before, because the
            // band's own reference below does most of this work now and the
            // two together overshot.
            let tilt = 1.0 + 0.45 * (bar as f32 / BARS as f32);
            heard[bar] = (level * tilt).clamp(0.0, 1.0);

            // Measured against what this band has been reaching.
            let rate = if heard[bar] > self.loudest[bar] { LOUDER } else { QUIETER };
            self.loudest[bar] += (heard[bar] - self.loudest[bar]) * rate;
            self.loudest[bar] = self.loudest[bar].max(LEAST);
            heard[bar] = (heard[bar] / self.loudest[bar]).clamp(0.0, 1.0);
        }

        // Across, then through time. The ends lean on the one neighbour they
        // have rather than on a nought that is not there, which would pull
        // the first and last bars down for no reason.
        let mut shaped = heard;
        for bar in 0..BARS {
            let left = heard[bar.saturating_sub(1)];
            let right = heard[(bar + 1).min(BARS - 1)];
            shaped[bar] = heard[bar] * (1.0 - NEIGHBOURS) + (left + right) / 2.0 * NEIGHBOURS;
        }

        for bar in 0..BARS {
            let rate = if shaped[bar] > self.levels[bar] { ATTACK } else { DECAY };
            self.levels[bar] += (shaped[bar] - self.levels[bar]) * rate;
        }
        self.levels
    }
}

/// Quantises a frame to bytes.
///
/// The levels cross an IPC boundary as JSON thirty times a second, and a
/// float spells out as nine characters where a byte spells out as three. At
/// 16px tall there are not 256 distinguishable heights, let alone 2^24.
fn quantise(bars: [f32; BARS]) -> Vec<u8> {
    bars.iter().map(|level| (level.clamp(0.0, 1.0) * 255.0) as u8).collect()
}

/// The recorder, ready to start.
fn recorder() -> Command {
    let mut command = Command::new("pw-record");
    command
        .args([
            // Raw, or pw-record writes a container header first and every
            // sample after it is read four bytes out of phase.
            "--raw",
            // `stream.capture.sink` is how PipeWire spells "monitor what is
            // coming out of the default sink", and leaving the target to the
            // session manager is what makes it *stay* the default sink: the
            // stream is moved when the default changes. Naming a target here
            // pins it instead — and `--target` takes a serial, not the id
            // `wpctl` prints, so a pinned one that stopped matching fell back
            // to the default source and the bars drew the microphone.
            //
            // Passive, so the capture alone never keeps the sink running.
            // With nothing playing the sink suspends, the recorder writes
            // nothing, and this thread sleeps in `poll` instead of analysing
            // forty-three windows of silence a second, all day.
            "--properties={ stream.capture.sink=true node.passive=true node.name=caelestia-visualiser }",
            "--rate=44100",
            "--channels=1",
            "--format=f32",
            "--latency=20ms",
            "-",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    // An orphaned recorder stays on the sink's monitor for good.
    children::bind_to_parent(&mut command);
    command
}

/// What came of waiting for the next window.
enum Next {
    Window,
    /// Nothing arrived in time: whatever was playing has stopped.
    Stalled,
    /// The recorder is gone.
    Closed,
}

/// The recorder's output, a window at a time.
///
/// The capture is passive, so when the music stops the samples simply stop
/// coming, often part-way through a window. A plain `read_exact` would sit on
/// that half window until the next song, with the bars frozen wherever the
/// last one left them.
struct Windows {
    stdout: ChildStdout,
    raw: Vec<u8>,
    filled: usize,
}

impl Windows {
    fn new(stdout: ChildStdout) -> Windows {
        Windows { stdout, raw: vec![0; WINDOW * 4], filled: 0 }
    }

    /// Waits for the rest of the current window, giving up once the recorder
    /// has written nothing for `patience`. Without one it waits for as long
    /// as it takes. What had already arrived is kept for the next call.
    fn next(&mut self, patience: Option<Duration>) -> Next {
        while self.filled < self.raw.len() {
            if !children::readable(&self.stdout, patience) {
                return Next::Stalled;
            }
            match self.stdout.read(&mut self.raw[self.filled..]) {
                Ok(0) => return Next::Closed,
                Ok(count) => self.filled += count,
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(_) => return Next::Closed,
            }
        }
        self.filled = 0;
        Next::Window
    }

    /// The window `next` last completed, as samples.
    fn decode(&self, samples: &mut [f32]) {
        for (sample, bytes) in samples.iter_mut().zip(self.raw.chunks_exact(4)) {
            *sample = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        }
    }
}

/// How long to wait for the next window before treating it as silence.
fn patience(showing: bool, stalled: bool) -> Option<Duration> {
    match (showing, stalled) {
        // With the bars down there is nothing left to animate, so nothing to
        // wake up for until sound comes back.
        (false, _) => None,
        (true, false) => Some(STALL),
        (true, true) => Some(WINDOW_TIME),
    }
}

/// Decides, window by window, what the front end is told.
struct Feed {
    analyser: Analyser,
    /// Whether the bars are up, which is whether anything on screen can move.
    showing: bool,
    /// The last frame handed over.
    shown: Vec<u8>,
    last_sent: Option<Instant>,
}

impl Feed {
    fn new() -> Feed {
        Feed { analyser: Analyser::new(), showing: false, shown: Vec::new(), last_sent: None }
    }

    /// The frame this window is worth, if it is worth one: the bars, and
    /// whether they are live.
    fn window(&mut self, samples: &[f32]) -> Option<(Vec<u8>, bool)> {
        let peak = samples.iter().fold(0.0f32, |peak, sample| peak.max(sample.abs()));

        // Silence with the bars already down. Something else can keep the
        // sink running with nothing audible on it, and there is no reason to
        // transform a window that cannot change the picture.
        if peak < FLOOR && !self.showing {
            return None;
        }

        let bars = self.analyser.frame(samples);
        if peak < FLOOR && bars.iter().all(|level| *level < 0.01) {
            // One last frame to put the bars down, then nothing until
            // something plays again.
            self.showing = false;
            self.shown.clear();
            return Some((vec![0; BARS], false));
        }
        self.showing = true;

        // Analysed every window regardless — the smoothing depends on it —
        // but only handed over at the rate anybody can see.
        let interval = Duration::from_millis(1000 / MAX_FPS);
        if self.last_sent.is_some_and(|sent| sent.elapsed() < interval) {
            return None;
        }

        // A held note quantises to the bytes it had a frame ago, and a frame
        // that draws the same picture is a whole-surface repaint for nothing.
        let frame = quantise(bars);
        if frame == self.shown {
            return None;
        }
        self.last_sent = Some(Instant::now());
        self.shown.clone_from(&frame);
        Some((frame, true))
    }
}

/// Records the default sink's monitor and calls `on_frame` with each frame of
/// bars, until the process ends.
///
/// Blocks; meant for its own thread. Silence is not sent: once the bars have
/// drained to nothing, nothing is emitted until sound returns, so a quiet
/// desktop costs one sleeping process and no repaints at all.
pub fn watch(mut on_frame: impl FnMut(Vec<u8>, bool)) {
    loop {
        let Ok(mut recorder) = recorder().spawn() else {
            eprintln!("caelestia-bar: pw-record is not available, so no visualiser");
            return;
        };
        let Some(stdout) = recorder.stdout.take() else { return };

        let mut windows = Windows::new(stdout);
        let mut feed = Feed::new();
        let mut samples = vec![0.0f32; WINDOW];
        let mut stalled = false;

        loop {
            match windows.next(patience(feed.showing, stalled)) {
                Next::Window => {
                    stalled = false;
                    windows.decode(&mut samples);
                }
                // The bars are still up and the music is gone: silence, at
                // the pace it would have been recorded, until they are down.
                Next::Stalled => {
                    stalled = true;
                    samples.fill(0.0);
                }
                Next::Closed => break,
            }

            if let Some((bars, live)) = feed.window(&samples) {
                on_frame(bars, live);
            }
        }

        // The recorder follows the default sink by itself, so it only exits
        // when PipeWire does; that is a reconnect, not a failure.
        let _ = recorder.wait();
        std::thread::sleep(Duration::from_millis(500));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One window of a 1kHz tone at half scale.
    fn tone() -> Vec<f32> {
        (0..WINDOW)
            .map(|i| (std::f32::consts::TAU * 1000.0 * i as f32 / RATE).sin() * 0.5)
            .collect()
    }

    /// A pure tone should light the band that contains it and leave the rest
    /// alone. This is the whole contract: if the transform or the banding is
    /// wrong, the bars are decorative noise rather than the music.
    #[test]
    fn a_tone_lands_in_one_band() {
        let mut analyser = Analyser::new();
        let tone = tone();

        // Several frames, because the smoothing means one frame only gets
        // part of the way there.
        let mut bars = [0.0; BARS];
        for _ in 0..40 {
            bars = analyser.frame(&tone);
        }

        let loudest = bars
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).expect("levels are never NaN"))
            .map(|(index, _)| index)
            .expect("there is always a loudest band");

        let edges = band_edges();
        let bin = (1000.0 / (RATE / WINDOW as f32)) as usize;
        let expected = (0..BARS)
            .find(|band| edges[*band] <= bin && bin < edges[band + 1])
            .expect("1kHz falls inside the analysed range");

        // Within a band either way: a tone that sits between two bins leaks
        // into both, and the treble tilt can lift the higher neighbour past
        // the band the tone is nominally in. Two bands out would mean the
        // banding itself is wrong.
        assert!(
            loudest.abs_diff(expected) <= 1,
            "1kHz is in band {expected} but lit band {loudest}"
        );

        // And the rest of the spectrum should be nowhere near it, or the
        // transform is smearing energy across every band.
        let far = bars[..expected.saturating_sub(3)].iter().fold(0.0f32, |peak, l| peak.max(*l));
        assert!(far < bars[loudest] * 0.5, "bass bands reached {far} on a 1kHz tone");
    }

    #[test]
    fn a_bar_no_longer_jumps_most_of_the_way_in_one_frame() {
        // The complaint this answers is "too reactive". A bar that arrives
        // in one or two frames is following the analyser's noise as much as
        // the music: every window of a real signal differs from the last
        // whether or not anything audible changed.
        let mut analyser = Analyser::new();
        let loud = tone();
        let reached = |analyser: &mut Analyser, frames: usize| {
            let mut last = [0.0; BARS];
            for _ in 0..frames {
                last = analyser.frame(&loud);
            }
            last.iter().cloned().fold(0.0f32, f32::max)
        };
        let after_one = reached(&mut analyser, 1);
        assert!(after_one < 0.45, "a single frame takes it to {after_one}, which is a jump not a rise");

        // It still gets there, and well within the time a note lasts.
        let mut analyser = Analyser::new();
        let after_fifteen = reached(&mut analyser, 15);
        assert!(after_fifteen > 0.7, "a second of the same tone only reaches {after_fifteen}");
    }

    #[test]
    fn a_quiet_band_still_has_somewhere_to_move() {
        // The complaint this answers is that it looked like mountains. Real
        // music is loud at one end and thin at the other, and white noise is
        // not — a row fed noise comes out even whatever the code does, which
        // is why the first version of this test passed without the fix and
        // proved nothing.
        //
        // So: a heavy low note and a faint high one, which is most music.
        // Without each band being measured against itself, the high band
        // sits on the floor for the whole song.
        let mut analyser = Analyser::new();
        let window: Vec<f32> = (0..WINDOW)
            .map(|at| {
                let time = at as f32 / RATE as f32;
                (time * std::f32::consts::TAU * 110.0).sin() * 0.5
                    + (time * std::f32::consts::TAU * 6000.0).sin() * 0.012
            })
            .collect();
        let mut bars = [0.0; BARS];
        for _ in 0..200 {
            bars = analyser.frame(&window);
        }

        let high = bars[BARS - 4..].iter().cloned().fold(0.0f32, f32::max);
        let low = bars[..4].iter().cloned().fold(0.0f32, f32::max);
        assert!(low > 0.5, "the loud note only reaches {low}");
        assert!(high > 0.3, "the faint note is left at {high} while the loud one is at {low}");
    }

    #[test]
    fn a_bar_is_pulled_toward_the_ones_beside_it() {
        // One band loud and its neighbours silent is what a picket fence
        // looks like. The neighbours should come up with it and it should
        // come down toward them, or the row rattles instead of moving.
        let mut analyser = Analyser::new();
        let mut spike = vec![0.0f32; WINDOW];
        for (at, sample) in spike.iter_mut().enumerate() {
            *sample = (at as f32 * std::f32::consts::TAU * 1000.0 / RATE as f32).sin() * 0.5;
        }
        let mut bars = [0.0; BARS];
        for _ in 0..40 {
            bars = analyser.frame(&spike);
        }
        let loudest = bars.iter().cloned().fold(0.0f32, f32::max);
        let at = bars.iter().position(|level| *level == loudest).unwrap();
        let beside = [at.saturating_sub(1), (at + 1).min(BARS - 1)]
            .iter()
            .map(|near| bars[*near])
            .fold(0.0f32, f32::max);
        assert!(beside > loudest * 0.15, "the neighbours of a loud band sit at {beside} against {loudest}");
    }

    #[test]
    fn silence_is_flat() {
        let mut analyser = Analyser::new();
        let mut bars = [1.0; BARS];
        for _ in 0..60 {
            bars = analyser.frame(&vec![0.0; WINDOW]);
        }
        assert!(bars.iter().all(|level| *level < 0.01), "silence did not drain");
    }

    #[test]
    fn the_transform_round_trips_a_constant() {
        // A DC signal has all its energy in bin zero and none anywhere else,
        // which catches a bit-reversal or butterfly that is subtly wrong.
        let mut real = vec![1.0f32; 8];
        let mut imaginary = vec![0.0f32; 8];
        fft(&mut real, &mut imaginary);

        assert!((real[0] - 8.0).abs() < 1e-4, "bin 0 was {}", real[0]);
        for bin in 1..8 {
            let magnitude = (real[bin] * real[bin] + imaginary[bin] * imaginary[bin]).sqrt();
            assert!(magnitude < 1e-4, "bin {bin} was {magnitude}");
        }
    }

    #[test]
    fn bands_rise_and_never_run_backwards() {
        let edges = band_edges();
        assert_eq!(edges.len(), BARS + 1);
        for pair in edges.windows(2) {
            assert!(pair[1] >= pair[0], "band edges went backwards: {pair:?}");
        }
    }

    /// When the music stops the bars have to come down, and the front end has
    /// to be told so exactly once: a second "down" is a repaint for nothing,
    /// and none at all leaves the last chord on the bar until the next song.
    #[test]
    fn the_bars_are_put_down_once() {
        let mut feed = Feed::new();
        assert!(matches!(feed.window(&tone()), Some((_, true))), "sound was not shown");

        let silence = vec![0.0; WINDOW];
        let downs: Vec<Vec<u8>> = (0..400)
            .filter_map(|_| feed.window(&silence))
            .filter(|(_, live)| !live)
            .map(|(bars, _)| bars)
            .collect();

        assert_eq!(downs.len(), 1, "the bars were put down {} times", downs.len());
        assert!(downs[0].iter().all(|level| *level == 0));
    }

    #[test]
    fn silence_on_a_quiet_bar_says_nothing() {
        let mut feed = Feed::new();
        let silence = vec![0.0; WINDOW];
        assert!((0..100).all(|_| feed.window(&silence).is_none()));
    }

    #[test]
    fn a_frame_that_draws_the_same_picture_is_not_sent() {
        let mut feed = Feed::new();
        let tone = tone();
        // Long enough for the smoothing to stop moving.
        for _ in 0..400 {
            feed.window(&tone);
        }

        // The rate limit is about time, not about content: lift it, so what
        // is left is only whether the frame differs. The first of these may
        // or may not be sent, depending on how long the loop above took.
        feed.last_sent = None;
        feed.window(&tone);
        feed.last_sent = None;
        assert!(feed.window(&tone).is_none(), "an identical frame was sent again");
    }
}
