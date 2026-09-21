//! Small copies of large pictures.
//!
//! A page of wallpapers is a page of photographs several thousand pixels
//! wide, each drawn a couple of hundred wide. Decoded whole, thirty of them
//! are most of a gigabyte. So each is decoded once, off the thread that
//! draws, and a small copy kept under the cache directory; from then on the
//! page costs what thirty small pictures cost.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Wide enough for a tile on a screen that doubles its pixels.
const WIDTH: u32 = 520;
const HEIGHT: u32 = 325;

fn cache_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let cache = std::env::var_os("XDG_CACHE_HOME").map_or_else(|| home.join(".cache"), PathBuf::from);
    Some(cache.join("caelestia/thumbs"))
}

/// FNV-1a. The name of a cache file has to mean the same thing the next time
/// the shell is built, which the standard library's hasher does not promise.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3))
}

/// Where the small copy of `picture` lives. The name is made of the path and
/// of when the file was last written and how big it is, so a picture that is
/// replaced under the same name gets a new copy rather than keeping the old.
fn kept_at(picture: &Path, dir: &Path) -> Option<PathBuf> {
    let about = std::fs::metadata(picture).ok()?;
    let written = about.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs();
    let key = format!("{}\n{written}\n{}", picture.display(), about.len());
    Some(dir.join(format!("{:016x}.jpg", fnv(key.as_bytes()))))
}

/// The small copy of `picture`, made now if there is none. Slow the first
/// time, which is why it is never called from the thread that draws. Nothing
/// for a file that is not a picture this can read.
pub fn thumbnail(picture: &Path) -> Option<PathBuf> {
    let dir = cache_dir()?;
    let kept = kept_at(picture, &dir)?;
    if kept.is_file() {
        return Some(kept);
    }
    std::fs::create_dir_all(&dir).ok()?;

    let small = image::open(picture).ok()?.thumbnail(WIDTH, HEIGHT).into_rgb8();
    // Beside it and then moved: two pages asking for the same picture at
    // once would otherwise each be reading the other's half-written file.
    let beside = kept.with_extension(format!("{}.part", std::process::id()));
    small.save_with_format(&beside, image::ImageFormat::Jpeg).ok()?;
    std::fs::rename(&beside, &kept).ok()?;
    Some(kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cache_name_follows_the_file_and_not_only_its_path() {
        let dir = std::env::temp_dir().join(format!("cae-thumbs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let picture = dir.join("wall.png");

        std::fs::write(&picture, b"one").unwrap();
        let first = kept_at(&picture, &dir).unwrap();
        assert_eq!(kept_at(&picture, &dir), Some(first.clone()), "the same file is the same name");

        std::fs::write(&picture, b"another picture").unwrap();
        assert_ne!(kept_at(&picture, &dir), Some(first), "a replaced file kept its old copy");
        assert_eq!(kept_at(&dir.join("missing.png"), &dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_hash_is_the_published_one() {
        // FNV-1a's own test vectors: a cache written by one build has to be
        // found by the next.
        assert_eq!(fnv(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
