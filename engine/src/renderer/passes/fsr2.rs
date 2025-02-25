use std::{mem::MaybeUninit, ffi::c_void, sync::Arc, collections::{HashMap, VecDeque}, path::PathBuf, io::Read};

use fsr2::*;
use log::warn;
use smallvec::SmallVec;
use sourcerenderer_core::{graphics::{Backend, Device, MemoryUsage, BufferInfo, BufferUsage, TextureDimension, SampleCount, TextureUsage, TextureInfo, Format, Buffer, Texture, CommandBuffer, Barrier, BarrierSync, BarrierAccess, TextureLayout, BarrierTextureRange, ShaderType, ComputePipeline, BindingFrequency, BindingType, TextureViewInfo, WHOLE_BUFFER, PipelineBinding}, atomic_refcell::{AtomicRefCell, AtomicRefMut}, Platform, platform::IO, Vec2, Vec2UI};
use sourcerenderer_core::graphics::Swapchain;
use widestring::{WideCStr, WideCString};
use crate::renderer::drawable::View;
use crate::renderer::passes::taa::halton_point;
use crate::renderer::render_path::FrameInfo;

use crate::renderer::renderer_resources::{HistoryResourceEntry, RendererResources};

pub struct Fsr2Pass<B: Backend> {
  device: Arc<B::Device>,
  context: FfxFsr2Context,
  scratch_context: Arc<AtomicRefCell<ScratchContext<B>>>,
}

impl<B: Backend> Fsr2Pass<B> {
  pub const UPSCALED_TEXTURE_NAME: &'static str = "FSR2Upscaled";

  pub fn new<P: Platform>(device: &Arc<B::Device>, resources: &mut RendererResources<B>, _resolution: Vec2UI, swapchain: &B::Swapchain) -> Self {
    let scratch_context = Arc::new(AtomicRefCell::new(ScratchContext::<B> {
      resources: HashMap::new(),
      next_resource_id: 1,
      dynamic_resources: Vec::new(),
      free_ids: VecDeque::new(),
      jobs: Vec::new(),
      device: device.clone(),
      point_sampler: resources.nearest_sampler().clone(),
      linear_sampler: resources.linear_sampler().clone()
    }));
    let context_size = std::mem::size_of_val(&scratch_context);

    let interface = FfxFsr2Interface {
      fpCreateBackendContext: Some(create_backend_context::<B>),
      fpDestroyBackendContext: Some(destroy_backend_context::<B>),
      fpGetDeviceCapabilities: Some(get_device_capabilities::<B>),
      fpCreateResource: Some(create_resource::<B>),
      fpRegisterResource: Some(register_resource::<B>),
      fpUnregisterResources: Some(unregister_resources::<B>),
      fpGetResourceDescription: Some(get_resource_description::<B>),
      fpDestroyResource: Some(destroy_resource::<B>),
      fpCreatePipeline: Some(create_pipeline::<P, B>),
      fpDestroyPipeline: Some(destroy_pipeline::<B>),
      fpScheduleGpuJob: Some(schedule_render_job::<B>),
      fpExecuteGpuJobs: Some(execute_render_jobs::<B>),
      scratchBuffer: Arc::into_raw(scratch_context.clone()) as *mut c_void,
      scratchBufferSize: context_size as u64,
    };

    resources.create_texture(
      Self::UPSCALED_TEXTURE_NAME,
      &TextureInfo {
        dimension: TextureDimension::Dim2D,
        format: Format::RGBA8UNorm,
        width: swapchain.width(),
        height: swapchain.height(),
        depth: 1,
        mip_levels: 1,
        array_length: 1,
        samples: SampleCount::Samples1,
        usage: TextureUsage::COPY_SRC | TextureUsage::STORAGE,
        supports_srgb: false,
      },
      false
    );

    let fsr_device: *mut B::Device = unsafe { std::mem::transmute(device.as_ref()) };
    let context_desc = FfxFsr2ContextDescription {
      flags: (FfxFsr2InitializationFlagBits_FFX_FSR2_ENABLE_AUTO_EXPOSURE | FfxFsr2InitializationFlagBits_FFX_FSR2_ENABLE_HIGH_DYNAMIC_RANGE) as u32,
      maxRenderSize: FfxDimensions2D {
        width: swapchain.width(),
        height: swapchain.height()
      },
      displaySize: FfxDimensions2D {
        width: swapchain.width(),
        height: swapchain.height()
      },
      callbacks: interface,
      device: fsr_device as FfxDevice
    };

    let mut context = MaybeUninit::<FfxFsr2Context>::uninit();
    unsafe {
      let result = ffxFsr2ContextCreate(context.as_mut_ptr(), &context_desc as *const FfxFsr2ContextDescription);
      assert_eq!(result, FFX_OK);
    }
    let context = unsafe { MaybeUninit::assume_init(context) };
    device.flush_transfers();

    Self {
      device: device.clone(),
      context,
      scratch_context
    }
  }

