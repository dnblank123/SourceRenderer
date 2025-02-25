use std::{cell::RefCell, collections::{HashMap, VecDeque}, hash::Hash, ops::Deref, rc::Rc, sync::Arc};

use log::warn;
use sourcerenderer_core::graphics::{BindingFrequency, BufferInfo, BufferUsage, GraphicsPipelineInfo, InputRate, MemoryUsage, PrimitiveType, ShaderType, TextureInfo, InputAssemblerElement, ShaderInputElement, RasterizerInfo, DepthStencilInfo, LogicOp, AttachmentBlendInfo, SamplerInfo, Format};

use web_sys::{Document, WebGl2RenderingContext, WebGlBuffer as WebGLBufferHandle, WebGlFramebuffer, WebGlProgram, WebGlRenderingContext, WebGlShader, WebGlTexture, WebGlVertexArrayObject, WebGlUniformLocation, WebGlSampler};

use crate::{WebGLBackend, WebGLSurface, raw_context::RawWebGLContext, texture::{format_to_internal_gl, compare_func_to_gl, mag_filter_to_gl, min_filter_to_gl, address_mode_to_gl}, spinlock::{SpinLock, SpinLockGuard}, WebGLWork, WebGLShader};

pub struct WebGLThreadQueue {
  write_queue: SpinLock<VecDeque<WebGLWork>>,
  read_queue: SpinLock<VecDeque<WebGLWork>>
}

impl WebGLThreadQueue {
  pub fn new() -> Self {
    Self {
      write_queue: SpinLock::new(VecDeque::new()),
      read_queue: SpinLock::new(VecDeque::new()),
    }
  }

  pub fn send(&self, work: WebGLWork) {
    let mut guard = self.write_queue.lock();
    guard.push_back(work);
  }

  pub fn swap_buffers(&self) {
    let mut write_guard = self.write_queue.lock();
    let mut read_guard = self.read_queue.lock();
    assert_eq!(read_guard.len(), 0);
    std::mem::swap(&mut *write_guard, &mut *read_guard);
  }

  pub fn read_queue(&self) -> SpinLockGuard<VecDeque<WebGLWork>> {
    self.read_queue.lock()
  }
}

pub struct WebGLThreadIterator<'a> {
  lock: SpinLockGuard<'a, Vec<WebGLWork>>
}

impl<'a> Iterator for WebGLThreadIterator<'a> {
  type Item = WebGLWork;

  fn next(&mut self) -> Option<Self::Item> {
    (self.lock.len() != 0).then(|| self.lock.remove(0))
  }
}

#[derive(Hash, PartialEq, Eq, Debug, Clone)]
pub struct WebGLTextureHandleView {
  pub texture: TextureHandle,
  pub array_layer: u32,
  pub mip: u32,
}

#[derive(Hash, PartialEq, Eq, Debug)]
struct FboKey {
  rts: [Option<WebGLTextureHandleView>; 8],
  ds: Option<WebGLTextureHandleView>
}

pub struct WebGLThreadTexture {
  texture: WebGlTexture,
  context: Rc<RawWebGLContext>,
  info: TextureInfo,
  is_cubemap: bool,
  target: u32
}

impl WebGLThreadTexture {
  pub fn new(context: &Rc<RawWebGLContext>, info: &TextureInfo) -> Self {
    assert!(info.array_length == 6 || info.array_length == 1);
    let is_cubemap = info.array_length == 6;
    if is_cubemap {
      todo!("Cubemaps are unimplemented");
    }
    if info.format.is_compressed() {
      todo!("Compressed textures are unimplemented");
    }
    let target = if is_cubemap { WebGlRenderingContext::TEXTURE_CUBE_MAP } else { WebGlRenderingContext::TEXTURE_2D };
    let texture = context.create_texture().unwrap();
    context.bind_texture(target, Some(&texture));
    context.tex_parameteri(target, WebGl2RenderingContext::TEXTURE_MAX_LEVEL, info.mip_levels as i32);
    context.tex_parameteri(target, WebGl2RenderingContext::TEXTURE_MAG_FILTER, WebGl2RenderingContext::NEAREST as i32);
    context.tex_parameteri(target, WebGl2RenderingContext::TEXTURE_MIN_FILTER, WebGl2RenderingContext::NEAREST_MIPMAP_NEAREST as i32);
    context.tex_storage_2d(target, info.mip_levels as i32, format_to_internal_gl(info.format), info.width as i32, info.height as i32);
    Self {
      texture,
      context: context.clone(),
      info: info.clone(),
      is_cubemap,
      target
    }
  }

