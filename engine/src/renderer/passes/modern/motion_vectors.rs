use sourcerenderer_core::graphics::{Backend, BarrierAccess, BarrierSync, BindingFrequency,
                                    CommandBuffer, Format, PipelineBinding, TextureInfo,
                                    TextureLayout, TextureStorageView, TextureUsage,
                                    TextureViewInfo, Texture, TextureDimension,
                                    SampleCount};
use sourcerenderer_core::{Platform, Vec2UI};
use crate::renderer::passes::modern::VisibilityBufferPass;
use crate::renderer::renderer_resources::{HistoryResourceEntry, RendererResources};
use crate::renderer::shader_manager::{ShaderManager, ComputePipelineHandle};

pub struct MotionVectorPass {
  pipeline: ComputePipelineHandle
}

impl MotionVectorPass {
  pub const MOTION_TEXTURE_NAME: &'static str = "Motion";

  pub fn new<P: Platform>(resources: &mut RendererResources<P::GraphicsBackend>, renderer_resolution: Vec2UI, shader_manager: &mut ShaderManager<P>) -> Self {
    let pipeline = shader_manager.request_compute_pipeline("shaders/motion_vectors_vis_buf.comp.spv");

    resources.create_texture(
        Self::MOTION_TEXTURE_NAME,
        &TextureInfo {
          dimension: TextureDimension::Dim2D,
          format: Format::RG16Float,
          width: renderer_resolution.x,
          height: renderer_resolution.y,
          depth: 1,
          mip_levels: 1,
          array_length: 1,
          samples: SampleCount::Samples1,
          usage: TextureUsage::SAMPLED | TextureUsage::STORAGE,
          supports_srgb: false,
        },
        false
    );
    Self {
      pipeline
    }
  }

  pub fn execute<P: Platform>(&mut self, cmd_buffer: &mut <P::GraphicsBackend as Backend>::CommandBuffer, resources: &RendererResources<P::GraphicsBackend>, shader_manager: &ShaderManager<P>) {
    let pipeline = shader_manager.get_compute_pipeline(self.pipeline);

    cmd_buffer.begin_label("Motion Vectors");

    let output_srv = resources.access_storage_view(
      cmd_buffer,
      Self::MOTION_TEXTURE_NAME,
      BarrierSync::COMPUTE_SHADER,
      BarrierAccess::STORAGE_WRITE,
      TextureLayout::Storage,
      true,
      &TextureViewInfo::default(),
      HistoryResourceEntry::Current
    );

    let (width, height) = {
      let info = output_srv.texture().info();
      (info.width, info.height)
    };

    let ids = resources.access_storage_view(
       cmd_buffer,
      VisibilityBufferPass::PRIMITIVE_ID_TEXTURE_NAME,
      BarrierSync::COMPUTE_SHADER,
      BarrierAccess::STORAGE_READ,
      TextureLayout::Storage,
      false,
      &TextureViewInfo::default(),
      HistoryResourceEntry::Current
    );

    let barycentrics = resources.access_storage_view(
      cmd_buffer,
      VisibilityBufferPass::BARYCENTRICS_TEXTURE_NAME,
      BarrierSync::COMPUTE_SHADER,
      BarrierAccess::STORAGE_READ,
      TextureLayout::Storage,
      false,
      &TextureViewInfo::default(),
      HistoryResourceEntry::Current
    );

    cmd_buffer.set_pipeline(PipelineBinding::Compute(&pipeline));
    cmd_buffer.bind_storage_texture(BindingFrequency::VeryFrequent, 0, &output_srv);
    cmd_buffer.bind_storage_texture(BindingFrequency::VeryFrequent, 1, &ids);
    cmd_buffer.bind_storage_texture(BindingFrequency::VeryFrequent, 2, &barycentrics);
    cmd_buffer.flush_barriers();
    cmd_buffer.finish_binding();
    cmd_buffer.dispatch((width + 7) / 8, (height + 7) / 8, 1);
    cmd_buffer.end_label();
  }
}
