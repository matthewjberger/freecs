# freecs

A high-performance archetype-based Entity Component System (ECS) in Rust using only static dispatch 🚀

## Quick Start

Add this to your `Cargo.toml`:

```toml
[dependencies]
freecs = "0.1.3"
rayon = "1.10.0"
serde = { version = "1.0.214", features = ["derive"] }
```

And in `main.rs`:

```rust
use freecs::{has_components, world};
use rayon::prelude::*;

world! {
  World {
      positions: Position => POSITION,
      velocities: Velocity => VELOCITY,
      health: Health => HEALTH,
  }
}

pub fn main() {
    let mut world = World::default();

    // Spawn entities with components
    let entity = spawn_entities(&mut world, POSITION | VELOCITY, 1)[0];
    println!(
        "Spawned {} with position and velocity",
        total_entities(&world)
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

    // Call this manually to compact tables, ideally periodically (such as every 60 frames).
    // This is a performance benefit and is optional.
    merge_tables(&mut world);

    // Despawn entities, freeing their table slots for reuse
    despawn_entities(&mut world, &[entity]);
}

use components::*;
mod components {
    #[derive(Default, Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct Position {
        pub x: f32,
        pub y: f32,
    }

    #[derive(Default, Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct Velocity {
        pub x: f32,
        pub y: f32,
    }

    #[derive(Default, Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct Health {
        pub value: f32,
    }
}

mod systems {
    use super::*;

    // Systems are functions that iterate over
    // the component tables and transform component data.
    // This function invokes two systems in parallel
    // for each table in the world filtered by component mask.
    pub fn run_systems(world: &mut World, dt: f32) {
        world.tables.par_iter_mut().for_each(|table| {
            if has_components!(table, POSITION | VELOCITY | HEALTH) {
                update_positions_system(&mut table.positions, &table.velocities, dt);
            }
            if has_components!(table, HEALTH) {
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

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE.md) file for details.
