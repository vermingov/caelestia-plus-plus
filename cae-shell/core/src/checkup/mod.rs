//! The system scan: what the shell needs from the machine, and whether it is
//! there.
//!
//! Binaries, daemons, the privileged halves under `system/`, the package
//! manager's health, the audio stack, the desktop entries. Everything the
//! scan looks at is looked at by one shell script — `assets/systemcheck-
//! probe.sh`, which prints `key|field|field` lines — because a hundred small
//! probes belong in the shell that is good at them. What is here is the
//! reading of those lines and what each one means.
//!
//! Nothing runs by itself. A finding may carry a [`Fix`], which is a
//! description first: its exact commands are shown and only [`run`] executes
//! them, after somebody has said yes. Root work goes through `pkexec`, so the
//! only thing ever typed is a password.

mod dismissed;
mod fixes;
mod rows;

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;

pub use dismissed::Dismissed;
pub use fixes::{Fix, everything, install_all};

use crate::about;

/// How bad a finding is. The order is the order they are shown in: what is
/// broken first, what is merely worth knowing last.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    Fail,
    Warn,
    Info,
    Ok,
}

impl Status {
    pub fn word(self) -> &'static str {
        match self {
            Status::Fail => "FAIL",
            Status::Warn => "WARN",
            Status::Info => "INFO",
            Status::Ok => "OK",
        }
    }

    /// Whether it is something to do rather than something to know.
    pub fn is_problem(self) -> bool {
        matches!(self, Status::Fail | Status::Warn)
    }
}

/// One thing the scan looked at.
#[derive(Clone, Debug)]
pub struct Row {
    pub id: String,
    pub name: String,
    pub detail: String,
    pub status: Status,
    /// Worth interrupting somebody over the first time it is seen: a missing
    /// package, or a privileged half left behind by an update.
    pub prompt: bool,
    pub fix: Option<Fix>,
}

/// What one of the privileged halves under `system/` is at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Version {
    /// What the checkout has.
    pub repo: i64,
    /// What is installed.
    pub installed: i64,
    /// Whether its unit is enabled, which is how "installed at all" is told.
    pub enabled: bool,
}

/// Everything one scan found.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub rows: Vec<Row>,
    /// The privileged halves by directory name, for the startup prompt.
    pub halves: HashMap<String, Version>,
}

impl Report {
    pub fn problems(&self) -> usize {
        self.rows.iter().filter(|row| row.status.is_problem()).count()
    }

    /// The packages the machine is missing, in the order they were found.
    pub fn missing_packages(&self) -> Vec<String> {
        self.rows.iter().filter_map(|row| row.fix.as_ref()?.pkg.clone()).collect()
    }

    /// The privileged halves that are installed but out of date.
    pub fn outdated_halves(&self) -> Vec<String> {
        self.rows
            .iter()
            .filter(|row| row.prompt && row.id.starts_with("roothalf-"))
            .filter_map(|row| row.fix.as_ref()?.dir.clone())
            .collect()
    }

    /// Whether a fix that needs a password could even ask for one.
    pub fn polkit_missing(&self) -> bool {
        self.rows.iter().any(|row| row.id == "polkit-agent" && row.status == Status::Fail)
    }

    pub fn row(&self, id: &str) -> Option<&Row> {
        self.rows.iter().find(|row| row.id == id)
    }
}

/// What the probe script printed, split up but not yet understood.
#[derive(Debug, Default)]
pub struct Probed {
    /// Everything else, by its key.
    fields: HashMap<String, Vec<String>>,
    /// The privileged halves.
    halves: HashMap<String, Version>,
    /// Each config file it could parse, and whether it is valid JSON.
    configs: Vec<(String, bool)>,
}

impl Probed {
    /// One field of a line, or "" where the line or the field is missing.
    fn at(&self, key: &str, index: usize) -> &str {
        self.fields.get(key).and_then(|fields| fields.get(index)).map_or("", String::as_str)
    }

    fn first(&self, key: &str) -> &str {
        self.at(key, 0)
    }

    /// A counted field, which the script prints as a number and sometimes
    /// leaves empty.
    fn count(&self, key: &str, index: usize) -> i64 {
        self.at(key, index).trim().parse().unwrap_or(0)
    }

    /// A field holding a list of paths or names separated by spaces.
    fn words(&self, key: &str, index: usize) -> Vec<String> {
        self.at(key, index).split_whitespace().map(str::to_string).collect()
    }

    fn read(printed: &str) -> Probed {
        let mut probed = Probed::default();
        for line in printed.trim().lines() {
            let mut parts = line.split('|');
            let Some(key) = parts.next() else { continue };
            let fields: Vec<String> = parts.map(str::to_string).collect();
            match key {
                "ver" => {
                    let number = |index: usize| fields.get(index).and_then(|field| field.parse().ok()).unwrap_or(0);
                    if let Some(dir) = fields.first() {
                        probed.halves.insert(
                            dir.clone(),
                            Version { repo: number(1), installed: number(2), enabled: fields.get(3).is_some_and(|field| field == "1") },
                        );
                    }
                }
                "cfgjson" => {
                    if let Some(file) = fields.first() {
                        probed.configs.push((file.clone(), fields.get(1).is_some_and(|field| field == "ok")));
                    }
                }
                // Two keys are indexed by a name of their own rather than
                // printed once, so they are kept under a joined key.
                "bin" | "pkg" => {
                    if let Some(name) = fields.first() {
                        probed.fields.insert(format!("{key}.{name}"), fields[1..].to_vec());
                    }
                }
                _ => {
                    probed.fields.insert(key.to_string(), fields);
                }
            }
        }
        probed
    }
}

