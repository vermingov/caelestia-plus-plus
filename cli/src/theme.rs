//! Pushing a palette out to everything that is not the shell.
//!
//! Terminals, Hyprland, GTK, Qt, btop, htop, nvtop, fuzzel, cava, Discord,
//! Spicetify, Zed, Warp, Chromium and whatever the user dropped in their own
//! templates directory. Each is a template with `{{ $name }}` holes in it,
//! filled and written atomically.
//!
//! Ported from `caelestia.utils.theme`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::config;
use crate::hypr;
use crate::paths::{self, atomic_write};
use crate::proc;

/// A palette as the templates see it: ordered, because the generated Hyprland
/// and SCSS files list every colour and their order is part of the output.
pub type Colours = Vec<(String, String)>;

fn lookup<'a>(colours: &'a Colours, name: &str) -> Option<&'a str> {
    colours.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

// ---- generators ----------------------------------------------------------

fn gen_conf(colours: &Colours) -> String {
    colours.iter().map(|(name, colour)| format!("${name} = {colour}\n")).collect()
}

fn gen_lua(colours: &Colours) -> String {
    let mut lua = String::from("return {\n");
    for (name, colour) in colours {
        lua.push_str(&format!("  {name} = \"{colour}\",\n"));
    }
    lua.push('}');
    lua
}

fn gen_scss(colours: &Colours) -> String {
    colours.iter().map(|(name, colour)| format!("${name}: #{colour};\n")).collect()
}

/// The plain substitution: every `{{ $name }}` becomes the colour, with or
/// without a leading `#`.
fn gen_replace(colours: &Colours, template: &str, hash: bool) -> String {
    let mut out = template.to_string();
    for (name, colour) in colours {
        let replacement = if hash { format!("#{colour}") } else { colour.clone() };
        out = out.replace(&format!("{{{{ ${name} }}}}"), &replacement);
    }
    out
}

/// One colour in the four forms a template can ask for.
struct Colour {
    hex: String,
    hexalpha: String,
    rgb: String,
    rgbalpha: String,
}

impl Colour {
    /// Short codes are padded with `f`, so a six-digit colour reads as fully
    /// opaque and a four-digit one still parses.
    fn new(code: &str) -> Colour {
        let padded: String = format!("{code:f<8}");
        let pairs: Vec<&str> = (0..4).map(|i| &padded[i * 2..i * 2 + 2]).collect();
        let values: Vec<u32> = pairs.iter().map(|p| u32::from_str_radix(p, 16).unwrap_or(0)).collect();
        let join = |v: &[u32]| v.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(",");
        Colour {
            hex: pairs[..3].concat(),
            hexalpha: pairs.concat(),
            rgb: format!("rgb({})", join(&values[..3])),
            rgbalpha: format!("rgba({})", join(&values)),
        }
    }

    fn form(&self, name: &str) -> Option<&str> {
        match name {
            "hex" => Some(&self.hex),
            "hexalpha" => Some(&self.hexalpha),
            "rgb" => Some(&self.rgb),
            "rgbalpha" => Some(&self.rgbalpha),
            _ => None,
        }
    }
}

