//! Frames presented through a swapchain: the portable way, and the way
//! wherever the compositor or the card cannot take them directly.

use std::rc::Rc;

use ash::vk;

use super::frame::{Fault, Frame, PATIENCE, Painted, draw};
use super::{Card, Mapped, Spec};

/// How long a frame waits for the compositor to let go of an image, in
/// nanoseconds: about a frame at sixty a second.
///
/// Not nought. A compositor that says it has let go through the kernel — a
/// timeline it signals, which is how Hyprland and NVIDIA's driver do it —
/// says nothing on the Wayland connection when it does, so a drawer that
/// gave up at once and slept until it heard something slept until the end:
/// the cinema showed two frames of its opening dark and nothing more.
const FREE_WITHIN: u64 = 17_000_000;

/// The swapchain, and a frame for each of its images.
pub struct Chain<K> {
    card: Rc<Card>,
    pub swapchain: vk::SwapchainKHR,
    extent: vk::Extent2D,
    pass: vk::RenderPass,
    views: Vec<vk::ImageView>,
    frames: Vec<Frame<K>>,
    /// What the next image is acquired with: one more than there are
    /// images, and each remembers which image it last went to, so it is not
    /// used again before that image's frame has waited on it.
    acquiring: Vec<(vk::Semaphore, Option<usize>)>,
    next: usize,
}

impl<K: Copy + PartialEq> Chain<K> {
    pub fn new(card: &Rc<Card>, surface: vk::SurfaceKHR, (width, height): (u32, u32), old: vk::SwapchainKHR, spec: &Spec) -> Result<Chain<K>, Fault> {
        let surfaces = &card.vulkan.surfaces;
        // SAFETY: a live surface on the card's physical device.
        let offered = unsafe { surfaces.get_physical_device_surface_capabilities(card.physical, surface) }?;
        let extent = if offered.current_extent.width != u32::MAX {
            offered.current_extent
        } else {
            vk::Extent2D {
                width: width.clamp(offered.min_image_extent.width, offered.max_image_extent.width),
                height: height.clamp(offered.min_image_extent.height, offered.max_image_extent.height),
            }
        };
        // As few images as the surface allows: a frame is only ever drawn
        // once the compositor has shown the last, so there is no queue of
        // them to keep, and each image of a screen-sized chain is megabytes.
        let mut count = offered.min_image_count.max(2);
        if offered.max_image_count > 0 {
            count = count.min(offered.max_image_count);
        }
        // What is seen through is premultiplied, which is how everything
        // here is drawn; what is not, the compositor may take as opaque.
        let preferred = if spec.see_through {
            [vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED, vk::CompositeAlphaFlagsKHR::INHERIT, vk::CompositeAlphaFlagsKHR::OPAQUE]
        } else {
            [vk::CompositeAlphaFlagsKHR::OPAQUE, vk::CompositeAlphaFlagsKHR::INHERIT, vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED]
        };
        let alpha = preferred.into_iter().find(|alpha| offered.supported_composite_alpha.contains(*alpha)).unwrap_or(vk::CompositeAlphaFlagsKHR::OPAQUE);
        let asked = vk::SwapchainCreateInfoKHR::default()
            .surface(surface)
            .min_image_count(count)
            .image_format(card.format.format)
            .image_color_space(card.format.color_space)
            .image_extent(extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(offered.current_transform)
            .composite_alpha(alpha)
            // The one mode every surface has. Nothing is ever waited on by
            // it here: a frame is drawn after the compositor has said it is
            // ready for one.
            .present_mode(vk::PresentModeKHR::FIFO)
            .clipped(true)
            .old_swapchain(old);
        // SAFETY: a well-formed create info for a live surface; the chain
        // made owns what is made and destroys it in `Drop`, whatever fails
        // part of the way.
        let swapchain = unsafe { card.swapchains.create_swapchain(&asked, None) }?;
        let pass = spec.passes.presented;
        let mut chain = Chain { card: card.clone(), swapchain, extent, pass, views: Vec::new(), frames: Vec::new(), acquiring: Vec::new(), next: 0 };
        // SAFETY: the swapchain just made.
        let images = unsafe { card.swapchains.get_swapchain_images(swapchain) }?;
        let whole = vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(1).layer_count(1);
        for image in images {
            let view = vk::ImageViewCreateInfo::default().image(image).view_type(vk::ImageViewType::TYPE_2D).format(card.format.format).subresource_range(whole);
            // SAFETY: an image of the chain's.
            let view = unsafe { card.device.create_image_view(&view, None) }?;
            chain.views.push(view);
            let frame = Frame::new(card, view, extent, pass, spec.room)?;
            chain.frames.push(frame);
        }
        for _ in 0..=chain.frames.len() {
            // SAFETY: a live device.
            let semaphore = unsafe { card.device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None) }?;
            chain.acquiring.push((semaphore, None));
        }
        Ok(chain)
    }

