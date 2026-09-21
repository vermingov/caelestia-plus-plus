//! The graphics card's side of the background: one device, a pipeline for
//! the helix and one for a picture, and a swapchain for every screen.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use raw_window_handle::{RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle};

/// The three colours the helix is drawn in, as red, green and blue from
/// nought to one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colours {
    pub deep: [f32; 3],
    pub primary: [f32; 3],
    pub hot: [f32; 3],
}

/// One screen's swapchain, at the size it was last given, and the picture
/// hung on it if one is.
pub struct Canvas {
    surface: wgpu::Surface<'static>,
    size: (u32, u32),
    hung: Option<Hung>,
}

impl Canvas {
    pub fn size(&self) -> (u32, u32) {
        self.size
    }
}

/// A picture as the card holds it, bound beside the one it took over from
/// for as long as the change from one to the other is being shown.
struct Hung {
    picture: wgpu::TextureView,
    bound: wgpu::BindGroup,
}

/// What there is once there has been a surface to choose a card by.
struct Card {
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    helix: wgpu::RenderPipeline,
    helix_params: wgpu::Buffer,
    helix_bound: wgpu::BindGroup,
    picture: wgpu::RenderPipeline,
    picture_layout: wgpu::BindGroupLayout,
    /// How far a change of picture has got, which is the same on every screen.
    change: wgpu::Buffer,
    sampler: wgpu::Sampler,
    /// What a first picture takes over from.
    black: wgpu::TextureView,
    /// Set once the card has refused anything. What it refused it will refuse
    /// again, thirty times a second.
    refused: Arc<AtomicBool>,
}

pub struct Painter {
    instance: wgpu::Instance,
    card: Option<Card>,
}

/// The helix shader's `Params`, which is sixteen floats however it is counted.
const HELIX_PARAMS_SIZE: u64 = 64;
/// The picture shader's `Change`: one float, and the rest of the sixteen
/// bytes a uniform is never smaller than.
const CHANGE_SIZE: u64 = 16;

impl Painter {
    pub fn new() -> Painter {
        // Vulkan and nothing to fall back on. GPUI keeps OpenGL in reserve,
        // and a second instance that did would be nine megabytes and seven
        // threads of driver, for the sake of a machine whose desktop can do
        // without a background of this shell's.
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            flags: wgpu::InstanceFlags::default(),
            backend_options: wgpu::BackendOptions::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            display: None,
        });
        Painter { instance, card: None }
    }

    /// Something to draw on, for one Wayland surface.
    ///
    /// # Safety
    /// Both pointers must be what they are called, and both must outlive the
    /// canvas: it is to be dropped before the surface is destroyed.
    pub unsafe fn canvas(&self, display: NonNull<c_void>, surface: NonNull<c_void>) -> Result<Canvas, String> {
        let target = wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(RawDisplayHandle::Wayland(WaylandDisplayHandle::new(display))),
            raw_window_handle: RawWindowHandle::Wayland(WaylandWindowHandle::new(surface)),
        };
        // SAFETY: the caller's promise is the one this asks for.
        let surface = unsafe { self.instance.create_surface_unsafe(target) }.map_err(|error| error.to_string())?;
        Ok(Canvas { surface, size: (0, 0), hung: None })
    }

    /// Makes `canvas` that many pixels. The first one sized is what the card
    /// is chosen by. A picture hung on it was cut for the size it had, and
    /// goes.
    pub fn size(&mut self, canvas: &mut Canvas, width: u32, height: u32) -> Result<(), String> {
        if self.card.is_none() {
            self.card = Some(Card::for_surface(&self.instance, &canvas.surface)?);
        }
        let card = self.card.as_ref().expect("just chosen");
        let offered = canvas.surface.get_capabilities(&card.adapter);
        // Paced by a clock here, not by the screen: a frame the compositor
        // has no use for must not be waited on.
        let present_mode =
            if offered.present_modes.contains(&wgpu::PresentMode::Mailbox) { wgpu::PresentMode::Mailbox } else { wgpu::PresentMode::Fifo };
        let alpha_mode = if offered.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
            wgpu::CompositeAlphaMode::Opaque
        } else {
            wgpu::CompositeAlphaMode::Auto
        };
        (canvas.size, canvas.hung) = ((width.max(1), height.max(1)), None);
        canvas.surface.configure(
            &card.device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: card.format,
                width: canvas.size.0,
                height: canvas.size.1,
                present_mode,
                desired_maximum_frame_latency: 2,
                alpha_mode,
                view_formats: vec![],
            },
        );
        Ok(())
    }

    /// Whether the card has refused something it was asked, which is the end
    /// of the background: there is nothing here that can be asked another way.
    pub fn refused(&self) -> bool {
        self.card.as_ref().is_some_and(|card| card.refused.load(Ordering::Relaxed))
    }

    /// Draws the helix as it is `time` seconds in.
    pub fn paint_helix(&self, canvas: &Canvas, time: f32, colours: &Colours) {
        let Some(card) = &self.card else { return };
        let linearise = if card.format.is_srgb() { 1. } else { 0. };
        let aspect = canvas.size.0 as f32 / canvas.size.1 as f32;
        let [deep, primary, hot] = [colours.deep, colours.primary, colours.hot].map(|[red, green, blue]| [red, green, blue, 1.]);
        let params: Vec<u8> =
            [[time, aspect, linearise, 0.], deep, primary, hot].iter().flatten().flat_map(|number| number.to_ne_bytes()).collect();
        card.queue.write_buffer(&card.helix_params, 0, &params);
        card.draw(canvas, &card.helix, &card.helix_bound);
    }

    /// Hangs `pixels` on `canvas`, which they were cut to the size of. What
    /// hung there before stays beside it until `settle`.
    pub fn hang(&self, canvas: &mut Canvas, pixels: &image::RgbaImage) {
        let Some(card) = &self.card else { return };
        let picture = picture(&card.device, &card.queue, card.format, pixels);
        let before = canvas.hung.take().map_or_else(|| card.black.clone(), |hung| hung.picture);
        canvas.hung = Some(Hung { bound: card.bind_pictures(&before, &picture), picture });
    }

    /// Lets go of the picture the one on `canvas` took over from, once the
    /// change has been shown to its end.
    pub fn settle(&self, canvas: &mut Canvas) {
        let (Some(card), Some(hung)) = (&self.card, &mut canvas.hung) else { return };
        hung.bound = card.bind_pictures(&card.black, &hung.picture);
    }

    /// Draws the picture hung on `canvas`, `change` of the way from the one
    /// before it.
    pub fn paint_picture(&self, canvas: &Canvas, change: f32) {
        let (Some(card), Some(hung)) = (&self.card, &canvas.hung) else { return };
        let done: Vec<u8> = [change, 0., 0., 0.].iter().flat_map(|number| number.to_ne_bytes()).collect();
        card.queue.write_buffer(&card.change, 0, &done);
        card.draw(canvas, &card.picture, &hung.bound);
    }
}

