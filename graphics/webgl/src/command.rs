use std::{collections::VecDeque, rc::Rc, sync::Arc};

use sourcerenderer_core::graphics::{BindingFrequency, Buffer, BufferInfo, BufferUsage, CommandBuffer, LoadOp, MemoryUsage, PipelineBinding, Queue, Scissor, ShaderType, Viewport, IndexFormat};
use web_sys::{WebGl2RenderingContext, WebGlBuffer, WebGlRenderingContext};

use crate::{GLThreadSender, WebGLBackend, WebGLBuffer, WebGLFence, WebGLGraphicsPipeline, WebGLSwapchain, WebGLTexture, WebGLTextureSamplingView, buffer, device::WebGLHandleAllocator, sync::WebGLSemaphore, texture::{WebGLSampler, WebGLUnorderedAccessView}, thread::{TextureHandle, WebGLThreadBuffer}, rt::WebGLAccelerationStructureStub};

use bitflags::bitflags;

bitflags! {
  pub struct WebGLCommandBufferDirty: u32 {
    const VAO = 0b0001;
  }
}

pub struct WebGLCommandBuffer {
  sender: GLThreadSender,
  pipeline: Option<Arc<WebGLGraphicsPipeline>>,
  commands: VecDeque<Box<dyn FnMut(&mut crate::thread::WebGLThreadDevice) + Send>>,
  inline_buffer: Arc<WebGLBuffer>,
  handles: Arc<WebGLHandleAllocator>,
  used_buffers: Vec<Arc<WebGLBuffer>>,
  used_textures: Vec<Arc<WebGLTexture>>,
  used_pipelines: Vec<Arc<WebGLGraphicsPipeline>>,
  dirty: WebGLCommandBufferDirty,
  vertex_buffer: Option<Arc<WebGLBuffer>>,
  index_buffer_offset: usize
}

impl WebGLCommandBuffer {
  pub fn new(sender: &GLThreadSender, handle_allocator: &Arc<WebGLHandleAllocator>) -> Self {
    let inline_buffer = Arc::new(WebGLBuffer::new(handle_allocator.new_buffer_handle(), &BufferInfo {
      size: 256,
      usage: BufferUsage::CONSTANT,
    }, MemoryUsage::CpuToGpu, sender));
    WebGLCommandBuffer {
      pipeline: None,
      commands: VecDeque::new(),
      sender: sender.clone(),
      handles: handle_allocator.clone(),
      inline_buffer,
      used_buffers: Vec::new(),
      used_textures: Vec::new(),
      used_pipelines: Vec::new(),
      dirty: WebGLCommandBufferDirty::empty(),
      vertex_buffer: None,
      index_buffer_offset: 0,
    }
  }

  fn before_draw(&mut self) {
    if self.dirty.is_empty() {
      return;
    }
    assert!(self.pipeline.is_some());
    assert!(self.vertex_buffer.is_some());

    let dirty = self.dirty;
    let pipeline = self.pipeline.as_ref().unwrap();
    let pipeline_handle = pipeline.handle();
    let vbo = self.vertex_buffer.as_ref().unwrap();
    let vbo_handle = vbo.handle();
    self.commands.push_back(Box::new(move |device| {
      let pipeline = device.pipeline(pipeline_handle);
      let vbo = device.buffer(vbo_handle);
      if dirty.contains(WebGLCommandBufferDirty::VAO) {
        let index_buffer: WebGlBuffer = device.get_parameter(WebGl2RenderingContext::ELEMENT_ARRAY_BUFFER_BINDING).unwrap().into();
        let mut vbs: [Option<Rc<WebGLThreadBuffer>>; 4] = Default::default();
        vbs[0] = Some(vbo.clone());
        let vao = pipeline.get_vao(&vbs);
        device.bind_vertex_array(Some(&vao));
        device.bind_buffer(WebGl2RenderingContext::ELEMENT_ARRAY_BUFFER, Some(&index_buffer));
      }
    }));
    self.dirty = WebGLCommandBufferDirty::empty();
  }
}