  pub fn info(&self) -> &TextureInfo {
    &self.info
  }

  pub fn is_cubemap(&self) -> bool {
    self.is_cubemap
  }

  pub fn target(&self) -> u32 {
    self.target
  }

  pub fn gl_handle(&self) -> &WebGlTexture {
    &self.texture
  }
}

impl Drop for WebGLThreadTexture {
  fn drop(&mut self) {
    self.context.delete_texture(Some(&self.texture));
  }
}

pub struct WebGLThreadSampler {
  context: Rc<RawWebGLContext>,
  sampler: WebGlSampler,
}

impl WebGLThreadSampler {
  pub fn new(
    context: &Rc<RawWebGLContext>,
    info: &SamplerInfo
  ) -> Self {
    let sampler = context.create_sampler().unwrap();
    if let Some(max_lod) = info.max_lod {
      context.sampler_parameterf(&sampler, WebGl2RenderingContext::TEXTURE_MAX_LOD, max_lod);
    }
    context.sampler_parameterf(&sampler, WebGl2RenderingContext::TEXTURE_MIN_LOD, info.min_lod);
    if let Some(compare_op) = info.compare_op {
      context.sampler_parameteri(&sampler, WebGl2RenderingContext::TEXTURE_COMPARE_MODE, WebGl2RenderingContext::COMPARE_REF_TO_TEXTURE as i32);
      context.sampler_parameteri(&sampler, WebGl2RenderingContext::TEXTURE_COMPARE_FUNC, compare_func_to_gl(compare_op) as i32);
    } else {
      context.sampler_parameteri(&sampler, WebGl2RenderingContext::TEXTURE_COMPARE_MODE, WebGl2RenderingContext::NONE as i32);
    }
    context.sampler_parameteri(&sampler, WebGl2RenderingContext::TEXTURE_MAG_FILTER, mag_filter_to_gl(info.mag_filter) as i32);
    context.sampler_parameteri(&sampler, WebGl2RenderingContext::TEXTURE_MIN_FILTER, min_filter_to_gl(info.min_filter, info.mip_filter) as i32);
    context.sampler_parameteri(&sampler, WebGl2RenderingContext::TEXTURE_WRAP_R, address_mode_to_gl(info.address_mode_u) as i32);
    context.sampler_parameteri(&sampler, WebGl2RenderingContext::TEXTURE_WRAP_S, address_mode_to_gl(info.address_mode_v) as i32);
    context.sampler_parameteri(&sampler, WebGl2RenderingContext::TEXTURE_WRAP_T, address_mode_to_gl(info.address_mode_w) as i32);

    if context.extensions().anisotropic_filter {
      // context.sampler_parameterf(&sampler, web_sys::ExtTextureFilterAnisotropic::TEXTURE_MAX_ANISOTROPY_EXT, info.max_anisotropy);
    }

    Self {
      context: context.clone(),
      sampler,
    }
  }

  pub fn gl_handle(&self) -> &WebGlSampler {
    &self.sampler
  }
}

impl Drop for WebGLThreadSampler {
  fn drop(&mut self) {
    self.context.delete_sampler(Some(&self.sampler));
  }
}


