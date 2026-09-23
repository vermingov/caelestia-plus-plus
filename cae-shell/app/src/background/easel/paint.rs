//! The background on the card: something to draw on for every screen, and a
//! frame put on it only when there is one to show.
//!
//! A frame of the helix is the dark behind it and its shapes over that, each
//! shape a quad; a frame of a picture is one triangle sampling two pictures
//! while one takes over from the other. Nothing is allocated for a frame:
//! every buffer, fence and semaphore belongs to an image and is used again
//! the next time that image comes round — and so are the helix's commands,
//! which are recorded once per image and say nothing that changes from one
//! frame to the next. The shapes and how many there are are read from the
//! image's buffer, which is all a frame writes.
//!
//! Frames reach the compositor handed over directly where it and the card
//! can do that (`direct`), and through a swapchain everywhere else.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::rc::Rc;

use ash::vk;

use super::card::{Card, Change, Mapped, Picture, Scene, Vulkan};
use super::direct::{Handed, Wayland};
use super::helix::{MOST, Shape};

/// How long a frame may wait for the card to finish with the last one drawn
/// on the same image. At sixty frames a second it has long since finished;
/// this is only there so that a card that has stopped answering stops the
/// background rather than hanging it.
pub const PATIENCE: u64 = 250_000_000;

/// Where the shapes start in a frame's buffer: after the draw's own count,
/// which the card reads from the front of it.
const SHAPES_AT: usize = 64;

/// What became of asking for a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Painted {
    /// On its way to the screen.
    Shown,
    /// Not drawn: no image was free, or the swapchain has to be made again.
    /// Nothing was sent to the compositor either.
    NotNow,
}

pub struct Painter {
    vulkan: Rc<Vulkan>,
    card: Option<Rc<Card>>,
    /// What a first picture takes over from.
    black: Option<Rc<Picture>>,
    refused: bool,
}

impl Painter {
    pub fn new() -> Result<Painter, String> {
        Ok(Painter { vulkan: Vulkan::new()?, card: None, black: None, refused: false })
    }

    /// Something to draw on, for one Wayland surface.
    ///
    /// # Safety
    /// Both pointers must be what they are called, and both must outlive the
    /// canvas: it is to be dropped before the surface is destroyed.
    pub unsafe fn canvas(&self, display: NonNull<c_void>, surface: NonNull<c_void>) -> Result<Canvas, String> {
        // SAFETY: the caller's promise is the one this asks for.
        let surface = unsafe { self.vulkan.surface(display, surface) }?;
        Ok(Canvas { vulkan: self.vulkan.clone(), surface, size: (0, 0), target: None, hung: None })
    }

    /// Whether the card has refused something, which is the end of the
    /// background: there is nothing here that can be asked another way.
    pub fn refused(&self) -> bool {
        self.refused
    }

    /// Makes `canvas` that many pixels. The first one sized is what the card
    /// is chosen by. A picture hung on it was cut for the size it had, and
    /// goes. Handed over directly where `wayland` says the compositor takes
    /// that and the card can give it; through a swapchain otherwise.
    pub fn size(&mut self, canvas: &mut Canvas, (width, height): (u32, u32), wayland: Option<Wayland>) -> Result<(), String> {
        let card = match &self.card {
            Some(card) => card.clone(),
            None => self.card.insert(Card::for_surface(&self.vulkan, canvas.surface)?).clone(),
        };
        canvas.size = (width.max(1), height.max(1));
        canvas.settled();
        canvas.hung = None;
        let old = canvas.target.take();

        let direct = wayland.filter(|_| card.exports.is_some() && card.format.format == super::card::OUT_FORMAT);
        if let Some(wayland) = direct {
            drop(old);
            match Handed::new(&card, canvas.size, &wayland) {
                Ok(handed) => {
                    log::info!("background: {}x{}, handed to the compositor directly", canvas.size.0, canvas.size.1);
                    canvas.target = Some(Target::Handed(handed));
                    return Ok(());
                }
                Err(fault) => eprintln!("cae: the background: {}, so it goes through a swapchain", fault.said),
            }
            let made = Chain::new(&card, canvas.surface, canvas.size, vk::SwapchainKHR::null());
            canvas.target = Some(Target::Chain(made.map_err(|error| self.fault(error))?));
            return Ok(());
        }
        let retiring = match &old {
            Some(Target::Chain(old)) => old.swapchain,
            _ => vk::SwapchainKHR::null(),
        };
        let made = Chain::new(&card, canvas.surface, canvas.size, retiring);
        drop(old);
        canvas.target = Some(Target::Chain(made.map_err(|error| self.fault(error))?));
        log::info!("background: {}x{}, through a swapchain", canvas.size.0, canvas.size.1);
        Ok(())
    }

