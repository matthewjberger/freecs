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

        // Handles allocation and reuse of entity IDs
        #[derive(Default, serde::Serialize, serde::Deserialize)]
        pub struct EntityAllocator {
            next_id: u32,
            free_ids: Vec<(u32, u32)>, // (id, next_generation)
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
            pub allocator: EntityAllocator,
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
            use std::collections::HashMap;

            let mut despawned = Vec::new();
            let mut table_removals: HashMap<usize, Vec<usize>> = HashMap::new();

            // Process entities in order they were passed in
            for &entity in entities {
                if let Some((table_index, array_index)) = location_get(&world.entity_locations, entity) {
                    table_removals.entry(table_index)
                        .or_insert_with(Vec::new)
                        .push(array_index);

                    let id = entity.id as usize;
                    if id < world.entity_locations.locations.len() {
                        world.entity_locations.locations[id] = None;

                        let current_generation = world.entity_locations.generations[id];
                        // Increment generation
                        let next_generation = if current_generation == u32::MAX {
                            1  // Skip 0 to avoid ABA issues
                        } else {
                            current_generation.wrapping_add(1)
                        };
                        world.entity_locations.generations[id] = next_generation;

                        // Push onto free list
                        world.allocator.free_ids.push((entity.id, next_generation));

                        despawned.push(entity);
                    }
                }
            }

        // Handle table removals
        let mut table_index = 0;
        while table_index < world.tables.len() {
            if let Some(mut indices) = table_removals.remove(&table_index) {
                indices.sort_unstable_by(|a, b| b.cmp(a));

                for &index in &indices {
                    remove_from_table(&mut world.tables[table_index], index);
                }

                if world.tables[table_index].entity_indices.is_empty() {
                    let last_index = world.tables.len() - 1;

                    if table_index != last_index {
                        let removed_mask = world.tables[table_index].mask;
                        let swapped_mask = world.tables[last_index].mask;

                        world.tables.swap_remove(table_index);

                        world.table_registry.retain(|(mask, _)| *mask != removed_mask);
                        if let Some(entry) = world.table_registry.iter_mut()
                            .find(|(mask, _)| *mask == swapped_mask) {
                            entry.1 = table_index;
                        }

                        for location in world.entity_locations.locations.iter_mut() {
                            if let Some((ref mut index, _)) = location {
                                if *index == last_index {
                                    *index = table_index;
                                }
                            }
                        }
                        continue;
                    } else {
                        let removed_mask = world.tables[table_index].mask;
                        world.tables.pop();
                        world.table_registry.retain(|(mask, _)| *mask != removed_mask);
                    }
                } else {
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
        pub fn add_components(world: &mut $world, entity: EntityId, mask: u32) -> bool {
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
            world: &mut $world,
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
            let id = entity.id as usize;
            if id >= locations.generations.len() {
                return None;
            }

            // Validate generation matches
            if locations.generations[id] != entity.generation {
                return None;
            }

            if id >= locations.locations.len() {
                return None;
            }

            locations.locations[id]
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
            // Always reuse latest freed id if available through LIFO
            if let Some((id, next_gen)) = world.allocator.free_ids.pop() {
                let id_usize = id as usize;

                // Ensure space
                while id_usize >= world.entity_locations.generations.len() {
                    let new_size = (world.entity_locations.generations.len() * 2).max(64);
                    world.entity_locations.generations.resize(new_size, 0);
                    world.entity_locations.locations.resize(new_size, None);
                }

                // Update generation
                world.entity_locations.generations[id_usize] = next_gen;
                EntityId { id, generation: next_gen }
            } else {
                // Allocate new id
                let id = world.allocator.next_id;
                world.allocator.next_id += 1;
                let id_usize = id as usize;

                // Ensure space
                while id_usize >= world.entity_locations.generations.len() {
                    let new_size = (world.entity_locations.generations.len() * 2).max(64);
                    world.entity_locations.generations.resize(new_size, 0);
                    world.entity_locations.locations.resize(new_size, None);
                }

                EntityId {
                    id,
                    generation: 0,
                }
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