/// Where the wallpapers are, which the probe checks exists.
fn walls_dir() -> PathBuf {
    let shell = crate::config::read(crate::config::File::Shell);
    let said = crate::config::lookup(&shell, "paths.wallpaperDir").and_then(serde_json::Value::as_str);
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    match said {
        Some(path) if path.starts_with('~') => home.join(path.trim_start_matches("~/")),
        Some(path) => PathBuf::from(path),
        None => home.join("Pictures/Wallpapers"),
    }
}

/// Runs the whole scan. Blocking, and slow enough — seconds — that it wants
/// a thread of its own.
pub fn scan() -> Report {
    let checkout = about::checkout();
    let script = checkout.join("assets/systemcheck-probe.sh");
    let binaries: Vec<&str> = rows::BINARIES.iter().map(|needed| needed.binary).collect();
    let printed = Command::new("bash")
        .arg(&script)
        .arg(&checkout)
        .arg(walls_dir())
        .arg(binaries.join(" "))
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();

    let probed = Probed::read(&printed);
    let mut report = Report { halves: probed.halves.clone(), rows: rows::all(&probed, &checkout) };
    // Problems first, untouchable notes next, what is well last.
    report.rows.sort_by_key(|row| row.status);
    report
}

/// Runs a staged fix, handing each line of its output to `line` as it
/// arrives. Blocking; returns what it exited with, or -1 when it could not
/// be started at all.
pub fn run(fix: &Fix, started: impl FnOnce(u32), mut line: impl FnMut(String)) -> i32 {
    let Some((program, args)) = fix.exec.split_first() else { return -1 };
    let mut command = Command::new(program);
    command.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(env) = &fix.env {
        command.env("CAELESTIA_FIX", env);
    }
    let Ok(mut running) = command.spawn() else {
        line("could not start the fix".to_string());
        return -1;
    };
    started(running.id());

    // Both streams matter and either may be the one saying what went wrong,
    // so each gets a reader of its own and they are merged in the order the
    // lines arrive.
    let (said, hearing) = mpsc::channel();
    relay(running.stdout.take(), said.clone());
    relay(running.stderr.take(), said);
    for spoken in hearing {
        line(spoken);
    }
    running.wait().ok().and_then(|status| status.code()).unwrap_or(-1)
}

/// Stops a fix that is stuck — nearly always on a password prompt that can
/// never appear, because there is no agent to draw it.
///
/// What is killed is the plain `sh` wrapper, not `pkexec`: `pkexec` is
/// setuid, so nothing here can signal it. A script already past the password
/// finishes root-side, which is safe because they are all short and each
/// step is its own.
pub fn stop(pid: u32) {
    let _ = Command::new("kill").arg(pid.to_string()).status();
}

/// Reads one of a child's streams on a thread, sending it on a line at a
/// time. The sender is dropped with the thread, which is what ends the
/// gathering loop once both streams are done.
fn relay<T: std::io::Read + Send + 'static>(stream: Option<T>, said: mpsc::Sender<String>) {
    let Some(stream) = stream else { return };
    std::thread::spawn(move || {
        for spoken in BufReader::new(stream).lines().map_while(Result::ok) {
            if said.send(spoken).is_err() {
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_probe_line_is_read_by_its_key() {
        let probed = Probed::read("bin|wl-copy|ok\nbin|swappy|missing\ndisk|71\nfailed|2|foo.service bar.service\n");
        assert_eq!(probed.first("bin.wl-copy"), "ok");
        assert_eq!(probed.first("bin.swappy"), "missing");
        assert_eq!(probed.count("disk", 0), 71);
        assert_eq!(probed.words("failed", 1), vec!["foo.service", "bar.service"]);
        assert_eq!(probed.first("nothing-printed-this"), "", "a key the script did not print reads empty");
    }

    #[test]
    fn the_privileged_halves_come_out_with_their_versions() {
        let probed = Probed::read("ver|max-perf|7|5|1\nver|bed-mode|2|0|0\n");
        assert_eq!(probed.halves["max-perf"], Version { repo: 7, installed: 5, enabled: true });
        assert_eq!(probed.halves["bed-mode"], Version { repo: 2, installed: 0, enabled: false });
    }

    #[test]
    fn config_files_are_kept_with_whether_they_parse() {
        let probed = Probed::read("cfgjson|/home/a/shell.json|ok\ncfgjson|/home/a/theme.json|bad\n");
        assert_eq!(probed.configs, vec![("/home/a/shell.json".to_string(), true), ("/home/a/theme.json".to_string(), false)]);
    }

    #[test]
    fn findings_are_ordered_worst_first() {
        let mut statuses = vec![Status::Ok, Status::Info, Status::Fail, Status::Warn];
        statuses.sort();
        assert_eq!(statuses, vec![Status::Fail, Status::Warn, Status::Info, Status::Ok]);
    }

    /// Not part of the run: the scan shells out to the whole machine and
    /// takes the better part of a minute. `cargo test -p cae-core
    /// the_scan_answers -- --ignored --nocapture` is the way to look at what
    /// this machine says.
    #[test]
    #[ignore = "runs the real probe on this machine"]
    fn the_scan_answers_on_this_machine() {
        let report = scan();
        for row in &report.rows {
            println!("{:>4} {:<52} {}", row.status.word(), row.name, row.fix.as_ref().map_or("", |fix| fix.label.as_str()));
        }
        println!("\n{} rows, {} problems", report.rows.len(), report.problems());
        assert!(!report.rows.is_empty());
    }
}
