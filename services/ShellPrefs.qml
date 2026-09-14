pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io
import qs.utils

// Small shell-owned preferences that don't belong in the compiled config
// schema (shell.json). Persisted to state and hot-reloaded on change, same
// pattern as Features.
Singleton {
    id: root

    readonly property string statePath: `${Paths.state}/prefs.json`

    // Bar logo style: true = oversized distro logo capping the pill's left
    // end; false = regular-size logo inside the pill with normal rounding
    readonly property bool barLogoEndcap: props.barLogoEndcap

    // Bar logo: visibility, size (factor), endcap position nudge, custom image
    readonly property bool barLogoShow: props.barLogoShow
    readonly property real barLogoScale: props.barLogoScale
    readonly property int barLogoOffsetX: props.barLogoOffsetX
    readonly property int barLogoOffsetY: props.barLogoOffsetY
    readonly property string barLogoSource: props.barLogoSource

    // Caelestia++ ships without the centre active-window readout; stock
    // configs still carry the entry, so it stays filtered until enabled here
    readonly property bool barShowActiveWindow: props.barShowActiveWindow

    // Bar system monitor: per-metric visibility of the usage pill
    readonly property bool barShowCpu: props.barShowCpu
    readonly property bool barShowRam: props.barShowRam
    readonly property bool barShowGpu: props.barShowGpu

    // Liquid glass: strip the wallpaper's hue out of the surface colours so
    // panels read as clear frosted glass instead of tinted plastic. Accents
    // (primary, error, …) keep their colour — Apple tints the controls on
    // the material, never the material itself.
    readonly property bool glassNeutral: props.glassNeutral

    // How the glass material is mixed. `scrim` is the dark base that keeps
    // light text legible over a bright backdrop; `lift` is the illumination
    // that makes the pane read as glass rather than a smoked panel. Their
    // sum must stay above the compositor's ignore_alpha cutoff (0.1) or
    // Hyprland stops blurring behind the panel entirely.
    readonly property real glassScrim: props.glassScrim
    readonly property real glassLift: props.glassLift
    readonly property real glassRim: props.glassRim

    // Animated DNA background (shown when no image wallpaper is set)
    readonly property bool dnaEnabled: props.dnaEnabled
    readonly property bool dnaUseThemeColor: props.dnaUseThemeColor
    readonly property string dnaCustomColor: props.dnaCustomColor

    function setGlassScrim(value: real): void {
        if (value === props.glassScrim)
            return;
        props.glassScrim = value;
        save();
    }

    function setGlassLift(value: real): void {
        if (value === props.glassLift)
            return;
        props.glassLift = value;
        save();
    }

    function setGlassRim(value: real): void {
        if (value === props.glassRim)
            return;
        props.glassRim = value;
        save();
    }

    function setGlassNeutral(value: bool): void {
        if (value === props.glassNeutral)
            return;
        props.glassNeutral = value;
        save();
    }

    function setBarLogoEndcap(value: bool): void {
        if (value === props.barLogoEndcap)
            return;
        props.barLogoEndcap = value;
        save();
    }

    function setBarLogoShow(value: bool): void {
        if (value === props.barLogoShow)
            return;
        props.barLogoShow = value;
        save();
    }

    function setBarLogoScale(value: real): void {
        if (value === props.barLogoScale)
            return;
        props.barLogoScale = value;
        save();
    }

    function setBarLogoOffsetX(value: int): void {
        if (value === props.barLogoOffsetX)
            return;
        props.barLogoOffsetX = value;
        save();
    }

    function setBarLogoOffsetY(value: int): void {
        if (value === props.barLogoOffsetY)
            return;
        props.barLogoOffsetY = value;
        save();
    }

    function setBarLogoSource(value: string): void {
        if (value === props.barLogoSource)
            return;
        props.barLogoSource = value;
        save();
    }

    function setBarShowActiveWindow(value: bool): void {
        if (value === props.barShowActiveWindow)
            return;
        props.barShowActiveWindow = value;
        save();
    }

    function setBarShowCpu(value: bool): void {
        if (value === props.barShowCpu)
            return;
        props.barShowCpu = value;
        save();
    }

    function setBarShowRam(value: bool): void {
        if (value === props.barShowRam)
            return;
        props.barShowRam = value;
        save();
    }

    function setBarShowGpu(value: bool): void {
        if (value === props.barShowGpu)
            return;
        props.barShowGpu = value;
        save();
    }

    function setDnaEnabled(value: bool): void {
        if (value === props.dnaEnabled)
            return;
        props.dnaEnabled = value;
        save();
    }

    function setDnaUseThemeColor(value: bool): void {
        if (value === props.dnaUseThemeColor)
            return;
        props.dnaUseThemeColor = value;
        save();
    }

    function setDnaCustomColor(value: string): void {
        if (value === props.dnaCustomColor)
            return;
        props.dnaCustomColor = value;
        save();
    }

    function save(): void {
        store.setText(JSON.stringify({
            barLogoEndcap: props.barLogoEndcap,
            barLogoShow: props.barLogoShow,
            barLogoScale: props.barLogoScale,
            barLogoOffsetX: props.barLogoOffsetX,
            barLogoOffsetY: props.barLogoOffsetY,
            barLogoSource: props.barLogoSource,
            barShowActiveWindow: props.barShowActiveWindow,
            barShowCpu: props.barShowCpu,
            barShowRam: props.barShowRam,
            barShowGpu: props.barShowGpu,
            dnaEnabled: props.dnaEnabled,
            dnaUseThemeColor: props.dnaUseThemeColor,
            dnaCustomColor: props.dnaCustomColor
        }, null, 2) + "\n");
    }

    QtObject {
        id: props

        property bool barLogoEndcap: true
        property bool barLogoShow: true
        property real barLogoScale: 1.0
        property int barLogoOffsetX: 0
        property int barLogoOffsetY: 0
        property string barLogoSource: ""
        property bool barShowActiveWindow: false
        property bool barShowCpu: true
        property bool barShowRam: true
        property bool barShowGpu: true
        property bool glassNeutral: true
        property real glassScrim: 0.0
        property real glassLift: 0.20
        property real glassRim: 1.0
        property bool dnaEnabled: true
        property bool dnaUseThemeColor: true
        property string dnaCustomColor: "#ff5449"
    }

    FileView {
        id: store

        path: root.statePath
        watchChanges: true
        printErrors: false

        onLoaded: {
            let saved;
            try {
                saved = JSON.parse(text());
            } catch (e) {
                return;
            }
            props.barLogoEndcap = saved.barLogoEndcap ?? true;
            props.barLogoShow = saved.barLogoShow ?? true;
            props.barLogoScale = saved.barLogoScale ?? 1.0;
            props.barLogoOffsetX = saved.barLogoOffsetX ?? 0;
            props.barLogoOffsetY = saved.barLogoOffsetY ?? 0;
            props.barLogoSource = saved.barLogoSource ?? "";
            props.barShowActiveWindow = saved.barShowActiveWindow ?? false;
            props.barShowCpu = saved.barShowCpu ?? true;
            props.barShowRam = saved.barShowRam ?? true;
            props.barShowGpu = saved.barShowGpu ?? true;
            props.dnaEnabled = saved.dnaEnabled ?? true;
            props.dnaUseThemeColor = saved.dnaUseThemeColor ?? true;
            props.dnaCustomColor = saved.dnaCustomColor ?? "#ff5449";
        }
        onFileChanged: reload()
    }
}
