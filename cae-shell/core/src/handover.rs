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
///
/// Asked only when there is something to ask. Starting `qs` to be told there
/// is no Quickshell costs about 25 ms, and every piece pays it once — which
/// on a desktop that has no Quickshell at all is 25 ms times thirteen, spent
/// finding out what was already true.
pub fn stood_down(what: &str) -> Answer {
    if !any_running() {
        return Answer::Ours;
    }
    read(std::process::Command::new("qs").args(["-c", SHELL, "ipc", "call", "cae", "stoodDown", what]).output())
}

/// Whether any Quickshell is up, by trying the sockets it advertises itself
/// on.
///
/// Every Quickshell that has ever run leaves its directory behind, so the
/// directory existing means nothing; the socket in it is what answers. A
/// dead one refuses at once, which is the whole test and costs no process.
fn any_running() -> bool {
    let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") else { return false };
    let by_id = std::path::PathBuf::from(runtime).join("quickshell/by-id");
    let Ok(instances) = std::fs::read_dir(by_id) else { return false };
    instances.filter_map(Result::ok).any(|instance| std::os::unix::net::UnixStream::connect(instance.path().join("ipc.sock")).is_ok())
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
    fn nothing_advertising_itself_is_nobody_to_ask() {
        // Whatever this machine is running, the two must agree: if no socket
        // answers then `stood_down` must not have gone looking for `qs`.
        if !any_running() {
            assert_eq!(stood_down("anything"), Answer::Ours);
        }
    }

    #[test]
    fn nobody_to_ask_is_nobody_else_to_draw_it() {
        assert_eq!(read(said(255, "")), Answer::Ours);
        assert_eq!(read(Err(std::io::Error::from(std::io::ErrorKind::NotFound))), Answer::Ours);
    }
}
