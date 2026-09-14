//! The wallpapers on disk, and setting one.
//!
//! Browsing wallpapers is the one mode that is pictures rather than rows, so
//! the backend's job is to hand the webview a path it can actually load and
//! a name to show. Thumbnails the CLI has already cached are preferred: a
//! grid of full-size wallpapers would have the webview decoding tens of
//! megabytes to draw a few hundred pixels.

use std::path::{Path, PathBuf};

use serde::Serialize;

const EXTENSIONS: [&str; 7] = ["jpg", "jpeg", "png", "webp", "tif", "tiff", "gif"];

#[derive(Clone, Serialize)]
pub struct Wallpaper {
    pub path: String,
    pub name: String,
    /// The directory under the wallpaper root, which the shell shows as a
    /// category; empty for loose files.
    pub category: String,
    /// What to actually display: a cached thumbnail when there is one.
    pub preview: String,
    #[serde(skip)]
    pub haystack: String,
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

fn cache_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".cache"))
        .join("caelestia/wallpapers")
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// The CLI caches a 128px thumbnail per wallpaper, keyed by the hash of the
/// file. Finding it means hashing, which is cheap next to decoding the image.
fn cached_thumbnail(path: &Path) -> Option<String> {
    let hash = sha256_file(path)?;
    let thumb = cache_dir().join(hash).join("thumbnail.jpg");
    thumb.is_file().then(|| thumb.to_string_lossy().into_owned())
}

pub fn load(root: &Path) -> Vec<Wallpaper> {
    let mut found = Vec::new();
    collect(root, root, &mut found);
    found.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    found
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<Wallpaper>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, out);
            continue;
        }
        if !is_image(&path) {
            continue;
        }
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let category = path
            .parent()
            .and_then(|parent| parent.strip_prefix(root).ok())
            .map(|rest| rest.to_string_lossy().into_owned())
            .unwrap_or_default();
        let full = path.to_string_lossy().into_owned();
        out.push(Wallpaper {
            haystack: format!("{} {}", name.to_lowercase(), category.to_lowercase()),
            preview: cached_thumbnail(&path).unwrap_or_else(|| full.clone()),
            path: full,
            name,
            category,
        });
    }
}

pub fn current() -> Option<String> {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/state"));
    std::fs::read_to_string(state.join("caelestia/wallpaper/path.txt"))
        .ok()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
}

/// Detached, like a scheme change: setting a wallpaper regenerates a palette
/// and rewrites every themed config, which outlasts the launcher's close.
pub fn set(path: &str) {
    let quoted = format!("'{}'", path.replace('\'', r"'\''"));
    let _ = std::process::Command::new("sh")
        .args(["-c", &format!("setsid -f caelestia wallpaper -f {quoted} >/dev/null 2>&1")])
        .spawn();
}

/// Enough SHA-256 to find a cache directory. The CLI names them by the hash
/// of the file's contents, so this has to agree with it byte for byte.
fn sha256_file(path: &Path) -> Option<String> {
    use std::io::Read;

    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Some(hasher.finish().iter().map(|b| format!("{b:02x}")).collect())
}

struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    buffered: usize,
    length: u64,
}

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

impl Sha256 {
    fn new() -> Sha256 {
        Sha256 {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buffer: [0; 64],
            buffered: 0,
            length: 0,
        }
    }

    fn update(&mut self, mut data: &[u8]) {
        self.length += data.len() as u64;
        if self.buffered > 0 {
            let want = 64 - self.buffered;
            let take = want.min(data.len());
            self.buffer[self.buffered..self.buffered + take].copy_from_slice(&data[..take]);
            self.buffered += take;
            data = &data[take..];
            if self.buffered < 64 {
                return;
            }
            let block = self.buffer;
            self.compress(&block);
            self.buffered = 0;
        }
        while data.len() >= 64 {
            let (block, rest) = data.split_at(64);
            self.compress(block.try_into().expect("64 bytes"));
            data = rest;
        }
        self.buffer[..data.len()].copy_from_slice(data);
        self.buffered = data.len();
    }

    fn finish(mut self) -> [u8; 32] {
        let bits = self.length * 8;
        let mut tail = Vec::with_capacity(72);
        tail.push(0x80u8);
        while (self.buffered + tail.len()) % 64 != 56 {
            tail.push(0);
        }
        tail.extend_from_slice(&bits.to_be_bytes());
        self.update_without_length(&tail);

        let mut out = [0u8; 32];
        for (i, word) in self.state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    /// The padding must not count toward the length it encodes.
    fn update_without_length(&mut self, data: &[u8]) {
        let length = self.length;
        self.update(data);
        self.length = length;
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(block[i * 4..i * 4 + 4].try_into().expect("4 bytes"));
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, value) in self.state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        hasher.finish().iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn the_hash_is_sha256() {
        assert_eq!(
            digest(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            digest(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // Longer than one block, so the buffering path is exercised too.
        assert_eq!(
            digest(&vec![b'a'; 1000]),
            "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3"
        );
    }

    #[test]
    fn it_agrees_with_the_cli_about_a_real_file() {
        // The cache directory is named by this hash; disagreeing means every
        // thumbnail lookup misses.
        let path = Path::new("/etc/hostname");
        let Some(ours) = sha256_file(path) else { return };
        let out = std::process::Command::new("sha256sum").arg(path).output().expect("sha256sum");
        let theirs = String::from_utf8_lossy(&out.stdout);
        assert_eq!(ours, theirs.split_whitespace().next().unwrap_or(""));
    }

    #[test]
    fn only_images_are_wallpapers() {
        assert!(is_image(Path::new("a/b/sky.jpg")));
        assert!(is_image(Path::new("a/b/sky.WEBP")));
        assert!(!is_image(Path::new("a/b/notes.txt")));
        assert!(!is_image(Path::new("a/b/noextension")));
    }
}
