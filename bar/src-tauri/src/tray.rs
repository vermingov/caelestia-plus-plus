//! The system tray, which is two DBus protocols in a trenchcoat.
//!
//! An application that wants a tray icon registers itself with the
//! StatusNotifierWatcher and then exposes `org.kde.StatusNotifierItem` on its
//! own bus name. A bar is a *host*: it registers its interest with the
//! watcher, reads the list of items, and asks each one what it looks like.
//! The menu behind a right click is a second protocol again —
//! `com.canonical.dbusmenu` — which is a tree the item hands over on request.
//!
//! Two things make this fiddlier than it sounds. The interface is named
//! `org.kde.*` by most implementations and `org.freedesktop.*` by a few, and
//! an icon arrives either as a theme name or as raw ARGB32 bytes that a
//! webview cannot display without being turned into an image first.

use std::collections::HashMap;
use std::io::Cursor;
use std::time::Duration;

use serde::Serialize;
use zbus::blocking::{Connection, Proxy};
use zbus::names::InterfaceName;
use zbus::zvariant::{OwnedValue, Value};

const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";

/// The two names the same interface goes by. Tried in this order, because the
/// KDE one is what almost everything actually publishes.
const ITEM_INTERFACES: [&str; 2] =
    ["org.kde.StatusNotifierItem", "org.freedesktop.StatusNotifierItem"];

const MENU_INTERFACE: &str = "com.canonical.dbusmenu";

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    /// `service/path`, which is how the front end names one back to us.
    pub key: String,
    pub id: String,
    pub title: String,
    /// "Active", "Passive" or "NeedsAttention".
    pub status: String,
    /// An absolute file path, or a `data:` URI for an icon that arrived as
    /// pixels. Empty when the item gave us neither.
    pub icon: String,
    pub has_menu: bool,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MenuEntry {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    /// "standard", "separator" or "checkmark".
    pub kind: String,
    pub checked: bool,
    /// Entries of a submenu, already fetched: the tree comes in one call.
    pub children: Vec<MenuEntry>,
}

fn connection() -> Option<Connection> {
    Connection::session().ok()
}

/// Splits the `service/path` key a front end hands back.
fn split_key(key: &str) -> Option<(String, String)> {
    let (service, path) = key.split_once('/')?;
    Some((service.to_string(), format!("/{path}")))
}

/// The items the watcher knows about, as `:1.42/StatusNotifierItem` strings.
fn registered(connection: &Connection) -> Vec<String> {
    let Ok(proxy) = Proxy::new(connection, WATCHER, WATCHER_PATH, "org.freedesktop.DBus.Properties")
    else {
        return Vec::new();
    };
    proxy
        .call::<_, _, OwnedValue>("Get", &(WATCHER, "RegisteredStatusNotifierItems"))
        .ok()
        .and_then(|value| Vec::<String>::try_from(value).ok())
        .unwrap_or_default()
}

/// Every property of one item, whichever interface name it answers to.
fn properties(connection: &Connection, service: &str, path: &str) -> Option<HashMap<String, OwnedValue>> {
    let proxy = Proxy::new(connection, service, path, "org.freedesktop.DBus.Properties").ok()?;
    for interface in ITEM_INTERFACES {
        if let Ok(map) = proxy.call::<_, _, HashMap<String, OwnedValue>>("GetAll", &(interface,)) {
            if !map.is_empty() {
                return Some(map);
            }
        }
    }
    None
}

fn string_of(properties: &HashMap<String, OwnedValue>, key: &str) -> String {
    properties
        .get(key)
        .and_then(|value| String::try_from(value.clone()).ok())
        .unwrap_or_default()
}

/// An object path, which is not a string as far as DBus is concerned.
///
/// `Menu` is typed `o`, so reading it as a string quietly produced nothing and
/// every item looked as though it had no menu at all — which is exactly how it
/// behaved.
fn path_of(properties: &HashMap<String, OwnedValue>, key: &str) -> String {
    let Some(value) = properties.get(key) else { return String::new() };
    if let Ok(path) = zbus::zvariant::ObjectPath::try_from(value.clone()) {
        return path.as_str().to_string();
    }
    String::try_from(value.clone()).unwrap_or_default()
}

