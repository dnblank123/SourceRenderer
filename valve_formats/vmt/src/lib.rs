mod read_util;

use std::collections::HashMap;
use std::io::{Read, Result as IOResult, Error as IOError};
use crate::read_util::RawDataRead;

pub const SHADER_LIGHT_MAPPED_GENERIC: &'static str = "lightmappedGeneric";
const BASE_TEXTURE_NAME: &'static str = "basetexture";
const PATCH: &'static str = "patch";

#[derive(Debug)]
pub enum VMTError {
  IOError(IOError),
  FileError(String)
}

pub struct VMTMaterial {
  shader_name: String,
  values: HashMap<String, String>
}

impl VMTMaterial {
  pub fn new(mut reader: &mut Read, length: u32) -> Result<Self, VMTError> {
    let mut values = HashMap::<String, String>::new();

    let data = reader.read_data(length as usize).map_err(|e| VMTError::IOError(e))?;
    let mut text = String::from_utf8(data.to_vec()).map_err(|e| VMTError::FileError("Could not read text".to_string()))?;
    text = text.replace("\r\n", "\n");
    text = text.replace('\t', " ");
    let block_start = text.find('{').ok_or_else(|| VMTError::FileError("Could not find start of material block".to_string()))?;
    let shader_name = (&text[0 .. block_start]).replace("\"", "").trim().to_string();

    if shader_name == PATCH {
      return Err(VMTError::FileError("Patch materials aren't supported".to_string()));
    }

    let block_end = text.find('}').ok_or_else(|| VMTError::FileError("Could not find end of material block".to_string()))?;
    let block = &text[block_start .. block_end];
    let lines = block.split("\n");
    for line in lines {
      let trimmed_line = line.trim().replace(&['$', '%', '"', '\''][..], "");
      let key_end_opt = trimmed_line.find(' ');
      if key_end_opt.is_none() {
        continue;
      }
      let key_end = key_end_opt.unwrap();
      let key = (&trimmed_line[.. key_end]).trim().to_lowercase();
      let value = (&trimmed_line[key_end + 1 ..]).trim().to_string();
      values.insert(key, value);
    }

    Ok(Self {
      shader_name,
      values
    })
  }

  pub fn get_value(&self, key: &str) -> Option<&str> {
    self.values.get(key).map(|v| v.as_str())
  }

  pub fn get_shader(&self) -> &str {
    self.shader_name.as_str()
  }

  pub fn get_base_texture_name(&self) -> Option<&str> {
    self.get_value(BASE_TEXTURE_NAME)
  }
}
