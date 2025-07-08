//! freecs is an abstraction-free ECS library for Rust, designed for high performance and simplicity.
//!
//! It provides an archetypal table-based storage system for components, allowing for fast queries,
//! fast system iteration, and parallel processing. Entities with the same components are stored together
//! in contiguous memory, optimizing for cache coherency and SIMD operations.
//!
//! A macro is used to define the world and its components, generating the entire entity component system
//! at compile time. The generated code contains only plain data structures and free functions that
//! transform them.
//!
//! The core implementation is ~500 loc, is fully statically dispatched and
//! does not use object orientation, generics, or traits.
//!
//! # Creating a World
//!
//! ```rust
//! use freecs::{ecs, table_has_components, EntityId};
//!
//! // First, define components.
//! // They must implement: `Default`
//!
//! #[derive(Default, Clone, Debug)]
//! struct Position { x: f32, y: f32 }
//!
//! #[derive(Default, Clone, Debug)]
//! struct Velocity { x: f32, y: f32 }
//!
//! // Then, create a world with the `ecs!` macro.
//! // Resources are stored independently of component data.
//! // The `World` and `Resources` type names can be customized.
//! ecs! {
//!   World {
//!     position: Position => POSITION,
//!     velocity: Velocity => VELOCITY,
//!     health: Health => HEALTH,
//!   }
//!   Resources {
//!     delta_time: f32
//!   }
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
//! // Lookup and modify a component
//! if let Some(pos) = world.get_component_mut::<Position>(entity, POSITION) {
//!     pos.x += 1.0;
//! }
//!
//! // Add new components to an entity by mask
//! world.add_components(entity, HEALTH | VELOCITY);
//!
//! // Remove components from an entity by mask
//! world.remove_components(entity, VELOCITY | POSITION);
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
//! ## Systems
//!
//! A system is any function that takes a *mutable* reference to a world,
//! querying the world for entities to process and operating on their components.
//!
//! ```rust
//! fn example_system(world: &mut World) {
//!   for entity in world.query_entities(POSITION | VELOCITY) {
//!       if let Some(position) = world.get_component_mut::<Position>(entity, POSITION) {
//!           position.x += 1.0;
//!       }
//!   }
//! }
//! ```
//!
//! ## Parallel Processing
//!
//! Systems are plain functions that iterate over
//! the component tables and transform component data.
//!
//! Parallelization of systems can be done with [rayon](https://docs.rs/rayon/latest/rayon/).
//! which lets you replace `.iter_mut()` with `.par_iter_mut()` when iterating over tables.
//!
//! > In practice, you should use `.iter_mut()` instead of `.par_iter_mut()`
//! > because sequential access is typically more performant until
//! > you are working with extreme numbers of entities.
//!
//! ```rust
//! pub fn run_systems(world: &mut World, dt: f32) {
//!     use rayon::prelude::*;
//!
//!     world.tables.par_iter_mut().for_each(|table| {
//!         if table_has_components!(table, POSITION | VELOCITY | HEALTH) {
//!             update_positions_system(&mut table.position, &table.velocity, dt);
//!         }
//!         if table_has_components!(table, HEALTH) {
//!             health_system(&mut table.health);
//!         }
//!     });
//! }
//!
//! // The system itself can also access components in parallel and be inlined for performance.
//! #[inline]
//! pub fn update_positions_system(positions: &mut [Position], velocities: &[Velocity], dt: f32) {
//!     positions
//!         .par_iter_mut()
//!         .zip(velocities.par_iter())
//!         .for_each(|(pos, vel)| {
//!             pos.x += vel.x * dt;
//!             pos.y += vel.y * dt;
//!         });
//! }
//!
//! #[inline]
//! pub fn health_system(health: &mut [Health]) {
//!     health.par_iter_mut().for_each(|health| {
//!         health.value *= 0.98;
//!     });
//! }
//! ```
//!
//! ## Change Detection
//!
//! freecs provides an opt-in change detection system that allows you to track when components are modified.
//! This is useful for systems that only need to process entities when their data has changed.
//!
//! ### Basic Usage
//!
//! ```rust
//! // Get mutable access and modify a component
//! if let Some(pos) = world.get_component_mut::<Position>(entity, POSITION) {
//!     pos.x += velocity.x * dt;
//!     pos.y += velocity.y * dt;
//! }
//!
//! // Explicitly mark the component as changed
//! world.mark_changed(entity, POSITION);
//!
//! // Later, process change events
//! while let Some(event) = world.try_next_event() {
//!     match event {
//!         Event::ComponentChanged { kind, entity } => {
//!             println!("Component {:b} changed for entity {:?}", kind, entity);
//!         }
//!     }
//! }
//!
//! // You can also clear the event queue
//! world.clear_events();
//! ```
//!
//! You can mark multiple components as changed in a single call:
//!
//! ```rust
//! // Mark both position and velocity as changed
//! world.mark_changed(entity, POSITION | VELOCITY);
//! ```
//!
//! The event queue is stored in the world's `Resources` struct and is automatically available
//! when you create a world with the `ecs!` macro.
//!

