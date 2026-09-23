//! The graphics card, spoken to directly.
//!
//! The rest of the shell draws through wgpu, and for a window that changes
//! when somebody does something that is the right tool. The background
//! changes sixty times a second for as long as the desktop is in view, and
//! at that rate what wgpu does around every frame — tracking each resource,
//! validating each command, making an encoder and a view and a staging
//! buffer — cost more than the drawing did: half a millisecond of processor
//! a frame. Here a frame is a handful of calls to the driver and a copy into
//! memory the card reads directly.
//!
//! What lives here is made once: the device, the three pipelines and what a
//! picture is uploaded with. A screen's own swapchain is `paint::Canvas`.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::rc::Rc;

use ash::vk;
use bytemuck::{Pod, Zeroable};

/// What the two shaders are compiled from, once, when the card is chosen.
const SCENE: &str = include_str!("scene.wgsl");
const PICTURE: &str = include_str!("picture.wgsl");

/// The immediates the scene's shaders read, in the order `scene.wgsl`
/// declares them.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct Scene {
    pub size: [f32; 2],
    pub axis_origin: [f32; 2],
    pub axis_direction: [f32; 2],
    pub linearise: f32,
    pub spare: f32,
    pub deep: [f32; 4],
    pub primary: [f32; 4],
    pub hot: [f32; 4],
}

/// The picture shader's immediates: how far one picture has taken over from
/// the one before it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Change {
    pub done: f32,
}

/// The loader and the instance: what a surface is made with, before there is
/// a card to show it.
pub struct Vulkan {
    _entry: ash::Entry,
    pub instance: ash::Instance,
    pub surfaces: ash::khr::surface::Instance,
    wayland: ash::khr::wayland_surface::Instance,
}

impl Vulkan {
    pub fn new() -> Result<Rc<Vulkan>, String> {
        // SAFETY: loading the system's Vulkan loader, which is what it is for.
        let entry = unsafe { ash::Entry::load() }.map_err(|error| format!("no Vulkan here: {error}"))?;
        let application = vk::ApplicationInfo::default().application_name(c"cae background").api_version(vk::API_VERSION_1_1);
        let extensions = [ash::khr::surface::NAME.as_ptr(), ash::khr::wayland_surface::NAME.as_ptr()];
        let asked = vk::InstanceCreateInfo::default().application_info(&application).enabled_extension_names(&extensions);
        // SAFETY: a well-formed create info whose pointers outlive the call.
        let instance = unsafe { entry.create_instance(&asked, None) }.map_err(|error| format!("Vulkan refused an instance: {error}"))?;
        let surfaces = ash::khr::surface::Instance::new(&entry, &instance);
        let wayland = ash::khr::wayland_surface::Instance::new(&entry, &instance);
        Ok(Rc::new(Vulkan { _entry: entry, instance, surfaces, wayland }))
    }

    /// A Vulkan surface for a Wayland one.
    ///
    /// # Safety
    /// Both pointers must be what they are called, and both must outlive the
    /// surface that is returned.
    pub unsafe fn surface(&self, display: NonNull<c_void>, surface: NonNull<c_void>) -> Result<vk::SurfaceKHR, String> {
        let asked = vk::WaylandSurfaceCreateInfoKHR::default().display(display.as_ptr()).surface(surface.as_ptr());
        // SAFETY: the caller's promise is the one this asks for.
        unsafe { self.wayland.create_wayland_surface(&asked, None) }.map_err(|error| format!("no Vulkan surface: {error}"))
    }
}

impl Drop for Vulkan {
    fn drop(&mut self) {
        // SAFETY: everything made from the instance holds an `Rc` of this,
        // so nothing made from it is left.
        unsafe { self.instance.destroy_instance(None) };
    }
}

