//! What each thing the probe looked at means.
//!
//! One function per group, in the order they are read: what the shell needs
//! to run, what Caelestia++ itself is at, the compositor, then the machine.
//! Each returns findings; `all` puts them together.

use std::path::Path;

use super::fixes::{self, Fix};
use super::{Probed, Row, Status};
use crate::{features, updates};

/// A program the shell shells out to, the package that brings it, and what
/// stops working without it.
pub struct Needed {
    pub binary: &'static str,
    pub package: &'static str,
    pub why: &'static str,
    pub severity: Status,
    /// Nothing a desktop has a use for.
    pub laptop_only: bool,
}

const fn needed(binary: &'static str, package: &'static str, why: &'static str, severity: Status) -> Needed {
    Needed { binary, package, why, severity, laptop_only: false }
}

pub const BINARIES: [Needed; 12] = [
    needed("wl-copy", "wl-clipboard", "Clipboard: screenshots, colour picker, console copy", Status::Fail),
    needed("nmcli", "networkmanager", "Network status and wifi connections in the bar", Status::Fail),
    needed("python3", "python", "Shell helper scripts", Status::Fail),
    needed("notify-send", "libnotify", "Desktop notifications from shell actions", Status::Warn),
    needed("powerprofilesctl", "power-profiles-daemon", "Power profile switching (battery popout, Dynamic, Maximum performance)", Status::Warn),
    needed("ddcutil", "ddcutil", "Brightness control for external monitors", Status::Warn),
    Needed { laptop_only: true, ..needed("brightnessctl", "brightnessctl", "Brightness control for the internal display", Status::Warn) },
    needed("gpu-screen-recorder", "gpu-screen-recorder", "Screen recording from the utilities drawer", Status::Warn),
    needed("swappy", "swappy", "Screenshot annotation", Status::Warn),
    needed("xmllint", "libxml2", "Weather and metadata parsing", Status::Warn),
    needed("bwrap", "bubblewrap", "sandrunner: the fake-root sandbox itself", Status::Warn),
    needed("fuse-overlayfs", "fuse-overlayfs", "sandrunner: writable throwaway system view (read-only fallback without it)", Status::Warn),
];

/// The privileged halves under `system/`, and how each is allowed to be
/// missing.
struct Half {
    dir: &'static str,
    name: &'static str,
    /// Only ever flagged for being out of date, never for being absent:
    /// something installed from its own page, or from a machine that has no
    /// use for it.
    upgrade_only: bool,
}

/// Anything that is the shell asking a question of itself, so the words stay
/// in one place.
const OURS: &str = "Caelestia++";

fn say(id: &str, name: impl Into<String>, detail: impl Into<String>, status: Status) -> Row {
    Row { id: id.to_string(), name: name.into(), detail: detail.into(), status, prompt: false, fix: None }
}

impl Row {
    fn fixed_by(mut self, fix: Fix) -> Row {
        self.fix = Some(fix);
        self
    }

    /// Worth saying unasked the first time it is found.
    fn worth_interrupting(mut self) -> Row {
        self.prompt = true;
        self
    }
}

/// Every finding, in the order the groups are written below.
pub fn all(probed: &Probed, checkout: &Path) -> Vec<Row> {
    let mut rows = Vec::new();
    let laptop = features::is_laptop();
    rows.extend(dependencies(probed, checkout, laptop));
    rows.extend(quickshell(probed, checkout));
    rows.extend(caelestia(probed, checkout));
    rows.extend(compositor(probed));
    rows.extend(packages(probed));
    rows.extend(health(probed, checkout, laptop));
    rows.extend(entries(probed));
    rows
}

/// A command run from the checkout, quoted for `sh`.
fn from_checkout(checkout: &Path, script: &str) -> String {
    format!("bash '{}/{script}'", checkout.display())
}

// -- What the shell needs to run -------------------------------------------

fn dependencies(probed: &Probed, checkout: &Path, laptop: bool) -> Vec<Row> {
    let mut rows = Vec::new();
    for want in BINARIES.iter().filter(|want| !want.laptop_only || laptop) {
        let there = probed.first(&format!("bin.{}", want.binary)) == "ok";
        let name = if there { format!("{} installed", want.package) } else { format!("{} missing", want.package) };
        let row = say(&format!("bin-{}", want.binary), name, want.why, if there { Status::Ok } else { want.severity });
        rows.push(if there { row } else { row.fixed_by(fixes::install(want.package)).worth_interrupting() });
    }

    let conflicts = probed.first("pmconflict").trim().to_string();
    if probed.first("bin.powerprofilesctl") == "ok" {
        rows.push(power_daemon(probed, checkout, &conflicts));
    }
    if !conflicts.is_empty() {
        rows.push(say(
            "pm-conflict",
            "Conflicting power manager installed",
            format!(
                "{conflicts} fights power-profiles-daemon over the same hardware knobs — the daemon often gets masked or fails to start (dbus NoReply). Keep one: either remove {conflicts}, or remove power-profiles-daemon. Your call, so no automatic fix."
            ),
            Status::Warn,
        ));
    }
    rows
}

