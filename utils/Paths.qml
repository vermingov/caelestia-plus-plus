pragma Singleton

import QtQuick
import Quickshell
import Caelestia
import Caelestia.Config

Singleton {
    id: root

    readonly property string home: Quickshell.env("HOME")
    readonly property string pictures: Quickshell.env("XDG_PICTURES_DIR") || `${home}/Pictures`
    readonly property string videos: Quickshell.env("XDG_VIDEOS_DIR") || `${home}/Videos`

    readonly property string data: `${Quickshell.env("XDG_DATA_HOME") || `${home}/.local/share`}/caelestia`
    readonly property string state: `${Quickshell.env("XDG_STATE_HOME") || `${home}/.local/state`}/caelestia`
    readonly property string cache: `${Quickshell.env("XDG_CACHE_HOME") || `${home}/.cache`}/caelestia`
    readonly property string config: `${Quickshell.env("XDG_CONFIG_HOME") || `${home}/.config`}/caelestia`

    readonly property string imagecache: `${cache}/imagecache`
    readonly property string notifimagecache: `${imagecache}/notifs`
    readonly property string wallsdir: Quickshell.env("CAELESTIA_WALLPAPERS_DIR") || absolutePath(GlobalConfig.paths.wallpaperDir)
    readonly property string recsdir: Quickshell.env("CAELESTIA_RECORDINGS_DIR") || `${videos}/Recordings`
    readonly property string libdir: Quickshell.env("CAELESTIA_LIB_DIR") || "/usr/lib/caelestia"
    // Where the install scripts put the shell's own programs: the bar, the
    // launcher, the tools.
    readonly property string bin: Quickshell.env("XDG_BIN_HOME") || `${home}/.local/bin`

    // A command that prints where `program` is, and fails when it is nowhere.
    //
    // `bin` is looked in before PATH, because PATH is whatever the session was
    // handed. A login shell that only extends it in ~/.zshrc leaves the desktop
    // without ~/.local/bin, so a shell started at login could not find a bar
    // that was installed and working — while the same shell restarted from a
    // terminal could, which is how that went unnoticed.
    function locate(program: string): var {
        return ["sh", "-c", 'p="$1/$2"; [ -x "$p" ] || p=$(command -v "$2") || exit 1; echo "$p"', "sh", bin, program];
    }

    function toLocalFile(path: url): string {
        path = Qt.resolvedUrl(path);
        return path.toString() ? CUtils.toLocalFile(path) : "";
    }

    function absolutePath(path: string): string {
        return toLocalFile(path.replace(/~|(\$({?)HOME(}?))+/, home));
    }

    function shortenHome(path: string): string {
        return path.replace(home, "~");
    }
}
