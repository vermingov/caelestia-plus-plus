//! Pictures a notification carries, as files the webview is allowed to load.
//!
//! They arrive three ways — a theme name, a path to anywhere on the disk, or
//! raw pixels on the wire — and a webview can use none of them as they come.
//! A theme name means nothing to it; the asset protocol only serves a short
//! list of directories, which `/tmp/album-art-Xa3f.jpg` is not in; and pixels
//! are not a URL.
//!
//! So everything ends up as a file in one cache directory that *is* on that
//! list. A file rather than a data URI, because the feed is sent whole every
//! time it changes: three hundred notifications with their album art inline
//! would be megabytes per toast.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use zbus::zvariant::{OwnedValue, Value};

use super::bus::string_hint;

/// The longest side a stored picture is allowed. The avatar is about forty
/// pixels across, so this is sharp at three times scale and still small.
const LONGEST_SIDE: usize = 128;

/// Nothing bigger than this is copied into the cache. A sender that points at
/// a forty-megabyte file does not get forty megabytes of this disk for it.
const LARGEST_COPY: u64 = 4 * 1024 * 1024;

/// System directories the asset protocol already serves; see
/// `tauri.conf.json`. A file under one of these is handed over as it is.
const SERVED: [&str; 4] = ["/usr/share/", "/usr/local/share/", "/opt/", "/var/lib/flatpak/exports/share/"];

/// Whether the webview can be given this path as it stands: it is under one
/// of the system directories, one of the user's own icon directories, or the
/// cache this module writes to.
fn is_served(path: &str) -> bool {
    if SERVED.iter().any(|root| path.starts_with(root)) {
        return true;
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let in_home = [".local/share/", ".icons/"].iter().any(|dir| path.starts_with(&format!("{home}/{dir}")));
    in_home || cache_dir().is_some_and(|dir| Path::new(path).starts_with(dir))
}

fn cache_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let cache = std::env::var("XDG_CACHE_HOME").unwrap_or(format!("{home}/.cache"));
    Some(PathBuf::from(cache).join("caelestia/imagecache/notifs"))
}

/// The sender's own icon.
///
/// Usually a theme name, which goes through the same lookup the tray uses.
/// The `desktop-entry` hint is tried after it: that names the application
/// rather than an icon, but most applications name their icon after
/// themselves.
pub fn app_icon(app_icon: &str, desktop_entry: Option<&str>) -> String {
    let path = app_icon.strip_prefix("file://").unwrap_or(app_icon);
    if path.starts_with('/') {
        return servable(Path::new(path)).unwrap_or_default();
    }
    [Some(app_icon), desktop_entry]
        .into_iter()
        .flatten()
        .filter(|name| !name.is_empty())
        .find_map(crate::icons::lookup)
        .unwrap_or_default()
}

/// The picture a notification carried.
///
/// Three hints can hold one, and the spec's order of preference is the order
/// they are tried here.
pub fn from_hints(hints: &HashMap<String, OwnedValue>) -> String {
    for key in ["image-data", "image_data"] {
        if let Some(path) = hints.get(key).and_then(raw_image) {
            return path;
        }
    }
    for key in ["image-path", "image_path"] {
        let Some(value) = string_hint(hints, key) else { continue };
        let path = value.strip_prefix("file://").unwrap_or(&value);
        let found = if path.starts_with('/') {
            servable(Path::new(path))
        } else {
            crate::icons::lookup(path)
        };
        if let Some(found) = found {
            return found;
        }
    }
    hints.get("icon_data").and_then(raw_image).unwrap_or_default()
}

/// A path the webview may load: the file itself if it is somewhere the asset
/// protocol serves, otherwise a copy of it in the cache.
///
/// The copy is also what keeps the history honest. Senders point at temporary
/// files and delete them; a notification from yesterday should still have its
/// picture.
fn servable(path: &Path) -> Option<String> {
    let text = path.to_str()?;
    if is_served(text) {
        return path.is_file().then(|| text.to_string());
    }

    let size = std::fs::metadata(path).ok()?.len();
    if size == 0 || size > LARGEST_COPY {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("png");
    store(&bytes, extension)
}

/// `(iiibiiay)`: width, height, rowstride, alpha, bits per sample, channels,
/// and the bytes. Senders really do vary every one of those, so none of them
/// can be assumed.
fn raw_image(value: &OwnedValue) -> Option<String> {
    let structure = zbus::zvariant::Structure::try_from(value.clone()).ok()?;
    let [width, height, rowstride, has_alpha, bits, channels, data] = structure.fields() else {
        return None;
    };

    let width = usize::try_from(i32::try_from(width.clone()).ok()?).ok()?;
    let height = usize::try_from(i32::try_from(height.clone()).ok()?).ok()?;
    let rowstride = usize::try_from(i32::try_from(rowstride.clone()).ok()?).ok()?;
    let has_alpha = bool::try_from(has_alpha.clone()).ok()?;
    let bits = i32::try_from(bits.clone()).ok()?;
    let channels = usize::try_from(i32::try_from(channels.clone()).ok()?).ok()?;
    let Value::Array(array) = data else { return None };
    let bytes: Vec<u8> = array.iter().filter_map(|v| u8::try_from(v.clone()).ok()).collect();

    // Eight bits per sample is the only depth anything sends, and the only
    // one a PNG written here would get right.
    if width == 0 || height == 0 || bits != 8 || !(channels == 3 || channels == 4) {
        return None;
    }

    // Rows are padded to `rowstride`, which is not always width * channels.
    let mut rgba = Vec::with_capacity(width * height * 4);
    for row in 0..height {
        let start = row.checked_mul(rowstride)?;
        let line = bytes.get(start..start + width * channels)?;
        for pixel in line.chunks_exact(channels) {
            let alpha = if has_alpha && channels == 4 { pixel[3] } else { 0xff };
            rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], alpha]);
        }
    }

    let (rgba, width, height) = shrink(rgba, width, height);
    let png = crate::tray::png_bytes(&rgba, width as u32, height as u32)?;
    store(&png, "png")
}