/// The card chosen to show the background, and everything made on it once.
pub struct Card {
    pub vulkan: Rc<Vulkan>,
    pub physical: vk::PhysicalDevice,
    pub device: ash::Device,
    pub queue: vk::Queue,
    pub swapchains: ash::khr::swapchain::Device,
    memory: vk::PhysicalDeviceMemoryProperties,
    pub format: vk::SurfaceFormatKHR,
    /// Whether the surface encodes what it is given, in which case the
    /// shaders must hand it linear light.
    pub encodes: bool,
    pub render_pass: vk::RenderPass,
    /// The same drawing, for an image handed straight to the compositor
    /// rather than through a swapchain: it ends in a layout another process
    /// may read, not the one presenting wants.
    pub render_pass_out: vk::RenderPass,
    /// How an image's memory is handed to another process, where the card
    /// can do that.
    pub exports: Option<ash::khr::external_memory_fd::Device>,
    pub scene_layout: vk::PipelineLayout,
    pub backdrop: vk::Pipeline,
    pub shapes: vk::Pipeline,
    picture_set_layout: vk::DescriptorSetLayout,
    pub picture_layout: vk::PipelineLayout,
    pub picture: vk::Pipeline,
    sampler: vk::Sampler,
    pub commands: vk::CommandPool,
    descriptors: vk::DescriptorPool,
}

impl Card {
    /// The card that can show `surface` for the least power: on a laptop with
    /// two, the one the screen is wired to, and the other stays asleep.
    pub fn for_surface(vulkan: &Rc<Vulkan>, surface: vk::SurfaceKHR) -> Result<Rc<Card>, String> {
        let instance = &vulkan.instance;
        // SAFETY: the instance is alive for as long as `vulkan` is.
        let physicals = unsafe { instance.enumerate_physical_devices() }.map_err(|error| error.to_string())?;
        let can_show = |physical: vk::PhysicalDevice| -> Option<u32> {
            // SAFETY: a physical device the instance just listed.
            let families = unsafe { instance.get_physical_device_queue_family_properties(physical) };
            families.iter().enumerate().find_map(|(index, family)| {
                let index = index as u32;
                let draws = family.queue_flags.contains(vk::QueueFlags::GRAPHICS);
                // SAFETY: as above, and a live surface.
                let shows = unsafe { vulkan.surfaces.get_physical_device_surface_support(physical, index, surface) }.unwrap_or(false);
                (draws && shows).then_some(index)
            })
        };
        let thrift = |physical: vk::PhysicalDevice| {
            // SAFETY: as above.
            match unsafe { instance.get_physical_device_properties(physical) }.device_type {
                vk::PhysicalDeviceType::INTEGRATED_GPU => 0,
                vk::PhysicalDeviceType::DISCRETE_GPU => 1,
                vk::PhysicalDeviceType::VIRTUAL_GPU => 2,
                _ => 3,
            }
        };
        let (physical, family) = physicals
            .into_iter()
            .filter_map(|physical| Some((physical, can_show(physical)?)))
            .min_by_key(|(physical, _)| thrift(*physical))
            .ok_or("no graphics card here can show the background")?;

        let priorities = [1.0];
        let queues = [vk::DeviceQueueCreateInfo::default().queue_family_index(family).queue_priorities(&priorities)];
        // Handing a frame to the compositor as a dma-buf needs two more,
        // where the card has them; without them it goes through a swapchain.
        let exporting = [ash::khr::external_memory_fd::NAME, ash::ext::external_memory_dma_buf::NAME];
        // SAFETY: a physical device the instance just listed.
        let offered = unsafe { instance.enumerate_device_extension_properties(physical) }.unwrap_or_default();
        let exports = exporting.iter().all(|wanted| offered.iter().any(|extension| extension.extension_name_as_c_str() == Ok(*wanted)));
        let mut extensions = vec![ash::khr::swapchain::NAME.as_ptr()];
        if exports {
            extensions.extend(exporting.iter().map(|name| name.as_ptr()));
        }
        let asked = vk::DeviceCreateInfo::default().queue_create_infos(&queues).enabled_extension_names(&extensions);
        // SAFETY: a device of the instance's, asked for with a well-formed
        // create info.
        let device = unsafe { instance.create_device(physical, &asked, None) }.map_err(|error| format!("the card refused a device: {error}"))?;
        Card::on(vulkan, physical, device, family, surface, exports).map(Rc::new)
    }

    /// The memory type that `wanted` allows and has `flags`.
    fn memory_type(&self, wanted: u32, flags: vk::MemoryPropertyFlags) -> Option<u32> {
        (0..self.memory.memory_type_count)
            .find(|index| wanted & (1 << index) != 0 && self.memory.memory_types[*index as usize].property_flags.contains(flags))
    }

