import QtQuick
import Quickshell
import Quickshell.Services.UPower
import qs.services

// Procedural animated DNA wallpaper (assets/shaders/dna.frag).
// Cost control: the shader renders into a half-resolution layer texture
// (4x fewer fragments) and only redraws when the clock timer ticks —
// 30 fps on AC, 15 fps on battery — and not at all while the desktop is
// covered, which is most of the time on a working machine.
Item {
    id: root

    required property ShellScreen screen

    // A layer-shell background surface stays `visible` in QML terms even when
    // every pixel of it is behind a window, so `visible` alone kept the shader
    // running at 30 fps against an occluded surface: ~11% CPU rendering pixels
    // nobody can see. Same test the desktop visualiser and clock already use —
    // any tiled window means the desktop is effectively covered.
    readonly property bool desktopVisible: Hypr.monitorFor(screen)?.activeWorkspace?.toplevels?.values.every(t => t.lastIpcObject?.floating) ?? true

    // Wrap keeps float32 phase math precise across long uptimes. This value
    // is a whole number of cycles for both helix speeds (0.35t and 0.21t),
    // so the loop point is seamless: 2*PI*50/0.35.
    readonly property real timeWrap: 897.5979010256552
    // Animation time actually shown, accumulated per frame rather than derived
    // from wall clock: pausing while covered must not fast-forward the helix,
    // or uncovering the desktop snaps it to a different phase.
    property real elapsed: 0
    property real lastTickMs: 0

    // One emission per rendered wallpaper frame; consumers that must redraw
    // in lockstep (the desktop clock's glass grab) listen to this instead of
    // running their own timer — two unsynced 30 fps timers made the window
    // render at 60 fps
    signal frameAdvanced()

    // Accent trio for the shader: theme primary or the user's colour, with
    // deep/hot variants derived in HSV so any hue keeps the original red
    // palette's contrast (defaults land on the old 93000a/ff5449/ffc4b8)
    readonly property color accent: ShellPrefs.dnaUseThemeColor ? Colours.palette.m3primary : ShellPrefs.dnaCustomColor

    ShaderEffect {
        id: fx

        property real uTime: 0
        readonly property real uAspect: width / Math.max(1, height)
        readonly property color uColMain: root.accent
        readonly property color uColDeep: Qt.hsva(root.accent.hsvHue, Math.min(1, root.accent.hsvSaturation * 1.4), root.accent.hsvValue * 0.58, 1)
        readonly property color uColHot: Qt.hsva(root.accent.hsvHue, root.accent.hsvSaturation * 0.39, Math.min(1, root.accent.hsvValue * 1.0), 1)

        anchors.fill: parent
        fragmentShader: Qt.resolvedUrl("../../assets/shaders/dna.frag.qsb")

        layer.enabled: true
        layer.smooth: true
        layer.textureSize: Qt.size(Math.ceil(width / 2), Math.ceil(height / 2))
    }

    Timer {
        running: root.visible && root.desktopVisible
        repeat: true
        triggeredOnStart: true
        interval: UPower.onBattery ? 66 : 33
        // Restarting after a pause must not count the time spent paused.
        onRunningChanged: root.lastTickMs = 0
        onTriggered: {
            const now = Date.now();
            // First tick of a run advances nothing; later ticks are clamped so a
            // stalled frame can't jump the helix either.
            const dt = root.lastTickMs === 0 ? 0 : Math.min((now - root.lastTickMs) / 1000, 0.25);
            root.lastTickMs = now;
            root.elapsed = (root.elapsed + dt) % root.timeWrap;
            fx.uTime = root.elapsed;
            root.frameAdvanced();
        }
    }
}
