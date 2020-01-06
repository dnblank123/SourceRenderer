use std::sync::Arc;

use graphics::Queue;
use graphics::Texture;
use graphics::Semaphore;

use graphics::Backend;

pub trait Surface<B: Backend> {

}

pub struct SwapchainInfo {
  pub width: u32,
  pub height: u32,
  pub vsync: bool
}

pub trait Swapchain<B: Backend> {
  fn prepare_back_buffer(&mut self, semaphore: &B::Semaphore) -> (Arc<B::Texture>, u32);
}