/// The item's icon, as something a webview can load.
///
/// A theme name is resolved against the icon theme; failing that, the raw
/// pixmap the item handed over is turned into a PNG. An item that gave a name
/// we cannot find and no pixels gets nothing, and the bar draws a placeholder.
fn icon_for(properties: &HashMap<String, OwnedValue>, theme_path: &str) -> String {
    for (name_key, pixmap_key) in
        [("IconName", "IconPixmap"), ("AttentionIconName", "AttentionIconPixmap")]
    {
        let name = string_of(properties, name_key);
        if !name.is_empty() {
            if name.starts_with('/') {
                return name;
            }
            // Some items ship their own icons and say where: that directory
            // comes before the theme, because it is what they meant.
            if !theme_path.is_empty() {
                if let Some(found) = crate::icons::in_directory(theme_path, &name) {
                    return found;
                }
            }
            if let Some(found) = crate::icons::lookup(&name) {
                return found;
            }
        }
        if let Some(uri) = pixmap_uri(properties, pixmap_key) {
            return uri;
        }
    }
    String::new()
}

/// Turns the largest `a(iiay)` pixmap into a PNG data URI.
///
/// The bytes are ARGB32, most significant byte first, which is not what a PNG
/// wants: they have to be shuffled to RGBA before they mean anything.
fn pixmap_uri(properties: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    let value = properties.get(key)?;
    let pixmaps = Vec::<(i32, i32, Vec<u8>)>::try_from(value.clone()).ok()?;
    // The biggest one: a bar at any sane scale wants the sharpest available.
    let (width, height, argb) =
        pixmaps.into_iter().filter(|(w, h, _)| *w > 0 && *h > 0).max_by_key(|(w, _, _)| *w)?;

    let expected = (width as usize).checked_mul(height as usize)?.checked_mul(4)?;
    if argb.len() < expected {
        return None;
    }

    let mut rgba = Vec::with_capacity(expected);
    for pixel in argb.chunks_exact(4).take(expected / 4) {
        rgba.extend_from_slice(&[pixel[1], pixel[2], pixel[3], pixel[0]]);
    }

    png_data_uri(&rgba, width as u32, height as u32)
}

/// RGBA bytes as a PNG file's worth of bytes.
///
/// Shared with the notification server, which receives pictures on the bus in
/// the same shape and writes them to its cache.
pub fn png_bytes(rgba: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    if rgba.len() < (width as usize).checked_mul(height as usize)?.checked_mul(4)? {
        return None;
    }
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(Cursor::new(&mut png), width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(rgba).ok()?;
    }
    Some(png)
}

/// The same, as something an `<img src>` will take directly. Right for a
/// tray icon, which is tiny and sent once; wrong for anything sent often.
fn png_data_uri(rgba: &[u8], width: u32, height: u32) -> Option<String> {
    Some(format!("data:image/png;base64,{}", base64(&png_bytes(rgba, width, height)?)))
}

/// Base64, because pulling a crate in for thirty lines of table lookup is not
/// a dependency, it is a liability.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let triple = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(triple >> (18 - i * 6) & 0x3f) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn read_items(connection: &Connection) -> Vec<Item> {
    registered(connection)
        .into_iter()
        .filter_map(|entry| {
            // "/org/ayatana/NotificationItem/foo" is a path with slashes in
            // it, so only the first one separates the name from the path.
            let (service, path) = match entry.split_once('/') {
                Some((service, path)) => (service.to_string(), format!("/{path}")),
                None => (entry.clone(), "/StatusNotifierItem".to_string()),
            };
            let properties = properties(connection, &service, &path)?;

            let status = string_of(&properties, "Status");
            // An item that says it is Passive is asking not to be drawn.
            if status == "Passive" {
                return None;
            }

            let menu = path_of(&properties, "Menu");
            Some(Item {
                key: format!("{service}{path}"),
                id: string_of(&properties, "Id"),
                title: {
                    let title = string_of(&properties, "Title");
                    if title.is_empty() { string_of(&properties, "Id") } else { title }
                },
                icon: icon_for(&properties, &string_of(&properties, "IconThemePath")),
                has_menu: !menu.is_empty(),
                status,
            })
        })
        .collect()
}

/// The current items, for a caller that has just started listening.
///
/// The watcher only emits when the list changes, and the first change happens
/// before the webview has subscribed — so without this the tray stays empty
/// until an application happens to add or remove an icon.
pub fn items() -> Vec<Item> {
    connection().map(|connection| read_items(&connection)).unwrap_or_default()
}

