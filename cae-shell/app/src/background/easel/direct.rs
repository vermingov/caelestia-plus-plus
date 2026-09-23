//! Frames handed to the compositor directly, as dma-bufs, rather than
//! presented through a swapchain.
//!
//! A swapchain is the portable way, and on this machine it was the dearest
//! part of a frame: acquiring an image, presenting it and the semaphores in
//! between cost more than working out and drawing the whole helix. Handing
//! over is two images of the card's own, a buffer on the surface for each,
//! and the three requests any Wayland client makes to show a picture. The
//! compositor is only given a picture once it is finished — the card is
//! waited on first — and an image is only drawn on again once the compositor
//! has said it has let go of it.

use std::os::fd::AsFd;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ash::vk;
use wayland_client::QueueHandle;
use wayland_client::protocol::{wl_buffer, wl_surface};
use wayland_protocols::wp::linux_dmabuf::zv1::client::{zwp_linux_buffer_params_v1, zwp_linux_dmabuf_v1};

use super::card::{Card, Exported, Mapped, Scene};
use super::desk::Desk;
use super::paint::{Fault, Frame, PATIENCE, Painted, draw};

/// Two: one on the screen, one being drawn. A frame is only drawn once the
/// compositor has shown the last, so a third would never be used.
const IMAGES: usize = 2;

/// XRGB8888 as a fourcc, which is what the images are drawn in.
const XRGB8888: u32 = u32::from_le_bytes(*b"XR24");

/// What handing over needs from the Wayland side.
pub struct Wayland<'a> {
    pub dmabuf: &'a zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
    pub queue: &'a QueueHandle<Desk>,
    pub surface: &'a wl_surface::WlSurface,
}

/// One image, its buffer on the surface, and what draws it.
struct Out {
    frame: Frame,
    buffer: wl_buffer::WlBuffer,
    /// Whether the compositor still has it.
    held: Arc<AtomicBool>,
    _image: Exported,
}

pub struct Handed {
    card: Rc<Card>,
    surface: wl_surface::WlSurface,
    extent: vk::Extent2D,
    outs: Vec<Out>,
}

impl Handed {
    pub fn new(card: &Rc<Card>, (width, height): (u32, u32), wayland: &Wayland) -> Result<Handed, Fault> {
        let extent = vk::Extent2D { width, height };
        let mut handed = Handed { card: card.clone(), surface: wayland.surface.clone(), extent, outs: Vec::new() };
        for _ in 0..IMAGES {
            let mut image = card.exported(width, height)?;
            let frame = Frame::new(card, image.view, extent, card.render_pass_out)?;
            let fd = image.fd.take().ok_or("an image with no memory to hand over")?;
            let held = Arc::new(AtomicBool::new(false));
            let params = wayland.dmabuf.create_params(wayland.queue, ());
            params.add(fd.as_fd(), 0, image.offset as u32, image.stride as u32, 0, 0);
            let flags = zwp_linux_buffer_params_v1::Flags::empty();
            let buffer = params.create_immed(width as i32, height as i32, XRGB8888, flags, wayland.queue, held.clone());
            params.destroy();
            // The compositor has its own copy of the descriptor by now; this
            // one closes as it drops.
            drop(fd);
            handed.outs.push(Out { frame, buffer, held, _image: image });
        }
        Ok(handed)
    }

    /// Draws a frame into whichever image the compositor is not holding and
    /// the card is done with, and hands it over, calling `sending` just
    /// before, as a swapchain would.
    pub fn paint(
        &mut self,
        sending: impl FnOnce(),
        scene: Option<Scene>,
        fill: impl FnOnce(&Mapped),
        record: impl FnOnce(&Card, &Frame),
    ) -> Result<Painted, Fault> {
        let device = &self.card.device;
        // SAFETY: fences of this target's own.
        let free = self.outs.iter().position(|out| {
            !out.held.load(Ordering::Acquire) && unsafe { device.get_fence_status(out.frame.done) }.unwrap_or(false)
        });
        let Some(at) = free else { return Ok(Painted::NotNow) };
        let out = &mut self.outs[at];
        draw(&self.card, &mut out.frame, self.extent, self.card.render_pass_out, scene, fill, record, &[], &[])?;
        // The compositor reads what it is given at once, and nothing tells it
        // to wait for the card: so the card is waited for here. A fraction of
        // a millisecond, spent asleep.
        // SAFETY: the frame's own fence, just submitted.
        unsafe { device.wait_for_fences(&[out.frame.done], true, PATIENCE) }?;

        sending();
        self.surface.attach(Some(&out.buffer), 0, 0);
        self.surface.damage_buffer(0, 0, i32::MAX, i32::MAX);
        self.surface.commit();
        out.held.store(true, Ordering::Release);
        Ok(Painted::Shown)
    }

    /// Waits until the card is done with every image.
    pub fn idle(&self) {
        let fences: Vec<vk::Fence> = self.outs.iter().map(|out| out.frame.done).collect();
        // SAFETY: fences of this target's own.
        let _ = unsafe { self.card.device.wait_for_fences(&fences, true, PATIENCE) };
    }
}

impl Drop for Handed {
    fn drop(&mut self) {
        self.idle();
        for out in self.outs.drain(..) {
            out.buffer.destroy();
            out.frame.destroy(&self.card);
        }
    }
}