  pub fn execute(
    &mut self,
    cmd_buffer: &mut B::CommandBuffer,
    resources: &RendererResources<B>,
    input_name: &str,
    depth_name: &str,
    motion_name: &str,
    view: &View,
    frame: &FrameInfo
  ) {
    cmd_buffer.begin_label("FSR2");

    let color_texture = resources.access_texture(
      cmd_buffer,
      input_name,
      &BarrierTextureRange::default(),
      BarrierSync::COMPUTE_SHADER,
      BarrierAccess::SAMPLING_READ,
      TextureLayout::Sampled,
      false,
      HistoryResourceEntry::Current
    ).clone();
    let color_sampling_view = resources.get_sampling_view(
      input_name,
      &TextureViewInfo::default(),
      HistoryResourceEntry::Current
    ).clone();
    let color_storage_view = resources.get_storage_view(
      input_name,
      &TextureViewInfo::default(),
      HistoryResourceEntry::Current
    ).clone();

    let depth_texture = resources.access_texture(
      cmd_buffer,
      depth_name,
      &BarrierTextureRange::default(),
      BarrierSync::COMPUTE_SHADER,
      BarrierAccess::SAMPLING_READ,
      TextureLayout::Sampled,
      false,
      HistoryResourceEntry::Current
    ).clone();
    let depth_sampling_view = resources.get_sampling_view(
      depth_name,
      &TextureViewInfo::default(),
      HistoryResourceEntry::Current
    ).clone();

    let output_texture = resources.access_texture(
      cmd_buffer,
      Self::UPSCALED_TEXTURE_NAME,
      &BarrierTextureRange::default(),
      BarrierSync::COMPUTE_SHADER,
      BarrierAccess::STORAGE_WRITE,
      TextureLayout::Storage,
      true,
      HistoryResourceEntry::Current
    ).clone();
    let output_sampling_view = resources.get_sampling_view(
      Self::UPSCALED_TEXTURE_NAME,
      &TextureViewInfo::default(),
      HistoryResourceEntry::Current
    ).clone();
    let output_storage_view = resources.get_storage_view(
      Self::UPSCALED_TEXTURE_NAME,
      &TextureViewInfo::default(),
      HistoryResourceEntry::Current
    ).clone();

    let motion_texture = resources.access_texture(
      cmd_buffer,
      motion_name,
      &BarrierTextureRange::default(),
      BarrierSync::COMPUTE_SHADER,
      BarrierAccess::SAMPLING_READ,
      TextureLayout::Sampled,
      false,
      HistoryResourceEntry::Current
    ).clone();
    let motion_sampling_view = resources.get_sampling_view(
      motion_name,
      &TextureViewInfo::default(),
      HistoryResourceEntry::Current
    ).clone();
    let motion_storage_view = resources.get_storage_view(
      motion_name,
      &TextureViewInfo::default(),
      HistoryResourceEntry::Current
    ).clone();

    let aspect_ratio = (output_texture.info().width as f32) / (output_texture.info().height as f32);
    let v_fov = 2f32 * ((view.camera_fov * 0.5f32).tan() * aspect_ratio).atan();

    let halton_point = halton_point((frame.frame % 8u64) as u32); // TODO: use FSR2s built in jitter

    unsafe {
      let desc = FfxFsr2DispatchDescription {
        commandList: command_buffer_into_ffx::<B>(cmd_buffer),
        color: texture_into_ffx::<B>(&color_texture, false, &color_sampling_view, Some(&color_storage_view)),
        depth: texture_into_ffx::<B>(&depth_texture, false, &depth_sampling_view, None),
        exposure: NULL_RESOURCE,
        motionVectors: texture_into_ffx::<B>(&motion_texture, false, &motion_sampling_view, Some(&motion_storage_view)),
        reactive: NULL_RESOURCE,
        transparencyAndComposition: NULL_RESOURCE,
        output: texture_into_ffx::<B>(&output_texture, true, &output_sampling_view, Some(&output_storage_view)),
        renderSize: FfxDimensions2D {
          width: color_texture.info().width,
          height: color_texture.info().height,
        },
        enableSharpening: true,
        sharpness: 0.33f32,
        cameraNear: view.near_plane,
        cameraFar: view.far_plane,
        preExposure: 0.5f32,
        frameTimeDelta: frame.delta.as_secs_f32() * 1000f32,
        cameraFovAngleVertical: v_fov,
        reset: false,
        jitterOffset: FfxFloatCoords2D {
          x: halton_point.x,
          y: halton_point.y
        },
        motionVectorScale: FfxFloatCoords2D {
          x: color_texture.info().width as f32 * -1f32,
          y: color_texture.info().height as f32 * -1f32
        }
      };

      let result = ffxFsr2ContextDispatch(&mut self.context as *mut FfxFsr2Context, &desc as *const FfxFsr2DispatchDescription);
      cmd_buffer.end_label();
      assert_eq!(result, FFX_OK);
    }
  }

  fn jitter(render_dimensions: Vec2UI, frame: u64) -> Vec2 {
    unsafe {
      let jitter_phase_count = ffxFsr2GetJitterPhaseCount(render_dimensions.x as i32, render_dimensions.y as i32);
      let mut jitter = Vec2::new(0f32, 0f32);
      ffxFsr2GetJitterOffset(&mut jitter.x as *mut f32, &mut jitter.y as *mut f32, (frame % (jitter_phase_count as u64)) as i32, jitter_phase_count);
      jitter.x = 2f32 * jitter.x / (render_dimensions.x as f32);
      jitter.y = -2f32 * jitter.y / (render_dimensions.y as f32);
      jitter
    }
  }

  fn scaled_jitter(render_dimensions: Vec2UI, frame: u64) -> Vec2 {
    unsafe {
      let jitter_phase_count = ffxFsr2GetJitterPhaseCount(render_dimensions.x as i32, render_dimensions.y as i32);
      let mut jitter = Vec2::new(0f32, 0f32);
      ffxFsr2GetJitterOffset(&mut jitter.x as *mut f32, &mut jitter.y as *mut f32, (frame % (jitter_phase_count as u64)) as i32, jitter_phase_count);
      jitter
    }
  }
}

impl<B: Backend> Drop for Fsr2Pass<B> {
  fn drop(&mut self) {
    unsafe {
      let result = ffxFsr2ContextDestroy(&mut self.context as *mut FfxFsr2Context);
      assert_eq!(result, FFX_OK);

      Arc::from_raw(Arc::into_raw(self.scratch_context.clone()));
    }
  }
}