pub struct WebGLThreadBuffer {
  context: Rc<RawWebGLContext>,
  buffer: WebGLBufferHandle,
  info: BufferInfo,
  gl_usage: u32,
  buffer_handle: BufferHandle
}

impl WebGLThreadBuffer {
  pub fn new(
    context: &Rc<RawWebGLContext>,
    info: &BufferInfo,
    buffer_handle: BufferHandle,
    _memory_usage: MemoryUsage,
  ) -> Self {
    let buffer_usage = info.usage & (BufferUsage::INDEX | BufferUsage::COPY_DST);

    let mut usage = WebGlRenderingContext::STATIC_DRAW;
    if buffer_usage.intersects(BufferUsage::COPY_DST) {
      if buffer_usage.intersects(BufferUsage::CONSTANT) {
        usage = WebGl2RenderingContext::STREAM_READ;
      } else {
        usage = WebGl2RenderingContext::STATIC_READ;
      }
    }
    if buffer_usage.intersects(BufferUsage::COPY_SRC) {
      /*if buffer_usage.intersects(BufferUsage::CONSTANT) {
        usage = WebGl2RenderingContext::STREAM_COPY;
      } else {
        usage = WebGl2RenderingContext::STATIC_COPY;
      }*/
      usage = WebGl2RenderingContext::STREAM_READ;
    }
    let buffer = context.create_buffer().unwrap();
    let target = crate::buffer::buffer_usage_to_target(info.usage);
    context.bind_buffer(target, Some(&buffer));
    context.buffer_data_with_i32(target, info.size as i32, usage);
    Self {
      context: context.clone(),
      info: info.clone(),
      gl_usage: usage,
      buffer,
      buffer_handle
    }
  }

  pub fn gl_buffer(&self) -> &WebGLBufferHandle {
    &self.buffer
  }

  pub fn gl_usage(&self) -> u32 {
    self.gl_usage
  }

  pub fn info(&self) -> &BufferInfo {
    &self.info
  }

  pub fn handle(&self) -> BufferHandle {
    self.buffer_handle
  }
}

impl Drop for WebGLThreadBuffer {
  fn drop(&mut self) {
    self.context.delete_buffer(Some(&self.buffer));
  }
}

pub struct WebGLThreadShader {
  context: Rc<RawWebGLContext>,
  shader: WebGlShader,
}

impl Drop for WebGLThreadShader {
  fn drop(&mut self) {
    self.context.delete_shader(Some(&self.shader));
  }
}

pub struct WebGLBlockInfo {
  pub name: String,
  pub binding: u32,
  pub size: u32
}

#[derive(Hash, Eq, PartialEq, Clone)]
pub struct WebGLVertexLayoutInfo {
  pub shader_inputs: Vec<ShaderInputElement>,
  pub input_assembler: Vec<InputAssemblerElement>
}

#[derive(Clone)]
pub struct WebGLBlendInfo {
  pub alpha_to_coverage_enabled: bool,
  pub logic_op_enabled: bool,
  pub logic_op: LogicOp,
  pub attachments: Vec<AttachmentBlendInfo>,
  pub constants: [f32; 4]
}

pub struct WebGLTextureUniformInfo {
  pub uniform_location: WebGlUniformLocation,
  pub texture_unit: u32,
}

#[derive(Debug, Hash, PartialEq, Eq, Clone)]
pub struct WebGLVBHandleBinding {
  pub buffer: u64,
  pub offset: u64,
}

pub struct WebGLVBThreadBinding {
  pub buffer: Rc<WebGLThreadBuffer>,
  pub offset: u64,
}

pub struct WebGLPipelineInfo {
  pub vs: Arc<WebGLShader>,
  pub fs: Option<Arc<WebGLShader>>,
  pub vertex_layout: WebGLVertexLayoutInfo,
  pub rasterizer: RasterizerInfo,
  pub depth_stencil: DepthStencilInfo,
  pub blend: WebGLBlendInfo,
  pub primitive_type: PrimitiveType
}

