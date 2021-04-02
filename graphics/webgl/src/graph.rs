use sourcerenderer_core::graphics::{RenderGraph, RenderGraphTemplate};

use crate::{WebGLBackend, WebGLSwapchain};

pub struct WebGLRenderGraph {

}

impl RenderGraph<WebGLBackend> for WebGLRenderGraph {
  fn recreate(_old: &Self, _swapchain: &std::sync::Arc<WebGLSwapchain>) -> Self {
    todo!()
  }

  fn render(&mut self) -> Result<(), sourcerenderer_core::graphics::SwapchainError> {
    todo!()
  }

  fn swapchain(&self) -> &std::sync::Arc<WebGLSwapchain> {
    todo!()
  }
}
