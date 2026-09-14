//! The bridge between the settings UI and the Hyprland config.
//!
//! Three surfaces, all of them writing Lua that `hyprland.lua` loads last so
//! an override wins over the base config:
//!
//!   - the curated knobs in `variables.lua`, edited in place so the file the
//!     user wrote keeps its comments and its shape;
//!   - overrides on any of Hyprland's several hundred options, plus custom
//!     keybinds and monitor layout, kept in one JSON state file and
//!     regenerated into Lua on every change;
//!   - a live apply, by `eval` where the option supports it and a reload
//!     where it does not.
//!
//! Ported from `assets/hyprmod-ctl.py`.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use redcommon::json::{self, Json};

/// Knobs Hyprland can take live, without a full reload.
const EVAL_PATHS: [(&str, &str); 17] = [
    ("blurEnabled", "decoration.blur.enabled"),
    ("blurXray", "decoration.blur.xray"),
    ("blurSpecialWs", "decoration.blur.special"),
    ("blurPopups", "decoration.blur.popups"),
    ("blurInputMethods", "decoration.blur.input_methods"),
    ("blurSize", "decoration.blur.size"),
    ("blurPasses", "decoration.blur.passes"),
    ("shadowEnabled", "decoration.shadow.enabled"),
    ("shadowRange", "decoration.shadow.range"),
    ("shadowRenderPower", "decoration.shadow.render_power"),
    ("windowRounding", "decoration.rounding"),
    ("workspaceGaps", "general.gaps_workspaces"),
    ("windowGapsIn", "general.gaps_in"),
    ("windowGapsOut", "general.gaps_out"),
    ("windowBorderSize", "general.border_size"),
    ("touchpadDisableTyping", "input.touchpad.disable_while_typing"),
    ("touchpadScrollFactor", "input.touchpad.scroll_factor"),
];

const BIND_FLAGS: [&str; 5] = ["locked", "release", "repeat", "mouse", "non_consuming"];
const MONITOR_KEYS: [&str; 7] = ["mode", "position", "scale", "transform", "vrr", "disabled", "mirror"];

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

fn variables_path() -> PathBuf {
    home().join(".config/hypr/variables.lua")
}

fn state_path() -> PathBuf {
    home().join(".config/caelestia/hyprmod-overrides.json")
}

fn generated_path() -> PathBuf {
    home().join(".config/caelestia/hyprmod-overrides.lua")
}

// ---- values --------------------------------------------------------------

/// A knob's value. Lua and JSON both need it, and the two spell it
/// differently, so it is kept as itself rather than as text.
#[derive(Debug, Clone, PartialEq)]
enum Value {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

impl Value {
    fn to_lua(&self) -> String {
        match self {
            Value::Bool(v) => (if *v { "true" } else { "false" }).to_string(),
            Value::Int(v) => v.to_string(),
            // Lua takes the same shortest round-trippable form Python's %g
            // produces, which is not Rust's default float formatting.
            Value::Float(v) => format_g(*v),
            Value::Str(v) => json::s(v.clone()).dump(),
        }
    }

    fn to_json(&self) -> Json {
        match self {
            Value::Bool(v) => Json::Bool(*v),
            Value::Int(v) => Json::Num(*v as f64),
            Value::Float(v) => Json::Num(*v),
            Value::Str(v) => json::s(v.clone()),
        }
    }