/// Halves and halves again until the longest side fits, averaging each block
/// of pixels into one. Crude next to a proper resampler, and at avatar size
/// nobody can tell.
fn shrink(rgba: Vec<u8>, width: usize, height: usize) -> (Vec<u8>, usize, usize) {
    let factor = width.max(height).div_ceil(LONGEST_SIDE);
    if factor <= 1 {
        return (rgba, width, height);
    }

    let (small_width, small_height) = (width / factor, height / factor);
    let mut small = Vec::with_capacity(small_width * small_height * 4);
    for y in 0..small_height {
        for x in 0..small_width {
            let mut sum = [0u32; 4];
            for block_y in 0..factor {
                for block_x in 0..factor {
                    let at = ((y * factor + block_y) * width + x * factor + block_x) * 4;
                    for (channel, total) in sum.iter_mut().enumerate() {
                        *total += u32::from(rgba[at + channel]);
                    }
                }
            }
            let count = (factor * factor) as u32;
            small.extend(sum.map(|total| (total / count) as u8));
        }
    }
    (small, small_width, small_height)
}

/// Writes bytes into the cache under a name made from their content, so the
/// same picture sent a hundred times is one file.
fn store(bytes: &[u8], extension: &str) -> Option<String> {
    let dir = cache_dir()?;
    std::fs::create_dir_all(&dir).ok()?;

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    let path = dir.join(format!("{:016x}.{extension}", hasher.finish()));

    if !path.exists() {
        std::fs::write(&path, bytes).ok()?;
    }
    path.to_str().map(str::to_string)
}

/// Makes a notification restored from disk loadable.
///
/// The QML server stored what the sender sent: an icon *name*, which its own
/// image provider resolved at draw time, and sometimes an `image://` URL that
/// only Qt understands. A webview can load neither, so they are resolved once
/// here — after which the entry is saved in the new shape and never needs it
/// again.
pub fn normalise(notif: &mut super::Notification) {
    if notif.image.starts_with("image://") {
        notif.image.clear();
    }
    if !notif.app_icon.is_empty() && !is_served(&notif.app_icon) {
        notif.app_icon = app_icon(&notif.app_icon, None);
    }
}

/// Deletes every cached picture no notification points at any more.
///
/// Run once at startup against the history that was just loaded. The QML
/// server's cache was only ever added to; a year of album art is a lot of
/// files to keep for notifications that were dismissed the day they arrived.
pub fn prune(still_used: &[String]) {
    let Some(dir) = cache_dir() else { return };
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let kept = path.to_str().is_some_and(|text| still_used.iter().any(|used| used == text));
        if !kept && path.is_file() {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_large_picture_is_shrunk_to_fit_and_keeps_its_colour() {
        // A flat orange square, so any averaging must give the same orange.
        let (width, height) = (512, 384);
        let rgba: Vec<u8> = std::iter::repeat([0xff, 0x80, 0x10, 0xff]).take(width * height).flatten().collect();

        let (small, small_width, small_height) = shrink(rgba, width, height);
        assert!(small_width.max(small_height) <= LONGEST_SIDE);
        assert_eq!(small.len(), small_width * small_height * 4);
        assert_eq!(&small[..4], &[0xff, 0x80, 0x10, 0xff]);
        // The shape survives: four by three in, four by three out.
        assert_eq!(small_width * 3, small_height * 4);
    }

    #[test]
    fn a_small_picture_is_left_alone() {
        let rgba = vec![1, 2, 3, 4].repeat(64 * 64);
        let (same, width, height) = shrink(rgba.clone(), 64, 64);
        assert_eq!((width, height), (64, 64));
        assert_eq!(same, rgba);
    }

    #[test]
    fn a_theme_name_never_reaches_the_webview_as_a_name() {
        // Whatever this machine has installed, the answer is a path or
        // nothing — a bare name is something a webview cannot load.
        let icon = app_icon("utilities-terminal", None);
        assert!(icon.is_empty() || icon.starts_with('/'), "got {icon:?}");
        assert_eq!(app_icon("", None), "");
        assert_eq!(app_icon("/nonexistent/icon.png", None), "");
    }
}
