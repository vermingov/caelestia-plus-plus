//! One image's worth of drawing: what it is drawn with, and the drawing.

use std::rc::Rc;
use std::time::Duration;

use ash::vk;

use super::{Card, Mapped};

/// How long a frame may wait for the card to finish with the last one drawn
/// on the same image. At sixty frames a second it has long since finished;
/// this is only there so that a card that has stopped answering stops the
/// drawer rather than hanging it.
pub const PATIENCE: u64 = 250_000_000;

/// How soon a drawer tries again after `Painted::NotNow` when no frame has
/// been asked for. The compositor may let go of an image without a word on
/// the Wayland connection — through a timeline in the kernel, as Hyprland
/// does with NVIDIA's driver — so a drawer that waited to hear from it
/// could wait for good.
pub const LOOK_AGAIN: Duration = Duration::from_millis(8);

/// What became of asking for a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Painted {
    /// On its way to the screen.
    Shown,
    /// Not drawn: no image was free, or the swapchain has to be made again.
    /// Nothing was sent to the compositor either: see `LOOK_AGAIN`.
    NotNow,
}

/// Something the card said no to, and whether it can ever say yes again.
pub struct Fault {
    pub said: String,
    pub lost: bool,
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

/// What one image is drawn with: its framebuffer and commands, the fence
/// that says the card is done with them, and the buffer a frame writes.
///
/// `K` is what a recording of the commands can be kept for: a drawer whose
/// commands say nothing that changes from frame to frame records them once
/// per image and gives the same key each time; one that gives none records
/// every frame.
pub struct Frame<K> {
    framebuffer: vk::Framebuffer,
    pub commands: vk::CommandBuffer,
    recorded: Option<K>,
    pub done: vk::Fence,
    /// Signalled when the frame is drawn, for a swapchain's present to wait
    /// on. Handing over waits on `done` instead.
    pub drawn: vk::Semaphore,
    /// What a frame writes for the card to read.
    pub data: Mapped,
}

impl<K: Copy + PartialEq> Frame<K> {
    pub fn new(card: &Rc<Card>, view: vk::ImageView, extent: vk::Extent2D, pass: vk::RenderPass, room: u64) -> Result<Frame<K>, Fault> {
        let data = card.mapped(room, vk::BufferUsageFlags::VERTEX_BUFFER | vk::BufferUsageFlags::INDIRECT_BUFFER)?;
        let mut frame = Frame {
            framebuffer: vk::Framebuffer::null(),
            commands: vk::CommandBuffer::null(),
            recorded: None,
            done: vk::Fence::null(),
            drawn: vk::Semaphore::null(),
            data,
        };
        let allocate = vk::CommandBufferAllocateInfo::default().command_pool(card.commands).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(1);
        let attachments = [view];
        let framebuffer = vk::FramebufferCreateInfo::default()
            .render_pass(pass)
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
}

impl<K> Frame<K> {
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
/// with `record` inside `pass` over the whole image — once, where `key` says
/// a recording for it can be kept — and submits, waiting on `waits` and
/// signalling `signals` and the frame's fence. A pass that clears, clears to
/// nothing at all.
#[allow(clippy::too_many_arguments)]
pub fn draw<K: Copy + PartialEq>(
    card: &Card,
    frame: &mut Frame<K>,
    extent: vk::Extent2D,
    pass: vk::RenderPass,
    key: Option<K>,
    fill: impl FnOnce(&Mapped),
    record: impl FnOnce(&Card, &Frame<K>),
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
        fill(&frame.data);
        if key.is_none() || frame.recorded != key {
            frame.recorded = None;
            device.reset_command_buffer(frame.commands, vk::CommandBufferResetFlags::empty())?;
            device.begin_command_buffer(frame.commands, &vk::CommandBufferBeginInfo::default())?;
            let whole = vk::Rect2D { offset: vk::Offset2D::default(), extent };
            let nothing = [vk::ClearValue { color: vk::ClearColorValue { float32: [0.; 4] } }];
            let begun = vk::RenderPassBeginInfo::default().render_pass(pass).framebuffer(frame.framebuffer).render_area(whole).clear_values(&nothing);
            device.cmd_begin_render_pass(frame.commands, &begun, vk::SubpassContents::INLINE);
            let viewport = vk::Viewport { x: 0., y: 0., width: extent.width as f32, height: extent.height as f32, min_depth: 0., max_depth: 1. };
            device.cmd_set_viewport(frame.commands, 0, &[viewport]);
            device.cmd_set_scissor(frame.commands, 0, &[whole]);
            record(card, frame);
            device.cmd_end_render_pass(frame.commands);
            device.end_command_buffer(frame.commands)?;
            frame.recorded = key;
        }
        let stages = vec![vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT; waits.len()];
        let commands = [frame.commands];
        let submit = vk::SubmitInfo::default().wait_semaphores(waits).wait_dst_stage_mask(&stages).command_buffers(&commands).signal_semaphores(signals);
        device.queue_submit(card.queue, &[submit], frame.done)?;
    }
    Ok(())
}