fn power_daemon(probed: &Probed, checkout: &Path, conflicts: &str) -> Row {
    let active = probed.first("ppd") == "active";
    if active {
        return say("ppd-service", "power-profiles-daemon running", "The daemon behind power profile switching", Status::Ok);
    }
    let enabled = probed.at("ppdunit", 0);
    let state = format!("unit state: {} / {}", if enabled.is_empty() { "?" } else { enabled }, {
        let active = probed.at("ppdunit", 1);
        if active.is_empty() { "?" } else { active }
    });
    let missing = ["not-found", "No such file", "could not be found"].iter().any(|said| enabled.contains(said));
    let cause = if missing {
        "service unit not found — the powerprofilesctl tool is present but the daemon package half is not; dbus activation times out with NoReply".to_string()
    } else if enabled.contains("masked") {
        "unit is masked, usually by another power tool — dbus activation times out with NoReply".to_string()
    } else if !conflicts.is_empty() {
        format!("likely blocked by {conflicts} (see the conflict finding)")
    } else {
        "the daemon behind power profile switching is not active".to_string()
    };
    say("ppd-service", "power-profiles-daemon not running", format!("{cause} — {state}"), Status::Warn).fixed_by(fixes::root(
        "Repair",
        "Runs a staged repair that diagnoses as it goes and only applies what the diagnosis calls for: installs or reinstalls the package if it or its unit is missing, unmasks the unit, stops and disables conflicting power daemons it finds (tlp, tuned, tuned-ppd, auto-cpufreq, laptop-mode — they cause exactly this failure), then starts the service and verifies it answers on D-Bus. On failure it prints the daemon's own journal.",
        vec![from_checkout(checkout, "system/repair/power-profiles.sh")],
    ))
}

// -- Quickshell, while there still is one ----------------------------------

fn quickshell(probed: &Probed, checkout: &Path) -> Vec<Row> {
    let mut rows = Vec::new();
    let starts = probed.at("qsbin", 0) == "ok";
    let why = probed.at("qsbin", 1);
    rows.push(if starts {
        say("qs-binary", "quickshell starts", "The installed qs binary loads against the current Qt", Status::Ok)
    } else {
        say(
            "qs-binary",
            "quickshell cannot start",
            format!(
                "The next login comes up without the QML shell: a Qt update moved private symbols the prebuilt quickshell links against ({}). Rebuilding it against the installed Qt fixes it for good.",
                if why.is_empty() { "undefined symbol" } else { why }
            ),
            Status::Fail,
        )
        .worth_interrupting()
        .fixed_by(fixes::root(
            "Rebuild",
            "Runs system/quickshell/install.sh as root: installs the build dependencies with pacman, builds quickshell from source as your user (a few minutes — output streams here), installs it pinned via IgnorePkg, and files the shell as a foreground process for ananicy.",
            vec![format!("{} --force", from_checkout(checkout, "system/quickshell/install.sh"))],
        ))
    });

    let niceness = probed.count("qsnice", 0);
    if niceness > 0 {
        let ananicy = probed.at("qsnice", 1) == "active";
        let detail = if ananicy {
            "ananicy-cpp files the shell under its Service class, so the bar and panels get starved whenever something heavy runs. A rule override puts it back at foreground priority."
        } else {
            "Something started the shell with a lowered priority, so the bar and panels get starved whenever something heavy runs."
        };
        let row = say("qs-nice", format!("Shell runs at background priority (nice {niceness})"), detail, Status::Warn);
        rows.push(if ananicy {
            row.fixed_by(fixes::root(
                "Fix",
                "Installs /etc/ananicy.d/zz-caelestia/quickshell.rules (foreground priority) and reloads ananicy-cpp. The rule applies from the shell's next start.",
                vec![from_checkout(checkout, "system/quickshell/install.sh")],
            ))
        } else {
            row
        });
    }
    rows
}

// -- Caelestia++ itself ----------------------------------------------------