impl From<&GraphicsPipelineInfo<'_, WebGLBackend>> for WebGLPipelineInfo {
  fn from(info: &GraphicsPipelineInfo<WebGLBackend>) -> Self {
    Self {
      vs: info.vs.clone(),
      fs: info.fs.cloned(),
      vertex_layout: WebGLVertexLayoutInfo {
        shader_inputs: info.vertex_layout.shader_inputs.iter().cloned().collect(),
        input_assembler: info.vertex_layout.input_assembler.iter().cloned().collect(),
      },
      rasterizer: info.rasterizer.clone(),
      depth_stencil: info.depth_stencil.clone(),
      blend: WebGLBlendInfo {
        alpha_to_coverage_enabled: info.blend.alpha_to_coverage_enabled,
        logic_op_enabled: info.blend.logic_op_enabled,
        logic_op: info.blend.logic_op,
        attachments: info.blend.attachments.iter().cloned().collect(),
        constants: info.blend.constants.clone()
      },
      primitive_type: info.primitive_type,
    }
  }
}

pub struct WebGLThreadPipeline {
  context: Rc<RawWebGLContext>,
  program: WebGlProgram,
  ubo_infos: HashMap<(BindingFrequency, u32), WebGLBlockInfo>,
  push_constants_info: Option<WebGLBlockInfo>,
  vao_cache: RefCell<HashMap<[Option<WebGLVBHandleBinding>; 4], WebGlVertexArrayObject>>,
  info: WebGLPipelineInfo,
  attribs: HashMap<u32, u32>,
  texture_uniform_map: HashMap<(BindingFrequency, u32), WebGLTextureUniformInfo>,

  // graphics state
  gl_draw_mode: u32,
  gl_front_face: u32,
  gl_cull_face: u32
}

impl WebGLThreadPipeline {
  pub fn gl_draw_mode(&self) -> u32 {
    self.gl_draw_mode
  }

  pub fn gl_cull_face(&self) -> u32 {
    self.gl_cull_face
  }

  pub fn gl_front_face(&self) -> u32 {
    self.gl_front_face
  }

  pub fn gl_program(&self) -> &WebGlProgram {
    &self.program
  }

  pub fn info(&self) -> &WebGLPipelineInfo {
    &self.info
  }

  pub fn get_vao(&self, vertex_buffers: &[Option<WebGLVBThreadBinding>; 4]) -> WebGlVertexArrayObject {
    let mut key: [Option<WebGLVBHandleBinding>; 4] = Default::default();
    for i in 0..vertex_buffers.len() {
      key[i] = vertex_buffers[i].as_ref().map(|b| WebGLVBHandleBinding {
        buffer: b.buffer.handle(),
        offset: b.offset,
      });
    }
    {
      let cache = self.vao_cache.borrow();
      if let Some(cached) = cache.get(&key) {
        return cached.clone();
      }
    }

    let mut cache_mut = self.vao_cache.borrow_mut();

    let vao = self.context.create_vertex_array().unwrap();
    self.context.bind_vertex_array(Some(&vao));
    for input in &self.info.vertex_layout.shader_inputs {
      let ia_element = self.info.vertex_layout.input_assembler.iter().find(|a| a.binding == input.input_assembler_binding).unwrap();
      let gl_attrib_index_opt = self.attribs.get(&input.location_vk_mtl).copied();
      if gl_attrib_index_opt.is_none() {
        warn!("Missing vertex attribute: {}", input.location_vk_mtl);
        continue;
      }
      let gl_attrib_index = gl_attrib_index_opt.unwrap();

      let buffer = vertex_buffers[ia_element.binding as usize].as_ref();
      if buffer.is_none() {
        warn!("Vertex buffer {} not bound", ia_element.binding);
        continue;
      }
      let buffer = buffer.unwrap();

      self.context.bind_buffer(WebGl2RenderingContext::ARRAY_BUFFER, Some(buffer.buffer.gl_buffer()));
      self.context.enable_vertex_attrib_array(gl_attrib_index);
      self.context.vertex_attrib_divisor(gl_attrib_index,  if ia_element.input_rate == InputRate::PerVertex { 0 } else { 1 });
      self.context.vertex_attrib_pointer_with_i32(gl_attrib_index, input.format.element_size() as i32 / std::mem::size_of::<f32>() as i32, WebGl2RenderingContext::FLOAT, false, ia_element.stride as i32, input.offset as i32 + buffer.offset as i32);
    }
    cache_mut.insert(key, vao.clone());
    vao
  }

