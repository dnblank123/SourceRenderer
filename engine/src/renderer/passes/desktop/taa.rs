use sourcerenderer_core::{Vec2, graphics::{AddressMode, Backend as GraphicsBackend, Barrier, BindingFrequency, CommandBuffer, Device, Filter, Format, InputUsage, Output, PassInfo, PassInput, PassType, PipelineBinding, PipelineStage, RenderPassCallbacks, RenderPassTextureExtent, SampleCount, SamplerInfo, ShaderType, Swapchain, Texture, TextureInfo, TextureShaderResourceView, TextureShaderResourceViewInfo, TextureUnorderedAccessViewInfo, TextureUsage}};
use sourcerenderer_core::Platform;
use std::sync::Arc;
use std::path::Path;
use std::io::Read;
use sourcerenderer_core::platform::io::IO;
use crate::renderer::passes::desktop::{geometry::OUTPUT_IMAGE, prepass::OUTPUT_MOTION};

const PASS_NAME: &str = "TAA";
pub(crate) const HISTORY_BUFFER_NAME: &str = "TAA_buffer";

pub(crate) fn build_pass_template<B: GraphicsBackend>() -> PassInfo {
  PassInfo {
    name: PASS_NAME.to_string(),
    pass_type: PassType::Compute {
      inputs: vec![
        PassInput {
          name: OUTPUT_IMAGE.to_string(),
          stage: PipelineStage::ComputeShader,
          usage: InputUsage::Sampled,
          is_history: false,
        },
        PassInput {
          name: HISTORY_BUFFER_NAME.to_string(),
          stage: PipelineStage::ComputeShader,
          usage: InputUsage::Sampled,
          is_history: true,
        },
        PassInput {
          name: OUTPUT_MOTION.to_string(),
          stage: PipelineStage::ComputeShader,
          usage: InputUsage::Sampled,
          is_history: false
        }
      ],
      outputs: vec![
        Output::RenderTarget {
          name: HISTORY_BUFFER_NAME.to_string(),
          format: Format::RGBA8,
          samples: sourcerenderer_core::graphics::SampleCount::Samples1,
          extent: RenderPassTextureExtent::RelativeToSwapchain {
            width: 1.0f32,
            height: 1.0f32,
          },
          depth: 1,
          levels: 1,
          external: false,
          clear: false
        }
      ]
    }
  }
}

pub(crate) fn build_pass<P: Platform>(device: &Arc<<P::GraphicsBackend as GraphicsBackend>::Device>) -> (String, RenderPassCallbacks<P::GraphicsBackend>) {
  let taa_compute_shader = {
    let mut file = <P::IO as IO>::open_asset(Path::new("shaders").join(Path::new("taa.comp.spv"))).unwrap();
    let mut bytes: Vec<u8> = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    device.create_shader(ShaderType::ComputeShader, &bytes, Some("taa.comp.spv"))
  };

  let linear_sampler = device.create_sampler(&SamplerInfo {
    mag_filter: Filter::Linear,
    min_filter: Filter::Linear,
    mip_filter: Filter::Linear,
    address_mode_u: AddressMode::Repeat,
    address_mode_v: AddressMode::Repeat,
    address_mode_w: AddressMode::Repeat,
    mip_bias: 0.0,
    max_anisotropy: 0.0,
    compare_op: None,
    min_lod: 0.0,
    max_lod: 1.0,
  });

  let nearest_sampler = device.create_sampler(&SamplerInfo {
    mag_filter: Filter::Linear,
    min_filter: Filter::Linear,
    mip_filter: Filter::Linear,
    address_mode_u: AddressMode::Repeat,
    address_mode_v: AddressMode::Repeat,
    address_mode_w: AddressMode::Repeat,
    mip_bias: 0.0,
    max_anisotropy: 0.0,
    compare_op: None,
    min_lod: 0.0,
    max_lod: 1.0,
  });

  let taa_pipeline = device.create_compute_pipeline(&taa_compute_shader);
  (PASS_NAME.to_string(), RenderPassCallbacks::Regular(
    vec![
      Arc::new(move |command_buffer_a, graph_resources, _frame_counter| {
        let command_buffer = command_buffer_a as &mut <P::GraphicsBackend as GraphicsBackend>::CommandBuffer;
        command_buffer.set_pipeline(PipelineBinding::Compute(&taa_pipeline));
        command_buffer.bind_texture_view(BindingFrequency::PerDraw, 0, graph_resources.get_texture_srv(OUTPUT_IMAGE, false).expect("Failed to get graph resource"), &linear_sampler);
        command_buffer.bind_texture_view(BindingFrequency::PerDraw, 1, graph_resources.get_texture_srv(HISTORY_BUFFER_NAME, true).expect("Failed to get graph resource"), &linear_sampler);
        command_buffer.bind_storage_texture(BindingFrequency::PerDraw, 2, graph_resources.get_texture_uav(HISTORY_BUFFER_NAME, false).expect("Failed to get graph resource"));
        command_buffer.bind_texture_view(BindingFrequency::PerDraw, 3, graph_resources.get_texture_srv(OUTPUT_MOTION, false).expect("Failed to get graph resource"), &nearest_sampler);
        command_buffer.finish_binding();

        let dimensions = graph_resources.texture_dimensions(OUTPUT_IMAGE).unwrap();
        command_buffer.dispatch(dimensions.width, dimensions.height, 1);
      })
    ]
  ))
}

