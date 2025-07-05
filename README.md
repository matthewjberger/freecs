# freECS

[<img alt="github" src="https://img.shields.io/badge/github-matthewjberger/freecs-8da0cb?style=for-the-badge&labelColor=555555&logo=github" height="20">](https://github.com/matthewjberger/freecs)
[<img alt="crates.io" src="https://img.shields.io/crates/v/freecs.svg?style=for-the-badge&color=fc8d62&logo=rust" height="20">](https://crates.io/crates/freecs)
[<img alt="docs.rs" src="https://img.shields.io/badge/docs.rs-freecs-66c2a5?style=for-the-badge&labelColor=555555&logo=docs.rs" height="20">](https://docs.rs/freecs)

`freecs` is a zero-abstraction table-based ECS library for Rust, in about ~500 lines

A macro is used to define the world and its components, and generates
the entity component system as part of your source code at compile time. The generated code
contains only plain data structures (no methods) and free functions that transform them, achieving static dispatch.

The internal implementation is ~500 loc (aside from tests, comments, and example code),
and does not use object orientation, generics, traits, or dynamic dispatch.

## Quick Start

Add this to your `Cargo.toml`:

```toml
[dependencies]
freecs = "0.4.6"

# (optional) add rayon if you want to parallelize systems
rayon = "^1.10.0"
```

And in `main.rs`:

```rust
use freecs::{ecs, table_has_components, EntityId};
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
    let entity = world.spawn_entities(POSITION | VELOCITY, 1)[0];
    println!(
        "Spawned {} with position and velocity",
        world.get_all_entities().len()
    );

    // Read arbitrary components
    let position = world.get_component::<Position>(entity, POSITION);
    println!("Position: {:?}", position);

    // Same as the above but more concise, these are generated for each component
    let position = world.get_position(entity);
    println!("Position: {:?}", position);

    // Mutate a component
    if let Some(position) = world.get_position_mut(entity) {
        position.x += 1.0;
    }

    // Get an entity's component mask
    println!(
        "Component mask before adding health component: {:b}",
        world.component_mask(entity).unwrap()
    );

    // Add a new component to an entity
    world.add_components(entity, HEALTH);

    println!(
        "Component mask after adding health component: {:b}",
        world.component_mask(entity).unwrap()
    );

    // Query all entities
    let entities = world.get_all_entities();
    println!("All entities: {entities:?}");

    // Query all entities with a specific component
    let players = world.query_entities(POSITION | VELOCITY | HEALTH);
    println!("Player entities: {players:?}");

    // Query the first entity with a specific component,
    // returning early instead of checking remaining entities
    let first_player_entity = world.query_first_entity(POSITION | VELOCITY | HEALTH);
    println!("First player entity : {first_player_entity:?}");

    // Remove a component from an entity
    world.remove_components(entity, HEALTH);

    // Systems are functions that iterate over
    // the component tables and transform component data.
    // This function invokes two systems in parallel
    // for each table in the world filtered by component mask.
    systems::run_systems(&mut world);

    // Despawn entities, freeing their table slots for reuse
    world.despawn_entities(&[entity]);
}

use components::*;
mod components {
    #[derive(Default, Debug, Clone)]
    pub struct Position {
        pub x: f32,
        pub y: f32,
    }

    #[derive(Default, Debug, Clone)]
    pub struct Velocity {
        pub x: f32,
        pub y: f32,
    }

    #[derive(Default, Debug, Clone)]
    pub struct Health {
        pub value: f32,
    }
}

mod systems {
    use super::*;

    pub fn run_systems(world: &mut World) {
        let delta_time = world.resources.delta_time;
        world.tables.par_iter_mut().for_each(|table| {
            if table_has_components!(table, POSITION | VELOCITY | HEALTH) {
                update_positions_system(&mut table.position, &table.velocity, delta_time);
            }
            if table_has_components!(table, HEALTH) {
                health_system(&mut table.health);
            }
        });
    }

    // The system itself can also access components in parallel
    #[inline]
    pub fn update_positions_system(positions: &mut [Position], velocities: &[Velocity], dt: f32) {
        positions
            .par_iter_mut()
            .zip(velocities.par_iter())
            .for_each(|(pos, vel)| {
                pos.x += vel.x * dt;
                pos.y += vel.y * dt;
            });
    }

    #[inline]
    pub fn health_system(health: &mut [Health]) {
        health.par_iter_mut().for_each(|health| {
            health.value *= 0.98;
        });
    }
}
```

## Change Detection

`freecs` provides an opt-in change detection system that allows you to track when components are modified.
This is useful for systems that only need to process entities when their data has changed.

## Basic Usage

```rust
// Get mutable access and modify a component
if let Some(pos) = world.get_component_mut::<Position>(entity, POSITION) {
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


## Examples

Run the examples with:

```rust
cargo run -r --example cubes
```

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE.md) file for details.
