import Quickshell
import qs.services

PersistentProperties {
    required property ShellScreen modelData

    // Drawer visibilities
    property bool bar
    property bool osd
    property bool session
    property bool launcher
    property bool dashboard
    property bool utilities
    property bool sidebar

    // The sidebar was the notification centre, and with the bar installed the
    // bar draws that. Everything that used to open the sidebar still sets
    // this flag: the corner of the screen, a drag in from the edge, the
    // `drawers` IPC. Handing the request on from here covers all of them at
    // once, and putting the flag back means the QML sidebar never opens
    // underneath the real one.
    onSidebarChanged: {
        if (sidebar && Notifs.external) {
            Notifs.openCentre();
            sidebar = false;
        }
    }

    // Dashboard state
    property int dashboardTab
    property date dashboardDate: new Date()
}