unsafe fn device_from_ffx<B: Backend>(device: FfxDevice) -> &'static B::Device {
  std::mem::transmute(&*((device as *mut B::Device) as *const B::Device))
}

unsafe fn command_buffer_from_ffx<B: Backend>(command_list: FfxCommandList) -> &'static mut B::CommandBuffer {
  std::mem::transmute(&mut *(command_list as *mut B::CommandBuffer))
}

unsafe fn command_buffer_into_ffx<B: Backend>(command_buffer: &mut B::CommandBuffer) -> FfxCommandList {
  (command_buffer as *mut B::CommandBuffer) as FfxCommandList
}

struct Fsr2TextureViews<B: Backend> {
  sampling_view: Arc<B::TextureSamplingView>,
  storage_view: Option<Arc<B::TextureStorageView>>,
}

unsafe fn texture_into_ffx<B: Backend>(texture: &Arc<B::Texture>, is_uav: bool, sampling_view: &Arc<B::TextureSamplingView>, mip0_storage_view: Option<&Arc<B::TextureStorageView>>) -> FfxResource {
  let texture_ptr = Arc::into_raw(texture.clone());
  let info = texture.info();
  let views = Box::new(Fsr2TextureViews::<B> {
    sampling_view: sampling_view.clone(),
    storage_view: mip0_storage_view.cloned()
  });
  let views_ptr = Box::into_raw(views);
  FfxResource {
    resource: texture_ptr as *mut c_void,
    state: if !is_uav { FfxResourceStates_FFX_RESOURCE_STATE_COMPUTE_READ } else { FfxResourceStates_FFX_RESOURCE_STATE_UNORDERED_ACCESS },
    isDepth: info.format.is_depth(),
    descriptorData: views_ptr as *mut c_void as u64,
    description: FfxResourceDescription {
      type_: match info.dimension {
        TextureDimension::Dim1D => FfxResourceType_FFX_RESOURCE_TYPE_TEXTURE1D,
        TextureDimension::Dim2D => FfxResourceType_FFX_RESOURCE_TYPE_TEXTURE2D,
        TextureDimension::Dim3D => FfxResourceType_FFX_RESOURCE_TYPE_TEXTURE3D,
      },
      format: format_to_ffx(info.format).unwrap_or(FfxSurfaceFormat_FFX_SURFACE_FORMAT_UNKNOWN),
      width: info.width,
      height: info.height,
      depth: info.depth,
      mipCount: info.mip_levels,
      flags: 0
    },
    name: [0; 64]
  }
}

const NULL_RESOURCE: FfxResource = FfxResource {
  resource: std::ptr::null_mut(),
  state: FfxResourceStates_FFX_RESOURCE_STATE_GENERIC_READ,
  isDepth: false,
  descriptorData: 0,
  description: FfxResourceDescription {
    type_: FfxResourceType_FFX_RESOURCE_TYPE_TEXTURE2D,
    format: FfxSurfaceFormat_FFX_SURFACE_FORMAT_UNKNOWN,
    width: 0,
    height: 0,
    depth: 0,
    mipCount: 0,
    flags: 0
  },
  name: [0; 64]
};

struct TextureSubresourceState {
  sync: BarrierSync,
  access: BarrierAccess,
  layout: TextureLayout,
}

impl Default for TextureSubresourceState {
  fn default() -> Self {
    Self {
      sync: BarrierSync::empty(),
      access: BarrierAccess::empty(),
      layout: TextureLayout::Undefined
    }
  }
}

enum Resource<B: Backend> {
  Texture {
    texture: Arc<B::Texture>,
    sampling_view: Arc<B::TextureSamplingView>,
    storage_views: SmallVec<[Arc<B::TextureStorageView>; 8]>,
    states: SmallVec<[TextureSubresourceState; 8]>,
  },
  Buffer {
    buffer: Arc<B::Buffer>,
    sync: BarrierSync,
    access: BarrierAccess,
  }
}

struct ScratchContext<B: Backend> {
  resources: HashMap<u32, Resource<B>>,
  dynamic_resources: Vec<u32>,
  next_resource_id: u32,
  free_ids: VecDeque<u32>,
  jobs: Vec<FfxGpuJobDescription>,
  device: Arc<B::Device>,
  point_sampler: Arc<B::Sampler>,
  linear_sampler: Arc<B::Sampler>,
}

impl<B: Backend> ScratchContext<B> {
  unsafe fn from_interface(backend_interface: *mut FfxFsr2Interface) -> AtomicRefMut<'static, Self> {
    let scratch = (*backend_interface).scratchBuffer as *mut AtomicRefCell<Self>;
    (*scratch).borrow_mut()
  }

  fn get_new_resource_id(&mut self) -> u32 {
    if let Some(id) = self.free_ids.pop_front() {
      return id;
    }
    let id = self.next_resource_id;
    self.next_resource_id += 1;
    id
  }
}

extern "C" fn create_backend_context<B: Backend>(
  _backend_interface: *mut FfxFsr2Interface,
  _out_device: FfxDevice,
) -> FfxErrorCode {
  //let context = unsafe { (*backend_interface).scratchBuffer as *mut FSR2Context<B> };
  // out_device is a void pointer. Not a pointer to a pointer.
  // No idea how thats supposed to work.
  return FFX_OK;
}