fn caelestia(probed: &Probed, checkout: &Path) -> Vec<Row> {
    let mut rows = vec![match updates::check() {
        Ok(found) if found.behind > 0 => {
            say("shell-updates", format!("{} is out", found.release), format!("{OURS} is {} commits behind it", found.behind), Status::Warn)
                .fixed_by(fixes::update_the_shell())
        }
        Ok(_) => say("shell-updates", "Shell up to date", "Checked against the newest published release", Status::Ok),
        Err(why) => say("shell-updates", "Could not check for updates", why, Status::Info),
    }];

    let halves = [
        Half { dir: "max-perf", name: "max-perf", upgrade_only: false },
        Half { dir: "anti-heat", name: "anti-heat", upgrade_only: false },
        Half { dir: "dynamic", name: "dynamic performance", upgrade_only: features::is_laptop() },
        Half { dir: "bed-mode", name: "bed mode", upgrade_only: true },
        // These two install from their own pages, so a missing one is a
        // choice rather than a fault; only a version behind is worth saying.
        Half { dir: "redguard", name: "protection", upgrade_only: true },
        Half { dir: "redwall", name: "firewall", upgrade_only: true },
    ];
    for half in halves {
        let at = probed.halves.get(half.dir).copied().unwrap_or_default();
        let id = format!("roothalf-{}", half.dir);
        let install = |label: &str| {
            fixes::root(
                label,
                &format!(
                    "Runs system/{}/install.sh from the checkout as root: copies its scripts to /usr/local/bin, its units to /etc/systemd/system, and enables the watcher unit. Overwrites previous versions of the same files only.",
                    half.dir
                ),
                vec![from_checkout(checkout, &format!("system/{}/install.sh", half.dir))],
            )
            .of_half(half.dir)
        };
        if at.enabled && at.repo > at.installed {
            rows.push(
                say(
                    &id,
                    format!("{} root half outdated (v{}, current v{})", half.name, at.installed, at.repo),
                    format!("The installed privileged half is from an older {OURS} — update to get the latest behaviour"),
                    Status::Warn,
                )
                .worth_interrupting()
                .fixed_by(install("Update")),
            );
        } else if !at.enabled && !half.upgrade_only {
            rows.push(
                say(
                    &id,
                    format!("{} root half missing", half.name),
                    format!("The {} feature stays inactive until its privileged half is installed", half.name),
                    Status::Warn,
                )
                .fixed_by(install("Install")),
            );
        } else if at.enabled {
            rows.push(say(&id, format!("{} root half installed and current", half.name), "Feature fully available", Status::Ok));
        }
    }

    rows.push(sandrunner(probed, checkout));
    for (file, valid) in &probed.configs {
        let name = Path::new(file).file_name().map_or(file.clone(), |name| name.to_string_lossy().into_owned());
        let id = format!("cfg-{file}");
        rows.push(if *valid {
            say(&id, format!("Config valid: {name}"), file.clone(), Status::Ok)
        } else {
            say(&id, format!("Config is not valid JSON: {name}"), format!("{file} — the shell falls back to defaults while this file is broken"), Status::Fail)
                .fixed_by(fixes::user(
                    "Reset",
                    "Moves the broken file aside (a .broken.bak copy stays next to it, nothing is deleted); the shell then regenerates defaults.",
                    vec![format!("mv '{file}' '{file}.broken.bak'")],
                ))
        });
    }

    let dirty = probed.count("gitdirty", 0);
    rows.push(if dirty > 0 {
        say("git-dirty", format!("Shell checkout has {dirty} modified files"), "Local edits are fine, but they can conflict with updates — no automatic action", Status::Info)
    } else {
        say("git-dirty", "Shell checkout clean", "No local modifications", Status::Ok)
    });

    let pinned = probed.first("ignpkg") == "ok";
    rows.push(if pinned {
        say("ignorepkg", "Pacman IgnorePkg set", "Repo packages won't clobber the git checkout", Status::Ok)
    } else {
        say("ignorepkg", "Pacman IgnorePkg not set", "A caelestia++ repo package could overwrite this checkout on -Syu", Status::Warn).fixed_by(fixes::root(
            "Fix",
            "Appends one IgnorePkg line to /etc/pacman.conf so system updates skip the caelestia++ packages. No other line is touched.",
            vec!["grep -q 'IgnorePkg.*caelestia' /etc/pacman.conf || printf 'IgnorePkg = caelestia++-shell caelestia++-cli\\n' >> /etc/pacman.conf".to_string()],
        ))
    });

    rows.push(match probed.first("cli") {
        "broken" => say("caelestia-cli", "caelestia CLI broken", "`caelestia --version` fails — schemes, wallpapers and recording die with it; reinstall the caelestia++-cli package", Status::Fail),
        _ => say("caelestia-cli", "caelestia CLI works", "Scheme, wallpaper and recorder plumbing available", Status::Ok),
    });

    if probed.first("walldir") == "missing" {
        let dir = probed.at("walldir", 1).to_string();
        rows.push(
            say("wallpaper-dir", "Wallpaper directory missing", format!("{dir} does not exist — the wallpaper picker has nothing to show"), Status::Info)
                .fixed_by(fixes::user("Create", "Creates the empty wallpaper directory the config points at. Nothing else changes.", vec![format!("mkdir -p '{dir}'")])),
        );
    }
    rows
}

