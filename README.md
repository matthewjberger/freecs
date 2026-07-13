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
- Two first-class entry points: the `ecs!` macro fixes the component set at compile time and generates named accessors, while dynamic worlds (`dynamic` feature) register components at runtime with bundle spawns, typed queries, and marker tags, over the same storage and with the same guarantees
- Plain public data all the way down: tables, allocator, tag sets, and logs are inspectable structs and vecs

The `ecs!` macro generates the entire ECS at compile time using only plain data structures, functions, and zero unsafe code.

## Table of Contents

- [How it works (build it from scratch)](#how-it-works-build-it-from-scratch)
- [Quick Start](#quick-start)
  - [Static: the `ecs!` macro](#static-the-ecs-macro)
  - [Dynamic: `DynWorld`](#dynamic-dynworld)
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

freecs has two first-class entry points over the same archetype storage. The
`ecs!` macro fixes your component set at compile time and generates a named
accessor for everything, which is the fastest path to a working game. The
dynamic world registers component types at runtime for programs that cannot
know their schema up front, editors, plugin hosts, data-driven prefabs, and
its typed queries compile to the same slice loops. Pick whichever fits, or
run both side by side.

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

### Dynamic: `DynWorld`

Enable the feature:

```toml
[dependencies]
freecs = { version = "3", features = ["dynamic"] }
```

The same game shape with runtime registration, no macro and no masks:

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

// Lazy: the spawn registers Position and Velocity.
let entity = world.spawn((Position::default(), Velocity::default()));

// Explicit: returns a copyable key carrying the component's mask bit.
let health = world.register::<Health>();
assert_eq!(health.mask, 0b100);

// Components and tags share 64 bits per world; check the budget in a
// startup assertion instead of meeting the panic at registration 65.
assert!(world.remaining_bits() > 32);
```

For several worlds that must agree on masks, or for snapshots, build one
`ComponentRegistry` up front and construct worlds from it:

```rust
use freecs::dynamic::{ComponentRegistry, DynWorld};

fn build_registry() -> ComponentRegistry {
    let mut registry = ComponentRegistry::new();
    registry.register::<Position>();
    registry.register::<Velocity>();
    registry.register::<Health>();
    registry
}

let world = DynWorld::from_registry(build_registry());
```

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

Borrow mutability comes from the tuple; mutable elements stamp change ticks
per visited entity. Up to eight elements, all component types distinct:

```rust
let mut world = DynWorld::new();
world.spawn((Position::default(), Velocity { x: 1.0, y: 0.0 }));
world.spawn((Position::default(),));

// The workhorse: a tuple query with a closure.
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

// Filters: with/without by type, mask, or tag, and changed/added windows.
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

// single() is the exactly-one-match read, the get-the-player call.
if let Some((entity, position)) = world.query_ref::<&Position>().single() {
    println!("{entity} at {}", position.x);
}

// iter_combinations() yields each unordered pair once, for pairwise
// logic like collision tests.
for ((entity_a, a), (entity_b, b)) in world.query_ref::<&Position>().iter_combinations() {
    let _ = (entity_a, entity_b, a.x - b.x);
}
```

#### Writing systems

Systems are plain functions over `&mut DynWorld` or `&DynWorld`; the borrow
checker is the access checker. The take/put scopes give a system a resource
and the world as independent borrows, so there is no borrow juggling:

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

// Fallible and infallible reads; expect_* panics with the type name.
assert!(world.resource::<Score>().is_none());
let delta_time = world.expect_resource::<DeltaTime>().0;
world.insert_resource(Score(0));
world.expect_resource_mut::<Score>().0 += 1;

// Scopes take resources out for one closure and put them back, even on
// panic; see Writing systems above for the tuple form.
world.resource_scope(|_world, score: &mut Score| score.0 += 1);
let _ = (delta_time, world.remove_resource::<Score>());
```

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
ticks; `added` ticks stamp when a component arrives and survive table
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

// Raw-tier writes skip stamping; opt in explicitly when tick diffing
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

Three access tiers, from ergonomic to explicit:

- **Typed**: `spawn(bundle)` / `spawn_bundles(bundle, count)` / `queue_spawn(bundle)` returning the handle before the command applies, `get::<T>` / `set` / `remove`, `query::<(&mut A, &B)>()` with `Option<&T>` elements, up to eight per tuple, and bare single elements (`query::<&mut A>()`), `changed::<T>()` and `added::<T>()` filters on both query forms, `query_ref` iterators on `&world` with `single()` and `iter_combinations()`, marker-type tags (`add_tag_type::<T>`, `with_tag_type::<T>()`), `despawn_with_any::<(A, B)>()`, `ChildOf` links with `children` / `despawn_recursive`, entity inspection (`entity_components`, `component_by_name`), `resource_scope` / `resources_scope` over tuples, `send(event)` / `consume_events::<T>(&mut cursor)`, `insert_resource` / `resource::<T>()` / `expect_resource::<T>()`. `TypeId` lookups happen at registration and per typed call, never inside iteration loops.
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

### Named accessors over the keyed tier

Heavy users who miss the macro's generated names (`get_position`,
`set_velocity`) can have them back in about ten lines: wrap the world with a
`Keys` struct resolved once at construction, and map names to the keyed tier
with a local macro. The keyed accessors stamp change ticks identically to the
macro's and skip the `TypeId` hash, so the wrappers run at keyed speed:

```rust
use freecs::dynamic::{ComponentKey, DynWorld};

struct Keys {
    position: ComponentKey<Position>,
    velocity: ComponentKey<Velocity>,
}

struct GameWorld {
    world: DynWorld,
    keys: Keys,
}

macro_rules! named_accessors {
    ($($name:ident: $type:ty),+ $(,)?) => {
        freecs::paste::paste! {
            impl GameWorld {
                $(
                    pub fn [<get_ $name>](&self, entity: freecs::Entity) -> Option<&$type> {
                        self.world.get_keyed(self.keys.$name, entity)
                    }

                    pub fn [<get_ $name _mut>](&mut self, entity: freecs::Entity) -> Option<&mut $type> {
                        self.world.get_mut_keyed(self.keys.$name, entity)
                    }

                    pub fn [<set_ $name>](&mut self, entity: freecs::Entity, value: $type) {
                        self.world.set_keyed(self.keys.$name, entity, value);
                    }
                )+
            }
        }
    };
}

named_accessors!(position: Position, velocity: Velocity);
```

The registration function that builds `Keys` becomes the single source of
truth for the component set: define the type, add one line there and one to
the macro invocation. The correctness story is the same suite the macro worlds
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

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE.md) file for details.
