//! An image's size, from its header.
//!
//! Picking a random wallpaper filters out anything smaller than the screen,
//! which needs the dimensions of every candidate and nothing else — so none
//! of these read past the header.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// The formats the wallpaper command accepts.
pub const WALLPAPER_EXTENSIONS: [&str; 7] = ["jpg", "jpeg", "png", "webp", "tif", "tiff", "gif"];

pub fn is_wallpaper(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|e| e.to_str()) else { return false };
    path.is_file() && WALLPAPER_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
}

pub fn dimensions(path: &Path) -> Option<(u32, u32)> {
    let mut file = File::open(path).ok()?;
    let mut head = [0u8; 32];
    let read = file.read(&mut head).ok()?;
    let head = &head[..read];

    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        return png(head);
    }
    if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        return Some((u16(head, 6)? as u32, u16(head, 8)? as u32));
    }
    if head.starts_with(b"RIFF") && head.get(8..12) == Some(b"WEBP") {
        return webp(&mut file);
    }
    if head.starts_with(b"\xff\xd8") {
        return jpeg(&mut file);
    }
    if head.starts_with(b"II\x2a\x00") || head.starts_with(b"MM\x00\x2a") {
        return tiff(&mut file, head[0] == b'I');
    }
    if head.starts_with(b"BM") {
        // A BMP height is signed: negative means the rows are top-down.
        return Some((u32le(head, 18)?, (u32le(head, 22)? as i32).unsigned_abs()));
    }
    None
}

fn u16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32le(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn u32be(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// IHDR is always the first chunk, and always at the same offset.
fn png(head: &[u8]) -> Option<(u32, u32)> {
    (head.get(12..16)? == b"IHDR").then(|| ())?;
    Some((u32be(head, 16)?, u32be(head, 20)?))
}

/// Three flavours: a plain VP8 keyframe, a lossless VP8L, or a VP8X header
/// that states the size outright.
fn webp(file: &mut File) -> Option<(u32, u32)> {
    // A one-pixel lossless file is shorter than this buffer, so take what is
    // there rather than insisting on all of it.
    let mut header = [0u8; 30];
    file.seek(SeekFrom::Start(12)).ok()?;
    let read = file.read(&mut header).ok()?;
    if read < 18 {
        return None;
    }

    match &header[0..4] {
        b"VP8X" if read >= 14 => {
            let width = u32::from_le_bytes([header[8], header[9], header[10], 0]) + 1;
            let height = u32::from_le_bytes([header[11], header[12], header[13], 0]) + 1;
            Some((width, height))
        }
        b"VP8L" if read >= 13 => {
            let bits = u32::from_le_bytes(header[9..13].try_into().ok()?);
            Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
        }
        b"VP8 " => {
            // The keyframe's start code sits three bytes into the frame tag.
            (read >= 18 && header[11..14] == [0x9d, 0x01, 0x2a]).then(|| ())?;
            let width = u16::from_le_bytes(header[14..16].try_into().ok()?) & 0x3fff;
            let height = u16::from_le_bytes(header[16..18].try_into().ok()?) & 0x3fff;
            Some((width as u32, height as u32))
        }
        _ => None,
    }
}

/// Walk the markers to the frame header, which is the only one that carries
/// the size.
fn jpeg(file: &mut File) -> Option<(u32, u32)> {
    file.seek(SeekFrom::Start(2)).ok()?;
    let mut byte = [0u8; 1];
    loop {
        // Markers can be preceded by any number of 0xff fill bytes.
        loop {
            file.read_exact(&mut byte).ok()?;
            if byte[0] == 0xff {
                break;
            }
        }
        let marker = loop {
            file.read_exact(&mut byte).ok()?;
            if byte[0] != 0xff {
                break byte[0];
            }
        };

        // Every SOFn but the two that are not frame headers.
        if (0xc0..=0xcf).contains(&marker) && ![0xc4, 0xc8, 0xcc].contains(&marker) {
            let mut frame = [0u8; 7];
            file.read_exact(&mut frame).ok()?;
            let height = u16::from_be_bytes([frame[3], frame[4]]) as u32;
            let width = u16::from_be_bytes([frame[5], frame[6]]) as u32;
            return Some((width, height));
        }
        if marker == 0xd9 || marker == 0xda {
            return None; // end of image, or the entropy data starts
        }
        if (0xd0..=0xd8).contains(&marker) || marker == 0x01 {
            continue; // no payload
        }
        let mut length = [0u8; 2];
        file.read_exact(&mut length).ok()?;
        let length = u16::from_be_bytes(length);
        file.seek(SeekFrom::Current(length as i64 - 2)).ok()?;
    }
}

/// The first directory's ImageWidth and ImageLength tags.
fn tiff(file: &mut File, little_endian: bool) -> Option<(u32, u32)> {
    let read_u16 = |bytes: [u8; 2]| {
        if little_endian {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        }
    };
    let read_u32 = |bytes: [u8; 4]| {
        if little_endian {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        }
    };

    let mut offset = [0u8; 4];
    file.seek(SeekFrom::Start(4)).ok()?;
    file.read_exact(&mut offset).ok()?;
    file.seek(SeekFrom::Start(read_u32(offset) as u64)).ok()?;

    let mut count = [0u8; 2];
    file.read_exact(&mut count).ok()?;
    let (mut width, mut height) = (None, None);
    for _ in 0..read_u16(count) {
        let mut entry = [0u8; 12];
        file.read_exact(&mut entry).ok()?;
        let tag = read_u16([entry[0], entry[1]]);
        let kind = read_u16([entry[2], entry[3]]);
        // A SHORT is stored in the first two bytes of the value field; a LONG
        // fills it.
        let value = match kind {
            3 => read_u16([entry[8], entry[9]]) as u32,
            4 => read_u32([entry[8], entry[9], entry[10], entry[11]]),
            _ => continue,
        };
        match tag {
            256 => width = Some(value),
            257 => height = Some(value),
            _ => {}
        }
        if let (Some(w), Some(h)) = (width, height) {
            return Some((w, h));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_size_of_every_jpeg_in_the_corpus() {
        let dir = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/jpeg"));
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut checked = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            let bytes = std::fs::read(&path).unwrap();
            let decoded = crate::material::jpeg::decode(&bytes).unwrap();
            assert_eq!(
                dimensions(&path),
                Some((decoded.width as u32, decoded.height as u32)),
                "{}",
                path.display()
            );
            checked += 1;
        }
        assert!(checked > 30, "only {checked} images checked");
    }

    #[test]
    fn only_image_extensions_are_wallpapers() {
        assert!(!is_wallpaper(std::path::Path::new("/tmp/not-there.png")));
        assert!(!is_wallpaper(std::path::Path::new("/etc/hostname")));
    }

    /// Every format the wallpaper command accepts, at sizes down to one
    /// pixel, against the size PIL reports for the same file.
    #[test]
    fn reads_the_size_of_every_format() {
        use redcommon::json::Json;

        let vectors = redcommon::json::parse(include_str!("../tests/size-vectors.json"))
            .expect("reference vectors are readable");
        let Some(Json::Arr(images)) = vectors.get("images") else { panic!("no images") };

        let dir = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/sizes"));
        for image in images {
            let name = image.str_field("name").expect("image name");
            let expected = (
                image.get("width").and_then(Json::as_u64).unwrap() as u32,
                image.get("height").and_then(Json::as_u64).unwrap() as u32,
            );
            assert_eq!(dimensions(&dir.join(name)), Some(expected), "{name}");
        }
        assert!(images.len() >= 20, "corpus shrank");
    }
}