impl CommandBuffer<WebGLBackend> for WebGLCommandBuffer {
  fn set_pipeline(&mut self, pipeline: PipelineBinding<WebGLBackend>) {
    match pipeline {
      PipelineBinding::Graphics(pipeline) => {
        self.pipeline = Some(pipeline.clone());
        self.used_pipelines.push(pipeline.clone());
        let handle = pipeline.handle();
        self.dirty |= WebGLCommandBufferDirty::VAO;
        self.commands.push_back(Box::new(move |device| {
          let pipeline = device.pipeline(handle).clone();
          device.use_program(Some(pipeline.gl_program()));
          device.enable(WebGl2RenderingContext::DEPTH_TEST);
          device.front_face(pipeline.gl_front_face());
          let cull_face = pipeline.gl_cull_face();
          if cull_face == 0 {
            device.disable(WebGl2RenderingContext::CULL_FACE);
          } else {
            device.enable(WebGl2RenderingContext::CULL_FACE);
            device.cull_face(cull_face);
          }
        }));
      },
      PipelineBinding::Compute(_) => panic!("WebGL does not support compute shaders"),
      PipelineBinding::RayTracing(_) => panic!("WebGL does not support ray tracing")
    }
  }

  fn set_vertex_buffer(&mut self, vertex_buffer: &Arc<WebGLBuffer>, offset: usize) {
    self.vertex_buffer = Some(vertex_buffer.clone());
    self.dirty |= WebGLCommandBufferDirty::VAO;
  }

  fn set_index_buffer(&mut self, index_buffer: &Arc<WebGLBuffer>, offset: usize, index_format: IndexFormat) {
    // TODO: maybe track dirty and do before draw

    if index_format != IndexFormat::U32 {
      unimplemented!("16 bit indices are not implemented");
    }

    self.index_buffer_offset = offset;

    let handle = index_buffer.handle();
    self.commands.push_back(Box::new(move |device| {
      let buffer = device.buffer(handle).clone();
      device.bind_buffer(WebGlRenderingContext::ELEMENT_ARRAY_BUFFER, Some(buffer.gl_buffer()));
    }));
  }

  fn set_viewports(&mut self, viewports: &[ Viewport ]) {
    // TODO: maybe track dirty and do before draw

    if viewports.len() == 0 {
      return;
    }
    debug_assert_eq!(viewports.len(), 1);
    let viewports: Vec<Viewport> = viewports.iter().cloned().collect();
    self.commands.push_back(Box::new(move |device| {
      let viewport = viewports.first().unwrap();
      device.viewport(viewport.position.x as i32, viewport.position.y as i32, viewport.extent.x as i32, viewport.extent.y as i32);
    }));
  }

  fn set_scissors(&mut self, scissors: &[ Scissor ]) {
    // TODO: maybe track dirty and do before draw

    if scissors.len() == 0 {
      return;
    }
    debug_assert_eq!(scissors.len(), 1);
    let scissors: Vec<Scissor> = scissors.iter().cloned().collect();
    self.commands.push_back(Box::new(move |device| {
      let scissor = scissors.first().unwrap();
      device.scissor(scissor.position.x as i32, scissor.position.y as i32, scissor.extent.x as i32, scissor.extent.y as i32);
    }));
  }