unsafe extern "C" fn create_resource<B: Backend>(
  backend_interface: *mut FfxFsr2Interface,
  desc: *const FfxCreateResourceDescription,
  out_resource: *mut FfxResourceInternal,
) -> FfxErrorCode {
  let mut context = ScratchContext::<B>::from_interface(backend_interface);
  let desc = &*desc;

  let resource_id = context.get_new_resource_id();
  (*out_resource).internalIndex = resource_id as i32;

  let device = &context.device;

  let name = if desc.name != std::ptr::null_mut() {
    Some(WideCStr::from_ptr_str(desc.name).to_string().unwrap())
  } else {
    None
  };

  let resource_desc = &desc.resourceDescription;

  let type_ = resource_desc.type_;
  if type_ == FfxResourceType_FFX_RESOURCE_TYPE_BUFFER {
    let mut buffer_usage = if desc.usage == FfxResourceUsage_FFX_RESOURCE_USAGE_UAV {
      BufferUsage::COPY_SRC | BufferUsage::COPY_DST | BufferUsage::STORAGE
    } else {
      BufferUsage::CONSTANT
    };
    if desc.initData != std::ptr::null_mut() {
      buffer_usage |= BufferUsage::COPY_SRC | BufferUsage::COPY_DST;
    }
    let memory_usage = if desc.heapType == FfxHeapType_FFX_HEAP_TYPE_DEFAULT {
      MemoryUsage::VRAM
    } else {
      MemoryUsage::MappableVRAM
    };

    let buffer = device.create_buffer(&BufferInfo {
        size: resource_desc.width as usize,
        usage: buffer_usage,
      },
      memory_usage,
      if let Some(name) = name.as_ref() { Some (name) } else { None });

      if memory_usage != MemoryUsage::VRAM && desc.initData != std::ptr::null_mut() {
        let dst = buffer.map_unsafe(false).unwrap();
        std::ptr::copy(desc.initData as *mut u8, dst, desc.initDataSize as usize);
        buffer.unmap_unsafe(true);
      } else {
        let init_data = std::slice::from_raw_parts(desc.initData as *const u8, desc.initDataSize as usize);
        let src_buffer = device.upload_data(init_data, MemoryUsage::MappableVRAM, BufferUsage::COPY_SRC);
        device.init_buffer(&src_buffer, &buffer, 0, 0, desc.initDataSize as usize);
      }

    context.resources.insert(resource_id, Resource::Buffer {
      buffer,
      sync: BarrierSync::empty(),
      access: BarrierAccess::empty()
    });
  } else {
    let dimen = if resource_desc.type_ == FfxResourceType_FFX_RESOURCE_TYPE_TEXTURE1D {
      TextureDimension::Dim1D
    } else if resource_desc.type_ == FfxResourceType_FFX_RESOURCE_TYPE_TEXTURE3D {
      TextureDimension::Dim3D
    } else {
      TextureDimension::Dim2D
    };
    let mut texture_usage = TextureUsage::SAMPLED;
    if (desc.usage & FfxResourceUsage_FFX_RESOURCE_USAGE_UAV) != 0 {
      texture_usage |= TextureUsage::STORAGE | TextureUsage::COPY_DST;
    }
    if (desc.usage & FfxResourceUsage_FFX_RESOURCE_USAGE_RENDERTARGET) != 0 {
      texture_usage |= TextureUsage::RENDER_TARGET;
    }
    if desc.initData != std::ptr::null_mut() {
      texture_usage |= TextureUsage::COPY_DST;
    }

    let mut mip_count = resource_desc.mipCount;
    if mip_count == 0 {
      mip_count = ((resource_desc.width.max(resource_desc.height) as f32).log2() as u32) + 1;
    }

    let texture = device.create_texture(&TextureInfo {
      dimension: dimen,
      width: resource_desc.width,
      height: resource_desc.height,
      depth: resource_desc.depth,
      mip_levels: mip_count,
      array_length: 1,
      samples: SampleCount::Samples1,
      usage: texture_usage,
      format: ffx_to_format(resource_desc.format),
      supports_srgb: false,
    }, if let Some(name) = name.as_ref() { Some (name) } else { None });

    if desc.initData != std::ptr::null_mut() {
      let init_data = std::slice::from_raw_parts(desc.initData as *const u8, desc.initDataSize as usize);
      let src_buffer = device.upload_data(init_data, MemoryUsage::MappableVRAM, BufferUsage::COPY_SRC);
      device.init_texture(&texture, &src_buffer, 0, 0, 0);
    }

    let sampling_name = name.as_ref().map(|name| name.as_str()).unwrap_or("").to_string() + "_sampling";
    let sampling_view = device.create_sampling_view(&texture, &TextureViewInfo::default(),
      if name.is_some() { Some(sampling_name.as_str()) } else { None });

    let mut storage_views = SmallVec::<[Arc<B::TextureStorageView>; 8]>::with_capacity(mip_count as usize);
    let mut states = SmallVec::<[TextureSubresourceState; 8]>::with_capacity(mip_count as usize);
    for i in 0..mip_count {
      let storage_name = format!("{}_storage_{}", name.as_ref().map(|name| name.as_str()).unwrap_or(""), mip_count);
      let storage_view = device.create_storage_view(&texture, &TextureViewInfo {
        format: None,
        mip_level_length: 1,
        array_layer_length: 1,
        base_array_layer: 0,
        base_mip_level: i
      }, if name.is_some() { Some(storage_name.as_str()) } else { None });
      storage_views.push(storage_view);
      states.push(TextureSubresourceState::default());
    }

    context.resources.insert(resource_id, Resource::Texture {
      texture,
      states,
      sampling_view,
      storage_views,
    });
  }

  return FFX_OK;
}

