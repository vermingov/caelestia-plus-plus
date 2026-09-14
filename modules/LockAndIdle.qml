import "lock"
import Quickshell

// Loaded by URL from shell.qml once the bar is on screen: the lock screen
// and the idle monitors are not needed for the first frame, and the lock
// module is a sizeable compile.
Scope {
    Lock {
        id: lock
    }

    IdleMonitors {
        lock: lock
    }
}
