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
//! - **Sparse Set Tags**: Lightweight markers that don't fragment archetypes
//! - **Command Buffers**: Queue structural changes during iteration
//! - **Change Detection**: Track component modifications for incremental updates
//! - **Events**: Type-safe double-buffered event system
//! - **Multi-World**: Split components across multiple worlds for >64 component types
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
//! struct Position { x: f32, y: f32 }
//!
//! #[derive(Default, Clone, Debug)]
//! struct Velocity { x: f32, y: f32 }
//!
//! #[derive(Default, Clone, Debug)]
//! struct Health { value: f32 }
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
//! struct CollisionEvent {
//!     entity_a: Entity,
//!     entity_b: Entity,
//! }
//! ```
//!
//! ## Entity and Component Access
//!
//! ```rust
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
//! # #[derive(Default, Clone)] struct Position { x: f32, y: f32 }
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
//! # #[derive(Default, Clone)] struct Position { x: f32, y: f32 }
//! # #[derive(Debug, Clone)] struct CollisionEvent { entity_a: Entity, entity_b: Entity }
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
//! fn physics_system(world: &mut World) {
//!     let dt = world.resources.delta_time;
//!
//!     // Method 1: High-performance query builder (recommended)
//!     world.query()
//!         .with(POSITION | VELOCITY)
//!         .iter(|entity, table, idx| {
//!             table.position[idx].x += table.velocity[idx].x * dt;
//!             table.position[idx].y += table.velocity[idx].y * dt;
//!         });
//!
//!     // Method 2: Per-entity lookups (simpler but slower)
//!     for entity in world.query_entities(POSITION | VELOCITY) {
//!         if let Some(position) = world.get_position_mut(entity) {
//!             if let Some(velocity) = world.get_velocity(entity) {
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
//! # #[derive(Default, Clone)] struct Position { x: f32, y: f32 }
//! # #[derive(Default, Clone)] struct Health { value: f32 }
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
//! # #[derive(Default, Clone)] struct Position { x: f32, y: f32 }
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
//! ## System Scheduling
//!
//! Organize systems into a schedule:
//!
//! ```rust
//! # use freecs::{ecs, Schedule, Entity};
//! # #[derive(Default, Clone)] struct Position { x: f32, y: f32 }
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
//! # #[derive(Default, Clone)] struct Position { x: f32, y: f32 }
//! # #[derive(Default, Clone)] struct Velocity { x: f32, y: f32 }
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
//! # #[derive(Default, Clone)] struct Position { x: f32, y: f32 }
//! # ecs! { World { position: Position => POSITION, } Resources { delta_time: f32 } }
//! # let mut world = World::default();
//! // Iterate over single component
//! world.iter_position_mut(|position| {
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
//! # #[derive(Default, Clone)] struct Position { x: f32, y: f32 }
//! # #[derive(Default, Clone)] struct Velocity { x: f32, y: f32 }
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
//! # #[derive(Default, Clone)] struct Position { x: f32, y: f32 }
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
//! # #[derive(Default, Clone)] struct Position { x: f32, y: f32 }
//! # #[derive(Debug, Clone)] struct CollisionEvent { entity_a: Entity, entity_b: Entity }
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
//! // Drain events (takes ownership)
//! for event in world.drain_collision() {
//!     // Process event
//! }
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

#[cfg(not(target_family = "wasm"))]
pub use rayon;

#[derive(
    Default, Clone, Copy, Debug, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize,
)]
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

#[derive(Default)]
pub struct EntityAllocator {
    pub next_id: u32,
    pub free_ids: Vec<(u32, u32)>,
}

impl EntityAllocator {
    pub fn allocate(&mut self) -> Entity {
        if let Some((id, next_gen)) = self.free_ids.pop() {
            Entity {
                id,
                generation: next_gen,
            }
        } else {
            let id = self.next_id;
            self.next_id += 1;
            Entity { id, generation: 0 }
        }
    }

