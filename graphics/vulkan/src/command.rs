use std::sync::{Arc, Mutex};

use ash::vk;
use ash::version::DeviceV1_0;

use sourcerenderer_core::graphics::{CommandPool, PipelineInfo, PipelineInfo2, Backend};
use sourcerenderer_core::graphics::CommandBuffer;
use sourcerenderer_core::graphics::CommandBufferType;
use sourcerenderer_core::graphics::RenderPass;
use sourcerenderer_core::graphics::RenderpassRecordingMode;
use sourcerenderer_core::graphics::Viewport;
use sourcerenderer_core::graphics::Scissor;
use sourcerenderer_core::graphics::Resettable;

use sourcerenderer_core::pool::Recyclable;
use std::sync::mpsc::{ Sender, Receiver, channel };

use crate::VkQueue;
use crate::VkDevice;
use crate::raw::RawVkDevice;
use crate::VkRenderPass;
use crate::VkBuffer;
use crate::VkPipeline;
use crate::VkBackend;

use crate::raw::*;
use pipeline::VkPipelineInfo;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use VkRenderPassLayout;
use context::{VkGraphicsContext, VkSharedCaches};
use bitflags::_core::ops::{Deref, DerefMut};
use std::marker::PhantomData;
use bitflags::_core::mem::MaybeUninit;

pub struct VkCommandPool {
  raw: Arc<RawVkCommandPool>,
  buffers: Vec<Box<VkCommandBuffer>>,
  receiver: Receiver<Box<VkCommandBuffer>>,
  sender: Sender<Box<VkCommandBuffer>>,
  caches: Arc<VkSharedCaches>
}

impl VkCommandPool {
  pub fn new(device: &Arc<RawVkDevice>, queue_family_index: u32, caches: &Arc<VkSharedCaches>) -> Self {
    let create_info = vk::CommandPoolCreateInfo {
      queue_family_index,
      flags: vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER,
      ..Default::default()
    };

    let (sender, receiver) = channel();

    return Self {
      raw: Arc::new(RawVkCommandPool::new(device, &create_info).unwrap()),
      buffers: Vec::new(),
      receiver,
      sender,
      caches: caches.clone()
    };
  }
}

impl CommandPool<VkBackend> for VkCommandPool {
  fn get_command_buffer(&mut self, command_buffer_type: CommandBufferType) -> VkCommandBufferRecorder {
    let mut buffer = self.buffers.pop().unwrap_or_else(|| Box::new(VkCommandBuffer::new(&self.raw.device, &self.raw, command_buffer_type, &self.caches)));
    buffer.begin();
    return VkCommandBufferRecorder::new(buffer, self.sender.clone());
  }
}

impl Resettable for VkCommandPool {
  fn reset(&mut self) {
    unsafe {
      self.raw.device.reset_command_pool(**self.raw, vk::CommandPoolResetFlags::empty()).unwrap();
    }

    for mut cmd_buf in self.receiver.try_iter() {
      cmd_buf.reset();
      self.buffers.push(cmd_buf);
    }
  }
}

#[derive(PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub enum VkCommandBufferState {
  Ready,
  Recording,
  Finished,
  Submitted
}

struct VkLifetimeTrackers {
  buffers: Vec<Arc<VkBuffer>>
}

struct VkCommandBuffer {
  buffer: vk::CommandBuffer,
  pool: Arc<RawVkCommandPool>,
  device: Arc<RawVkDevice>,
  caches: Arc<VkSharedCaches>,
  render_pass: Option<Arc<VkRenderPassLayout>>,
  sub_pass: u32,
  state: VkCommandBufferState,
  trackers: VkLifetimeTrackers
}

impl VkCommandBuffer {
  fn new(device: &Arc<RawVkDevice>, pool: &Arc<RawVkCommandPool>, command_buffer_type: CommandBufferType, caches: &Arc<VkSharedCaches>) -> Self {
    let buffers_create_info = vk::CommandBufferAllocateInfo {
      command_pool: ***pool,
      level: if command_buffer_type == CommandBufferType::PRIMARY { vk::CommandBufferLevel::PRIMARY } else { vk::CommandBufferLevel::SECONDARY }, // TODO: support secondary command buffers / bundles
      command_buffer_count: 1, // TODO: figure out how many buffers per pool (maybe create a new pool once we've run out?)
      ..Default::default()
    };
    let mut buffers = unsafe { device.allocate_command_buffers(&buffers_create_info) }.unwrap();
    return VkCommandBuffer {
      buffer: buffers.pop().unwrap(),
      pool: pool.clone(),
      device: device.clone(),
      render_pass: None,
      sub_pass: 0u32,
      caches: caches.clone(),
      state: VkCommandBufferState::Ready,
      trackers: VkLifetimeTrackers {
        buffers: Vec::new()
      }
    };
  }

  pub fn get_handle(&self) -> &vk::CommandBuffer {
    return &self.buffer;
  }

