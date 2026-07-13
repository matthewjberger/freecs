//! A high-performance, archetype-based Entity Component System (ECS) for Rust.
//!
//! freecs provides a table-based storage system where entities with identical component sets
//! are stored together in contiguous memory (Structure of Arrays layout), optimizing for cache
//! coherency and SIMD operations.
//!
//! # Key Features
//!
//! - **Zero-cost Abstractions**: Fully statically dispatched, no custom traits
//! - **Parallel Processing**: Multi-threaded iteration using Rayon (automatically enabled on non-WASM platforms)
//! - **Sparse Set Tags**: Deterministic, generation-checked markers that don't fragment archetypes
//! - **Command Buffers**: Queue structural changes during iteration
//! - **Change Detection**: Track component modifications for incremental updates
//! - **Events**: Sequence-numbered channels with exactly-once cursor consumption
//! - **Structural Change Log**: Cursor-based log of spawns, despawns, component moves, and tag flips
//! - **Multi-World**: Split components across multiple worlds for >64 component types
//! - **Dynamic Worlds** (optional `dynamic` feature): register component types at
//!   runtime with bundle spawns and typed queries, same storage underneath
//!
//! The `ecs!` macro generates the entire ECS at compile time using only plain data structures, functions, and zero unsafe code.
//!
//! # Quick Start
//!
//! ```rust
//! use freecs::{ecs, Entity};
//!
//! // First, define components (must implement Default)
//! #[derive(Default, Clone, Debug)]
//! pub struct Position { pub x: f32, pub y: f32 }
//!
//! #[derive(Default, Clone, Debug)]
//! pub struct Velocity { pub x: f32, pub y: f32 }
//!
//! #[derive(Default, Clone, Debug)]
//! pub struct Health { pub value: f32 }
//!
//! // Then, create a world with the `ecs!` macro
//! ecs! {
//!   World {
//!     position: Position => POSITION,
//!     velocity: Velocity => VELOCITY,
//!     health: Health => HEALTH,
//!   }
//!   Tags {
//!     player => PLAYER,
//!     enemy => ENEMY,
//!   }
//!   Events {
//!     collision: CollisionEvent,
//!   }
//!   Resources {
//!     delta_time: f32
//!   }
//! }
//!
//! #[derive(Debug, Clone)]
//! pub struct CollisionEvent {
//!     pub entity_a: Entity,
//!     pub entity_b: Entity,
//! }
//! ```
//!
//! ## Entity and Component Access
//!
//! ```rust
//! # use freecs::ecs;
//! # #[derive(Default, Clone)] pub struct Position { x: f32, y: f32 }
//! # #[derive(Default, Clone)] pub struct Velocity { x: f32, y: f32 }
//! # #[derive(Default, Clone)] pub struct Health { value: f32 }
//! # ecs! { World { position: Position => POSITION, velocity: Velocity => VELOCITY, health: Health => HEALTH, } Resources { delta_time: f32 } }
//! let mut world = World::default();
//!
//! // Spawn entities with components by mask
//! let entity = world.spawn_entities(POSITION | VELOCITY, 1)[0];
//!
//! // Lookup and modify a component using generated methods
//! if let Some(pos) = world.get_position_mut(entity) {
//!     pos.x += 1.0;
//! }
//!
//! // Read components
//! if let Some(pos) = world.get_position(entity) {
//!     println!("Position: ({}, {})", pos.x, pos.y);
//! }
//!
//! // Set components (adds if not present)
//! world.set_position(entity, Position { x: 10.0, y: 20.0 });
//! world.set_velocity(entity, Velocity { x: 1.0, y: 0.0 });
//!
//! // Add new components to an entity by mask
//! world.add_components(entity, HEALTH | VELOCITY);
//!
//! // Or use the generated add methods
//! world.add_health(entity);
//!
//! // Remove components from an entity by mask
//! world.remove_components(entity, VELOCITY | POSITION);
//!
//! // Or use the generated remove methods
//! world.remove_velocity(entity);
//!
//! // Check if entity has components
//! if world.entity_has_position(entity) {
//!     println!("Entity has position component");
//! }
//!
//! // Query all entities
//! let entities = world.get_all_entities();
//! println!("All entities: {entities:?}");
//!
//! // Query entities, iterating over all entities matching the component mask
//! let entities = world.query_entities(POSITION | VELOCITY);
//!
//! // Query for the first entity matching the component mask, returning early when found
//! let player = world.query_first_entity(POSITION | VELOCITY);
//! ```
//!
//! ## Tags
//!
//! Tags are lightweight markers that don't cause archetype fragmentation:
//!
//! ```rust
//! # use freecs::{ecs, Entity};
//! # #[derive(Default, Clone)] pub struct Position { x: f32, y: f32 }
//! # ecs! { World { position: Position => POSITION, } Tags { player => PLAYER, enemy => ENEMY, } Resources { delta_time: f32 } }
//! # let mut world = World::default();
//! # let entity = world.spawn_entities(POSITION, 1)[0];
//! // Add tags to entities
//! world.add_player(entity);
//!
//! // Check if entity has a tag
//! if world.has_player(entity) {
//!     println!("Entity is a player");
//! }
//!
//! // Query entities by tag
//! for entity in world.query_player() {
//!     println!("Player: {:?}", entity);
//! }
//!
//! // Remove tags
//! world.remove_player(entity);
//! ```
//!
//! ## Events
//!
//! Events provide type-safe communication between systems:
//!
//! ```rust
//! # use freecs::{ecs, Entity};
//! # #[derive(Default, Clone)] pub struct Position { x: f32, y: f32 }
//! # #[derive(Debug, Clone)] pub struct CollisionEvent { entity_a: Entity, entity_b: Entity }
//! # ecs! { World { position: Position => POSITION, } Events { collision: CollisionEvent, } Resources { delta_time: f32 } }
//! # let mut world = World::default();
//! # let entity = world.spawn_entities(POSITION, 1)[0];
//! // Send events
//! world.send_collision(CollisionEvent {
//!     entity_a: entity,
//!     entity_b: entity,
//! });
//!
//! // Process events in systems
//! for event in world.collect_collision() {
//!     println!("Collision: {:?} and {:?}", event.entity_a, event.entity_b);
//! }
//!
//! // Clean up events and increment tick at end of frame
//! world.step();
//! ```
//!
//! ## Systems
//!
//! Systems are functions that query entities and transform their components.
//! For maximum performance, use the query builder API for direct table access:
//!
//! ```rust
//! # use freecs::ecs;
//! # #[derive(Default, Clone)] pub struct Position { x: f32, y: f32 }
//! # #[derive(Default, Clone)] pub struct Velocity { x: f32, y: f32 }
//! # ecs! { World { position: Position => POSITION, velocity: Velocity => VELOCITY, } Resources { delta_time: f32 } }
//! fn physics_system(world: &mut World) {
//!     let dt = world.resources.delta_time;
//!
//!     // Method 1: High-performance query builder (recommended)
//!     world.query_mut()
//!         .with(POSITION | VELOCITY)
//!         .iter(|entity, table, idx| {
//!             table.position[idx].x += table.velocity[idx].x * dt;
//!             table.position[idx].y += table.velocity[idx].y * dt;
//!         });
//!
//!     // Method 2: Per-entity lookups (simpler but slower)
//!     let entities: Vec<_> = world.query_entities(POSITION | VELOCITY).collect();
//!     for entity in entities {
//!         if let Some(velocity) = world.get_velocity(entity).cloned() {
//!             if let Some(position) = world.get_position_mut(entity) {
//!                 position.x += velocity.x * dt;
//!                 position.y += velocity.y * dt;
//!             }
//!         }
//!     }
//! }
//! ```
//!
//! ## Parallel Processing
//!
//! Process large entity counts across multiple CPU cores using Rayon. Parallel iteration is
//! automatically available on non-WASM platforms:
//!
//! ```rust
//! # use freecs::ecs;
//! # #[derive(Default, Clone)] pub struct Position { x: f32, y: f32 }
//! # #[derive(Default, Clone)] pub struct Velocity { x: f32, y: f32 }
//! # ecs! { World { position: Position => POSITION, velocity: Velocity => VELOCITY, } Resources { delta_time: f32 } }
//! use freecs::rayon::prelude::*;
//!
//! fn parallel_physics(world: &mut World) {
//!     let dt = world.resources.delta_time;
//!
//!     world.par_for_each_mut(POSITION | VELOCITY, 0, |entity, table, idx| {
//!         table.position[idx].x += table.velocity[idx].x * dt;
//!         table.position[idx].y += table.velocity[idx].y * dt;
//!     });
//! }
//! ```
//!
//! Parallel iteration is best suited for processing 100K+ entities with non-trivial
//! per-entity computation.
//!
//! ## Command Buffers
//!
//! Queue structural changes during iteration to avoid borrow conflicts:
//!
//! ```rust
//! # use freecs::{ecs, Entity};
//! # #[derive(Default, Clone)] pub struct Position { x: f32, y: f32 }
//! # #[derive(Default, Clone)] pub struct Health { value: f32 }
//! # ecs! { World { position: Position => POSITION, health: Health => HEALTH, } Resources { delta_time: f32 } }
//! # let mut world = World::default();
//! # world.spawn_entities(POSITION | HEALTH, 10);
//! // Queue despawns during iteration
//! let entities_to_despawn: Vec<Entity> = world
//!     .query_entities(HEALTH)
//!     .filter(|&entity| {
//!         world.get_health(entity).map_or(false, |h| h.value <= 0.0)
//!     })
//!     .collect();
//!
//! for entity in entities_to_despawn {
//!     world.queue_despawn_entity(entity);
//! }
//!
//! // Apply all queued commands at once
//! world.apply_commands();
//! ```
//!
//! ## Change Detection
//!
//! Track which components have been modified since the last frame:
//!
//! ```rust
//! # use freecs::{ecs, Entity};
//! # #[derive(Default, Clone)] pub struct Position { x: f32, y: f32 }
//! # ecs! { World { position: Position => POSITION, } Resources { delta_time: f32 } }
//! # let mut world = World::default();
//! // Process only entities with changed components
//! world.for_each_mut_changed(POSITION, 0, |entity, table, idx| {
//!     // Only processes entities where position changed since last step()
//! });
//!
//! // Automatically increments tick counter
//! world.step();
//! ```
//!
//! Writes through `set_*`, `get_*_mut`, and `modify_*` mark the slot as
//! changed, as do spawns and component add/remove migrations. Raw table
//! access does not mark. Changed queries skip whole tables that no write
//! has touched since the last `step()`, using a per-table high-water tick
//! per component.
//!
//! ## System Scheduling
//!
//! Organize systems into a schedule:
//!
//! ```rust
//! # use freecs::{ecs, Schedule, Entity};
//! # #[derive(Default, Clone)] pub struct Position { x: f32, y: f32 }
//! # ecs! { World { position: Position => POSITION, } Resources { delta_time: f32 } }
//! # fn input_system(world: &mut World) {}
//! # fn physics_system(world: &mut World) {}
//! # fn render_system(world: &World) {}
//! let mut world = World::default();
//! let mut schedule = Schedule::new();
//!
//! schedule
//!     .push("input", input_system)
//!     .push("physics", physics_system)
//!     .push_readonly("render", render_system);
//!
//! // Game loop
//! loop {
//!     schedule.run(&mut world);
//!     world.step();
//! #   break;
//! }
//! ```
//!
//! ## Entity Builder
//!
//! ```rust
//! # use freecs::ecs;
//! # #[derive(Default, Clone)] pub struct Position { x: f32, y: f32 }
//! # #[derive(Default, Clone)] pub struct Velocity { x: f32, y: f32 }
//! # ecs! { World { position: Position => POSITION, velocity: Velocity => VELOCITY, } Resources { delta_time: f32 } }
//! let mut world = World::default();
//! let entities = EntityBuilder::new()
//!     .with_position(Position { x: 1.0, y: 2.0 })
//!     .with_velocity(Velocity { x: 0.0, y: 1.0 })
//!     .spawn(&mut world, 2);
//!
//! // Access the spawned entities
//! let first_pos = world.get_position(entities[0]).unwrap();
//! assert_eq!(first_pos.x, 1.0);
//! ```
//!
//! # Advanced Features
//!
//! ## Batch Spawning
//!
//! ```rust
//! # use freecs::{ecs, Entity};
//! # #[derive(Default, Clone)] pub struct Position { x: f32, y: f32 }
//! # #[derive(Default, Clone)] pub struct Velocity { x: f32, y: f32 }
//! # ecs! { World { position: Position => POSITION, velocity: Velocity => VELOCITY, } Resources { delta_time: f32 } }
//! # let mut world = World::default();
//! // Spawn with initialization callback
//! let entities = world.spawn_batch(POSITION | VELOCITY, 1000, |table, idx| {
//!     table.position[idx] = Position { x: idx as f32, y: 0.0 };
//!     table.velocity[idx] = Velocity { x: 1.0, y: 0.0 };
//! });
//! ```
//!
//! ## Per-Component Iteration
//!
//! ```rust
//! # use freecs::{ecs, Entity};
//! # #[derive(Default, Clone)] pub struct Position { x: f32, y: f32 }
//! # ecs! { World { position: Position => POSITION, } Resources { delta_time: f32 } }
//! # let mut world = World::default();
//! // Iterate over single component
//! world.iter_position_mut(|_entity, position| {
//!     position.x += 1.0;
//! });
//!
//! // Slice-based iteration (most efficient)
//! for slice in world.iter_position_slices_mut() {
//!     for position in slice {
//!         position.x *= 2.0;
//!     }
//! }
//! ```
//!
//! ## Low-Level Iteration
//!
//! ```rust
//! # use freecs::{ecs, Entity};
//! # #[derive(Default, Clone)] pub struct Position { x: f32, y: f32 }
//! # #[derive(Default, Clone)] pub struct Velocity { x: f32, y: f32 }
//! # ecs! { World { position: Position => POSITION, velocity: Velocity => VELOCITY, } Tags { player => PLAYER, } Resources { delta_time: f32 } }
//! # let mut world = World::default();
//! // Include/exclude with masks
//! world.for_each_mut(POSITION | VELOCITY, PLAYER, |entity, table, idx| {
//!     // Process non-player entities
//!     table.position[idx].x += table.velocity[idx].x;
//! });
//! ```
//!
//! ## Advanced Command Operations
//!
//! ```rust
//! # use freecs::{ecs, Entity};
//! # #[derive(Default, Clone)] pub struct Position { x: f32, y: f32 }
//! # ecs! { World { position: Position => POSITION, } Tags { player => PLAYER, } Resources { delta_time: f32 } }
//! # let mut world = World::default();
//! # let entity = world.spawn_entities(POSITION, 1)[0];
//! // Queue batch operations
//! world.queue_spawn_entities(POSITION, 100);
//! world.queue_set_position(entity, Position { x: 10.0, y: 20.0 });
//! world.queue_add_player(entity);
//!
//! // Check command buffer status
//! if world.command_count() > 100 {
//!     world.apply_commands();
//! }
//!
//! // Clear without applying
//! world.clear_commands();
//! ```
//!
//! ## Event Management
//!
//! ```rust
//! # use freecs::{ecs, Entity};
//! # #[derive(Default, Clone)] pub struct Position { x: f32, y: f32 }
//! # #[derive(Debug, Clone)] pub struct CollisionEvent { entity_a: Entity, entity_b: Entity }
//! # ecs! { World { position: Position => POSITION, } Events { collision: CollisionEvent, } Resources { delta_time: f32 } }
//! # let mut world = World::default();
//! # let entity = world.spawn_entities(POSITION, 1)[0];
//! # world.send_collision(CollisionEvent { entity_a: entity, entity_b: entity });
//! // Peek at events without consuming
//! if let Some(event) = world.peek_collision() {
//!     println!("Next collision: {:?}", event.entity_a);
//! }
//!
//! // Check event count
//! if !world.is_empty_collision() {
//!     let count = world.len_collision();
//!     println!("Processing {} events", count);
//! }
//!
//! // Cursor-based, exactly-once consumption: record where you left off,
//! // read everything newer, then advance your cursor.
//! let mut cursor = 0;
//! for event in world.read_collision_since(cursor) {
//!     // Process event
//! }
//! cursor = world.sequence_collision();
//! assert!(world.read_collision_since(cursor).is_empty());
//! ```
//!
//! ## Conditional Compilation
//!
//! Both components and resources support `#[cfg(...)]` attributes for conditional compilation.
//! This is useful for debug-only components, optional features, or platform-specific functionality:
//!
//! ```rust,ignore
//! ecs! {
//!     World {
//!         position: Position => POSITION,
//!         velocity: Velocity => VELOCITY,
//!         #[cfg(debug_assertions)]
//!         debug_info: DebugInfo => DEBUG_INFO,
//!         #[cfg(feature = "physics")]
//!         rigid_body: RigidBody => RIGID_BODY,
//!     }
//!     Resources {
//!         delta_time: f32,
//!         #[cfg(feature = "audio")]
//!         audio_engine: AudioEngine,
//!     }
//! }
//! ```
//!
//! When a component or resource has a `#[cfg(...)]` attribute, all related generated code
//! (struct fields, accessor methods, mask constants, etc.) is conditionally compiled.

pub use paste;

#[cfg(feature = "dynamic")]
pub mod dynamic;

/// Declares a dynamic world's schema in one place: the mask constants (bits
/// assigned in declaration order, which is the registration order and
/// therefore the snapshot schema) and the registration function that builds
/// a [`dynamic::ComponentRegistry`] in that exact order, asserting each
/// key's mask against its constant. Declare every component on every build
/// configuration, and only ever append, so masks stay identical across
/// feature sets and saves stay loadable.
///
/// The leading field name documents intent and keeps the shape drop-in
/// compatible with `ecs!` component blocks; only the type and constant are
/// used. Prefix the function with `serde` to register every component with
/// a snapshot codec (requires the `snapshot` feature and serde derives on
/// the components).
///
/// ```rust
/// #[derive(Default, Clone, Debug)]
/// struct Position { x: f32, y: f32 }
///
/// #[derive(Default, Clone, Debug)]
/// struct Velocity { x: f32, y: f32 }
///
/// freecs::dynamic_schema! {
///     pub fn register_components {
///         position: Position => POSITION,
///         velocity: Velocity => VELOCITY,
///     }
/// }
///
/// let world = freecs::dynamic::DynWorld::from_registry(register_components());
/// assert_eq!(POSITION, 1);
/// assert_eq!(VELOCITY, 2);
/// assert_eq!(world.remaining_bits(), 62);
/// ```
#[cfg(feature = "dynamic")]
#[macro_export]
macro_rules! dynamic_schema {
    (@consts $bit:expr;) => {};
    (@consts $bit:expr; $const:ident $(, $rest:ident)*) => {
        pub const $const: u64 = $bit;
        $crate::dynamic_schema!(@consts $bit << 1; $($rest),*);
    };
    (
        $vis:vis fn $register_fn:ident {
            $($field:ident: $ty:ty => $const:ident,)+
        }
    ) => {
        $crate::dynamic_schema!(@consts 1u64; $($const),+);

        $vis fn $register_fn() -> $crate::dynamic::ComponentRegistry {
            let mut registry = $crate::dynamic::ComponentRegistry::new();
            $(
                let key = registry.register::<$ty>();
                assert_eq!(
                    key.mask,
                    $const,
                    "schema declaration order must match registration order"
                );
            )+
            registry
        }
    };
    (
        serde $vis:vis fn $register_fn:ident {
            $($field:ident: $ty:ty => $const:ident,)+
        }
    ) => {
        $crate::dynamic_schema!(@consts 1u64; $($const),+);

        $vis fn $register_fn() -> $crate::dynamic::ComponentRegistry {
            let mut registry = $crate::dynamic::ComponentRegistry::new();
            $(
                let key = registry.register_serde::<$ty>();
                assert_eq!(
                    key.mask,
                    $const,
                    "schema declaration order must match registration order"
                );
            )+
            registry
        }
    };
}

/// Generates the macro-world accessor ergonomics over the keyed tier: a
/// keys struct holding one [`dynamic::ComponentKey`] or
/// [`dynamic::TagKey`] per entry, a `resolve` constructor registering them
/// in declaration order, and named methods on your wrapper type —
/// `get_<name>` / `get_<name>_mut` / `set_<name>` / `remove_<name>` /
/// `has_<name>` per component, `add_<name>` / `remove_<name>` /
/// `has_<name>` / `query_<name>` per tag. The wrapper must expose the named
/// world and keys fields. Accessors run at keyed speed and stamp change
/// ticks exactly like the generated macro world's.
///
/// ```rust
/// use freecs::dynamic::DynWorld;
///
/// #[derive(Default, Clone, Debug)]
/// struct Position { x: f32 }
///
/// struct Boss;
///
/// struct Game {
///     world: DynWorld,
///     keys: GameKeys,
/// }
///
/// freecs::dynamic_accessors! {
///     pub struct GameKeys for Game { world, keys }
///     components {
///         position: Position,
///     }
///     tags {
///         boss: Boss,
///     }
/// }
///
/// let mut world = DynWorld::new();
/// let keys = GameKeys::resolve(&mut world);
/// let mut game = Game { world, keys };
///
/// let entity = game.world.spawn((Position { x: 1.0 },));
/// game.set_position(entity, Position { x: 2.0 });
/// game.add_boss(entity);
/// assert_eq!(game.get_position(entity).unwrap().x, 2.0);
/// assert!(game.has_boss(entity));
/// assert_eq!(game.query_boss().count(), 1);
/// ```
#[cfg(feature = "dynamic")]
#[macro_export]
macro_rules! dynamic_accessors {
    (
        $vis:vis struct $keys:ident for $wrapper:ident { $world_field:ident, $keys_field:ident }
        components {
            $($component:ident: $component_type:ty,)*
        }
        tags {
            $($tag:ident: $tag_type:ty,)*
        }
    ) => {
        $vis struct $keys {
            $(pub $component: $crate::dynamic::ComponentKey<$component_type>,)*
            $(pub $tag: $crate::dynamic::TagKey,)*
        }

        impl $keys {
            $vis fn resolve(world: &mut $crate::dynamic::DynWorld) -> Self {
                Self {
                    $($component: world.register::<$component_type>(),)*
                    $($tag: world.tag_key::<$tag_type>(),)*
                }
            }
        }

        $crate::paste::paste! {
            impl $wrapper {
                $(
                    $vis fn [<get_ $component>](&self, entity: $crate::Entity) -> Option<&$component_type> {
                        self.$world_field.get_keyed(self.$keys_field.$component, entity)
                    }

                    $vis fn [<get_ $component _mut>](&mut self, entity: $crate::Entity) -> Option<&mut $component_type> {
                        self.$world_field.get_mut_keyed(self.$keys_field.$component, entity)
                    }

                    $vis fn [<set_ $component>](&mut self, entity: $crate::Entity, value: $component_type) {
                        self.$world_field.set_keyed(self.$keys_field.$component, entity, value);
                    }

                    $vis fn [<remove_ $component>](&mut self, entity: $crate::Entity) -> bool {
                        self.$world_field.remove_components(entity, self.$keys_field.$component.mask)
                    }

                    $vis fn [<has_ $component>](&self, entity: $crate::Entity) -> bool {
                        self.$world_field.entity_has_components(entity, self.$keys_field.$component.mask)
                    }
                )*

                $(
                    $vis fn [<add_ $tag>](&mut self, entity: $crate::Entity) {
                        self.$world_field.add_tag(self.$keys_field.$tag, entity);
                    }

                    $vis fn [<remove_ $tag>](&mut self, entity: $crate::Entity) -> bool {
                        self.$world_field.remove_tag(self.$keys_field.$tag, entity)
                    }

                    $vis fn [<has_ $tag>](&self, entity: $crate::Entity) -> bool {
                        self.$world_field.has_tag(self.$keys_field.$tag, entity)
                    }

                    $vis fn [<query_ $tag>](&self) -> impl Iterator<Item = $crate::Entity> + '_ {
                        self.$world_field.query_tag(self.$keys_field.$tag)
                    }
                )*
            }
        }
    };
}

#[cfg(not(target_family = "wasm"))]
pub use rayon;

#[derive(Default, Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Entity {
    pub id: u32,
    pub generation: u32,
}

impl std::fmt::Display for Entity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self { id, generation } = self;
        write!(f, "Id: {id} - Generation: {generation}")
    }
}

/// Liveness record for one entity id: the generation currently associated
/// with the id and whether that handle is live.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EntitySlot {
    pub generation: u32,
    pub alive: bool,
}

/// Allocates generational entity handles and tracks which handles are live.
///
/// Liveness is authoritative here: `deallocate` refuses stale or already-freed
/// handles, so an id can never enter the free list twice and two live entities
/// can never share an id and generation.
#[derive(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EntityAllocator {
    pub next_id: u32,
    pub free_ids: Vec<(u32, u32)>,
    pub slots: Vec<EntitySlot>,
}

impl EntityAllocator {
    #[inline]
    pub fn allocate(&mut self) -> Entity {
        let entity = if let Some((id, next_generation)) = self.free_ids.pop() {
            Entity {
                id,
                generation: next_generation,
            }
        } else {
            let id = self.next_id;
            self.next_id += 1;
            Entity { id, generation: 0 }
        };
        let index = entity.id as usize;
        if index >= self.slots.len() {
            self.slots.resize(index + 1, EntitySlot::default());
        }
        self.slots[index] = EntitySlot {
            generation: entity.generation,
            alive: true,
        };
        entity
    }

    /// Allocates `count` handles into `entities`, recycling freed ids first.
    /// Fresh ids are contiguous, so their liveness slots are written with one
    /// bulk fill instead of a store per entity.
    pub fn allocate_batch(&mut self, count: usize, entities: &mut Vec<Entity>) {
        entities.reserve(count);
        let recycled = count.min(self.free_ids.len());
        for _ in 0..recycled {
            let Some((id, generation)) = self.free_ids.pop() else {
                break;
            };
            let index = id as usize;
            if index >= self.slots.len() {
                self.slots.resize(index + 1, EntitySlot::default());
            }
            self.slots[index] = EntitySlot {
                generation,
                alive: true,
            };
            entities.push(Entity { id, generation });
        }

        let fresh = count - recycled;
        if fresh > 0 {
            let start = self.next_id;
            self.next_id += fresh as u32;
            let start_index = start as usize;
            let end_index = start_index + fresh;
            if end_index > self.slots.len() {
                self.slots.resize(end_index, EntitySlot::default());
            }
            self.slots[start_index..end_index].fill(EntitySlot {
                generation: 0,
                alive: true,
            });
            for id in start..self.next_id {
                entities.push(Entity { id, generation: 0 });
            }
        }
    }

    /// Frees the handle if it is currently live. Returns false for stale
    /// generations and double frees, leaving the allocator untouched.
    #[inline]
    pub fn deallocate(&mut self, entity: Entity) -> bool {
        if !self.is_alive(entity) {
            return false;
        }
        self.slots[entity.id as usize].alive = false;
        self.free_ids
            .push((entity.id, entity.generation.wrapping_add(1)));
        true
    }

    #[inline]
    pub fn is_alive(&self, entity: Entity) -> bool {
        self.slots
            .get(entity.id as usize)
            .is_some_and(|slot| slot.alive && slot.generation == entity.generation)
    }
}

#[derive(Copy, Clone, Default)]
pub struct EntityLocation {
    pub generation: u32,
    pub table_index: u32,
    pub array_index: u32,
    pub allocated: bool,
}

#[derive(Default)]
pub struct EntityLocations {
    pub locations: Vec<EntityLocation>,
}

impl EntityLocations {
    pub fn get(&self, id: u32) -> Option<&EntityLocation> {
        self.locations.get(id as usize)
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut EntityLocation> {
        self.locations.get_mut(id as usize)
    }

    pub fn ensure_slot(&mut self, id: u32, generation: u32) {
        let id_usize = id as usize;
        if id_usize >= self.locations.len() {
            self.locations.resize(
                (self.locations.len() * 2).max(64).max(id_usize + 1),
                EntityLocation::default(),
            );
        }
        self.locations[id_usize].generation = generation;
    }

    pub fn insert(&mut self, id: u32, location: EntityLocation) {
        let id_usize = id as usize;
        if id_usize >= self.locations.len() {
            self.locations.resize(
                (self.locations.len() * 2).max(64).max(id_usize + 1),
                EntityLocation::default(),
            );
        }
        self.locations[id_usize] = location;
    }

    pub fn mark_deallocated(&mut self, id: u32) {
        if let Some(loc) = self.locations.get_mut(id as usize) {
            loc.allocated = false;
        }
    }
}

/// Returns true if `tick` was stamped after `since_tick`, treating ticks as a
/// wrapping sequence so detection keeps working after `u32` overflow.
#[inline]
pub const fn tick_is_newer(tick: u32, since_tick: u32) -> bool {
    tick.wrapping_sub(since_tick) as i32 > 0
}

const SPARSE_TAG_ABSENT: u32 = u32::MAX;

/// A sparse set of entities backing one tag.
///
/// Dense storage gives contiguous, deterministic iteration; the sparse index
/// gives O(1) insert, remove, and contains without hashing. Membership is
/// generation-checked, so a stale handle never matches a reused id.
///
/// # Examples
///
/// ```
/// use freecs::{Entity, SparseTagSet};
///
/// let mut set = SparseTagSet::default();
/// let entity = Entity { id: 3, generation: 0 };
///
/// assert!(set.insert(entity));
/// assert!(set.contains(entity));
/// assert_eq!(set.iter().collect::<Vec<_>>(), vec![entity]);
///
/// let stale = Entity { id: 3, generation: 1 };
/// assert!(!set.contains(stale));
///
/// assert!(set.remove(entity));
/// assert!(set.is_empty());
/// ```
#[derive(Default, Clone, Debug)]
pub struct SparseTagSet {
    pub dense: Vec<Entity>,
    pub sparse: Vec<u32>,
}

impl SparseTagSet {
    /// Adds the entity, replacing any stale entry for the same id. Returns
    /// false if this exact handle was already present.
    #[inline]
    pub fn insert(&mut self, entity: Entity) -> bool {
        let index = entity.id as usize;
        if index >= self.sparse.len() {
            self.sparse.resize(index + 1, SPARSE_TAG_ABSENT);
        }
        let slot = self.sparse[index];
        if slot != SPARSE_TAG_ABSENT {
            let existing = &mut self.dense[slot as usize];
            if *existing == entity {
                return false;
            }
            *existing = entity;
            return true;
        }
        self.sparse[index] = self.dense.len() as u32;
        self.dense.push(entity);
        true
    }

    #[inline]
    pub fn remove(&mut self, entity: Entity) -> bool {
        let index = entity.id as usize;
        let Some(&slot) = self.sparse.get(index) else {
            return false;
        };
        if slot == SPARSE_TAG_ABSENT || self.dense[slot as usize] != entity {
            return false;
        }
        self.dense.swap_remove(slot as usize);
        self.sparse[index] = SPARSE_TAG_ABSENT;
        if (slot as usize) < self.dense.len() {
            let moved = self.dense[slot as usize];
            self.sparse[moved.id as usize] = slot;
        }
        true
    }

    #[inline]
    pub fn contains(&self, entity: Entity) -> bool {
        self.sparse
            .get(entity.id as usize)
            .is_some_and(|&slot| slot != SPARSE_TAG_ABSENT && self.dense[slot as usize] == entity)
    }

    pub fn iter(&self) -> impl Iterator<Item = Entity> + '_ {
        self.dense.iter().copied()
    }

    pub fn len(&self) -> usize {
        self.dense.len()
    }

    pub fn is_empty(&self) -> bool {
        self.dense.is_empty()
    }

    pub fn clear(&mut self) {
        self.dense.clear();
        self.sparse.fill(SPARSE_TAG_ABSENT);
    }
}

/// Archetype graph edges for one table: which table an entity lands in when a
/// single component bit is added or removed, plus memoized targets for
/// multi-bit changes. Shared by the macro-generated worlds and the dynamic
/// world, since none of it depends on component types.
#[derive(Clone, Default)]
pub struct ArchetypeEdges {
    pub add_edges: Vec<Option<usize>>,
    pub remove_edges: Vec<Option<usize>>,
    pub multi_add_cache: std::collections::HashMap<u64, usize>,
    pub multi_remove_cache: std::collections::HashMap<u64, usize>,
}

impl ArchetypeEdges {
    pub fn new(component_count: usize) -> Self {
        Self {
            add_edges: vec![None; component_count],
            remove_edges: vec![None; component_count],
            multi_add_cache: std::collections::HashMap::default(),
            multi_remove_cache: std::collections::HashMap::default(),
        }
    }
}

/// Registers a newly pushed table with the archetype routing structures:
/// inserts the mask lookup, appends the table's edge record, extends every
/// query-cache entry the new table satisfies, and wires single-component
/// edges from existing tables toward the new one. `table_masks` must iterate
/// every table including the new one, in index order.
pub struct ArchetypeRouting<'world> {
    pub table_lookup: &'world mut std::collections::HashMap<u64, usize>,
    pub table_edges: &'world mut Vec<ArchetypeEdges>,
    pub query_cache: &'world mut std::collections::HashMap<u64, Vec<usize>>,
}

pub fn archetype_register_table<M>(
    routing: ArchetypeRouting<'_>,
    component_count: usize,
    mask: u64,
    table_index: usize,
    table_masks: M,
    component_bits: impl Iterator<Item = (u64, usize)>,
) where
    M: Iterator<Item = u64> + Clone,
{
    routing
        .table_edges
        .push(ArchetypeEdges::new(component_count));
    routing.table_lookup.insert(mask, table_index);

    for (query_mask, cached_tables) in routing.query_cache.iter_mut() {
        if mask & *query_mask == *query_mask {
            cached_tables.push(table_index);
        }
    }

    for (component_mask, component_index) in component_bits {
        for (index, existing_mask) in table_masks.clone().enumerate() {
            if existing_mask | component_mask == mask {
                routing.table_edges[index].add_edges[component_index] = Some(table_index);
            }
            if existing_mask & !component_mask == mask {
                routing.table_edges[index].remove_edges[component_index] = Some(table_index);
            }
        }
    }
}

/// Returns the memoized list of table indices whose masks contain `mask`,
/// computing and caching it on first use. Taking the cache and the table
/// masks as separate parameters keeps the borrows disjoint, so callers can
/// mutate tables while holding the returned slice.
pub fn archetype_cached_tables<M>(
    query_cache: &mut std::collections::HashMap<u64, Vec<usize>>,
    table_masks: M,
    mask: u64,
) -> &[usize]
where
    M: Iterator<Item = u64>,
{
    query_cache.entry(mask).or_insert_with(|| {
        table_masks
            .enumerate()
            .filter(|(_, table_mask)| table_mask & mask == mask)
            .map(|(table_index, _)| table_index)
            .collect()
    })
}