    /// Draws `shapes` over the dark that `scene` describes. `sending` is
    /// called just before the frame goes to the compositor, which is the
    /// moment to ask it for the next one.
    pub fn paint_helix<'a>(
        &mut self,
        canvas: &mut Canvas,
        scene: &Scene,
        shapes: impl ExactSizeIterator<Item = &'a Shape>,
        sending: impl FnOnce(),
    ) -> Painted {
        let Some(target) = &mut canvas.target else { return Painted::NotNow };
        let fill = |written: &Mapped| {
            let count = shapes.len().min(MOST);
            for (at, shape) in shapes.take(count).enumerate() {
                written.write(SHAPES_AT + at * size_of::<Shape>(), bytemuck::bytes_of(shape));
            }
            // A draw's count as the card reads it: four corners, `count`
            // shapes, from the first of each.
            written.write(0, bytemuck::bytes_of(&[4, count as u32, 0, 0]));
        };
        let record = |card: &Card, frame: &Frame| {
            let commands = frame.commands;
            // SAFETY: commands being recorded on this card, inside its render
            // pass, with a buffer that lives as long as the frame.
            unsafe {
                card.device.cmd_bind_pipeline(commands, vk::PipelineBindPoint::GRAPHICS, card.backdrop);
                let stages = vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT;
                card.device.cmd_push_constants(commands, card.scene_layout, stages, 0, bytemuck::bytes_of(scene));
                card.device.cmd_draw(commands, 3, 1, 0, 0);
                card.device.cmd_bind_pipeline(commands, vk::PipelineBindPoint::GRAPHICS, card.shapes);
                card.device.cmd_bind_vertex_buffers(commands, 0, &[frame.shapes.buffer], &[SHAPES_AT as u64]);
                card.device.cmd_draw_indirect(commands, frame.shapes.buffer, 0, 1, 0);
            }
        };
        let painted = target.paint(sending, Some(*scene), fill, record);
        painted.unwrap_or_else(|error| {
            self.fault(error);
            Painted::NotNow
        })
    }

    /// Hangs `pixels` on `canvas`, which they were cut to the size of. What
    /// hung there before stays beside it until `settle`.
    pub fn hang(&mut self, canvas: &mut Canvas, pixels: &image::RgbaImage) {
        let Some(card) = self.card.clone() else { return };
        let black = match self.black() {
            Ok(black) => black,
            Err(error) => return drop(self.fault(error)),
        };
        let picture = match card.picture(pixels) {
            Ok(picture) => Rc::new(picture),
            Err(error) => return drop(self.fault(error)),
        };
        canvas.settled();
        let before = canvas.hung.take().map_or(black, |hung| hung.picture.clone());
        match card.bind_pictures(before.view, picture.view) {
            Ok(set) => canvas.hung = Some(Hung { card, set, before: Some(before), picture }),
            Err(error) => drop(self.fault(error)),
        }
    }

    /// Lets go of the picture the one on `canvas` took over from, once the
    /// change has been shown to its end.
    pub fn settle(&mut self, canvas: &mut Canvas) {
        let Ok(black) = self.black() else { return };
        canvas.settled();
        if let Some(hung) = &mut canvas.hung {
            hung.card.rebind(hung.set, black.view, hung.picture.view);
            hung.before = None;
        }
    }

    /// Draws the picture hung on `canvas`, `done` of the way from the one
    /// before it.
    pub fn paint_picture(&mut self, canvas: &mut Canvas, done: f32, sending: impl FnOnce()) -> Painted {
        let (Some(target), Some(hung)) = (&mut canvas.target, &canvas.hung) else { return Painted::NotNow };
        let set = hung.set;
        let painted = target.paint(sending, None, |_| {}, |card, frame| {
            let commands = frame.commands;
            // SAFETY: as for the helix, with a set whose pictures are alive.
            unsafe {
                card.device.cmd_bind_pipeline(commands, vk::PipelineBindPoint::GRAPHICS, card.picture);
                card.device.cmd_bind_descriptor_sets(commands, vk::PipelineBindPoint::GRAPHICS, card.picture_layout, 0, &[set], &[]);
                card.device.cmd_push_constants(commands, card.picture_layout, vk::ShaderStageFlags::FRAGMENT, 0, bytemuck::bytes_of(&Change { done }));
                card.device.cmd_draw(commands, 3, 1, 0, 0);
            }
        });
        painted.unwrap_or_else(|error| {
            self.fault(error);
            Painted::NotNow
        })
    }

    /// Whether the card wants the shaders to hand it linear light.
    pub fn encodes(&self) -> bool {
        self.card.as_ref().is_some_and(|card| card.encodes)
    }

    fn black(&mut self) -> Result<Rc<Picture>, String> {
        if let Some(black) = &self.black {
            return Ok(black.clone());
        }
        let card = self.card.as_ref().ok_or("no card yet")?;
        let black = Rc::new(card.picture(&image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 255])))?);
        Ok(self.black.insert(black).clone())
    }

    /// Says what went wrong, and remembers a card that has gone for good.
    fn fault(&mut self, error: impl Into<Fault>) -> String {
        let error = error.into();
        if error.lost {
            self.refused = true;
        }
        eprintln!("cae: the background: {}", error.said);
        error.said
    }
}

