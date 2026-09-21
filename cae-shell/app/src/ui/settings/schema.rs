//! The pages that are only a list of settings, written down as one.
//!
//! Most of what can be set is a switch, a number or a word, kept under a key.
//! Pages of those are data: what the row is called, where its value lives and
//! what it is when the file says nothing. One view draws all of them. A page
//! that does more than that (a network to join, a monitor to drag) is a view
//! of its own under `pages`.
//!
//! The default beside each key has to be the one whatever reads that key
//! falls back to, or a switch would show a state the desktop is not in. For
//! what the QML shell reads those are its plugin's; for what this shell reads
//! they are `cae_core`'s, and `tests` holds the two together.

use super::Page;
use super::store::{Source, Store, knob, prefs, shell};

/// A value a choice can stand for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pick {
    Flag(bool),
    Word(&'static str),
}

impl Pick {
    pub fn to_json(self) -> serde_json::Value {
        match self {
            Pick::Flag(flag) => flag.into(),
            Pick::Word(word) => word.into(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Control {
    Switch { otherwise: bool },
    /// A number between limits. What is kept and what is shown can differ by
    /// a factor: a volume step is kept as 0.1 and is ten per cent to a person.
    Stepper { otherwise: f64, from: f64, to: f64, step: f64, shown_times: f64, unit: &'static str },
    /// One of a few named values, all of them in view.
    Choice { otherwise: usize, options: &'static [(&'static str, Pick)] },
    Text { placeholder: &'static str },
}

#[derive(Clone, Copy, Debug)]
pub struct Row {
    pub label: &'static str,
    pub note: &'static str,
    pub at: Source,
    pub control: Control,
    /// A switch this row means nothing without, and what that switch is when
    /// the file does not say.
    pub needs: Option<(Source, bool)>,
}

impl Row {
    const fn noted(mut self, note: &'static str) -> Row {
        self.note = note;
        self
    }

    const fn needs(mut self, switch: Source, otherwise: bool) -> Row {
        self.needs = Some((switch, otherwise));
        self
    }
}

const fn switch(label: &'static str, at: Source, otherwise: bool) -> Item {
    Item::Row(Row { label, note: "", at, control: Control::Switch { otherwise }, needs: None })
}

const fn stepper(label: &'static str, at: Source, otherwise: f64, (from, to, step): (f64, f64, f64), unit: &'static str) -> Item {
    let control = Control::Stepper { otherwise, from, to, step, shown_times: 1., unit };
    Item::Row(Row { label, note: "", at, control, needs: None })
}

/// A stepper whose kept value is the shown one divided by `shown_times`.
const fn scaled(
    label: &'static str,
    at: Source,
    otherwise: f64,
    (from, to, step): (f64, f64, f64),
    shown_times: f64,
    unit: &'static str,
) -> Item {
    let control = Control::Stepper { otherwise, from, to, step, shown_times, unit };
    Item::Row(Row { label, note: "", at, control, needs: None })
}

const fn choice(label: &'static str, at: Source, otherwise: usize, options: &'static [(&'static str, Pick)]) -> Item {
    Item::Row(Row { label, note: "", at, control: Control::Choice { otherwise, options }, needs: None })
}

const fn text(label: &'static str, at: Source, placeholder: &'static str) -> Item {
    Item::Row(Row { label, note: "", at, control: Control::Text { placeholder }, needs: None })
}

pub enum Item {
    Row(Row),
    /// A way through to another page, saying what will be found there. The
    /// page is made when it is wanted: some pages are named by something
    /// found while running, and a table written at compile time cannot hold
    /// a type that might.
    Leads { glyph: &'static str, label: &'static str, to: fn() -> Page, says: fn(&Store) -> String },
}

impl Item {
    const fn noted(self, note: &'static str) -> Item {
        match self {
            Item::Row(row) => Item::Row(row.noted(note)),
            leads => leads,
        }
    }

    const fn needs(self, switch: Source, otherwise: bool) -> Item {
        match self {
            Item::Row(row) => Item::Row(row.needs(switch, otherwise)),
            leads => leads,
        }
    }
}

pub struct Section {
    /// Nothing, for a page that is one list and needs no heading over it.
    pub title: &'static str,
    pub items: &'static [Item],
}

fn on_or_off(store: &Store, at: Source, otherwise: bool) -> String {
    if store.flag(at, otherwise) { "On" } else { "Off" }.to_string()
}

const UNITS: &[(&str, Pick)] = &[("°C", Pick::Flag(false)), ("°F", Pick::Flag(true))];

/// The sections of `page`, when it is one of the pages that is only a list.
pub fn sections(page: &Page) -> Option<&'static [Section]> {
    Some(match page {
        Page::Panels => PANELS,
        Page::Compositor => COMPOSITOR,
        Page::Dashboard => DASHBOARD,
        Page::BarWorkspaces => BAR_WORKSPACES,
        Page::BarStatus => BAR_STATUS,
        Page::Launcher => LAUNCHER,
        Page::Sidebar => SIDEBAR,
        Page::Services => SERVICES,
        Page::Notifications => NOTIFICATIONS,
        Page::Region => REGION,
        Page::WindowLooks => WINDOW_LOOKS,
        Page::Gaps => GAPS,
        Page::Input => INPUT,
        Page::LaunchedApps => LAUNCHED_APPS,
        Page::Keys => KEYS,
        _ => return None,
    })
}

const PANELS: &[Section] = &[Section {
    title: "",
    items: &[
        Item::Leads { glyph: "dock_to_bottom", label: "Bar", to: || Page::Bar, says: |_| "What it carries, workspaces, status icons".to_string() },
        Item::Leads { glyph: "apps", label: "Launcher", to: || Page::Launcher, says: |_| "Results, keys, commands".to_string() },
        Item::Leads {
            glyph: "dashboard",
            label: "Dashboard",
            to: || Page::Dashboard,
            says: |store| on_or_off(store, shell("dashboard.enabled"), true),
        },
        Item::Leads {
            glyph: "dock_to_right",
            label: "Sidebar",
            to: || Page::Sidebar,
            says: |store| on_or_off(store, shell("sidebar.enabled"), true),
        },
    ],
}];

const DASHBOARD: &[Section] = &[
    Section {
        title: "",
        items: &[
            switch("Enabled", shell("dashboard.enabled"), true),
            switch("Show on hover", shell("dashboard.showOnHover"), true)
                .noted("Opens when the pointer reaches the foot of the screen, at the left")
                .needs(shell("dashboard.enabled"), true),
        ],
    },
    Section {
        title: "Tabs",
        items: &[
            switch("Dashboard", shell("dashboard.showDashboard"), true),
            switch("Media", shell("dashboard.showMedia"), true),
            switch("Performance", shell("dashboard.showPerformance"), true),
            switch("Weather", shell("dashboard.showWeather"), true),
        ],
    },
    Section {
        title: "On the performance tab",
        items: &[
            switch("Battery", shell("dashboard.performance.showBattery"), true),
            switch("GPU", shell("dashboard.performance.showGpu"), true),
            switch("CPU", shell("dashboard.performance.showCpu"), true),
            switch("Memory", shell("dashboard.performance.showMemory"), true),
            switch("Storage", shell("dashboard.performance.showStorage"), true),
            switch("Network", shell("dashboard.performance.showNetwork"), true),
        ],
    },
];

/// The nine places the desktop clock can sit, in the order they are read:
/// down the screen, then across it. The last is where it is by default.
const CLOCK_SPOTS: &[(&str, Pick)] = &[
    ("Top left", Pick::Word("top-left")),
    ("Top centre", Pick::Word("top-center")),
    ("Top right", Pick::Word("top-right")),
    ("Middle left", Pick::Word("middle-left")),
    ("Middle", Pick::Word("middle-center")),
    ("Middle right", Pick::Word("middle-right")),
    ("Bottom left", Pick::Word("bottom-left")),
    ("Bottom centre", Pick::Word("bottom-center")),
    ("Bottom right", Pick::Word("bottom-right")),
];

/// What the page about how the desktop looks lists under the wallpaper.
pub const LOOK: &[Section] = &[
    Section {
        title: "",
        items: &[
            switch("Show the wallpaper", shell("background.wallpaperEnabled"), true),
            switch("Transparency", shell("appearance.transparency.enabled"), false).noted("Panels let what is behind them through"),
        ],
    },
    Section {
        title: "Desktop clock",
        items: &[
            switch("Show it", shell("background.desktopClock.enabled"), false).noted("The time on the wallpaper, behind everything that is worked in"),
            choice("Where", shell("background.desktopClock.position"), 8, CLOCK_SPOTS).needs(shell("background.desktopClock.enabled"), false),
            scaled("Size", shell("background.desktopClock.scale"), 1., (40., 400., 10.), 100., "%")
                .needs(shell("background.desktopClock.enabled"), false),
            switch("Plate behind it", shell("background.desktopClock.background.enabled"), true)
                .noted("What keeps it readable over a bright wallpaper")
                .needs(shell("background.desktopClock.enabled"), false),
            scaled("How solid the plate is", shell("background.desktopClock.background.opacity"), 0.7, (10., 100., 5.), 100., "%")
                .needs(shell("background.desktopClock.background.enabled"), true),
            switch("Shadow under the plate", shell("background.desktopClock.shadow.enabled"), true)
                .needs(shell("background.desktopClock.background.enabled"), true),
        ],
    },
    Section {
        title: "With no wallpaper",
        items: &[
            switch("DNA helix", prefs("dnaEnabled"), true).noted("Something that moves, drawn behind the windows"),
            switch("In the scheme's colour", prefs("dnaUseThemeColor"), true).needs(prefs("dnaEnabled"), true),
            text("Its own colour", prefs("dnaCustomColor"), "#ff5449")
                .noted("As hexadecimal. Used when it is not in the scheme's")
                .needs(prefs("dnaEnabled"), true),
        ],
    },
];

/// What the bar page lists under what the bar carries.
pub const BAR: &[Section] = &[
    Section {
        title: "",
        items: &[
            switch("Logo", prefs("barLogoShow"), true),
            switch("Window title", prefs("barShowActiveWindow"), false).noted("What the focused window calls itself, in the middle of the bar"),
        ],
    },
    Section {
        title: "System monitor",
        items: &[
            switch("CPU", prefs("barShowCpu"), true),
            switch("Memory", prefs("barShowRam"), true),
            switch("GPU", prefs("barShowGpu"), true),
        ],
    },
    Section {
        title: "",
        items: &[
            Item::Leads { glyph: "workspaces", label: "Workspaces", to: || Page::BarWorkspaces, says: |_| "How many, the marker, labels".to_string() },
            Item::Leads { glyph: "signal_cellular_alt", label: "Status icons", to: || Page::BarStatus, says: |_| "Which are shown".to_string() },
        ],
    },
];

const BAR_WORKSPACES: &[Section] = &[
    Section {
        title: "",
        items: &[
            stepper("Workspaces shown", shell("bar.workspaces.shown"), 5., (1., 20., 1.), ""),
            switch("Track behind occupied workspaces", shell("bar.workspaces.occupiedBg"), true),
            switch("Trail behind the marker", shell("bar.workspaces.activeTrail"), true)
                .noted("The marker stretches as it travels instead of jumping"),
            switch("Window icons", shell("bar.workspaces.showWindows"), false)
                .noted("An icon for each window open on a workspace"),
            switch("Per-monitor workspaces", shell("bar.workspaces.perMonitorWorkspaces"), false)
                .noted("Each screen's bar shows only the workspaces on that screen"),
        ],
    },
    Section {
        title: "Labels",
        items: &[
            text("Every workspace", shell("bar.workspaces.label"), "The number").noted("Text drawn in place of the number"),
            text("Occupied", shell("bar.workspaces.occupiedLabel"), "The number"),
            text("Focused", shell("bar.workspaces.activeLabel"), "The number"),
        ],
    },
];

const BAR_STATUS: &[Section] = &[Section {
    title: "",
    items: &[
        switch("Network", shell("bar.status.showNetwork"), true),
        switch("Bluetooth", shell("bar.status.showBluetooth"), true),
        switch("Battery", shell("bar.status.showBattery"), true),
        switch("Speakers", shell("bar.status.showAudio"), false),
        switch("Microphone", shell("bar.status.showMicrophone"), false),
        switch("Keyboard layout", shell("bar.status.showKbLayout"), false),
        switch("Caps lock", shell("bar.status.showLockStatus"), false),
    ],
}];

const LAUNCHER: &[Section] = &[
    Section {
        title: "",
        items: &[
            stepper("Results shown", shell("launcher.maxShown"), 8., (1., 20., 1.), "").noted("More than this and the list scrolls"),
            stepper("Wallpapers shown", shell("launcher.maxWallpapers"), 9., (1., 30., 1.), ""),
        ],
    },
    Section {
        title: "Keys and commands",
        items: &[
            switch("Vim keys", shell("launcher.vimKeybinds"), true).noted("Ctrl+J and Ctrl+K walk the results"),
            text("Command prefix", shell("launcher.actionPrefix"), ">").noted("What turns a search into a command"),
            switch("Dangerous commands", shell("launcher.enableDangerousActions"), false)
                .noted("Lets the launcher shut down, reboot and log out"),
        ],
    },
];

const SIDEBAR: &[Section] = &[Section {
    title: "",
    items: &[
        switch("Enabled", shell("sidebar.enabled"), true),
        stepper("Drag threshold", shell("sidebar.dragThreshold"), 80., (0., 200., 5.), "px")
            .noted("How far a drag has to go before it opens")
            .needs(shell("sidebar.enabled"), true),
    ],
}];

const SERVICES: &[Section] = &[
    Section {
        title: "",
        items: &[Item::Leads {
            glyph: "notifications",
            label: "Notifications",
            to: || Page::Notifications,
            says: |_| "Timeouts, fullscreen, toasts".to_string(),
        }],
    },
    Section {
        title: "How often things are read",
        items: &[
            stepper("Media position", shell("dashboard.mediaUpdateInterval"), 500., (100., 2000., 50.), "ms"),
            scaled("System monitor", shell("dashboard.resourceUpdateInterval"), 1000., (0.5, 10., 0.5), 0.001, "s")
                .noted("CPU, memory and GPU on the dashboard"),
        ],
    },
    Section {
        title: "Media",
        items: &[choice(
            "Lyrics from",
            shell("services.lyricsBackend"),
            0,
            &[("Auto", Pick::Word("Auto")), ("Local", Pick::Word("Local")), ("LRCLIB", Pick::Word("LRCLIB")), ("NetEase", Pick::Word("NetEase"))],
        )],
    },
    Section {
        title: "Steps",
        items: &[
            scaled("Volume step", shell("services.audioIncrement"), 0.1, (1., 50., 1.), 100., "%").noted("How much one scroll changes it"),
            scaled("Brightness step", shell("services.brightnessIncrement"), 0.1, (1., 50., 1.), 100., "%"),
            scaled("Loudest volume", shell("services.maxVolume"), 1., (50., 200., 5.), 100., "%"),
        ],
    },
    Section {
        title: "Tuning",
        items: &[
            stepper("Visualiser bars", shell("services.visualiserBars"), 60., (10., 120., 2.), ""),
            switch("Scheme from the wallpaper", shell("services.smartScheme"), true)
                .noted("Light or dark, and which variant, follow the picture"),
            choice(
                "GPU to monitor",
                shell("services.gpuType"),
                0,
                &[("Auto", Pick::Word("")), ("NVIDIA", Pick::Word("NVIDIA")), ("Generic", Pick::Word("GENERIC")), ("None", Pick::Word("None"))],
            ),
        ],
    },
];

const FULLSCREEN_TOASTS: &[(&str, Pick)] =
    &[("Never", Pick::Word("off")), ("Important", Pick::Word("important")), ("Always", Pick::Word("all"))];

const NOTIFICATIONS: &[Section] = &[
    Section {
        title: "",
        items: &[
            choice("Over fullscreen windows", shell("notifs.fullscreen"), 1, &[("Hidden", Pick::Word("off")), ("Shown", Pick::Word("on"))]),
            switch("Leave by themselves", shell("notifs.expire"), true)
                .noted("Off, a notification stays until it is dismissed"),
            stepper("Stay for", shell("notifs.defaultExpireTimeout"), 5000., (1000., 60000., 500.), "ms")
                .noted("When the sender does not say")
                .needs(shell("notifs.expire"), true),
            switch("Open unfolded", shell("notifs.openExpanded"), false),
            stepper("Shown per group", shell("notifs.groupPreviewNum"), 3., (1., 10., 1.), "").noted("The rest fold away under them"),
            switch("Click presses the only action", shell("notifs.actionOnClick"), false),
        ],
    },
    Section {
        title: "Shell messages",
        items: &[
            choice("Over fullscreen windows", shell("utilities.toasts.fullscreen"), 0, FULLSCREEN_TOASTS),
            stepper("At once", shell("utilities.maxToasts"), 4., (1., 10., 1.), ""),
        ],
    },
    Section {
        title: "Say when",
        items: &[
            switch("Charging starts or stops", shell("utilities.toasts.chargingChanged"), true),
            switch("Game mode changes", shell("utilities.toasts.gameModeChanged"), true),
            switch("Do not disturb changes", shell("utilities.toasts.dndChanged"), true),
            switch("The audio output changes", shell("utilities.toasts.audioOutputChanged"), true),
            switch("The audio input changes", shell("utilities.toasts.audioInputChanged"), true),
            switch("Caps lock changes", shell("utilities.toasts.capsLockChanged"), true),
            switch("Num lock changes", shell("utilities.toasts.numLockChanged"), true),
            switch("The keyboard layout changes", shell("utilities.toasts.kbLayoutChanged"), true),
            switch("The VPN connects or drops", shell("utilities.toasts.vpnChanged"), true),
            switch("A new track starts", shell("utilities.toasts.nowPlaying"), false),
        ],
    },
];

const REGION: &[Section] = &[
    Section {
        title: "Units",
        items: &[
            choice("Weather", shell("services.useFahrenheit"), 0, UNITS),
            choice("CPU and GPU temperatures", shell("services.useFahrenheitPerformance"), 0, UNITS),
        ],
    },
    Section {
        title: "Time",
        items: &[choice(
            "Clock",
            shell("services.useTwelveHourClock"),
            0,
            &[("24-hour", Pick::Flag(false)), ("12-hour", Pick::Flag(true))],
        )],
    },
];

const COMPOSITOR: &[Section] = &[Section {
    title: "",
    items: &[
        Item::Leads {
            glyph: "blur_on",
            label: "Blur, shadows and windows",
            to: || Page::WindowLooks,
            says: |store| format!("Blur {}", if store.flag(knob("blurEnabled"), false) { "on" } else { "off" }),
        },
        Item::Leads {
            glyph: "space_dashboard",
            label: "Gaps",
            to: || Page::Gaps,
            says: |store| {
                format!("{} px between windows, {} px at the edges", store.number(knob("windowGapsIn"), 5.), store.number(knob("windowGapsOut"), 10.))
            },
        },
        Item::Leads {
            glyph: "touchpad_mouse",
            label: "Input and gestures",
            to: || Page::Input,
            says: |_| "Touchpad, gestures, volume keys, cursor".to_string(),
        },
        Item::Leads { glyph: "apps", label: "Apps it starts", to: || Page::LaunchedApps, says: |store| store.text(knob("terminal"), "") },
        Item::Leads { glyph: "keyboard", label: "Keybinds", to: || Page::Keys, says: |_| "Workspaces, windows, apps, the shell".to_string() },
        Item::Leads { glyph: "keyboard_command_key", label: "Your own keybinds", to: || Page::CustomKeys, says: |_| "Keys that run something of yours".to_string() },
        Item::Leads { glyph: "monitor", label: "Screens", to: || Page::Monitors, says: |_| "Where each stands, resolution, scale".to_string() },
        Item::Leads { glyph: "manufacturing", label: "Every option", to: || Page::AllOptions, says: |_| "All of Hyprland's, by name".to_string() },
    ],
}];

const WINDOW_LOOKS: &[Section] = &[
    Section {
        title: "Blur",
        items: &[
            switch("Enabled", knob("blurEnabled"), false).noted("Behind translucent windows. Turns itself off on battery"),
            switch("Popups", knob("blurPopups"), false).needs(knob("blurEnabled"), false),
            switch("Input methods", knob("blurInputMethods"), false).needs(knob("blurEnabled"), false),
            switch("Special workspace", knob("blurSpecialWs"), false).needs(knob("blurEnabled"), false),
            switch("X-ray", knob("blurXray"), false)
                .noted("Blurs only what is directly beneath. Cheaper, and less true")
                .needs(knob("blurEnabled"), false),
            stepper("Size", knob("blurSize"), 5., (1., 15., 1.), "").needs(knob("blurEnabled"), false),
            stepper("Passes", knob("blurPasses"), 2., (1., 6., 1.), "")
                .noted("More is smoother, and costs more")
                .needs(knob("blurEnabled"), false),
        ],
    },
    Section {
        title: "Shadows",
        items: &[
            switch("Enabled", knob("shadowEnabled"), false),
            stepper("Range", knob("shadowRange"), 15., (0., 40., 1.), "px").needs(knob("shadowEnabled"), false),
            stepper("Falloff", knob("shadowRenderPower"), 4., (1., 4., 1.), "").needs(knob("shadowEnabled"), false),
        ],
    },
    Section {
        title: "Windows",
        items: &[
            stepper("Corner rounding", knob("windowRounding"), 15., (0., 30., 1.), "px"),
            scaled("Opacity", knob("windowOpacity"), 1., (50., 100., 1.), 100., "%").noted("Applies when the compositor reloads"),
            stepper("Border", knob("windowBorderSize"), 2., (0., 10., 1.), "px"),
        ],
    },
];

const GAPS: &[Section] = &[Section {
    title: "",
    items: &[
        stepper("Between windows", knob("windowGapsIn"), 5., (0., 50., 1.), "px"),
        stepper("At the screen's edges", knob("windowGapsOut"), 10., (0., 50., 1.), "px"),
        stepper("At the edges, with one window", knob("singleWindowGapsOut"), 20., (0., 60., 1.), "px")
            .noted("Applies when the compositor reloads"),
        stepper("Between workspaces", knob("workspaceGaps"), 20., (0., 100., 1.), "px").noted("Seen while sliding from one to the next"),
    ],
}];

const INPUT: &[Section] = &[
    Section {
        title: "Touchpad",
        items: &[
            switch("Off while typing", knob("touchpadDisableTyping"), true),
            scaled("Scroll speed", knob("touchpadScrollFactor"), 0.3, (5., 100., 5.), 100., "%"),
        ],
    },
    Section {
        title: "Gestures",
        items: &[
            stepper("Fingers", knob("gestureFingers"), 3., (3., 5., 1.), "")
                .noted("For the special workspace and window gestures. Applies on reload"),
            stepper("Fingers to change workspace", knob("workspaceSwipeFingers"), 4., (3., 5., 1.), "").noted("Applies on reload"),
            stepper("Fingers to sleep", knob("gestureFingersMore"), 4., (3., 5., 1.), "").noted("A swipe down. Applies on reload"),
            text("Sleep gesture runs", knob("sleepGestureCmd"), "systemctl suspend"),
        ],
    },
    Section {
        title: "Volume keys",
        items: &[
            stepper("Step", knob("volumeStep"), 10., (1., 25., 1.), "%").noted("Applies on reload"),
            stepper("Limit", knob("volumeMax"), 100., (100., 150., 10.), "%"),
        ],
    },
    Section {
        title: "Cursor",
        items: &[text("Theme", knob("cursorTheme"), "Adwaita"), stepper("Size", knob("cursorSize"), 24., (16., 48., 4.), "px")],
    },
];

const LAUNCHED_APPS: &[Section] = &[Section {
    title: "What the compositor's keybinds and gestures start. Applies on reload",
    items: &[
        text("Terminal", knob("terminal"), "kitty"),
        text("Browser", knob("browser"), "firefox"),
        text("Editor", knob("editor"), "code"),
        text("File manager", knob("fileExplorer"), "thunar"),
        text("Audio mixer", knob("audioSettings"), "pavucontrol"),
    ],
}];

const fn key(label: &'static str, name: &'static str) -> Item {
    text(label, knob(name), "SUPER + T")
}

const KEYS: &[Section] = &[
    Section {
        title: "Workspaces",
        items: &[
            key("Go to workspace", "kbGoToWs"),
            key("Go to workspace group", "kbGoToWsGroup"),
            key("Move window to workspace", "kbMoveWinToWs"),
            key("Move window to workspace group", "kbMoveWinToWsGroup"),
            key("Next workspace", "kbNextWs"),
            key("Previous workspace", "kbPrevWs"),
        ],
    },
    Section {
        title: "Windows",
        items: &[
            key("Move", "kbMoveWindow"),
            key("Resize", "kbResizeWindow"),
            key("Close", "kbCloseWindow"),
            key("Fullscreen", "kbWindowFullscreen"),
            key("Fullscreen with borders", "kbWindowBorderedFullscreen"),
            key("Float", "kbToggleWindowFloating"),
            key("Pin", "kbPinWindow"),
            key("Picture in picture", "kbWindowPip"),
        ],
    },
    Section {
        title: "Window groups",
        items: &[
            key("Group", "kbToggleGroup"),
            key("Ungroup", "kbUngroup"),
            key("Next in group", "kbWindowGroupCycleNext"),
            key("Previous in group", "kbWindowGroupCyclePrev"),
        ],
    },
    Section {
        title: "Special workspaces",
        items: &[
            key("Special workspace", "kbSpecialWs"),
            key("System monitor", "kbSystemMonitorWs"),
            key("Music", "kbMusicWs"),
            key("Communication", "kbCommunicationWs"),
            key("To do", "kbTodoWs"),
        ],
    },
    Section {
        title: "Apps",
        items: &[
            key("Terminal", "kbTerminal"),
            key("Browser", "kbBrowser"),
            key("Editor", "kbEditor"),
            key("File manager", "kbFileExplorer"),
        ],
    },
    Section {
        title: "Shell",
        items: &[
            key("Session menu", "kbSession"),
            key("Sidebar", "kbShowSidebar"),
            key("Panels", "kbShowPanels"),
            key("Clear notifications", "kbClearNotifs"),
            key("Lock", "kbLock"),
            key("Restore the lock screen", "kbRestoreLock"),
        ],
    },
];

/// A row the search can find: the row, the page it is on, and the heading it
/// is under there, which is what tells one "Enabled" from another.
pub struct Listed {
    pub row: &'static Row,
    pub page: Page,
    pub under: &'static str,
}

/// Every row on every list page, for the search.
pub fn every_row() -> impl Iterator<Item = Listed> {
    LISTED.iter().flat_map(|page| {
        let listed = match page {
            Page::Bar => Some(BAR),
            Page::Look => Some(LOOK),
            page => sections(page),
        };
        let sections = listed.into_iter().flatten();
        sections.flat_map(move |section| {
            section.items.iter().filter_map(move |item| match item {
                Item::Row(row) => Some(Listed { row, page: page.clone(), under: section.title }),
                Item::Leads { .. } => None,
            })
        })
    })
}

/// The pages with rows of their own to find. Two are views of their own
/// with a list in them, and are here by hand.
const LISTED: &[Page] = &[
    Page::Look,
    Page::Bar,
    Page::Dashboard,
    Page::BarWorkspaces,
    Page::BarStatus,
    Page::Launcher,
    Page::Sidebar,
    Page::Services,
    Page::Notifications,
    Page::Region,
    Page::WindowLooks,
    Page::Gaps,
    Page::Input,
    Page::LaunchedApps,
    Page::Keys,
];

#[cfg(test)]
mod tests {
    use cae_core::launcher::config::Launcher;
    use cae_core::logo::{BarConfig, NotifsConfig, Status};

    use super::*;

    fn default_of(path: &'static str) -> Control {
        every_row().find(|listed| listed.row.at == shell(path)).unwrap_or_else(|| panic!("no row for {path}")).row.control
    }

    fn switch_default(path: &'static str) -> bool {
        match default_of(path) {
            Control::Switch { otherwise } => otherwise,
            other => panic!("{path} is {other:?}, not a switch"),
        }
    }

    fn stepper_default(path: &'static str) -> f64 {
        match default_of(path) {
            Control::Stepper { otherwise, .. } => otherwise,
            other => panic!("{path} is {other:?}, not a stepper"),
        }
    }

    /// A switch nobody has touched shows its default. If that is not the
    /// default the bar itself falls back to, the switch is lying.
    #[test]
    fn what_the_bar_reads_defaults_to_what_the_bar_falls_back_to() {
        let (bar, status) = (BarConfig::default(), Status::default());
        assert_eq!(stepper_default("bar.workspaces.shown"), bar.shown as f64);
        assert_eq!(switch_default("bar.workspaces.occupiedBg"), bar.occupied_bg);
        assert_eq!(switch_default("bar.workspaces.activeTrail"), bar.active_trail);
        assert_eq!(switch_default("bar.workspaces.showWindows"), bar.show_windows);
        assert_eq!(switch_default("bar.workspaces.perMonitorWorkspaces"), bar.per_monitor);

        assert_eq!(switch_default("bar.status.showNetwork"), status.show_network);
        assert_eq!(switch_default("bar.status.showBluetooth"), status.show_bluetooth);
        assert_eq!(switch_default("bar.status.showBattery"), status.show_battery);
        assert_eq!(switch_default("bar.status.showAudio"), status.show_audio);
        assert_eq!(switch_default("bar.status.showMicrophone"), status.show_microphone);
        assert_eq!(switch_default("bar.status.showKbLayout"), status.show_kb_layout);
        assert_eq!(switch_default("bar.status.showLockStatus"), status.show_lock_status);
    }

    #[test]
    fn what_the_notification_server_reads_defaults_the_same() {
        let notifs = NotifsConfig::default();
        assert_eq!(switch_default("notifs.expire"), notifs.expire);
        assert_eq!(stepper_default("notifs.defaultExpireTimeout"), notifs.default_expire_timeout as f64);
        assert_eq!(stepper_default("notifs.groupPreviewNum"), notifs.group_preview_num as f64);
        assert_eq!(switch_default("notifs.openExpanded"), notifs.open_expanded);
        assert_eq!(switch_default("notifs.actionOnClick"), notifs.action_on_click);
    }

    #[test]
    fn what_the_launcher_reads_defaults_the_same() {
        let launcher = Launcher::default();
        assert_eq!(stepper_default("launcher.maxShown"), launcher.max_shown as f64);
        assert_eq!(stepper_default("launcher.maxWallpapers"), launcher.max_wallpapers as f64);
        assert_eq!(switch_default("launcher.vimKeybinds"), launcher.vim_keybinds);
        assert_eq!(switch_default("launcher.enableDangerousActions"), launcher.enable_dangerous_actions);
    }

    #[test]
    fn every_stepper_starts_inside_its_own_limits() {
        for Listed { row, page, .. } in every_row() {
            if let Control::Stepper { otherwise, from, to, step, shown_times, .. } = row.control {
                let shown = otherwise * shown_times;
                assert!(from <= shown && shown <= to, "{page:?}: {} starts at {shown}, outside {from} to {to}", row.label);
                assert!(step > 0., "{page:?}: {} never moves", row.label);
            }
        }
    }

    #[test]
    fn every_choice_starts_on_one_of_its_options() {
        for Listed { row, page, .. } in every_row() {
            if let Control::Choice { otherwise, options } = row.control {
                assert!(otherwise < options.len(), "{page:?}: {} starts on an option it does not have", row.label);
            }
        }
    }

    #[test]
    fn every_page_that_is_searched_has_rows_to_find() {
        for page in LISTED {
            assert!(every_row().any(|listed| listed.page == *page), "{page:?} is listed and has no rows");
        }
    }
}
