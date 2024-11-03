# freecs

A high-performance, archetype-based Entity Component System (ECS) written in Rust using only static dispatch, written for cache-coherency with data oriented design.

## Quick Start

Add this to your `Cargo.toml`:

```toml
[dependencies]
freecs = "0.1.1"
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

    // Read a component
    let position = get_component::<Position>(&world, entity, POSITION);
    println!("Position: {:?}", position); // Prints "Some(Position { x: 0.0, y: 0.0 })"

    // Mutate a component
    if let Some(position) = get_component_mut::<Position>(&mut world, entity, POSITION) {
        position.x += 1.0;
    }

    // Assign a component
    set_component(&mut world, entity, VELOCITY, Velocity { x: 1.0, y: 2.0 });

    systems::run_systems(&mut world, 0.01);
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
    //
    // Here, this function invokes two systems in parallel.
    pub fn run_systems(world: &mut World, dt: f32) {
        world.tables.par_iter_mut().for_each(|table| {
            if has_components!(table, POSITION | VELOCITY) {
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