/// sandrunner has no privileged half: being installed is a `~/.local/bin`
/// symlink into the checkout. An update delivered by `git pull` alone never
/// runs an installer, so a missing link is simply made again here rather
/// than reported — it is ours, it is idempotent, and it points at a file
/// that is already there.
fn sandrunner(probed: &Probed, checkout: &Path) -> Row {
    match (probed.at("sandrunner", 0), probed.at("sandrunner", 1)) {
        ("ok", "missing") => say(
            "sandrunner",
            "sandrunner on PATH after next login",
            "~/.local/bin was added to your login shell's profile automatically — open a new terminal (or log in again) and `sandrunner` works",
            Status::Info,
        ),
        ("ok", _) => say("sandrunner", "sandrunner installed", "Full-simulation sandbox available as `sandrunner FILE`", Status::Ok),
        _ => {
            let link = format!(
                "mkdir -p \"$HOME/.local/bin\" && ln -sf '{}/system/sandrunner/sandrunner' \"$HOME/.local/bin/sandrunner\"",
                checkout.display()
            );
            let _ = std::process::Command::new("sh").arg("-c").arg(link).status();
            say("sandrunner", "sandrunner PATH link restored", "The ~/.local/bin symlink was missing and has been recreated", Status::Info)
        }
    }
}

// -- The compositor and what sits beside it --------------------------------

fn compositor(probed: &Probed) -> Vec<Row> {
    let mut rows = Vec::new();
    let errors = probed.count("hyprerr", 0);
    rows.push(if errors > 0 {
        say(
            "hypr-config",
            format!("Hyprland config has {errors} errors"),
            format!("First: {} — full list via `hyprctl configerrors`; needs a manual edit", probed.at("hyprerr", 1)),
            Status::Fail,
        )
    } else {
        say("hypr-config", "Hyprland config parses clean", "hyprctl configerrors reports none", Status::Ok)
    });

    for package in ["xdg-desktop-portal-hyprland", "qt6-wayland"] {
        let there = probed.first(&format!("pkg.{package}")) == "ok";
        let why = if package == "qt6-wayland" {
            "Qt apps need it to run natively on Wayland"
        } else {
            "Screen sharing and file pickers break without the Hyprland portal"
        };
        let name = if there { format!("{package} installed") } else { format!("{package} missing") };
        let row = say(&format!("pkg-{package}"), name, why, if there { Status::Ok } else { Status::Fail });
        rows.push(if there { row } else { row.worth_interrupting().fixed_by(fixes::install(package)) });
    }

    let extra = probed.first("portals").trim().to_string();
    rows.push(if extra.is_empty() {
        say("portals-extra", "No conflicting desktop portals", "Only the Hyprland (and gtk) portals are present", Status::Ok)
    } else {
        say(
            "portals-extra",
            "Extra desktop portals installed",
            format!("{extra} — apps can pick the wrong portal and hang on start; remove them manually if you see slow app launches"),
            Status::Info,
        )
    });

    let agent = probed.first("polkitagent") == "ok";
    rows.push(if agent {
        say("polkit-agent", "Polkit agent running", "Password prompts for privileged actions work", Status::Ok)
    } else {
        say(
            "polkit-agent",
            "No polkit authentication agent running",
            "Without one, no password dialog can appear — including these quick fixes. Install one and start it with the session.",
            Status::Fail,
        )
        .fixed_by(fixes::pacman(
            "Install",
            "Installs the hyprpolkitagent package. You still need to start it with your session — that part is not automated.",
            vec!["pacman -S --needed --noconfirm hyprpolkitagent".to_string()],
        ))
    });

    let portal_up = probed.first("portalsvc") == "active";
    rows.push(if portal_up {
        say("portal-service", "Desktop portal service running", "Screen sharing and file pickers are wired up", Status::Ok)
    } else {
        say("portal-service", "Desktop portal service not running", "Flatpaks, screen sharing and file pickers break without it", Status::Warn)
    });

    let slow = probed.count("refresh", 0);
    if slow > 0 {
        rows.push(say(
            "monitor-refresh",
            format!("{slow} monitors run below their best refresh rate"),
            format!("{} — set the higher rate in your Hyprland monitor config; free smoothness", probed.at("refresh", 1)),
            Status::Info,
        ));
    }
    rows
}