/// The richer substitution: `{{ primary.rgba }}` and `{{ mode }}`. A hole
/// naming something that does not exist is left alone, so a template can
/// contain literal braces without being mangled.
fn gen_replace_dynamic(colours: &Colours, template: &str, mode: &str) -> String {
    let dynamic: BTreeMap<&str, Colour> =
        colours.iter().map(|(name, code)| (name.as_str(), Colour::new(code))).collect();

    let mut out = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if !template[i..].starts_with("{{") {
            let ch = template[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        // The body runs to the first `}}`, and may not contain `{{` or a
        // newline — the same shape the Python regex matches.
        let rest = &template[i + 2..];
        let end = rest.find("}}");
        let body = end.map(|e| &rest[..e]);
        let valid = body.is_some_and(|b| !b.contains("{{") && !b.contains('\n'));
        if !valid {
            out.push_str("{{");
            i += 2;
            continue;
        }
        let (body, end) = (body.unwrap(), end.unwrap());
        let trimmed = body.trim();
        let filled = if trimmed == "mode" {
            Some(mode.to_string())
        } else {
            let mut parts = trimmed.split('.');
            match (parts.next(), parts.next(), parts.next()) {
                (Some(name), Some(form), None) => {
                    dynamic.get(name).and_then(|c| c.form(form)).map(str::to_string)
                }
                _ => None,
            }
        };
        match filled {
            Some(value) => out.push_str(&value),
            None => out.push_str(&template[i..i + 2 + end + 2]),
        }
        i += 2 + end + 2;
    }
    out
}

/// A colour as an OSC escape, e.g. `ffffff, 11` becomes `]11;rgb:ff/ff/ff`.
fn hex_to_ansi(colour: &str, fields: &[u32]) -> String {
    let parts: Vec<String> = fields.iter().map(|f| f.to_string()).collect();
    format!("\x1b]{};rgb:{}/{}/{}\x1b\\", parts.join(";"), &colour[0..2], &colour[2..4], &colour[4..6])
}

/// The escape sequence that repaints every open terminal: foreground,
/// background, cursor, selection, then the sixteen palette slots.
fn gen_sequences(colours: &Colours) -> String {
    let mut out = String::new();
    let mut push = |name: &str, fields: &[u32]| {
        if let Some(colour) = lookup(colours, name) {
            out.push_str(&hex_to_ansi(colour, fields));
        }
    };
    push("onSurface", &[10]);
    push("surface", &[11]);
    push("secondary", &[12]);
    push("secondary", &[17]);
    for i in 0..16u32 {
        push(&format!("term{i}"), &[4, i]);
    }
    push("primary", &[4, 16]);
    push("secondary", &[4, 17]);
    push("tertiary", &[4, 18]);
    out
}

// ---- appliers ------------------------------------------------------------

fn template(name: &str) -> Option<String> {
    std::fs::read_to_string(paths::templates_dir()?.join(name)).ok()
}

fn write(path: PathBuf, content: &str) {
    if let Err(e) = atomic_write(&path, content) {
        eprintln!("caelestia: cannot write {}: {e}", path.display());
    }
}

/// Repaint every terminal that will accept the write, and leave the sequence
/// on disk so new ones can pick it up.
fn apply_terms(sequences: &str) {
    write(paths::caelestia_state_dir().join("sequences.txt"), sequences);

    let Ok(entries) = std::fs::read_dir("/dev/pts") else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        // Non-blocking, and never becoming our controlling terminal: a
        // terminal that is wedged must not wedge the theme change too.
        write_nonblocking(&entry.path(), sequences.as_bytes());
    }
}

fn write_nonblocking(path: &Path, bytes: &[u8]) {
    use std::os::unix::ffi::OsStrExt;

    let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return };
    // SAFETY: the path is a valid NUL-terminated C string, and the buffer
    // outlives the call.
    unsafe {
        let fd = libc_open(c_path.as_ptr(), O_WRONLY | O_NONBLOCK | O_NOCTTY);
        if fd < 0 {
            return;
        }
        libc_write(fd, bytes.as_ptr() as *const core::ffi::c_void, bytes.len());
        libc_close(fd);
    }
}

const O_WRONLY: i32 = 1;
const O_NONBLOCK: i32 = 0o4000;
const O_NOCTTY: i32 = 0o400;

unsafe extern "C" {
    #[link_name = "open"]
    fn libc_open(path: *const core::ffi::c_char, flags: i32, ...) -> i32;
    #[link_name = "write"]
    fn libc_write(fd: i32, buf: *const core::ffi::c_void, count: usize) -> isize;
    #[link_name = "close"]
    fn libc_close(fd: i32) -> i32;
}

fn is_lua_config() -> bool {
    hypr::message("status")
        .and_then(|status| status.str_field("configProvider").map(|p| p == "lua"))
        .unwrap_or(false)
}

fn apply_hypr(conf: &str, lua: bool) {
    let extension = if lua { "lua" } else { "conf" };
    write(paths::config_dir().join(format!("hypr/scheme/current.{extension}")), conf);
}