pub(crate) fn scaled_halton_point(width: u32, height: u32, index: u32) -> Vec2 {
  let width_frac = 1.0f32 / width as f32;
  let height_frac = 1.0f32 / height as f32;
  let mut halton_point = halton_point(index);
  halton_point.x *= width_frac;
  halton_point.y *= height_frac;
  halton_point
}

pub(crate) fn halton_point(index: u32) -> Vec2 {
  Vec2::new(
    halton_sequence(index, 2) * 2f32 - 1f32, halton_sequence(index, 3) * 2f32 - 1f32
  )
}

pub(crate) fn halton_sequence(mut index: u32, base: u32) -> f32 {
  let mut f = 1.0f32;
  let mut r = 0.0f32;

  while index > 0 {
    f = f / (base as f32);
    r += f * (index as f32 % (base as f32));
    index = (index as f32 / (base as f32)).floor() as u32;
  }

  return r;
}


// =============================================

pub struct TAAPass<B: GraphicsBackend> {
  taa_texture: Arc<B::Texture>,
  taa_texture_b: Arc<B::Texture>,
  taa_srv: Arc<B::TextureShaderResourceView>,
  taa_srv_b: Arc<B::TextureShaderResourceView>,
  taa_uav: Arc<B::TextureUnorderedAccessView>,
  taa_uav_b: Arc<B::TextureUnorderedAccessView>,
  pipeline: Arc<B::ComputePipeline>,
  nearest_sampler: Arc<B::Sampler>,
  linear_sampler: Arc<B::Sampler>
}

impl<B: GraphicsBackend> TAAPass<B> {
  pub fn new<P: Platform>(device: &Arc<B::Device>, swapchain: &Arc<B::Swapchain>, init_cmd_buffer: &mut B::CommandBuffer) -> Self {
    let taa_compute_shader = {
      let mut file = <P::IO as IO>::open_asset(Path::new("shaders").join(Path::new("taa.comp.spv"))).unwrap();
      let mut bytes: Vec<u8> = Vec::new();
      file.read_to_end(&mut bytes).unwrap();
      device.create_shader(ShaderType::ComputeShader, &bytes, Some("taa.comp.spv"))
    };
    let pipeline = device.create_compute_pipeline(&taa_compute_shader);
  
    let linear_sampler = device.create_sampler(&SamplerInfo {
      mag_filter: Filter::Linear,
      min_filter: Filter::Linear,
      mip_filter: Filter::Linear,
      address_mode_u: AddressMode::Repeat,
      address_mode_v: AddressMode::Repeat,
      address_mode_w: AddressMode::Repeat,
      mip_bias: 0.0,
      max_anisotropy: 0.0,
      compare_op: None,
      min_lod: 0.0,
      max_lod: 1.0,
    });
  
    let nearest_sampler = device.create_sampler(&SamplerInfo {
      mag_filter: Filter::Linear,
      min_filter: Filter::Linear,
      mip_filter: Filter::Linear,
      address_mode_u: AddressMode::Repeat,
      address_mode_v: AddressMode::Repeat,
      address_mode_w: AddressMode::Repeat,
      mip_bias: 0.0,
      max_anisotropy: 0.0,
      compare_op: None,
      min_lod: 0.0,
      max_lod: 1.0,
    });

    let texture_info = TextureInfo {
      format: Format::RGBA8,
      width: swapchain.width(),
      height: swapchain.height(),
      depth: 1,
      mip_levels: 1,
      array_length: 1,
      samples: SampleCount::Samples1,
      usage: TextureUsage::COMPUTE_SHADER_SAMPLED | TextureUsage::COMPUTE_SHADER_STORAGE_WRITE,
    };
    let taa_texture = device.create_texture(&texture_info, Some("TAAOutput"));
    let taa_texture_b = device.create_texture(&texture_info, Some("TAAOutput_b"));

    let srv_info = TextureShaderResourceViewInfo {
      base_mip_level: 0,
      mip_level_length: 1,
      base_array_level: 0,
      array_level_length: 1,
    };
    let taa_srv = device.create_shader_resource_view(&taa_texture, &srv_info);
    let taa_srv_b = device.create_shader_resource_view(&taa_texture_b, &srv_info);

    let uav_info = TextureUnorderedAccessViewInfo {
      base_mip_level: 0,
      mip_level_length: 1,
      base_array_level: 0,
      array_level_length: 1,
    };
    let taa_uav = device.create_unordered_access_view(&taa_texture, &uav_info);
    let taa_uav_b = device.create_unordered_access_view(&taa_texture_b, &uav_info);

    init_cmd_buffer.barrier(&[
      Barrier::TextureBarrier {
        old_primary_usage: TextureUsage::UNINITIALIZED,
        new_primary_usage: TextureUsage::COMPUTE_SHADER_SAMPLED,
        old_usages: TextureUsage::empty(),
        new_usages: TextureUsage::empty(),
        texture: &taa_texture,
      },
      Barrier::TextureBarrier {
        old_primary_usage: TextureUsage::UNINITIALIZED,
        new_primary_usage: TextureUsage::COMPUTE_SHADER_SAMPLED,
        old_usages: TextureUsage::empty(),
        new_usages: TextureUsage::empty(),
        texture: &taa_texture_b,
      }
    ]);

    Self {
      pipeline,
      taa_texture,
      taa_texture_b,
      taa_srv,
      taa_srv_b,
      taa_uav,
      taa_uav_b,
      linear_sampler,
      nearest_sampler
    }
  }

