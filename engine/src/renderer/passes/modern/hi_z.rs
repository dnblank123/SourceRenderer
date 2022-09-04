use std::sync::Arc;

use nalgebra_glm::Vec2;
use smallvec::SmallVec;
use sourcerenderer_core::{graphics::{Backend, TextureUsage, Format, Device, ShaderType, CommandBuffer, PipelineBinding, BarrierSync, BarrierAccess, TextureLayout, TextureViewInfo, BindingFrequency, SamplerInfo, Filter, AddressMode, BufferInfo, BufferUsage, MemoryUsage, WHOLE_BUFFER}, Platform};

use crate::renderer::{renderer_resources::{RendererResources, HistoryResourceEntry}, shader_manager::{PipelineHandle, ShaderManager}};

pub struct HierarchicalZPass<P: Platform> {
  ffx_pipeline: PipelineHandle,
  copy_pipeline: PipelineHandle,
  sampler: Arc<<P::GraphicsBackend as Backend>::Sampler>,
  device: Arc<<P::GraphicsBackend as Backend>::Device>,
}

impl<P: Platform> HierarchicalZPass<P> {
  pub const HI_Z_BUFFER_NAME: &'static str = "Hierarchical Z Buffer";
  const FFX_COUNTER_BUFFER_NAME: &'static str = "FFX Downscaling Counter Buffer";

  pub fn new(device: &Arc<<P::GraphicsBackend as Backend>::Device>, resources: &mut RendererResources<P::GraphicsBackend>, shader_manager: &mut ShaderManager<P>, init_cmd_buffer: &mut <P::GraphicsBackend as Backend>::CommandBuffer, depth_name: &str) -> Self {
    let mut texture_info = resources.texture_info(depth_name).clone();
    let size = texture_info.width.max(texture_info.height) as f32;
    texture_info.mip_levels = (size.log(2f32).ceil() as u32).max(1);
    texture_info.usage = TextureUsage::STORAGE | TextureUsage::SAMPLED;
    texture_info.format = Format::R32Float;

    resources.create_texture(Self::HI_Z_BUFFER_NAME, &texture_info, false);

    assert!(device.supports_min_max_filter()); // TODO: Implement variant that doesn't rely on min-max filter. PLS JUST ADD IT TO METAL @APPLE

    let ffx_pipeline = shader_manager.request_compute_pipeline("shaders/ffx_downsampler.comp.spv");
    let copy_pipeline = shader_manager.request_compute_pipeline("shaders/hi_z_copy.comp.spv");

    let sampler = if device.supports_min_max_filter() {
      device.create_sampler(&SamplerInfo {
        mag_filter: Filter::Linear,
        min_filter: Filter::Max,
        mip_filter: Filter::Linear,
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        address_mode_w: AddressMode::ClampToEdge,
        mip_bias: 0f32,
        max_anisotropy: 0f32,
        compare_op: None,
        min_lod: 0f32,
        max_lod: None,
    })
    } else {
      resources.nearest_sampler().clone()
    };

    resources.create_buffer(Self::FFX_COUNTER_BUFFER_NAME, &BufferInfo {
      size: 4,
      usage: BufferUsage::STORAGE,
    }, MemoryUsage::VRAM, false);

    {
      // Initial clear
      let counter_buffer = resources.access_buffer(
        init_cmd_buffer,
        Self::FFX_COUNTER_BUFFER_NAME,
        BarrierSync::COMPUTE_SHADER,
        BarrierAccess::STORAGE_WRITE,
        HistoryResourceEntry::Current
      );
      init_cmd_buffer.flush_barriers();
      init_cmd_buffer.clear_storage_buffer(&counter_buffer, 0, 4, 0);
    }

    Self {
      copy_pipeline,
      ffx_pipeline,
      sampler,
      device: device.clone(),
    }
  }