/// Discord themes are SCSS, so this is the one applier that needs a compiler.
fn apply_discord(scss: &str) {
    let Some(templates) = paths::templates_dir() else { return };
    let tmp = std::env::temp_dir().join(format!("caelestia-scss-{}", std::process::id()));
    if std::fs::create_dir_all(&tmp).is_err() {
        return;
    }
    let _ = std::fs::write(tmp.join("_colours.scss"), scss);

    let source = templates.join("discord.scss");
    let compiled = proc::capture_text(
        "sass",
        &["-I", tmp.to_str().unwrap_or("/tmp"), source.to_str().unwrap_or("")],
    );
    let _ = std::fs::remove_dir_all(&tmp);

    let Some(compiled) = compiled else { return };
    for client in ["Equicord", "Vencord", "BetterDiscord", "equibop", "vesktop", "legcord"] {
        write(paths::config_dir().join(client).join("themes/caelestia.theme.css"), &compiled);
    }
}

fn apply_pandora(colours: &Colours, mode: &str) {
    let Some(source) = template("pandora.json") else { return };
    let filled = gen_replace(colours, &source, true).replace("{{ $mode }}", mode);
    write(paths::data_dir().join("PandoraLauncher/themes/caelestia.json"), &filled);
}

fn apply_spicetify(colours: &Colours, mode: &str) {
    let Some(source) = template(&format!("spicetify-{mode}.ini")) else { return };
    write(
        paths::config_dir().join("spicetify/Themes/caelestia/color.ini"),
        &gen_replace(colours, &source, false),
    );
}

fn apply_fuzzel(colours: &Colours) {
    let Some(source) = template("fuzzel.ini") else { return };
    write(paths::config_dir().join("fuzzel/fuzzel.ini"), &gen_replace(colours, &source, false));
}

/// btop, htop and cava reread their theme on SIGUSR2 rather than restarting.
fn apply_btop(colours: &Colours) {
    let Some(source) = template("btop.theme") else { return };
    write(paths::config_dir().join("btop/themes/caelestia.theme"), &gen_replace(colours, &source, true));
    proc::run("killall", &["-USR2", "btop"]);
}

fn apply_nvtop(colours: &Colours) {
    let Some(source) = template("nvtop.colors") else { return };
    write(paths::config_dir().join("nvtop/nvtop.colors"), &gen_replace(colours, &source, true));
}

fn apply_htop(colours: &Colours) {
    let Some(source) = template("htop.theme") else { return };
    write(paths::config_dir().join("htop/htoprc"), &gen_replace(colours, &source, true));
    proc::run("killall", &["-USR2", "htop"]);
}

fn apply_cava(colours: &Colours) {
    let Some(source) = template("cava.conf") else { return };
    write(paths::config_dir().join("cava/config"), &gen_replace(colours, &source, true));
    proc::run("killall", &["-USR2", "cava"]);
}

fn apply_gtk(colours: &Colours, mode: &str, icon_theme: Option<&str>) {
    let (Some(gtk), Some(thunar)) = (template("gtk.css"), template("thunar.css")) else { return };
    let gtk = gen_replace(colours, &gtk, true);
    let thunar = gen_replace(colours, &thunar, true);
    for version in ["gtk-3.0", "gtk-4.0"] {
        write(paths::config_dir().join(version).join("gtk.css"), &gtk);
        write(paths::config_dir().join(version).join("thunar.css"), &thunar);
    }

    proc::run("dconf", &["write", "/org/gnome/desktop/interface/gtk-theme", "'adw-gtk3-dark'"]);
    proc::run("dconf", &["write", "/org/gnome/desktop/interface/color-scheme", &format!("'prefer-{mode}'")]);
    let icons = icon_theme.map(str::to_string).unwrap_or_else(|| format!("Papirus-{}", capitalise(mode)));
    proc::run("dconf", &["write", "/org/gnome/desktop/interface/icon-theme", &format!("'{icons}'")]);

    if let Some(primary) = lookup(colours, "primary") {
        sync_papirus_colours(primary);
    }
}