    /// A buffer of `size` bytes in memory the processor writes and the card
    /// reads, mapped for as long as it lives.
    pub fn mapped(self: &Rc<Card>, size: u64, usage: vk::BufferUsageFlags) -> Result<Mapped, String> {
        let asked = vk::BufferCreateInfo::default().size(size).usage(usage).sharing_mode(vk::SharingMode::EXCLUSIVE);
        // SAFETY: a live device and a well-formed create info, and every
        // object made here is destroyed by `Mapped` or on the way out below.
        unsafe {
            let buffer = self.device.create_buffer(&asked, None).map_err(|error| error.to_string())?;
            let needs = self.device.get_buffer_memory_requirements(buffer);
            let shared = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
            let Some(kind) = self.memory_type(needs.memory_type_bits, shared) else {
                self.device.destroy_buffer(buffer, None);
                return Err("no memory both the card and the processor can reach".to_string());
            };
            let memory = match self.device.allocate_memory(&vk::MemoryAllocateInfo::default().allocation_size(needs.size).memory_type_index(kind), None) {
                Ok(memory) => memory,
                Err(error) => {
                    self.device.destroy_buffer(buffer, None);
                    return Err(error.to_string());
                }
            };
            let mapped = self
                .device
                .bind_buffer_memory(buffer, memory, 0)
                .and_then(|()| self.device.map_memory(memory, 0, size, vk::MemoryMapFlags::empty()));
            match mapped {
                Ok(at) => Ok(Mapped { card: self.clone(), buffer, memory, at: at.cast(), size: size as usize }),
                Err(error) => {
                    self.device.destroy_buffer(buffer, None);
                    self.device.free_memory(memory, None);
                    Err(error.to_string())
                }
            }
        }
    }