  pub fn execute(
    &mut self,
    cmd_buffer: &mut <P::GraphicsBackend as Backend>::CommandBuffer,
    resources: &RendererResources<P::GraphicsBackend>,
    shader_manager: &ShaderManager<P>,
    depth_name: &str
  ) {
    let (width, height, mips) = {
      let info = resources.texture_info(Self::HI_Z_BUFFER_NAME);
      (info.width, info.height, info.mip_levels)
    };

    assert!(mips <= 13); // TODO support >8k?

    cmd_buffer.begin_label("Hierarchical Z");
    let src_texture = resources.access_sampling_view(
      cmd_buffer,
      depth_name,
      BarrierSync::COMPUTE_SHADER,
      BarrierAccess::SAMPLING_READ,
      TextureLayout::Sampled,
      false,
      &TextureViewInfo::default(),
      HistoryResourceEntry::Past
    );
    let dst_mip0 = resources.access_storage_view(
      cmd_buffer,
      Self::HI_Z_BUFFER_NAME,
      BarrierSync::COMPUTE_SHADER,
      BarrierAccess::STORAGE_WRITE,
      TextureLayout::Storage,
      true,
      &TextureViewInfo::default(), HistoryResourceEntry::Current
    ).clone();
    let copy_pipeline = shader_manager.get_compute_pipeline(self.copy_pipeline);
    cmd_buffer.set_pipeline(PipelineBinding::Compute(&copy_pipeline));
    cmd_buffer.bind_sampling_view_and_sampler(BindingFrequency::VeryFrequent, 0, &src_texture, resources.nearest_sampler());
    cmd_buffer.bind_storage_texture(BindingFrequency::VeryFrequent, 1, &dst_mip0);
    cmd_buffer.flush_barriers();
    cmd_buffer.finish_binding();
    cmd_buffer.dispatch((width + 7) / 8, (height + 7) / 8, 1);

    let counter_buffer = resources.access_buffer(
      cmd_buffer,
      Self::FFX_COUNTER_BUFFER_NAME,
      BarrierSync::COMPUTE_SHADER,
      BarrierAccess::STORAGE_READ | BarrierAccess::STORAGE_WRITE,
      HistoryResourceEntry::Current
    );
    let mut dst_texture_views = SmallVec::<[Arc<<P::GraphicsBackend as Backend>::TextureStorageView>; 12]>::new();
    for i in 1..mips {
      dst_texture_views.push(resources.access_storage_view(
        cmd_buffer,
        Self::HI_Z_BUFFER_NAME,
        BarrierSync::COMPUTE_SHADER,
        BarrierAccess::STORAGE_WRITE,
        TextureLayout::Storage,
        true,
        &TextureViewInfo {
          base_mip_level: i,
          mip_level_length: 1,
          base_array_layer: 0,
          array_layer_length: 1,
          format: None,
        }, HistoryResourceEntry::Current
      ).clone());
    }
    let mut texture_refs = SmallVec::<[&Arc<<P::GraphicsBackend as Backend>::TextureStorageView>; 12]>::new();
    for i in 0 .. (mips - 1) as usize {
      texture_refs.push(&dst_texture_views[i]);
    }
    for _ in (mips - 1) .. 12 {
      texture_refs.push(&dst_texture_views[0]); // fill the rest of the array with views that never get used, so the validation layers shut up
    }

    let ffx_pipeline = shader_manager.get_compute_pipeline(self.ffx_pipeline);
    cmd_buffer.set_pipeline(PipelineBinding::Compute(&ffx_pipeline));
    cmd_buffer.bind_sampling_view_and_sampler(BindingFrequency::VeryFrequent, 0, &src_texture, &self.sampler);
    cmd_buffer.bind_storage_view_array(BindingFrequency::VeryFrequent, 1, &texture_refs);
    cmd_buffer.bind_storage_buffer(BindingFrequency::VeryFrequent, 2, &counter_buffer, 0, WHOLE_BUFFER);

    #[repr(C)]
    #[derive(Clone, Debug)]
    struct SpdConstants {
      mips: u32,
      num_work_groups: u32,
      work_group_offset: Vec2
    }
    let work_groups_x = (width + 63) >> 6;
    let work_groups_y = (height + 63) >> 6;
    cmd_buffer.upload_dynamic_data_inline(&[SpdConstants {
      mips: mips - 1,
      num_work_groups: work_groups_x * work_groups_y,
      work_group_offset: Vec2::new(0f32, 0f32)
    }], ShaderType::ComputeShader);

    cmd_buffer.flush_barriers();
    cmd_buffer.finish_binding();
    cmd_buffer.dispatch(work_groups_x, work_groups_y, 1);
    cmd_buffer.end_label();
  }
}