  pub fn push_constants_info(&self) -> Option<&WebGLBlockInfo> {
    self.push_constants_info.as_ref()
  }

  pub fn ubo_info(&self, frequency: BindingFrequency, binding: u32) -> Option<&WebGLBlockInfo> {
    self.ubo_infos.get(&(frequency, binding))
  }

  pub fn uniform_location(&self, frequency: BindingFrequency, binding: u32) -> Option<&WebGLTextureUniformInfo> {
    self.texture_uniform_map.get(&(frequency, binding))
  }
}

impl Drop for WebGLThreadPipeline {
  fn drop(&mut self) {
    let mut cache = self.vao_cache.borrow_mut();
    for (_key, vao) in cache.drain() {
      self.context.delete_vertex_array(Some(&vao));
    }

    self.context.delete_program(Some(&self.program));
  }
}

pub struct WebGLThreadDevice {
  context: Rc<RawWebGLContext>,
  textures: HashMap<TextureHandle, Rc<WebGLThreadTexture>>,
  shaders: HashMap<ShaderHandle, Rc<WebGLThreadShader>>,
  pipelines: HashMap<PipelineHandle, Rc<WebGLThreadPipeline>>,
  buffers: HashMap<BufferHandle, Rc<WebGLThreadBuffer>>,
  samplers: HashMap<TextureHandle, Rc<WebGLThreadSampler>>,
  thread_queue: Arc<WebGLThreadQueue>,
  fbo_cache: HashMap<FboKey, WebGlFramebuffer>
}

pub type BufferHandle = u64;
pub type TextureHandle = u64;
pub type ShaderHandle = u64;
pub type PipelineHandle = u64;
pub type SamplerHandle = u64;

impl WebGLThreadDevice {
  pub fn new(thread_queue: &Arc<WebGLThreadQueue>, surface: &WebGLSurface, document: &Document) -> Self {
    Self {
      context: Rc::new(RawWebGLContext::new(document, surface)),
      textures: HashMap::new(),
      shaders: HashMap::new(),
      pipelines: HashMap::new(),
      buffers: HashMap::new(),
      samplers: HashMap::new(),
      thread_queue: thread_queue.clone(),
      fbo_cache: HashMap::new()
    }
  }

  pub fn create_buffer(&mut self, id: BufferHandle, info: &BufferInfo, memory_usage: MemoryUsage, _name: Option<&str>) {
    let buffer = WebGLThreadBuffer::new(&self.context, info, id, memory_usage);
    assert!(self.buffers.insert(id, Rc::new(buffer)).is_none());
  }

  pub fn remove_buffer(&mut self, id: BufferHandle) {
    self.buffers.remove(&id).expect("Buffer didnt exist");
  }

  pub fn buffer(&self, id: BufferHandle) -> &Rc<WebGLThreadBuffer> {
    self.buffers.get(&id).expect("Cant find buffer")
  }