// -- Packages --------------------------------------------------------------

fn packages(probed: &Probed) -> Vec<Row> {
    let mut rows = Vec::new();
    let stale_lock = probed.first("paclock") == "stale";
    rows.push(if stale_lock {
        say("pacman-lock", "Stale pacman database lock", "db.lck exists but no pacman process is running — every install will fail until it is removed", Status::Fail)
            .fixed_by(fixes::root(
                "Remove",
                "Deletes /var/lib/pacman/db.lck. Safe only because the scan verified no pacman process is running right now.",
                vec!["rm /var/lib/pacman/db.lck".to_string()],
            ))
    } else {
        say("pacman-lock", "Pacman database unlocked", "No leftover db.lck", Status::Ok)
    });

    let corrupt = probed.count("corrupt", 0);
    rows.push(if corrupt > 0 {
        say(
            "corrupt-pkgs",
            format!("{corrupt} packages have missing files"),
            format!("{} — files these packages installed are gone from disk; reinstalling restores them", probed.at("corrupt", 1)),
            Status::Warn,
        )
        .fixed_by(fixes::pacman(
            "Reinstall",
            "Reinstalls the affected packages with pacman, restoring their missing files. Configs in /etc marked as backup files are preserved by pacman.",
            vec!["p=$(LC_ALL=C pacman -Qk 2>&1 >/dev/null | awk -F': ' '/No such file or directory/ {print $2}' | sort -u); [ -n \"$p\" ] && pacman -S --noconfirm $p || echo 'nothing missing anymore'".to_string()],
        ))
    } else {
        say("corrupt-pkgs", "All package files present", "pacman -Qk finds nothing missing", Status::Ok)
    });

    let orphans = probed.count("orphans", 0);
    rows.push(if orphans > 0 {
        let kept = BINARIES.iter().map(|want| want.package).collect::<Vec<_>>().join(" ");
        say(
            "orphans",
            format!("{orphans} orphaned packages"),
            format!(
                "Installed as dependencies, no longer needed by anything: {}{}",
                probed.at("orphans", 1),
                if orphans > 10 { "…" } else { "" }
            ),
            Status::Info,
        )
        .fixed_by(fixes::pacman(
            "Remove",
            "First marks everything the shell itself needs as explicitly installed (so it can never be swept), then removes the remaining orphans and their unneeded dependencies.",
            vec![
                format!("pacman -D --asexplicit {kept} qt6-wayland xdg-desktop-portal-hyprland hyprpolkitagent pacman-contrib >/dev/null 2>&1 || true"),
                "o=$(pacman -Qtdq); [ -n \"$o\" ] && pacman -Rns --noconfirm $o || echo 'nothing left to remove'".to_string(),
            ],
        ))
    } else {
        say("orphans", "No orphaned packages", "pacman -Qtdq is empty", Status::Ok)
    });

    let cache = probed.count("paccache", 0);
    if cache >= 8 {
        let cleaner = probed.at("paccache", 1) == "1";
        let row = say(
            "pac-cache",
            format!("Package cache is {cache} GiB"),
            if cleaner {
                "Old package versions pile up in /var/cache/pacman/pkg".to_string()
            } else {
                "Old package versions pile up in /var/cache/pacman/pkg — install pacman-contrib for the paccache cleaner".to_string()
            },
            Status::Info,
        );
        rows.push(if cleaner {
            row.fixed_by(fixes::pacman(
                "Clean",
                "Deletes cached package files except the two most recent versions of each package. Installed software is not affected.",
                vec!["command -v paccache >/dev/null || pacman -S --needed --noconfirm pacman-contrib".to_string(), "paccache -rk2".to_string()],
            ))
        } else {
            row
        });
    }

    let on_cachy = probed.first("osid") == "cachyos";
    let foreign = probed.count("foreignrepo", 0);
    if foreign > 0 {
        let switch = "pacman -S --noconfirm $(pacman -Qmq | grep -v '^caelestia++' | grep -v -- '-debug$' | while read -r p; do pacman -Si \"$p\" >/dev/null 2>&1 && printf '%s ' \"$p\"; done)";
        rows.push(
            say(
                "foreign-repo",
                if on_cachy {
                    format!("{foreign} AUR packages have CachyOS repo builds")
                } else {
                    format!("{foreign} foreign packages have repo builds")
                },
                format!(
                    "{}— the repo versions update with the system{}",
                    probed.at("foreignrepo", 1),
                    if on_cachy { " and CachyOS ships them compiler-optimized (v3/znver) — free speedup over the local AUR builds" } else { "" }
                ),
                Status::Info,
            )
            .fixed_by(fixes::pacman(
                "Switch",
                "Replaces each locally-built AUR package with the repo build of the same name. Versions may differ slightly; the packages themselves stay installed. caelestia++ and -debug packages are excluded.",
                vec![switch.to_string()],
            )),
        );
    }

    let kernel = probed.first("kernel").to_string();
    if on_cachy && !kernel.contains("cachyos") {
        rows.push(say(
            "cachy-kernel",
            "Not running a CachyOS kernel",
            format!("Running {kernel} — the linux-cachyos kernel carries the scheduler and compiler tuning this distro is about. Install it and reboot into it (boot entries update automatically)"),
            Status::Info,
        ));
    }
    if on_cachy && probed.count("cachyrepos", 0) == 0 {
        rows.push(say(
            "cachy-repos",
            "CachyOS repositories missing from pacman.conf",
            "All packages come from plain Arch repos — no optimized builds at all. Re-add them with the cachyos-repo script from the CachyOS wiki",
            Status::Warn,
        ));
    }

    if probed.first("dbage") == "stale" {
        rows.push(
            say(
                "pacman-db",
                "Package databases older than two weeks",
                "Sync databases have not been refreshed in 14+ days — installs pull outdated versions and fixes here may target stale packages",
                Status::Info,
            )
            .fixed_by(fixes::pacman(
                "Update system",
                "Runs a full system upgrade. This updates every package, can take a while, and is the only safe way to refresh the databases (a plain -Sy risks a partial upgrade). Review the system afterwards.",
                vec!["pacman -Syu --noconfirm".to_string()],
            )),
        );
    }

    let pacnew = probed.count("pacnew", 0);
    if pacnew > 0 {
        rows.push(say(
            "pacnew",
            format!("{pacnew} unmerged .pacnew/.pacsave files"),
            format!("{}— package updates shipped new default configs you have not merged; run `pacdiff` in a terminal to review them. Merging is judgment work, so no automatic fix", probed.at("pacnew", 1)),
            Status::Warn,
        ));
    }
    rows
}

