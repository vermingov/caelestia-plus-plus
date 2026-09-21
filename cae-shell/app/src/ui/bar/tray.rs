//! The tray: whatever applications have put there.
//!
//! Icons arrive as a path the backend resolved, or as a `data:` URI it built
//! from raw pixels an item handed over. GPUI loads a path itself; the pixels
//! are decoded once and kept, because the strip is redrawn every second and
//! an application's icon is not something to decode that often.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use cae_core::tray;
use gpui::{AnyElement, Image, ImageFormat, IntoElement, MouseButton, ObjectFit, Styled, img, prelude::*, px};

use super::pieces::{Tone, section, slot};
use crate::ui::glyph::glyph;
use crate::ui::popout::{Hover, Kind};
use crate::ui::rsx;

/// Icons that came as pixels, by the URI they came as.
#[derive(Default)]
pub struct Icons {
    decoded: HashMap<String, Arc<Image>>,
}

impl Icons {
    /// Forgets icons no item is using any more. An application that animates
    /// its tray icon sends a new one every frame of it.
    pub fn keep_only(&mut self, items: &[tray::Item]) {
        self.decoded.retain(|uri, _| items.iter().any(|item| &item.icon == uri));
    }

    fn pixels(&mut self, uri: &str) -> Option<Arc<Image>> {
        if let Some(image) = self.decoded.get(uri) {
            return Some(image.clone());
        }
        let bytes = base64(uri.strip_prefix("data:image/png;base64,")?)?;
        let image = Arc::new(Image::from_bytes(ImageFormat::Png, bytes));
        self.decoded.insert(uri.to_string(), image.clone());
        Some(image)
    }
}

/// The reverse of the thirty lines in the backend that built the URI.
fn base64(text: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(text.len() / 4 * 3);
    let (mut buffer, mut bits) = (0_u32, 0_u32);
    for character in text.bytes() {
        let value = match character {
            b'A'..=b'Z' => character - b'A',
            b'a'..=b'z' => character - b'a' + 26,
            b'0'..=b'9' => character - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => return None,
        };
        buffer = buffer << 6 | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((buffer >> bits) as u8);
        }
    }
    Some(bytes)
}

pub fn row(items: &[tray::Item], icons: &mut Icons, hover: &Hover) -> Option<impl IntoElement + use<>> {
    if items.is_empty() {
        return None;
    }
    icons.keep_only(items);

    Some(rsx! {
        <div base={section()} class="gap-[1px]">
            {for (index, item) in items.iter().enumerate() {
                {button(index, item, icons, hover)}
            }}
        </div>
    })
}

/// An item's picture: the file the backend found for it, the pixels it handed
/// over, or a placeholder. A blank box says nothing; the placeholder at least
/// says there is an item here.
fn picture(item: &tray::Item, icons: &mut Icons) -> AnyElement {
    // Sized on the image itself: an image left to its own size is as big as
    // its file says it is, which for a tray icon is often 256px.
    let sized = |source: gpui::ImageSource| {
        rsx! { <img src={source} class="size-[16px]" object_fit={ObjectFit::Contain} /> }.into_any_element()
    };

    if item.icon.starts_with("data:") {
        if let Some(pixels) = icons.pixels(&item.icon) {
            return sized(pixels.into());
        }
    } else if !item.icon.is_empty() {
        return sized(PathBuf::from(&item.icon).into());
    }
    glyph("web_asset", px(17.)).into_any_element()
}

fn button(index: usize, item: &tray::Item, icons: &mut Icons, hover: &Hover) -> impl IntoElement + use<> {
    let (activate, secondary, used) = (item.key.clone(), item.key.clone(), hover.clone());
    let button = rsx! {
        <div
            base={slot(if item.status == "NeedsAttention" { Tone::Lit } else { Tone::Plain })}
            id={("tray", index)}
            // Left click is whatever the application decided it is. The
            // coordinates are the pointer's, because an application may put a
            // menu of its own there. The menu this bar drew goes: the item
            // has been used, and it would be left hanging over what that did.
            onClick={move |event, _, cx| {
                let (key, at) = (activate.clone(), event.position());
                cx.background_spawn(async move { tray::activate(&key, f32::from(at.x) as i32, f32::from(at.y) as i32) })
                    .detach();
                used.dismiss(cx);
            }}
            onMouseDown={(MouseButton::Middle, move |event, _, cx| {
                let (key, at) = (secondary.clone(), event.position);
                cx.background_spawn(async move {
                    tray::secondary_activate(&key, f32::from(at.x) as i32, f32::from(at.y) as i32)
                })
                .detach();
            })}
        >
            {picture(item, icons)}
        </div>
    };
    // Hovering opens the item's menu, and so does a right click: a tray icon
    // whose menu is the whole point of it should not need to be right-clicked
    // to admit that.
    hover.menu(Kind::Tray(item.key.clone().into()), button)
}

#[cfg(test)]
mod tests {
    use super::base64;

    #[test]
    fn base64_comes_back_as_what_went_in() {
        assert_eq!(base64("aGVsbG8=").as_deref(), Some(&b"hello"[..]));
        assert_eq!(base64("aGk=").as_deref(), Some(&b"hi"[..]));
        assert_eq!(base64("").as_deref(), Some(&b""[..]));
        assert_eq!(base64("not base64!"), None);
    }
}
