//! freecs is a zero-abstraction ECS library for Rust, designed for high performance and simplicity.
//!
//! It provides an archetypal table-based storage system for components, allowing for fast queries,
//! fast system iteration, and parallel processing.
//!
//! A macro is used to define the world and its components, and generates
//! the entity component system as part of your source code at compile time. The generated code
//! contains only plain data structures (no methods) and free functions that transform them, achieving static dispatch.
//!
//! The internal implementation is ~500 loc, and does not use object orientation, generics, traits, or dynamic dispatch.
//!
//! # Key Features
//!
//! - **Table-based Storage**: Entities with the same components are stored together in memory
//! - **Raw Access**: Functions work directly on the underlying vectors of components
//! - **Parallel Processing**: Built-in support for processing tables in parallel with rayon
//! - **Simple Queries**: Find entities by their components using bit masks
//! - **Serialization**: Save and load worlds using serde
//!
//! # Creating a World
//!
//! ```rust,ignore
//! use freecs::{world, has_components};
//! use serde::{Serialize, Deserialize};
//!
//! // First, define components.
//! // They must implement: `Default + Clone + Serialize + Deserialize`
//!
//! #[derive(Default, Clone, Debug, Serialize, Deserialize)]
//! struct Position { x: f32, y: f32 }
//!
//! #[derive(Default, Clone, Debug, Serialize, Deserialize)]
//! struct Velocity { x: f32, y: f32 }
//!
//! // Then, create a world with the `world!` macro.
//! // Resources are stored independently of component data and are not serialized.
//! world! {
//!   World {
//!       components {
//!         position: Position => POSITION,
//!         velocity: Velocity => VELOCITY,
//!         health: Health => HEALTH,
//!       },
//!       Resources {
//!           delta_time: f32
//!       }
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
//! let entity = spawn_entities(&mut world, POSITION | VELOCITY, 1)[0];
//!
//! // Lookup and modify a component
//! if let Some(pos) = get_component_mut::<Position>(&mut world, entity, POSITION) {
//!     pos.x += 1.0;
//! }
//!
//! // Add new components to an entity by mask
//! add_components(&mut world, entity, HEALTH | VELOCITY);
//!
//! // Remove components from an entity by mask
//! remove_components(&mut world, entity, VELOCITY | POSITION);
//!
//! // Query entities, iterating over all entities matching the component mask
//! let entities = query_entities(&world, POSITION | VELOCITY);
//!
//! // Query for the first entity matching the component mask, returning early when found
//! let player = query_first_entity(&world, POSITION | VELOCITY);
//! ```
//!
//! ## Systems and Parallel Processing
//!
//! Systems are plain functions that iterate over
//! the component tables and transform component data.
//!
//! The example function below invokes two systems in parallel
//! for each table in the world, filtered by component mask.
//!
//! ```rust
//! pub fn run_systems(world: &mut World, dt: f32) {
//!     use rayon::prelude::*;
//!     world.tables.par_iter_mut().for_each(|table| {
//!         if has_components!(table, POSITION | VELOCITY | HEALTH) {
//!             update_positions_system(&mut table.position, &table.velocity, dt);
//!         }
//!         if has_components!(table, HEALTH) {
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
//!         health.value *= 0.98; // gradually decline health value
//!     });
//! }
//! ```
#[macro_export]
macro_rules! world {
    (
        $world:ident {
            components {
                $($name:ident: $type:ty => $mask:ident),* $(,)?
            }$(,)?
            $resources:ident {
                $($resource_name:ident: $resource_type:ty),* $(,)?
            }
        }
    ) => {

        /// Component masks
        #[repr(u32)]
        #[allow(clippy::upper_case_acronyms)]
        #[allow(non_camel_case_types)]
        pub enum Component {
            $($mask,)*
        }

        $(pub const $mask: u32 = 1 << (Component::$mask as u32);)*

        /// Entity ID, an index into storage and a generation counter to prevent stale references
        #[derive(Default, Clone, Copy, Debug, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
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

        /// Entity location cache for quick access
        #[derive(Default, serde::Serialize, serde::Deserialize)]
        pub struct EntityLocations {
            pub generations: Vec<u32>,
            pub locations: Vec<Option<(usize, usize)>>,
        }

        /// A collection of component tables and resources
        #[derive(Default, serde::Serialize, serde::Deserialize)]
        pub struct $world {
            pub entity_locations: EntityLocations,
            pub tables: Vec<ComponentArrays>,
            pub next_entity_id: u32,
            pub table_registry: Vec<(u32, usize)>,
            #[serde(skip)]
            #[allow(unused)]
            pub resources: $resources,
        }

        /// Resources
        #[derive(Default)]
        pub struct $resources {
            $(pub $resource_name: $resource_type,)*
        }

        /// Component Table
        #[derive(Default, serde::Serialize, serde::Deserialize)]
        pub struct ComponentArrays {
            $(pub $name: Vec<$type>,)*
            pub entity_indices: Vec<EntityId>,
            pub mask: u32,
        }

        /// Spawn a batch of new entities with the same component mask
        pub fn spawn_entities(world: &mut $world, mask: u32, count: usize) -> Vec<EntityId> {
            let mut entities = Vec::with_capacity(count);
            let table_index = get_or_create_table(world, mask);

            // Pre-size entity locations
            let max_id = (world.next_entity_id + count as u32) as usize;
            if max_id > world.entity_locations.generations.len() {
                world.entity_locations.generations.resize(max_id, 0);
                world.entity_locations.locations.resize(max_id, None);
            }

            // Reserve space in components
            $(
                if mask & $mask != 0 {
                    world.tables[table_index].$name.reserve(count);
                }
            )*
            world.tables[table_index].entity_indices.reserve(count);

            for _ in 0..count {
                let entity = create_entity(world);
                add_to_table(
                    &mut world.tables[table_index],
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
                location_insert(
                    &mut world.entity_locations,
                    entity,
                    (table_index, world.tables[table_index].entity_indices.len() - 1),
                );
            }

            entities
        }

        /// Query for all entities that match the component mask
        pub fn query_entities(world: &$world, mask: u32) -> Vec<EntityId> {
            let mut result = Vec::new();
            for table in &world.tables {
                if table.mask & mask == mask {
                    result.extend(table.entity_indices.iter().copied());
                }
            }
            result
        }

        /// Query for the first entity that matches the component mask
        /// Returns as soon as a match is found, instead of running for all entities
        /// Useful for components where only one instance exists on any entity at a time,
        /// such as keyboard input / mouse input / controllers.
        pub fn query_first_entity(world: &$world, mask: u32) -> Option<EntityId> {
            for table in &world.tables {
                if table.mask & mask == mask {
                    return table.entity_indices.first().copied();
                }
            }
            None
        }

        /// Get a specific component for an entity
        pub fn get_component<T: 'static>(world: &$world, entity: EntityId, mask: u32) -> Option<&T> {
           let (table_index, array_index) = location_get(&world.entity_locations, entity)?;
           let table = &world.tables[table_index];

           if table.mask & mask == 0 {
               return None;
           }

           $(
               if mask == $mask && std::any::TypeId::of::<T>() == std::any::TypeId::of::<$type>() {
                   // SAFETY: This operation is safe because:
                   // 1. We verify the component type T exactly matches $type via TypeId
                   // 2. We confirm the table contains this component via mask check
                   // 3. array_index is valid from location_get bounds check
                   // 4. The reference is valid for the lifetime of the return value
                   //    because it's tied to the table reference lifetime
                   // 5. No mutable aliases can exist during the shared borrow
                   // 6. The type cast maintains proper alignment as types are identical
                   return Some(unsafe { &*(&table.$name[array_index] as *const $type as *const T) });
               }
           )*

           None
        }

        /// Get a mutable reference to a specific component for an entity
        pub fn get_component_mut<T: 'static>(world: &mut $world, entity: EntityId, mask: u32) -> Option<&mut T> {
            let (table_index, array_index) = location_get(&world.entity_locations, entity)?;
            let table = &mut world.tables[table_index];

            if table.mask & mask == 0 {
                return None;
            }

            $(
                if mask == $mask && std::any::TypeId::of::<T>() == std::any::TypeId::of::<$type>() {
                    // SAFETY: This operation is safe because:
                    // 1. We verify the component type T exactly matches $type via TypeId
                    // 2. We confirm the table contains this component via mask check
                    // 3. array_index is valid from location_get bounds check
                    // 4. We have exclusive access through the mutable borrow
                    // 5. The borrow checker ensures no other references exist
                    // 6. The pointer cast is valid as we verified the types are identical
                    // 7. Proper alignment is maintained as the types are the same
                    return Some(unsafe { &mut *(&mut table.$name[array_index] as *mut $type as *mut T) });
                }
            )*

            None
        }

        /// Despawn a batch of entities
        pub fn despawn_entities(world: &mut $world, entities: &[EntityId]) -> Vec<EntityId> {
            use std::collections::{HashMap, HashSet};

            let entities: HashSet<_> = entities.iter().copied().collect();
            let mut despawned = Vec::new();
            let mut table_removals: HashMap<usize, Vec<usize>> = HashMap::new();

            // First pass: collect removals
            for &entity in &entities {
                if let Some((table_index, array_index)) = location_get(&world.entity_locations, entity) {
                    table_removals.entry(table_index)
                        .or_insert_with(Vec::new)
                        .push(array_index);

                    let id = entity.id as usize;
                    if id < world.entity_locations.locations.len() {
                        world.entity_locations.locations[id] = None;
                        world.entity_locations.generations[id] =
                            world.entity_locations.generations[id].wrapping_add(1);
                        despawned.push(entity);
                    }
                }
            }

            // Second pass: remove entities and cleanup
            let mut table_index = 0;
            while table_index < world.tables.len() {
                if let Some(mut indices) = table_removals.remove(&table_index) {
                    indices.sort_unstable_by(|a, b| b.cmp(a));

                    // Remove entities
                    for &index in &indices {
                        remove_from_table(&mut world.tables[table_index], index);
                    }

                    // If table is now empty
                    if world.tables[table_index].entity_indices.is_empty() {
                        let last_index = world.tables.len() - 1;

                        if table_index != last_index {
                            // Remember masks before swap
                            let removed_mask = world.tables[table_index].mask;
                            let swapped_mask = world.tables[last_index].mask;

                            // Do the swap
                            world.tables.swap_remove(table_index);

                            // Update registry for both removed and swapped table
                            world.table_registry.retain(|(mask, _)| *mask != removed_mask);
                            if let Some(entry) = world.table_registry.iter_mut()
                                .find(|(mask, _)| *mask == swapped_mask) {
                                entry.1 = table_index;
                            }

                            // Critical: Update ALL entity locations for swapped table
                            for loc in world.entity_locations.locations.iter_mut() {
                                if let Some((ref mut index, _)) = loc {
                                    if *index == last_index {
                                        *index = table_index;
                                    }
                                }
                            }

                            // Don't increment table_index since we swapped a new table here
                            continue;
                        } else {
                            // Just remove the last table and its registry entry
                            let removed_mask = world.tables[table_index].mask;
                            world.tables.pop();
                            world.table_registry.retain(|(mask, _)| *mask != removed_mask);
                        }
                    } else {
                        // Update indices for remaining entities in non-empty table
                        for (new_index, &entity) in world.tables[table_index].entity_indices.iter().enumerate() {
                            if !entities.contains(&entity) {
                                location_insert(&mut world.entity_locations, entity, (table_index, new_index));
                            }
                        }
                    }
                }
                table_index += 1;
            }

            despawned
        }

        /// Add components to an entity
        pub fn add_components(world: &mut World, entity: EntityId, mask: u32) -> bool {
            if let Some((table_index, array_index)) = location_get(&world.entity_locations, entity) {
                let current_mask = world.tables[table_index].mask;

                // If entity already has all these components, no need to move
                if current_mask & mask == mask {
                    return true;
                }

                let new_mask = current_mask | mask;
                let new_table_index = get_or_create_table(world, new_mask);

                // Move entity to new table
                move_entity(world, entity, table_index, array_index, new_table_index);

                // If old table is now empty, merge tables
                if world.tables[table_index].entity_indices.is_empty() {
                    merge_tables(world);
                }

                true
            } else {
                false
            }
        }

        /// Remove components from an entity
        pub fn remove_components(world: &mut $world, entity: EntityId, mask: u32) -> bool {
            if let Some((table_index, array_index)) = location_get(&world.entity_locations, entity) {
                let current_mask = world.tables[table_index].mask;
                // If entity doesn't have any of these components, no need to move
                if current_mask & mask == 0 {
                    return true;
                }

                let source_table_index = table_index;  // Keep track of source table
                let new_mask = current_mask & !mask;
                let new_table_index = get_or_create_table(world, new_mask);

                // Move entity first
                move_entity(world, entity, table_index, array_index, new_table_index);

                // Check if source table is now empty
                if world.tables[source_table_index].entity_indices.is_empty() {
                    // Remove the empty table using swap_remove
                    let last_index = world.tables.len() - 1;
                    if source_table_index != last_index {
                        let removed_mask = world.tables[source_table_index].mask;
                        let swapped_mask = world.tables[last_index].mask;

                        // Update entity locations for the swapped table
                        for loc in world.entity_locations.locations.iter_mut() {
                            if let Some((ref mut index, _)) = loc {
                                if *index == last_index {
                                    *index = source_table_index;
                                }
                            }
                        }

                        // Remove table and update registry
                        world.tables.swap_remove(source_table_index);
                        world.table_registry.retain(|(mask, _)| *mask != removed_mask);
                        if let Some(entry) = world.table_registry.iter_mut()
                            .find(|(mask, _)| *mask == swapped_mask) {
                            entry.1 = source_table_index;
                        }
                    } else {
                        // Just remove the last table
                        let removed_mask = world.tables[source_table_index].mask;
                        world.tables.pop();
                        world.table_registry.retain(|(mask, _)| *mask != removed_mask);
                    }
                }

                true
            } else {
                false
            }
        }

        /// Get the current component mask for an entity
        pub fn component_mask(world: &$world, entity: EntityId) -> Option<u32> {
            location_get(&world.entity_locations, entity)
                .map(|(table_index, _)| world.tables[table_index].mask)
        }

        /// Merge tables that have the same mask
        fn merge_tables(world: &mut $world) {
            let mut index = 0;
            while index < world.tables.len() {
                if world.tables[index].entity_indices.is_empty() {
                    let last_index = world.tables.len() - 1;

                    if index != last_index {
                        let removed_mask = world.tables[index].mask;
                        let swapped_mask = world.tables[last_index].mask;

                        world.tables.swap_remove(index);

                        world.table_registry.retain(|(mask, _)| *mask != removed_mask);
                        if let Some(entry) = world.table_registry.iter_mut()
                            .find(|(mask, _)| *mask == swapped_mask) {
                            entry.1 = index;
                        }

                        for location in world.entity_locations.locations.iter_mut() {
                            if let Some((ref mut table_index, _)) = location {
                                if *table_index == last_index {
                                    *table_index = index;
                                }
                            }
                        }
                        // Don't increment i since we have a new table here
                    } else {
                        let removed_mask = world.tables[index].mask;
                        world.tables.pop();
                        world.table_registry.retain(|(mask, _)| *mask != removed_mask);
                    }
                } else {
                    index += 1;
                }
            }
        }

        /// Get the total number of entities in the world
        pub fn total_entities(world: &$world) -> usize {
            world.tables.iter().map(|table| table.entity_indices.len()).sum()
        }

        // Implementation details

        fn remove_from_table(arrays: &mut ComponentArrays, index: usize) {
            $(
                if arrays.mask & $mask != 0 {
                    arrays.$name.swap_remove(index);
                }
            )*
            arrays.entity_indices.swap_remove(index);
        }

        fn move_entity(
            world: &mut World,
            entity: EntityId,
            from_table: usize,
            from_index: usize,
            to_table: usize,
        ) {
            // Get components before any modifications
            let components = get_components(&world.tables[from_table], from_index);

            // Add to new table
            add_to_table(&mut world.tables[to_table], entity, components);
            let new_index = world.tables[to_table].entity_indices.len() - 1;

            // Update entity location BEFORE removing from old table
            location_insert(&mut world.entity_locations, entity, (to_table, new_index));

            // Remove from old table - this may trigger a swap_remove
            if from_index < world.tables[from_table].entity_indices.len() {
                remove_from_table(&mut world.tables[from_table], from_index);

                // If there was a swap, update the swapped entity's location
                if from_index < world.tables[from_table].entity_indices.len() {
                    let swapped_entity = world.tables[from_table].entity_indices[from_index];
                    location_insert(&mut world.entity_locations, swapped_entity, (from_table, from_index));
                }
            }
        }

        fn get_components(
            arrays: &ComponentArrays,
            index: usize,
        ) -> (  $(Option<$type>,)* ) {
            (
                $(
                    if arrays.mask & $mask != 0 {
                        Some(arrays.$name[index].clone())
                    } else {
                        None
                    },
                )*
            )
        }

        fn location_get(locations: &EntityLocations, entity: EntityId) -> Option<(usize, usize)> {
            if entity.id as usize >= locations.generations.len() {
                return None;
            }

            if locations.generations[entity.id as usize] != entity.generation {
                return None;
            }

            if entity.id as usize >= locations.locations.len() {
                None
            } else {
                locations.locations[entity.id as usize]
            }
        }

        fn location_insert(
            locations: &mut EntityLocations,
            entity: EntityId,
            location: (usize, usize),
        ) {
            let id = entity.id as usize;

            if id >= locations.generations.len() {
                locations.generations.resize(id + 1, 0);
            }
            if id >= locations.locations.len() {
                locations.locations.resize(id + 1, None);
            }

            locations.generations[id] = entity.generation;
            locations.locations[id] = Some(location);
        }

        fn create_entity(world: &mut World) -> EntityId {
            let id = world.next_entity_id;
            let id_usize = id as usize;

            // CRITICAL: Resize before using the array
            while id_usize >= world.entity_locations.generations.len() {
                // Double the size to amortize resize cost
                let new_size = (world.entity_locations.generations.len() * 2).max(64);
                world.entity_locations.generations.resize(new_size, 0);
                world.entity_locations.locations.resize(new_size, None);
            }

            // Now safe to use id_usize since arrays are properly sized
            let generation = world.entity_locations.generations[id_usize];
            world.next_entity_id += 1;

            EntityId { id, generation }
        }

        fn add_to_table(
            arrays: &mut ComponentArrays,
            entity: EntityId,
            components: ( $(Option<$type>,)* ),
        ) {
            let ($($name,)*) = components;
            $(
                if arrays.mask & $mask != 0 {
                    arrays
                        .$name
                        .push($name.unwrap_or_default());
                }
            )*
            arrays.entity_indices.push(entity);
        }

        fn get_or_create_table(world: &mut $world, mask: u32) -> usize {
            // Look for EXACT match only by mask
            if let Some(pos) = world.table_registry
                .iter()
                .position(|(m, _)| *m == mask)
            {
                return world.table_registry[pos].1;
            }

            // Create new table
            let table_index = world.tables.len();
            world.tables.push(ComponentArrays {
                mask,
                ..Default::default()
            });
            world.table_registry.push((mask, table_index));
            table_index
        }
    };
}

