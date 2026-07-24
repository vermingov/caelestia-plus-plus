#!/usr/bin/env python3
"""Bridge between the Caelestia++ settings UI and the HyprMod Hyprland customizer.

Curated knobs (variables.lua):
  dump                    scalar knobs as JSON
  set KEY VALUE           rewrite knob, apply live (eval or reload)

Full option surface (overrides on top of the lua config):
  schema                  hyprctl descriptions passthrough (all 341 options)
  overrides               current overrides state as JSON
  set-option NAME VALUE   override any option (name like decoration:blur:size)
  unset-option NAME       drop an override, reload to restore config value

Custom keybinds:
  add-bind COMBO KIND VALUE [FLAGS]   kind: exec | global | lua
  del-bind INDEX                      flags: comma list of locked,release,repeat
  binds                   custom binds as JSON

Monitor layout:
  set-monitor NAME JSON   save one monitor's config ({mode, position, scale,
                          transform?, vrr?, disabled?, mirror?}), apply live
  del-monitor NAME        forget a monitor (falls back to the default rule)
  set-primary NAME        pin workspace 1 to NAME ("" clears)

State lives in ~/.config/caelestia/hyprmod-overrides.json; every change
regenerates hyprmod-overrides.lua, which hyprland.lua loads last so
overrides win over the base config.
"""

import glob
import json
import os
import re
import socket
import subprocess
import sys

VARIABLES = os.path.expanduser("~/.config/hypr/variables.lua")
STATE = os.path.expanduser("~/.config/caelestia/hyprmod-overrides.json")
GENERATED = os.path.expanduser("~/.config/caelestia/hyprmod-overrides.lua")

KNOB_LINE = re.compile(r"^(\s*)(\w+)(\s*=\s*)(.+?)(,\s*)$")

EVAL_PATHS = {
    "blurEnabled": "decoration.blur.enabled",
    "blurXray": "decoration.blur.xray",
    "blurSpecialWs": "decoration.blur.special",
    "blurPopups": "decoration.blur.popups",
    "blurInputMethods": "decoration.blur.input_methods",
    "blurSize": "decoration.blur.size",
    "blurPasses": "decoration.blur.passes",
    "shadowEnabled": "decoration.shadow.enabled",
    "shadowRange": "decoration.shadow.range",
    "shadowRenderPower": "decoration.shadow.render_power",
    "windowRounding": "decoration.rounding",
    "workspaceGaps": "general.gaps_workspaces",
    "windowGapsIn": "general.gaps_in",
    "windowGapsOut": "general.gaps_out",
    "windowBorderSize": "general.border_size",
    "touchpadDisableTyping": "input.touchpad.disable_while_typing",
    "touchpadScrollFactor": "input.touchpad.scroll_factor",
}

BIND_FLAGS = {"locked", "release", "repeat", "mouse", "non_consuming"}


# ---- lua encoding ----

def to_lua(value) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (int, float)):
        return f"{value:g}"
    return json.dumps(str(value))  # double-quoted + escaped, lua-compatible


def nested_config(path_parts: list, value) -> str:
    table = to_lua(value)
    for part in reversed(path_parts):
        table = f"{{{part}={table}}}"
    return table


def parse_cli_value(raw: str):
    if raw == "true":
        return True
    if raw == "false":
        return False
    if re.fullmatch(r"-?\d+", raw):
        return int(raw)
    if re.fullmatch(r"-?\d*\.\d+", raw):
        return float(raw)
    return raw


# ---- variables.lua knobs ----

def parse_scalar(raw: str):
    if raw in ("true", "false"):
        return raw == "true"
    if re.fullmatch(r"-?\d+", raw):
        return int(raw)
    if re.fullmatch(r"-?\d*\.\d+", raw):
        return float(raw)
    m = re.fullmatch(r'"([^"]*)"', raw)
    return m.group(1) if m else None


def read_knobs() -> dict:
    knobs = {}
    for line in open(VARIABLES):
        m = KNOB_LINE.match(line.rstrip("\n"))
        if m:
            value = parse_scalar(m.group(4).strip())
            if value is not None:
                knobs[m.group(2)] = value
    return knobs


