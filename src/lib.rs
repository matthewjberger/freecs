//! A high-performance, archetype-based Entity Component System (ECS) for Rust.
//!
//! freecs provides a table-based storage system where entities with identical component sets
//! are stored together in contiguous memory (Structure of Arrays layout), optimizing for cache
//! coherency and SIMD operations.
//!
//! # Key Features
//!
//! - **Zero-cost Abstractions**: Fully statically dispatched, no generics or traits
//! - **Parallel Processing**: Multi-threaded iteration using Rayon
//! - **Sparse Set Tags**: Lightweight markers that don't fragment archetypes
//! - **Command Buffers**: Queue structural changes during iteration
//! - **Change Detection**: Track component modifications for incremental updates
//! - **Events**: Type-safe double-buffered event system
//!
//! The `ecs!` macro generates the entire ECS at compile time. The core implementation is ~500 LOC,
//! contains only plain data structures and functions, and uses zero unsafe code.
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
//! // Clean up events at end of frame
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
//! Process large entity counts across multiple CPU cores using Rayon:
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
//! Track which components have been modified:
//!
//! ```rust
//! # use freecs::{ecs, Entity};
//! # #[derive(Default, Clone)] struct Position { x: f32, y: f32 }
//! # ecs! { World { position: Position => POSITION, } Resources { delta_time: f32 } }
//! # let mut world = World::default();
//! let current_tick = world.current_tick();
//!
//! // Process only changed entities
//! world.for_each_mut_changed(POSITION, current_tick - 1, |entity, table, idx| {
//!     // Only processes entities where position changed
//! });
//!
//! world.increment_tick();
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
//! # fn render_system(world: &mut World) {}
//! let mut world = World::default();
//! let mut schedule = Schedule::new();
//!
//! schedule
//!     .add_system(input_system)
//!     .add_system(physics_system)
//!     .add_system(render_system);
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

pub use paste;
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

/// Double-buffered event queue for inter-system communication.
///
/// Events persist for 2 frames to prevent systems from missing events
/// in parallel execution. Call [`update()`](EventQueue::update) between frames to swap buffers.
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

/// System scheduling for automatic execution ordering.
///
/// Allows organizing systems into stages that run sequentially,
/// with systems within a stage optionally running in parallel.
///
/// # Examples
///
/// ```
/// use freecs::Schedule;
///
/// fn physics_system(world: &mut World) {
///     // Physics logic
/// }
///
/// fn render_system(world: &mut World) {
///     // Rendering logic
/// }
///
/// let mut schedule = Schedule::new();
/// schedule.add_system(physics_system);
/// schedule.add_system(render_system);
///
/// loop {
///     schedule.run(&mut world);
///     world.step();
/// }
/// ```
type SystemFn<W> = Box<dyn FnMut(&mut W) + Send>;

pub struct Schedule<W> {
    systems: Vec<SystemFn<W>>,
}

impl<W> Schedule<W> {
    /// Creates a new empty schedule.
    pub fn new() -> Self {
        Self {
            systems: Vec::new(),
        }
    }

    /// Adds a system to the schedule.
    ///
    /// Systems are executed in the order they are added.
    /// Returns a mutable reference to the schedule for method chaining.
    ///
    /// # Example
    ///
    /// ```rust
    /// let mut schedule = Schedule::new();
    /// schedule
    ///     .add_system(physics_system)
    ///     .add_system(collision_system)
    ///     .add_system(render_system);
    /// ```
    pub fn add_system<F>(&mut self, system: F) -> &mut Self
    where
        F: FnMut(&mut W) + Send + 'static,
    {
        self.systems.push(Box::new(system));
        self
    }

