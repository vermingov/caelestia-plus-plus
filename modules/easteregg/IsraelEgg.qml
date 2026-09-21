pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Shapes
import QtMultimedia
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import qs.services

// Desktop easter egg: `qs -c caelestia ipc call israelEgg pop` plays
// israel.mp3 at 80% volume under a fullscreen five-act cinematic -
// a glowing Star of David, then the flag sweeping in and waving, then
// the tank interlude, then the statesman portrait rising under a rain
// of floating eyes, and finally a sleeping face whose eyes get clamped
// open - with a white flash between acts. The whole scene rides a slow
// camera zoom, pumps on each act cut and shakes on impacts. Everything
// fades out when the track ends. Triggered by typing i-s-r-a-e-l on an
// empty desktop (same evdev watcher as the other egg). Clicks pass
// through.
Scope {
    id: root

    // cae plays this one now; while it does, this whole thing stays where
    // it is and never starts.
    readonly property bool active: !ExternalBar.hasCinema

    // 0 idle, 1 star act, 2 flag act, 3 tank act, 4 portrait act,
    // 5 awakening act, 6 fading out
    property int phase: 0

    IpcHandler {
        target: "israelEgg"

        function pop(): void {
            if (root.active && root.phase === 0)
                root.phase = 1;
        }
    }

    // Keeps the window alive just past the 1200ms outro fade
    Timer {
        running: root.phase === 6
        interval: 1300
        onTriggered: root.phase = 0
    }

    LazyLoader {
        active: root.phase !== 0

        PanelWindow {
            id: window

            anchors.top: true
            anchors.bottom: true
            anchors.left: true
            anchors.right: true
            exclusionMode: ExclusionMode.Ignore
            color: "transparent"

            WlrLayershell.layer: WlrLayer.Overlay
            WlrLayershell.namespace: "caelestia-israelegg"

            // Empty input region: purely decorative, clicks pass through
            mask: Region {}

            MediaPlayer {
                id: track

                source: Qt.resolvedUrl("../../assets/israel.mp3")
                audioOutput: AudioOutput {
                    volume: 0.8
                }

                Component.onCompleted: play()
                onMediaStatusChanged: {
                    if (mediaStatus === MediaPlayer.EndOfMedia)
                        root.phase = 6;
                }
                // A broken file would otherwise leave the overlay up forever
                onErrorOccurred: root.phase = 6
            }

            // Backstop for the same reason, in case neither signal fires
            Timer {
                interval: 30000
                running: root.phase >= 1 && root.phase <= 5
                onTriggered: root.phase = 6
            }

            // Act changes; the ~18s track covers act 5 until EndOfMedia.
            // One-shot act animations (tank drive, awakening rig) are cued
            // from here explicitly - a `running: phase === N` binding on a
            // finite animation restarts it forever once it completes.
            SequentialAnimation {
                running: true

                PauseAnimation {
                    duration: 4200
                }
                ScriptAction {
                    script: {
                        root.phase = 2;
                        actFlash.restart();
                    }
                }
                PauseAnimation {
                    duration: 3600
                }
                ScriptAction {
                    script: {
                        root.phase = 3;
                        actFlash.restart();
                        tankDrive.restart();
                    }
                }
                // Quick tank interlude
                PauseAnimation {
                    duration: 2800
                }
                ScriptAction {
                    script: {
                        root.phase = 4;
                        actFlash.restart();
                    }
                }
                PauseAnimation {
                    duration: 2600
                }
                ScriptAction {
                    script: {
                        root.phase = 5;
                        actFlash.restart();
                        wakeSeq.restart();
                    }
                }
            }

            Item {
                id: scene

                readonly property real centerY: height * 0.46

                // 0 -> 1 over the intro; drives the fade-in and bar slide
                property real reveal: 0
                // Shared breathing clock for every glow and throb
                property real pulse: 0
                // White blink covering each act transition
                property real flash: 0
                // Camera: slow push-in over the whole runtime...
                property real drift: 1
                // ...plus a quick pump on every act cut
                property real pump: 1
                // Impact shake; 1 = settled, kick() rewinds it
                property real shake: 1
                property real shakeAmp: 0

                function kick(amp) {
                    shakeAmp = amp;
                    shakeJolt.restart();
                }

                anchors.fill: parent
                opacity: root.phase === 6 ? 0 : 1

                NumberAnimation on reveal {
                    from: 0
                    to: 1
                    duration: 1400
                    easing.type: Easing.OutCubic
                }

                NumberAnimation on drift {
                    from: 1
                    to: 1.05
                    duration: 26000
                }

                SequentialAnimation on pulse {
                    loops: Animation.Infinite

                    NumberAnimation {
                        to: 1
                        duration: 1600
                        easing.type: Easing.InOutSine
                    }
                    NumberAnimation {
                        to: 0
                        duration: 1600
                        easing.type: Easing.InOutSine
                    }
                }

                Behavior on opacity {
                    NumberAnimation {
                        duration: 1200
                        easing.type: Easing.InQuad
                    }
                }

                ParallelAnimation {
                    id: actFlash

                    SequentialAnimation {
                        NumberAnimation {
                            target: scene
                            property: "flash"
                            to: 0.85
                            duration: 100
                            easing.type: Easing.OutQuad
                        }
                        NumberAnimation {
                            target: scene
                            property: "flash"
                            to: 0
                            duration: 500
                            easing.type: Easing.InQuad
                        }
                    }
                    SequentialAnimation {
                        NumberAnimation {
                            target: scene
                            property: "pump"
                            to: 1.03
                            duration: 130
                            easing.type: Easing.OutQuad
                        }
                        NumberAnimation {
                            target: scene
                            property: "pump"
                            to: 1
                            duration: 550
                            easing.type: Easing.OutCubic
                        }
                    }
                }

                NumberAnimation {
                    id: shakeJolt

                    target: scene
                    property: "shake"
                    from: 0
                    to: 1
                    duration: 420
                }

                // Everything the "camera" sees; letterbox, vignette and
                // flash stay outside so the frame itself never moves
                Item {
                    id: world

                    anchors.fill: parent
                    scale: scene.drift * scene.pump
                    transform: Translate {
                        x: Math.sin(scene.shake * 31.4) * scene.shakeAmp * (1 - scene.shake)
                        y: Math.sin(scene.shake * 23.6) * scene.shakeAmp * 0.6 * (1 - scene.shake)
                    }

                    // Two overlapping stroked triangles; used by the hero
                    // star and the flag's smaller one
                    component TrianglePair: Shape {
                        id: pair

                        property real lineWidth
                        property color lineColor
                        property var upPoints
                        property var downPoints

                        anchors.fill: parent
                        preferredRendererType: Shape.CurveRenderer

                        ShapePath {
                            strokeWidth: pair.lineWidth
                            strokeColor: pair.lineColor
                            fillColor: "transparent"
                            joinStyle: ShapePath.RoundJoin
                            PathPolyline {
                                path: pair.upPoints
                            }
                        }

                        ShapePath {
                            strokeWidth: pair.lineWidth
                            strokeColor: pair.lineColor
                            fillColor: "transparent"
                            joinStyle: ShapePath.RoundJoin
                            PathPolyline {
                                path: pair.downPoints
                            }
                        }
                    }

                    component MagenDavid: Item {
                        id: magen

                        property color lineColor: "#f4f7ff"
                        property color glowColor: "#3d6bff"
                        readonly property real lineWidth: width * 0.028

                        height: width

                        function trianglePoints(startDeg) {
                            const r = width / 2 / 1.2;
                            const pts = [];
                            for (let i = 0; i <= 3; i++) {
                                const a = (startDeg + i * 120) * Math.PI / 180;
                                pts.push(Qt.point(width / 2 + r * Math.cos(a), width / 2 + r * Math.sin(a)));
                            }
                            return pts;
                        }

                        readonly property var upPoints: trianglePoints(-90)
                        readonly property var downPoints: trianglePoints(90)

                        TrianglePair {
                            lineWidth: magen.lineWidth * 3
                            lineColor: magen.glowColor
                            upPoints: magen.upPoints
                            downPoints: magen.downPoints
                            opacity: 0.25 + scene.pulse * 0.2
                        }

                        TrianglePair {
                            lineWidth: magen.lineWidth
                            lineColor: magen.lineColor
                            upPoints: magen.upPoints
                            downPoints: magen.downPoints
                        }
                    }

                    // Soft radial aura; sits behind each act's centerpiece
                    component GlowDisc: Shape {
                        id: disc

                        property color tint: "#3d6bff"
                        property real strength: 0.4

                        height: width
                        preferredRendererType: Shape.CurveRenderer

                        ShapePath {
                            strokeWidth: -1
                            strokeColor: "transparent"
                            fillGradient: RadialGradient {
                                centerX: disc.width / 2
                                centerY: disc.height / 2
                                focalX: centerX
                                focalY: centerY
                                centerRadius: disc.width / 2

                                GradientStop {
                                    position: 0
                                    color: Qt.alpha(disc.tint, disc.strength)
                                }
                                GradientStop {
                                    position: 0.55
                                    color: Qt.alpha(disc.tint, disc.strength * 0.35)
                                }
                                GradientStop {
                                    position: 1
                                    color: "transparent"
                                }
                            }
                            PathAngleArc {
                                centerX: disc.width / 2
                                centerY: disc.height / 2
                                radiusX: disc.width / 2
                                radiusY: disc.height / 2
                                sweepAngle: 360
                            }
                        }
                    }

                    component RayFan: Item {
                        id: fan

                        property int count: 8
                        property real beamLength: Math.min(window.width, window.height) * 0.68
                        property real beamWidth: 3
                        property real period: 40000
                        property real dir: 1

                        NumberAnimation on rotation {
                            from: 0
                            to: 360 * fan.dir
                            duration: fan.period
                            loops: Animation.Infinite
                        }

                        Repeater {
                            model: fan.count

                            Rectangle {
                                required property int index

                                x: -width / 2
                                y: -height
                                width: fan.beamWidth
                                height: fan.beamLength
                                rotation: index * 360 / fan.count
                                transformOrigin: Item.Bottom
                                gradient: Gradient {
                                    GradientStop {
                                        position: 0
                                        color: "transparent"
                                    }
                                    GradientStop {
                                        position: 1
                                        color: "#dce6ff"
                                    }
                                }
                            }
                        }
                    }

                    Rectangle {
                        anchors.fill: parent
                        color: "#050d26"
                        opacity: scene.reveal * 0.55
                    }

                    // Slow-turning rays; two counter-rotating fans for
                    // depth, following the current act's centerpiece
                    Item {
                        id: rays

                        anchors.horizontalCenter: parent.horizontalCenter
                        y: root.phase >= 4 ? parent.height * 0.42 : scene.centerY
                        opacity: scene.reveal * (root.phase >= 4 ? 0.2 : 0.14)

                        Behavior on y {
                            NumberAnimation {
                                duration: 800
                                easing.type: Easing.InOutQuad
                            }
                        }

                        RayFan {}

                        RayFan {
                            count: 6
                            dir: -1
                            period: 61000
                            beamWidth: 2
                            beamLength: Math.min(window.width, window.height) * 0.5
                            opacity: 0.6
                        }
                    }

                    // Act 2 centerpiece; parks small at the top for act 4
                    Item {
                        id: flag

                        property real wave: 0
                        property real flutter: 0
                        readonly property bool shown: root.phase === 2 || root.phase === 4
                        readonly property real targetCx: window.width / 2
                        readonly property real targetCy: root.phase >= 4 ? window.height * 0.17 : scene.centerY

                        width: window.width * 0.46
                        height: width * 8 / 11
                        x: (root.phase >= 2 ? targetCx : -width * 1.6) - width / 2
                        y: targetCy - height / 2
                        scale: root.phase >= 4 ? 0.5 : 1
                        opacity: shown ? 1 : 0

                        // Cloth motion pinned at the hoist: an in-plane
                        // wave plus a perspective flutter about the pole
                        transform: [
                            Rotation {
                                origin.y: flag.height / 2
                                axis {
                                    x: 0
                                    y: 1
                                    z: 0
                                }
                                angle: flag.flutter
                            },
                            Rotation {
                                origin.y: flag.height / 2
                                angle: flag.wave
                            }
                        ]

                        Behavior on opacity {
                            NumberAnimation {
                                duration: 500
                            }
                        }

                        Behavior on x {
                            NumberAnimation {
                                duration: 900
                                easing.type: Easing.OutBack
                                easing.overshoot: 1.1
                            }
                        }

                        Behavior on y {
                            NumberAnimation {
                                duration: 800
                                easing.type: Easing.InOutQuad
                            }
                        }

                        Behavior on scale {
                            NumberAnimation {
                                duration: 800
                                easing.type: Easing.InOutQuad
                            }
                        }

                        SequentialAnimation on wave {
                            running: flag.shown
                            loops: Animation.Infinite

                            NumberAnimation {
                                to: 2.5
                                duration: 1400
                                easing.type: Easing.InOutSine
                            }
                            NumberAnimation {
                                to: -2.5
                                duration: 1400
                                easing.type: Easing.InOutSine
                            }
                        }

                        // Off-beat from the wave so the cloth never settles
                        SequentialAnimation on flutter {
                            running: flag.shown
                            loops: Animation.Infinite

                            NumberAnimation {
                                to: 7
                                duration: 760
                                easing.type: Easing.InOutSine
                            }
                            NumberAnimation {
                                to: -7
                                duration: 760
                                easing.type: Easing.InOutSine
                            }
                        }

                        GlowDisc {
                            anchors.centerIn: parent
                            width: parent.width * 1.5
                            z: -1
                            tint: "#bcd0ff"
                            strength: 0.22
                        }

                        Rectangle {
                            id: pole

                            x: -width - flag.width * 0.015
                            y: -flag.height * 0.14
                            width: Math.max(4, flag.width * 0.016)
                            height: flag.height * 1.34
                            radius: width / 2
                            gradient: Gradient {
                                GradientStop {
                                    position: 0
                                    color: "#e8edf5"
                                }
                                GradientStop {
                                    position: 1
                                    color: "#98a2b3"
                                }
                            }
                        }

                        Rectangle {
                            x: pole.x + pole.width / 2 - width / 2
                            y: pole.y - height * 0.7
                            width: pole.width * 2.6
                            height: width
                            radius: width / 2
                            color: "#f2d27c"
                        }

                        Rectangle {
                            anchors.fill: parent
                            radius: 6
                            color: "#fdfdfd"
                            border.color: "#c9d4e8"
                            border.width: 1
                        }

                        component FlagStripe: Rectangle {
                            anchors.left: parent.left
                            anchors.right: parent.right
                            anchors.leftMargin: parent.width * 0.02
                            anchors.rightMargin: parent.width * 0.02
                            height: parent.height * 0.11
                            color: "#0038b8"
                        }

                        FlagStripe {
                            y: flag.height * 0.14
                        }

                        FlagStripe {
                            y: flag.height * 0.75
                        }

                        MagenDavid {
                            anchors.centerIn: parent
                            width: flag.height * 0.5
                            lineColor: "#0038b8"
                            glowColor: "#0038b8"
                        }

                        // Light sweeping across the cloth while it waves
                        Item {
                            anchors.fill: parent
                            clip: true

                            Rectangle {
                                id: sheen

                                property real sweep: 0

                                x: -width + sweep * (flag.width + 2 * width)
                                y: -flag.height * 0.25
                                width: flag.width * 0.3
                                height: flag.height * 1.5
                                rotation: 14
                                opacity: 0.5
                                gradient: Gradient {
                                    orientation: Gradient.Horizontal

                                    GradientStop {
                                        position: 0
                                        color: "transparent"
                                    }
                                    GradientStop {
                                        position: 0.5
                                        color: "#30ffffff"
                                    }
                                    GradientStop {
                                        position: 1
                                        color: "transparent"
                                    }
                                }

                                SequentialAnimation on sweep {
                                    running: root.phase === 2
                                    loops: Animation.Infinite

                                    NumberAnimation {
                                        from: 0
                                        to: 1
                                        duration: 1900
                                        easing.type: Easing.InOutQuad
                                    }
                                    PauseAnimation {
                                        duration: 900
                                    }
                                }
                            }
                        }
                    }

                    // Act 1 centerpiece aura + a halo ring that blooms
                    // outward as the star lands
                    GlowDisc {
                        anchors.horizontalCenter: parent.horizontalCenter
                        y: scene.centerY - width / 2
                        width: heroStar.width * 2.1
                        strength: 0.5
                        opacity: heroStar.opacity * (0.45 + scene.pulse * 0.35)
                    }

                    Rectangle {
                        id: halo

                        property real burst: 0

                        anchors.horizontalCenter: parent.horizontalCenter
                        y: scene.centerY - height / 2
                        width: heroStar.width * (0.5 + burst * 1.3)
                        height: width
                        radius: width / 2
                        color: "transparent"
                        border.color: "#f4f7ff"
                        border.width: Math.max(1.5, 4 * (1 - burst))
                        opacity: root.phase === 1 ? (1 - burst) * 0.7 : 0

                        NumberAnimation on burst {
                            from: 0
                            to: 1
                            duration: 1600
                            easing.type: Easing.OutCubic
                        }
                    }

                    // Act 1 centerpiece: swings in with a little overshoot,
                    // then breathes and sways; the act-2 flash covers its
                    // hard cut
                    MagenDavid {
                        id: heroStar

                        property real arrive: 0
                        property real sway: 0

                        anchors.horizontalCenter: parent.horizontalCenter
                        y: scene.centerY - height / 2
                        width: Math.min(window.width, window.height) * 0.5
                        scale: (0.55 + 0.45 * arrive) * (0.96 + scene.pulse * 0.05)
                        rotation: (1 - arrive) * -12 + sway
                        opacity: scene.reveal * (root.phase === 1 ? 1 : 0)

                        NumberAnimation on arrive {
                            from: 0
                            to: 1
                            duration: 1500
                            easing.type: Easing.OutBack
                            easing.overshoot: 1.4
                        }

                        SequentialAnimation on sway {
                            running: root.phase === 1
                            loops: Animation.Infinite

                            NumberAnimation {
                                to: 2.5
                                duration: 2400
                                easing.type: Easing.InOutSine
                            }
                            NumberAnimation {
                                to: -2.5
                                duration: 2400
                                easing.type: Easing.InOutSine
                            }
                        }
                    }

                    GlowDisc {
                        anchors.horizontalCenter: parent.horizontalCenter
                        y: bibi.y + bibi.height / 2 - width / 2
                        width: bibi.height * 1.4
                        tint: "#4d79ff"
                        strength: 0.35
                        opacity: root.phase === 4 ? 0.8 : 0

                        Behavior on opacity {
                            NumberAnimation {
                                duration: 600
                            }
                        }
                    }

                    // Act 4 centerpiece: rises from the bottom edge, sways
                    Image {
                        id: bibi

                        // 0 sunk below the edge, 1 fully risen
                        property real rise: root.phase === 4 ? 1 : 0
                        property real sway: 0

                        anchors.horizontalCenter: parent.horizontalCenter
                        y: window.height - height * rise
                        height: window.height * 0.52
                        width: height * 0.8
                        sourceSize: Qt.size(width, height)
                        source: Qt.resolvedUrl("../../assets/israel-bibi.svg")
                        transformOrigin: Item.Bottom
                        rotation: sway
                        scale: 1 + scene.pulse * 0.03

                        Behavior on rise {
                            NumberAnimation {
                                duration: 900
                                easing.type: Easing.OutBack
                                easing.overshoot: 1.2
                            }
                        }

                        SequentialAnimation on sway {
                            running: root.phase >= 4
                            loops: Animation.Infinite

                            NumberAnimation {
                                to: 2.5
                                duration: 1100
                                easing.type: Easing.InOutSine
                            }
                            NumberAnimation {
                                to: -2.5
                                duration: 1100
                                easing.type: Easing.InOutSine
                            }
                        }
                    }

                    // Act 4 weather: floating eyes drifting down on one
                    // clock; each blinks on its own beat and every pupil
                    // tracks the portrait at screen centre
                    Item {
                        id: eyeRain

                        property real clock: 0

                        anchors.fill: parent
                        opacity: root.phase === 4 ? 1 : 0

                        Behavior on opacity {
                            NumberAnimation {
                                duration: 600
                            }
                        }

                        NumberAnimation on clock {
                            from: 0
                            to: 1
                            duration: 3400
                            loops: Animation.Infinite
                            running: root.phase === 4
                        }

                        Repeater {
                            model: 22

                            Item {
                                id: eye

                                required property int index

                                // Per-eye lane, fall offset and size, rolled once
                                readonly property real lane: Math.random()
                                readonly property real drop: Math.random()
                                readonly property real t: (eyeRain.clock + drop) % 1
                                // Brief lid squash whenever this sine crests
                                readonly property real blink: Math.max(0, Math.sin(t * 18.8 + drop * 40) - 0.86) / 0.14
                                readonly property real gaze: Math.max(-1, Math.min(1, (window.width / 2 - (x + width / 2)) / (window.width / 2)))

                                x: lane * eyeRain.width + Math.sin(t * 12.6 + drop * 6.3) * 30
                                y: -height + (eyeRain.height + 2 * height) * t
                                width: 30 + (index % 3) * 8
                                height: width * 0.6
                                rotation: Math.sin(t * 6.3 + drop * 6.3) * 16
                                opacity: Math.min(1, Math.sin(t * Math.PI) * 1.8)

                                transform: Scale {
                                    origin.y: eye.height / 2
                                    yScale: 1 - eye.blink * 0.85
                                }

                                Rectangle {
                                    anchors.fill: parent
                                    radius: height / 2
                                    color: "#fdfdfd"
                                    border.color: "#c9d4e8"
                                    border.width: 1
                                }

                                Rectangle {
                                    anchors.centerIn: parent
                                    anchors.horizontalCenterOffset: eye.gaze * eye.width * 0.14
                                    width: eye.height * 0.62
                                    height: width
                                    radius: width / 2
                                    color: "#2e6bd6"

                                    Rectangle {
                                        anchors.centerIn: parent
                                        width: parent.width * 0.45
                                        height: width
                                        radius: width / 2
                                        color: "#101318"
                                    }
                                }
                            }
                        }
                    }

                    // One eye of the act-5 face, plus the retractor that
                    // pins it
                    component ForcedEye: Item {
                        id: rig

                        // 0 lid shut, 1 lid pinned wide
                        property real open: 0
                        // 0 retractor still off-screen above, 1 clamped on
                        property real grip: 0
                        // -1..1 sideways dart of the iris once awake
                        property real look: 0

                        readonly property real aperture: height * Math.min(1, open)

                        // The lid opening; eyeball is drawn full size and clipped
                        Item {
                            id: slit

                            anchors.centerIn: parent
                            width: rig.width
                            height: Math.max(rig.height * 0.05, rig.aperture)
                            clip: true

                            Item {
                                y: (slit.height - rig.height) / 2
                                width: rig.width
                                height: rig.height

                                Rectangle {
                                    anchors.fill: parent
                                    radius: height / 2
                                    color: "#fdfdfd"
                                }

                                Rectangle {
                                    anchors.centerIn: parent
                                    anchors.horizontalCenterOffset: rig.look * rig.height * 0.16
                                    width: rig.height * 0.66
                                    height: width
                                    radius: width / 2
                                    color: "#2e6bd6"

                                    // Pupil blows open with the rest of the eye
                                    Rectangle {
                                        anchors.centerIn: parent
                                        width: parent.width * (0.32 + rig.open * 0.3)
                                        height: width
                                        radius: width / 2
                                        color: "#101318"
                                    }
                                }
                            }
                        }

                        // Lid rim; reads as a lash line while the eye is shut
                        Rectangle {
                            anchors.centerIn: parent
                            width: slit.width
                            height: slit.height
                            radius: height / 2
                            color: "transparent"
                            border.color: "#8a5f3c"
                            border.width: Math.max(2, rig.height * 0.07)
                        }

                        Item {
                            id: retractor

                            readonly property real clawHeight: Math.max(4, rig.height * 0.14)

                            anchors.horizontalCenter: parent.horizontalCenter
                            y: parent.height / 2
                            // Drops in from off the top edge, then rides the lids
                            transform: Translate {
                                y: -(1 - rig.grip) * window.height
                            }

                            // Shaft running up off the top of the screen
                            Rectangle {
                                x: -width / 2
                                y: -window.height
                                width: Math.max(3, rig.height * 0.1)
                                height: window.height - rig.aperture / 2 - retractor.clawHeight
                                color: "#9aa6bb"
                            }

                            // Upper claw, hooking the top lid open
                            Rectangle {
                                x: -width / 2
                                y: -rig.aperture / 2 - height
                                width: rig.width * 0.92
                                height: retractor.clawHeight
                                radius: height / 2
                                gradient: Gradient {
                                    GradientStop {
                                        position: 0
                                        color: "#f2f6ff"
                                    }
                                    GradientStop {
                                        position: 1
                                        color: "#7f8ca3"
                                    }
                                }
                            }

                            // Lower claw, hooking the bottom lid open
                            Rectangle {
                                x: -width / 2
                                y: rig.aperture / 2
                                width: rig.width * 0.92
                                height: retractor.clawHeight
                                radius: height / 2
                                gradient: Gradient {
                                    GradientStop {
                                        position: 0
                                        color: "#f2f6ff"
                                    }
                                    GradientStop {
                                        position: 1
                                        color: "#7f8ca3"
                                    }
                                }
                            }
                        }
                    }

                    // Act 3: an IDF tank crosses a dusty roadside past a
                    // "Gaza" sign - rubble skyline behind, tread marks and
                    // a hanging dust wake behind it - and flattens the
                    // figures standing in its path
                    Item {
                        id: tankScene

                        // 0 tank off the right edge, 1 fully across to the left
                        property real drive: 0

                        readonly property real groundY: height * 0.72
                        readonly property real tankW: width * 0.28
                        readonly property real tankH: tankW * 0.46
                        // Leading (left) edge of the hull as it crosses the screen
                        readonly property real tankFront: width - drive * (width + tankW)
                        readonly property real tankRear: tankFront + tankW

                        anchors.fill: parent
                        opacity: root.phase === 3 ? scene.reveal : 0

                        Behavior on opacity {
                            NumberAnimation {
                                duration: 500
                            }
                        }

                        NumberAnimation {
                            id: tankDrive

                            target: tankScene
                            property: "drive"
                            from: 0
                            to: 1
                            duration: 2750
                            easing.type: Easing.InOutSine
                        }

                        // Ruined skyline on the horizon
                        Repeater {
                            model: 6

                            Item {
                                required property int index

                                readonly property real h: tankScene.tankH * (0.35 + (index * 2 % 3) * 0.22)

                                x: tankScene.width * (0.02 + index * 0.16)
                                y: tankScene.groundY - h
                                width: tankScene.width * (0.05 + (index % 2) * 0.03)
                                height: h
                                opacity: 0.55

                                Rectangle {
                                    anchors.fill: parent
                                    color: "#141b2c"
                                }

                                // Off-centre spur breaks the roofline
                                Rectangle {
                                    x: parent.width * 0.6
                                    y: -parent.height * 0.25
                                    width: parent.width * 0.35
                                    height: parent.height * 1.25
                                    color: "#141b2c"
                                }
                            }
                        }

                        // Ground the tank rides along
                        Rectangle {
                            anchors.left: parent.left
                            anchors.right: parent.right
                            y: tankScene.groundY
                            height: parent.height - tankScene.groundY
                            gradient: Gradient {
                                GradientStop {
                                    position: 0
                                    color: "#3a3326"
                                }
                                GradientStop {
                                    position: 1
                                    color: "#221d15"
                                }
                            }
                        }

                        // Scattered stones so the ground isn't a flat band
                        Repeater {
                            model: 8

                            Rectangle {
                                required property int index

                                x: tankScene.width * (0.03 + index * 0.127 + (index % 3) * 0.02)
                                y: tankScene.groundY + 8 + (index % 4) * (tankScene.height - tankScene.groundY) * 0.2
                                width: 8 + (index % 3) * 6
                                height: width * 0.45
                                radius: 2
                                color: "#171310"
                                opacity: 0.8
                            }
                        }

                        // Tread marks left behind the tracks
                        Rectangle {
                            x: tankScene.tankRear
                            y: tankScene.groundY + 6
                            width: Math.max(0, tankScene.width - tankScene.tankRear)
                            height: 3
                            color: "#151109"
                            opacity: 0.7
                        }

                        Rectangle {
                            x: tankScene.tankRear
                            y: tankScene.groundY + 13
                            width: Math.max(0, tankScene.width - tankScene.tankRear)
                            height: 3
                            color: "#151109"
                            opacity: 0.7
                        }

                        // Green highway sign reading "Gaza" on a post;
                        // rattles when the hull clips it
                        Item {
                            id: gazaSign

                            readonly property real boardW: tankScene.width * 0.14
                            readonly property real boardH: boardW * 0.5
                            readonly property bool bumped: tankScene.tankFront <= x + boardW * 0.5

                            onBumpedChanged: {
                                if (bumped)
                                    signShake.restart();
                            }

                            x: tankScene.width * 0.76
                            y: tankScene.groundY - height
                            width: boardW
                            height: tankScene.groundY - tankScene.height * 0.34
                            transformOrigin: Item.Bottom

                            SequentialAnimation {
                                id: signShake

                                NumberAnimation {
                                    target: gazaSign
                                    property: "rotation"
                                    to: -9
                                    duration: 90
                                    easing.type: Easing.OutQuad
                                }
                                NumberAnimation {
                                    target: gazaSign
                                    property: "rotation"
                                    to: 4
                                    duration: 150
                                }
                                NumberAnimation {
                                    target: gazaSign
                                    property: "rotation"
                                    to: -2
                                    duration: 130
                                }
                                NumberAnimation {
                                    target: gazaSign
                                    property: "rotation"
                                    to: 0
                                    duration: 180
                                    easing.type: Easing.OutQuad
                                }
                            }

                            Rectangle {
                                anchors.horizontalCenter: parent.horizontalCenter
                                anchors.bottom: parent.bottom
                                width: Math.max(4, gazaSign.boardW * 0.06)
                                height: parent.height - gazaSign.boardH
                                color: "#8a8f96"
                            }

                            Rectangle {
                                anchors.top: parent.top
                                anchors.horizontalCenter: parent.horizontalCenter
                                width: gazaSign.boardW
                                height: gazaSign.boardH
                                radius: height * 0.12
                                color: "#0f7a3d"
                                border.color: "#ffffff"
                                border.width: Math.max(2, gazaSign.boardW * 0.03)

                                Text {
                                    anchors.centerIn: parent
                                    text: "Gaza"
                                    color: "#ffffff"
                                    font.bold: true
                                    font.pixelSize: gazaSign.boardH * 0.42
                                }
                            }
                        }

                        // Figures in the tank's path: they throw their arms
                        // up and lean away as the hull looms, then each
                        // folds flat once its leading edge arrives, kicking
                        // up a dust ring and a screen bump
                        Repeater {
                            model: 5

                            Item {
                                id: victim

                                required property int index

                                readonly property real footX: tankScene.width * (0.12 + index * 0.13)
                                readonly property real bodyH: tankScene.height * (0.22 + (index % 3) * 0.03)
                                readonly property bool crushed: tankScene.tankFront <= footX
                                readonly property bool panicked: !crushed && tankScene.drive > 0 && tankScene.tankFront - footX < tankScene.width * 0.14
                                readonly property color shirt: ["#9c5a3c", "#5a6b8c", "#7a8560", "#8c5a75", "#a08a4a"][index]

                                onCrushedChanged: {
                                    if (crushed) {
                                        impact.restart();
                                        scene.kick(3.5);
                                    }
                                }

                                x: footX
                                y: tankScene.groundY - bodyH
                                width: bodyH * 0.42
                                height: bodyH

                                // Dust ring stays unsquashed while the body
                                // beneath it folds flat
                                Rectangle {
                                    id: poof

                                    property real ring: 1

                                    anchors.horizontalCenter: parent.horizontalCenter
                                    anchors.bottom: parent.bottom
                                    anchors.bottomMargin: -height / 2
                                    width: victim.width * 1.7
                                    height: width * 0.5
                                    radius: height / 2
                                    color: "transparent"
                                    border.color: "#b9a98c"
                                    border.width: 2
                                    scale: 0.3 + ring * 0.9
                                    opacity: (1 - ring) * 0.8

                                    NumberAnimation {
                                        id: impact

                                        target: poof
                                        property: "ring"
                                        from: 0
                                        to: 1
                                        duration: 450
                                        easing.type: Easing.OutQuad
                                    }
                                }

                                Item {
                                    anchors.fill: parent
                                    rotation: victim.panicked ? -7 : 0
                                    transformOrigin: Item.Bottom

                                    Behavior on rotation {
                                        NumberAnimation {
                                            duration: 150
                                        }
                                    }

                                    transform: Scale {
                                        origin.x: victim.width / 2
                                        origin.y: victim.height
                                        xScale: victim.crushed ? 1.5 : 1
                                        yScale: victim.crushed ? 0.1 : 1

                                        Behavior on yScale {
                                            NumberAnimation {
                                                duration: 140
                                                easing.type: Easing.OutQuad
                                            }
                                        }
                                        Behavior on xScale {
                                            NumberAnimation {
                                                duration: 140
                                                easing.type: Easing.OutQuad
                                            }
                                        }
                                    }

                                    // Legs
                                    Rectangle {
                                        x: parent.width * 0.18
                                        anchors.bottom: parent.bottom
                                        width: parent.width * 0.2
                                        height: parent.height * 0.24
                                        radius: width / 2
                                        color: "#2e3138"
                                    }

                                    Rectangle {
                                        x: parent.width * 0.58
                                        anchors.bottom: parent.bottom
                                        width: parent.width * 0.2
                                        height: parent.height * 0.24
                                        radius: width / 2
                                        color: "#2e3138"
                                    }

                                    // Torso
                                    Rectangle {
                                        anchors.horizontalCenter: parent.horizontalCenter
                                        y: parent.height * 0.3
                                        width: parent.width * 0.66
                                        height: parent.height * 0.5
                                        radius: width * 0.35
                                        color: victim.shirt
                                    }

                                    // Arms; thrown up when the hull looms
                                    Rectangle {
                                        x: parent.width * 0.08
                                        y: parent.height * 0.34
                                        width: parent.width * 0.13
                                        height: parent.height * 0.34
                                        radius: width / 2
                                        color: victim.shirt
                                        transformOrigin: Item.Top
                                        rotation: victim.panicked ? 150 : 12

                                        Behavior on rotation {
                                            NumberAnimation {
                                                duration: 160
                                                easing.type: Easing.OutBack
                                            }
                                        }
                                    }

                                    Rectangle {
                                        x: parent.width * 0.79
                                        y: parent.height * 0.34
                                        width: parent.width * 0.13
                                        height: parent.height * 0.34
                                        radius: width / 2
                                        color: victim.shirt
                                        transformOrigin: Item.Top
                                        rotation: victim.panicked ? -150 : -12

                                        Behavior on rotation {
                                            NumberAnimation {
                                                duration: 160
                                                easing.type: Easing.OutBack
                                            }
                                        }
                                    }

                                    // Head
                                    Rectangle {
                                        anchors.top: parent.top
                                        anchors.horizontalCenter: parent.horizontalCenter
                                        width: parent.width * 0.56
                                        height: width
                                        radius: width / 2
                                        color: "#e8c7a2"
                                    }
                                }
                            }
                        }

                        // Dust hanging in the tank's wake; each puff wakes
                        // as the rear of the hull clears its spot, then
                        // grows, lifts and thins out
                        Repeater {
                            model: 9

                            GlowDisc {
                                required property int index

                                readonly property real age: Math.max(0, Math.min(1, (x + width / 2 - tankScene.tankRear) / (tankScene.width * 0.28)))

                                x: tankScene.width * (0.04 + index * 0.115) - width / 2
                                y: tankScene.groundY - height * 0.55 - age * tankScene.tankH * 0.25
                                width: tankScene.tankH * (0.75 + (index % 3) * 0.3)
                                tint: "#c4a26e"
                                strength: 0.85
                                scale: 0.5 + age * 1.3
                                opacity: Math.max(0, Math.min(age / 0.12, (1 - age) / 0.88)) * 0.95
                            }
                        }

                        // The tank: layered hull and turret silhouettes
                        // over live running gear, riding a two-spring
                        // suspension bob
                        Item {
                            id: tank

                            readonly property real joltA: Math.sin(tankScene.drive * Math.PI * 16)
                            readonly property real joltB: Math.sin(tankScene.drive * Math.PI * 7 + 1.3)

                            readonly property var hullPoints: [Qt.point(0, height * 0.52), Qt.point(width * 0.1, height * 0.36), Qt.point(width * 0.88, height * 0.34), Qt.point(width, height * 0.44), Qt.point(width * 0.97, height * 0.62), Qt.point(width * 0.05, height * 0.62), Qt.point(0, height * 0.52)]
                            readonly property var turretPoints: [Qt.point(width * 0.3, height * 0.36), Qt.point(width * 0.36, height * 0.16), Qt.point(width * 0.64, height * 0.14), Qt.point(width * 0.74, height * 0.28), Qt.point(width * 0.78, height * 0.36), Qt.point(width * 0.3, height * 0.36)]

                            x: tankScene.tankFront
                            y: tankScene.groundY - height - (Math.abs(joltA) * 0.6 + Math.abs(joltB) * 0.4) * height * 0.04
                            width: tankScene.tankW
                            height: tankScene.tankH
                            rotation: joltA * 0.8 + joltB * 0.5
                            transformOrigin: Item.Bottom

                            // Track band
                            Rectangle {
                                x: parent.width * 0.02
                                y: parent.height * 0.7
                                width: parent.width * 0.97
                                height: parent.height * 0.3
                                radius: height / 2
                                color: "#1c1c1c"
                            }

                            // Tread links scrolling rearward along the base
                            Repeater {
                                model: 12

                                Rectangle {
                                    required property int index

                                    x: tank.width * 0.03 + ((index / 12 + tankScene.drive * 10) % 1) * tank.width * 0.9
                                    y: tank.height * 0.93
                                    width: tank.width * 0.045
                                    height: tank.height * 0.05
                                    radius: 2
                                    color: "#0d0d0d"
                                }
                            }

                            // Road wheels; crossed spokes make the spin read
                            Repeater {
                                model: 6

                                Item {
                                    id: wheel

                                    required property int index

                                    readonly property real d: tank.height * 0.24

                                    x: tank.width * 0.08 + index * (tank.width * 0.8 / 5) - d / 2
                                    y: tank.height - d
                                    width: d
                                    height: d
                                    rotation: -tankScene.drive * 2800

                                    Rectangle {
                                        anchors.fill: parent
                                        radius: width / 2
                                        color: "#3a3a3a"
                                        border.color: "#111111"
                                        border.width: Math.max(1, wheel.d * 0.12)
                                    }

                                    Rectangle {
                                        anchors.centerIn: parent
                                        width: parent.width * 0.74
                                        height: parent.width * 0.12
                                        radius: height / 2
                                        color: "#585858"
                                    }

                                    Rectangle {
                                        anchors.centerIn: parent
                                        width: parent.width * 0.12
                                        height: parent.width * 0.74
                                        radius: width / 2
                                        color: "#585858"
                                    }

                                    Rectangle {
                                        anchors.centerIn: parent
                                        width: parent.width * 0.26
                                        height: width
                                        radius: width / 2
                                        color: "#666666"
                                    }
                                }
                            }

                            // Side skirt shading the wheel tops
                            Rectangle {
                                x: parent.width * 0.03
                                y: parent.height * 0.56
                                width: parent.width * 0.94
                                height: parent.height * 0.18
                                radius: parent.height * 0.05
                                color: "#3e4520"
                            }

                            // Barrel, pointing the way it travels; flexes
                            // slightly with the suspension
                            Item {
                                x: 0
                                y: parent.height * 0.22
                                width: parent.width * 0.34
                                height: parent.height * 0.06
                                transformOrigin: Item.Right
                                rotation: tank.joltA * 0.7

                                Rectangle {
                                    x: -tank.width * 0.38
                                    width: tank.width * 0.72
                                    height: parent.height
                                    radius: height / 2
                                    color: "#2b2f22"
                                }

                                // Muzzle step
                                Rectangle {
                                    x: -tank.width * 0.38
                                    y: -parent.height * 0.25
                                    width: tank.width * 0.05
                                    height: parent.height * 1.5
                                    radius: 2
                                    color: "#242817"
                                }
                            }

                            // Hull with a sloped glacis
                            Shape {
                                anchors.fill: parent
                                preferredRendererType: Shape.CurveRenderer

                                ShapePath {
                                    strokeWidth: 2
                                    strokeColor: "#333a18"
                                    fillColor: "#4b5320"
                                    joinStyle: ShapePath.RoundJoin
                                    PathPolyline {
                                        path: tank.hullPoints
                                    }
                                }
                            }

                            // Turret
                            Shape {
                                anchors.fill: parent
                                preferredRendererType: Shape.CurveRenderer

                                ShapePath {
                                    strokeWidth: 2
                                    strokeColor: "#3b421e"
                                    fillColor: "#59653c"
                                    joinStyle: ShapePath.RoundJoin
                                    PathPolyline {
                                        path: tank.turretPoints
                                    }
                                }
                            }

                            // Commander's hatch
                            Rectangle {
                                x: parent.width * 0.46
                                y: parent.height * 0.1
                                width: parent.width * 0.1
                                height: parent.height * 0.05
                                radius: height / 2
                                color: "#3b421e"
                            }

                            Text {
                                x: parent.width * 0.44
                                y: parent.height * 0.4
                                text: "IDF"
                                color: "#e6ead2"
                                font.bold: true
                                font.pixelSize: tank.height * 0.16
                            }

                            // Whip antenna dragged backwards, flying a
                            // pennant of its own
                            Item {
                                x: parent.width * 0.9
                                y: parent.height * 0.34
                                rotation: 14 + tank.joltA * 3

                                Rectangle {
                                    x: -1
                                    y: -tank.height * 0.5
                                    width: 3
                                    height: tank.height * 0.5
                                    radius: 1
                                    color: "#222519"
                                }

                                Rectangle {
                                    x: 2
                                    y: -tank.height * 0.5
                                    width: tank.width * 0.09
                                    height: tank.height * 0.07
                                    color: "#fdfdfd"

                                    Rectangle {
                                        anchors.centerIn: parent
                                        width: parent.width
                                        height: parent.height * 0.3
                                        color: "#0038b8"
                                    }
                                }
                            }
                        }

                        // Dusty warm wash over the whole interlude
                        Rectangle {
                            anchors.fill: parent
                            gradient: Gradient {
                                GradientStop {
                                    position: 0
                                    color: "#1ab98d54"
                                }
                                GradientStop {
                                    position: 0.55
                                    color: "transparent"
                                }
                                GradientStop {
                                    position: 1
                                    color: "#40b98d54"
                                }
                            }
                        }
                    }

                    GlowDisc {
                        anchors.horizontalCenter: parent.horizontalCenter
                        y: scene.centerY - width / 2
                        width: awakening.width * 1.9
                        tint: "#4d79ff"
                        strength: 0.3
                        opacity: awakening.opacity
                    }

                    // Act 5: the sleeper, woken whether he likes it or not
                    Item {
                        id: awakening

                        // 0 asleep, 1 eyes pinned open
                        property real open: 0
                        property real grip: 0
                        property real tremble: 0
                        // Rage judder + darting eyes once the claws have won
                        property bool awake: false
                        property real quiver: 0
                        property real look: 0

                        readonly property real eyeWidth: width * 0.3
                        readonly property real eyeHeight: width * 0.2

                        anchors.horizontalCenter: parent.horizontalCenter
                        y: scene.centerY - height / 2
                        width: Math.min(window.width, window.height) * 0.46
                        height: width * 1.18
                        opacity: root.phase >= 5 ? scene.reveal : 0
                        rotation: tremble + quiver
                        scale: root.phase >= 5 ? 1 : 1.1
                        transformOrigin: Item.Bottom

                        Behavior on opacity {
                            NumberAnimation {
                                duration: 600
                            }
                        }

                        Behavior on scale {
                            NumberAnimation {
                                duration: 900
                                easing.type: Easing.OutCubic
                            }
                        }

                        // Cued from the act timeline; a `running: phase===5`
                        // binding would restart this forever on completion
                        SequentialAnimation {
                            id: wakeSeq

                            PauseAnimation {
                                duration: 1100
                            }
                            NumberAnimation {
                                target: awakening
                                property: "grip"
                                to: 1
                                duration: 380
                                easing.type: Easing.InCubic
                            }
                            ScriptAction {
                                script: scene.kick(9)
                            }
                            PauseAnimation {
                                duration: 140
                            }
                            ParallelAnimation {
                                NumberAnimation {
                                    target: awakening
                                    property: "open"
                                    to: 1
                                    duration: 620
                                    easing.type: Easing.OutBack
                                    easing.overshoot: 1.8
                                }

                                // Head snapping awake under the pull
                                SequentialAnimation {
                                    NumberAnimation {
                                        target: awakening
                                        property: "tremble"
                                        to: 1.6
                                        duration: 90
                                    }
                                    NumberAnimation {
                                        target: awakening
                                        property: "tremble"
                                        to: -1.6
                                        duration: 120
                                    }
                                    NumberAnimation {
                                        target: awakening
                                        property: "tremble"
                                        to: 0.7
                                        duration: 110
                                    }
                                    NumberAnimation {
                                        target: awakening
                                        property: "tremble"
                                        to: 0
                                        duration: 160
                                    }
                                }
                            }
                            ScriptAction {
                                script: awakening.awake = true
                            }
                        }

                        SequentialAnimation on quiver {
                            running: awakening.awake
                            loops: Animation.Infinite

                            NumberAnimation {
                                to: 0.5
                                duration: 120
                                easing.type: Easing.InOutSine
                            }
                            NumberAnimation {
                                to: -0.5
                                duration: 120
                                easing.type: Easing.InOutSine
                            }
                        }

                        // Pinned eyes darting around the room
                        SequentialAnimation on look {
                            running: awakening.awake
                            loops: Animation.Infinite

                            NumberAnimation {
                                to: 1
                                duration: 240
                                easing.type: Easing.OutCubic
                            }
                            PauseAnimation {
                                duration: 620
                            }
                            NumberAnimation {
                                to: -1
                                duration: 300
                                easing.type: Easing.OutCubic
                            }
                            PauseAnimation {
                                duration: 480
                            }
                            NumberAnimation {
                                to: 0.35
                                duration: 260
                                easing.type: Easing.OutCubic
                            }
                            PauseAnimation {
                                duration: 700
                            }
                            NumberAnimation {
                                to: 0
                                duration: 240
                                easing.type: Easing.OutCubic
                            }
                            PauseAnimation {
                                duration: 400
                            }
                        }

                        Rectangle {
                            anchors.fill: parent
                            radius: width * 0.45
                            color: "#e8c7a2"
                            border.color: "#a9805a"
                            border.width: 2
                        }

                        component Brow: Rectangle {
                            width: awakening.eyeWidth * 0.9
                            height: Math.max(3, awakening.eyeHeight * 0.16)
                            radius: height / 2
                            color: "#4a3a2a"
                        }

                        Brow {
                            x: awakening.width * 0.19
                            y: awakening.height * 0.34 - awakening.height * 0.03 * awakening.open
                        }

                        Brow {
                            x: awakening.width * 0.53
                            y: awakening.height * 0.34 - awakening.height * 0.03 * awakening.open
                        }

                        ForcedEye {
                            x: awakening.width * 0.18
                            y: awakening.height * 0.42 - height / 2
                            width: awakening.eyeWidth
                            height: awakening.eyeHeight
                            open: awakening.open
                            grip: awakening.grip
                            look: awakening.look
                        }

                        ForcedEye {
                            x: awakening.width * 0.52
                            y: awakening.height * 0.42 - height / 2
                            width: awakening.eyeWidth
                            height: awakening.eyeHeight
                            open: awakening.open
                            grip: awakening.grip
                            look: awakening.look
                        }

                        Rectangle {
                            anchors.horizontalCenter: parent.horizontalCenter
                            y: awakening.height * 0.5
                            width: Math.max(3, awakening.width * 0.03)
                            height: awakening.height * 0.14
                            radius: width / 2
                            color: "#c9a37c"
                        }

                        // Slack while asleep, a grimace once the claws bite
                        Rectangle {
                            anchors.horizontalCenter: parent.horizontalCenter
                            y: awakening.height * 0.72
                            width: awakening.width * (0.2 + awakening.open * 0.12)
                            height: Math.max(4, awakening.height * 0.055 * awakening.open)
                            radius: Math.min(width, height) / 2
                            color: "#7a3b3b"
                        }

                        Item {
                            id: zzz

                            property real clock: 0

                            anchors.horizontalCenter: parent.horizontalCenter
                            anchors.horizontalCenterOffset: awakening.width * 0.32
                            y: awakening.height * 0.16
                            opacity: Math.max(0, 1 - awakening.open * 3)

                            NumberAnimation on clock {
                                from: 0
                                to: 1
                                duration: 2400
                                loops: Animation.Infinite
                                running: root.phase === 5
                            }

                            Repeater {
                                model: 3

                                Text {
                                    required property int index

                                    readonly property real t: (zzz.clock + index / 3) % 1

                                    text: "Z"
                                    color: "#f4f7ff"
                                    font.bold: true
                                    font.pixelSize: awakening.width * (0.08 + index * 0.03)
                                    x: t * awakening.width * 0.14
                                    y: -t * awakening.height * 0.24
                                    opacity: Math.sin(t * Math.PI)
                                }
                            }
                        }
                    }

                    // Sparks drifting up behind everything, all acts
                    Item {
                        id: sparks

                        property real clock: 0

                        anchors.fill: parent
                        z: -1
                        opacity: scene.reveal

                        NumberAnimation on clock {
                            from: 0
                            to: 1
                            duration: 7000
                            loops: Animation.Infinite
                        }

                        Repeater {
                            model: 28

                            Rectangle {
                                required property int index

                                // Per-spark lane and offset, rolled once at creation
                                readonly property real lane: Math.random()
                                readonly property real drift: Math.random()
                                readonly property real t: (sparks.clock + drift) % 1

                                x: lane * sparks.width + Math.sin(t * 12.6 + drift * 9) * 14
                                y: sparks.height * (1 - t)
                                width: 3 + (index % 3) * 2
                                height: width
                                radius: width / 2
                                color: index % 4 === 0 ? "#7d9bff" : "#f4f7ff"
                                opacity: Math.sin(t * Math.PI) * 0.7
                            }
                        }
                    }
                }

                // Soft edge darkening between the world and the letterbox
                Rectangle {
                    anchors.top: parent.top
                    anchors.left: parent.left
                    anchors.right: parent.right
                    height: parent.height * 0.28
                    opacity: scene.reveal
                    gradient: Gradient {
                        GradientStop {
                            position: 0
                            color: "#66000000"
                        }
                        GradientStop {
                            position: 1
                            color: "transparent"
                        }
                    }
                }

                Rectangle {
                    anchors.bottom: parent.bottom
                    anchors.left: parent.left
                    anchors.right: parent.right
                    height: parent.height * 0.28
                    opacity: scene.reveal
                    gradient: Gradient {
                        GradientStop {
                            position: 0
                            color: "transparent"
                        }
                        GradientStop {
                            position: 1
                            color: "#66000000"
                        }
                    }
                }

                // Letterbox bars sliding in from the screen edges
                component LetterboxBar: Rectangle {
                    anchors.left: parent.left
                    anchors.right: parent.right
                    height: parent.height * 0.085
                    color: "#000000"
                }

                LetterboxBar {
                    y: -height + height * scene.reveal
                }

                LetterboxBar {
                    y: parent.height - height * scene.reveal
                }

                Rectangle {
                    anchors.fill: parent
                    color: "#ffffff"
                    opacity: scene.flash
                }
            }
        }
    }
}