// -- The machine -----------------------------------------------------------

fn health(probed: &Probed, checkout: &Path, laptop: bool) -> Vec<Row> {
    let mut rows = Vec::new();
    let (system, user) = (probed.count("failed", 0), probed.count("userfailed", 0));
    rows.push(if system + user > 0 {
        let names = format!("{} {}", probed.at("failed", 1), probed.at("userfailed", 1)).trim().to_string();
        let mut commands = Vec::new();
        if user > 0 {
            commands.push("systemctl --user reset-failed".to_string());
        }
        if system > 0 {
            commands.push("pkexec systemctl reset-failed".to_string());
        }
        say(
            "failed-units",
            format!("{} systemd units failed", system + user),
            format!("{names} — check them with `systemctl status <unit>`; the quick fix only clears the failed markers, it does not repair the services"),
            Status::Warn,
        )
        .fixed_by(fixes::user(
            "Clear",
            "Clears systemd's failed-unit markers (user units directly, system units through pkexec). Purely cosmetic: the services themselves are not repaired and will show up again if they fail again.",
            commands,
        ))
    } else {
        say("failed-units", "No failed systemd units", "System and user managers are clean", Status::Ok)
    });

    let full = probed.count("disk", 0);
    rows.push(say(
        "disk-root",
        if full >= 90 { format!("Root filesystem {full}% full") } else { format!("Root filesystem at {full}%") },
        if full >= 90 { "Things start failing in odd ways when / fills up — free some space" } else { "Plenty of room" },
        if full >= 95 {
            Status::Fail
        } else if full >= 90 {
            Status::Warn
        } else {
            Status::Ok
        },
    ));

    let audio = probed.first("pipewire") == "active";
    rows.push(if audio {
        say("pipewire", "PipeWire running", "Audio stack is up", Status::Ok)
    } else {
        say("pipewire", "PipeWire not running", "No audio and no visualiser without it", Status::Fail).fixed_by(fixes::user(
            "Start",
            "Staged: checks each audio unit exists, unmasks if needed, then enables and starts pipewire, pipewire-pulse and wireplumber.",
            vec![format!("{} --user pipewire.service pipewire-pulse.service wireplumber.service", from_checkout(checkout, "system/repair/service.sh"))],
        ))
    });

    let duplicates = probed.first("dupsinks").trim().to_string();
    rows.push(if duplicates.is_empty() {
        say("dup-sinks", "No duplicate audio sinks", "Each output device appears once", Status::Ok)
    } else {
        say(
            "dup-sinks",
            "Duplicate audio sinks",
            format!("{duplicates} — the same device shows up twice (usually a stale ALSA/PipeWire profile); restarting the audio stack rebuilds the device list"),
            Status::Warn,
        )
        .fixed_by(fixes::user(
            "Restart audio",
            "Restarts pipewire, pipewire-pulse and wireplumber, one at a time with a check after each. Audio cuts out for a second or two; apps reconnect by themselves.",
            vec![format!("{} --user --restart pipewire.service pipewire-pulse.service wireplumber.service", from_checkout(checkout, "system/repair/service.sh"))],
        ))
    });

    let broken_locale = probed.first("locale") == "bad";
    rows.push(say(
        "locale",
        if broken_locale { "Broken locale configuration" } else { "Locale configuration valid" },
        if broken_locale {
            "`locale` prints errors — apps misbehave with unset locales. Uncomment your locale in /etc/locale.gen, run locale-gen, and set LANG in /etc/locale.conf — machine-specific, so no automatic fix"
        } else {
            "locale reports no errors"
        },
        if broken_locale { Status::Warn } else { Status::Ok },
    ));

    let synced = probed.first("ntp") != "no";
    rows.push(if synced {
        say("ntp", "Clock synchronisation on", "systemd-timesyncd keeps the clock right", Status::Ok)
    } else {
        say("ntp", "Clock synchronisation off", "A drifting clock breaks TLS and package signatures eventually", Status::Info).fixed_by(fixes::root(
            "Enable",
            "Runs timedatectl set-ntp true, turning on systemd's time synchronisation.",
            vec!["timedatectl set-ntp true".to_string()],
        ))
    });

    let errors = probed.count("journal", 0);
    rows.push(if errors > 0 {
        say(
            "journal-errors",
            format!("{errors} error-level journal entries this boot"),
            "Not all are serious — read them with `journalctl -b -p err`",
            Status::Info,
        )
    } else {
        say("journal-errors", "No error-level journal entries this boot", "journalctl -b -p err is empty", Status::Ok)
    });

    let dumps = probed.count("coredumps", 0);
    if dumps > 0 {
        rows.push(say(
            "coredumps",
            format!("{dumps} application crashes in the last 24h"),
            "Something is segfaulting — `coredumpctl list --since -24h` names it",
            Status::Info,
        ));
    }

    if probed.count("swap", 0) == 0 {
        rows.push(say(
            "swap",
            "No swap or zram configured",
            "Under memory pressure the kernel kills apps instead of paging — install zram-generator for compressed in-RAM swap (the CachyOS default)",
            Status::Warn,
        ));
    }

    if probed.at("fstrim", 1) == "1" && probed.at("fstrim", 0) != "1" {
        rows.push(
            say("fstrim", "SSD TRIM timer disabled", "Without weekly TRIM the SSD slows down as it fills and wears faster", Status::Warn).fixed_by(fixes::root(
                "Enable",
                "Staged: verifies the fstrim.timer unit exists (reinstalls util-linux if not), then enables the weekly TRIM timer. No data is touched.",
                vec![format!("{} --pkg util-linux fstrim.timer", from_checkout(checkout, "system/repair/service.sh"))],
            )),
        );
    }

    if probed.first("rtkit") == "inactive" {
        rows.push(
            say("rtkit", "rtkit daemon not running", "PipeWire cannot get realtime priority without it — audio crackles under load", Status::Info).fixed_by(fixes::root(
                "Enable",
                "Staged: installs the rtkit package if it is missing, then enables and starts rtkit-daemon, which grants PipeWire realtime scheduling priority.",
                vec![format!("{} --pkg rtkit rtkit-daemon.service", from_checkout(checkout, "system/repair/service.sh"))],
            )),
        );
    }

    let pinned_fast = features::modes().iter().any(|mode| mode.id == "maxPerf" && mode.enabled);
    if laptop && probed.first("governor") == "performance" && !pinned_fast {
        rows.push(
            say(
                "governor",
                "CPU governor pinned to performance",
                "Max-perf is off but the governor is still \"performance\" — clocks stay high and the battery drains for nothing",
                Status::Info,
            )
            .fixed_by(fixes::user(
                "Rebalance",
                "Sets the power profile back to balanced, which restores the normal governor.",
                vec!["powerprofilesctl set balanced".to_string()],
            )),
        );
    }

    rows.push(match probed.first("face") {
        "ok" => say("face", "Avatar set", "The dashboard's user card has a picture", Status::Ok),
        _ => say("face", "No avatar (~/.face)", "Cosmetic: click the avatar in the dashboard to set one", Status::Info),
    });
    rows
}