    /// Acquires an image without waiting for one, draws on it, and presents
    /// it, calling `sending` just before.
    pub fn paint(&mut self, sending: impl FnOnce(), key: Option<K>, fill: impl FnOnce(&Mapped), record: impl FnOnce(&Card, &Frame<K>)) -> Result<Painted, Fault> {
        let slot = self.next;
        let (acquired, last_for) = self.acquiring[slot];
        if let Some(image) = last_for {
            // SAFETY: a fence of this chain's.
            unsafe { self.card.device.wait_for_fences(&[self.frames[image].done], true, PATIENCE) }?;
        }
        // An image that is not free yet is waited for a little, and after
        // that is a frame for later.
        // SAFETY: a live swapchain, and a semaphore nothing is waiting on.
        let image = match unsafe { self.card.swapchains.acquire_next_image(self.swapchain, FREE_WITHIN, acquired, vk::Fence::null()) } {
            Ok((image, _)) => image as usize,
            Err(vk::Result::NOT_READY | vk::Result::TIMEOUT | vk::Result::ERROR_OUT_OF_DATE_KHR) => return Ok(Painted::NotNow),
            Err(error) => return Err(error.into()),
        };
        self.next = (slot + 1) % self.acquiring.len();
        self.acquiring[slot].1 = Some(image);

        let frame = &mut self.frames[image];
        let drawn = frame.drawn;
        draw(&self.card, frame, self.extent, self.pass, key, fill, record, &[acquired], &[drawn])?;

        sending();
        let (waits, swapchains, images) = ([drawn], [self.swapchain], [image as u32]);
        let present = vk::PresentInfoKHR::default().wait_semaphores(&waits).swapchains(&swapchains).image_indices(&images);
        // SAFETY: an image of this chain's, drawn and signalled above.
        match unsafe { self.card.swapchains.queue_present(self.card.queue, &present) } {
            Ok(_) => Ok(Painted::Shown),
            // Resized under it: the configure that says so makes a new one.
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => Ok(Painted::NotNow),
            Err(error) => Err(error.into()),
        }
    }
}

impl<K> Chain<K> {
    /// Waits until the card is done with every frame of this chain.
    pub fn idle(&self) {
        let fences: Vec<vk::Fence> = self.frames.iter().map(|frame| frame.done).collect();
        if fences.is_empty() {
            return;
        }
        // SAFETY: fences of this chain's.
        let _ = unsafe { self.card.device.wait_for_fences(&fences, true, PATIENCE) };
    }
}

impl<K> Drop for Chain<K> {
    fn drop(&mut self) {
        self.idle();
        for frame in self.frames.drain(..) {
            frame.destroy(&self.card);
        }
        // SAFETY: nothing in flight uses any of it any more.
        unsafe {
            for view in self.views.drain(..) {
                self.card.device.destroy_image_view(view, None);
            }
            for (semaphore, _) in self.acquiring.drain(..) {
                self.card.device.destroy_semaphore(semaphore, None);
            }
            self.card.swapchains.destroy_swapchain(self.swapchain, None);
        }
    }
}