  fn upload_dynamic_data<T>(&mut self, data: &[T], usage: BufferUsage) -> Arc<WebGLBuffer>
  where T: 'static + Send + Sync + Sized + Clone {
    let buffer_handle = self.handles.new_buffer_handle();
    let buffer = Arc::new(WebGLBuffer::new(buffer_handle, &BufferInfo { size: std::mem::size_of_val(data), usage }, MemoryUsage::CpuToGpu, &self.sender));
    unsafe {
      let mapped = buffer.map_unsafe(false).unwrap();
      std::ptr::copy(data.as_ptr() as *const u8, mapped, std::mem::size_of_val(data));
      buffer.unmap_unsafe(true);
    }
    buffer
  }

  fn upload_dynamic_data_inline<T>(&mut self, data: &[T], _visible_for_shader_stage: ShaderType)
  where T: 'static + Send + Sync + Sized + Clone {
    assert!(self.pipeline.is_some());
    let pipeline = self.pipeline.as_ref().unwrap();
    unsafe {
      let mapped = self.inline_buffer.map_unsafe(false).unwrap();
      std::ptr::copy(data.as_ptr() as *const u8, mapped, std::mem::size_of_val(data));
      self.inline_buffer.unmap_unsafe(true);
    }
    let pipeline_handle = pipeline.handle();
    let buffer_handle = self.inline_buffer.handle();
    self.commands.push_back(Box::new(move |device| {
      let pipeline = device.pipeline(pipeline_handle);
      if let Some(info) = pipeline.push_constants_info() {
        let binding = info.binding;
        let buffer = device.buffer(buffer_handle);
        debug_assert!(buffer.info().size as u32 >= info.size);
        device.bind_buffer_base(WebGl2RenderingContext::UNIFORM_BUFFER, binding, Some(&buffer.gl_buffer()));
      }
    }));
  }

  fn draw(&mut self, vertices: u32, offset: u32) {
    self.before_draw();

    self.commands.push_back(Box::new(move |device| {
      device.draw_arrays(
        WebGlRenderingContext::TRIANGLES, // TODO: self.pipeline.as_ref().unwrap().gl_draw_mode(),
        offset as i32,
        vertices as i32
      );
    }));
  }

  fn draw_indexed(&mut self, instances: u32, first_instance: u32, indices: u32, first_index: u32, vertex_offset: i32) {
    self.before_draw();

    // TODO: support instancing with WebGL2
    assert_eq!(instances, 1);
    assert_eq!(first_instance, 0);
    assert_eq!(vertex_offset, 0);
    let pipeline_handle = self.pipeline.as_ref().unwrap().handle();
    let index_offset = self.index_buffer_offset as i32;
    self.commands.push_back(Box::new(move |device| {
      let pipeline = device.pipeline(pipeline_handle);
      device.draw_elements_with_i32(
        pipeline.gl_draw_mode(),
        indices as i32,
        WebGlRenderingContext::UNSIGNED_INT,
        first_index as i32 * std::mem::size_of::<u32>() as i32 + index_offset,
      );
    }));
  }

  fn bind_texture_view(&mut self, frequency: BindingFrequency, binding: u32, texture: &Arc<WebGLTextureSamplingView>, sampler: &Arc<WebGLSampler>) {
    /*assert_eq!(frequency, BindingFrequency::High);
    let gl_texture = texture.texture().handle();
    let view_info = texture.info();
    let info = texture.texture().info();
    let is_cubemap = info.array_length == 6;
    let target = if is_cubemap { WebGlRenderingContext::TEXTURE_BINDING_CUBE_MAP } else { WebGlRenderingContext::TEXTURE_BINDING_2D };
    let bind_texture = self.context.get_parameter(target).unwrap();
    self.context.active_texture(WebGlRenderingContext::TEXTURE0 + binding);
    self.context.bind_texture(target, Some(gl_texture));
    {
      // TODO: optimize state changes
      /*self.context.tex_parameteri(target, WebGlRenderingContext::TEXTURE_WRAP_S, address_mode_to_gl(view_info.address_mode_u) as i32);
      self.context.tex_parameteri(target, WebGlRenderingContext::TEXTURE_WRAP_T, address_mode_to_gl(view_info.address_mode_v) as i32);
      self.context.tex_parameteri(target, WebGlRenderingContext::TEXTURE_MIN_FILTER, min_filter_to_gl(view_info.min_filter, view_info.mip_filter) as i32);
      self.context.tex_parameteri(target, WebGlRenderingContext::TEXTURE_MAG_FILTER, max_filter_to_gl(view_info.mag_filter) as i32);*/
    }
    self.context.active_texture(WebGlRenderingContext::TEXTURE0 + binding);
    //self.context.uniform1i(LOCATION, 0);

    if !bind_texture.is_null() {
      let bind_texture = bind_texture.unchecked_into::<WebGlTexture>();
      self.context.bind_texture(target, Some(&bind_texture));
    }*/
  }

  fn bind_uniform_buffer(&mut self, frequency: BindingFrequency, binding: u32, buffer: &Arc<WebGLBuffer>) {
    assert!(self.pipeline.is_some());

    self.used_buffers.push(buffer.clone());
    let pipeline = self.pipeline.as_ref().unwrap();
    let pipeline_handle = pipeline.handle();
    let buffer_handle = buffer.handle();
    self.commands.push_back(Box::new(move |device| {
      let buffer = device.buffer(buffer_handle);
      let pipeline = device.pipeline(pipeline_handle);
      let info = pipeline.ubo_info(frequency, binding);
      if let Some(info) = info {
        debug_assert!(buffer.info().size as u32 >= info.size);
        let binding_index = info.binding;
        device.bind_buffer_base(WebGl2RenderingContext::UNIFORM_BUFFER, binding_index, Some(buffer.gl_buffer()));
      }
    }));
  }

  fn bind_storage_buffer(&mut self, _frequency: BindingFrequency, _binding: u32, _buffer: &Arc<WebGLBuffer>) {
    panic!("WebGL does not support storage buffers");
  }

  fn finish_binding(&mut self) {
    // nop
  }

  fn begin_label(&mut self, _label: &str) {}
  fn end_label(&mut self) {}

  fn dispatch(&mut self, _group_count_x: u32, _group_count_y: u32, _group_count_z: u32) {
    panic!("WebGL does not support compute shaders");
  }

  fn blit(&mut self, _src_texture: &Arc<WebGLTexture>, _src_array_layer: u32, _src_mip_level: u32, _dst_texture: &Arc<WebGLTexture>, _dst_array_layer: u32, _dst_mip_level: u32) {
    unimplemented!()
  }

  fn finish(self) -> WebGLCommandSubmission {
    // nop
    WebGLCommandSubmission {
      cmd_buffer: self
    }
  }

  fn bind_storage_texture(&mut self, _frequency: BindingFrequency, _binding: u32, _texture: &Arc<WebGLUnorderedAccessView>) {
    panic!("WebGL does not support storage textures")
  }

  fn begin_render_pass(&mut self, renderpass_info: &sourcerenderer_core::graphics::RenderPassBeginInfo<WebGLBackend>, recording_mode: sourcerenderer_core::graphics::RenderpassRecordingMode) {
    let mut clear_mask: u32 = 0;
    let mut color_attachments: [Option<TextureHandle>; 8] = Default::default();
    let mut depth_attachment = Option::<TextureHandle>::None;
    let subpass = &renderpass_info.subpasses[0];
    for (index, attachment_ref) in subpass.output_color_attachments.iter().enumerate() {
      let attachment = &renderpass_info.attachments[attachment_ref.index as usize];
      match &attachment.view {
        sourcerenderer_core::graphics::RenderPassAttachmentView::RenderTarget(rt) => {
          if attachment.load_op == LoadOp::Clear {
            clear_mask |= WebGl2RenderingContext::COLOR_BUFFER_BIT;
          }
          color_attachments[index] = Some(rt.texture().handle());
        },
        sourcerenderer_core::graphics::RenderPassAttachmentView::DepthStencil(ds) => {
          if attachment.load_op == LoadOp::Clear {
            clear_mask |= WebGl2RenderingContext::DEPTH_BUFFER_BIT;
          }
          depth_attachment = Some(ds.texture().handle());
        },
      }
    }

    self.commands.push_back(Box::new(move |context| {
      let fbo = context.get_framebuffer(&color_attachments, depth_attachment);
      context.bind_framebuffer(WebGl2RenderingContext::DRAW_FRAMEBUFFER, fbo.as_ref());
      context.clear_color(0f32, 0f32, 0f32, 1f32);
      context.clear(clear_mask);
    }));
  }

  fn advance_subpass(&mut self) {
  }

  fn end_render_pass(&mut self) {
  }

  fn barrier<'a>(&mut self, _barriers: &[sourcerenderer_core::graphics::Barrier<WebGLBackend>]) {
    // nop
  }

  fn flush_barriers(&mut self) {
    // nop
  }

  fn inheritance(&self) -> &Self::CommandBufferInheritance {
    panic!("WebGL does not support inner command buffers")
  }

  type CommandBufferInheritance = ();

  fn execute_inner(&mut self, _submission: Vec<WebGLCommandSubmission>) {
    panic!("WebGL does not support inner command buffers")
  }

  fn create_query_range(&mut self, _count: u32) -> Arc<()> {
    todo!()
  }

  fn begin_query(&mut self, _query_range: &Arc<()>, _query_index: u32) {
    todo!()
  }

  fn end_query(&mut self, _query_range: &Arc<()>, _query_index: u32) {
    todo!()
  }

  fn copy_query_results_to_buffer(&mut self, _query_range: &Arc<()>, _buffer: &Arc<WebGLBuffer>, _start_index: u32, _count: u32) {
    todo!()
  }

  fn create_temporary_buffer(&mut self, _info: &BufferInfo, _memory_usage: MemoryUsage) -> Arc<WebGLBuffer> {
    unimplemented!()
  }

  fn bind_sampler(&mut self, _frequency: BindingFrequency, _binding: u32, _sampler: &Arc<WebGLSampler>) {
    panic!("WebGL does not support separate samplers")
  }

  fn bind_acceleration_structure(&mut self, _frequency: BindingFrequency, _binding: u32, _acceleration_structure: &Arc<WebGLAccelerationStructureStub>) {
    panic!("WebGL does not support ray tracing")
  }

  fn create_bottom_level_acceleration_structure(&mut self, _info: &sourcerenderer_core::graphics::BottomLevelAccelerationStructureInfo<WebGLBackend>, _size: usize, _target_buffer: &Arc<WebGLBuffer>, _scratch_buffer: &Arc<WebGLBuffer>) -> Arc<WebGLAccelerationStructureStub> {
    panic!("WebGL does not support ray tracing")
  }

  fn upload_top_level_instances(&mut self, _instances: &[sourcerenderer_core::graphics::AccelerationStructureInstance<WebGLBackend>]) -> Arc<WebGLBuffer> {
    panic!("WebGL does not support ray tracing")
  }

  fn create_top_level_acceleration_structure(&mut self, _info: &sourcerenderer_core::graphics::TopLevelAccelerationStructureInfo<WebGLBackend>, _size: usize, _target_buffer: &Arc<WebGLBuffer>, _scratch_buffer: &Arc<WebGLBuffer>) -> Arc<WebGLAccelerationStructureStub> {
    panic!("WebGL does not support ray tracing")
  }

  fn trace_ray(&mut self, _width: u32, _height: u32, _depth: u32) {
    panic!("WebGL does not support ray tracing")
  }

  fn track_texture_view(&mut self, _texture_view: &Arc<WebGLTextureSamplingView>) {
    // nop
  }
}

