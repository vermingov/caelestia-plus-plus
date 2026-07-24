pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Widgets
import Caelestia.Config
import qs.components
import qs.components.controls
import qs.services
import qs.modules.nexus.common

PageBase {
    id: root

    readonly property int maxResults: 30

    // Filtered imperatively (not a binding): the sort reads GpuPrefs
    // assignments, and tracking them would resort the list under the
    // cursor every time a chip is clicked.
    property var shown: []
    property int matchCount

    function refilter(query: string): void {
        const q = query.toLowerCase();
        const matches = [...DesktopEntries.applications.values].filter(a => !q || a.name.toLowerCase().includes(q)).sort((a, b) => {
            const aSet = GpuPrefs.assignments[a.id] !== undefined;
            const bSet = GpuPrefs.assignments[b.id] !== undefined;
            if (aSet !== bSet)
                return aSet ? -1 : 1;
            return a.name.localeCompare(b.name);
        });
        matchCount = matches.length;
        shown = matches.slice(0, maxResults);
    }

    title: qsTr("App GPUs")
    isSubPage: true

    Component.onCompleted: refilter("")

    ColumnLayout {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: parent.top
        width: root.cappedWidth
        spacing: Tokens.spacing.extraSmall / 2

        StyledText {
            visible: !GpuPrefs.multiGpu
            Layout.fillWidth: true
            Layout.bottomMargin: Tokens.spacing.medium
            text: qsTr("Only one GPU detected (%1) — assignments will matter once another GPU is present.").arg(GpuPrefs.gpus[0]?.name ?? "?")
            color: Colours.palette.m3outline
            font: Tokens.font.label.small
            wrapMode: Text.Wrap
        }

        StyledTextField {
            id: searchField

            Layout.fillWidth: true
            Layout.bottomMargin: Tokens.spacing.medium
            leadingIcon: "search"
            placeholderText: qsTr("Search apps…")
            onTextChanged: queryDebounce.restart()

            Timer {
                id: queryDebounce

                interval: 150
                onTriggered: root.refilter(searchField.text)
            }
        }

        StyledText {
            Layout.alignment: Qt.AlignHCenter
            Layout.bottomMargin: Tokens.spacing.small
            visible: root.matchCount > root.maxResults
            text: qsTr("Showing %1 of %2 apps — keep typing to narrow down").arg(root.maxResults).arg(root.matchCount)
            color: Colours.palette.m3outline
            font: Tokens.font.label.small
        }

        Repeater {
            id: list

            model: root.shown

            ConnectedRect {
                id: appItem

                required property DesktopEntry modelData
                required property int index
                readonly property var assignedGpu: GpuPrefs.gpuFor(modelData.id)

                Layout.fillWidth: true
                first: index === 0
                last: index === list.count - 1
                implicitHeight: appRow.implicitHeight + appRow.anchors.margins * 2

                RowLayout {
                    id: appRow

                    anchors.fill: parent
                    anchors.margins: Tokens.padding.medium
                    anchors.leftMargin: Tokens.padding.largeIncreased
                    anchors.rightMargin: Tokens.padding.largeIncreased
                    spacing: Tokens.spacing.medium

                    IconImage {
                        asynchronous: true
                        implicitSize: Math.round(Tokens.font.icon.large.pointSize * 1.8)
                        source: Quickshell.iconPath(appItem.modelData.icon, "image-missing")
                    }

                    StyledText {
                        Layout.fillWidth: true
                        text: appItem.modelData.name
                        font: Tokens.font.body.small
                        elide: Text.ElideRight
                    }

                    TextButton {
                        type: TextButton.Tonal
                        checked: !appItem.assignedGpu
                        text: qsTr("Default")
                        onClicked: GpuPrefs.setGpu(appItem.modelData.id, "")
                    }

                    Repeater {
                        model: GpuPrefs.gpus

                        TextButton {
                            required property var modelData

                            type: TextButton.Tonal
                            checked: appItem.assignedGpu?.slot === modelData.slot
                            text: modelData.name
                            onClicked: GpuPrefs.setGpu(appItem.modelData.id, modelData.slot)
                        }
                    }
                }
            }
        }
    }
}