    /// `pixels`, uploaded, for a picture pipeline to sample: cut to the size
    /// of the screen already, and taken as they are, or decoded on the way
    /// in where the surface will encode them again.
    pub fn picture(self: &Rc<Card>, pixels: &image::RgbaImage) -> Result<Picture, String> {
        let (width, height) = pixels.dimensions();
        let format = if self.encodes { vk::Format::R8G8B8A8_SRGB } else { vk::Format::R8G8B8A8_UNORM };
        let extent = vk::Extent3D { width, height, depth: 1 };
        let asked = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(format)
            .extent(extent)
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        let staging = self.mapped(u64::from(width) * u64::from(height) * 4, vk::BufferUsageFlags::TRANSFER_SRC)?;
        staging.write(0, pixels.as_raw());
        // SAFETY: a live device, well-formed create infos, and every object
        // owned by the `Picture` that is returned or destroyed before an
        // error is.
        unsafe {
            let image = self.device.create_image(&asked, None).map_err(|error| error.to_string())?;
            let needs = self.device.get_image_memory_requirements(image);
            let kind = self.memory_type(needs.memory_type_bits, vk::MemoryPropertyFlags::DEVICE_LOCAL).unwrap_or(0);
            let memory = match self.device.allocate_memory(&vk::MemoryAllocateInfo::default().allocation_size(needs.size).memory_type_index(kind), None) {
                Ok(memory) => memory,
                Err(error) => {
                    self.device.destroy_image(image, None);
                    return Err(error.to_string());
                }
            };
            let mut picture = Picture { card: self.clone(), image, memory, view: vk::ImageView::null() };
            self.device.bind_image_memory(image, memory, 0).map_err(|error| error.to_string())?;
            let whole = vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(1).layer_count(1);
            self.once(|commands| {
                let to_copy = vk::ImageMemoryBarrier::default()
                    .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .image(image)
                    .subresource_range(whole);
                self.device.cmd_pipeline_barrier(commands, vk::PipelineStageFlags::TOP_OF_PIPE, vk::PipelineStageFlags::TRANSFER, vk::DependencyFlags::empty(), &[], &[], &[to_copy]);
                let region = vk::BufferImageCopy::default()
                    .image_subresource(vk::ImageSubresourceLayers::default().aspect_mask(vk::ImageAspectFlags::COLOR).layer_count(1))
                    .image_extent(extent);
                self.device.cmd_copy_buffer_to_image(commands, staging.buffer, image, vk::ImageLayout::TRANSFER_DST_OPTIMAL, &[region]);
                let to_sample = vk::ImageMemoryBarrier::default()
                    .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ)
                    .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .image(image)
                    .subresource_range(whole);
                self.device.cmd_pipeline_barrier(commands, vk::PipelineStageFlags::TRANSFER, vk::PipelineStageFlags::FRAGMENT_SHADER, vk::DependencyFlags::empty(), &[], &[], &[to_sample]);
            })?;
            let view = vk::ImageViewCreateInfo::default().image(image).view_type(vk::ImageViewType::TYPE_2D).format(format).subresource_range(whole);
            picture.view = self.device.create_image_view(&view, None).map_err(|error| error.to_string())?;
            Ok(picture)
        }
    }

    /// An image the compositor can be handed directly: its memory a dma-buf,
    /// laid out plainly, so that whatever reads it needs to know nothing
    /// about this card. The dma-buf itself is `Exported::fd`, for the
    /// caller to hand over and close.
    pub fn exported(self: &Rc<Card>, width: u32, height: u32) -> Result<Exported, String> {
        let exports = self.exports.as_ref().ok_or("the card cannot hand its memory over")?;
        let dma_buf = vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT;
        let mut external = vk::ExternalMemoryImageCreateInfo::default().handle_types(dma_buf);
        let asked = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(OUT_FORMAT)
            .extent(vk::Extent3D { width, height, depth: 1 })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::LINEAR)
            .usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .push_next(&mut external);
        let failed = |error: vk::Result| error.to_string();
        // SAFETY: a live device and well-formed create infos; everything made
        // belongs to the `Exported` from the moment it exists, whose `Drop`
        // destroys it, null handles included.
        unsafe {
            let mut out = Exported::empty(self);
            out.image = self.device.create_image(&asked, None).map_err(failed)?;
            let needs = self.device.get_image_memory_requirements(out.image);
            let kind = self.memory_type(needs.memory_type_bits, vk::MemoryPropertyFlags::DEVICE_LOCAL).unwrap_or(0);
            let mut exportable = vk::ExportMemoryAllocateInfo::default().handle_types(dma_buf);
            let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(out.image);
            let allocate = vk::MemoryAllocateInfo::default()
                .allocation_size(needs.size)
                .memory_type_index(kind)
                .push_next(&mut exportable)
                .push_next(&mut dedicated);
            out.memory = self.device.allocate_memory(&allocate, None).map_err(failed)?;
            self.device.bind_image_memory(out.image, out.memory, 0).map_err(failed)?;
            let plane = self.device.get_image_subresource_layout(
                out.image,
                vk::ImageSubresource { aspect_mask: vk::ImageAspectFlags::COLOR, mip_level: 0, array_layer: 0 },
            );
            (out.offset, out.stride) = (plane.offset, plane.row_pitch);
            let whole = vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(1).layer_count(1);
            let view = vk::ImageViewCreateInfo::default().image(out.image).view_type(vk::ImageViewType::TYPE_2D).format(OUT_FORMAT).subresource_range(whole);
            out.view = self.device.create_image_view(&view, None).map_err(failed)?;
            let fd = exports.get_memory_fd(&vk::MemoryGetFdInfoKHR::default().memory(out.memory).handle_type(dma_buf)).map_err(failed)?;
            out.fd = Some(std::os::fd::FromRawFd::from_raw_fd(fd));
            Ok(out)
        }
    }

    /// Records `work` into a command buffer of its own, runs it, and waits
    /// for it: for the rare thing that happens once, like an upload.
    fn once(&self, work: impl FnOnce(vk::CommandBuffer)) -> Result<(), String> {
        let asked = vk::CommandBufferAllocateInfo::default().command_pool(self.commands).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(1);
        // SAFETY: a live device and pool; the buffer and fence are freed
        // before this returns, and the wait means nothing is still using them.
        unsafe {
            let commands = self.device.allocate_command_buffers(&asked).map_err(|error| error.to_string())?[0];
            let fence = self.device.create_fence(&vk::FenceCreateInfo::default(), None).map_err(|error| error.to_string())?;
            let ran = (|| {
                self.device.begin_command_buffer(commands, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT))?;
                work(commands);
                self.device.end_command_buffer(commands)?;
                let buffers = [commands];
                self.device.queue_submit(self.queue, &[vk::SubmitInfo::default().command_buffers(&buffers)], fence)?;
                self.device.wait_for_fences(&[fence], true, u64::MAX)
            })();
            self.device.destroy_fence(fence, None);
            self.device.free_command_buffers(self.commands, &[commands]);
            ran.map_err(|error| error.to_string())
        }
    }

    /// A set binding two pictures for the picture pipeline: the one taking
    /// over and the one it takes over from.
    pub fn bind_pictures(&self, before: vk::ImageView, after: vk::ImageView) -> Result<vk::DescriptorSet, String> {
        let layouts = [self.picture_set_layout];
        let asked = vk::DescriptorSetAllocateInfo::default().descriptor_pool(self.descriptors).set_layouts(&layouts);
        // SAFETY: a live device and pool; the set is freed by `free_set`.
        let set = unsafe { self.device.allocate_descriptor_sets(&asked) }.map_err(|error| error.to_string())?[0];
        self.rebind(set, before, after);
        Ok(set)
    }

    pub fn rebind(&self, set: vk::DescriptorSet, before: vk::ImageView, after: vk::ImageView) {
        let image = |view| [vk::DescriptorImageInfo::default().image_view(view).image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        let (before, after) = (image(before), image(after));
        let sampler = [vk::DescriptorImageInfo::default().sampler(self.sampler)];
        let writes = [
            vk::WriteDescriptorSet::default().dst_set(set).dst_binding(0).descriptor_type(vk::DescriptorType::SAMPLED_IMAGE).image_info(&before),
            vk::WriteDescriptorSet::default().dst_set(set).dst_binding(1).descriptor_type(vk::DescriptorType::SAMPLED_IMAGE).image_info(&after),
            vk::WriteDescriptorSet::default().dst_set(set).dst_binding(2).descriptor_type(vk::DescriptorType::SAMPLER).image_info(&sampler),
        ];
        // SAFETY: a set of this card's and views that are alive; a set is
        // only rewritten once nothing in flight reads it.
        unsafe { self.device.update_descriptor_sets(&writes, &[]) };
    }

    pub fn free_set(&self, set: vk::DescriptorSet) {
        // SAFETY: a set from this pool that nothing in flight uses.
        let _ = unsafe { self.device.free_descriptor_sets(self.descriptors, &[set]) };
    }
}

