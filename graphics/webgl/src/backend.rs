use sourcerenderer_core::graphics::Backend;
use crate::{WebGLAdapter, WebGLBuffer, WebGLCommandBuffer, WebGLCommandSubmission, WebGLComputePipeline, WebGLDevice, WebGLFence, WebGLGraphicsPipeline, WebGLInstance, WebGLRenderGraph, WebGLRenderGraphTemplate, WebGLShader, WebGLSurface, WebGLSwapchain, WebGLTexture, WebGLTextureShaderResourceView};

#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
pub enum WebGLBackend {}

impl Backend for WebGLBackend {
  type Instance = WebGLInstance;
  type Adapter = WebGLAdapter;
  type Device = WebGLDevice;
  type Surface = WebGLSurface;
  type Swapchain = WebGLSwapchain;
  type CommandBuffer = WebGLCommandBuffer;
  type CommandBufferSubmission = WebGLCommandSubmission;
  type Texture = WebGLTexture;
  type TextureShaderResourceView = WebGLTextureShaderResourceView;
  type Buffer = WebGLBuffer;
  type Shader = WebGLShader;
  type GraphicsPipeline = WebGLGraphicsPipeline;
  type ComputePipeline = WebGLComputePipeline;
  type RenderGraphTemplate = WebGLRenderGraphTemplate;
  type RenderGraph = WebGLRenderGraph;
  type Fence = WebGLFence;
}
