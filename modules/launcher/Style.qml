pragma Singleton

import QtQuick
import Quickshell

// Geometry for the launcher, in one place so the panel, the rows and the list
// agree. The plugin's Tokens.sizes.launcher only carries the old two-line row
// height, which is nearly twice what a single-line row needs.
//
// Plain numbers only. Tokens and Colours are scoped to a screen and a
// singleton has no screen context — reading them here yields unscoped values
// and logs "accessed without a screen set". Anything themed belongs at the
// use site.
Singleton {
    readonly property int panelWidth: 720

    readonly property int rowHeight: 44
    readonly property int rowRadius: 8
    readonly property int iconSize: 22

    // Rows inset from the panel edge; the selection fill uses the same inset
    // so it reads as a pill inside the panel rather than a full-width band
    readonly property int rowInset: 6
    readonly property int contentPadding: 16

    readonly property int searchHeight: 56
    readonly property int footerHeight: 40
    readonly property int sectionHeight: 26
}