  pub fn create_shader(&mut self, id: ShaderHandle, shader_type: ShaderType, data: &[u8]) {
    let gl_shader_type = match shader_type {
      ShaderType::VertexShader => WebGl2RenderingContext::VERTEX_SHADER,
      ShaderType::FragmentShader => WebGl2RenderingContext::FRAGMENT_SHADER,
      _ => panic!("Shader type is not supported by WebGL")
    };
    let shader = self.context.create_shader(gl_shader_type).unwrap();
    let source = String::from_utf8(data.iter().copied().collect()).unwrap();
    self.context.shader_source(&shader, source.as_str());
    self.context.compile_shader(&shader);
    let info = self.context.get_shader_info_log(&shader);
    if let Some(info) = info {
      if !info.is_empty() {
        warn!("Shader info: {}", info);
      }
    }
    assert!(self.shaders.insert(id, Rc::new(WebGLThreadShader {
      context: self.context.clone(),
      shader: shader,
    })).is_none());
  }

  pub fn shader(&self, id: ShaderHandle) -> &Rc<WebGLThreadShader> {
    self.shaders.get(&id).expect("Shader does not exist")
  }

  pub fn remove_shader(&mut self, id: ShaderHandle) {
    self.shaders.remove(&id).expect("Shader does not exist");
  }

  pub fn create_pipeline(&mut self, id: PipelineHandle, info: WebGLPipelineInfo) {
    let vs = self.shader(info.vs.handle()).clone();
    let fs = info.fs.as_ref().map(|fs| self.shader(fs.handle()).clone());

    let program = self.context.create_program().unwrap();
    self.context.attach_shader(&program, &vs.shader);
    if let Some(fs) = &fs {
      self.context.attach_shader(&program, &fs.shader);
    }
    self.context.link_program(&program);
    if !self.context.get_program_parameter(&program, WebGl2RenderingContext::LINK_STATUS).as_bool().unwrap() {
      panic!("Linking shader failed.");
    }

    let mut attrib_map = HashMap::<u32, u32>::new();
    let attrib_count = self.context.get_program_parameter(&program, WebGl2RenderingContext::ACTIVE_ATTRIBUTES).as_f64().unwrap() as u32;
    for i in 0..attrib_count {
      let attrib_info = self.context.get_active_attrib(&program, i).unwrap();
      let name = attrib_info.name();
      let mut name_parts = name.split("_"); // name should be like this: "vs_input_X"
      name_parts.next();
      name_parts.next();
      let location = name_parts.next().unwrap().parse::<u32>().unwrap();
      let gl_location = self.context.get_attrib_location(&program, &name);
      attrib_map.insert(location, gl_location as u32);
    }

    let mut push_constants_info = Option::<WebGLBlockInfo>::None;
    let mut ubo_infos = HashMap::<(BindingFrequency, u32), WebGLBlockInfo>::new();
    let ubo_count = self.context.get_program_parameter(&program, WebGl2RenderingContext::ACTIVE_UNIFORM_BLOCKS).as_f64().unwrap() as u32;
    for i in 0..ubo_count {
      let binding = i + 1;
      self.context.uniform_block_binding(&program, i, binding);
      let size = self.context.get_active_uniform_block_parameter(&program, i, WebGl2RenderingContext::UNIFORM_BLOCK_DATA_SIZE).unwrap().as_f64().unwrap() as u32;
      let ubo_name = self.context.get_active_uniform_block_name(&program, i).unwrap();
      if ubo_name == "push_constants_t" {
        push_constants_info = Some(WebGLBlockInfo {
          name: ubo_name,
          size,
          binding: binding
        });
        continue;
      }
      let mut ubo_name_parts = ubo_name.split("_"); // name should be like this: "res_X_X_t"
      ubo_name_parts.next();
      let set = ubo_name_parts.next().unwrap();
      let descriptor_set_binding = ubo_name_parts.next().unwrap();
      let frequency = match set.parse::<u32>().unwrap() {
        0 => BindingFrequency::VeryFrequent,
        1 => BindingFrequency::VeryFrequent,
        2 => BindingFrequency::Frame,
        _ => panic!("Invalid binding frequency")
      };
      ubo_infos.insert((frequency, descriptor_set_binding.parse::<u32>().unwrap()), WebGLBlockInfo {
        name: ubo_name,
        size,
        binding: binding
      });
    }

    let mut uniform_map = HashMap::<(BindingFrequency, u32), WebGLTextureUniformInfo>::new();
    let uniform_count = self.context.get_program_parameter(&program, WebGl2RenderingContext::ACTIVE_UNIFORMS).as_f64().unwrap() as u32;
    for i in 0..uniform_count {
      let uniform = self.context.get_active_uniform(&program, i).unwrap();
      let uniform_name = uniform.name();
      let location_opt = self.context.get_uniform_location(&program, uniform_name.as_str());
      if location_opt.is_none() {
        continue;
      }

      let mut uniform_name_parts = uniform_name.split("_"); // name should be like this: "res_X_X"
      uniform_name_parts.next();
      let set = uniform_name_parts.next().unwrap();
      let descriptor_set_binding = uniform_name_parts.next().unwrap();
      let frequency = match set.parse::<u32>().unwrap() {
        0 => BindingFrequency::VeryFrequent,
        1 => BindingFrequency::Frequent,
        2 => BindingFrequency::Frame,
        _ => panic!("Invalid binding frequency")
      };
      uniform_map.insert((frequency, descriptor_set_binding.parse::<u32>().unwrap()), WebGLTextureUniformInfo {
        uniform_location: location_opt.unwrap(),
        texture_unit: uniform_map.len() as u32
      });
    }

    let gl_draw_mode = match &info.primitive_type {
        PrimitiveType::Triangles => WebGl2RenderingContext::TRIANGLES,
        PrimitiveType::TriangleStrip => WebGl2RenderingContext::TRIANGLE_STRIP,
        PrimitiveType::Lines => WebGl2RenderingContext::LINES,
        PrimitiveType::LineStrip => WebGl2RenderingContext::LINE_STRIP,
        PrimitiveType::Points => WebGl2RenderingContext::POINTS,
    };

    let gl_front_face = match info.rasterizer.front_face {
      sourcerenderer_core::graphics::FrontFace::CounterClockwise => WebGl2RenderingContext::CCW,
      sourcerenderer_core::graphics::FrontFace::Clockwise => WebGl2RenderingContext::CW,
    };

    let gl_cull_face = match info.rasterizer.cull_mode {
      sourcerenderer_core::graphics::CullMode::None => 0,
      sourcerenderer_core::graphics::CullMode::Front => WebGl2RenderingContext::FRONT,
      sourcerenderer_core::graphics::CullMode::Back => WebGl2RenderingContext::BACK,
    };

    assert!(self.pipelines.insert(id, Rc::new(WebGLThreadPipeline {
      program,
      context: self.context.clone(),
      gl_draw_mode,
      ubo_infos,
      push_constants_info,
      vao_cache: RefCell::new(HashMap::new()),
      info,
      attribs: attrib_map,
      gl_cull_face,
      gl_front_face,
      texture_uniform_map: uniform_map,
    })).is_none());
  }