impl Drop for Card {
    fn drop(&mut self) {
        // SAFETY: every canvas, picture and buffer holds an `Rc` of the card,
        // so all of them are gone, and the wait leaves nothing in flight.
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_descriptor_pool(self.descriptors, None);
            self.device.destroy_command_pool(self.commands, None);
            self.device.destroy_sampler(self.sampler, None);
            for pipeline in [self.backdrop, self.shapes, self.picture] {
                self.device.destroy_pipeline(pipeline, None);
            }
            self.device.destroy_pipeline_layout(self.scene_layout, None);
            self.device.destroy_pipeline_layout(self.picture_layout, None);
            self.device.destroy_descriptor_set_layout(self.picture_set_layout, None);
            self.device.destroy_render_pass(self.render_pass, None);
            self.device.destroy_render_pass(self.render_pass_out, None);
            self.device.destroy_device(None);
        }
    }
}

impl Card {
    /// Everything that follows the device, made in the order it is needed.
    fn on(
        vulkan: &Rc<Vulkan>,
        physical: vk::PhysicalDevice,
        device: ash::Device,
        family: u32,
        surface: vk::SurfaceKHR,
        exports: bool,
    ) -> Result<Card, String> {
        let instance = &vulkan.instance;
        // SAFETY: a device just made on the instance, and a live surface.
        let (queue, memory, formats) = unsafe {
            (
                device.get_device_queue(family, 0),
                instance.get_physical_device_memory_properties(physical),
                vulkan.surfaces.get_physical_device_surface_formats(physical, surface).unwrap_or_default(),
            )
        };
        // What is drawn is already what the screen should show, so a surface
        // that would encode it a second time is the second choice, and the
        // shaders are told when they have been given one.
        let given = [vk::Format::B8G8R8A8_UNORM, vk::Format::R8G8B8A8_UNORM];
        let encoded = [vk::Format::B8G8R8A8_SRGB, vk::Format::R8G8B8A8_SRGB];
        let pick = given.iter().chain(&encoded).find_map(|wanted| formats.iter().find(|offered| offered.format == *wanted).copied());
        let Some(format) = pick else {
            // SAFETY: nothing has been made on the device yet.
            unsafe { device.destroy_device(None) };
            return Err("the screen takes no format the background is drawn in".to_string());
        };
        let encodes = encoded.contains(&format.format);

        let mut card = Card {
            vulkan: vulkan.clone(),
            physical,
            swapchains: ash::khr::swapchain::Device::new(instance, &device),
            exports: exports.then(|| ash::khr::external_memory_fd::Device::new(instance, &device)),
            device,
            queue,
            memory,
            format,
            encodes,
            render_pass: vk::RenderPass::null(),
            render_pass_out: vk::RenderPass::null(),
            scene_layout: vk::PipelineLayout::null(),
            backdrop: vk::Pipeline::null(),
            shapes: vk::Pipeline::null(),
            picture_set_layout: vk::DescriptorSetLayout::null(),
            picture_layout: vk::PipelineLayout::null(),
            picture: vk::Pipeline::null(),
            sampler: vk::Sampler::null(),
            commands: vk::CommandPool::null(),
            descriptors: vk::DescriptorPool::null(),
        };
        // Everything below is destroyed by the card's `Drop` if it fails part
        // of the way: destroying a null handle is allowed and does nothing.
        card.fill(family)?;
        Ok(card)
    }

