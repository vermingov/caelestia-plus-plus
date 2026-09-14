import QtQuick
import QtQuick.Layouts
import Quickshell
import Caelestia
import Caelestia.Config
import qs.components
import qs.components.controls
import qs.services
import qs.modules.nexus.common

PageBase {
    id: root

    title: qsTr("Updates")

    ColumnLayout {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: parent.top
        width: root.cappedWidth
        spacing: Tokens.spacing.extraSmall / 2

        // Status hero
        ConnectedRect {
            Layout.fillWidth: true
            first: true
            last: true
            implicitHeight: hero.implicitHeight + Tokens.padding.extraLarge * 2

            ColumnLayout {
                id: hero

                anchors.centerIn: parent
                width: parent.width - Tokens.padding.largeIncreased * 2
                spacing: Tokens.spacing.small

                MaterialIcon {
                    Layout.alignment: Qt.AlignHCenter
                    text: {
                        if (ShellUpdates.lastError)
                            return "cloud_off";
                        if (ShellUpdates.updateAvailable)
                            return "deployed_code_update";
                        return "check_circle";
                    }
                    color: ShellUpdates.updateAvailable ? Colours.palette.m3primary : Colours.palette.m3onSurfaceVariant
                    fontStyle: Tokens.font.icon.extraLarge
                    fill: 1
                }

                StyledText {
                    Layout.alignment: Qt.AlignHCenter
                    text: {
                        if (ShellUpdates.checking)
                            return qsTr("Checking for updates…");
                        if (ShellUpdates.updating)
                            return qsTr("Updating…");
                        if (ShellUpdates.lastError)
                            return ShellUpdates.lastError;
                        if (ShellUpdates.updateAvailable)
                            return qsTr("%1 is available").arg(ShellUpdates.latestRelease || qsTr("A new release"));
                        return qsTr("Caelestia++ is up to date");
                    }
                    font: Tokens.font.title.large
                }

                StyledText {
                    Layout.alignment: Qt.AlignHCenter
                    visible: ShellUpdates.lastChecked.length > 0
                    text: qsTr("Last checked at %1").arg(ShellUpdates.lastChecked)
                    color: Colours.palette.m3onSurfaceVariant
                    font: Tokens.font.body.medium
                }

                RowLayout {
                    Layout.alignment: Qt.AlignHCenter
                    Layout.topMargin: Tokens.spacing.small
                    spacing: Tokens.spacing.medium

                    IconTextButton {
                        icon: "refresh"
                        text: qsTr("Check now")
                        font: Tokens.font.body.large
                        isRound: true
                        shapeMorph: true
                        type: IconTextButton.Tonal
                        disabled: ShellUpdates.checking || ShellUpdates.updating
                        onClicked: ShellUpdates.check()
                    }

                    IconTextButton {
                        visible: ShellUpdates.updateAvailable
                        icon: "download"
                        text: qsTr("Update & restart")
                        font: Tokens.font.body.large
                        isRound: true
                        shapeMorph: true
                        type: IconTextButton.Filled
                        disabled: ShellUpdates.updating
                        onClicked: ShellUpdates.update()
                    }
                }
            }
        }

        // What the pending release contains
        SectionHeader {
            visible: ShellUpdates.updateAvailable
            text: ShellUpdates.latestRelease ? qsTr("What's new in %1").arg(ShellUpdates.latestRelease) : qsTr("What's new")
        }

        Repeater {
            model: ShellUpdates.changelog

            ConnectedRect {
                required property string modelData
                required property int index

                Layout.fillWidth: true
                first: index === 0
                last: index === ShellUpdates.changelog.length - 1
                implicitHeight: changeText.implicitHeight + Tokens.padding.large * 2

                StyledText {
                    id: changeText

                    anchors.verticalCenter: parent.verticalCenter
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.margins: Tokens.padding.large
                    text: modelData
                    font: Tokens.font.body.medium
                    elide: Text.ElideRight
                }
            }
        }

        // Settings
        SectionHeader {
            text: qsTr("Settings")
        }

        ToggleRow {
            first: true
            last: true
            text: qsTr("Automatic updates")
            subtext: qsTr("Apply new updates on startup so you're always current")
            checked: ShellUpdates.autoUpdate
            onToggled: ShellUpdates.autoUpdate = checked
        }

        // Installed
        SectionHeader {
            text: qsTr("Installed")
        }

        InfoRow {
            first: true
            label: qsTr("Version")
            value: CUtils.version ? `v${CUtils.version}` : "…"
        }

        InfoRow {
            label: qsTr("Release")
            value: ShellUpdates.latestRelease && !ShellUpdates.updateAvailable ? ShellUpdates.latestRelease : qsTr("between releases")
        }

        InfoRow {
            last: true
            label: qsTr("Revision")
            value: ShellUpdates.headCommit || "…"
        }

        // Back to upstream
        SectionHeader {
            text: qsTr("Switch back")
        }

        ConnectedRect {
            Layout.fillWidth: true
            first: true
            last: true
            implicitHeight: revertRow.implicitHeight + Tokens.padding.medium * 2

            RowLayout {
                id: revertRow

                anchors.verticalCenter: parent.verticalCenter
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.leftMargin: Tokens.padding.largeIncreased
                anchors.rightMargin: Tokens.padding.largeIncreased
                spacing: Tokens.spacing.medium

                Column {
                    Layout.fillWidth: true
                    spacing: 0

                    StyledText {
                        text: qsTr("Back to regular caelestia")
                        font: Tokens.font.body.large
                    }

                    StyledText {
                        width: parent.width
                        text: qsTr("Swaps the packages for upstream's and removes the privileged halves. Settings, schemes, wallpapers and saved rules stay; this checkout is moved aside, not deleted.")
                        color: Colours.palette.m3onSurfaceVariant
                        font: Tokens.font.body.small
                        wrapMode: Text.WordWrap
                    }
                }

                IconTextButton {
                    icon: "undo"
                    text: qsTr("Revert")
                    font: Tokens.font.body.large
                    isRound: true
                    shapeMorph: true
                    type: IconTextButton.Tonal
                    // In a terminal on purpose: it prints what it is about to
                    // do, asks you to type `revert`, and the AUR helper wants
                    // a password. All three need somewhere to be typed.
                    onClicked: Quickshell.execDetached([...GlobalConfig.general.apps.terminal, "fish", "-C", "exec cae revert"])
                }
            }
        }
    }
}
