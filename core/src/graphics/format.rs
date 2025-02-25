
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
  Unknown,
  R32UNorm,
  R16UNorm,
  R8Unorm,
  RGBA8UNorm,
  RGBA8Srgb,
  BGR8UNorm,
  BGRA8UNorm,
  DXT1,
  DXT1Alpha,
  DXT3,
  DXT5,
  R16Float,
  R32Float,
  RG32Float,
  RG16Float,
  RGB32Float,
  RGBA32Float,
  RG16UNorm,
  RG8UNorm,
  R32UInt,
  RGBA16Float,
  R11G11B10Float,
  RG16UInt,
  R16UInt,
  R16SNorm,

  D16,
  D16S8,
  D32,
  D32S8,
  D24
}

impl Format {
  pub fn is_depth(&self) -> bool {
    matches!(self,
      Format::D32
      | Format::D16
      | Format::D16S8
      | Format::D24
      | Format::D32S8)
  }

  pub fn is_stencil(&self) -> bool {
    matches!(self,
      Format::D16S8
      | Format::D24
      | Format::D32S8)
  }

  pub fn is_compressed(&self) -> bool {
    matches!(self,
      Format::DXT1
      | Format::DXT1Alpha
      | Format::DXT3
      | Format::DXT5)
  }

  pub fn element_size(&self) -> u32 {
    match self {
      Format::R32Float => 4,
      Format::R16Float => 2,
      Format::RG32Float => 8,
      Format::RGB32Float => 12,
      Format::RGBA32Float => 16,
      _ => todo!()
    }
  }

  pub fn srgb_format(&self) -> Option<Format> {
    match self {
      Format::RGBA8UNorm => Some(Format::RGBA8Srgb),
      _ => None
    }
  }
}
