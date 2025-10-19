# freECS

[<img alt="github" src="https://img.shields.io/badge/github-matthewjberger/freecs-8da0cb?style=for-the-badge&labelColor=555555&logo=github" height="20">](https://github.com/matthewjberger/freecs)
[<img alt="crates.io" src="https://img.shields.io/crates/v/freecs.svg?style=for-the-badge&color=fc8d62&logo=rust" height="20">](https://crates.io/crates/freecs)
[<img alt="docs.rs" src="https://img.shields.io/badge/docs.rs-freecs-66c2a5?style=for-the-badge&labelColor=555555&logo=docs.rs" height="20">](https://docs.rs/freecs)

A high-performance, archetype-based Entity Component System (ECS) for Rust

**Key Features**:
- Zero-cost abstractions with static dispatch
- Multi-threaded parallel processing using Rayon
- Sparse set tags that don't fragment archetypes
- Command buffers for deferred structural changes
- Change detection for incremental updates
- Type-safe double-buffered event system

A macro generates the ECS as part of your source code at compile time. Zero-unsafe, table-based architecture with no generics, traits, or dynamic dispatch.

## Quick Start

Add this to your `Cargo.toml`:

```toml
[dependencies]
freecs = "0.6.0"
```

And in `main.rs`:

```rust
use freecs::{ecs, Entity};

ecs! {
    World {
        position: Position => POSITION,
        velocity: Velocity => VELOCITY,
        health: Health => HEALTH,
    }
    Tags {
        player => PLAYER,
        enemy => ENEMY,
    }
    Events {
        collision: CollisionEvent,
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

    // Add tags to entities (lightweight markers)
    world.add_player(entity);

    // Check if entity has a tag
    if world.has_player(entity) {
        println!("Entity is a player");
    }

    // Remove tags
    world.remove_player(entity);

    // Send events
    world.send_collision(CollisionEvent {
        entity_a: entity,
        entity_b: entity,
    });

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

use events::*;
mod events {
    use super::*;

    #[derive(Debug, Clone)]
    pub struct CollisionEvent {
        pub entity_a: Entity,
        pub entity_b: Entity,
    }
}

mod systems {
    use super::*;

    pub fn run_systems(world: &mut World) {
        // Systems use queries and component accessors
        example_system(world);
        update_positions_system(world);
        collision_handler_system(world);
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

    fn collision_handler_system(world: &mut World) {
        // Process collision events
        for event in world.collect_collision() {
            println!("Collision detected between {:?} and {:?}", event.entity_a, event.entity_b);
        }
    }

    fn health_system(world: &mut World) {
        for entity in world.query_entities(HEALTH) {
            if let Some(health) = world.get_health_mut(entity) {
                health.value *= 0.98;
            }
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

## Batched Processing

For performance-critical systems with large numbers of entities, you can batch process components:

```rust
fn batched_physics_system(world: &mut World) {
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
    
    // Process all entities
    for (_, pos, vel) in &mut entities {
        pos.x += vel.x * dt;
        pos.y += vel.y * dt;
    }
    
    // Write back results
    for (entity, new_pos, _) in entities {
        world.set_position(entity, new_pos);
    }
}
```

This approach minimizes borrowing conflicts and can improve performance by processing data in batches.

## Events

Events provide a type-safe way to communicate between systems:

```rust
ecs! {
    World {
        position: Position => POSITION,
        velocity: Velocity => VELOCITY,
    }
    Events {
        collision: CollisionEvent,
        damage: DamageEvent,
    }
}

use events::*;
mod events {
    use super::*;

    #[derive(Debug, Clone)]
    pub struct CollisionEvent {
        pub entity_a: Entity,
        pub entity_b: Entity,
    }

    #[derive(Debug, Clone)]
    pub struct DamageEvent {
        pub entity: Entity,
        pub amount: f32,
    }
}

fn physics_system(world: &mut World) {
    for entity_a in world.query_entities(POSITION) {
        for entity_b in world.query_entities(POSITION) {
            if check_collision(entity_a, entity_b) {
                world.send_collision(CollisionEvent { entity_a, entity_b });
            }
        }
    }
}

fn damage_system(world: &mut World) {
    for event in world.collect_collision() {
        world.send_damage(DamageEvent {
            entity: event.entity_a,
            amount: 10.0
        });
    }
}

fn health_system(world: &mut World) {
    for event in world.collect_damage() {
        if let Some(health) = world.get_health_mut(event.entity) {
            health.value -= event.amount;
        }
    }
}
```

Each event type gets these generated methods:
- `send_<event>(event)` - Queue an event
- `read_<event>()` - Get an iterator over all queued events
- `collect_<event>()` - Collect events into a Vec (eliminates boilerplate)
- `peek_<event>()` - Get reference to first event without consuming
- `drain_<event>()` - Consume all events (takes ownership)
- `update_<event>()` - Swap buffers (old events cleared, current becomes previous)
- `clear_<event>()` - Immediately clear all events
- `len_<event>()` - Get count of all queued events
- `is_empty_<event>()` - Check if queue is empty

### Game Loop Integration

Call `world.step()` at the end of each frame to handle event cleanup:

```rust
loop {
    input_system(&mut world);
    physics_system(&mut world);
    collision_system(&mut world);

    world.step();  // Cleans up events from previous frame
}
```

The `step()` method handles event lifecycle automatically. For fine-grained control, you can use `update_<event>()` to update individual event types.

### Double Buffering

Events use double buffering to prevent systems from missing events in parallel execution. Events persist for 2 frames by default, then auto-clear on the next `step()` call. For immediate clearing, use `clear_<event>()`.

## High-Performance Features

### Query Builder API

For maximum performance, use the query builder which provides direct table access:

```rust
fn physics_update_system(world: &mut World) {
    let dt = world.resources.delta_time;

    world.query()
        .with(POSITION | VELOCITY)
        .iter(|entity, table, idx| {
            table.position[idx].x += table.velocity[idx].x * dt;
            table.position[idx].y += table.velocity[idx].y * dt;
        });
}
```

This eliminates per-entity lookups and provides cache-friendly sequential access.

The query builder also supports filtering:

```rust
// Exclude entities with specific components
world.query()
    .with(POSITION | VELOCITY)
    .without(PLAYER)
    .iter(|entity, table, idx| {
        // Only processes entities that have position and velocity but NOT player
    });
```

You can also use the lower-level iteration methods directly:

```rust
// Mutable iteration
world.for_each_mut(POSITION | VELOCITY, 0, |entity, table, idx| {
    table.position[idx].x += table.velocity[idx].x;
});

// Read-only iteration
for entity in world.query_entities(POSITION | VELOCITY) {
    let pos = world.get_position(entity).unwrap();
    let vel = world.get_velocity(entity).unwrap();
    println!("Entity {:?} at ({}, {})", entity, pos.x, pos.y);
}
```

### Batch Spawning

Spawn multiple entities efficiently (5.5x faster than individual spawns):

```rust
// Method 1: spawn_batch with initialization callback
let entities = world.spawn_batch(POSITION | VELOCITY, 1000, |table, idx| {
    table.position[idx] = Position { x: idx as f32, y: 0.0 };
    table.velocity[idx] = Velocity { x: 1.0, y: 0.0 };
});

// Method 2: spawn_entities (uses component defaults)
let entities = world.spawn_entities(POSITION | VELOCITY, 1000);

// Method 3: entity builder for small batches
let entities = EntityBuilder::new()
    .with_position(Position { x: 0.0, y: 0.0 })
    .with_velocity(Velocity { x: 1.0, y: 1.0 })
    .spawn(&mut world, 100);
```

### Single-Component Iteration

Optimized iteration for single components:

```rust
world.for_each_position(|position| {
    position.x += 1.0;
});

world.for_each_position_mut(|position| {
    position.y *= 0.99;
});
```

### Parallel Iteration

Process large entity counts across multiple CPU cores using Rayon:

```rust
use freecs::rayon::prelude::*;

fn parallel_physics_system(world: &mut World) {
    let dt = world.resources.delta_time;

    world.par_for_each_mut(POSITION | VELOCITY, 0, |entity, table, idx| {
        table.position[idx].x += table.velocity[idx].x * dt;
        table.position[idx].y += table.velocity[idx].y * dt;
    });
}
```

Best for 100K+ entities with non-trivial per-entity computation. For smaller entity counts, serial iteration may be more efficient due to parallelization overhead.

### Sparse Set Tags

Tags are lightweight markers stored in sparse sets rather than archetypes. This means adding/removing tags doesn't trigger archetype migrations, avoiding fragmentation:

```rust
ecs! {
    World {
        position: Position => POSITION,
        velocity: Velocity => VELOCITY,
    }
    Tags {
        player => PLAYER,
        enemy => ENEMY,
        selected => SELECTED,
    }
}

// Adding tags doesn't move entities between archetypes
world.add_player(entity);
world.add_selected(entity);

// Check if entity has a tag
if world.has_player(entity) {
    println!("Entity is a player");
}

// Query entities by component and filter by tag
for entity in world.query_entities(POSITION | VELOCITY) {
    if world.has_enemy(entity) {
        // Process enemies
    }
}

// Remove tags
world.remove_player(entity);
world.remove_selected(entity);
```

Tags are perfect for:
- Runtime categorization (player, enemy, npc)
- Selection/highlighting states
- Temporary status flags
- Any marker that changes frequently

### Command Buffers

Command buffers allow you to queue structural changes (spawn, despawn, add/remove components) during iteration, then apply them all at once. This avoids borrowing conflicts and archetype invalidation during queries:

```rust
fn death_system(world: &mut World) {
    // Queue despawns during iteration
    let entities_to_despawn: Vec<Entity> = world
        .query_entities(HEALTH)
        .filter(|&entity| {
            world.get_health(entity).map_or(false, |h| h.value <= 0.0)
        })
        .collect();

    for entity in entities_to_despawn {
        world.queue_despawn_entity(entity);
    }

    // Apply all queued commands at once
    world.apply_commands();
}

fn spawn_system(world: &mut World) {
    // Queue entity spawns
    for _ in 0..10 {
        world.queue_spawn(POSITION | VELOCITY);
    }

    // Queue component additions
    for entity in world.query_entities(POSITION) {
        if should_add_health(entity) {
            world.queue_add_components(entity, HEALTH);
        }
    }

    // Queue component removals
    for entity in world.query_entities(VELOCITY) {
        if should_stop(entity) {
            world.queue_remove_components(entity, VELOCITY);
        }
    }

    world.apply_commands();
}
```

Available command buffer operations:
- `queue_spawn(mask)` - Queue entity spawn
- `queue_despawn_entity(entity)` - Queue entity despawn
- `queue_add_components(entity, mask)` - Queue component addition
- `queue_remove_components(entity, mask)` - Queue component removal
- `queue_set_component(entity, component)` - Queue component set/update
- `apply_commands()` - Apply all queued commands

### Change Detection

Track which components have been modified since a specific tick. Useful for incremental updates, networking, or rendering optimizations:

```rust
fn render_system(world: &mut World) {
    let current_tick = world.current_tick();

    // Process only entities whose position changed this frame
    world.query()
        .with(POSITION)
        .changed_since(current_tick - 1)
        .iter(|entity, table, idx| {
            update_sprite_position(&table.position[idx]);
        });

    // Increment the tick counter at the end of the frame
    world.increment_tick();
}

// You can also use for_each_mut_changed for direct iteration
world.for_each_mut_changed(POSITION | VELOCITY, last_tick, |entity, table, idx| {
    // Only processes entities where position OR velocity changed
    sync_to_physics_engine(entity, &table.position[idx], &table.velocity[idx]);
});
```

Change detection tracks modifications at the component table level. Any mutation via `get_*_mut()` or table access marks that component slot as changed for the current tick.

**Performance note**: Change detection adds a small overhead (~20 Melem/s vs normal iteration). Only use it when you need to track changes.

### System Scheduling

Organize systems into a schedule for automatic execution:

```rust
use freecs::Schedule;

fn main() {
    let mut world = World::default();
    let mut schedule = Schedule::new();

    // Add systems in order
    schedule
        .add_system(input_system)
        .add_system(physics_system)
        .add_system(collision_system)
        .add_system(render_system);

    // Game loop
    loop {
        schedule.run(&mut world);
        world.step();
    }
}

fn input_system(world: &mut World) {
    // Handle input
}

fn physics_system(world: &mut World) {
    // Update physics
}

fn collision_system(world: &mut World) {
    // Check collisions
}

fn render_system(world: &mut World) {
    // Render entities
}
```

Systems in a schedule execute sequentially in the order they were added. This provides a simple way to organize your game loop without manually calling each system.

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

## Advanced Features

### Per-Component Iteration

For iterating over a single component type, specialized methods are generated:

```rust
// Read-only iteration
world.iter_position(|position| {
    println!("Position: ({}, {})", position.x, position.y);
});

// Mutable iteration
world.iter_position_mut(|position| {
    position.x += 1.0;
});

// Slice-based iteration (most efficient)
for slice in world.iter_position_slices() {
    for position in slice {
        println!("Position: ({}, {})", position.x, position.y);
    }
}

for slice in world.iter_position_slices_mut() {
    for position in slice {
        position.x *= 2.0;
    }
}

// Query entities with specific component
for entity in world.query_position() {
    println!("Entity with position: {:?}", entity);
}
```

### Tag Queries

Query entities by specific tags:

```rust
// Get all entities with a specific tag
for entity in world.query_player() {
    println!("Player entity: {:?}", entity);
}

for entity in world.query_enemy() {
    if let Some(pos) = world.get_position(entity) {
        println!("Enemy at ({}, {})", pos.x, pos.y);
    }
}
```

### Advanced Command Buffer Operations

Beyond the basic command buffer operations, you can queue additional operations:

```rust
// Queue batch spawns
world.queue_spawn_entities(POSITION | VELOCITY, 100);

// Queue batch despawns
let entities_to_remove = vec![entity1, entity2, entity3];
world.queue_despawn_entities(entities_to_remove);

// Queue component sets (generated per component)
world.queue_set_position(entity, Position { x: 10.0, y: 20.0 });
world.queue_set_velocity(entity, Velocity { x: 1.0, y: 0.0 });

// Queue tag operations
world.queue_add_player(entity);
world.queue_remove_enemy(entity);

// Check command buffer status
if world.command_count() > 100 {
    world.apply_commands();
}

// Clear pending commands without applying
world.clear_commands();
```

### Query Builder (Advanced)

The query builder provides a fluent API for complex queries:

```rust
// Mutable query builder
world.query_mut()
    .with(POSITION | VELOCITY)
    .without(PLAYER)
    .iter(|entity, table, idx| {
        table.position[idx].x += table.velocity[idx].x;
    });

// Read-only query builder
world.query()
    .with(POSITION)
    .without(ENEMY)
    .iter(|entity, table, idx| {
        println!("Position: ({}, {})", table.position[idx].x, table.position[idx].y);
    });
```

### Low-Level Iteration

For maximum control, use the low-level iteration methods:

```rust
// Read-only iteration with include/exclude masks
world.for_each(POSITION | VELOCITY, PLAYER, |entity, table, idx| {
    let pos = &table.position[idx];
    let vel = &table.velocity[idx];
    println!("Non-player entity at ({}, {})", pos.x, pos.y);
});

// Mutable iteration with include/exclude masks
world.for_each_mut(POSITION | VELOCITY, 0, |entity, table, idx| {
    table.position[idx].x += table.velocity[idx].x;
    table.position[idx].y += table.velocity[idx].y;
});

// Check if entity has multiple components
if world.entity_has_components(entity, POSITION | VELOCITY | HEALTH) {
    println!("Entity has all required components");
}
```

### Tick Management

Advanced tick management for change detection:

```rust
let current = world.current_tick();
let previous = world.last_tick();

// Process only entities changed in the last frame
world.for_each_mut_changed(POSITION, previous, |entity, table, idx| {
    sync_transform(entity, &table.position[idx]);
});

// Manually increment tick
world.increment_tick();
```

### Event Peeking

Preview events without consuming them:

```rust
// Peek at the first event
if let Some(event) = world.peek_collision() {
    println!("Next collision: {:?} and {:?}", event.entity_a, event.entity_b);
}

// Check if events exist
if !world.is_empty_collision() {
    let count = world.len_collision();
    println!("Processing {} collision events", count);
}

// Drain events (takes ownership)
for event in world.drain_collision() {
    process_collision(event);
}
```

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE.md) file for details.
