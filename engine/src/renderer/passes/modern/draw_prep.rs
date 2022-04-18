use std::{sync::Arc, path::Path, io::Read};

use sourcerenderer_core::{graphics::{Backend, Device, ShaderType, BufferInfo, BufferUsage, MemoryUsage, BarrierSync, BarrierAccess, CommandBuffer, BindingFrequency, WHOLE_BUFFER, PipelineBinding}, Platform, platform::io::IO};

use crate::renderer::{renderer_resources::{RendererResources, HistoryResourceEntry}, renderer_scene::RendererScene, passes::modern::gpu_scene::{PART_CAPACITY, DRAWABLE_CAPACITY}};

pub struct DrawPrepPass<B: Backend> {
  culling_pipeline: Arc<B::ComputePipeline>,
  prep_pipeline: Arc<B::ComputePipeline>
}

impl<B: Backend> DrawPrepPass<B> {
  pub const VISIBLE_DRAWABLES_BITFIELD_BUFFER: &'static str = "VisibleDrawables";
  pub const INDIRECT_DRAW_BUFFER: &'static str = "IndirectDraws";

  pub fn new<P: Platform>(device: &Arc<B::Device>, resources: &mut RendererResources<B>) -> Self {
    let culling_shader = {
      let mut file = <P::IO as IO>::open_asset(Path::new("shaders").join(Path::new("culling.comp.spv"))).unwrap();
      let mut bytes: Vec<u8> = Vec::new();
      file.read_to_end(&mut bytes).unwrap();
      device.create_shader(ShaderType::ComputeShader, &bytes, Some("culling.comp.spv"))
    };
    let culling_pipeline = device.create_compute_pipeline(&culling_shader, Some("Culling"));
    let prep_shader = {
      let mut file = <P::IO as IO>::open_asset(Path::new("shaders").join(Path::new("draw_prep.comp.spv"))).unwrap();
      let mut bytes: Vec<u8> = Vec::new();
      file.read_to_end(&mut bytes).unwrap();
      device.create_shader(ShaderType::ComputeShader, &bytes, Some("draw_prep.comp.spv"))
    };
    let prep_pipeline = device.create_compute_pipeline(&prep_shader, Some("DrawPrep"));
    resources.create_buffer(Self::VISIBLE_DRAWABLES_BITFIELD_BUFFER, &BufferInfo {
      size: (DRAWABLE_CAPACITY as usize + std::mem::size_of::<u32>() - 1) / std::mem::size_of::<u32>(),
      usage: BufferUsage::STORAGE
    }, MemoryUsage::GpuOnly, false);
    resources.create_buffer(Self::INDIRECT_DRAW_BUFFER, &BufferInfo {
      size: 4 + 20 * PART_CAPACITY as usize,
      usage: BufferUsage::STORAGE | BufferUsage::INDIRECT
    }, MemoryUsage::GpuOnly, false);
    Self {
      culling_pipeline,
      prep_pipeline
    }
  }

  pub fn execute(
    &self,
    cmd_buffer: &mut B::CommandBuffer,
    resources: &RendererResources<B>,
    scene: &RendererScene<B>,
    scene_buffer: &Arc<B::Buffer>
  ) {
    {
      let buffer = resources.access_buffer(
        cmd_buffer,
        Self::VISIBLE_DRAWABLES_BITFIELD_BUFFER,
        BarrierSync::COMPUTE_SHADER,
        BarrierAccess::STORAGE_WRITE,
        HistoryResourceEntry::Current
      );
      cmd_buffer.bind_storage_buffer(BindingFrequency::PerDraw, 0, scene_buffer, 0, WHOLE_BUFFER);
      cmd_buffer.bind_storage_buffer(BindingFrequency::PerDraw, 1, &*buffer, 0, WHOLE_BUFFER);
      cmd_buffer.set_pipeline(PipelineBinding::Compute(&self.culling_pipeline));
      cmd_buffer.flush_barriers();
      cmd_buffer.finish_binding();
      cmd_buffer.dispatch((scene.static_drawables().len() as u32 + 63) / 64, 1, 1);
    }

    assert!(scene.static_drawables().len() as u32 <= DRAWABLE_CAPACITY);
    let part_count = scene.static_drawables().iter().map(|d| d.model.mesh().parts.len()).fold(0, |a, b| a + b) as u32;
    assert!(part_count <= PART_CAPACITY);

    let visibility_buffer = resources.access_buffer(
      cmd_buffer,
      Self::VISIBLE_DRAWABLES_BITFIELD_BUFFER,
      BarrierSync::COMPUTE_SHADER,
      BarrierAccess::STORAGE_READ,
      HistoryResourceEntry::Current
    );
    let draw_buffer = resources.access_buffer(
      cmd_buffer,
      Self::INDIRECT_DRAW_BUFFER,
      BarrierSync::COMPUTE_SHADER,
      BarrierAccess::STORAGE_WRITE,
      HistoryResourceEntry::Current
    );
    cmd_buffer.bind_storage_buffer(BindingFrequency::PerDraw, 0, scene_buffer, 0, WHOLE_BUFFER);
    cmd_buffer.bind_storage_buffer(BindingFrequency::PerDraw, 1, &*visibility_buffer, 0, WHOLE_BUFFER);
    cmd_buffer.bind_storage_buffer(BindingFrequency::PerDraw, 2, &*draw_buffer, 0, WHOLE_BUFFER);
    cmd_buffer.set_pipeline(PipelineBinding::Compute(&self.prep_pipeline));
    cmd_buffer.flush_barriers();
    cmd_buffer.finish_binding();
    cmd_buffer.dispatch((part_count + 63) / 64, 1, 1);
  }
}
