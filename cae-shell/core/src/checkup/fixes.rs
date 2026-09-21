//! What a finding offers to do about itself.
//!
//! A fix is a description before it is a command: the words shown to the
//! person are `summary` and `commands`, and `exec` is what actually runs once
//! they have said yes. Nothing here runs anything.

/// Something the scan can put right.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fix {
    /// The word on the button.
    pub label: String,
    /// What it will do, in full, in plain words.
    pub summary: String,
    /// The commands as they should be read.
    pub commands: Vec<String>,
    /// What is run. Not always the commands above: root work is wrapped.
    pub exec: Vec<String>,
    /// Handed over in `CAELESTIA_FIX` rather than on the command line, so
    /// that no layer of quoting can mangle it.
    pub env: Option<String>,
    /// Whether it will ask for a password.
    pub root: bool,
    /// The package it installs, where it installs one. What the "install
    /// everything missing" bundle is built from.
    pub pkg: Option<String>,
    /// The `system/` directory it installs, where it installs a privileged
    /// half.
    pub dir: Option<String>,
    /// An update is not run as a command: the updater does it.
    pub updates_the_shell: bool,
}

impl Fix {
    fn new(label: &str, summary: &str, commands: Vec<String>, exec: Vec<String>) -> Fix {
        Fix {
            label: label.to_string(),
            summary: summary.to_string(),
            commands,
            exec,
            env: None,
            root: false,
            pkg: None,
            dir: None,
            updates_the_shell: false,
        }
    }

    /// The package this one installs, which is what lets it be bundled with
    /// the others.
    pub fn of_package(mut self, package: &str) -> Fix {
        self.pkg = Some(package.to_string());
        self
    }

    /// The privileged half this one installs.
    pub fn of_half(mut self, dir: &str) -> Fix {
        self.dir = Some(dir.to_string());
        self
    }

    /// Carries a schema or a command list in the environment instead of on
    /// the command line.
    pub fn carrying(mut self, env: String) -> Fix {
        self.env = Some(env);
        self
    }
}

/// A fix that runs as the user. Nothing asks for a password.
pub fn user(label: &str, summary: &str, commands: Vec<String>) -> Fix {
    let run = commands.join(" && ");
    Fix::new(label, summary, commands, vec!["sh".to_string(), "-c".to_string(), run])
}

/// A fix that runs as root.
///
/// `pkexec` is setuid, so once it has started, the shell cannot signal it —
/// a fix spawned as a direct `pkexec` child made cancelling a no-op. It is
/// started under a plain user-level `sh` instead, which can always be
/// killed; a script already past the password keeps running root-side to its
/// end, and they are all short. The command travels in the environment so
/// that no quoting layer mangles it.
pub fn root(label: &str, summary: &str, commands: Vec<String>) -> Fix {
    let carried = commands.join(" && ");
    let mut fix = Fix::new(
        label,
        summary,
        commands,
        vec!["sh".to_string(), "-c".to_string(), "pkexec sh -c \"$CAELESTIA_FIX\"".to_string()],
    );
    fix.root = true;
    fix.carrying(carried)
}

/// The classic way for a pacman fix to fail: a lock file left behind by a
/// pacman that crashed. Checked for a live process before it is removed.
const UNLOCK: &str = "{ [ -e /var/lib/pacman/db.lck ] && ! pgrep -x pacman >/dev/null && rm -f /var/lib/pacman/db.lck; true; }";

/// A root fix that touches pacman, which clears a stale lock first.
pub fn pacman(label: &str, summary: &str, commands: Vec<String>) -> Fix {
    let mut all = vec![UNLOCK.to_string()];
    all.extend(commands);
    root(label, summary, all)
}

/// Installing one package, which is most of what the scan offers.
pub fn install(package: &str) -> Fix {
    pacman(
        "Install",
        &format!("Installs the {package} package with pacman. Nothing is removed."),
        vec![format!("pacman -S --needed --noconfirm {package}")],
    )
    .of_package(package)
}

/// The updater rather than a command: pulling the checkout forward is its
/// job, not a shell line's.
pub fn update_the_shell() -> Fix {
    let mut fix = Fix::new(
        "Update",
        "Pulls origin/main into the shell checkout and restarts the shell. Local files are not touched beyond git's fast-forward.",
        vec!["git pull --ff-only origin main".to_string()],
        Vec::new(),
    );
    fix.updates_the_shell = true;
    fix
}

/// Everything missing, in one password. `None` when nothing is missing.
pub fn install_all(packages: &[String]) -> Option<Fix> {
    (!packages.is_empty()).then(|| {
        pacman(
            "Install all",
            "Installs the packages the shell needs through pacman, as root. Nothing is removed or reconfigured.",
            vec![format!("pacman -S --needed --noconfirm {}", packages.join(" "))],
        )
    })
}

/// The startup prompt's one button: the missing packages and the privileged
/// halves an update left behind, under a single password.
pub fn everything(packages: &[String], halves: &[String], checkout: &std::path::Path) -> Option<Fix> {
    let mut commands = Vec::new();
    if !packages.is_empty() {
        commands.push(format!("pacman -S --needed --noconfirm {}", packages.join(" ")));
    }
    for dir in halves {
        commands.push(format!("bash '{}/system/{dir}/install.sh'", checkout.display()));
    }
    (!commands.is_empty()).then(|| {
        pacman(
            "Fix everything",
            "Installs the missing packages and re-runs the installers of the privileged components that are out of date (they overwrite their own files under /usr/local/bin and /etc/systemd/system, then restart their units). One password, everything as root.",
            commands,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_root_fix_carries_its_commands_in_the_environment() {
        let fix = root("Fix", "does a thing", vec!["a".to_string(), "b".to_string()]);
        assert!(fix.root);
        assert_eq!(fix.env.as_deref(), Some("a && b"));
        assert_eq!(fix.exec, vec!["sh", "-c", "pkexec sh -c \"$CAELESTIA_FIX\""]);
        assert_eq!(fix.commands, vec!["a", "b"], "what is shown is what was asked for");
    }

    #[test]
    fn a_pacman_fix_clears_a_stale_lock_first() {
        let fix = pacman("Install", "…", vec!["pacman -S foo".to_string()]);
        assert_eq!(fix.commands.len(), 2);
        assert!(fix.commands[0].contains("db.lck"));
        assert_eq!(fix.commands[1], "pacman -S foo");
    }

    #[test]
    fn a_user_fix_runs_what_it_shows() {
        let fix = user("Reset", "…", vec!["mv a b".to_string()]);
        assert!(!fix.root);
        assert_eq!(fix.env, None);
        assert_eq!(fix.exec, vec!["sh", "-c", "mv a b"]);
    }

    #[test]
    fn a_bundle_is_only_made_when_there_is_something_in_it() {
        assert_eq!(install_all(&[]), None);
        assert_eq!(everything(&[], &[], std::path::Path::new("/x")), None);
        let both = everything(&["swappy".to_string()], &["max-perf".to_string()], std::path::Path::new("/x")).unwrap();
        assert_eq!(both.commands.len(), 3, "the lock guard, the packages, the half");
        assert!(both.commands[1].contains("swappy"));
        assert!(both.commands[2].contains("/x/system/max-perf/install.sh"));
    }

    #[test]
    fn an_install_names_the_package_it_is_for() {
        assert_eq!(install("swappy").pkg.as_deref(), Some("swappy"));
    }
}