impl Card {
    /// One triangle over the whole of `canvas`, through `pipeline`. A frame
    /// there is nothing to draw on just now is a frame missed, and the next
    /// may fare better.
    fn draw(&self, canvas: &Canvas, pipeline: &wgpu::RenderPipeline, bound: &wgpu::BindGroup) {
        let (wgpu::CurrentSurfaceTexture::Success(frame) | wgpu::CurrentSurfaceTexture::Suboptimal(frame)) =
            canvas.surface.get_current_texture()
        else {
            return;
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("background"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bound, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([encoder.finish()]);
        frame.present();
    }

    fn bind_pictures(&self, before: &wgpu::TextureView, after: &wgpu::TextureView) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("picture"),
            layout: &self.picture_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.change.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(before) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(after) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        })
    }

    /// The card that can show `surface` for the least power: on a laptop with
    /// two, the one the screen is wired to, and the other stays asleep.
    fn for_surface(instance: &wgpu::Instance, surface: &wgpu::Surface<'static>) -> Result<Card, String> {
        let adapter = futures::executor::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: Some(surface),
        }))
        .map_err(|error| error.to_string())?;
        let (device, queue) = futures::executor::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("background"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
        }))
        .map_err(|error| error.to_string())?;
        // The default is to panic, and a panic here is the whole shell's: a
        // desktop without its background still has a bar.
        let refused = Arc::new(AtomicBool::new(false));
        device.on_uncaptured_error(Arc::new({
            let refused = refused.clone();
            move |error| {
                eprintln!("cae: the background: {error}");
                refused.store(true, Ordering::Relaxed);
            }
        }));

        // What is drawn is already what the screen should show, so a surface
        // that would encode it a second time is the second choice, and both
        // shaders are told when they have been given one.
        const AS_GIVEN: [wgpu::TextureFormat; 2] = [wgpu::TextureFormat::Bgra8Unorm, wgpu::TextureFormat::Rgba8Unorm];
        const ENCODED: [wgpu::TextureFormat; 2] = [wgpu::TextureFormat::Bgra8UnormSrgb, wgpu::TextureFormat::Rgba8UnormSrgb];
        let offered = surface.get_capabilities(&adapter).formats;
        let format = AS_GIVEN.into_iter().chain(ENCODED).find(|format| offered.contains(format));
        let format = format.ok_or("the surface takes no format the background is drawn in")?;

        let uniform = |binding: u32, size: u64| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: std::num::NonZeroU64::new(size),
            },
            count: None,
        };
        let texture = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let sampled = wgpu::BindGroupLayoutEntry {
            binding: 3,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let buffer = |size: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("background"),
                size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };

        let helix_layout = device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("helix"), entries: &[uniform(0, HELIX_PARAMS_SIZE)] });
        let helix_params = buffer(HELIX_PARAMS_SIZE);
        let helix_bound = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("helix"),
            layout: &helix_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: helix_params.as_entire_binding() }],
        });
        let picture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("picture"),
            entries: &[uniform(0, CHANGE_SIZE), texture(1), texture(2), sampled],
        });

        let black = picture(&device, &queue, format, &image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 255])));
        Ok(Card {
            helix: pipeline(&device, "helix", include_str!("helix.wgsl"), &helix_layout, format),
            picture: pipeline(&device, "picture", include_str!("picture.wgsl"), &picture_layout, format),
            change: buffer(CHANGE_SIZE),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            black,
            adapter,
            device,
            queue,
            format,
            helix_params,
            helix_bound,
            picture_layout,
            refused,
        })
    }
}

/// `pixels` on the card. A picture's bytes are encoded for a screen already,
/// so they are taken as they are where the surface shows what it is given,
/// and decoded on the way in where the surface will encode them again.
fn picture(device: &wgpu::Device, queue: &wgpu::Queue, surface: wgpu::TextureFormat, pixels: &image::RgbaImage) -> wgpu::TextureView {
    let size = wgpu::Extent3d { width: pixels.width(), height: pixels.height(), depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("picture"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: if surface.is_srgb() { wgpu::TextureFormat::Rgba8UnormSrgb } else { wgpu::TextureFormat::Rgba8Unorm },
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        pixels,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * size.width), rows_per_image: Some(size.height) },
        size,
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// A pipeline that draws one triangle over everything through the shader in
/// `source`, whose two entry points are `corner` and `paint`.
fn pipeline(
    device: &wgpu::Device,
    label: &str,
    source: &str,
    layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(source.into()) });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("corner"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("paint"),
            targets: &[Some(format.into())],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}
