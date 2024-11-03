# freecs

A high-performance, archetype-based Entity Component System (ECS) written in Rust. Designed for parallel processing and cache-friendly data layouts, freecs provides a simple macro-based API for defining and managing game worlds with components.

## Features

- 🚀 **Archetype-based Storage**: Optimal cache coherency through grouped component storage
- 🔄 **Built-in Parallel Processing**: Leverages rayon for efficient multi-threading
- 📦 **Zero-overhead Component Access**: Direct slice indexing for component data
- 🛡️ **Type-safe Component Management**: Compile-time component type checking
- 🔧 **Dynamic Components**: Add or remove components at runtime
- 🧹 **Automatic Table Defragmentation**: Maintains optimal memory layout
- 📝 **Batch Operations**: Efficient bulk entity operations
- 💾 **Serialization Support**: Built-in serde support for all core types

## Quick Start

```rust
use freecs::{impl_world, has_components};

// Define your components
#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
struct Position { x: f32, y: f32 }

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
struct Velocity { x: f32, y: f32 }

// Create a world with your components
impl_world! {
    World {
        positions: Position => POSITION,
        velocities: Velocity => VELOCITY,
    }
}

// Create a new world
let mut world = World::default();

// Spawn entities with components
let entity = spawn_entities(&mut world, POSITION | VELOCITY, 1)[0];
```

## Parallel Systems Example

```rust
use rayon::prelude::*;

fn run_systems(world: &mut World, dt: f32) {
    world.tables.par_iter_mut().for_each(|table| {
        if has_components!(table, POSITION | VELOCITY) {
            update_positions(&mut table.positions, &table.velocities, dt);
        }
    });
}

#[inline]
fn update_positions(positions: &mut [Position], velocities: &[Velocity], dt: f32) {
    positions
        .par_iter_mut()
        .zip(velocities.par_iter())
        .for_each(|(pos, vel)| {
            pos.x += vel.x * dt;
            pos.y += vel.y * dt;
        });
}
```

## Running the Demo

The project includes a demo using macroquad for rendering:

```bash
cargo run -r --example demo
```

Controls:
- Use arrow keys to rotate the camera
- Press Escape to exit

## Performance Tips

- Use batch operations when possible (`spawn_entities`, `despawn_entities`)
- Leverage parallel systems for large worlds using rayon
- Call `merge_tables` periodically to alleviate fragmentation

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
freecs = "0.1.0"
```

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