unsafe extern "C" fn register_resource<B: Backend>(
  backend_interface: *mut FfxFsr2Interface,
  in_resource: *const FfxResource,
  out_resource: *mut FfxResourceInternal
) -> FfxErrorCode {
  let mut context = ScratchContext::<B>::from_interface(backend_interface);

  let resource_id = context.get_new_resource_id();
  (*out_resource).internalIndex = resource_id as i32;
  context.dynamic_resources.push(resource_id);

  let resource_desc = &(*in_resource).description;

  let mut sync = BarrierSync::empty();
  let mut access = BarrierAccess::empty();
  let mut layout = TextureLayout::Undefined;
  if ((*in_resource).state & FfxResourceStates_FFX_RESOURCE_STATE_UNORDERED_ACCESS) != 0 {
    assert_eq!(layout, TextureLayout::Undefined);
    layout = TextureLayout::Storage;
    access = BarrierAccess::STORAGE_READ | BarrierAccess::STORAGE_WRITE;
    sync = BarrierSync::COMPUTE_SHADER;
  }
  if ((*in_resource).state & FfxResourceStates_FFX_RESOURCE_STATE_COMPUTE_READ) != 0 {
    assert_eq!(layout, TextureLayout::Undefined);
    layout = TextureLayout::Sampled;
    access = BarrierAccess::STORAGE_READ | BarrierAccess::SAMPLING_READ | BarrierAccess::CONSTANT_READ;
    sync = BarrierSync::COMPUTE_SHADER;
  }
  if ((*in_resource).state & FfxResourceStates_FFX_RESOURCE_STATE_COPY_SRC) != 0 {
    assert_eq!(layout, TextureLayout::Undefined);
    layout = TextureLayout::CopySrc;
    access = BarrierAccess::COPY_READ;
    sync = BarrierSync::COPY;
  }
  if ((*in_resource).state & FfxResourceStates_FFX_RESOURCE_STATE_COPY_DEST) != 0 {
    assert_eq!(layout, TextureLayout::Undefined);
    layout = TextureLayout::CopyDst;
    access = BarrierAccess::COPY_WRITE;
    sync = BarrierSync::COPY;
  }

  let type_ = resource_desc.type_;
  if type_ != FfxResourceType_FFX_RESOURCE_TYPE_BUFFER {
    let texture = Arc::<B::Texture>::from_raw((*in_resource).resource as *mut B::Texture);
    let ptr: *mut Fsr2TextureViews<B> = std::mem::transmute((*in_resource).descriptorData);
    let views_box = Box::from_raw(ptr);
    let views = *views_box;

    let mut storage_views = SmallVec::<[Arc<B::TextureStorageView>; 8]>::new();
    let mut states = SmallVec::<[TextureSubresourceState; 8]>::new();
    let Fsr2TextureViews {
      sampling_view, storage_view
    } = views;

    if let Some(storage_view) = storage_view {
      storage_views.push(storage_view);
    }
    states.push(TextureSubresourceState {
      layout,
      access,
      sync
    });

    context.resources.insert(resource_id, Resource::Texture {
      texture, sampling_view: sampling_view, storage_views, states
    });
  } else {
    unimplemented!("FSR2 never registers buffers")
  }

  FFX_OK
}

unsafe extern "C" fn destroy_backend_context<B: Backend>(
  backend_interface: *mut FfxFsr2Interface) ->FfxErrorCode {
  let ptr = (*backend_interface).scratchBuffer as *mut AtomicRefCell<ScratchContext<B>>;
  let _ = Arc::from_raw(ptr);

  FFX_OK
}

unsafe extern "C" fn destroy_resource<B: Backend>(
  backend_interface: *mut FfxFsr2Interface,
  resource: FfxResourceInternal,
) -> FfxErrorCode {
  let mut context = ScratchContext::<B>::from_interface(backend_interface);
  let id = resource.internalIndex as u32;
  context.free_ids.push_back(id);
  return if context.resources.remove(&id).is_some() {
    FFX_OK
  } else {
    warn!("Trying to recycle invalid pointer with id {} in FSR2 integration.", id);
    FFX_ERROR_INVALID_POINTER
  };
}

unsafe extern "C" fn unregister_resources<B: Backend>(
  backend_interface: *mut FfxFsr2Interface
) -> FfxErrorCode {
  let mut context = ScratchContext::<B>::from_interface(backend_interface);
  let mut freed_resources = SmallVec::<[u32; 16]>::with_capacity(context.dynamic_resources.len());

  for resource_id in context.dynamic_resources.drain(..) {
    freed_resources.push(resource_id);
  }

  for resource_id in freed_resources {
    context.resources.remove(&resource_id);
    context.free_ids.push_back(resource_id);
  }

  FFX_OK
}

unsafe extern "C" fn get_resource_description<B: Backend>(
  backend_interface: *mut FfxFsr2Interface,
  resource: FfxResourceInternal
) -> FfxResourceDescription {
  let context = ScratchContext::<B>::from_interface(backend_interface);
  let internal_resource = context.resources.get(&(resource.internalIndex as u32)).unwrap();
  match internal_resource {
    Resource::Texture { texture, .. } => {
      let info = texture.info();
      FfxResourceDescription {
        type_: match info.dimension {
          TextureDimension::Dim1D => FfxResourceType_FFX_RESOURCE_TYPE_TEXTURE1D,
          TextureDimension::Dim2D => FfxResourceType_FFX_RESOURCE_TYPE_TEXTURE2D,
          TextureDimension::Dim3D => FfxResourceType_FFX_RESOURCE_TYPE_TEXTURE3D,
        },
        format: format_to_ffx(info.format).unwrap_or_else(|| panic!("Unsupported format: {:?}", info.format)),
        width: info.width,
        height: info.height,
        depth: info.depth,
        mipCount: info.mip_levels,
        flags: FfxResourceFlags_FFX_RESOURCE_FLAGS_NONE,
      }
    },
    Resource::Buffer { buffer, .. } =>
      FfxResourceDescription {
        type_: FfxResourceType_FFX_RESOURCE_TYPE_BUFFER,
        format: FfxSurfaceFormat_FFX_SURFACE_FORMAT_UNKNOWN,
        width: buffer.info().size as u32,
        height: 0,
        depth: 0,
        mipCount: 0,
        flags: FfxResourceFlags_FFX_RESOURCE_FLAGS_NONE,
      },
  }
}

