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

use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use ash::vk;
use wayland_client::Dispatch;
use wayland_client::protocol::wl_buffer;
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_buffer_params_v1;

use super::helix::{MOST, Shape};
use super::kit::{Change, Kit, SHAPES_AT, Scene};
use crate::card::{self, Card, Fault, Mapped, Painted, Picture, Vulkan, Wayland};

pub struct Painter {
    vulkan: Rc<Vulkan>,
    card: Option<Rc<Card>>,
    kit: Option<Rc<Kit>>,
    /// What a first picture takes over from.
    black: Option<Rc<Picture>>,
    refused: bool,
}

impl Painter {
    pub fn new() -> Result<Painter, String> {
        Ok(Painter { vulkan: Vulkan::new()?, card: None, kit: None, black: None, refused: false })
    }

    /// Something to draw on, for one Wayland surface.
    ///
    /// # Safety
    /// Both pointers must be what they are called, and both must outlive the
    /// canvas: it is to be dropped before the surface is destroyed.
    pub unsafe fn canvas(&self, display: NonNull<c_void>, surface: NonNull<c_void>) -> Result<Canvas, String> {
        // SAFETY: the caller's promise is the one this asks for.
        let drawn = unsafe { card::Canvas::new(&self.vulkan, display, surface) }?;
        Ok(Canvas { drawn, hung: None, hung_from: None })
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
    pub fn size<D>(&mut self, canvas: &mut Canvas, pixels: (u32, u32), wayland: Option<Wayland<D>>) -> Result<(), String>
    where
        D: Dispatch<zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1, ()> + Dispatch<wl_buffer::WlBuffer, Arc<AtomicBool>> + 'static,
    {
        let card = match &self.card {
            Some(card) => card.clone(),
            None => self.card.insert(Card::for_surface(&self.vulkan, canvas.drawn.surface)?).clone(),
        };
        let kit = match &self.kit {
            Some(kit) => kit.clone(),
            None => self.kit.insert(Rc::new(Kit::new(&card)?)).clone(),
        };
        canvas.drawn.idle();
        (canvas.hung, canvas.hung_from) = (None, None);
        canvas.drawn.fit(&card, pixels, wayland, &kit.spec).map_err(|fault| self.fault(fault))
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
        let Some(kit) = self.kit.clone() else { return Painted::NotNow };
        let fill = |written: &Mapped| {
            let count = shapes.len().min(MOST);
            for (at, shape) in shapes.take(count).enumerate() {
                written.write(SHAPES_AT + at * size_of::<Shape>(), bytemuck::bytes_of(shape));
            }
            // A draw's count as the card reads it: four corners, `count`
            // shapes, from the first of each.
            written.write(0, bytemuck::bytes_of(&[4, count as u32, 0, 0]));
        };
        let record = |card: &Card, frame: &card::Frame<Scene>| {
            let commands = frame.commands;
            // SAFETY: commands being recorded on this card, inside its render
            // pass, with a buffer that lives as long as the frame.
            unsafe {
                card.device.cmd_bind_pipeline(commands, vk::PipelineBindPoint::GRAPHICS, kit.backdrop);
                let stages = vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT;
                card.device.cmd_push_constants(commands, kit.scene_layout, stages, 0, bytemuck::bytes_of(scene));
                card.device.cmd_draw(commands, 3, 1, 0, 0);
                card.device.cmd_bind_pipeline(commands, vk::PipelineBindPoint::GRAPHICS, kit.shapes);
                card.device.cmd_bind_vertex_buffers(commands, 0, &[frame.data.buffer], &[SHAPES_AT as u64]);
                card.device.cmd_draw_indirect(commands, frame.data.buffer, 0, 1, 0);
            }
        };
        let painted = canvas.drawn.paint(sending, Some(*scene), fill, record);
        painted.unwrap_or_else(|error| {
            self.fault(error);
            Painted::NotNow
        })
    }

    /// Hangs `pixels` on `canvas`, which they were cut to the size of. What
    /// hung there before stays beside it until `settle`.
    pub fn hang(&mut self, canvas: &mut Canvas, pixels: &image::RgbaImage) {
        let (Some(card), Some(kit)) = (self.card.clone(), self.kit.clone()) else { return };
        let black = match self.black() {
            Ok(black) => black,
            Err(error) => return drop(self.fault(error)),
        };
        let picture = match card.picture(pixels) {
            Ok(picture) => Rc::new(picture),
            Err(error) => return drop(self.fault(error)),
        };
        canvas.drawn.idle();
        let before = canvas.hung.take().map_or(black, |hung| hung.picture.clone());
        match kit.bind_pictures(before.view, picture.view) {
            Ok(set) => canvas.hung = Some(Hung { kit, set, before: Some(before), picture }),
            Err(error) => drop(self.fault(error)),
        }
    }

    /// Lets go of the picture the one on `canvas` took over from, once the
    /// change has been shown to its end.
    pub fn settle(&mut self, canvas: &mut Canvas) {
        let Ok(black) = self.black() else { return };
        canvas.drawn.idle();
        if let Some(hung) = &mut canvas.hung {
            hung.kit.rebind(hung.set, black.view, hung.picture.view);
            hung.before = None;
        }
    }

    /// Draws the picture hung on `canvas`, `done` of the way from the one
    /// before it.
    pub fn paint_picture(&mut self, canvas: &mut Canvas, done: f32, sending: impl FnOnce()) -> Painted {
        let Some(hung) = &canvas.hung else { return Painted::NotNow };
        let (kit, set) = (hung.kit.clone(), hung.set);
        let painted = canvas.drawn.paint(sending, None, |_| {}, |card, frame| {
            let commands = frame.commands;
            // SAFETY: as for the helix, with a set whose pictures are alive.
            unsafe {
                card.device.cmd_bind_pipeline(commands, vk::PipelineBindPoint::GRAPHICS, kit.picture);
                card.device.cmd_bind_descriptor_sets(commands, vk::PipelineBindPoint::GRAPHICS, kit.picture_layout, 0, &[set], &[]);
                card.device.cmd_push_constants(commands, kit.picture_layout, vk::ShaderStageFlags::FRAGMENT, 0, bytemuck::bytes_of(&Change { done }));
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

/// One screen's surface on the card, and the picture hung on it.
pub struct Canvas {
    /// First, so that it goes first: its frames are waited for before what
    /// they read is let go of.
    drawn: card::Canvas<Scene>,
    hung: Option<Hung>,
    /// The picture that has been hung on it, or that could not be.
    pub hung_from: Option<PathBuf>,
}

impl Canvas {
    pub fn size(&self) -> (u32, u32) {
        self.drawn.size()
    }
}

/// A picture as a canvas shows it, bound beside the one it took over from.
struct Hung {
    kit: Rc<Kit>,
    set: vk::DescriptorSet,
    before: Option<Rc<Picture>>,
    picture: Rc<Picture>,
}

impl Drop for Hung {
    fn drop(&mut self) {
        self.kit.free_set(self.set);
    }
}