    fn fill(&mut self, family: u32) -> Result<(), String> {
        let card = self;
        let device = &card.device;
        let failed = |error: vk::Result| error.to_string();
        // SAFETY: a live device, and create infos whose pointers outlive
        // each call.
        unsafe {
            card.commands = device
                .create_command_pool(&vk::CommandPoolCreateInfo::default().queue_family_index(family).flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER), None)
                .map_err(failed)?;
            let sizes = [
                vk::DescriptorPoolSize { ty: vk::DescriptorType::SAMPLED_IMAGE, descriptor_count: 16 },
                vk::DescriptorPoolSize { ty: vk::DescriptorType::SAMPLER, descriptor_count: 8 },
            ];
            card.descriptors = device
                .create_descriptor_pool(&vk::DescriptorPoolCreateInfo::default().max_sets(8).pool_sizes(&sizes).flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET), None)
                .map_err(failed)?;
            card.sampler = device
                .create_sampler(&vk::SamplerCreateInfo::default().mag_filter(vk::Filter::LINEAR).min_filter(vk::Filter::LINEAR), None)
                .map_err(failed)?;

            // One colour attachment, drawn over whole: the backdrop covers
            // every pixel, so nothing need be cleared or kept from before.
            let attachment = [vk::AttachmentDescription::default()
                .format(card.format.format)
                .samples(vk::SampleCountFlags::TYPE_1)
                .load_op(vk::AttachmentLoadOp::DONT_CARE)
                .store_op(vk::AttachmentStoreOp::STORE)
                .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                .initial_layout(vk::ImageLayout::UNDEFINED)
                .final_layout(vk::ImageLayout::PRESENT_SRC_KHR)];
            let colour = [vk::AttachmentReference { attachment: 0, layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL }];
            let subpass = [vk::SubpassDescription::default().pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS).color_attachments(&colour)];
            let after_acquiring = [vk::SubpassDependency::default()
                .src_subpass(vk::SUBPASS_EXTERNAL)
                .dst_subpass(0)
                .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
                .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
                .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)];
            card.render_pass = device
                .create_render_pass(&vk::RenderPassCreateInfo::default().attachments(&attachment).subpasses(&subpass).dependencies(&after_acquiring), None)
                .map_err(failed)?;
            let handed_over = [attachment[0].final_layout(vk::ImageLayout::GENERAL)];
            card.render_pass_out = device
                .create_render_pass(&vk::RenderPassCreateInfo::default().attachments(&handed_over).subpasses(&subpass), None)
                .map_err(failed)?;

            let immediates = |size: usize, stages| [vk::PushConstantRange::default().stage_flags(stages).offset(0).size(size as u32)];
            let scene_immediates = immediates(size_of::<Scene>(), vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT);
            card.scene_layout = device
                .create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default().push_constant_ranges(&scene_immediates), None)
                .map_err(failed)?;
            let bindings = [
                (0, vk::DescriptorType::SAMPLED_IMAGE),
                (1, vk::DescriptorType::SAMPLED_IMAGE),
                (2, vk::DescriptorType::SAMPLER),
            ]
            .map(|(binding, kind)| {
                vk::DescriptorSetLayoutBinding::default().binding(binding).descriptor_type(kind).descriptor_count(1).stage_flags(vk::ShaderStageFlags::FRAGMENT)
            });
            card.picture_set_layout = device
                .create_descriptor_set_layout(&vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings), None)
                .map_err(failed)?;
            let picture_immediates = immediates(size_of::<Change>(), vk::ShaderStageFlags::FRAGMENT);
            let set_layouts = [card.picture_set_layout];
            card.picture_layout = device
                .create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default().set_layouts(&set_layouts).push_constant_ranges(&picture_immediates), None)
                .map_err(failed)?;
        }

        let scene = shader(&card.device, SCENE)?;
        let picture = shader(&card.device, PICTURE)?;
        let made = (|| -> Result<(), String> {
            card.backdrop = pipeline(card, card.scene_layout, scene, (c"cover", c"backdrop"), Draws::Screen)?;
            card.shapes = pipeline(card, card.scene_layout, scene, (c"outline", c"shade"), Draws::Shapes)?;
            card.picture = pipeline(card, card.picture_layout, picture, (c"cover", c"paint"), Draws::Screen)?;
            Ok(())
        })();
        // SAFETY: modules are only needed while pipelines are made.
        unsafe {
            card.device.destroy_shader_module(scene, None);
            card.device.destroy_shader_module(picture, None);
        }
        made
    }
}