/// Backstop for worlds whose structural log is never consumed. When the log
/// reaches this length it is cleared wholesale, so a world with no consumer
/// stays bounded instead of leaking. Consumers that drain every frame never
/// come near it.
pub const STRUCTURAL_LOG_CAPACITY: usize = 262_144;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuralChangeKind {
    Spawned,
    Despawned,
    ComponentsAdded,
    ComponentsRemoved,
    TagsAdded,
    TagsRemoved,
}

/// One structural mutation recorded by the world: a spawn, a despawn, or a
/// component add or remove. `mask` holds the components involved: the full
/// mask for spawns and despawns, the delta for adds and removes. Consumers
/// track their own `sequence` cursor via `structural_changes_since` and the
/// owner trims consumed entries with `trim_structural_log`.
#[derive(Debug, Clone, Copy)]
pub struct StructuralChange {
    pub sequence: u64,
    pub entity: Entity,
    pub kind: StructuralChangeKind,
    pub mask: u64,
}

/// Backstop for event channels whose events are never consumed or expired.
/// When the buffer reaches this length the oldest events are dropped, so a
/// channel with no consumer stays bounded instead of leaking.
pub const EVENT_CHANNEL_CAPACITY: usize = 262_144;

/// A sequence-numbered event channel for inter-system communication.
///
/// Events live in one flat `Vec` and every event gets a monotonically
/// increasing sequence number, the same cursor scheme the structural log
/// uses. Two consumption styles are supported:
///
/// - **Frame-scoped**: [`read()`](EventChannel::read) sees every buffered
///   event, and [`update()`](EventChannel::update) once per frame expires
///   events after they have been visible for two frames, matching the old
///   double-buffer lifetime.
/// - **Cursor-based, exactly-once**: each consumer records
///   [`sequence()`](EventChannel::sequence) after reading
///   [`events_since(cursor)`](EventChannel::events_since). Multiple consumers
///   each see every event exactly once, and a consumer that skips a frame
///   catches up instead of double-processing or missing events.
///
/// # Examples
///
/// ```
/// use freecs::EventChannel;
///
/// #[derive(Debug, Clone)]
/// struct DamageEvent { amount: i32 }
///
/// let mut channel = EventChannel::new();
///
/// channel.send(DamageEvent { amount: 10 });
/// assert_eq!(channel.len(), 1);
///
/// let mut cursor = 0;
/// let seen = channel.events_since(cursor);
/// assert_eq!(seen.len(), 1);
/// cursor = channel.sequence();
///
/// assert!(channel.events_since(cursor).is_empty(), "cursor consumers see each event once");
///
/// channel.update();
/// assert_eq!(channel.len(), 1, "event persists after first update");
///
/// channel.update();
/// assert_eq!(channel.len(), 0, "event expired after second update");
/// ```
#[derive(Clone)]
pub struct EventChannel<T> {
    pub events: Vec<T>,
    pub base_sequence: u64,
    pub previous_update_sequence: u64,
}

impl<T> Default for EventChannel<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> EventChannel<T> {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            base_sequence: 0,
            previous_update_sequence: 0,
        }
    }

    /// Sends an event. It stays readable until it expires two `update()`
    /// calls later or a cursor consumer trims past it.
    #[inline]
    pub fn send(&mut self, event: T) {
        if self.events.len() >= EVENT_CHANNEL_CAPACITY {
            self.drop_oldest_half();
        }
        self.events.push(event);
    }

    #[cold]
    fn drop_oldest_half(&mut self) {
        let half = self.events.len() / 2;
        self.events.drain(..half);
        self.base_sequence += half as u64;
    }

    /// The sequence number of the most recently sent event. Record this as
    /// your cursor after consuming `events_since`.
    #[inline]
    pub fn sequence(&self) -> u64 {
        self.base_sequence + self.events.len() as u64
    }

    /// All buffered events sent after `cursor`, oldest first. A cursor older
    /// than the buffer yields everything still buffered.
    #[inline]
    pub fn events_since(&self, cursor: u64) -> &[T] {
        let start = cursor
            .saturating_sub(self.base_sequence)
            .min(self.events.len() as u64) as usize;
        &self.events[start..]
    }

    /// The exactly-once read: yields every event sent after `cursor` and
    /// advances the cursor past them, so calling this every frame delivers
    /// each event to this consumer exactly once. Each consumer owns one
    /// `u64` cursor; the buffer itself is untouched, so other consumers and
    /// the two-frame expiry are unaffected. This is the spelling to reach
    /// for by default; `read()` re-reads the whole buffer every call.
    #[inline]
    pub fn consume(&self, cursor: &mut u64) -> &[T] {
        let events = self.events_since(*cursor);
        *cursor = self.sequence();
        events
    }

    /// Returns an iterator over every buffered event, oldest first.
    pub fn read(&self) -> impl Iterator<Item = &T> {
        self.events.iter()
    }

    /// Returns a reference to the oldest buffered event, if any.
    pub fn peek(&self) -> Option<&T> {
        self.events.first()
    }

    /// Drops all events up to and including `up_to_sequence`. Call with the
    /// minimum cursor across consumers to reclaim memory early.
    pub fn trim(&mut self, up_to_sequence: u64) {
        let drop_count = up_to_sequence
            .saturating_sub(self.base_sequence)
            .min(self.events.len() as u64) as usize;
        self.events.drain(..drop_count);
        self.base_sequence += drop_count as u64;
    }

    /// Expires events that have now been visible for two frames. Call once
    /// per frame; `step()` on a generated world does this for every channel.
    pub fn update(&mut self) {
        let expire = self.previous_update_sequence;
        self.trim(expire);
        self.previous_update_sequence = self.sequence();
    }

    /// Immediately drops every buffered event, advancing past them.
    pub fn clear(&mut self) {
        let sequence = self.sequence();
        self.trim(sequence);
    }

    /// Returns the number of buffered events.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Returns `true` if no events are buffered.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

struct ScheduleEntry<W> {
    name: &'static str,
    system: Box<dyn FnMut(&mut W) + Send>,
}

pub struct Schedule<W> {
    entries: Vec<ScheduleEntry<W>>,
}

