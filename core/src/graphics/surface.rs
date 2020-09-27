use std::sync::Arc;

use crate::graphics::Texture;

use crate::graphics::Backend;

pub trait Surface {

}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwapchainError {
  ZeroExtents,
  Other
}

pub trait Swapchain : Sized {
  fn recreate(old: &Self, width: u32, height: u32) -> Result<Arc<Self>, SwapchainError>;
}