unsafe extern "C" fn get_device_capabilities<B: Backend>(
  _backend_interface: *mut FfxFsr2Interface,
  capabilities: *mut FfxDeviceCapabilities,
  device: FfxDevice
) -> FfxErrorCode {
  let device = device_from_ffx::<B>(device);
  (*capabilities).raytracingSupported = device.supports_ray_tracing();
  (*capabilities).minimumSupportedShaderModel = FfxShaderModel_FFX_SHADER_MODEL_5_1;
  (*capabilities).waveLaneCountMin = 32;
  (*capabilities).waveLaneCountMax = 32;
  // TODO
  FFX_OK
}

unsafe extern "C" fn schedule_render_job<B: Backend>(
  backend_interface: *mut FfxFsr2Interface,
  job: *const FfxGpuJobDescription
) -> FfxErrorCode {
  let mut context = ScratchContext::<B>::from_interface(backend_interface);
  context.jobs.push((*job).clone());
  FFX_OK
}

unsafe extern "C" fn execute_render_jobs<B: Backend>(
  backend_interface: *mut FfxFsr2Interface,
  command_list: FfxCommandList
) -> FfxErrorCode {
  let mut context = ScratchContext::<B>::from_interface(backend_interface);
  let cmd_buf = command_buffer_from_ffx::<B>(command_list);

  let mut jobs = SmallVec::<[FfxGpuJobDescription; 16]>::new();
  for job in context.jobs.drain(..) {
    jobs.push(job);
  }

  for job in jobs {
    if job.jobType == FfxGpuJobType_FFX_GPU_JOB_COPY {
      let _copy_job = &job.__bindgen_anon_1.clearJobDescriptor;
      unimplemented!("FSR2 never uses copy jobs internally")
    } else if job.jobType == FfxGpuJobType_FFX_GPU_JOB_CLEAR_FLOAT {
      let clear_job = &job.__bindgen_anon_1.clearJobDescriptor;
      execute_clear_job(&clear_job, &mut context, cmd_buf);
    } else if job.jobType == FfxGpuJobType_FFX_GPU_JOB_COMPUTE {
      let compute_job = &job.__bindgen_anon_1.computeJobDescriptor;
      execute_dispatch_job(&compute_job, &mut context, cmd_buf);
    }
  }

  FFX_OK
}

unsafe fn execute_clear_job<B: Backend>(job: &FfxClearFloatJobDescription, context: &mut ScratchContext<B>, cmd_buf: &mut B::CommandBuffer) {
  let resource = context.resources.get_mut(&(job.target.internalIndex as u32)).unwrap();
  match resource {
    Resource::Texture { texture, states, .. } => {
      add_texture_barrier::<B>(cmd_buf, texture, 0, 1, states, BarrierSync::COMPUTE_SHADER, BarrierAccess::STORAGE_WRITE, TextureLayout::Storage);
      cmd_buf.flush_barriers();
      cmd_buf.clear_storage_texture(texture, 0, 0, std::mem::transmute_copy(&job.color))
    }
    Resource::Buffer { .. } => unimplemented!("FSR2 never clears a buffer internally."),
  }
}

unsafe fn execute_dispatch_job<B: Backend>(job: &FfxComputeJobDescription, context: &mut ScratchContext<B>, cmd_buf: &mut B::CommandBuffer) {
  let p_pipeline = job.pipeline.pipeline as *const B::ComputePipeline;
  let pipeline = Arc::from_raw(p_pipeline);
  cmd_buf.set_pipeline(PipelineBinding::Compute(&pipeline));
  std::mem::forget(pipeline);

  for i in 0..job.pipeline.uavCount as usize {
    let uav = &job.uavs[i];
    let resource = context.resources.get_mut(&(uav.internalIndex as u32)).unwrap();
    match resource {
      Resource::Texture {
        texture, states, storage_views, ..
      } => {
        add_texture_barrier::<B>(cmd_buf, texture, job.uavMip[i], 1, states, BarrierSync::COMPUTE_SHADER, BarrierAccess::STORAGE_WRITE, TextureLayout::Storage);
        cmd_buf.bind_storage_texture(BindingFrequency::Frequent, job.pipeline.uavResourceBindings[i].slotIndex, &storage_views[job.uavMip[i] as usize]);
      },
      Resource::Buffer { .. } => unreachable!(),
    }
  }

  for i in 0..job.pipeline.srvCount as usize {
    let srv = &job.srvs[i];
    let resource = context.resources.get_mut(&(srv.internalIndex as u32)).unwrap();
    match resource {
      Resource::Texture {
        texture, states, sampling_view, ..
      } => {
        add_texture_barrier::<B>(cmd_buf, texture, 0, texture.info().mip_levels, states, BarrierSync::COMPUTE_SHADER, BarrierAccess::SAMPLING_READ, TextureLayout::Sampled);
        cmd_buf.bind_sampling_view(BindingFrequency::Frequent, job.pipeline.srvResourceBindings[i].slotIndex, sampling_view);
      },
      Resource::Buffer { .. } => unreachable!(),
    }
  }

  for i in 0..job.pipeline.constCount as usize {
    let cb = &job.cbs[i];

    let buffer = cmd_buf.create_temporary_buffer(&BufferInfo {
      size: cb.uint32Size as usize * std::mem::size_of::<u32>(),
      usage: BufferUsage::CONSTANT,
    }, MemoryUsage::MappableVRAM);

    let ptr = buffer.map_unsafe(false).unwrap();
    std::ptr::copy(cb.data.as_ptr(), ptr as *mut u32, cb.uint32Size as usize);
    buffer.unmap_unsafe(true);
    cmd_buf.bind_uniform_buffer(BindingFrequency::Frequent, job.pipeline.cbResourceBindings[i].slotIndex, &buffer, 0, WHOLE_BUFFER);
  }

  cmd_buf.bind_sampler(BindingFrequency::VeryFrequent, 0, &context.point_sampler);
  cmd_buf.bind_sampler(BindingFrequency::VeryFrequent, 1, &context.linear_sampler);

  cmd_buf.flush_barriers();
  cmd_buf.finish_binding();
  cmd_buf.dispatch(job.dimensions[0], job.dimensions[1], job.dimensions[2]);
}