// -- Desktop entries and the PATH ------------------------------------------

/// Moving a broken entry aside rather than deleting it: they are somebody's
/// files, and a scan has no business removing those.
fn shelve(files: &[String]) -> Vec<String> {
    let mut commands = vec!["mkdir -p \"$HOME/.local/share/applications/broken-backup\"".to_string()];
    commands.extend(files.iter().map(|file| format!("mv '{file}' \"$HOME/.local/share/applications/broken-backup/\"")));
    commands
}

fn entries(probed: &Probed) -> Vec<Row> {
    let mut rows = Vec::new();
    let broken = probed.count("desktopbroken", 0);
    let broken_ours = probed.count("desktopbroken", 1);
    if broken > 0 {
        let ours = probed.words("desktopbrokenuser", 0);
        let row = say(
            "desktop-broken",
            format!("{broken} launcher entries point at missing programs"),
            format!(
                "{}— leftovers from removed apps; they clutter the launcher and fail on click. {broken_ours} are yours{}",
                probed.at("desktopbroken", 2),
                if broken_ours > 0 { " (fixable here); the rest belong to packages" } else { "; all belong to packages — reinstall or remove those packages" }
            ),
            Status::Info,
        );
        rows.push(if ours.is_empty() {
            row
        } else {
            row.fixed_by(fixes::user(
                "Shelve",
                "Moves the broken entries of yours into ~/.local/share/applications/broken-backup/ — nothing is deleted, and package-owned entries are not touched.",
                shelve(&ours),
            ))
        });
    } else {
        rows.push(say("desktop-broken", "All launcher entries resolve", "Every .desktop Exec points at an existing program", Status::Ok));
    }

    let malformed = probed.count("desktopmalformed", 0);
    if malformed > 0 {
        let ours = probed.words("desktopmalformeduser", 0);
        let row = say(
            "desktop-malformed",
            format!("{malformed} malformed .desktop files"),
            format!(
                "{}— invalid lines make every desktop-entry parser log warnings (the shell included){}",
                probed.at("desktopmalformed", 1),
                if ours.is_empty() { "; all package-owned, harmless but noisy" } else { "" }
            ),
            Status::Info,
        );
        rows.push(if ours.is_empty() {
            row
        } else {
            row.fixed_by(fixes::user(
                "Shelve",
                "Moves the malformed entries of yours into ~/.local/share/applications/broken-backup/ — nothing is deleted.",
                shelve(&ours),
            ))
        });
    }

    let dead_paths = probed.count("pathdirs", 0);
    if dead_paths > 0 {
        rows.push(say(
            "path-dirs",
            format!("{dead_paths} $PATH entries do not exist"),
            format!("{} — every command lookup walks these dead directories; remove them from your shell profile", probed.at("pathdirs", 1)),
            Status::Info,
        ));
    }

    let (ours, theirs) = (probed.words("danglinguser", 1), probed.words("danglingroot", 1));
    if !ours.is_empty() || !theirs.is_empty() {
        let quoted = |files: &[String]| files.iter().map(|file| format!("'{file}'")).collect::<Vec<_>>().join(" ");
        let mut commands = Vec::new();
        if !ours.is_empty() {
            commands.push(format!("rm {}", quoted(&ours)));
        }
        if !theirs.is_empty() {
            commands.push(format!("pkexec rm {}", quoted(&theirs)));
        }
        rows.push(
            say(
                "dangling-links",
                format!("{} dangling symlinks in bin directories", ours.len() + theirs.len()),
                format!("{} {}— they point at nothing and shadow command lookups", ours.join(" "), theirs.join(" ")),
                Status::Info,
            )
            .fixed_by(fixes::user(
                "Remove",
                "Deletes exactly the dangling symlinks listed (their targets are already gone, so the links do nothing). System-level ones go through pkexec.",
                commands,
            )),
        );
    }
    rows
}