  pub fn pipeline(&self, id: PipelineHandle) -> &Rc<WebGLThreadPipeline> {
    self.pipelines.get(&id).expect("Pipeline does not exist")
  }

  pub fn remove_pipeline(&mut self, id: PipelineHandle) {
    self.pipelines.remove(&id).expect("Pipeline does not exist");
  }

  pub fn create_texture(&mut self, id: TextureHandle, info: &TextureInfo) {
    let texture = WebGLThreadTexture::new(&self.context, info);
    assert!(self.textures.insert(id, Rc::new(texture)).is_none());
  }

  pub fn texture(&self, id: TextureHandle) -> &WebGLThreadTexture {
    self.textures.get(&id).expect("Texture does not exist")
  }

  pub fn remove_texture(&mut self, id: TextureHandle) {
    self.textures.remove(&id).expect("Texture does not exist");
  }

  pub fn create_sampler(&mut self, id: SamplerHandle, info: &SamplerInfo) {
    let sampler = WebGLThreadSampler::new(&self.context, info);
    assert!(self.samplers.insert(id, Rc::new(sampler)).is_none());
  }

  pub fn sampler(&self, id: SamplerHandle) -> &WebGLThreadSampler {
    self.samplers.get(&id).expect("Sampler does not exist")
  }

  pub fn remove_sampler(&mut self, id: SamplerHandle) {
    self.samplers.remove(&id).expect("Texture does not exist");
  }

