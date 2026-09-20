//! The default sink and source: how loud, and whether muted.
//!
//! PipeWire keeps this in no file, so every answer costs a `wpctl`. It used
//! to be asked twice a second for the life of the session, whether or not
//! anything had been touched, which made it most of the processes the bar
//! ever started. Now `pactl subscribe` sleeps on PipeWire's own event stream,
//! and the question is only asked when the answer can have changed — which is
//! also the moment it changes, rather than up to a second later.

use std::io::{BufRead, BufReader};
use std::process::{ChildStdout, Command, Stdio};
use std::time::Duration;

use serde::Serialize;

use crate::children;

/// The least time between two readings while the levels keep moving. A held
/// volume key is twenty-five changes a second, and nobody reads the number
/// that fast.
const SETTLE: Duration = Duration::from_millis(100);

/// How often to look when there is no event stream to wait on.
const POLL: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    /// 0–100, which may exceed 100 where the sink allows it.
    pub level: i64,
    pub muted: bool,
}

/// Both ends at once, since one event can move either.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Levels {
    pub volume: Option<Volume>,
    /// The default source, for the microphone icon.
    pub microphone: Option<Volume>,
}

fn read() -> Levels {
    Levels { volume: read_node("@DEFAULT_AUDIO_SINK@"), microphone: read_node("@DEFAULT_AUDIO_SOURCE@") }
}

fn read_node(node: &str) -> Option<Volume> {
    let output = Command::new("wpctl").args(["get-volume", node]).output().ok()?;
    parse(&String::from_utf8_lossy(&output.stdout))
}

/// "Volume: 0.42" or "Volume: 0.42 [MUTED]".
fn parse(reply: &str) -> Option<Volume> {
    let level = reply.split_whitespace().nth(1)?.parse::<f64>().ok()?;
    Some(Volume { level: (level * 100.0).round() as i64, muted: reply.contains("MUTED") })
}

/// Whether a line of `pactl subscribe` can have moved either level.
///
/// "Event 'change' on sink #56". Sinks and sources carry the levels, and the
/// server is where the choice of default lives. The rest is noise — above all
/// `client`, which is how every `wpctl` run from here shows up: an answer
/// that counted as a question would never stop asking.
fn concerns_levels(event: &str) -> bool {
    matches!(event.split_whitespace().nth(3), Some("sink" | "source" | "server"))
}

fn subscription() -> Command {
    let mut command = Command::new("pactl");
    command
        .arg("subscribe")
        // The event lines are translated, and they are parsed by position.
        .env("LC_ALL", "C")
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    children::bind_to_parent(&mut command);
    command
}

/// What the event stream had to say.
enum Heard {
    /// Something that can have moved a level.
    Levels,
    /// Something else: a client coming or going, a stream starting.
    Noise,
    /// Nothing, in the time allowed.
    Nothing,
    /// `pactl` is gone.
    Closed,
}

struct Events {
    lines: BufReader<ChildStdout>,
    line: String,
}

impl Events {
    fn new(stdout: ChildStdout) -> Events {
        Events { lines: BufReader::new(stdout), line: String::new() }
    }

    /// The next event, waiting up to `patience` for one, or for as long as it
    /// takes when given none.
    fn next(&mut self, patience: Option<Duration>) -> Heard {
        // Lines that arrived together are already in the buffer, and the pipe
        // will never turn readable on their account.
        if self.lines.buffer().is_empty() && !children::readable(self.lines.get_ref(), patience) {
            return Heard::Nothing;
        }
        self.line.clear();
        match self.lines.read_line(&mut self.line) {
            Ok(0) | Err(_) => Heard::Closed,
            Ok(_) if concerns_levels(&self.line) => Heard::Levels,
            Ok(_) => Heard::Noise,
        }
    }

    /// Reads through everything already waiting, and says whether any of it
    /// can have moved a level.
    fn moved_meanwhile(&mut self) -> bool {
        let mut moved = false;
        loop {
            match self.next(Some(Duration::ZERO)) {
                Heard::Levels => moved = true,
                Heard::Noise => {}
                // A stream that has ended says so again to whoever asks
                // next, which leaves what came before the end to be answered.
                Heard::Nothing | Heard::Closed => return moved,
            }
        }
    }
}

/// Calls `on_move` each time the event stream says a level may have moved,
/// until the stream ends.
///
/// The first event of a burst is answered at once, so one press of a volume
/// key shows without delay. After that it is once per `SETTLE` for as long as
/// the events keep coming, and whatever arrived in between is answered at
/// the end of it, so the last change of a burst is never the one dropped.
fn follow(stdout: ChildStdout, mut on_move: impl FnMut()) {
    let mut events = Events::new(stdout);
    loop {
        match events.next(None) {
            Heard::Levels => {}
            Heard::Noise | Heard::Nothing => continue,
            Heard::Closed => return,
        }
        loop {
            on_move();
            std::thread::sleep(SETTLE);
            if !events.moved_meanwhile() {
                break;
            }
        }
    }
}