/// Announces this process as a tray host and then keeps the item list current.
///
/// Blocks; meant for its own thread. The watcher is somebody else's — the
/// shell runs one — so this registers with whatever is already there rather
/// than trying to own the name itself.
pub fn watch(mut on_change: impl FnMut(Vec<Item>)) {
    let Some(connection) = connection() else {
        eprintln!("caelestia-bar: no session bus, so no tray");
        return;
    };

    if let Ok(proxy) = Proxy::new(&connection, WATCHER, WATCHER_PATH, WATCHER) {
        let host = format!("org.kde.StatusNotifierHost-{}", std::process::id());
        let _ = connection.request_name(host.as_str());
        let _ = proxy.call::<_, _, ()>("RegisterStatusNotifierHost", &(host.as_str(),));
    }

    // The watcher emits when items come and go, and each item emits when it
    // changes; subscribing to every one of those is a lot of plumbing for a
    // list that is a handful of entries. Re-reading it on a slow tick, and
    // only telling the front end when it actually differs, is the same
    // outcome for a fraction of the code.
    let mut last: Option<Vec<Item>> = None;
    let diagnosing = std::env::var_os("CAELESTIA_BAR_DIAG").is_some();
    loop {
        let items = read_items(&connection);
        if diagnosing && last.as_ref().map(Vec::len) != Some(items.len()) {
            eprintln!("caelestia-bar[tray]: {} item(s)", items.len());
        }
        if last.as_ref() != Some(&items) {
            last = Some(items.clone());
            on_change(items);
        }
        // Four DBus round trips per item per pass, and a tray changes when a
        // person starts or stops an application.
        std::thread::sleep(Duration::from_secs(4));
    }
}

/// A left click, which is whatever the application decided it is.
pub fn activate(key: &str, x: i32, y: i32) {
    call_item(key, "Activate", x, y);
}

pub fn secondary_activate(key: &str, x: i32, y: i32) {
    call_item(key, "SecondaryActivate", x, y);
}

fn call_item(key: &str, method: &str, x: i32, y: i32) {
    let Some((service, path)) = split_key(key) else { return };
    let Some(connection) = connection() else { return };
    for interface in ITEM_INTERFACES {
        let Ok(name) = InterfaceName::try_from(interface) else { continue };
        if let Ok(proxy) = Proxy::new(&connection, service.as_str(), path.as_str(), name) {
            if proxy.call::<_, _, ()>(method, &(x, y)).is_ok() {
                return;
            }
        }
    }
}

/// The item's menu, as a tree.
///
/// `GetLayout` with a depth of -1 returns the whole thing in one call, which
/// is what a popout wants: opening a submenu should not be a round trip.
pub fn menu(key: &str) -> Vec<MenuEntry> {
    let Some((service, path)) = menu_path(key) else { return Vec::new() };
    let Some(connection) = connection() else { return Vec::new() };
    let Ok(proxy) = Proxy::new(&connection, service.as_str(), path.as_str(), MENU_INTERFACE) else {
        return Vec::new();
    };

    // Asking the item to refresh before reading it: menus that change with
    // state (a "Pause" that becomes "Play") only update when prompted.
    let _ = proxy.call::<_, _, bool>("AboutToShow", &(0i32,));

    let properties: Vec<&str> = vec![];
    let Ok((_revision, root)) =
        proxy.call::<_, _, (u32, MenuNode)>("GetLayout", &(0i32, -1i32, properties.as_slice()))
    else {
        return Vec::new();
    };
    root.entries()
}

/// Clicks one entry. The menu is the item's, so the click has to go back to
/// it rather than being acted on here.
pub fn click(key: &str, id: i32) {
    let Some((service, path)) = menu_path(key) else { return };
    let Some(connection) = connection() else { return };
    let Ok(proxy) = Proxy::new(&connection, service.as_str(), path.as_str(), MENU_INTERFACE) else {
        return;
    };
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0);
    let _ = proxy.call::<_, _, ()>("Event", &(id, "clicked", Value::from(0i32), timestamp));
}

/// Where an item keeps its menu, which is a path on the item's own name.
fn menu_path(key: &str) -> Option<(String, String)> {
    let (service, path) = split_key(key)?;
    let connection = connection()?;
    let properties = properties(&connection, &service, &path)?;
    let menu = path_of(&properties, "Menu");
    if menu.is_empty() {
        return None;
    }
    Some((service, menu))
}

/// One node of a dbusmenu layout.
///
/// The wire type is `(ia{sv}av)` — and it is *not* recursive, however much it
/// looks it: a child is a variant that happens to contain another node. Which
/// is just as well, because a type that contains itself has no constant
/// signature and cannot be derived at all.
#[derive(Debug, zbus::zvariant::Type, serde::Deserialize)]
struct MenuNode {
    // The root's own id and properties are never drawn — it is the menu, not
    // an entry in it — but both have to be here or the reply does not match
    // the signature and nothing deserialises at all.
    #[allow(dead_code)]
    id: i32,
    #[allow(dead_code)]
    properties: HashMap<String, OwnedValue>,
    children: Vec<OwnedValue>,
}

