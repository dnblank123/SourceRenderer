use lump::{Lump};

use std::io::{Read, Error};
use byteorder::{ReadBytesExt, LittleEndian};

const LUMP_COUNT: usize = 64;

pub struct MapHeader {
  pub identifier: i32,
  pub version: i32,
  pub lumps: [Lump; LUMP_COUNT],
}

impl MapHeader {
  pub fn read(reader: &mut dyn Read) -> Result<MapHeader, Error> {
    let identifier = reader.read_i32::<LittleEndian>()?;
    let version = reader.read_i32::<LittleEndian>()?;
    let mut lumps: [Lump; LUMP_COUNT] = [
      Lump {
        file_offset: 0,
        file_length: 0,
        version: 0,
        four_cc: 0,
      };
      LUMP_COUNT
    ];
    for i in 0..LUMP_COUNT {
      let lump = Lump::read(reader)?;
      lumps[i] = lump;
    }
    return Ok(MapHeader {
      identifier,
      version,
      lumps,
    });
  }
}