fn add_texture_barrier<B: Backend>(cmd_buffer: &mut B::CommandBuffer, texture: &Arc<B::Texture>, mip: u32, mip_count: u32, states: &mut [TextureSubresourceState], new_sync: BarrierSync, new_access: BarrierAccess, new_layout: TextureLayout) {
  for i in mip..(mip + mip_count) {
    let state = &mut states[i as usize];
    cmd_buffer.barrier(&[Barrier::TextureBarrier {
      old_sync: state.sync,
      new_sync: new_sync,
      old_layout: state.layout,
      new_layout: new_layout,
      old_access: state.access,
      new_access: new_access,
      texture: texture,
      range: BarrierTextureRange {
        base_mip_level: i,
        mip_level_length: 1,
        base_array_layer: 0,
        array_layer_length: 1,
      }
    }]);
    state.sync = new_sync;
    state.access = new_access;
    state.layout = new_layout;
  }
}

unsafe extern "C" fn create_pipeline<P: Platform, B: Backend>(
  backend_interface: *mut FfxFsr2Interface,
  pass: FfxFsr2Pass,
  _pipeline_description: *const FfxPipelineDescription,
  out_pipeline: *mut FfxPipelineState
) -> FfxErrorCode {
  let context = ScratchContext::<B>::from_interface(backend_interface);

  let mut path: PathBuf = PathBuf::from("shaders");
  let name: String;
  if pass == FfxFsr2Pass_FFX_FSR2_PASS_PREPARE_INPUT_COLOR {
    path = path.join("ffx_fsr2_prepare_input_color_pass.spv");
    name = "ffx_fsr2_prepare_input_color_pass".to_string();
  } else if pass == FfxFsr2Pass_FFX_FSR2_PASS_DEPTH_CLIP {
    path = path.join("ffx_fsr2_depth_clip_pass.spv");
    name = "ffx_fsr2_depth_clip_pass".to_string();
  } else if pass == FfxFsr2Pass_FFX_FSR2_PASS_RECONSTRUCT_PREVIOUS_DEPTH {
    path = path.join("ffx_fsr2_reconstruct_previous_depth_pass.spv");
    name = "ffx_fsr2_reconstruct_previous_depth_pass".to_string();
  } else if pass == FfxFsr2Pass_FFX_FSR2_PASS_LOCK {
    path = path.join("ffx_fsr2_lock_pass.spv");
    name = "ffx_fsr2_lock_pass".to_string();
  } else if pass == FfxFsr2Pass_FFX_FSR2_PASS_ACCUMULATE {
    path = path.join("ffx_fsr2_accumulate_pass.spv");
    name = "ffx_fsr2_accumulate_pass".to_string();
  } else if pass == FfxFsr2Pass_FFX_FSR2_PASS_ACCUMULATE_SHARPEN {
    path = path.join("ffx_fsr2_accumulate_sharpen_pass.spv");
    name = "ffx_fsr2_accumulate_sharpen_pass".to_string();
  } else if pass == FfxFsr2Pass_FFX_FSR2_PASS_RCAS {
    path = path.join("ffx_fsr2_rcas_pass.spv");
    name = "ffx_fsr2_rcas_pass".to_string();
  } else if pass == FfxFsr2Pass_FFX_FSR2_PASS_COMPUTE_LUMINANCE_PYRAMID {
    path = path.join("ffx_fsr2_compute_luminance_pyramid_pass.spv");
    name = "ffx_fsr2_compute_luminance_pyramid_pass".to_string();
  } else if pass == FfxFsr2Pass_FFX_FSR2_PASS_GENERATE_REACTIVE {
    path = path.join("ffx_fsr2_autogen_reactive_pass.spv");
    name = "ffx_fsr2_autogen_reactive_pass".to_string();
  } else {
    panic!("Unsupported pass: {}", pass);
  }

  let shader = {
    let mut file = <P::IO as IO>::open_asset(path).unwrap();
    let mut bytes: Vec<u8> = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    context.device.create_shader(ShaderType::ComputeShader, &bytes, Some(&name))
  };
  let pipeline = context.device.create_compute_pipeline(&shader, Some(&name));

  core::ptr::write_bytes(out_pipeline, 0, 1);
  let ffx_pipeline = &mut (*out_pipeline);
  ffx_pipeline.rootSignature = std::ptr::null_mut();


  for i in 0..16 {
    let info = pipeline.binding_info(BindingFrequency::Frequent, i);
    if info.is_none() {
      continue;
    }
    let info = info.unwrap();

    let binding = match info.binding_type {
      BindingType::StorageTexture => {
        ffx_pipeline.uavCount += 1;
        &mut ffx_pipeline.uavResourceBindings[ffx_pipeline.uavCount as usize - 1]
      }
      BindingType::SampledTexture => {
        ffx_pipeline.srvCount += 1;
        &mut ffx_pipeline.srvResourceBindings[ffx_pipeline.srvCount as usize - 1]
      }
      BindingType::ConstantBuffer => {
        ffx_pipeline.constCount += 1;
        &mut ffx_pipeline.cbResourceBindings[ffx_pipeline.constCount as usize - 1]
      }
      _ => unimplemented!()
    };
    let c_name = WideCString::from_str(info.name).unwrap();
    binding.slotIndex = i;
    binding.resourceIdentifier = 0; // initialized by FSR2 CPP code
    let c_name_slice = c_name.as_ucstr().as_slice_with_nul();
    binding.name[..c_name_slice.len()].copy_from_slice(c_name_slice);
  }

  let pipeline_ptr = Arc::into_raw(pipeline);
  ffx_pipeline.pipeline = pipeline_ptr as FfxPipeline;

  FFX_OK
}

