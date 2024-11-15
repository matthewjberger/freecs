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
//! // The `World` and `Resources` type names can be customized.
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
//!
//! }
//!
//! // Add new components to an entity by mask
//! add_components(&mut world, entity, HEALTH | VELOCITY);
//!
//! // Remove components from an entity by mask
//! remove_components(&mut world, entity, VELOCITY | POSITION);
//!
//! // Query all entities
//! let entities = query_entities(&world, ALL);
//! println!("All entities: {entities:?}");
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
//! Parallelization of systems can be done with Rayon,
//! which is useful when working with more than 3 million entities.
//! In practice, you should use `.iter_mut()` instead of `.par_iter_mut()`
//! unless you have a large number of entities, because sequential access
//! is more performant until you are working with extreme numbers of entities.
//!
//! The example function below invokes two systems in parallel
//! for each table in the world, filtered by component mask.
//!
//! ```rust
//! pub fn run_systems(world: &mut World, dt: f32) {
//!     use rayon::prelude::*;
//!
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
//! # Prefabs
//!
//! Prefabs allow you to define reusable entity templates with remapped entity references:
//!
//! ```rust
//! // Create a prefab world with parent-child hierarchy
//! let mut prefab = World::default();
//! let parent = spawn_entities(&mut prefab, POSITION, 1)[0];
//! let child = spawn_entities(&mut prefab, POSITION | PARENT, 1)[0];
//!
//! // Set up parent relationship
//! if let Some(parent_component) = get_component_mut::<Parent>(&mut prefab, child, PARENT) {
//!     parent_component.0 = parent; // Store reference to parent entity
//! }
//!
//! // Instance the prefab, remapping the parent reference to the new entity ID
//! let mut game_world = World::default();
//! let mapping = copy_entities(&mut game_world, &prefab, &[parent, child],
//!     |mapping, source_table, dest_table| {
//!         // Remap any Parent component references
//!         if has_components!(source_table, PARENT) {
//!             for (i, parent_comp) in dest_table.parent.iter_mut().enumerate() {
//!                 if let Some((_, new_id)) = mapping.iter()
//!                     .find(|(old_id, _)| *old_id == source_table.parent[i].0)
//!                 {
//!                     parent_comp.0 = *new_id; // Update to reference new parent entity
//!                 }
//!             }
//!         }
//!     }
//! );
//! ```
//!
//! The system will remap entity references when copying prefabs, maintaining hierarchical relationships
//! between entities while ensuring all references point to the newly created entities.
//!
//! # Performance
//!
//! The table-based design means entities with the same components are stored together in contiguous
//! memory, maximizing cache utilization. Component access and queries are O(1), with table transitions
//! being the only O(n) operations.
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
            All,
        }

        pub const ALL: u32 = 0;
        $(pub const $mask: u32 = 1 << (Component::$mask as u32);)*

        pub const COMPONENT_COUNT: usize = { Component::All as usize };

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

        // Handles allocation and reuse of entity IDs
        #[derive(Default, serde::Serialize, serde::Deserialize)]
        pub struct EntityAllocator {
            next_id: u32,
            free_ids: Vec<(u32, u32)>, // (id, next_generation)
        }

        #[derive(Copy, Clone, Default, serde::Serialize, serde::Deserialize)]
        struct EntityLocation {
            generation: u32,
            table_index: u16,
            array_index: u16,
            allocated: bool,
        }

        /// Entity location cache for quick access
        #[derive(Default, serde::Serialize, serde::Deserialize)]
        pub struct EntityLocations {
            locations: Vec<EntityLocation>,
        }

        /// A collection of component tables and resources
        #[derive(Default, serde::Serialize, serde::Deserialize)]
        pub struct $world {
            pub entity_locations: EntityLocations,
            pub tables: Vec<ComponentArrays>,
            pub allocator: EntityAllocator,
            #[serde(skip)]
            #[allow(unused)]
            pub resources: $resources,
            table_edges: Vec<TableEdges>,
            pending_despawns: Vec<EntityId>,
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

        #[derive(Copy, Clone, Default, serde::Serialize, serde::Deserialize)]
        struct TableEdges {
            add_edges: [Option<usize>; COMPONENT_COUNT],
            remove_edges: [Option<usize>; COMPONENT_COUNT],
        }

        fn get_component_index(mask: u32) -> Option<usize> {
            match mask {
                $($mask => Some(Component::$mask as _),)*
                _ => None,
            }
        }

        /// Spawn a batch of new entities with the same component mask
        pub fn spawn_entities(world: &mut $world, mask: u32, count: usize) -> Vec<EntityId> {
            let mut entities = Vec::with_capacity(count);
            let table_index = get_or_create_table(world, mask);

            world.tables[table_index].entity_indices.reserve(count);

            // Reserve space in components
            $(
                if mask & $mask != 0 {
                    world.tables[table_index].$name.reserve(count);
                }
            )*

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
            let total_capacity = world
                .tables
                .iter()
                .filter(|table| table.mask & mask == mask)
                .map(|table| table.entity_indices.len())
                .sum();

            let mut result = Vec::with_capacity(total_capacity);
            for table in &world.tables {
                if table.mask & mask == mask {
                    // Only include allocated entities
                    result.extend(
                        table
                            .entity_indices
                            .iter()
                            .copied()
                            .filter(|&e| world.entity_locations.locations[e.id as usize].allocated),
                    );
                }
            }
            result
        }

        /// Query for the first entity that matches the component mask
        /// Returns as soon as a match is found, instead of running for all entities
        pub fn query_first_entity(world: &$world, mask: u32) -> Option<EntityId> {
            for table in &world.tables {
                if !has_components!(table, mask) {
                    continue;
                }
                let indices = table
                    .entity_indices
                    .iter()
                    .copied()
                    .filter(|&e| world.entity_locations.locations[e.id as usize].allocated)
                    .collect::<Vec<_>>();
                if let Some(entity) = indices.first() {
                    return Some(*entity);
                }
            }
            None
        }

        /// Get a specific component for an entity
        pub fn get_component<T: 'static>(world: &$world, entity: EntityId, mask: u32) -> Option<&T> {
           let (table_index, array_index) = location_get(&world.entity_locations, entity)?;

           // Early return if entity is despawned
           if !world.entity_locations.locations[entity.id as usize].allocated {
               return None;
           }

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
            let mut despawned = Vec::with_capacity(entities.len());
            let mut tables_to_update = Vec::new();

            // First pass: mark entities as despawned and collect their table locations
            for &entity in entities {
                let id = entity.id as usize;
                if id < world.entity_locations.locations.len() {
                    let loc = &mut world.entity_locations.locations[id];
                    if loc.allocated && loc.generation == entity.generation {
                        // Get table info before marking as despawned
                        let table_idx = loc.table_index as usize;
                        let array_idx = loc.array_index as usize;

                        // Mark as despawned
                        loc.allocated = false;
                        loc.generation = loc.generation.wrapping_add(1);
                        world.allocator.free_ids.push((entity.id, loc.generation));

                        // Collect table info for updates
                        tables_to_update.push((table_idx, array_idx));
                        despawned.push(entity);
                    }
                }
            }

            // Second pass: remove entities from tables in reverse order to maintain indices
            for (table_idx, array_idx) in tables_to_update.into_iter().rev() {
                if table_idx >= world.tables.len() {
                    continue;
                }

                let table = &mut world.tables[table_idx];
                let last_idx = table.entity_indices.len() - 1;

                // If we're not removing the last element, update the moved entity's location
                if array_idx < last_idx {
                    let moved_entity = table.entity_indices[last_idx];
                    if let Some(loc) = world.entity_locations.locations.get_mut(moved_entity.id as usize) {
                        if loc.allocated {
                            loc.array_index = array_idx as u16;
                        }
                    }
                }

                // Remove the entity's components
                $(
                    if table.mask & $mask != 0 {
                        table.$name.swap_remove(array_idx);
                    }
                )*
                table.entity_indices.swap_remove(array_idx);
            }

            despawned
        }

        /// Add components to an entity
        pub fn add_components(world: &mut $world, entity: EntityId, mask: u32) -> bool {
            if let Some((table_index, array_index)) = location_get(&world.entity_locations, entity) {
                let current_mask = world.tables[table_index].mask;
                if current_mask & mask == mask {
                    return true;
                }

                let target_table = if mask.count_ones() == 1 {
                    get_component_index(mask).and_then(|idx| world.table_edges[table_index].add_edges[idx])
                } else {
                    None
                };

                let new_table_index =
                    target_table.unwrap_or_else(|| get_or_create_table(world, current_mask | mask));

                move_entity(world, entity, table_index, array_index, new_table_index);
                true
            } else {
                false
            }
        }

        /// Remove components from an entity
        pub fn remove_components(world: &mut $world, entity: EntityId, mask: u32) -> bool {
            if let Some((table_index, array_index)) = location_get(&world.entity_locations, entity) {
                let current_mask = world.tables[table_index].mask;
                if current_mask & mask == 0 {
                    return true;
                }

                let target_table = if mask.count_ones() == 1 {
                    get_component_index(mask)
                        .and_then(|idx| world.table_edges[table_index].remove_edges[idx])
                } else {
                    None
                };

                let new_table_index =
                    target_table.unwrap_or_else(|| get_or_create_table(world, current_mask & !mask));

                move_entity(world, entity, table_index, array_index, new_table_index);
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

        /// Copy entities from source world to destination world
        pub fn copy_entities<T: FnMut(&[(EntityId, EntityId)], &ComponentArrays, &mut ComponentArrays)>(
           dest: &mut $world,
           source: &$world,
           entities: &[EntityId],
           mut remap: T
        ) -> Vec<(EntityId, EntityId)> {
           let mut entity_mapping = Vec::with_capacity(entities.len());
           let mut table_groups: std::collections::HashMap<usize, Vec<(EntityId, usize)>> = std::collections::HashMap::new();

           for &entity in entities {
               if let Some((table_idx, array_idx)) = location_get(&source.entity_locations, entity) {
                   if table_idx < source.tables.len() {
                       table_groups.entry(table_idx)
                           .or_default()
                           .push((entity, array_idx));
                   }
               }
           }

           // Create all entities first to build complete mapping table
           for (source_table_idx, entities_to_copy) in &table_groups {
               let source_table = &source.tables[*source_table_idx];
               let entity_mask = source_table.mask;
               let count = entities_to_copy.len();
               let new_entities = spawn_entities(dest, entity_mask, count);

               for ((old_entity, _), new_entity) in entities_to_copy.iter().zip(new_entities) {
                   entity_mapping.push((*old_entity, new_entity));
               }
           }

           // Process tables and remap with complete mapping table
           for (source_table_idx, entities_to_copy) in table_groups {
               let source_table = &source.tables[source_table_idx];
               let entity_mask = source_table.mask;

               // Create temp table and copy valid components
               let mut temp_table = ComponentArrays {
                   mask: entity_mask,
                   ..Default::default()
               };

               // Determine valid components by checking array lengths
               let valid_mask = {
                   let mut mask = entity_mask;
                   $(
                       if mask & $mask != 0 && source_table.$name.len() != source_table.entity_indices.len() {
                           mask &= !$mask;
                       }
                   )*
                   mask
               };

               // Copy data using valid mask
               for (_, source_idx) in &entities_to_copy {
                   if *source_idx < source_table.entity_indices.len() {
                       $(
                           if valid_mask & $mask != 0 {
                               temp_table.$name.push(source_table.$name[*source_idx].clone());
                           }
                       )*
                   }
               }

               // Remap references using complete mapping table
               remap(&entity_mapping, source_table, &mut temp_table);

               // Copy remapped data to destination
               if let Some(dest_table) = dest.tables.last_mut() {
                   let start_idx = dest_table.entity_indices.len() - entities_to_copy.len();

                   // Copy components
                   for i in 0..entities_to_copy.len() {
                       $(
                           if valid_mask & $mask != 0 {
                               dest_table.$name[start_idx + i] = temp_table.$name[i].clone();
                           }
                       )*
                   }
               }
           }

           entity_mapping
        }

        fn remove_from_table(arrays: &mut ComponentArrays, index: usize) -> Option<EntityId> {
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
            entity: EntityId,
            from_table: usize,
            from_index: usize,
            to_table: usize,
        ) {
            let components = get_components(&world.tables[from_table], from_index);
            add_to_table(&mut world.tables[to_table], entity, components);
            let new_index = world.tables[to_table].entity_indices.len() - 1;
            location_insert(&mut world.entity_locations, entity, (to_table, new_index));

            if let Some(swapped) = remove_from_table(&mut world.tables[from_table], from_index) {
                location_insert(
                    &mut world.entity_locations,
                    swapped,
                    (from_table, from_index),
                );
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
            let id = entity.id as usize;
            if id >= locations.locations.len() {
                return None;
            }

            let location = &locations.locations[id];
            // Only return location if entity is allocated AND generation matches
            if !location.allocated || location.generation != entity.generation {
                return None;
            }

            Some((location.table_index as usize, location.array_index as usize))        }

        fn location_insert(
            locations: &mut EntityLocations,
            entity: EntityId,
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
                table_index: location.0 as u16,
                array_index: location.1 as u16,
                allocated: true,
            };
        }

        fn create_entity(world: &mut $world) -> EntityId {
            if let Some((id, next_gen)) = world.allocator.free_ids.pop() {
                let id_usize = id as usize;
                if id_usize >= world.entity_locations.locations.len() {
                    world.entity_locations.locations.resize(
                        (world.entity_locations.locations.len() * 2).max(64),
                        EntityLocation::default(),
                    );
                }
                world.entity_locations.locations[id_usize].generation = next_gen;
                EntityId {
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
                EntityId { id, generation: 0 }
            }
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
            if let Some((index, _)) = world
                .tables
                .iter()
                .enumerate()
                .find(|(_, t)| t.mask == mask)
            {
                return index;
            }

            let table_index = world.tables.len();
            world.tables.push(ComponentArrays {
                mask,
                ..Default::default()
            });
            world.table_edges.push(TableEdges::default());

            // Remove table registry updates and only update edges
            for comp_mask in [
                $($mask,)*
            ] {
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
            parent: Parent => PARENT,
            node: Node => NODE,
          },
          Resources {
              _delta_time: f32
          }
      }
    }

    use components::*;
    mod components {
        use super::*;

        #[derive(Default, Debug, Copy, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
        pub struct Parent(pub EntityId);

        #[derive(Default, Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
        pub struct Node {
            pub id: EntityId,
            pub parent: Option<EntityId>,
            pub children: Vec<EntityId>,
        }

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

        #[derive(Default, Debug, Clone, serde::Serialize, serde::Deserialize)]
        pub struct EntityRefs {
            pub parent: EntityId,
            pub children: Vec<EntityId>,
        }
    }

    mod systems {
        use super::*;

        // Systems are functions that iterate over
        // the component tables and transform component data.
        // This function invokes two systems in parallel
        // for each table in the world filtered by component mask.
        pub fn run_systems(world: &mut World, dt: f32) {
            world.tables.iter_mut().for_each(|table| {
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
        assert_eq!(query_entities(&world, ALL).len(), 3);

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
        assert_eq!(query_entities(&world, ALL).len(), 3);

        // Despawn one entity
        let despawned = despawn_entities(&mut world, &[entities[1]]);
        assert_eq!(despawned.len(), 1);
        assert_eq!(query_entities(&world, ALL).len(), 2);

        // Verify the entity is truly despawned
        assert!(get_component::<Position>(&world, entities[1], POSITION).is_none());

        // Verify other entities still exist
        assert!(get_component::<Position>(&world, entities[0], POSITION).is_some());
        assert!(get_component::<Position>(&world, entities[2], POSITION).is_some());
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
    fn test_table_cleanup_after_despawn() {
        let mut world = World::default();

        // Create entities with different component combinations
        let e1 = spawn_entities(&mut world, POSITION, 1)[0];
        let e2 = spawn_entities(&mut world, POSITION | VELOCITY, 1)[0];

        // Record initial table count
        let initial_tables = world.tables.len();
        assert_eq!(initial_tables, 2, "Should have two tables initially");

        // Despawn entity with unique component combination
        despawn_entities(&mut world, &[e2]);

        // Verify entity properly despawned
        assert!(get_component::<Position>(&world, e2, POSITION).is_none());
        assert!(get_component::<Velocity>(&world, e2, VELOCITY).is_none());

        // Verify first entity still accessible
        assert!(get_component::<Position>(&world, e1, POSITION).is_some());

        // Verify remaining table has correct entities
        let remaining = query_entities(&world, POSITION);
        assert_eq!(remaining.len(), 1);
        assert!(remaining.contains(&e1));

        // Verify tables were properly cleaned up
        assert!(
            world.tables.len() <= initial_tables,
            "Should not have more tables than initial state"
        );

        // Verify all remaining entities have valid locations
        for table in &world.tables {
            for &entity in &table.entity_indices {
                assert!(
                    location_get(&world.entity_locations, entity).is_some(),
                    "Entity location should be valid for remaining entities"
                );
            }
        }
    }

    #[test]
    fn test_concurrent_entity_references() {
        let mut world = World::default();

        // Create two entities
        let entity1 = spawn_entities(&mut world, POSITION | HEALTH, 1)[0];
        let entity2 = spawn_entities(&mut world, POSITION | HEALTH, 1)[0];

        // Set up some initial data
        if let Some(pos) = get_component_mut::<Position>(&mut world, entity1, POSITION) {
            pos.x = 1.0;
        }
        if let Some(health) = get_component_mut::<Health>(&mut world, entity1, HEALTH) {
            health.value = 100.0;
        }

        // Store entity1's ID for later
        let id1 = entity1.id;

        // Despawn entity1
        despawn_entities(&mut world, &[entity1]);

        // Create new entity with same ID but different generation
        let entity3 = spawn_entities(&mut world, POSITION | HEALTH, 1)[0];
        assert_eq!(entity3.id, id1, "Should reuse entity1's ID");
        assert_eq!(
            entity3.generation,
            entity1.generation + 1,
            "Should have incremented generation"
        );

        // Set different data for entity3
        if let Some(pos) = get_component_mut::<Position>(&mut world, entity3, POSITION) {
            pos.x = 3.0;
        }
        if let Some(health) = get_component_mut::<Health>(&mut world, entity3, HEALTH) {
            health.value = 50.0;
        }

        // Verify entity2 is unaffected by entity1's despawn and entity3's spawn
        if let Some(pos) = get_component::<Position>(&world, entity2, POSITION) {
            assert_eq!(pos.x, 0.0, "Entity2's data should be unchanged");
        }

        // Verify we can't access entity1's old data through entity3's ID
        if let Some(pos) = get_component::<Position>(&world, entity3, POSITION) {
            assert_eq!(pos.x, 3.0, "Should get entity3's data, not entity1's");
        }
        assert!(
            get_component::<Position>(&world, entity1, POSITION).is_none(),
            "Should not be able to access entity1's old data"
        );
    }

    #[test]
    fn test_generational_indices_aba() {
        let mut world = World::default();

        // Create an initial entity with Position
        let entity_a1 = spawn_entities(&mut world, POSITION, 1)[0];
        assert_eq!(
            entity_a1.generation, 0,
            "First use of ID should have generation 0"
        );

        // Set initial position
        if let Some(pos) = get_component_mut::<Position>(&mut world, entity_a1, POSITION) {
            pos.x = 1.0;
            pos.y = 1.0;
        }

        // Store the ID for later reuse
        let id = entity_a1.id;

        // Despawn the entity
        despawn_entities(&mut world, &[entity_a1]);

        // Create a new entity that reuses the same ID (entity A2)
        let entity_a2 = spawn_entities(&mut world, POSITION, 1)[0];
        assert_eq!(entity_a2.id, id, "Should reuse the same ID");
        assert_eq!(
            entity_a2.generation, 1,
            "Second use of ID should have generation 1"
        );

        // Set different position for A2
        if let Some(pos) = get_component_mut::<Position>(&mut world, entity_a2, POSITION) {
            pos.x = 2.0;
            pos.y = 2.0;
        }

        // Verify that the old reference (A1) is invalid
        assert!(
            get_component::<Position>(&world, entity_a1, POSITION).is_none(),
            "Old reference to entity should be invalid"
        );

        // Despawn A2
        despawn_entities(&mut world, &[entity_a2]);

        // Create another entity with the same ID (entity A3)
        let entity_a3 = spawn_entities(&mut world, POSITION, 1)[0];
        assert_eq!(entity_a3.id, id, "Should reuse the same ID again");
        assert_eq!(
            entity_a3.generation, 2,
            "Third use of ID should have generation 2"
        );

        // Set different position for A3
        if let Some(pos) = get_component_mut::<Position>(&mut world, entity_a3, POSITION) {
            pos.x = 3.0;
            pos.y = 3.0;
        }

        // Verify that both old references are invalid
        assert!(
            get_component::<Position>(&world, entity_a1, POSITION).is_none(),
            "First generation reference should be invalid"
        );
        assert!(
            get_component::<Position>(&world, entity_a2, POSITION).is_none(),
            "Second generation reference should be invalid"
        );

        // Verify that the current reference is valid and has the correct data
        let pos = get_component::<Position>(&world, entity_a3, POSITION);
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

        // Create entities with different component combinations
        let e1 = spawn_entities(&mut world, POSITION, 1)[0];
        let e2 = spawn_entities(&mut world, POSITION | VELOCITY, 1)[0];
        let e3 = spawn_entities(&mut world, POSITION | HEALTH, 1)[0];
        let e4 = spawn_entities(&mut world, POSITION | VELOCITY | HEALTH, 1)[0];

        // Get all entities
        let all = query_entities(&world, ALL);

        // Verify count
        assert_eq!(all.len(), 4, "Should have 4 total entities");

        // Verify all entities are present
        assert!(all.contains(&e1), "Missing entity 1");
        assert!(all.contains(&e2), "Missing entity 2");
        assert!(all.contains(&e3), "Missing entity 3");
        assert!(all.contains(&e4), "Missing entity 4");

        // Test after despawning
        despawn_entities(&mut world, &[e2, e3]);
        let remaining = query_entities(&world, ALL);

        // Verify count after despawn
        assert_eq!(remaining.len(), 2, "Should have 2 entities after despawn");

        // Verify correct entities remain
        assert!(remaining.contains(&e1), "Missing entity 1 after despawn");
        assert!(remaining.contains(&e4), "Missing entity 4 after despawn");
        assert!(!remaining.contains(&e2), "Entity 2 should be despawned");
        assert!(!remaining.contains(&e3), "Entity 3 should be despawned");
    }

    #[test]
    fn test_all_entities_empty_world() {
        assert!(
            query_entities(&World::default(), ALL).is_empty(),
            "Empty world should return empty vector"
        );
    }

    #[test]
    fn test_all_entities_after_table_merges() {
        let mut world = World::default();

        // Create entities that will end up in the same table
        let e1 = spawn_entities(&mut world, POSITION, 1)[0];
        let e2 = spawn_entities(&mut world, VELOCITY, 1)[0];

        // Add components to force table merges
        add_components(&mut world, e1, VELOCITY);
        add_components(&mut world, e2, POSITION);

        let all = query_entities(&world, ALL);
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

        // Create entity with 3 components
        let entity = spawn_entities(&mut world, POSITION | VELOCITY | HEALTH, 1)[0];

        println!(
            "Initial mask: {:b}",
            component_mask(&world, entity).unwrap()
        );

        // Get indices before transition
        let (old_table_idx, _) = location_get(&world.entity_locations, entity).unwrap();

        // Add new component
        add_components(&mut world, entity, POSITION); // Try to add one we already have

        let final_mask = component_mask(&world, entity).unwrap();
        println!("Final mask: {:b}", final_mask);
        let (new_table_idx, _) = location_get(&world.entity_locations, entity).unwrap();

        // Print the table info
        println!(
            "Old table index: {}, New table index: {}",
            old_table_idx, new_table_idx
        );
        println!("Tables after operation:");
        for (i, table) in world.tables.iter().enumerate() {
            println!("Table {}: mask={:b}", i, table.mask);
        }

        // Verify the entity still has all its components
        assert_eq!(
            final_mask & (POSITION | VELOCITY | HEALTH),
            POSITION | VELOCITY | HEALTH,
            "Entity should still have all original components"
        );
    }

    #[test]
    fn test_real_camera_scenario() {
        let mut world = World::default();

        // Create camera entity with same components as in your app
        let entity = spawn_entities(
            &mut world,
            POSITION | VELOCITY | HEALTH, // Simulating your camera's many components
            1,
        )[0];

        // Basic query should work
        let query_results = query_entities(&world, POSITION | VELOCITY);
        assert!(
            query_results.contains(&entity),
            "Initial query should match\n\
                Entity mask: {:b}\n\
                Query mask: {:b}",
            component_mask(&world, entity).unwrap(),
            POSITION | VELOCITY
        );

        // Add another component (like LIGHT in your case)
        add_components(&mut world, entity, HEALTH);

        // Query should still work
        let query_results = query_entities(&world, POSITION | VELOCITY);
        assert!(
            query_results.contains(&entity),
            "Query should still match after adding component\n\
                Entity mask: {:b}\n\
                Query mask: {:b}",
            component_mask(&world, entity).unwrap(),
            POSITION | VELOCITY
        );
    }

    #[test]
    fn test_table_transitions_with_light() {
        let mut world = World::default();

        // Create entity with camera-like setup (multiple components)
        let entity = spawn_entities(&mut world, POSITION | VELOCITY | HEALTH, 1)[0];
        println!(
            "Initial mask: {:b}",
            component_mask(&world, entity).unwrap()
        );

        // Verify query works pre-transition
        let query_mask = POSITION | VELOCITY; // Like ACTIVE_CAMERA | LOCAL_TRANSFORM | PLAYER
        let results = query_entities(&world, query_mask);
        assert!(
            results.contains(&entity),
            "Pre-transition query should work"
        );

        // Add a new component (like LIGHT)
        println!("Before adding new component...");
        println!(
            "Entity components: {:b}",
            component_mask(&world, entity).unwrap()
        );
        println!(
            "Current table mask: {:b}",
            world.tables[location_get(&world.entity_locations, entity).unwrap().0].mask
        );

        add_components(&mut world, entity, HEALTH); // Like adding LIGHT

        println!("After adding new component...");
        println!(
            "Entity components: {:b}",
            component_mask(&world, entity).unwrap()
        );
        let (table_idx, _) = location_get(&world.entity_locations, entity).unwrap();
        println!("New table mask: {:b}", world.tables[table_idx].mask);

        // Query should still work post-transition
        let results = query_entities(&world, query_mask);
        assert!(
            results.contains(&entity),
            "Post-transition query should still work\nQuery mask: {:b}\nEntity mask: {:b}",
            query_mask,
            component_mask(&world, entity).unwrap()
        );
    }

    #[test]
    fn test_query_consistency() {
        let mut world = World::default();

        // Create entity with extra components beyond what we'll query for
        let entity = spawn_entities(&mut world, POSITION | VELOCITY | HEALTH, 1)[0];

        // Query for subset of components
        let query_mask = POSITION | VELOCITY;

        // Normal query should work
        let query_results = query_entities(&world, query_mask);
        assert!(
            query_results.contains(&entity),
            "query_entities should find entity with mask {:b} when querying for {:b}",
            component_mask(&world, entity).unwrap(),
            query_mask
        );

        // First entity query should also work
        let first_result = query_first_entity(&world, query_mask);
        assert!(
            first_result.is_some(),
            "query_first_entity should find entity with mask {:b} when querying for {:b}",
            component_mask(&world, entity).unwrap(),
            query_mask
        );
        assert_eq!(
            first_result.unwrap(),
            entity,
            "query_first_entity should find same entity as query_entities"
        );

        // Add another component and test again
        add_components(&mut world, entity, HEALTH);

        let query_results = query_entities(&world, query_mask);
        assert!(
            query_results.contains(&entity),
            "query_entities should still find entity after adding component\n\
            Entity mask: {:b}\n\
            Query mask: {:b}",
            component_mask(&world, entity).unwrap(),
            query_mask
        );

        let first_result = query_first_entity(&world, query_mask);
        assert!(
            first_result.is_some(),
            "query_first_entity should still find entity after adding component\n\
            Entity mask: {:b}\n\
            Query mask: {:b}",
            component_mask(&world, entity).unwrap(),
            query_mask
        );
    }

    #[test]
    fn test_copy_entities() {
        let mut source = World::default();
        let mut dest = World::default();

        assert_eq!(
            query_entities(&source, ALL).len(),
            0,
            "Source world should start empty"
        );
        assert_eq!(
            query_entities(&dest, ALL).len(),
            0,
            "Dest world should start empty"
        );

        // Create source entities with parent relationships
        let parent = spawn_entities(&mut source, POSITION | PARENT, 1)[0];
        let child = spawn_entities(&mut source, POSITION | PARENT, 1)[0];

        assert_eq!(
            query_entities(&source, ALL).len(),
            2,
            "Source should have 2 entities"
        );

        // Setup initial component values
        if let Some(pos) = get_component_mut::<Position>(&mut source, parent, POSITION) {
            pos.x = 1.0;
            pos.y = 2.0;
        }
        if let Some(parent_comp) = get_component_mut::<Parent>(&mut source, child, PARENT) {
            parent_comp.0 = parent;
        }

        // Verify source world setup
        let source_parent_pos = get_component::<Position>(&source, parent, POSITION)
            .expect("Parent position should exist in source");
        assert_eq!(source_parent_pos.x, 1.0);
        assert_eq!(source_parent_pos.y, 2.0);

        let source_child_parent = get_component::<Parent>(&source, child, PARENT)
            .expect("Child parent ref should exist in source");
        assert_eq!(source_child_parent.0, parent);

        // Copy entities and remap references
        let mapping = copy_entities(
            &mut dest,
            &source,
            &[parent, child],
            |mapping, source_table, dest_table| {
                if has_components!(source_table, PARENT) {
                    // Remap Parent component references
                    for (i, parent_comp) in dest_table.parent.iter_mut().enumerate() {
                        if let Some((_, new_id)) = mapping
                            .iter()
                            .find(|(old_id, _)| *old_id == source_table.parent[i].0)
                        {
                            parent_comp.0 = *new_id;
                        }
                    }
                }
            },
        );

        // Verify mapping
        assert_eq!(mapping.len(), 2, "Should map exactly two entities");
        assert!(
            mapping.iter().any(|(old, _)| *old == parent),
            "Mapping should contain parent"
        );
        assert!(
            mapping.iter().any(|(old, _)| *old == child),
            "Mapping should contain child"
        );

        // Get the new entity IDs
        let new_parent = mapping
            .iter()
            .find(|(old, _)| *old == parent)
            .expect("Parent mapping should exist")
            .1;
        let new_child = mapping
            .iter()
            .find(|(old, _)| *old == child)
            .expect("Child mapping should exist")
            .1;

        // Verify dest world state
        assert_eq!(
            query_entities(&dest, ALL).len(),
            2,
            "Dest should have 2 entities"
        );

        let all_dest_entities = query_entities(&dest, ALL);
        assert!(
            all_dest_entities.contains(&new_parent),
            "New parent should exist in dest"
        );
        assert!(
            all_dest_entities.contains(&new_child),
            "New child should exist in dest"
        );

        // Verify positions copied correctly
        let parent_pos = get_component::<Position>(&dest, new_parent, POSITION)
            .expect("Parent position should exist in dest");
        assert_eq!(parent_pos.x, 1.0);
        assert_eq!(parent_pos.y, 2.0);

        // Verify parent relationship was correctly remapped
        let child_parent = get_component::<Parent>(&dest, new_child, PARENT)
            .expect("Child parent ref should exist in dest");
        assert_eq!(
            child_parent.0, new_parent,
            "Child should reference new parent ID"
        );

        // Verify component masks match
        assert_eq!(
            component_mask(&dest, new_parent).expect("New parent should have components"),
            component_mask(&source, parent).expect("Source parent should have components"),
            "Parent component masks should match"
        );
        assert_eq!(
            component_mask(&dest, new_child).expect("New child should have components"),
            component_mask(&source, child).expect("Source child should have components"),
            "Child component masks should match"
        );
    }

    #[test]
    fn test_prefab_instantiation() {
        let mut base_world = World::default();

        // Create some entities in base world first so we get different IDs
        let dummy = spawn_entities(&mut base_world, POSITION, 5);
        despawn_entities(&mut base_world, &dummy); // This ensures next IDs will be different
        assert_eq!(query_entities(&base_world, ALL).len(), 0);

        let mut prefab = World::default();

        // Create hierarchy in prefab
        let prefab_parent = spawn_entities(&mut prefab, POSITION | PARENT, 1)[0];
        let prefab_child1 = spawn_entities(&mut prefab, POSITION | PARENT, 1)[0];
        let prefab_child2 = spawn_entities(&mut prefab, POSITION | PARENT, 1)[0];

        // Set positions and relationships in prefab
        if let Some(pos) = get_component_mut::<Position>(&mut prefab, prefab_parent, POSITION) {
            pos.x = 10.0;
            pos.y = 20.0;
        }

        if let Some(parent) = get_component_mut::<Parent>(&mut prefab, prefab_child1, PARENT) {
            parent.0 = prefab_parent;
        }
        if let Some(parent) = get_component_mut::<Parent>(&mut prefab, prefab_child2, PARENT) {
            parent.0 = prefab_parent;
        }

        // Verify prefab relationships
        let child1_prefab_parent = get_component::<Parent>(&prefab, prefab_child1, PARENT).unwrap();
        let child2_prefab_parent = get_component::<Parent>(&prefab, prefab_child2, PARENT).unwrap();
        assert_eq!(child1_prefab_parent.0, prefab_parent);
        assert_eq!(child2_prefab_parent.0, prefab_parent);

        // Print entity IDs for debugging
        println!("Prefab parent: {:?}", prefab_parent);
        println!("Prefab child1: {:?}", prefab_child1);
        println!("Prefab child2: {:?}", prefab_child2);

        let prefab_entities = &[prefab_parent, prefab_child1, prefab_child2];
        let mapping = copy_entities(
            &mut base_world,
            &prefab,
            prefab_entities,
            |mapping, source_table, dest_table| {
                // Print the mapping table for debugging
                println!("Mapping table: {:?}", mapping);

                if has_components!(source_table, PARENT) {
                    for (i, parent_comp) in dest_table.parent.iter_mut().enumerate() {
                        println!("Checking parent component: {:?}", parent_comp);
                        if let Some((_, new_id)) = mapping
                            .iter()
                            .find(|(old_id, _)| *old_id == source_table.parent[i].0)
                        {
                            println!("Remapping from {:?} to {:?}", parent_comp.0, new_id);
                            parent_comp.0 = *new_id;
                        }
                    }
                }
            },
        );

        // Get new entity IDs
        let new_parent = mapping
            .iter()
            .find(|(old, _)| *old == prefab_parent)
            .unwrap()
            .1;
        let new_child1 = mapping
            .iter()
            .find(|(old, _)| *old == prefab_child1)
            .unwrap()
            .1;
        let new_child2 = mapping
            .iter()
            .find(|(old, _)| *old == prefab_child2)
            .unwrap()
            .1;

        // Print new IDs for debugging
        println!("New parent: {:?}", new_parent);
        println!("New child1: {:?}", new_child1);
        println!("New child2: {:?}", new_child2);

        let child1_parent = get_component::<Parent>(&base_world, new_child1, PARENT).unwrap();
        let child2_parent = get_component::<Parent>(&base_world, new_child2, PARENT).unwrap();

        println!("Child1's parent ID: {:?}", child1_parent.0);
        println!("Child2's parent ID: {:?}", child2_parent.0);

        // These assertions should now pass because IDs will be different
        assert_eq!(
            child1_parent.0, new_parent,
            "Child1's parent should reference the new parent entity"
        );
        assert_ne!(
            child1_parent.0, prefab_parent,
            "Child1's parent should not reference the prefab parent entity"
        );

        assert_eq!(
            child2_parent.0, new_parent,
            "Child2's parent should reference the new parent entity"
        );
        assert_ne!(
            child2_parent.0, prefab_parent,
            "Child2's parent should not reference the prefab parent entity"
        );
    }

    #[test]
    fn test_entity_reference_remapping() {
        let mut source = World::default();
        let mut dest = World::default();

        // Create some entities in base world to ensure different IDs
        let dummy = spawn_entities(&mut dest, POSITION, 5);
        despawn_entities(&mut dest, &dummy);
        assert_eq!(query_entities(&dest, ALL).len(), 0);

        // Create entity hierarchy in source world
        let parent = spawn_entities(&mut source, POSITION | PARENT, 1)[0];
        let child = spawn_entities(&mut source, POSITION | PARENT, 1)[0];

        // Record initial entity positions
        let parent_start_pos = Position { x: 1.0, y: 2.0 };
        let child_start_pos = Position { x: 3.0, y: 4.0 };

        // Set up components - position and parent reference
        if let Some(pos) = get_component_mut::<Position>(&mut source, parent, POSITION) {
            *pos = parent_start_pos.clone();
        }
        if let Some(pos) = get_component_mut::<Position>(&mut source, child, POSITION) {
            *pos = child_start_pos.clone();
        }
        if let Some(parent_comp) = get_component_mut::<Parent>(&mut source, child, PARENT) {
            parent_comp.0 = parent;
        }

        // Verify source world setup
        let source_parent_pos = get_component::<Position>(&source, parent, POSITION).unwrap();
        let source_child_pos = get_component::<Position>(&source, child, POSITION).unwrap();
        let source_child_parent = get_component::<Parent>(&source, child, PARENT).unwrap();

        println!("\nBefore copy:");
        println!(
            "Source parent: {:?}, position: {:?}",
            parent, source_parent_pos
        );
        println!(
            "Source child: {:?}, position: {:?}, parent ref: {:?}",
            child, source_child_pos, source_child_parent
        );

        // Copy entities and remap
        let mapping = copy_entities(
            &mut dest,
            &source,
            &[parent, child],
            |mapping, source_table, dest_table| {
                println!("\nRemapping table:");
                println!("Source table mask: {:b}", source_table.mask);
                println!("Mapping table: {:?}", mapping);

                if source_table.mask & PARENT != 0 {
                    for i in 0..dest_table.parent.len() {
                        let old_parent_id = source_table.parent[i].0;
                        if let Some((_, new_id)) =
                            mapping.iter().find(|(old_id, _)| *old_id == old_parent_id)
                        {
                            dest_table.parent[i].0 = *new_id;
                        }
                    }
                }
            },
        );

        println!("\nEntity mapping: {:?}", mapping);

        // Get the new entity IDs
        let new_parent = mapping.iter().find(|(old, _)| *old == parent).unwrap().1;
        let new_child = mapping.iter().find(|(old, _)| *old == child).unwrap().1;

        println!("\nAfter copy:");
        println!("New parent: {:?}", new_parent);
        println!("New child: {:?}", new_child);

        // Verify positions were copied correctly
        let new_parent_pos = get_component::<Position>(&dest, new_parent, POSITION).unwrap();
        assert_eq!(new_parent_pos.x, parent_start_pos.x);
        assert_eq!(new_parent_pos.y, parent_start_pos.y);

        let new_child_pos = get_component::<Position>(&dest, new_child, POSITION).unwrap();
        assert_eq!(new_child_pos.x, child_start_pos.x);
        assert_eq!(new_child_pos.y, child_start_pos.y);

        // Verify parent reference was remapped
        let new_child_parent = get_component::<Parent>(&dest, new_child, PARENT).unwrap();
        println!(
            "Child's parent reference: {:?}, should be: {:?}",
            new_child_parent.0, new_parent
        );

        assert_eq!(
            new_child_parent.0, new_parent,
            "Child should reference new parent entity"
        );
        assert_ne!(
            new_child_parent.0, parent,
            "Child should not reference old parent entity"
        );

        // Verify world integrity
        assert_eq!(
            query_entities(&dest, ALL).len(),
            2,
            "Destination world should have exactly two entities"
        );

        let dest_entities = query_entities(&dest, ALL);
        assert!(
            dest_entities.contains(&new_parent),
            "New parent entity should exist in destination world"
        );
        assert!(
            dest_entities.contains(&new_child),
            "New child entity should exist in destination world"
        );
    }

    #[test]
    fn test_deep_hierarchy_copy() {
        let mut source = World::default();
        let mut dest = World::default();

        // Create a very deep hierarchy in source world
        let mut prev_entity = None;
        let mut all_entities = Vec::new();

        // Create 10000 nested entities with parent references
        for _ in 0..10000 {
            let entity = spawn_entities(&mut source, PARENT | NODE, 1)[0];

            if let Some(prev) = prev_entity {
                if let Some(parent) = get_component_mut::<Parent>(&mut source, entity, PARENT) {
                    parent.0 = prev;
                }
            }

            all_entities.push(entity);
            prev_entity = Some(entity);
        }

        // Copy the entire hierarchy
        let mapping = copy_entities(
            &mut dest,
            &source,
            &all_entities,
            |mapping, source_table, dest_table| {
                if source_table.mask & PARENT != 0 {
                    for i in 0..dest_table.parent.len() {
                        if let Some((_, new_id)) = mapping
                            .iter()
                            .find(|(old_id, _)| *old_id == source_table.parent[i].0)
                        {
                            dest_table.parent[i].0 = *new_id;
                        }
                    }
                }
            },
        );

        assert_eq!(mapping.len(), 10000);

        // Verify parent relationships were maintained
        for (old_id, new_id) in mapping.iter().skip(1) {
            let old_parent = get_component::<Parent>(&source, *old_id, PARENT).unwrap();
            let new_parent = get_component::<Parent>(&dest, *new_id, PARENT).unwrap();

            let expected_new_parent = mapping
                .iter()
                .find(|(old, _)| *old == old_parent.0)
                .unwrap()
                .1;

            assert_eq!(new_parent.0, expected_new_parent);
        }
    }

    #[test]
    fn test_copy_entities_component_array_bounds() {
        let mut source = World::default();
        let mut dest = World::default();

        let e1 = spawn_entities(&mut source, POSITION | PARENT, 1)[0];

        // Print initial state
        if let Some((table_idx, _)) = location_get(&source.entity_locations, e1) {
            let table = &mut source.tables[table_idx];
            println!(
                "Before clear: position len={}, parent len={}",
                table.position.len(),
                table.parent.len()
            );

            table.parent.clear();

            println!(
                "After clear: position len={}, parent len={}",
                table.position.len(),
                table.parent.len()
            );
        }

        println!("Entity to copy: {:?}", e1);

        let mapping = copy_entities(&mut dest, &source, &[e1], |_, _, _| {});
        println!("Mapping: {:?}", mapping);

        // Check resulting entities
        let dest_entities = query_entities(&dest, ALL);
        println!("Destination entities: {:?}", dest_entities);

        for e in dest_entities {
            println!(
                "Entity {:?} components: mask={:?}",
                e,
                component_mask(&dest, e)
            );
        }
    }
}