fn apply_qt(colours: &Colours, mode: &str, icon_theme: Option<&str>) {
    if let Some(source) = template(&format!("qt{mode}.colors")) {
        write(paths::config_dir().join("qtengine/caelestia.colors"), &gen_replace(colours, &source, true));
    }
    let Some(source) = template("qtengine.json") else { return };
    let mut config = source.replace("{{ $mode }}", &capitalise(mode));
    if let Some(icons) = icon_theme {
        let stock = format!("\"iconTheme\": \"Papirus-{}\"", capitalise(mode));
        config = config.replace(&stock, &format!("\"iconTheme\": \"{icons}\""));
    }
    write(paths::config_dir().join("qtengine/config.json"), &config);
}

fn apply_warp(colours: &Colours, mode: &str) {
    let Some(source) = template("warp.yaml") else { return };
    let warp_mode = if mode == "dark" { "darker" } else { "lighter" };
    let filled = gen_replace(colours, &source, true).replace("{{ $warp_mode }}", warp_mode);
    write(paths::data_dir().join("warp-terminal/themes/caelestia.yaml"), &filled);
}

/// Chromium takes its window colour from a managed policy, which lives under
/// /etc — so this is the one applier that needs root, and it asks for it
/// non-interactively or gives up.
fn apply_chromium(colours: &Colours) {
    let Some(surface) = lookup(colours, "surface") else { return };
    let policy = format!(
        "{{\"BrowserThemeColor\": \"#{surface}\", \"BrowserColorScheme\": \"device\"}}"
    );

    for (command, dir) in [
        ("chromium", "/etc/chromium/policies/managed"),
        ("brave", "/etc/brave/policies/managed"),
        ("google-chrome-stable", "/etc/opt/chrome/policies/managed"),
    ] {
        if !proc::which(command) {
            continue;
        }
        if !Path::new(dir).is_dir() {
            proc::run("sudo", &["-n", "mkdir", "-p", dir]);
        }
        if !Path::new(dir).is_dir() {
            eprintln!("caelestia: unable to create {dir}");
            continue;
        }
        let target = format!("{dir}/caelestia.json");
        proc::spawn_detached_with_input("sudo", &["-n", "tee", &target], policy.as_bytes());
        proc::run(command, &["--refresh-platform-policy", "--no-startup-window"]);
    }
}

/// Zed's file watcher does not follow symlinks, so a linked theme has to be
/// replaced by a real file before writing.
fn apply_zed(colours: &Colours, mode: &str) {
    let path = paths::config_dir().join("zed/themes/caelestia.json");
    if path.is_symlink() {
        let _ = std::fs::remove_file(&path);
    }
    let Some(source) = template("zed.json") else { return };
    write(path, &gen_replace_dynamic(colours, &source, mode));
}

