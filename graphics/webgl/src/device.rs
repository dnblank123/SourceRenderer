use std::{rc::Rc, sync::{Arc, atomic::{AtomicU64, Ordering}}};

use crossbeam_channel::{Sender, unbounded};
use sourcerenderer_core::graphics::{Buffer, BufferInfo, Device, GraphicsPipelineInfo, MemoryUsage, RenderPassInfo, SamplerInfo, Surface, Texture, TextureDepthStencilViewInfo, TextureRenderTargetViewInfo, TextureUnorderedAccessViewInfo};
use wasm_bindgen::JsCast;
use web_sys::{WebGlRenderingContext, WebGlTexture};

use crate::{GLThreadReceiver, GLThreadSender, RawWebGLContext, WebGLBackend, WebGLBuffer, WebGLComputePipeline, WebGLFence, WebGLGraphicsPipeline, WebGLShader, WebGLSurface, WebGLSwapchain, WebGLTexture, WebGLTextureShaderResourceView, command::WebGLQueue, format_to_internal_gl, sync::WebGLSemaphore, texture::{WebGLDepthStencilView, WebGLRenderTargetView, WebGLSampler, WebGLUnorderedAccessView}, thread::WebGLThreadDevice};

pub struct WebGLDevice {
  next_buffer_id: AtomicU64,
  next_texture_id: AtomicU64,
  next_shader_id: AtomicU64,
  next_pipeline_id: AtomicU64,
  queue: Arc<WebGLQueue>,
  sender: GLThreadSender,
  receiver: GLThreadReceiver,
  surface: Arc<WebGLSurface>
}

impl WebGLDevice {
  pub fn new(surface: &Arc<WebGLSurface>) -> Self {
    let (sender, receiver): (GLThreadSender, GLThreadReceiver) = unbounded();
    Self {
      queue: Arc::new(WebGLQueue::new(&sender)),
      sender: sender.clone(),
      receiver,
      next_buffer_id: AtomicU64::new(0),
      next_texture_id: AtomicU64::new(0),
      next_shader_id: AtomicU64::new(0),
      next_pipeline_id: AtomicU64::new(0),
      surface: surface.clone()
    }
  }

  pub fn receiver(&self) -> &GLThreadReceiver {
    &self.receiver
  }
}

impl Device<WebGLBackend> for WebGLDevice {
  fn create_buffer(&self, info: &BufferInfo, memory_usage: MemoryUsage, _name: Option<&str>) -> Arc<WebGLBuffer> {
    let id = self.next_buffer_id.fetch_add(1, Ordering::SeqCst) + 1;
    Arc::new(WebGLBuffer::new(id, info, memory_usage, &self.sender))
  }

