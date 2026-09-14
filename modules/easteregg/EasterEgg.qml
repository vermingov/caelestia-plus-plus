pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland

// Desktop easter egg: `qs -c caelestia ipc call easterEgg pop` plays a 7s
// bottom-edge scene - rise + jiggle, partner joins, approach, thrusting
// (tip hidden behind the partner), burst, withdraw with hanging drips and
// a floor puddle, everyone sinks. Triggered by the penis-egg-watch evdev
// watcher (~/.local/bin). Purely decorative, clicks pass through.
Scope {
    id: root

    // 0 idle, 1 rise, 2 partner, 3 approach, 4 thrust, 5 climax, 6 afterglow
    property int phase: 0

    IpcHandler {
        target: "easterEgg"

        function pop(): void {
            if (root.phase === 0)
                timeline.restart();
        }
    }

    // The evdev watcher ships with the shell; a flock in the script makes
    // this a no-op when another copy (e.g. a compositor autostart) already
    // runs. Needs /dev/input read access (input group) to do anything.
    Process {
        running: Helpers.ready
        command: Helpers.command("egg-watch")
    }

    SequentialAnimation {
        id: timeline

        // Durations sum to exactly 7000ms
        ScriptAction {
            script: root.phase = 1
        }
        PauseAnimation {
            duration: 1200
        }
        ScriptAction {
            script: root.phase = 2
        }
        PauseAnimation {
            duration: 1000
        }
        ScriptAction {
            script: root.phase = 3
        }
        PauseAnimation {
            duration: 700
        }
        ScriptAction {
            script: root.phase = 4
        }
        PauseAnimation {
            duration: 1500
        }
        ScriptAction {
            script: root.phase = 5
        }
        PauseAnimation {
            duration: 1000
        }
        ScriptAction {
            script: root.phase = 6
        }
        PauseAnimation {
            duration: 1600
        }
        ScriptAction {
            script: {
                root.phase = 0;
                sinkHold.restart();
            }
        }
    }

    // Keeps the window alive just long enough for the sink animation
    Timer {
        id: sinkHold

        interval: 700
    }

    LazyLoader {
        id: loader

        active: root.phase !== 0 || sinkHold.running

        PanelWindow {
            id: window

            // Where the two meet while joined, and the floor line
            readonly property real junctionX: vulva.x + 120
            readonly property real junctionY: vulva.y + 92
            readonly property real groundY: height - 12
            readonly property bool joined: root.phase >= 4 && root.phase <= 5

            visible: phallus.offset < 1 || vulva.offset < 1

            anchors.bottom: true
            anchors.left: true
            anchors.right: true
            exclusionMode: ExclusionMode.Ignore
            implicitHeight: 520
            color: "transparent"

            WlrLayershell.layer: WlrLayer.Overlay
            WlrLayershell.namespace: "caelestia-easteregg"

            // Empty input region: purely decorative, clicks pass through
            mask: Region {}

            component RisingImage: Image {
                // 1 = fully sunk below the edge, 0 = fully risen
                property real offset: 1

                sourceSize: Qt.size(width, height)
                transformOrigin: Item.Bottom

                Behavior on offset {
                    NumberAnimation {
                        duration: 600
                        easing.type: Easing.OutBack
                        easing.overshoot: 2.5
                    }
                }
            }

            // Clips the shaft at the slit line while joined: drawn above the
            // partner (z 2) but terminated exactly at the slit, it reads as
            // going in, not standing behind the sticker
            Item {
                id: phallusClip

                clip: true
                z: 2
                x: 0
                y: 0
                // Right edge sits on the slit centre from approach to sink
                width: root.phase >= 3 ? window.junctionX : window.width
                height: window.height

            RisingImage {
                id: phallus

                // Fresh random spot each pop, decided when the window loads;
                // keeps enough right-hand room for the partner to fit
                readonly property real spawnX: window.width * (0.1 + Math.random() * 0.4)

                // Joined pose: lean 62deg so the tip points into the partner.
                // With transformOrigin Bottom the tip sits height*sin/cos away
                // from the bottom-centre; solve x/dip so the tip lands 30px
                // past the slit (trimmed there by phallusClip)
                readonly property real joinRad: 62 * Math.PI / 180
                readonly property real tipDx: height * Math.sin(joinRad)
                readonly property real tipDy: height * Math.cos(joinRad)

                property real dip: window.joined || root.phase === 3 ? vulva.y + 118 + tipDy - window.height : 0
                property real restX: window.joined || root.phase === 3 ? vulva.x + 120 - width / 2 - tipDx + 30 : root.phase === 6 ? vulva.x - 210 : spawnX

                // Thrust travels along the lean axis, deeper behind the partner
                x: restX + thrust.push * 20 * Math.sin(joinRad)
                y: window.height - height * (1 - offset) + dip - thrust.push * 20 * Math.cos(joinRad)
                width: 220
                height: 270

                // Lean is Behavior-animated on phase changes only; wobble is
                // added raw - running it through the Behavior retargets the
                // ease-in every frame and freezes the rotation near zero
                property real lean: window.joined || root.phase === 3 ? 62 : 0

                source: Qt.resolvedUrl("../../assets/easter-egg.svg")
                offset: root.phase >= 1 ? 0 : 1
                rotation: lean + wobble.angle * (root.phase < 3 ? 1 : 0.2)
                // Throb: heartbeat pulse from the base, harder at climax
                scale: 1 + throb.beat * (root.phase === 5 ? 0.09 : 0.045)

                Behavior on restX {
                    NumberAnimation {
                        duration: 600
                        easing.type: Easing.InOutQuad
                    }
                }

                Behavior on dip {
                    NumberAnimation {
                        duration: 600
                        easing.type: Easing.InOutQuad
                    }
                }

                Behavior on lean {
                    NumberAnimation {
                        duration: 300
                        easing.type: Easing.InOutSine
                    }
                }
            }
            }

            RisingImage {
                id: vulva

                x: Math.min(phallus.spawnX + 400, window.width - width - 30)
                y: window.height - height * (1 - offset)
                z: 1
                width: 240
                height: 200

                source: Qt.resolvedUrl("../../assets/easter-egg-partner.svg")
                offset: root.phase >= 2 ? 0 : 1
                // Rock and squash in sympathy with each thrust
                rotation: thrust.push * 3
                scale: 1 - thrust.push * 0.04
            }

            SequentialAnimation {
                id: wobble

                property real angle

                running: root.phase >= 1
                loops: Animation.Infinite

                NumberAnimation {
                    target: wobble
                    property: "angle"
                    to: 6
                    duration: 350
                    easing.type: Easing.InOutSine
                }
                NumberAnimation {
                    target: wobble
                    property: "angle"
                    to: -6
                    duration: 350
                    easing.type: Easing.InOutSine
                }
            }

            SequentialAnimation {
                id: thrust

                property real push

                running: window.joined
                loops: Animation.Infinite

                onRunningChanged: {
                    if (!running)
                        push = 0;
                }

                NumberAnimation {
                    target: thrust
                    property: "push"
                    to: 1
                    duration: 260
                    easing.type: Easing.InQuad
                }
                NumberAnimation {
                    target: thrust
                    property: "push"
                    to: 0
                    duration: 240
                    easing.type: Easing.OutQuad
                }
            }

            // Heartbeat: sharp rise, slow relax
            SequentialAnimation {
                id: throb

                property real beat

                running: root.phase >= 3 && root.phase <= 5
                loops: Animation.Infinite

                onRunningChanged: {
                    if (!running)
                        beat = 0;
                }

                NumberAnimation {
                    target: throb
                    property: "beat"
                    to: 1
                    duration: 160
                    easing.type: Easing.OutQuad
                }
                NumberAnimation {
                    target: throb
                    property: "beat"
                    to: 0
                    duration: 340
                    easing.type: Easing.InQuad
                }
            }

            // Climax burst: droplets ejected from the junction on one shared
            // clock, ballistic arcs with gravity, staggered per droplet
            Item {
                id: spray

                property real clock

                anchors.fill: parent
                z: 2
                visible: root.phase === 5

                NumberAnimation on clock {
                    running: root.phase === 5
                    from: 0
                    to: 1
                    duration: 1000
                }

                Repeater {
                    model: 9

                    Rectangle {
                        required property int index

                        // Per-droplet launch vector, rolled once at creation
                        readonly property real vx: (Math.random() - 0.35) * 0.9
                        readonly property real vy: 0.55 + Math.random() * 0.65
                        readonly property real t: Math.max(0, Math.min(1, (spray.clock - index * 0.045) / 0.55))

                        x: window.junctionX + vx * 260 * t - width / 2
                        y: window.junctionY - vy * 260 * t + 330 * t * t

                        width: 9 + (index % 3) * 4
                        height: width
                        radius: width / 2
                        color: "#f7f7f0"
                        border.color: "#d8d8cc"
                        border.width: 1

                        visible: t > 0 && t < 1 && y < window.groundY - height
                    }
                }
            }

            // Afterglow: drips hang, stretch, detach and fall
            Item {
                id: drips

                property real clock

                anchors.fill: parent
                z: 2
                visible: root.phase === 6

                NumberAnimation on clock {
                    running: root.phase === 6
                    from: 0
                    to: 1
                    duration: 1600
                }

                Repeater {
                    model: [
                        // Two off the partner's slit, one off the withdrawn tip
                        {hangUntil: 0.35, sx: () => vulva.x + 120, sy: () => vulva.y + 160},
                        {hangUntil: 0.6, sx: () => vulva.x + 106, sy: () => vulva.y + 150},
                        {hangUntil: 0.5, sx: () => phallus.x + phallus.width / 2 + 34, sy: () => phallus.y + 44}
                    ]

                    Rectangle {
                        id: drip

                        required property var modelData

                        readonly property real anchorX: modelData.sx()
                        readonly property real anchorY: modelData.sy()
                        readonly property bool hanging: drips.clock < modelData.hangUntil
                        // 0..1 of the free fall after detaching
                        readonly property real fall: hanging ? 0 : Math.min(1, (drips.clock - modelData.hangUntil) / 0.3)

                        x: anchorX - width / 2
                        y: hanging ? anchorY : anchorY + (window.groundY - anchorY) * fall * fall

                        width: 9
                        // Stretch while hanging, teardrop while falling
                        height: hanging ? 10 + 26 * (drips.clock / modelData.hangUntil) : 22
                        radius: width / 2
                        color: "#f7f7f0"
                        border.color: "#d8d8cc"
                        border.width: 1

                        visible: fall < 1
                    }
                }
            }

            // What lands has to end up somewhere
            Rectangle {
                id: puddle

                property real grow

                x: window.junctionX - width / 2
                y: window.groundY - height / 2
                z: 2
                width: 40 + 140 * grow
                height: 14
                radius: 7
                color: "#f7f7f0"
                border.color: "#d8d8cc"
                border.width: 1

                visible: root.phase >= 5 && grow > 0
                opacity: Math.min(1, grow * 4) * 0.9

                NumberAnimation on grow {
                    running: root.phase >= 5
                    from: 0
                    to: 1
                    duration: 2600
                }
            }
        }
    }
}