/// WGSL compiled to SPIR-V, by the same compiler wgpu uses, and handed to
/// the driver as a module.
fn shader(device: &ash::Device, source: &str) -> Result<vk::ShaderModule, String> {
    let module = naga::front::wgsl::parse_str(source).map_err(|error| error.emit_to_string(source))?;
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::IMMEDIATES)
        .validate(&module)
        .map_err(|error| error.emit_to_string(source))?;
    // Written for Vulkan's own coordinates, so nothing is to be flipped.
    let options = naga::back::spv::Options { flags: naga::back::spv::WriterFlags::empty(), ..Default::default() };
    let words = naga::back::spv::write_vec(&module, &info, &options, None).map_err(|error| error.to_string())?;
    // SAFETY: SPIR-V the compiler just validated and wrote.
    unsafe { device.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&words), None) }.map_err(|error| error.to_string())
}

/// What a pipeline is fed.
enum Draws {
    /// One triangle over the whole screen, from nothing.
    Screen,
    /// A strip of four corners per shape, from the shapes buffer, laid over
    /// what is there by how much each covers.
    Shapes,
}

fn pipeline(card: &Card, layout: vk::PipelineLayout, module: vk::ShaderModule, (vertex, fragment): (&std::ffi::CStr, &std::ffi::CStr), draws: Draws) -> Result<vk::Pipeline, String> {
    let stages = [
        vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::VERTEX).module(module).name(vertex),
        vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::FRAGMENT).module(module).name(fragment),
    ];
    let shape_binding = [vk::VertexInputBindingDescription { binding: 0, stride: size_of::<super::helix::Shape>() as u32, input_rate: vk::VertexInputRate::INSTANCE }];
    let shape_attributes = [0, 1, 2, 3].map(|location| vk::VertexInputAttributeDescription { location, binding: 0, format: vk::Format::R32G32B32A32_SFLOAT, offset: location * 16 });
    let (input, topology, blend) = match draws {
        Draws::Screen => (vk::PipelineVertexInputStateCreateInfo::default(), vk::PrimitiveTopology::TRIANGLE_LIST, false),
        Draws::Shapes => (
            vk::PipelineVertexInputStateCreateInfo::default().vertex_binding_descriptions(&shape_binding).vertex_attribute_descriptions(&shape_attributes),
            vk::PrimitiveTopology::TRIANGLE_STRIP,
            true,
        ),
    };
    let assembly = vk::PipelineInputAssemblyStateCreateInfo::default().topology(topology);
    let viewport = vk::PipelineViewportStateCreateInfo::default().viewport_count(1).scissor_count(1);
    let raster = vk::PipelineRasterizationStateCreateInfo::default().polygon_mode(vk::PolygonMode::FILL).cull_mode(vk::CullModeFlags::NONE).line_width(1.);
    let samples = vk::PipelineMultisampleStateCreateInfo::default().rasterization_samples(vk::SampleCountFlags::TYPE_1);
    // Premultiplied: what a shape covers replaces as much of what is behind,
    // and a glow adds light without covering anything.
    let over = [vk::PipelineColorBlendAttachmentState::default()
        .blend_enable(blend)
        .src_color_blend_factor(vk::BlendFactor::ONE)
        .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .color_blend_op(vk::BlendOp::ADD)
        .src_alpha_blend_factor(vk::BlendFactor::ONE)
        .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .alpha_blend_op(vk::BlendOp::ADD)
        .color_write_mask(vk::ColorComponentFlags::RGBA)];
    let blending = vk::PipelineColorBlendStateCreateInfo::default().attachments(&over);
    let dynamic = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic);
    let asked = vk::GraphicsPipelineCreateInfo::default()
        .stages(&stages)
        .vertex_input_state(&input)
        .input_assembly_state(&assembly)
        .viewport_state(&viewport)
        .rasterization_state(&raster)
        .multisample_state(&samples)
        .color_blend_state(&blending)
        .dynamic_state(&dynamic)
        .layout(layout)
        .render_pass(card.render_pass);
    // SAFETY: a live device, and a create info whose pointers outlive the call.
    unsafe { card.device.create_graphics_pipelines(vk::PipelineCache::null(), &[asked], None) }
        .map(|made| made[0])
        .map_err(|(_, error)| error.to_string())
}