impl<W> Schedule<W> {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn push<F>(&mut self, name: &'static str, system: F) -> &mut Self
    where
        F: FnMut(&mut W) + Send + 'static,
    {
        self.assert_unique(name);
        self.entries.push(ScheduleEntry {
            name,
            system: Box::new(system),
        });
        self
    }

    pub fn push_readonly<F>(&mut self, name: &'static str, mut system: F) -> &mut Self
    where
        F: FnMut(&W) + Send + 'static,
    {
        self.assert_unique(name);
        self.entries.push(ScheduleEntry {
            name,
            system: Box::new(move |world: &mut W| {
                system(&*world);
            }),
        });
        self
    }

    /// Adds a system that runs only when the condition holds. The condition
    /// reads the world at each pass; a false skips the system for that pass
    /// without removing it from the schedule.
    pub fn push_if<C, F>(&mut self, name: &'static str, condition: C, mut system: F) -> &mut Self
    where
        C: Fn(&W) -> bool + Send + 'static,
        F: FnMut(&mut W) + Send + 'static,
    {
        self.push(name, move |world: &mut W| {
            if condition(world) {
                system(world);
            }
        })
    }

    pub fn insert_before<F>(&mut self, target: &str, name: &'static str, system: F) -> &mut Self
    where
        F: FnMut(&mut W) + Send + 'static,
    {
        self.assert_unique(name);
        let index = self.index_of_or_panic(target, "insert_before");
        self.entries.insert(
            index,
            ScheduleEntry {
                name,
                system: Box::new(system),
            },
        );
        self
    }

    pub fn insert_after<F>(&mut self, target: &str, name: &'static str, system: F) -> &mut Self
    where
        F: FnMut(&mut W) + Send + 'static,
    {
        self.assert_unique(name);
        let index = self.index_of_or_panic(target, "insert_after");
        self.entries.insert(
            index + 1,
            ScheduleEntry {
                name,
                system: Box::new(system),
            },
        );
        self
    }

    pub fn replace<F>(&mut self, name: &str, system: F) -> &mut Self
    where
        F: FnMut(&mut W) + Send + 'static,
    {
        let index = self
            .index_of(name)
            .unwrap_or_else(|| panic!("Schedule::replace: system \"{name}\" not found"));
        self.entries[index].system = Box::new(system);
        self
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let len_before = self.entries.len();
        self.entries.retain(|entry| entry.name != name);
        self.entries.len() != len_before
    }

    pub fn contains(&self, name: &str) -> bool {
        self.entries.iter().any(|entry| entry.name == name)
    }

    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.entries.iter().map(|entry| entry.name)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn run(&mut self, world: &mut W) {
        for entry in &mut self.entries {
            (entry.system)(world);
        }
    }

    fn index_of(&self, name: &str) -> Option<usize> {
        self.entries.iter().position(|entry| entry.name == name)
    }

    fn assert_unique(&self, name: &str) {
        assert!(
            !self.contains(name),
            "Schedule: system \"{name}\" already exists"
        );
    }

    fn index_of_or_panic(&self, target: &str, method: &str) -> usize {
        self.index_of(target)
            .unwrap_or_else(|| panic!("Schedule::{method}: system \"{target}\" not found"))
    }
}

impl<W> Default for Schedule<W> {
    fn default() -> Self {
        Self::new()
    }
}

#[macro_export]
macro_rules! ecs {
    (
        $world:ident {
            $($(#[$comp_attr:meta])* $name:ident: $type:ty => $mask:ident),* $(,)?
        }
        Tags {
            $($tag_name:ident => $tag_mask:ident),* $(,)?
        }
        Events {
            $($event_name:ident: $event_type:ty),* $(,)?
        }
        $resources:ident {
            $($(#[$attr:meta])*  $resource_name:ident: $resource_type:ty),* $(,)?
        }
    ) => {
        $crate::ecs_impl! {
            $world {
                $($(#[$comp_attr])* $name: $type => $mask),*
            }
            Tags {
                $($tag_name => $tag_mask),*
            }
            Events {
                $($event_name: $event_type),*
            }
            $resources {
                $($(#[$attr])* $resource_name: $resource_type),*
            }
        }
    };

    (
        $world:ident {
            $($(#[$comp_attr:meta])* $name:ident: $type:ty => $mask:ident),* $(,)?
        }
        Tags {
            $($tag_name:ident => $tag_mask:ident),* $(,)?
        }
        $resources:ident {
            $($(#[$attr:meta])*  $resource_name:ident: $resource_type:ty),* $(,)?
        }
    ) => {
        $crate::ecs_impl! {
            $world {
                $($(#[$comp_attr])* $name: $type => $mask),*
            }
            Tags {
                $($tag_name => $tag_mask),*
            }
            Events {}
            $resources {
                $($(#[$attr])* $resource_name: $resource_type),*
            }
        }
    };

    (
        $world:ident {
            $($(#[$comp_attr:meta])* $name:ident: $type:ty => $mask:ident),* $(,)?
        }
        Events {
            $($event_name:ident: $event_type:ty),* $(,)?
        }
        $resources:ident {
            $($(#[$attr:meta])*  $resource_name:ident: $resource_type:ty),* $(,)?
        }
    ) => {
        $crate::ecs_impl! {
            $world {
                $($(#[$comp_attr])* $name: $type => $mask),*
            }
            Tags {}
            Events {
                $($event_name: $event_type),*
            }
            $resources {
                $($(#[$attr])* $resource_name: $resource_type),*
            }
        }
    };

    (
        $world:ident {
            $($(#[$comp_attr:meta])* $name:ident: $type:ty => $mask:ident),* $(,)?
        }
        $resources:ident {
            $($(#[$attr:meta])*  $resource_name:ident: $resource_type:ty),* $(,)?
        }
    ) => {
        $crate::ecs_impl! {
            $world {
                $($(#[$comp_attr])* $name: $type => $mask),*
            }
            Tags {}
            Events {}
            $resources {
                $($(#[$attr])* $resource_name: $resource_type),*
            }
        }
    };

    (
        $ecs:ident {
            $($world_name:ident {
                $($(#[$comp_attr:meta])* $name:ident: $type:ty => $mask:ident),* $(,)?
            })+
        }
        Tags {
            $($tag_name:ident => $tag_mask:ident),* $(,)?
        }
        Events {
            $($event_name:ident: $event_type:ty),* $(,)?
        }
        $resources:ident {
            $($(#[$attr:meta])* $resource_name:ident: $resource_type:ty),* $(,)?
        }
    ) => {
        $crate::ecs_multi_impl! {
            $ecs {
                $($world_name {
                    $($(#[$comp_attr])* $name: $type => $mask),*
                })+
            }
            Tags {
                $($tag_name => $tag_mask),*
            }
            Events {
                $($event_name: $event_type),*
            }
            $resources {
                $($(#[$attr])* $resource_name: $resource_type),*
            }
        }
    };

    (
        $ecs:ident {
            $($world_name:ident {
                $($(#[$comp_attr:meta])* $name:ident: $type:ty => $mask:ident),* $(,)?
            })+
        }
        Tags {
            $($tag_name:ident => $tag_mask:ident),* $(,)?
        }
        $resources:ident {
            $($(#[$attr:meta])* $resource_name:ident: $resource_type:ty),* $(,)?
        }
    ) => {
        $crate::ecs_multi_impl! {
            $ecs {
                $($world_name {
                    $($(#[$comp_attr])* $name: $type => $mask),*
                })+
            }
            Tags {
                $($tag_name => $tag_mask),*
            }
            Events {}
            $resources {
                $($(#[$attr])* $resource_name: $resource_type),*
            }
        }
    };

    (
        $ecs:ident {
            $($world_name:ident {
                $($(#[$comp_attr:meta])* $name:ident: $type:ty => $mask:ident),* $(,)?
            })+
        }
        Events {
            $($event_name:ident: $event_type:ty),* $(,)?
        }
        $resources:ident {
            $($(#[$attr:meta])* $resource_name:ident: $resource_type:ty),* $(,)?
        }
    ) => {
        $crate::ecs_multi_impl! {
            $ecs {
                $($world_name {
                    $($(#[$comp_attr])* $name: $type => $mask),*
                })+
            }
            Tags {}
            Events {
                $($event_name: $event_type),*
            }
            $resources {
                $($(#[$attr])* $resource_name: $resource_type),*
            }
        }
    };

    (
        $ecs:ident {
            $($world_name:ident {
                $($(#[$comp_attr:meta])* $name:ident: $type:ty => $mask:ident),* $(,)?
            })+
        }
        $resources:ident {
            $($(#[$attr:meta])* $resource_name:ident: $resource_type:ty),* $(,)?
        }
    ) => {
        $crate::ecs_multi_impl! {
            $ecs {
                $($world_name {
                    $($(#[$comp_attr])* $name: $type => $mask),*
                })+
            }
            Tags {}
            Events {}
            $resources {
                $($(#[$attr])* $resource_name: $resource_type),*
            }
        }
    };
}

/// Emits the shared archetype kernel for one world type: component constants,
/// table storage, entity location bookkeeping, structural log, change-tick
/// tracking, and every component accessor. The invoking macro defines the
/// world struct itself (which must carry the kernel fields) and everything
/// tag-, event-, and command-related, since those differ per mode.
///
/// `$allow_insert` controls whether `add_components` may materialize a row
/// for a live entity this world has never stored. Multi-world worlds need
/// that (entities gain components per world lazily); a single world never
/// does, because spawning always creates the row.
///
/// # Extending these macros
///
/// `macro_rules!` cannot nest repetitions from different capture groups: a
/// `$($tag_name ...)*` inside a `$($name ...)*` fails with "meta-variable
/// repeats N times, but ... repeats M times" because the expander tries to
/// iterate the groups in lockstep. That single constraint shaped the tag
/// architecture, and any new feature that needs component and tag captures
/// in one body must use one of the three patterns already in this file:
///
/// - Generate the code at method level, outside any per-component
///   repetition, where each group can repeat independently (the mode-level
///   `for_each*` wrappers and their filter closures).
/// - Route the shared body through a kernel free function that takes the
///   tables and query cache as separate parameters, and pass tag state in
///   as a closure built where tag names are in scope, destructuring `self`
///   into disjoint fields when a `&mut` path needs it (`tables_for_each_*`).
/// - Inside a per-component repetition, avoid capturing tag names at all
///   and call a whole-`self` helper like `entity_matches_tags` between
///   short-lived borrows (`query_<name>_mut`).
///
/// Generated items carry `#[allow(unused)]` because a user's declaration
/// legitimately uses a fraction of the emitted surface. The cost is that
/// genuinely dead generated code will not warn, so removals here need a
/// manual search for remaining emit sites.
#[doc(hidden)]
#[macro_export]
macro_rules! ecs_kernel_impl {
    (
        $world:ident,
        $allow_insert:literal,
        { $($(#[$comp_attr:meta])* $name:ident: $type:ty => $mask:ident),* $(,)? }
    ) => {
        $crate::paste::paste! {
            #[repr(u64)]
            #[allow(clippy::upper_case_acronyms)]
            #[allow(non_camel_case_types)]
            #[allow(unused)]
            pub enum [<$world Component>] {
                $($(#[$comp_attr])* $mask,)*
            }

            $($(#[$comp_attr])* pub const $mask: u64 = 1 << ([<$world Component>]::$mask as u64);)*

            #[allow(unused)]
            pub const [<$world:snake:upper _COMPONENT_COUNT>]: usize = {
                let mut count = 0;
                $($(#[$comp_attr])* { count += 1; let _ = [<$world Component>]::$mask; })*
                count
            };

            #[allow(unused)]
            pub const [<$world:snake:upper _ALL_COMPONENTS>]: u64 = {
                let mut mask = 0;
                $($(#[$comp_attr])* { mask |= $mask; })*
                mask
            };

            #[derive(Default)]
            pub struct [<$world ComponentArrays>] {
                $($(#[$comp_attr])* pub $name: Vec<$type>,)*
                $($(#[$comp_attr])* pub [<$name _changed>]: Vec<u32>,)*
                $($(#[$comp_attr])* pub [<$name _peak_changed>]: u32,)*
                pub entity_indices: Vec<$crate::Entity>,
                pub mask: u64,
            }

            #[allow(unused)]
            impl [<$world ComponentArrays>] {
                /// Stamps every row of the masked columns as changed at
                /// `tick`, the bulk opt-in for whole-column raw writes:
                /// after filling columns through `query_mut()` closures or
                /// `for_each_*_mut` table loops, one call here makes the
                /// pass visible to tick-diffing consumers at zero per-row
                /// cost during the write. Pass the world's `current_tick`.
                pub fn mark_columns_changed(&mut self, mask: u64, tick: u32) {
                    $(
                        $(#[$comp_attr])*
                        {
                            if self.mask & mask & $mask != 0 {
                                self.[<$name _changed>].fill(tick);
                                self.[<$name _peak_changed>] = tick;
                            }
                        }
                    )*
                }
            }

            #[allow(unused)]
            fn [<get_component_index_ $world:snake>](mask: u64) -> Option<usize> {
                match mask {
                    $($(#[$comp_attr])* $mask => Some([<$world Component>]::$mask as _),)*
                    _ => None,
                }
            }

            #[allow(unused)]
            fn [<get_location_ $world:snake>](
                locations: &$crate::EntityLocations,
                entity: $crate::Entity,
            ) -> Option<(usize, usize)> {
                let location = locations.get(entity.id)?;
                if !location.allocated || location.generation != entity.generation {
                    return None;
                }
                Some((location.table_index as usize, location.array_index as usize))
            }

            #[allow(unused)]
            fn [<insert_location_ $world:snake>](
                locations: &mut $crate::EntityLocations,
                entity: $crate::Entity,
                location: (usize, usize),
            ) {
                locations.insert(entity.id, $crate::EntityLocation {
                    generation: entity.generation,
                    table_index: location.0 as u32,
                    array_index: location.1 as u32,
                    allocated: true,
                });
            }

            #[allow(unused)]
            fn [<remove_from_table_ $world:snake>](
                arrays: &mut [<$world ComponentArrays>],
                index: usize,
            ) -> Option<$crate::Entity> {
                let last_index = arrays.entity_indices.len() - 1;
                let mut swapped_entity = None;
                if index < last_index {
                    swapped_entity = Some(arrays.entity_indices[last_index]);
                }
                $(
                    $(#[$comp_attr])*
                    {
                        if arrays.mask & $mask != 0 {
                            arrays.$name.swap_remove(index);
                            arrays.[<$name _changed>].swap_remove(index);
                        }
                    }
                )*
                arrays.entity_indices.swap_remove(index);
                swapped_entity
            }

            #[allow(unused)]
            fn [<move_entity_ $world:snake>](
                world: &mut $world,
                entity: $crate::Entity,
                from_table: usize,
                from_index: usize,
                to_table: usize,
            ) {
                let tick = world.current_tick;
                $(
                    $(#[$comp_attr])*
                    {
                        let component = if world.tables[from_table].mask & $mask != 0 {
                            Some(std::mem::take(&mut world.tables[from_table].$name[from_index]))
                        } else {
                            None
                        };
                        if world.tables[to_table].mask & $mask != 0 {
                            let arrays = &mut world.tables[to_table];
                            arrays.$name.push(component.unwrap_or_default());
                            arrays.[<$name _changed>].push(tick);
                            arrays.[<$name _peak_changed>] = tick;
                        }
                    }
                )*
                world.tables[to_table].entity_indices.push(entity);
                let new_index = world.tables[to_table].entity_indices.len() - 1;
                [<insert_location_ $world:snake>](&mut world.entity_locations, entity, (to_table, new_index));

                if let Some(swapped) = [<remove_from_table_ $world:snake>](&mut world.tables[from_table], from_index) {
                    [<insert_location_ $world:snake>](
                        &mut world.entity_locations,
                        swapped,
                        (from_table, from_index),
                    );
                }
            }

            #[allow(unused)]
            fn [<get_or_create_table_ $world:snake>](world: &mut $world, mask: u64) -> usize {
                debug_assert_eq!(
                    mask & ![<$world:snake:upper _ALL_COMPONENTS>],
                    0,
                    "archetype masks must not contain tag bits or unknown component bits"
                );
                if let Some(&index) = world.table_lookup.get(&mask) {
                    return index;
                }

                let table_index = world.tables.len();
                world.tables.push([<$world ComponentArrays>] {
                    mask,
                    ..Default::default()
                });
                $crate::archetype_register_table(
                    $crate::ArchetypeRouting {
                        table_lookup: &mut world.table_lookup,
                        table_edges: &mut world.table_edges,
                        query_cache: &mut world.query_cache,
                    },
                    [<$world:snake:upper _COMPONENT_COUNT>],
                    mask,
                    table_index,
                    world.tables.iter().map(|table| table.mask),
                    [$($(#[$comp_attr])* $mask,)*].into_iter().filter_map(|component_mask| {
                        [<get_component_index_ $world:snake>](component_mask)
                            .map(|component_index| (component_mask, component_index))
                    }),
                );

                table_index
            }

            #[allow(unused)]
            fn [<cached_tables_ $world:snake>]<'cache>(
                query_cache: &'cache mut std::collections::HashMap<u64, Vec<usize>>,
                tables: &[[<$world ComponentArrays>]],
                mask: u64,
            ) -> &'cache [usize] {
                $crate::archetype_cached_tables(
                    query_cache,
                    tables.iter().map(|table| table.mask),
                    mask,
                )
            }

            #[allow(unused)]
            fn [<tables_for_each_ $world:snake>]<F, P>(
                tables: &[[<$world ComponentArrays>]],
                query_cache: &std::collections::HashMap<u64, Vec<usize>>,
                include: u64,
                exclude: u64,
                filter: P,
                mut f: F,
            ) where
                F: FnMut($crate::Entity, &[<$world ComponentArrays>], usize),
                P: Fn($crate::Entity) -> bool,
            {
                if let Some(cached) = query_cache.get(&include) {
                    for &table_index in cached {
                        let table = &tables[table_index];
                        if table.mask & exclude != 0 {
                            continue;
                        }
                        for (index, &entity) in table.entity_indices.iter().enumerate() {
                            if filter(entity) {
                                f(entity, table, index);
                            }
                        }
                    }
                    return;
                }
                for table in tables {
                    if table.mask & include != include || table.mask & exclude != 0 {
                        continue;
                    }
                    for (index, &entity) in table.entity_indices.iter().enumerate() {
                        if filter(entity) {
                            f(entity, table, index);
                        }
                    }
                }
            }

            #[allow(unused)]
            fn [<tables_for_each_mut_ $world:snake>]<F, P>(
                tables: &mut [[<$world ComponentArrays>]],
                query_cache: &mut std::collections::HashMap<u64, Vec<usize>>,
                include: u64,
                exclude: u64,
                filter: P,
                mut f: F,
            ) where
                F: FnMut($crate::Entity, &mut [<$world ComponentArrays>], usize),
                P: Fn($crate::Entity) -> bool,
            {
                let table_indices = [<cached_tables_ $world:snake>](query_cache, tables, include);
                for position in 0..table_indices.len() {
                    let table_index = table_indices[position];
                    let table = &mut tables[table_index];
                    if table.mask & exclude != 0 {
                        continue;
                    }
                    for index in 0..table.entity_indices.len() {
                        let entity = table.entity_indices[index];
                        if filter(entity) {
                            f(entity, table, index);
                        }
                    }
                }
            }

            #[cfg(not(target_family = "wasm"))]
            #[allow(unused)]
            fn [<tables_par_for_each_mut_ $world:snake>]<F, P>(
                tables: &mut [[<$world ComponentArrays>]],
                include: u64,
                exclude: u64,
                filter: P,
                f: F,
            ) where
                F: Fn($crate::Entity, &mut [<$world ComponentArrays>], usize) + Send + Sync,
                P: Fn($crate::Entity) -> bool + Send + Sync,
            {
                use $crate::rayon::prelude::*;
                tables
                    .par_iter_mut()
                    .filter(|table| table.mask & include == include && table.mask & exclude == 0)
                    .for_each(|table| {
                        for index in 0..table.entity_indices.len() {
                            let entity = table.entity_indices[index];
                            if filter(entity) {
                                f(entity, table, index);
                            }
                        }
                    });
            }

            #[allow(unused)]
            fn [<tables_for_each_mut_changed_ $world:snake>]<F, P>(
                tables: &mut [[<$world ComponentArrays>]],
                query_cache: &mut std::collections::HashMap<u64, Vec<usize>>,
                include: u64,
                exclude: u64,
                since_tick: u32,
                filter: P,
                mut f: F,
            ) where
                F: FnMut($crate::Entity, &mut [<$world ComponentArrays>], usize),
                P: Fn($crate::Entity) -> bool,
            {
                let table_indices = [<cached_tables_ $world:snake>](query_cache, tables, include);
                for position in 0..table_indices.len() {
                    let table_index = table_indices[position];
                    let table = &mut tables[table_index];
                    if table.mask & exclude != 0 {
                        continue;
                    }

                    let mut table_changed = false;
                    $(
                        $(#[$comp_attr])*
                        {
                            if include & $mask != 0
                                && table.mask & $mask != 0
                                && $crate::tick_is_newer(table.[<$name _peak_changed>], since_tick)
                            {
                                table_changed = true;
                            }
                        }
                    )*
                    if !table_changed {
                        continue;
                    }

                    for index in 0..table.entity_indices.len() {
                        let entity = table.entity_indices[index];
                        if !filter(entity) {
                            continue;
                        }

                        let mut changed = false;
                        $(
                            $(#[$comp_attr])*
                            {
                                if include & $mask != 0
                                    && table.mask & $mask != 0
                                    && $crate::tick_is_newer(table.[<$name _changed>][index], since_tick)
                                {
                                    changed = true;
                                }
                            }
                        )*
                        if changed {
                            f(entity, table, index);
                        }
                    }
                }
            }

            pub struct [<$world EntityQueryIter>]<'world> {
                tables: &'world [[<$world ComponentArrays>]],
                mask: u64,
                table_index: usize,
                array_index: usize,
            }

            impl<'world> Iterator for [<$world EntityQueryIter>]<'world> {
                type Item = $crate::Entity;

                fn next(&mut self) -> Option<Self::Item> {
                    loop {
                        if self.table_index >= self.tables.len() {
                            return None;
                        }
                        let table = &self.tables[self.table_index];
                        if table.mask & self.mask != self.mask {
                            self.table_index += 1;
                            self.array_index = 0;
                            continue;
                        }
                        if self.array_index >= table.entity_indices.len() {
                            self.table_index += 1;
                            self.array_index = 0;
                            continue;
                        }
                        let entity = table.entity_indices[self.array_index];
                        self.array_index += 1;
                        return Some(entity);
                    }
                }

                fn size_hint(&self) -> (usize, Option<usize>) {
                    let mut remaining = 0;
                    for table_index in self.table_index..self.tables.len() {
                        let table = &self.tables[table_index];
                        if table.mask & self.mask != self.mask {
                            continue;
                        }
                        if table_index == self.table_index {
                            remaining += table.entity_indices.len().saturating_sub(self.array_index);
                        } else {
                            remaining += table.entity_indices.len();
                        }
                    }
                    (remaining, Some(remaining))
                }
            }

            pub struct [<$world ChangedEntityQueryIter>]<'world> {
                tables: &'world [[<$world ComponentArrays>]],
                mask: u64,
                since_tick: u32,
                table_index: usize,
                array_index: usize,
            }

            impl<'world> Iterator for [<$world ChangedEntityQueryIter>]<'world> {
                type Item = $crate::Entity;

                fn next(&mut self) -> Option<Self::Item> {
                    loop {
                        if self.table_index >= self.tables.len() {
                            return None;
                        }
                        let table = &self.tables[self.table_index];
                        if table.mask & self.mask != self.mask {
                            self.table_index += 1;
                            self.array_index = 0;
                            continue;
                        }
                        if self.array_index == 0 {
                            let mut table_changed = false;
                            $(
                                $(#[$comp_attr])*
                                {
                                    if self.mask & $mask != 0
                                        && table.mask & $mask != 0
                                        && $crate::tick_is_newer(table.[<$name _peak_changed>], self.since_tick)
                                    {
                                        table_changed = true;
                                    }
                                }
                            )*
                            if !table_changed {
                                self.table_index += 1;
                                continue;
                            }
                        }
                        if self.array_index >= table.entity_indices.len() {
                            self.table_index += 1;
                            self.array_index = 0;
                            continue;
                        }
                        let index = self.array_index;
                        self.array_index += 1;

                        let mut changed = false;
                        $(
                            $(#[$comp_attr])*
                            {
                                if self.mask & $mask != 0
                                    && table.mask & $mask != 0
                                    && $crate::tick_is_newer(table.[<$name _changed>][index], self.since_tick)
                                {
                                    changed = true;
                                }
                            }
                        )*
                        if changed {
                            return Some(table.entity_indices[index]);
                        }
                    }
                }
            }

            $(
                $(#[$comp_attr])*
                pub struct [<$mask:camel QueryIter>]<'world> {
                    tables: &'world [[<$world ComponentArrays>]],
                    table_index: usize,
                    array_index: usize,
                }

                $(#[$comp_attr])*
                impl<'world> Iterator for [<$mask:camel QueryIter>]<'world> {
                    type Item = &'world $type;

                    fn next(&mut self) -> Option<Self::Item> {
                        loop {
                            if self.table_index >= self.tables.len() {
                                return None;
                            }
                            let table = &self.tables[self.table_index];
                            if table.mask & $mask == 0 {
                                self.table_index += 1;
                                self.array_index = 0;
                                continue;
                            }
                            if self.array_index >= table.$name.len() {
                                self.table_index += 1;
                                self.array_index = 0;
                                continue;
                            }
                            let component = &table.$name[self.array_index];
                            self.array_index += 1;
                            return Some(component);
                        }
                    }

                    fn size_hint(&self) -> (usize, Option<usize>) {
                        let mut remaining = 0;
                        for table_index in self.table_index..self.tables.len() {
                            let table = &self.tables[table_index];
                            if table.mask & $mask == 0 {
                                continue;
                            }
                            if table_index == self.table_index {
                                remaining += table.$name.len().saturating_sub(self.array_index);
                            } else {
                                remaining += table.$name.len();
                            }
                        }
                        (remaining, Some(remaining))
                    }
                }
            )*

            #[allow(unused)]
            pub struct [<$world QueryBuilder>]<'world> {
                world: &'world $world,
                include: u64,
                exclude: u64,
            }

            #[allow(unused)]
            impl<'world> [<$world QueryBuilder>]<'world> {
                pub fn new(world: &'world $world) -> Self {
                    Self {
                        world,
                        include: 0,
                        exclude: 0,
                    }
                }

                pub fn with(mut self, mask: u64) -> Self {
                    self.include |= mask;
                    self
                }

                pub fn without(mut self, mask: u64) -> Self {
                    self.exclude |= mask;
                    self
                }

                pub fn iter<F>(self, f: F)
                where
                    F: FnMut($crate::Entity, &[<$world ComponentArrays>], usize),
                {
                    self.world.for_each(self.include, self.exclude, f);
                }
            }

            #[allow(unused)]
            pub struct [<$world QueryBuilderMut>]<'world> {
                world: &'world mut $world,
                include: u64,
                exclude: u64,
            }

            #[allow(unused)]
            impl<'world> [<$world QueryBuilderMut>]<'world> {
                pub fn new(world: &'world mut $world) -> Self {
                    Self {
                        world,
                        include: 0,
                        exclude: 0,
                    }
                }

                pub fn with(mut self, mask: u64) -> Self {
                    self.include |= mask;
                    self
                }

                pub fn without(mut self, mask: u64) -> Self {
                    self.exclude |= mask;
                    self
                }

                pub fn iter<F>(self, f: F)
                where
                    F: FnMut($crate::Entity, &mut [<$world ComponentArrays>], usize),
                {
                    self.world.for_each_mut(self.include, self.exclude, f);
                }
            }

            #[allow(unused)]
            impl $world {
                const KERNEL_ALLOW_INSERT: bool = $allow_insert;

                pub fn query(&self) -> [<$world QueryBuilder>]<'_> {
                    [<$world QueryBuilder>]::new(self)
                }

                pub fn query_mut(&mut self) -> [<$world QueryBuilderMut>]<'_> {
                    [<$world QueryBuilderMut>]::new(self)
                }

                pub fn contains_entity(&self, entity: $crate::Entity) -> bool {
                    [<get_location_ $world:snake>](&self.entity_locations, entity).is_some()
                }

                $(
                    $(#[$comp_attr])*
                    #[inline]
                    pub fn [<get_ $name>](&self, entity: $crate::Entity) -> Option<&$type> {
                        let (table_index, array_index) = [<get_location_ $world:snake>](&self.entity_locations, entity)?;
                        let table = &self.tables[table_index];
                        if table.mask & $mask == 0 {
                            return None;
                        }
                        Some(&table.$name[array_index])
                    }

                    $(#[$comp_attr])*
                    #[inline]
                    pub fn [<get_ $name _mut>](&mut self, entity: $crate::Entity) -> Option<&mut $type> {
                        let (table_index, array_index) = [<get_location_ $world:snake>](&self.entity_locations, entity)?;
                        let current_tick = self.current_tick;
                        let table = &mut self.tables[table_index];
                        if table.mask & $mask == 0 {
                            return None;
                        }
                        table.[<$name _changed>][array_index] = current_tick;
                        table.[<$name _peak_changed>] = current_tick;
                        Some(&mut table.$name[array_index])
                    }

                    $(#[$comp_attr])*
                    #[inline]
                    pub fn [<modify_ $name>]<R>(&mut self, entity: $crate::Entity, f: impl FnOnce(&mut $type) -> R) -> Option<R> {
                        let (table_index, array_index) = [<get_location_ $world:snake>](&self.entity_locations, entity)?;
                        let current_tick = self.current_tick;
                        let table = &mut self.tables[table_index];
                        if table.mask & $mask == 0 {
                            return None;
                        }
                        table.[<$name _changed>][array_index] = current_tick;
                        table.[<$name _peak_changed>] = current_tick;
                        Some(f(&mut table.$name[array_index]))
                    }

                    $(#[$comp_attr])*
                    #[inline]
                    pub fn [<entity_has_ $name>](&self, entity: $crate::Entity) -> bool {
                        self.entity_has_components(entity, $mask)
                    }

                    $(#[$comp_attr])*
                    #[inline]
                    pub fn [<set_ $name>](&mut self, entity: $crate::Entity, value: $type) {
                        let current_tick = self.current_tick;
                        if let Some((table_index, array_index)) = [<get_location_ $world:snake>](&self.entity_locations, entity) {
                            let table = &mut self.tables[table_index];
                            if table.mask & $mask != 0 {
                                table.$name[array_index] = value;
                                table.[<$name _changed>][array_index] = current_tick;
                                table.[<$name _peak_changed>] = current_tick;
                                return;
                            }
                        }
                        if self.add_components(entity, $mask) {
                            if let Some((table_index, array_index)) = [<get_location_ $world:snake>](&self.entity_locations, entity) {
                                let table = &mut self.tables[table_index];
                                table.$name[array_index] = value;
                                table.[<$name _changed>][array_index] = current_tick;
                                table.[<$name _peak_changed>] = current_tick;
                            }
                        }
                    }

                    $(#[$comp_attr])*
                    #[inline]
                    pub fn [<add_ $name>](&mut self, entity: $crate::Entity) {
                        self.add_components(entity, $mask);
                    }

                    $(#[$comp_attr])*
                    #[inline]
                    pub fn [<remove_ $name>](&mut self, entity: $crate::Entity) -> bool {
                        self.remove_components(entity, $mask)
                    }

                    $(#[$comp_attr])*
                    #[inline]
                    pub fn [<query_ $name>](&self) -> [<$mask:camel QueryIter>]<'_> {
                        [<$mask:camel QueryIter>] {
                            tables: &self.tables,
                            table_index: 0,
                            array_index: 0,
                        }
                    }

                    $(#[$comp_attr])*
                    pub fn [<iter_ $name>]<F>(&self, mut f: F)
                    where
                        F: FnMut($crate::Entity, &$type),
                    {
                        [<tables_for_each_ $world:snake>](
                            &self.tables,
                            &self.query_cache,
                            $mask,
                            0,
                            |_| true,
                            |entity, table, index| f(entity, &table.$name[index]),
                        );
                    }

                    $(#[$comp_attr])*
                    pub fn [<for_each_ $name _mut>]<F>(&mut self, mut f: F)
                    where
                        F: FnMut(&mut $type),
                    {
                        let table_indices =
                            [<cached_tables_ $world:snake>](&mut self.query_cache, &self.tables, $mask);
                        for position in 0..table_indices.len() {
                            let table_index = table_indices[position];
                            for component in &mut self.tables[table_index].$name {
                                f(component);
                            }
                        }
                    }

                    $(#[$comp_attr])*
                    #[cfg(not(target_family = "wasm"))]
                    pub fn [<par_for_each_ $name _mut>]<F>(&mut self, f: F)
                    where
                        F: Fn(&mut $type) + Send + Sync,
                    {
                        use $crate::rayon::prelude::*;
                        self.tables
                            .par_iter_mut()
                            .filter(|table| table.mask & $mask != 0)
                            .for_each(|table| {
                                table.$name.par_iter_mut().for_each(|component| f(component));
                            });
                    }

                    $(#[$comp_attr])*
                    pub fn [<iter_ $name _slices>](&self) -> impl Iterator<Item = &[$type]> {
                        self.tables
                            .iter()
                            .filter(|table| table.mask & $mask != 0)
                            .map(|table| table.$name.as_slice())
                    }

                    $(#[$comp_attr])*
                    pub fn [<iter_ $name _slices_mut>](&mut self) -> impl Iterator<Item = &mut [$type]> {
                        self.tables
                            .iter_mut()
                            .filter(|table| table.mask & $mask != 0)
                            .map(|table| table.$name.as_mut_slice())
                    }
                )*

                pub fn spawn_entities_with(
                    &mut self,
                    allocator: &mut $crate::EntityAllocator,
                    mask: u64,
                    count: usize,
                ) -> Vec<$crate::Entity> {
                    debug_assert_eq!(
                        mask & ![<$world:snake:upper _ALL_COMPONENTS>],
                        0,
                        "spawn masks must not contain tag bits or unknown component bits"
                    );
                    let table_index = [<get_or_create_table_ $world:snake>](self, mask);
                    let start_index = self.tables[table_index].entity_indices.len();

                    self.tables[table_index].entity_indices.reserve(count);
                    $(
                        $(#[$comp_attr])*
                        {
                            if mask & $mask != 0 {
                                self.tables[table_index].$name.reserve(count);
                                self.tables[table_index].[<$name _changed>].reserve(count);
                                self.tables[table_index].[<$name _peak_changed>] = self.current_tick;
                            }
                        }
                    )*

                    let mut entities = Vec::new();
                    allocator.allocate_batch(count, &mut entities);
                    for (offset, &entity) in entities.iter().enumerate() {
                        self.tables[table_index].entity_indices.push(entity);
                        $(
                            $(#[$comp_attr])*
                            {
                                if mask & $mask != 0 {
                                    self.tables[table_index].$name.push(<$type>::default());
                                    self.tables[table_index].[<$name _changed>].push(self.current_tick);
                                }
                            }
                        )*

                        [<insert_location_ $world:snake>](
                            &mut self.entity_locations,
                            entity,
                            (table_index, start_index + offset),
                        );
                        self.record_structural(entity, $crate::StructuralChangeKind::Spawned, mask);
                    }

                    entities
                }

                pub fn spawn_batch_with<F>(
                    &mut self,
                    allocator: &mut $crate::EntityAllocator,
                    mask: u64,
                    count: usize,
                    mut init: F,
                ) -> Vec<$crate::Entity>
                where
                    F: FnMut(&mut [<$world ComponentArrays>], usize),
                {
                    debug_assert_eq!(
                        mask & ![<$world:snake:upper _ALL_COMPONENTS>],
                        0,
                        "spawn masks must not contain tag bits or unknown component bits"
                    );
                    let table_index = [<get_or_create_table_ $world:snake>](self, mask);
                    let start_index = self.tables[table_index].entity_indices.len();

                    self.tables[table_index].entity_indices.reserve(count);
                    $(
                        $(#[$comp_attr])*
                        {
                            if mask & $mask != 0 {
                                self.tables[table_index].$name.reserve(count);
                                self.tables[table_index].[<$name _changed>].reserve(count);
                                self.tables[table_index].[<$name _peak_changed>] = self.current_tick;
                            }
                        }
                    )*

                    let mut entities = Vec::new();
                    allocator.allocate_batch(count, &mut entities);
                    for (offset, &entity) in entities.iter().enumerate() {
                        self.tables[table_index].entity_indices.push(entity);
                        $(
                            $(#[$comp_attr])*
                            {
                                if mask & $mask != 0 {
                                    self.tables[table_index].$name.push(<$type>::default());
                                    self.tables[table_index].[<$name _changed>].push(self.current_tick);
                                }
                            }
                        )*

                        [<insert_location_ $world:snake>](
                            &mut self.entity_locations,
                            entity,
                            (table_index, start_index + offset),
                        );
                        self.record_structural(entity, $crate::StructuralChangeKind::Spawned, mask);

                        init(&mut self.tables[table_index], start_index + offset);
                    }

                    entities
                }

                /// Frees each live handle through the allocator and retires its
                /// row. Stale or already-despawned handles are skipped.
                pub fn despawn_entities_with(
                    &mut self,
                    allocator: &mut $crate::EntityAllocator,
                    entities: &[$crate::Entity],
                ) -> Vec<$crate::Entity> {
                    let mut despawned = Vec::with_capacity(entities.len());
                    for &entity in entities {
                        if allocator.deallocate(entity) {
                            self.retire_entity(entity);
                            despawned.push(entity);
                        }
                    }
                    despawned
                }

                /// Removes the entity's row if present and records the next
                /// generation for this id, so stale writes can be refused even
                /// in worlds that never stored the entity. Must only be called
                /// with a handle the allocator confirmed live; in multi-world
                /// mode this is the per-world eviction step that `despawn`
                /// drives across every world. Calling it directly on a
                /// single world leaks the handle: the row disappears while the
                /// allocator still counts it live, so the id is never
                /// recycled. Use `despawn_entities` there instead.
                pub fn retire_entity(&mut self, entity: $crate::Entity) -> bool {
                    let mut removed = false;
                    if let Some(loc) = self.entity_locations.get_mut(entity.id) {
                        if loc.allocated && loc.generation == entity.generation {
                            let table_index = loc.table_index as usize;
                            let array_index = loc.array_index as usize;

                            self.entity_locations.mark_deallocated(entity.id);

                            let despawned_mask = if table_index < self.tables.len() {
                                self.tables[table_index].mask
                            } else {
                                0
                            };
                            self.record_structural(entity, $crate::StructuralChangeKind::Despawned, despawned_mask);

                            if table_index < self.tables.len() {
                                if let Some(swapped) = [<remove_from_table_ $world:snake>](&mut self.tables[table_index], array_index) {
                                    if let Some(swapped_location) = self.entity_locations.get_mut(swapped.id) {
                                        if swapped_location.allocated {
                                            swapped_location.array_index = array_index as u32;
                                        }
                                    }
                                }
                            }
                            removed = true;
                        }
                    }

                    let next_generation = entity.generation.wrapping_add(1);
                    let should_retire = match self.entity_locations.get(entity.id) {
                        None => true,
                        Some(loc) => {
                            !loc.allocated && $crate::tick_is_newer(next_generation, loc.generation)
                        }
                    };
                    if should_retire {
                        self.entity_locations.ensure_slot(entity.id, next_generation);
                    }

                    removed
                }

                /// Adds components, migrating the entity's row. In
                /// multi-world mode a live handle this world has never stored
                /// gets a row inserted directly; the generation check refuses
                /// stale handles for any id this ECS has despawned, but a
                /// handle forged for an id the allocator never issued cannot
                /// be detected here because worlds hold no allocator access.
                /// Only handles minted by the shared allocator are supported.
                pub fn add_components(&mut self, entity: $crate::Entity, mask: u64) -> bool {
                    debug_assert_eq!(
                        mask & ![<$world:snake:upper _ALL_COMPONENTS>],
                        0,
                        "component masks must not contain tag bits or unknown component bits"
                    );
                    if let Some((table_index, array_index)) = [<get_location_ $world:snake>](&self.entity_locations, entity) {
                        let current_mask = self.tables[table_index].mask;
                        if current_mask & mask == mask {
                            return true;
                        }

                        let target_table = if mask.count_ones() == 1 {
                            [<get_component_index_ $world:snake>](mask)
                                .and_then(|component_index| self.table_edges[table_index].add_edges[component_index])
                        } else {
                            self.table_edges[table_index].multi_add_cache.get(&mask).copied()
                        };

                        let new_table_index = target_table.unwrap_or_else(|| {
                            let new_index = [<get_or_create_table_ $world:snake>](self, current_mask | mask);
                            self.table_edges[table_index].multi_add_cache.insert(mask, new_index);
                            new_index
                        });

                        [<move_entity_ $world:snake>](self, entity, table_index, array_index, new_table_index);
                        self.record_structural(entity, $crate::StructuralChangeKind::ComponentsAdded, mask & !current_mask);
                        true
                    } else if !Self::KERNEL_ALLOW_INSERT {
                        false
                    } else {
                        if let Some(loc) = self.entity_locations.get(entity.id) {
                            if loc.allocated || loc.generation != entity.generation {
                                return false;
                            }
                        }

                        let table_index = [<get_or_create_table_ $world:snake>](self, mask);
                        let start_index = self.tables[table_index].entity_indices.len();

                        self.tables[table_index].entity_indices.push(entity);
                        $(
                            $(#[$comp_attr])*
                            {
                                if mask & $mask != 0 {
                                    self.tables[table_index].$name.push(<$type>::default());
                                    self.tables[table_index].[<$name _changed>].push(self.current_tick);
                                    self.tables[table_index].[<$name _peak_changed>] = self.current_tick;
                                }
                            }
                        )*

                        [<insert_location_ $world:snake>](
                            &mut self.entity_locations,
                            entity,
                            (table_index, start_index),
                        );
                        self.record_structural(entity, $crate::StructuralChangeKind::Spawned, mask);
                        true
                    }
                }

                pub fn remove_components(&mut self, entity: $crate::Entity, mask: u64) -> bool {
                    debug_assert_eq!(
                        mask & ![<$world:snake:upper _ALL_COMPONENTS>],
                        0,
                        "component masks must not contain tag bits or unknown component bits"
                    );
                    if let Some((table_index, array_index)) = [<get_location_ $world:snake>](&self.entity_locations, entity) {
                        let current_mask = self.tables[table_index].mask;
                        if current_mask & mask == 0 {
                            return true;
                        }

                        let target_table = if mask.count_ones() == 1 {
                            [<get_component_index_ $world:snake>](mask)
                                .and_then(|component_index| self.table_edges[table_index].remove_edges[component_index])
                        } else {
                            self.table_edges[table_index].multi_remove_cache.get(&mask).copied()
                        };

                        let new_table_index = target_table.unwrap_or_else(|| {
                            let new_index = [<get_or_create_table_ $world:snake>](self, current_mask & !mask);
                            self.table_edges[table_index].multi_remove_cache.insert(mask, new_index);
                            new_index
                        });

                        [<move_entity_ $world:snake>](self, entity, table_index, array_index, new_table_index);
                        self.record_structural(entity, $crate::StructuralChangeKind::ComponentsRemoved, current_mask & mask);
                        true
                    } else {
                        false
                    }
                }

                pub fn component_mask(&self, entity: $crate::Entity) -> Option<u64> {
                    [<get_location_ $world:snake>](&self.entity_locations, entity)
                        .map(|(table_index, _)| self.tables[table_index].mask)
                }

                pub fn get_all_entities(&self) -> Vec<$crate::Entity> {
                    let mut result = Vec::with_capacity(self.entity_count());
                    for table in &self.tables {
                        result.extend(table.entity_indices.iter().copied());
                    }
                    result
                }

                pub fn entity_count(&self) -> usize {
                    self.tables.iter().map(|table| table.entity_indices.len()).sum()
                }

                pub fn entity_has_components(&self, entity: $crate::Entity, components: u64) -> bool {
                    self.component_mask(entity).unwrap_or(0) & components == components
                }

                pub fn increment_tick(&mut self) {
                    self.last_tick = self.current_tick;
                    self.current_tick = self.current_tick.wrapping_add(1);
                }

                pub fn current_tick(&self) -> u32 {
                    self.current_tick
                }

                pub fn last_tick(&self) -> u32 {
                    self.last_tick
                }

                fn record_structural(&mut self, entity: $crate::Entity, kind: $crate::StructuralChangeKind, mask: u64) {
                    if self.structural_log.len() >= $crate::STRUCTURAL_LOG_CAPACITY {
                        self.structural_log.clear();
                    }
                    self.structural_sequence += 1;
                    self.structural_log.push($crate::StructuralChange {
                        sequence: self.structural_sequence,
                        entity,
                        kind,
                        mask,
                    });
                }

                pub fn structural_sequence(&self) -> u64 {
                    self.structural_sequence
                }

                pub fn structural_changes_since(&self, cursor: u64) -> &[$crate::StructuralChange] {
                    let start = self.structural_log.partition_point(|change| change.sequence <= cursor);
                    &self.structural_log[start..]
                }

                pub fn trim_structural_log(&mut self, up_to_sequence: u64) {
                    let end = self.structural_log.partition_point(|change| change.sequence <= up_to_sequence);
                    self.structural_log.drain(..end);
                }

                pub fn clear_structural_log(&mut self) {
                    self.structural_log.clear();
                }

                pub fn query_entities(&self, mask: u64) -> [<$world EntityQueryIter>]<'_> {
                    debug_assert_eq!(
                        mask & ![<$world:snake:upper _ALL_COMPONENTS>],
                        0,
                        "query_entities takes component masks only; use for_each or query_<tag> for tag filtering"
                    );
                    [<$world EntityQueryIter>] {
                        tables: &self.tables,
                        mask,
                        table_index: 0,
                        array_index: 0,
                    }
                }

                pub fn query_entities_changed(&self, mask: u64) -> [<$world ChangedEntityQueryIter>]<'_> {
                    self.query_entities_changed_since(mask, self.last_tick)
                }

                pub fn query_entities_changed_since(&self, mask: u64, since_tick: u32) -> [<$world ChangedEntityQueryIter>]<'_> {
                    debug_assert_eq!(
                        mask & ![<$world:snake:upper _ALL_COMPONENTS>],
                        0,
                        "changed queries take component masks only; use for_each_mut_changed for tag filtering"
                    );
                    [<$world ChangedEntityQueryIter>] {
                        tables: &self.tables,
                        mask,
                        since_tick,
                        table_index: 0,
                        array_index: 0,
                    }
                }

                /// Explicitly stamps change ticks for the masked components
                /// on one entity. This is the opt-in for raw-tier writes:
                /// `query_mut()` closures, `for_each_*_mut`, and slice
                /// iterators do not stamp, so follow such writes with this
                /// call when downstream consumers diff by ticks. Returns
                /// false if the entity is missing or its table lacks every
                /// masked component.
                pub fn mark_changed(&mut self, entity: $crate::Entity, mask: u64) -> bool {
                    let Some((table_index, array_index)) = [<get_location_ $world:snake>](&self.entity_locations, entity) else {
                        return false;
                    };
                    let current_tick = self.current_tick;
                    let table = &mut self.tables[table_index];
                    let present = table.mask & mask & [<$world:snake:upper _ALL_COMPONENTS>];
                    if present == 0 {
                        return false;
                    }
                    $(
                        $(#[$comp_attr])*
                        {
                            if present & $mask != 0 {
                                table.[<$name _changed>][array_index] = current_tick;
                                table.[<$name _peak_changed>] = current_tick;
                            }
                        }
                    )*
                    true
                }

                pub fn query_first_entity(&self, mask: u64) -> Option<$crate::Entity> {
                    debug_assert_eq!(
                        mask & ![<$world:snake:upper _ALL_COMPONENTS>],
                        0,
                        "query_first_entity takes component masks only"
                    );
                    for table in &self.tables {
                        if table.mask & mask != mask {
                            continue;
                        }
                        if let Some(&entity) = table.entity_indices.first() {
                            return Some(entity);
                        }
                    }
                    None
                }

                #[inline]
                pub fn for_each_with_tags<F>(
                    &self,
                    include: u64,
                    exclude: u64,
                    include_tags: &[&$crate::SparseTagSet],
                    exclude_tags: &[&$crate::SparseTagSet],
                    f: F,
                ) where
                    F: FnMut($crate::Entity, &[<$world ComponentArrays>], usize),
                {
                    [<tables_for_each_ $world:snake>](
                        &self.tables,
                        &self.query_cache,
                        include,
                        exclude,
                        |entity| {
                            include_tags.iter().all(|tag_set| tag_set.contains(entity))
                                && !exclude_tags.iter().any(|tag_set| tag_set.contains(entity))
                        },
                        f,
                    );
                }

                #[inline]
                pub fn for_each_mut_with_tags<F>(
                    &mut self,
                    include: u64,
                    exclude: u64,
                    include_tags: &[&$crate::SparseTagSet],
                    exclude_tags: &[&$crate::SparseTagSet],
                    f: F,
                ) where
                    F: FnMut($crate::Entity, &mut [<$world ComponentArrays>], usize),
                {
                    [<tables_for_each_mut_ $world:snake>](
                        &mut self.tables,
                        &mut self.query_cache,
                        include,
                        exclude,
                        |entity| {
                            include_tags.iter().all(|tag_set| tag_set.contains(entity))
                                && !exclude_tags.iter().any(|tag_set| tag_set.contains(entity))
                        },
                        f,
                    );
                }

                #[cfg(not(target_family = "wasm"))]
                #[inline]
                pub fn par_for_each_mut_with_tags<F>(
                    &mut self,
                    include: u64,
                    exclude: u64,
                    include_tags: &[&$crate::SparseTagSet],
                    exclude_tags: &[&$crate::SparseTagSet],
                    f: F,
                ) where
                    F: Fn($crate::Entity, &mut [<$world ComponentArrays>], usize) + Send + Sync,
                {
                    [<tables_par_for_each_mut_ $world:snake>](
                        &mut self.tables,
                        include,
                        exclude,
                        |entity| {
                            include_tags.iter().all(|tag_set| tag_set.contains(entity))
                                && !exclude_tags.iter().any(|tag_set| tag_set.contains(entity))
                        },
                        f,
                    );
                }
            }
        }
    };
}

#[macro_export]
macro_rules! ecs_impl {
    (
        $world:ident {
            $($(#[$comp_attr:meta])* $name:ident: $type:ty => $mask:ident),* $(,)?
        }
        Tags {
            $($tag_name:ident => $tag_mask:ident),* $(,)?
        }
        Events {
            $($event_name:ident: $event_type:ty),* $(,)?
        }
        $resources:ident {
            $($(#[$attr:meta])*  $resource_name:ident: $resource_type:ty),* $(,)?
        }
    ) => {
        $crate::ecs_kernel_impl! {
            $world,
            false,
            { $($(#[$comp_attr])* $name: $type => $mask),* }
        }

        $crate::paste::paste! {
            #[allow(non_camel_case_types)]
            #[allow(unused)]
            pub enum [<$world Tag>] {
                $($tag_name,)*
            }

            $(pub const $tag_mask: u64 = 1 << (63 - ([<$world Tag>]::$tag_name as u64));)*

            #[allow(unused)]
            pub const ALL_TAGS_MASK: u64 = 0 $(| $tag_mask)*;

            #[allow(unused)]
            pub const [<$world:snake:upper _TAG_COUNT>]: usize = {
                let mut count = 0;
                $(count += 1; let _ = [<$world Tag>]::$tag_name;)*
                count
            };

            const _: () = assert!(
                [<$world:snake:upper _COMPONENT_COUNT>] + [<$world:snake:upper _TAG_COUNT>] <= 64,
                "components plus tags must fit in a u64 mask"
            );

            #[allow(unused)]
            pub type ComponentArrays = [<$world ComponentArrays>];
        }

        #[allow(unused)]
        #[derive(Default, Clone)]
        pub struct EntityBuilder {
            $($(#[$comp_attr])* $name: Option<$type>,)*
        }

        #[allow(unused)]
        impl EntityBuilder {
            pub fn new() -> Self {
                Self::default()
            }

            $(
                $(#[$comp_attr])*
                $crate::paste::paste! {
                    pub fn [<with_$name>](mut self, value: $type) -> Self {
                        self.$name = Some(value);
                        self
                    }
                }
            )*

            pub fn spawn(self, world: &mut $world, instances: usize) -> Vec<$crate::Entity> {
                let mut mask = 0;
                $(
                    $(#[$comp_attr])*
                    if self.$name.is_some() {
                        mask |= $mask;
                    }
                )*
                let entities = world.spawn_entities(mask, instances);
                let last_entity_index = entities.len().saturating_sub(1);
                for (entity_index, entity) in entities.iter().enumerate() {
                    if entity_index == last_entity_index {
                        $(
                            $(#[$comp_attr])*
                            $crate::paste::paste! {
                                if let Some(component) = self.$name {
                                    world.[<set_$name>](*entity, component);
                                }
                            }
                        )*
                        break;
                    } else {
                        $(
                            $(#[$comp_attr])*
                            $crate::paste::paste! {
                                if let Some(ref component) = self.$name {
                                    world.[<set_$name>](*entity, component.clone());
                                }
                            }
                        )*
                    }
                }
                entities
            }
        }

        $crate::paste::paste! {
            pub enum Command {
                SpawnEntities { mask: u64, count: usize },
                DespawnEntity { entity: $crate::Entity },
                DespawnEntities { entities: Vec<$crate::Entity> },
                AddComponents { entity: $crate::Entity, mask: u64 },
                RemoveComponents { entity: $crate::Entity, mask: u64 },
                $(
                    $(#[$comp_attr])*
                    [<Set $mask:camel>] { entity: $crate::Entity, value: $type },
                )*
                $(
                    [<Add $tag_mask:camel>] { entity: $crate::Entity },
                    [<Remove $tag_mask:camel>] { entity: $crate::Entity },
                )*
            }
        }

        $crate::paste::paste! {
            #[allow(unused)]
            #[derive(Default)]
            pub struct $world {
                pub entity_locations: $crate::EntityLocations,
                pub tables: Vec<ComponentArrays>,
                pub allocator: $crate::EntityAllocator,
                pub resources: $resources,
                pub table_edges: Vec<$crate::ArchetypeEdges>,
                pub table_lookup: std::collections::HashMap<u64, usize>,
                pub query_cache: std::collections::HashMap<u64, Vec<usize>>,
                pub current_tick: u32,
                pub last_tick: u32,
                pub structural_log: Vec<$crate::StructuralChange>,
                pub structural_sequence: u64,
                $(pub $tag_name: $crate::SparseTagSet,)*
                pub command_buffer: Vec<Command>,
                $(pub $event_name: $crate::EventChannel<$event_type>,)*
            }
        }

        #[allow(unused)]
        impl $world {
            pub fn spawn_entities(&mut self, mask: u64, count: usize) -> Vec<$crate::Entity> {
                let mut allocator = std::mem::take(&mut self.allocator);
                let entities = self.spawn_entities_with(&mut allocator, mask, count);
                self.allocator = allocator;
                entities
            }

            pub fn spawn_batch<F>(&mut self, mask: u64, count: usize, init: F) -> Vec<$crate::Entity>
            where
                F: FnMut(&mut ComponentArrays, usize),
            {
                let mut allocator = std::mem::take(&mut self.allocator);
                let entities = self.spawn_batch_with(&mut allocator, mask, count, init);
                self.allocator = allocator;
                entities
            }

            pub fn despawn_entities(&mut self, entities: &[$crate::Entity]) -> Vec<$crate::Entity> {
                let mut allocator = std::mem::take(&mut self.allocator);
                let despawned = self.despawn_entities_with(&mut allocator, entities);
                self.allocator = allocator;
                $(
                    for &entity in &despawned {
                        self.$tag_name.remove(entity);
                    }
                )*
                despawned
            }

            pub fn is_alive(&self, entity: $crate::Entity) -> bool {
                self.allocator.is_alive(entity)
            }

            fn entity_matches_tags(&self, entity: $crate::Entity, include_tags: u64, exclude_tags: u64) -> bool {
                $(
                    if include_tags & $tag_mask != 0 && !self.$tag_name.contains(entity) {
                        return false;
                    }
                    if exclude_tags & $tag_mask != 0 && self.$tag_name.contains(entity) {
                        return false;
                    }
                )*
                let _ = (entity, include_tags, exclude_tags);
                true
            }

            /// Returns None when an included tag has no members, since nothing
            /// can match. Otherwise drops excluded tags whose sets are empty,
            /// so queries like "not DEAD" stay on the unfiltered fast path
            /// while no entity carries the tag.
            fn reduce_tag_masks(&self, tag_include: u64, tag_exclude: u64) -> Option<(u64, u64)> {
                let mut reduced_exclude = tag_exclude;
                $(
                    if tag_include & $tag_mask != 0 && self.$tag_name.is_empty() {
                        return None;
                    }
                    if reduced_exclude & $tag_mask != 0 && self.$tag_name.is_empty() {
                        reduced_exclude &= !$tag_mask;
                    }
                )*
                Some((tag_include, reduced_exclude))
            }
        }

        $crate::paste::paste! {
            #[allow(unused)]
            impl $world {
                #[inline]
                pub fn for_each<F>(&self, include: u64, exclude: u64, f: F)
                where
                    F: FnMut($crate::Entity, &ComponentArrays, usize),
                {
                    let component_include = include & !ALL_TAGS_MASK;
                    let component_exclude = exclude & !ALL_TAGS_MASK;
                    let Some((tag_include, tag_exclude)) =
                        self.reduce_tag_masks(include & ALL_TAGS_MASK, exclude & ALL_TAGS_MASK)
                    else {
                        return;
                    };

                    if tag_include == 0 && tag_exclude == 0 {
                        [<tables_for_each_ $world:snake>](
                            &self.tables,
                            &self.query_cache,
                            component_include,
                            component_exclude,
                            |_| true,
                            f,
                        );
                    } else {
                        [<tables_for_each_ $world:snake>](
                            &self.tables,
                            &self.query_cache,
                            component_include,
                            component_exclude,
                            |entity| self.entity_matches_tags(entity, tag_include, tag_exclude),
                            f,
                        );
                    }
                }

                #[inline]
                pub fn for_each_mut<F>(&mut self, include: u64, exclude: u64, f: F)
                where
                    F: FnMut($crate::Entity, &mut ComponentArrays, usize),
                {
                    let component_include = include & !ALL_TAGS_MASK;
                    let component_exclude = exclude & !ALL_TAGS_MASK;
                    let Some((tag_include, tag_exclude)) =
                        self.reduce_tag_masks(include & ALL_TAGS_MASK, exclude & ALL_TAGS_MASK)
                    else {
                        return;
                    };

                    if tag_include == 0 && tag_exclude == 0 {
                        [<tables_for_each_mut_ $world:snake>](
                            &mut self.tables,
                            &mut self.query_cache,
                            component_include,
                            component_exclude,
                            |_| true,
                            f,
                        );
                    } else {
                        let Self { tables, query_cache, $($tag_name,)* .. } = self;
                        $(let $tag_name = &*$tag_name;)*
                        [<tables_for_each_mut_ $world:snake>](
                            tables,
                            query_cache,
                            component_include,
                            component_exclude,
                            |entity| {
                                let _ = entity;
                                $(
                                    if tag_include & $tag_mask != 0 && !$tag_name.contains(entity) {
                                        return false;
                                    }
                                    if tag_exclude & $tag_mask != 0 && $tag_name.contains(entity) {
                                        return false;
                                    }
                                )*
                                true
                            },
                            f,
                        );
                    }
                }

                #[cfg(not(target_family = "wasm"))]
                #[inline]
                pub fn par_for_each_mut<F>(&mut self, include: u64, exclude: u64, f: F)
                where
                    F: Fn($crate::Entity, &mut ComponentArrays, usize) + Send + Sync,
                {
                    let component_include = include & !ALL_TAGS_MASK;
                    let component_exclude = exclude & !ALL_TAGS_MASK;
                    let Some((tag_include, tag_exclude)) =
                        self.reduce_tag_masks(include & ALL_TAGS_MASK, exclude & ALL_TAGS_MASK)
                    else {
                        return;
                    };

                    if tag_include == 0 && tag_exclude == 0 {
                        [<tables_par_for_each_mut_ $world:snake>](
                            &mut self.tables,
                            component_include,
                            component_exclude,
                            |_| true,
                            f,
                        );
                    } else {
                        let Self { tables, $($tag_name,)* .. } = self;
                        $(let $tag_name = &*$tag_name;)*
                        [<tables_par_for_each_mut_ $world:snake>](
                            tables,
                            component_include,
                            component_exclude,
                            |entity| {
                                let _ = entity;
                                $(
                                    if tag_include & $tag_mask != 0 && !$tag_name.contains(entity) {
                                        return false;
                                    }
                                    if tag_exclude & $tag_mask != 0 && $tag_name.contains(entity) {
                                        return false;
                                    }
                                )*
                                true
                            },
                            f,
                        );
                    }
                }

                #[inline]
                pub fn for_each_mut_changed<F>(&mut self, include: u64, exclude: u64, f: F)
                where
                    F: FnMut($crate::Entity, &mut ComponentArrays, usize),
                {
                    let since_tick = self.last_tick;
                    self.for_each_mut_changed_since(include, exclude, since_tick, f);
                }

                pub fn for_each_mut_changed_since<F>(&mut self, include: u64, exclude: u64, since_tick: u32, f: F)
                where
                    F: FnMut($crate::Entity, &mut ComponentArrays, usize),
                {
                    let component_include = include & !ALL_TAGS_MASK;
                    let component_exclude = exclude & !ALL_TAGS_MASK;
                    let Some((tag_include, tag_exclude)) =
                        self.reduce_tag_masks(include & ALL_TAGS_MASK, exclude & ALL_TAGS_MASK)
                    else {
                        return;
                    };

                    if tag_include == 0 && tag_exclude == 0 {
                        [<tables_for_each_mut_changed_ $world:snake>](
                            &mut self.tables,
                            &mut self.query_cache,
                            component_include,
                            component_exclude,
                            since_tick,
                            |_| true,
                            f,
                        );
                    } else {
                        let Self { tables, query_cache, $($tag_name,)* .. } = self;
                        $(let $tag_name = &*$tag_name;)*
                        [<tables_for_each_mut_changed_ $world:snake>](
                            tables,
                            query_cache,
                            component_include,
                            component_exclude,
                            since_tick,
                            |entity| {
                                let _ = entity;
                                $(
                                    if tag_include & $tag_mask != 0 && !$tag_name.contains(entity) {
                                        return false;
                                    }
                                    if tag_exclude & $tag_mask != 0 && $tag_name.contains(entity) {
                                        return false;
                                    }
                                )*
                                true
                            },
                            f,
                        );
                    }
                }

                $(
                    $(#[$comp_attr])*
                    /// Visits the component mutably for every entity matching
                    /// `mask` (components and tags), stamping change ticks.
                    /// Scans tables by mask rather than the query cache: the
                    /// per-entity tag checks here live inside a per-component
                    /// macro repetition, which rules out the borrow shapes the
                    /// cached paths use (see the note on `ecs_kernel_impl`).
                    pub fn [<query_ $name _mut>]<F>(&mut self, mask: u64, mut f: F)
                    where
                        F: FnMut($crate::Entity, &mut $type),
                    {
                        let component_include = (mask & !ALL_TAGS_MASK) | $mask;
                        let tag_include = mask & ALL_TAGS_MASK;
                        let current_tick = self.current_tick;

                        for table_index in 0..self.tables.len() {
                            if self.tables[table_index].mask & component_include != component_include {
                                continue;
                            }
                            if tag_include == 0 {
                                let table = &mut self.tables[table_index];
                                for index in 0..table.entity_indices.len() {
                                    let entity = table.entity_indices[index];
                                    table.[<$name _changed>][index] = current_tick;
                                    table.[<$name _peak_changed>] = current_tick;
                                    f(entity, &mut table.$name[index]);
                                }
                            } else {
                                for index in 0..self.tables[table_index].entity_indices.len() {
                                    let entity = self.tables[table_index].entity_indices[index];
                                    if !self.entity_matches_tags(entity, tag_include, 0) {
                                        continue;
                                    }
                                    let table = &mut self.tables[table_index];
                                    table.[<$name _changed>][index] = current_tick;
                                    table.[<$name _peak_changed>] = current_tick;
                                    f(entity, &mut table.$name[index]);
                                }
                            }
                        }
                    }

                    $(#[$comp_attr])*
                    pub fn [<iter_ $name _mut>]<F>(&mut self, mut f: F)
                    where
                        F: FnMut($crate::Entity, &mut $type),
                    {
                        self.[<query_ $name _mut>](0, |entity, component| f(entity, component));
                    }
                )*

                $(
                    pub fn [<add_ $tag_name>](&mut self, entity: $crate::Entity) {
                        if self.contains_entity(entity) && self.$tag_name.insert(entity) {
                            self.record_structural(entity, $crate::StructuralChangeKind::TagsAdded, $tag_mask);
                        }
                    }

                    pub fn [<remove_ $tag_name>](&mut self, entity: $crate::Entity) -> bool {
                        let removed = self.$tag_name.remove(entity);
                        if removed {
                            self.record_structural(entity, $crate::StructuralChangeKind::TagsRemoved, $tag_mask);
                        }
                        removed
                    }

                    pub fn [<has_ $tag_name>](&self, entity: $crate::Entity) -> bool {
                        self.$tag_name.contains(entity)
                    }

                    pub fn [<query_ $tag_name>](&self) -> impl Iterator<Item = $crate::Entity> + '_ {
                        self.$tag_name.iter()
                    }
                )*

                $(
                    pub fn [<send_ $event_name>](&mut self, event: $event_type) {
                        self.$event_name.send(event);
                    }

                    pub fn [<read_ $event_name>](&self) -> impl Iterator<Item = &$event_type> {
                        self.$event_name.read()
                    }

                    pub fn [<read_ $event_name _since>](&self, cursor: u64) -> &[$event_type] {
                        self.$event_name.events_since(cursor)
                    }

                    /// The exactly-once read: yields events sent after the
                    /// cursor and advances it past them, so a handler calling
                    /// this every frame sees each event once. Events stay
                    /// buffered for two frames, so `read_` and `collect_`
                    /// re-deliver on the second frame; keep one `u64` cursor
                    /// per consumer and reach for this by default.
                    pub fn [<consume_ $event_name>](&self, cursor: &mut u64) -> &[$event_type] {
                        self.$event_name.consume(cursor)
                    }

                    pub fn [<sequence_ $event_name>](&self) -> u64 {
                        self.$event_name.sequence()
                    }

                    pub fn [<trim_ $event_name>](&mut self, up_to_sequence: u64) {
                        self.$event_name.trim(up_to_sequence);
                    }

                    pub fn [<clear_ $event_name>](&mut self) {
                        self.$event_name.clear();
                    }

                    /// Expires this channel's events after their two-frame
                    /// window. `step()` already calls this once per frame for
                    /// every channel; calling both halves event lifetime, so
                    /// use this directly only when managing frame boundaries
                    /// yourself.
                    pub fn [<update_ $event_name>](&mut self) {
                        self.$event_name.update();
                    }

                    pub fn [<len_ $event_name>](&self) -> usize {
                        self.$event_name.len()
                    }

                    pub fn [<is_empty_ $event_name>](&self) -> bool {
                        self.$event_name.is_empty()
                    }

                    pub fn [<peek_ $event_name>](&self) -> Option<&$event_type> {
                        self.$event_name.peek()
                    }

                    pub fn [<collect_ $event_name>](&self) -> Vec<$event_type>
                    where
                        $event_type: Clone,
                    {
                        self.$event_name.read().cloned().collect()
                    }
                )*

                fn update_events(&mut self) {
                    $(
                        self.$event_name.update();
                    )*
                }

                pub fn step(&mut self) {
                    self.update_events();
                    self.last_tick = self.current_tick;
                    self.current_tick = self.current_tick.wrapping_add(1);
                }

                pub fn queue_spawn_entities(&mut self, mask: u64, count: usize) {
                    debug_assert_eq!(
                        mask & ALL_TAGS_MASK,
                        0,
                        "spawn masks must not contain tag bits; use queue_add_<tag> for tags"
                    );
                    self.command_buffer.push(Command::SpawnEntities { mask, count });
                }

                pub fn queue_despawn_entities(&mut self, entities: Vec<$crate::Entity>) {
                    self.command_buffer.push(Command::DespawnEntities { entities });
                }

                pub fn queue_despawn_entity(&mut self, entity: $crate::Entity) {
                    self.command_buffer.push(Command::DespawnEntity { entity });
                }

                pub fn queue_add_components(&mut self, entity: $crate::Entity, mask: u64) {
                    debug_assert_eq!(
                        mask & ALL_TAGS_MASK,
                        0,
                        "component masks must not contain tag bits; use queue_add_<tag> for tags"
                    );
                    self.command_buffer.push(Command::AddComponents { entity, mask });
                }

                pub fn queue_remove_components(&mut self, entity: $crate::Entity, mask: u64) {
                    debug_assert_eq!(
                        mask & ALL_TAGS_MASK,
                        0,
                        "component masks must not contain tag bits; use queue_remove_<tag> for tags"
                    );
                    self.command_buffer.push(Command::RemoveComponents { entity, mask });
                }

                $(
                    $(#[$comp_attr])*
                    pub fn [<queue_set_ $name>](&mut self, entity: $crate::Entity, value: $type) {
                        self.command_buffer.push(Command::[<Set $mask:camel>] { entity, value });
                    }
                )*

                $(
                    pub fn [<queue_add_ $tag_name>](&mut self, entity: $crate::Entity) {
                        self.command_buffer.push(Command::[<Add $tag_mask:camel>] { entity });
                    }

                    pub fn [<queue_remove_ $tag_name>](&mut self, entity: $crate::Entity) {
                        self.command_buffer.push(Command::[<Remove $tag_mask:camel>] { entity });
                    }
                )*

                pub fn apply_commands(&mut self) {
                    let commands = std::mem::take(&mut self.command_buffer);

                    for command in commands {
                        match command {
                            Command::SpawnEntities { mask, count } => {
                                self.spawn_entities(mask, count);
                            }
                            Command::DespawnEntity { entity } => {
                                self.despawn_entities(&[entity]);
                            }
                            Command::DespawnEntities { entities } => {
                                self.despawn_entities(&entities);
                            }
                            Command::AddComponents { entity, mask } => {
                                self.add_components(entity, mask);
                            }
                            Command::RemoveComponents { entity, mask } => {
                                self.remove_components(entity, mask);
                            }
                            $(
                                $(#[$comp_attr])*
                                Command::[<Set $mask:camel>] { entity, value } => {
                                    self.[<set_ $name>](entity, value);
                                }
                            )*
                            $(
                                Command::[<Add $tag_mask:camel>] { entity } => {
                                    self.[<add_ $tag_name>](entity);
                                }
                                Command::[<Remove $tag_mask:camel>] { entity } => {
                                    self.[<remove_ $tag_name>](entity);
                                }
                            )*
                        }
                    }
                }

                pub fn command_count(&self) -> usize {
                    self.command_buffer.len()
                }

                pub fn clear_commands(&mut self) {
                    self.command_buffer.clear();
                }
            }
        }

        #[derive(Default)]
        pub struct $resources {
            $($(#[$attr])* pub $resource_name: $resource_type,)*
        }
    };
}

#[macro_export]
macro_rules! ecs_multi_impl {
    (
        $ecs:ident {
            $($world_name:ident {
                $($(#[$comp_attr:meta])* $name:ident: $type:ty => $mask:ident),* $(,)?
            })+
        }
        Tags {
            $($tag_name:ident => $tag_mask:ident),* $(,)?
        }
        Events {
            $($event_name:ident: $event_type:ty),* $(,)?
        }
        $resources:ident {
            $($(#[$attr:meta])* $resource_name:ident: $resource_type:ty),* $(,)?
        }
    ) => {
        $crate::paste::paste! {
            #[allow(non_camel_case_types)]
            #[allow(unused)]
            pub enum [<$ecs Tag>] {
                $($tag_name,)*
            }

            $(pub const $tag_mask: u64 = 1 << (63 - ([<$ecs Tag>]::$tag_name as u64));)*

            #[allow(unused)]
            pub const ALL_TAGS_MASK: u64 = 0 $(| $tag_mask)*;

            #[allow(unused)]
            pub const [<$ecs:snake:upper _TAG_COUNT>]: usize = {
                let mut count = 0;
                $(count += 1; let _ = [<$ecs Tag>]::$tag_name;)*
                count
            };
        }

        $(
            $crate::ecs_kernel_impl! {
                $world_name,
                true,
                { $($(#[$comp_attr])* $name: $type => $mask),* }
            }

            $crate::paste::paste! {
                const _: () = assert!(
                    [<$world_name:snake:upper _COMPONENT_COUNT>] + [<$ecs:snake:upper _TAG_COUNT>] <= 64,
                    "components plus tags must fit in a u64 mask"
                );

                #[allow(unused)]
                #[derive(Default)]
                pub struct $world_name {
                    pub entity_locations: $crate::EntityLocations,
                    pub tables: Vec<[<$world_name ComponentArrays>]>,
                    pub table_edges: Vec<$crate::ArchetypeEdges>,
                    pub table_lookup: std::collections::HashMap<u64, usize>,
                    pub query_cache: std::collections::HashMap<u64, Vec<usize>>,
                    pub current_tick: u32,
                    pub last_tick: u32,
                    pub structural_log: Vec<$crate::StructuralChange>,
                    pub structural_sequence: u64,
                }

                #[allow(unused)]
                impl $world_name {
                    pub fn spawn_entities(
                        &mut self,
                        allocator: &mut $crate::EntityAllocator,
                        mask: u64,
                        count: usize,
                    ) -> Vec<$crate::Entity> {
                        self.spawn_entities_with(allocator, mask, count)
                    }

                    pub fn spawn_batch<F>(
                        &mut self,
                        allocator: &mut $crate::EntityAllocator,
                        mask: u64,
                        count: usize,
                        init: F,
                    ) -> Vec<$crate::Entity>
                    where
                        F: FnMut(&mut [<$world_name ComponentArrays>], usize),
                    {
                        self.spawn_batch_with(allocator, mask, count, init)
                    }

                    #[inline]
                    pub fn for_each<F>(&self, include: u64, exclude: u64, f: F)
                    where
                        F: FnMut($crate::Entity, &[<$world_name ComponentArrays>], usize),
                    {
                        debug_assert_eq!(
                            include & ![<$world_name:snake:upper _ALL_COMPONENTS>],
                            0,
                            "per-world queries take component masks only; use for_each_with_tags for tag filtering"
                        );
                        [<tables_for_each_ $world_name:snake>](
                            &self.tables,
                            &self.query_cache,
                            include,
                            exclude,
                            |_| true,
                            f,
                        );
                    }

                    #[inline]
                    pub fn for_each_mut<F>(&mut self, include: u64, exclude: u64, f: F)
                    where
                        F: FnMut($crate::Entity, &mut [<$world_name ComponentArrays>], usize),
                    {
                        debug_assert_eq!(
                            include & ![<$world_name:snake:upper _ALL_COMPONENTS>],
                            0,
                            "per-world queries take component masks only; use for_each_mut_with_tags for tag filtering"
                        );
                        [<tables_for_each_mut_ $world_name:snake>](
                            &mut self.tables,
                            &mut self.query_cache,
                            include,
                            exclude,
                            |_| true,
                            f,
                        );
                    }

                    #[cfg(not(target_family = "wasm"))]
                    #[inline]
                    pub fn par_for_each_mut<F>(&mut self, include: u64, exclude: u64, f: F)
                    where
                        F: Fn($crate::Entity, &mut [<$world_name ComponentArrays>], usize) + Send + Sync,
                    {
                        debug_assert_eq!(
                            include & ![<$world_name:snake:upper _ALL_COMPONENTS>],
                            0,
                            "per-world queries take component masks only; use par_for_each_mut_with_tags for tag filtering"
                        );
                        [<tables_par_for_each_mut_ $world_name:snake>](
                            &mut self.tables,
                            include,
                            exclude,
                            |_| true,
                            f,
                        );
                    }

                    #[inline]
                    pub fn for_each_mut_changed<F>(&mut self, include: u64, exclude: u64, f: F)
                    where
                        F: FnMut($crate::Entity, &mut [<$world_name ComponentArrays>], usize),
                    {
                        let since_tick = self.last_tick;
                        self.for_each_mut_changed_since(include, exclude, since_tick, f);
                    }

                    pub fn for_each_mut_changed_since<F>(&mut self, include: u64, exclude: u64, since_tick: u32, f: F)
                    where
                        F: FnMut($crate::Entity, &mut [<$world_name ComponentArrays>], usize),
                    {
                        debug_assert_eq!(
                            include & ![<$world_name:snake:upper _ALL_COMPONENTS>],
                            0,
                            "per-world changed queries take component masks only"
                        );
                        [<tables_for_each_mut_changed_ $world_name:snake>](
                            &mut self.tables,
                            &mut self.query_cache,
                            include,
                            exclude,
                            since_tick,
                            |_| true,
                            f,
                        );
                    }

                    $(
                        $(#[$comp_attr])*
                        pub fn [<query_ $name _mut>]<F>(&mut self, mask: u64, mut f: F)
                        where
                            F: FnMut($crate::Entity, &mut $type),
                        {
                            debug_assert_eq!(
                                mask & ![<$world_name:snake:upper _ALL_COMPONENTS>],
                                0,
                                "per-world queries take component masks only"
                            );
                            let current_tick = self.current_tick;
                            [<tables_for_each_mut_ $world_name:snake>](
                                &mut self.tables,
                                &mut self.query_cache,
                                mask | $mask,
                                0,
                                |_| true,
                                |entity, table, index| {
                                    table.[<$name _changed>][index] = current_tick;
                                    table.[<$name _peak_changed>] = current_tick;
                                    f(entity, &mut table.$name[index]);
                                },
                            );
                        }

                        $(#[$comp_attr])*
                        pub fn [<iter_ $name _mut>]<F>(&mut self, mut f: F)
                        where
                            F: FnMut($crate::Entity, &mut $type),
                        {
                            self.[<query_ $name _mut>](0, |entity, component| f(entity, component));
                        }
                    )*
                }
            }
        )+

        #[derive(Default)]
        pub struct $resources {
            $($(#[$attr])* pub $resource_name: $resource_type,)*
        }

        $crate::paste::paste! {
            #[allow(unused)]
            #[derive(Default, Clone)]
            pub struct EntityBuilder {
                $($(
                    $(#[$comp_attr])* $name: Option<$type>,
                )*)+
            }

            #[allow(unused)]
            impl EntityBuilder {
                pub fn new() -> Self {
                    Self::default()
                }

                $($(
                    $(#[$comp_attr])*
                    pub fn [<with_ $name>](mut self, value: $type) -> Self {
                        self.$name = Some(value);
                        self
                    }
                )*)+

                pub fn spawn(self, ecs: &mut $ecs, instances: usize) -> Vec<$crate::Entity> {
                    let mut entities = Vec::with_capacity(instances);
                    for _ in 0..instances {
                        entities.push(ecs.spawn());
                    }

                    $(
                        {
                            let mut mask: u64 = 0;
                            $(
                                $(#[$comp_attr])*
                                if self.$name.is_some() {
                                    mask |= $mask;
                                }
                            )*
                            if mask != 0 {
                                let last_entity_index = entities.len().saturating_sub(1);
                                for (entity_index, entity) in entities.iter().enumerate() {
                                    ecs.[<$world_name:snake>].add_components(*entity, mask);
                                    if entity_index == last_entity_index {
                                        $(
                                            $(#[$comp_attr])*
                                            if let Some(component) = self.$name {
                                                ecs.[<$world_name:snake>].[<set_ $name>](*entity, component);
                                            }
                                        )*
                                        break;
                                    } else {
                                        $(
                                            $(#[$comp_attr])*
                                            if let Some(ref component) = self.$name {
                                                ecs.[<$world_name:snake>].[<set_ $name>](*entity, component.clone());
                                            }
                                        )*
                                    }
                                }
                            }
                        }
                    )+

                    entities
                }
            }

            pub enum Command {
                Spawn { count: usize },
                DespawnEntity { entity: $crate::Entity },
                Despawn { entities: Vec<$crate::Entity> },
                $($(
                    $(#[$comp_attr])*
                    [<$world_name Set $mask:camel>] { entity: $crate::Entity, value: $type },
                    $(#[$comp_attr])*
                    [<$world_name AddComponents $mask:camel>] { entity: $crate::Entity },
                    $(#[$comp_attr])*
                    [<$world_name RemoveComponents $mask:camel>] { entity: $crate::Entity },
                )*)+
                $(
                    [<Add $tag_mask:camel>] { entity: $crate::Entity },
                    [<Remove $tag_mask:camel>] { entity: $crate::Entity },
                )*
            }

            #[allow(unused)]
            #[derive(Default)]
            pub struct $ecs {
                $(pub [<$world_name:snake>]: $world_name,)+
                pub allocator: $crate::EntityAllocator,
                pub resources: $resources,
                $(pub $tag_name: $crate::SparseTagSet,)*
                pub command_buffer: Vec<Command>,
                $(pub $event_name: $crate::EventChannel<$event_type>,)*
                pub structural_log: Vec<$crate::StructuralChange>,
                pub structural_sequence: u64,
            }

            #[allow(unused)]
            impl $ecs {
                pub fn spawn(&mut self) -> $crate::Entity {
                    let entity = self.allocator.allocate();
                    self.record_structural(entity, $crate::StructuralChangeKind::Spawned, 0);
                    entity
                }

                pub fn spawn_count(&mut self, count: usize) -> Vec<$crate::Entity> {
                    let mut entities = Vec::new();
                    self.allocator.allocate_batch(count, &mut entities);
                    for index in 0..entities.len() {
                        let entity = entities[index];
                        self.record_structural(entity, $crate::StructuralChangeKind::Spawned, 0);
                    }
                    entities
                }

                pub fn is_alive(&self, entity: $crate::Entity) -> bool {
                    self.allocator.is_alive(entity)
                }

                /// Despawns the entity across every world, dropping its tags.
                /// Returns false for stale or already-despawned handles, which
                /// are left untouched. The per-world retirement broadcast
                /// grows each world's location table to cover the despawned
                /// id, 16 bytes per id per world even in worlds that never
                /// stored the entity; that footprint is what makes stale
                /// writes refusable everywhere.
                pub fn despawn(&mut self, entity: $crate::Entity) -> bool {
                    if !self.allocator.deallocate(entity) {
                        return false;
                    }
                    $(self.[<$world_name:snake>].retire_entity(entity);)+
                    $(self.$tag_name.remove(entity);)*
                    self.record_structural(entity, $crate::StructuralChangeKind::Despawned, 0);
                    true
                }

                pub fn despawn_entities(&mut self, entities: &[$crate::Entity]) {
                    for &entity in entities {
                        self.despawn(entity);
                    }
                }

                fn record_structural(&mut self, entity: $crate::Entity, kind: $crate::StructuralChangeKind, mask: u64) {
                    if self.structural_log.len() >= $crate::STRUCTURAL_LOG_CAPACITY {
                        self.structural_log.clear();
                    }
                    self.structural_sequence += 1;
                    self.structural_log.push($crate::StructuralChange {
                        sequence: self.structural_sequence,
                        entity,
                        kind,
                        mask,
                    });
                }

                pub fn structural_sequence(&self) -> u64 {
                    self.structural_sequence
                }

                /// The ECS-level lifecycle log: handle allocation (`Spawned`
                /// with mask 0), handle death (`Despawned` with mask 0), and
                /// tag flips. Row-level history lives in each world's own log,
                /// where an entity is `Spawned` with a component mask when its
                /// first components arrive there. Sync world contents from the
                /// world logs and handle lifetime or tags from this one; a
                /// consumer merging both will see one entity spawn twice.
                pub fn structural_changes_since(&self, cursor: u64) -> &[$crate::StructuralChange] {
                    let start = self.structural_log.partition_point(|change| change.sequence <= cursor);
                    &self.structural_log[start..]
                }

                pub fn trim_structural_log(&mut self, up_to_sequence: u64) {
                    let end = self.structural_log.partition_point(|change| change.sequence <= up_to_sequence);
                    self.structural_log.drain(..end);
                }

                pub fn clear_structural_log(&mut self) {
                    self.structural_log.clear();
                }

                $(
                    pub fn [<add_ $tag_name>](&mut self, entity: $crate::Entity) {
                        if self.allocator.is_alive(entity) && self.$tag_name.insert(entity) {
                            self.record_structural(entity, $crate::StructuralChangeKind::TagsAdded, $tag_mask);
                        }
                    }

                    pub fn [<remove_ $tag_name>](&mut self, entity: $crate::Entity) -> bool {
                        let removed = self.$tag_name.remove(entity);
                        if removed {
                            self.record_structural(entity, $crate::StructuralChangeKind::TagsRemoved, $tag_mask);
                        }
                        removed
                    }

                    pub fn [<has_ $tag_name>](&self, entity: $crate::Entity) -> bool {
                        self.$tag_name.contains(entity)
                    }

                    pub fn [<query_ $tag_name>](&self) -> impl Iterator<Item = $crate::Entity> + '_ {
                        self.$tag_name.iter()
                    }
                )*

                $(
                    pub fn [<send_ $event_name>](&mut self, event: $event_type) {
                        self.$event_name.send(event);
                    }

                    pub fn [<read_ $event_name>](&self) -> impl Iterator<Item = &$event_type> {
                        self.$event_name.read()
                    }

                    pub fn [<read_ $event_name _since>](&self, cursor: u64) -> &[$event_type] {
                        self.$event_name.events_since(cursor)
                    }

                    /// The exactly-once read: yields events sent after the
                    /// cursor and advances it past them, so a handler calling
                    /// this every frame sees each event once. Events stay
                    /// buffered for two frames, so `read_` and `collect_`
                    /// re-deliver on the second frame; keep one `u64` cursor
                    /// per consumer and reach for this by default.
                    pub fn [<consume_ $event_name>](&self, cursor: &mut u64) -> &[$event_type] {
                        self.$event_name.consume(cursor)
                    }

                    pub fn [<sequence_ $event_name>](&self) -> u64 {
                        self.$event_name.sequence()
                    }

                    pub fn [<trim_ $event_name>](&mut self, up_to_sequence: u64) {
                        self.$event_name.trim(up_to_sequence);
                    }

                    pub fn [<clear_ $event_name>](&mut self) {
                        self.$event_name.clear();
                    }

                    /// Expires this channel's events after their two-frame
                    /// window. `step()` already calls this once per frame for
                    /// every channel; calling both halves event lifetime, so
                    /// use this directly only when managing frame boundaries
                    /// yourself.
                    pub fn [<update_ $event_name>](&mut self) {
                        self.$event_name.update();
                    }

                    pub fn [<len_ $event_name>](&self) -> usize {
                        self.$event_name.len()
                    }

                    pub fn [<is_empty_ $event_name>](&self) -> bool {
                        self.$event_name.is_empty()
                    }

                    pub fn [<peek_ $event_name>](&self) -> Option<&$event_type> {
                        self.$event_name.peek()
                    }

                    pub fn [<collect_ $event_name>](&self) -> Vec<$event_type>
                    where
                        $event_type: Clone,
                    {
                        self.$event_name.read().cloned().collect()
                    }
                )*

                fn update_events(&mut self) {
                    $(
                        self.$event_name.update();
                    )*
                }

                pub fn step(&mut self) {
                    self.update_events();
                    $(
                        self.[<$world_name:snake>].increment_tick();
                    )+
                }

                pub fn queue_spawn(&mut self, count: usize) {
                    self.command_buffer.push(Command::Spawn { count });
                }

                pub fn queue_despawn_entity(&mut self, entity: $crate::Entity) {
                    self.command_buffer.push(Command::DespawnEntity { entity });
                }

                pub fn queue_despawn_entities(&mut self, entities: Vec<$crate::Entity>) {
                    self.command_buffer.push(Command::Despawn { entities });
                }

                $($(
                    $(#[$comp_attr])*
                    pub fn [<queue_set_ $name>](&mut self, entity: $crate::Entity, value: $type) {
                        self.command_buffer.push(Command::[<$world_name Set $mask:camel>] { entity, value });
                    }

                    $(#[$comp_attr])*
                    pub fn [<queue_add_ $name>](&mut self, entity: $crate::Entity) {
                        self.command_buffer.push(Command::[<$world_name AddComponents $mask:camel>] { entity });
                    }

                    $(#[$comp_attr])*
                    pub fn [<queue_remove_ $name>](&mut self, entity: $crate::Entity) {
                        self.command_buffer.push(Command::[<$world_name RemoveComponents $mask:camel>] { entity });
                    }
                )*)+

                $(
                    pub fn [<queue_add_ $tag_name>](&mut self, entity: $crate::Entity) {
                        self.command_buffer.push(Command::[<Add $tag_mask:camel>] { entity });
                    }

                    pub fn [<queue_remove_ $tag_name>](&mut self, entity: $crate::Entity) {
                        self.command_buffer.push(Command::[<Remove $tag_mask:camel>] { entity });
                    }
                )*

                pub fn apply_commands(&mut self) {
                    let commands = std::mem::take(&mut self.command_buffer);
                    for command in commands {
                        match command {
                            Command::Spawn { count } => {
                                self.spawn_count(count);
                            }
                            Command::DespawnEntity { entity } => {
                                self.despawn(entity);
                            }
                            Command::Despawn { entities } => {
                                self.despawn_entities(&entities);
                            }
                            $($(
                                $(#[$comp_attr])*
                                Command::[<$world_name Set $mask:camel>] { entity, value } => {
                                    self.[<$world_name:snake>].[<set_ $name>](entity, value);
                                }
                                $(#[$comp_attr])*
                                Command::[<$world_name AddComponents $mask:camel>] { entity } => {
                                    self.[<$world_name:snake>].add_components(entity, $mask);
                                }
                                $(#[$comp_attr])*
                                Command::[<$world_name RemoveComponents $mask:camel>] { entity } => {
                                    self.[<$world_name:snake>].remove_components(entity, $mask);
                                }
                            )*)+
                            $(
                                Command::[<Add $tag_mask:camel>] { entity } => {
                                    self.[<add_ $tag_name>](entity);
                                }
                                Command::[<Remove $tag_mask:camel>] { entity } => {
                                    self.[<remove_ $tag_name>](entity);
                                }
                            )*
                        }
                    }
                }

                pub fn command_count(&self) -> usize {
                    self.command_buffer.len()
                }

                pub fn clear_commands(&mut self) {
                    self.command_buffer.clear();
                }
            }
        }
    };
}
#[macro_export]
macro_rules! table_has_components {
    ($table:expr, $mask:expr) => {
        $table.mask & $mask == $mask
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    ecs! {
        World {
            position: Position => POSITION,
            velocity: Velocity => VELOCITY,
            health: Health => HEALTH,
            parent: Parent => PARENT,
            node: Node => NODE,
        }
        Tags {
            player => PLAYER,
            enemy => ENEMY,
            active => ACTIVE,
        }
        Events {
            ping: PingEvent,
        }
        Resources {
            _delta_time: f32,
        }
    }

    #[derive(Debug, Clone)]
    pub struct PingEvent {
        pub value: u32,
    }

    use components::*;
    mod components {
        use super::*;

        #[derive(Default, Debug, Copy, Clone, PartialEq)]
        pub struct Parent(pub Entity);

        #[derive(Default, Debug, Clone, PartialEq)]
        pub struct Node {
            pub id: Entity,
            pub parent: Option<Entity>,
            pub children: Vec<Entity>,
        }

        #[derive(Default, Debug, Clone)]
        pub struct Position {
            pub x: f32,
            pub y: f32,
        }

        #[derive(Default, Debug, Clone)]
        pub struct Velocity {
            pub x: f32,
            pub y: f32,
        }

        #[derive(Default, Debug, Clone)]
        pub struct Health {
            pub value: f32,
        }
    }

    mod systems {
        use super::*;

        pub fn run_systems(world: &mut World, dt: f32) {
            world.tables.iter_mut().for_each(|table| {
                if super::table_has_components!(table, POSITION | VELOCITY | HEALTH) {
                    update_positions_system(&mut table.position, &table.velocity, dt);
                }
                if super::table_has_components!(table, HEALTH) {
                    health_system(&mut table.health);
                }
            });
        }

        #[inline]
        pub fn update_positions_system(
            positions: &mut [Position],
            velocities: &[Velocity],
            dt: f32,
        ) {
            positions
                .iter_mut()
                .zip(velocities.iter())
                .for_each(|(pos, vel)| {
                    pos.x += vel.x * dt;
                    pos.y += vel.y * dt;
                });
        }

        #[inline]
        pub fn health_system(health: &mut [Health]) {
            health.iter_mut().for_each(|health| {
                health.value *= 0.98;
            });
        }
    }

    fn setup_test_world() -> (World, Entity) {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION | VELOCITY, 1)[0];

        if let Some(pos) = world.get_position_mut(entity) {
            pos.x = 1.0;
            pos.y = 2.0;
        }
        if let Some(vel) = world.get_velocity_mut(entity) {
            vel.x = 3.0;
            vel.y = 4.0;
        }

        (world, entity)
    }

    #[test]
    fn test_spawn_entities() {
        let mut world = World::default();
        let entities = world.spawn_entities(POSITION | VELOCITY, 3);

        assert_eq!(entities.len(), 3);
        assert_eq!(world.get_all_entities().len(), 3);

        for entity in entities {
            assert!(world.get_position(entity).is_some());
            assert!(world.get_velocity(entity).is_some());
            assert!(world.get_health(entity).is_none());
        }
    }

    #[test]
    fn test_component_access() {
        let (mut world, entity) = setup_test_world();

        let pos = world.get_position(entity).unwrap();
        assert_eq!(pos.x, 1.0);
        assert_eq!(pos.y, 2.0);

        if let Some(pos) = world.get_position_mut(entity) {
            pos.x = 5.0;
        }

        let pos = world.get_position(entity).unwrap();
        assert_eq!(pos.x, 5.0);
    }

    #[test]
    fn test_modify_component() {
        let (mut world, entity) = setup_test_world();

        let pos = world.get_position(entity).unwrap();
        assert_eq!(pos.x, 1.0);
        assert_eq!(pos.y, 2.0);

        let old_x = world.modify_position(entity, |pos| {
            let old = pos.x;
            pos.x = 10.0;
            pos.y = 20.0;
            old
        });
        assert_eq!(old_x, Some(1.0));

        let pos = world.get_position(entity).unwrap();
        assert_eq!(pos.x, 10.0);
        assert_eq!(pos.y, 20.0);

        let invalid_entity = Entity {
            id: 9999,
            generation: 0,
        };
        let result = world.modify_position(invalid_entity, |pos| pos.x = 100.0);
        assert!(result.is_none());

        let entity_no_health = world.spawn_entities(POSITION, 1)[0];
        let result = world.modify_health(entity_no_health, |h| h.value = 50.0);
        assert!(result.is_none());
    }

    #[test]
    fn test_add_remove_components() {
        let (mut world, entity) = setup_test_world();

        assert!(world.get_health(entity).is_none());

        world.add_components(entity, HEALTH);
        assert!(world.get_health(entity).is_some());

        world.remove_components(entity, HEALTH);
        assert!(world.get_health(entity).is_none());
    }

    #[test]
    fn test_component_mask() {
        let (mut world, entity) = setup_test_world();

        let mask = world.component_mask(entity).unwrap();
        assert_eq!(mask, POSITION | VELOCITY);

        world.add_components(entity, HEALTH);
        let mask = world.component_mask(entity).unwrap();
        assert_eq!(mask, POSITION | VELOCITY | HEALTH);
    }

    #[test]
    fn test_query_entities() {
        let mut world = World::default();

        let e1 = world.spawn_entities(POSITION | VELOCITY, 1)[0];
        let _e2 = world.spawn_entities(POSITION | HEALTH, 1)[0];
        let e3 = world.spawn_entities(POSITION | VELOCITY | HEALTH, 1)[0];

        let pos_vel: Vec<_> = world.query_entities(POSITION | VELOCITY).collect();
        let pos_health: Vec<_> = world.query_entities(POSITION | HEALTH).collect();
        let all: Vec<_> = world.query_entities(POSITION | VELOCITY | HEALTH).collect();

        assert_eq!(pos_vel.len(), 2);
        assert_eq!(pos_health.len(), 2);
        assert_eq!(all.len(), 1);

        let pos_vel: HashSet<_> = pos_vel.into_iter().collect();
        assert!(pos_vel.contains(&e1));
        assert!(pos_vel.contains(&e3));

        assert_eq!(all[0], e3);
    }

    #[test]
    fn test_query_first_entity() {
        let mut world = World::default();

        let e1 = world.spawn_entities(POSITION | VELOCITY, 1)[0];
        let e2 = world.spawn_entities(POSITION | VELOCITY | HEALTH, 1)[0];

        let first = world.query_first_entity(POSITION | VELOCITY).unwrap();
        assert!(first == e1 || first == e2);

        assert!(world.query_first_entity(HEALTH).is_some());
        assert!(
            world
                .query_first_entity(POSITION | VELOCITY | HEALTH)
                .is_some()
        );
    }

    #[test]
    fn test_despawn_entities() {
        let mut world = World::default();

        let entities = world.spawn_entities(POSITION | VELOCITY, 3);
        assert_eq!(world.get_all_entities().len(), 3);

        let despawned = world.despawn_entities(&[entities[1]]);
        assert_eq!(despawned.len(), 1);
        assert_eq!(world.get_all_entities().len(), 2);

        assert!(world.get_position(entities[1]).is_none());

        assert!(world.get_position(entities[0]).is_some());
        assert!(world.get_position(entities[2]).is_some());
    }

    #[test]
    fn test_parallel_systems() {
        let mut world = World::default();

        let entity = world.spawn_entities(POSITION | VELOCITY | HEALTH, 1)[0];

        if let Some(pos) = world.get_position_mut(entity) {
            pos.x = 0.0;
            pos.y = 0.0;
        }
        if let Some(vel) = world.get_velocity_mut(entity) {
            vel.x = 1.0;
            vel.y = 1.0;
        }
        if let Some(health) = world.get_health_mut(entity) {
            health.value = 100.0;
        }

        systems::run_systems(&mut world, 1.0);

        let pos = world.get_position(entity).unwrap();
        let health = world.get_health(entity).unwrap();

        assert_eq!(pos.x, 1.0);
        assert_eq!(pos.y, 1.0);
        assert!(health.value < 100.0);
    }

    #[test]
    fn test_add_components() {
        let (mut world, entity) = setup_test_world();

        assert!(world.get_health(entity).is_none());

        world.add_components(entity, HEALTH);
        assert!(world.get_health(entity).is_some());

        world.remove_components(entity, HEALTH);
        assert!(world.get_health(entity).is_none());
    }

    #[test]
    fn test_multiple_component_addition() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION, 1)[0];

        world.add_components(entity, VELOCITY | HEALTH);

        assert!(world.get_position(entity).is_some());
        assert!(world.get_velocity(entity).is_some());
        assert!(world.get_health(entity).is_some());

        if let Some(pos) = world.get_position_mut(entity) {
            pos.x = 1.0;
        }
        world.add_components(entity, VELOCITY);
        assert_eq!(world.get_position(entity).unwrap().x, 1.0);
    }

    #[test]
    fn test_component_chain_addition() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION, 1)[0];

        if let Some(pos) = world.get_position_mut(entity) {
            pos.x = 1.0;
        }

        world.add_components(entity, VELOCITY);
        world.add_components(entity, HEALTH);

        assert_eq!(world.get_position(entity).unwrap().x, 1.0);
    }

    #[test]
    fn test_component_removal_order() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION | VELOCITY | HEALTH, 1)[0];

        world.remove_components(entity, VELOCITY);
        world.remove_components(entity, HEALTH);
        assert!(world.get_position(entity).is_some());
        assert!(world.get_velocity(entity).is_none());
        assert!(world.get_health(entity).is_none());
    }

    #[test]
    fn test_edge_cases() {
        let mut world = World::default();

        let empty = world.spawn_entities(0, 1)[0];

        world.add_components(empty, POSITION);
        assert!(world.get_position(empty).is_some());

        world.add_components(empty, POSITION);
        world.add_components(empty, POSITION);

        world.remove_components(empty, VELOCITY);

        world.remove_components(empty, POSITION);
        assert_eq!(world.component_mask(empty).unwrap(), 0);

        let invalid = Entity {
            id: 9999,
            generation: 0,
        };
        assert!(!world.add_components(invalid, POSITION));
    }

    #[test]
    fn test_component_data_integrity() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION | VELOCITY, 1)[0];

        {
            let pos = world.get_position_mut(entity).unwrap();
            pos.x = 1.0;
            pos.y = 2.0;
            let vel = world.get_velocity_mut(entity).unwrap();
            vel.x = 3.0;
            vel.y = 4.0;
        }

        world.add_components(entity, HEALTH);
        world.remove_components(entity, HEALTH);
        world.add_components(entity, HEALTH);

        let pos = world.get_position(entity).unwrap();
        let vel = world.get_velocity(entity).unwrap();
        assert_eq!(pos.x, 1.0);
        assert_eq!(pos.y, 2.0);
        assert_eq!(vel.x, 3.0);
        assert_eq!(vel.y, 4.0);
    }

    #[test]
    fn test_entity_references_through_moves() {
        let mut world = World::default();

        let entity1 = world.spawn_entities(POSITION, 1)[0];
        let entity2 = world.spawn_entities(POSITION, 1)[0];

        world.add_components(entity1, VELOCITY);
        if let Some(vel) = world.get_velocity_mut(entity1) {
            vel.x = entity2.id as f32;
        }

        world.add_components(entity2, VELOCITY | HEALTH);

        let stored_id = world.get_velocity(entity1).unwrap().x as u32;
        let entity2_loc = get_location_world(&world.entity_locations, entity2);
        assert!(entity2_loc.is_some());
        assert_eq!(stored_id, entity2.id);
    }

    #[test]
    fn test_table_cleanup_after_despawn() {
        let mut world = World::default();

        let e1 = world.spawn_entities(POSITION, 1)[0];
        let e2 = world.spawn_entities(POSITION | VELOCITY, 1)[0];

        let initial_tables = world.tables.len();
        assert_eq!(initial_tables, 2, "Should have two tables initially");

        world.despawn_entities(&[e2]);

        assert!(world.get_position(e2).is_none());
        assert!(world.get_velocity(e2).is_none());

        assert!(world.get_position(e1).is_some());

        let remaining: Vec<_> = world.query_entities(POSITION).collect();
        assert_eq!(remaining.len(), 1);
        assert!(remaining.contains(&e1));

        assert!(
            world.tables.len() <= initial_tables,
            "Should not have more tables than initial state"
        );

        for table in &world.tables {
            for &entity in &table.entity_indices {
                assert!(
                    get_location_world(&world.entity_locations, entity).is_some(),
                    "Entity location should be valid for remaining entities"
                );
            }
        }
    }

    #[test]
    fn test_concurrent_entity_references() {
        let mut world = World::default();

        let entity1 = world.spawn_entities(POSITION | HEALTH, 1)[0];
        let entity2 = world.spawn_entities(POSITION | HEALTH, 1)[0];

        if let Some(pos) = world.get_position_mut(entity1) {
            pos.x = 1.0;
        }
        if let Some(health) = world.get_health_mut(entity1) {
            health.value = 100.0;
        }

        let id1 = entity1.id;

        world.despawn_entities(&[entity1]);

        let entity3 = world.spawn_entities(POSITION | HEALTH, 1)[0];
        assert_eq!(entity3.id, id1, "Should reuse entity1's ID");
        assert_eq!(
            entity3.generation,
            entity1.generation + 1,
            "Should have incremented generation"
        );

        if let Some(pos) = world.get_position_mut(entity3) {
            pos.x = 3.0;
        }
        if let Some(health) = world.get_health_mut(entity3) {
            health.value = 50.0;
        }

        if let Some(pos) = world.get_position(entity2) {
            assert_eq!(pos.x, 0.0, "Entity2's data should be unchanged");
        }

        if let Some(pos) = world.get_position(entity3) {
            assert_eq!(pos.x, 3.0, "Should get entity3's data, not entity1's");
        }
        assert!(
            world.get_position(entity1).is_none(),
            "Should not be able to access entity1's old data"
        );
    }

    #[test]
    fn test_generational_indices_aba() {
        let mut world = World::default();

        let entity_a1 = world.spawn_entities(POSITION, 1)[0];
        assert_eq!(
            entity_a1.generation, 0,
            "First use of ID should have generation 0"
        );

        if let Some(pos) = world.get_position_mut(entity_a1) {
            pos.x = 1.0;
            pos.y = 1.0;
        }

        let id = entity_a1.id;

        world.despawn_entities(&[entity_a1]);

        let entity_a2 = world.spawn_entities(POSITION, 1)[0];
        assert_eq!(entity_a2.id, id, "Should reuse the same ID");
        assert_eq!(
            entity_a2.generation, 1,
            "Second use of ID should have generation 1"
        );

        if let Some(pos) = world.get_position_mut(entity_a2) {
            pos.x = 2.0;
            pos.y = 2.0;
        }

        assert!(
            world.get_position(entity_a1).is_none(),
            "Old reference to entity should be invalid"
        );

        world.despawn_entities(&[entity_a2]);

        let entity_a3 = world.spawn_entities(POSITION, 1)[0];
        assert_eq!(entity_a3.id, id, "Should reuse the same ID again");
        assert_eq!(
            entity_a3.generation, 2,
            "Third use of ID should have generation 2"
        );

        if let Some(pos) = world.get_position_mut(entity_a3) {
            pos.x = 3.0;
            pos.y = 3.0;
        }

        assert!(
            world.get_position(entity_a1).is_none(),
            "First generation reference should be invalid"
        );
        assert!(
            world.get_position(entity_a2).is_none(),
            "Second generation reference should be invalid"
        );

        let pos = world.get_position(entity_a3);
        assert!(
            pos.is_some(),
            "Current generation reference should be valid"
        );
        let pos = pos.unwrap();
        assert_eq!(pos.x, 3.0, "Should have the current generation's data");
        assert_eq!(pos.y, 3.0, "Should have the current generation's data");
    }

    #[test]
    fn test_all_entities() {
        let mut world = World::default();

        let e1 = world.spawn_entities(POSITION, 1)[0];
        let e2 = world.spawn_entities(POSITION | VELOCITY, 1)[0];
        let e3 = world.spawn_entities(POSITION | HEALTH, 1)[0];
        let e4 = world.spawn_entities(POSITION | VELOCITY | HEALTH, 1)[0];

        let all = world.get_all_entities();

        assert_eq!(all.len(), 4, "Should have 4 total entities");

        assert!(all.contains(&e1), "Missing entity 1");
        assert!(all.contains(&e2), "Missing entity 2");
        assert!(all.contains(&e3), "Missing entity 3");
        assert!(all.contains(&e4), "Missing entity 4");

        world.despawn_entities(&[e2, e3]);
        let remaining = world.get_all_entities();

        assert_eq!(remaining.len(), 2, "Should have 2 entities after despawn");

        assert!(remaining.contains(&e1), "Missing entity 1 after despawn");
        assert!(remaining.contains(&e4), "Missing entity 4 after despawn");
        assert!(!remaining.contains(&e2), "Entity 2 should be despawned");
        assert!(!remaining.contains(&e3), "Entity 3 should be despawned");
    }

    #[test]
    fn test_all_entities_empty_world() {
        assert!(
            World::default().get_all_entities().is_empty(),
            "Empty world should return empty vector"
        );
    }

    #[test]
    fn test_all_entities_after_table_merges() {
        let mut world = World::default();

        let e1 = world.spawn_entities(POSITION, 1)[0];
        let e2 = world.spawn_entities(VELOCITY, 1)[0];

        world.add_components(e1, VELOCITY);
        world.add_components(e2, POSITION);

        let all = world.get_all_entities();
        assert_eq!(
            all.len(),
            2,
            "Should maintain all entities through table merges"
        );
        assert!(all.contains(&e1), "Should contain first entity after merge");
        assert!(
            all.contains(&e2),
            "Should contain second entity after merge"
        );
    }

    #[test]
    fn test_table_transitions() {
        let mut world = World::default();

        let entity = world.spawn_entities(POSITION | VELOCITY | HEALTH, 1)[0];

        println!("Initial mask: {:b}", world.component_mask(entity).unwrap());

        let (old_table_idx, _) = get_location_world(&world.entity_locations, entity).unwrap();

        world.add_components(entity, POSITION);

        let final_mask = world.component_mask(entity).unwrap();
        println!("Final mask: {:b}", final_mask);
        let (new_table_idx, _) = get_location_world(&world.entity_locations, entity).unwrap();

        println!(
            "Old table index: {}, New table index: {}",
            old_table_idx, new_table_idx
        );
        println!("Tables after operation:");
        for (index, table) in world.tables.iter().enumerate() {
            println!("Table {}: mask={:b}", index, table.mask);
        }

        assert_eq!(
            final_mask & (POSITION | VELOCITY | HEALTH),
            POSITION | VELOCITY | HEALTH,
            "Entity should still have all original components"
        );
    }

    #[test]
    fn test_real_camera_scenario() {
        let mut world = World::default();

        let entity = world.spawn_entities(POSITION | VELOCITY | HEALTH, 1)[0];

        let query_results: Vec<_> = world.query_entities(POSITION | VELOCITY).collect();
        assert!(
            query_results.contains(&entity),
            "Initial query should match\n\
                Entity mask: {:b}\n\
                Query mask: {:b}",
            world.component_mask(entity).unwrap(),
            POSITION | VELOCITY
        );

        world.add_components(entity, HEALTH);

        let query_results: Vec<_> = world.query_entities(POSITION | VELOCITY).collect();
        assert!(
            query_results.contains(&entity),
            "Query should still match after adding component\n\
                Entity mask: {:b}\n\
                Query mask: {:b}",
            world.component_mask(entity).unwrap(),
            POSITION | VELOCITY
        );
    }

    #[test]
    fn test_query_consistency() {
        let mut world = World::default();

        let entity = world.spawn_entities(POSITION | VELOCITY | HEALTH, 1)[0];

        let query_mask = POSITION | VELOCITY;

        let query_results: Vec<_> = world.query_entities(query_mask).collect();
        assert!(
            query_results.contains(&entity),
            "query_entities should find entity with mask {:b} when querying for {:b}",
            world.component_mask(entity).unwrap(),
            query_mask
        );

        let first_result = world.query_first_entity(query_mask);
        assert!(
            first_result.is_some(),
            "query_first_entity should find entity with mask {:b} when querying for {:b}",
            world.component_mask(entity).unwrap(),
            query_mask
        );
        assert_eq!(
            first_result.unwrap(),
            entity,
            "query_first_entity should find same entity as query_entities"
        );

        world.add_components(entity, HEALTH);

        let query_results: Vec<_> = world.query_entities(query_mask).collect();
        assert!(
            query_results.contains(&entity),
            "query_entities should still find entity after adding component\n\
            Entity mask: {:b}\n\
            Query mask: {:b}",
            world.component_mask(entity).unwrap(),
            query_mask
        );

        let first_result = world.query_first_entity(query_mask);
        assert!(
            first_result.is_some(),
            "query_first_entity should still find entity after adding component\n\
            Entity mask: {:b}\n\
            Query mask: {:b}",
            world.component_mask(entity).unwrap(),
            query_mask
        );
    }

    #[test]
    fn entity_has_components_test() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION | VELOCITY, 1)[0];
        assert!(world.entity_has_components(entity, POSITION | VELOCITY));
        assert!(!world.entity_has_components(entity, HEALTH));
    }

    #[test]
    fn test_set_component() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION, 1)[0];
        world.set_position(entity, Position { x: 1.0, y: 2.0 });
        assert_eq!(world.get_position(entity).unwrap().x, 1.0);
        assert_eq!(world.get_position(entity).unwrap().y, 2.0);

        world.set_position(entity, Position { x: 3.0, y: 4.0 });
        assert_eq!(world.get_position(entity).unwrap().x, 3.0);
        assert_eq!(world.get_position(entity).unwrap().y, 4.0);
    }

    #[test]
    fn test_entity_builder() {
        let mut world = World::default();
        let entities = EntityBuilder::new()
            .with_position(Position { x: 1.0, y: 2.0 })
            .spawn(&mut world, 2);
        assert_eq!(world.get_position(entities[0]).unwrap().x, 1.0);
        assert_eq!(world.get_position(entities[1]).unwrap().y, 2.0);
    }

    #[test]
    fn test_query_composition_exclude() {
        let mut world = World::default();

        let e1 = world.spawn_entities(POSITION | VELOCITY, 1)[0];
        let e2 = world.spawn_entities(POSITION | VELOCITY | HEALTH, 1)[0];
        let e3 = world.spawn_entities(POSITION, 1)[0];

        let mut count = 0;
        world.for_each_mut(POSITION | VELOCITY, HEALTH, |_entity, _table, _idx| {
            count += 1;
        });

        assert_eq!(count, 1);

        let mut found_entities = Vec::new();
        world.for_each_mut(POSITION | VELOCITY, HEALTH, |entity, _table, _idx| {
            found_entities.push(entity);
        });

        assert!(found_entities.contains(&e1));
        assert!(!found_entities.contains(&e2));
        assert!(!found_entities.contains(&e3));
    }

    #[test]
    fn test_query_composition_include_only() {
        let mut world = World::default();

        let e1 = world.spawn_entities(POSITION, 1)[0];
        let e2 = world.spawn_entities(POSITION | VELOCITY, 1)[0];
        let e3 = world.spawn_entities(POSITION | VELOCITY | HEALTH, 1)[0];

        let mut count = 0;
        world.for_each_mut(POSITION | VELOCITY, 0, |_entity, _table, _idx| {
            count += 1;
        });

        assert_eq!(count, 2);

        let mut found_entities = Vec::new();
        world.for_each_mut(POSITION | VELOCITY, 0, |entity, _table, _idx| {
            found_entities.push(entity);
        });

        assert!(!found_entities.contains(&e1));
        assert!(found_entities.contains(&e2));
        assert!(found_entities.contains(&e3));
    }

    #[test]
    fn test_change_detection_basic() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION | VELOCITY, 1)[0];

        world.step();

        world.get_position_mut(entity).unwrap().x = 10.0;

        let mut changed_count = 0;
        world.for_each_mut_changed(POSITION, 0, |_entity, _table, _idx| {
            changed_count += 1;
        });

        assert_eq!(changed_count, 1);
    }

    #[test]
    fn test_change_detection_unchanged() {
        let mut world = World::default();
        let e1 = world.spawn_entities(POSITION, 1)[0];
        let e2 = world.spawn_entities(POSITION, 1)[0];

        world.step();

        world.get_position_mut(e1).unwrap().x = 5.0;

        let mut changed_entities = Vec::new();
        world.for_each_mut_changed(POSITION, 0, |entity, _table, _idx| {
            changed_entities.push(entity);
        });

        assert_eq!(changed_entities.len(), 1);
        assert!(changed_entities.contains(&e1));
        assert!(!changed_entities.contains(&e2));
    }

    #[test]
    fn test_change_detection_tick_tracking() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION, 1)[0];

        assert_eq!(world.current_tick(), 0);
        assert_eq!(world.last_tick(), 0);

        world.step();
        assert_eq!(world.current_tick(), 1);
        assert_eq!(world.last_tick(), 0);

        world.step();
        assert_eq!(world.current_tick(), 2);
        assert_eq!(world.last_tick(), 1);

        world.step();
        world.get_position_mut(entity).unwrap().x = 10.0;

        let mut count = 0;
        world.for_each_mut_changed(POSITION, 0, |_entity, _table, _idx| {
            count += 1;
        });
        assert_eq!(count, 1);

        world.step();
        count = 0;
        world.for_each_mut_changed(POSITION, 0, |_entity, _table, _idx| {
            count += 1;
        });
        assert_eq!(count, 0);
    }

    #[test]
    fn test_change_detection_multiple_components() {
        let mut world = World::default();
        let e1 = world.spawn_entities(POSITION | VELOCITY, 1)[0];
        let e2 = world.spawn_entities(POSITION | VELOCITY, 1)[0];

        world.step();

        world.get_position_mut(e1).unwrap().x = 5.0;
        world.get_velocity_mut(e2).unwrap().x = 10.0;

        let mut changed_entities = Vec::new();
        world.for_each_mut_changed(POSITION | VELOCITY, 0, |entity, _table, _idx| {
            changed_entities.push(entity);
        });

        assert_eq!(changed_entities.len(), 2);
        assert!(changed_entities.contains(&e1));
        assert!(changed_entities.contains(&e2));
    }

    #[test]
    fn test_change_detection_with_exclude() {
        let mut world = World::default();
        let e1 = world.spawn_entities(POSITION, 1)[0];
        let e2 = world.spawn_entities(POSITION | HEALTH, 1)[0];

        world.step();

        world.get_position_mut(e1).unwrap().x = 5.0;
        world.get_position_mut(e2).unwrap().x = 10.0;

        let mut changed_entities = Vec::new();
        world.for_each_mut_changed(POSITION, HEALTH, |entity, _table, _idx| {
            changed_entities.push(entity);
        });

        assert_eq!(changed_entities.len(), 1);
        assert!(changed_entities.contains(&e1));
        assert!(!changed_entities.contains(&e2));
    }

    #[test]
    fn test_change_detection_set() {
        let mut world = World::default();
        let e1 = world.spawn_entities(POSITION, 1)[0];
        let e2 = world.spawn_entities(POSITION, 1)[0];

        world.step();

        world.set_position(e1, Position { x: 5.0, y: 0.0 });

        let mut changed_entities = Vec::new();
        world.for_each_mut_changed(POSITION, 0, |entity, _table, _idx| {
            changed_entities.push(entity);
        });

        assert_eq!(changed_entities.len(), 1);
        assert!(changed_entities.contains(&e1));
        assert!(!changed_entities.contains(&e2));

        let queried: Vec<_> = world.query_entities_changed(POSITION).collect();
        assert_eq!(queried, vec![e1]);
    }

    #[test]
    fn test_change_detection_spawn() {
        let mut world = World::default();
        let e1 = world.spawn_entities(POSITION, 1)[0];

        world.step();

        let e2 = world.spawn_entities(POSITION, 1)[0];

        let changed: Vec<_> = world.query_entities_changed(POSITION).collect();
        assert_eq!(changed, vec![e2]);

        world.step();

        let changed: Vec<_> = world.query_entities_changed(POSITION).collect();
        assert!(changed.is_empty());
        assert!(world.get_position(e1).is_some());
    }

    #[test]
    fn test_change_detection_skips_untouched_tables() {
        let mut world = World::default();
        world.spawn_entities(POSITION, 3);
        let e2 = world.spawn_entities(POSITION | VELOCITY, 1)[0];

        world.step();

        world.get_position_mut(e2).unwrap().x = 1.0;

        let changed: Vec<_> = world.query_entities_changed(POSITION).collect();
        assert_eq!(changed, vec![e2]);

        for table in &world.tables {
            if table.mask == POSITION {
                assert!(!crate::tick_is_newer(
                    table.position_peak_changed,
                    world.last_tick
                ));
            }
            if table.mask == POSITION | VELOCITY {
                assert!(crate::tick_is_newer(
                    table.position_peak_changed,
                    world.last_tick
                ));
            }
        }
    }

    #[test]
    fn test_mark_changed_stamps_raw_writes() {
        let mut world = World::default();
        let entities = world.spawn_entities(POSITION, 3);

        world.step();

        world.for_each_mut(POSITION, 0, |entity, table, index| {
            if entity == entities[1] {
                table.position[index].x = 5.0;
            }
        });
        assert_eq!(world.query_entities_changed(POSITION).count(), 0);

        assert!(world.mark_changed(entities[1], POSITION));
        let changed: Vec<_> = world.query_entities_changed(POSITION).collect();
        assert_eq!(changed, vec![entities[1]]);
    }

    #[test]
    fn test_mark_changed_rejects_missing_rows() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION, 1)[0];
        let dead = world.spawn_entities(POSITION, 1)[0];
        world.despawn_entities(&[dead]);

        assert!(!world.mark_changed(entity, VELOCITY));
        assert!(!world.mark_changed(dead, POSITION));
        assert!(world.mark_changed(entity, POSITION | VELOCITY));
    }

    #[test]
    fn test_mark_columns_changed_bulk_stamps_one_table() {
        let mut world = World::default();
        world.spawn_entities(POSITION, 2);
        let moving = world.spawn_entities(POSITION | VELOCITY, 2);

        world.step();

        let current_tick = world.current_tick();
        for table in &mut world.tables {
            if table.mask & (POSITION | VELOCITY) != POSITION | VELOCITY {
                continue;
            }
            for value in &mut table.position {
                value.x += 1.0;
            }
            table.mark_columns_changed(POSITION, current_tick);
        }

        let changed: Vec<_> = world.query_entities_changed(POSITION).collect();
        assert_eq!(changed, moving);
        assert_eq!(world.query_entities_changed(VELOCITY).count(), 0);
    }

    #[test]
    fn test_change_detection_since_cursor() {
        let mut world = World::default();
        let e1 = world.spawn_entities(POSITION, 1)[0];
        world.step();

        let cursor = world.last_tick();
        world.get_position_mut(e1).unwrap().x = 1.0;
        world.step();
        world.step();

        let changed: Vec<_> = world
            .query_entities_changed_since(POSITION, cursor)
            .collect();
        assert_eq!(changed, vec![e1]);

        let cursor = world.current_tick();
        let changed: Vec<_> = world
            .query_entities_changed_since(POSITION, cursor)
            .collect();
        assert!(changed.is_empty());

        let mut visited = Vec::new();
        world.for_each_mut_changed_since(POSITION, 0, 0, |entity, _table, _idx| {
            visited.push(entity)
        });
        assert_eq!(visited, vec![e1]);
    }

    #[test]
    fn test_structural_log_records_lifecycle() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION, 1)[0];
        world.add_components(entity, VELOCITY);
        world.remove_components(entity, POSITION);
        world.despawn_entities(&[entity]);

        let changes: Vec<_> = world.structural_changes_since(0).to_vec();
        assert_eq!(changes.len(), 4);
        assert!(changes.iter().all(|change| change.entity == entity));
        assert_eq!(changes[0].kind, StructuralChangeKind::Spawned);
        assert_eq!(changes[0].mask, POSITION);
        assert_eq!(changes[1].kind, StructuralChangeKind::ComponentsAdded);
        assert_eq!(changes[1].mask, VELOCITY);
        assert_eq!(changes[2].kind, StructuralChangeKind::ComponentsRemoved);
        assert_eq!(changes[2].mask, POSITION);
        assert_eq!(changes[3].kind, StructuralChangeKind::Despawned);
        assert_eq!(changes[3].mask, VELOCITY);

        let cursor = changes[1].sequence;
        assert_eq!(world.structural_changes_since(cursor).len(), 2);

        world.trim_structural_log(cursor);
        assert_eq!(world.structural_changes_since(0).len(), 2);
        assert_eq!(
            world.structural_changes_since(0)[0].kind,
            StructuralChangeKind::ComponentsRemoved
        );

        world.clear_structural_log();
        assert!(world.structural_changes_since(0).is_empty());
        assert_eq!(world.structural_sequence(), 4);
    }

    #[test]
    fn test_structural_log_set_component_records_add() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION, 1)[0];
        world.set_velocity(entity, Velocity { x: 1.0, y: 0.0 });

        let changes = world.structural_changes_since(0);
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[1].kind, StructuralChangeKind::ComponentsAdded);
        assert_eq!(changes[1].mask, VELOCITY);
    }

    #[test]
    fn test_tick_is_newer() {
        assert!(crate::tick_is_newer(1, 0));
        assert!(!crate::tick_is_newer(0, 0));
        assert!(!crate::tick_is_newer(0, 1));
        assert!(crate::tick_is_newer(0, u32::MAX));
        assert!(crate::tick_is_newer(5, u32::MAX - 3));
        assert!(!crate::tick_is_newer(u32::MAX, 0));
    }

    #[test]
    fn test_allocator_liveness() {
        let mut allocator = EntityAllocator::default();

        let entity = allocator.allocate();
        assert!(allocator.is_alive(entity));

        assert!(allocator.deallocate(entity));
        assert!(!allocator.is_alive(entity));
        assert!(!allocator.deallocate(entity), "double free must be refused");

        let reused = allocator.allocate();
        assert_eq!(reused.id, entity.id);
        assert_eq!(reused.generation, entity.generation + 1);
        assert!(allocator.is_alive(reused));
        assert!(!allocator.is_alive(entity));

        assert!(
            !allocator.deallocate(entity),
            "stale free must not kill the reused id"
        );
        assert!(allocator.is_alive(reused));

        let other = allocator.allocate();
        assert_ne!((other.id, other.generation), (reused.id, reused.generation));
    }

    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0 >> 16
        }
    }

    #[test]
    fn test_event_channel_capacity_backstop() {
        let mut channel: EventChannel<u32> = EventChannel::new();
        let total = EVENT_CHANNEL_CAPACITY as u32 + 10;
        for value in 0..total {
            channel.send(value);
        }

        assert_eq!(
            channel.len(),
            EVENT_CHANNEL_CAPACITY / 2 + 10,
            "crossing capacity must drop the oldest half exactly once"
        );
        assert_eq!(
            channel.sequence(),
            u64::from(total),
            "sequences must keep counting across the backstop"
        );
        assert_eq!(
            channel.peek(),
            Some(&(EVENT_CHANNEL_CAPACITY as u32 / 2)),
            "the survivor at the front is the first event after the dropped half"
        );
        assert_eq!(channel.events_since(u64::from(total) - 5).len(), 5);

        channel.update();
        assert_eq!(
            channel.len(),
            EVENT_CHANNEL_CAPACITY / 2 + 10,
            "a stale previous-update watermark behind the base must not over-trim"
        );

        channel.update();
        assert!(channel.is_empty());
        assert_eq!(channel.sequence(), u64::from(total));
    }

    #[test]
    fn test_structural_log_capacity_backstop() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION, 1)[0];

        for _ in 0..STRUCTURAL_LOG_CAPACITY {
            world.record_structural(entity, StructuralChangeKind::ComponentsAdded, POSITION);
        }

        assert_eq!(
            world.structural_log.len(),
            1,
            "the record that found the log full must clear it wholesale first"
        );
        assert_eq!(
            world.structural_sequence(),
            STRUCTURAL_LOG_CAPACITY as u64 + 1,
            "sequences must stay monotone across the wholesale clear"
        );

        let tail = world.structural_changes_since(0);
        assert_eq!(tail.len(), 1);
        assert_eq!(tail[0].sequence, world.structural_sequence());
        assert!(
            world
                .structural_changes_since(world.structural_sequence())
                .is_empty()
        );
    }

    #[derive(Default, Clone)]
    struct ModelEntity {
        mask: u64,
        position: Option<f32>,
        position_changed: bool,
        player: bool,
        enemy: bool,
    }

    enum ModelCommand {
        Spawn(u64),
        Despawn(Entity),
        AddComponents(Entity, u64),
        RemoveComponents(Entity, u64),
        SetPosition(Entity, f32),
        AddPlayer(Entity),
        RemovePlayer(Entity),
    }

    fn model_add_components(model_entity: &mut ModelEntity, mask: u64) {
        let migrated = mask & !model_entity.mask != 0;
        if mask & POSITION != 0 && model_entity.mask & POSITION == 0 {
            model_entity.position = Some(0.0);
        }
        model_entity.mask |= mask;
        if migrated && model_entity.mask & POSITION != 0 {
            model_entity.position_changed = true;
        }
    }

    fn model_remove_components(model_entity: &mut ModelEntity, mask: u64) {
        let migrated = mask & model_entity.mask != 0;
        if mask & POSITION != 0 {
            model_entity.position = None;
        }
        model_entity.mask &= !mask;
        if migrated && model_entity.mask & POSITION != 0 {
            model_entity.position_changed = true;
        }
    }

    fn model_set_position(model_entity: &mut ModelEntity, value: f32) {
        model_entity.mask |= POSITION;
        model_entity.position = Some(value);
        model_entity.position_changed = true;
    }

    fn model_spawn(mask: u64) -> ModelEntity {
        ModelEntity {
            mask,
            position: (mask & POSITION != 0).then_some(0.0),
            position_changed: mask & POSITION != 0,
            ..Default::default()
        }
    }

    fn apply_and_replay(
        world: &mut World,
        model: &mut std::collections::HashMap<Entity, ModelEntity>,
        queued: &mut Vec<ModelCommand>,
        handles: &mut Vec<Entity>,
    ) {
        assert_eq!(
            world.command_count(),
            queued.len(),
            "world and model must queue in lockstep"
        );
        world.apply_commands();

        let mut spawned_masks: Vec<u64> = Vec::new();
        for command in queued.drain(..) {
            match command {
                ModelCommand::Spawn(mask) => spawned_masks.push(mask),
                ModelCommand::Despawn(entity) => {
                    model.remove(&entity);
                }
                ModelCommand::AddComponents(entity, mask) => {
                    if let Some(model_entity) = model.get_mut(&entity) {
                        model_add_components(model_entity, mask);
                    }
                }
                ModelCommand::RemoveComponents(entity, mask) => {
                    if let Some(model_entity) = model.get_mut(&entity) {
                        model_remove_components(model_entity, mask);
                    }
                }
                ModelCommand::SetPosition(entity, value) => {
                    if let Some(model_entity) = model.get_mut(&entity) {
                        model_set_position(model_entity, value);
                    }
                }
                ModelCommand::AddPlayer(entity) => {
                    if let Some(model_entity) = model.get_mut(&entity) {
                        model_entity.player = true;
                    }
                }
                ModelCommand::RemovePlayer(entity) => {
                    if let Some(model_entity) = model.get_mut(&entity) {
                        model_entity.player = false;
                    }
                }
            }
        }

        if !spawned_masks.is_empty() {
            let known: std::collections::HashSet<Entity> = model.keys().copied().collect();
            let new_entities: Vec<Entity> = world
                .get_all_entities()
                .into_iter()
                .filter(|entity| !known.contains(entity))
                .collect();
            assert_eq!(
                new_entities.len(),
                spawned_masks.len(),
                "queued spawns must materialize exactly"
            );

            let mut actual_masks: Vec<u64> = new_entities
                .iter()
                .map(|&entity| world.component_mask(entity).unwrap())
                .collect();
            actual_masks.sort_unstable();
            spawned_masks.sort_unstable();
            assert_eq!(
                actual_masks, spawned_masks,
                "queued spawn masks must match the materialized archetypes"
            );

            for &entity in &new_entities {
                let mask = world.component_mask(entity).unwrap();
                model.insert(entity, model_spawn(mask));
                handles.push(entity);
            }
        }
    }

    #[test]
    fn test_property_single_world_matches_model() {
        let component_masks = [POSITION, VELOCITY, HEALTH];

        for seed in [1u64, 42, 4242, 987654321] {
            let mut rng = Lcg(seed);
            let mut world = World::default();
            let mut model: std::collections::HashMap<Entity, ModelEntity> =
                std::collections::HashMap::new();
            let mut handles: Vec<Entity> = Vec::new();
            let mut queued: Vec<ModelCommand> = Vec::new();
            let mut pending_ping_values: Vec<u32> = Vec::new();
            let mut total_pings: u64 = 0;

            world.step();

            let random_mask = |rng: &mut Lcg| {
                let mut mask = 0;
                for &component in &component_masks {
                    if rng.next().is_multiple_of(2) {
                        mask |= component;
                    }
                }
                mask
            };
            let pick = |rng: &mut Lcg, handles: &[Entity]| {
                if handles.is_empty() {
                    None
                } else {
                    Some(handles[rng.next() as usize % handles.len()])
                }
            };

            for _ in 0..4000 {
                match rng.next() % 16 {
                    0 | 1 => {
                        let mask = random_mask(&mut rng);
                        let entity = world.spawn_entities(mask, 1)[0];
                        model.insert(entity, model_spawn(mask));
                        handles.push(entity);
                    }
                    2 => {
                        if let Some(entity) = pick(&mut rng, &handles) {
                            let despawned = world.despawn_entities(&[entity]);
                            let was_live = model.remove(&entity).is_some();
                            assert_eq!(
                                despawned.len() == 1,
                                was_live,
                                "despawn must succeed exactly for model-live handles"
                            );
                        }
                    }
                    3 => {
                        if let Some(entity) = pick(&mut rng, &handles) {
                            let mask = random_mask(&mut rng);
                            let accepted = world.add_components(entity, mask);
                            match model.get_mut(&entity) {
                                Some(model_entity) => {
                                    assert!(accepted);
                                    model_add_components(model_entity, mask);
                                }
                                None => assert!(!accepted, "stale add must be refused"),
                            }
                        }
                    }
                    4 => {
                        if let Some(entity) = pick(&mut rng, &handles) {
                            let mask = random_mask(&mut rng);
                            let accepted = world.remove_components(entity, mask);
                            match model.get_mut(&entity) {
                                Some(model_entity) => {
                                    assert!(accepted);
                                    model_remove_components(model_entity, mask);
                                }
                                None => assert!(!accepted, "stale remove must be refused"),
                            }
                        }
                    }
                    5 => {
                        if let Some(entity) = pick(&mut rng, &handles) {
                            world.add_player(entity);
                            if let Some(model_entity) = model.get_mut(&entity) {
                                model_entity.player = true;
                            }
                            assert_eq!(
                                world.has_player(entity),
                                model.get(&entity).map(|m| m.player).unwrap_or(false)
                            );
                        }
                    }
                    6 => {
                        if let Some(entity) = pick(&mut rng, &handles) {
                            let removed = world.remove_player(entity);
                            let expected = match model.get_mut(&entity) {
                                Some(model_entity) => {
                                    let had = model_entity.player;
                                    model_entity.player = false;
                                    had
                                }
                                None => false,
                            };
                            assert_eq!(removed, expected);
                        }
                    }
                    7 => {
                        if let Some(entity) = pick(&mut rng, &handles) {
                            let value = (rng.next() % 1000) as f32;
                            world.set_position(entity, Position { x: value, y: 0.0 });
                            match model.get_mut(&entity) {
                                Some(model_entity) => {
                                    model_set_position(model_entity, value);
                                    assert_eq!(world.get_position(entity).unwrap().x, value);
                                }
                                None => assert!(
                                    world.get_position(entity).is_none(),
                                    "stale set must not resurrect a component"
                                ),
                            }
                        }
                    }
                    8 => {
                        if let Some(entity) = pick(&mut rng, &handles) {
                            world.queue_despawn_entity(entity);
                            queued.push(ModelCommand::Despawn(entity));
                        }
                    }
                    9 => {
                        if let Some(entity) = pick(&mut rng, &handles) {
                            let mask = random_mask(&mut rng);
                            world.queue_add_components(entity, mask);
                            queued.push(ModelCommand::AddComponents(entity, mask));
                        }
                    }
                    10 => {
                        if let Some(entity) = pick(&mut rng, &handles) {
                            let mask = random_mask(&mut rng);
                            world.queue_remove_components(entity, mask);
                            queued.push(ModelCommand::RemoveComponents(entity, mask));
                        }
                    }
                    11 => {
                        if let Some(entity) = pick(&mut rng, &handles) {
                            let value = (rng.next() % 1000) as f32;
                            world.queue_set_position(entity, Position { x: value, y: 0.0 });
                            queued.push(ModelCommand::SetPosition(entity, value));
                        }
                    }
                    12 => {
                        if let Some(entity) = pick(&mut rng, &handles) {
                            if rng.next().is_multiple_of(2) {
                                world.queue_add_player(entity);
                                queued.push(ModelCommand::AddPlayer(entity));
                            } else {
                                world.queue_remove_player(entity);
                                queued.push(ModelCommand::RemovePlayer(entity));
                            }
                        }
                    }
                    13 => {
                        let mask = random_mask(&mut rng);
                        world.queue_spawn_entities(mask, 1);
                        queued.push(ModelCommand::Spawn(mask));
                    }
                    14 => {
                        let value = rng.next() as u32;
                        world.send_ping(PingEvent { value });
                        pending_ping_values.push(value);
                        total_pings += 1;
                    }
                    _ => {
                        apply_and_replay(&mut world, &mut model, &mut queued, &mut handles);

                        let changed: std::collections::HashSet<Entity> =
                            world.query_entities_changed(POSITION).collect();
                        let expected: std::collections::HashSet<Entity> = model
                            .iter()
                            .filter(|(_, model_entity)| {
                                model_entity.mask & POSITION != 0 && model_entity.position_changed
                            })
                            .map(|(&entity, _)| entity)
                            .collect();
                        assert_eq!(
                            changed, expected,
                            "changed-query set diverged from model with seed {seed}"
                        );

                        world.step();
                        for model_entity in model.values_mut() {
                            model_entity.position_changed = false;
                        }

                        let buffered: Vec<u32> = world.read_ping().map(|ping| ping.value).collect();
                        assert_eq!(
                            buffered, pending_ping_values,
                            "post-step event buffer must hold exactly the just-ended frame, in order"
                        );
                        assert_eq!(world.sequence_ping(), total_pings);
                        pending_ping_values.clear();
                    }
                }
            }

            apply_and_replay(&mut world, &mut model, &mut queued, &mut handles);

            assert_eq!(world.entity_count(), model.len());

            for (&entity, model_entity) in &model {
                assert_eq!(world.component_mask(entity), Some(model_entity.mask));
                assert_eq!(
                    world.get_position(entity).map(|position| position.x),
                    model_entity.position,
                    "position value diverged from model with seed {seed}"
                );
                assert_eq!(world.has_player(entity), model_entity.player);
                assert_eq!(world.has_enemy(entity), model_entity.enemy);
                assert!(world.is_alive(entity));
            }

            for &handle in &handles {
                if !model.contains_key(&handle) {
                    assert_eq!(world.component_mask(handle), None);
                    assert!(!world.has_player(handle));
                    assert!(world.get_position(handle).is_none());
                    assert!(!world.is_alive(handle));
                }
            }

            for mask in [
                POSITION,
                VELOCITY,
                HEALTH,
                POSITION | VELOCITY,
                POSITION | HEALTH,
            ] {
                let expected = model
                    .values()
                    .filter(|model_entity| model_entity.mask & mask == mask)
                    .count();
                assert_eq!(
                    world.query_entities(mask).count(),
                    expected,
                    "query count diverged from model for mask {mask:b} with seed {seed}"
                );
            }

            let expected_players = model.values().filter(|m| m.player).count();
            assert_eq!(world.query_player().count(), expected_players);
        }
    }

    #[test]
    fn test_allocator_batch_mixes_recycled_and_fresh() {
        let mut allocator = EntityAllocator::default();

        let first = allocator.allocate();
        let second = allocator.allocate();
        assert!(allocator.deallocate(first));
        assert!(allocator.deallocate(second));

        let mut entities = Vec::new();
        allocator.allocate_batch(4, &mut entities);
        assert_eq!(entities.len(), 4);

        let recycled_count = entities
            .iter()
            .filter(|entity| entity.generation > 0)
            .count();
        assert_eq!(recycled_count, 2, "both freed ids must be recycled first");

        for &entity in &entities {
            assert!(allocator.is_alive(entity));
        }
        assert!(!allocator.is_alive(first));
        assert!(!allocator.is_alive(second));

        let mut seen: Vec<_> = entities
            .iter()
            .map(|entity| (entity.id, entity.generation))
            .collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 4, "batch handles must be distinct");
    }

    #[test]
    fn test_event_channel_cursor_consumers() {
        let mut channel: EventChannel<u32> = EventChannel::new();
        channel.send(1);
        channel.send(2);

        let mut cursor_a = 0;
        let mut cursor_b = 0;

        assert_eq!(channel.events_since(cursor_a), &[1, 2]);
        cursor_a = channel.sequence();

        channel.send(3);
        assert_eq!(channel.events_since(cursor_a), &[3]);
        cursor_a = channel.sequence();

        assert_eq!(channel.events_since(cursor_b), &[1, 2, 3]);
        cursor_b = channel.sequence();

        assert!(channel.events_since(cursor_a).is_empty());
        assert!(channel.events_since(cursor_b).is_empty());
    }

    #[cfg(feature = "dynamic")]
    mod dynamic_schema_tests {
        #[derive(Default, Clone, Debug)]
        #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
        struct SchemaPosition {
            _x: f32,
        }

        #[derive(Default, Clone, Debug)]
        #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
        struct SchemaVelocity {
            _x: f32,
        }

        crate::dynamic_schema! {
            fn register_schema {
                position: SchemaPosition => SCHEMA_POSITION,
                velocity: SchemaVelocity => SCHEMA_VELOCITY,
            }
        }

        #[test]
        fn test_dynamic_schema_declares_consts_and_registry_in_order() {
            assert_eq!(SCHEMA_POSITION, 1);
            assert_eq!(SCHEMA_VELOCITY, 2);

            let registry = register_schema();
            assert_eq!(registry.components.len(), 2);
            assert_eq!(registry.remaining_bits(), 62);

            let mut world = crate::dynamic::DynWorld::from_registry(register_schema());
            let key = world.register::<SchemaVelocity>();
            assert_eq!(key.mask, SCHEMA_VELOCITY);
        }

        #[cfg(feature = "snapshot")]
        crate::dynamic_schema! {
            serde fn register_schema_serde {
                position: SchemaPosition => SERDE_SCHEMA_POSITION,
                velocity: SchemaVelocity => SERDE_SCHEMA_VELOCITY,
            }
        }

        #[cfg(feature = "snapshot")]
        #[test]
        fn test_dynamic_schema_serde_mode_registers_codecs() {
            let mut world = crate::dynamic::DynWorld::from_registry(register_schema_serde());
            world.spawn((SchemaPosition { _x: 1.0 }, SchemaVelocity { _x: 2.0 }));

            let snapshot = world.snapshot().unwrap();
            let restored =
                crate::dynamic::DynWorld::from_snapshot(register_schema_serde(), &snapshot)
                    .unwrap();
            assert_eq!(restored.entity_count(), 1);
            assert_eq!(SERDE_SCHEMA_POSITION, 1);
        }
    }

    #[test]
    fn test_schedule_push_if_gates_on_condition() {
        let mut world = World::default();
        world.spawn_entities(POSITION, 1);

        let mut schedule = Schedule::new();
        schedule.push_if(
            "conditional",
            |world: &World| world.entity_count() > 1,
            |world: &mut World| {
                let entity = world.get_all_entities()[0];
                world.get_position_mut(entity).unwrap().x += 1.0;
            },
        );

        schedule.run(&mut world);
        let entity = world.get_all_entities()[0];
        assert_eq!(world.get_position(entity).unwrap().x, 0.0);

        world.spawn_entities(POSITION, 1);
        schedule.run(&mut world);
        assert_eq!(world.get_position(entity).unwrap().x, 1.0);
    }

    #[test]
    fn test_event_channel_consume_is_exactly_once() {
        let mut channel: EventChannel<u32> = EventChannel::new();
        channel.send(1);
        channel.send(2);

        let mut cursor_a = 0;
        let mut cursor_b = 0;

        assert_eq!(channel.consume(&mut cursor_a), &[1, 2]);
        assert!(channel.consume(&mut cursor_a).is_empty());

        channel.update();
        channel.send(3);
        assert_eq!(
            channel.consume(&mut cursor_a),
            &[3],
            "the two-frame buffer must not re-deliver"
        );

        assert_eq!(channel.consume(&mut cursor_b), &[1, 2, 3]);
    }

    #[test]
    fn test_generated_consume_event_is_exactly_once() {
        let mut world = World::default();

        world.send_ping(PingEvent { value: 1 });

        let mut cursor = 0;
        assert_eq!(world.consume_ping(&mut cursor).len(), 1);
        assert!(world.consume_ping(&mut cursor).is_empty());

        world.step();
        assert!(
            world.consume_ping(&mut cursor).is_empty(),
            "collect_ would re-deliver here; consume_ must not"
        );

        world.send_ping(PingEvent { value: 2 });
        assert_eq!(world.consume_ping(&mut cursor).len(), 1);
    }

    #[test]
    fn test_event_channel_two_frame_expiry() {
        let mut channel: EventChannel<u32> = EventChannel::new();
        channel.send(1);

        channel.update();
        assert_eq!(channel.len(), 1, "event survives its first frame boundary");

        channel.send(2);
        channel.update();
        assert_eq!(
            channel.events_since(0),
            &[2],
            "first event expired, second survives"
        );

        channel.update();
        assert!(channel.is_empty());
    }

    #[test]
    fn test_event_channel_trim_and_lagging_cursor() {
        let mut channel: EventChannel<u32> = EventChannel::new();
        for value in 0..10 {
            channel.send(value);
        }

        channel.trim(4);
        assert_eq!(channel.len(), 6);
        assert_eq!(
            channel.events_since(0),
            &[4, 5, 6, 7, 8, 9],
            "a lagging cursor sees everything still buffered"
        );
        assert_eq!(channel.events_since(7), &[7, 8, 9]);
        assert_eq!(channel.sequence(), 10);

        channel.clear();
        assert!(channel.is_empty());
        assert_eq!(
            channel.sequence(),
            10,
            "clearing advances past events without reusing sequences"
        );
    }

    #[test]
    fn test_single_world_double_despawn() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION, 1)[0];

        assert_eq!(world.despawn_entities(&[entity]).len(), 1);
        assert!(world.despawn_entities(&[entity]).is_empty());
        assert!(world.despawn_entities(&[entity, entity]).is_empty());

        let e1 = world.spawn_entities(POSITION, 1)[0];
        let e2 = world.spawn_entities(POSITION, 1)[0];
        assert_ne!((e1.id, e1.generation), (e2.id, e2.generation));
    }

    #[test]
    fn test_single_world_duplicate_despawn_in_one_call() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION, 1)[0];

        let despawned = world.despawn_entities(&[entity, entity, entity]);
        assert_eq!(despawned.len(), 1);

        let e1 = world.spawn_entities(POSITION, 1)[0];
        let e2 = world.spawn_entities(POSITION, 1)[0];
        assert_ne!((e1.id, e1.generation), (e2.id, e2.generation));
    }

    #[test]
    #[should_panic(expected = "spawn masks must not contain tag bits")]
    fn test_spawn_with_tag_bits_panics_in_debug() {
        let mut world = World::default();
        world.spawn_entities(POSITION | PLAYER, 1);
    }

    #[test]
    #[should_panic(expected = "component masks must not contain tag bits")]
    fn test_add_components_with_tag_bits_panics_in_debug() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION, 1)[0];
        world.add_components(entity, VELOCITY | PLAYER);
    }

    #[test]
    #[should_panic(expected = "component masks only")]
    fn test_query_entities_with_tag_bits_panics_in_debug() {
        let world = World::default();
        let _ = world.query_entities(POSITION | PLAYER);
    }

    #[test]
    fn test_tag_masks_with_empty_sets() {
        let mut world = World::default();
        world.spawn_entities(POSITION, 3);

        let mut count = 0;
        world.for_each(POSITION, PLAYER, |_entity, _table, _idx| count += 1);
        assert_eq!(count, 3, "excluding a tag nobody has excludes nothing");

        count = 0;
        world.for_each(POSITION | PLAYER, 0, |_entity, _table, _idx| count += 1);
        assert_eq!(count, 0, "including a tag nobody has matches nothing");

        count = 0;
        world.for_each_mut(POSITION | PLAYER, 0, |_entity, _table, _idx| count += 1);
        assert_eq!(count, 0);

        count = 0;
        world.for_each_mut(POSITION, PLAYER, |_entity, _table, _idx| count += 1);
        assert_eq!(count, 3);
    }

    #[test]
    fn test_query_component_mut_with_tag_filter() {
        let mut world = World::default();
        let e1 = world.spawn_entities(POSITION, 1)[0];
        let e2 = world.spawn_entities(POSITION, 1)[0];
        world.add_player(e1);

        let mut visited = Vec::new();
        world.query_position_mut(PLAYER, |entity, position| {
            position.x = 42.0;
            visited.push(entity);
        });

        assert_eq!(visited, vec![e1]);
        assert_eq!(world.get_position(e1).unwrap().x, 42.0);
        assert_eq!(world.get_position(e2).unwrap().x, 0.0);
    }

    #[test]
    fn test_query_component_mut_marks_changed() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION, 1)[0];
        world.step();

        world.query_position_mut(0, |_entity, position| {
            position.x = 1.0;
        });

        let changed: Vec<Entity> = world.query_entities_changed(POSITION).collect();
        assert_eq!(changed, vec![entity]);
    }

    #[test]
    fn test_simd_slice_iteration_read() {
        let mut world = World::default();
        world.spawn_entities(POSITION, 3);

        let mut total_count = 0;
        for slice in world.iter_position_slices() {
            total_count += slice.len();
            for pos in slice {
                assert_eq!(pos.x, 0.0);
                assert_eq!(pos.y, 0.0);
            }
        }

        assert_eq!(total_count, 3);
    }

    #[test]
    fn test_simd_slice_iteration_write() {
        let mut world = World::default();
        let entities = world.spawn_entities(POSITION, 5);

        for slice in world.iter_position_slices_mut() {
            for pos in slice {
                pos.x = 10.0;
                pos.y = 20.0;
            }
        }

        for entity in entities {
            let pos = world.get_position(entity).unwrap();
            assert_eq!(pos.x, 10.0);
            assert_eq!(pos.y, 20.0);
        }
    }

    #[test]
    fn test_simd_slice_iteration_multiple_archetypes() {
        let mut world = World::default();
        world.spawn_entities(POSITION, 2);
        world.spawn_entities(POSITION | VELOCITY, 3);
        world.spawn_entities(POSITION | HEALTH, 4);

        let mut slice_count = 0;
        let mut total_entities = 0;

        for slice in world.iter_position_slices() {
            slice_count += 1;
            total_entities += slice.len();
        }

        assert_eq!(slice_count, 3);
        assert_eq!(total_entities, 9);
    }

    #[test]
    fn test_simd_slice_vectorizable_operation() {
        let mut world = World::default();
        world.spawn_entities(POSITION | VELOCITY, 1000);

        for pos in world.iter_position_slices_mut() {
            for p in pos {
                p.x += 1.0;
                p.y += 2.0;
            }
        }

        for vel in world.iter_velocity_slices_mut() {
            for v in vel {
                v.x *= 0.99;
                v.y *= 0.99;
            }
        }

        let mut checked = 0;
        for slice in world.iter_position_slices() {
            for pos in slice {
                assert_eq!(pos.x, 1.0);
                assert_eq!(pos.y, 2.0);
                checked += 1;
            }
        }
        assert_eq!(checked, 1000);
    }

    #[test]
    fn test_simd_slice_empty_world() {
        let world = World::default();

        let mut count = 0;
        for _ in world.iter_position_slices() {
            count += 1;
        }
        assert_eq!(count, 0);
    }

    #[test]
    fn test_simd_slice_no_matching_archetype() {
        let mut world = World::default();
        world.spawn_entities(VELOCITY, 5);

        let mut count = 0;
        for _ in world.iter_position_slices() {
            count += 1;
        }
        assert_eq!(count, 0);
    }

    #[test]
    fn test_sparse_set_add_remove_has() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION, 1)[0];

        assert!(!world.has_player(entity));

        world.add_player(entity);
        assert!(world.has_player(entity));

        world.remove_player(entity);
        assert!(!world.has_player(entity));
    }

    #[test]
    fn test_sparse_set_query() {
        let mut world = World::default();
        let e1 = world.spawn_entities(POSITION, 1)[0];
        let e2 = world.spawn_entities(POSITION, 1)[0];
        let e3 = world.spawn_entities(POSITION, 1)[0];

        world.add_player(e1);
        world.add_player(e3);

        let players: Vec<Entity> = world.query_player().collect();
        assert_eq!(players.len(), 2);
        assert!(players.contains(&e1));
        assert!(players.contains(&e3));
        assert!(!players.contains(&e2));
    }

    #[test]
    fn test_sparse_set_no_archetype_fragmentation() {
        let mut world = World::default();
        let e1 = world.spawn_entities(POSITION, 1)[0];
        let e2 = world.spawn_entities(POSITION, 1)[0];
        let e3 = world.spawn_entities(POSITION, 1)[0];

        world.add_player(e1);
        world.add_enemy(e2);

        let mut count = 0;
        world.for_each(POSITION, 0, |_, _, _| {
            count += 1;
        });

        assert_eq!(count, 3);

        let mask1 = world.component_mask(e1).unwrap();
        let mask2 = world.component_mask(e2).unwrap();
        let mask3 = world.component_mask(e3).unwrap();

        assert_eq!(mask1, mask2);
        assert_eq!(mask2, mask3);
    }

    #[test]
    fn test_sparse_set_tag_component_query() {
        let mut world = World::default();
        let e1 = world.spawn_entities(POSITION | VELOCITY, 1)[0];
        let e2 = world.spawn_entities(POSITION | VELOCITY, 1)[0];
        let e3 = world.spawn_entities(POSITION, 1)[0];
        let e4 = world.spawn_entities(POSITION | VELOCITY, 1)[0];

        world.add_player(e1);
        world.add_player(e2);

        let mut count = 0;
        world.for_each(POSITION | VELOCITY | PLAYER, 0, |_, _, _| {
            count += 1;
        });
        assert_eq!(count, 2);

        let mut found = Vec::new();
        world.for_each(POSITION | VELOCITY | PLAYER, 0, |entity, _, _| {
            found.push(entity);
        });
        assert!(found.contains(&e1));
        assert!(found.contains(&e2));
        assert!(!found.contains(&e3));
        assert!(!found.contains(&e4));
    }

    #[test]
    fn test_sparse_set_exclude_tag() {
        let mut world = World::default();
        let e1 = world.spawn_entities(POSITION, 1)[0];
        let e2 = world.spawn_entities(POSITION, 1)[0];
        let e3 = world.spawn_entities(POSITION, 1)[0];

        world.add_player(e1);
        world.add_player(e2);

        let mut count = 0;
        world.for_each(POSITION, PLAYER, |_, _, _| {
            count += 1;
        });
        assert_eq!(count, 1);

        let mut found = Vec::new();
        world.for_each(POSITION, PLAYER, |entity, _, _| {
            found.push(entity);
        });
        assert_eq!(found.len(), 1);
        assert!(found.contains(&e3));
    }

    #[test]
    fn test_sparse_set_cleanup_on_despawn() {
        let mut world = World::default();
        let e1 = world.spawn_entities(POSITION, 1)[0];
        let e2 = world.spawn_entities(POSITION, 1)[0];

        world.add_player(e1);
        world.add_enemy(e2);

        assert_eq!(world.query_player().count(), 1);
        assert_eq!(world.query_enemy().count(), 1);

        world.despawn_entities(&[e1]);

        assert_eq!(world.query_player().count(), 0);
        assert_eq!(world.query_enemy().count(), 1);

        world.despawn_entities(&[e2]);
        assert_eq!(world.query_enemy().count(), 0);
    }

    #[test]
    fn test_sparse_set_multiple_tags() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION, 1)[0];

        world.add_player(entity);
        world.add_active(entity);

        assert!(world.has_player(entity));
        assert!(world.has_active(entity));
        assert!(!world.has_enemy(entity));

        let mut count = 0;
        world.for_each(POSITION | PLAYER | ACTIVE, 0, |_, _, _| {
            count += 1;
        });
        assert_eq!(count, 1);

        world.remove_active(entity);
        assert!(world.has_player(entity));
        assert!(!world.has_active(entity));

        count = 0;
        world.for_each(POSITION | PLAYER | ACTIVE, 0, |_, _, _| {
            count += 1;
        });
        assert_eq!(count, 0);
    }

    #[test]
    fn test_sparse_set_for_each_mut() {
        let mut world = World::default();
        let e1 = world.spawn_entities(POSITION, 1)[0];
        let e2 = world.spawn_entities(POSITION, 1)[0];
        let e3 = world.spawn_entities(POSITION, 1)[0];

        world.add_player(e1);
        world.add_player(e3);

        world.for_each_mut(POSITION | PLAYER, 0, |_, table, idx| {
            table.position[idx].x = 100.0;
        });

        assert_eq!(world.get_position(e1).unwrap().x, 100.0);
        assert_ne!(world.get_position(e2).unwrap().x, 100.0);
        assert_eq!(world.get_position(e3).unwrap().x, 100.0);
    }

    #[test]
    #[cfg(not(target_family = "wasm"))]
    fn test_sparse_set_par_for_each_mut() {
        let mut world = World::default();
        let entities = world.spawn_entities(POSITION, 100);

        for &entity in &entities[0..50] {
            world.add_player(entity);
        }

        world.par_for_each_mut(POSITION | PLAYER, 0, |_, table, idx| {
            table.position[idx].x = 200.0;
        });

        for &entity in entities.iter().take(50) {
            assert_eq!(world.get_position(entity).unwrap().x, 200.0);
        }
        for &entity in entities.iter().skip(50).take(50) {
            assert_ne!(world.get_position(entity).unwrap().x, 200.0);
        }
    }

    #[test]
    fn test_sparse_set_complex_query() {
        let mut world = World::default();
        let e1 = world.spawn_entities(POSITION | VELOCITY, 1)[0];
        let e2 = world.spawn_entities(POSITION | VELOCITY, 1)[0];
        let e3 = world.spawn_entities(POSITION | VELOCITY, 1)[0];
        let _e4 = world.spawn_entities(POSITION, 1)[0];

        world.add_player(e1);
        world.add_enemy(e2);
        world.add_enemy(e3);
        world.add_active(e1);
        world.add_active(e2);

        let mut count = 0;
        world.for_each(POSITION | VELOCITY | ENEMY | ACTIVE, 0, |_, _, _| {
            count += 1;
        });
        assert_eq!(count, 1);

        let mut found = Vec::new();
        world.for_each(POSITION | VELOCITY | ENEMY | ACTIVE, 0, |entity, _, _| {
            found.push(entity);
        });
        assert_eq!(found.len(), 1);
        assert!(found.contains(&e2));
    }

    #[test]
    fn test_command_buffer_spawn() {
        let mut world = World::default();

        world.queue_spawn_entities(POSITION, 3);
        world.queue_spawn_entities(VELOCITY, 2);

        assert_eq!(world.command_count(), 2);
        assert_eq!(world.get_all_entities().len(), 0);

        world.apply_commands();

        assert_eq!(world.command_count(), 0);
        assert_eq!(world.get_all_entities().len(), 5);

        let mut pos_count = 0;
        world.for_each(POSITION, 0, |_, _, _| pos_count += 1);
        assert_eq!(pos_count, 3);

        let mut vel_count = 0;
        world.for_each(VELOCITY, 0, |_, _, _| vel_count += 1);
        assert_eq!(vel_count, 2);
    }

    #[test]
    fn test_command_buffer_despawn() {
        let mut world = World::default();
        let entities = world.spawn_entities(POSITION, 5);

        world.queue_despawn_entity(entities[0]);
        world.queue_despawn_entity(entities[2]);
        world.queue_despawn_entity(entities[4]);

        assert_eq!(world.command_count(), 3);
        assert_eq!(world.get_all_entities().len(), 5);

        world.apply_commands();

        assert_eq!(world.command_count(), 0);
        assert_eq!(world.get_all_entities().len(), 2);

        let remaining = world.get_all_entities();
        assert!(remaining.contains(&entities[1]));
        assert!(remaining.contains(&entities[3]));
    }

    #[test]
    fn test_command_buffer_add_remove_components() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION, 1)[0];

        world.queue_add_components(entity, VELOCITY);
        assert_eq!(world.command_count(), 1);
        assert!(world.get_velocity(entity).is_none());

        world.apply_commands();
        assert!(world.get_velocity(entity).is_some());

        world.queue_remove_components(entity, VELOCITY);
        world.apply_commands();
        assert!(world.get_velocity(entity).is_none());
    }

    #[test]
    fn test_command_buffer_set_component() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION, 1)[0];

        world.queue_set_position(entity, Position { x: 100.0, y: 200.0 });
        assert_eq!(world.command_count(), 1);
        assert_ne!(world.get_position(entity).unwrap().x, 100.0);

        world.apply_commands();

        assert_eq!(world.command_count(), 0);
        let pos = world.get_position(entity).unwrap();
        assert_eq!(pos.x, 100.0);
        assert_eq!(pos.y, 200.0);
    }

    #[test]
    fn test_command_buffer_tags() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION, 1)[0];

        world.queue_add_player(entity);
        world.queue_add_active(entity);

        assert_eq!(world.command_count(), 2);
        assert!(!world.has_player(entity));
        assert!(!world.has_active(entity));

        world.apply_commands();

        assert!(world.has_player(entity));
        assert!(world.has_active(entity));

        world.queue_remove_player(entity);
        world.apply_commands();

        assert!(!world.has_player(entity));
        assert!(world.has_active(entity));
    }

    #[test]
    fn test_command_buffer_mixed_operations() {
        let mut world = World::default();
        let e1 = world.spawn_entities(POSITION, 1)[0];

        world.queue_spawn_entities(VELOCITY, 2);
        world.queue_add_components(e1, VELOCITY);
        world.queue_set_position(e1, Position { x: 50.0, y: 75.0 });
        world.queue_add_player(e1);

        assert_eq!(world.command_count(), 4);

        world.apply_commands();

        assert_eq!(world.command_count(), 0);
        assert_eq!(world.get_all_entities().len(), 3);
        assert!(world.get_velocity(e1).is_some());
        assert_eq!(world.get_position(e1).unwrap().x, 50.0);
        assert!(world.has_player(e1));
    }

    #[test]
    fn test_command_buffer_clear() {
        let mut world = World::default();

        world.queue_spawn_entities(POSITION, 5);
        world.queue_spawn_entities(VELOCITY, 3);

        assert_eq!(world.command_count(), 2);

        world.clear_commands();

        assert_eq!(world.command_count(), 0);
        assert_eq!(world.get_all_entities().len(), 0);
    }

    #[test]
    fn test_command_buffer_multiple_batches() {
        let mut world = World::default();

        world.queue_spawn_entities(POSITION, 2);
        world.apply_commands();
        assert_eq!(world.get_all_entities().len(), 2);

        world.queue_spawn_entities(VELOCITY, 3);
        world.apply_commands();
        assert_eq!(world.get_all_entities().len(), 5);

        world.queue_spawn_entities(POSITION | VELOCITY, 1);
        world.apply_commands();
        assert_eq!(world.get_all_entities().len(), 6);
    }

    #[test]
    fn test_command_buffer_despawn_batch() {
        let mut world = World::default();
        let entities = world.spawn_entities(POSITION, 10);

        let to_despawn = vec![entities[0], entities[3], entities[5], entities[9]];
        world.queue_despawn_entities(to_despawn.clone());

        assert_eq!(world.command_count(), 1);
        world.apply_commands();

        assert_eq!(world.get_all_entities().len(), 6);

        let remaining = world.get_all_entities();
        for &entity in &to_despawn {
            assert!(!remaining.contains(&entity));
        }
    }

    #[test]
    fn test_command_buffer_parallel_safety() {
        let mut world = World::default();
        let _ = world.spawn_entities(POSITION | VELOCITY, 100);

        let mut to_despawn = Vec::new();
        world.for_each(POSITION | VELOCITY, 0, |entity, table, idx| {
            if table.position[idx].x < 0.0 {
                to_despawn.push(entity);
            }
        });

        for entity in to_despawn {
            world.queue_despawn_entity(entity);
        }

        let command_count_before = world.command_count();
        world.apply_commands();

        assert_eq!(world.get_all_entities().len(), 100 - command_count_before);
    }

    #[test]
    fn test_ergonomic_query_builder() {
        let mut world = World::default();
        world.spawn_entities(POSITION | VELOCITY, 3);
        world.spawn_entities(POSITION, 2);

        let mut count = 0;
        world
            .query()
            .with(POSITION)
            .with(VELOCITY)
            .iter(|_entity, _table, _idx| {
                count += 1;
            });
        assert_eq!(count, 3);

        count = 0;
        world
            .query()
            .with(POSITION)
            .without(VELOCITY)
            .iter(|_entity, _table, _idx| {
                count += 1;
            });
        assert_eq!(count, 2);
    }

    #[test]
    fn test_ergonomic_query_builder_mut() {
        let mut world = World::default();
        let entities = world.spawn_entities(POSITION, 3);

        world
            .query_mut()
            .with(POSITION)
            .iter(|_entity, table, idx| {
                table.position[idx].x = 100.0;
            });

        for &entity in &entities {
            assert_eq!(world.get_position(entity).unwrap().x, 100.0);
        }
    }

    #[test]
    fn test_ergonomic_single_component_iter() {
        let mut world = World::default();
        world.spawn_entities(POSITION, 5);

        let mut count = 0;
        world.iter_position(|_entity, pos| {
            assert_eq!(pos.x, 0.0);
            count += 1;
        });
        assert_eq!(count, 5);
    }

    #[test]
    fn test_ergonomic_single_component_iter_mut() {
        let mut world = World::default();
        let entities = world.spawn_entities(POSITION, 5);

        world.iter_position_mut(|_entity, pos| {
            pos.x = 42.0;
        });

        for &entity in &entities {
            assert_eq!(world.get_position(entity).unwrap().x, 42.0);
        }
    }

    #[test]
    fn test_ergonomic_iter_with_entity() {
        let mut world = World::default();
        let e1 = world.spawn_entities(POSITION, 1)[0];
        let e2 = world.spawn_entities(POSITION, 1)[0];

        let mut found = vec![];
        world.iter_position(|entity, _pos| {
            found.push(entity);
        });

        assert_eq!(found.len(), 2);
        assert!(found.contains(&e1));
        assert!(found.contains(&e2));
    }

    #[test]
    fn test_ergonomic_batch_spawn() {
        let mut world = World::default();

        let entities = world.spawn_batch(POSITION | VELOCITY, 100, |table, idx| {
            table.position[idx] = Position {
                x: idx as f32,
                y: idx as f32 * 2.0,
            };
            table.velocity[idx] = Velocity { x: 1.0, y: -1.0 };
        });

        assert_eq!(entities.len(), 100);

        for (i, &entity) in entities.iter().enumerate() {
            let pos = world.get_position(entity).unwrap();
            assert_eq!(pos.x, i as f32);
            assert_eq!(pos.y, i as f32 * 2.0);

            let vel = world.get_velocity(entity).unwrap();
            assert_eq!(vel.x, 1.0);
            assert_eq!(vel.y, -1.0);
        }
    }

    #[test]
    fn test_ergonomic_query_with_tags() {
        let mut world = World::default();
        let e1 = world.spawn_entities(POSITION, 1)[0];
        let e2 = world.spawn_entities(POSITION, 1)[0];
        let _e3 = world.spawn_entities(POSITION, 1)[0];

        world.add_player(e1);
        world.add_player(e2);

        let mut count = 0;
        world
            .query()
            .with(POSITION | PLAYER)
            .iter(|_entity, _table, _idx| {
                count += 1;
            });
        assert_eq!(count, 2);

        count = 0;
        world
            .query()
            .with(POSITION)
            .without(PLAYER)
            .iter(|_entity, _table, _idx| {
                count += 1;
            });
        assert_eq!(count, 1);
    }

    #[test]
    fn test_query_builder_mut_without() {
        let mut world = World::default();
        let e1 = world.spawn_entities(POSITION | VELOCITY, 1)[0];
        let e2 = world.spawn_entities(POSITION, 1)[0];

        world.set_position(e1, Position { x: 1.0, y: 2.0 });
        world.set_position(e2, Position { x: 3.0, y: 4.0 });

        let mut count = 0;
        world
            .query_mut()
            .with(POSITION)
            .without(VELOCITY)
            .iter(|_entity, _table, _idx| {
                count += 1;
            });
        assert_eq!(count, 1);
    }

    #[test]
    fn test_iter_methods() {
        let mut world = World::default();
        let e1 = world.spawn_entities(POSITION | VELOCITY | HEALTH, 1)[0];
        let e2 = world.spawn_entities(POSITION | VELOCITY, 1)[0];

        world.set_position(e1, Position { x: 1.0, y: 2.0 });
        world.set_position(e2, Position { x: 3.0, y: 4.0 });
        world.set_velocity(e1, Velocity { x: 1.0, y: 1.0 });
        world.set_velocity(e2, Velocity { x: 2.0, y: 2.0 });
        world.set_health(e1, Health { value: 100.0 });

        let mut sum_x = 0.0;
        world.iter_position(|_entity, pos| {
            sum_x += pos.x;
        });
        assert_eq!(sum_x, 4.0);

        world.iter_position_mut(|_entity, pos| {
            pos.x *= 2.0;
        });

        let mut count = 0;
        world.iter_velocity(|_entity, _vel| {
            count += 1;
        });
        assert_eq!(count, 2);

        world.iter_velocity_mut(|_entity, vel| {
            vel.x *= 2.0;
        });

        world.iter_health_mut(|_entity, health| {
            health.value += 10.0;
        });

        let mut health_sum = 0.0;
        world.iter_health(|_entity, health| {
            health_sum += health.value;
        });
        assert_eq!(health_sum, 110.0);

        let e3 = world.spawn_entities(PARENT | NODE, 1)[0];
        world.set_parent(e3, Parent(e1));
        world.set_node(
            e3,
            Node {
                id: e3,
                parent: Some(e1),
                children: Vec::new(),
            },
        );

        let mut parent_count = 0;
        world.iter_parent(|_entity, _parent| {
            parent_count += 1;
        });
        assert_eq!(parent_count, 1);

        let mut node_count = 0;
        world.iter_node(|_entity, _node| {
            node_count += 1;
        });
        assert_eq!(node_count, 1);

        world.iter_parent_mut(|_entity, parent| {
            parent.0 = e2;
        });

        world.iter_node_mut(|_entity, _node| {});
    }

    #[test]
    fn test_schedule_basic() {
        let mut world = World::default();
        world.resources._delta_time = 0.016;

        let mut schedule = Schedule::new();
        schedule.push("tick", |world: &mut World| {
            world.resources._delta_time += 0.016;
        });

        schedule.run(&mut world);
        assert_eq!(world.resources._delta_time, 0.032);

        schedule.run(&mut world);
        assert_eq!(world.resources._delta_time, 0.048);
    }

    #[test]
    fn test_schedule_multiple_systems() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION | VELOCITY, 1)[0];
        world.set_position(entity, Position { x: 0.0, y: 0.0 });
        world.set_velocity(entity, Velocity { x: 10.0, y: 5.0 });
        world.resources._delta_time = 0.1;

        let mut schedule = Schedule::new();

        schedule.push("physics", |world: &mut World| {
            let dt = world.resources._delta_time;
            let updates: Vec<(Entity, Velocity)> = world
                .query_entities(POSITION | VELOCITY)
                .filter_map(|entity| world.get_velocity(entity).map(|vel| (entity, vel.clone())))
                .collect();

            for (entity, vel) in updates {
                if let Some(pos) = world.get_position_mut(entity) {
                    pos.x += vel.x * dt;
                    pos.y += vel.y * dt;
                }
            }
        });

        schedule.push("double_dt", |world: &mut World| {
            world.resources._delta_time *= 2.0;
        });

        schedule.run(&mut world);

        let pos = world.get_position(entity).unwrap();
        assert_eq!(pos.x, 1.0);
        assert_eq!(pos.y, 0.5);
        assert_eq!(world.resources._delta_time, 0.2);
    }

    #[test]
    fn test_schedule_builder_pattern() {
        let mut world = World::default();
        world.resources._delta_time = 1.0;

        let mut schedule = Schedule::new();
        schedule
            .push("add", |world: &mut World| {
                world.resources._delta_time += 1.0;
            })
            .push("multiply", |world: &mut World| {
                world.resources._delta_time *= 2.0;
            });

        schedule.run(&mut world);
        assert_eq!(world.resources._delta_time, 4.0);
    }

    #[test]
    fn test_schedule_system_order() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION, 1)[0];
        world.set_position(entity, Position { x: 0.0, y: 0.0 });

        let mut schedule = Schedule::new();

        schedule.push("set_x", |world: &mut World| {
            let entities: Vec<Entity> = world.query_entities(POSITION).collect();
            if let Some(pos) = world.get_position_mut(entities[0]) {
                pos.x = 10.0;
            }
        });

        schedule.push("double_x", |world: &mut World| {
            let entities: Vec<Entity> = world.query_entities(POSITION).collect();
            if let Some(pos) = world.get_position_mut(entities[0]) {
                pos.x *= 2.0;
            }
        });

        schedule.run(&mut world);

        let pos = world.get_position(entity).unwrap();
        assert_eq!(pos.x, 20.0);
    }

    #[test]
    fn test_schedule_with_components() {
        let mut world = World::default();
        let entity = world.spawn_entities(HEALTH, 1)[0];
        world.set_health(entity, Health { value: 100.0 });

        let mut schedule = Schedule::new();

        schedule.push("damage", |world: &mut World| {
            let entities: Vec<Entity> = world.query_entities(HEALTH).collect();
            for entity in entities {
                if let Some(health) = world.get_health_mut(entity) {
                    health.value -= 10.0;
                }
            }
        });

        schedule.push("decay", |world: &mut World| {
            let entities: Vec<Entity> = world.query_entities(HEALTH).collect();
            for entity in entities {
                if let Some(health) = world.get_health_mut(entity) {
                    health.value *= 0.9;
                }
            }
        });

        schedule.run(&mut world);

        let health = world.get_health(entity).unwrap();
        assert_eq!(health.value, 81.0);
    }

    #[test]
    fn test_schedule_insert_before() {
        let mut world = World::default();
        world.resources._delta_time = 1.0;

        let mut schedule = Schedule::new();
        schedule.push("first", |world: &mut World| {
            world.resources._delta_time += 10.0;
        });
        schedule.push("third", |world: &mut World| {
            world.resources._delta_time *= 3.0;
        });
        schedule.insert_before("third", "second", |world: &mut World| {
            world.resources._delta_time *= 2.0;
        });

        schedule.run(&mut world);
        assert_eq!(world.resources._delta_time, 66.0);
    }

    #[test]
    fn test_schedule_insert_after() {
        let mut world = World::default();
        world.resources._delta_time = 1.0;

        let mut schedule = Schedule::new();
        schedule.push("first", |world: &mut World| {
            world.resources._delta_time += 10.0;
        });
        schedule.push("third", |world: &mut World| {
            world.resources._delta_time *= 3.0;
        });
        schedule.insert_after("first", "second", |world: &mut World| {
            world.resources._delta_time *= 2.0;
        });

        schedule.run(&mut world);
        assert_eq!(world.resources._delta_time, 66.0);
    }

    #[test]
    fn test_schedule_remove() {
        let mut world = World::default();
        world.resources._delta_time = 1.0;

        let mut schedule = Schedule::new();
        schedule.push("add", |world: &mut World| {
            world.resources._delta_time += 10.0;
        });
        schedule.push("multiply", |world: &mut World| {
            world.resources._delta_time *= 5.0;
        });

        schedule.remove("multiply");
        schedule.run(&mut world);
        assert_eq!(world.resources._delta_time, 11.0);
    }

    #[test]
    fn test_schedule_remove_nonexistent() {
        let mut schedule: Schedule<World> = Schedule::new();
        schedule.push("a", |_world: &mut World| {});
        schedule.remove("nonexistent");
        assert!(schedule.contains("a"));
    }

    #[test]
    fn test_schedule_contains() {
        let mut schedule: Schedule<World> = Schedule::new();
        assert!(!schedule.contains("physics"));

        schedule.push("physics", |_world: &mut World| {});
        assert!(schedule.contains("physics"));
        assert!(!schedule.contains("render"));

        schedule.remove("physics");
        assert!(!schedule.contains("physics"));
    }

    #[test]
    #[should_panic(expected = "system \"nonexistent\" not found")]
    fn test_schedule_insert_before_panics() {
        let mut schedule: Schedule<World> = Schedule::new();
        schedule.insert_before("nonexistent", "new", |_world: &mut World| {});
    }

    #[test]
    #[should_panic(expected = "system \"nonexistent\" not found")]
    fn test_schedule_insert_after_panics() {
        let mut schedule: Schedule<World> = Schedule::new();
        schedule.insert_after("nonexistent", "new", |_world: &mut World| {});
    }

    #[test]
    fn test_schedule_ordering_verification() {
        let order = std::sync::Arc::new(std::sync::Mutex::new(Vec::<&'static str>::new()));

        let mut schedule: Schedule<World> = Schedule::new();
        let order_clone = order.clone();
        schedule.push("a", move |_world: &mut World| {
            order_clone.lock().unwrap().push("a");
        });
        let order_clone = order.clone();
        schedule.push("c", move |_world: &mut World| {
            order_clone.lock().unwrap().push("c");
        });
        let order_clone = order.clone();
        schedule.insert_before("c", "b", move |_world: &mut World| {
            order_clone.lock().unwrap().push("b");
        });
        let order_clone = order.clone();
        schedule.insert_after("c", "d", move |_world: &mut World| {
            order_clone.lock().unwrap().push("d");
        });

        let mut world = World::default();
        schedule.run(&mut world);
        assert_eq!(*order.lock().unwrap(), vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn test_schedule_readonly_wrapper() {
        let mut world = World::default();
        world.resources._delta_time = 42.0;

        fn read_system(world: &World) -> f32 {
            world.resources._delta_time
        }

        let observed = std::sync::Arc::new(std::sync::Mutex::new(0.0_f32));
        let observed_clone = observed.clone();

        let mut schedule = Schedule::new();
        schedule.push("reader", move |w: &mut World| {
            *observed_clone.lock().unwrap() = read_system(w);
        });

        schedule.run(&mut world);
        assert_eq!(*observed.lock().unwrap(), 42.0);
    }

    #[test]
    #[should_panic(expected = "already exists")]
    fn test_schedule_push_duplicate_panics() {
        let mut schedule: Schedule<World> = Schedule::new();
        schedule.push("a", |_w: &mut World| {});
        schedule.push("a", |_w: &mut World| {});
    }

    #[test]
    #[should_panic(expected = "already exists")]
    fn test_schedule_insert_before_duplicate_panics() {
        let mut schedule: Schedule<World> = Schedule::new();
        schedule.push("a", |_w: &mut World| {});
        schedule.push("b", |_w: &mut World| {});
        schedule.insert_before("b", "a", |_w: &mut World| {});
    }

    #[test]
    #[should_panic(expected = "already exists")]
    fn test_schedule_insert_after_duplicate_panics() {
        let mut schedule: Schedule<World> = Schedule::new();
        schedule.push("a", |_w: &mut World| {});
        schedule.insert_after("a", "a", |_w: &mut World| {});
    }

    #[test]
    fn test_schedule_replace() {
        let mut world = World::default();
        world.resources._delta_time = 1.0;

        let mut schedule = Schedule::new();
        schedule.push("add", |world: &mut World| {
            world.resources._delta_time += 10.0;
        });

        schedule.run(&mut world);
        assert_eq!(world.resources._delta_time, 11.0);

        schedule.replace("add", |world: &mut World| {
            world.resources._delta_time += 100.0;
        });

        schedule.run(&mut world);
        assert_eq!(world.resources._delta_time, 111.0);
    }

    #[test]
    #[should_panic(expected = "system \"nonexistent\" not found")]
    fn test_schedule_replace_panics_on_missing() {
        let mut schedule: Schedule<World> = Schedule::new();
        schedule.replace("nonexistent", |_w: &mut World| {});
    }

    #[test]
    fn test_schedule_replace_preserves_order() {
        let mut world = World::default();
        world.resources._delta_time = 0.0;

        let mut schedule = Schedule::new();
        schedule.push("first", |world: &mut World| {
            world.resources._delta_time += 1.0;
        });
        schedule.push("second", |world: &mut World| {
            world.resources._delta_time *= 10.0;
        });
        schedule.push("third", |world: &mut World| {
            world.resources._delta_time += 5.0;
        });

        schedule.replace("second", |world: &mut World| {
            world.resources._delta_time *= 100.0;
        });

        schedule.run(&mut world);
        assert_eq!(world.resources._delta_time, 105.0);
    }

    #[test]
    fn test_schedule_names() {
        let mut schedule: Schedule<World> = Schedule::new();
        schedule.push("a", |_w: &mut World| {});
        schedule.push("b", |_w: &mut World| {});
        schedule.push("c", |_w: &mut World| {});

        let names: Vec<&str> = schedule.names().collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_schedule_len_and_is_empty() {
        let mut schedule: Schedule<World> = Schedule::new();
        assert!(schedule.is_empty());
        assert_eq!(schedule.len(), 0);

        schedule.push("a", |_w: &mut World| {});
        assert!(!schedule.is_empty());
        assert_eq!(schedule.len(), 1);

        schedule.push("b", |_w: &mut World| {});
        assert_eq!(schedule.len(), 2);

        schedule.remove("a");
        assert_eq!(schedule.len(), 1);

        schedule.remove("b");
        assert!(schedule.is_empty());
    }

    #[test]
    fn test_schedule_remove_returns_bool() {
        let mut schedule: Schedule<World> = Schedule::new();
        schedule.push("a", |_w: &mut World| {});

        assert!(schedule.remove("a"));
        assert!(!schedule.remove("a"));
        assert!(!schedule.remove("nonexistent"));
    }

    #[test]
    fn test_schedule_remove_then_push_reuses_name() {
        let mut world = World::default();
        world.resources._delta_time = 0.0;

        let mut schedule = Schedule::new();
        schedule.push("sys", |world: &mut World| {
            world.resources._delta_time += 1.0;
        });

        schedule.remove("sys");
        schedule.push("sys", |world: &mut World| {
            world.resources._delta_time += 100.0;
        });

        schedule.run(&mut world);
        assert_eq!(world.resources._delta_time, 100.0);
    }

    #[test]
    fn test_schedule_insert_before_first() {
        let mut schedule: Schedule<World> = Schedule::new();
        schedule.push("b", |_w: &mut World| {});
        schedule.insert_before("b", "a", |_w: &mut World| {});

        let names: Vec<&str> = schedule.names().collect();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn test_schedule_insert_after_last() {
        let mut schedule: Schedule<World> = Schedule::new();
        schedule.push("a", |_w: &mut World| {});
        schedule.insert_after("a", "b", |_w: &mut World| {});

        let names: Vec<&str> = schedule.names().collect();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn test_schedule_push_readonly() {
        let mut world = World::default();
        world.resources._delta_time = 42.0;

        let observed = std::sync::Arc::new(std::sync::Mutex::new(0.0_f32));
        let obs = observed.clone();

        let mut schedule = Schedule::new();
        schedule.push_readonly("reader", move |world: &World| {
            *obs.lock().unwrap() = world.resources._delta_time;
        });

        schedule.run(&mut world);
        assert_eq!(*observed.lock().unwrap(), 42.0);
    }

    #[test]
    fn test_query_cache_persistence_on_new_archetype() {
        let mut world = World::default();

        world.spawn_entities(POSITION, 5);

        let mut count1 = 0;
        world.for_each_mut(POSITION, 0, |_entity, _table, _idx| {
            count1 += 1;
        });
        assert_eq!(count1, 5);

        let cache_size_before = world.query_cache.len();
        assert_eq!(cache_size_before, 1);

        world.spawn_entities(POSITION | VELOCITY, 3);

        let cache_size_after = world.query_cache.len();
        assert_eq!(cache_size_after, 1);

        let mut count2 = 0;
        world.for_each_mut(POSITION, 0, |_entity, _table, _idx| {
            count2 += 1;
        });
        assert_eq!(count2, 8);

        world.spawn_entities(POSITION | HEALTH, 2);

        let mut count3 = 0;
        world.for_each_mut(POSITION, 0, |_entity, _table, _idx| {
            count3 += 1;
        });
        assert_eq!(count3, 10);
    }

    #[test]
    fn test_multi_component_add_performance() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION, 1)[0];
        let source_table_index =
            world.entity_locations.get(entity.id).unwrap().table_index as usize;

        world.add_components(entity, VELOCITY | HEALTH);

        assert!(world.get_position(entity).is_some());
        assert!(world.get_velocity(entity).is_some());
        assert!(world.get_health(entity).is_some());

        let cache_entry_in_source = world.table_edges[source_table_index]
            .multi_add_cache
            .get(&(VELOCITY | HEALTH))
            .copied();
        assert!(cache_entry_in_source.is_some());

        let entity2 = world.spawn_entities(POSITION, 1)[0];
        let source_table_index2 =
            world.entity_locations.get(entity2.id).unwrap().table_index as usize;

        assert_eq!(source_table_index, source_table_index2);

        world.add_components(entity2, VELOCITY | HEALTH);

        let cache_hit = world.table_edges[source_table_index2]
            .multi_add_cache
            .get(&(VELOCITY | HEALTH))
            .copied();
        assert_eq!(cache_hit, cache_entry_in_source);
    }

    #[test]
    fn test_multi_component_remove_performance() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION | VELOCITY | HEALTH, 1)[0];
        let source_table_index =
            world.entity_locations.get(entity.id).unwrap().table_index as usize;

        world.remove_components(entity, VELOCITY | HEALTH);

        assert!(world.get_position(entity).is_some());
        assert!(world.get_velocity(entity).is_none());
        assert!(world.get_health(entity).is_none());

        let cache_entry_in_source = world.table_edges[source_table_index]
            .multi_remove_cache
            .get(&(VELOCITY | HEALTH))
            .copied();
        assert!(cache_entry_in_source.is_some());

        let entity2 = world.spawn_entities(POSITION | VELOCITY | HEALTH, 1)[0];
        let source_table_index2 =
            world.entity_locations.get(entity2.id).unwrap().table_index as usize;

        assert_eq!(source_table_index, source_table_index2);

        world.remove_components(entity2, VELOCITY | HEALTH);

        let cache_hit = world.table_edges[source_table_index2]
            .multi_remove_cache
            .get(&(VELOCITY | HEALTH))
            .copied();
        assert_eq!(cache_hit, cache_entry_in_source);
    }

    #[test]
    fn test_query_cache_invalidation_selective() {
        let mut world = World::default();

        world.spawn_entities(POSITION, 10);
        world.spawn_entities(VELOCITY, 5);

        let mut pos_count1 = 0;
        world.for_each_mut(POSITION, 0, |_entity, _table, _idx| {
            pos_count1 += 1;
        });
        let mut vel_count1 = 0;
        world.for_each_mut(VELOCITY, 0, |_entity, _table, _idx| {
            vel_count1 += 1;
        });
        assert_eq!(pos_count1, 10);
        assert_eq!(vel_count1, 5);

        let cache_size_before = world.query_cache.len();
        assert_eq!(cache_size_before, 2);

        world.spawn_entities(POSITION | VELOCITY, 3);

        let cache_size_after_archetype = world.query_cache.len();
        assert_eq!(cache_size_after_archetype, 2);

        let mut pos_count2 = 0;
        world.for_each_mut(POSITION, 0, |_entity, _table, _idx| {
            pos_count2 += 1;
        });
        let mut vel_count2 = 0;
        world.for_each_mut(VELOCITY, 0, |_entity, _table, _idx| {
            vel_count2 += 1;
        });
        assert_eq!(pos_count2, 13);
        assert_eq!(vel_count2, 8);

        world.spawn_entities(HEALTH, 2);

        let cache_size_after_health = world.query_cache.len();
        assert_eq!(cache_size_after_health, 2);

        let mut pos_count3 = 0;
        world.for_each_mut(POSITION, 0, |_entity, _table, _idx| {
            pos_count3 += 1;
        });
        let mut vel_count3 = 0;
        world.for_each_mut(VELOCITY, 0, |_entity, _table, _idx| {
            vel_count3 += 1;
        });
        let mut health_count = 0;
        world.for_each_mut(HEALTH, 0, |_entity, _table, _idx| {
            health_count += 1;
        });
        assert_eq!(pos_count3, 13);
        assert_eq!(vel_count3, 8);
        assert_eq!(health_count, 2);

        let final_cache_size = world.query_cache.len();
        assert_eq!(final_cache_size, 3);
    }

    #[test]
    fn test_entity_has_components_requires_all() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION, 1)[0];

        assert!(
            !world.entity_has_components(entity, POSITION | VELOCITY),
            "Should return false when entity only has POSITION but query asks for POSITION | VELOCITY"
        );

        assert!(
            world.entity_has_components(entity, POSITION),
            "Should return true when entity has the single queried component"
        );

        world.add_components(entity, VELOCITY);
        assert!(
            world.entity_has_components(entity, POSITION | VELOCITY),
            "Should return true when entity has all queried components"
        );

        assert!(
            !world.entity_has_components(entity, POSITION | VELOCITY | HEALTH),
            "Should return false when entity is missing one of three queried components"
        );
    }

    #[test]
    fn test_entity_has_generated_methods_consistency() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION | VELOCITY, 1)[0];

        assert!(world.entity_has_position(entity));
        assert!(world.entity_has_velocity(entity));
        assert!(!world.entity_has_health(entity));

        assert!(world.entity_has_components(entity, POSITION));
        assert!(world.entity_has_components(entity, VELOCITY));
        assert!(world.entity_has_components(entity, POSITION | VELOCITY));
        assert!(!world.entity_has_components(entity, HEALTH));
        assert!(!world.entity_has_components(entity, POSITION | HEALTH));
    }

    #[test]
    fn test_despawn_preserves_change_vec_consistency() {
        let mut world = World::default();
        let entities = world.spawn_entities(POSITION, 5);

        world.step();

        world.get_position_mut(entities[0]).unwrap().x = 10.0;
        world.get_position_mut(entities[4]).unwrap().x = 40.0;

        world.despawn_entities(&[entities[2]]);

        for table in &world.tables {
            if table.mask & POSITION != 0 {
                let entity_count = table.entity_indices.len();
                assert_eq!(
                    table.position.len(),
                    entity_count,
                    "Position vec length should match entity count after despawn"
                );
                assert_eq!(
                    table.position_changed.len(),
                    entity_count,
                    "Position changed vec length should match entity count after despawn"
                );
            }
        }
    }

    #[test]
    fn test_change_detection_after_despawn() {
        let mut world = World::default();
        let e1 = world.spawn_entities(POSITION, 1)[0];
        let e2 = world.spawn_entities(POSITION, 1)[0];
        let e3 = world.spawn_entities(POSITION, 1)[0];

        world.set_position(e1, Position { x: 1.0, y: 0.0 });
        world.set_position(e2, Position { x: 2.0, y: 0.0 });
        world.set_position(e3, Position { x: 3.0, y: 0.0 });

        world.step();

        world.get_position_mut(e3).unwrap().x = 30.0;

        world.despawn_entities(&[e2]);

        let mut changed_entities = Vec::new();
        world.for_each_mut_changed(POSITION, 0, |entity, _table, _idx| {
            changed_entities.push(entity);
        });

        assert!(
            changed_entities.contains(&e3),
            "e3 was modified and should be detected as changed"
        );
        assert!(
            !changed_entities.contains(&e1),
            "e1 was not modified and should not be detected as changed"
        );
    }

    #[test]
    fn test_change_detection_only_checks_queried_components() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION | VELOCITY, 1)[0];

        world.step();

        world.get_velocity_mut(entity).unwrap().x = 99.0;

        let mut pos_changed_count = 0;
        world.for_each_mut_changed(POSITION, 0, |_entity, _table, _idx| {
            pos_changed_count += 1;
        });
        assert_eq!(
            pos_changed_count, 0,
            "Changing velocity should not trigger position change detection"
        );

        let mut vel_changed_count = 0;
        world.for_each_mut_changed(VELOCITY, 0, |_entity, _table, _idx| {
            vel_changed_count += 1;
        });
        assert_eq!(
            vel_changed_count, 1,
            "Changing velocity should trigger velocity change detection"
        );
    }

    #[test]
    fn test_change_detection_multi_component_query_only_relevant() {
        let mut world = World::default();
        let e1 = world.spawn_entities(POSITION | VELOCITY | HEALTH, 1)[0];
        let e2 = world.spawn_entities(POSITION | VELOCITY | HEALTH, 1)[0];

        world.step();

        world.get_position_mut(e1).unwrap().x = 5.0;
        world.get_health_mut(e2).unwrap().value = 50.0;

        let mut changed_entities = Vec::new();
        world.for_each_mut_changed(POSITION | VELOCITY, 0, |entity, _table, _idx| {
            changed_entities.push(entity);
        });

        assert!(
            changed_entities.contains(&e1),
            "e1 had position changed, which is in the query"
        );
        assert!(
            !changed_entities.contains(&e2),
            "e2 only had health changed, which is NOT in the query"
        );
    }

    #[test]
    fn test_entity_count() {
        let mut world = World::default();
        assert_eq!(world.entity_count(), 0);

        world.spawn_entities(POSITION, 5);
        assert_eq!(world.entity_count(), 5);

        world.spawn_entities(VELOCITY, 3);
        assert_eq!(world.entity_count(), 8);

        let entities = world.spawn_entities(POSITION | VELOCITY, 2);
        assert_eq!(world.entity_count(), 10);

        world.despawn_entities(&[entities[0]]);
        assert_eq!(world.entity_count(), 9);
    }

    #[test]
    fn test_entity_count_matches_get_all_entities() {
        let mut world = World::default();

        world.spawn_entities(POSITION, 10);
        world.spawn_entities(VELOCITY, 5);
        world.spawn_entities(POSITION | VELOCITY | HEALTH, 3);

        assert_eq!(world.entity_count(), world.get_all_entities().len());
    }

    #[test]
    fn test_entity_query_iter_size_hint() {
        let mut world = World::default();

        world.spawn_entities(POSITION | VELOCITY, 5);
        world.spawn_entities(POSITION, 3);
        world.spawn_entities(VELOCITY | HEALTH, 2);

        let iter = world.query_entities(POSITION);
        let (lower, upper) = iter.size_hint();
        assert_eq!(lower, 8);
        assert_eq!(upper, Some(8));

        let count = world.query_entities(POSITION).count();
        assert_eq!(count, 8);

        let iter = world.query_entities(POSITION | VELOCITY);
        let (lower, upper) = iter.size_hint();
        assert_eq!(lower, 5);
        assert_eq!(upper, Some(5));
    }

    #[test]
    fn test_entity_query_iter_size_hint_decreases() {
        let mut world = World::default();
        world.spawn_entities(POSITION, 3);

        let mut iter = world.query_entities(POSITION);
        assert_eq!(iter.size_hint(), (3, Some(3)));

        iter.next();
        assert_eq!(iter.size_hint(), (2, Some(2)));

        iter.next();
        assert_eq!(iter.size_hint(), (1, Some(1)));

        iter.next();
        assert_eq!(iter.size_hint(), (0, Some(0)));
    }

    #[test]
    fn test_component_query_iter_size_hint() {
        let mut world = World::default();
        world.spawn_entities(POSITION, 4);
        world.spawn_entities(POSITION | VELOCITY, 3);

        let iter = world.query_position();
        let (lower, upper) = iter.size_hint();
        assert_eq!(lower, 7);
        assert_eq!(upper, Some(7));

        let count = world.query_position().count();
        assert_eq!(count, 7);
    }

    #[test]
    fn test_query_entities_collect_preallocates() {
        let mut world = World::default();
        world.spawn_entities(POSITION | VELOCITY, 100);

        let entities: Vec<Entity> = world.query_entities(POSITION | VELOCITY).collect();
        assert_eq!(entities.len(), 100);
    }

    #[test]
    fn test_despawn_multiple_same_table_change_vecs() {
        let mut world = World::default();
        let entities = world.spawn_entities(POSITION | VELOCITY, 6);

        world.step();

        world.get_position_mut(entities[0]).unwrap().x = 10.0;
        world.get_position_mut(entities[5]).unwrap().x = 50.0;

        world.despawn_entities(&[entities[1], entities[3]]);

        for table in &world.tables {
            if table.mask & POSITION != 0 {
                let entity_count = table.entity_indices.len();
                assert_eq!(table.position.len(), entity_count);
                assert_eq!(table.position_changed.len(), entity_count);
            }
            if table.mask & VELOCITY != 0 {
                let entity_count = table.entity_indices.len();
                assert_eq!(table.velocity.len(), entity_count);
                assert_eq!(table.velocity_changed.len(), entity_count);
            }
        }

        let remaining = world.get_all_entities();
        assert_eq!(remaining.len(), 4);
        assert!(world.get_position(entities[0]).is_some());
        assert!(world.get_position(entities[5]).is_some());
    }

    mod cfg_test {
        #[derive(Default, Debug, Clone)]
        pub struct BaseComponent {
            pub value: i32,
        }

        #[derive(Default, Debug, Clone)]
        pub struct DebugComponent {
            pub debug_value: i32,
        }

        crate::ecs! {
            CfgWorld {
                base: BaseComponent => BASE,
                #[cfg(debug_assertions)]
                debug_only: DebugComponent => DEBUG_ONLY,
            }
            CfgResources {
                counter: i32,
            }
        }

        #[test]
        fn test_cfg_attribute_on_components() {
            let mut world = CfgWorld::default();

            let entities = world.spawn_entities(BASE, 3);
            assert_eq!(entities.len(), 3);

            world.set_base(entities[0], BaseComponent { value: 10 });
            assert_eq!(world.get_base(entities[0]).unwrap().value, 10);

            world.resources.counter = 42;
            assert_eq!(world.resources.counter, 42);

            let mut count = 0;
            world.query().with(BASE).iter(|_entity, _table, _idx| {
                count += 1;
            });
            assert_eq!(count, 3);

            world.query_mut().with(BASE).iter(|_entity, table, idx| {
                table.base[idx].value += 1;
            });
            assert_eq!(world.get_base(entities[0]).unwrap().value, 11);

            let mut without_count = 0;
            world
                .query()
                .with(BASE)
                .without(0)
                .iter(|_entity, _table, _idx| {
                    without_count += 1;
                });
            assert_eq!(without_count, 3);

            let mut without_mut_count = 0;
            world
                .query_mut()
                .with(BASE)
                .without(0)
                .iter(|_entity, _table, _idx| {
                    without_mut_count += 1;
                });
            assert_eq!(without_mut_count, 3);

            let mut iter_count = 0;
            world.iter_base(|_entity, _base| {
                iter_count += 1;
            });
            assert_eq!(iter_count, 3);

            world.iter_base_mut(|_entity, base| {
                base.value *= 2;
            });
            assert_eq!(world.get_base(entities[0]).unwrap().value, 22);

            #[cfg(debug_assertions)]
            {
                world.set_debug_only(entities[0], DebugComponent { debug_value: 42 });
                assert_eq!(world.get_debug_only(entities[0]).unwrap().debug_value, 42);

                let debug_entities = world.spawn_entities(DEBUG_ONLY, 2);
                assert_eq!(debug_entities.len(), 2);

                let mut debug_count = 0;
                world.iter_debug_only(|_entity, _debug| {
                    debug_count += 1;
                });
                assert_eq!(debug_count, 3);

                world.iter_debug_only_mut(|_entity, debug| {
                    debug.debug_value += 1;
                });
                assert_eq!(world.get_debug_only(entities[0]).unwrap().debug_value, 43);
            }
        }
    }

    mod multi_world_test {
        use super::*;

        #[derive(Default, Debug, Clone, PartialEq)]
        pub struct Position {
            pub x: f32,
            pub y: f32,
        }

        #[derive(Default, Debug, Clone, PartialEq)]
        pub struct Velocity {
            pub x: f32,
            pub y: f32,
        }

        #[derive(Default, Debug, Clone, PartialEq)]
        pub struct Sprite {
            pub id: u32,
        }

        #[derive(Default, Debug, Clone, PartialEq)]
        pub struct Color {
            pub r: f32,
            pub g: f32,
            pub b: f32,
        }

        #[derive(Debug, Clone)]
        #[allow(dead_code)]
        pub struct CollisionEvent {
            pub entity_a: Entity,
            pub entity_b: Entity,
        }

        crate::ecs! {
            GameEcs {
                CoreWorld {
                    position: Position => MW_POSITION,
                    velocity: Velocity => MW_VELOCITY,
                }
                RenderWorld {
                    sprite: Sprite => MW_SPRITE,
                    color: Color => MW_COLOR,
                }
            }
            Tags {
                player => MW_PLAYER,
            }
            Events {
                collision: CollisionEvent,
            }
            GameResources {
                delta_time: f32,
            }
        }

        #[test]
        fn test_multi_world_spawn() {
            let mut ecs = GameEcs::default();
            let entity = ecs.spawn();
            assert_eq!(entity.id, 0);
            assert_eq!(entity.generation, 0);
        }

        #[test]
        fn test_multi_world_per_world_components() {
            let mut ecs = GameEcs::default();
            let entity = ecs.spawn();

            ecs.core_world
                .add_components(entity, MW_POSITION | MW_VELOCITY);
            ecs.core_world
                .set_position(entity, Position { x: 1.0, y: 2.0 });

            assert_eq!(ecs.core_world.get_position(entity).unwrap().x, 1.0);
            assert_eq!(ecs.core_world.get_position(entity).unwrap().y, 2.0);
            assert!(ecs.core_world.get_velocity(entity).is_some());
        }

        #[test]
        fn test_multi_world_cross_world_access() {
            let mut ecs = GameEcs::default();
            let entity = ecs.spawn();

            ecs.core_world
                .set_position(entity, Position { x: 5.0, y: 10.0 });
            ecs.render_world.set_sprite(entity, Sprite { id: 42 });

            assert_eq!(ecs.core_world.get_position(entity).unwrap().x, 5.0);
            assert_eq!(ecs.render_world.get_sprite(entity).unwrap().id, 42);
        }

        #[test]
        fn test_multi_world_split_borrow() {
            let mut ecs = GameEcs::default();
            let entity = ecs.spawn();

            ecs.core_world
                .set_position(entity, Position { x: 1.0, y: 2.0 });
            ecs.render_world.set_sprite(entity, Sprite { id: 1 });

            let GameEcs {
                core_world,
                render_world,
                ..
            } = &mut ecs;
            core_world.for_each_mut(MW_POSITION, 0, |entity, table, idx| {
                if let Some(sprite) = render_world.get_sprite(entity) {
                    table.position[idx].x += sprite.id as f32;
                }
            });

            assert_eq!(ecs.core_world.get_position(entity).unwrap().x, 2.0);
        }

        #[test]
        fn test_multi_world_despawn_cascades() {
            let mut ecs = GameEcs::default();
            let entity = ecs.spawn();

            ecs.core_world
                .set_position(entity, Position { x: 1.0, y: 2.0 });
            ecs.render_world.set_sprite(entity, Sprite { id: 1 });
            ecs.add_player(entity);

            assert!(ecs.core_world.get_position(entity).is_some());
            assert!(ecs.render_world.get_sprite(entity).is_some());
            assert!(ecs.has_player(entity));

            ecs.despawn(entity);

            assert!(ecs.core_world.get_position(entity).is_none());
            assert!(ecs.render_world.get_sprite(entity).is_none());
            assert!(!ecs.has_player(entity));
        }

        #[test]
        fn test_multi_world_entity_in_one_world_only() {
            let mut ecs = GameEcs::default();
            let entity = ecs.spawn();

            ecs.core_world
                .set_position(entity, Position { x: 1.0, y: 2.0 });

            assert!(ecs.core_world.get_position(entity).is_some());
            assert!(ecs.render_world.get_sprite(entity).is_none());
            assert!(ecs.render_world.get_color(entity).is_none());
        }

        #[test]
        fn test_multi_world_entity_in_no_world() {
            let mut ecs = GameEcs::default();
            let entity = ecs.spawn();

            assert!(ecs.core_world.get_position(entity).is_none());
            assert!(ecs.render_world.get_sprite(entity).is_none());
        }

        #[test]
        fn test_multi_world_entity_builder() {
            let mut ecs = GameEcs::default();
            let entities = EntityBuilder::new()
                .with_position(Position { x: 1.0, y: 2.0 })
                .with_sprite(Sprite { id: 42 })
                .spawn(&mut ecs, 2);

            assert_eq!(entities.len(), 2);
            assert_eq!(ecs.core_world.get_position(entities[0]).unwrap().x, 1.0);
            assert_eq!(ecs.core_world.get_position(entities[1]).unwrap().x, 1.0);
            assert_eq!(ecs.render_world.get_sprite(entities[0]).unwrap().id, 42);
            assert_eq!(ecs.render_world.get_sprite(entities[1]).unwrap().id, 42);
        }

        #[test]
        fn test_multi_world_tags() {
            let mut ecs = GameEcs::default();
            let entity = ecs.spawn();

            ecs.add_player(entity);
            assert!(ecs.has_player(entity));

            let players: Vec<Entity> = ecs.query_player().collect();
            assert_eq!(players.len(), 1);

            ecs.remove_player(entity);
            assert!(!ecs.has_player(entity));
        }

        #[test]
        fn test_multi_world_tag_split_borrow() {
            let mut ecs = GameEcs::default();
            let e1 = ecs.spawn();
            let e2 = ecs.spawn();

            ecs.core_world.set_position(e1, Position { x: 1.0, y: 0.0 });
            ecs.core_world.set_position(e2, Position { x: 2.0, y: 0.0 });
            ecs.add_player(e1);

            let GameEcs {
                core_world, player, ..
            } = &mut ecs;
            let mut player_positions = Vec::new();
            core_world.for_each_mut(MW_POSITION, 0, |entity, table, idx| {
                if player.contains(entity) {
                    player_positions.push(table.position[idx].clone());
                }
            });

            assert_eq!(player_positions.len(), 1);
            assert_eq!(player_positions[0].x, 1.0);
        }

        #[test]
        fn test_multi_world_events() {
            let mut ecs = GameEcs::default();
            let e1 = ecs.spawn();
            let e2 = ecs.spawn();

            ecs.send_collision(CollisionEvent {
                entity_a: e1,
                entity_b: e2,
            });

            let events: Vec<_> = ecs.collect_collision();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].entity_a, e1);
        }

        #[test]
        fn test_multi_world_resources() {
            let mut ecs = GameEcs::default();
            ecs.resources.delta_time = 0.016;
            assert_eq!(ecs.resources.delta_time, 0.016);
        }

        #[test]
        fn test_multi_world_schedule() {
            let mut ecs = GameEcs::default();
            let entity = ecs.spawn();
            ecs.core_world
                .set_position(entity, Position { x: 0.0, y: 0.0 });
            ecs.core_world
                .set_velocity(entity, Velocity { x: 1.0, y: 2.0 });
            ecs.resources.delta_time = 0.1;

            let mut schedule: Schedule<GameEcs> = Schedule::new();
            schedule.push("physics", |ecs: &mut GameEcs| {
                let dt = ecs.resources.delta_time;
                ecs.core_world
                    .for_each_mut(MW_POSITION | MW_VELOCITY, 0, |_entity, table, idx| {
                        table.position[idx].x += table.velocity[idx].x * dt;
                        table.position[idx].y += table.velocity[idx].y * dt;
                    });
            });

            schedule.run(&mut ecs);

            let pos = ecs.core_world.get_position(entity).unwrap();
            assert!((pos.x - 0.1).abs() < f32::EPSILON);
            assert!((pos.y - 0.2).abs() < f32::EPSILON);
        }

        #[test]
        fn test_multi_world_mask_independence() {
            assert_eq!(MW_POSITION, 1 << 0);
            assert_eq!(MW_VELOCITY, 1 << 1);
            assert_eq!(MW_SPRITE, 1 << 0);
            assert_eq!(MW_COLOR, 1 << 1);
        }

        #[test]
        fn test_multi_world_command_buffer() {
            let mut ecs = GameEcs::default();
            let entity = ecs.spawn();

            ecs.core_world
                .set_position(entity, Position { x: 0.0, y: 0.0 });

            ecs.queue_set_position(entity, Position { x: 10.0, y: 20.0 });
            ecs.queue_add_player(entity);

            assert_eq!(ecs.core_world.get_position(entity).unwrap().x, 0.0);
            assert!(!ecs.has_player(entity));

            ecs.apply_commands();

            assert_eq!(ecs.core_world.get_position(entity).unwrap().x, 10.0);
            assert!(ecs.has_player(entity));
        }

        #[test]
        fn test_multi_world_step() {
            let mut ecs = GameEcs::default();

            assert_eq!(ecs.core_world.current_tick(), 0);
            assert_eq!(ecs.render_world.current_tick(), 0);

            ecs.step();

            assert_eq!(ecs.core_world.current_tick(), 1);
            assert_eq!(ecs.render_world.current_tick(), 1);
        }

        #[test]
        fn test_multi_world_for_each_within_world() {
            let mut ecs = GameEcs::default();
            let e1 = ecs.spawn();
            let e2 = ecs.spawn();

            ecs.core_world.set_position(e1, Position { x: 1.0, y: 0.0 });
            ecs.core_world.set_position(e2, Position { x: 2.0, y: 0.0 });
            ecs.core_world
                .set_velocity(e2, Velocity { x: 10.0, y: 0.0 });

            let mut count = 0;
            ecs.core_world
                .for_each(MW_POSITION, 0, |_entity, _table, _idx| {
                    count += 1;
                });
            assert_eq!(count, 2);

            count = 0;
            ecs.core_world
                .for_each(MW_POSITION | MW_VELOCITY, 0, |_entity, _table, _idx| {
                    count += 1;
                });
            assert_eq!(count, 1);
        }

        #[test]
        fn test_multi_world_query_builder() {
            let mut ecs = GameEcs::default();
            let e1 = ecs.spawn();
            let e2 = ecs.spawn();

            ecs.core_world.set_position(e1, Position { x: 1.0, y: 0.0 });
            ecs.core_world.set_position(e2, Position { x: 2.0, y: 0.0 });
            ecs.core_world
                .set_velocity(e2, Velocity { x: 10.0, y: 0.0 });

            let mut count = 0;
            ecs.core_world
                .query()
                .with(MW_POSITION)
                .without(MW_VELOCITY)
                .iter(|_entity, _table, _idx| {
                    count += 1;
                });
            assert_eq!(count, 1);
        }

        #[test]
        fn test_multi_world_generational_reuse() {
            let mut ecs = GameEcs::default();

            let e1 = ecs.spawn();
            ecs.core_world.set_position(e1, Position { x: 1.0, y: 0.0 });

            ecs.despawn(e1);
            assert!(ecs.core_world.get_position(e1).is_none());

            let e2 = ecs.spawn();
            assert_eq!(e2.id, e1.id);
            assert_eq!(e2.generation, e1.generation + 1);

            assert!(ecs.core_world.get_position(e1).is_none());
        }

        #[test]
        fn test_multi_world_ghost_entity_guard() {
            let mut ecs = GameEcs::default();
            let e1 = ecs.spawn();
            ecs.core_world.set_position(e1, Position { x: 1.0, y: 0.0 });
            ecs.despawn(e1);

            let e2 = ecs.spawn();
            assert_eq!(e2.id, e1.id);

            ecs.core_world.set_position(e2, Position { x: 5.0, y: 5.0 });
            assert_eq!(ecs.core_world.get_position(e2).unwrap().x, 5.0);

            assert!(ecs.core_world.get_position(e1).is_none());
        }

        #[test]
        fn test_multi_world_for_each_with_tags() {
            let mut ecs = GameEcs::default();
            let e1 = ecs.spawn();
            let e2 = ecs.spawn();
            let e3 = ecs.spawn();

            ecs.core_world.set_position(e1, Position { x: 1.0, y: 0.0 });
            ecs.core_world.set_position(e2, Position { x: 2.0, y: 0.0 });
            ecs.core_world.set_position(e3, Position { x: 3.0, y: 0.0 });

            ecs.add_player(e1);
            ecs.add_player(e3);

            let GameEcs {
                core_world, player, ..
            } = &ecs;

            let mut count = 0;
            core_world.for_each_with_tags(
                MW_POSITION,
                0,
                &[player],
                &[],
                |_entity, _table, _idx| {
                    count += 1;
                },
            );
            assert_eq!(count, 2);

            count = 0;
            core_world.for_each_with_tags(
                MW_POSITION,
                0,
                &[],
                &[player],
                |_entity, _table, _idx| {
                    count += 1;
                },
            );
            assert_eq!(count, 1);
        }

        #[test]
        fn test_multi_world_for_each_mut_with_tags() {
            let mut ecs = GameEcs::default();
            let e1 = ecs.spawn();
            let e2 = ecs.spawn();

            ecs.core_world.set_position(e1, Position { x: 1.0, y: 0.0 });
            ecs.core_world.set_position(e2, Position { x: 2.0, y: 0.0 });

            ecs.add_player(e1);

            let player_set = ecs.player.clone();
            ecs.core_world.for_each_mut_with_tags(
                MW_POSITION,
                0,
                &[&player_set],
                &[],
                |_entity, table, idx| {
                    table.position[idx].x += 100.0;
                },
            );

            assert_eq!(ecs.core_world.get_position(e1).unwrap().x, 101.0);
            assert_eq!(ecs.core_world.get_position(e2).unwrap().x, 2.0);
        }

        #[test]
        fn test_multi_world_command_buffer_spawn_despawn() {
            let mut ecs = GameEcs::default();
            ecs.queue_spawn(3);
            assert_eq!(ecs.command_count(), 1);
            ecs.apply_commands();

            let next = ecs.spawn();
            assert_eq!(next.id, 3);

            let entity = ecs.spawn();
            ecs.core_world
                .set_position(entity, Position { x: 1.0, y: 0.0 });
            ecs.queue_despawn_entity(entity);
            ecs.apply_commands();
            assert!(ecs.core_world.get_position(entity).is_none());
        }

        #[test]
        fn test_multi_world_command_buffer_component_add_remove() {
            let mut ecs = GameEcs::default();
            let entity = ecs.spawn();

            ecs.queue_add_position(entity);
            ecs.apply_commands();
            assert!(ecs.core_world.get_position(entity).is_some());

            ecs.queue_remove_position(entity);
            ecs.apply_commands();
            assert!(ecs.core_world.get_position(entity).is_none());
        }

        #[test]
        fn test_multi_world_per_world_spawn_entities() {
            let mut ecs = GameEcs::default();
            let entities = {
                let GameEcs {
                    core_world,
                    allocator,
                    ..
                } = &mut ecs;
                core_world.spawn_entities(allocator, MW_POSITION | MW_VELOCITY, 3)
            };
            assert_eq!(entities.len(), 3);
            for &entity in &entities {
                assert!(ecs.core_world.get_position(entity).is_some());
                assert!(ecs.core_world.get_velocity(entity).is_some());
            }

            let next = ecs.spawn();
            assert_eq!(next.id, 3);
        }

        #[test]
        fn test_multi_world_change_detection() {
            let mut ecs = GameEcs::default();
            let e1 = ecs.spawn();
            let e2 = ecs.spawn();
            ecs.core_world.set_position(e1, Position { x: 1.0, y: 0.0 });
            ecs.core_world.set_position(e2, Position { x: 2.0, y: 0.0 });

            ecs.step();

            ecs.core_world.get_position_mut(e1).unwrap().x = 10.0;

            let changed: Vec<Entity> = ecs.core_world.query_entities_changed(MW_POSITION).collect();
            assert_eq!(changed, vec![e1]);

            let mut visited = Vec::new();
            ecs.core_world
                .for_each_mut_changed(MW_POSITION, 0, |entity, _table, _idx| {
                    visited.push(entity);
                });
            assert_eq!(visited, vec![e1]);
        }

        #[test]
        fn test_multi_world_structural_log() {
            let mut ecs = GameEcs::default();
            let entity = ecs.spawn();
            ecs.core_world
                .set_position(entity, Position { x: 1.0, y: 0.0 });
            ecs.render_world.set_sprite(entity, Sprite { id: 1 });

            let core_changes = ecs.core_world.structural_changes_since(0);
            assert_eq!(core_changes.len(), 1);
            assert_eq!(core_changes[0].kind, StructuralChangeKind::Spawned);
            assert_eq!(core_changes[0].mask, MW_POSITION);

            let render_changes = ecs.render_world.structural_changes_since(0);
            assert_eq!(render_changes.len(), 1);
            assert_eq!(render_changes[0].mask, MW_SPRITE);

            ecs.despawn(entity);

            let core_changes = ecs.core_world.structural_changes_since(0);
            assert_eq!(core_changes.len(), 2);
            assert_eq!(core_changes[1].kind, StructuralChangeKind::Despawned);
        }

        #[test]
        fn test_multi_world_event_cursor() {
            let mut ecs = GameEcs::default();
            let entity = ecs.spawn();

            ecs.send_collision(CollisionEvent {
                entity_a: entity,
                entity_b: entity,
            });

            let mut cursor = 0;
            assert_eq!(ecs.read_collision_since(cursor).len(), 1);
            cursor = ecs.sequence_collision();
            assert!(ecs.read_collision_since(cursor).is_empty());

            ecs.send_collision(CollisionEvent {
                entity_a: entity,
                entity_b: entity,
            });
            assert_eq!(ecs.read_collision_since(cursor).len(), 1);
        }

        #[test]
        fn test_multi_world_event_lifecycle() {
            let mut ecs = GameEcs::default();
            let entity = ecs.spawn();

            ecs.send_collision(CollisionEvent {
                entity_a: entity,
                entity_b: entity,
            });
            assert_eq!(ecs.len_collision(), 1);

            ecs.step();
            assert_eq!(ecs.len_collision(), 1);

            ecs.step();
            assert_eq!(ecs.len_collision(), 0);
        }

        #[test]
        fn test_multi_world_despawn_entities_batch() {
            let mut ecs = GameEcs::default();
            let e1 = ecs.spawn();
            let e2 = ecs.spawn();
            let e3 = ecs.spawn();
            for &entity in &[e1, e2, e3] {
                ecs.core_world.set_position(entity, Position::default());
            }

            ecs.despawn_entities(&[e1, e3]);

            assert!(ecs.core_world.get_position(e1).is_none());
            assert!(ecs.core_world.get_position(e2).is_some());
            assert!(ecs.core_world.get_position(e3).is_none());
        }

        #[test]
        fn test_multi_world_entity_builder_single_world_components() {
            let mut ecs = GameEcs::default();
            let entities = EntityBuilder::new()
                .with_color(Color {
                    r: 1.0,
                    g: 0.0,
                    b: 0.0,
                })
                .spawn(&mut ecs, 1);

            assert!(ecs.render_world.get_color(entities[0]).is_some());
            assert!(ecs.core_world.get_position(entities[0]).is_none());
            assert!(ecs.render_world.get_sprite(entities[0]).is_none());
        }

        #[test]
        fn test_multi_world_double_despawn_is_rejected() {
            let mut ecs = GameEcs::default();
            let entity = ecs.spawn();
            ecs.core_world.set_position(entity, Position::default());

            assert!(ecs.despawn(entity));
            assert!(!ecs.despawn(entity));

            let e1 = ecs.spawn();
            let e2 = ecs.spawn();
            assert_ne!(
                (e1.id, e1.generation),
                (e2.id, e2.generation),
                "double despawn must never mint two identical live handles"
            );

            ecs.core_world.set_position(e1, Position { x: 1.0, y: 0.0 });
            ecs.core_world.set_position(e2, Position { x: 2.0, y: 0.0 });
            assert_eq!(ecs.core_world.get_position(e1).unwrap().x, 1.0);
            assert_eq!(ecs.core_world.get_position(e2).unwrap().x, 2.0);
        }

        #[test]
        fn test_multi_world_componentless_double_despawn_is_rejected() {
            let mut ecs = GameEcs::default();
            let entity = ecs.spawn();

            assert!(ecs.despawn(entity));
            assert!(!ecs.despawn(entity));

            let e1 = ecs.spawn();
            let e2 = ecs.spawn();
            assert_ne!((e1.id, e1.generation), (e2.id, e2.generation));
        }

        #[test]
        fn test_multi_world_stale_despawn_cannot_kill_reused_id() {
            let mut ecs = GameEcs::default();
            let old = ecs.spawn();
            ecs.core_world.set_position(old, Position::default());
            assert!(ecs.despawn(old));

            let reused = ecs.spawn();
            assert_eq!(reused.id, old.id);
            ecs.core_world
                .set_position(reused, Position { x: 7.0, y: 0.0 });

            assert!(!ecs.despawn(old), "stale handle must not despawn anything");
            assert_eq!(
                ecs.core_world.get_position(reused).unwrap().x,
                7.0,
                "live entity must survive a stale despawn attempt"
            );

            let fresh = ecs.spawn();
            assert_ne!(
                (fresh.id, fresh.generation),
                (reused.id, reused.generation),
                "stale despawn must not recycle a live handle"
            );
        }

        #[test]
        fn test_multi_world_stale_handle_cannot_resurrect() {
            let mut ecs = GameEcs::default();
            let old = ecs.spawn();
            ecs.core_world.set_position(old, Position::default());
            assert!(ecs.despawn(old));

            assert!(
                !ecs.core_world.add_components(old, MW_POSITION),
                "stale add must be refused in a world that stored the entity"
            );
            ecs.core_world
                .set_position(old, Position { x: 9.0, y: 0.0 });
            assert!(ecs.core_world.get_position(old).is_none());

            assert!(
                !ecs.render_world.add_components(old, MW_SPRITE),
                "stale add must be refused in a world that never stored the entity"
            );
            ecs.render_world.set_sprite(old, Sprite { id: 3 });
            assert!(ecs.render_world.get_sprite(old).is_none());

            assert!(
                ecs.core_world.structural_changes_since(0).len() <= 2,
                "refused stale writes must not append structural log entries"
            );
        }

        #[test]
        fn test_multi_world_reused_id_still_gets_components() {
            let mut ecs = GameEcs::default();
            let old = ecs.spawn();
            ecs.core_world.set_position(old, Position::default());
            assert!(ecs.despawn(old));

            let reused = ecs.spawn();
            assert_eq!(reused.id, old.id);

            ecs.core_world
                .set_position(reused, Position { x: 4.0, y: 0.0 });
            ecs.render_world.set_sprite(reused, Sprite { id: 5 });

            assert_eq!(ecs.core_world.get_position(reused).unwrap().x, 4.0);
            assert_eq!(ecs.render_world.get_sprite(reused).unwrap().id, 5);
            assert!(ecs.core_world.get_position(old).is_none());
        }

        #[test]
        #[cfg(not(target_family = "wasm"))]
        fn test_multi_world_par_for_each_mut_with_tags() {
            let mut ecs = GameEcs::default();
            let e1 = ecs.spawn();
            let e2 = ecs.spawn();

            ecs.core_world.set_position(e1, Position { x: 1.0, y: 0.0 });
            ecs.core_world.set_position(e2, Position { x: 2.0, y: 0.0 });
            ecs.add_player(e1);

            let player_set = ecs.player.clone();
            ecs.core_world.par_for_each_mut_with_tags(
                MW_POSITION,
                0,
                &[&player_set],
                &[],
                |_entity, table, index| {
                    table.position[index].x += 100.0;
                },
            );

            assert_eq!(ecs.core_world.get_position(e1).unwrap().x, 101.0);
            assert_eq!(ecs.core_world.get_position(e2).unwrap().x, 2.0);

            ecs.core_world.par_for_each_mut_with_tags(
                MW_POSITION,
                0,
                &[],
                &[&player_set],
                |_entity, table, index| {
                    table.position[index].y = 7.0;
                },
            );

            assert_eq!(ecs.core_world.get_position(e1).unwrap().y, 0.0);
            assert_eq!(ecs.core_world.get_position(e2).unwrap().y, 7.0);
        }

        #[derive(Default, Clone)]
        struct MultiModelEntity {
            core_mask: Option<u64>,
            render_mask: Option<u64>,
            position: Option<f32>,
            position_changed: bool,
            sprite: Option<u32>,
            player: bool,
        }

        enum MultiModelCommand {
            Spawn,
            Despawn(Entity),
            SetPosition(Entity, f32),
            AddPosition(Entity),
            RemovePosition(Entity),
            SetSprite(Entity, u32),
            AddPlayer(Entity),
            RemovePlayer(Entity),
        }

        fn multi_model_set_position(model_entity: &mut MultiModelEntity, value: f32) {
            let mask = model_entity.core_mask.get_or_insert(0);
            *mask |= MW_POSITION;
            model_entity.position = Some(value);
            model_entity.position_changed = true;
        }

        fn multi_model_add_position(model_entity: &mut MultiModelEntity) {
            let mask = model_entity.core_mask.get_or_insert(0);
            if *mask & MW_POSITION == 0 {
                *mask |= MW_POSITION;
                model_entity.position = Some(0.0);
                model_entity.position_changed = true;
            }
        }

        fn multi_model_add_velocity(model_entity: &mut MultiModelEntity) {
            let mask = model_entity.core_mask.get_or_insert(0);
            let migrated = *mask & MW_VELOCITY == 0;
            *mask |= MW_VELOCITY;
            if migrated && *mask & MW_POSITION != 0 {
                model_entity.position_changed = true;
            }
        }

        fn multi_model_remove_position(model_entity: &mut MultiModelEntity) {
            if let Some(mask) = model_entity.core_mask.as_mut() {
                *mask &= !MW_POSITION;
                model_entity.position = None;
            }
        }

        fn multi_model_set_sprite(model_entity: &mut MultiModelEntity, id: u32) {
            let mask = model_entity.render_mask.get_or_insert(0);
            *mask |= MW_SPRITE;
            model_entity.sprite = Some(id);
        }

        fn multi_apply_and_replay(
            ecs: &mut GameEcs,
            model: &mut std::collections::HashMap<Entity, MultiModelEntity>,
            queued: &mut Vec<MultiModelCommand>,
            handles: &mut Vec<Entity>,
        ) {
            assert_eq!(
                ecs.command_count(),
                queued.len(),
                "ecs and model must queue in lockstep"
            );

            let cursor = ecs.structural_sequence();
            ecs.apply_commands();

            let mut queued_spawn_count = 0usize;
            for command in queued.drain(..) {
                match command {
                    MultiModelCommand::Spawn => queued_spawn_count += 1,
                    MultiModelCommand::Despawn(entity) => {
                        model.remove(&entity);
                    }
                    MultiModelCommand::SetPosition(entity, value) => {
                        if let Some(model_entity) = model.get_mut(&entity) {
                            multi_model_set_position(model_entity, value);
                        }
                    }
                    MultiModelCommand::AddPosition(entity) => {
                        if let Some(model_entity) = model.get_mut(&entity) {
                            multi_model_add_position(model_entity);
                        }
                    }
                    MultiModelCommand::RemovePosition(entity) => {
                        if let Some(model_entity) = model.get_mut(&entity) {
                            multi_model_remove_position(model_entity);
                        }
                    }
                    MultiModelCommand::SetSprite(entity, id) => {
                        if let Some(model_entity) = model.get_mut(&entity) {
                            multi_model_set_sprite(model_entity, id);
                        }
                    }
                    MultiModelCommand::AddPlayer(entity) => {
                        if let Some(model_entity) = model.get_mut(&entity) {
                            model_entity.player = true;
                        }
                    }
                    MultiModelCommand::RemovePlayer(entity) => {
                        if let Some(model_entity) = model.get_mut(&entity) {
                            model_entity.player = false;
                        }
                    }
                }
            }

            let spawned: Vec<Entity> = ecs
                .structural_changes_since(cursor)
                .iter()
                .filter(|change| change.kind == StructuralChangeKind::Spawned)
                .map(|change| change.entity)
                .collect();
            assert_eq!(
                spawned.len(),
                queued_spawn_count,
                "the ecs lifecycle log must record exactly the queued spawns"
            );
            for entity in spawned {
                assert!(ecs.is_alive(entity));
                model.insert(entity, MultiModelEntity::default());
                handles.push(entity);
            }
        }

        #[test]
        fn test_property_multi_world_matches_model() {
            for seed in [7u64, 1337, 24601] {
                let mut rng = Lcg(seed);
                let mut ecs = GameEcs::default();
                let mut model: std::collections::HashMap<Entity, MultiModelEntity> =
                    std::collections::HashMap::new();
                let mut handles: Vec<Entity> = Vec::new();
                let mut queued: Vec<MultiModelCommand> = Vec::new();
                let mut pending_collisions: Vec<(u32, u32)> = Vec::new();
                let mut total_collisions: u64 = 0;

                ecs.step();

                let pick = |rng: &mut Lcg, handles: &[Entity]| {
                    if handles.is_empty() {
                        None
                    } else {
                        Some(handles[rng.next() as usize % handles.len()])
                    }
                };

                for _ in 0..3000 {
                    match rng.next() % 16 {
                        0 | 1 => {
                            let entity = ecs.spawn();
                            model.insert(entity, MultiModelEntity::default());
                            handles.push(entity);
                        }
                        2 => {
                            if let Some(entity) = pick(&mut rng, &handles) {
                                let accepted = ecs.despawn(entity);
                                let was_live = model.remove(&entity).is_some();
                                assert_eq!(
                                    accepted, was_live,
                                    "despawn must succeed exactly for model-live handles"
                                );
                            }
                        }
                        3 => {
                            if let Some(entity) = pick(&mut rng, &handles) {
                                let value = (rng.next() % 1000) as f32;
                                ecs.core_world
                                    .set_position(entity, Position { x: value, y: 0.0 });
                                match model.get_mut(&entity) {
                                    Some(model_entity) => {
                                        multi_model_set_position(model_entity, value);
                                        assert_eq!(
                                            ecs.core_world.get_position(entity).unwrap().x,
                                            value
                                        );
                                    }
                                    None => assert!(
                                        ecs.core_world.get_position(entity).is_none(),
                                        "stale cross-world set must not resurrect"
                                    ),
                                }
                            }
                        }
                        4 => {
                            if let Some(entity) = pick(&mut rng, &handles) {
                                let id = rng.next() as u32 % 1000;
                                ecs.render_world.set_sprite(entity, Sprite { id });
                                match model.get_mut(&entity) {
                                    Some(model_entity) => {
                                        multi_model_set_sprite(model_entity, id);
                                        assert_eq!(
                                            ecs.render_world.get_sprite(entity).unwrap().id,
                                            id
                                        );
                                    }
                                    None => {
                                        assert!(ecs.render_world.get_sprite(entity).is_none())
                                    }
                                }
                            }
                        }
                        5 => {
                            if let Some(entity) = pick(&mut rng, &handles) {
                                let accepted = ecs.core_world.add_components(entity, MW_VELOCITY);
                                match model.get_mut(&entity) {
                                    Some(model_entity) => {
                                        assert!(accepted);
                                        multi_model_add_velocity(model_entity);
                                    }
                                    None => assert!(!accepted, "stale add must be refused"),
                                }
                            }
                        }
                        6 => {
                            if let Some(entity) = pick(&mut rng, &handles) {
                                let accepted =
                                    ecs.core_world.remove_components(entity, MW_POSITION);
                                match model.get_mut(&entity) {
                                    Some(model_entity) => {
                                        if model_entity.core_mask.is_some() {
                                            assert!(accepted);
                                            multi_model_remove_position(model_entity);
                                        } else {
                                            assert!(
                                                !accepted,
                                                "remove without a row must report false"
                                            );
                                        }
                                    }
                                    None => assert!(!accepted),
                                }
                            }
                        }
                        7 => {
                            if let Some(entity) = pick(&mut rng, &handles) {
                                ecs.add_player(entity);
                                if let Some(model_entity) = model.get_mut(&entity) {
                                    model_entity.player = true;
                                }
                                assert_eq!(
                                    ecs.has_player(entity),
                                    model.get(&entity).map(|m| m.player).unwrap_or(false)
                                );
                            }
                        }
                        8 => {
                            if let Some(entity) = pick(&mut rng, &handles) {
                                ecs.queue_despawn_entity(entity);
                                queued.push(MultiModelCommand::Despawn(entity));
                            }
                        }
                        9 => {
                            if let Some(entity) = pick(&mut rng, &handles) {
                                let value = (rng.next() % 1000) as f32;
                                ecs.queue_set_position(entity, Position { x: value, y: 0.0 });
                                queued.push(MultiModelCommand::SetPosition(entity, value));
                            }
                        }
                        10 => {
                            if let Some(entity) = pick(&mut rng, &handles) {
                                ecs.queue_add_position(entity);
                                queued.push(MultiModelCommand::AddPosition(entity));
                            }
                        }
                        11 => {
                            if let Some(entity) = pick(&mut rng, &handles) {
                                ecs.queue_remove_position(entity);
                                queued.push(MultiModelCommand::RemovePosition(entity));
                            }
                        }
                        12 => {
                            if let Some(entity) = pick(&mut rng, &handles) {
                                let id = rng.next() as u32 % 1000;
                                ecs.queue_set_sprite(entity, Sprite { id });
                                queued.push(MultiModelCommand::SetSprite(entity, id));
                            }
                        }
                        13 => {
                            if let Some(entity) = pick(&mut rng, &handles) {
                                if rng.next().is_multiple_of(2) {
                                    ecs.queue_add_player(entity);
                                    queued.push(MultiModelCommand::AddPlayer(entity));
                                } else {
                                    ecs.queue_remove_player(entity);
                                    queued.push(MultiModelCommand::RemovePlayer(entity));
                                }
                            } else {
                                ecs.queue_spawn(1);
                                queued.push(MultiModelCommand::Spawn);
                            }
                        }
                        14 => {
                            let entity_a = pick(&mut rng, &handles).unwrap_or(Entity {
                                id: 0,
                                generation: 0,
                            });
                            let entity_b = pick(&mut rng, &handles).unwrap_or(entity_a);
                            ecs.send_collision(CollisionEvent { entity_a, entity_b });
                            pending_collisions.push((entity_a.id, entity_b.id));
                            total_collisions += 1;
                        }
                        _ => {
                            multi_apply_and_replay(&mut ecs, &mut model, &mut queued, &mut handles);

                            let changed: std::collections::HashSet<Entity> =
                                ecs.core_world.query_entities_changed(MW_POSITION).collect();
                            let expected: std::collections::HashSet<Entity> = model
                                .iter()
                                .filter(|(_, model_entity)| {
                                    model_entity
                                        .core_mask
                                        .is_some_and(|mask| mask & MW_POSITION != 0)
                                        && model_entity.position_changed
                                })
                                .map(|(&entity, _)| entity)
                                .collect();
                            assert_eq!(
                                changed, expected,
                                "core changed-query set diverged from model with seed {seed}"
                            );

                            ecs.step();
                            for model_entity in model.values_mut() {
                                model_entity.position_changed = false;
                            }

                            let buffered: Vec<(u32, u32)> = ecs
                                .read_collision()
                                .map(|event| (event.entity_a.id, event.entity_b.id))
                                .collect();
                            assert_eq!(
                                buffered, pending_collisions,
                                "post-step event buffer must hold exactly the just-ended frame"
                            );
                            assert_eq!(ecs.sequence_collision(), total_collisions);
                            pending_collisions.clear();
                        }
                    }
                }

                multi_apply_and_replay(&mut ecs, &mut model, &mut queued, &mut handles);

                for (&entity, model_entity) in &model {
                    assert!(ecs.is_alive(entity));
                    assert_eq!(
                        ecs.core_world.component_mask(entity),
                        model_entity.core_mask,
                        "core mask diverged with seed {seed}"
                    );
                    assert_eq!(
                        ecs.render_world.component_mask(entity),
                        model_entity.render_mask,
                        "render mask diverged with seed {seed}"
                    );
                    assert_eq!(
                        ecs.core_world
                            .get_position(entity)
                            .map(|position| position.x),
                        model_entity.position,
                        "position value diverged with seed {seed}"
                    );
                    assert_eq!(
                        ecs.render_world.get_sprite(entity).map(|sprite| sprite.id),
                        model_entity.sprite,
                        "sprite value diverged with seed {seed}"
                    );
                    assert_eq!(ecs.has_player(entity), model_entity.player);
                }

                for &handle in &handles {
                    if !model.contains_key(&handle) {
                        assert!(!ecs.is_alive(handle));
                        assert_eq!(ecs.core_world.component_mask(handle), None);
                        assert_eq!(ecs.render_world.component_mask(handle), None);
                        assert!(!ecs.has_player(handle));
                        assert!(!ecs.despawn(handle), "double despawn must stay refused");
                    }
                }

                let expected_core_rows = model.values().filter(|m| m.core_mask.is_some()).count();
                assert_eq!(ecs.core_world.entity_count(), expected_core_rows);

                let expected_render_rows =
                    model.values().filter(|m| m.render_mask.is_some()).count();
                assert_eq!(ecs.render_world.entity_count(), expected_render_rows);

                let expected_players = model.values().filter(|m| m.player).count();
                assert_eq!(ecs.query_player().count(), expected_players);
            }
        }

        #[test]
        fn test_multi_world_repeated_reuse_cycle_stays_consistent() {
            let mut ecs = GameEcs::default();
            let mut previous = ecs.spawn();
            ecs.render_world.set_sprite(previous, Sprite { id: 0 });

            for cycle in 1..5u32 {
                assert!(ecs.despawn(previous));
                let entity = ecs.spawn();
                assert_eq!(entity.id, previous.id);
                assert_eq!(entity.generation, cycle);

                ecs.render_world.set_sprite(entity, Sprite { id: cycle });
                assert_eq!(ecs.render_world.get_sprite(entity).unwrap().id, cycle);
                assert!(ecs.render_world.get_sprite(previous).is_none());

                previous = entity;
            }
        }
    }
}
