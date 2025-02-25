mod backend;
mod instance;
mod device;
mod surface;
mod command;
mod texture;
mod buffer;
mod pipeline;
mod sync;
mod raw_context;
mod thread;
mod spinlock;
mod rt;

pub use backend::WebGLBackend;
pub use instance::{WebGLInstance, WebGLAdapter};
pub use device::WebGLDevice;
pub use surface::{WebGLSurface, WebGLSwapchain};
pub use command::{WebGLCommandBuffer, WebGLCommandSubmission};
pub use texture::{WebGLTexture, WebGLTextureSamplingView};
pub(crate) use texture::format_to_internal_gl;
pub use buffer::WebGLBuffer;
pub use pipeline::{WebGLShader, WebGLGraphicsPipeline, WebGLComputePipeline};
pub use sync::WebGLFence;
pub(crate) use raw_context::RawWebGLContext;

pub use thread::WebGLThreadDevice;

use std::sync::Arc;
pub type WebGLWork = Box<dyn FnOnce(&mut crate::thread::WebGLThreadDevice) + Send>;
pub type GLThreadSender = Arc<thread::WebGLThreadQueue>;