    pub fn deallocate(&mut self, entity: Entity) {
        self.free_ids
            .push((entity.id, entity.generation.wrapping_add(1)));
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

/// Double-buffered event queue for inter-system communication.
///
/// Events persist for 2 frames to prevent systems from missing events
/// during execution. Call [`update()`](EventQueue::update) between frames to swap buffers.
///
/// # Examples
///
/// ```
/// use freecs::EventQueue;
///
/// #[derive(Debug, Clone)]
/// struct DamageEvent { amount: i32 }
///
/// let mut queue = EventQueue::new();
///
/// queue.send(DamageEvent { amount: 10 });
/// assert_eq!(queue.len(), 1);
///
/// for event in queue.read() {
///     println!("Damage: {}", event.amount);
/// }
///
/// queue.update();
/// assert_eq!(queue.len(), 1, "Event persists after first update");
///
/// queue.update();
/// assert_eq!(queue.len(), 0, "Event cleared after second update");
/// ```
#[derive(Clone)]
pub struct EventQueue<T> {
    current: Vec<T>,
    previous: Vec<T>,
}

impl<T> Default for EventQueue<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> EventQueue<T> {
    /// Creates a new empty event queue.
    pub fn new() -> Self {
        Self {
            current: Vec::new(),
            previous: Vec::new(),
        }
    }

    /// Sends an event to the queue.
    ///
    /// The event will be available for reading until two [`update()`](EventQueue::update) calls have been made.
    pub fn send(&mut self, event: T) {
        #[cfg(debug_assertions)]
        {
            const WARN_THRESHOLD: usize = 10000;
            let total = self.len();
            if total > WARN_THRESHOLD {
                eprintln!(
                    "WARNING: EventQueue has {} events. Did you forget to call update()?",
                    total
                );
            }
        }
        self.current.push(event);
    }

    /// Returns an iterator over all events in both buffers (previous frame, then current frame).
    ///
    /// Events are yielded in the order they were sent, with previous frame events first.
    pub fn read(&self) -> impl Iterator<Item = &T> {
        self.previous.iter().chain(self.current.iter())
    }

    /// Returns a reference to the first event without consuming it, if any exists.
    pub fn peek(&self) -> Option<&T> {
        self.previous.first().or_else(|| self.current.first())
    }

    /// Drains all events from both buffers, returning an iterator that takes ownership.
    ///
    /// After calling this, the queue will be empty.
    pub fn drain(&mut self) -> impl Iterator<Item = T> + '_ {
        self.previous.drain(..).chain(self.current.drain(..))
    }

    /// Swaps the event buffers and clears old events.
    ///
    /// After calling this:
    /// - Events from the previous frame are cleared
    /// - Events from the current frame become the previous frame
    /// - The current frame is empty
    ///
    /// Call this once per frame to maintain the 2-frame event lifetime.
    pub fn update(&mut self) {
        self.previous.clear();
        std::mem::swap(&mut self.current, &mut self.previous);
    }

    /// Immediately clears all events from both buffers.
    ///
    /// Unlike [`update()`](EventQueue::update), this discards events immediately without the 2-frame persistence.
    pub fn clear(&mut self) {
        self.current.clear();
        self.previous.clear();
    }

    /// Returns the total number of events in both buffers.
    pub fn len(&self) -> usize {
        self.current.len() + self.previous.len()
    }

    /// Returns `true` if both buffers are empty.
    pub fn is_empty(&self) -> bool {
        self.current.is_empty() && self.previous.is_empty()
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
        #[allow(unused)]
        #[derive(Default, Debug, Clone)]
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

        #[repr(u64)]
        #[allow(clippy::upper_case_acronyms)]
        #[allow(non_camel_case_types)]
        pub enum Component {
            $($(#[$comp_attr])* $mask,)*
            $($tag_mask,)*
        }

        $($(#[$comp_attr])* pub const $mask: u64 = 1 << (Component::$mask as u64);)*
        $(pub const $tag_mask: u64 = 1 << (Component::$tag_mask as u64);)*

        const ALL_TAGS_MASK: u64 = 0 $(| $tag_mask)*;

        pub const COMPONENT_COUNT: usize = {
            let mut count = 0;
            $($(#[$comp_attr])* { count += 1; let _ = Component::$mask; })*
            $(count += 1; let _ = Component::$tag_mask;)*
            count
        };

        $crate::paste::paste! {
            pub enum Command {
                SpawnEntities { mask: u64, count: usize },
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

        #[allow(unused)]
        pub struct $world {
            entity_locations: $crate::EntityLocations,
            tables: Vec<ComponentArrays>,
            allocator: $crate::EntityAllocator,
            pub resources: $resources,
            table_edges: Vec<TableEdges>,
            table_lookup: std::collections::HashMap<u64, usize>,
            query_cache: std::collections::HashMap<u64, Vec<usize>>,
            current_tick: u32,
            last_tick: u32,
            $($tag_name: std::collections::HashSet<$crate::Entity>,)*
            command_buffer: Vec<Command>,
            $($event_name: $crate::EventQueue<$event_type>,)*
        }

        impl Default for $world {
            fn default() -> Self {
                Self {
                    entity_locations: $crate::EntityLocations::default(),
                    tables: Vec::default(),
                    allocator: $crate::EntityAllocator::default(),
                    resources: $resources::default(),
                    table_edges: Vec::default(),
                    table_lookup: std::collections::HashMap::default(),
                    query_cache: std::collections::HashMap::default(),
                    current_tick: 0,
                    last_tick: 0,
                    $(
                        $tag_name: std::collections::HashSet::default(),
                    )*
                    command_buffer: Vec::default(),
                    $(
                        $event_name: $crate::EventQueue::new(),
                    )*
                }
            }
        }

        #[allow(unused)]
        impl $world {
            fn get_cached_tables(&mut self, mask: u64) -> &[usize] {
                if !self.query_cache.contains_key(&mask) {
                    let matching_tables: Vec<usize> = self.tables
                        .iter()
                        .enumerate()
                        .filter(|(_, table)| table.mask & mask == mask)
                        .map(|(idx, _)| idx)
                        .collect();
                    self.query_cache.insert(mask, matching_tables);
                }
                &self.query_cache[&mask]
            }

            fn invalidate_query_cache_for_table(&mut self, new_table_mask: u64, new_table_index: usize) {
                self.query_cache.retain(|query_mask, cached_tables| {
                    if new_table_mask & query_mask == *query_mask {
                        cached_tables.push(new_table_index);
                        true
                    } else {
                        true
                    }
                });
            }

            $(
                $(#[$comp_attr])*
                $crate::paste::paste! {
                    #[inline]
                    pub fn [<get_ $name>](&self, entity: $crate::Entity) -> Option<&$type> {
                        let (table_index, array_index) = get_location(&self.entity_locations, entity)?;
                        let table = &self.tables[table_index];

                        if table.mask & $mask == 0 {
                            return None;
                        }

                        Some(&table.$name[array_index])
                    }

                    $crate::paste::paste! {
                        #[inline]
                        pub fn [<get_ $name _mut>](&mut self, entity: $crate::Entity) -> Option<&mut $type> {
                            let (table_index, array_index) = get_location(&self.entity_locations, entity)?;
                            let current_tick = self.current_tick;
                            let table = &mut self.tables[table_index];

                            if table.mask & $mask == 0 {
                                return None;
                            }

                            table.[<$name _changed>][array_index] = current_tick;
                            Some(&mut table.$name[array_index])
                        }
                    }

                    $crate::paste::paste! {
                        #[inline]
                        pub fn [<modify_ $name>]<R>(&mut self, entity: $crate::Entity, f: impl FnOnce(&mut $type) -> R) -> Option<R> {
                            let (table_index, array_index) = get_location(&self.entity_locations, entity)?;
                            let current_tick = self.current_tick;
                            let table = &mut self.tables[table_index];

                            if table.mask & $mask == 0 {
                                return None;
                            }

                            table.[<$name _changed>][array_index] = current_tick;
                            Some(f(&mut table.$name[array_index]))
                        }
                    }

                    #[inline]
                    pub fn [<entity_has_ $name>](&self, entity: $crate::Entity) -> bool {
                        self.entity_has_components(entity, $mask)
                    }

                    #[inline]
                    pub fn [<set_ $name>](&mut self, entity: $crate::Entity, value: $type) {
                        if let Some((table_index, array_index)) = get_location(&self.entity_locations, entity) {
                            if self.tables[table_index].mask & $mask != 0 {
                                self.tables[table_index].$name[array_index] = value;
                                return;
                            }
                            self.add_components_at(entity, $mask, table_index, array_index);
                            if let Some((new_table_index, new_array_index)) = get_location(&self.entity_locations, entity) {
                                self.tables[new_table_index].$name[new_array_index] = value;
                            }
                        }
                    }

                    #[inline]
                    pub fn [<add_ $name>](&mut self, entity: $crate::Entity) {
                        self.add_components(entity, $mask);
                    }

                    #[inline]
                    pub fn [<remove_ $name>](&mut self, entity: $crate::Entity) -> bool {
                        self.remove_components(entity, $mask)
                    }

                    #[inline]
                    pub fn [<query_ $name>](&self) -> [<$mask:camel QueryIter>]<'_> {
                        [<$mask:camel QueryIter>] {
                            tables: &self.tables,
                            table_index: 0,
                            array_index: 0,
                        }
                    }

                    pub fn [<for_each_ $name _mut>]<F>(&mut self, mut f: F)
                    where
                        F: FnMut(&mut $type),
                    {
                        let table_indices: Vec<usize> = self.get_cached_tables($mask).to_vec();

                        for table_index in table_indices {
                            for component in &mut self.tables[table_index].$name {
                                f(component);
                            }
                        }
                    }

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

                    pub fn [<iter_ $name _slices>](&self) -> impl Iterator<Item = &[$type]> {
                        self.tables
                            .iter()
                            .filter(|table| table.mask & $mask != 0)
                            .map(|table| table.$name.as_slice())
                    }

                    pub fn [<iter_ $name _slices_mut>](&mut self) -> impl Iterator<Item = &mut [$type]> {
                        self.tables
                            .iter_mut()
                            .filter(|table| table.mask & $mask != 0)
                            .map(|table| table.$name.as_mut_slice())
                    }
                }
            )*

            pub fn spawn_entities(&mut self, mask: u64, count: usize) -> Vec<$crate::Entity> {
                let mut entities = Vec::with_capacity(count);
                let table_index = get_or_create_table(self, mask);
                let start_index = self.tables[table_index].entity_indices.len();

                self.tables[table_index].entity_indices.reserve(count);
                $(
                    $(#[$comp_attr])*
                    $crate::paste::paste! {
                        if mask & $mask != 0 {
                            self.tables[table_index].$name.reserve(count);
                            self.tables[table_index].[<$name _changed>].reserve(count);
                        }
                    }
                )*

                for i in 0..count {
                    let entity = create_entity(self);
                    entities.push(entity);

                    self.tables[table_index].entity_indices.push(entity);
                    $(
                        $(#[$comp_attr])*
                        $crate::paste::paste! {
                            if mask & $mask != 0 {
                                self.tables[table_index].$name.push(<$type>::default());
                                self.tables[table_index].[<$name _changed>].push(self.current_tick);
                            }
                        }
                    )*

                    insert_location(
                        &mut self.entity_locations,
                        entity,
                        (table_index, start_index + i),
                    );
                }

                entities
            }

            pub fn spawn_batch<F>(&mut self, mask: u64, count: usize, mut init: F) -> Vec<$crate::Entity>
            where
                F: FnMut(&mut ComponentArrays, usize),
            {
                let table_index = get_or_create_table(self, mask);
                let start_index = self.tables[table_index].entity_indices.len();

                self.tables[table_index].entity_indices.reserve(count);
                $(
                    $(#[$comp_attr])*
                    $crate::paste::paste! {
                        if mask & $mask != 0 {
                            self.tables[table_index].$name.reserve(count);
                            self.tables[table_index].[<$name _changed>].reserve(count);
                        }
                    }
                )*

                let mut entities = Vec::with_capacity(count);

                for i in 0..count {
                    let entity = create_entity(self);
                    entities.push(entity);

                    self.tables[table_index].entity_indices.push(entity);
                    $(
                        $(#[$comp_attr])*
                        $crate::paste::paste! {
                            if mask & $mask != 0 {
                                self.tables[table_index].$name.push(<$type>::default());
                                self.tables[table_index].[<$name _changed>].push(self.current_tick);
                            }
                        }
                    )*

                    insert_location(
                        &mut self.entity_locations,
                        entity,
                        (table_index, start_index + i),
                    );

                    init(&mut self.tables[table_index], start_index + i);
                }

                entities
            }

            pub fn query_entities(&self, mask: u64) -> EntityQueryIter<'_> {
                EntityQueryIter {
                    tables: &self.tables,
                    mask,
                    table_index: 0,
                    array_index: 0,
                }
            }

            pub fn query_entities_changed(&self, mask: u64) -> ChangedEntityQueryIter<'_> {
                ChangedEntityQueryIter {
                    tables: &self.tables,
                    mask,
                    since_tick: self.last_tick,
                    table_index: 0,
                    array_index: 0,
                }
            }

            pub fn query_first_entity(&self, mask: u64) -> Option<$crate::Entity> {
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

            pub fn despawn_entities(&mut self, entities: &[$crate::Entity]) -> Vec<$crate::Entity> {
                let mut despawned = Vec::with_capacity(entities.len());
                let mut tables_to_update = Vec::new();

                for &entity in entities {
                    if let Some(loc) = self.entity_locations.get_mut(entity.id) {
                        if loc.allocated && loc.generation == entity.generation {
                            let table_idx = loc.table_index as usize;
                            let array_idx = loc.array_index as usize;

                            let next_gen = loc.generation.wrapping_add(1);
                            self.entity_locations.mark_deallocated(entity.id);
                            if let Some(loc) = self.entity_locations.get_mut(entity.id) {
                                loc.generation = next_gen;
                            }
                            self.allocator.free_ids.push((entity.id, next_gen));

                            tables_to_update.push((table_idx, array_idx));
                            despawned.push(entity);
                        }
                    }
                }

                tables_to_update.sort_by(|a, b| b.cmp(a));

                for (table_idx, array_idx) in tables_to_update {
                    if table_idx >= self.tables.len() {
                        continue;
                    }

                    let table = &mut self.tables[table_idx];
                    if table.entity_indices.is_empty() {
                        continue;
                    }
                    let last_idx = table.entity_indices.len() - 1;

                    if array_idx < last_idx {
                        let moved_entity = table.entity_indices[last_idx];
                        if let Some(loc) = self.entity_locations.get_mut(moved_entity.id) {
                            if loc.allocated {
                                loc.array_index = array_idx as u32;
                            }
                        }
                    }

                    $(
                        $(#[$comp_attr])*
                        $crate::paste::paste! {
                            if table.mask & $mask != 0 {
                                table.$name.swap_remove(array_idx);
                                table.[<$name _changed>].swap_remove(array_idx);
                            }
                        }
                    )*
                    table.entity_indices.swap_remove(array_idx);
                }

                $(
                    for &entity in &despawned {
                        self.$tag_name.remove(&entity);
                    }
                )*

                despawned
            }

            fn add_components_at(&mut self, entity: $crate::Entity, mask: u64, table_index: usize, array_index: usize) {
                let current_mask = self.tables[table_index].mask;
                if current_mask & mask == mask {
                    return;
                }

                let target_table = if mask.count_ones() == 1 {
                    get_component_index(mask).and_then(|idx| self.table_edges[table_index].add_edges[idx])
                } else {
                    self.table_edges[table_index].multi_add_cache.get(&mask).copied()
                };

                let new_table_index = target_table.unwrap_or_else(|| {
                    let new_idx = get_or_create_table(self, current_mask | mask);
                    self.table_edges[table_index].multi_add_cache.insert(mask, new_idx);
                    new_idx
                });

                move_entity(self, entity, table_index, array_index, new_table_index);
            }

            pub fn add_components(&mut self, entity: $crate::Entity, mask: u64) -> bool {
                if let Some((table_index, array_index)) = get_location(&self.entity_locations, entity) {
                    self.add_components_at(entity, mask, table_index, array_index);
                    true
                } else {
                    false
                }
            }

            pub fn remove_components(&mut self, entity: $crate::Entity, mask: u64) -> bool {
                if let Some((table_index, array_index)) = get_location(&self.entity_locations, entity) {
                    let current_mask = self.tables[table_index].mask;
                    if current_mask & mask == 0 {
                        return true;
                    }

                    let target_table = if mask.count_ones() == 1 {
                        get_component_index(mask)
                            .and_then(|idx| self.table_edges[table_index].remove_edges[idx])
                    } else {
                        self.table_edges[table_index].multi_remove_cache.get(&mask).copied()
                    };

                    let new_table_index = target_table.unwrap_or_else(|| {
                        let new_idx = get_or_create_table(self, current_mask & !mask);
                        self.table_edges[table_index].multi_remove_cache.insert(mask, new_idx);
                        new_idx
                    });

                    move_entity(self, entity, table_index, array_index, new_table_index);
                    true
                } else {
                    false
                }
            }

            pub fn component_mask(&self, entity: $crate::Entity) -> Option<u64> {
                get_location(&self.entity_locations, entity)
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

            $(
                $crate::paste::paste! {
                    pub fn [<add_ $tag_name>](&mut self, entity: $crate::Entity) {
                        if get_location(&self.entity_locations, entity).is_some() {
                            self.$tag_name.insert(entity);
                        }
                    }

                    pub fn [<remove_ $tag_name>](&mut self, entity: $crate::Entity) -> bool {
                        self.$tag_name.remove(&entity)
                    }

                    pub fn [<has_ $tag_name>](&self, entity: $crate::Entity) -> bool {
                        self.$tag_name.contains(&entity)
                    }

                    pub fn [<query_ $tag_name>](&self) -> impl Iterator<Item = $crate::Entity> + '_ {
                        self.$tag_name.iter().copied()
                    }
                }
            )*

            fn entity_matches_tags(&self, entity: $crate::Entity, include_tags: u64, exclude_tags: u64) -> bool {
                $(
                    if include_tags & $tag_mask != 0 && !self.$tag_name.contains(&entity) {
                        return false;
                    }
                    if exclude_tags & $tag_mask != 0 && self.$tag_name.contains(&entity) {
                        return false;
                    }
                )*
                true
            }

            $(
                $crate::paste::paste! {
                    pub fn [<send_ $event_name>](&mut self, event: $event_type) {
                        self.$event_name.send(event);
                    }

                    pub fn [<read_ $event_name>](&self) -> impl Iterator<Item = &$event_type> {
                        self.$event_name.read()
                    }

                    pub fn [<drain_ $event_name>](&mut self) -> impl Iterator<Item = $event_type> + '_ {
                        self.$event_name.drain()
                    }

                    pub fn [<clear_ $event_name>](&mut self) {
                        self.$event_name.clear();
                    }

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
                self.current_tick += 1;
            }

            pub fn queue_spawn_entities(&mut self, mask: u64, count: usize) {
                self.command_buffer.push(Command::SpawnEntities { mask, count });
            }

            pub fn queue_despawn_entities(&mut self, entities: Vec<$crate::Entity>) {
                self.command_buffer.push(Command::DespawnEntities { entities });
            }

            pub fn queue_despawn_entity(&mut self, entity: $crate::Entity) {
                self.command_buffer.push(Command::DespawnEntities { entities: vec![entity] });
            }

            pub fn queue_add_components(&mut self, entity: $crate::Entity, mask: u64) {
                self.command_buffer.push(Command::AddComponents { entity, mask });
            }

            pub fn queue_remove_components(&mut self, entity: $crate::Entity, mask: u64) {
                self.command_buffer.push(Command::RemoveComponents { entity, mask });
            }

            $(
                $crate::paste::paste! {
                    pub fn [<queue_set_ $name>](&mut self, entity: $crate::Entity, value: $type) {
                        self.command_buffer.push(Command::[<Set $mask:camel>] { entity, value });
                    }
                }
            )*

            $(
                $crate::paste::paste! {
                    pub fn [<queue_add_ $tag_name>](&mut self, entity: $crate::Entity) {
                        self.command_buffer.push(Command::[<Add $tag_mask:camel>] { entity });
                    }

                    pub fn [<queue_remove_ $tag_name>](&mut self, entity: $crate::Entity) {
                        self.command_buffer.push(Command::[<Remove $tag_mask:camel>] { entity });
                    }
                }
            )*

            pub fn apply_commands(&mut self) {
                let commands = std::mem::take(&mut self.command_buffer);

                $crate::paste::paste! {
                    for command in commands {
                        match command {
                            Command::SpawnEntities { mask, count } => {
                                self.spawn_entities(mask, count);
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
            }

            pub fn command_count(&self) -> usize {
                self.command_buffer.len()
            }

            pub fn clear_commands(&mut self) {
                self.command_buffer.clear();
            }


            $(
                $crate::paste::paste! {
                    pub fn [<query_ $name _mut>]<F>(&mut self, mask: u64, mut f: F)
                    where
                        F: FnMut($crate::Entity, &mut $type),
                    {
                        let table_indices: Vec<usize> = self.get_cached_tables(mask).to_vec();

                        for &table_index in &table_indices {
                            let table = &mut self.tables[table_index];
                            if table.mask & $mask == 0 {
                                continue;
                            }

                            for idx in 0..table.entity_indices.len() {
                                let entity = table.entity_indices[idx];
                                f(entity, &mut table.$name[idx]);
                            }
                        }
                    }
                }
            )*
        }

        #[allow(unused)]
        impl $world {
            #[inline]
            pub fn for_each<F>(&self, include: u64, exclude: u64, mut f: F)
            where
                F: FnMut($crate::Entity, &ComponentArrays, usize),
            {
                let component_include = include & !ALL_TAGS_MASK;
                let component_exclude = exclude & !ALL_TAGS_MASK;
                let tag_include = include & ALL_TAGS_MASK;
                let tag_exclude = exclude & ALL_TAGS_MASK;

                if let Some(cached) = self.query_cache.get(&component_include) {
                    for &table_index in cached {
                        let table = &self.tables[table_index];
                        if table.mask & component_exclude != 0 {
                            continue;
                        }
                        if tag_include == 0 && tag_exclude == 0 {
                            for (idx, &entity) in table.entity_indices.iter().enumerate() {
                                f(entity, table, idx);
                            }
                        } else {
                            for (idx, &entity) in table.entity_indices.iter().enumerate() {
                                if self.entity_matches_tags(entity, tag_include, tag_exclude) {
                                    f(entity, table, idx);
                                }
                            }
                        }
                    }
                    return;
                }

                for table in &self.tables {
                    if table.mask & component_include != component_include || table.mask & component_exclude != 0 {
                        continue;
                    }

                    if tag_include == 0 && tag_exclude == 0 {
                        for (idx, &entity) in table.entity_indices.iter().enumerate() {
                            f(entity, table, idx);
                        }
                    } else {
                        for (idx, &entity) in table.entity_indices.iter().enumerate() {
                            if self.entity_matches_tags(entity, tag_include, tag_exclude) {
                                f(entity, table, idx);
                            }
                        }
                    }
                }
            }

            #[inline]
            pub fn for_each_mut<F>(&mut self, include: u64, exclude: u64, mut f: F)
            where
                F: FnMut($crate::Entity, &mut ComponentArrays, usize),
            {
                let component_include = include & !ALL_TAGS_MASK;
                let component_exclude = exclude & !ALL_TAGS_MASK;
                let tag_include = include & ALL_TAGS_MASK;
                let tag_exclude = exclude & ALL_TAGS_MASK;

                let table_indices: Vec<usize> = self.get_cached_tables(component_include).to_vec();

                if tag_include == 0 && tag_exclude == 0 {
                    for &table_index in &table_indices {
                        let table = &mut self.tables[table_index];
                        if table.mask & component_exclude != 0 {
                            continue;
                        }

                        for idx in 0..table.entity_indices.len() {
                            let entity = table.entity_indices[idx];
                            f(entity, table, idx);
                        }
                    }
                } else {
                    for &table_index in &table_indices {
                        let table = &mut self.tables[table_index];
                        if table.mask & component_exclude != 0 {
                            continue;
                        }

                        for idx in 0..table.entity_indices.len() {
                            let entity = table.entity_indices[idx];
                            let mut tag_match = true;
                            $(
                                if tag_include & $tag_mask != 0 && !self.$tag_name.contains(&entity) {
                                    tag_match = false;
                                }
                                if tag_match && tag_exclude & $tag_mask != 0 && self.$tag_name.contains(&entity) {
                                    tag_match = false;
                                }
                            )*
                            if tag_match {
                                f(entity, table, idx);
                            }
                        }
                    }
                }
            }

            #[cfg(not(target_family = "wasm"))]
            #[inline]
            pub fn par_for_each_mut<F>(&mut self, include: u64, exclude: u64, f: F)
            where
                F: Fn($crate::Entity, &mut ComponentArrays, usize) + Send + Sync,
            {
                use $crate::rayon::prelude::*;

                let component_include = include & !ALL_TAGS_MASK;
                let component_exclude = exclude & !ALL_TAGS_MASK;
                let tag_include = include & ALL_TAGS_MASK;
                let tag_exclude = exclude & ALL_TAGS_MASK;

                if tag_include == 0 && tag_exclude == 0 {
                    self.tables
                        .par_iter_mut()
                        .filter(|table| table.mask & component_include == component_include && table.mask & component_exclude == 0)
                        .for_each(|table| {
                            for idx in 0..table.entity_indices.len() {
                                let entity = table.entity_indices[idx];
                                f(entity, table, idx);
                            }
                        });
                } else {
                    $(let $tag_name = &self.$tag_name;)*
                    self.tables
                        .par_iter_mut()
                        .filter(|table| table.mask & component_include == component_include && table.mask & component_exclude == 0)
                        .for_each(|table| {
                            for idx in 0..table.entity_indices.len() {
                                let entity = table.entity_indices[idx];
                                let mut tag_match = true;
                                $(
                                    if tag_include & $tag_mask != 0 && !$tag_name.contains(&entity) {
                                        tag_match = false;
                                    }
                                    if tag_match && tag_exclude & $tag_mask != 0 && $tag_name.contains(&entity) {
                                        tag_match = false;
                                    }
                                )*
                                if tag_match {
                                    f(entity, table, idx);
                                }
                            }
                        });
                }
            }

            #[inline]
            pub fn for_each_mut_changed<F>(&mut self, include: u64, exclude: u64, mut f: F)
            where
                F: FnMut($crate::Entity, &mut ComponentArrays, usize),
            {
                let component_include = include & !ALL_TAGS_MASK;
                let component_exclude = exclude & !ALL_TAGS_MASK;
                let tag_include = include & ALL_TAGS_MASK;
                let tag_exclude = exclude & ALL_TAGS_MASK;

                let table_indices: Vec<usize> = self.get_cached_tables(component_include).to_vec();
                let since_tick = self.last_tick;

                if tag_include == 0 && tag_exclude == 0 {
                    for &table_index in &table_indices {
                        let table = &mut self.tables[table_index];
                        if table.mask & component_exclude != 0 {
                            continue;
                        }

                        for idx in 0..table.entity_indices.len() {
                            let entity = table.entity_indices[idx];

                            let mut changed = false;
                            $(
                                $crate::paste::paste! {
                                    if component_include & $mask != 0 && table.mask & $mask != 0 && table.[<$name _changed>][idx] > since_tick {
                                        changed = true;
                                    }
                                }
                            )*

                            if changed {
                                f(entity, table, idx);
                            }
                        }
                    }
                } else {
                    for &table_index in &table_indices {
                        let table = &mut self.tables[table_index];
                        if table.mask & component_exclude != 0 {
                            continue;
                        }

                        for idx in 0..table.entity_indices.len() {
                            let entity = table.entity_indices[idx];
                            let mut tag_match = true;
                            $(
                                if tag_include & $tag_mask != 0 && !self.$tag_name.contains(&entity) {
                                    tag_match = false;
                                }
                                if tag_match && tag_exclude & $tag_mask != 0 && self.$tag_name.contains(&entity) {
                                    tag_match = false;
                                }
                            )*
                            if !tag_match {
                                continue;
                            }

                            let mut changed = false;
                            $(
                                $crate::paste::paste! {
                                    if component_include & $mask != 0 && table.mask & $mask != 0 && table.[<$name _changed>][idx] > since_tick {
                                        changed = true;
                                    }
                                }
                            )*

                            if changed {
                                f(entity, table, idx);
                            }
                        }
                    }
                }
            }

        }


        #[derive(Default)]
        pub struct $resources {
            $($(#[$attr])* pub $resource_name: $resource_type,)*
        }

        $crate::paste::paste! {
            #[derive(Default)]
            pub struct ComponentArrays {
                $($(#[$comp_attr])* pub $name: Vec<$type>,)*
                $($(#[$comp_attr])* pub [<$name _changed>]: Vec<u32>,)*
                pub entity_indices: Vec<$crate::Entity>,
                pub mask: u64,
            }
        }


        pub struct QueryBuilder<'a> {
            world: &'a $world,
            include: u64,
            exclude: u64,
        }

        impl<'a> QueryBuilder<'a> {
            pub fn new(world: &'a $world) -> Self {
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
                F: FnMut($crate::Entity, &ComponentArrays, usize),
            {
                self.world.for_each(self.include, self.exclude, f);
            }
        }

        pub struct QueryBuilderMut<'a> {
            world: &'a mut $world,
            include: u64,
            exclude: u64,
        }

        impl<'a> QueryBuilderMut<'a> {
            pub fn new(world: &'a mut $world) -> Self {
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
                F: FnMut($crate::Entity, &mut ComponentArrays, usize),
            {
                self.world.for_each_mut(self.include, self.exclude, f);
            }
        }

        impl $world {
            pub fn query(&self) -> QueryBuilder<'_> {
                QueryBuilder::new(self)
            }

            pub fn query_mut(&mut self) -> QueryBuilderMut<'_> {
                QueryBuilderMut::new(self)
            }

            $(
                $(#[$comp_attr])*
                $crate::paste::paste! {
                    pub fn [<iter_ $name>]<F>(&self, mut f: F)
                    where
                        F: FnMut($crate::Entity, &$type),
                    {
                        self.for_each($mask, 0, |entity, table, idx| {
                            f(entity, &table.$name[idx]);
                        });
                    }

                    pub fn [<iter_ $name _mut>]<F>(&mut self, mut f: F)
                    where
                        F: FnMut($crate::Entity, &mut $type),
                    {
                        self.for_each_mut($mask, 0, |entity, table, idx| {
                            f(entity, &mut table.$name[idx]);
                        });
                    }
                }
            )*
        }

        pub struct EntityQueryIter<'a> {
            tables: &'a [ComponentArrays],
            mask: u64,
            table_index: usize,
            array_index: usize,
        }

        impl<'a> Iterator for EntityQueryIter<'a> {
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
                for table_idx in self.table_index..self.tables.len() {
                    let table = &self.tables[table_idx];
                    if table.mask & self.mask != self.mask {
                        continue;
                    }
                    if table_idx == self.table_index {
                        remaining += table.entity_indices.len().saturating_sub(self.array_index);
                    } else {
                        remaining += table.entity_indices.len();
                    }
                }
                (remaining, Some(remaining))
            }
        }

        pub struct ChangedEntityQueryIter<'a> {
            tables: &'a [ComponentArrays],
            mask: u64,
            since_tick: u32,
            table_index: usize,
            array_index: usize,
        }

        impl<'a> Iterator for ChangedEntityQueryIter<'a> {
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

                    let idx = self.array_index;
                    self.array_index += 1;

                    let mut changed = false;
                    $(
                        $crate::paste::paste! {
                            if self.mask & $mask != 0 && table.mask & $mask != 0 && table.[<$name _changed>][idx] > self.since_tick {
                                changed = true;
                            }
                        }
                    )*

                    if changed {
                        return Some(table.entity_indices[idx]);
                    }
                }
            }
        }

        $(
            $(#[$comp_attr])*
            $crate::paste::paste! {
                pub struct [<$mask:camel QueryIter>]<'a> {
                    tables: &'a [ComponentArrays],
                    table_index: usize,
                    array_index: usize,
                }

                impl<'a> Iterator for [<$mask:camel QueryIter>]<'a> {
                    type Item = &'a $type;

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
                        for table_idx in self.table_index..self.tables.len() {
                            let table = &self.tables[table_idx];
                            if table.mask & $mask == 0 {
                                continue;
                            }
                            if table_idx == self.table_index {
                                remaining += table.$name.len().saturating_sub(self.array_index);
                            } else {
                                remaining += table.$name.len();
                            }
                        }
                        (remaining, Some(remaining))
                    }
                }

            }
        )*

        #[derive(Clone)]
        struct TableEdges {
            add_edges: [Option<usize>; COMPONENT_COUNT],
            remove_edges: [Option<usize>; COMPONENT_COUNT],
            multi_add_cache: std::collections::HashMap<u64, usize>,
            multi_remove_cache: std::collections::HashMap<u64, usize>,
        }

        impl Default for TableEdges {
            fn default() -> Self {
                Self {
                    add_edges: [None; COMPONENT_COUNT],
                    remove_edges: [None; COMPONENT_COUNT],
                    multi_add_cache: std::collections::HashMap::default(),
                    multi_remove_cache: std::collections::HashMap::default(),
                }
            }
        }

        fn get_component_index(mask: u64) -> Option<usize> {
            match mask {
                $($(#[$comp_attr])* $mask => Some(Component::$mask as _),)*
                _ => None,
            }
        }

        fn remove_from_table(arrays: &mut ComponentArrays, index: usize) -> Option<$crate::Entity> {
            let last_index = arrays.entity_indices.len() - 1;
            let mut swapped_entity = None;

            if index < last_index {
                swapped_entity = Some(arrays.entity_indices[last_index]);
            }

            $(
                $(#[$comp_attr])*
                $crate::paste::paste! {
                    if arrays.mask & $mask != 0 {
                        arrays.$name.swap_remove(index);
                        arrays.[<$name _changed>].swap_remove(index);
                    }
                }
            )*
            arrays.entity_indices.swap_remove(index);

            swapped_entity
        }

        fn move_entity(
            world: &mut $world,
            entity: $crate::Entity,
            from_table: usize,
            from_index: usize,
            to_table: usize,
        ) {
            let tick = world.current_tick;
            let components = {
                let from_table_ref = &mut world.tables[from_table];
                (
                    $(
                        $(#[$comp_attr])*
                        {
                            if from_table_ref.mask & $mask != 0 {
                                Some(std::mem::take(&mut from_table_ref.$name[from_index]))
                            } else {
                                None
                            }
                        },
                    )*
                )
            };

            add_to_table(&mut world.tables[to_table], entity, components, tick);
            let new_index = world.tables[to_table].entity_indices.len() - 1;
            insert_location(&mut world.entity_locations, entity, (to_table, new_index));

            if let Some(swapped) = remove_from_table(&mut world.tables[from_table], from_index) {
                insert_location(
                    &mut world.entity_locations,
                    swapped,
                    (from_table, from_index),
                );
            }
        }

        fn get_location(locations: &$crate::EntityLocations, entity: $crate::Entity) -> Option<(usize, usize)> {
            let location = locations.get(entity.id)?;
            if !location.allocated || location.generation != entity.generation {
                return None;
            }

            Some((location.table_index as usize, location.array_index as usize))
        }

        fn insert_location(
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

        fn create_entity(world: &mut $world) -> $crate::Entity {
            let entity = world.allocator.allocate();
            world.entity_locations.ensure_slot(entity.id, entity.generation);
            entity
        }

        fn add_to_table(
            arrays: &mut ComponentArrays,
            entity: $crate::Entity,
            components: ( $(Option<$type>,)* ),
            tick: u32,
        ) {
            let ($($name,)*) = components;
            $(
                $(#[$comp_attr])*
                $crate::paste::paste! {
                    if arrays.mask & $mask != 0 {
                        if let Some(component) = $name {
                            arrays.$name.push(component);
                        } else {
                            arrays.$name.push(<$type>::default());
                        }
                        arrays.[<$name _changed>].push(tick);
                    }
                }
            )*
            arrays.entity_indices.push(entity);
        }

        fn get_or_create_table(world: &mut $world, mask: u64) -> usize {
            if let Some(&index) = world.table_lookup.get(&mask) {
                return index;
            }

            let table_index = world.tables.len();
            world.tables.push(ComponentArrays {
                mask,
                ..Default::default()
            });
            world.table_edges.push(TableEdges::default());
            world.table_lookup.insert(mask, table_index);

            world.invalidate_query_cache_for_table(mask, table_index);

            for comp_mask in [$($(#[$comp_attr])* $mask,)*] {
                if let Some(comp_idx) = get_component_index(comp_mask) {
                    for (idx, table) in world.tables.iter().enumerate() {
                        if table.mask | comp_mask == mask {
                            world.table_edges[idx].add_edges[comp_idx] = Some(table_index);
                        }
                        if table.mask & !comp_mask == mask {
                            world.table_edges[idx].remove_edges[comp_idx] = Some(table_index);
                        }
                    }
                }
            }

            table_index
        }
    };
}

#[macro_export]
macro_rules! ecs_world_impl {
    (
        $world:ident {
            $($(#[$comp_attr:meta])* $name:ident: $type:ty => $mask:ident),* $(,)?
        }
    ) => {
        $crate::paste::paste! {
            #[repr(u64)]
            #[allow(clippy::upper_case_acronyms)]
            #[allow(non_camel_case_types)]
            pub enum [<$world Component>] {
                $($(#[$comp_attr])* $mask,)*
            }

            $($(#[$comp_attr])* pub const $mask: u64 = 1 << ([<$world Component>]::$mask as u64);)*

            pub const [<$world:snake:upper _COMPONENT_COUNT>]: usize = {
                let mut count = 0;
                $($(#[$comp_attr])* { count += 1; let _ = [<$world Component>]::$mask; })*
                count
            };

            #[derive(Default)]
            pub struct [<$world ComponentArrays>] {
                $($(#[$comp_attr])* pub $name: Vec<$type>,)*
                $($(#[$comp_attr])* pub [<$name _changed>]: Vec<u32>,)*
                pub entity_indices: Vec<$crate::Entity>,
                pub mask: u64,
            }

            #[derive(Clone)]
            struct [<$world TableEdges>] {
                add_edges: [Option<usize>; [<$world:snake:upper _COMPONENT_COUNT>]],
                remove_edges: [Option<usize>; [<$world:snake:upper _COMPONENT_COUNT>]],
                multi_add_cache: std::collections::HashMap<u64, usize>,
                multi_remove_cache: std::collections::HashMap<u64, usize>,
            }

            impl Default for [<$world TableEdges>] {
                fn default() -> Self {
                    Self {
                        add_edges: [None; [<$world:snake:upper _COMPONENT_COUNT>]],
                        remove_edges: [None; [<$world:snake:upper _COMPONENT_COUNT>]],
                        multi_add_cache: std::collections::HashMap::default(),
                        multi_remove_cache: std::collections::HashMap::default(),
                    }
                }
            }

            #[allow(unused)]
            pub struct $world {
                pub entity_locations: $crate::EntityLocations,
                tables: Vec<[<$world ComponentArrays>]>,
                table_edges: Vec<[<$world TableEdges>]>,
                table_lookup: std::collections::HashMap<u64, usize>,
                query_cache: std::collections::HashMap<u64, Vec<usize>>,
                current_tick: u32,
                last_tick: u32,
            }

            impl Default for $world {
                fn default() -> Self {
                    Self {
                        entity_locations: $crate::EntityLocations::default(),
                        tables: Vec::default(),
                        table_edges: Vec::default(),
                        table_lookup: std::collections::HashMap::default(),
                        query_cache: std::collections::HashMap::default(),
                        current_tick: 0,
                        last_tick: 0,
                    }
                }
            }

            #[allow(unused)]
            impl $world {
                fn get_cached_tables(&mut self, mask: u64) -> &[usize] {
                    if !self.query_cache.contains_key(&mask) {
                        let matching_tables: Vec<usize> = self.tables
                            .iter()
                            .enumerate()
                            .filter(|(_, table)| table.mask & mask == mask)
                            .map(|(idx, _)| idx)
                            .collect();
                        self.query_cache.insert(mask, matching_tables);
                    }
                    &self.query_cache[&mask]
                }

                fn invalidate_query_cache_for_table(&mut self, new_table_mask: u64, new_table_index: usize) {
                    self.query_cache.retain(|query_mask, cached_tables| {
                        if new_table_mask & query_mask == *query_mask {
                            cached_tables.push(new_table_index);
                            true
                        } else {
                            true
                        }
                    });
                }

                $(
                    $(#[$comp_attr])*
                    $crate::paste::paste! {
                        #[inline]
                        pub fn [<get_ $name>](&self, entity: $crate::Entity) -> Option<&$type> {
                            let (table_index, array_index) = [<get_location_ $world:snake>](&self.entity_locations, entity)?;
                            let table = &self.tables[table_index];
                            if table.mask & $mask == 0 {
                                return None;
                            }
                            Some(&table.$name[array_index])
                        }

                        #[inline]
                        pub fn [<get_ $name _mut>](&mut self, entity: $crate::Entity) -> Option<&mut $type> {
                            let (table_index, array_index) = [<get_location_ $world:snake>](&self.entity_locations, entity)?;
                            let current_tick = self.current_tick;
                            let table = &mut self.tables[table_index];
                            if table.mask & $mask == 0 {
                                return None;
                            }
                            table.[<$name _changed>][array_index] = current_tick;
                            Some(&mut table.$name[array_index])
                        }

                        #[inline]
                        pub fn [<modify_ $name>]<R>(&mut self, entity: $crate::Entity, f: impl FnOnce(&mut $type) -> R) -> Option<R> {
                            let (table_index, array_index) = [<get_location_ $world:snake>](&self.entity_locations, entity)?;
                            let current_tick = self.current_tick;
                            let table = &mut self.tables[table_index];
                            if table.mask & $mask == 0 {
                                return None;
                            }
                            table.[<$name _changed>][array_index] = current_tick;
                            Some(f(&mut table.$name[array_index]))
                        }

                        #[inline]
                        pub fn [<entity_has_ $name>](&self, entity: $crate::Entity) -> bool {
                            self.entity_has_components(entity, $mask)
                        }

                        #[inline]
                        pub fn [<set_ $name>](&mut self, entity: $crate::Entity, value: $type) {
                            if let Some((table_index, array_index)) = [<get_location_ $world:snake>](&self.entity_locations, entity) {
                                let table = &mut self.tables[table_index];
                                if table.mask & $mask != 0 {
                                    table.$name[array_index] = value;
                                    return;
                                }
                            }
                            self.add_components(entity, $mask);
                            if let Some((table_index, array_index)) = [<get_location_ $world:snake>](&self.entity_locations, entity) {
                                self.tables[table_index].$name[array_index] = value;
                            }
                        }

                        #[inline]
                        pub fn [<add_ $name>](&mut self, entity: $crate::Entity) {
                            self.add_components(entity, $mask);
                        }

                        #[inline]
                        pub fn [<remove_ $name>](&mut self, entity: $crate::Entity) -> bool {
                            self.remove_components(entity, $mask)
                        }

                        #[inline]
                        pub fn [<query_ $name>](&self) -> [<$mask:camel QueryIter>]<'_> {
                            [<$mask:camel QueryIter>] {
                                tables: &self.tables,
                                table_index: 0,
                                array_index: 0,
                            }
                        }

                        pub fn [<for_each_ $name _mut>]<F>(&mut self, mut f: F)
                        where
                            F: FnMut(&mut $type),
                        {
                            let table_indices: Vec<usize> = self.get_cached_tables($mask).to_vec();
                            for table_index in table_indices {
                                for component in &mut self.tables[table_index].$name {
                                    f(component);
                                }
                            }
                        }

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

                        pub fn [<iter_ $name _slices>](&self) -> impl Iterator<Item = &[$type]> {
                            self.tables
                                .iter()
                                .filter(|table| table.mask & $mask != 0)
                                .map(|table| table.$name.as_slice())
                        }

                        pub fn [<iter_ $name _slices_mut>](&mut self) -> impl Iterator<Item = &mut [$type]> {
                            self.tables
                                .iter_mut()
                                .filter(|table| table.mask & $mask != 0)
                                .map(|table| table.$name.as_mut_slice())
                        }
                    }
                )*

                pub fn spawn_entities(&mut self, allocator: &mut $crate::EntityAllocator, mask: u64, count: usize) -> Vec<$crate::Entity> {
                    let mut entities = Vec::with_capacity(count);
                    let table_index = [<get_or_create_table_ $world:snake>](self, mask);
                    let start_index = self.tables[table_index].entity_indices.len();

                    self.tables[table_index].entity_indices.reserve(count);
                    $(
                        $(#[$comp_attr])*
                        {
                            if mask & $mask != 0 {
                                self.tables[table_index].$name.reserve(count);
                                self.tables[table_index].[<$name _changed>].reserve(count);
                            }
                        }
                    )*

                    for local_index in 0..count {
                        let entity = allocator.allocate();
                        self.entity_locations.ensure_slot(entity.id, entity.generation);
                        entities.push(entity);

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
                            (table_index, start_index + local_index),
                        );
                    }

                    entities
                }

                pub fn spawn_batch<F>(&mut self, allocator: &mut $crate::EntityAllocator, mask: u64, count: usize, mut init: F) -> Vec<$crate::Entity>
                where
                    F: FnMut(&mut [<$world ComponentArrays>], usize),
                {
                    let table_index = [<get_or_create_table_ $world:snake>](self, mask);
                    let start_index = self.tables[table_index].entity_indices.len();

                    self.tables[table_index].entity_indices.reserve(count);
                    $(
                        $(#[$comp_attr])*
                        {
                            if mask & $mask != 0 {
                                self.tables[table_index].$name.reserve(count);
                                self.tables[table_index].[<$name _changed>].reserve(count);
                            }
                        }
                    )*

                    let mut entities = Vec::with_capacity(count);
                    for local_index in 0..count {
                        let entity = allocator.allocate();
                        self.entity_locations.ensure_slot(entity.id, entity.generation);
                        entities.push(entity);

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
                            (table_index, start_index + local_index),
                        );

                        init(&mut self.tables[table_index], start_index + local_index);
                    }

                    entities
                }

                pub fn query_entities(&self, mask: u64) -> [<$world EntityQueryIter>]<'_> {
                    [<$world EntityQueryIter>] {
                        tables: &self.tables,
                        mask,
                        table_index: 0,
                        array_index: 0,
                    }
                }

                pub fn query_entities_changed(&self, mask: u64) -> [<$world ChangedEntityQueryIter>]<'_> {
                    [<$world ChangedEntityQueryIter>] {
                        tables: &self.tables,
                        mask,
                        since_tick: self.last_tick,
                        table_index: 0,
                        array_index: 0,
                    }
                }

                pub fn query_first_entity(&self, mask: u64) -> Option<$crate::Entity> {
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

                pub fn remove_entity(&mut self, entity: $crate::Entity) -> bool {
                    if let Some(loc) = self.entity_locations.get_mut(entity.id) {
                        if loc.allocated && loc.generation == entity.generation {
                            let table_idx = loc.table_index as usize;
                            let array_idx = loc.array_index as usize;

                            self.entity_locations.mark_deallocated(entity.id);

                            if table_idx < self.tables.len() {
                                let table = &mut self.tables[table_idx];
                                if !table.entity_indices.is_empty() {
                                    let last_idx = table.entity_indices.len() - 1;

                                    if array_idx < last_idx {
                                        let moved_entity = table.entity_indices[last_idx];
                                        if let Some(moved_loc) = self.entity_locations.get_mut(moved_entity.id) {
                                            if moved_loc.allocated {
                                                moved_loc.array_index = array_idx as u32;
                                            }
                                        }
                                    }

                                    $(
                                        $(#[$comp_attr])*
                                        {
                                            if table.mask & $mask != 0 {
                                                table.$name.swap_remove(array_idx);
                                                table.[<$name _changed>].swap_remove(array_idx);
                                            }
                                        }
                                    )*
                                    table.entity_indices.swap_remove(array_idx);
                                }
                            }
                            return true;
                        }
                    }
                    false
                }

                pub fn add_components(&mut self, entity: $crate::Entity, mask: u64) -> bool {
                    if let Some((table_index, array_index)) = [<get_location_ $world:snake>](&self.entity_locations, entity) {
                        let current_mask = self.tables[table_index].mask;
                        if current_mask & mask == mask {
                            return true;
                        }

                        let target_table = if mask.count_ones() == 1 {
                            [<get_component_index_ $world:snake>](mask).and_then(|idx| self.table_edges[table_index].add_edges[idx])
                        } else {
                            self.table_edges[table_index].multi_add_cache.get(&mask).copied()
                        };

                        let new_table_index = target_table.unwrap_or_else(|| {
                            let new_idx = [<get_or_create_table_ $world:snake>](self, current_mask | mask);
                            self.table_edges[table_index].multi_add_cache.insert(mask, new_idx);
                            new_idx
                        });

                        [<move_entity_ $world:snake>](self, entity, table_index, array_index, new_table_index);
                        true
                    } else {
                        if let Some(loc) = self.entity_locations.get(entity.id) {
                            if loc.allocated {
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
                                }
                            }
                        )*

                        self.entity_locations.insert(entity.id, $crate::EntityLocation {
                            generation: entity.generation,
                            table_index: table_index as u32,
                            array_index: start_index as u32,
                            allocated: true,
                        });
                        true
                    }
                }

                pub fn remove_components(&mut self, entity: $crate::Entity, mask: u64) -> bool {
                    if let Some((table_index, array_index)) = [<get_location_ $world:snake>](&self.entity_locations, entity) {
                        let current_mask = self.tables[table_index].mask;
                        if current_mask & mask == 0 {
                            return true;
                        }

                        let target_table = if mask.count_ones() == 1 {
                            [<get_component_index_ $world:snake>](mask)
                                .and_then(|idx| self.table_edges[table_index].remove_edges[idx])
                        } else {
                            self.table_edges[table_index].multi_remove_cache.get(&mask).copied()
                        };

                        let new_table_index = target_table.unwrap_or_else(|| {
                            let new_idx = [<get_or_create_table_ $world:snake>](self, current_mask & !mask);
                            self.table_edges[table_index].multi_remove_cache.insert(mask, new_idx);
                            new_idx
                        });

                        [<move_entity_ $world:snake>](self, entity, table_index, array_index, new_table_index);
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

                $(
                    $(#[$comp_attr])*
                    $crate::paste::paste! {
                        pub fn [<query_ $name _mut>]<F>(&mut self, mask: u64, mut f: F)
                        where
                            F: FnMut($crate::Entity, &mut $type),
                        {
                            let table_indices: Vec<usize> = self.get_cached_tables(mask).to_vec();
                            for &table_index in &table_indices {
                                let table = &mut self.tables[table_index];
                                if table.mask & $mask == 0 {
                                    continue;
                                }
                                for idx in 0..table.entity_indices.len() {
                                    let entity = table.entity_indices[idx];
                                    f(entity, &mut table.$name[idx]);
                                }
                            }
                        }
                    }
                )*
            }

            #[allow(unused)]
            impl $world {
                #[inline]
                pub fn for_each<F>(&self, include: u64, exclude: u64, mut f: F)
                where
                    F: FnMut($crate::Entity, &[<$world ComponentArrays>], usize),
                {
                    for table in &self.tables {
                        if table.mask & include != include || table.mask & exclude != 0 {
                            continue;
                        }
                        for (idx, &entity) in table.entity_indices.iter().enumerate() {
                            f(entity, table, idx);
                        }
                    }
                }

                #[inline]
                pub fn for_each_mut<F>(&mut self, include: u64, exclude: u64, mut f: F)
                where
                    F: FnMut($crate::Entity, &mut [<$world ComponentArrays>], usize),
                {
                    let table_indices: Vec<usize> = self.get_cached_tables(include).to_vec();

                    for &table_index in &table_indices {
                        let table = &mut self.tables[table_index];
                        if table.mask & exclude != 0 {
                            continue;
                        }
                        for idx in 0..table.entity_indices.len() {
                            let entity = table.entity_indices[idx];
                            f(entity, table, idx);
                        }
                    }
                }

                #[cfg(not(target_family = "wasm"))]
                #[inline]
                pub fn par_for_each_mut<F>(&mut self, include: u64, exclude: u64, f: F)
                where
                    F: Fn($crate::Entity, &mut [<$world ComponentArrays>], usize) + Send + Sync,
                {
                    use $crate::rayon::prelude::*;

                    self.tables
                        .par_iter_mut()
                        .filter(|table| table.mask & include == include && table.mask & exclude == 0)
                        .for_each(|table| {
                            for idx in 0..table.entity_indices.len() {
                                let entity = table.entity_indices[idx];
                                f(entity, table, idx);
                            }
                        });
                }

                #[inline]
                pub fn for_each_mut_changed<F>(&mut self, include: u64, exclude: u64, mut f: F)
                where
                    F: FnMut($crate::Entity, &mut [<$world ComponentArrays>], usize),
                {
                    let table_indices: Vec<usize> = self.get_cached_tables(include).to_vec();
                    let since_tick = self.last_tick;

                    for &table_index in &table_indices {
                        let table = &mut self.tables[table_index];
                        if table.mask & exclude != 0 {
                            continue;
                        }

                        for idx in 0..table.entity_indices.len() {
                            let entity = table.entity_indices[idx];

                            let mut changed = false;
                            $(
                                $(#[$comp_attr])*
                                {
                                    if include & $mask != 0 && table.mask & $mask != 0 && table.[<$name _changed>][idx] > since_tick {
                                        changed = true;
                                    }
                                }
                            )*

                            if changed {
                                f(entity, table, idx);
                            }
                        }
                    }
                }
            }

            #[allow(unused)]
            impl $world {
                #[inline]
                pub fn for_each_with_tags<F>(
                    &self,
                    include: u64,
                    exclude: u64,
                    include_tags: &[&std::collections::HashSet<$crate::Entity>],
                    exclude_tags: &[&std::collections::HashSet<$crate::Entity>],
                    mut f: F,
                )
                where
                    F: FnMut($crate::Entity, &[<$world ComponentArrays>], usize),
                {
                    let has_tag_filter = !include_tags.is_empty() || !exclude_tags.is_empty();

                    for table in &self.tables {
                        if table.mask & include != include || table.mask & exclude != 0 {
                            continue;
                        }

                        if has_tag_filter {
                            for (idx, &entity) in table.entity_indices.iter().enumerate() {
                                if include_tags.iter().all(|tag_set| tag_set.contains(&entity))
                                    && !exclude_tags.iter().any(|tag_set| tag_set.contains(&entity))
                                {
                                    f(entity, table, idx);
                                }
                            }
                        } else {
                            for (idx, &entity) in table.entity_indices.iter().enumerate() {
                                f(entity, table, idx);
                            }
                        }
                    }
                }

                #[inline]
                pub fn for_each_mut_with_tags<F>(
                    &mut self,
                    include: u64,
                    exclude: u64,
                    include_tags: &[&std::collections::HashSet<$crate::Entity>],
                    exclude_tags: &[&std::collections::HashSet<$crate::Entity>],
                    mut f: F,
                )
                where
                    F: FnMut($crate::Entity, &mut [<$world ComponentArrays>], usize),
                {
                    let has_tag_filter = !include_tags.is_empty() || !exclude_tags.is_empty();
                    let table_indices: Vec<usize> = self.get_cached_tables(include).to_vec();

                    if has_tag_filter {
                        let matching_entities: std::collections::HashSet<$crate::Entity> = table_indices
                            .iter()
                            .filter_map(|&idx| self.tables.get(idx))
                            .filter(|table| table.mask & exclude == 0)
                            .flat_map(|table| table.entity_indices.iter().copied())
                            .filter(|entity| {
                                include_tags.iter().all(|tag_set| tag_set.contains(entity))
                                    && !exclude_tags.iter().any(|tag_set| tag_set.contains(entity))
                            })
                            .collect();

                        for &table_index in &table_indices {
                            let table = &mut self.tables[table_index];
                            if table.mask & exclude != 0 {
                                continue;
                            }
                            for idx in 0..table.entity_indices.len() {
                                let entity = table.entity_indices[idx];
                                if matching_entities.contains(&entity) {
                                    f(entity, table, idx);
                                }
                            }
                        }
                    } else {
                        for &table_index in &table_indices {
                            let table = &mut self.tables[table_index];
                            if table.mask & exclude != 0 {
                                continue;
                            }
                            for idx in 0..table.entity_indices.len() {
                                let entity = table.entity_indices[idx];
                                f(entity, table, idx);
                            }
                        }
                    }
                }

                #[cfg(not(target_family = "wasm"))]
                #[inline]
                pub fn par_for_each_mut_with_tags<F>(
                    &mut self,
                    include: u64,
                    exclude: u64,
                    include_tags: &[&std::collections::HashSet<$crate::Entity>],
                    exclude_tags: &[&std::collections::HashSet<$crate::Entity>],
                    f: F,
                )
                where
                    F: Fn($crate::Entity, &mut [<$world ComponentArrays>], usize) + Send + Sync,
                {
                    use $crate::rayon::prelude::*;

                    let table_indices: Vec<usize> = self.get_cached_tables(include).to_vec();
                    let table_index_set: std::collections::HashSet<usize> = table_indices.iter().copied().collect();
                    let has_tag_filter = !include_tags.is_empty() || !exclude_tags.is_empty();

                    if has_tag_filter {
                        let matching_entities: std::collections::HashSet<$crate::Entity> = table_indices
                            .iter()
                            .filter_map(|&idx| self.tables.get(idx))
                            .filter(|table| table.mask & exclude == 0)
                            .flat_map(|table| table.entity_indices.iter().copied())
                            .filter(|entity| {
                                include_tags.iter().all(|tag_set| tag_set.contains(entity))
                                    && !exclude_tags.iter().any(|tag_set| tag_set.contains(entity))
                            })
                            .collect();

                        self.tables
                            .par_iter_mut()
                            .enumerate()
                            .filter(|(idx, table)| table_index_set.contains(idx) && table.mask & exclude == 0)
                            .for_each(|(_, table)| {
                                for idx in 0..table.entity_indices.len() {
                                    let entity = table.entity_indices[idx];
                                    if matching_entities.contains(&entity) {
                                        f(entity, table, idx);
                                    }
                                }
                            });
                    } else {
                        self.tables
                            .par_iter_mut()
                            .enumerate()
                            .filter(|(idx, table)| table_index_set.contains(idx) && table.mask & exclude == 0)
                            .for_each(|(_, table)| {
                                for idx in 0..table.entity_indices.len() {
                                    let entity = table.entity_indices[idx];
                                    f(entity, table, idx);
                                }
                            });
                    }
                }
            }

            #[allow(unused)]
            pub struct [<$world QueryBuilder>]<'a> {
                world: &'a $world,
                include: u64,
                exclude: u64,
            }

            #[allow(unused)]
            impl<'a> [<$world QueryBuilder>]<'a> {
                pub fn new(world: &'a $world) -> Self {
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
            pub struct [<$world QueryBuilderMut>]<'a> {
                world: &'a mut $world,
                include: u64,
                exclude: u64,
            }

            #[allow(unused)]
            impl<'a> [<$world QueryBuilderMut>]<'a> {
                pub fn new(world: &'a mut $world) -> Self {
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
                pub fn query(&self) -> [<$world QueryBuilder>]<'_> {
                    [<$world QueryBuilder>]::new(self)
                }

                pub fn query_mut(&mut self) -> [<$world QueryBuilderMut>]<'_> {
                    [<$world QueryBuilderMut>]::new(self)
                }

                $(
                    $(#[$comp_attr])*
                    $crate::paste::paste! {
                        pub fn [<iter_ $name>]<F>(&self, mut f: F)
                        where
                            F: FnMut($crate::Entity, &$type),
                        {
                            self.for_each($mask, 0, |entity, table, idx| {
                                f(entity, &table.$name[idx]);
                            });
                        }

                        pub fn [<iter_ $name _mut>]<F>(&mut self, mut f: F)
                        where
                            F: FnMut($crate::Entity, &mut $type),
                        {
                            self.for_each_mut($mask, 0, |entity, table, idx| {
                                f(entity, &mut table.$name[idx]);
                            });
                        }
                    }
                )*
            }

            pub struct [<$world EntityQueryIter>]<'a> {
                tables: &'a [[<$world ComponentArrays>]],
                mask: u64,
                table_index: usize,
                array_index: usize,
            }

            impl<'a> Iterator for [<$world EntityQueryIter>]<'a> {
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
                    for table_idx in self.table_index..self.tables.len() {
                        let table = &self.tables[table_idx];
                        if table.mask & self.mask != self.mask {
                            continue;
                        }
                        if table_idx == self.table_index {
                            remaining += table.entity_indices.len().saturating_sub(self.array_index);
                        } else {
                            remaining += table.entity_indices.len();
                        }
                    }
                    (remaining, Some(remaining))
                }
            }

            pub struct [<$world ChangedEntityQueryIter>]<'a> {
                tables: &'a [[<$world ComponentArrays>]],
                mask: u64,
                since_tick: u32,
                table_index: usize,
                array_index: usize,
            }

            impl<'a> Iterator for [<$world ChangedEntityQueryIter>]<'a> {
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
                        let idx = self.array_index;
                        self.array_index += 1;

                        let mut changed = false;
                        $(
                            $(#[$comp_attr])*
                            {
                                if self.mask & $mask != 0 && table.mask & $mask != 0 && table.[<$name _changed>][idx] > self.since_tick {
                                    changed = true;
                                }
                            }
                        )*

                        if changed {
                            return Some(table.entity_indices[idx]);
                        }
                    }
                }
            }

            $(
                $(#[$comp_attr])*
                $crate::paste::paste! {
                    pub struct [<$mask:camel QueryIter>]<'a> {
                        tables: &'a [[<$world ComponentArrays>]],
                        table_index: usize,
                        array_index: usize,
                    }

                    impl<'a> Iterator for [<$mask:camel QueryIter>]<'a> {
                        type Item = &'a $type;

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
                            for table_idx in self.table_index..self.tables.len() {
                                let table = &self.tables[table_idx];
                                if table.mask & $mask == 0 {
                                    continue;
                                }
                                if table_idx == self.table_index {
                                    remaining += table.$name.len().saturating_sub(self.array_index);
                                } else {
                                    remaining += table.$name.len();
                                }
                            }
                            (remaining, Some(remaining))
                        }
                    }
                }
            )*

            fn [<get_component_index_ $world:snake>](mask: u64) -> Option<usize> {
                match mask {
                    $($(#[$comp_attr])* $mask => Some([<$world Component>]::$mask as _),)*
                    _ => None,
                }
            }

            fn [<remove_from_table_ $world:snake>](arrays: &mut [<$world ComponentArrays>], index: usize) -> Option<$crate::Entity> {
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

            fn [<move_entity_ $world:snake>](
                world: &mut $world,
                entity: $crate::Entity,
                from_table: usize,
                from_index: usize,
                to_table: usize,
            ) {
                let tick = world.current_tick;
                let components = {
                    let from_table_ref = &mut world.tables[from_table];
                    (
                        $(
                            $(#[$comp_attr])*
                            {
                                if from_table_ref.mask & $mask != 0 {
                                    Some(std::mem::take(&mut from_table_ref.$name[from_index]))
                                } else {
                                    None
                                }
                            },
                        )*
                    )
                };

                [<add_to_table_ $world:snake>](&mut world.tables[to_table], entity, components, tick);
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

            fn [<get_location_ $world:snake>](locations: &$crate::EntityLocations, entity: $crate::Entity) -> Option<(usize, usize)> {
                let location = locations.get(entity.id)?;
                if !location.allocated || location.generation != entity.generation {
                    return None;
                }
                Some((location.table_index as usize, location.array_index as usize))
            }

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

            fn [<add_to_table_ $world:snake>](
                arrays: &mut [<$world ComponentArrays>],
                entity: $crate::Entity,
                components: ( $($(#[$comp_attr])* Option<$type>,)* ),
                tick: u32,
            ) {
                let ($($name,)*) = components;
                $(
                    $(#[$comp_attr])*
                    {
                        if arrays.mask & $mask != 0 {
                            if let Some(component) = $name {
                                arrays.$name.push(component);
                            } else {
                                arrays.$name.push(<$type>::default());
                            }
                            arrays.[<$name _changed>].push(tick);
                        }
                    }
                )*
                arrays.entity_indices.push(entity);
            }

            fn [<get_or_create_table_ $world:snake>](world: &mut $world, mask: u64) -> usize {
                if let Some(&index) = world.table_lookup.get(&mask) {
                    return index;
                }

                let table_index = world.tables.len();
                world.tables.push([<$world ComponentArrays>] {
                    mask,
                    ..Default::default()
                });
                world.table_edges.push([<$world TableEdges>]::default());
                world.table_lookup.insert(mask, table_index);

                world.invalidate_query_cache_for_table(mask, table_index);

                for comp_mask in [$($(#[$comp_attr])* $mask,)*] {
                    if let Some(comp_idx) = [<get_component_index_ $world:snake>](comp_mask) {
                        for (idx, table) in world.tables.iter().enumerate() {
                            if table.mask | comp_mask == mask {
                                world.table_edges[idx].add_edges[comp_idx] = Some(table_index);
                            }
                            if table.mask & !comp_mask == mask {
                                world.table_edges[idx].remove_edges[comp_idx] = Some(table_index);
                            }
                        }
                    }
                }

                table_index
            }
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
        $(
            $crate::ecs_world_impl! {
                $world_name {
                    $($(#[$comp_attr])* $name: $type => $mask),*
                }
            }
        )+

        #[derive(Default)]
        pub struct $resources {
            $($(#[$attr])* pub $resource_name: $resource_type,)*
        }

        $crate::paste::paste! {
            #[allow(unused)]
            #[derive(Default, Debug, Clone)]
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
                        entities.push(ecs.allocator.allocate());
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
            pub struct $ecs {
                $(pub [<$world_name:snake>]: $world_name,)+
                allocator: $crate::EntityAllocator,
                pub resources: $resources,
                $(pub $tag_name: std::collections::HashSet<$crate::Entity>,)*
                command_buffer: Vec<Command>,
                $($event_name: $crate::EventQueue<$event_type>,)*
            }

            impl Default for $ecs {
                fn default() -> Self {
                    Self {
                        $([<$world_name:snake>]: $world_name::default(),)+
                        allocator: $crate::EntityAllocator::default(),
                        resources: $resources::default(),
                        $(
                            $tag_name: std::collections::HashSet::default(),
                        )*
                        command_buffer: Vec::default(),
                        $(
                            $event_name: $crate::EventQueue::new(),
                        )*
                    }
                }
            }

            #[allow(unused)]
            impl $ecs {
                pub fn spawn(&mut self) -> $crate::Entity {
                    self.allocator.allocate()
                }

                pub fn spawn_count(&mut self, count: usize) -> Vec<$crate::Entity> {
                    let mut entities = Vec::with_capacity(count);
                    for _ in 0..count {
                        entities.push(self.allocator.allocate());
                    }
                    entities
                }

                pub fn despawn(&mut self, entity: $crate::Entity) {
                    $(self.[<$world_name:snake>].remove_entity(entity);)+
                    $(self.$tag_name.remove(&entity);)*
                    self.allocator.deallocate(entity);
                }

                pub fn despawn_entities(&mut self, entities: &[$crate::Entity]) {
                    for &entity in entities {
                        self.despawn(entity);
                    }
                }

                $(
                    pub fn [<add_ $tag_name>](&mut self, entity: $crate::Entity) {
                        self.$tag_name.insert(entity);
                    }

                    pub fn [<remove_ $tag_name>](&mut self, entity: $crate::Entity) -> bool {
                        self.$tag_name.remove(&entity)
                    }

                    pub fn [<has_ $tag_name>](&self, entity: $crate::Entity) -> bool {
                        self.$tag_name.contains(&entity)
                    }

                    pub fn [<query_ $tag_name>](&self) -> impl Iterator<Item = $crate::Entity> + '_ {
                        self.$tag_name.iter().copied()
                    }
                )*

                $(
                    pub fn [<send_ $event_name>](&mut self, event: $event_type) {
                        self.$event_name.send(event);
                    }

                    pub fn [<read_ $event_name>](&self) -> impl Iterator<Item = &$event_type> {
                        self.$event_name.read()
                    }

                    pub fn [<drain_ $event_name>](&mut self) -> impl Iterator<Item = $event_type> + '_ {
                        self.$event_name.drain()
                    }

                    pub fn [<clear_ $event_name>](&mut self) {
                        self.$event_name.clear();
                    }

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
                        self.[<$world_name:snake>].last_tick = self.[<$world_name:snake>].current_tick;
                        self.[<$world_name:snake>].current_tick += 1;
                    )+
                }

                pub fn queue_spawn(&mut self, count: usize) {
                    self.command_buffer.push(Command::Spawn { count });
                }

                pub fn queue_despawn_entity(&mut self, entity: $crate::Entity) {
                    self.command_buffer.push(Command::Despawn { entities: vec![entity] });
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
        Resources {
            _delta_time: f32,
        }
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
        let entity2_loc = get_location(&world.entity_locations, entity2);
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
                    get_location(&world.entity_locations, entity).is_some(),
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

        let (old_table_idx, _) = get_location(&world.entity_locations, entity).unwrap();

        world.add_components(entity, POSITION);

        let final_mask = world.component_mask(entity).unwrap();
        println!("Final mask: {:b}", final_mask);
        let (new_table_idx, _) = get_location(&world.entity_locations, entity).unwrap();

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
                if player.contains(&entity) {
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
    }
}
