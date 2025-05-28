# freECS

[<img alt="github" src="https://img.shields.io/badge/github-matthewjberger/freecs-8da0cb?style=for-the-badge&labelColor=555555&logo=github" height="20">](https://github.com/matthewjberger/freecs)
[<img alt="crates.io" src="https://img.shields.io/crates/v/freecs.svg?style=for-the-badge&color=fc8d62&logo=rust" height="20">](https://crates.io/crates/freecs)
[<img alt="docs.rs" src="https://img.shields.io/badge/docs.rs-freecs-66c2a5?style=for-the-badge&labelColor=555555&logo=docs.rs" height="20">](https://docs.rs/freecs)

`freecs` is a zero-abstraction ECS library for Rust, designed for high performance and simplicity. 🚀

It provides an archetypal table-based storage system for components, allowing for fast queries,
fast system iteration, and parallel processing.

A macro is used to define the world and its components, and generates
the entity component system as part of your source code at compile time. The generated code
contains only plain data structures (no methods) and free functions that transform them, achieving static dispatch.

The internal implementation is ~500 loc (aside from tests, comments, and example code),
and does not use object orientation, generics, traits, or dynamic dispatch.

## Quick Start

Add this to your `Cargo.toml`:

```toml
[dependencies]
freecs = "0.3.3"

# (optional) add rayon if you want to parallelize systems
rayon = "^1.10.0"
```

And in `main.rs`:

```rust
use freecs::{table_has_components, ecs};
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

    // Add resources for systems to use
    world.resources.delta_time = 0.016;

    // Spawn entities with components
    let entity = spawn_entities(&mut world, POSITION | VELOCITY, 1)[0];
    println!(
        "Spawned {} with position and velocity",
        query_entities(&world, ALL).len(),
    );

    // Read a component
    let position = get_component::<Position>(&world, entity, POSITION);
    println!("Position: {:?}", position);

    // Mutate a component
    if let Some(position) = get_component_mut::<Position>(&mut world, entity, POSITION) {
        position.x += 1.0;
    }

    // Get an entity's component mask
    println!(
        "Component mask before adding health component: {:b}",
        component_mask(&world, entity).unwrap()
    );

    // Add a new component to an entity
    add_components(&mut world, entity, HEALTH);

    println!(
        "Component mask after adding health component: {:b}",
        component_mask(&world, entity).unwrap()
    );

    // Query all entities with a specific component
    let players = query_entities(&world, POSITION | VELOCITY | HEALTH);
    println!("Player entities: {players:?}");

    // Query the first entity with a specific component,
    // returning early instead of checking remaining entities
    let first_player_entity = query_first_entity(&world, POSITION | VELOCITY | HEALTH);
    println!("First player entity : {first_player_entity:?}");

    // Remove a component from an entity
    remove_components(&mut world, entity, HEALTH);

    // This runs the systems once in parallel
    // Not part of the library's public API, but a demonstration of how to run systems
    systems::run_systems(&mut world, 0.01);

    // Despawn entities, freeing their table slots for reuse
    despawn_entities(&mut world, &[entity]);
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

    // Systems are functions that iterate over the component tables
    // and transform component data.
    // This function invokes two systems in parallel
    // for each table in the world filtered by component mask.
    pub fn run_systems(world: &mut World) {
        let delta_time = world.resources.delta_time;

        // Parallelization of systems can be done with Rayon, which is useful when working with more than 3 million entities.
        //
        // In practice, you should use `.iter_mut()` instead of `.par_iter_mut()` unless you have a large number of entities,
        // because sequential access is more performant until you are working with extreme numbers of entities.
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
            health.value *= 0.98; // gradually decline health value
        });
    }
}
```

## Examples

Run the examples with:

```rust
cargo run -r --example cubes
```

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE.md) file for details.