impl MenuNode {
    fn entries(self) -> Vec<MenuEntry> {
        self.children.iter().filter_map(entry_from_value).collect()
    }
}

/// Unwraps one child variant into an entry, and its own children with it.
fn entry_from_value(value: &OwnedValue) -> Option<MenuEntry> {
    // A child arrives boxed in a variant; some implementations box it twice.
    let mut inner = Value::from(value.try_clone().ok()?);
    while let Value::Value(nested) = inner {
        inner = *nested;
    }
    let Value::Structure(structure) = inner else { return None };

    let fields = structure.into_fields();
    let mut fields = fields.into_iter();
    let id = i32::try_from(fields.next()?).ok()?;
    let properties: HashMap<String, OwnedValue> = HashMap::try_from(
        OwnedValue::try_from(fields.next()?).ok()?,
    )
    .ok()?;
    let children: Vec<OwnedValue> =
        Vec::try_from(OwnedValue::try_from(fields.next()?).ok()?).unwrap_or_default();

    Some(entry_from_parts(id, &properties, &children))
}

fn entry_from_parts(
    id: i32,
    properties: &HashMap<String, OwnedValue>,
    children: &[OwnedValue],
) -> MenuEntry {
    let text = |key: &str| -> String {
        properties
            .get(key)
            .and_then(|value| String::try_from(value.clone()).ok())
            .unwrap_or_default()
    };
    let flag = |key: &str, fallback: bool| -> bool {
        properties.get(key).and_then(|value| bool::try_from(value.clone()).ok()).unwrap_or(fallback)
    };

    let kind = match text("type").as_str() {
        "separator" => "separator",
        // A toggle says so through `toggle-type`, not through `type`.
        _ if !text("toggle-type").is_empty() => "checkmark",
        _ => "standard",
    };
    let checked = properties
        .get("toggle-state")
        .and_then(|value| i32::try_from(value.clone()).ok())
        .unwrap_or(0)
        == 1;

    MenuEntry {
        id,
        // Menus mark their access key with an underscore, which is not
        // something a pointer-driven popout has any use for.
        label: text("label").replace('_', ""),
        enabled: flag("enabled", true) && flag("visible", true),
        kind: kind.to_string(),
        checked,
        children: children.iter().filter_map(entry_from_value).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn a_key_splits_into_a_name_and_a_path() {
        let (service, path) = split_key(":1.42/StatusNotifierItem").expect("a well formed key");
        assert_eq!(service, ":1.42");
        assert_eq!(path, "/StatusNotifierItem");
    }

    #[test]
    fn a_nested_path_keeps_its_slashes() {
        // Ayatana's items live several levels down, and splitting on the last
        // slash rather than the first loses the name.
        let (service, path) =
            split_key(":1.7/org/ayatana/NotificationItem/nm_applet").expect("a well formed key");
        assert_eq!(service, ":1.7");
        assert_eq!(path, "/org/ayatana/NotificationItem/nm_applet");
    }
}

#[cfg(test)]
mod probe {
    #[test]
    #[ignore = "diagnostic: prints each item's menu"]
    fn dump_menus() {
        for item in super::items() {
            let entries = super::menu(&item.key);
            println!("{} menu_path={} entries={}", item.id, item.has_menu, entries.len());
            for entry in entries.iter().take(4) {
                println!("   {:?} {:?} kids={}", entry.label, entry.kind, entry.children.len());
            }
        }
    }

    #[test]
    #[ignore = "diagnostic: prints what this session's tray actually exposes"]
    fn dump() {
        let connection = super::connection().expect("session bus");
        for entry in super::registered(&connection) {
            let (service, path) = match entry.split_once('/') {
                Some((s, p)) => (s.to_string(), format!("/{p}")),
                None => (entry.clone(), "/StatusNotifierItem".to_string()),
            };
            let props = super::properties(&connection, &service, &path);
            println!("{entry} -> {:?}", props.as_ref().map(|p| {
                let mut keys: Vec<_> = p.keys().cloned().collect();
                keys.sort();
                (keys, super::string_of(p, "Status"), super::string_of(p, "Id"))
            }));
        }
        for item in super::read_items(&connection) {
            let icon = &item.icon;
            println!(
                "{} icon={}",
                item.id,
                if icon.len() > 80 { format!("{}…", &icon[..80]) } else { icon.clone() }
            );
        }
    }
}