    /// Runs all systems in order, passing the world to each.
    ///
    /// Systems execute sequentially in the order they were added.
    pub fn run(&mut self, world: &mut W) {
        for system in &mut self.systems {
            system(world);
        }
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
            $($name:ident: $type:ty => $mask:ident),* $(,)?
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
                $($name: $type => $mask),*
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
            $($name:ident: $type:ty => $mask:ident),* $(,)?
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
                $($name: $type => $mask),*
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
            $($name:ident: $type:ty => $mask:ident),* $(,)?
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
                $($name: $type => $mask),*
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
            $($name:ident: $type:ty => $mask:ident),* $(,)?
        }
        $resources:ident {
            $($(#[$attr:meta])*  $resource_name:ident: $resource_type:ty),* $(,)?
        }
    ) => {
        $crate::ecs_impl! {
            $world {
                $($name: $type => $mask),*
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
            $($name:ident: $type:ty => $mask:ident),* $(,)?
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
            $($name: Option<$type>,)*
        }

        #[allow(unused)]
        impl EntityBuilder {
            pub fn new() -> Self {
                Self::default()
            }

            $(
                $crate::paste::paste! {
                    pub fn [<with_$name>](&mut self, value: $type) -> &mut Self {
                        self.$name = Some(value);
                        self
                    }
                }
            )*

            pub fn spawn(&self, world: &mut $world, instances: usize) -> Vec<$crate::Entity> {
                let mut mask = 0;
                $(
                    if self.$name.is_some() {
                        mask |= $mask;
                    }
                )*
                let entities = world.spawn_entities(mask, instances);
                for entity in entities.iter() {
                    $(
                        $crate::paste::paste! {
                            if let Some(component) = self.$name.clone() {
                                world.[<set_$name>](*entity, component);
                            }
                        }
                    )*
                }
                entities
            }
        }

        #[repr(u64)]
        #[allow(clippy::upper_case_acronyms)]
        #[allow(non_camel_case_types)]
        pub enum Component {
            $($mask,)*
            $($tag_mask,)*
        }

        $(pub const $mask: u64 = 1 << (Component::$mask as u64);)*
        $(pub const $tag_mask: u64 = 1 << (Component::$tag_mask as u64);)*

        const ALL_TAGS_MASK: u64 = 0 $(| $tag_mask)*;

        pub const COMPONENT_COUNT: usize = {
            let mut count = 0;
            $(count += 1; let _ = Component::$mask;)*
            $(count += 1; let _ = Component::$tag_mask;)*
            count
        };

        #[derive(Default)]
        pub struct EntityAllocator {
            next_id: u32,
            free_ids: Vec<(u32, u32)>,
        }

        #[derive(Copy, Clone, Default)]
        struct EntityLocation {
            generation: u32,
            table_index: u32,
            array_index: u32,
            allocated: bool,
        }

        #[derive(Default)]
        pub struct EntityLocations {
            locations: Vec<EntityLocation>,
        }

        $crate::paste::paste! {
            pub enum Command {
                SpawnEntities { mask: u64, count: usize },
                DespawnEntities { entities: Vec<$crate::Entity> },
                AddComponents { entity: $crate::Entity, mask: u64 },
                RemoveComponents { entity: $crate::Entity, mask: u64 },
                $(
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
            entity_locations: EntityLocations,
            tables: Vec<ComponentArrays>,
            allocator: EntityAllocator,
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
                    entity_locations: EntityLocations::default(),
                    tables: Vec::default(),
                    allocator: EntityAllocator::default(),
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
                $crate::paste::paste! {
                    #[inline]
                    pub fn [<get_ $name>](&self, entity: $crate::Entity) -> Option<&$type> {
                        let (table_index, array_index) = get_location(&self.entity_locations, entity)?;

                        if !self.entity_locations.locations[entity.id as usize].allocated {
                            return None;
                        }

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

                    #[inline]
                    pub fn [<entity_has_ $name>](&self, entity: $crate::Entity) -> bool {
                        self.entity_has_components(entity, $mask)
                    }

                    #[inline]
                    pub fn [<set_ $name>](&mut self, entity: $crate::Entity, value: $type) {
                        if let Some((table_index, array_index)) = get_location(&self.entity_locations, entity) {
                            if self.entity_locations.locations[entity.id as usize].allocated {
                                let table = &mut self.tables[table_index];
                                if table.mask & $mask != 0 {
                                    table.$name[array_index] = value;
                                    return;
                                }
                            }
                        }

                        self.add_components(entity, $mask);
                        if let Some((table_index, array_index)) = get_location(&self.entity_locations, entity) {
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
                        let table_indices: Vec<usize> = self.tables
                            .iter()
                            .enumerate()
                            .filter(|(_, table)| table.mask & $mask != 0)
                            .map(|(idx, _)| idx)
                            .collect();

                        for table_index in table_indices {
                            for component in &mut self.tables[table_index].$name {
                                f(component);
                            }
                        }
                    }

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
                    entity_locations: &self.entity_locations,
                    mask,
                    table_index: 0,
                    array_index: 0,
                }
            }

            pub fn query_first_entity(&self, mask: u64) -> Option<$crate::Entity> {
                for table in &self.tables {
                    if table.mask & mask != mask {
                        continue;
                    }
                    for &entity in &table.entity_indices {
                        if self.entity_locations.locations[entity.id as usize].allocated {
                            return Some(entity);
                        }
                    }
                }
                None
            }

            pub fn despawn_entities(&mut self, entities: &[$crate::Entity]) -> Vec<$crate::Entity> {
                let mut despawned = Vec::with_capacity(entities.len());
                let mut tables_to_update = Vec::new();

                for &entity in entities {
                    let id = entity.id as usize;
                    if id < self.entity_locations.locations.len() {
                        let loc = &mut self.entity_locations.locations[id];
                        if loc.allocated && loc.generation == entity.generation {
                            let table_idx = loc.table_index as usize;
                            let array_idx = loc.array_index as usize;

                            loc.allocated = false;
                            loc.generation = loc.generation.wrapping_add(1);
                            self.allocator.free_ids.push((entity.id, loc.generation));

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
                        if let Some(loc) = self.entity_locations.locations.get_mut(moved_entity.id as usize) {
                            if loc.allocated {
                                loc.array_index = array_idx as u32;
                            }
                        }
                    }

                    $(
                        if table.mask & $mask != 0 {
                            table.$name.swap_remove(array_idx);
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

            pub fn add_components(&mut self, entity: $crate::Entity, mask: u64) -> bool {
                if let Some((table_index, array_index)) = get_location(&self.entity_locations, entity) {
                    let current_mask = self.tables[table_index].mask;
                    if current_mask & mask == mask {
                        return true;
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
                let mut result = Vec::new();
                for table in &self.tables {
                    result.extend(
                        table
                            .entity_indices
                            .iter()
                            .copied()
                            .filter(|&e| self.entity_locations.locations[e.id as usize].allocated),
                    );
                }
                result
            }

            pub fn entity_has_components(&self, entity: $crate::Entity, components: u64) -> bool {
                self.component_mask(entity).unwrap_or(0) & components != 0
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
                                if self.entity_locations.locations[entity.id as usize].allocated {
                                    f(entity, &mut table.$name[idx]);
                                }
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

                for table in &self.tables {
                    if table.mask & component_include == component_include && table.mask & component_exclude == 0 {
                        for (idx, &entity) in table.entity_indices.iter().enumerate() {
                            if self.entity_locations.locations[entity.id as usize].allocated
                                && self.entity_matches_tags(entity, tag_include, tag_exclude)
                            {
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

                let matching_entities: std::collections::HashSet<$crate::Entity> = table_indices
                    .iter()
                    .filter_map(|&idx| self.tables.get(idx))
                    .filter(|table| table.mask & component_exclude == 0)
                    .flat_map(|table| table.entity_indices.iter().copied())
                    .filter(|&entity| self.entity_locations.locations[entity.id as usize].allocated
                        && self.entity_matches_tags(entity, tag_include, tag_exclude))
                    .collect();

                for &table_index in &table_indices {
                    let table = &mut self.tables[table_index];
                    if table.mask & component_exclude != 0 {
                        continue;
                    }

                    for idx in 0..table.entity_indices.len() {
                        let entity = table.entity_indices[idx];
                        if matching_entities.contains(&entity) {
                            f(entity, table, idx);
                        }
                    }
                }
            }

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

                let table_indices: Vec<usize> = self.get_cached_tables(component_include).to_vec();

                if tag_include == 0 && tag_exclude == 0 {
                    self.tables
                        .par_iter_mut()
                        .enumerate()
                        .filter(|(idx, table)| table_indices.contains(idx) && table.mask & component_exclude == 0)
                        .for_each(|(_, table)| {
                            for idx in 0..table.entity_indices.len() {
                                let entity = table.entity_indices[idx];
                                f(entity, table, idx);
                            }
                        });
                } else {
                    let matching_entities: std::collections::HashSet<$crate::Entity> = table_indices
                        .iter()
                        .filter_map(|&idx| self.tables.get(idx))
                        .filter(|table| table.mask & component_exclude == 0)
                        .flat_map(|table| table.entity_indices.iter().copied())
                        .filter(|&entity| self.entity_locations.locations[entity.id as usize].allocated
                            && self.entity_matches_tags(entity, tag_include, tag_exclude))
                        .collect();

                    self.tables
                        .par_iter_mut()
                        .enumerate()
                        .filter(|(idx, table)| table_indices.contains(idx) && table.mask & component_exclude == 0)
                        .for_each(|(_, table)| {
                            for idx in 0..table.entity_indices.len() {
                                let entity = table.entity_indices[idx];
                                if matching_entities.contains(&entity) {
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

                let matching_entities: std::collections::HashSet<$crate::Entity> = table_indices
                    .iter()
                    .filter_map(|&idx| self.tables.get(idx))
                    .filter(|table| table.mask & component_exclude == 0)
                    .flat_map(|table| table.entity_indices.iter().copied())
                    .filter(|&entity| self.entity_locations.locations[entity.id as usize].allocated
                        && self.entity_matches_tags(entity, tag_include, tag_exclude))
                    .collect();

                for &table_index in &table_indices {
                    let table = &mut self.tables[table_index];
                    if table.mask & component_exclude != 0 {
                        continue;
                    }

                    for idx in 0..table.entity_indices.len() {
                        let entity = table.entity_indices[idx];
                        if !matching_entities.contains(&entity) {
                            continue;
                        }

                        let mut changed = false;
                        $(
                            $crate::paste::paste! {
                                if table.mask & $mask != 0 && table.[<$name _changed>][idx] > since_tick {
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


        #[derive(Default)]
        pub struct $resources {
            $($(#[$attr])* pub $resource_name: $resource_type,)*
        }

        $crate::paste::paste! {
            #[derive(Default)]
            pub struct ComponentArrays {
                $(pub $name: Vec<$type>,)*
                $(pub [<$name _changed>]: Vec<u32>,)*
                pub entity_indices: Vec<$crate::Entity>,
                pub mask: u64,
            }
        }

        pub struct Read<T>(std::marker::PhantomData<T>);
        pub struct Write<T>(std::marker::PhantomData<T>);

        pub trait SystemParam {
            const READ_MASK: u64;
            const WRITE_MASK: u64;
        }

        pub trait SystemConflict<Other: SystemParam> {
            const CONFLICTS: bool;
        }

        $(
            $crate::paste::paste! {
                pub struct [<$mask:camel Query>];

                impl SystemParam for Read<[<$mask:camel Query>]> {
                    const READ_MASK: u64 = $mask;
                    const WRITE_MASK: u64 = 0;
                }

                impl SystemParam for Write<[<$mask:camel Query>]> {
                    const READ_MASK: u64 = 0;
                    const WRITE_MASK: u64 = $mask;
                }

                impl<T: SystemParam> SystemConflict<T> for Read<[<$mask:camel Query>]> {
                    const CONFLICTS: bool = T::WRITE_MASK & $mask != 0;
                }

                impl<T: SystemParam> SystemConflict<T> for Write<[<$mask:camel Query>]> {
                    const CONFLICTS: bool = (T::READ_MASK | T::WRITE_MASK) & $mask != 0;
                }
            }
        )*

        impl SystemParam for () {
            const READ_MASK: u64 = 0;
            const WRITE_MASK: u64 = 0;
        }

        impl<T: SystemParam> SystemConflict<T> for () {
            const CONFLICTS: bool = false;
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

            pub fn check_query_conflicts<T1, T2>() -> bool
            where
                T1: SystemParam + SystemConflict<T2>,
                T2: SystemParam + SystemConflict<T1>,
            {
                <T1 as SystemConflict<T2>>::CONFLICTS || <T2 as SystemConflict<T1>>::CONFLICTS
            }

            pub fn verify_parallel_safety<T1, T2>()
            where
                T1: SystemParam + SystemConflict<T2>,
                T2: SystemParam + SystemConflict<T1>,
            {
                if Self::check_query_conflicts::<T1, T2>() {
                    panic!("Parallel safety violation: systems access overlapping mutable components");
                }
            }

            $(
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
            entity_locations: &'a EntityLocations,
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

                    let id = entity.id as usize;
                    if id < self.entity_locations.locations.len()
                        && self.entity_locations.locations[id].allocated
                    {
                        return Some(entity);
                    }
                }
            }
        }

        $(
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
                $($mask => Some(Component::$mask as _),)*
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
                        if from_table_ref.mask & $mask != 0 {
                            Some(std::mem::take(&mut from_table_ref.$name[from_index]))
                        } else {
                            None
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

        fn get_location(locations: &EntityLocations, entity: $crate::Entity) -> Option<(usize, usize)> {
            let id = entity.id as usize;
            if id >= locations.locations.len() {
                return None;
            }

            let location = &locations.locations[id];
            if !location.allocated || location.generation != entity.generation {
                return None;
            }

            Some((location.table_index as usize, location.array_index as usize))
        }

        fn insert_location(
            locations: &mut EntityLocations,
            entity: $crate::Entity,
            location: (usize, usize),
        ) {
            let id = entity.id as usize;
            if id >= locations.locations.len() {
                locations
                    .locations
                    .resize(id + 1, EntityLocation::default());
            }

            locations.locations[id] = EntityLocation {
                generation: entity.generation,
                table_index: location.0 as u32,
                array_index: location.1 as u32,
                allocated: true,
            };
        }

        fn create_entity(world: &mut $world) -> $crate::Entity {
            if let Some((id, next_gen)) = world.allocator.free_ids.pop() {
                let id_usize = id as usize;
                if id_usize >= world.entity_locations.locations.len() {
                    world.entity_locations.locations.resize(
                        (world.entity_locations.locations.len() * 2).max(64),
                        EntityLocation::default(),
                    );
                }
                world.entity_locations.locations[id_usize].generation = next_gen;
                $crate::Entity {
                    id,
                    generation: next_gen,
                }
            } else {
                let id = world.allocator.next_id;
                world.allocator.next_id += 1;
                let id_usize = id as usize;
                if id_usize >= world.entity_locations.locations.len() {
                    world.entity_locations.locations.resize(
                        (world.entity_locations.locations.len() * 2).max(64),
                        EntityLocation::default(),
                    );
                }
                $crate::Entity { id, generation: 0 }
            }
        }

        fn add_to_table(
            arrays: &mut ComponentArrays,
            entity: $crate::Entity,
            components: ( $(Option<$type>,)* ),
            tick: u32,
        ) {
            let ($($name,)*) = components;
            $(
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

            for comp_mask in [$($mask,)*] {
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

        world.increment_tick();

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

        world.increment_tick();

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

        world.increment_tick();
        assert_eq!(world.current_tick(), 1);
        assert_eq!(world.last_tick(), 0);

        world.increment_tick();
        assert_eq!(world.current_tick(), 2);
        assert_eq!(world.last_tick(), 1);

        world.increment_tick();
        world.get_position_mut(entity).unwrap().x = 10.0;

        let mut count = 0;
        world.for_each_mut_changed(POSITION, 0, |_entity, _table, _idx| {
            count += 1;
        });
        assert_eq!(count, 1);

        world.increment_tick();
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

        world.increment_tick();

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

        world.increment_tick();

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
    fn test_parallel_safety_no_conflict_read_read() {
        assert!(!World::check_query_conflicts::<
            Read<PositionQuery>,
            Read<PositionQuery>,
        >());
        assert!(!World::check_query_conflicts::<
            Read<PositionQuery>,
            Read<VelocityQuery>,
        >());
    }

    #[test]
    fn test_parallel_safety_conflict_write_write() {
        assert!(World::check_query_conflicts::<
            Write<PositionQuery>,
            Write<PositionQuery>,
        >());
    }

    #[test]
    fn test_parallel_safety_conflict_read_write() {
        assert!(World::check_query_conflicts::<
            Read<PositionQuery>,
            Write<PositionQuery>,
        >());
        assert!(World::check_query_conflicts::<
            Write<PositionQuery>,
            Read<PositionQuery>,
        >());
    }

    #[test]
    fn test_parallel_safety_no_conflict_different_components() {
        assert!(!World::check_query_conflicts::<
            Write<PositionQuery>,
            Write<VelocityQuery>,
        >());
        assert!(!World::check_query_conflicts::<
            Write<PositionQuery>,
            Read<VelocityQuery>,
        >());
        assert!(!World::check_query_conflicts::<
            Read<PositionQuery>,
            Write<VelocityQuery>,
        >());
    }

    #[test]
    fn test_parallel_safety_verify_safe() {
        World::verify_parallel_safety::<Read<PositionQuery>, Read<VelocityQuery>>();
        World::verify_parallel_safety::<Write<PositionQuery>, Read<VelocityQuery>>();
        World::verify_parallel_safety::<Write<PositionQuery>, Write<VelocityQuery>>();
    }

    #[test]
    #[should_panic(expected = "Parallel safety violation")]
    fn test_parallel_safety_verify_conflict_write_write() {
        World::verify_parallel_safety::<Write<PositionQuery>, Write<PositionQuery>>();
    }

    #[test]
    #[should_panic(expected = "Parallel safety violation")]
    fn test_parallel_safety_verify_conflict_read_write() {
        World::verify_parallel_safety::<Read<PositionQuery>, Write<PositionQuery>>();
    }

    #[test]
    fn test_parallel_safety_component_access() {
        assert_eq!(Read::<PositionQuery>::READ_MASK, POSITION);
        assert_eq!(Read::<PositionQuery>::WRITE_MASK, 0);
        assert_eq!(Write::<PositionQuery>::READ_MASK, 0);
        assert_eq!(Write::<PositionQuery>::WRITE_MASK, POSITION);
        assert_eq!(Read::<VelocityQuery>::READ_MASK, VELOCITY);
        assert_eq!(Write::<VelocityQuery>::WRITE_MASK, VELOCITY);
    }

    #[test]
    fn test_parallel_safety_multiple_reads() {
        assert!(!World::check_query_conflicts::<
            Read<PositionQuery>,
            Read<PositionQuery>,
        >());
        assert!(!World::check_query_conflicts::<
            Read<VelocityQuery>,
            Read<HealthQuery>,
        >());
    }

    #[test]
    fn test_parallel_safety_mixed_access() {
        assert!(World::check_query_conflicts::<
            Write<PositionQuery>,
            Read<PositionQuery>,
        >());
        assert!(!World::check_query_conflicts::<
            Write<PositionQuery>,
            Read<HealthQuery>,
        >());
        assert!(!World::check_query_conflicts::<
            Write<VelocityQuery>,
            Write<PositionQuery>,
        >());
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
        schedule.add_system(|world: &mut World| {
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

        schedule.add_system(|world: &mut World| {
            let dt = world.resources._delta_time;
            let updates: Vec<(Entity, Velocity)> = world
                .query_entities(POSITION | VELOCITY)
                .into_iter()
                .filter_map(|entity| world.get_velocity(entity).map(|vel| (entity, vel.clone())))
                .collect();

            for (entity, vel) in updates {
                if let Some(pos) = world.get_position_mut(entity) {
                    pos.x += vel.x * dt;
                    pos.y += vel.y * dt;
                }
            }
        });

        schedule.add_system(|world: &mut World| {
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
            .add_system(|world: &mut World| {
                world.resources._delta_time += 1.0;
            })
            .add_system(|world: &mut World| {
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

        schedule.add_system(|world: &mut World| {
            let entities: Vec<Entity> = world.query_entities(POSITION).into_iter().collect();
            if let Some(pos) = world.get_position_mut(entities[0]) {
                pos.x = 10.0;
            }
        });

        schedule.add_system(|world: &mut World| {
            let entities: Vec<Entity> = world.query_entities(POSITION).into_iter().collect();
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

        schedule.add_system(|world: &mut World| {
            let entities: Vec<Entity> = world.query_entities(HEALTH).into_iter().collect();
            for entity in entities {
                if let Some(health) = world.get_health_mut(entity) {
                    health.value -= 10.0;
                }
            }
        });

        schedule.add_system(|world: &mut World| {
            let entities: Vec<Entity> = world.query_entities(HEALTH).into_iter().collect();
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
            world.entity_locations.locations[entity.id as usize].table_index as usize;

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
            world.entity_locations.locations[entity2.id as usize].table_index as usize;

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
            world.entity_locations.locations[entity.id as usize].table_index as usize;

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
            world.entity_locations.locations[entity2.id as usize].table_index as usize;

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
}