/// Something the card said no to, and whether it can ever say yes again.
pub struct Fault {
    pub said: String,
    lost: bool,
}

impl From<vk::Result> for Fault {
    fn from(error: vk::Result) -> Fault {
        Fault { said: error.to_string(), lost: matches!(error, vk::Result::ERROR_DEVICE_LOST | vk::Result::ERROR_SURFACE_LOST_KHR) }
    }
}

impl From<String> for Fault {
    fn from(said: String) -> Fault {
        Fault { said, lost: false }
    }
}

impl From<&str> for Fault {
    fn from(said: &str) -> Fault {
        Fault { said: said.to_string(), lost: false }
    }
}

/// One screen's surface on the card, and what it is drawn on once it has a
/// size.
pub struct Canvas {
    vulkan: Rc<Vulkan>,
    surface: vk::SurfaceKHR,
    size: (u32, u32),
    target: Option<Target>,
    hung: Option<Hung>,
}

impl Canvas {
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Waits for every frame of this canvas to be done with what it reads,
    /// before a picture it reads is let go of or rebound.
    fn settled(&self) {
        match &self.target {
            Some(Target::Chain(chain)) => chain.idle(),
            Some(Target::Handed(handed)) => handed.idle(),
            None => {}
        }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        // Nothing may be let go of while a frame still reads it, and what is
        // drawn on goes before the surface it was made for.
        self.settled();
        self.hung = None;
        self.target = None;
        // SAFETY: nothing made on the surface is left.
        unsafe { self.vulkan.surfaces.destroy_surface(self.surface, None) };
    }
}

/// A picture as a canvas shows it, bound beside the one it took over from.
struct Hung {
    card: Rc<Card>,
    set: vk::DescriptorSet,
    before: Option<Rc<Picture>>,
    picture: Rc<Picture>,
}

impl Drop for Hung {
    fn drop(&mut self) {
        self.card.free_set(self.set);
    }
}

/// How a canvas's frames reach the compositor.
enum Target {
    Chain(Chain),
    Handed(Handed),
}

impl Target {
    fn paint(&mut self, sending: impl FnOnce(), scene: Option<Scene>, fill: impl FnOnce(&Mapped), record: impl FnOnce(&Card, &Frame)) -> Result<Painted, Fault> {
        match self {
            Target::Chain(chain) => chain.paint(sending, scene, fill, record),
            Target::Handed(handed) => handed.paint(sending, scene, fill, record),
        }
    }
}

/// What one image is drawn with: its framebuffer and commands, the fence
/// that says the card is done with them, and the buffer a frame writes.
pub struct Frame {
    framebuffer: vk::Framebuffer,
    pub commands: vk::CommandBuffer,
    /// The helix scene `commands` hold a recording of, if they do. Anything
    /// else in them is recorded again for each frame.
    recorded: Option<Scene>,
    pub done: vk::Fence,
    /// Signalled when the frame is drawn, for a swapchain's present to wait
    /// on. Handing over waits on `done` instead.
    drawn: vk::Semaphore,
    pub shapes: Mapped,
}

