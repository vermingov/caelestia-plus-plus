//! What the cinema draws with, made on the card as it starts: one pipeline
//! for every kind of mark, and the portrait bound for it to sample.

use std::rc::Rc;

use ash::vk;
use bytemuck::{Pod, Zeroable};

use super::marks::{MOST, Mark};
use crate::card::{Card, Feed, Frame, Picture, Pipe, Spec, pipeline, shader};

pub const SOURCE: &str = include_str!("film.wgsl");

/// The shader's immediates, in the order `film.wgsl` declares them.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Film {
    pub size: [f32; 2],
    pub time: f32,
    pub linearise: f32,
}

pub struct Kit {
    card: Rc<Card>,
    /// What the canvas is made of: cleared to nothing every frame, and seen
    /// through wherever nothing is drawn.
    pub spec: Spec,
    layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    set_layout: vk::DescriptorSetLayout,
    pool: vk::DescriptorPool,
    sampler: vk::Sampler,
    set: vk::DescriptorSet,
    /// Last, so that it goes after the set that reads it.
    portrait: Picture,
}

impl Kit {
    pub fn new(card: &Rc<Card>, portrait: &image::RgbaImage) -> Result<Kit, String> {
        let spec = Spec { name: "cinema", passes: card.passes(true)?, room: (MOST * size_of::<Mark>()) as u64, see_through: true };
        let mut kit = Kit {
            card: card.clone(),
            spec,
            layout: vk::PipelineLayout::null(),
            pipeline: vk::Pipeline::null(),
            set_layout: vk::DescriptorSetLayout::null(),
            pool: vk::DescriptorPool::null(),
            sampler: vk::Sampler::null(),
            set: vk::DescriptorSet::null(),
            portrait: card.picture(portrait)?,
        };
        // Whatever is made before something fails is destroyed by the kit's
        // `Drop`: destroying a null handle is allowed and does nothing.
        kit.fill()?;
        Ok(kit)
    }

    fn fill(&mut self) -> Result<(), String> {
        let card = self.card.clone();
        let device = &card.device;
        let failed = |error: vk::Result| error.to_string();
        // SAFETY: a live device, and create infos whose pointers outlive
        // each call.
        unsafe {
            let sizes = [
                vk::DescriptorPoolSize { ty: vk::DescriptorType::SAMPLED_IMAGE, descriptor_count: 1 },
                vk::DescriptorPoolSize { ty: vk::DescriptorType::SAMPLER, descriptor_count: 1 },
            ];
            self.pool = device.create_descriptor_pool(&vk::DescriptorPoolCreateInfo::default().max_sets(1).pool_sizes(&sizes), None).map_err(failed)?;
            let smooth = vk::SamplerCreateInfo::default()
                .mag_filter(vk::Filter::LINEAR)
                .min_filter(vk::Filter::LINEAR)
                .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE);
            self.sampler = device.create_sampler(&smooth, None).map_err(failed)?;
            let bindings = [(0, vk::DescriptorType::SAMPLED_IMAGE), (1, vk::DescriptorType::SAMPLER)].map(|(binding, kind)| {
                vk::DescriptorSetLayoutBinding::default().binding(binding).descriptor_type(kind).descriptor_count(1).stage_flags(vk::ShaderStageFlags::FRAGMENT)
            });
            self.set_layout = device.create_descriptor_set_layout(&vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings), None).map_err(failed)?;
            let set_layouts = [self.set_layout];
            let immediates = [vk::PushConstantRange::default()
                .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
                .size(size_of::<Film>() as u32)];
            self.layout = device
                .create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default().set_layouts(&set_layouts).push_constant_ranges(&immediates), None)
                .map_err(failed)?;
            self.set = device.allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::default().descriptor_pool(self.pool).set_layouts(&set_layouts)).map_err(failed)?[0];
            let picture = [vk::DescriptorImageInfo::default().image_view(self.portrait.view).image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
            let sampler = [vk::DescriptorImageInfo::default().sampler(self.sampler)];
            let writes = [
                vk::WriteDescriptorSet::default().dst_set(self.set).dst_binding(0).descriptor_type(vk::DescriptorType::SAMPLED_IMAGE).image_info(&picture),
                vk::WriteDescriptorSet::default().dst_set(self.set).dst_binding(1).descriptor_type(vk::DescriptorType::SAMPLER).image_info(&sampler),
            ];
            device.update_descriptor_sets(&writes, &[]);
        }

        let module = shader(device, SOURCE)?;
        let vec4s = (size_of::<Mark>() / 16) as u32;
        let pipe = Pipe { layout: self.layout, module, vertex: c"place", fragment: c"shade", feed: Feed::Instances { vec4s }, blend: true };
        let made = pipeline(&card, self.spec.passes.presented, &pipe);
        // SAFETY: the module is only needed while the pipeline is made.
        unsafe { device.destroy_shader_module(module, None) };
        self.pipeline = made?;
        Ok(())
    }

    /// Records the frame: every mark `frame` holds, `count` of them, in one
    /// draw.
    pub fn record(&self, card: &Card, frame: &Frame<()>, film: &Film, count: u32) {
        let commands = frame.commands;
        // SAFETY: commands being recorded on this card, inside its render
        // pass, with a buffer and a set that live as long as the frame does.
        unsafe {
            card.device.cmd_bind_pipeline(commands, vk::PipelineBindPoint::GRAPHICS, self.pipeline);
            let stages = vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT;
            card.device.cmd_push_constants(commands, self.layout, stages, 0, bytemuck::bytes_of(film));
            card.device.cmd_bind_descriptor_sets(commands, vk::PipelineBindPoint::GRAPHICS, self.layout, 0, &[self.set], &[]);
            card.device.cmd_bind_vertex_buffers(commands, 0, &[frame.data.buffer], &[0]);
            card.device.cmd_draw(commands, 4, count, 0, 0);
        }
    }
}

impl Drop for Kit {
    fn drop(&mut self) {
        let device = &self.card.device;
        // SAFETY: the wait leaves nothing in flight that uses what follows,
        // and nothing draws with the kit once it is going; the set goes with
        // its pool.
        unsafe {
            let _ = device.device_wait_idle();
            device.destroy_pipeline(self.pipeline, None);
            device.destroy_pipeline_layout(self.layout, None);
            device.destroy_descriptor_set_layout(self.set_layout, None);
            device.destroy_descriptor_pool(self.pool, None);
            device.destroy_sampler(self.sampler, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shader is compiled as the cinema starts, on a machine that is
    /// playing it; a mistake in it should fail here instead.
    #[test]
    fn the_shader_compiles() {
        let module = naga::front::wgsl::parse_str(SOURCE).unwrap_or_else(|error| panic!("{}", error.emit_to_string(SOURCE)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::IMMEDIATES)
            .validate(&module)
            .unwrap_or_else(|error| panic!("{}", error.emit_to_string(SOURCE)));
    }

    #[test]
    fn a_mark_is_what_the_shader_reads() {
        // Six vectors of four floats at locations nought to five.
        assert_eq!(size_of::<Mark>(), 6 * 16);
        assert_eq!(size_of::<Film>(), 16);
    }
}
