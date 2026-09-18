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

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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
const ATTACK: f32 = 0.45;
const DECAY: f32 = 0.12;

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
        Analyser { window, edges: band_edges(), levels: [0.0; BARS] }
    }

    fn frame(&mut self, samples: &[f32]) -> [f32; BARS] {
        let mut real: Vec<f32> = samples
            .iter()
            .zip(&self.window)
            .map(|(sample, window)| sample * window)
            .collect();
        let mut imaginary = vec![0.0; WINDOW];
        fft(&mut real, &mut imaginary);

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
            // never leave the floor.
            let tilt = 1.0 + 0.9 * (bar as f32 / BARS as f32);
            let level = (level * tilt).clamp(0.0, 1.0);

            let rate = if level > self.levels[bar] { ATTACK } else { DECAY };
            self.levels[bar] += (level - self.levels[bar]) * rate;
        }
        self.levels
    }
}

/// The node id of whatever the session manager currently calls the default
/// sink. Re-read on every reconnect, so changing outputs picks the new one up.
fn default_sink() -> Option<String> {
    let output = Command::new("wpctl").args(["inspect", "@DEFAULT_AUDIO_SINK@"]).output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    // "id 56, type PipeWire:Interface:Node"
    let first = text.lines().next()?;
    let id = first.strip_prefix("id ")?.split(',').next()?;
    id.parse::<u32>().ok().map(|id| id.to_string())
}

/// Quantises a frame to bytes.
///
/// The levels cross an IPC boundary as JSON thirty times a second, and a
/// float spells out as nine characters where a byte spells out as three. At
/// 16px tall there are not 256 distinguishable heights, let alone 2^24.
fn quantise(bars: [f32; BARS]) -> Vec<u8> {
    bars.iter().map(|level| (level.clamp(0.0, 1.0) * 255.0) as u8).collect()
}

/// Records the default sink's monitor and calls `on_frame` with each frame of
/// bars, until the process ends.
///
/// Blocks; meant for its own thread. Silence is not sent: once the bars have
/// drained to nothing, nothing is emitted until sound returns, so a quiet
/// desktop costs one sleeping process and no repaints at all.
pub fn watch(mut on_frame: impl FnMut(Vec<u8>, bool)) {
    loop {
        // Recording *from a sink* is how PipeWire spells "monitor what is
        // coming out of it". Without a target, pw-record takes the default
        // source instead, and the visualiser draws the microphone.
        let target = default_sink().unwrap_or_else(|| "0".to_string());
        let mut command = Command::new("pw-record");
        command
            .args([
                // Raw, or pw-record writes a container header first and every
                // sample after it is read four bytes out of phase.
                "--raw",
                &format!("--target={target}"),
                "--rate=44100",
                "--channels=1",
                "--format=f32",
                "--latency=20ms",
                "-",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        // Dies with the bar, whatever the bar dies of. Nothing here runs when
        // the bar is terminated, and a recorder left alone only finds out by
        // writing to the closed pipe. One with nothing to write — no samples
        // are reaching it — never finds out, and stays on the sink's monitor:
        // one more for every restart. The signal follows the thread that
        // started the child; this one runs for as long as the bar does.
        //
        // SAFETY: `prctl` is async-signal-safe and touches only the child.
        unsafe {
            command.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }

        let Ok(mut recorder) = command.spawn() else {
            eprintln!("caelestia-bar: pw-record is not available, so no visualiser");
            return;
        };

        let Some(mut stdout) = recorder.stdout.take() else { return };
        let mut analyser = Analyser::new();
        let mut raw = vec![0u8; WINDOW * 4];
        let mut samples = vec![0.0f32; WINDOW];
        let mut quiet_since: Option<Instant> = None;
        let mut draining = true;
        let interval = Duration::from_millis(1000 / MAX_FPS);
        let mut last_sent = Instant::now() - interval;

        while stdout.read_exact(&mut raw).is_ok() {
            for (sample, bytes) in samples.iter_mut().zip(raw.chunks_exact(4)) {
                *sample = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            }

            let peak = samples.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
            let bars = analyser.frame(&samples);
            let settled = bars.iter().all(|level| *level < 0.01);

            if peak < FLOOR && settled {
                // One last frame to put the bars down, then nothing until
                // something plays again.
                if draining {
                    draining = false;
                    on_frame(vec![0; BARS], false);
                }
                quiet_since.get_or_insert_with(Instant::now);
                continue;
            }

            quiet_since = None;
            draining = true;

            // Analysed every window regardless — the smoothing depends on it —
            // but only handed over at the rate anybody can see.
            if last_sent.elapsed() >= interval {
                last_sent = Instant::now();
                on_frame(quantise(bars), true);
            }
        }

        // pw-record exits when the default sink changes underneath it; that
        // is a reconnect, not a failure.
        let _ = recorder.wait();
        std::thread::sleep(Duration::from_millis(500));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pure tone should light the band that contains it and leave the rest
    /// alone. This is the whole contract: if the transform or the banding is
    /// wrong, the bars are decorative noise rather than the music.
    #[test]
    fn a_tone_lands_in_one_band() {
        let mut analyser = Analyser::new();
        let tone: Vec<f32> = (0..WINDOW)
            .map(|i| (std::f32::consts::TAU * 1000.0 * i as f32 / RATE).sin() * 0.5)
            .collect();

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
}
