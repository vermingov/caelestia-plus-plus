//! A Wayland surface of the drawer's own, as the card draws on it.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use ash::vk;
use wayland_client::Dispatch;
use wayland_client::protocol::wl_buffer;
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_buffer_params_v1;

use super::chain::Chain;
use super::direct::{Handed, Wayland};
use super::frame::{Fault, Frame, Painted};
use super::{Card, Mapped, OUT_FORMAT, Passes, Vulkan};

/// What a drawer's canvases are made of.
pub struct Spec {
    /// What it is called in what is logged.
    pub name: &'static str,
    pub passes: Passes,
    /// How many bytes a frame writes for the card to read.
    pub room: u64,
    /// Whether the surface is seen through wherever nothing is drawn, or
    /// covered whole.
    pub see_through: bool,
}

/// How a canvas's frames reach the compositor.
enum Target<K> {
    Chain(Chain<K>),
    Handed(Handed<K>),
}

/// One surface on the card, and what it is drawn on once it has a size.
pub struct Canvas<K> {
    vulkan: &'static Vulkan,
    pub surface: vk::SurfaceKHR,
    size: (u32, u32),
    target: Option<Target<K>>,
}

impl<K: Copy + PartialEq> Canvas<K> {
    /// Something to draw on, for one Wayland surface.
    ///
    /// # Safety
    /// Both pointers must be what they are called, and both must outlive the
    /// canvas: it is to be dropped before the surface is destroyed.
    pub unsafe fn new(vulkan: &'static Vulkan, display: NonNull<c_void>, surface: NonNull<c_void>) -> Result<Canvas<K>, String> {
        // SAFETY: the caller's promise is the one this asks for.
        let surface = unsafe { vulkan.surface(display, surface) }?;
        Ok(Canvas { vulkan, surface, size: (0, 0), target: None })
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Makes it that many pixels. Handed to the compositor directly where
    /// `wayland` says it takes that and the card can give it; through a
    /// swapchain otherwise. What it was drawn on before goes.
    pub fn fit<D>(&mut self, card: &Rc<Card>, (width, height): (u32, u32), wayland: Option<Wayland<D>>, spec: &Spec) -> Result<(), Fault>
    where
        D: Dispatch<zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1, ()> + Dispatch<wl_buffer::WlBuffer, Arc<AtomicBool>> + 'static,
    {
        self.size = (width.max(1), height.max(1));
        self.idle();
        let old = self.target.take();

        let direct = wayland.filter(|_| card.exports.is_some() && card.format.format == OUT_FORMAT);
        if let Some(wayland) = direct {
            drop(old);
            match Handed::new(card, self.size, &wayland, spec) {
                Ok(handed) => {
                    log::info!("{}: {}x{}, handed to the compositor directly", spec.name, self.size.0, self.size.1);
                    self.target = Some(Target::Handed(handed));
                    return Ok(());
                }
                Err(fault) => eprintln!("cae: the {}: {}, so it goes through a swapchain", spec.name, fault.said),
            }
            self.target = Some(Target::Chain(Chain::new(card, self.surface, self.size, vk::SwapchainKHR::null(), spec)?));
            return Ok(());
        }
        let retiring = match &old {
            Some(Target::Chain(old)) => old.swapchain,
            _ => vk::SwapchainKHR::null(),
        };
        let made = Chain::new(card, self.surface, self.size, retiring, spec);
        drop(old);
        self.target = Some(Target::Chain(made?));
        log::info!("{}: {}x{}, through a swapchain", spec.name, self.size.0, self.size.1);
        Ok(())
    }

    /// Draws a frame and sends it, calling `sending` just before it goes;
    /// see `frame::draw` for the rest. Nothing is drawn before the canvas has
    /// a size, or while no image is free.
    pub fn paint(&mut self, sending: impl FnOnce(), key: Option<K>, fill: impl FnOnce(&Mapped), record: impl FnOnce(&Card, &Frame<K>)) -> Result<Painted, Fault> {
        match &mut self.target {
            Some(Target::Chain(chain)) => chain.paint(sending, key, fill, record),
            Some(Target::Handed(handed)) => handed.paint(sending, key, fill, record),
            None => Ok(Painted::NotNow),
        }
    }
}

impl<K> Canvas<K> {
    /// Waits for every frame of this canvas to be done, before anything a
    /// frame reads is let go of or rebound.
    pub fn idle(&self) {
        match &self.target {
            Some(Target::Chain(chain)) => chain.idle(),
            Some(Target::Handed(handed)) => handed.idle(),
            None => {}
        }
    }
}

impl<K> Drop for Canvas<K> {
    fn drop(&mut self) {
        // What is drawn on goes before the surface it was made for.
        self.idle();
        self.target = None;
        // SAFETY: nothing made on the surface is left.
        unsafe { self.vulkan.surfaces.destroy_surface(self.surface, None) };
    }
}
