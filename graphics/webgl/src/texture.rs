use std::{rc::Rc, sync::Arc};

use sourcerenderer_core::graphics::{AddressMode, Filter, Format, SamplerInfo, Texture, TextureDepthStencilView, TextureDepthStencilViewInfo, TextureInfo, TextureRenderTargetView, TextureRenderTargetViewInfo, TextureShaderResourceView, TextureShaderResourceViewInfo, TextureUnorderedAccessView};

use web_sys::{WebGl2RenderingContext, WebGlRenderingContext, WebGlTexture as WebGLTextureHandle, WebglCompressedTextureS3tc};

use crate::{GLThreadSender, RawWebGLContext, WebGLBackend, thread::TextureHandle};

enum WebGLTextureInner {
  Internal,
  Explicit {
    handle: crate::thread::TextureHandle,
    sender: GLThreadSender
  }
}

pub struct WebGLTexture {
  inner: WebGLTextureInner,
  info: TextureInfo
}

unsafe impl Send for WebGLTexture {}
unsafe impl Sync for WebGLTexture {}

impl WebGLTexture {
  pub fn new(id: TextureHandle, info: &TextureInfo, sender: &GLThreadSender) -> Self {
    let c_info = info.clone();
    sender.send(Box::new(move |device| {
      device.create_texture(id, &c_info);
    })).unwrap();

    Self {
      inner: WebGLTextureInner::Explicit {
        handle: id,
        sender: sender.clone(),
      },
      info: info.clone()
    }
  }

  pub fn new_internal(info: &TextureInfo) -> Self {
    Self {
      inner: WebGLTextureInner::Internal,
      info: info.clone()
    }
  }

  pub fn handle(&self) -> TextureHandle {
    match &self.inner {
      WebGLTextureInner::Internal => 1,
      WebGLTextureInner::Explicit { handle, ..} => *handle,
    }
  }
}

impl Texture for WebGLTexture {
  fn get_info(&self) -> &TextureInfo {
    &self.info
  }
}

impl Drop for WebGLTexture {
  fn drop(&mut self) {
    match &self.inner {
      WebGLTextureInner::Internal => { /* nothing to do */ },
      WebGLTextureInner::Explicit { handle, sender} => {
        let handle = *handle;
        sender.send(Box::new(move |device| {
          device.remove_texture(handle);
        })).unwrap();
      },
    }
  }
}

impl PartialEq for WebGLTexture {
  fn eq(&self, other: &Self) -> bool {
    match (&self.inner, &other.inner) {
      (WebGLTextureInner::Internal, WebGLTextureInner::Internal) => true,
      (WebGLTextureInner::Explicit { handle, .. }, WebGLTextureInner::Explicit { handle: other_handle, .. }) => handle == other_handle,
      _ => false
    }
  }
}

impl Eq for WebGLTexture {}

pub struct WebGLTextureShaderResourceView {
  texture: Arc<WebGLTexture>,
  info: TextureShaderResourceViewInfo
}

impl WebGLTextureShaderResourceView {
  pub fn new(texture: &Arc<WebGLTexture>, info: &TextureShaderResourceViewInfo) -> Self {
    Self {
      texture: texture.clone(),
      info: info.clone()
    }
  }

  pub fn texture(&self) -> &Arc<WebGLTexture> {
    &self.texture
  }

  pub fn info(&self) -> &TextureShaderResourceViewInfo {
    &self.info
  }
}

impl TextureShaderResourceView<WebGLBackend> for WebGLTextureShaderResourceView {
  fn texture(&self) -> &Arc<WebGLTexture> {
    &self.texture
  }
}

impl PartialEq for WebGLTextureShaderResourceView {
  fn eq(&self, other: &Self) -> bool {
    self.texture == other.texture
  }
}

impl Eq for WebGLTextureShaderResourceView {}

pub struct WebGLRenderTargetView {
  texture: Arc<WebGLTexture>,
  info: TextureRenderTargetViewInfo
}

impl WebGLRenderTargetView {
  pub fn new(texture: &Arc<WebGLTexture>, info: &TextureRenderTargetViewInfo) -> Self {
    Self {
      texture: texture.clone(),
      info: info.clone()
    }
  }

  pub fn texture(&self) -> &Arc<WebGLTexture> {
    &self.texture
  }

  pub fn info(&self) -> &TextureRenderTargetViewInfo {
    &self.info
  }
}

impl TextureRenderTargetView<WebGLBackend> for WebGLRenderTargetView {
  fn texture(&self) -> &Arc<WebGLTexture> {
    &self.texture
  }
}

impl PartialEq for WebGLRenderTargetView {
  fn eq(&self, other: &Self) -> bool {
    self.texture == other.texture
  }
}

