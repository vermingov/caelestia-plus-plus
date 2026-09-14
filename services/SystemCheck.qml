pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io
import Caelestia
import qs.services
import qs.utils

// System health scanner behind the debug window's "System scan" tab. Probes
// the shell's runtime needs (binaries, daemons, privileged halves, updates)
// plus Hyprland and general system health: config errors, broken configs,
// failed units, package corruption, orphans, audio stack, locale, disk.
//
// Every finding can carry a fix descriptor; nothing runs directly. A fix is
// staged as `pendingFix` — the UI shows its exact commands and what they
// change, and only confirmPendingFix() executes them (root work through
// pkexec, so the user only ever types a password).
//
// A scan also runs once at startup: missing packages and outdated privileged
// halves raise SetupPrompt (modules/debug) with the same confirm-first flow.
Singleton {
    id: root

    property bool scanning
    property string lastScan
    property string busyId

    // [{id, name, detail, status: ok|warn|fail|info, prompt?, fix?}]
    // fix: {label, summary, commands: [display strings], exec: [argv], kind?}
    property var results: []
    readonly property int problemCount: results.filter(r => r.status === "warn" || r.status === "fail").length
    readonly property var missingPackages: results.filter(r => r.fix?.pkg).map(r => r.fix.pkg)

    // Rows that warrant the unprompted startup dialog: missing packages and
    // outdated (already installed) privileged halves
    readonly property var promptItems: results.filter(r => r.prompt)
    readonly property var outdatedRootHalves: results.filter(r => r.prompt && r.id.startsWith("roothalf-")).map(r => r.fix.dir)

    // Staged fix awaiting user confirmation: {id, title, summary, commands, exec, kind}
    property var pendingFix: null

    // Live output of the running (or just finished) fix, streamed line by
    // line into the fix card so "Working" is never a black box
    readonly property ListModel fixLog: ListModel {}
    // null while idle/running; {success, code} once the fix ends
    property var fixResult: null
    // Whether the currently running fix goes through pkexec — the progress
    // card words its waiting state around this (no password prompt promise
    // for fixes that never ask for one)
    property bool runningFixRoot: false

    function dismissFixResult(): void {
        fixResult = null;
        fixLog.clear();
    }

    function copyFixLog(): void {
        const rows = [];
        for (let i = 0; i < fixLog.count; i++)
            rows.push(fixLog.get(i).line);
        Quickshell.execDetached(["wl-copy", rows.join("\n")]);
        Toaster.toast(qsTr("Fix log copied"), qsTr("%1 lines on the clipboard").arg(fixLog.count), "content_copy");
    }

    // A root fix cannot even show its password prompt without an agent;
    // the confirmation cards warn on this instead of hanging silently
    readonly property bool polkitAgentMissing: results.some(r => r.id === "polkit-agent" && r.status === "fail")

    property bool promptOpen: false
    readonly property string statePath: `${Paths.state}/systemcheck.json`
    property var dismissedPackages: []
    property var dismissedRootHalves: ({})
    property var _verState: ({})

    // Runtime tools the shell calls, with the Arch package that provides
    // them and what breaks without them
    readonly property var binaries: [
        {bin: "wl-copy", pkg: "wl-clipboard", why: qsTr("Clipboard: screenshots, colour picker, console copy"), severity: "fail"},
        {bin: "nmcli", pkg: "networkmanager", why: qsTr("Network status and wifi connections in the bar"), severity: "fail"},
        {bin: "python3", pkg: "python", why: qsTr("Shell helper scripts"), severity: "fail"},
        {bin: "notify-send", pkg: "libnotify", why: qsTr("Desktop notifications from shell actions"), severity: "warn"},
        {bin: "powerprofilesctl", pkg: "power-profiles-daemon", why: qsTr("Power profile switching (battery popout, Dynamic, Maximum performance)"), severity: "warn"},
        {bin: "ddcutil", pkg: "ddcutil", why: qsTr("Brightness control for external monitors"), severity: "warn"},
        {bin: "brightnessctl", pkg: "brightnessctl", why: qsTr("Brightness control for the internal display"), severity: "warn", laptopOnly: true},
        {bin: "gpu-screen-recorder", pkg: "gpu-screen-recorder", why: qsTr("Screen recording from the utilities drawer"), severity: "warn"},
        {bin: "swappy", pkg: "swappy", why: qsTr("Screenshot annotation"), severity: "warn"},
        {bin: "xmllint", pkg: "libxml2", why: qsTr("Weather and metadata parsing"), severity: "warn"},
        {bin: "bwrap", pkg: "bubblewrap", why: qsTr("sandrunner: the fake-root sandbox itself"), severity: "warn"},
        {bin: "fuse-overlayfs", pkg: "fuse-overlayfs", why: qsTr("sandrunner: writable throwaway system view (read-only fallback without it)"), severity: "warn"}
    ]

    function scan(): void {
        if (scanning)
            return;
        scanning = true;
        ShellUpdates.check();
        _launchProbes();
    }

    // A scan asked for before the helper check has answered would launch the
    // wrong command, so it waits here and is resumed by the Connections below.
    function _launchProbes(): void {
        if (!Helpers.ready)
            return;
        _probesLeft = 2;
        // All general probing lives in one shell script (assets/systemcheck-
        // probe.sh) printing machine-readable "key|field|field" lines;
        // shell.json gets its own doctor, fed the runtime config schema so
        // its findings match what this build of the shell actually accepts
        probe.command = ["bash", Quickshell.shellPath("assets/systemcheck-probe.sh"), Quickshell.shellDir, Paths.wallsdir, binaries.map(b => b.bin).join(" ")];
        probe.running = true;
        _doctorSchema = ConfigDoctor.schemaJson();
        doctorProbe.environment = {CAELESTIA_SCHEMA: _doctorSchema};
        doctorProbe.command = Helpers.command("config-doctor", [ConfigDoctor.configPath]);
        doctorProbe.running = true;
    }

    property int _probesLeft: 0
    property string _probeOut: ""
    property string _doctorOut: ""
    property string _doctorSchema: ""

    Connections {
        target: Helpers

        function onReadyChanged(): void {
            if (root.scanning)
                root._launchProbes();
        }
    }

    function _probeDone(): void {
        if (--_probesLeft === 0)
            _finish(_probeOut);
    }

    // --- Fix staging: nothing executes without an explicit confirm ----------

    function requestFix(id: string): void {
        const r = results.find(x => x.id === id);
        if (!r?.fix || busyId)
            return;
        pendingFix = Object.assign({id: r.id, title: r.name}, r.fix);
    }

    // Scan-tab "install all missing" bundle
    function requestInstallAll(): void {
        if (!missingPackages.length || busyId)
            return;
        const cmd = "pacman -S --needed --noconfirm " + missingPackages.join(" ");
        pendingFix = Object.assign({
            id: "all",
            title: qsTr("Install %1 missing packages").arg(missingPackages.length)
        }, _pacmanFix(qsTr("Installs the packages the shell needs through pacman, as root via pkexec. Nothing is removed or reconfigured."), [cmd]));
    }

    // Startup-prompt bundle: missing packages + outdated privileged halves,
    // one pkexec for everything
    function requestFixAll(): void {
        if (busyId)
            return;
        const commands = [];
        if (missingPackages.length)
            commands.push("pacman -S --needed --noconfirm " + missingPackages.join(" "));
        for (const dir of outdatedRootHalves)
            commands.push(`bash '${Quickshell.shellDir}/system/${dir}/install.sh'`);
        if (!commands.length)
            return;
        pendingFix = Object.assign({
            id: "all",
            title: qsTr("Fix everything found")
        }, _pacmanFix(qsTr("Installs missing packages and re-runs the installers of outdated privileged components (they overwrite their own files under /usr/local/bin and /etc/systemd/system, then restart their units). One password, everything runs as root via pkexec."), commands));
    }

    function confirmPendingFix(): void {
        const fix = pendingFix;
        pendingFix = null;
        if (!fix || busyId)
            return;
        if (fix.kind === "update") {
            ShellUpdates.update();
            return;
        }
        fixLog.clear();
        fixResult = null;
        runningFixRoot = fix.root ?? false;
        busyId = fix.id;
        fixProc.environment = fix.env ? {CAELESTIA_FIX: fix.env} : {};
        fixProc.command = fix.exec;
        fixProc.running = true;
    }

    function cancelPendingFix(): void {
        pendingFix = null;
    }

    // Kills a fix stuck on e.g. a pkexec prompt that can never appear
    function cancelRunningFix(): void {
        if (fixProc.running) {
            _fixCancelled = true;
            fixProc.running = false;
        }
    }

    property bool _fixCancelled: false

    function dismissPrompt(): void {
        dismissedPackages = [...new Set(dismissedPackages.concat(missingPackages))];
        const halves = Object.assign({}, dismissedRootHalves);
        for (const dir of outdatedRootHalves)
            halves[dir] = _verState[dir]?.repo ?? 0;
        dismissedRootHalves = halves;
        store.setText(JSON.stringify({dismissedPackages: dismissedPackages, dismissedRootHalves: dismissedRootHalves}, null, 2) + "\n");
        promptOpen = false;
    }

    // --- Row builders --------------------------------------------------------

    // Root fixes run pkexec under a plain user-level sh wrapper on purpose:
    // pkexec is setuid, so once it starts, this process cannot signal it —
    // a direct pkexec child made "Cancel fix" a no-op. Killing the wrapper
    // always works and unsticks the UI; a script already past auth keeps
    // running root-side to completion (they are short and self-limiting).
    // The command travels in $CAELESTIA_FIX so no quoting layer mangles it.
    function _rootFix(summary: string, commands: var): var {
        return {summary: summary, commands: commands, root: true, exec: ["sh", "-c", `pkexec sh -c "$CAELESTIA_FIX"`], env: commands.join(" && ")};
    }

    function _userFix(summary: string, commands: var): var {
        return {summary: summary, commands: commands, exec: ["sh", "-c", commands.join(" && ")]};
    }

    // Every pacman-touching fix self-heals the classic failure mode first:
    // a stale db.lck left by a crashed pacman (checked for a live process)
    readonly property string _pacmanGuard: "{ [ -e /var/lib/pacman/db.lck ] && ! pgrep -x pacman >/dev/null && rm -f /var/lib/pacman/db.lck; true; }"

    function _pacmanFix(summary: string, commands: var): var {
        return _rootFix(summary, [_pacmanGuard].concat(commands));
    }

    function _finish(out: string): void {
        const flags = {};
        const vers = {};
        const cfgs = [];
        for (const line of out.trim().split("\n")) {
            const parts = line.split("|");
            if (parts[0] === "ver")
                vers[parts[1]] = {repo: parseInt(parts[2], 10) || 0, inst: parseInt(parts[3], 10) || 0, enabled: parts[4] === "1"};
            else if (parts[0] === "cfgjson")
                cfgs.push({file: parts[1], ok: parts[2] === "ok"});
            else if (parts[0] === "bin" || parts[0] === "pkg")
                flags[`${parts[0]}.${parts[1]}`] = parts[2];
            else
                flags[parts[0]] = parts.slice(1);
        }
        _verState = vers;

        const rows = [];
        const push = (id, name, detail, status, extra) => rows.push(Object.assign({id, name, detail, status}, extra ?? {}));

        // -- Shell runtime dependencies
        for (const b of binaries) {
            if (b.laptopOnly && !SysInfo.isLaptop)
                continue;
            const ok = flags[`bin.${b.bin}`] === "ok";
            push(`bin-${b.bin}`, ok ? qsTr("%1 installed").arg(b.pkg) : qsTr("%1 missing").arg(b.pkg), b.why, ok ? "ok" : b.severity, ok ? null : {
                prompt: true,
                fix: Object.assign({label: qsTr("Install"), pkg: b.pkg}, _pacmanFix(qsTr("Installs the %1 package with pacman. Nothing is removed.").arg(b.pkg), [`pacman -S --needed --noconfirm ${b.pkg}`]))
            });
        }

        const pmConflicts = (flags.pmconflict?.[0] ?? "").trim();
        if (flags["bin.powerprofilesctl"] === "ok") {
            const active = flags.ppd?.[0] === "active";
            const unitEnabled = flags.ppdunit?.[0] ?? "";
            const unitMissing = /not-found|No such file|could not be found/i.test(unitEnabled);
            const unitMasked = unitEnabled.includes("masked");
            const state = qsTr("unit state: %1 / %2").arg(unitEnabled || "?").arg(flags.ppdunit?.[1] ?? "?");
            const cause = unitMissing ? qsTr("service unit not found — the powerprofilesctl tool is present but the daemon package half is not; dbus activation times out with NoReply") : unitMasked ? qsTr("unit is masked, usually by another power tool — dbus activation times out with NoReply") : pmConflicts ? qsTr("likely blocked by %1 (see the conflict finding)").arg(pmConflicts) : qsTr("the daemon behind power profile switching is not active");
            push("ppd-service", active ? qsTr("power-profiles-daemon running") : qsTr("power-profiles-daemon not running"), active ? qsTr("The daemon behind power profile switching") : `${cause} — ${state}`, active ? "ok" : "warn", active ? null : {
                fix: Object.assign({label: qsTr("Repair")}, _rootFix(qsTr("Runs a staged repair that diagnoses as it goes and only applies what the diagnosis calls for: installs/reinstalls the package if it or its unit is missing, unmasks the unit, stops and DISABLES conflicting power daemons it finds (tlp, tuned, tuned-ppd, auto-cpufreq, laptop-mode — they cause exactly this failure), then starts the service and verifies it answers on D-Bus. On failure it prints the daemon's own journal. Every step streams into this window; reload the shell at the end so it reconnects."), [`bash '${Quickshell.shellDir}/system/repair/power-profiles.sh'`]))
            });
        }

        if (pmConflicts)
            push("pm-conflict", qsTr("Conflicting power manager installed"), qsTr("%1 fights power-profiles-daemon over the same hardware knobs — the daemon often gets masked or fails to start (dbus NoReply). Keep one: either remove %1, or remove power-profiles-daemon. Your call, so no automatic fix.").arg(pmConflicts.trim()), "warn");

        // -- quickshell itself: does the binary on disk still start (a Qt
        // update can break it while this process keeps running on the old
        // libraries), and is the shell scheduled as a foreground process
        const [qsBin, qsBinReason] = flags.qsbin ?? [];
        push("qs-binary", qsBin === "ok" ? qsTr("quickshell starts") : qsTr("quickshell cannot start"), qsBin === "ok" ? qsTr("The installed qs binary loads against the current Qt") : qsTr("This shell keeps running on the old libraries, but the next login or restart comes up without a shell: a Qt update moved private symbols the prebuilt quickshell links against (%1). Rebuilding it against the installed Qt fixes it for good.").arg(qsBinReason || "undefined symbol"), qsBin === "ok" ? "ok" : "fail", qsBin === "ok" ? null : {
            prompt: true,
            fix: Object.assign({label: qsTr("Rebuild")}, _rootFix(qsTr("Runs system/quickshell/install.sh as root: installs the build dependencies with pacman, builds quickshell from source as your user (a few minutes — output streams here), installs it as caelestia++-quickshell pinned via IgnorePkg, and files the shell as a foreground process for ananicy. Restart the shell afterwards."), [`bash '${Quickshell.shellDir}/system/quickshell/install.sh' --force`]))
        });

        const [qsNice, ananicy] = flags.qsnice ?? [];
        const niceValue = parseInt(qsNice, 10);
        if (!isNaN(niceValue) && niceValue > 0)
            push("qs-nice", qsTr("Shell runs at background priority (nice %1)").arg(niceValue), ananicy === "active" ? qsTr("ananicy-cpp files qs under its Service class, so the bar and panels get starved whenever something heavy runs. A rule override puts the shell back at foreground priority.") : qsTr("Something started the shell with a lowered priority, so the bar and panels get starved whenever something heavy runs."), "warn", ananicy === "active" ? {
                fix: Object.assign({label: qsTr("Fix")}, _rootFix(qsTr("Installs /etc/ananicy.d/zz-caelestia/quickshell.rules (qs at foreground priority) and reloads ananicy-cpp. The rule applies to the shell from its next start."), [`bash '${Quickshell.shellDir}/system/quickshell/install.sh'`]))
            } : null);

        // -- Caelestia: updates, privileged halves, config, checkout
        push("shell-updates", ShellUpdates.updateAvailable ? qsTr("Shell %1 commits behind").arg(ShellUpdates.commitsBehind) : qsTr("Shell up to date"), ShellUpdates.updateAvailable ? qsTr("Newer Caelestia++ is on origin/main") : qsTr("Checked against origin/main"), ShellUpdates.updateAvailable ? "warn" : "ok", ShellUpdates.updateAvailable ? {
            fix: {label: qsTr("Update"), kind: "update", summary: qsTr("Pulls origin/main into the shell checkout and reloads the shell. Local files are not touched beyond git's fast-forward."), commands: ["git pull --ff-only origin main"]}
        } : null);

        const rootHalves = [
            {dir: "max-perf", name: qsTr("max-perf")},
            {dir: "anti-heat", name: qsTr("anti-heat")},
            {dir: "dynamic", name: qsTr("dynamic performance"), upgradeOnly: SysInfo.isLaptop},
            {dir: "bed-mode", name: qsTr("bed mode"), upgradeOnly: true},
            // Protection and the firewall install from their own tabs; only
            // flag version upgrades, never a missing install
            {dir: "redguard", name: qsTr("protection"), upgradeOnly: true},
            {dir: "redwall", name: qsTr("firewall"), upgradeOnly: true}
        ];
        for (const h of rootHalves) {
            const v = vers[h.dir] ?? {repo: 0, inst: 0, enabled: false};
            const installFix = label => Object.assign({label, dir: h.dir}, _rootFix(qsTr("Runs system/%1/install.sh from the checkout as root: copies its scripts to /usr/local/bin, its units to /etc/systemd/system, and enables the watcher unit. Overwrites previous versions of the same files only.").arg(h.dir), [`bash '${Quickshell.shellDir}/system/${h.dir}/install.sh'`]));
            if (v.enabled && v.repo > v.inst)
                push(`roothalf-${h.dir}`, qsTr("%1 root half outdated (v%2, current v%3)").arg(h.name).arg(v.inst).arg(v.repo), qsTr("The installed privileged half is from an older Caelestia++ — update to get the latest behaviour"), "warn", {prompt: true, fix: installFix(qsTr("Update"))});
            else if (!v.enabled && !h.upgradeOnly)
                push(`roothalf-${h.dir}`, qsTr("%1 root half missing").arg(h.name), qsTr("The %1 feature stays inactive until its privileged half is installed").arg(h.name), "warn", {fix: installFix(qsTr("Install"))});
            else if (v.enabled)
                push(`roothalf-${h.dir}`, qsTr("%1 root half installed and current").arg(h.name), qsTr("Feature fully available"), "ok");
        }
        // sandrunner has no privileged half — "installed" is a ~/.local/bin
        // symlink into the checkout. Its packages (bubblewrap, fuse-overlayfs)
        // ride the binaries list above, so missing ones raise the startup
        // prompt like any other shell dependency.
        const [srLink, srPath] = flags.sandrunner ?? [];
        if (srLink !== "ok") {
            // Complete our own install silently: the link is user-level,
            // idempotent and points into the checkout — updates delivered by
            // git pull alone never run the installer and would leave the
            // command missing forever.
            Quickshell.execDetached(["sh", "-c", `mkdir -p "$HOME/.local/bin" && ln -sf '${Quickshell.shellDir}/system/sandrunner/sandrunner' "$HOME/.local/bin/sandrunner"`]);
            push("sandrunner", qsTr("sandrunner PATH link restored"), qsTr("The ~/.local/bin symlink was missing and has been recreated"), "info");
        } else if (srPath === "missing")
            push("sandrunner", qsTr("sandrunner on PATH after next login"), qsTr("~/.local/bin was added to your login shell's profile automatically — open a new terminal (or re-login) and `sandrunner` works"), "info");
        else
            push("sandrunner", qsTr("sandrunner installed"), qsTr("Full-simulation sandbox available as `sandrunner FILE`"), "ok");
        // shell.json has a dedicated doctor (assets/config-doctor.py): it
        // diagnoses against the runtime schema and its fix repairs the file
        // surgically — typos renamed, wrong types coerced or defaulted,
        // broken bar entries mended — with the original kept next to it
        const doctorLines = _doctorOut.trim().split("\n").filter(l => l);
        const doctorIssues = doctorLines.filter(l => l.startsWith("issue|")).map(l => {
            const p = l.split("|");
            return {sev: p[1], problem: p[2], action: p[3]};
        });
        const doctorRan = doctorLines.length > 0;
        if (doctorRan) {
            const complaints = ConfigDoctor.loadComplaints;
            if (doctorIssues.length) {
                push("shell-config", qsTr("%n problem(s) in shell.json", "", doctorIssues.length), doctorIssues.map(i => i.problem).slice(0, 4).join("  ·  ") + (doctorIssues.length > 4 ? "  ·  …" : ""), doctorIssues.some(i => i.sev === "fail") ? "fail" : "warn", {
                    fix: {
                        label: qsTr("Repair"),
                        summary: qsTr("Applies exactly the repairs listed below, nothing else. The original file is copied aside first (.doctor-bak), every unrecognised setting close to a real one is treated as a typo and renamed rather than lost, and the shell picks the fixed config up immediately."),
                        commands: doctorIssues.map(i => i.action),
                        exec: Helpers.command("config-doctor", [ConfigDoctor.configPath, "--repair"]),
                        env: _doctorSchema
                    }
                });
            } else {
                push("shell-config", qsTr("shell.json valid"), complaints.length ? qsTr("Valid now, but the shell complained at load: %1").arg(complaints.join("; ")) : qsTr("Structure and types match what this shell accepts"), complaints.length ? "info" : "ok");
            }
        }

        for (const c of cfgs) {
            if (doctorRan && c.file === ConfigDoctor.configPath)
                continue;
            push(`cfg-${c.file}`, c.ok ? qsTr("Config valid: %1").arg(c.file.split("/").pop()) : qsTr("Config is not valid JSON: %1").arg(c.file.split("/").pop()), c.ok ? c.file : qsTr("%1 — the shell falls back to defaults while this file is broken").arg(c.file), c.ok ? "ok" : "fail", c.ok ? null : {
                fix: Object.assign({label: qsTr("Reset")}, _userFix(qsTr("Moves the broken file aside (a .broken.bak copy stays next to it, nothing is deleted); the shell then regenerates defaults."), [`mv '${c.file}' '${c.file}.broken.bak'`]))
            });
        }

        // -- Notification daemon ownership
        // Only the process owning org.freedesktop.Notifications draws notifs.
        // If it isn't this quickshell, the shell's own notifications never
        // show and a foreign daemon's popups appear instead.
        const [notifOwner, notifUnit] = flags.notifowner ?? [];
        const ownedByShell = notifOwner === "qs" || notifOwner === "quickshell";
        if (notifOwner && notifOwner !== "none" && !ownedByShell) {
            // Stop a real daemon unit for good (disable + mask); otherwise just
            // kill the process. Either way, relaunch the shell — detached so it
            // survives its own restart — to claim the freed name.
            const killCmds = [];
            let summary = qsTr("Another notification daemon, %1, currently owns the notification service, so Caelestia++'s own notifications can't appear — you see that daemon's plain popups instead. ").arg(notifOwner);
            if (notifUnit) {
                killCmds.push(`systemctl --user disable --now '${notifUnit}'`);
                killCmds.push(`systemctl --user mask '${notifUnit}'`);
                summary += qsTr("This stops and masks its user service (%1) so it stays gone, ").arg(notifUnit);
            } else {
                killCmds.push(`pkill -x '${notifOwner}' || true`);
                summary += qsTr("This stops it now (it has no systemd unit — if it comes back after a reboot, remove it from your Hyprland exec-once), ");
            }
            summary += qsTr("then reloads the shell so Caelestia++ takes over notifications.");
            killCmds.push("setsid sh -c 'sleep 1; pkill -x qs; sleep 1; caelestia shell -d' >/dev/null 2>&1 &");
            push("notif-daemon", qsTr("Notifications hijacked by %1").arg(notifOwner), qsTr("%1 owns the notification service — Caelestia++'s notifications don't show, its popups do (that plain look)").arg(notifOwner), "warn", {
                fix: Object.assign({label: qsTr("Reclaim")}, _userFix(summary, killCmds))
            });
        } else if (notifOwner === "none") {
            push("notif-daemon", qsTr("No notification server running"), qsTr("Nothing owns the notification service yet — reload the shell so Caelestia++ can claim it"), "warn", {
                fix: Object.assign({label: qsTr("Reload")}, _userFix(qsTr("Reloads the shell so its notification server binds the free notification service name."), ["setsid sh -c 'sleep 1; pkill -x qs; sleep 1; caelestia shell -d' >/dev/null 2>&1 &"]))
            });
        } else {
            push("notif-daemon", qsTr("Notifications handled by Caelestia++"), qsTr("The shell owns the notification service — its own notifications show"), "ok");
        }

        const dirty = parseInt(flags.gitdirty?.[0] ?? "0", 10);
        push("git-dirty", dirty > 0 ? qsTr("Shell checkout has %1 modified files").arg(dirty) : qsTr("Shell checkout clean"), dirty > 0 ? qsTr("Local edits are fine, but they can conflict with updates — no automatic action") : qsTr("No local modifications"), dirty > 0 ? "info" : "ok");

        push("ignorepkg", flags.ignpkg?.[0] === "ok" ? qsTr("Pacman IgnorePkg set") : qsTr("Pacman IgnorePkg not set"), flags.ignpkg?.[0] === "ok" ? qsTr("Repo packages won't clobber the git checkout") : qsTr("A caelestia++ repo package could overwrite this checkout on -Syu"), flags.ignpkg?.[0] === "ok" ? "ok" : "warn", flags.ignpkg?.[0] === "ok" ? null : {
            fix: Object.assign({label: qsTr("Fix")}, _rootFix(qsTr("Appends one IgnorePkg line to /etc/pacman.conf so system updates skip the caelestia++ packages. No other line is touched."), ["grep -q 'IgnorePkg.*caelestia' /etc/pacman.conf || printf 'IgnorePkg = caelestia++-shell caelestia++-cli\\n' >> /etc/pacman.conf"]))
        });

        // -- Hyprland
        const hyprErrs = parseInt(flags.hyprerr?.[0] ?? "0", 10);
        push("hypr-config", hyprErrs > 0 ? qsTr("Hyprland config has %1 errors").arg(hyprErrs) : qsTr("Hyprland config parses clean"), hyprErrs > 0 ? qsTr("First: %1 — full list via `hyprctl configerrors`; needs a manual edit").arg(flags.hyprerr?.[1] ?? "") : qsTr("hyprctl configerrors reports none"), hyprErrs > 0 ? "fail" : "ok");

        for (const p of ["xdg-desktop-portal-hyprland", "qt6-wayland"]) {
            const ok = flags[`pkg.${p}`] === "ok";
            push(`pkg-${p}`, ok ? qsTr("%1 installed").arg(p) : qsTr("%1 missing").arg(p), p === "qt6-wayland" ? qsTr("Qt apps need it to run natively on Wayland") : qsTr("Screen sharing and file pickers break without the Hyprland portal"), ok ? "ok" : "fail", ok ? null : {
                prompt: true,
                fix: Object.assign({label: qsTr("Install"), pkg: p}, _pacmanFix(qsTr("Installs the %1 package with pacman. Nothing is removed.").arg(p), [`pacman -S --needed --noconfirm ${p}`]))
            });
        }

        const extraPortals = (flags.portals?.[0] ?? "").trim();
        push("portals-extra", extraPortals ? qsTr("Extra desktop portals installed") : qsTr("No conflicting desktop portals"), extraPortals ? qsTr("%1 — apps can pick the wrong portal and hang on start; remove them manually if you see slow app launches").arg(extraPortals) : qsTr("Only the Hyprland (and gtk) portals are present"), extraPortals ? "info" : "ok");

        const agentOk = flags.polkitagent?.[0] === "ok";
        push("polkit-agent", agentOk ? qsTr("Polkit agent running") : qsTr("No polkit authentication agent running"), agentOk ? qsTr("Password prompts for privileged actions work") : qsTr("Without one, no password dialog can appear — including these quick fixes. Install one and add it to Hyprland's exec-once."), agentOk ? "ok" : "fail", agentOk ? null : {
            fix: Object.assign({label: qsTr("Install")}, _pacmanFix(qsTr("Installs the hyprpolkitagent package. You still need to add `exec-once = systemctl --user start hyprpolkitagent` to your Hyprland config — that part is not automated."), ["pacman -S --needed --noconfirm hyprpolkitagent"]))
        });

        // -- System health
        const failedSys = parseInt(flags.failed?.[0] ?? "0", 10);
        const failedUser = parseInt(flags.userfailed?.[0] ?? "0", 10);
        if (failedSys + failedUser > 0) {
            const names = `${flags.failed?.[1] ?? ""} ${flags.userfailed?.[1] ?? ""}`.trim();
            const commands = [];
            if (failedUser > 0)
                commands.push("systemctl --user reset-failed");
            if (failedSys > 0)
                commands.push("pkexec systemctl reset-failed");
            push("failed-units", qsTr("%1 systemd units failed").arg(failedSys + failedUser), qsTr("%1 — check them with `systemctl status <unit>`; the quick fix only clears the failed markers, it does not repair the services").arg(names), "warn", {
                fix: Object.assign({label: qsTr("Clear")}, _userFix(qsTr("Clears systemd's failed-unit markers (user units directly, system units via pkexec). Purely cosmetic: the underlying services are NOT repaired and will show up again if they fail again."), commands))
            });
        } else {
            push("failed-units", qsTr("No failed systemd units"), qsTr("System and user managers are clean"), "ok");
        }

        const lockStale = flags.paclock?.[0] === "stale";
        push("pacman-lock", lockStale ? qsTr("Stale pacman database lock") : qsTr("Pacman database unlocked"), lockStale ? qsTr("db.lck exists but no pacman process is running — every install will fail until it is removed") : qsTr("No leftover db.lck"), lockStale ? "fail" : "ok", lockStale ? {
            fix: Object.assign({label: qsTr("Remove")}, _rootFix(qsTr("Deletes /var/lib/pacman/db.lck. Safe only because the scan verified no pacman process is running right now."), ["rm /var/lib/pacman/db.lck"]))
        } : null);

        const corrupt = parseInt(flags.corrupt?.[0] ?? "0", 10);
        push("corrupt-pkgs", corrupt > 0 ? qsTr("%1 packages have missing files").arg(corrupt) : qsTr("All package files present"), corrupt > 0 ? qsTr("%1 — files these packages installed are gone from disk (deleted or corrupted); reinstalling restores them").arg(flags.corrupt?.[1] ?? "") : qsTr("pacman -Qk finds nothing missing"), corrupt > 0 ? "warn" : "ok", corrupt > 0 ? {
            fix: Object.assign({label: qsTr("Reinstall")}, _pacmanFix(qsTr("Reinstalls the affected packages with pacman, restoring their missing files. Configs in /etc marked as backup files are preserved by pacman."), ["p=$(LC_ALL=C pacman -Qk 2>&1 >/dev/null | awk -F': ' '/No such file or directory/ {print $2}' | sort -u); [ -n \"$p\" ] && pacman -S --noconfirm $p || echo 'nothing missing anymore'"]))
        } : null);

        const orphans = parseInt(flags.orphans?.[0] ?? "0", 10);
        push("orphans", orphans > 0 ? qsTr("%1 orphaned packages").arg(orphans) : qsTr("No orphaned packages"), orphans > 0 ? qsTr("Installed as dependencies, no longer needed by anything: %1%2").arg(flags.orphans?.[1] ?? "").arg(orphans > 10 ? "…" : "") : qsTr("pacman -Qtdq is empty"), orphans > 0 ? "info" : "ok", orphans > 0 ? {
            fix: Object.assign({label: qsTr("Remove")}, _pacmanFix(qsTr("First marks everything the shell itself needs as explicitly installed (so it can never be swept), then removes the remaining orphans and their unneeded dependencies (pacman -Rns)."), [`pacman -D --asexplicit wl-clipboard networkmanager python libnotify power-profiles-daemon ddcutil brightnessctl gpu-screen-recorder swappy libxml2 qt6-wayland xdg-desktop-portal-hyprland hyprpolkitagent pacman-contrib >/dev/null 2>&1 || true`, "o=$(pacman -Qtdq); [ -n \"$o\" ] && pacman -Rns --noconfirm $o || echo 'nothing left to remove'"]))
        } : null);

        const cacheGB = parseInt(flags.paccache?.[0] ?? "0", 10);
        const hasPaccache = flags.paccache?.[1] === "1";
        if (cacheGB >= 8)
            push("pac-cache", qsTr("Package cache is %1 GiB").arg(cacheGB), hasPaccache ? qsTr("Old package versions pile up in /var/cache/pacman/pkg") : qsTr("Old package versions pile up in /var/cache/pacman/pkg — install pacman-contrib for the paccache cleaner"), "info", hasPaccache ? {
                fix: Object.assign({label: qsTr("Clean")}, _pacmanFix(qsTr("Installs pacman-contrib first if the paccache tool is missing, then deletes cached package files except the two most recent versions of each package. Installed software is not affected."), ["command -v paccache >/dev/null || pacman -S --needed --noconfirm pacman-contrib", "paccache -rk2"]))
            } : null);

        const diskPct = parseInt(flags.disk?.[0] ?? "0", 10);
        push("disk-root", diskPct >= 90 ? qsTr("Root filesystem %1% full").arg(diskPct) : qsTr("Root filesystem at %1%").arg(diskPct), diskPct >= 90 ? qsTr("Things start failing in odd ways when / fills up — free some space") : qsTr("Plenty of room"), diskPct >= 95 ? "fail" : diskPct >= 90 ? "warn" : "ok");

        // -- Audio
        const pipewireOk = flags.pipewire?.[0] === "active";
        push("pipewire", pipewireOk ? qsTr("PipeWire running") : qsTr("PipeWire not running"), pipewireOk ? qsTr("Audio stack is up") : qsTr("No audio and no visualiser without it"), pipewireOk ? "ok" : "fail", pipewireOk ? null : {
            fix: Object.assign({label: qsTr("Start")}, _userFix(qsTr("Staged: checks each audio unit exists, unmasks if needed, then enables and starts pipewire, pipewire-pulse and wireplumber. Streams its progress here."), [`bash '${Quickshell.shellDir}/system/repair/service.sh' --user pipewire.service pipewire-pulse.service wireplumber.service`]))
        });

        const dupSinks = (flags.dupsinks?.[0] ?? "").trim();
        push("dup-sinks", dupSinks ? qsTr("Duplicate audio sinks") : qsTr("No duplicate audio sinks"), dupSinks ? qsTr("%1 — the same device shows up twice (usually a stale ALSA/PipeWire profile); restarting the audio stack rebuilds the device list").arg(dupSinks) : qsTr("Each output device appears once"), dupSinks ? "warn" : "ok", dupSinks ? {
            fix: Object.assign({label: qsTr("Restart audio")}, _userFix(qsTr("Restarts pipewire, pipewire-pulse and wireplumber (staged, with per-unit verification). Audio cuts out for a second or two; apps reconnect automatically."), [`bash '${Quickshell.shellDir}/system/repair/service.sh' --user --restart pipewire.service pipewire-pulse.service wireplumber.service`]))
        } : null);

        // -- Misc environment
        push("locale", flags.locale?.[0] === "bad" ? qsTr("Broken locale configuration") : qsTr("Locale configuration valid"), flags.locale?.[0] === "bad" ? qsTr("`locale` prints errors — apps misbehave with unset locales. Fix /etc/locale.gen (uncomment your locale), run locale-gen, and set LANG in /etc/locale.conf — machine-specific, so no automatic fix") : qsTr("locale reports no errors"), flags.locale?.[0] === "bad" ? "warn" : "ok");

        const ntpOn = (flags.ntp?.[0] ?? "yes") !== "no";
        push("ntp", ntpOn ? qsTr("Clock synchronisation on") : qsTr("Clock synchronisation off"), ntpOn ? qsTr("systemd-timesyncd keeps the clock right") : qsTr("A drifting clock breaks TLS and package signatures eventually"), ntpOn ? "ok" : "info", ntpOn ? null : {
            fix: Object.assign({label: qsTr("Enable")}, _rootFix(qsTr("Runs timedatectl set-ntp true, enabling systemd's time synchronisation."), ["timedatectl set-ntp true"]))
        });

        const journalErrs = parseInt(flags.journal?.[0] ?? "0", 10);
        push("journal-errors", journalErrs > 0 ? qsTr("%1 error-level journal entries this boot").arg(journalErrs) : qsTr("No error-level journal entries this boot"), journalErrs > 0 ? qsTr("Not all are serious — read them with `journalctl -b -p err`") : qsTr("journalctl -b -p err is empty"), journalErrs > 0 ? "info" : "ok");

        push("face", flags.face?.[0] === "ok" ? qsTr("Avatar set") : qsTr("No avatar (~/.face)"), flags.face?.[0] === "ok" ? qsTr("Dashboard user card has a picture") : qsTr("Cosmetic: click the avatar in the dashboard to set one"), flags.face?.[0] === "ok" ? "ok" : "info");

        push("log-errors", DebugConsole.errorCount > 0 ? qsTr("%1 errors in the shell log").arg(DebugConsole.errorCount) : qsTr("No errors in the shell log"), DebugConsole.errorCount > 0 ? qsTr("See the console tab; copy diagnostics collects the distinct ones") : qsTr("This session so far"), DebugConsole.errorCount > 0 ? "warn" : "ok");

        // -- Caelestia extras
        push("caelestia-cli", flags.cli?.[0] === "broken" ? qsTr("caelestia CLI broken") : qsTr("caelestia CLI works"), flags.cli?.[0] === "broken" ? qsTr("`caelestia --version` fails — schemes, wallpapers and recording die with it; reinstall the caelestia++-cli package") : qsTr("Scheme, wallpaper and recorder plumbing available"), flags.cli?.[0] === "broken" ? "fail" : "ok");

        if (flags.walldir?.[0] === "missing")
            push("wallpaper-dir", qsTr("Wallpaper directory missing"), qsTr("%1 does not exist — the wallpaper picker has nothing to show").arg(flags.walldir?.[1] ?? ""), "info", {
                fix: Object.assign({label: qsTr("Create")}, _userFix(qsTr("Creates the empty wallpaper directory the config points at. Nothing else changes."), [`mkdir -p '${flags.walldir?.[1] ?? ""}'`]))
            });

        // -- Hyprland extras
        const portalUp = flags.portalsvc?.[0] === "active";
        push("portal-service", portalUp ? qsTr("Desktop portal service running") : qsTr("Desktop portal service not running"), portalUp ? qsTr("Screen sharing and file pickers are wired up") : qsTr("Flatpaks, screen sharing and file pickers break without it"), portalUp ? "ok" : "warn", portalUp ? null : {
            fix: Object.assign({label: qsTr("Start")}, _userFix(qsTr("Staged: brings up your user's xdg-desktop-portal (static units are started directly), then restarts the Hyprland portal behind it. Streams its progress here."), [`bash '${Quickshell.shellDir}/system/repair/service.sh' --user xdg-desktop-portal.service`, `bash '${Quickshell.shellDir}/system/repair/service.sh' --user --restart xdg-desktop-portal-hyprland.service`]))
        });

        const lowRefresh = parseInt(flags.refresh?.[0] ?? "0", 10);
        if (lowRefresh > 0)
            push("monitor-refresh", qsTr("%1 monitors run below their best refresh rate").arg(lowRefresh), qsTr("%1 — set the higher rate in your Hyprland monitor config (monitor = name, resolution@rate, …); free smoothness").arg(flags.refresh?.[1] ?? ""), "info");

        // -- Desktop entries and paths
        const brokenDesk = parseInt(flags.desktopbroken?.[0] ?? "0", 10);
        const brokenDeskUser = parseInt(flags.desktopbroken?.[1] ?? "0", 10);
        if (brokenDesk > 0) {
            const userFiles = (flags.desktopbrokenuser?.[0] ?? "").trim().split(" ").filter(f => f);
            push("desktop-broken", qsTr("%1 launcher entries point at missing programs").arg(brokenDesk), qsTr("%1— leftovers from removed apps; they clutter the launcher and fail on click. %2 are user-level%3").arg(flags.desktopbroken?.[2] ?? "").arg(brokenDeskUser).arg(brokenDeskUser > 0 ? qsTr(" (fixable here); the rest belong to packages") : qsTr("; all belong to packages — reinstall or remove those packages")), "info", brokenDeskUser > 0 ? {
                fix: Object.assign({label: qsTr("Shelve")}, _userFix(qsTr("Moves the broken user-level .desktop files into ~/.local/share/applications/broken-backup/ — nothing is deleted, and package-owned entries are not touched."), ["mkdir -p \"$HOME/.local/share/applications/broken-backup\""].concat(userFiles.map(f => `mv '${f}' \"$HOME/.local/share/applications/broken-backup/\"`))))
            } : null);
        } else {
            push("desktop-broken", qsTr("All launcher entries resolve"), qsTr("Every .desktop Exec points at an existing program"), "ok");
        }

        const malformed = parseInt(flags.desktopmalformed?.[0] ?? "0", 10);
        if (malformed > 0) {
            const userFiles = (flags.desktopmalformeduser?.[0] ?? "").trim().split(" ").filter(f => f);
            push("desktop-malformed", qsTr("%1 malformed .desktop files").arg(malformed), qsTr("%1— invalid lines make every desktop-entry parser log warnings (the shell included)%2").arg(flags.desktopmalformed?.[1] ?? "").arg(userFiles.length ? "" : qsTr("; all package-owned, harmless but noisy")), "info", userFiles.length ? {
                fix: Object.assign({label: qsTr("Shelve")}, _userFix(qsTr("Moves the malformed user-level .desktop files into ~/.local/share/applications/broken-backup/ — nothing is deleted."), ["mkdir -p \"$HOME/.local/share/applications/broken-backup\""].concat(userFiles.map(f => `mv '${f}' \"$HOME/.local/share/applications/broken-backup/\"`))))
            } : null);
        }

        const missingPathDirs = parseInt(flags.pathdirs?.[0] ?? "0", 10);
        if (missingPathDirs > 0)
            push("path-dirs", qsTr("%1 $PATH entries do not exist").arg(missingPathDirs), qsTr("%1 — every command lookup walks these dead directories; remove them from your shell profile").arg(flags.pathdirs?.[1] ?? ""), "info");

        const dangUser = (flags.danglinguser?.[1] ?? "").trim().split(" ").filter(f => f);
        const dangRoot = (flags.danglingroot?.[1] ?? "").trim().split(" ").filter(f => f);
        if (dangUser.length + dangRoot.length > 0) {
            const commands = [];
            if (dangUser.length)
                commands.push(`rm ${dangUser.map(f => `'${f}'`).join(" ")}`);
            if (dangRoot.length)
                commands.push(`pkexec rm ${dangRoot.map(f => `'${f}'`).join(" ")}`);
            push("dangling-links", qsTr("%1 dangling symlinks in bin directories").arg(dangUser.length + dangRoot.length), qsTr("%1 %2— they point at nothing and shadow command lookups").arg(dangUser.join(" ")).arg(dangRoot.join(" ")), "info", {
                fix: Object.assign({label: qsTr("Remove")}, _userFix(qsTr("Deletes exactly the dangling symlinks listed (targets are already gone; the links do nothing). System-level ones go through pkexec."), commands))
            });
        }

        // -- Distro and packages
        const onCachy = flags.osid?.[0] === "cachyos";
        const foreignCount = parseInt(flags.foreignrepo?.[0] ?? "0", 10);
        if (foreignCount > 0) {
            const switchCmd = `pacman -S --noconfirm $(pacman -Qmq | grep -v '^caelestia++' | grep -v -- '-debug$' | while read -r p; do pacman -Si "$p" >/dev/null 2>&1 && printf '%s ' "$p"; done)`;
            push("foreign-repo", onCachy ? qsTr("%1 AUR packages have CachyOS repo builds").arg(foreignCount) : qsTr("%1 foreign packages have repo builds").arg(foreignCount), qsTr("%1— the repo versions update with the system%2").arg(flags.foreignrepo?.[1] ?? "").arg(onCachy ? qsTr(" and CachyOS ships them compiler-optimized (v3/znver) — free speedup over the local AUR builds") : ""), "info", {
                fix: Object.assign({label: qsTr("Switch")}, _pacmanFix(qsTr("Replaces each locally-built AUR package with the repo build of the same name (pacman -S). Versions may differ slightly; the packages themselves stay installed. caelestia++ and -debug packages are excluded."), [switchCmd]))
            });
        }

        if (onCachy && !(flags.kernel?.[0] ?? "").includes("cachyos"))
            push("cachy-kernel", qsTr("Not running a CachyOS kernel"), qsTr("Running %1 — the linux-cachyos kernel carries the scheduler and compiler tuning this distro is about. Install it and reboot into it (boot entries update automatically)").arg(flags.kernel?.[0] ?? "?"), "info");

        if (onCachy && parseInt(flags.cachyrepos?.[0] ?? "0", 10) === 0)
            push("cachy-repos", qsTr("CachyOS repositories missing from pacman.conf"), qsTr("All packages come from plain Arch repos — no optimized builds at all. Re-add them with the cachyos-repo script from the CachyOS wiki"), "warn");

        if (flags.dbage?.[0] === "stale")
            push("pacman-db", qsTr("Package databases older than two weeks"), qsTr("Sync databases have not been refreshed in 14+ days — installs pull outdated versions and fixes here may target stale packages"), "info", {
                fix: Object.assign({label: qsTr("Update system")}, _pacmanFix(qsTr("Runs a FULL system upgrade (pacman -Syu). This updates every package, can take a while, and is the only safe way to refresh the databases (a plain -Sy risks partial upgrades). Review the system afterwards."), ["pacman -Syu --noconfirm"]))
            });

        const pacnewCount = parseInt(flags.pacnew?.[0] ?? "0", 10);
        if (pacnewCount > 0)
            push("pacnew", qsTr("%1 unmerged .pacnew/.pacsave files").arg(pacnewCount), qsTr("%1— package updates shipped new default configs you have not merged; run `pacdiff` (pacman-contrib) in a terminal to review them. Merging is judgment work, so no automatic fix").arg(flags.pacnew?.[1] ?? ""), "warn");

        // -- System health extras
        const dumps = parseInt(flags.coredumps?.[0] ?? "0", 10);
        if (dumps > 0)
            push("coredumps", qsTr("%1 application crashes in the last 24h").arg(dumps), qsTr("Something is segfaulting — `coredumpctl list --since -24h` names it"), "info");

        if (parseInt(flags.swap?.[0] ?? "1", 10) === 0)
            push("swap", qsTr("No swap or zram configured"), qsTr("Under memory pressure the kernel OOM-kills apps instead of paging — install zram-generator for compressed in-RAM swap (the CachyOS default)"), "warn");

        const fstrimOn = flags.fstrim?.[0] === "1";
        const isSsd = flags.fstrim?.[1] === "1";
        if (isSsd && !fstrimOn)
            push("fstrim", qsTr("SSD TRIM timer disabled"), qsTr("Without weekly TRIM the SSD slows down as it fills and wears faster"), "warn", {
                fix: Object.assign({label: qsTr("Enable")}, _rootFix(qsTr("Staged: verifies the fstrim.timer unit exists (reinstalls util-linux if not), then enables the weekly TRIM timer. No data is touched."), [`bash '${Quickshell.shellDir}/system/repair/service.sh' --pkg util-linux fstrim.timer`]))
            });

        if (flags.rtkit?.[0] === "inactive")
            push("rtkit", qsTr("rtkit daemon not running"), qsTr("PipeWire cannot get realtime priority without it — audio crackles under load"), "info", {
                fix: Object.assign({label: qsTr("Enable")}, _rootFix(qsTr("Staged: installs the rtkit package if it is missing (the unit cannot exist without it), then enables and starts rtkit-daemon, which grants PipeWire realtime scheduling priority. Streams its progress here."), [`bash '${Quickshell.shellDir}/system/repair/service.sh' --pkg rtkit rtkit-daemon.service`]))
            });

        if (SysInfo.isLaptop && flags.governor?.[0] === "performance" && !MaxPerf.enabled)
            push("governor", qsTr("CPU governor pinned to performance"), qsTr("Max-perf is off but the governor is still \"performance\" — clocks stay high and the battery drains for nothing"), "info", {
                fix: Object.assign({label: qsTr("Rebalance")}, _userFix(qsTr("Sets the power profile back to balanced via powerprofilesctl, which restores the normal governor."), ["powerprofilesctl set balanced"]))
            });

        // Problems first, then untouchable info rows, healthy last
        const rank = {fail: 0, warn: 1, info: 2, ok: 3};
        rows.sort((a, b) => rank[a.status] - rank[b.status]);
        results = rows;
        lastScan = Qt.formatTime(new Date(), "hh:mm:ss");
        scanning = false;

        const freshPackages = missingPackages.filter(p => !dismissedPackages.includes(p));
        const freshHalves = outdatedRootHalves.filter(dir => (vers[dir]?.repo ?? 0) > (dismissedRootHalves[dir] ?? 0));
        if ((freshPackages.length || freshHalves.length) && !DebugConsole.open)
            promptOpen = true;
    }

    Process {
        id: probe

        stdout: StdioCollector {
            onStreamFinished: {
                root._probeOut = text;
                root._probeDone();
            }
        }
    }

    Process {
        id: doctorProbe

        stdout: StdioCollector {
            onStreamFinished: {
                root._doctorOut = text;
                root._probeDone();
            }
        }
        // A doctor crash leaves stdout empty and the row falls back to the
        // plain JSON-validity check; the traceback should still be findable
        stderr: SplitParser {
            onRead: data => console.warn("systemcheck configdoctor:", data)
        }
    }

    Process {
        id: fixProc

        stdout: SplitParser {
            onRead: data => {
                console.info("systemcheck fix:", data);
                root.fixLog.append({line: data});
            }
        }
        stderr: SplitParser {
            onRead: data => {
                console.warn("systemcheck fix:", data);
                root.fixLog.append({line: data});
            }
        }
        onExited: code => {
            root.busyId = "";
            watchdog.stop();
            if (root._fixCancelled) {
                root._fixCancelled = false;
                root.fixLog.append({line: qsTr("— cancelled by user —")});
                root.fixResult = {success: false, code: -1};
                Toaster.toast(qsTr("Fix cancelled"), qsTr("Nothing further was changed"), "block");
            } else if (code === 0) {
                root.fixResult = {success: true, code: 0};
                Toaster.toast(qsTr("Fix applied"), qsTr("Rescanning to verify"), "task_alt");
            } else {
                root.fixResult = {success: false, code: code};
                Toaster.toast(qsTr("Fix did not complete"), code === 126 || code === 127 ? qsTr("Authentication was dismissed or no polkit agent answered") : qsTr("Exited with code %1 — the log shows where it stopped").arg(code), "warning");
            }
            root.scan();
        }
    }

    // pkexec without a reachable polkit agent hangs forever and would leave
    // every fix button stuck on "Working" — kill runaway fixes after 10min
    // (long installs are fine, an unanswerable password prompt is not)
    Timer {
        id: watchdog

        running: root.busyId !== ""
        interval: 600000
        onTriggered: root.cancelRunningFix()
    }

    Process {
        id: ensureStateDir

        command: ["mkdir", "-p", Paths.state]
    }

    FileView {
        id: store

        path: root.statePath
        printErrors: false

        onLoaded: {
            try {
                const saved = JSON.parse(text());
                root.dismissedPackages = saved.dismissedPackages ?? [];
                root.dismissedRootHalves = saved.dismissedRootHalves ?? {};
            } catch (e) {}
        }
    }

    // First scan waits out the startup rush; the shell is fully up by then
    Timer {
        running: true
        interval: 15000
        onTriggered: root.scan()
    }

    Component.onCompleted: ensureStateDir.running = true

    IpcHandler {
        target: "systemcheck"

        function scan(): void {
            root.scan();
        }
        function status(): string {
            return root.results.map(r => `${r.status.toUpperCase()} ${r.name}`).join("\n");
        }
    }
}
