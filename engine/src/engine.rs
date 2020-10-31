use sourcerenderer_core::platform::{Platform, PlatformEvent, GraphicsApi};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::fs::File;
use std::io::*;
use sourcerenderer_core::graphics::CommandBufferType;
use sourcerenderer_core::graphics::CommandBuffer;
use sourcerenderer_core::graphics::MemoryUsage;
use sourcerenderer_core::graphics::BufferUsage;
use sourcerenderer_core::{Vec2, ThreadPoolBuilder};
use sourcerenderer_core::Vec2I;
use sourcerenderer_core::Vec2UI;
use sourcerenderer_core::Vec3;
use sourcerenderer_core::graphics::*;
use std::rc::Rc;
use std::path::Path;
use sourcerenderer_core::platform::Window;
use async_std::task;
use async_std::prelude::*;
use async_std::future;
use std::thread::{Thread};
use std::future::Future;
use async_std::task::JoinHandle;
use std::cell::RefCell;
use sourcerenderer_core::graphics::{RenderGraph, RenderGraphInfo, BACK_BUFFER_ATTACHMENT_NAME};
use std::collections::HashMap;
use image::{GenericImage, GenericImageView};
use nalgebra::{Matrix4, Point3, Vector3, Rotation3};
use std::sync::atomic::Ordering;
use std::sync::atomic::AtomicUsize;
use crate::asset::AssetManager;
use crate::renderer::Renderer;
use crate::scene::Scene;
use async_std::sync::{channel, Sender, Receiver};
use sourcerenderer_core::graphics::Backend as GraphicsBackend;
use sourcerenderer_core::job::*;
use legion::{World, Resources, Schedule};
use crate::fps_camera::{FPSCamera, fps_camera_rotation};

const TICK_RATE: u32 = 5;

pub struct Engine<P: Platform> {
    platform: Box<P>
}

struct Vertex {
  pub position: Vec3,
  pub color: Vec3,
  pub uv: Vec2
}

impl<P: Platform> Engine<P> {
  pub fn new(platform: Box<P>) -> Box<Engine<P>> {
    return Box::new(Engine {
      platform
    });
  }

  pub fn run(&mut self) {
    let cores = num_cpus::get();
    ThreadPoolBuilder::new().num_threads(cores - 2).build_global().unwrap();

    let instance = self.platform.create_graphics(true).expect("Failed to initialize graphics");
    let surface = self.platform.window().create_surface(instance.clone());

    let mut adapters = instance.list_adapters();
    let device = Arc::new(adapters.remove(0).create_device(&surface));
    let mut swapchain = Arc::new(self.platform.window().create_swapchain(true, &device, &surface));
    let asset_manager = AssetManager::<P>::new(&device);
    let renderer = Renderer::<P>::run(self.platform.window(), &device, &swapchain, &asset_manager, TICK_RATE);
    Scene::run::<P>(&renderer, &asset_manager, self.platform.input(), TICK_RATE);

    let mut fps_camera = FPSCamera::new();
    'event_loop: loop {
      let event = self.platform.handle_events();
      if event == PlatformEvent::Quit {
        break 'event_loop;
      }
      renderer.set_window_state(self.platform.window().state());
      renderer.primary_camera().update_rotation(fps_camera_rotation::<P>(self.platform.input(), &mut fps_camera, 1.0f32));
      std::thread::sleep(Duration::new(0, 4_000_000)); // 4ms
    }
  }
}