impl Eq for WebGLRenderTargetView {}

pub struct WebGLDepthStencilView {
  texture: Arc<WebGLTexture>,
  info: TextureDepthStencilViewInfo
}

impl WebGLDepthStencilView {
  pub fn new(texture: &Arc<WebGLTexture>, info: &TextureDepthStencilViewInfo) -> Self {
    Self {
      texture: texture.clone(),
      info: info.clone()
    }
  }

  pub fn texture(&self) -> &Arc<WebGLTexture> {
    &self.texture
  }

  pub fn info(&self) -> &TextureDepthStencilViewInfo {
    &self.info
  }
}

impl TextureDepthStencilView<WebGLBackend> for WebGLDepthStencilView {
  fn texture(&self) -> &Arc<WebGLTexture> {
    &self.texture
  }
}

impl PartialEq for WebGLDepthStencilView {
  fn eq(&self, other: &Self) -> bool {
    self.texture == other.texture
  }
}

impl Eq for WebGLDepthStencilView {}

pub struct WebGLUnorderedAccessView {}

impl TextureUnorderedAccessView<WebGLBackend> for WebGLUnorderedAccessView {
  fn texture(&self) -> &Arc<WebGLTexture> {
    panic!("WebGL does not support storage textures")
  }
}

impl PartialEq for WebGLUnorderedAccessView {
  fn eq(&self, other: &Self) -> bool {
    true
  }
}

impl Eq for WebGLUnorderedAccessView {}

pub struct WebGLSampler {

}

impl WebGLSampler {
  pub fn new(info: &SamplerInfo) -> Self {
    Self {} 
  }
}

pub(crate) fn format_to_type(_format: Format) -> u32 {
  WebGl2RenderingContext::UNSIGNED_BYTE
}

pub(crate) fn format_to_internal_gl(format: Format) -> u32 {
  match format {
    Format::RGBA8 => WebGl2RenderingContext::RGBA8,
    Format::DXT1 => WebglCompressedTextureS3tc::COMPRESSED_RGB_S3TC_DXT1_EXT,
    Format::DXT1Alpha => WebglCompressedTextureS3tc::COMPRESSED_RGBA_S3TC_DXT1_EXT,
    Format::DXT3 => WebglCompressedTextureS3tc::COMPRESSED_RGBA_S3TC_DXT3_EXT,
    Format::DXT5 => WebglCompressedTextureS3tc::COMPRESSED_RGBA_S3TC_DXT5_EXT,
    _ => panic!("Unsupported texture format")
  }
}

pub(crate) fn format_to_gl(format: Format) -> u32 {
  match format {
    Format::RGBA8 => WebGl2RenderingContext::RGBA,
    Format::DXT1 => WebglCompressedTextureS3tc::COMPRESSED_RGB_S3TC_DXT1_EXT,
    Format::DXT1Alpha => WebglCompressedTextureS3tc::COMPRESSED_RGBA_S3TC_DXT1_EXT,
    Format::DXT3 => WebglCompressedTextureS3tc::COMPRESSED_RGBA_S3TC_DXT3_EXT,
    Format::DXT5 => WebglCompressedTextureS3tc::COMPRESSED_RGBA_S3TC_DXT5_EXT,
    _ => panic!("Unsupported texture format")
  }
}

pub(crate) fn address_mode_to_gl(address_mode: AddressMode) -> u32 {
  match address_mode {
    AddressMode::ClampToBorder => WebGlRenderingContext::CLAMP_TO_EDGE,
    AddressMode::ClampToEdge => WebGlRenderingContext::CLAMP_TO_EDGE,
    AddressMode::Repeat => WebGlRenderingContext::REPEAT,
    AddressMode::MirroredRepeat => WebGlRenderingContext::MIRRORED_REPEAT
  }
}

pub(crate) fn max_filter_to_gl(filter: Filter) -> u32 {
  match filter {
    Filter::Linear => WebGlRenderingContext::LINEAR,
    Filter::Nearest => WebGlRenderingContext::NEAREST,
  }
}

pub(crate) fn min_filter_to_gl(filter: Filter, mip_filter: Filter) -> u32 {
  match (filter, mip_filter) {
    (Filter::Linear, Filter::Linear) => WebGlRenderingContext::LINEAR_MIPMAP_LINEAR,
    (Filter::Linear, Filter::Nearest) => WebGlRenderingContext::LINEAR_MIPMAP_NEAREST,
    (Filter::Nearest, Filter::Linear) => WebGlRenderingContext::NEAREST_MIPMAP_LINEAR,
    (Filter::Nearest, Filter::Nearest) => WebGlRenderingContext::NEAREST_MIPMAP_NEAREST,
  }
}
