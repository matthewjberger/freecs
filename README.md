# freecs

A high-performance, archetype-based Entity Component System (ECS) written in Rust using only static dispatch, written for cache-coherency with data oriented design.

## Quick Start

Add this to your `Cargo.toml`:

```toml
[dependencies]
freecs = "0.1.1"
```

And in `main.rs`:

```rust
use freecs::{world, has_components};

// Define your components
#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
struct Position { x: f32, y: f32 }

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
struct Velocity { x: f32, y: f32 }

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
struct Health { value: f32 }

// Create a world with your components
world! {
  World {
      positions: Position => POSITION,
      velocities: Velocity => VELOCITY,
      health: Health => HEALTH,
  }
}

pub fn main() {
    // Create a new world
    let mut world = World::default();

    // Spawn entities with components
    let entity = spawn_entities(&mut world, POSITION | VELOCITY, 1)[0];

    // Read a component
    let position = get_component::<Position>(&world, *entity, POSITION);
    println!("Position: {:?}", position); // Prints "Some(Position { x: 0.0, y: 0.0 })"

    // Write a component
    if let Some(_position) = get_component_mut::<Position>(&mut world, *entity, POSITION) {
        // _position could be mutated here
    }

    // Assign a component
    set_component(&mut world, *entity, VELOCITY, Velocity { x: 1.0 y: 2.0 } );
}


// Systems are plain functions that iterate (potentially in parallel)
// over the entity tables, operating on the tables that match the component mask
#[inline]
fn update_positions_system(positions: &mut [Position], velocities: &[Velocity], dt: f32) {
  positions
      .par_iter_mut()
      .zip(velocities.par_iter())
      .for_each(|(pos, vel)| {
          pos.x += vel.x * dt;
          pos.y += vel.y * dt;
      });
}

// You can create multiple systems and run them in parallel
fn run_systems(world: &mut World, dt: f32) {
    use rayon::prelude::*;
    world.tables.par_iter_mut().for_each(|table| {
        if has_components!(table, POSITION | VELOCITY) {
            update_positions_system(&mut table.positions, &table.velocities, dt);
        }
        if has_components!(table, HEALTH) {
            // Here you could run other systems in parallel
            // if the systems don't operate on the same component streams
            // health_system(&mut table.health);
        }
    });
}

```

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE.md) file for details.