pub use paste;

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    ComponentChanged { kind: u64, entity: EntityId },
}

#[derive(Default, Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct EntityId {
    pub id: u32,
    pub generation: u32,
}

impl std::fmt::Display for EntityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self { id, generation } = self;
        write!(f, "Id: {id} - Generation: {generation}")
    }
}

#[macro_export]
macro_rules! ecs {
    (
        $world:ident {
            $($name:ident: $type:ty => $mask:ident),* $(,)?
        }
        $resources:ident {
            $($(#[$attr:meta])*  $resource_name:ident: $resource_type:ty),* $(,)?
        }
    ) => {
        #[repr(u64)]
        #[allow(clippy::upper_case_acronyms)]
        #[allow(non_camel_case_types)]
        pub enum Component {
            $($mask,)*
        }

        $(pub const $mask: u64 = 1 << (Component::$mask as u64);)*

        pub const COMPONENT_COUNT: usize = {
            let mut count = 0;
            $(count += 1; let _ = Component::$mask;)*
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

        #[derive(Default)]
        #[allow(unused)]
        pub struct $world {
            pub entity_locations: EntityLocations,
            pub tables: Vec<ComponentArrays>,
            pub allocator: EntityAllocator,
            pub resources: $resources,
            table_edges: Vec<TableEdges>,
            table_lookup: std::collections::HashMap<u64, usize>,
        }

        #[allow(unused)]
        impl $world {
            pub fn try_next_event(&mut self) -> Option<$crate::Event> {
                self.resources.events.pop_front()
            }

            pub fn clear_events(&mut self) {
                self.resources.events.clear();
            }

            pub fn mark_changed(&mut self, entity: $crate::EntityId, mask: u64) {
                self.resources.events.push_back($crate::Event::ComponentChanged { kind: mask, entity });
            }

            $(
                $crate::paste::paste! {
                    #[inline]
                    pub fn [<get_ $name>](&self, entity: $crate::EntityId) -> Option<&$type> {
                        self.get_component::<$type>(entity, $mask)
                    }

                    #[inline]
                    pub fn [<get_ $name _mut>](&mut self, entity: $crate::EntityId) -> Option<&mut $type> {
                        self.get_component_mut::<$type>(entity, $mask)
                    }

                    #[inline]
                    pub fn [<entity_has_ $name>](&self, entity: $crate::EntityId) -> bool {
                        self.entity_has_components(entity, $mask)
                    }

                    #[inline]
                    pub fn [<set_ $name>](&mut self, entity: $crate::EntityId, value: $type) {
                        if let Some(component) = self.get_component_mut(entity, $mask) {
                            *component = value;
                        } else {
                            self.add_components(entity, $mask);
                            *self.get_component_mut(entity, $mask).unwrap() = value;
                        }
                    }

                    #[inline]
                    pub fn [<add_ $name>](&mut self, entity: $crate::EntityId) {
                        self.add_components(entity, $mask);
                    }

                    #[inline]
                    pub fn [<remove_ $name>](&mut self, entity: $crate::EntityId) -> bool {
                        self.remove_components(entity, $mask)
                    }
                }
            )*

            pub fn get_component<T: 'static>(&self, entity: $crate::EntityId, mask: u64) -> Option<&T> {
                let (table_index, array_index) = get_location(&self.entity_locations, entity)?;

                if !self.entity_locations.locations[entity.id as usize].allocated {
                    return None;
                }

                let table = &self.tables[table_index];

                if table.mask & mask == 0 {
                    return None;
                }

                $(
                    if mask == $mask && std::any::TypeId::of::<T>() == std::any::TypeId::of::<$type>() {
                        return Some(unsafe { &*(&table.$name[array_index] as *const $type as *const T) });
                    }
                )*

                None
            }

            pub fn get_component_mut<T: 'static>(&mut self, entity: $crate::EntityId, mask: u64) -> Option<&mut T> {
                let (table_index, array_index) = get_location(&self.entity_locations, entity)?;
                let table = &mut self.tables[table_index];
                if table.mask & mask == 0 {
                    return None;
                }

                $(
                    if mask == $mask && std::any::TypeId::of::<T>() == std::any::TypeId::of::<$type>() {
                        return Some(unsafe { &mut *(&mut table.$name[array_index] as *mut $type as *mut T) });
                    }
                )*

                None
            }

            pub fn spawn_entities(&mut self, mask: u64, count: usize) -> Vec<$crate::EntityId> {
                let mut entities = Vec::with_capacity(count);
                let table_index = get_or_create_table(self, mask);

                self.tables[table_index].entity_indices.reserve(count);

                $(
                    if mask & $mask != 0 {
                        self.tables[table_index].$name.reserve(count);
                    }
                )*

                for _ in 0..count {
                    let entity = create_entity(self);
                    add_to_table(
                        &mut self.tables[table_index],
                        entity,
                        (
                            $(
                            if mask & $mask != 0 {
                                Some(<$type>::default())
                            } else {
                                None
                            },
                            )*
                        ),
                    );
                    entities.push(entity);
                    insert_location(
                        &mut self.entity_locations,
                        entity,
                        (table_index, self.tables[table_index].entity_indices.len() - 1),
                    );
                }

                entities
            }

            pub fn query_entities(&self, mask: u64) -> Vec<$crate::EntityId> {
                let total_capacity = self
                    .tables
                    .iter()
                    .filter(|table| table.mask & mask == mask)
                    .map(|table| table.entity_indices.len())
                    .sum();

                let mut result = Vec::with_capacity(total_capacity);
                for table in &self.tables {
                    if table.mask & mask == mask {
                        result.extend(
                            table
                                .entity_indices
                                .iter()
                                .copied()
                                .filter(|&e| self.entity_locations.locations[e.id as usize].allocated),
                        );
                    }
                }
                result
            }

            pub fn query_first_entity(&self, mask: u64) -> Option<$crate::EntityId> {
                for table in &self.tables {
                    if !$crate::table_has_components!(table, mask) {
                        continue;
                    }
                    let indices = table
                        .entity_indices
                        .iter()
                        .copied()
                        .filter(|&e| self.entity_locations.locations[e.id as usize].allocated)
                        .collect::<Vec<_>>();
                    if let Some(entity) = indices.first() {
                        return Some(*entity);
                    }
                }
                None
            }

            pub fn despawn_entities(&mut self, entities: &[$crate::EntityId]) -> Vec<$crate::EntityId> {
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

                for (table_idx, array_idx) in tables_to_update.into_iter().rev() {
                    if table_idx >= self.tables.len() {
                        continue;
                    }

                    let table = &mut self.tables[table_idx];
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

                despawned
            }

            pub fn add_components(&mut self, entity: $crate::EntityId, mask: u64) -> bool {
                if let Some((table_index, array_index)) = get_location(&self.entity_locations, entity) {
                    let current_mask = self.tables[table_index].mask;
                    if current_mask & mask == mask {
                        return true;
                    }

                    let target_table = if mask.count_ones() == 1 {
                        get_component_index(mask).and_then(|idx| self.table_edges[table_index].add_edges[idx])
                    } else {
                        None
                    };

                    let new_table_index =
                        target_table.unwrap_or_else(|| get_or_create_table(self, current_mask | mask));

                    move_entity(self, entity, table_index, array_index, new_table_index);
                    true
                } else {
                    false
                }
            }

            pub fn remove_components(&mut self, entity: $crate::EntityId, mask: u64) -> bool {
                if let Some((table_index, array_index)) = get_location(&self.entity_locations, entity) {
                    let current_mask = self.tables[table_index].mask;
                    if current_mask & mask == 0 {
                        return true;
                    }

                    let target_table = if mask.count_ones() == 1 {
                        get_component_index(mask)
                            .and_then(|idx| self.table_edges[table_index].remove_edges[idx])
                    } else {
                        None
                    };

                    let new_table_index =
                        target_table.unwrap_or_else(|| get_or_create_table(self, current_mask & !mask));

                    move_entity(self, entity, table_index, array_index, new_table_index);
                    true
                } else {
                    false
                }
            }

            pub fn component_mask(&self, entity: $crate::EntityId) -> Option<u64> {
                get_location(&self.entity_locations, entity)
                    .map(|(table_index, _)| self.tables[table_index].mask)
            }

            pub fn get_all_entities(&self) -> Vec<$crate::EntityId> {
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

            pub fn entity_has_components(&self, entity: $crate::EntityId, components: u64) -> bool {
                self.component_mask(entity).unwrap_or(0) & components != 0
            }
        }

        #[derive(Default)]
        pub struct $resources {
            pub events: std::collections::VecDeque<$crate::Event>,
            $($(#[$attr])* pub $resource_name: $resource_type,)*
        }

        #[derive(Default)]
        pub struct ComponentArrays {
            $(pub $name: Vec<$type>,)*
            pub entity_indices: Vec<$crate::EntityId>,
            pub mask: u64,
        }

        #[derive(Copy, Clone)]
        struct TableEdges {
            add_edges: [Option<usize>; COMPONENT_COUNT],
            remove_edges: [Option<usize>; COMPONENT_COUNT],
        }

        impl Default for TableEdges {
            fn default() -> Self {
                Self {
                    add_edges: [None; COMPONENT_COUNT],
                    remove_edges: [None; COMPONENT_COUNT],
                }
            }
        }

        fn get_component_index(mask: u64) -> Option<usize> {
            match mask {
                $($mask => Some(Component::$mask as _),)*
                _ => None,
            }
        }

        fn remove_from_table(arrays: &mut ComponentArrays, index: usize) -> Option<$crate::EntityId> {
            let last_index = arrays.entity_indices.len() - 1;
            let mut swapped_entity = None;

            if index < last_index {
                swapped_entity = Some(arrays.entity_indices[last_index]);
            }

            $(
                if arrays.mask & $mask != 0 {
                    arrays.$name.swap_remove(index);
                }
            )*
            arrays.entity_indices.swap_remove(index);

            swapped_entity
        }

        fn move_entity(
            world: &mut $world,
            entity: $crate::EntityId,
            from_table: usize,
            from_index: usize,
            to_table: usize,
        ) {
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

            add_to_table(&mut world.tables[to_table], entity, components);
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

        fn get_location(locations: &EntityLocations, entity: $crate::EntityId) -> Option<(usize, usize)> {
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
            entity: $crate::EntityId,
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

        fn create_entity(world: &mut $world) -> $crate::EntityId {
            if let Some((id, next_gen)) = world.allocator.free_ids.pop() {
                let id_usize = id as usize;
                if id_usize >= world.entity_locations.locations.len() {
                    world.entity_locations.locations.resize(
                        (world.entity_locations.locations.len() * 2).max(64),
                        EntityLocation::default(),
                    );
                }
                world.entity_locations.locations[id_usize].generation = next_gen;
                $crate::EntityId {
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
                $crate::EntityId { id, generation: 0 }
            }
        }

        fn add_to_table(
            arrays: &mut ComponentArrays,
            entity: $crate::EntityId,
            components: ( $(Option<$type>,)* ),
        ) {
            let ($($name,)*) = components;
            $(
                if arrays.mask & $mask != 0 {
                    if let Some(component) = $name {
                        arrays.$name.push(component);
                    } else {
                        arrays.$name.push(<$type>::default());
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
        Resources {
            _delta_time: f32,
        }
    }

    use components::*;
    mod components {
        use super::*;

        #[derive(Default, Debug, Copy, Clone, PartialEq)]
        pub struct Parent(pub EntityId);

        #[derive(Default, Debug, Clone, PartialEq)]
        pub struct Node {
            pub id: EntityId,
            pub parent: Option<EntityId>,
            pub children: Vec<EntityId>,
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

    fn setup_test_world() -> (World, EntityId) {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION | VELOCITY, 1)[0];

        if let Some(pos) = world.get_component_mut::<Position>(entity, POSITION) {
            pos.x = 1.0;
            pos.y = 2.0;
        }
        if let Some(vel) = world.get_component_mut::<Velocity>(entity, VELOCITY) {
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
            assert!(world.get_component::<Position>(entity, POSITION).is_some());
            assert!(world.get_component::<Velocity>(entity, VELOCITY).is_some());
            assert!(world.get_component::<Health>(entity, HEALTH).is_none());
        }
    }

    #[test]
    fn test_component_access() {
        let (mut world, entity) = setup_test_world();

        let pos = world.get_component::<Position>(entity, POSITION).unwrap();
        assert_eq!(pos.x, 1.0);
        assert_eq!(pos.y, 2.0);

        if let Some(pos) = world.get_component_mut::<Position>(entity, POSITION) {
            pos.x = 5.0;
        }

        let pos = world.get_component::<Position>(entity, POSITION).unwrap();
        assert_eq!(pos.x, 5.0);
    }

    #[test]
    fn test_add_remove_components() {
        let (mut world, entity) = setup_test_world();

        assert!(world.get_component::<Health>(entity, HEALTH).is_none());

        world.add_components(entity, HEALTH);
        assert!(world.get_component::<Health>(entity, HEALTH).is_some());

        world.remove_components(entity, HEALTH);
        assert!(world.get_component::<Health>(entity, HEALTH).is_none());
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

        let pos_vel = world.query_entities(POSITION | VELOCITY);
        let pos_health = world.query_entities(POSITION | HEALTH);
        let all = world.query_entities(POSITION | VELOCITY | HEALTH);

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

        assert!(
            world
                .get_component::<Position>(entities[1], POSITION)
                .is_none()
        );

        assert!(
            world
                .get_component::<Position>(entities[0], POSITION)
                .is_some()
        );
        assert!(
            world
                .get_component::<Position>(entities[2], POSITION)
                .is_some()
        );
    }

    #[test]
    fn test_parallel_systems() {
        let mut world = World::default();

        let entity = world.spawn_entities(POSITION | VELOCITY | HEALTH, 1)[0];

        if let Some(pos) = world.get_component_mut::<Position>(entity, POSITION) {
            pos.x = 0.0;
            pos.y = 0.0;
        }
        if let Some(vel) = world.get_component_mut::<Velocity>(entity, VELOCITY) {
            vel.x = 1.0;
            vel.y = 1.0;
        }
        if let Some(health) = world.get_component_mut::<Health>(entity, HEALTH) {
            health.value = 100.0;
        }

        systems::run_systems(&mut world, 1.0);

        let pos = world.get_component::<Position>(entity, POSITION).unwrap();
        let health = world.get_component::<Health>(entity, HEALTH).unwrap();

        assert_eq!(pos.x, 1.0);
        assert_eq!(pos.y, 1.0);
        assert!(health.value < 100.0);
    }

    #[test]
    fn test_add_components() {
        let (mut world, entity) = setup_test_world();

        assert!(world.get_component::<Health>(entity, HEALTH).is_none());

        world.add_components(entity, HEALTH);
        assert!(world.get_component::<Health>(entity, HEALTH).is_some());

        world.remove_components(entity, HEALTH);
        assert!(world.get_component::<Health>(entity, HEALTH).is_none());
    }

    #[test]
    fn test_multiple_component_addition() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION, 1)[0];

        world.add_components(entity, VELOCITY | HEALTH);

        assert!(world.get_component::<Position>(entity, POSITION).is_some());
        assert!(world.get_component::<Velocity>(entity, VELOCITY).is_some());
        assert!(world.get_component::<Health>(entity, HEALTH).is_some());

        if let Some(pos) = world.get_component_mut::<Position>(entity, POSITION) {
            pos.x = 1.0;
        }
        world.add_components(entity, VELOCITY);
        assert_eq!(
            world.get_component::<Position>(entity, POSITION).unwrap().x,
            1.0
        );
    }

    #[test]
    fn test_component_chain_addition() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION, 1)[0];

        if let Some(pos) = world.get_component_mut::<Position>(entity, POSITION) {
            pos.x = 1.0;
        }

        world.add_components(entity, VELOCITY);
        world.add_components(entity, HEALTH);

        assert_eq!(
            world.get_component::<Position>(entity, POSITION).unwrap().x,
            1.0
        );
    }

    #[test]
    fn test_component_removal_order() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION | VELOCITY | HEALTH, 1)[0];

        world.remove_components(entity, VELOCITY);
        world.remove_components(entity, HEALTH);
        assert!(world.get_component::<Position>(entity, POSITION).is_some());
        assert!(world.get_component::<Velocity>(entity, VELOCITY).is_none());
        assert!(world.get_component::<Health>(entity, HEALTH).is_none());
    }

    #[test]
    fn test_edge_cases() {
        let mut world = World::default();

        let empty = world.spawn_entities(0, 1)[0];

        world.add_components(empty, POSITION);
        assert!(world.get_component::<Position>(empty, POSITION).is_some());

        world.add_components(empty, POSITION);
        world.add_components(empty, POSITION);

        world.remove_components(empty, VELOCITY);

        world.remove_components(empty, POSITION);
        assert_eq!(world.component_mask(empty).unwrap(), 0);

        let invalid = EntityId {
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
            let pos = world
                .get_component_mut::<Position>(entity, POSITION)
                .unwrap();
            pos.x = 1.0;
            pos.y = 2.0;
            let vel = world
                .get_component_mut::<Velocity>(entity, VELOCITY)
                .unwrap();
            vel.x = 3.0;
            vel.y = 4.0;
        }

        world.add_components(entity, HEALTH);
        world.remove_components(entity, HEALTH);
        world.add_components(entity, HEALTH);

        let pos = world.get_component::<Position>(entity, POSITION).unwrap();
        let vel = world.get_component::<Velocity>(entity, VELOCITY).unwrap();
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
        if let Some(vel) = world.get_component_mut::<Velocity>(entity1, VELOCITY) {
            vel.x = entity2.id as f32;
        }

        world.add_components(entity2, VELOCITY | HEALTH);

        let stored_id = world
            .get_component::<Velocity>(entity1, VELOCITY)
            .unwrap()
            .x as u32;
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

        assert!(world.get_component::<Position>(e2, POSITION).is_none());
        assert!(world.get_component::<Velocity>(e2, VELOCITY).is_none());

        assert!(world.get_component::<Position>(e1, POSITION).is_some());

        let remaining = world.query_entities(POSITION);
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

        if let Some(pos) = world.get_component_mut::<Position>(entity1, POSITION) {
            pos.x = 1.0;
        }
        if let Some(health) = world.get_component_mut::<Health>(entity1, HEALTH) {
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

        if let Some(pos) = world.get_component_mut::<Position>(entity3, POSITION) {
            pos.x = 3.0;
        }
        if let Some(health) = world.get_component_mut::<Health>(entity3, HEALTH) {
            health.value = 50.0;
        }

        if let Some(pos) = world.get_component::<Position>(entity2, POSITION) {
            assert_eq!(pos.x, 0.0, "Entity2's data should be unchanged");
        }

        if let Some(pos) = world.get_component::<Position>(entity3, POSITION) {
            assert_eq!(pos.x, 3.0, "Should get entity3's data, not entity1's");
        }
        assert!(
            world.get_component::<Position>(entity1, POSITION).is_none(),
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

        if let Some(pos) = world.get_component_mut::<Position>(entity_a1, POSITION) {
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

        if let Some(pos) = world.get_component_mut::<Position>(entity_a2, POSITION) {
            pos.x = 2.0;
            pos.y = 2.0;
        }

        assert!(
            world
                .get_component::<Position>(entity_a1, POSITION)
                .is_none(),
            "Old reference to entity should be invalid"
        );

        world.despawn_entities(&[entity_a2]);

        let entity_a3 = world.spawn_entities(POSITION, 1)[0];
        assert_eq!(entity_a3.id, id, "Should reuse the same ID again");
        assert_eq!(
            entity_a3.generation, 2,
            "Third use of ID should have generation 2"
        );

        if let Some(pos) = world.get_component_mut::<Position>(entity_a3, POSITION) {
            pos.x = 3.0;
            pos.y = 3.0;
        }

        assert!(
            world
                .get_component::<Position>(entity_a1, POSITION)
                .is_none(),
            "First generation reference should be invalid"
        );
        assert!(
            world
                .get_component::<Position>(entity_a2, POSITION)
                .is_none(),
            "Second generation reference should be invalid"
        );

        let pos = world.get_component::<Position>(entity_a3, POSITION);
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
        for (i, table) in world.tables.iter().enumerate() {
            println!("Table {}: mask={:b}", i, table.mask);
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

        let query_results = world.query_entities(POSITION | VELOCITY);
        assert!(
            query_results.contains(&entity),
            "Initial query should match\n\
                Entity mask: {:b}\n\
                Query mask: {:b}",
            world.component_mask(entity).unwrap(),
            POSITION | VELOCITY
        );

        world.add_components(entity, HEALTH);

        let query_results = world.query_entities(POSITION | VELOCITY);
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

        let query_results = world.query_entities(query_mask);
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

        let query_results = world.query_entities(query_mask);
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
    fn test_change_detection() {
        let mut world = World::default();

        let entity = world.spawn_entities(POSITION, 1)[0];

        world.add_components(entity, VELOCITY);

        assert!(world.try_next_event().is_none());

        if let Some(vel) = world.get_component_mut::<Velocity>(entity, VELOCITY) {
            vel.x = 10.0;
        }

        assert!(world.try_next_event().is_none());

        world.mark_changed(entity, VELOCITY);

        let event = world.try_next_event();
        assert!(event.is_some());

        if let Some(Event::ComponentChanged {
            kind,
            entity: changed_entity,
        }) = event
        {
            assert_eq!(kind, VELOCITY);
            assert_eq!(changed_entity, entity);
        } else {
            panic!("Expected ComponentChanged event");
        }

        assert!(world.try_next_event().is_none());
    }

    #[test]
    fn test_multiple_change_events() {
        let mut world = World::default();

        let entity = world.spawn_entities(POSITION | VELOCITY, 1)[0];

        if let Some(pos) = world.get_component_mut::<Position>(entity, POSITION) {
            pos.x = 5.0;
        }

        if let Some(vel) = world.get_component_mut::<Velocity>(entity, VELOCITY) {
            vel.x = 10.0;
        }

        world.mark_changed(entity, POSITION);
        world.mark_changed(entity, VELOCITY);

        let event1 = world.try_next_event();
        assert!(event1.is_some());
        if let Some(Event::ComponentChanged { kind, .. }) = event1 {
            assert_eq!(kind, POSITION);
        }

        let event2 = world.try_next_event();
        assert!(event2.is_some());
        if let Some(Event::ComponentChanged { kind, .. }) = event2 {
            assert_eq!(kind, VELOCITY);
        }

        assert!(world.try_next_event().is_none());
    }

    #[test]
    fn test_mark_changed_with_combined_mask() {
        let mut world = World::default();

        let entity = world.spawn_entities(POSITION | VELOCITY | HEALTH, 1)[0];

        if let Some(pos) = world.get_component_mut::<Position>(entity, POSITION) {
            pos.x = 1.0;
        }
        if let Some(vel) = world.get_component_mut::<Velocity>(entity, VELOCITY) {
            vel.x = 2.0;
        }

        world.mark_changed(entity, POSITION | VELOCITY);

        let event = world.try_next_event();
        assert!(event.is_some());
        if let Some(Event::ComponentChanged {
            kind,
            entity: changed_entity,
        }) = event
        {
            assert_eq!(kind, POSITION | VELOCITY);
            assert_eq!(changed_entity, entity);
        }

        assert!(world.try_next_event().is_none());
    }

    #[test]
    fn test_clear_events() {
        let mut world = World::default();

        let entity = world.spawn_entities(POSITION | VELOCITY, 1)[0];

        world.mark_changed(entity, POSITION);
        world.mark_changed(entity, VELOCITY);
        world.mark_changed(entity, POSITION | VELOCITY);

        world.clear_events();

        assert!(world.try_next_event().is_none());
    }

    #[test]
    fn test_set_component() {
        let mut world = World::default();
        let entity = world.spawn_entities(POSITION, 1)[0];
        world.set_position(entity, Position { x: 1.0, y: 2.0 });
        assert_eq!(
            world.get_component::<Position>(entity, POSITION).unwrap().x,
            1.0
        );
        assert_eq!(
            world.get_component::<Position>(entity, POSITION).unwrap().y,
            2.0
        );

        world.set_position(entity, Position { x: 3.0, y: 4.0 });
        assert_eq!(
            world.get_component::<Position>(entity, POSITION).unwrap().x,
            3.0
        );
        assert_eq!(
            world.get_component::<Position>(entity, POSITION).unwrap().y,
            4.0
        );
    }
}