  pub fn process(&mut self) {
    let queue = self.thread_queue.clone();
    queue.swap_buffers();
    let mut read_queue = queue.read_queue();
    if read_queue.is_empty() {
      // log::warn!("No WebGL calls to process on the main thread. The render thread is too slow.");
    } else {
      for cmd in read_queue.drain(..) {
        cmd(self);
      }
    }
  }

  pub fn get_framebuffer(&mut self, rts: &[Option<WebGLTextureHandleView>; 8], ds: Option<WebGLTextureHandleView>, ds_format: Format) -> WebGlFramebuffer {
    let key = FboKey {
      rts: rts.clone(),
      ds: ds.clone()
    };

    let fbo = self.fbo_cache.get(&key);
    if let Some(fbo) = fbo {
      return fbo.clone();
    }

    let fbo = self.context.create_framebuffer().unwrap();
    self.context.bind_framebuffer(WebGl2RenderingContext::DRAW_FRAMEBUFFER, Some(&fbo));
    for (index, rt) in rts.iter().enumerate() {
      if rt.is_none() {
        continue;
      }
      let rt = rt.as_ref().unwrap();
      assert_eq!(rt.mip, 0); // Stupid WebGL restriction.
      let rt_texture = self.texture(rt.texture);
      let target = if rt.array_layer == 0 { WebGl2RenderingContext::TEXTURE_2D } else { WebGl2RenderingContext::TEXTURE_CUBE_MAP_POSITIVE_X + rt.array_layer };
      self.context.framebuffer_texture_2d(WebGl2RenderingContext::DRAW_FRAMEBUFFER, WebGl2RenderingContext::COLOR_ATTACHMENT0 + index as u32, target, Some(&rt_texture.texture), rt.mip as i32);
    }

    if let Some(ds) = ds {
      assert_eq!(ds.mip, 0); // Stupid WebGL restriction.
      let ds_texture = self.texture(ds.texture);
      let target = if ds.array_layer == 0 { WebGl2RenderingContext::TEXTURE_2D } else { WebGl2RenderingContext::TEXTURE_CUBE_MAP_POSITIVE_X + ds.array_layer };
      let attachment = if ds_format.is_depth() && ds_format.is_stencil() {
        WebGl2RenderingContext::DEPTH_STENCIL_ATTACHMENT
      } else if ds_format.is_depth() {
        WebGl2RenderingContext::DEPTH_ATTACHMENT
      } else {
        WebGl2RenderingContext::STENCIL_ATTACHMENT
      };
      self.context.framebuffer_texture_2d(WebGl2RenderingContext::DRAW_FRAMEBUFFER, attachment, target, Some(&ds_texture.texture), ds.mip as i32);
    }

    assert!(self.context.is_framebuffer(Some(&fbo)));
    assert_eq!(self.context.check_framebuffer_status(WebGl2RenderingContext::DRAW_FRAMEBUFFER), WebGl2RenderingContext::FRAMEBUFFER_COMPLETE);
    self.fbo_cache.insert(key, fbo.clone());
    fbo
  }
}

impl Drop for WebGLThreadDevice {
  fn drop(&mut self) {
    self.textures.clear();
    self.buffers.clear();
    self.shaders.clear();
    self.pipelines.clear();
    for (_key, fbo) in self.fbo_cache.drain() {
      self.context.delete_framebuffer(Some(&fbo));
    }
  }
}

impl Deref for WebGLThreadDevice {
  type Target = WebGl2RenderingContext;

  fn deref(&self) -> &Self::Target {
    &self.context
  }
}