  fn reset(&mut self) {
    self.state = VkCommandBufferState::Ready;
    self.trackers.buffers.clear();
  }

  fn begin(&mut self) {
    assert_eq!(self.state, VkCommandBufferState::Ready);
    self.state = VkCommandBufferState::Recording;
    unsafe {
      let begin_info = vk::CommandBufferBeginInfo {
        ..Default::default()
      };
      self.device.begin_command_buffer(self.buffer, &begin_info);
    }
  }

  fn end(&mut self) {
    assert_eq!(self.state, VkCommandBufferState::Recording);
    self.state = VkCommandBufferState::Finished;
    unsafe {
      self.device.end_command_buffer(self.buffer);
    }
  }

  fn set_pipeline2(&mut self, pipeline: &PipelineInfo2<VkBackend>) {
    debug_assert_eq!(self.state, VkCommandBufferState::Recording);
    if self.render_pass.is_none() {
      panic!("Cant set pipeline outside of render pass");
    }

    let render_pass = self.render_pass.clone().unwrap();

    let info = VkPipelineInfo {
      info: pipeline,
      render_pass: &render_pass,
      sub_pass: self.sub_pass
    };

    let mut hasher = DefaultHasher::new();
    info.hash(&mut hasher);
    let hash = hasher.finish();

    {
      let lock = self.caches.get_pipelines().read().unwrap();
      let cached_pipeline = lock.get(&hash);
      if let Some(pipeline) = cached_pipeline {
        let vk_pipeline = *pipeline.get_handle();
        unsafe {
          self.device.cmd_bind_pipeline(self.buffer, vk::PipelineBindPoint::GRAPHICS, vk_pipeline);
        }
        return;
      }
    }
    let pipeline = VkPipeline::new2(&self.device, &info);
    let mut lock = self.caches.get_pipelines().write().unwrap();
    lock.insert(hash, pipeline);
    let stored_pipeline = lock.get(&hash).unwrap();
    let vk_pipeline = *stored_pipeline.get_handle();
    unsafe {
      self.device.cmd_bind_pipeline(self.buffer, vk::PipelineBindPoint::GRAPHICS, vk_pipeline);
    }
  }

  fn begin_render_pass(&mut self, renderpass: &VkRenderPass, recording_mode: RenderpassRecordingMode) {
    debug_assert_eq!(self.state, VkCommandBufferState::Recording);
    unsafe {
      let begin_info = vk::RenderPassBeginInfo {
        framebuffer: *renderpass.get_framebuffer(),
        render_pass: *renderpass.get_layout().get_handle(),
        render_area: vk::Rect2D {
          offset: vk::Offset2D { x: 0i32, y: 0i32 },
          extent: vk::Extent2D { width: renderpass.get_info().width, height: renderpass.get_info().height }
        },
        clear_value_count: 1,
        p_clear_values: &[
          vk::ClearValue {
            color: vk::ClearColorValue {
              float32: [0.0f32, 0.0f32, 0.0f32, 1.0f32]
            }
          },
          vk::ClearValue {
            depth_stencil: vk::ClearDepthStencilValue {
              depth: 0.0f32,
              stencil: 0u32
            }
          }
        ] as *const vk::ClearValue,
        ..Default::default()
      };
      self.device.cmd_begin_render_pass(self.buffer, &begin_info, if recording_mode == RenderpassRecordingMode::Commands { vk::SubpassContents::INLINE } else { vk::SubpassContents::SECONDARY_COMMAND_BUFFERS });
    }
    self.render_pass = Some(renderpass.get_layout().clone());
    self.sub_pass = 0;
  }

  fn end_render_pass(&mut self) {
    debug_assert_eq!(self.state, VkCommandBufferState::Recording);
    unsafe {
      self.device.cmd_end_render_pass(self.buffer);
    }
    self.render_pass = None;
  }

  fn set_pipeline(&mut self, pipeline: Arc<VkPipeline>) {
    debug_assert_eq!(self.state, VkCommandBufferState::Recording);
    unsafe {
      self.device.cmd_bind_pipeline(self.buffer, vk::PipelineBindPoint::GRAPHICS, *pipeline.get_handle());
    }
  }

  fn set_vertex_buffer(&mut self, vertex_buffer: Arc<VkBuffer>) {
    debug_assert_eq!(self.state, VkCommandBufferState::Recording);
    self.trackers.buffers.push(vertex_buffer.clone());
    unsafe {
      self.device.cmd_bind_vertex_buffers(self.buffer, 0, &[*(*vertex_buffer).get_handle()], &[0]);
    }
  }

  fn set_viewports(&mut self, viewports: &[ Viewport ]) {
    debug_assert_eq!(self.state, VkCommandBufferState::Recording);
    unsafe {
      for i in 0..viewports.len() {
        self.device.cmd_set_viewport(self.buffer, i as u32, &[vk::Viewport {
          x: viewports[i].position.x,
          y: viewports[i].position.y,
          width: viewports[i].extent.x,
          height: viewports[i].extent.y,
          min_depth: viewports[i].min_depth,
          max_depth: viewports[i].max_depth
        }]);
      }
    }
  }

