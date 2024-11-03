# freecs

A high-performance, archetype-based Entity Component System (ECS) written in Rust.

```rust

## Features

- 🚀 **Archetype-based Storage**: Optimal cache coherency through grouped component storage
- 🔄 **Built-in Parallel Processing**: Leverages rayon for efficient multi-threading
- 📦 **Zero-overhead Component Access**: Direct slice indexing for component data
- 🛡️ **Type-safe Component Management**: Compile-time component type checking
- 🔧 **Dynamic Components**: Add or remove components at runtime
- 🧹 **Table Defragmentation**: Maintains optimal memory layout
- 📝 **Batch Operations**: Efficient bulk entity operations
- 💾 **Serialization Support**: Built-in serde support for all core types

## Quick Start

Add this to your `Cargo.toml`:

```toml
[dependencies]
freecs = "0.1.0"
```

And in `main.rs`:

```rust
use freecs::{world, has_components};

// Define your components
#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
struct Position { x: f32, y: f32 }

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
struct Velocity { x: f32, y: f32 }

// Create a world with your components
world! {
  World {
      positions: Position => POSITION,
      velocities: Velocity => VELOCITY,
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

// You can write plain functions
fn run_systems(world: &mut World, dt: f32) {
    use rayon::prelude::*;
    world.tables.par_iter_mut().for_each(|table| {
        if has_components!(table, POSITION | VELOCITY) {
            update_positions_system(&mut table.positions, &table.velocities, dt);
        }
    });
}

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
```

## Running the Demo

The project includes a demo:

```bash
cargo run -r --example demo
```

Controls:
- Use arrow keys to rotate the camera
- Press Escape to exit


### Data Oriented vs Object Oriented

When processing entities, the cache-friendly layout makes a significant difference:

```rust
// Given 10,000 entities with Position and Velocity:

// Traditional OOP: Cache misses, unpredictable memory access
for entity in entities {
  entity.position.x += entity.velocity.x;  // 💥 Cache miss probable
  entity.position.y += entity.velocity.y;  // 💥 Cache miss probable
}

// freecs: Streaming through contiguous memory
for table in &mut world.tables {
  if has_components!(table, POSITION | VELOCITY) {
      // ✨ CPU prefetcher can predict this pattern
      // ✨ Full cache lines are utilized
      // ✨ Minimal memory stalls
      table.positions.iter_mut()
          .zip(table.velocities.iter())
          .for_each(|(pos, vel)| {
              pos.x += vel.x;
              pos.y += vel.y;
          });
  }
}
```

### Benefits of Macro-based Generation

- **Inlining**: The entire ECS can be inlined into your crate by inlining the `impl_world!` macro
- **Zero Overhead**: No virtual dispatch or runtime type checking. Only static dispatch is used.
- **Compile-time Optimization**: The compiler sees your exact component types and can optimize accordingly
- **No Runtime Dependencies**: The core ECS can work without external crates
- **Specialized Code**: Generated code is specific to your component types

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE.md) file for details.
