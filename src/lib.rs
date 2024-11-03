//! # freecs
//!
//! `freecs` is a high-performance, archetype-based Entity Component System (ECS) for Rust
//! that emphasizes parallel processing and cache-friendly data layouts. It provides a simple,
//! macro-based API for defining and managing game worlds with components.
//!
//! ## Features
//!
//! - Archetype-based storage for optimal cache coherency
//! - Built-in parallel processing support
//! - Zero-overhead component access
//! - Type-safe component management
//! - Dynamic component addition and removal
//! - Automatic table defragmentation
//! - Batch entity operations
//! - Serialization support via serde
//!
//! ## Quick Start
//!
//! ```rust
//! use freecs::{impl_world, has_components};
//!
//! // Define your components
//! #[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
//! struct Position {
//!     x: f32,
//!     y: f32,
//! }
//!
//! #[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
//! struct Velocity {
//!     x: f32,
//!     y: f32,
//! }
//!
//! // Create a world with your components
//! impl_world! {
//!     World {
//!         positions: Position => POSITION,
//!         velocities: Velocity => VELOCITY,
//!     }
//! }
//!
//! // Create a new world
//! let mut world = World::default();
//!
//! // Spawn entities with components
//! let entity = spawn_entities(&mut world, POSITION | VELOCITY, 1)[0];
//! ```
//!
//! ## Parallel Systems Example
//!
//! ```rust
//! use rayon::prelude::*;
//!
//! fn run_systems(world: &mut World, dt: f32) {
//!     world.tables.par_iter_mut().for_each(|table| {
//!         if has_components!(table, POSITION | VELOCITY) {
//!             update_positions(&mut table.positions, &table.velocities, dt);
//!         }
//!     });
//! }
//!
//! #[inline]
//! fn update_positions(positions: &mut [Position], velocities: &[Velocity], dt: f32) {
//!     positions
//!         .par_iter_mut()
//!         .zip(velocities.par_iter())
//!         .for_each(|(pos, vel)| {
//!             pos.x += vel.x * dt;
//!             pos.y += vel.y * dt;
//!         });
//! }
//! ```
//!
//! ## Component Access
//!
//! ```rust
//! // Get component
//! if let Some(pos) = get_component::<Position>(&world, entity, POSITION) {
//!     println!("Position: ({}, {})", pos.x, pos.y);
//! }
//!
//! // Modify component
//! if let Some(vel) = get_component_mut::<Velocity>(&mut world, entity, VELOCITY) {
//!     vel.x += 1.0;
//! }
//! ```
//!
//! ## Entity Management
//!
//! ```rust
//! // Spawn multiple entities
//! let entities = spawn_entities(&mut world, POSITION | VELOCITY, 1000);
//!
//! // Add components to existing entity
//! add_components(&mut world, entity, NEW_COMPONENT);
//!
//! // Remove components
//! remove_components(&mut world, entity, VELOCITY);
//!
//! // Despawn entities
//! despawn_entities(&mut world, &entities);
//! ```
//!
//! ## Performance Notes
//!
//! The library is designed for high performance with the following features:
//!
//! - Archetype tables store entities with identical component layouts together
//! - Parallel iteration over components using rayon
//! - Contiguous memory layout for cache-friendly access
//! - Zero-cost component access through direct slice indexing
//! - Efficient batch operations for entity management
//!
//! For optimal performance:
//!
//! ```rust
//! // Periodically merge tables to prevent fragmentation
//! merge_tables(&mut world);
//!
//! // Use batch operations when possible
//! let entities = spawn_entities(&mut world, POSITION | VELOCITY, 1000);
//!
//! // Leverage parallel systems for large worlds
//! world.tables.par_iter_mut().for_each(|table| {
//!     // Process components in parallel
//! });
//! ```
//!
//! ## Safety
//!
//! The library uses several mechanisms to ensure safety:
//!
//! - Generational indices prevent use-after-free bugs
//! - Type-safe component access through generics
//! - Automatic table consistency maintenance
//! - Safe parallel access through rayon's parallel iterators
//!
//! ## Example Game Architecture
//!
//! Here's a typical game architecture using freecs:
//!
//! ```rust
//! mod components {
//!     #[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
//!     pub struct Position3D { pub x: f32, pub y: f32, pub z: f32 }
//!
//!     #[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
//!     pub struct Rotation { pub x: f32, pub y: f32, pub z: f32 }
//!
//!     #[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
//!     pub struct Velocity { pub x: f32, pub y: f32, pub z: f32 }
//! }
//!
//! impl_world! {
//!     World {
//!         positions: Position3D => POSITION,
//!         rotations: Rotation => ROTATION,
//!         velocities: Velocity => VELOCITY,
//!     }
//! }
//!
//! mod systems {
//!     pub fn run_systems(world: &mut World, dt: f32) {
//!         world.tables.par_iter_mut().for_each(|table| {
//!             if has_components!(table, POSITION | VELOCITY) {
//!                 movement_system(&mut table.positions, &table.velocities, dt);
//!             }
//!             if has_components!(table, ROTATION) {
//!                 rotation_system(&mut table.rotations, dt);
//!             }
//!         });
//!     }
//! }
//!
//! fn game_loop(world: &mut World) {
//!     let dt = get_frame_time();
//!     systems::run_systems(world, dt);
//!     // Render entities...
//! }
//! ```
//!
//! ## Implementation Details
//!
//! The core of freecs is built around:
//!
//! - **Archetype Tables**: Store entities with identical component layouts
//! - **Component Arrays**: Contiguous storage for each component type
//! - **Entity Locations**: Fast entity-to-component lookup
//! - **Component Masks**: Bitflags for component combinations
//! - **Generational Indices**: Safe entity reference management
//!
//! Table operations maintain consistency by:
//!
//! 1. Updating entity locations when components change
//! 2. Managing table creation and merging automatically
//! 3. Handling entity despawning with proper cleanup
//! 4. Maintaining generation counters for safety
//!
//! ## Serialization
//!
//! All core types implement `serde::Serialize` and `serde::Deserialize`:
//!
//! ```rust
//! // Save world state
//! let serialized = serde_json::to_string(&world)?;
//!
//! // Load world state
//! let mut world: World = serde_json::from_str(&serialized)?;
//! ```
#[macro_export]
macro_rules! impl_world {
    (
        $world:ident {
            $($name:ident: $type:ty => $mask:ident),* $(,)?
        }
    ) => {

        /// Component masks
        #[repr(u32)]
        #[allow(clippy::upper_case_acronyms)]
        #[allow(non_camel_case_types)]
        enum ComponentIndex {
            $($mask,)*
        }

        $(pub const $mask: u32 = 1 << (ComponentIndex::$mask as u32);)*

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
        pub fn query_entities(world: &World, mask: u32) -> Vec<EntityId> {
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
        pub fn query_first_entity(world: &World, mask: u32) -> Option<EntityId> {
            for table in &world.tables {
                if table.mask & mask == mask {
                    return table.entity_indices.first().copied();
                }
            }
            None
        }

        /// Set a specific component for an entity
        pub fn set_component<T: 'static>(world: &mut World, entity: EntityId, mask: u32, component: T) {
           if let Some((table_idx, array_idx)) = location_get(&world.entity_locations, entity) {
               let table = &mut world.tables[table_idx];

               if table.mask & mask == 0 {
                   return;
               }

               $(
                   if mask == $mask && std::any::TypeId::of::<T>() == std::any::TypeId::of::<$type>() {
                       // SAFETY: We've verified that T is exactly $type via TypeId,
                       // and we know the component exists in the table because we checked the mask.
                       // The pointer cast is valid because they are the same type and properly aligned.
                       unsafe { *(&mut table.$name[array_idx] as *mut $type as *mut T) = component; }
                       return;
                   }
               )*
           }
        }

        /// Get a specific component for an entity
        pub fn get_component<T: 'static>(world: &World, entity: EntityId, mask: u32) -> Option<&T> {
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
        pub fn get_component_mut<T: 'static>(world: &mut World, entity: EntityId, mask: u32) -> Option<&mut T> {
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
        pub fn despawn_entities(world: &mut World, entities: &[EntityId]) -> Vec<EntityId> {
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
        pub fn add_components(world: &mut World, entity: EntityId, mask: u32) -> bool {
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
        pub fn remove_components(world: &mut World, entity: EntityId, mask: u32) -> bool {
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
        pub fn component_mask(world: &World, entity: EntityId) -> Option<u32> {
            location_get(&world.entity_locations, entity)
                .map(|(table_idx, _)| world.tables[table_idx].mask)
        }

        /// Merge tables that have the same mask. Call this periodically, such as every 60 frames.
        pub fn merge_tables(world: &mut World) {
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
        pub fn total_entities(world: &World) -> usize {
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

        fn create_entity(world: &mut World) -> EntityId {
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

        fn get_or_create_table(world: &mut World, mask: u32) -> usize {
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