impl Frame {
    pub fn new(card: &Rc<Card>, view: vk::ImageView, extent: vk::Extent2D, render_pass: vk::RenderPass) -> Result<Frame, Fault> {
        let room = (SHAPES_AT + MOST * size_of::<Shape>()) as u64;
        let shapes = card.mapped(room, vk::BufferUsageFlags::VERTEX_BUFFER | vk::BufferUsageFlags::INDIRECT_BUFFER)?;
        let mut frame = Frame {
            framebuffer: vk::Framebuffer::null(),
            commands: vk::CommandBuffer::null(),
            recorded: None,
            done: vk::Fence::null(),
            drawn: vk::Semaphore::null(),
            shapes,
        };
        let allocate = vk::CommandBufferAllocateInfo::default().command_pool(card.commands).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(1);
        let attachments = [view];
        let framebuffer = vk::FramebufferCreateInfo::default()
            .render_pass(render_pass)
            .attachments(&attachments)
            .width(extent.width)
            .height(extent.height)
            .layers(1);
        // SAFETY: a live device; whatever is made belongs to the frame, and a
        // frame made half way is destroyed here, null handles and all.
        let made = unsafe {
            (|| -> Result<(), vk::Result> {
                frame.commands = card.device.allocate_command_buffers(&allocate)?[0];
                frame.done = card.device.create_fence(&vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED), None)?;
                frame.drawn = card.device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None)?;
                frame.framebuffer = card.device.create_framebuffer(&framebuffer, None)?;
                Ok(())
            })()
        };
        match made {
            Ok(()) => Ok(frame),
            Err(error) => {
                frame.destroy(card);
                Err(error.into())
            }
        }
    }

    /// Destroys what the frame made. The card must be done with it.
    pub fn destroy(self, card: &Card) {
        // SAFETY: nothing in flight uses any of it; destroying a null handle
        // does nothing.
        unsafe {
            card.device.destroy_framebuffer(self.framebuffer, None);
            card.device.destroy_fence(self.done, None);
            card.device.destroy_semaphore(self.drawn, None);
            if self.commands != vk::CommandBuffer::null() {
                card.device.free_command_buffers(card.commands, &[self.commands]);
            }
        }
    }
}

/// Draws one frame into `frame` and submits it: waits for the card to be
/// done with the frame's last use, has `fill` write what changes, records
/// with `record` inside `render_pass` over the whole image — once, where
/// `scene` says a recording for it can be kept — and submits, waiting on
/// `waits` and signalling `signals` and the frame's fence.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    card: &Card,
    frame: &mut Frame,
    extent: vk::Extent2D,
    render_pass: vk::RenderPass,
    scene: Option<Scene>,
    fill: impl FnOnce(&Mapped),
    record: impl FnOnce(&Card, &Frame),
    waits: &[vk::Semaphore],
    signals: &[vk::Semaphore],
) -> Result<(), Fault> {
    let device = &card.device;
    // SAFETY: the frame's own fence, buffer, command buffer and framebuffer;
    // the wait means the card is done with all of them before they are
    // touched again.
    unsafe {
        device.wait_for_fences(&[frame.done], true, PATIENCE)?;
        device.reset_fences(&[frame.done])?;
        fill(&frame.shapes);
        if scene.is_none() || frame.recorded != scene {
            frame.recorded = None;
            device.reset_command_buffer(frame.commands, vk::CommandBufferResetFlags::empty())?;
            device.begin_command_buffer(frame.commands, &vk::CommandBufferBeginInfo::default())?;
            let whole = vk::Rect2D { offset: vk::Offset2D::default(), extent };
            let pass = vk::RenderPassBeginInfo::default().render_pass(render_pass).framebuffer(frame.framebuffer).render_area(whole);
            device.cmd_begin_render_pass(frame.commands, &pass, vk::SubpassContents::INLINE);
            let viewport = vk::Viewport { x: 0., y: 0., width: extent.width as f32, height: extent.height as f32, min_depth: 0., max_depth: 1. };
            device.cmd_set_viewport(frame.commands, 0, &[viewport]);
            device.cmd_set_scissor(frame.commands, 0, &[whole]);
            record(card, frame);
            device.cmd_end_render_pass(frame.commands);
            device.end_command_buffer(frame.commands)?;
            frame.recorded = scene;
        }
        let stages = vec![vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT; waits.len()];
        let commands = [frame.commands];
        let submit = vk::SubmitInfo::default().wait_semaphores(waits).wait_dst_stage_mask(&stages).command_buffers(&commands).signal_semaphores(signals);
        device.queue_submit(card.queue, &[submit], frame.done)?;
    }
    Ok(())
}

