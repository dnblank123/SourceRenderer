use crate::asset::{AssetLoader, Asset, AssetManager};
use sourcerenderer_core::Platform;
use crate::asset::asset_manager::{AssetFile, AssetLoaderResult, AssetFileData, LoadedAsset, AssetLoaderProgress, AssetLoadPriority};
use std::io::{Cursor, BufReader};
use sourcerenderer_vtf::{VtfTexture, ImageFormat as VTFTextureFormat, ImageFormat, Header as VTFHeader};
use std::fs::File;
use sourcerenderer_core::graphics::{Device, TextureInfo, SampleCount, TextureShaderResourceViewInfo, Filter, AddressMode, MemoryUsage, BufferUsage};
use sourcerenderer_core::graphics::Format;
use sourcerenderer_core::graphics::Backend as GraphicsBackend;
use std::sync::Arc;

pub struct VTFTextureLoader {

}

impl VTFTextureLoader {
  pub fn new() -> Self {
    Self {}
  }

  fn load_texture<P: Platform>(data: &[u8], width: u32, height: u32, format: VTFTextureFormat, device: &Arc<<P::GraphicsBackend as GraphicsBackend>::Device>, priority: AssetLoadPriority) -> (Arc<<P::GraphicsBackend as GraphicsBackend>::TextureShaderResourceView>, Arc<<P::GraphicsBackend as GraphicsBackend>::Fence>) {
    let buffer = device.upload_data_raw(data, MemoryUsage::CpuToGpu, BufferUsage::COPY_SRC);
    let texture = device.create_texture(&TextureInfo {
      format: convert_vtf_texture_format(format),
      width,
      height,
      depth: 1,
      mip_levels: 1,
      array_length: 1,
      samples: SampleCount::Samples1
    }, None);
    let fence = if priority == AssetLoadPriority::Low {
      device.init_texture_async(&texture, &buffer, 0, 0)
    } else {
      device.init_texture(&texture, &buffer, 0, 0)
    };
    let srv = device.create_shader_resource_view(&texture, &TextureShaderResourceViewInfo {
      base_mip_level: 0,
      mip_level_length: 1,
      base_array_level: 0,
      array_level_length: 1,
      mag_filter: Filter::Linear,
      min_filter: Filter::Linear,
      mip_filter: Filter::Linear,
      address_mode_u: AddressMode::Repeat,
      address_mode_v: AddressMode::Repeat,
      address_mode_w: AddressMode::Repeat,
      mip_bias: 0.0,
      max_anisotropy: 0.0,
      compare_op: None,
      min_lod: 0.0,
      max_lod: 0.0
    });
    (srv, fence)
  }
}

impl<P: Platform> AssetLoader<P> for VTFTextureLoader {
  fn matches(&self, file: &mut AssetFile) -> bool {
    if !file.path.ends_with(".vtf") {
      return false;
    }

    match &mut file.data {
      AssetFileData::File(file) => {
        VtfTexture::<File>::check_file(file).unwrap_or(false)
      }
      AssetFileData::Memory(memory) => {
        VtfTexture::<Cursor<Box<[u8]>>>::check_file(memory).unwrap_or(false)
      }
    }
  }

  fn load(&self, file: AssetFile, manager: &AssetManager<P>, priority: AssetLoadPriority, progress: &Arc<AssetLoaderProgress>) -> Result<AssetLoaderResult, ()> {
    let path = file.path.clone();
    let (texture_view, fence) = match file.data {
      AssetFileData::File(file) => {
        let mut texture = VtfTexture::new(BufReader::new(file)).unwrap();
        let mipmap = &texture.read_mip_map(texture.header().mipmap_count as u32 - 1).unwrap();
        VTFTextureLoader::load_texture::<P>(&mipmap.frames[0].faces[0].slices[0].data, mipmap.width, mipmap.height, mipmap.format, manager.graphics_device(), priority)
      }
      AssetFileData::Memory(cursor) => {
        let mut texture = VtfTexture::new(BufReader::new(cursor)).unwrap();
        let mipmap = &texture.read_mip_map(texture.header().mipmap_count as u32 - 1).unwrap();
        VTFTextureLoader::load_texture::<P>(&mipmap.frames[0].faces[0].slices[0].data, mipmap.width, mipmap.height, mipmap.format, manager.graphics_device(), priority)
      }
    };

    manager.add_asset_with_progress(&path, Asset::Texture(texture_view), Some(progress), priority, Some(fence));

    Ok(AssetLoaderResult {
      level: None
    })
  }
}

fn convert_vtf_texture_format(texture_format: VTFTextureFormat) -> Format {
  match texture_format {
    VTFTextureFormat::DXT1 => Format::DXT1,
    VTFTextureFormat::DXT1OneBitAlpha => Format::DXT1Alpha,
    VTFTextureFormat::DXT3 => Format::DXT3,
    VTFTextureFormat::DXT5 => Format::DXT5,
    VTFTextureFormat::RGBA8888 => Format::RGBA8,
    _ => panic!(format!("VTF format {:?} is not supported", texture_format))
  }
}