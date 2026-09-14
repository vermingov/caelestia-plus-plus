import "performance"
import QtQuick
import QtQuick.Layouts
import Quickshell.Services.UPower
import Caelestia.Config
import Caelestia.Services
import qs.components
import qs.services

Item {
    id: root

    implicitWidth: placeholder.active ? Tokens.sizes.dashboard.perfPlaceholderWidth : content.implicitWidth
    implicitHeight: placeholder.active ? placeholder.implicitHeight + Tokens.padding.extraLarge * 2 : content.implicitHeight

    Loader {
        id: placeholder

        anchors.centerIn: parent
        active: !Config.dashboard.performance.showCpu && !(Config.dashboard.performance.showGpu && Gpu.type !== Gpu.None) && !Config.dashboard.performance.showMemory && !Config.dashboard.performance.showStorage && !Config.dashboard.performance.showNetwork && !(UPower.displayDevice.isLaptopBattery && Config.dashboard.performance.showBattery)
        asynchronous: true

        sourceComponent: ColumnLayout {
            spacing: Tokens.spacing.medium

            MaterialIcon {
                Layout.alignment: Qt.AlignHCenter
                text: "tune"
                fontStyle: Tokens.font.icon.builders.extraLarge.scale(2).build()
                color: Colours.palette.m3onSurfaceVariant
            }

            StyledText {
                Layout.topMargin: -Tokens.spacing.small
                Layout.alignment: Qt.AlignHCenter
                text: qsTr("No widgets enabled")
                font: Tokens.font.title.large
                color: Colours.palette.m3onSurface
            }

            StyledText {
                Layout.alignment: Qt.AlignHCenter
                text: qsTr("Enable widgets in the dashboard settings")
                font: Tokens.font.body.small
                color: Colours.palette.m3onSurfaceVariant
            }
        }
    }

    RowLayout {
        id: content

        anchors.left: parent.left
        anchors.right: parent.right
        // Panel height is locked to the tallest tab; center instead of
        // leaving the slack as a dead band under the cards
        anchors.verticalCenter: parent.verticalCenter
        spacing: Tokens.spacing.medium
        visible: !placeholder.active

        ColumnLayout {
            id: mainColumn

            Layout.fillWidth: true
            spacing: Tokens.spacing.medium

            RowLayout {
                spacing: Tokens.spacing.medium
                visible: cpuCard.active || gpuCard.active

                WrappedLoader {
                    id: cpuCard

                    active: Config.dashboard.performance.showCpu
                    stagger: 0

                    sourceComponent: HeroCard {
                        icon: "memory"
                        label: qsTr("CPU")
                        subLabel: Cpu.name
                        usage: Cpu.percentage
                        temperature: Cpu.temperature
                        accent: Colours.palette.m3primary

                        // Resident pane: hold the poller only while on screen
                        Loader {
                            active: root.visible

                            sourceComponent: ServiceRef {
                                service: Cpu
                            }
                        }
                    }
                }

                WrappedLoader {
                    id: gpuCard

                    active: Config.dashboard.performance.showGpu && Gpu.type !== Gpu.None
                    stagger: 1

                    sourceComponent: HeroCard {
                        icon: "desktop_windows"
                        label: qsTr("GPU")
                        subLabel: Gpu.name
                        usage: Gpu.percentage
                        temperature: Gpu.temperature
                        accent: Colours.palette.m3secondary

                        Loader {
                            active: root.visible

                            sourceComponent: ServiceRef {
                                service: Gpu
                            }
                        }
                    }
                }
            }

            RowLayout {
                spacing: Tokens.spacing.medium
                visible: storageCard.active || networkCard.active || memoryCard.active

                WrappedLoader {
                    id: storageCard

                    active: Config.dashboard.performance.showStorage
                    stagger: 2
                    sourceComponent: StorageCard {}
                }

                WrappedLoader {
                    id: networkCard

                    active: Config.dashboard.performance.showNetwork
                    stagger: 3
                    sourceComponent: NetworkCard {}
                }

                WrappedLoader {
                    id: memoryCard

                    active: Config.dashboard.performance.showMemory
                    stagger: 4
                    sourceComponent: MemoryCard {}
                }
            }
        }

        WrappedLoader {
            Layout.fillWidth: false
            active: UPower.displayDevice.isLaptopBattery && Config.dashboard.performance.showBattery
            stagger: 5
            sourceComponent: BatteryTank {}
        }
    }

    component WrappedLoader: Loader {
        id: loader

        property int stagger: 0
        property real revealOffset: Tokens.padding.large

        Layout.fillWidth: true
        Layout.fillHeight: true
        visible: active
        opacity: 0

        transform: Translate {
            y: loader.revealOffset
        }

        SequentialAnimation {
            running: loader.status === Loader.Ready

            PauseAnimation {
                duration: loader.stagger * 40
            }
            ParallelAnimation {
                Anim {
                    target: loader
                    property: "opacity"
                    to: 1
                    type: Anim.DefaultEffects
                }
                Anim {
                    target: loader
                    property: "revealOffset"
                    to: 0
                    type: Anim.DefaultSpatial
                }
            }
        }
    }
}