  fn set_scissors(&mut self, scissors: &[ Scissor ])  {
    debug_assert_eq!(self.state, VkCommandBufferState::Recording);
    unsafe {
      let vk_scissors: Vec<vk::Rect2D> = scissors.iter().map(|scissor| vk::Rect2D {
        offset: vk::Offset2D {
          x: scissor.position.x,
          y: scissor.position.y
        },
        extent: vk::Extent2D {
          width: scissor.extent.x,
          height: scissor.extent.y
        }
      }).collect();
      self.device.cmd_set_scissor(self.buffer, 0, &vk_scissors);
    }
  }

  fn draw(&mut self, vertices: u32, offset: u32) {
    debug_assert_eq!(self.state, VkCommandBufferState::Recording);
    unsafe {
      self.device.cmd_draw(self.buffer, vertices, 1, offset, 0);
    }
  }
}

pub struct VkCommandBufferRecorder {
  item: MaybeUninit<Box<VkCommandBuffer>>,
  sender: Sender<Box<VkCommandBuffer>>,
  is_submitted: bool,
  phantom: PhantomData<*const u8>
}

impl Drop for VkCommandBufferRecorder {
  fn drop(&mut self) {
    if self.is_submitted {
      return;
    }
    let item = unsafe {
      std::mem::replace(&mut self.item, MaybeUninit::uninit()).assume_init()
    };
    self.sender.send(item);
  }
}

impl VkCommandBufferRecorder {
  fn new(item: Box<VkCommandBuffer>, sender: Sender<Box<VkCommandBuffer>>) -> Self {
    Self {
      item: MaybeUninit::new(item),
      sender,
      is_submitted: false,
      phantom: PhantomData
    }
  }
}

impl CommandBuffer<VkBackend> for VkCommandBufferRecorder {
  fn finish(self) -> VkCommandBufferSubmission {
    assert_eq!(unsafe { (*(self.item.as_ptr())).state }, VkCommandBufferState::Recording);
    let mut mut_self = self;
    unsafe { (*(mut_self.item.as_mut_ptr())).as_mut() }.end();
    mut_self.is_submitted = true;
    let item = unsafe {
      std::mem::replace(&mut mut_self.item, MaybeUninit::uninit()).assume_init()
    };
    VkCommandBufferSubmission::new(item, mut_self.sender.clone())
  }

  #[inline(always)]
  fn set_pipeline(&mut self, pipeline: Arc<VkPipeline>) {
    unsafe { (*(self.item.as_mut_ptr())).as_mut() }.set_pipeline(pipeline);
  }

  #[inline(always)]
  fn set_pipeline2(&mut self, pipeline: &PipelineInfo2<VkBackend>) {
    unsafe { (*(self.item.as_mut_ptr())).as_mut() }.set_pipeline2(pipeline);
  }

  #[inline(always)]
  fn begin_render_pass(&mut self, renderpass: &VkRenderPass, recording_mode: RenderpassRecordingMode) {
    unsafe { (*(self.item.as_mut_ptr())).as_mut() }.begin_render_pass(renderpass, recording_mode);
  }

  #[inline(always)]
  fn end_render_pass(&mut self) {
    unsafe { (*(self.item.as_mut_ptr())).as_mut() }.end_render_pass();
  }

  #[inline(always)]
  fn set_vertex_buffer(&mut self, vertex_buffer: Arc<VkBuffer>) {
    unsafe { (*(self.item.as_mut_ptr())).as_mut() }.set_vertex_buffer(vertex_buffer)
  }

  #[inline(always)]
  fn set_viewports(&mut self, viewports: &[Viewport]) {
    unsafe { (*(self.item.as_mut_ptr())).as_mut() }.set_viewports(viewports);
  }

  #[inline(always)]
  fn set_scissors(&mut self, scissors: &[Scissor]) {
    unsafe { (*(self.item.as_mut_ptr())).as_mut() }.set_scissors(scissors);
  }

  #[inline(always)]
  fn draw(&mut self, vertices: u32, offset: u32) {
    unsafe { (*(self.item.as_mut_ptr())).as_mut() }.draw(vertices, offset);
  }
}

pub struct VkCommandBufferSubmission {
  item: Box<VkCommandBuffer>,
  sender: Sender<Box<VkCommandBuffer>>
}

unsafe impl Send for VkCommandBufferSubmission {}

impl VkCommandBufferSubmission {
  fn new(item: Box<VkCommandBuffer>, sender: Sender<Box<VkCommandBuffer>>) -> Self {
    Self {
      item,
      sender
    }
  }

  pub(crate) fn get_handle(&self) -> &vk::CommandBuffer {
    debug_assert_eq!(self.item.state, VkCommandBufferState::Finished);
    &self.item.buffer
  }
}