    fn from_json(value: &Json) -> Option<Value> {
        Some(match value {
            Json::Bool(v) => Value::Bool(*v),
            Json::Num(v) if v.fract() == 0.0 => Value::Int(*v as i64),
            Json::Num(v) => Value::Float(*v),
            Json::Str(v) => Value::Str(v.clone()),
            _ => return None,
        })
    }
}

/// Python's `%g`: six significant digits, trailing zeros trimmed, an exponent
/// only when the number needs one.
fn format_g(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    let exponent = value.abs().log10().floor() as i32;
    if !(-5..6).contains(&exponent) {
        let mantissa = value / 10f64.powi(exponent);
        return format!("{}e{}{:02}", trim_zeros(&format!("{mantissa:.5}")), if exponent < 0 { "-" } else { "+" }, exponent.abs());
    }
    trim_zeros(&format!("{:.*}", (5 - exponent).max(0) as usize, value))
}

fn trim_zeros(text: &str) -> String {
    if !text.contains('.') {
        return text.to_string();
    }
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A value typed on the command line: the shape decides the type.
fn parse_cli_value(raw: &str) -> Value {
    match raw {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        _ => {}
    }
    if let Ok(int) = raw.parse::<i64>() {
        if looks_like_integer(raw) {
            return Value::Int(int);
        }
    }
    if looks_like_float(raw) {
        if let Ok(float) = raw.parse::<f64>() {
            return Value::Float(float);
        }
    }
    Value::Str(raw.to_string())
}

/// `-?\d+`, and nothing else — `1e3` and `+1` are strings, as they are to the
/// regex this replaces.
fn looks_like_integer(raw: &str) -> bool {
    let digits = raw.strip_prefix('-').unwrap_or(raw);
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

/// `-?\d*\.\d+`: a decimal point with digits after it.
fn looks_like_float(raw: &str) -> bool {
    let body = raw.strip_prefix('-').unwrap_or(raw);
    let Some((whole, fraction)) = body.split_once('.') else { return false };
    whole.bytes().all(|b| b.is_ascii_digit())
        && !fraction.is_empty()
        && fraction.bytes().all(|b| b.is_ascii_digit())
}

/// `decoration.blur.size` and a value become `{decoration={blur={size=8}}}`.
fn nested_config(parts: &[&str], value: &Value) -> String {
    let mut table = value.to_lua();
    for part in parts.iter().rev() {
        table = format!("{{{part}={table}}}");
    }
    table
}

fn split_option_name(name: &str) -> Vec<&str> {
    name.split(|c| c == ':' || c == '.').collect()
}

// ---- variables.lua -------------------------------------------------------

/// One `name = value,` line, split into the parts an edit has to preserve.
struct KnobLine<'a> {
    indent: &'a str,
    key: &'a str,
    between: &'a str,
    value: &'a str,
    trailer: &'a str,
}

fn match_knob_line(line: &str) -> Option<KnobLine<'_>> {
    let indent_end = line.len() - line.trim_start().len();
    let (indent, rest) = line.split_at(indent_end);

    let key_end = rest.find(|c: char| !(c.is_alphanumeric() || c == '_'))?;
    let (key, rest_after_key) = rest.split_at(key_end);
    if key.is_empty() {
        return None;
    }

    let equals = rest_after_key.find('=')?;
    if !rest_after_key[..equals].chars().all(char::is_whitespace) {
        return None;
    }
    let after_equals = &rest_after_key[equals + 1..];
    let value_start = after_equals.len() - after_equals.trim_start().len();
    let between_len = equals + 1 + value_start;
    let (between, value_and_trailer) = rest_after_key.split_at(between_len);

    // The line has to end with a comma and then nothing but whitespace.
    let trimmed = value_and_trailer.trim_end();
    let value = trimmed.strip_suffix(',')?;
    if value.is_empty() {
        return None;
    }
    Some(KnobLine {
        indent,
        key,
        between,
        value,
        trailer: &value_and_trailer[value.len()..],
    })
}

/// The Lua literal a knob line holds, or None for anything this tool will not
/// touch — a table, a function call, a concatenation.
fn parse_scalar(raw: &str) -> Option<Value> {
    let raw = raw.trim();
    match raw {
        "true" => return Some(Value::Bool(true)),
        "false" => return Some(Value::Bool(false)),
        _ => {}
    }
    if looks_like_integer(raw) {
        return raw.parse().ok().map(Value::Int);
    }
    if looks_like_float(raw) {
        return raw.parse().ok().map(Value::Float);
    }
    let inner = raw.strip_prefix('"')?.strip_suffix('"')?;
    (!inner.contains('"')).then(|| Value::Str(inner.to_string()))
}

fn read_knobs() -> BTreeMap<String, Value> {
    let mut knobs = BTreeMap::new();
    let Ok(text) = std::fs::read_to_string(variables_path()) else { return knobs };
    for line in text.lines() {
        let Some(knob) = match_knob_line(line) else { continue };
        if let Some(value) = parse_scalar(knob.value) {
            knobs.insert(knob.key.to_string(), value);
        }
    }
    knobs
}

fn write_knob(key: &str, value: &Value) -> Result<(), String> {
    let path = variables_path();
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out = String::with_capacity(text.len());
    let mut written = false;

    for line in text.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        match match_knob_line(body) {
            Some(knob) if !written && knob.key == key => {
                out.push_str(&format!(
                    "{}{key}{}{}{}\n",
                    knob.indent,
                    knob.between,
                    value.to_lua(),
                    knob.trailer
                ));
                written = true;
            }
            _ => out.push_str(line),
        }
    }
    if !written {
        return Err(format!("unknown knob: {key}"));
    }
    std::fs::write(&path, out).map_err(|e| format!("{}: {e}", path.display()))
}