#[macro_export]
macro_rules! has_components {
    ($table:expr, $mask:expr) => {
        $table.mask & $mask == $mask
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use rayon::*;
    use std::collections::HashSet;

    world! {
      World {
          components {
            position: Position => POSITION,
            velocity: Velocity => VELOCITY,
            health: Health => HEALTH,
          },
          Resources {
              _delta_time: f32
          }
      }
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
        use rayon::prelude::*;

        // Systems are functions that iterate over
        // the component tables and transform component data.
        // This function invokes two systems in parallel
        // for each table in the world filtered by component mask.
        pub fn run_systems(world: &mut World, dt: f32) {
            world.tables.par_iter_mut().for_each(|table| {
                if has_components!(table, POSITION | VELOCITY | HEALTH) {
                    update_positions_system(&mut table.position, &table.velocity, dt);
                }
                if has_components!(table, HEALTH) {
                    health_system(&mut table.health);
                }
            });
        }

        // The system itself can also access components in parallel
        #[inline]
        pub fn update_positions_system(
            positions: &mut [Position],
            velocities: &[Velocity],
            dt: f32,
        ) {
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
    // Helper function to create a test world with some entities
    fn setup_test_world() -> (World, EntityId) {
        let mut world = World::default();
        let entity = spawn_entities(&mut world, POSITION | VELOCITY, 1)[0];

        // Set initial component values
        if let Some(pos) = get_component_mut::<Position>(&mut world, entity, POSITION) {
            pos.x = 1.0;
            pos.y = 2.0;
        }
        if let Some(vel) = get_component_mut::<Velocity>(&mut world, entity, VELOCITY) {
            vel.x = 3.0;
            vel.y = 4.0;
        }

        (world, entity)
    }

    #[test]
    fn test_spawn_entities() {
        let mut world = World::default();
        let entities = spawn_entities(&mut world, POSITION | VELOCITY, 3);

        assert_eq!(entities.len(), 3);
        assert_eq!(total_entities(&world), 3);

        // Verify each entity has the correct components
        for entity in entities {
            assert!(get_component::<Position>(&world, entity, POSITION).is_some());
            assert!(get_component::<Velocity>(&world, entity, VELOCITY).is_some());
            assert!(get_component::<Health>(&world, entity, HEALTH).is_none());
        }
    }

    #[test]
    fn test_component_access() {
        let (mut world, entity) = setup_test_world();

        // Test reading components
        let pos = get_component::<Position>(&world, entity, POSITION).unwrap();
        assert_eq!(pos.x, 1.0);
        assert_eq!(pos.y, 2.0);

        // Test mutating components
        if let Some(pos) = get_component_mut::<Position>(&mut world, entity, POSITION) {
            pos.x = 5.0;
        }

        let pos = get_component::<Position>(&world, entity, POSITION).unwrap();
        assert_eq!(pos.x, 5.0);
    }

    #[test]
    fn test_add_remove_components() {
        let (mut world, entity) = setup_test_world();

        // Initial state
        assert!(get_component::<Health>(&world, entity, HEALTH).is_none());

        // Add component
        add_components(&mut world, entity, HEALTH);
        assert!(get_component::<Health>(&world, entity, HEALTH).is_some());

        // Remove component
        remove_components(&mut world, entity, HEALTH);
        assert!(get_component::<Health>(&world, entity, HEALTH).is_none());
    }

    #[test]
    fn test_component_mask() {
        let (mut world, entity) = setup_test_world();

        // Check initial mask
        let mask = component_mask(&world, entity).unwrap();
        assert_eq!(mask, POSITION | VELOCITY);

        // Check mask after adding component
        add_components(&mut world, entity, HEALTH);
        let mask = component_mask(&world, entity).unwrap();
        assert_eq!(mask, POSITION | VELOCITY | HEALTH);
    }

    #[test]
    fn test_query_entities() {
        let mut world = World::default();

        // Create entities with different component combinations
        let e1 = spawn_entities(&mut world, POSITION | VELOCITY, 1)[0];
        let _e2 = spawn_entities(&mut world, POSITION | HEALTH, 1)[0];
        let e3 = spawn_entities(&mut world, POSITION | VELOCITY | HEALTH, 1)[0];

        // Test queries
        let pos_vel = query_entities(&world, POSITION | VELOCITY);
        let pos_health = query_entities(&world, POSITION | HEALTH);
        let all = query_entities(&world, POSITION | VELOCITY | HEALTH);

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

        let e1 = spawn_entities(&mut world, POSITION | VELOCITY, 1)[0];
        let e2 = spawn_entities(&mut world, POSITION | VELOCITY | HEALTH, 1)[0];

        let first = query_first_entity(&world, POSITION | VELOCITY).unwrap();
        assert!(first == e1 || first == e2);

        assert!(query_first_entity(&world, HEALTH).is_some());
        assert!(query_first_entity(&world, POSITION | VELOCITY | HEALTH).is_some());
    }

    #[test]
    fn test_despawn_entities() {
        let mut world = World::default();

        // Spawn multiple entities
        let entities = spawn_entities(&mut world, POSITION | VELOCITY, 3);
        assert_eq!(total_entities(&world), 3);

        // Despawn one entity
        let despawned = despawn_entities(&mut world, &[entities[1]]);
        assert_eq!(despawned.len(), 1);
        assert_eq!(total_entities(&world), 2);

        // Verify the entity is truly despawned
        assert!(get_component::<Position>(&world, entities[1], POSITION).is_none());

        // Verify other entities still exist
        assert!(get_component::<Position>(&world, entities[0], POSITION).is_some());
        assert!(get_component::<Position>(&world, entities[2], POSITION).is_some());
    }

    #[test]
    fn test_merge_tables() {
        let mut world = World::default();

        // Create entities in different tables with same components
        let e1 = spawn_entities(&mut world, POSITION | VELOCITY, 1)[0];
        let e2 = spawn_entities(&mut world, POSITION | VELOCITY, 1)[0];

        // Add and remove a component to create a fragmented table
        add_components(&mut world, e1, HEALTH);
        remove_components(&mut world, e1, HEALTH);

        let initial_table_count = world.tables.len();
        merge_tables(&mut world);

        // Verify tables were merged
        assert!(world.tables.len() <= initial_table_count);

        // Verify all entities still accessible
        assert!(get_component::<Position>(&world, e1, POSITION).is_some());
        assert!(get_component::<Position>(&world, e2, POSITION).is_some());
    }

    #[test]
    fn test_parallel_systems() {
        let mut world = World::default();

        let entity = spawn_entities(&mut world, POSITION | VELOCITY | HEALTH, 1)[0];

        // Set initial values
        if let Some(pos) = get_component_mut::<Position>(&mut world, entity, POSITION) {
            pos.x = 0.0;
            pos.y = 0.0;
        }
        if let Some(vel) = get_component_mut::<Velocity>(&mut world, entity, VELOCITY) {
            vel.x = 1.0;
            vel.y = 1.0;
        }
        if let Some(health) = get_component_mut::<Health>(&mut world, entity, HEALTH) {
            health.value = 100.0;
        }

        // Run systems
        systems::run_systems(&mut world, 1.0);

        // Verify system effects
        let pos = get_component::<Position>(&world, entity, POSITION).unwrap();
        let health = get_component::<Health>(&world, entity, HEALTH).unwrap();

        assert_eq!(pos.x, 1.0);
        assert_eq!(pos.y, 1.0);
        assert!(health.value < 100.0); // Health should have decreased
    }

    #[test]
    fn test_add_components() {
        let (mut world, entity) = setup_test_world();

        // Initial state
        assert!(get_component::<Health>(&world, entity, HEALTH).is_none());

        // Add component
        add_components(&mut world, entity, HEALTH);
        assert!(get_component::<Health>(&world, entity, HEALTH).is_some());

        // Remove component
        remove_components(&mut world, entity, HEALTH);
        assert!(get_component::<Health>(&world, entity, HEALTH).is_none());
    }

    #[test]
    fn test_multiple_component_addition() {
        let mut world = World::default();
        let entity = spawn_entities(&mut world, POSITION, 1)[0];

        // Add multiple components at once
        add_components(&mut world, entity, VELOCITY | HEALTH);

        // Verify all components exist and are accessible
        assert!(get_component::<Position>(&world, entity, POSITION).is_some());
        assert!(get_component::<Velocity>(&world, entity, VELOCITY).is_some());
        assert!(get_component::<Health>(&world, entity, HEALTH).is_some());

        // Verify component data persists through moves
        if let Some(pos) = get_component_mut::<Position>(&mut world, entity, POSITION) {
            pos.x = 1.0;
        }
        add_components(&mut world, entity, VELOCITY); // Should be no-op
        assert_eq!(
            get_component::<Position>(&world, entity, POSITION)
                .unwrap()
                .x,
            1.0
        );
    }

    #[test]
    fn test_component_chain_addition() {
        let mut world = World::default();
        let entity = spawn_entities(&mut world, POSITION, 1)[0];

        // Set initial value
        if let Some(pos) = get_component_mut::<Position>(&mut world, entity, POSITION) {
            pos.x = 1.0;
        }

        // Add components one at a time to force multiple table moves
        add_components(&mut world, entity, VELOCITY);
        add_components(&mut world, entity, HEALTH);

        // Verify original data survived multiple moves
        assert_eq!(
            get_component::<Position>(&world, entity, POSITION)
                .unwrap()
                .x,
            1.0
        );
    }

    #[test]
    fn test_component_removal_order() {
        let mut world = World::default();
        let entity = spawn_entities(&mut world, POSITION | VELOCITY | HEALTH, 1)[0];

        // Remove in different orders to test table transitions
        remove_components(&mut world, entity, VELOCITY);
        remove_components(&mut world, entity, HEALTH);
        assert!(get_component::<Position>(&world, entity, POSITION).is_some());
        assert!(get_component::<Velocity>(&world, entity, VELOCITY).is_none());
        assert!(get_component::<Health>(&world, entity, HEALTH).is_none());
    }

    #[test]
    fn test_edge_cases() {
        let mut world = World::default();

        // Test empty entity
        let empty = spawn_entities(&mut world, 0, 1)[0];

        // Add to empty
        add_components(&mut world, empty, POSITION);
        assert!(get_component::<Position>(&world, empty, POSITION).is_some());

        // Add same component multiple times
        add_components(&mut world, empty, POSITION);
        add_components(&mut world, empty, POSITION);

        // Remove non-existent component
        remove_components(&mut world, empty, VELOCITY);

        // Remove all components
        remove_components(&mut world, empty, POSITION);
        assert_eq!(component_mask(&world, empty).unwrap(), 0);

        // Test invalid entity
        let invalid = EntityId {
            id: 9999,
            generation: 0,
        };
        assert!(!add_components(&mut world, invalid, POSITION));
    }

    #[test]
    fn test_component_data_integrity() {
        let mut world = World::default();
        let entity = spawn_entities(&mut world, POSITION | VELOCITY, 1)[0];

        // Set initial values
        {
            let pos = get_component_mut::<Position>(&mut world, entity, POSITION).unwrap();
            pos.x = 1.0;
            pos.y = 2.0;
            let vel = get_component_mut::<Velocity>(&mut world, entity, VELOCITY).unwrap();
            vel.x = 3.0;
            vel.y = 4.0;
        }

        // Add/remove other components
        add_components(&mut world, entity, HEALTH);
        remove_components(&mut world, entity, HEALTH);
        add_components(&mut world, entity, HEALTH);

        // Verify original values maintained
        let pos = get_component::<Position>(&world, entity, POSITION).unwrap();
        let vel = get_component::<Velocity>(&world, entity, VELOCITY).unwrap();
        assert_eq!(pos.x, 1.0);
        assert_eq!(pos.y, 2.0);
        assert_eq!(vel.x, 3.0);
        assert_eq!(vel.y, 4.0);
    }

    fn validate_world_integrity(world: &World) -> Result<(), String> {
        // Check table registry matches tables
        if world.table_registry.len() != world.tables.len() {
            return Err(format!(
                "Table registry length ({}) doesn't match tables length ({})",
                world.table_registry.len(),
                world.tables.len()
            ));
        }

        // Verify all registry entries point to valid tables
        for (i, (mask, table_index)) in world.table_registry.iter().enumerate() {
            if *table_index >= world.tables.len() {
                return Err(format!(
                    "Registry entry {} points to invalid table {} (max {})",
                    i,
                    table_index,
                    world.tables.len() - 1
                ));
            }
            if world.tables[*table_index].mask != *mask {
                return Err(format!(
                    "Registry mask mismatch at {}: registry={:b}, table={:b}",
                    i, mask, world.tables[*table_index].mask
                ));
            }
        }

        // Verify all entity locations point to valid tables
        for (entity_id, location) in world.entity_locations.locations.iter().enumerate() {
            if let Some((table_index, array_index)) = location {
                if *table_index >= world.tables.len() {
                    return Err(format!(
                        "Entity {} location points to invalid table {} (max {})",
                        entity_id,
                        table_index,
                        world.tables.len() - 1
                    ));
                }
                let table = &world.tables[*table_index];
                if *array_index >= table.entity_indices.len() {
                    return Err(format!(
                        "Entity {} location points to invalid index {} in table {} (max {})",
                        entity_id,
                        array_index,
                        table_index,
                        table.entity_indices.len() - 1
                    ));
                }
            }
        }

        Ok(())
    }

    #[test]
    fn test_entity_references_through_moves() {
        let mut world = World::default();

        // Create entities with references to each other
        let entity1 = spawn_entities(&mut world, POSITION, 1)[0];
        let entity2 = spawn_entities(&mut world, POSITION, 1)[0];

        // Store reference to entity2 in entity1
        add_components(&mut world, entity1, VELOCITY);
        if let Some(vel) = get_component_mut::<Velocity>(&mut world, entity1, VELOCITY) {
            vel.x = entity2.id as f32; // Store reference
        }

        // Move referenced entity
        add_components(&mut world, entity2, VELOCITY | HEALTH);

        // Verify reference still works
        let stored_id = get_component::<Velocity>(&world, entity1, VELOCITY)
            .unwrap()
            .x as u32;
        let entity2_loc = location_get(&world.entity_locations, entity2);
        assert!(entity2_loc.is_some());
        assert_eq!(stored_id, entity2.id);
    }

    #[test]
    fn test_table_fragmentation() {
        let mut world = World::default();
        let mut all_entities = Vec::new();

        println!("\nCreating initial state with multiple tables:");

        // Create entities in first table (POSITION only)
        let e1 = spawn_entities(&mut world, POSITION, 3);
        all_entities.extend(e1.clone());

        println!("\nAfter spawning POSITION entities:");
        for (i, table) in world.tables.iter().enumerate() {
            println!(
                "Table {}: mask={:b}, entities={}",
                i,
                table.mask,
                table.entity_indices.len()
            );
        }

        // Create entities in second table (POSITION | VELOCITY)
        let e2 = spawn_entities(&mut world, POSITION | VELOCITY, 3);
        all_entities.extend(e2.clone());

        println!("\nAfter spawning POSITION | VELOCITY entities:");
        for (i, table) in world.tables.iter().enumerate() {
            println!(
                "Table {}: mask={:b}, entities={}",
                i,
                table.mask,
                table.entity_indices.len()
            );
        }

        // Create entities in third table (POSITION | VELOCITY | HEALTH)
        let e3 = spawn_entities(&mut world, POSITION | VELOCITY | HEALTH, 3);
        all_entities.extend(e3.clone());

        println!("\nAfter spawning POSITION | VELOCITY | HEALTH entities:");
        println!("Number of tables: {}", world.tables.len());
        for (i, table) in world.tables.iter().enumerate() {
            println!(
                "Table {}: mask={:b}, entities={}",
                i,
                table.mask,
                table.entity_indices.len()
            );
        }

        let initial_table_count = world.tables.len();
        println!("\nInitial table count: {}", initial_table_count);

        // Remove VELOCITY from e2 entities one by one and verify table cleanup
        for (i, &entity) in e2.iter().enumerate() {
            println!("\nRemoving VELOCITY from entity {}", i);
            remove_components(&mut world, entity, VELOCITY);

            println!("Tables after removal {}:", i);
            for (j, table) in world.tables.iter().enumerate() {
                println!(
                    "Table {}: mask={:b}, entities={}",
                    j,
                    table.mask,
                    table.entity_indices.len()
                );
            }
            // After the last entity is moved, the source table should be gone
            if i == e2.len() - 1 {
                assert!(
                    world.tables.len() < initial_table_count,
                    "Table count should decrease after moving last entity"
                );
            }
        }

        println!("\nFinal state:");
        println!("Number of tables: {}", world.tables.len());
        for (i, table) in world.tables.iter().enumerate() {
            println!(
                "Table {}: mask={:b}, entities={}",
                i,
                table.mask,
                table.entity_indices.len()
            );
        }

        // Verify no empty tables exist
        for (i, table) in world.tables.iter().enumerate() {
            assert!(
                !table.entity_indices.is_empty(),
                "Table {} is empty (mask={:b})",
                i,
                table.mask
            );
        }

        // Verify table count decreased
        assert!(
            world.tables.len() < initial_table_count,
            "Expected fewer than {} tables, got {}",
            initial_table_count,
            world.tables.len()
        );

        // Verify components
        for &entity in &e1 {
            assert!(get_component::<Position>(&world, entity, POSITION).is_some());
        }

        for &entity in &e2 {
            assert!(get_component::<Position>(&world, entity, POSITION).is_some());
            assert!(get_component::<Velocity>(&world, entity, VELOCITY).is_none());
        }

        for &entity in &e3 {
            assert!(get_component::<Position>(&world, entity, POSITION).is_some());
            assert!(get_component::<Velocity>(&world, entity, VELOCITY).is_some());
            assert!(get_component::<Health>(&world, entity, HEALTH).is_some());
        }

        // Verify registry matches tables
        assert_eq!(world.table_registry.len(), world.tables.len());
        for (mask, table_index) in &world.table_registry {
            assert!(*table_index < world.tables.len());
            assert_eq!(world.tables[*table_index].mask, *mask);
        }
    }

    #[test]
    fn test_table_registry_integrity() {
        let mut world = World::default();
        validate_world_integrity(&world).expect("Initial world state invalid");

        // Create entities with various component combinations
        let mut entities = Vec::new();
        for mask in [
            POSITION,
            POSITION | VELOCITY,
            POSITION | HEALTH,
            POSITION | VELOCITY | HEALTH,
        ] {
            let entity = spawn_entities(&mut world, mask, 1)[0];
            validate_world_integrity(&world).unwrap_or_else(|_| {
                panic!("World invalid after spawning entity with mask {mask:b}")
            });

            println!("Spawned entity with mask {:b}", mask);
            println!(
                "Tables: {}, Registry: {}",
                world.tables.len(),
                world.table_registry.len()
            );
            for (i, table) in world.tables.iter().enumerate() {
                println!(
                    "Table {}: mask={:b}, entities={}",
                    i,
                    table.mask,
                    table.entity_indices.len()
                );
            }
            entities.push(entity);
        }

        // Remove entities one by one, checking integrity
        for entity in entities {
            println!("\nBefore despawning entity {:?}", entity);
            println!(
                "Tables: {}, Registry: {}",
                world.tables.len(),
                world.table_registry.len()
            );

            despawn_entities(&mut world, &[entity]);

            let result = validate_world_integrity(&world);
            if result.is_err() {
                println!("World state when validation failed:");
                println!("Number of tables: {}", world.tables.len());
                for (i, table) in world.tables.iter().enumerate() {
                    println!(
                        "Table {}: mask={:b}, entities={}",
                        i,
                        table.mask,
                        table.entity_indices.len()
                    );
                }
                println!("Registry entries:");
                for (i, (mask, index)) in world.table_registry.iter().enumerate() {
                    println!("Registry {}: mask={:b}, points to table {}", i, mask, index);
                }
            }
            result.expect("World invalid after despawn");

            println!("After despawn:");
            println!(
                "Tables: {}, Registry: {}",
                world.tables.len(),
                world.table_registry.len()
            );

            // Verify all remaining entities are still accessible
            for table in &world.tables {
                for &e in &table.entity_indices {
                    assert!(
                        location_get(&world.entity_locations, e).is_some(),
                        "Entity {:?} location invalid after despawning {:?}",
                        e,
                        entity
                    );
                }
            }

            // Verify table registry matches actual tables
            assert_eq!(
                world.table_registry.len(),
                world.tables.len(),
                "Registry length {} doesn't match table count {}",
                world.table_registry.len(),
                world.tables.len()
            );

            for (mask, index) in &world.table_registry {
                assert!(
                    *index < world.tables.len(),
                    "Registry points to invalid table {} (max {})",
                    index,
                    world.tables.len() - 1
                );
                assert_eq!(
                    world.tables[*index].mask, *mask,
                    "Table {} has wrong mask: expected {:b}, got {:b}",
                    index, mask, world.tables[*index].mask
                );
            }
        }
    }

    #[test]
    fn test_table_management_during_component_add() {
        let mut world = World::default();

        println!("\nInitial world state:");
        println!("Tables: {}", world.tables.len());

        // Create entities with different component combinations
        let e1 = spawn_entities(&mut world, POSITION, 1)[0];
        let e2 = spawn_entities(&mut world, POSITION, 1)[0];
        let e3 = spawn_entities(&mut world, POSITION, 1)[0];

        println!("\nAfter spawning 3 entities with POSITION:");
        println!("Tables: {}", world.tables.len());
        for (i, table) in world.tables.iter().enumerate() {
            println!(
                "Table {}: mask={:b}, entities={}",
                i,
                table.mask,
                table.entity_indices.len()
            );
        }

        // Set initial values to track data preservation
        if let Some(pos) = get_component_mut::<Position>(&mut world, e1, POSITION) {
            pos.x = 1.0;
            pos.y = 2.0;
        }

        // Initial state checks
        assert_eq!(world.tables.len(), 1, "Should have one table initially");
        assert_eq!(
            world.tables[0].entity_indices.len(),
            3,
            "First table should have 3 entities"
        );
        assert_eq!(
            world.tables[0].mask, POSITION,
            "First table should have POSITION mask"
        );

        // Add VELOCITY to first entity
        add_components(&mut world, e1, VELOCITY);

        println!("\nAfter adding VELOCITY to e1:");
        println!("Tables: {}", world.tables.len());
        for (i, table) in world.tables.iter().enumerate() {
            println!(
                "Table {}: mask={:b}, entities={}",
                i,
                table.mask,
                table.entity_indices.len()
            );
        }

        // Verify entity moved to new table
        assert_eq!(
            world.tables.len(),
            2,
            "Should have two tables after adding VELOCITY"
        );

        // Find tables with each mask
        let pos_table = world.tables.iter().find(|t| t.mask == POSITION).unwrap();
        let pos_vel_table = world
            .tables
            .iter()
            .find(|t| t.mask == (POSITION | VELOCITY))
            .unwrap();

        assert_eq!(
            pos_table.entity_indices.len(),
            2,
            "POSITION table should have 2 entities"
        );
        assert_eq!(
            pos_vel_table.entity_indices.len(),
            1,
            "POSITION|VELOCITY table should have 1 entity"
        );

        // Verify data preserved
        let pos = get_component::<Position>(&world, e1, POSITION).unwrap();
        assert_eq!(pos.x, 1.0);
        assert_eq!(pos.y, 2.0);

        // Move second entity to same table as first
        add_components(&mut world, e2, VELOCITY);

        println!("\nAfter adding VELOCITY to e2:");
        println!("Tables: {}", world.tables.len());
        for (i, table) in world.tables.iter().enumerate() {
            println!(
                "Table {}: mask={:b}, entities={}",
                i,
                table.mask,
                table.entity_indices.len()
            );
        }

        // Remove VELOCITY from both entities
        remove_components(&mut world, e1, VELOCITY);
        remove_components(&mut world, e2, VELOCITY);

        println!("\nAfter removing VELOCITY from e1 and e2:");
        println!("Tables: {}", world.tables.len());
        for (i, table) in world.tables.iter().enumerate() {
            println!(
                "Table {}: mask={:b}, entities={}",
                i,
                table.mask,
                table.entity_indices.len()
            );
        }

        // Add VELOCITY back to all entities
        add_components(&mut world, e1, VELOCITY);
        add_components(&mut world, e2, VELOCITY);
        add_components(&mut world, e3, VELOCITY);

        println!("\nAfter adding VELOCITY to all entities:");
        println!("Tables: {}", world.tables.len());
        for (i, table) in world.tables.iter().enumerate() {
            println!(
                "Table {}: mask={:b}, entities={}",
                i,
                table.mask,
                table.entity_indices.len()
            );
        }

        // Verify tables merged correctly
        assert_eq!(
            world.tables.len(),
            1,
            "Should have merged back to one table"
        );
        assert_eq!(
            world.tables[0].entity_indices.len(),
            3,
            "All entities should be in the same table"
        );
        assert_eq!(
            world.tables[0].mask,
            POSITION | VELOCITY,
            "Table should have both components"
        );

        // Verify all entity locations are valid
        for entity in [e1, e2, e3] {
            let location = world.entity_locations.locations[entity.id as usize];
            assert!(
                location.is_some(),
                "Entity {:?} has invalid location",
                entity
            );
            let (table_index, array_index) = location.unwrap();
            assert!(
                table_index < world.tables.len(),
                "Entity {:?} points to invalid table {} (max {})",
                entity,
                table_index,
                world.tables.len() - 1
            );
            let table = &world.tables[table_index];
            assert!(
                array_index < table.entity_indices.len(),
                "Entity {:?} points to invalid index {} in table {} (length {})",
                entity,
                array_index,
                table_index,
                table.entity_indices.len()
            );
        }

        // Verify table registry is correct
        assert_eq!(
            world.table_registry.len(),
            world.tables.len(),
            "Table registry size mismatch: registry={}, tables={}",
            world.table_registry.len(),
            world.tables.len()
        );

        for (mask, table_index) in &world.table_registry {
            assert!(
                *table_index < world.tables.len(),
                "Registry points to invalid table {} (max {})",
                table_index,
                world.tables.len() - 1
            );
            assert_eq!(
                world.tables[*table_index].mask, *mask,
                "Registry mask mismatch: registry={:b}, table={:b}",
                mask, world.tables[*table_index].mask
            );
        }
    }
}