fn apply_user_templates(colours: &Colours, mode: &str) {
    let dir = paths::user_templates_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    for entry in entries.flatten() {
        if !entry.path().is_file() {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(entry.path()) else { continue };
        write(paths::theme_dir().join(entry.file_name()), &gen_replace_dynamic(colours, &source, mode));
    }
}

fn capitalise(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

// ---- papirus -------------------------------------------------------------

/// Papirus ships folder icons in a fixed set of colours, so the scheme's
/// primary is matched to the nearest named one rather than recoloured.
fn sync_papirus_colours(hex: &str) {
    if !proc::which("papirus-folders") {
        return;
    }
    let installed = [
        PathBuf::from("/usr/share/icons/Papirus"),
        PathBuf::from("/usr/share/icons/Papirus-Dark"),
        paths::home().join(".local/share/icons/Papirus"),
        paths::home().join(".icons/Papirus"),
    ];
    if !installed.iter().any(|p| p.exists()) {
        return;
    }

    let channel = |i: usize| u32::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap_or(0);
    let (r, g, b) = (channel(0), channel(1), channel(2));
    let brightness = r.max(g).max(b);
    let min = r.min(g).min(b);
    let saturation = if brightness == 0 { 0 } else { (brightness - min) * 100 / brightness };

    let colour = if saturation < 20 {
        if brightness < 85 {
            "black"
        } else if brightness < 170 {
            "grey"
        } else {
            "white"
        }
    } else if saturation < 60 && brightness > 180 {
        hue_colour(r, g, b, brightness, true)
    } else {
        hue_colour(r, g, b, brightness, false)
    };

    proc::spawn_detached("sudo", &["-n", "papirus-folders", "-C", colour, "-u"]);
}

fn hue_colour(r: u32, g: u32, b: u32, brightness: u32, pale: bool) -> &'static str {
    if b > r && b > g {
        let r_ratio = if b > 0 { r * 100 / b } else { 0 };
        let g_ratio = if b > 0 { g * 100 / b } else { 0 };
        let rg_diff = r.abs_diff(g);
        if r_ratio > 70 && g_ratio > 70 {
            // Both channels high against blue: a light blue, and which way it
            // leans decides between violet and cyan.
            if rg_diff < 15 {
                "blue"
            } else if r > g {
                "violet"
            } else {
                "cyan"
            }
        } else if r_ratio > 60 && r > g {
            "violet"
        } else if g_ratio > 60 && g > r {
            "cyan"
        } else {
            "blue"
        }
    } else if r > g && r > b {
        if g > b + 30 {
            let rg_ratio = if r > 0 { g * 100 / r } else { 0 };
            if pale {
                if rg_ratio > 70 && brightness < 220 {
                    "palebrown"
                } else {
                    "paleorange"
                }
            } else if rg_ratio > 70 && brightness < 180 {
                "brown"
            } else {
                "orange"
            }
        } else if b > g + 20 {
            "pink"
        } else if pale {
            "pink"
        } else {
            "red"
        }
    } else if g > r && g > b {
        if r > b + 30 {
            "yellow"
        } else {
            "green"
        }
    } else {
        "grey"
    }
}

// ---- the whole pass ------------------------------------------------------

/// Writes every enabled theme file. Held under a lock, because two scheme
/// changes racing would interleave their writes across applications.
pub fn apply_colours(colours: &Colours, mode: &str) {
    let lock_path = paths::caelestia_state_dir().join("theme.lock");
    let _ = std::fs::create_dir_all(paths::caelestia_state_dir());
    let Some(_lock) = Lock::take(&lock_path) else { return };

    let config = config::user_config();
    let theme = config.get("theme").cloned();
    let enabled = |key: &str| theme.as_ref().map_or(true, |t| t.bool_field(key, true));
    let string = |key: &str| {
        theme
            .as_ref()
            .and_then(|t| t.str_field(key))
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };

    if enabled("enableTerm") {
        apply_terms(&gen_sequences(colours));
    }
    if enabled("enableHypr") {
        let lua = is_lua_config();
        apply_hypr(&if lua { gen_lua(colours) } else { gen_conf(colours) }, lua);
    }
    if enabled("enableDiscord") {
        apply_discord(&gen_scss(colours));
    }
    if enabled("enableSpicetify") {
        apply_spicetify(colours, mode);
    }
    if enabled("enablePandora") {
        apply_pandora(colours, mode);
    }
    if enabled("enableFuzzel") {
        apply_fuzzel(colours);
    }
    if enabled("enableBtop") {
        apply_btop(colours);
    }
    if enabled("enableNvtop") {
        apply_nvtop(colours);
    }
    if enabled("enableHtop") {
        apply_htop(colours);
    }
    let icon_theme = string(&format!("iconTheme{}", capitalise(mode))).or_else(|| string("iconTheme"));
    if enabled("enableGtk") {
        apply_gtk(colours, mode, icon_theme.as_deref());
    }
    if enabled("enableQt") {
        apply_qt(colours, mode, icon_theme.as_deref());
    }
    if enabled("enableWarp") {
        apply_warp(colours, mode);
    }
    if enabled("enableChromium") {
        apply_chromium(colours);
    }
    if enabled("enableZed") {
        apply_zed(colours, mode);
    }
    if enabled("enableCava") {
        apply_cava(colours);
    }
    apply_user_templates(colours, mode);

    if let Some(hook) = string("postHook") {
        run_post_hook(&hook, colours);
    }
}

/// An exclusive lock that is dropped, and deleted, when the pass ends.
struct Lock {
    file: std::fs::File,
    path: PathBuf,
}

impl Lock {
    fn take(path: &Path) -> Option<Lock> {
        let file = std::fs::File::create(path).ok()?;
        // SAFETY: the descriptor is open and owned by `file`.
        let locked = unsafe { flock(as_raw_fd(&file), LOCK_EX | LOCK_NB) } == 0;
        locked.then(|| Lock { file, path: path.to_path_buf() })
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        // SAFETY: as above; the lock is released before the file closes.
        unsafe { flock(as_raw_fd(&self.file), LOCK_UN) };
        let _ = std::fs::remove_file(&self.path);
    }
}

fn as_raw_fd(file: &std::fs::File) -> i32 {
    use std::os::fd::AsRawFd;
    file.as_raw_fd()
}

const LOCK_EX: i32 = 2;
const LOCK_NB: i32 = 4;
const LOCK_UN: i32 = 8;

unsafe extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}