// ---- override state ------------------------------------------------------

/// Everything the settings UI can change that is not a curated knob.
struct State {
    options: BTreeMap<String, Value>,
    binds: Vec<Bind>,
    monitors: BTreeMap<String, BTreeMap<String, Value>>,
    primary: String,
}

#[derive(Clone)]
struct Bind {
    combo: String,
    kind: String,
    value: String,
    flags: Vec<String>,
}

impl State {
    fn load() -> State {
        let parsed = std::fs::read_to_string(state_path()).ok().and_then(|t| json::parse(&t));
        let mut state = State {
            options: BTreeMap::new(),
            binds: Vec::new(),
            monitors: BTreeMap::new(),
            primary: String::new(),
        };
        let Some(parsed) = parsed else { return state };

        if let Some(Json::Obj(options)) = parsed.get("options") {
            for (name, value) in options {
                if let Some(value) = Value::from_json(value) {
                    state.options.insert(name.clone(), value);
                }
            }
        }
        if let Some(Json::Arr(binds)) = parsed.get("binds") {
            for bind in binds {
                let (Some(combo), Some(kind), Some(value)) =
                    (bind.str_field("combo"), bind.str_field("kind"), bind.str_field("value"))
                else {
                    continue;
                };
                let flags = match bind.get("flags") {
                    Some(Json::Arr(flags)) => flags
                        .iter()
                        .filter_map(|f| match f {
                            Json::Str(s) => Some(s.clone()),
                            _ => None,
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                state.binds.push(Bind {
                    combo: combo.to_string(),
                    kind: kind.to_string(),
                    value: value.to_string(),
                    flags,
                });
            }
        }
        if let Some(Json::Obj(monitors)) = parsed.get("monitors") {
            for (name, spec) in monitors {
                let mut fields = BTreeMap::new();
                for key in MONITOR_KEYS {
                    if let Some(value) = spec.get(key).and_then(Value::from_json) {
                        if value != Value::Str(String::new()) {
                            fields.insert(key.to_string(), value);
                        }
                    }
                }
                state.monitors.insert(name.clone(), fields);
            }
        }
        state.primary = parsed.str_field("primary").unwrap_or("").to_string();
        state
    }

    fn to_json(&self) -> Json {
        let options: BTreeMap<String, Json> =
            self.options.iter().map(|(k, v)| (k.clone(), v.to_json())).collect();
        let binds: Vec<Json> = self
            .binds
            .iter()
            .map(|b| {
                json::obj([
                    ("combo", json::s(b.combo.clone())),
                    ("kind", json::s(b.kind.clone())),
                    ("value", json::s(b.value.clone())),
                    ("flags", Json::Arr(b.flags.iter().map(|f| json::s(f.clone())).collect())),
                ])
            })
            .collect();
        let monitors: BTreeMap<String, Json> = self
            .monitors
            .iter()
            .map(|(name, spec)| {
                let fields: BTreeMap<String, Json> =
                    spec.iter().map(|(k, v)| (k.clone(), v.to_json())).collect();
                (name.clone(), Json::Obj(fields))
            })
            .collect();

        json::obj([
            ("options", Json::Obj(options)),
            ("binds", Json::Arr(binds)),
            ("monitors", Json::Obj(monitors)),
            ("primary", json::s(self.primary.clone())),
        ])
    }

    fn save(&self) -> Result<(), String> {
        let path = state_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&path, self.to_json().dump()).map_err(|e| format!("{}: {e}", path.display()))?;
        self.generate_lua()
    }

    /// The whole override set as one Lua file. Rewritten from scratch every
    /// time, so a removed override leaves nothing behind.
    fn generate_lua(&self) -> Result<(), String> {
        let mut lines =
            vec!["-- Generated by Caelestia++ settings; do not edit by hand."
                .to_string()];

        for (name, value) in &self.options {
            let parts = split_option_name(name);
            lines.push(format!("hl.config({})", nested_config(&parts, value)));
        }
        for (name, spec) in &self.monitors {
            lines.push(monitor_lua(name, spec));
        }
        if !self.primary.is_empty() {
            lines.push(format!(
                "hl.workspace_rule({{ workspace = \"1\", monitor = {}, default = true }})",
                Value::Str(self.primary.clone()).to_lua()
            ));
        }
        for bind in &self.binds {
            lines.push(bind_lua(bind));
        }

        let path = generated_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&path, lines.join("\n") + "\n").map_err(|e| format!("{}: {e}", path.display()))
    }
}

fn monitor_lua(name: &str, spec: &BTreeMap<String, Value>) -> String {
    let mut fields = vec![format!("output = {}", Value::Str(name.to_string()).to_lua())];
    // Written in the order Hyprland documents them, not alphabetically.
    for key in MONITOR_KEYS {
        if let Some(value) = spec.get(key) {
            fields.push(format!("{key} = {}", value.to_lua()));
        }
    }
    format!("hl.monitor({{ {} }})", fields.join(", "))
}

fn bind_lua(bind: &Bind) -> String {
    let quoted = |text: &str| json::s(text.to_string()).dump();
    let action = match bind.kind.as_str() {
        "exec" => format!("hl.dsp.exec_cmd({})", quoted(&bind.value)),
        "global" => format!("hl.dsp.global({})", quoted(&bind.value)),
        _ => bind.value.clone(), // raw Lua
    };
    let mut flags: Vec<&String> = bind.flags.iter().filter(|f| BIND_FLAGS.contains(&f.as_str())).collect();
    flags.sort();
    flags.dedup();
    if flags.is_empty() {
        return format!("hl.bind({}, {action})", quoted(&bind.combo));
    }
    let table: Vec<String> = flags.iter().map(|f| format!("{f} = true")).collect();
    format!("hl.bind({}, {action}, {{ {} }})", quoted(&bind.combo), table.join(", "))
}

// ---- compositor ----------------------------------------------------------

fn hypr_socket() -> Result<PathBuf, String> {
    let runtime = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/run/user/{}", unsafe { getuid() }));
    if let Ok(signature) = std::env::var("HYPRLAND_INSTANCE_SIGNATURE") {
        return Ok(PathBuf::from(format!("{runtime}/hypr/{signature}/.socket.sock")));
    }
    // No signature: the most recently touched instance is the live one.
    let entries = std::fs::read_dir(format!("{runtime}/hypr")).map_err(|_| "no hyprland socket".to_string())?;
    entries
        .flatten()
        .map(|e| e.path().join(".socket.sock"))
        .filter(|p| p.exists())
        .max_by_key(|p| p.metadata().and_then(|m| m.modified()).ok())
        .ok_or_else(|| "no hyprland socket".to_string())
}

unsafe extern "C" {
    fn getuid() -> u32;
}

fn send(command: &str) -> Result<String, String> {
    let mut stream = UnixStream::connect(hypr_socket()?).map_err(|e| e.to_string())?;
    stream.write_all(command.as_bytes()).map_err(|e| e.to_string())?;
    let mut reply = vec![0u8; 8192];
    let read = stream.read(&mut reply).unwrap_or(0);
    Ok(String::from_utf8_lossy(&reply[..read]).into_owned())
}

fn run_command(program: &str, args: &[&str]) {
    let _ = std::process::Command::new(program).args(args).status();
}

/// Push a knob change into the running compositor: by `eval` where the option
/// takes one, and otherwise by reloading the config.
fn apply_knob_live(key: &str, value: &Value) -> Result<(), String> {
    if key == "cursorTheme" || key == "cursorSize" {
        let knobs = read_knobs();
        let text = |name: &str| match knobs.get(name) {
            Some(Value::Str(v)) => v.clone(),
            Some(other) => other.to_lua(),
            None => String::new(),
        };
        let (theme, size) = (text("cursorTheme"), text("cursorSize"));
        run_command("hyprctl", &["setcursor", &theme, &size]);
        run_command("gsettings", &["set", "org.gnome.desktop.interface", "cursor-theme", &theme]);
        run_command("gsettings", &["set", "org.gnome.desktop.interface", "cursor-size", &size]);
        return Ok(());
    }
    match EVAL_PATHS.iter().find(|(name, _)| *name == key) {
        Some((_, path)) => {
            let parts: Vec<&str> = path.split('.').collect();
            send(&format!("eval hl.config({})", nested_config(&parts, value)))?;
        }
        None => {
            send("reload")?;
        }
    }
    Ok(())
}

// ---- commands ------------------------------------------------------------

fn set_knob(key: &str, raw: &str) -> Result<(), String> {
    let knobs = read_knobs();
    let current = knobs.get(key).ok_or_else(|| format!("unknown or non-scalar knob: {key}"))?;
    // The knob's current type decides how the new text is read, so a UI
    // sending "1" for a float knob does not turn it into an integer.
    let value = match current {
        Value::Bool(_) => Value::Bool(raw == "true"),
        Value::Int(_) | Value::Float(_) => {
            if raw.contains('.') {
                Value::Float(raw.parse().map_err(|_| format!("not a number: {raw}"))?)
            } else {
                Value::Int(raw.parse().map_err(|_| format!("not a number: {raw}"))?)
            }
        }
        Value::Str(_) => Value::Str(raw.to_string()),
    };
    write_knob(key, &value)?;
    apply_knob_live(key, &value)
}

fn set_option(name: &str, raw: &str) -> Result<(), String> {
    let value = parse_cli_value(raw);
    let mut state = State::load();
    state.options.insert(name.to_string(), value.clone());
    state.save()?;
    let parts = split_option_name(name);
    send(&format!("eval hl.config({})", nested_config(&parts, &value))).map(|_| ())
}

fn unset_option(name: &str) -> Result<(), String> {
    let mut state = State::load();
    state.options.remove(name);
    state.save()?;
    send("reload").map(|_| ())
}

fn set_monitor(name: &str, raw: &str) -> Result<(), String> {
    let parsed = json::parse(raw).ok_or("monitor spec must be JSON")?;
    let Json::Obj(spec) = &parsed else { return Err("monitor spec must be a JSON object".into()) };

    let mut fields = BTreeMap::new();
    for key in MONITOR_KEYS {
        if let Some(value) = spec.get(key).and_then(Value::from_json) {
            if value != Value::Str(String::new()) {
                fields.insert(key.to_string(), value);
            }
        }
    }
    let mut state = State::load();
    state.monitors.insert(name.to_string(), fields);
    state.save()?;
    send(&format!("eval {}", monitor_lua(name, &state.monitors[name]))).map(|_| ())
}

fn del_monitor(name: &str) -> Result<(), String> {
    let mut state = State::load();
    state.monitors.remove(name);
    state.save()?;
    send("reload").map(|_| ())
}

fn set_primary(name: &str) -> Result<(), String> {
    let mut state = State::load();
    state.primary = name.to_string();
    state.save()?;
    send("reload").map(|_| ())
}

fn add_bind(combo: &str, kind: &str, value: &str, flags: &str) -> Result<(), String> {
    if !["exec", "global", "lua"].contains(&kind) {
        return Err("kind must be exec, global or lua".into());
    }
    let mut state = State::load();
    state.binds.push(Bind {
        combo: combo.to_string(),
        kind: kind.to_string(),
        value: value.to_string(),
        flags: flags.split(',').filter(|f| !f.is_empty()).map(str::to_string).collect(),
    });
    state.save()?;
    send("reload").map(|_| ())
}

fn del_bind(index: &str) -> Result<(), String> {
    let mut state = State::load();
    let index: usize = index.parse().map_err(|_| "bind index out of range".to_string())?;
    if index >= state.binds.len() {
        return Err("bind index out of range".into());
    }
    state.binds.remove(index);
    state.save()?;
    send("reload").map(|_| ())
}

pub fn run(args: &[String]) -> i32 {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match words.as_slice() {
        ["dump"] => {
            let knobs: BTreeMap<String, Json> =
                read_knobs().iter().map(|(k, v)| (k.clone(), v.to_json())).collect();
            println!("{}", Json::Obj(knobs).dump());
            Ok(())
        }
        ["set", key, raw] => set_knob(key, raw),
        ["schema"] => match std::process::Command::new("hyprctl").arg("descriptions").output() {
            Ok(out) => {
                print!("{}", String::from_utf8_lossy(&out.stdout));
                Ok(())
            }
            Err(e) => Err(format!("hyprctl: {e}")),
        },
        ["overrides"] => {
            println!("{}", State::load().to_json().dump());
            Ok(())
        }
        ["binds"] => {
            let Json::Obj(state) = State::load().to_json() else { unreachable!() };
            println!("{}", state["binds"].dump());
            Ok(())
        }
        ["set-option", name, raw] => set_option(name, raw),
        ["unset-option", name] => unset_option(name),
        ["add-bind", combo, kind, value] => add_bind(combo, kind, value, ""),
        ["add-bind", combo, kind, value, flags] => add_bind(combo, kind, value, flags),
        ["del-bind", index] => del_bind(index),
        ["set-monitor", name, raw] => set_monitor(name, raw),
        ["del-monitor", name] => del_monitor(name),
        ["set-primary", name] => set_primary(name),
        _ => {
            eprintln!("{USAGE}");
            return 2;
        }
    };

    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("caelestia-tools hyprmod: {e}");
            1
        }
    }
}

