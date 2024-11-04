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
//! // Resources are stored independently of component data.
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
//!
//! # Optional Performance Tips
//!
//! - Call `merge_tables(&mut world)` periodically to combine tables with identical layouts, boosting iteration performance
//! - Group commonly accessed components at entity creation, rather than adding them at runtime to reduce copying entities between archtype tables
//! - Leverage parallel iteration for large datasets
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

        /// Entity location cache for quick access
        #[derive(Default, serde::Serialize, serde::Deserialize)]
        pub struct EntityLocations {
            pub generations: Vec<u32>,
            pub locations: Vec<Option<(usize, usize)>>,
        }

        /// A collection of component tables
        #[derive(Default, serde::Serialize, serde::Deserialize)]
        pub struct $world {
            pub entity_locations: EntityLocations,
            pub tables: Vec<ComponentArrays>,
            pub next_entity_id: u32,
            pub table_registry: Vec<(u32, usize)>,
            pub resources: $resources,
        }

        /// Resources
        #[derive(Default, serde::Serialize, serde::Deserialize)]
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
                    (
                        table_index,
                        world.tables[table_index].entity_indices.len() - 1,
                    ),
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
           let (table_idx, array_idx) = location_get(&world.entity_locations, entity)?;
           let table = &world.tables[table_idx];

           if table.mask & mask == 0 {
               return None;
           }

           $(
               if mask == $mask && std::any::TypeId::of::<T>() == std::any::TypeId::of::<$type>() {
                   // SAFETY: This operation is safe because:
                   // 1. We verify the component type T exactly matches $type via TypeId
                   // 2. We confirm the table contains this component via mask check
                   // 3. array_idx is valid from location_get bounds check
                   // 4. The reference is valid for the lifetime of the return value
                   //    because it's tied to the table reference lifetime
                   // 5. No mutable aliases can exist during the shared borrow
                   // 6. The type cast maintains proper alignment as types are identical
                   return Some(unsafe { &*(&table.$name[array_idx] as *const $type as *const T) });
               }
           )*

           None
        }

        /// Get a mutable reference to a specific component for an entity
        pub fn get_component_mut<T: 'static>(world: &mut $world, entity: EntityId, mask: u32) -> Option<&mut T> {
            let (table_idx, array_idx) = location_get(&world.entity_locations, entity)?;
            let table = &mut world.tables[table_idx];

            if table.mask & mask == 0 {
                return None;
            }

            $(
                if mask == $mask && std::any::TypeId::of::<T>() == std::any::TypeId::of::<$type>() {
                    // SAFETY: This operation is safe because:
                    // 1. We verify the component type T exactly matches $type via TypeId
                    // 2. We confirm the table contains this component via mask check
                    // 3. array_idx is valid from location_get bounds check
                    // 4. We have exclusive access through the mutable borrow
                    // 5. The borrow checker ensures no other references exist
                    // 6. The pointer cast is valid as we verified the types are identical
                    // 7. Proper alignment is maintained as the types are the same
                    return Some(unsafe { &mut *(&mut table.$name[array_idx] as *mut $type as *mut T) });
                }
            )*

            None
        }

        /// Despawn a batch of entities
        pub fn despawn_entities(world: &mut $world, entities: &[EntityId]) -> Vec<EntityId> {
            use std::collections::{HashMap, HashSet};

            // Deduplicate entities to prevent double-removal issues
            let entities: HashSet<_> = entities.iter().copied().collect();
            let mut despawned = Vec::new();

            // Track which tables need cleanup
            let mut table_removals: HashMap<usize, Vec<usize>> = HashMap::new();
            let mut tables_to_check: HashSet<usize> = HashSet::new();

            // First pass: collect all removals and mark entities as despawned
            for &entity in &entities {
                if let Some((table_idx, array_idx)) = location_get(&world.entity_locations, entity) {
                    table_removals.entry(table_idx)
                        .or_insert_with(Vec::new)
                        .push(array_idx);

                    tables_to_check.insert(table_idx);

                    // Clear the entity's location and increment generation
                    let id = entity.id as usize;
                    if id < world.entity_locations.locations.len() {
                        world.entity_locations.locations[id] = None;
                        world.entity_locations.generations[id] =
                            world.entity_locations.generations[id].wrapping_add(1);
                        despawned.push(entity);
                    }
                }
            }

            // Second pass: perform removals for each table
            for table_idx in tables_to_check {
                if let Some(mut indices) = table_removals.remove(&table_idx) {
                    // Sort indices in descending order for safe swap_remove
                    indices.sort_unstable_by(|a, b| b.cmp(a));

                    let table = &mut world.tables[table_idx];
                    for &index in &indices {
                        remove_from_table(table, index);
                    }

                    // Update locations for remaining valid entities that may have been moved
                    for (new_idx, &entity) in table.entity_indices.iter().enumerate() {
                        if !entities.contains(&entity) {
                            // Only update location if entity wasn't despawned
                            location_insert(
                                &mut world.entity_locations,
                                entity,
                                (table_idx, new_idx)
                            );
                        }
                    }
                }
            }

            // Clean up empty tables
            let mut i = 0;
            while i < world.tables.len() {
                if world.tables[i].entity_indices.is_empty() {
                    world.tables.swap_remove(i);
                    // Update table registry
                    world.table_registry.retain(|(_, table_idx)| *table_idx != i);
                    for (_, table_idx) in world.table_registry.iter_mut() {
                        if *table_idx > i {
                            *table_idx -= 1;
                        }
                    }
                } else {
                    i += 1;
                }
            }

            despawned
        }

        /// Add components to an entity
        pub fn add_components(world: &mut $world, entity: EntityId, mask: u32) -> bool {
            if let Some((table_idx, array_idx)) = location_get(&world.entity_locations, entity) {
                let current_mask = world.tables[table_idx].mask;
                // If entity already has all these components, no need to move
                if current_mask & mask == mask {
                    return true;
                }

                let new_mask = current_mask | mask;
                let new_table_idx = get_or_create_table(world, new_mask);
                move_entity(world, entity, table_idx, array_idx, new_table_idx);
                true
            } else {
                false
            }
        }

        /// Remove components from an entity
        pub fn remove_components(world: &mut $world, entity: EntityId, mask: u32) -> bool {
            if let Some((table_idx, array_idx)) = location_get(&world.entity_locations, entity) {
                let current_mask = world.tables[table_idx].mask;
                // If entity doesn't have any of these components, no need to move
                if current_mask & mask == 0 {
                    return true;
                }

                let new_mask = current_mask & !mask;
                let new_table_idx = get_or_create_table(world, new_mask);
                move_entity(world, entity, table_idx, array_idx, new_table_idx);
                true
            } else {
                false
            }
        }

        /// Get the current component mask for an entity
        pub fn component_mask(world: &$world, entity: EntityId) -> Option<u32> {
            location_get(&world.entity_locations, entity)
                .map(|(table_idx, _)| world.tables[table_idx].mask)
        }

        /// Merge tables that have the same mask
        pub fn merge_tables(world: &mut $world) {
            let mut moves = Vec::new();

            // Collect all moves first to avoid holding references while mutating
            {
                let mut mask_to_tables = std::collections::HashMap::new();
                for (i, table) in world.tables.iter().enumerate() {
                    mask_to_tables
                        .entry(table.mask)
                        .or_insert_with(Vec::new)
                        .push(i);
                }

                for tables in mask_to_tables.values() {
                    if tables.len() <= 1 {
                        continue;
                    }

                    let target_idx = tables[0];
                    for &source_idx in &tables[1..] {
                        let source = &world.tables[source_idx];
                        for (i, &entity) in source.entity_indices.iter().enumerate() {
                            if let Some((table_idx, _array_idx)) =
                                location_get(&world.entity_locations, entity)
                            {
                                if table_idx == source_idx {
                                    moves.push((entity, source_idx, i, target_idx));
                                }
                            }
                        }
                    }
                }
            }

            // Now perform all moves
            for (entity, source_idx, array_idx, target_idx) in moves {
                move_entity(world, entity, source_idx, array_idx, target_idx);
            }

            // Clean up empty tables
            let mut i = 0;
            while i < world.tables.len() {
                if world.tables[i].entity_indices.is_empty() {
                    world.tables.swap_remove(i);
                    world
                        .table_registry
                        .retain(|(_, table_idx)| *table_idx != i);
                    for (_, table_idx) in world.table_registry.iter_mut() {
                        if *table_idx > i {
                            *table_idx -= 1;
                        }
                    }
                } else {
                    i += 1;
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
            world: &mut $world,
            entity: EntityId,
            from_table: usize,
            from_index: usize,
            to_table: usize,
        ) {
            let ($($name,)*) =
                get_components(&world.tables[from_table], from_index);
            remove_from_table(&mut world.tables[from_table], from_index);

            let dst = &mut world.tables[to_table];
            add_to_table(dst, entity, ($($name,)*));

            location_insert(
                &mut world.entity_locations,
                entity,
                (to_table, dst.entity_indices.len() - 1),
            );
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

        fn create_entity(world: &mut $world) -> EntityId {
            let id = world.next_entity_id;
            world.next_entity_id += 1;

            let generation = if id as usize >= world.entity_locations.generations.len() {
                0
            } else {
                world.entity_locations.generations[id as usize]
            };

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
            if let Some(idx) = world.table_registry.iter().position(|(m, _)| *m == mask) {
                return world.table_registry[idx].1;
            }

            world.tables.push(ComponentArrays {
                mask,
                ..Default::default()
            });
            let table_idx = world.tables.len() - 1;
            world.table_registry.push((mask, table_idx));
            table_idx
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
              delta_time: f32
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
}