fn run_post_hook(hook: &str, colours: &Colours) {
    use std::process::Command;

    let Some(current) = crate::scheme::current() else { return };
    let mut json = String::from("{");
    for (i, (name, colour)) in colours.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!("\"{name}\": \"{colour}\""));
    }
    json.push('}');

    let _ = Command::new("sh")
        .args(["-c", hook])
        .env("SCHEME_NAME", &current.name)
        .env("SCHEME_FLAVOUR", &current.flavour)
        .env("SCHEME_MODE", &current.mode)
        .env("SCHEME_VARIANT", &current.variant)
        .env("SCHEME_COLOURS", json)
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> Colours {
        vec![
            ("primary".to_string(), "aabbcc".to_string()),
            ("surface".to_string(), "112233".to_string()),
        ]
    }

    #[test]
    fn the_three_flat_formats_list_every_colour() {
        let colours = palette();
        assert_eq!(gen_conf(&colours), "$primary = aabbcc\n$surface = 112233\n");
        assert_eq!(gen_scss(&colours), "$primary: #aabbcc;\n$surface: #112233;\n");
        assert_eq!(gen_lua(&colours), "return {\n  primary = \"aabbcc\",\n  surface = \"112233\",\n}");
    }

    #[test]
    fn replacement_honours_the_hash_flag() {
        let colours = palette();
        assert_eq!(gen_replace(&colours, "a {{ $primary }} b", false), "a aabbcc b");
        assert_eq!(gen_replace(&colours, "a {{ $primary }} b", true), "a #aabbcc b");
        assert_eq!(gen_replace(&colours, "{{ $missing }}", true), "{{ $missing }}");
    }

    #[test]
    fn dynamic_replacement_covers_every_form() {
        let colours = palette();
        let filled = gen_replace_dynamic(
            &colours,
            "{{ primary.hex }} {{ primary.hexalpha }} {{ primary.rgb }} {{ primary.rgbalpha }} {{ mode }}",
            "dark",
        );
        assert_eq!(filled, "aabbcc aabbccff rgb(170,187,204) rgba(170,187,204,255) dark");
    }

    #[test]
    fn a_hole_naming_nothing_is_left_alone() {
        let colours = palette();
        for template in ["{{ nope.hex }}", "{{ primary.nope }}", "{{ primary }}", "{{ a.b.c }}", "{{ unclosed"] {
            assert_eq!(gen_replace_dynamic(&colours, template, "dark"), template, "{template}");
        }
    }

    #[test]
    fn a_hole_spanning_a_newline_is_not_a_hole() {
        let colours = palette();
        let template = "{{ primary.\nhex }}";
        assert_eq!(gen_replace_dynamic(&colours, template, "dark"), template);
    }

    #[test]
    fn sequences_carry_the_sixteen_terminal_slots() {
        let mut colours = palette();
        colours.push(("onSurface".to_string(), "ffffff".to_string()));
        colours.push(("secondary".to_string(), "00ff00".to_string()));
        colours.push(("tertiary".to_string(), "0000ff".to_string()));
        for i in 0..16 {
            colours.push((format!("term{i}"), format!("{:02x}0000", i * 16)));
        }
        let sequences = gen_sequences(&colours);
        assert!(sequences.contains("\x1b]11;rgb:11/22/33\x1b\\"), "background");
        assert!(sequences.contains("\x1b]4;15;rgb:f0/00/00\x1b\\"), "bright white slot");
        assert_eq!(sequences.matches("\x1b]").count(), 23);
    }

    #[test]
    fn papirus_picks_a_named_colour_for_each_hue() {
        assert_eq!(hue_colour(0x11, 0x22, 0xcc, 0xcc, false), "blue");
        assert_eq!(hue_colour(0xcc, 0x22, 0x11, 0xcc, false), "red");
        assert_eq!(hue_colour(0x22, 0xcc, 0x11, 0xcc, false), "green");
        assert_eq!(hue_colour(0xcc, 0xaa, 0x11, 0xcc, false), "orange");
        assert_eq!(hue_colour(0x11, 0x11, 0x11, 0x11, false), "grey");
    }

    fn digest(text: &str) -> String {
        let mut hasher = crate::sha256::Sha256::new();
        hasher.update(text.as_bytes());
        crate::sha256::hex(&hasher.finish())
    }

    /// Every template the CLI ships, rendered three ways against four real
    /// palettes, against what the Python produces for the same inputs.
    #[test]
    fn templates_render_exactly_as_python_renders_them() {
        use redcommon::json::Json;

        let vectors = redcommon::json::parse(include_str!("../tests/theme-vectors.json"))
            .expect("reference vectors are readable");
        let probe = vectors.str_field("probe").expect("probe template");
        let Some(Json::Arr(entries)) = vectors.get("palettes") else { panic!("no palettes") };

        let Some(templates_dir) = paths::templates_dir() else {
            eprintln!("no caelestia package installed; skipping");
            return;
        };

        let mut checked = 0usize;
        for entry in entries {
            let label = entry.str_field("label").expect("label");
            let mode = entry.str_field("mode").expect("mode");
            let Some(Json::Arr(pairs)) = entry.get("colours") else { panic!("no colours") };
            let colours: Colours = pairs
                .iter()
                .map(|pair| {
                    let Json::Arr(pair) = pair else { panic!("pairs") };
                    match (&pair[0], &pair[1]) {
                        (Json::Str(k), Json::Str(v)) => (k.clone(), v.clone()),
                        _ => panic!("pairs are strings"),
                    }
                })
                .collect();

            assert_eq!(digest(&gen_conf(&colours)), entry.str_field("conf").unwrap(), "{label}: conf");
            assert_eq!(digest(&gen_lua(&colours)), entry.str_field("lua").unwrap(), "{label}: lua");
            assert_eq!(digest(&gen_scss(&colours)), entry.str_field("scss").unwrap(), "{label}: scss");
            assert_eq!(gen_sequences(&colours), entry.str_field("sequences").unwrap(), "{label}: sequences");
            assert_eq!(
                gen_replace_dynamic(&colours, probe, mode),
                entry.str_field("probe").unwrap(),
                "{label}: the awkward-cases probe"
            );
            checked += 5;

            let Some(Json::Obj(templates)) = entry.get("templates") else { panic!("no templates") };
            for (name, expected) in templates {
                let source = std::fs::read_to_string(templates_dir.join(name))
                    .unwrap_or_else(|e| panic!("{name}: {e}"));
                for (how, ours) in [
                    ("plain", gen_replace(&colours, &source, false)),
                    ("hash", gen_replace(&colours, &source, true)),
                    ("dynamic", gen_replace_dynamic(&colours, &source, mode)),
                ] {
                    assert_eq!(
                        digest(&ours),
                        expected.str_field(how).unwrap(),
                        "{label}: {name} rendered {how}"
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked >= 200, "only {checked} renders checked");
    }
}
