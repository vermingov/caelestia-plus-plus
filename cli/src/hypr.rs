//! Talking to Hyprland over its control socket.
//!
//! Two things make this more than a socket write. Replies are read to the end
//! rather than in one recv, because `clients` on a busy session is far larger
//! than any single read; and on a Lua-configured Hyprland a dispatch is not a
//! dispatcher name at all but a line of Lua, so the few dispatchers this CLI
//! uses are translated. Getting that wrong means the keybind silently does
//! nothing, which is exactly how it fails.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::OnceLock;

use redcommon::json::{self, Json};

fn socket_path() -> Option<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")?;
    let signature = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    Some(
        PathBuf::from(runtime)
            .join("hypr")
            .join(signature)
            .join(".socket.sock"),
    )
}

fn request(payload: &str) -> Option<String> {
    let mut sock = UnixStream::connect(socket_path()?).ok()?;
    sock.write_all(payload.as_bytes()).ok()?;
    let mut reply = String::new();
    sock.read_to_string(&mut reply).ok()?;
    Some(reply)
}

/// A JSON query: `clients`, `monitors`, `status`.
pub fn message(what: &str) -> Option<Json> {
    json::parse(request(&format!("j/{what}"))?.trim())
}

/// Hyprland answers "ok" to a dispatch it accepted.
fn dispatch_raw(payload: &str) -> bool {
    request(payload).is_some_and(|r| r.trim() == "ok")
}

/// Whether this Hyprland is driven by a Lua config, cached for the life of
/// the process — it cannot change underneath a single command.
fn lua_config() -> bool {
    static LUA: OnceLock<bool> = OnceLock::new();
    *LUA.get_or_init(|| {
        message("status")
            .and_then(|s| s.str_field("configProvider").map(|p| p == "lua"))
            .unwrap_or(false)
    })
}

pub fn toggle_special_workspace(name: &str) -> bool {
    dispatch_raw(&toggle_payload(lua_config(), name))
}

pub fn move_to_workspace_silent(workspace: &str, address: &str) -> bool {
    dispatch_raw(&move_payload(lua_config(), workspace, address))
}

pub fn exec(command: &str) -> bool {
    dispatch_raw(&exec_payload(lua_config(), command))
}

// The payloads are built apart from the socket so they can be checked against
// what the Python CLI sends, which is the only definition of "right" here: a
// wrong Lua line is accepted by Hyprland and does nothing at all.

fn toggle_payload(lua: bool, name: &str) -> String {
    if lua {
        let call = if name.is_empty() {
            "hl.dsp.workspace.toggle_special()".to_string()
        } else {
            format!("hl.dsp.workspace.toggle_special(\"{name}\")")
        };
        return format!("dispatch {call}");
    }
    format!("dispatch togglespecialworkspace {name}")
        .trim_end()
        .to_string()
}

fn move_payload(lua: bool, workspace: &str, address: &str) -> String {
    if lua {
        return format!(
            "dispatch hl.dsp.window.move({{window = \"address:{address}\", workspace = \"{workspace}\", follow = false}})"
        );
    }
    format!("dispatch movetoworkspacesilent {workspace},address:{address}")
}

fn exec_payload(lua: bool, command: &str) -> String {
    if lua {
        let escaped = command.replace('\\', "\\\\").replace('"', "\\\"");
        return format!("dispatch hl.dsp.exec_cmd(\"{escaped}\")");
    }
    format!("dispatch exec {command}")
}

/// Every client, as Hyprland reports them.
pub fn clients() -> Vec<Json> {
    match message("clients") {
        Some(Json::Arr(items)) => items,
        _ => Vec::new(),
    }
}

pub fn monitors() -> Vec<Json> {
    match message("monitors") {
        Some(Json::Arr(items)) => items,
        _ => Vec::new(),
    }
}

pub fn focused_monitor() -> Option<Json> {
    monitors().into_iter().find(|m| m.bool_field("focused", false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_with_no_hyprland_is_absent_not_a_panic() {
        // The CLI runs from a TTY and from scripts too; nothing here may
        // depend on a compositor being there.
        let saved = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE");
        std::env::remove_var("HYPRLAND_INSTANCE_SIGNATURE");
        assert!(socket_path().is_none());
        assert!(message("clients").is_none());
        assert!(clients().is_empty());
        if let Some(v) = saved {
            std::env::set_var("HYPRLAND_INSTANCE_SIGNATURE", v);
        }
    }

    #[test]
    fn a_lua_hyprland_is_sent_lua() {
        assert_eq!(
            toggle_payload(true, "sysmon"),
            r#"dispatch hl.dsp.workspace.toggle_special("sysmon")"#
        );
        assert_eq!(
            toggle_payload(true, ""),
            "dispatch hl.dsp.workspace.toggle_special()"
        );
        assert_eq!(
            move_payload(true, "special:music", "0x55d1"),
            r#"dispatch hl.dsp.window.move({window = "address:0x55d1", workspace = "special:music", follow = false})"#
        );
        assert_eq!(
            exec_payload(true, "[workspace special:sysmon] foot -a btop"),
            r#"dispatch hl.dsp.exec_cmd("[workspace special:sysmon] foot -a btop")"#
        );
    }

    #[test]
    fn quotes_and_backslashes_survive_the_trip_into_lua() {
        assert_eq!(
            exec_payload(true, r#"foot -e sh -c "echo \ ""#),
            r#"dispatch hl.dsp.exec_cmd("foot -e sh -c \"echo \\ \"")"#
        );
    }

    #[test]
    fn a_plain_hyprland_is_sent_dispatchers() {
        assert_eq!(
            toggle_payload(false, "sysmon"),
            "dispatch togglespecialworkspace sysmon"
        );
        assert_eq!(
            toggle_payload(false, ""),
            "dispatch togglespecialworkspace",
            "no trailing space for a nameless toggle"
        );
        assert_eq!(
            move_payload(false, "special:music", "0x55d1"),
            "dispatch movetoworkspacesilent special:music,address:0x55d1"
        );
        assert_eq!(exec_payload(false, "foot"), "dispatch exec foot");
    }

    /// Read-only, and skipped where there is no compositor.
    #[test]
    fn reads_the_live_compositor() {
        if socket_path().is_none_or(|p| !p.exists()) {
            return;
        }
        let monitors = monitors();
        assert!(!monitors.is_empty(), "a running Hyprland has monitors");
        assert!(monitors.iter().all(|m| m.str_field("name").is_some()));

        if let Some(focused) = focused_monitor() {
            assert!(focused.bool_field("focused", false));
        }
        for client in clients() {
            assert!(client.str_field("address").is_some(), "every client has an address");
            assert!(client.get("workspace").is_some());
        }
    }
}
