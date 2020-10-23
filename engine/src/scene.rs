use crate::{Vertex, camera};
use std::sync::{Arc, Mutex};
use sourcerenderer_core::{Platform, Vec3, Vec2};
use crossbeam_channel::bounded;
use async_std::task;
use std::path::Path;
use sourcerenderer_core::graphics::{TextureInfo, Format, SampleCount};
use image::GenericImageView;
use nalgebra::{Point3, Matrix4, Rotation3, Vector3};
use crate::renderer::*;
use std::thread;
use std::time::{Duration, SystemTime};
use crate::asset::AssetManager;
use legion::{World, Resources, Schedule};
use legion::systems::Builder as SystemBuilder;
use crate::transform;
use crate::fps_camera;

pub struct Scene {

}

pub struct DeltaTime(Duration);

impl DeltaTime {
  pub fn secs(&self) -> f32 {
    self.0.as_secs_f32()
  }
}

pub struct Tick(u64);

impl Scene {
  pub fn run<P: Platform>(renderer: &Arc<Renderer<P>>,
                          asset_manager: &Arc<AssetManager<P>>,
                          input: &Arc<P::Input>,
                          tick_rate: u32) {
    let c_renderer = renderer.clone();
    let c_asset_manager = asset_manager.clone();
    let c_input = input.clone();
    thread::spawn(move || {
      let mut world = World::default();
      let mut systems = Schedule::builder();
      let mut resources = Resources::default();

      resources.insert(c_input);

      crate::spinning_cube::install(&mut world, &mut resources, &mut systems, &c_asset_manager);
      fps_camera::install::<P>(&mut world, &mut systems);

      transform::install(&mut systems);
      camera::install(&mut systems);
      c_renderer.install(&mut world, &mut resources, &mut systems);

      let mut tick = 0u64;
      let mut schedule = systems.build();
      let mut last_iter_time = SystemTime::now();
      loop {
        let now = SystemTime::now();
        let delta = now.duration_since(last_iter_time).unwrap();

        if delta.as_millis() < ((1000 + tick_rate - 1) / tick_rate) as u128 {
          continue;
        }
        last_iter_time = now;
        resources.insert(DeltaTime(delta));
        resources.insert(Tick(tick));
        tick += 1;

        let mut spin_counter = 0u32;
        while c_renderer.is_saturated() {
          if spin_counter > 1024 {
            thread::sleep(Duration::new(0, 1_000_000)); // 1ms
          } else if spin_counter > 128 {
            thread::yield_now();
          }
          spin_counter += 1;
        }
        schedule.execute(&mut world, &mut resources);
      }
    });
  }
}