/// A buffer the processor writes into directly, for as long as it lives.
pub struct Mapped {
    card: Rc<Card>,
    pub buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    at: *mut u8,
    size: usize,
}

impl Mapped {
    /// Copies `bytes` in at `offset`, as far as they fit.
    pub fn write(&self, offset: usize, bytes: &[u8]) {
        let fits = bytes.len().min(self.size.saturating_sub(offset));
        // SAFETY: the mapping is `size` bytes long and lives as long as
        // `self`; `fits` keeps the copy inside it, and memory the card may be
        // reading is never handed out, because a frame's buffer is written
        // only after its fence says the card is done with it.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.at.add(offset), fits) };
    }
}

impl Drop for Mapped {
    fn drop(&mut self) {
        // SAFETY: nothing in flight uses the buffer: a canvas waits for its
        // frames before letting go of anything.
        unsafe {
            self.card.device.destroy_buffer(self.buffer, None);
            self.card.device.free_memory(self.memory, None);
        }
    }
}

/// What an image handed to the compositor is drawn in: what Wayland calls
/// XRGB8888, which every compositor that takes a dma-buf takes.
pub const OUT_FORMAT: vk::Format = vk::Format::B8G8R8A8_UNORM;

/// An image of the card's that the compositor reads directly.
pub struct Exported {
    card: Rc<Card>,
    pub image: vk::Image,
    memory: vk::DeviceMemory,
    pub view: vk::ImageView,
    /// The memory as a dma-buf, until it has been handed over.
    pub fd: Option<std::os::fd::OwnedFd>,
    /// Where the picture starts in it, and how far apart its rows are.
    pub offset: u64,
    pub stride: u64,
}

impl Exported {
    fn empty(card: &Rc<Card>) -> Exported {
        Exported {
            card: card.clone(),
            image: vk::Image::null(),
            memory: vk::DeviceMemory::null(),
            view: vk::ImageView::null(),
            fd: None,
            offset: 0,
            stride: 0,
        }
    }
}

impl Drop for Exported {
    fn drop(&mut self) {
        // SAFETY: as for `Mapped`; destroying a null handle does nothing.
        unsafe {
            self.card.device.destroy_image_view(self.view, None);
            self.card.device.destroy_image(self.image, None);
            self.card.device.free_memory(self.memory, None);
        }
    }
}

/// A picture on the card.
pub struct Picture {
    card: Rc<Card>,
    image: vk::Image,
    memory: vk::DeviceMemory,
    pub view: vk::ImageView,
}

impl Drop for Picture {
    fn drop(&mut self) {
        // SAFETY: as for `Mapped`.
        unsafe {
            self.card.device.destroy_image_view(self.view, None);
            self.card.device.destroy_image(self.image, None);
            self.card.device.free_memory(self.memory, None);
        }
    }
}