unsafe extern "C" fn destroy_pipeline<B: Backend>(
  _backend_interface: *mut FfxFsr2Interface,
  pipeline: *mut FfxPipelineState
) -> FfxErrorCode {
  Arc::<B::ComputePipeline>::from_raw((*pipeline).pipeline as *mut B::ComputePipeline);
  FFX_OK
}


fn ffx_to_format(format: FfxSurfaceFormat) -> Format {
  if format == FfxSurfaceFormat_FFX_SURFACE_FORMAT_UNKNOWN {
    return Format::Unknown;
  }
  if format == FfxSurfaceFormat_FFX_SURFACE_FORMAT_R32G32B32A32_TYPELESS {
    return Format::RGBA32Float;
  }
  if format == FfxSurfaceFormat_FFX_SURFACE_FORMAT_R32G32B32A32_FLOAT {
    return Format::RGBA32Float;
  }
  if format == FfxSurfaceFormat_FFX_SURFACE_FORMAT_R16G16B16A16_FLOAT {
    return Format::RGBA16Float;
  }
  if format == FfxSurfaceFormat_FFX_SURFACE_FORMAT_R32G32_FLOAT {
    return Format::RG32Float;
  }
  if format == FfxSurfaceFormat_FFX_SURFACE_FORMAT_R32_UINT {
    return Format::R32UInt;
  }
  if format == FfxSurfaceFormat_FFX_SURFACE_FORMAT_R8G8B8A8_TYPELESS {
    return Format::RGBA8UNorm;
  }
  if format == FfxSurfaceFormat_FFX_SURFACE_FORMAT_R8G8B8A8_UNORM {
    return Format::RGBA8UNorm;
  }
  if format == FfxSurfaceFormat_FFX_SURFACE_FORMAT_R11G11B10_FLOAT {
    return Format::R11G11B10Float;
  }
  if format == FfxSurfaceFormat_FFX_SURFACE_FORMAT_R16G16_FLOAT {
    return Format::RG16Float;
  }
  if format == FfxSurfaceFormat_FFX_SURFACE_FORMAT_R16G16_UINT {
    return Format::RG16UInt;
  }
  if format == FfxSurfaceFormat_FFX_SURFACE_FORMAT_R16_FLOAT {
    return Format::R16Float;
  }
  if format == FfxSurfaceFormat_FFX_SURFACE_FORMAT_R16_UINT {
    return Format::R16UInt;
  }
  if format == FfxSurfaceFormat_FFX_SURFACE_FORMAT_R16_UNORM {
    return Format::R16UNorm;
  }
  if format == FfxSurfaceFormat_FFX_SURFACE_FORMAT_R16_SNORM {
    return Format::R16SNorm;
  }
  if format == FfxSurfaceFormat_FFX_SURFACE_FORMAT_R8_UNORM {
    return Format::R8Unorm;
  }
  if format == FfxSurfaceFormat_FFX_SURFACE_FORMAT_R32_FLOAT {
    return Format::R32Float;
  }
  if format == FfxSurfaceFormat_FFX_SURFACE_FORMAT_R8G8_UNORM {
    return Format::RG8UNorm;
  }
  unimplemented!()
}

fn format_to_ffx(format: Format) -> Option<FfxSurfaceFormat> {
  match format {
    Format::Unknown => Some(FfxSurfaceFormat_FFX_SURFACE_FORMAT_UNKNOWN),
    Format::R32UNorm => Some(FfxSurfaceFormat_FFX_SURFACE_FORMAT_R32_UINT),
    Format::R16UNorm => Some(FfxSurfaceFormat_FFX_SURFACE_FORMAT_R16_UNORM),
    Format::RGBA8UNorm => Some(FfxSurfaceFormat_FFX_SURFACE_FORMAT_R8G8B8A8_UNORM),
    Format::R16Float => Some(FfxSurfaceFormat_FFX_SURFACE_FORMAT_R16_FLOAT),
    Format::RG16Float => Some(FfxSurfaceFormat_FFX_SURFACE_FORMAT_R16G16_FLOAT),
    Format::R32Float => Some(FfxSurfaceFormat_FFX_SURFACE_FORMAT_R32_FLOAT),
    Format::RG32Float => Some(FfxSurfaceFormat_FFX_SURFACE_FORMAT_R32G32_FLOAT),
    Format::RGBA32Float => Some(FfxSurfaceFormat_FFX_SURFACE_FORMAT_R32G32B32A32_FLOAT),
    Format::RGBA16Float => Some(FfxSurfaceFormat_FFX_SURFACE_FORMAT_R16G16B16A16_FLOAT),
    Format::R8Unorm => Some(FfxSurfaceFormat_FFX_SURFACE_FORMAT_R8_UNORM),
    Format::R11G11B10Float => Some(FfxSurfaceFormat_FFX_SURFACE_FORMAT_R11G11B10_FLOAT),
    Format::RG16UInt => Some(FfxSurfaceFormat_FFX_SURFACE_FORMAT_R16G16_UINT),
    Format::R16UInt => Some(FfxSurfaceFormat_FFX_SURFACE_FORMAT_R16_UINT),
    Format::R16SNorm => Some(FfxSurfaceFormat_FFX_SURFACE_FORMAT_R16_SNORM),
    Format::RG8UNorm => Some(FfxSurfaceFormat_FFX_SURFACE_FORMAT_R8G8_UNORM),
    _ => None
  }
}
