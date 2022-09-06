use std::sync::Arc;

use sourcerenderer_core::{graphics::{Backend, TextureInfo, Format, SampleCount, TextureUsage, TextureViewInfo, CommandBuffer, BindingFrequency, PipelineBinding, TextureStorageView, Texture, BarrierSync, TextureLayout, BarrierAccess, TextureDimension}, Vec2UI, Platform};

use crate::renderer::{renderer_resources::{HistoryResourceEntry, RendererResources}, shader_manager::{RayTracingPipelineHandle, ShaderManager, RayTracingPipelineInfo}};

pub struct RTShadowPass {
  pipeline: RayTracingPipelineHandle,
}

impl RTShadowPass {
  pub const SHADOWS_TEXTURE_NAME: &'static str = "RTShadow";

  pub fn new<P: Platform>(resolution: Vec2UI, resources: &mut RendererResources<P::GraphicsBackend>, shader_manager: &mut ShaderManager<P>) -> Self {
    resources.create_texture(Self::SHADOWS_TEXTURE_NAME, &TextureInfo {
      dimension: TextureDimension::Dim2D,
      format: Format::RGBA8UNorm,
      width: resolution.x,
      height: resolution.y,
      depth: 1,
      mip_levels: 1,
      array_length: 1,
      samples: SampleCount::Samples1,
      usage: TextureUsage::STORAGE | TextureUsage::SAMPLED,
      supports_srgb: false,
    }, false);

    let pipeline = shader_manager.request_ray_tracing_pipeline(&RayTracingPipelineInfo {
      ray_gen_shader: "shaders/shadows.rgen.spv",
      closest_hit_shaders: &["shaders/shadows.rchit.spv"],
      miss_shaders: &["shaders/shadows.rmiss.spv"],
    });

    Self {
      pipeline,
    }
  }

  pub fn execute<P: Platform>(
    &mut self,
    cmd_buffer: &mut <P::GraphicsBackend as Backend>::CommandBuffer,
    resources: &RendererResources<P::GraphicsBackend>,
    shader_manager: &ShaderManager<P>,
    depth_name: &str,
    acceleration_structure: &Arc<<P::GraphicsBackend as Backend>::AccelerationStructure>,
    blue_noise: &Arc<<P::GraphicsBackend as Backend>::TextureSamplingView>,
    blue_noise_sampler: &Arc<<P::GraphicsBackend as Backend>::Sampler>) {
    let texture_uav = resources.access_storage_view(
      cmd_buffer,
      Self::SHADOWS_TEXTURE_NAME,
      BarrierSync::COMPUTE_SHADER | BarrierSync::RAY_TRACING,
      BarrierAccess::STORAGE_WRITE,
      TextureLayout::Storage,
      true,
      &TextureViewInfo::default(),
      HistoryResourceEntry::Current
    );

    let depth = resources.access_sampling_view(
      cmd_buffer,
      depth_name,
      BarrierSync::RAY_TRACING | BarrierSync::COMPUTE_SHADER,
      BarrierAccess::SAMPLING_READ,
      TextureLayout::Sampled,
      false,
      &TextureViewInfo::default(),
      HistoryResourceEntry::Current
    );

    let pipeline = shader_manager.get_ray_tracing_pipeline(self.pipeline);
    cmd_buffer.set_pipeline(PipelineBinding::RayTracing(&pipeline));
    cmd_buffer.bind_acceleration_structure(BindingFrequency::Frequent, 0, acceleration_structure);
    cmd_buffer.bind_storage_texture(BindingFrequency::Frequent, 1, &*texture_uav);
    cmd_buffer.bind_sampling_view_and_sampler(BindingFrequency::Frequent, 2, &*depth, resources.linear_sampler());
    cmd_buffer.bind_sampling_view_and_sampler(BindingFrequency::Frequent, 3, blue_noise, blue_noise_sampler);
    let info = texture_uav.texture().info();

    cmd_buffer.flush_barriers();
    cmd_buffer.finish_binding();
    cmd_buffer.trace_ray(info.width, info.height, 1);
  }
}
