//! What the background draws with, made once on the card: its render
//! passes, its three pipelines, and what a picture is sampled with.

use std::rc::Rc;

use ash::vk;
use bytemuck::{Pod, Zeroable};

use super::helix::{MOST, Shape};
use crate::card::{Card, Feed, Pipe, Spec, pipeline, shader};

/// What the two shaders are compiled from, once, when the card is chosen.
const SCENE: &str = include_str!("scene.wgsl");
const PICTURE: &str = include_str!("picture.wgsl");

/// Where the shapes start in a frame's buffer: after the draw's own count,
/// which the card reads from the front of it.
pub const SHAPES_AT: usize = 64;

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

pub struct Kit {
    card: Rc<Card>,
    /// What a screen's canvas is made of: the backdrop covers every pixel,
    /// so nothing is cleared and nothing is seen through.
    pub spec: Spec,
    pub scene_layout: vk::PipelineLayout,
    pub backdrop: vk::Pipeline,
    pub shapes: vk::Pipeline,
    picture_set_layout: vk::DescriptorSetLayout,
    pub picture_layout: vk::PipelineLayout,
    pub picture: vk::Pipeline,
    sampler: vk::Sampler,
    descriptors: vk::DescriptorPool,
}

impl Kit {
    pub fn new(card: &Rc<Card>) -> Result<Kit, String> {
        let spec = Spec { name: "background", passes: card.passes(false)?, room: (SHAPES_AT + MOST * size_of::<Shape>()) as u64, see_through: false };
        let mut kit = Kit {
            card: card.clone(),
            spec,
            scene_layout: vk::PipelineLayout::null(),
            backdrop: vk::Pipeline::null(),
            shapes: vk::Pipeline::null(),
            picture_set_layout: vk::DescriptorSetLayout::null(),
            picture_layout: vk::PipelineLayout::null(),
            picture: vk::Pipeline::null(),
            sampler: vk::Sampler::null(),
            descriptors: vk::DescriptorPool::null(),
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
                vk::DescriptorPoolSize { ty: vk::DescriptorType::SAMPLED_IMAGE, descriptor_count: 16 },
                vk::DescriptorPoolSize { ty: vk::DescriptorType::SAMPLER, descriptor_count: 8 },
            ];
            self.descriptors = device
                .create_descriptor_pool(&vk::DescriptorPoolCreateInfo::default().max_sets(8).pool_sizes(&sizes).flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET), None)
                .map_err(failed)?;
            self.sampler = device
                .create_sampler(&vk::SamplerCreateInfo::default().mag_filter(vk::Filter::LINEAR).min_filter(vk::Filter::LINEAR), None)
                .map_err(failed)?;
            let immediates = |size: usize, stages| [vk::PushConstantRange::default().stage_flags(stages).offset(0).size(size as u32)];
            let scene_immediates = immediates(size_of::<Scene>(), vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT);
            self.scene_layout = device
                .create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default().push_constant_ranges(&scene_immediates), None)
                .map_err(failed)?;
            let bindings = [(0, vk::DescriptorType::SAMPLED_IMAGE), (1, vk::DescriptorType::SAMPLED_IMAGE), (2, vk::DescriptorType::SAMPLER)].map(|(binding, kind)| {
                vk::DescriptorSetLayoutBinding::default().binding(binding).descriptor_type(kind).descriptor_count(1).stage_flags(vk::ShaderStageFlags::FRAGMENT)
            });
            self.picture_set_layout = device
                .create_descriptor_set_layout(&vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings), None)
                .map_err(failed)?;
            let picture_immediates = immediates(size_of::<Change>(), vk::ShaderStageFlags::FRAGMENT);
            let set_layouts = [self.picture_set_layout];
            self.picture_layout = device
                .create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default().set_layouts(&set_layouts).push_constant_ranges(&picture_immediates), None)
                .map_err(failed)?;
        }

        let scene = shader(device, SCENE)?;
        let picture = shader(device, PICTURE)?;
        let pass = self.spec.passes.presented;
        let made = (|| -> Result<(), String> {
            let pipe = |layout, module, (vertex, fragment), feed, blend| Pipe { layout, module, vertex, fragment, feed, blend };
            let shape_vec4s = (size_of::<Shape>() / 16) as u32;
            self.backdrop = pipeline(&card, pass, &pipe(self.scene_layout, scene, (c"cover", c"backdrop"), Feed::Screen, false))?;
            self.shapes = pipeline(&card, pass, &pipe(self.scene_layout, scene, (c"outline", c"shade"), Feed::Instances { vec4s: shape_vec4s }, true))?;
            self.picture = pipeline(&card, pass, &pipe(self.picture_layout, picture, (c"cover", c"paint"), Feed::Screen, false))?;
            Ok(())
        })();
        // SAFETY: modules are only needed while pipelines are made.
        unsafe {
            device.destroy_shader_module(scene, None);
            device.destroy_shader_module(picture, None);
        }
        made
    }

    /// A set binding two pictures for the picture pipeline: the one taking
    /// over and the one it takes over from.
    pub fn bind_pictures(&self, before: vk::ImageView, after: vk::ImageView) -> Result<vk::DescriptorSet, String> {
        let layouts = [self.picture_set_layout];
        let asked = vk::DescriptorSetAllocateInfo::default().descriptor_pool(self.descriptors).set_layouts(&layouts);
        // SAFETY: a live device and pool; the set is freed by `free_set`.
        let set = unsafe { self.card.device.allocate_descriptor_sets(&asked) }.map_err(|error| error.to_string())?[0];
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
        // SAFETY: a set of this kit's and views that are alive; a set is only
        // rewritten once nothing in flight reads it.
        unsafe { self.card.device.update_descriptor_sets(&writes, &[]) };
    }

    pub fn free_set(&self, set: vk::DescriptorSet) {
        // SAFETY: a set from this pool that nothing in flight uses.
        let _ = unsafe { self.card.device.free_descriptor_sets(self.descriptors, &[set]) };
    }
}

impl Drop for Kit {
    fn drop(&mut self) {
        let device = &self.card.device;
        // SAFETY: the wait leaves nothing in flight that uses what follows,
        // and nothing draws with the kit once it is going.
        unsafe {
            let _ = device.device_wait_idle();
            for pipeline in [self.backdrop, self.shapes, self.picture] {
                device.destroy_pipeline(pipeline, None);
            }
            device.destroy_pipeline_layout(self.scene_layout, None);
            device.destroy_pipeline_layout(self.picture_layout, None);
            device.destroy_descriptor_set_layout(self.picture_set_layout, None);
            device.destroy_sampler(self.sampler, None);
            device.destroy_descriptor_pool(self.descriptors, None);
        }
    }
}