  pub fn execute(
    &mut self,
    cmd_buf: &mut B::CommandBuffer,
    output_srv: &Arc<B::TextureShaderResourceView>,
    motion_srv: &Arc<B::TextureShaderResourceView>
  ) {
    cmd_buf.barrier(&[
      Barrier::TextureBarrier {
        old_primary_usage: TextureUsage::RENDER_TARGET,
        new_primary_usage: TextureUsage::COMPUTE_SHADER_SAMPLED,
        old_usages: TextureUsage::RENDER_TARGET,
        new_usages: TextureUsage::COMPUTE_SHADER_SAMPLED,
        texture: output_srv.texture(),
      },
      Barrier::TextureBarrier {
        old_primary_usage: TextureUsage::RENDER_TARGET,
        new_primary_usage: TextureUsage::COMPUTE_SHADER_SAMPLED,
        old_usages: TextureUsage::RENDER_TARGET,
        new_usages: TextureUsage::COMPUTE_SHADER_SAMPLED,
        texture: motion_srv.texture(),
      },
      Barrier::TextureBarrier {
        old_primary_usage: TextureUsage::COMPUTE_SHADER_SAMPLED,
        new_primary_usage: TextureUsage::COMPUTE_SHADER_STORAGE_WRITE,
        old_usages: TextureUsage::COMPUTE_SHADER_SAMPLED,
        new_usages: TextureUsage::COMPUTE_SHADER_STORAGE_WRITE,
        texture: self.taa_srv.texture(),
      }
    ]);

    cmd_buf.set_pipeline(PipelineBinding::Compute(&self.pipeline));
    cmd_buf.bind_texture_view(BindingFrequency::PerDraw, 0, output_srv, &self.linear_sampler);
    cmd_buf.bind_texture_view(BindingFrequency::PerDraw, 1, &self.taa_srv_b, &self.linear_sampler);
    cmd_buf.bind_storage_texture(BindingFrequency::PerDraw, 2, &self.taa_uav);
    cmd_buf.bind_texture_view(BindingFrequency::PerDraw, 3, motion_srv, &self.nearest_sampler);
    cmd_buf.finish_binding();

    let info = self.taa_texture.get_info();
    cmd_buf.dispatch(info.width, info.height, 1);
  }

  pub fn taa_srv(&self) -> &Arc<B::TextureShaderResourceView> {
    &self.taa_srv
  }

  pub fn swap_history_resources(&mut self) {    
    std::mem::swap(&mut self.taa_texture, &mut self.taa_texture_b);
    std::mem::swap(&mut self.taa_srv, &mut self.taa_srv_b);
    std::mem::swap(&mut self.taa_uav, &mut self.taa_uav_b);
  }
}
