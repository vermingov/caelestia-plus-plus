//! Which shell draws what, while there are two.
//!
//! cae takes Quickshell's place a piece at a time, and for each piece there
//! is a moment when both could draw it: the new cae is installed, and the
//! Quickshell that is running was started before it knew to stand down. So
//! cae asks, and draws only on a yes.

const SHELL: &str = "caelestia";

/// What Quickshell said when asked whether it has stood `what` down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// It has, or there is no Quickshell to ask: either way nobody else
    /// will draw it.
    Ours,
    /// It has not. One that was started before it knew the question says
    /// so by not knowing the question.
    Theirs,
}

fn read(asked: std::io::Result<std::process::Output>) -> Answer {
    match asked {
        Ok(said) if said.status.success() => {
            if String::from_utf8_lossy(&said.stdout).trim() == "1" { Answer::Ours } else { Answer::Theirs }
        }
        // `qs` says there is nothing running under that name by failing,
        // and a machine without `qs` has no Quickshell either.
        _ => Answer::Ours,
    }
}

/// Asks the running Quickshell. A process start and a round trip over its
/// socket: never from the thread that draws.
pub fn stood_down(what: &str) -> Answer {
    read(std::process::Command::new("qs").args(["-c", SHELL, "ipc", "call", "cae", "stoodDown", what]).output())
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Output};

    use super::*;

    fn said(code: i32, stdout: &str) -> std::io::Result<Output> {
        Ok(Output { status: ExitStatus::from_raw(code << 8), stdout: stdout.as_bytes().to_vec(), stderr: Vec::new() })
    }

    #[test]
    fn only_a_yes_is_a_yes() {
        assert_eq!(read(said(0, "1\n")), Answer::Ours);
        assert_eq!(read(said(0, "0\n")), Answer::Theirs);
        // What a Quickshell from before the question existed says, with a
        // clean exit: it has stood nothing down.
        assert_eq!(read(said(0, "Target not found.\n")), Answer::Theirs);
        assert_eq!(read(said(0, "Function not found.\n")), Answer::Theirs);
    }

    #[test]
    fn nobody_to_ask_is_nobody_else_to_draw_it() {
        assert_eq!(read(said(255, "")), Answer::Ours);
        assert_eq!(read(Err(std::io::Error::from(std::io::ErrorKind::NotFound))), Answer::Ours);
    }
}