const USAGE: &str = "\
hyprmod — the settings UI's bridge to the Hyprland config

Curated knobs (variables.lua):
  dump                    scalar knobs as JSON
  set KEY VALUE           rewrite knob, apply live (eval or reload)

Full option surface (overrides on top of the lua config):
  schema                  hyprctl descriptions passthrough
  overrides               current overrides state as JSON
  set-option NAME VALUE   override any option (name like decoration:blur:size)
  unset-option NAME       drop an override, reload to restore config value

Custom keybinds:
  add-bind COMBO KIND VALUE [FLAGS]   kind: exec | global | lua
  del-bind INDEX                      flags: comma list of locked,release,repeat
  binds                   custom binds as JSON

Monitor layout:
  set-monitor NAME JSON   save one monitor's config, apply live
  del-monitor NAME        forget a monitor
  set-primary NAME        pin workspace 1 to NAME (\"\" clears)";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knob_lines_are_split_without_losing_their_shape() {
        let knob = match_knob_line("    blurSize = 8,").expect("a knob line");
        assert_eq!((knob.indent, knob.key, knob.between, knob.value, knob.trailer), ("    ", "blurSize", " = ", "8", ","));

        let knob = match_knob_line("\tcursorTheme  =  \"Bibata\",  ").expect("a knob line");
        assert_eq!(knob.key, "cursorTheme");
        assert_eq!(knob.value, "\"Bibata\"");
        assert_eq!(knob.trailer, ",  ");

        assert!(match_knob_line("-- a comment").is_none());
        assert!(match_knob_line("  nested = { a = 1 },").is_some(), "a table is a line, just not a scalar");
        assert!(match_knob_line("  noComma = 1").is_none());
        assert!(match_knob_line("  = 1,").is_none());
    }

    #[test]
    fn only_scalars_are_knobs() {
        assert_eq!(parse_scalar("true"), Some(Value::Bool(true)));
        assert_eq!(parse_scalar("-3"), Some(Value::Int(-3)));
        assert_eq!(parse_scalar("0.5"), Some(Value::Float(0.5)));
        assert_eq!(parse_scalar("-.25"), Some(Value::Float(-0.25)));
        assert_eq!(parse_scalar("\"Bibata\""), Some(Value::Str("Bibata".into())));
        assert_eq!(parse_scalar("{ a = 1 }"), None);
        assert_eq!(parse_scalar("some.call()"), None);
        assert_eq!(parse_scalar("1e3"), None, "the regex this replaces does not take exponents");
    }

    #[test]
    fn floats_print_the_way_python_prints_them() {
        assert_eq!(format_g(1.0), "1");
        assert_eq!(format_g(0.5), "0.5");
        assert_eq!(format_g(-0.25), "-0.25");
        assert_eq!(format_g(1.5000000001), "1.5");
        assert_eq!(format_g(1234567.0), "1.23457e+06");
        assert_eq!(format_g(0.0000001), "1e-07");
        assert_eq!(format_g(0.0), "0");
    }

    #[test]
    fn a_dotted_option_becomes_a_nested_lua_table() {
        let value = Value::Int(8);
        assert_eq!(nested_config(&["decoration", "blur", "size"], &value), "{decoration={blur={size=8}}}");
        assert_eq!(nested_config(&["general"], &Value::Bool(false)), "{general=false}");
        assert_eq!(split_option_name("decoration:blur.size"), ["decoration", "blur", "size"]);
    }

    #[test]
    fn a_bind_names_its_action_and_only_the_flags_hyprland_knows() {
        let exec = Bind {
            combo: "SUPER, T".into(),
            kind: "exec".into(),
            value: "kitty".into(),
            flags: vec!["repeat".into(), "nonsense".into(), "locked".into()],
        };
        assert_eq!(
            bind_lua(&exec),
            "hl.bind(\"SUPER, T\", hl.dsp.exec_cmd(\"kitty\"), { locked = true, repeat = true })"
        );

        let raw = Bind { combo: "SUPER, K".into(), kind: "lua".into(), value: "hl.dsp.killactive()".into(), flags: vec![] };
        assert_eq!(bind_lua(&raw), "hl.bind(\"SUPER, K\", hl.dsp.killactive())");
    }

    #[test]
    fn a_monitor_keeps_hyprlands_field_order() {
        let mut spec = BTreeMap::new();
        spec.insert("scale".to_string(), Value::Float(1.5));
        spec.insert("mode".to_string(), Value::Str("2560x1440@165".into()));
        spec.insert("position".to_string(), Value::Str("0x0".into()));
        assert_eq!(
            monitor_lua("DP-1", &spec),
            "hl.monitor({ output = \"DP-1\", mode = \"2560x1440@165\", position = \"0x0\", scale = 1.5 })"
        );
    }

    #[test]
    fn command_line_values_take_the_type_their_shape_implies() {
        assert_eq!(parse_cli_value("true"), Value::Bool(true));
        assert_eq!(parse_cli_value("12"), Value::Int(12));
        assert_eq!(parse_cli_value("-1.5"), Value::Float(-1.5));
        assert_eq!(parse_cli_value("2560x1440"), Value::Str("2560x1440".into()));
        assert_eq!(parse_cli_value("+1"), Value::Str("+1".into()));
    }
}
