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
    property string query
    // One entry per desktop id — DesktopEntries already dedupes multiple
    // .desktop files for the same id, assigned apps sort first
    readonly property var filtered: {
        const q = query.toLowerCase();
        return [...DesktopEntries.applications.values].filter(a => !q || a.name.toLowerCase().includes(q)).sort((a, b) => {
            const aSet = GpuPrefs.assignments[a.id] !== undefined;
            const bSet = GpuPrefs.assignments[b.id] !== undefined;
            if (aSet !== bSet)
                return aSet ? -1 : 1;
            return a.name.localeCompare(b.name);
        });
    }

    title: qsTr("App GPUs")
    isSubPage: true

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
                onTriggered: root.query = searchField.text
            }
        }

        StyledText {
            Layout.alignment: Qt.AlignHCenter
            Layout.bottomMargin: Tokens.spacing.small
            visible: root.filtered.length > root.maxResults
            text: qsTr("Showing %1 of %2 apps — keep typing to narrow down").arg(root.maxResults).arg(root.filtered.length)
            color: Colours.palette.m3outline
            font: Tokens.font.label.small
        }

        Repeater {
            id: list

            model: root.filtered.slice(0, root.maxResults)

            ConnectedRect {
                id: appItem

                required property DesktopEntry modelData
                required property int index
                readonly property var assignedGpu: GpuPrefs.gpuFor(modelData.id)

                Layout.fillWidth: true
                first: index === 0
                last: index === Math.min(list.count, root.maxResults) - 1
                implicitHeight: appRow.implicitHeight + appRow.anchors.margins * 2
                clip: false
                z: gpuButton.expanded ? 1 : 0

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

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 0

                        StyledText {
                            Layout.fillWidth: true
                            text: appItem.modelData.name
                            font: Tokens.font.body.small
                            elide: Text.ElideRight
                        }

                        StyledText {
                            Layout.fillWidth: true
                            visible: text
                            text: appItem.assignedGpu?.name ?? ""
                            color: Colours.palette.m3primary
                            font: Tokens.font.label.small
                            elide: Text.ElideRight
                        }
                    }

                    SplitButton {
                        id: gpuButton

                        type: SplitButton.Tonal
                        fallbackIcon: "memory"
                        fallbackText: qsTr("Default")
                        active: menuItems.find(i => i.text === appItem.assignedGpu?.name) ?? null
                        stateLayer.onClicked: gpuButton.expanded = !gpuButton.expanded
                        menu.onItemSelected: item => {
                            const gpu = GpuPrefs.gpus.find(g => g.name === item.text);
                            GpuPrefs.setGpu(appItem.modelData.id, gpu?.slot ?? "");
                        }
                        menuItems: {
                            const items = [defaultItem];
                            for (let i = 0; i < gpuVariants.instances.length; i++)
                                items.push(gpuVariants.instances[i]);
                            return items;
                        }

                        readonly property MenuItem defaultItem: MenuItem {
                            text: qsTr("Default")
                            icon: appItem.assignedGpu ? "" : "check"
                        }

                        Variants {
                            id: gpuVariants

                            model: GpuPrefs.gpus

                            MenuItem {
                                required property var modelData

                                text: modelData.name
                                icon: appItem.assignedGpu?.slot === modelData.slot ? "check" : ""
                                activeIcon: "memory"
                            }
                        }
                    }
                }
            }
        }
    }
}