  fn upload_data<T>(&self, data: &[T], memory_usage: sourcerenderer_core::graphics::MemoryUsage, usage: sourcerenderer_core::graphics::BufferUsage) -> Arc<WebGLBuffer> where T: 'static + Send + Sync + Sized + Clone {
    /*let data = data.clone();
    let buffer = Arc::new(WebGLBuffer::new(&self.context, &BufferInfo { size: std::mem::size_of_val(data), usage }, memory_usage));
    unsafe {
      let ptr = buffer.map_unsafe(true).unwrap();
      std::ptr::copy(data.as_ptr(), ptr as *mut T, data.len());
      buffer.unmap_unsafe(true);
    }
    buffer*/
    unimplemented!()
  }

  fn create_shader(&self, shader_type: sourcerenderer_core::graphics::ShaderType, bytecode: &[u8], _name: Option<&str>) -> Arc<WebGLShader> {
    let id = self.next_shader_id.fetch_add(1, Ordering::SeqCst) + 1;
    Arc::new(WebGLShader::new(id, shader_type, bytecode, &self.sender))
  }

  fn create_texture(&self, info: &sourcerenderer_core::graphics::TextureInfo, _name: Option<&str>) -> Arc<WebGLTexture> {
    let id = self.next_texture_id.fetch_add(1, Ordering::SeqCst) + 1;
    Arc::new(WebGLTexture::new(id, info, &self.sender))
  }

  fn create_shader_resource_view(&self, texture: &Arc<WebGLTexture>, info: &sourcerenderer_core::graphics::TextureShaderResourceViewInfo) -> Arc<WebGLTextureShaderResourceView> {
    Arc::new(WebGLTextureShaderResourceView::new(texture, info))
  }

  fn create_graphics_pipeline(&self, info: &GraphicsPipelineInfo<WebGLBackend>, _pass_info: &RenderPassInfo, _subpass_index: u32) -> Arc<WebGLGraphicsPipeline> {
    let id = self.next_pipeline_id.fetch_add(1, Ordering::SeqCst) + 1;
    Arc::new(WebGLGraphicsPipeline::new(id, info, &self.sender))
  }

  fn create_compute_pipeline(&self, _shader: &Arc<WebGLShader>) -> Arc<WebGLComputePipeline> {
    panic!("WebGL does not support compute shaders");
  }

  fn wait_for_idle(&self) {
    // Can't implement that but it's also not our problem
  }

  fn init_texture(&self, texture: &Arc<WebGLTexture>, buffer: &Arc<WebGLBuffer>, mip_level: u32, array_layer: u32) {
    /*let info = texture.get_info();
    let data_ref = buffer.data();
    let data = data_ref.as_ref().unwrap();
    let target = texture.target();
    let bind_texture = self.context.get_parameter(target).unwrap();
    self.context.bind_texture(target, Some(texture.handle()));
    if info.format.is_compressed() {
      self.context.compressed_tex_image_2d_with_u8_array(
        if texture.is_cubemap() { WebGlRenderingContext::TEXTURE_CUBE_MAP_POSITIVE_X + array_layer } else { target },
        mip_level as i32,
        format_to_internal_gl(info.format),
        info.width as i32,
        info.height as i32,
        0,
        &data[..]
      );
    } else {
      self.context.tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_u8_array(
        if texture.is_cubemap() { WebGlRenderingContext::TEXTURE_CUBE_MAP_POSITIVE_X + array_layer } else { target },
        mip_level as i32,
        format_to_internal_gl(info.format) as i32,
        info.width as i32,
        info.height as i32,
        0,
        format_to_internal_gl(info.format), // TODO: change for Webgl 2
        WebGlRenderingContext::UNSIGNED_BYTE,
        Some(&data[..])
      ).unwrap();
    }
    if !bind_texture.is_null() {
      let bind_texture = bind_texture.unchecked_into::<WebGlTexture>();
      self.context.bind_texture(target, Some(&bind_texture));
    }*/
    unimplemented!()
  }

  fn init_texture_async(&self, texture: &Arc<WebGLTexture>, buffer: &Arc<WebGLBuffer>, mip_level: u32, array_layer: u32) -> Option<Arc<WebGLFence>> {
    //self.init_texture(texture, buffer, mip_level, array_layer);
    //Some(Arc::new(WebGLFence::new()))
    unimplemented!()
  }

  fn init_buffer(&self, src_buffer: &Arc<WebGLBuffer>, dst_buffer: &Arc<WebGLBuffer>) {
    /*let data_ref = src_buffer.data();
    let data = data_ref.as_ref().unwrap();
    unsafe {
      let mapped_dst = dst_buffer.map_unsafe(true).unwrap();
      std::ptr::copy(data.as_ptr() as *const u8, mapped_dst, std::mem::size_of_val(data));
      dst_buffer.unmap_unsafe(true);
    }*/
    unimplemented!()
  }

  fn flush_transfers(&self) {
    // nop
  }

  fn free_completed_transfers(&self) {
    // nop
  }

  fn create_render_target_view(&self, texture: &Arc<WebGLTexture>, info: &TextureRenderTargetViewInfo) -> Arc<WebGLRenderTargetView> {
    Arc::new(WebGLRenderTargetView::new(texture, info))
  }

  fn create_unordered_access_view(&self, texture: &Arc<WebGLTexture>, info: &TextureUnorderedAccessViewInfo) -> Arc<WebGLUnorderedAccessView> {
    panic!("WebGL does not support storage textures")
  }

  fn create_depth_stencil_view(&self, texture: &Arc<WebGLTexture>, info: &TextureDepthStencilViewInfo) -> Arc<WebGLDepthStencilView> {
    Arc::new(WebGLDepthStencilView::new(texture, info))
  }

  fn create_sampler(&self, info: &SamplerInfo) -> Arc<WebGLSampler> {
    //Arc::new(WebGLSampler::new(info))
    unimplemented!()
  }

  fn create_fence(&self) -> Arc<WebGLFence> {
    Arc::new(WebGLFence {})
  }

  fn create_semaphore(&self) -> Arc<WebGLSemaphore> {
    Arc::new(WebGLSemaphore {})
  }

  fn graphics_queue(&self) -> &Arc<WebGLQueue> {
    &self.queue
  }
}