def write_knob(key: str, value) -> None:
    lines = open(VARIABLES).readlines()
    for i, line in enumerate(lines):
        m = KNOB_LINE.match(line.rstrip("\n"))
        if m and m.group(2) == key:
            lines[i] = f"{m.group(1)}{key}{m.group(3)}{to_lua(value)}{m.group(5)}\n"
            break
    else:
        sys.exit(f"unknown knob: {key}")
    with open(VARIABLES, "w") as f:
        f.writelines(lines)


# ---- overrides state ----

def load_state() -> dict:
    try:
        state = json.load(open(STATE))
    except (FileNotFoundError, json.JSONDecodeError):
        state = {}
    state.setdefault("options", {})
    state.setdefault("binds", [])
    state.setdefault("monitors", {})
    state.setdefault("primary", "")
    return state


def save_state(state: dict) -> None:
    os.makedirs(os.path.dirname(STATE), exist_ok=True)
    with open(STATE, "w") as f:
        json.dump(state, f, indent=2)
    generate_lua(state)


def bind_action_lua(bind: dict) -> str:
    kind, value = bind["kind"], bind["value"]
    if kind == "exec":
        return f"hl.dsp.exec_cmd({json.dumps(value)})"
    if kind == "global":
        return f"hl.dsp.global({json.dumps(value)})"
    return value  # raw lua expression


MONITOR_KEYS = ("mode", "position", "scale", "transform", "vrr", "disabled", "mirror")


def monitor_lua(name: str, spec: dict) -> str:
    fields = [f"output = {to_lua(name)}"]
    fields += [f"{k} = {to_lua(spec[k])}" for k in MONITOR_KEYS if spec.get(k) not in (None, "")]
    return f"hl.monitor({{ {', '.join(fields)} }})"


def generate_lua(state: dict) -> None:
    lines = ["-- Generated by Caelestia++ settings (hyprmod-ctl.py); do not edit by hand."]

    for name, value in sorted(state["options"].items()):
        parts = re.split(r"[:.]", name)
        lines.append(f"hl.config({nested_config(parts, value)})")

    for name, spec in sorted(state["monitors"].items()):
        lines.append(monitor_lua(name, spec))

    if state["primary"]:
        lines.append(f"hl.workspace_rule({{ workspace = \"1\", monitor = {to_lua(state['primary'])}, default = true }})")

    for bind in state["binds"]:
        flags = {f for f in bind.get("flags", []) if f in BIND_FLAGS}
        action = bind_action_lua(bind)
        if flags:
            flag_table = "{ " + ", ".join(f"{f} = true" for f in sorted(flags)) + " }"
            lines.append(f"hl.bind({json.dumps(bind['combo'])}, {action}, {flag_table})")
        else:
            lines.append(f"hl.bind({json.dumps(bind['combo'])}, {action})")

    with open(GENERATED, "w") as f:
        f.write("\n".join(lines) + "\n")


# ---- compositor IPC ----

def hypr_socket() -> str:
    runtime = os.environ.get("XDG_RUNTIME_DIR", f"/run/user/{os.getuid()}")
    signature = os.environ.get("HYPRLAND_INSTANCE_SIGNATURE")
    if signature:
        return f"{runtime}/hypr/{signature}/.socket.sock"
    candidates = glob.glob(f"{runtime}/hypr/*/.socket.sock")
    if not candidates:
        sys.exit("no hyprland socket")
    return max(candidates, key=os.path.getmtime)


def send(command: str) -> str:
    with socket.socket(socket.AF_UNIX) as s:
        s.connect(hypr_socket())
        s.sendall(command.encode())
        return s.recv(8192).decode()


