use std::collections::HashMap;
use std::fs::*;
use std::path::*;
use std::process::Command;

pub fn compile_shaders<F>(source_dir: &Path, out_dir: &Path, include_debug_info: bool, as_c_headers: bool, arguments: &HashMap<String, String>, file_filter: F)
  where F: Fn(&Path) -> bool {
  println!("cargo:rerun-if-changed={}", source_dir.to_str().unwrap());
  let contents = read_dir(&source_dir).expect("Shader directory couldn't be opened.");
  contents
    .filter(|file_result| file_result.is_ok())
    .map(|file_result| file_result.unwrap())
    .filter(|file|
      file.path().extension().and_then(|os_str| os_str.to_str()).unwrap_or("") == "glsl"
      && !file.path().file_stem().and_then(|ext| ext.to_str()).map(|s| s.contains(".inc")).unwrap_or(false)
      && file_filter(&file.path())
    )
    .for_each(|file| {
      println!("cargo:rerun-if-changed={}", (&file.path()).to_str().unwrap());

      let mut is_rt = false;
      let mut assume_compute = false;
      let path = file.path();
      if let Some(path) = path.to_str() {
        is_rt = path.contains(".rchit") || path.contains(".rgen") || path.contains(".rmiss");
        assume_compute = !is_rt && !path.contains(".comp") && !path.contains(".frag") && !path.contains(".vert");
      }

      let file_stem = path.file_stem().unwrap().to_str().unwrap();
      let generated_file_type = if !as_c_headers { ".spv" } else { ".h" };
      let compiled_file_path = Path::join(out_dir, [file_stem, generated_file_type].concat());
      let mut command = Command::new("glslangValidator");
      command
        .arg("--target-env")
        .arg(if is_rt { "spirv1.4" } else { "spirv1.3" })
        .arg("-V");

      if as_c_headers {
        command
          .arg("--vn")
          .arg(&file_stem);
      }

      if include_debug_info {
        command.arg("-g");
      }
      if assume_compute {
        command.arg("-S")
          .arg("comp");
      }

      for (key, value) in arguments {
        if !value.is_empty() {
          command.arg("-D".to_string() + key + "=" + value);
        } else {
          command.arg("-D".to_string() + key);
        }
      }

      command
       .arg("-o")
       .arg(&compiled_file_path)
       .arg(&path);

      let output = command
      .output()
      .unwrap_or_else(|e| panic!("Failed to compile shader: {}\n{}", path.to_str().unwrap(), e.to_string()));

      if !output.status.success() {
        panic!("Failed to compile shader: {}\n{:?}\n", path.to_str().unwrap(), output);
      }
    }
  );
}
