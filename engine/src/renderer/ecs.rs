use crate::renderer::renderable::{StaticModelRenderable, Renderable, RenderableType};
use std::collections::HashSet;
use legion::{Entity, Resources, SystemBuilder, IntoQuery, World, maybe_changed, EntityStore};
use crossbeam_channel::Sender;
use crate::renderer::command::RendererCommand;
use crate::asset::AssetKey;
use nalgebra::Matrix4;
use legion::systems::{Builder, CommandBuffer};
use legion::component;
use legion::world::SubWorld;
use crate::transform::GlobalTransform;
use crate::camera::GlobalCamera;
use crate::ActiveCamera;
use crate::renderer::Renderer;
use std::sync::Arc;
use sourcerenderer_core::Platform;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StaticRenderableComponent {
  pub model: AssetKey,
  pub receive_shadows: bool,
  pub cast_shadows: bool,
  pub can_move: bool
}

#[derive(Clone, Default, Debug)]
pub struct ActiveStaticRenderables(HashSet<Entity>);
#[derive(Clone, Default, Debug)]
pub struct RegisteredStaticRenderables(HashSet<Entity>);

pub fn install<P: Platform>(systems: &mut Builder, renderer: &Arc<Renderer<P>>) {
  systems.add_system(renderer_system::<P>(renderer.clone(), ActiveStaticRenderables(HashSet::new()), RegisteredStaticRenderables(HashSet::new())));
}

#[system]
#[read_component(GlobalCamera)]
#[read_component(StaticRenderableComponent)]
#[read_component(GlobalTransform)]
fn renderer<P: Platform>(world: &mut SubWorld,
            #[state] renderer: &Arc<Renderer<P>>,
            #[state] active_static_renderables: &mut ActiveStaticRenderables,
            #[state] registered_static_renderables: &mut RegisteredStaticRenderables,
            #[resource] active_camera: &ActiveCamera) {

  let camera_entry = world.entry_ref(active_camera.0).ok();
  let camera_component = camera_entry.as_ref().and_then(|entry| entry.get_component::<GlobalCamera>().ok());
  if let Some(camera) = camera_component {
    renderer.update_camera(camera.0);
  }

  let mut static_components_query = <(Entity, &StaticRenderableComponent, &GlobalTransform)>::query();
  for (entity, component, transform) in static_components_query.iter(world) {
    if active_static_renderables.0.contains(entity) {
      continue;
    }

    if !registered_static_renderables.0.contains(entity) {
      renderer.register_static_renderable(Renderable {
        renderable_type: RenderableType::Static(StaticModelRenderable {
          model: component.model,
          receive_shadows: component.receive_shadows,
          cast_shadows: component.cast_shadows,
          can_move: component.can_move
        }),
        entity: *entity,
        transform: transform.0,
        old_transform: Matrix4::<f32>::identity(),
        older_transform: Matrix4::<f32>::identity(),
        interpolated_transform: Matrix4::<f32>::identity()
      });

      registered_static_renderables.0.insert(*entity);
    }

    active_static_renderables.0.insert(*entity);
  }

  let mut static_components_update_transforms_query = <(Entity, &GlobalTransform)>::query()
    .filter(component::<StaticRenderableComponent>() & maybe_changed::<GlobalTransform>());

  for (entity, transform) in static_components_update_transforms_query.iter(world) {
    renderer.update_transform(*entity, transform.0.clone());
  }

  registered_static_renderables.0.retain(|entity| {
    if !active_static_renderables.0.contains(entity) {
      renderer.unregister_static_renderable(*entity);
      false
    } else {
      true
    }
  });

  renderer.end_frame();
}
