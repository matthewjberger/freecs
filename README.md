# freecs

[<img alt="github" src="https://img.shields.io/badge/github-matthewjberger/freecs-8da0cb?style=for-the-badge&labelColor=555555&logo=github" height="20">](https://github.com/matthewjberger/freecs)
[<img alt="crates.io" src="https://img.shields.io/crates/v/freecs.svg?style=for-the-badge&color=fc8d62&logo=rust" height="20">](https://crates.io/crates/freecs)
[<img alt="docs.rs" src="https://img.shields.io/badge/docs.rs-freecs-66c2a5?style=for-the-badge&labelColor=555555&logo=docs.rs" height="20">](https://docs.rs/freecs)

freecs is a zero-abstraction ECS library for Rust, designed for high performance and simplicity. 🚀

It provides an archetypal table-based storage system for components, allowing for fast queries,
fast system iteration, and parallel processing.

A macro is used to define the world and its components, and generates
the entity component system as part of your source code at compile time. The generated code
contains only plain data structures (no methods) and free functions that transform them, achieving static dispatch.

The internal implementation is ~500 loc (aside from tests, comments, and example code),
and does not use object orientation, generics, traits, or dynamic dispatch.

### Key Features

- **Table-based Storage**: Entities with the same components are stored together in memory
- **Raw Access**: Functions work directly on the underlying vectors of components
- **Parallel Processing**: Built-in support for processing tables in parallel with rayon
- **Simple Queries**: Find entities by their components using bit masks
- **Serialization**: Save and load worlds using serde
- **World Merging**: Clone and remap entity hierarchies between worlds
- **Zero Overhead**: No dynamic dispatch, traits, or runtime abstractions
- **Data Oriented**: Focus on cache coherence and performance

## Quick Start

Add this to your `Cargo.toml`:

```toml
[dependencies]
freecs = "0.2.15"
serde = { version = "1.0", features = ["derive"] }

# (optional) add rayon if you want to parallelize systems
rayon = "1.10.0" # or higher
```

And in `main.rs`:

```rust
use freecs::{has_components, world};
use rayon::prelude::*;

// The `World` and `Resources` type names can be customized.
world! {
  World {
      components {
        position: Position => POSITION,
        velocity: Velocity => VELOCITY,
        health: Health => HEALTH,
      },
      Resources {
          delta_time: f32
      }
  }
}

pub fn main() {
    let mut world = World::default();

    // Inject resources for systems to use
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
            if has_components!(table, POSITION | VELOCITY | HEALTH) {
                update_positions_system(&mut table.position, &table.velocity, delta_time);
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

## Examples

Run the examples with:

```rust
cargo run -r --example cubes
```

# World Merging

The ECS supports cloning entities from one world to another while maintaining their relationships.
This is useful for implementing prefabs, prototypes, and scene loading.

```rust
let mut source = World::default();
let mut game_world = World::default();

// Spawn a hierarchy of entities
let [root, child1, child2] = spawn_entities(&mut source, POSITION | NODE, 3)[..] else {
panic!("Failed to spawn entities");
};

// Set up entity references
if let Some(node) = get_component_mut::<Node>(&mut source, root, NODE) {
node.id = root;
node.children = vec![child1, child2];
}

// Copy entities to game world and get mapping of old->new IDs
let mapping = merge_worlds(&mut game_world, &source);
```

## Remapping Entity References

When components contain EntityIds (for parent-child relationships, inventories, etc),
these need to be updated to point to the newly spawned entities:

```rust
#[derive(Default, Clone)]
struct Node {
    id: EntityId,
    parent: Option<EntityId>,
    children: Vec<EntityId>,
}

// Update references
remap_entity_refs(&mut game_world, &mapping, |mapping, table| {
    if table.mask & NODE != 0 {
        for node in &mut table.node {
            // Remap simple field
            if let Some(new_id) = remap_entity(mapping, node.id) {
                node.id = new_id;
            }

            // Remap Option<EntityId>
            if let Some(ref mut parent_id) = node.parent {
                if let Some(new_id) = remap_entity(mapping, *parent_id) {
                    *parent_id = new_id;
                }
            }

            // Remap Vec<EntityId>
            for child_id in &mut node.children {
                if let Some(new_id) = remap_entity(mapping, *child_id) {
                    *child_id = new_id;
                }
            }
        }
    }
});
```

## Example: Character Prefab

```rust
fn spawn_character(world: &mut World, position: Vec2) -> EntityId {
    // Create prefab hierarchy
    let mut prefab = World::default();
    let [root, weapon, effects] = spawn_entities(&mut prefab, MODEL | NODE, 3)[..] else {
        panic!("Failed to spawn prefab");
    };

    // Set up components and relationships
    if let Some(root_node) = get_component_mut::<Node>(&mut prefab, root, NODE) {
        root_node.id = root;
        root_node.children = vec![weapon, effects];
    }

    // Copy to game world and update references
    let mapping = merge_worlds(world, &prefab);

    remap_entity_refs(world, &mapping, |mapping, table| {
        if table.mask & NODE != 0 {
            for node in &mut table.node {
                if let Some(new_id) = remap_entity(mapping, node.id) {
                    node.id = new_id;
                }
                for child_id in &mut node.children {
                    if let Some(new_id) = remap_entity(mapping, *child_id) {
                        *child_id = new_id;
                    }
                }
            }
        }
    });

    // Return the new root entity
    remap_entity(&mapping, root).unwrap()
}
```

## Performance Notes

- Entities and components are copied in bulk using table-based storage
- Component data is copied directly with no individual allocations
- Entity remapping is O(n) where n is the number of references
- No additional overhead beyond the new entities and temporary mapping table

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE.md) file for details.
