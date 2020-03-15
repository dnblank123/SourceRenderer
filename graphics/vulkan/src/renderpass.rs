use crate::texture::VkRenderTargetView;
use std::sync::Arc;

use ash::vk;
use ash::version::DeviceV1_0;

use sourcerenderer_core::graphics::*;

use crate::VkDevice;
use crate::raw::RawVkDevice;
use crate::pipeline::samples_to_vk;
use crate::format::format_to_vk;
use crate::VkBackend;
use std::hash::{Hash, Hasher};

pub struct VkRenderPass {
  device: Arc<RawVkDevice>,
  render_pass: vk::RenderPass
}

impl VkRenderPass {
  pub fn new(device: &Arc<RawVkDevice>, info: &vk::RenderPassCreateInfo) -> Self {
    Self {
      device: device.clone(),
      render_pass: unsafe { device.create_render_pass(info, None).unwrap() }
    }
  }

  pub fn get_handle(&self) -> &vk::RenderPass {
    return &self.render_pass;
  }
}

impl Drop for VkRenderPass {
  fn drop(&mut self) {
    unsafe {
      self.device.destroy_render_pass(self.render_pass, None);
    }
  }
}

impl Hash for VkRenderPass {
  fn hash<H: Hasher>(&self, state: &mut H) {
    self.render_pass.hash(state);
  }
}

impl PartialEq for VkRenderPass {
  fn eq(&self, other: &Self) -> bool {
    self.render_pass == other.render_pass
  }
}

impl Eq for VkRenderPass {}

pub struct VkFrameBuffer {
  device: Arc<RawVkDevice>,
  frame_buffer: vk::Framebuffer
}

impl VkFrameBuffer {
  pub fn new(device: &Arc<RawVkDevice>, info: &vk::FramebufferCreateInfo) -> Self {
    Self {
      device: device.clone(),
      frame_buffer: unsafe { device.create_framebuffer(info, None).unwrap() }
    }
  }

  pub fn get_handle(&self) -> &vk::Framebuffer {
    &self.frame_buffer
  }
}

impl Drop for VkFrameBuffer {
  fn drop(&mut self) {
    unsafe {
      self.device.destroy_framebuffer(self.frame_buffer, None);
    }
  }
}
