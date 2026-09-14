pragma ComponentBehavior: Bound

import QtQuick
import Quickshell.Services.UPower
import Caelestia.Config
import qs.components
import qs.components.controls
import qs.services
import qs.utils

Column {
    id: root

    spacing: Tokens.spacing.medium
    width: Tokens.sizes.bar.batteryWidth

    StyledText {
        text: UPower.displayDevice.isLaptopBattery ? qsTr("Remaining: %1%").arg(Math.round(UPower.displayDevice.percentage * 100)) : qsTr("No battery detected")
    }

    StyledText {
        function formatSeconds(s: int, fallback: string): string {
            const day = Math.floor(s / 86400);
            const hr = Math.floor(s / 3600) % 24;
            const min = Math.floor(s / 60) % 60;

            let comps = [];
            if (day > 0)
                comps.push(`${day} days`);
            if (hr > 0)
                comps.push(`${hr} hours`);
            if (min > 0)
                comps.push(`${min} mins`);

            return comps.join(", ") || fallback;
        }

        text: UPower.displayDevice.isLaptopBattery ? qsTr("Time %1: %2").arg(UPower.onBattery ? "remaining" : "until charged").arg(UPower.onBattery ? formatSeconds(UPower.displayDevice.timeToEmpty, "Calculating...") : formatSeconds(UPower.displayDevice.timeToFull, "Fully charged!")) : qsTr("Power profile: %1").arg(PowerDaemon.profileLabel)
    }

    Loader {
        asynchronous: true
        anchors.horizontalCenter: parent.horizontalCenter

        active: PowerDaemon.degradation !== ""

        height: active ? ((item as Item)?.implicitHeight ?? 0) : 0

        sourceComponent: StyledRect {
            implicitWidth: child.implicitWidth + Tokens.padding.medium * 2
            implicitHeight: child.implicitHeight + Tokens.padding.large

            color: Colours.palette.m3error
            radius: Tokens.rounding.large

            Column {
                id: child

                anchors.centerIn: parent

                Row {
                    anchors.horizontalCenter: parent.horizontalCenter
                    spacing: Tokens.spacing.small

                    MaterialIcon {
                        anchors.verticalCenter: parent.verticalCenter
                        anchors.verticalCenterOffset: -font.pointSize / 10

                        text: "warning"
                        color: Colours.palette.m3onError
                    }

                    StyledText {
                        anchors.verticalCenter: parent.verticalCenter
                        text: qsTr("Performance Degraded")
                        color: Colours.palette.m3onError
                        font: Tokens.font.mono.builders.medium.weight(Font.Medium).build()
                    }

                    MaterialIcon {
                        anchors.verticalCenter: parent.verticalCenter
                        anchors.verticalCenterOffset: -font.pointSize / 10

                        text: "warning"
                        color: Colours.palette.m3onError
                    }
                }

                StyledText {
                    anchors.horizontalCenter: parent.horizontalCenter

                    text: qsTr("Reason: %1").arg(PowerDaemon.degradation)
                    color: Colours.palette.m3onError
                }
            }
        }
    }

    StyledRect {
        id: profiles

        property string current: {
            if (Dynamic.enabled)
                return dyn.icon;
            const p = PowerDaemon.profile;
            if (p === "power-saver")
                return saver.icon;
            if (p === "performance")
                return perf.icon;
            return balance.icon;
        }

        anchors.horizontalCenter: parent.horizontalCenter

        implicitWidth: saver.implicitWidth + balance.implicitWidth + perf.implicitWidth + dyn.implicitWidth + Tokens.spacing.largeIncreased * 3 + Tokens.padding.extraSmall * 2
        implicitHeight: saver.implicitHeight + Tokens.padding.small

        color: Colours.tPalette.m3surfaceContainer
        radius: Tokens.rounding.full
        border.width: 1
        border.color: Qt.alpha(Colours.palette.m3outlineVariant, 0.4)

        StyledRect {
            id: indicator

            color: Colours.palette.m3primary
            radius: Tokens.rounding.full
            state: profiles.current

            states: [
                State {
                    name: saver.icon
                    Fill { item: saver }
                },
                State {
                    name: balance.icon
                    Fill { item: balance }
                },
                State {
                    name: perf.icon
                    Fill { item: perf }
                },
                State {
                    name: dyn.icon
                    Fill { item: dyn }
                }
            ]

            transitions: Transition {
                AnchorAnim {}
            }
        }

        Profile {
            id: saver

            anchors.verticalCenter: parent.verticalCenter
            anchors.left: parent.left
            anchors.leftMargin: Tokens.padding.extraSmall

            profile: "power-saver"
            icon: "energy_savings_leaf"
        }

        Profile {
            id: balance

            anchors.verticalCenter: parent.verticalCenter
            anchors.left: saver.right
            anchors.leftMargin: Tokens.spacing.largeIncreased

            profile: "balanced"
            icon: "balance"
        }

        Profile {
            id: perf

            anchors.verticalCenter: parent.verticalCenter
            anchors.left: balance.right
            anchors.leftMargin: Tokens.spacing.largeIncreased

            profile: "performance"
            icon: "rocket_launch"
        }

        Profile {
            id: dyn

            anchors.verticalCenter: parent.verticalCenter
            anchors.left: perf.right
            anchors.leftMargin: Tokens.spacing.largeIncreased

            dynamic: true
            icon: "auto_mode"
        }
    }

    StyledText {
        width: root.width
        visible: !PowerDaemon.ready

        text: qsTr("Power daemon unreachable — reconnecting; your choice applies when it's back")
        color: Colours.palette.m3error
        font: Tokens.font.body.small
        wrapMode: Text.WordWrap
    }

    StyledText {
        width: root.width
        visible: Dynamic.enabled

        text: {
            const t = Dynamic.currentTier;
            const label = t === "power-saver" ? qsTr("Eco") : t === "balanced" ? qsTr("Balanced") : t === "performance" ? qsTr("Performance") : t === "yield" ? qsTr("paused — Max performance on") : qsTr("starting…");
            return qsTr("Auto-switching by load — now: %1").arg(label);
        }
        color: Colours.palette.m3onSurfaceVariant
        font: Tokens.font.body.small
        wrapMode: Text.WordWrap
    }

    Item {
        width: root.width
        height: visible ? Math.max(bedIcon.implicitHeight, bedLabel.implicitHeight, bedSwitch.implicitHeight) : 0
        // Bed mode is a laptop fan curve; desktops can still get this popout
        // via peripheral/UPS batteries
        visible: SysInfo.isLaptop

        MaterialIcon {
            id: bedIcon

            anchors.left: parent.left
            anchors.verticalCenter: parent.verticalCenter

            text: "bed"
            color: Colours.palette.m3onSurfaceVariant
        }

        StyledText {
            id: bedLabel

            anchors.left: bedIcon.right
            anchors.leftMargin: Tokens.spacing.medium
            anchors.right: bedSwitch.left
            anchors.rightMargin: Tokens.spacing.medium
            anchors.verticalCenter: parent.verticalCenter

            text: qsTr("Bed mode")
            elide: Text.ElideRight
        }

        StyledSwitch {
            id: bedSwitch

            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter

            checked: BedMode.enabled
            onToggled: BedMode.setEnabled(checked)
        }
    }

    StyledText {
        width: root.width
        visible: SysInfo.isLaptop

        text: qsTr("Much more sensitive fan curve for restricted airflow, e.g. on a bed")
        color: Colours.palette.m3onSurfaceVariant
        font: Tokens.font.body.small
        wrapMode: Text.WordWrap
    }

    component Fill: AnchorChanges {
        required property Item item

        target: indicator
        anchors.left: item.left
        anchors.right: item.right
        anchors.top: item.top
        anchors.bottom: item.bottom
    }

    component Profile: Item {
        required property string icon
        property string profile: ""
        property bool dynamic: false

        implicitWidth: icon.implicitHeight + Tokens.padding.small
        implicitHeight: icon.implicitHeight + Tokens.padding.small

        StateLayer {
            id: profileLayer

            radius: Tokens.rounding.full
            color: profiles.current === parent.icon ? Colours.palette.m3onPrimary : Colours.palette.m3onSurface
            onClicked: {
                if (parent.dynamic) {
                    Dynamic.setEnabled(true);
                } else {
                    Dynamic.setEnabled(false);
                    // Bed mode owns the plan at balanced and holds CPU boost
                    // off; asking for performance means leaving it
                    if (parent.profile === "performance")
                        BedMode.setEnabled(false);
                    PowerDaemon.setProfile(parent.profile);
                }
            }
        }

        MaterialIcon {
            id: icon

            anchors.centerIn: parent

            text: parent.icon
            fontStyle: Tokens.font.icon.large
            color: profiles.current === text ? Colours.palette.m3onPrimary : Colours.palette.m3onSurfaceVariant
            fill: profiles.current === text ? 1 : 0
            scale: profileLayer.pressed ? 0.9 : 1

            Behavior on scale {
                Anim {
                    type: Anim.FastSpatial
                }
            }

            Behavior on fill {
                Anim {
                    type: Anim.DefaultEffects
                }
            }
        }
    }
}