pub struct WebGLCommandSubmission {
  cmd_buffer: WebGLCommandBuffer
}

pub struct WebGLQueue {
  sender: GLThreadSender,
  handle_allocator: Arc<WebGLHandleAllocator>
}

impl WebGLQueue {
  pub fn new(sender: &GLThreadSender, handle_allocator: &Arc<WebGLHandleAllocator>) -> Self {
    Self {
      sender: sender.clone(),
      handle_allocator: handle_allocator.clone()
    }
  }
}

impl Queue<WebGLBackend> for WebGLQueue {
  fn create_command_buffer(&self) -> WebGLCommandBuffer {
    WebGLCommandBuffer::new(&self.sender, &self.handle_allocator)
  }

  fn create_inner_command_buffer(&self, _inheritance: &()) -> WebGLCommandBuffer {
    panic!("WebGL does not support inner command buffers")
  }

  fn submit(&self, mut submission: WebGLCommandSubmission, _fence: Option<&Arc<WebGLFence>>, _wait_semaphores: &[&Arc<WebGLSemaphore>], _signal_semaphores: &[&Arc<WebGLSemaphore>], _delay: bool) {
    while let Some(cmd) = submission.cmd_buffer.commands.pop_front() {
      self.sender.send(cmd);
    }
  }

  fn present(&self, swapchain: &Arc<WebGLSwapchain>, _wait_semaphores: &[&Arc<WebGLSemaphore>], _delay: bool) {
    swapchain.present();
    let c_swapchain = swapchain.clone();
    self.sender.send(Box::new(move |_context| {
      c_swapchain.bump_frame();
    }));
  }

  fn process_submissions(&self) {
    // WebGL Queue isn't threaded right now
  }
}
