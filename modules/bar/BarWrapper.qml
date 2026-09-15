pragma ComponentBehavior: Bound

import "components"
import QtQuick
import Quickshell
import Caelestia.Config
import qs.components
import qs.components.effects
import qs.services
import qs.utils
import qs.modules.bar.popouts as BarPopouts

Item {
    id: root

    required property ShellScreen screen
    required property ScreenState screenState
    required property BarPopouts.Wrapper popouts
    required property bool fullscreen

    // Stood down entirely while the external bar is installed — it owns the
    // strip, its own exclusive zone and its own popouts, and two bars fighting
    // over the top of the screen is worse than either. Everything below is
    // left intact: uninstalling the other one puts this back.
    readonly property bool disabled: ExternalBar.external || Strings.testRegexList(Config.bar.excludedScreens, screen.name)

    // Nothing at the top at all when the external bar owns the strip. The
    // usual floor exists so the border still reads as a border with the bar
    // hidden; here there is another bar sitting on that exact strip, and the
    // floor showed up underneath it as a band of the background's own glow.
    readonly property int clampedHeight: ExternalBar.external ? 0 : Math.max(Config.border.minThickness, implicitHeight)
    readonly property int padding: Math.max(Tokens.padding.small, Config.border.thickness)
    // Gap between the pill and the screen edges — the "float". Fixed to
    // hyprland gaps_out (variables.lua windowGapsOut = 10, keep in sync) so
    // the pill lines up with tiles without ever moving itself.
    readonly property int floatMargin: 10
    readonly property int pillHeight: Tokens.sizes.bar.innerWidth + padding * 2
    // Endcap style: oversized distro logo capping the pill's left end (the
    // pill starts inside the logo's mouth). Off = regular-size logo as the
    // first row entry, pill fully rounded on both ends. Hiding the logo
    // altogether collapses the endcap the same way.
    readonly property bool logoEndcap: ShellPrefs.barLogoEndcap && ShellPrefs.barLogoShow
    readonly property real logoScale: ShellPrefs.barLogoScale
    readonly property int logoInset: logoEndcap ? Math.round(pillHeight * 0.1) : 0
    // Where the bar row itself starts (past the logo endcap). Hit-testing in
    // checkPopout/handleWheel must subtract this exact inset, or hover
    // targets drift left of the rendered icons. Scales and shifts with the
    // logo so content never slides under it.
    readonly property int contentLeftInset: logoEndcap ? floatMargin + logoInset + Math.round(44 * logoScale) + Math.max(0, ShellPrefs.barLogoOffsetX) : floatMargin
    // Pill hangs flush at the wrapper bottom so popout blobs merge into it;
    // tiles add their own gaps_out below the zone, matching the side gaps
    readonly property int contentHeight: pillHeight + floatMargin
    readonly property int exclusiveZone: !disabled && (Config.bar.persistent || screenState.bar) ? contentHeight : Config.border.thickness
    readonly property bool shouldBeVisible: !fullscreen && !disabled && (Config.bar.persistent || screenState.bar || isHovered)
    property bool isHovered

    function barItem(): Bar {
        return (content.item as Bar) ?? null;
    }

    function closeTray(): void {
        barItem()?.closeTray();
    }

    function checkPopout(x: real): void {
        barItem()?.checkPopout(x - contentLeftInset);
    }

    function handleWheel(x: real, angleDelta: point): void {
        barItem()?.handleWheel(x - contentLeftInset, angleDelta);
    }

    // No clip: the logo endcap is centred on the pill and overhangs the
    // wrapper bottom by a few px; hidden state is gated by `visible` anyway
    visible: height > Config.border.thickness
    implicitHeight: fullscreen ? 0 : Config.border.thickness

    states: State {
        name: "visible"
        when: root.shouldBeVisible

        PropertyChanges {
            root.implicitHeight: root.contentHeight
        }
    }

    transitions: [
        Transition {
            from: ""
            to: "visible"

            Anim {
                target: root
                property: "implicitHeight"
            }
        },
        Transition {
            from: "visible"
            to: ""

            Anim {
                target: root
                property: "implicitHeight"
                type: Anim.Emphasized
            }
        }
    ]

    // Spectrum sits between the pill background (drawn in ContentWindow's
    // blob layer) and the bar entries loaded below
    Loader {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom

        anchors.leftMargin: root.floatMargin + root.logoInset
        anchors.rightMargin: root.floatMargin

        height: root.pillHeight
        active: root.shouldBeVisible

        sourceComponent: BarVisualiser {}
    }

    // Distro logo as the pill's left endcap; replaces the old in-row entry,
    // keeping its launcher toggle and hover feedback
    Item {
        id: logoCap

        anchors.left: parent.left
        anchors.bottom: parent.bottom
        anchors.leftMargin: root.floatMargin + ShellPrefs.barLogoOffsetX
        // centre the C on the pill, lifted 1px: the glyph's bottom bevel is
        // visually heavier, so true centring reads low
        anchors.bottomMargin: -Math.round((height - root.pillHeight) / 2) + 1 - ShellPrefs.barLogoOffsetY

        width: Math.round(root.pillHeight * 1.38 * root.logoScale)
        height: Math.round(root.pillHeight * 1.38 * root.logoScale)
        visible: root.shouldBeVisible && root.logoEndcap

        scale: logoMouse.pressed ? 0.95 : logoMouse.containsMouse ? 1.08 : 1

        Behavior on scale {
            Anim {
                type: Anim.FastSpatial
            }
        }

        // Monochrome svg logos take the accent tint; raster images (png/webp/
        // jpg) are someone's actual artwork and render untinted
        readonly property bool rasterLogo: /\.(png|webp|jpe?g|tiff?)$/i.test(ShellPrefs.barLogoSource)

        ColouredIcon {
            visible: !logoCap.rasterLogo
            anchors.centerIn: parent
            // rounded-corner variant of SysInfo.osLogo (assets/cachyos-rounded.svg),
            // or the user's own image from prefs
            source: logoCap.rasterLogo ? "" : (ShellPrefs.barLogoSource || Qt.resolvedUrl("../../assets/cachyos-rounded.svg"))
            implicitSize: Math.round(root.pillHeight * 1.38 * root.logoScale)
            colour: Colours.palette.m3primary
        }

        Image {
            visible: logoCap.rasterLogo
            anchors.centerIn: parent
            source: logoCap.rasterLogo ? ShellPrefs.barLogoSource : ""
            width: Math.round(root.pillHeight * 1.38 * root.logoScale)
            height: width
            sourceSize: Qt.size(width, height)
            fillMode: Image.PreserveAspectFit
            smooth: true
            asynchronous: true
        }

        MouseArea {
            id: logoMouse

            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: {
                const screenState = ShellState.forActive();
                screenState.launcher = !screenState.launcher;
            }
        }
    }

    Loader {
        id: content

        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom

        anchors.leftMargin: root.contentLeftInset
        anchors.rightMargin: root.floatMargin

        active: root.shouldBeVisible

        sourceComponent: Bar {
            height: root.pillHeight
            screen: root.screen
            screenState: root.screenState
            popouts: root.popouts // qmllint disable incompatible-type
            fullscreen: root.fullscreen
        }
    }
}
