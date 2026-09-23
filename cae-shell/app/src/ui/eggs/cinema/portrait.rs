//! The portrait, painted once, at the size it will be seen at.
//!
//! It is `portrait.svg`, drawn over the proportions of the official portrait
//! (Avi Ohayon, Israeli Government Press Office, CC BY-SA 3.0): gradients
//! for the light across the face, soft shadows, eyes clipped to their lids.
//! It does not move except as a whole, so it is rasterised here as the
//! cinema starts, by the same resvg GPUI draws its own SVGs with, and the
//! frames draw it as one picture, turned and scaled and rimmed with light by
//! the shader.

use resvg::{tiny_skia, usvg};

const DRAWING: &str = include_str!("portrait.svg");

/// The portrait `height` pixels tall, premultiplied, as the card takes it.
pub fn paint(height: u32) -> Option<image::RgbaImage> {
    let tree = usvg::Tree::from_str(DRAWING, &usvg::Options::default()).ok()?;
    let size = tree.size();
    let scale = height as f32 / size.height();
    let width = (size.width() * scale).ceil() as u32;
    let mut pixmap = tiny_skia::Pixmap::new(width.max(1), height.max(1))?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    image::RgbaImage::from_raw(pixmap.width(), pixmap.height(), pixmap.take())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_portrait_is_painted_at_the_size_asked_and_is_not_empty() {
        let picture = paint(300).expect("a picture");
        assert_eq!(picture.height(), 300);
        assert_eq!(picture.width(), 240);
        // Something where the face is, nothing in the top corners.
        assert!(picture.get_pixel(120, 150)[3] > 200, "the face is drawn");
        assert_eq!(picture.get_pixel(2, 2)[3], 0, "the corner is clear");
    }
}
