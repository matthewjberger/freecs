# freECS

[<img alt="github" src="https://img.shields.io/badge/github-matthewjberger/freecs-8da0cb?style=for-the-badge&labelColor=555555&logo=github" height="20">](https://github.com/matthewjberger/freecs)
[<img alt="crates.io" src="https://img.shields.io/crates/v/freecs.svg?style=for-the-badge&color=fc8d62&logo=rust" height="20">](https://crates.io/crates/freecs)
[<img alt="docs.rs" src="https://img.shields.io/badge/docs.rs-freecs-66c2a5?style=for-the-badge&labelColor=555555&logo=docs.rs" height="20">](https://docs.rs/freecs)

A high-performance, archetype-based Entity Component System (ECS) for Rust

> Used as the foundation of [Nightshade](https://github.com/matthewjberger/nightshade), a data-oriented game engine in Rust.

**Key Features**:

- Zero-cost abstractions with static dispatch
- Multi-threaded parallel processing using Rayon (automatically enabled on non-WASM platforms)
- Sparse set tags with deterministic iteration that don't fragment archetypes
- Command buffers for deferred structural changes
- Change detection for incremental updates
- Sequence-numbered event channels with exactly-once cursor consumption
- Structural change log covering spawns, despawns, component moves, and tag flips
- Multi-world support for >64 component types with shared entity allocator
- Optional dynamic worlds (`dynamic` feature): register component types at runtime with bundle spawns and typed queries, same storage underneath, still zero unsafe
- Plain public data all the way down: tables, allocator, tag sets, and logs are inspectable structs and vecs

The `ecs!` macro generates the entire ECS at compile time using only plain data structures, functions, and zero unsafe code.

## How it works (build it from scratch)

If you want to understand the data layout under the macro, there is a three-part series that builds the same kernel by hand in around 1500 lines of Rust, motivating each design choice.

- [Part 1, archetype storage](https://matthewberger.dev/articles/posts/build-your-own-ecs-archetype-storage). Generational entity handles, archetype tables in struct-of-arrays layout, spawn and despawn.
- [Part 2, structural change and queries](https://matthewberger.dev/articles/posts/build-your-own-ecs-structural-change). Adding and removing components via archetype migration, walking tables for queries, the archetype graph and query cache.
- [Part 3, change detection, events, tags, and commands](https://matthewberger.dev/articles/posts/build-your-own-ecs-events-changes-tags-commands). Watermark-based change detection, sequence-numbered event channels with cursor consumption, sparse-set tags, deferred command buffers, a system schedule.

freecs is what you get when you put a declarative macro on top of that kernel.

## Quick Start

Add this to your `Cargo.toml`:

```toml
[dependencies]
freecs = "3"
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

    // Query all entities with a specific set of components
    let _players: Vec<Entity> = world.query_entities(POSITION | VELOCITY | HEALTH).collect();

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
        example_system(world);
        update_positions_system(world);
        collision_handler_system(world);
        health_system(world);
    }

    fn example_system(world: &mut World) {
        world.query_position_mut(VELOCITY, |_entity, position| {
            position.x += 1.0;
        });
    }

    fn update_positions_system(world: &mut World) {
        let dt = world.resources.delta_time;

        world.for_each_mut(POSITION | VELOCITY, 0, |_entity, table, idx| {
            table.position[idx].x += table.velocity[idx].x * dt;
            table.position[idx].y += table.velocity[idx].y * dt;
        });
    }

    fn collision_handler_system(world: &mut World) {
        for event in world.collect_collision() {
            println!("Collision detected between {:?} and {:?}", event.entity_a, event.entity_b);
        }
    }

    fn health_system(world: &mut World) {
        world.query_health_mut(0, |_entity, health| {
            health.value *= 0.98;
        });
    }
}
```

## Generated API

The `ecs!` macro generates type-safe methods for each component:

```rust
// For each component, you get:
world.get_position(entity)          // -> Option<&Position>
world.get_position_mut(entity)      // -> Option<&mut Position>
world.modify_position(entity, f)    // -> Option<R> - mutate via closure, returns closure result
world.set_position(entity, pos)     // Sets or adds the component
world.add_position(entity)          // Adds with default value
world.remove_position(entity)       // Removes the component
world.entity_has_position(entity)   // Checks if entity has component
world.query_position()              // Iterator over &Position across all tables
world.query_position_mut(mask, f)   // Visit (Entity, &mut Position) for entities matching mask
world.iter_position(f)              // Visit (Entity, &Position)
world.iter_position_mut(f)          // Visit (Entity, &mut Position)
world.for_each_position_mut(f)      // Visit &mut Position only, fastest typed path, no change stamping
world.par_for_each_position_mut(f)  // Parallel &mut Position (non-WASM)
world.iter_position_slices()        // Iterator over &[Position], one slice per table
world.iter_position_slices_mut()    // Iterator over &mut [Position]
```

### Closure-Based Mutation

The `modify_<component>` methods allow you to mutate a component via a closure, which automatically releases the borrow when done. This is useful when you need to mutate a component and then immediately access the world again:

```rust
// Instead of this pattern (requires explicit drop):
let player = world.get_player_mut(entity).unwrap();
player.stamina -= 10.0;
let _ = player;  // Must drop to release borrow
let pos = world.get_position(entity);

// Use modify for cleaner code:
world.modify_player(entity, |p| p.stamina -= 10.0);
let pos = world.get_position(entity);  // No drop needed

// The closure can return values:
let old_health = world.modify_health(entity, |h| {
    let old = h.value;
    h.value = 100.0;
    old
});
```

## Systems

Systems are functions that query entities and transform their components:

```rust
pub fn update_global_transforms_system(world: &mut World) {
    let entities: Vec<Entity> = world
        .query_entities(LOCAL_TRANSFORM | GLOBAL_TRANSFORM)
        .collect();
    for entity in entities {
        // The entities we queried for are guaranteed to have
        // a local transform and global transform here
        let new_global_transform = query_global_transform(world, entity);
        let global_transform = world.get_global_transform_mut(entity).unwrap();
        *global_transform = GlobalTransform(new_global_transform);
    }
}

pub fn query_global_transform(world: &World, entity: Entity) -> nalgebra_glm::Mat4 {
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

## Events

Events are stored in sequence-numbered channels, the same cursor scheme the structural change log uses. Two consumption styles are supported.

**Frame-scoped** consumption reads whatever is buffered. Events stay visible for two frames, then `world.step()` expires them:

```rust
fn collision_handler_system(world: &mut World) {
    for event in world.collect_collision() {
        println!("Collision: {:?} and {:?}", event.entity_a, event.entity_b);
    }
}
```

**Cursor-based** consumption gives each consumer exactly-once delivery. Record the channel sequence after reading, and read everything newer next time. Two systems consuming the same channel never steal from or double-process each other, and a consumer that skips a frame catches up:

```rust
struct RenderSync {
    collision_cursor: u64,
}

fn render_sync_system(world: &mut World, sync: &mut RenderSync) {
    for event in world.read_collision_since(sync.collision_cursor) {
        // Seen exactly once by this consumer
    }
    sync.collision_cursor = world.sequence_collision();
}
```

Each event type gets these generated methods:

- `send_<event>(event)` - Queue an event
- `read_<event>()` - Iterator over all buffered events
- `read_<event>_since(cursor)` - Slice of events sent after `cursor`
- `sequence_<event>()` - Sequence number of the newest event; record it as your cursor
- `trim_<event>(up_to_sequence)` - Drop consumed events early (pass the minimum cursor across consumers)
- `collect_<event>()` - Collect buffered events into a Vec
- `peek_<event>()` - Reference to the oldest buffered event
- `update_<event>()` - Expire events older than one frame; `step()` already calls this per frame, so calling both halves event lifetime
- `clear_<event>()` - Immediately drop all buffered events
- `len_<event>()` / `is_empty_<event>()` - Buffered event count

Channels are bounded: a channel nobody consumes drops its oldest half at `EVENT_CHANNEL_CAPACITY` entries instead of leaking.

### Game Loop Integration

Call `world.step()` at the end of each frame to handle event expiry and the change-detection tick:

```rust
loop {
    input_system(&mut world);
    physics_system(&mut world);
    collision_system(&mut world);

    world.step();  // Expires old events and increments the tick counter
}
```

### Event Lifetime

Events sent during frame N remain readable through frame N+1 and are dropped by the `step()` that ends frame N+1. This preserves the classic double-buffer property: a system scheduled before the sender still sees the event on the next frame. Cursor consumers are unaffected by expiry as long as they read at least every other frame; `read_<event>_since` with a cursor older than the buffer returns everything still buffered.

## High-Performance Features

### Query Builder API

For maximum performance, use the query builder which provides direct table access:

```rust
fn physics_update_system(world: &mut World) {
    let dt = world.resources.delta_time;

    world.query_mut()
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

Query iteration allocates nothing per call. Mutable iteration paths maintain a query cache keyed by component mask so repeated queries skip table matching. One asymmetry to know about: the read-only `for_each` can consult the cache but cannot populate it (it takes `&self`), so a mask that has only ever been used read-only falls back to a linear scan over tables. Table counts are small in practice, and any mutable query with the same mask warms the cache for both.

### Batch Spawning

Spawn multiple entities efficiently:

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

Optimized iteration when you only need one component type:

```rust
// Entity and component reference
world.iter_position(|entity, position| {
    println!("{entity}: ({}, {})", position.x, position.y);
});

// Mutable, marks the component changed for change detection
world.iter_position_mut(|_entity, position| {
    position.y *= 0.99;
});

// Component-only fast path, no entity, no change stamping
world.for_each_position_mut(|position| {
    position.x += 1.0;
});
```

### Parallel Iteration

Process large entity counts across multiple CPU cores using Rayon. Parallel iteration is automatically available on non-WASM platforms:

```rust
fn parallel_physics_system(world: &mut World) {
    let dt = world.resources.delta_time;

    world.par_for_each_mut(POSITION | VELOCITY, 0, |entity, table, idx| {
        table.position[idx].x += table.velocity[idx].x * dt;
        table.position[idx].y += table.velocity[idx].y * dt;
    });
}
```

**Parallelism granularity**: `par_for_each_mut` parallelizes across archetype tables, so a world with two big archetypes gets at most two-way parallelism from it. The single-component variant `par_for_each_<component>_mut` additionally parallelizes within each table and is the better choice when most matching entities live in one archetype.

Best for 100K+ entities with non-trivial per-entity computation. For smaller entity counts, serial iteration may be more efficient due to parallelization overhead.

**Note**: Parallel methods are only available when targeting non-WASM platforms. On WASM targets, use the regular serial iteration methods instead.

### Sparse Set Tags

Tags are lightweight markers stored in sparse sets (a dense `Vec<Entity>` plus a sparse index array), not in archetypes. Adding or removing a tag never migrates the entity, membership checks are O(1) array lookups with no hashing, iteration over a tag is contiguous and deterministic, and membership is generation-checked so a stale handle never matches a reused id:

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

// Adding tags doesn't move entities between archetypes.
// The entity must be alive; tags on dead handles are refused.
world.add_player(entity);
world.add_selected(entity);

// Check if entity has a tag
if world.has_player(entity) {
    println!("Entity is a player");
}

// Iterate a tag directly (deterministic order)
for entity in world.query_player() {
    println!("Player entity: {:?}", entity);
}

// Tags participate in query masks alongside components
world.for_each_mut(POSITION | VELOCITY, PLAYER, |entity, table, idx| {
    // Entities with position and velocity that are NOT players
});

// Remove tags
world.remove_player(entity);
```

Tag masks occupy the top bits of the `u64`, components fill from the bottom, and the macro asserts at compile time that they fit together. Tag adds and removes are recorded in the structural change log, so incremental consumers see tag flips the same way they see component changes.

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
```

Available command buffer operations:

- `queue_spawn_entities(mask, count)` - Queue a batch spawn
- `queue_despawn_entity(entity)` / `queue_despawn_entities(entities)` - Queue despawns
- `queue_add_components(entity, mask)` - Queue component addition
- `queue_remove_components(entity, mask)` - Queue component removal
- `queue_set_<component>(entity, value)` - Queue component set/update
- `queue_add_<tag>(entity)` / `queue_remove_<tag>(entity)` - Queue tag changes
- `apply_commands()` - Apply all queued commands
- `command_count()` / `clear_commands()` - Inspect or drop the queue

### Mask Hygiene

Component masks and tag masks share the `u64` but not the same APIs. Spawn masks, `add_components`, `remove_components`, and the mask-only queries (`query_entities`, `query_first_entity`, changed queries) take component bits only; `for_each`, `for_each_mut`, their changed and parallel variants, and `query_<component>_mut` accept tag bits and filter per entity. Passing tag bits where they don't belong is a `debug_assert` failure rather than a silently empty result, so misuse fails loudly in debug builds and costs nothing in release.

### Change Detection

Track which components have been modified since the last frame. Useful for incremental updates, networking, or rendering optimizations:

```rust
fn render_system(world: &mut World) {
    // Process only entities whose components changed since last step()
    world.for_each_mut_changed(POSITION, 0, |entity, table, idx| {
        update_sprite_position(&table.position[idx]);
    });
}

// At the end of your game loop
world.step();  // Increments tick counter and expires old events
```

Mutations through `set_*()`, `get_*_mut()`, `modify_*()`, `query_*_mut()`, and `iter_*_mut()` mark the component slot as changed for the current tick, as do spawns and component add/remove migrations. Raw table access (`query_mut()` closures, slice iterators, `for_each_*_mut`) does not mark, so route writes through the accessors when you rely on change detection.

Each table also keeps a per-component high-water tick. Changed queries compare it first and skip whole tables that no write has touched since the last `step()`, so scanning cost is proportional to tables with activity rather than total entity count. Tick comparisons are wrapping-safe, so detection keeps working after the `u32` tick counter overflows.

Multiple independent consumers can track their own change windows with the explicit-cursor variants `query_entities_changed_since(mask, since_tick)` and `for_each_mut_changed_since(include, exclude, since_tick, f)`. Record `current_tick()` when you consume, then call `increment_tick()` to fence, so writes made later in the same tick stamp a newer value and land in your next window.

**Fixed cost**: change tracking stores one `u32` per component per entity plus a tick stamp on every accessor write, whether or not you consume it. That is the price of the feature always being available.

### Structural Change Log

Change ticks cover component writes. Structural changes are recorded in a per-world log of plain `StructuralChange` entries: entity, kind, and the mask involved. Kinds cover `Spawned`, `Despawned`, `ComponentsAdded`, `ComponentsRemoved`, `TagsAdded`, and `TagsRemoved` (the full mask for spawns and despawns, the delta for adds and removes, the tag mask for tag flips). Consumers read `structural_changes_since(cursor)` against a `u64` sequence cursor they own and record `structural_sequence()` after consuming. The owner of the frame loop calls `trim_structural_log(up_to_sequence)` with the minimum cursor across consumers. A world whose log is never consumed self-clears at `STRUCTURAL_LOG_CAPACITY` entries, so it stays bounded instead of leaking.

```rust
let mut cursor = 0;
// ... spawns, despawns, component and tag changes happen ...
for change in world.structural_changes_since(cursor) {
    match change.kind {
        StructuralChangeKind::Spawned | StructuralChangeKind::ComponentsAdded => { /* mask gained */ }
        StructuralChangeKind::Despawned | StructuralChangeKind::ComponentsRemoved => { /* mask lost */ }
        StructuralChangeKind::TagsAdded | StructuralChangeKind::TagsRemoved => { /* tag mask flipped */ }
    }
}
cursor = world.structural_sequence();
world.trim_structural_log(cursor);
```

A despawn is logged as a single `Despawned` entry; the tags an entity held are dropped implicitly rather than logged individually.

### System Scheduling

Organize systems into a schedule for automatic execution:

```rust
use freecs::Schedule;

fn main() {
    let mut world = World::default();

    // Create separate schedules for game logic and rendering
    let mut game_schedule = Schedule::new();
    game_schedule
        .push("input", input_system)
        .push("physics", physics_system)
        .push("collision", collision_system);

    let mut render_schedule = Schedule::new();
    render_schedule
        .push_readonly("render_grid", render_grid)
        .push_readonly("render_entities", render_entities);

    // Game loop
    loop {
        game_schedule.run(&mut world);     // Run game logic
        render_schedule.run(&mut world);   // Run rendering
        world.step();
    }
}
```

**Schedule API**:

- `push(name, system)` / `push_readonly(name, system)` - Append a mutable or read-only system
- `insert_before(target, name, system)` / `insert_after(target, name, system)` - Positional insertion
- `replace(name, system)` - Swap a system in-place, preserving execution order
- `remove(name)` - Remove a system by name (returns `bool`)
- `contains(name)` / `names()` / `len()` / `is_empty()` - Introspection

All systems require a unique `&'static str` name. Duplicates panic at insertion time.

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

## Entity Liveness

Entity handles are generational, and the allocator is the single source of truth for liveness. Double despawns and despawns through stale handles are refused rather than corrupting the free list, so two live entities can never share an id and generation. `world.is_alive(entity)` answers liveness directly, and `despawn_entities` returns the subset of handles that were actually despawned.

Liveness costs one slot record write per singleton spawn. Batch spawns write the slot table with one bulk fill for the contiguous fresh ids, so batch spawning and despawning are both faster than 2.x despite the added guarantee.

## Advanced Features

### Per-Component Iteration

For iterating over a single component type, specialized methods are generated:

```rust
// Read-only iteration with the owning entity
world.iter_position(|entity, position| {
    println!("{entity}: ({}, {})", position.x, position.y);
});

// Mutable iteration (marks changed)
world.iter_position_mut(|_entity, position| {
    position.x += 1.0;
});

// Slice-based iteration (most efficient, no change stamping)
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

// Iterate component values directly
for position in world.query_position() {
    println!("Position: ({}, {})", position.x, position.y);
}

// Visit a component for entities matching an additional mask (components or tags)
world.query_position_mut(VELOCITY | PLAYER, |entity, position| {
    // Position of every player that also has velocity; marks position changed
});
```

### Low-Level Iteration

For maximum control, use the low-level iteration methods:

```rust
// Read-only iteration with include/exclude masks (tags allowed in both)
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

Query the current and previous tick counters for advanced change detection:

```rust
let current = world.current_tick();
let previous = world.last_tick();

// Process only entities changed since last frame
world.for_each_mut_changed(POSITION, 0, |entity, table, idx| {
    sync_transform(entity, &table.position[idx]);
});

// Tick is automatically incremented by world.step()
world.step();
```

## Conditional Compilation

Both components and resources support `#[cfg(...)]` attributes for conditional compilation. This is useful for debug-only components, optional features, or platform-specific functionality:

```rust
ecs! {
    World {
        position: Position => POSITION,
        velocity: Velocity => VELOCITY,
        #[cfg(debug_assertions)]
        debug_info: DebugInfo => DEBUG_INFO,
        #[cfg(feature = "physics")]
        rigid_body: RigidBody => RIGID_BODY,
    }
    Resources {
        delta_time: f32,
        #[cfg(feature = "audio")]
        audio_engine: AudioEngine,
    }
}
```

When a component or resource has a `#[cfg(...)]` attribute, all related generated code (struct fields, accessor methods, mask constants, enum variants, etc.) is conditionally compiled based on the feature flag or target configuration.

## Cargo Features

- `serde` (default): derives `Serialize`/`Deserialize` on `Entity`. Disable with `default-features = false` if you don't need it.
- `dynamic` (off by default): the runtime-registered [dynamic world](#dynamic-worlds) entry point. Costs the default build nothing.
- `snapshot` (off by default, implies `dynamic` and `serde`): serializable snapshots of dynamic worlds and groups, with per-type column codecs registered alongside components.

## Dynamic Worlds

The `dynamic` feature adds a second entry point for programs that cannot fix
their component set at compile time, editors, plugin boundaries, data-driven
prefab schemas. `DynWorld` registers component types at runtime and keeps the
rest of the design: contiguous `Vec<T>` columns per archetype, `u64` masks,
the same change detection, structural log, sparse-set tags, event channels,
and liveness guarantees, and zero `unsafe`. Columns are erased as whole vecs
behind `Box<dyn Any + Send + Sync>`, never as raw bytes, and structural
changes dispatch through a per-type record of plain function pointers that is
itself public data.

```rust
use freecs::dynamic::DynWorld;

#[derive(Default, Clone, Debug)]
struct Position { x: f32, y: f32 }

#[derive(Default, Clone, Debug)]
struct Velocity { x: f32, y: f32 }

let mut world = DynWorld::new();

// Types register lazily on first use; tuples spawn as bundles.
let entity = world.spawn((
    Position { x: 0.0, y: 0.0 },
    Velocity { x: 1.0, y: 2.0 },
));

// Typed queries take mutability from the tuple and support
// with/without/changed and tag filters. Mutable elements stamp change ticks.
world
    .query::<(&mut Position, &Velocity)>()
    .for_each(|_entity, (position, velocity)| {
        position.x += velocity.x;
        position.y += velocity.y;
    });

assert_eq!(world.get::<Position>(entity).unwrap().x, 1.0);

// Optional elements visit every match and yield None where the component
// is missing; tuples take up to eight elements.
world
    .query::<(&mut Position, Option<&Velocity>)>()
    .for_each(|_entity, (position, velocity)| {
        if let Some(velocity) = velocity {
            position.x += velocity.x;
        }
    });

// On a shared borrow, read-only tuples run as a real Iterator whose items
// borrow the world, so results collect and compose with adapters.
let fastest = world
    .query_ref::<(&Position, Option<&Velocity>)>()
    .iter()
    .filter_map(|(_entity, (_position, velocity))| velocity)
    .map(|velocity| velocity.x)
    .fold(0.0_f32, f32::max);
assert_eq!(fastest, 1.0);

// resource_scope takes a resource out of the world for one closure, so a
// system can mutate the resource and the world without borrow juggling.
world.insert_resource(0u32);
world.resource_scope(|world, kills: &mut u32| {
    *kills += world.query_ref::<(&Position,)>().iter().count() as u32;
});
```

Three access tiers, from ergonomic to explicit:

- **Typed**: `spawn(bundle)`, `get::<T>` / `set` / `remove`, `query::<(&mut A, &B)>()` with `Option<&T>` elements and up to eight per tuple, `query_ref` iterators on `&world`, `resource_scope`, `send(event)` / `read_events_since::<T>(cursor)`, `insert_resource` / `resource::<T>()`. `TypeId` lookups happen at registration and per typed call, never inside iteration loops.
- **Keyed**: `register::<T>()` returns a copyable `ComponentKey<T>` carrying the component's mask bit; `get_keyed` / `set_keyed` and mask-based `for_each` / `for_each_mut` skip the hash entirely.
- **Raw tables**: `for_each_tables_mut(mask, 0, |table| ...)` with `table.columns_pair(a, b)` hoists concrete slices once per table for the tightest loops, no change stamping, same covenant as the static path.

Measured against the macro world on the same two-component mutation workload
(three `f32` writes per entity), the typed query runs 0.83 µs per 1k entities
versus 1.12 µs for the static `for_each_mut` closure form, and 75 µs versus
116 µs per 100k; the hoisted table form does 7.3 µs per 10k versus 11.6 µs.
The slice-zip loop shapes vectorize better than per-entity index closures, so
the dynamic fast paths are not merely competitive, at scale they are ahead.
The costs live elsewhere and are bounded: batch spawning pays function-pointer
column fills (16.5 µs versus 12.2 µs per 1k spawns), per-entity typed access
pays the `TypeId` map (16.5 ns versus 6.8 ns keyed), and every column adds one
`Box` indirection per table.

Component types need `Send + Sync + Default + 'static`, the same effective bounds the macro path relies on (`Default` because migration moves values with `mem::take`, `Send + Sync` for parallel iteration).

### Grouped dynamic worlds

`DynEcs` groups dynamic worlds over one shared entity allocator, the dynamic
counterpart of the macro's multi-world form and the escape hatch past 64
components. Each member world carries its own registry and full mask space,
one entity can hold rows in any combination of worlds, and despawning retires
it everywhere with the same generation broadcast the static multi-world uses,
so stale handles are refused in every member. Group tags live outside any
world's mask space and filter per-world typed queries by set reference:

```rust
use freecs::dynamic::{ComponentRegistry, DynEcs};

let mut ecs = DynEcs::new();
let core = ecs.add_world(ComponentRegistry::new());
let render = ecs.add_world(ComponentRegistry::new());
let selected = ecs.register_tag();

let entity = ecs.spawn();
ecs.worlds[core].set(entity, Position { x: 1.0, y: 0.0 });
ecs.worlds[render].set(entity, Sprite { id: 7 });
ecs.add_tag(selected, entity);

let DynEcs { worlds, tags, .. } = &mut ecs;
worlds[core]
    .query::<(&mut Position,)>()
    .with_tag_set(&tags[selected])
    .for_each(|_entity, (position,)| position.x += 1.0);
```

### Snapshots

The `snapshot` feature makes dynamic worlds serializable. Components register
with a column codec, `register_serde::<T>()` uses postcard for the column
bytes, or `register_codec` supplies any byte format, and `world.snapshot()`
produces a plain `DynWorldSnapshot` you serialize with whatever serde format
you like. `DynWorld::from_snapshot(registry, &snapshot)` rebuilds the world
over a registry with the same registration order (appending new components
after the snapshot's schema is fine, masks stay stable). Allocator state
survives, so despawned ids recycle correctly after a load, stale-handle
refusal is reconstructed from allocator liveness even for entities that never
had a row, and every restored slot reads as changed so incremental consumers
resync. Events, pending commands, and the structural log are transient and
not captured. `DynEcs` snapshots the same way with one registry per member
world.

The trust boundary is the registry. Bits are assigned in registration order,
so registration is schema: build one `ComponentRegistry`, clone it into every
world that must agree on masks, and register deterministically if masks are
ever serialized. Keys carry their registry id and are debug-checked against
the world using them. Query tuples must not repeat a component type, and a
wrong-type column swapped in by hand panics on the next typed access rather
than misbehaving. The correctness story is the same suite the macro worlds
earned: unit coverage, the three-oracle property tests, and a differential
oracle that drives a macro world and a `DynWorld` with one seeded op stream
and requires identical observable state at every step.

## Multi-World ECS

For projects exceeding 64 component types, you can split components across multiple independent worlds that share a single entity allocator. Each world retains full `u64` bitmask performance (up to 64 components per world).

```rust
use freecs::{ecs, Entity, Schedule};

ecs! {
    GameEcs {
        CoreWorld {
            position: Position => POSITION,
            velocity: Velocity => VELOCITY,
        }
        RenderWorld {
            sprite: Sprite => SPRITE,
            color: Color => COLOR,
        }
    }
    Tags { player => PLAYER }
    Events { collision: CollisionEvent }
    GameResources { delta_time: f32 }
}
```

Entities are spawned from the shared allocator and can have components in any combination of worlds:

```rust
let mut ecs = GameEcs::default();

// Spawn an entity and add components across worlds
let entity = ecs.spawn();
ecs.core_world.set_position(entity, Position { x: 0.0, y: 0.0 });
ecs.render_world.set_sprite(entity, Sprite { id: 1 });

// EntityBuilder spans worlds automatically
let entities = EntityBuilder::new()
    .with_position(Position { x: 0.0, y: 0.0 })
    .with_sprite(Sprite { id: 2 })
    .spawn(&mut ecs, 1);

// Per-world queries run at full bitmask speed
ecs.core_world.for_each_mut(POSITION | VELOCITY, 0, |entity, table, idx| {
    table.position[idx].x += table.velocity[idx].x;
});

// Cross-world access via split borrowing
let GameEcs { core_world, render_world, player, .. } = &mut ecs;
core_world.for_each(POSITION, 0, |entity, table, idx| {
    if let Some(sprite) = render_world.get_sprite(entity) {
        // Access components from both worlds
    }
});

// Despawn cascades across all worlds; returns false for stale handles
ecs.despawn(entity);
```

Despawning is safe against reuse: `despawn` refuses stale or already-despawned handles (returning `false`), and stale handles cannot re-add components in any world, including worlds that never stored the entity. That guarantee is paid for in memory. Despawn broadcasts the retired generation into every world's location table, so each world's table grows to cover any despawned id, 16 bytes per id per world. The trust boundary is the shared allocator: a handle forged for an id it never issued can still insert a row, since worlds have no allocator access.

Tags, events, resources, command buffers, and `Schedule` all work identically in multi-world mode. Structural history is split across two kinds of log, and consumers should pick one oracle per purpose. The ECS keeps a lifecycle log (`structural_changes_since` on the ECS) recording handle allocation and death (`Spawned`/`Despawned` with mask 0) plus tag flips. Each world keeps its own row-level log, where an entity is `Spawned` with a component mask when its first components arrive in that world and `Despawned` when its row leaves. An entity that gains components therefore appears as `Spawned` once in the ECS log and once per world it enters; sync world contents from world logs, and handle lifetime or tags from the ECS log, rather than merging both.

One asymmetry: per-world query masks contain only that world's component bits, so tags cannot appear in per-world masks (this is asserted in debug builds). Tag filtering in multi-world uses the tag-set variants with split borrows:

```rust
let GameEcs { core_world, player, .. } = &ecs;
core_world.for_each_with_tags(POSITION, 0, &[player], &[], |entity, table, idx| {
    // Entities with position that carry the player tag
});
```

Component mask constants (e.g. `POSITION`, `SPRITE`) have globally unique names but each world numbers its components independently starting at bit 0, so never mix masks from different worlds in one query.

Single-world syntax remains unchanged. Multi-world is detected by the presence of multiple `Ident { ... }` blocks inside the first group.

## Migrating from 2.x

3.0 is a breaking release. The changes that affect user code:

- `drain_<event>()` is gone; use `collect_<event>()` plus `clear_<event>()`, or better, the cursor API (`read_<event>_since` / `sequence_<event>`).
- Tags are `SparseTagSet` instead of `HashSet<Entity>`: `contains(entity)` takes the entity by value, and tag adds require a live entity.
- Tag mask values moved to the top bits of the `u64`. Constants are opaque, so this only matters for masks you serialized.
- Multi-world `despawn` returns `bool`, and `EntityAllocator::deallocate` returns `bool` (false for stale or double frees).
- Multi-world `remove_entity` is now `retire_entity` and is generation-safe.
- The single-world `Component` enum is now named `<World>Component`, and `COMPONENT_COUNT` is `<WORLD>_COMPONENT_COUNT` (components only, tags are counted separately).
- Passing tag bits to spawn masks, `add_components`/`remove_components`, or the mask-only queries is a `debug_assert` failure instead of silent misbehavior.
- `serde` support is now behind the (default-on) `serde` feature.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE.md) file for details.
