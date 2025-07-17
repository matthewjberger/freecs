# freECS

[<img alt="github" src="https://img.shields.io/badge/github-matthewjberger/freecs-8da0cb?style=for-the-badge&labelColor=555555&logo=github" height="20">](https://github.com/matthewjberger/freecs)
[<img alt="crates.io" src="https://img.shields.io/crates/v/freecs.svg?style=for-the-badge&color=fc8d62&logo=rust" height="20">](https://crates.io/crates/freecs)
[<img alt="docs.rs" src="https://img.shields.io/badge/docs.rs-freecs-66c2a5?style=for-the-badge&labelColor=555555&logo=docs.rs" height="20">](https://docs.rs/freecs)

`freecs` is a zero-unsafe, table-based ECS library for Rust, in about ~600 lines

A macro is used to define the world and its components, and generates
the entity component system as part of your source code at compile time.

The internal implementation is minimal and does not use object orientation, generics, traits, or dynamic dispatch.

## Quick Start

Add this to your `Cargo.toml`:

```toml
[dependencies]
freecs = "0.6.0"

# (optional) add rayon if you want to parallelize systems
rayon = "^1.10.0"
```

And in `main.rs`:

```rust
use freecs::{ecs, Entity};
use rayon::prelude::*;

ecs! {
    World {
        position: Position => POSITION,
        velocity: Velocity => VELOCITY,
        health: Health => HEALTH,
    }
    Resources {
        delta_time: f32
    }
}

pub fn main() {
    let mut world = World::default();

    // Spawn entities with components
    let _entity = world.spawn_entities(POSITION | VELOCITY, 1)[0];

    // Or use the entity builder
    let entity = EntityBuilder::new()
        .with_position(Position { x: 1.0, y: 2.0 })
        .spawn(&mut world, 1)[0];

    // Read components using the generated methods
    let position = world.get_position(entity);
    println!("Position: {:?}", position);

    // Set components (adds if not present)
    world.set_position(entity, Position { x: 1.0, y: 2.0 });

    // Mutate a component
    if let Some(position) = world.get_position_mut(entity) {
        position.x += 1.0;
    }

    // Get an entity's component mask
    let _component_mask = world.component_mask(entity).unwrap();

    // Add a new component to an entity
    world.add_components(entity, HEALTH);

    // Or use the generated add method
    world.add_health(entity);

    // Query all entities
    let _entities = world.get_all_entities();

    // Query all entities with a specific component
    let _players = world.query_entities(POSITION | VELOCITY | HEALTH);

    // Query the first entity with a specific component,
    // returning early instead of checking remaining entities
    let _first_player_entity = world.query_first_entity(POSITION | VELOCITY | HEALTH);

    // Remove a component from an entity
    world.remove_components(entity, HEALTH);

    // Or use the generated remove method
    world.remove_health(entity);

    // Check if entity has components
    if world.entity_has_position(entity) {
        println!("Entity has position component");
    }

    // Systems are functions that transform component data
    systems::run_systems(&mut world);

    // Despawn entities, freeing their table slots for reuse
    world.despawn_entities(&[entity]);
}

use components::*;
mod components {
    #[derive(Default, Debug, Clone, Copy)]
    pub struct Position {
        pub x: f32,
        pub y: f32,
    }

    #[derive(Default, Debug, Clone, Copy)]
    pub struct Velocity {
        pub x: f32,
        pub y: f32,
    }

    #[derive(Default, Debug, Clone, Copy)]
    pub struct Health {
        pub value: f32,
    }
}

mod systems {
    use super::*;

    pub fn run_systems(world: &mut World) {
        // Systems use queries and component accessors
        example_system(world);
        update_positions_system(world);
        health_system(world);
    }

    fn example_system(world: &mut World) {
        for entity in world.query_entities(POSITION | VELOCITY) {
            if let Some(position) = world.get_position_mut(entity) {
                position.x += 1.0;
            }
        }
    }

    fn update_positions_system(world: &mut World) {
        let dt = world.resources.delta_time;
        
        // Collect entities with their velocities first to avoid borrow conflicts
        let updates: Vec<(Entity, Velocity)> = world
            .query_entities(POSITION | VELOCITY)
            .into_iter()
            .filter_map(|entity| {
                world.get_velocity(entity).map(|vel| (entity, *vel))
            })
            .collect();
        
        // Now update positions
        for (entity, vel) in updates {
            if let Some(pos) = world.get_position_mut(entity) {
                pos.x += vel.x * dt;
                pos.y += vel.y * dt;
            }
        }
    }

    fn health_system(world: &mut World) {
        for entity in world.query_entities(HEALTH) {
            if let Some(health) = world.get_health_mut(entity) {
                health.value *= 0.98;
            }
        }
    }

    // Alternative: Parallel processing for large numbers of entities
    #[allow(dead_code)]
    fn parallel_update_positions_system(world: &mut World) {
        let dt = world.resources.delta_time;
        
        // Collect all entity data first
        let mut entity_data: Vec<(Entity, Position, Velocity)> = world
            .query_entities(POSITION | VELOCITY)
            .into_iter()
            .filter_map(|entity| {
                match (world.get_position(entity), world.get_velocity(entity)) {
                    (Some(pos), Some(vel)) => Some((entity, *pos, *vel)),
                    _ => None
                }
            })
            .collect();
        
        // Process in parallel
        entity_data
            .par_iter_mut()
            .for_each(|(_, pos, vel)| {
                pos.x += vel.x * dt;
                pos.y += vel.y * dt;
            });
        
        // Write back the results
        for (entity, new_pos, _) in entity_data {
            world.set_position(entity, new_pos);
        }
    }
}
```

## Generated API

The `ecs!` macro generates type-safe methods for each component:

```rust
// For each component, you get:
world.get_position(entity)        // -> Option<&Position>
world.get_position_mut(entity)    // -> Option<&mut Position>
world.set_position(entity, pos)   // Sets or adds the component
world.add_position(entity)        // Adds with default value
world.remove_position(entity)     // Removes the component
world.entity_has_position(entity) // Checks if entity has component
```

## Systems

Systems are functions that query entities and transform their components:

```rust
pub fn update_global_transforms_system(world: &mut World) {
    world
        .query_entities(LOCAL_TRANSFORM | GLOBAL_TRANSFORM)
        .into_iter()
        .for_each(|entity| {
            // The entities we queried for are guaranteed to have
            // a local transform and global transform here
            let new_global_transform = query_global_transform(world, entity);
            let global_transform = world.get_global_transform_mut(entity).unwrap();
            *global_transform = GlobalTransform(new_global_transform);
        });
}

pub fn query_global_transform(world: &World, entity: EntityId) -> nalgebra_glm::Mat4 {
    let Some(local_transform) = world.get_local_transform(entity) else {
        return nalgebra_glm::Mat4::identity();
    };
    if let Some(Parent(parent)) = world.get_parent(entity) {
        query_global_transform(world, *parent) * local_transform
    } else {
        local_transform
    }
}
```

## Parallel Processing

For performance-critical systems with large numbers of entities, you can use parallel processing:

```rust
use rayon::prelude::*;

fn parallel_physics_system(world: &mut World) {
    let dt = world.resources.delta_time;
    
    // Collect entity data
    let mut entities: Vec<(Entity, Position, Velocity)> = world
        .query_entities(POSITION | VELOCITY)
        .into_iter()
        .filter_map(|entity| {
            match (world.get_position(entity), world.get_velocity(entity)) {
                (Some(pos), Some(vel)) => Some((entity, *pos, *vel)),
                _ => None
            }
        })
        .collect();
    
    // Process in parallel
    entities.par_iter_mut().for_each(|(_, pos, vel)| {
        pos.x += vel.x * dt;
        pos.y += vel.y * dt;
    });
    
    // Write back results
    for (entity, new_pos, _) in entities {
        world.set_position(entity, new_pos);
    }
}
```

## Change Detection

`freecs` provides an opt-in change detection system that allows you to track when components are modified.
This is useful for systems that only need to process entities when their data has changed.

```rust
// Get mutable access and modify a component
if let Some(pos) = world.get_position_mut(entity) {
    pos.x += velocity.x * dt;
    pos.y += velocity.y * dt;
}

// Explicitly mark the component as changed
world.mark_changed(entity, POSITION);

// Later, process change events
while let Some(event) = world.try_next_event() {
    match event {
        Event::ComponentChanged { kind, entity } => {
            println!("Component {:b} changed for entity {:?}", kind, entity);
        }
    }
}

// You can also clear the event queue
world.clear_events();
```

You can mark multiple components as changed in a single call:

```rust
// Mark both position and velocity as changed
world.mark_changed(entity, POSITION | VELOCITY);
```

The event queue is stored in the world's `Resources` struct and is automatically available when you create a world with the `ecs!` macro.


## Entity Builder

An entity builder is generated automatically:

```rust
let mut world = World::default();

let entities = EntityBuilder::new()
    .with_position(Position { x: 1.0, y: 2.0 })
    .with_velocity(Velocity { x: 0.0, y: 1.0 })
    .spawn(&mut world, 2);

assert_eq!(world.get_position(entities[0]).unwrap().x, 1.0);
assert_eq!(world.get_position(entities[1]).unwrap().y, 2.0);
```


## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE.md) file for details.
