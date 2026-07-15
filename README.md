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
- Two entry points over the same storage. The `ecs!` macro fixes the component set at compile time and generates named accessors, while dynamic worlds (`dynamic` feature) register components at runtime with bundle spawns, typed queries, and marker tags, with the same guarantees
- Plain public data all the way down: tables, allocator, tag sets, and logs are inspectable structs and vecs

The `ecs!` macro generates the entire ECS at compile time using only plain data structures, functions, and zero unsafe code.

## Table of Contents

- [How it works (build it from scratch)](#how-it-works-build-it-from-scratch)
- [Quick Start](#quick-start)
  - [Dynamic: `DynWorld`](#dynamic-dynworld)
  - [Static: the `ecs!` macro](#static-the-ecs-macro)
- [Generated API](#generated-api)
  - [Closure-Based Mutation](#closure-based-mutation)
- [Systems](#systems)
- [Events](#events)
  - [Game Loop Integration](#game-loop-integration)
  - [Event Lifetime](#event-lifetime)
- [High-Performance Features](#high-performance-features)
  - [Query Builder API](#query-builder-api)
  - [Batch Spawning](#batch-spawning)
  - [Single-Component Iteration](#single-component-iteration)
  - [Parallel Iteration](#parallel-iteration)
  - [Sparse Set Tags](#sparse-set-tags)
  - [Command Buffers](#command-buffers)
  - [Mask Hygiene](#mask-hygiene)
  - [Change Detection](#change-detection)
  - [Structural Change Log](#structural-change-log)
  - [System Scheduling](#system-scheduling)
- [Entity Builder](#entity-builder)
- [Entity Liveness](#entity-liveness)
- [Advanced Features](#advanced-features)
  - [Per-Component Iteration](#per-component-iteration)
  - [Low-Level Iteration](#low-level-iteration)
  - [Tick Management](#tick-management)
- [Conditional Compilation](#conditional-compilation)
- [Cargo Features](#cargo-features)
- [Dynamic Worlds](#dynamic-worlds)
  - [Component registration](#component-registration)
  - [Spawning and despawning](#spawning-and-despawning)
  - [Component access](#component-access)
  - [Queries](#queries)
  - [Writing systems](#writing-systems)
  - [Events](#events-1)
  - [Resources](#resources)
  - [Tags](#tags)
  - [Hierarchies](#hierarchies)
  - [Deferred commands](#deferred-commands)
  - [Change detection and sync](#change-detection-and-sync)
  - [Entity inspection](#entity-inspection)
  - [Grouped dynamic worlds](#grouped-dynamic-worlds)
  - [Snapshots](#snapshots)
  - [Named accessors over the keyed tier](#named-accessors-over-the-keyed-tier)
- [Multi-World ECS](#multi-world-ecs)
- [License](#license)

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

freecs has two entry points over the same archetype storage. Reach for the
dynamic world by default. Typed tuple queries with change and added filters,
Bevy-style `Res`/`ResMut`/`Query` system functions, cross-world joins,
prepared queries, hierarchies, exactly-once events, snapshots and deltas, and
the whole grouped multi-world substrate live there, its typed queries compile
to the same slice loops the macro emits, and new surface lands on the dynamic
side first. The `ecs!` macro is the compile-time tier. It fixes a small
component set up front and generates a named accessor for everything over
full static dispatch and no runtime registration, which keeps it the fastest
path to a tiny fixed-schema game, the most explicit API, and the executable
specification the dynamic implementation is tested against. Both run side by
side when you want them to.

### Dynamic: `DynWorld`

Enable the feature:

```toml
[dependencies]
freecs = { version = "3", features = ["dynamic"] }
```

Runtime registration, no macro and no masks. Types register lazily on first
use, tuples spawn as bundles, and systems are plain functions whose arguments
are `Res`, `ResMut`, and `Query`:

```rust
use freecs::Schedule;
use freecs::dynamic::DynWorld;
use freecs::system_param::{Query, Res, ResMut, ScheduleExt};

#[derive(Default, Clone, Debug)]
struct Position { x: f32, y: f32 }

#[derive(Default, Clone, Debug)]
struct Velocity { x: f32, y: f32 }

#[derive(Default, Clone, Debug)]
struct Health { value: f32 }

struct Player;

struct DeltaTime(f32);
struct Score(u32);

// Typed queries take mutability from the tuple. Option elements match
// entities with or without the component.
fn physics(dt: Res<DeltaTime>, mut score: ResMut<Score>, query: Query<(&mut Position, &Velocity, Option<&mut Health>)>) {
    query.for_each(|_entity, (position, velocity, health)| {
        position.x += velocity.x * dt.0;
        position.y += velocity.y * dt.0;
        if let Some(health) = health {
            health.value *= 0.98;
        }
    });
    score.0 += 1;
}

fn main() {
    let mut world = DynWorld::new();
    world.insert_resource(DeltaTime(0.016));
    world.insert_resource(Score(0));

    let player = world.spawn((
        Position { x: 0.0, y: 0.0 },
        Velocity { x: 1.0, y: 2.0 },
        Health { value: 100.0 },
    ));
    world.add_tag_type::<Player>(player);

    let mut schedule = Schedule::new();
    schedule.add_system("physics", physics);
    schedule.run(&mut world);

    // Read-only queries on &world are real iterators.
    let player_positions: Vec<_> = world
        .query_ref::<(&Position,)>()
        .with_tag_type::<Player>()
        .iter()
        .map(|(entity, (position,))| (entity, position.x, position.y))
        .collect();
    println!("{player_positions:?}");

    world.step();
}
```

Everything else in this README's static sections has a dynamic counterpart.
The [Dynamic Worlds](#dynamic-worlds) section covers the full API, the three
access tiers, and measured performance against the macro path. The repository
ships the same complete tower defense game written both ways
(`examples/tower-defense.rs` and `examples/tower-defense-dynamic.rs`).

### Static: the `ecs!` macro

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

Events are stored in sequence-numbered channels, the same cursor scheme the structural change log uses.

The default consumption style is `consume_<event>`: each consumer owns one `u64` cursor, and every call yields the events sent since that consumer last looked, advancing the cursor past them. Calling it every frame delivers each event exactly once, two consumers never steal from or double-process each other, and a consumer that skips a frame catches up:

```rust
struct RenderSync {
    collision_cursor: u64,
}

fn render_sync_system(world: &mut World, sync: &mut RenderSync) {
    for event in world.consume_collision(&mut sync.collision_cursor) {
        // Seen exactly once by this consumer
    }
}
```

The buffer-reading forms, `read_<event>()` and `collect_<event>()`, return everything still buffered. Events stay buffered for **two frames** before `world.step()` expires them, so a per-frame handler using these sees each event twice. They are for debugging, one-shot inspection, or frame setups you manage yourself. When in doubt, use `consume_`.

Each event type gets these generated methods:

- `send_<event>(event)` - Queue an event
- `consume_<event>(&mut cursor)` - Events sent since this cursor, advancing it. Exactly-once per consumer, the default
- `read_<event>_since(cursor)` - Slice of events sent after `cursor`, cursor untouched
- `sequence_<event>()` - Sequence number of the newest event. Record it as your cursor
- `trim_<event>(up_to_sequence)` - Drop consumed events early (pass the minimum cursor across consumers)
- `read_<event>()` - Iterator over all buffered events (up to two frames' worth)
- `collect_<event>()` - Collect buffered events into a Vec (up to two frames' worth)
- `peek_<event>()` - Reference to the oldest buffered event
- `update_<event>()` - Expire events older than one frame. `step()` already calls this per frame, so calling both halves event lifetime
- `clear_<event>()` - Immediately drop all buffered events
- `len_<event>()` / `is_empty_<event>()` - Buffered event count

The dynamic world has the same pair: `consume_events::<T>(&mut cursor)` for exactly-once handling and `read_events::<T>()` for the raw buffer.

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

Events sent during frame N remain readable through frame N+1 and are dropped by the `step()` that ends frame N+1. This preserves the classic double-buffer property: a system scheduled before the sender still sees the event on the next frame. It is also why per-frame handlers must consume through cursors. The two-frame buffer means `read_`/`collect_` deliver the same event on both frames, and `consume_` turns the buffer into exactly-once delivery. Cursor consumers are unaffected by expiry as long as they read at least every other frame. A cursor older than the buffer yields everything still buffered.

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

Query iteration allocates nothing per call. Mutable iteration paths maintain a query cache keyed by component mask so repeated queries skip table matching. One asymmetry to know about is that the read-only `for_each` can consult the cache but cannot populate it (it takes `&self`), so a mask that has only ever been used read-only falls back to a linear scan over tables. Table counts are small in practice, and any mutable query with the same mask warms the cache for both.

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

`par_for_each_mut` parallelizes across archetype tables, so a world with two big archetypes gets at most two-way parallelism from it. The single-component variant `par_for_each_<component>_mut` additionally parallelizes within each table and is the better choice when most matching entities live in one archetype.

Best for 100K+ entities with non-trivial per-entity computation. For smaller entity counts, serial iteration may be more efficient due to parallelization overhead.

Parallel methods are only available when targeting non-WASM platforms. On WASM targets, use the serial iteration methods instead.

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
// The entity must be alive. Tags on dead handles are refused.
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

Tags work well for:

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

Component masks and tag masks share the `u64` but not the same APIs. Spawn masks, `add_components`, `remove_components`, and the mask-only queries (`query_entities`, `query_first_entity`, changed queries) take component bits only, while `for_each`, `for_each_mut`, their changed and parallel variants, and `query_<component>_mut` accept tag bits and filter per entity. Passing tag bits where they don't belong is a `debug_assert` failure rather than a silently empty result, so misuse fails loudly in debug builds and costs nothing in release.

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

Mutations through `set_*()`, `get_*_mut()`, `modify_*()`, `query_*_mut()`, and `iter_*_mut()` mark the component slot as changed for the current tick, as do spawns and component add/remove migrations. Raw table access (`query_mut()` closures, slice iterators, `for_each_*_mut`) does not mark. This matters the moment a downstream consumer diffs by ticks (delta sync, incremental render extraction): a raw-tier write is invisible to it until something else stamps the slot. Route writes through the accessors, or opt in explicitly:

```rust
// Per entity, after a raw write:
world.mark_changed(entity, POSITION | VELOCITY);

// Per table, after a whole-column pass:
let current_tick = world.current_tick();
for table in &mut world.tables {
    if table.mask & POSITION == 0 { continue; }
    for position in &mut table.position { position.x += 1.0; }
    table.mark_columns_changed(POSITION, current_tick);
}
```

`mark_changed(entity, mask)` stamps the masked components on one entity and returns false when the entity is missing or carries none of them. `table.mark_columns_changed(mask, tick)` stamps every row of the masked columns at once, so bulk passes stay free of per-row bookkeeping during the write. The dynamic world has the same pair (`DynWorld::mark_changed`, `mark_columns_changed` on its tables).

Each table also keeps a per-component high-water tick. Changed queries compare it first and skip whole tables that no write has touched since the last `step()`, so scanning cost is proportional to tables with activity rather than total entity count. Tick comparisons are wrapping-safe, so detection keeps working after the `u32` tick counter overflows.

Multiple independent consumers can track their own change windows with the explicit-cursor variants `query_entities_changed_since(mask, since_tick)` and `for_each_mut_changed_since(include, exclude, since_tick, f)`. Record `current_tick()` when you consume, then call `increment_tick()` to fence, so writes made later in the same tick stamp a newer value and land in your next window.

Change tracking stores one `u32` per component per entity plus a tick stamp on every accessor write, whether or not you consume it. That is the price of the feature always being available.

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

A despawn is logged as a single `Despawned` entry. The tags an entity held are dropped implicitly rather than logged individually.

### System Scheduling

Organize systems into a schedule for automatic execution. For programs
assembled from independent parts (engine plugins, feature crates), `Stages`
holds an ordered list of named stages, each its own `Schedule`: the app
declares stage order once, parts push systems into stages by name
(`stages.stage_mut("simulation").push(...)`), stages run in declaration
order and systems within a stage in push order, and pushing into an
undeclared stage panics with the declared list. Deterministic composition
with no labels or constraint graphs.

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
    // Position of every player that also has velocity, marking position changed
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

Everything in this section is also one runnable program,
`examples/tour.rs`, with a comment on each block mapping it back to the
subsection that explains it. It compiles on every CI run, so it cannot
drift from the API:

```sh
cargo run --example tour --features dynamic
```

The examples below share these component types:

```rust
use freecs::dynamic::DynWorld;

#[derive(Default, Clone, Debug)]
struct Position { x: f32, y: f32 }

#[derive(Default, Clone, Debug)]
struct Velocity { x: f32, y: f32 }

#[derive(Default, Clone, Debug)]
struct Health { value: f32 }
```

#### Component registration

Types register lazily on first use, so most programs never register anything
by hand. Register explicitly when you want a `ComponentKey` for the keyed
tier, or to fix mask bits up front (bits are assigned in registration order):

```rust
let mut world = DynWorld::new();

// The spawn registers Position and Velocity lazily.
let entity = world.spawn((Position::default(), Velocity::default()));

// Explicit registration returns a copyable key carrying the mask bit.
let health = world.register::<Health>();
assert_eq!(health.mask, 0b100);

// Components and tags share 64 bits per world, so check the budget in a
// startup assertion instead of meeting the panic at registration 65.
assert!(world.remaining_bits() > 32);
```

For several worlds that must agree on masks, or for snapshots, declare the
schema in one place with `dynamic_schema!`: it generates the mask constants
in declaration order, the registration function building a
`ComponentRegistry` in that exact order, and an assertion per component
that the two agree. Declare every component on every build configuration
and only ever append, so masks stay identical across feature sets and saves
stay loadable. Runtime components keep registering after the declared base;
the schema is a floor, not a ceiling:

```rust
use freecs::dynamic::DynWorld;

freecs::dynamic_schema! {
    pub fn register_components {
        position: Position => POSITION,
        velocity: Velocity => VELOCITY,
        health: Health => HEALTH,
    }
}

let world = DynWorld::from_registry(register_components());
assert_eq!(HEALTH, 0b100);
```

Prefix the function with `serde` (`dynamic_schema! { serde pub fn ... }`)
to register every component with a snapshot codec, so the same declaration
is the save-format schema.

#### Spawning and despawning

```rust
let mut world = DynWorld::new();

// One entity from a bundle of component values.
let player = world.spawn((Position { x: 1.0, y: 2.0 }, Health { value: 100.0 }));

// Many entities carrying clones of one bundle.
let squad = world.spawn_bundles((Position::default(), Velocity::default()), 32);

// Deferred spawn: the handle comes back immediately, alive with no
// components until apply_commands runs the queued bundle write.
let reserved = world.queue_spawn((Position::default(),));
assert!(world.is_alive(reserved));
world.apply_commands();

// Despawn by handle, in bulk, or by component membership.
world.despawn_entities(&squad);
world.despawn_with_any::<(Health,)>();
```

For per-entity initialization at batch speed, the keyed
`spawn_batch(mask, count, |table, index| ...)` fills columns directly.

#### Component access

```rust
let mut world = DynWorld::new();
let entity = world.spawn((Position::default(),));

// Typed access pays one TypeId lookup per call. set adds if missing.
world.set(entity, Velocity { x: 1.0, y: 0.0 });
if let Some(position) = world.get_mut::<Position>(entity) {
    position.x += 1.0;
}
assert!(world.has::<Velocity>(entity));
world.remove::<Velocity>(entity);

// Keyed access skips the hash entirely, for per-entity hot paths.
let position = world.register::<Position>();
world.set_keyed(position, entity, Position { x: 5.0, y: 0.0 });
assert_eq!(world.get_keyed(position, entity).unwrap().x, 5.0);
```

#### Queries

Borrow mutability comes from the tuple, and mutable elements stamp change ticks
per visited entity. Up to eight elements, all component types distinct:

```rust
let mut world = DynWorld::new();
world.spawn((Position::default(), Velocity { x: 1.0, y: 0.0 }));
world.spawn((Position::default(),));

// Most systems are a tuple query with a closure.
world
    .query::<(&mut Position, &Velocity)>()
    .for_each(|_entity, (position, velocity)| {
        position.x += velocity.x;
    });

// Single-component queries skip the tuple.
world.query::<&mut Position>().for_each(|_entity, position| {
    position.y = 0.0;
});

// Option elements match entities with or without the component.
world
    .query::<(&mut Position, Option<&Velocity>)>()
    .for_each(|_entity, (position, velocity)| {
        if let Some(velocity) = velocity {
            position.x += velocity.x;
        }
    });

// Filter with/without by type, mask, or tag, plus changed/added windows.
struct Frozen;
world
    .query::<&mut Position>()
    .without_tag_type::<Frozen>()
    .changed::<Position>()
    .for_each(|_entity, _position| {});
```

On a shared borrow, `query_ref` runs read-only tuples as a real `Iterator`
whose items borrow the world, so results collect and compose with adapters:

```rust
let total: f32 = world
    .query_ref::<(&Position, Option<&Velocity>)>()
    .iter()
    .map(|(_entity, (position, velocity))| {
        position.x + velocity.map_or(0.0, |velocity| velocity.x)
    })
    .sum();

// single() returns the match when exactly one entity qualifies.
if let Some((entity, position)) = world.query_ref::<&Position>().single() {
    println!("{entity} at {}", position.x);
}

// iter_combinations() yields each unordered pair once, for pairwise
// logic like collision tests.
for ((entity_a, a), (entity_b, b)) in world.query_ref::<&Position>().iter_combinations() {
    let _ = (entity_a, entity_b, a.x - b.x);
}
```

In a `DynEcs` group, tuples whose components live in different member
worlds run through `ecs.query_join` with the same filter vocabulary. See
[Grouped dynamic worlds](#grouped-dynamic-worlds).

Hot systems freeze a configured query into a `PreparedQuery` with
`.prepare()` and rerun it without per-call `TypeId` resolution
(`prepared.query(&mut world).for_each(...)`). The read form
(`PreparedQueryRef`) does the same for iterators. Prepared masks are plain
copyable data.

Heavy passes go parallel with `par_for_each`, table-granular like the
keyed tier's `par_for_each_mut`, with the same filters and stamping:

```rust
world
    .query::<(&mut Position, &Velocity)>()
    .par_for_each(|_entity, (position, velocity)| {
        position.x += velocity.x;
    });
```

#### Writing systems

Systems are plain functions over `&mut DynWorld` or `&DynWorld`, and the
borrow checker is the access checker. The take/put scopes give a system a
resource and the world as independent borrows:

```rust
struct DeltaTime(f32);
struct Score(u32);

fn movement_system(world: &mut DynWorld) {
    world.resource_scope(|world, delta_time: &mut DeltaTime| {
        world
            .query::<(&mut Position, &Velocity)>()
            .for_each(|_entity, (position, velocity)| {
                position.x += velocity.x * delta_time.0;
                position.y += velocity.y * delta_time.0;
            });
    });
}

fn score_system(world: &mut DynWorld) {
    world.resources_scope(|world, (score, delta_time): &mut (Score, DeltaTime)| {
        score.0 += world.query_ref::<&Health>().iter().count() as u32;
        let _ = delta_time;
    });
}

fn render_system(world: &DynWorld) {
    for (_entity, position) in world.query_ref::<&Position>().iter() {
        let _ = position;
    }
}

let mut world = DynWorld::new();
world.insert_resource(DeltaTime(0.016));
world.insert_resource(Score(0));

let mut schedule = freecs::Schedule::new();
schedule
    .push("movement", movement_system)
    .push("score", score_system)
    .push_if(
        "expensive",
        |world: &DynWorld| world.entity_count() > 0,
        |_world| {},
    )
    .push_readonly("render", render_system);

schedule.run(&mut world);
world.step();
```

For Bevy-style ergonomics, the `system_param` module turns a plain function
whose arguments are `Res`, `ResMut`, and `Query` into a runnable system, so
the take/put scope disappears from the call site. `add_system` on a
`Schedule<DynWorld>` infers the shape from the signature.

```rust
use freecs::system_param::{Query, Res, ResMut, ScheduleExt};

struct DeltaTime(f32);
struct Score(u32);

fn movement(dt: Res<DeltaTime>, mut score: ResMut<Score>, query: Query<(&mut Position, &Velocity)>) {
    query.for_each(|_entity, (position, velocity)| {
        position.x += velocity.x * dt.0;
        position.y += velocity.y * dt.0;
    });
    score.0 += 1;
}

let mut world = DynWorld::new();
world.insert_resource(DeltaTime(0.016));
world.insert_resource(Score(0));

let mut schedule = freecs::Schedule::new();
schedule.add_system("movement", movement);
schedule.run(&mut world);
```

Resource parameters resolve out of the world's `ResourceMap` through the same
take/put `resources_scope` uses, so they never alias a query's table borrow.
Resource parameters come first, query parameters after. Type-level filters
(`With`, `Without`, `Changed`, `Added`, `WithTag`, `WithoutTag`, and tuples of
them) narrow a query as `Query<(&mut Position,), With<Player>>`.

A single query borrows the world directly. Several queries in one system share
the world through a cell and each take it only for one `for_each`, so two
queries run in sequence rather than nested, at a cost of one borrow check per
call rather than anything per entity:

```rust
use freecs::system_param::{Query, ScheduleExt};

fn resolve(positions: Query<&mut Position>, velocities: Query<&Velocity>) {
    let mut sample = (0.0, 0.0);
    velocities.for_each(|_entity, velocity| sample = (velocity.x, velocity.y));
    positions.for_each(|_entity, position| {
        position.x += sample.0;
        position.y += sample.1;
    });
}
```

`ParamSet<(Query<…>, Query<…>)>` is the alternative when you would rather name
a grouped set and reach its members through `p0()` and `p1()`.

#### Events

Events buffer for two frames. The default consumption is `consume_events`
with one `u64` cursor per consumer: calling it every frame delivers each
event exactly once, and independent consumers never steal from each other.
`read_events` re-reads the whole buffer and is for debugging and one-shot
inspection:

```rust
#[derive(Clone, Debug)]
struct Damage { amount: f32 }

let mut world = DynWorld::new();
world.send(Damage { amount: 10.0 });

let mut cursor = 0;
for event in world.consume_events::<Damage>(&mut cursor) {
    println!("took {}", event.amount);
}
assert!(world.consume_events::<Damage>(&mut cursor).is_empty());

world.step(); // expires events after their two-frame window
```

Store cursors wherever the consumer lives, typically a field on a resource
struct, one per event type per consumer.

#### Resources

```rust
let mut world = DynWorld::new();
world.insert_resource(DeltaTime(0.016));

// Fallible and infallible reads. expect_* panics with the type name.
assert!(world.resource::<Score>().is_none());
let delta_time = world.expect_resource::<DeltaTime>().0;
world.insert_resource(Score(0));
world.expect_resource_mut::<Score>().0 += 1;

// Scopes take resources out for one closure and put them back, even on
// panic. See Writing systems above for the tuple form.
world.resource_scope(|_world, score: &mut Score| score.0 += 1);
let _ = (delta_time, world.remove_resource::<Score>());
```

The scopes are the intended take/put pattern. Reach for them before
hand-rolling remove then reinsert. An engine that wraps `DynWorld` or
`DynEcs` in its own world type needs the closure to receive that wrapper,
which the scope methods cannot do since their closures receive the bare
world. Implement `ResourceHost` on the wrapper (one method returning the
wrapped resource map) and import `ResourceHostExt` for the same scopes as
methods on the host, `host.resource_scope(...)` and
`host.resources_scope(...)`. Their closures receive the host itself, with
the same take/put semantics and panic reinsertion. The host must return
the same map on every call, and debug builds verify the reinserted
resource is still reachable, so a misrouted map fails loudly.

#### Tags

Tags are sparse sets outside the archetype tables: adding or removing one
never migrates the entity. Name them by marker type, or hold `TagKey`
values when the tag set itself is dynamic:

```rust
struct Boss;

let mut world = DynWorld::new();
let entity = world.spawn((Position::default(),));

world.add_tag_type::<Boss>(entity);
assert!(world.has_tag_type::<Boss>(entity));
assert_eq!(world.query_tag_type::<Boss>().count(), 1);
world
    .query_ref::<&Position>()
    .with_tag_type::<Boss>()
    .iter()
    .count();
world.remove_tag_type::<Boss>(entity);

// The keyed form for runtime-defined tags.
let elite = world.register_tag();
world.add_tag(elite, entity);
assert!(world.has_tag(elite, entity));
```

#### Hierarchies

`ChildOf` is a plain up-pointing link, pull-maintained with no hooks:

```rust
use freecs::dynamic::ChildOf;

let mut world = DynWorld::new();
let parent = world.spawn((Position::default(),));
let child = world.spawn((Position::default(), ChildOf(parent)));

assert_eq!(world.children(parent), vec![child]);
world.despawn_recursive(parent); // cycle-tolerant, follows links breadth-first
assert!(!world.is_alive(child));
```

In a `DynEcs` group, use `ecs.despawn_recursive(root)` instead so the
cascade despawns through the group.

Hierarchy-heavy consumers keep a `HierarchyIndex`, a consumer-owned map
synced by pull from the structural log and change ticks, so lookups stop
scanning and each sync costs what changed:

```rust
use freecs::dynamic::{ChildOf, HierarchyIndex};

let mut world = DynWorld::new();
let mut hierarchy = HierarchyIndex::new();

let parent = world.spawn((Position::default(),));
let child = world.spawn((Position::default(), ChildOf(parent)));

hierarchy.sync(&mut world); // once a frame, or before reading
assert_eq!(hierarchy.children(parent), &[child]);
assert_eq!(hierarchy.descendants(parent), vec![child]);
hierarchy.despawn_recursive(&mut world, parent);
```

Every link write that stamps change ticks is picked up: spawns, `set`,
migrations, and raw-tier writes followed by `mark_changed`. Reads reflect
the last sync.

#### Deferred commands

Queue structural changes while iterating and apply them at a safe point:

```rust
let mut world = DynWorld::new();
let entity = world.spawn((Position::default(), Health { value: 1.0 }));

world.query::<&Health>().for_each(|entity, health| {
    let _ = (entity, health);
});

world.queue_set(entity, Health { value: 50.0 });
world.queue_despawn_entity(entity);
world.queue(|world| {
    let _ = world.spawn((Position::default(),));
});
world.apply_commands();
```

`queue_add_components`, `queue_remove_components`, `queue_add_tag_type`,
and `queue_spawn_entities` round out the set.

#### Change detection and sync

Mutable typed-query elements and the typed/keyed accessors stamp change
ticks, and `added` ticks stamp when a component arrives and survive table
migrations. Incremental consumers diff by tick, structural consumers read
the log by cursor:

```rust
let mut world = DynWorld::new();
let position = world.register::<Position>();
let entity = world.spawn((Position::default(),));

world.step();
world.get_mut::<Position>(entity).unwrap().x = 1.0;

// Which entities changed since the last step?
assert_eq!(world.query_entities_changed(position.mask).count(), 1);

// changed/added as query filters, on both query forms.
world
    .query_ref::<&Position>()
    .added::<Position>()
    .iter()
    .count();

// Structural history: spawns, despawns, component moves, tag flips.
let mut cursor = 0;
for change in world.structural_changes_since(cursor) {
    let _ = (change.entity, change.kind, change.mask);
}
cursor = world.structural_sequence();
world.trim_structural_log(cursor);

// Raw-tier writes skip stamping, so opt in explicitly when tick diffing
// matters (see Change Detection above for the static twin).
world.mark_changed(entity, position.mask);
```

#### Entity inspection

The registry is the schema, and it is queryable, which is what editors and
tooling protocols build on:

```rust
let mut world = DynWorld::new();
let entity = world.spawn((Position::default(), Health { value: 3.0 }));

for info in world.entity_components(entity) {
    println!("{} on bit {}", info.type_name, info.mask.trailing_zeros());
}

// Add-by-name for a default value works today with public pieces.
let named_mask = world
    .component_by_name(std::any::type_name::<Health>())
    .map(|info| info.mask);
if let Some(mask) = named_mask {
    let other = world.spawn((Position::default(),));
    world.add_components(other, mask);
}
```

With the `snapshot` feature, serde-registered components also move as
values: `set_component_by_name` / `get_component_by_name` exchange one
component as codec bytes resolved by type name (grouped worlds route the
name to the owning member), so an editor protocol reads and writes any
component over the wire with no per-type dispatch. Writes add the
component when absent and stamp change ticks like any `set`.

Three access tiers, from ergonomic to explicit:

- **Typed**: `spawn(bundle)` / `spawn_bundles(bundle, count)` / `queue_spawn(bundle)` returning the handle before the command applies, `get::<T>` / `set` / `remove`, `query::<(&mut A, &B)>()` with `Option<&T>` elements, up to eight per tuple, and bare single elements (`query::<&mut A>()`), `changed::<T>()` and `added::<T>()` filters on both query forms, `query_ref` iterators on `&world` with `single()` and `iter_combinations()`, marker-type tags (`add_tag_type::<T>`, `with_tag_type::<T>()`), `despawn_with_any::<(A, B)>()`, `ChildOf` links with `children` / `despawn_recursive`, entity inspection (`entity_components`, `component_by_name`), `resource_scope` / `resources_scope` over tuples, `send(event)` / `consume_events::<T>(&mut cursor)`, `insert_resource` / `resource::<T>()` / `expect_resource::<T>()`. `TypeId` lookups happen at registration and per typed call, never inside iteration loops.
- **Keyed**: `register::<T>()` returns a copyable `ComponentKey<T>` carrying the component's mask bit. `get_keyed` / `set_keyed` and mask-based `for_each` / `for_each_mut` skip the hash entirely.
- **Raw tables**: `for_each_tables_mut(mask, 0, |table| ...)` with `table.columns_pair(a, b)` hoists concrete slices once per table for the tightest loops, no change stamping, same covenant as the static path.

Measured against the macro world on the same two-component mutation workload
(three `f32` writes per entity), the typed query runs 0.83 µs per 1k entities
versus 1.12 µs for the static `for_each_mut` closure form, and 75 µs versus
116 µs per 100k. The hoisted table form does 7.3 µs per 10k versus 11.6 µs.
The slice-zip loop shapes vectorize better than per-entity index closures, so
at scale the dynamic fast paths come out ahead. The costs are elsewhere and
bounded. Batch spawning pays function-pointer
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
world's mask space and filter per-world typed queries by set reference.

The group is also where state and signals that cross member (and plugin)
boundaries live: `DynEcs` carries its own resource map
(`insert_resource` / `expect_resource` / `resource_scope` /
`resources_scope`, same semantics as the world's) and its own event
channels (`send` / `consume_events` with per-consumer cursors), and
`ecs.step()` expires group events and steps every member world in one
call. World-local resources and events remain on each `DynWorld`. The two
levels are separate channels, not mirrors.

Marker tags exist at the group level too (`ecs.add_tag_type::<Selected>`,
`remove_tag_type`, `has_tag_type`, `query_tag_type`), and they're the
right home for entity-scoped markers in a grouped world. They spend no
member world's mask bits, need no world index to touch, land in the group
structural log, and drop on group despawn. `ecs.tag_set_type::<T>()` hands
the set to any per-world typed query via `with_tag_set`/`without_tag_set`.
Marker memberships survive snapshots: the group persists each tag's type
name and re-binds the marker to its set on first use after a load.

Tuples that span member worlds run through `ecs.query_join`: one world
drives the iteration at full slice speed (the world holding every mutable
element), the others resolve their elements per entity at `get` speed,
read-only, skipping entities that lack a required foreign component.
Mutable elements in two different worlds panic. Mutate your own state and
read theirs, or co-locate the types in one schema when a hot loop needs
slice speed for everything (registries compose, so two parts sharing a
member world is a declaration choice, not a framework feature). A tuple
that resolves to one world degenerates to a plain scan. Joins filter by
group marker tags (`with_tag_type` / `without_tag_type`) and by
driver-world `changed` / `added` windows:

```rust
ecs.query_join::<(&mut Position, &Burning)>()
    .for_each(|_entity, (position, burning)| {
        position.y += burning.lift;
    });
```

Joins carry the same surface as the single-world forms.
`query_join_ref` runs the read-only join as a real `Iterator` on `&ecs`,
and `query_join(...).par_for_each(...)` walks driver tables in parallel
with foreign worlds shared read-only across threads. `world.stats()` and
`ecs.stats()` return a census of tables, budgets, logs, and caches for
editor overlays, and `compact()` drops empty archetype tables at loading
screens.

Declare the members once with `dynamic_worlds!` (index constants plus the
build function, each member asserted at its declared index, and apps extending a
built group use `add_world_at`):

```rust
freecs::dynamic_worlds! {
    pub fn build_ecs {
        CORE => register_core_components,
        UI => register_ui_components,
    }
}
```

Since a component type lives in exactly one member world (enforced when a
member is added, and diagnosed with both world indices if a later lazy
registration creates a duplicate), the group routes typed access itself: `ecs.get::<T>()`, `set`, `get_mut`, `has`, `remove`,
`query`, and `query_ref` resolve the owning world per type (first member in
index order, cached in the public `type_routes` map), and
`ecs.spawn_with(bundle)` spawns one group entity with each component routed
to its world, so bundles span worlds. Routed access never registers types
lazily. Where a type lives is a schema decision, so `set` on an unregistered
type panics instead of guessing. Member indexing remains for world-level
operations, snapshots, and structural logs:

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

// The group keeps its own lifecycle log, the same two-log split as the
// macro multi-world: "entity spawned or died anywhere" and group tag flips
// are one cursor-consumed stream on the group, while each member world's
// structural log records that world's row history.
let mut cursor = 0;
for change in ecs.structural_changes_since(cursor) {
    // Spawned / Despawned with mask 0, TagsAdded / TagsRemoved carrying
    // the group tag index in the mask field.
    let _ = (change.entity, change.kind);
}
cursor = ecs.structural_sequence();
ecs.trim_structural_log(cursor);
```

The lifecycle log is verified against the macro multi-world's by the
differential oracle: one seeded op stream drives both forms and requires
entry-for-entry identical logs.

`ChildOf` hierarchies cascade at the group level too:
`ecs.despawn_recursive(root)` follows links across every member world and
despawns through the group, so retirement broadcasts everywhere and each
death lands in the lifecycle log. In a group, prefer it over the
single-world form.

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
than misbehaving.

Components and tags share each world's 64 mask bits, components from bit 0 up
and tags from bit 63 down, and lazy registration spends bits silently, so
check `world.remaining_bits()` in a startup assertion rather than discovering
the ceiling when registration 65 panics.


Deltas are the incremental form: `world.delta_cursor()` starts a stream
(fencing the change window), `world.delta_since(&cursor)` captures
everything that changed as a serialized change-set, and
`replica.apply_delta(&delta)` replays it onto a replica seeded from a full
snapshot of the same lineage. Structural entries replay in order, changed
component values ride the same codecs snapshots use, and a trimmed or
overflowed structural log fails the capture loudly so the caller reseeds
from a full snapshot instead of silently diverging. `DynEcs` has the group
forms (`delta_cursor` / `delta_since` / `apply_delta`), covering handle
lifecycle, group tags, and every member world in one change-set. This is
the substrate for network replication and efficient autosave.

### Named accessors over the keyed tier

Heavy users who miss the macro world's generated names (`get_position`,
`set_velocity`, `add_boss`) get them back from `dynamic_accessors!`: it
generates a keys struct, a `resolve` constructor registering everything in
declaration order, and the named methods on your wrapper type, all over the
keyed tier, so they skip the `TypeId` hash and stamp change ticks exactly
like the macro world's accessors:

```rust
use freecs::dynamic::DynWorld;

struct Boss;

struct Game {
    world: DynWorld,
    keys: GameKeys,
}

freecs::dynamic_accessors! {
    pub struct GameKeys for Game { world, keys }
    components {
        position: Position,
        velocity: Velocity,
    }
    tags {
        boss: Boss,
    }
}

let mut world = DynWorld::new();
let keys = GameKeys::resolve(&mut world);
let mut game = Game { world, keys };

let entity = game.world.spawn((Position::default(),));
game.set_position(entity, Position { x: 1.0, y: 0.0 });
game.add_boss(entity);
assert!(game.has_boss(entity));
```

Per component: `get_` / `get_<name>_mut` / `set_` / `remove_` / `has_`.
Per tag: `add_` / `remove_` / `has_` / `query_`. Combine with
`dynamic_schema!` when the same declaration also needs mask constants and a
shared registry.

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

// Despawn cascades across all worlds and returns false for stale handles
ecs.despawn(entity);
```

Despawning is safe against reuse: `despawn` refuses stale or already-despawned handles (returning `false`), and stale handles cannot re-add components in any world, including worlds that never stored the entity. That guarantee is paid for in memory. Despawn broadcasts the retired generation into every world's location table, so each world's table grows to cover any despawned id, 16 bytes per id per world. The trust boundary is the shared allocator: a handle forged for an id it never issued can still insert a row, since worlds have no allocator access.

Tags, events, resources, command buffers, and `Schedule` all work identically in multi-world mode. Structural history is split across two kinds of log, and consumers should pick one oracle per purpose. The ECS keeps a lifecycle log (`structural_changes_since` on the ECS) recording handle allocation and death (`Spawned`/`Despawned` with mask 0) plus tag flips. Each world keeps its own row-level log, where an entity is `Spawned` with a component mask when its first components arrive in that world and `Despawned` when its row leaves. An entity that gains components therefore appears as `Spawned` once in the ECS log and once per world it enters. Sync world contents from world logs, and handle lifetime or tags from the ECS log, rather than merging both.

One asymmetry is that per-world query masks contain only that world's component bits, so tags cannot appear in per-world masks (asserted in debug builds). Tag filtering in multi-world uses the tag-set variants with split borrows:

```rust
let GameEcs { core_world, player, .. } = &ecs;
core_world.for_each_with_tags(POSITION, 0, &[player], &[], |entity, table, idx| {
    // Entities with position that carry the player tag
});
```

Component mask constants (e.g. `POSITION`, `SPRITE`) have globally unique names but each world numbers its components independently starting at bit 0, so never mix masks from different worlds in one query.

Single-world syntax remains unchanged. Multi-world is detected by the presence of multiple `Ident { ... }` blocks inside the first group.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE.md) file for details.