def apply_knob_live(key: str, value) -> None:
    if key in ("cursorTheme", "cursorSize"):
        knobs = read_knobs()
        subprocess.run(["hyprctl", "setcursor", str(knobs["cursorTheme"]), str(knobs["cursorSize"])])
        subprocess.run(["gsettings", "set", "org.gnome.desktop.interface", "cursor-theme", str(knobs["cursorTheme"])])
        subprocess.run(["gsettings", "set", "org.gnome.desktop.interface", "cursor-size", str(knobs["cursorSize"])])
        return
    path = EVAL_PATHS.get(key)
    if path:
        send(f"eval hl.config({nested_config(path.split('.'), value)})")
    else:
        send("reload")


# ---- commands ----

def cmd_set_knob(key: str, raw: str) -> None:
    current = read_knobs().get(key)
    if current is None:
        sys.exit(f"unknown or non-scalar knob: {key}")
    if isinstance(current, bool):
        value = raw == "true"
    elif isinstance(current, (int, float)):
        value = float(raw) if "." in raw else int(raw)
    else:
        value = raw
    write_knob(key, value)
    apply_knob_live(key, value)


def cmd_set_option(name: str, raw: str) -> None:
    value = parse_cli_value(raw)
    state = load_state()
    state["options"][name] = value
    save_state(state)
    send(f"eval hl.config({nested_config(re.split(r'[:.]', name), value)})")


def cmd_unset_option(name: str) -> None:
    state = load_state()
    state["options"].pop(name, None)
    save_state(state)
    send("reload")


def cmd_set_monitor(name: str, raw: str) -> None:
    spec = json.loads(raw)
    if not isinstance(spec, dict):
        sys.exit("monitor spec must be a JSON object")
    state = load_state()
    state["monitors"][name] = {k: spec[k] for k in MONITOR_KEYS if spec.get(k) not in (None, "")}
    save_state(state)
    send(f"eval {monitor_lua(name, state['monitors'][name])}")


def cmd_del_monitor(name: str) -> None:
    state = load_state()
    state["monitors"].pop(name, None)
    save_state(state)
    send("reload")


def cmd_set_primary(name: str) -> None:
    state = load_state()
    state["primary"] = name
    save_state(state)
    send("reload")


def cmd_add_bind(combo: str, kind: str, value: str, flags: str) -> None:
    if kind not in ("exec", "global", "lua"):
        sys.exit("kind must be exec, global or lua")
    state = load_state()
    state["binds"].append({
        "combo": combo,
        "kind": kind,
        "value": value,
        "flags": [f for f in flags.split(",") if f],
    })
    save_state(state)
    send("reload")


def cmd_del_bind(index: str) -> None:
    state = load_state()
    i = int(index)
    if not 0 <= i < len(state["binds"]):
        sys.exit("bind index out of range")
    state["binds"].pop(i)
    save_state(state)
    send("reload")


def main() -> None:
    args = sys.argv[1:]
    if args[:1] == ["dump"]:
        print(json.dumps(read_knobs()))
    elif args[:1] == ["set"] and len(args) == 3:
        cmd_set_knob(args[1], args[2])
    elif args[:1] == ["schema"]:
        print(subprocess.check_output(["hyprctl", "descriptions"], text=True))
    elif args[:1] == ["overrides"]:
        print(json.dumps(load_state()))
    elif args[:1] == ["set-option"] and len(args) == 3:
        cmd_set_option(args[1], args[2])
    elif args[:1] == ["unset-option"] and len(args) == 2:
        cmd_unset_option(args[1])
    elif args[:1] == ["binds"]:
        print(json.dumps(load_state()["binds"]))
    elif args[:1] == ["add-bind"] and len(args) in (4, 5):
        cmd_add_bind(args[1], args[2], args[3], args[4] if len(args) == 5 else "")
    elif args[:1] == ["del-bind"] and len(args) == 2:
        cmd_del_bind(args[1])
    elif args[:1] == ["set-monitor"] and len(args) == 3:
        cmd_set_monitor(args[1], args[2])
    elif args[:1] == ["del-monitor"] and len(args) == 2:
        cmd_del_monitor(args[1])
    elif args[:1] == ["set-primary"] and len(args) == 2:
        cmd_set_primary(args[1])
    else:
        sys.exit(__doc__.strip())


if __name__ == "__main__":
    main()
