use std::ops::Deref;

use wasm_bindgen::JsCast;

use web_sys::{WebGl2RenderingContext, WebGlRenderingContext, WebglCompressedTextureS3tc};

use crate::WebGLSurface;

pub struct RawWebGLContext {
  context: WebGlRenderingContext,
  context2: Option<WebGl2RenderingContext>,
  extensions: WebGLExtensions
}

pub struct WebGLExtensions {
  pub compressed_textures: Option<WebglCompressedTextureS3tc>
}

impl RawWebGLContext {
  pub fn new(surface: &WebGLSurface) -> Self {
    let context_obj = surface.canvas().get_context("webgl2").unwrap();
    match context_obj {
      Some(context_obj) => {
        let webgl2_context = context_obj.dyn_into::<WebGl2RenderingContext>().unwrap();
        let webgl_context = webgl2_context.clone().dyn_into::<WebGlRenderingContext>().unwrap();
        Self {
          context: webgl_context,
          context2: Some(webgl2_context),
          extensions: WebGLExtensions {
            compressed_textures: None
          }
        }
      }
      None => {
        let context_obj = surface.canvas().get_context("webgl").unwrap().unwrap();
        let webgl_context = context_obj.dyn_into::<WebGlRenderingContext>().unwrap();
        Self {
          context: webgl_context,
          context2: None,
          extensions: WebGLExtensions {
            compressed_textures: None
          }
        }
      }
    }
  }

  pub fn context2(&self) -> Option<&WebGl2RenderingContext> {
    self.context2.as_ref()
  }

  pub fn extensions(&self) -> &WebGLExtensions {
    &self.extensions
  }
}

impl Deref for RawWebGLContext {
  type Target = WebGlRenderingContext;

  fn deref(&self) -> &Self::Target {
    &self.context
  }
}