/// The swapchain, and a frame for each of its images.
struct Chain {
    card: Rc<Card>,
    swapchain: vk::SwapchainKHR,
    extent: vk::Extent2D,
    views: Vec<vk::ImageView>,
    frames: Vec<Frame>,
    /// What the next image is acquired with: one more than there are
    /// images, and each remembers which image it last went to, so it is not
    /// used again before that image's frame has waited on it.
    acquiring: Vec<(vk::Semaphore, Option<usize>)>,
    next: usize,
}

impl Chain {
    fn new(card: &Rc<Card>, surface: vk::SurfaceKHR, (width, height): (u32, u32), old: vk::SwapchainKHR) -> Result<Chain, Fault> {
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
        let alpha = [vk::CompositeAlphaFlagsKHR::OPAQUE, vk::CompositeAlphaFlagsKHR::INHERIT, vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED]
            .into_iter()
            .find(|alpha| offered.supported_composite_alpha.contains(*alpha))
            .unwrap_or(vk::CompositeAlphaFlagsKHR::OPAQUE);
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
        let mut chain = Chain { card: card.clone(), swapchain, extent, views: Vec::new(), frames: Vec::new(), acquiring: Vec::new(), next: 0 };
        // SAFETY: the swapchain just made.
        let images = unsafe { card.swapchains.get_swapchain_images(swapchain) }?;
        let whole = vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(1).layer_count(1);
        for image in images {
            let view = vk::ImageViewCreateInfo::default().image(image).view_type(vk::ImageViewType::TYPE_2D).format(card.format.format).subresource_range(whole);
            // SAFETY: an image of the chain's.
            let view = unsafe { card.device.create_image_view(&view, None) }?;
            chain.views.push(view);
            let frame = Frame::new(card, view, extent, card.render_pass)?;
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
    fn paint(&mut self, sending: impl FnOnce(), scene: Option<Scene>, fill: impl FnOnce(&Mapped), record: impl FnOnce(&Card, &Frame)) -> Result<Painted, Fault> {
        let slot = self.next;
        let (acquired, last_for) = self.acquiring[slot];
        if let Some(image) = last_for {
            // SAFETY: a fence of this chain's.
            unsafe { self.card.device.wait_for_fences(&[self.frames[image].done], true, PATIENCE) }?;
        }
        // An image that is not free yet is a frame for later.
        // SAFETY: a live swapchain, and a semaphore nothing is waiting on.
        let image = match unsafe { self.card.swapchains.acquire_next_image(self.swapchain, 0, acquired, vk::Fence::null()) } {
            Ok((image, _)) => image as usize,
            Err(vk::Result::NOT_READY | vk::Result::TIMEOUT | vk::Result::ERROR_OUT_OF_DATE_KHR) => return Ok(Painted::NotNow),
            Err(error) => return Err(error.into()),
        };
        self.next = (slot + 1) % self.acquiring.len();
        self.acquiring[slot].1 = Some(image);

        let frame = &mut self.frames[image];
        let drawn = frame.drawn;
        draw(&self.card, frame, self.extent, self.card.render_pass, scene, fill, record, &[acquired], &[drawn])?;

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

    /// Waits until the card is done with every frame of this chain.
    fn idle(&self) {
        let fences: Vec<vk::Fence> = self.frames.iter().map(|frame| frame.done).collect();
        if fences.is_empty() {
            return;
        }
        // SAFETY: fences of this chain's.
        let _ = unsafe { self.card.device.wait_for_fences(&fences, true, PATIENCE) };
    }
}

impl Drop for Chain {
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
