# SourceRenderer
Source Engine map renderer written in Rust

It's a Rust port of https://github.com/K0bin/sourceloader and I plan to add a Vulkan renderer.
The main goal of this project is to learn Rust and Vulkan.

What's working:
* extremely basic Vulkan forward renderer
  * temporal anti aliasing
  * SSAO
  * clustered shading (currently only supports point lights)
  * Frustum culling & occlusion culling (based on GPU queries)
  * GPU driven rendering with bindless textures
  * Vulkan ray tracing
    * Soft shadows (denoising is still TODO)
* loading BSP levels
  * basic brush geometry
  * displacements (at least to some degree)
  * light maps
  * static models
* loading 2D VTF textures
* loading the most basic VMT materials
* FPS camera with late latching on the GPU for minimal latency
* loading GLTF levels (currently without textures)