/// Calls `on_change` with the levels as they stand, and again each time
/// either changes, until the process ends.
///
/// Blocks; meant for its own thread.
pub fn watch(mut on_change: impl FnMut(Levels)) {
    let mut known = None;
    let mut look = move || {
        let levels = read();
        if known.as_ref() != Some(&levels) {
            known = Some(levels.clone());
            on_change(levels);
        }
    };

    loop {
        // Started before the first look rather than after, so the
        // subscription is coming up while the look is being taken and little
        // can move unseen between the two.
        let pactl = subscription().spawn();
        look();
        if let Ok(mut pactl) = pactl {
            if let Some(stdout) = pactl.stdout.take() {
                follow(stdout, &mut look);
            }
            let _ = pactl.wait();
        }
        // Without `pactl`, or without a PipeWire for it to reach, this is
        // the loop it replaced: a look a second, until one of them turns up.
        std::thread::sleep(POLL);
    }
}

/// Moves the volume by `delta` percent, with the headroom above 100 that the
/// wheel is allowed and the slider is not.
pub fn nudge(delta: i64) {
    let sign = if delta >= 0 { "+" } else { "-" };
    let _ = Command::new("wpctl")
        .args(["set-volume", "-l", "1.5", "@DEFAULT_AUDIO_SINK@", &format!("{}%{sign}", delta.abs())])
        .status();
}

/// Sets the volume outright, for the popout's slider. Capped at 100: the
/// headroom above it is for the wheel, where asking for it is deliberate.
pub fn set(level: i64) {
    let _ = Command::new("wpctl")
        .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{}%", level.clamp(0, 100))])
        .status();
}

pub fn toggle_mute() {
    let _ = Command::new("wpctl").args(["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"]).status();
}

pub fn toggle_microphone() {
    let _ = Command::new("wpctl").args(["set-mute", "@DEFAULT_AUDIO_SOURCE@", "toggle"]).status();
}

pub fn set_microphone(level: i64) {
    let _ = Command::new("wpctl")
        .args(["set-volume", "@DEFAULT_AUDIO_SOURCE@", &format!("{}%", level.clamp(0, 100))])
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for `pactl`: a shell that says what it is told to.
    fn scripted(script: &str) -> ChildStdout {
        Command::new("sh")
            .args(["-c", script])
            .stdout(Stdio::piped())
            .spawn()
            .expect("sh")
            .stdout
            .take()
            .expect("a piped stdout")
    }

    #[test]
    fn only_sinks_sources_and_the_server_concern_the_levels() {
        assert!(concerns_levels("Event 'change' on sink #56\n"));
        assert!(concerns_levels("Event 'change' on source #57\n"));
        assert!(concerns_levels("Event 'change' on server #4294967295\n"));

        // Every `wpctl` this module runs shows up as one of these.
        assert!(!concerns_levels("Event 'new' on client #317825\n"));
        // A stream starting is not the sink changing, whatever it is called.
        assert!(!concerns_levels("Event 'new' on sink-input #112\n"));
        assert!(!concerns_levels("Event 'change' on card #52\n"));
        assert!(!concerns_levels("\n"));
    }

    #[test]
    fn a_reply_is_a_level_and_whether_it_is_muted() {
        assert_eq!(parse("Volume: 0.42\n"), Some(Volume { level: 42, muted: false }));
        assert_eq!(parse("Volume: 1.25 [MUTED]\n"), Some(Volume { level: 125, muted: true }));
        // What `wpctl` prints with no PipeWire to ask goes to stderr.
        assert_eq!(parse(""), None);
    }

    #[test]
    fn noise_is_never_answered() {
        let mut looks = 0;
        follow(
            scripted("echo \"Event 'new' on client #1\"; echo \"Event 'new' on sink-input #2\""),
            || looks += 1,
        );
        assert_eq!(looks, 0);
    }

    #[test]
    fn a_burst_is_answered_at_once_and_once_more_at_the_end() {
        let mut looks = 0;
        follow(
            scripted("for change in 1 2 3 4 5 6; do echo \"Event 'change' on sink #56\"; done"),
            || looks += 1,
        );
        assert_eq!(looks, 2, "one look for the first event, one for the five behind it");
    }

    #[test]
    fn changes_further_apart_than_the_settle_are_each_answered() {
        let mut looks = 0;
        follow(
            scripted(
                "echo \"Event 'change' on sink #56\"; sleep 0.3; echo \"Event 'change' on source #57\"",
            ),
            || looks += 1,
        );
        assert_eq!(looks, 2);
    }
}
