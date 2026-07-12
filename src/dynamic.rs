//! Runtime-registered components over the same archetype kernel.
//!
//! The `ecs!` macro fixes the component set at compile time and generates a
//! typed accessor per name. This module is the other entry point: component
//! types register at runtime, storage is still one contiguous `Vec<T>` per
//! component per archetype, and dynamic dispatch is confined to the places
//! runtime registration makes unavoidable. Structural changes go through a
//! per-type table of plain function pointers, and typed access crosses one
//! safe `Any` downcast per column per table. Query inner loops run over
//! concrete slices, the same machine code the macro produces.
//!
//! Nothing here uses `unsafe`. Columns are erased as whole `Vec<T>` values
//! behind `Box<dyn Any + Send + Sync>`, never as raw bytes, so `Drop` and
//! thread-safety come from the vec itself. The boundary this buys is
//! bring-your-own *Rust* types at runtime; component layouts described only
//! by data (an editor or script schema with no Rust type behind it) are out
//! of scope and would require byte-level erasure.
//!
//! # Quick start
//!
//! ```rust
//! use freecs::dynamic::DynWorld;
//!
//! #[derive(Default, Clone, Debug, PartialEq)]
//! struct Position { x: f32, y: f32 }
//!
//! #[derive(Default, Clone, Debug)]
//! struct Velocity { x: f32, y: f32 }
//!
//! let mut world = DynWorld::new();
//!
//! // Types register lazily on first use; tuples spawn as bundles.
//! let entity = world.spawn((
//!     Position { x: 0.0, y: 0.0 },
//!     Velocity { x: 1.0, y: 2.0 },
//! ));
//!
//! world
//!     .query::<(&mut Position, &Velocity)>()
//!     .for_each(|_entity, (position, velocity)| {
//!         position.x += velocity.x;
//!         position.y += velocity.y;
//!     });
//!
//! assert_eq!(world.get::<Position>(entity), Some(&Position { x: 1.0, y: 2.0 }));
//! ```
//!
//! Registration is a schema: mask bits are assigned in registration order, so
//! anything serializing masks should register components deterministically,
//! or build one [`ComponentRegistry`] up front and construct worlds from it.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::{
    ArchetypeEdges, ArchetypeRouting, Entity, EntityAllocator, EntityLocation, EntityLocations,
    EventChannel, STRUCTURAL_LOG_CAPACITY, SparseTagSet, StructuralChange, StructuralChangeKind,
    archetype_cached_tables, archetype_register_table, tick_is_newer,
};

static NEXT_REGISTRY_ID: AtomicU32 = AtomicU32::new(1);

type ErasedColumn = Box<dyn Any + Send + Sync>;

fn column_new<T: Send + Sync + Default + 'static>() -> ErasedColumn {
    Box::new(Vec::<T>::new())
}

fn column_vec<T: 'static>(column: &(dyn Any + Send + Sync)) -> &Vec<T> {
    column
        .downcast_ref::<Vec<T>>()
        .expect("column type does not match its registered component")
}

fn column_vec_mut<T: 'static>(column: &mut (dyn Any + Send + Sync)) -> &mut Vec<T> {
    column
        .downcast_mut::<Vec<T>>()
        .expect("column type does not match its registered component")
}

fn column_push_default<T: Send + Sync + Default + 'static>(
    column: &mut (dyn Any + Send + Sync),
    count: usize,
) {
    let column = column_vec_mut::<T>(column);
    column.reserve(count);
    for _ in 0..count {
        column.push(T::default());
    }
}

fn column_swap_remove<T: Send + Sync + Default + 'static>(
    column: &mut (dyn Any + Send + Sync),
    index: usize,
) {
    column_vec_mut::<T>(column).swap_remove(index);
}

fn column_move_row<T: Send + Sync + Default + 'static>(
    source: &mut (dyn Any + Send + Sync),
    index: usize,
    destination: &mut (dyn Any + Send + Sync),
) {
    let value = std::mem::take(&mut column_vec_mut::<T>(source)[index]);
    column_vec_mut::<T>(destination).push(value);
}

/// The per-type operations a column needs, as a plain record of function
/// pointers captured at registration. This is the vtable, visible as data.
#[derive(Clone, Copy)]
pub struct ComponentInfo {
    pub type_id: TypeId,
    pub type_name: &'static str,
    pub mask: u64,
    pub new_column: fn() -> ErasedColumn,
    pub push_default: fn(&mut (dyn Any + Send + Sync), usize),
    pub swap_remove: fn(&mut (dyn Any + Send + Sync), usize),
    pub move_row: fn(&mut (dyn Any + Send + Sync), usize, &mut (dyn Any + Send + Sync)),
}

/// A typed handle to a registered component: the component's index, its mask
/// bit, and the registry it belongs to. Copyable plain data; holding one
/// skips the `TypeId` lookup the lazy typed API pays per call.
pub struct ComponentKey<T> {
    pub component_index: u32,
    pub mask: u64,
    pub registry_id: u32,
    marker: PhantomData<fn() -> T>,
}

impl<T> Clone for ComponentKey<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for ComponentKey<T> {}

/// A handle to a registered tag. Tag mask bits are assigned from the top of
/// the `u64` downward, so they never collide with component bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TagKey {
    pub tag_index: u32,
    pub mask: u64,
    pub registry_id: u32,
}

/// The component and tag schema for dynamic worlds. Bits are assigned in
/// registration order, so a registry built once and shared across worlds
/// guarantees every world agrees on masks.
#[derive(Clone)]
pub struct ComponentRegistry {
    pub registry_id: u32,
    pub components: Vec<ComponentInfo>,
    pub components_by_type: HashMap<TypeId, u32>,
    pub tag_count: u32,
}

impl Default for ComponentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ComponentRegistry {
    pub fn new() -> Self {
        Self {
            registry_id: NEXT_REGISTRY_ID.fetch_add(1, Ordering::Relaxed),
            components: Vec::new(),
            components_by_type: HashMap::new(),
            tag_count: 0,
        }
    }

    /// Registers `T` if it is not already registered and returns its key.
    /// Idempotent per type.
    pub fn register<T: Send + Sync + Default + 'static>(&mut self) -> ComponentKey<T> {
        if let Some(&component_index) = self.components_by_type.get(&TypeId::of::<T>()) {
            return self.key_for(component_index);
        }

        let component_index = self.components.len() as u32;
        assert!(
            (self.components.len() + self.tag_count as usize) < 64,
            "components plus tags must fit in a u64 mask"
        );
        self.components.push(ComponentInfo {
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            mask: 1 << component_index,
            new_column: column_new::<T>,
            push_default: column_push_default::<T>,
            swap_remove: column_swap_remove::<T>,
            move_row: column_move_row::<T>,
        });
        self.components_by_type
            .insert(TypeId::of::<T>(), component_index);
        self.key_for(component_index)
    }

    pub fn register_tag(&mut self) -> TagKey {
        assert!(
            (self.components.len() + self.tag_count as usize) < 64,
            "components plus tags must fit in a u64 mask"
        );
        let tag_index = self.tag_count;
        self.tag_count += 1;
        TagKey {
            tag_index,
            mask: 1 << (63 - tag_index),
            registry_id: self.registry_id,
        }
    }

    fn key_for<T>(&self, component_index: u32) -> ComponentKey<T> {
        ComponentKey {
            component_index,
            mask: 1 << component_index,
            registry_id: self.registry_id,
            marker: PhantomData,
        }
    }

    pub fn all_components_mask(&self) -> u64 {
        if self.components.len() == 64 {
            u64::MAX
        } else {
            (1u64 << self.components.len()) - 1
        }
    }

    pub fn all_tags_mask(&self) -> u64 {
        let mut mask = 0;
        for tag_index in 0..self.tag_count {
            mask |= 1 << (63 - tag_index);
        }
        mask
    }
}

/// One erased column plus its change ticks. `data` always holds the `Vec<T>`
/// of the registered component; a hand-swapped wrong-type box panics on the
/// next typed access rather than misbehaving.
pub struct ColumnSlot {
    pub component_index: u32,
    pub data: ErasedColumn,
    pub changed: Vec<u32>,
    pub peak_changed: u32,
}

/// An archetype table for dynamic worlds: entities plus one [`ColumnSlot`]
/// per component bit in `mask`, ordered by ascending bit.
pub struct DynComponentArrays {
    pub mask: u64,
    pub entity_indices: Vec<Entity>,
    pub columns: Vec<ColumnSlot>,
}

/// Index of `component_mask`'s column within a table of `table_mask`,
/// assuming columns are stored in ascending bit order.
#[inline]
pub fn column_position(table_mask: u64, component_mask: u64) -> usize {
    (table_mask & (component_mask - 1)).count_ones() as usize
}

impl DynComponentArrays {
    pub fn column<T: 'static>(&self, key: ComponentKey<T>) -> &[T] {
        let position = column_position(self.mask, key.mask);
        column_vec::<T>(self.columns[position].data.as_ref())
    }

    /// Mutable raw column access. Does not stamp change ticks; stamp through
    /// `changed_column_mut` or use the typed query tier when change detection
    /// matters.
    pub fn column_mut<T: 'static>(&mut self, key: ComponentKey<T>) -> &mut [T] {
        let position = column_position(self.mask, key.mask);
        column_vec_mut::<T>(self.columns[position].data.as_mut())
    }

    pub fn has_component(&self, mask: u64) -> bool {
        self.mask & mask != 0
    }

    /// Disjoint mutable and shared column slices in one call, for hoisting
    /// column access out of per-entity loops. Panics if the two components
    /// are the same or either is absent from this table.
    pub fn columns_pair<A: 'static, B: 'static>(
        &mut self,
        first: ComponentKey<A>,
        second: ComponentKey<B>,
    ) -> (&mut [A], &[B]) {
        let first_position = column_position(self.mask, first.mask);
        let second_position = column_position(self.mask, second.mask);
        let [first_slot, second_slot] = self
            .columns
            .get_disjoint_mut([first_position, second_position])
            .expect("columns_pair components must be distinct");
        (
            column_vec_mut::<A>(first_slot.data.as_mut()).as_mut_slice(),
            column_vec::<B>(second_slot.data.as_ref()).as_slice(),
        )
    }
}

enum DynCommand {
    SpawnEntities { mask: u64, count: usize },
    DespawnEntity(Entity),
    DespawnEntities(Vec<Entity>),
    AddComponents(Entity, u64),
    RemoveComponents(Entity, u64),
    AddTag(Entity, TagKey),
    RemoveTag(Entity, TagKey),
    Closure(Box<dyn FnOnce(&mut DynWorld) + Send>),
}

struct EventSlot {
    type_id: TypeId,
    data: ErasedColumn,
    update: fn(&mut (dyn Any + Send + Sync)),
}

fn event_update<T: Send + Sync + 'static>(data: &mut (dyn Any + Send + Sync)) {
    data.downcast_mut::<EventChannel<T>>()
        .expect("event channel type mismatch")
        .update();
}

/// A world whose component set is a runtime value. Same archetype storage,
/// change detection, structural log, tags, events, and command deferral as
/// the macro-generated worlds, with dispatch confined to registration
/// boundaries. All fields are public plain data, matching the crate's
/// design philosophy; the invariants the methods maintain are documented on
/// [`ColumnSlot`] and [`ComponentRegistry`].
pub struct DynWorld {
    pub registry: ComponentRegistry,
    pub allocator: EntityAllocator,
    pub entity_locations: EntityLocations,
    pub tables: Vec<DynComponentArrays>,
    pub table_lookup: HashMap<u64, usize>,
    pub table_edges: Vec<ArchetypeEdges>,
    pub query_cache: HashMap<u64, Vec<usize>>,
    pub current_tick: u32,
    pub last_tick: u32,
    pub structural_log: Vec<StructuralChange>,
    pub structural_sequence: u64,
    pub tags: Vec<SparseTagSet>,
    command_buffer: Vec<DynCommand>,
    event_slots: Vec<EventSlot>,
    events_by_type: HashMap<TypeId, usize>,
    pub resources: HashMap<TypeId, ErasedColumn>,
}

impl Default for DynWorld {
    fn default() -> Self {
        Self::new()
    }
}

fn get_location(locations: &EntityLocations, entity: Entity) -> Option<(usize, usize)> {
    let location = locations.get(entity.id)?;
    if !location.allocated || location.generation != entity.generation {
        return None;
    }
    Some((location.table_index as usize, location.array_index as usize))
}

fn insert_location(locations: &mut EntityLocations, entity: Entity, location: (usize, usize)) {
    locations.insert(
        entity.id,
        EntityLocation {
            generation: entity.generation,
            table_index: location.0 as u32,
            array_index: location.1 as u32,
            allocated: true,
        },
    );
}

impl DynWorld {
    pub fn new() -> Self {
        Self::from_registry(ComponentRegistry::new())
    }

    /// Builds a world over a prebuilt schema. Worlds built from clones of one
    /// registry agree on every mask bit.
    pub fn from_registry(registry: ComponentRegistry) -> Self {
        Self {
            registry,
            allocator: EntityAllocator::default(),
            entity_locations: EntityLocations::default(),
            tables: Vec::new(),
            table_lookup: HashMap::new(),
            table_edges: Vec::new(),
            query_cache: HashMap::new(),
            current_tick: 0,
            last_tick: 0,
            structural_log: Vec::new(),
            structural_sequence: 0,
            tags: Vec::new(),
            command_buffer: Vec::new(),
            event_slots: Vec::new(),
            events_by_type: HashMap::new(),
            resources: HashMap::new(),
        }
    }

    /// Registers `T` on this world's registry and returns its key.
    pub fn register<T: Send + Sync + Default + 'static>(&mut self) -> ComponentKey<T> {
        self.registry.register::<T>()
    }

    pub fn register_tag(&mut self) -> TagKey {
        let key = self.registry.register_tag();
        while self.tags.len() < self.registry.tag_count as usize {
            self.tags.push(SparseTagSet::default());
        }
        key
    }

    fn check_key(&self, registry_id: u32) {
        debug_assert_eq!(
            registry_id, self.registry.registry_id,
            "key was minted by a different registry than this world's"
        );
    }

    fn record_structural(&mut self, entity: Entity, kind: StructuralChangeKind, mask: u64) {
        if self.structural_log.len() >= STRUCTURAL_LOG_CAPACITY {
            self.structural_log.clear();
        }
        self.structural_sequence += 1;
        self.structural_log.push(StructuralChange {
            sequence: self.structural_sequence,
            entity,
            kind,
            mask,
        });
    }

    fn get_or_create_table(&mut self, mask: u64) -> usize {
        debug_assert_eq!(
            mask & !self.registry.all_components_mask(),
            0,
            "archetype masks must not contain tag bits or unregistered component bits"
        );
        if let Some(&index) = self.table_lookup.get(&mask) {
            return index;
        }

        let component_count = self.registry.components.len();
        for edges in &mut self.table_edges {
            if edges.add_edges.len() < component_count {
                edges.add_edges.resize(component_count, None);
                edges.remove_edges.resize(component_count, None);
            }
        }

        let table_index = self.tables.len();
        let mut columns = Vec::with_capacity(mask.count_ones() as usize);
        for info in &self.registry.components {
            if mask & info.mask != 0 {
                columns.push(ColumnSlot {
                    component_index: (info.mask.trailing_zeros()),
                    data: (info.new_column)(),
                    changed: Vec::new(),
                    peak_changed: self.current_tick,
                });
            }
        }
        self.tables.push(DynComponentArrays {
            mask,
            entity_indices: Vec::new(),
            columns,
        });

        archetype_register_table(
            ArchetypeRouting {
                table_lookup: &mut self.table_lookup,
                table_edges: &mut self.table_edges,
                query_cache: &mut self.query_cache,
            },
            self.registry.components.len(),
            mask,
            table_index,
            self.tables.iter().map(|table| table.mask),
            self.registry
                .components
                .iter()
                .enumerate()
                .map(|(component_index, info)| (info.mask, component_index)),
        );

        table_index
    }

    pub fn spawn_entities(&mut self, mask: u64, count: usize) -> Vec<Entity> {
        let table_index = self.get_or_create_table(mask);
        let current_tick = self.current_tick;

        let mut entities = Vec::new();
        self.allocator.allocate_batch(count, &mut entities);

        let start_index = self.tables[table_index].entity_indices.len();
        {
            let table = &mut self.tables[table_index];
            table.entity_indices.reserve(count);
            for column in &mut table.columns {
                let info = &self.registry.components[column.component_index as usize];
                (info.push_default)(column.data.as_mut(), count);
                column.changed.reserve(count);
                for _ in 0..count {
                    column.changed.push(current_tick);
                }
                column.peak_changed = current_tick;
            }
            for &entity in &entities {
                table.entity_indices.push(entity);
            }
        }

        for (offset, &entity) in entities.iter().enumerate() {
            insert_location(
                &mut self.entity_locations,
                entity,
                (table_index, start_index + offset),
            );
            self.record_structural(entity, StructuralChangeKind::Spawned, mask);
        }

        entities
    }

    pub fn spawn_batch<F>(&mut self, mask: u64, count: usize, mut init: F) -> Vec<Entity>
    where
        F: FnMut(&mut DynComponentArrays, usize),
    {
        let entities = self.spawn_entities(mask, count);
        if let Some(&first) = entities.first() {
            let (table_index, start_index) =
                get_location(&self.entity_locations, first).expect("just spawned");
            let table = &mut self.tables[table_index];
            for offset in 0..count {
                init(table, start_index + offset);
            }
        }
        entities
    }

    fn remove_row(&mut self, table_index: usize, array_index: usize) {
        let table = &mut self.tables[table_index];
        let last_index = table.entity_indices.len() - 1;
        let swapped = if array_index < last_index {
            Some(table.entity_indices[last_index])
        } else {
            None
        };

        for column in &mut table.columns {
            let info = &self.registry.components[column.component_index as usize];
            (info.swap_remove)(column.data.as_mut(), array_index);
            column.changed.swap_remove(array_index);
        }
        table.entity_indices.swap_remove(array_index);

        if let Some(swapped_entity) = swapped
            && let Some(location) = self.entity_locations.get_mut(swapped_entity.id)
            && location.allocated
        {
            location.array_index = array_index as u32;
        }
    }

    /// Removes the entity's row if present and retires this handle by
    /// recording the next generation. Must only be called with a handle the
    /// allocator confirmed live; `despawn_entities` guarantees that.
    pub fn retire_entity(&mut self, entity: Entity) -> bool {
        let mut removed = false;
        if let Some((table_index, array_index)) = get_location(&self.entity_locations, entity) {
            self.entity_locations.mark_deallocated(entity.id);
            let despawned_mask = self.tables[table_index].mask;
            self.record_structural(entity, StructuralChangeKind::Despawned, despawned_mask);
            self.remove_row(table_index, array_index);
            removed = true;
        }

        let next_generation = entity.generation.wrapping_add(1);
        let should_retire = match self.entity_locations.get(entity.id) {
            None => true,
            Some(location) => {
                !location.allocated && tick_is_newer(next_generation, location.generation)
            }
        };
        if should_retire {
            self.entity_locations
                .ensure_slot(entity.id, next_generation);
        }

        removed
    }

    pub fn despawn_entities(&mut self, entities: &[Entity]) -> Vec<Entity> {
        let mut despawned = Vec::with_capacity(entities.len());
        for &entity in entities {
            if self.allocator.deallocate(entity) {
                self.retire_entity(entity);
                for tag_set in &mut self.tags {
                    tag_set.remove(entity);
                }
                despawned.push(entity);
            }
        }
        despawned
    }

    fn move_entity(
        &mut self,
        entity: Entity,
        from_table: usize,
        from_index: usize,
        to_table: usize,
    ) {
        let tick = self.current_tick;
        {
            let [source, destination] = self
                .tables
                .get_disjoint_mut([from_table, to_table])
                .expect("migration source and destination must differ");

            let shared = source.mask & destination.mask;
            let mut bits = shared;
            while bits != 0 {
                let component_mask = bits & bits.wrapping_neg();
                bits &= bits - 1;

                let source_position = column_position(source.mask, component_mask);
                let destination_position = column_position(destination.mask, component_mask);
                let info = &self.registry.components
                    [source.columns[source_position].component_index as usize];
                (info.move_row)(
                    source.columns[source_position].data.as_mut(),
                    from_index,
                    destination.columns[destination_position].data.as_mut(),
                );
                let destination_column = &mut destination.columns[destination_position];
                destination_column.changed.push(tick);
                destination_column.peak_changed = tick;
            }

            let mut gained = destination.mask & !source.mask;
            while gained != 0 {
                let component_mask = gained & gained.wrapping_neg();
                gained &= gained - 1;

                let destination_position = column_position(destination.mask, component_mask);
                let destination_column = &mut destination.columns[destination_position];
                let info = &self.registry.components[destination_column.component_index as usize];
                (info.push_default)(destination_column.data.as_mut(), 1);
                destination_column.changed.push(tick);
                destination_column.peak_changed = tick;
            }

            destination.entity_indices.push(entity);
        }

        let new_index = self.tables[to_table].entity_indices.len() - 1;
        insert_location(&mut self.entity_locations, entity, (to_table, new_index));
        self.remove_row(from_table, from_index);
    }

    pub fn add_components(&mut self, entity: Entity, mask: u64) -> bool {
        debug_assert_eq!(
            mask & !self.registry.all_components_mask(),
            0,
            "component masks must not contain tag bits or unregistered component bits"
        );
        let Some((table_index, array_index)) = get_location(&self.entity_locations, entity) else {
            return false;
        };
        let current_mask = self.tables[table_index].mask;
        if current_mask & mask == mask {
            return true;
        }

        let target_table = if mask.count_ones() == 1 {
            self.table_edges[table_index]
                .add_edges
                .get(mask.trailing_zeros() as usize)
                .copied()
                .flatten()
        } else {
            self.table_edges[table_index]
                .multi_add_cache
                .get(&mask)
                .copied()
        };

        let new_table_index = target_table.unwrap_or_else(|| {
            let new_index = self.get_or_create_table(current_mask | mask);
            self.table_edges[table_index]
                .multi_add_cache
                .insert(mask, new_index);
            new_index
        });

        self.move_entity(entity, table_index, array_index, new_table_index);
        self.record_structural(
            entity,
            StructuralChangeKind::ComponentsAdded,
            mask & !current_mask,
        );
        true
    }

    pub fn remove_components(&mut self, entity: Entity, mask: u64) -> bool {
        debug_assert_eq!(
            mask & !self.registry.all_components_mask(),
            0,
            "component masks must not contain tag bits or unregistered component bits"
        );
        let Some((table_index, array_index)) = get_location(&self.entity_locations, entity) else {
            return false;
        };
        let current_mask = self.tables[table_index].mask;
        if current_mask & mask == 0 {
            return true;
        }

        let target_table = if mask.count_ones() == 1 {
            self.table_edges[table_index]
                .remove_edges
                .get(mask.trailing_zeros() as usize)
                .copied()
                .flatten()
        } else {
            self.table_edges[table_index]
                .multi_remove_cache
                .get(&mask)
                .copied()
        };

        let new_table_index = target_table.unwrap_or_else(|| {
            let new_index = self.get_or_create_table(current_mask & !mask);
            self.table_edges[table_index]
                .multi_remove_cache
                .insert(mask, new_index);
            new_index
        });

        self.move_entity(entity, table_index, array_index, new_table_index);
        self.record_structural(
            entity,
            StructuralChangeKind::ComponentsRemoved,
            current_mask & mask,
        );
        true
    }

    pub fn get_keyed<T: 'static>(&self, key: ComponentKey<T>, entity: Entity) -> Option<&T> {
        self.check_key(key.registry_id);
        let (table_index, array_index) = get_location(&self.entity_locations, entity)?;
        let table = &self.tables[table_index];
        if table.mask & key.mask == 0 {
            return None;
        }
        let position = column_position(table.mask, key.mask);
        Some(&column_vec::<T>(table.columns[position].data.as_ref())[array_index])
    }

    pub fn get_mut_keyed<T: 'static>(
        &mut self,
        key: ComponentKey<T>,
        entity: Entity,
    ) -> Option<&mut T> {
        self.check_key(key.registry_id);
        let (table_index, array_index) = get_location(&self.entity_locations, entity)?;
        let current_tick = self.current_tick;
        let table = &mut self.tables[table_index];
        if table.mask & key.mask == 0 {
            return None;
        }
        let position = column_position(table.mask, key.mask);
        let column = &mut table.columns[position];
        column.changed[array_index] = current_tick;
        column.peak_changed = current_tick;
        Some(&mut column_vec_mut::<T>(column.data.as_mut())[array_index])
    }

    pub fn set_keyed<T: 'static>(&mut self, key: ComponentKey<T>, entity: Entity, value: T) {
        self.check_key(key.registry_id);
        if let Some((table_index, array_index)) = get_location(&self.entity_locations, entity) {
            let current_tick = self.current_tick;
            let table = &mut self.tables[table_index];
            if table.mask & key.mask != 0 {
                let position = column_position(table.mask, key.mask);
                let column = &mut table.columns[position];
                column_vec_mut::<T>(column.data.as_mut())[array_index] = value;
                column.changed[array_index] = current_tick;
                column.peak_changed = current_tick;
                return;
            }
            if self.add_components(entity, key.mask)
                && let Some((table_index, array_index)) =
                    get_location(&self.entity_locations, entity)
            {
                let table = &mut self.tables[table_index];
                let position = column_position(table.mask, key.mask);
                let column = &mut table.columns[position];
                column_vec_mut::<T>(column.data.as_mut())[array_index] = value;
                column.changed[array_index] = current_tick;
                column.peak_changed = current_tick;
            }
        }
    }

    pub fn component_mask(&self, entity: Entity) -> Option<u64> {
        get_location(&self.entity_locations, entity)
            .map(|(table_index, _)| self.tables[table_index].mask)
    }

    pub fn entity_has_components(&self, entity: Entity, mask: u64) -> bool {
        self.component_mask(entity).unwrap_or(0) & mask == mask
    }

    pub fn contains_entity(&self, entity: Entity) -> bool {
        get_location(&self.entity_locations, entity).is_some()
    }

    pub fn is_alive(&self, entity: Entity) -> bool {
        self.allocator.is_alive(entity)
    }

    pub fn entity_count(&self) -> usize {
        self.tables
            .iter()
            .map(|table| table.entity_indices.len())
            .sum()
    }

    pub fn get_all_entities(&self) -> Vec<Entity> {
        let mut result = Vec::with_capacity(self.entity_count());
        for table in &self.tables {
            result.extend(table.entity_indices.iter().copied());
        }
        result
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

    pub fn structural_sequence(&self) -> u64 {
        self.structural_sequence
    }

    pub fn structural_changes_since(&self, cursor: u64) -> &[StructuralChange] {
        let start = self
            .structural_log
            .partition_point(|change| change.sequence <= cursor);
        &self.structural_log[start..]
    }

    pub fn trim_structural_log(&mut self, up_to_sequence: u64) {
        let end = self
            .structural_log
            .partition_point(|change| change.sequence <= up_to_sequence);
        self.structural_log.drain(..end);
    }

    pub fn clear_structural_log(&mut self) {
        self.structural_log.clear();
    }

    pub fn add_tag(&mut self, key: TagKey, entity: Entity) {
        self.check_key(key.registry_id);
        if self.contains_entity(entity) && self.tags[key.tag_index as usize].insert(entity) {
            self.record_structural(entity, StructuralChangeKind::TagsAdded, key.mask);
        }
    }

    pub fn remove_tag(&mut self, key: TagKey, entity: Entity) -> bool {
        self.check_key(key.registry_id);
        let removed = self.tags[key.tag_index as usize].remove(entity);
        if removed {
            self.record_structural(entity, StructuralChangeKind::TagsRemoved, key.mask);
        }
        removed
    }

    pub fn has_tag(&self, key: TagKey, entity: Entity) -> bool {
        self.check_key(key.registry_id);
        self.tags[key.tag_index as usize].contains(entity)
    }

    pub fn query_tag(&self, key: TagKey) -> impl Iterator<Item = Entity> + '_ {
        self.check_key(key.registry_id);
        self.tags[key.tag_index as usize].iter()
    }

    fn entity_matches_tags(&self, entity: Entity, tag_include: u64, tag_exclude: u64) -> bool {
        for (tag_index, tag_set) in self.tags.iter().enumerate() {
            let tag_mask = 1u64 << (63 - tag_index as u32);
            if tag_include & tag_mask != 0 && !tag_set.contains(entity) {
                return false;
            }
            if tag_exclude & tag_mask != 0 && tag_set.contains(entity) {
                return false;
            }
        }
        true
    }

    /// Returns None when an included tag has no members. Drops excluded tags
    /// whose sets are empty, so exclusion of an unused tag stays on the
    /// unfiltered path.
    fn reduce_tag_masks(&self, tag_include: u64, tag_exclude: u64) -> Option<(u64, u64)> {
        let mut reduced_exclude = tag_exclude;
        for (tag_index, tag_set) in self.tags.iter().enumerate() {
            let tag_mask = 1u64 << (63 - tag_index as u32);
            if tag_include & tag_mask != 0 && tag_set.is_empty() {
                return None;
            }
            if reduced_exclude & tag_mask != 0 && tag_set.is_empty() {
                reduced_exclude &= !tag_mask;
            }
        }
        Some((tag_include, reduced_exclude))
    }

    fn split_masks(&self, include: u64, exclude: u64) -> Option<(u64, u64, u64, u64)> {
        let all_tags = self.registry.all_tags_mask();
        let (tag_include, tag_exclude) =
            self.reduce_tag_masks(include & all_tags, exclude & all_tags)?;
        Some((
            include & !all_tags,
            exclude & !all_tags,
            tag_include,
            tag_exclude,
        ))
    }

    /// Table-granular iteration, the raw fast path: resolve columns once per
    /// table, then loop entities over concrete slices. Component masks only.
    /// Does not stamp change ticks.
    pub fn for_each_tables_mut<F>(&mut self, include: u64, exclude: u64, mut f: F)
    where
        F: FnMut(&mut DynComponentArrays),
    {
        debug_assert_eq!(
            include & !self.registry.all_components_mask(),
            0,
            "table-granular iteration takes component masks only"
        );
        let table_indices = archetype_cached_tables(
            &mut self.query_cache,
            self.tables.iter().map(|table| table.mask),
            include,
        );
        let tables = &mut self.tables;
        for &table_index in table_indices {
            let table = &mut tables[table_index];
            if table.mask & exclude != 0 || table.entity_indices.is_empty() {
                continue;
            }
            f(table);
        }
    }

    pub fn for_each_tables<F>(&self, include: u64, exclude: u64, mut f: F)
    where
        F: FnMut(&DynComponentArrays),
    {
        debug_assert_eq!(
            include & !self.registry.all_components_mask(),
            0,
            "table-granular iteration takes component masks only"
        );
        for table in &self.tables {
            if table.mask & include != include
                || table.mask & exclude != 0
                || table.entity_indices.is_empty()
            {
                continue;
            }
            f(table);
        }
    }

    pub fn for_each<F>(&self, include: u64, exclude: u64, mut f: F)
    where
        F: FnMut(Entity, &DynComponentArrays, usize),
    {
        let Some((component_include, component_exclude, tag_include, tag_exclude)) =
            self.split_masks(include, exclude)
        else {
            return;
        };

        for table in &self.tables {
            if table.mask & component_include != component_include
                || table.mask & component_exclude != 0
            {
                continue;
            }
            if tag_include == 0 && tag_exclude == 0 {
                for (index, &entity) in table.entity_indices.iter().enumerate() {
                    f(entity, table, index);
                }
            } else {
                for (index, &entity) in table.entity_indices.iter().enumerate() {
                    if self.entity_matches_tags(entity, tag_include, tag_exclude) {
                        f(entity, table, index);
                    }
                }
            }
        }
    }

    pub fn for_each_mut<F>(&mut self, include: u64, exclude: u64, mut f: F)
    where
        F: FnMut(Entity, &mut DynComponentArrays, usize),
    {
        let Some((component_include, component_exclude, tag_include, tag_exclude)) =
            self.split_masks(include, exclude)
        else {
            return;
        };

        let tags = &self.tags;
        let table_indices = archetype_cached_tables(
            &mut self.query_cache,
            self.tables.iter().map(|table| table.mask),
            component_include,
        );
        let tables = &mut self.tables;

        for &table_index in table_indices {
            let table = &mut tables[table_index];
            if table.mask & component_exclude != 0 {
                continue;
            }
            for index in 0..table.entity_indices.len() {
                let entity = table.entity_indices[index];
                if (tag_include != 0 || tag_exclude != 0)
                    && !tags_match(tags, entity, tag_include, tag_exclude)
                {
                    continue;
                }
                f(entity, table, index);
            }
        }
    }

    pub fn for_each_mut_changed<F>(&mut self, include: u64, exclude: u64, f: F)
    where
        F: FnMut(Entity, &mut DynComponentArrays, usize),
    {
        let since_tick = self.last_tick;
        self.for_each_mut_changed_since(include, exclude, since_tick, f);
    }

    pub fn for_each_mut_changed_since<F>(
        &mut self,
        include: u64,
        exclude: u64,
        since_tick: u32,
        mut f: F,
    ) where
        F: FnMut(Entity, &mut DynComponentArrays, usize),
    {
        let Some((component_include, component_exclude, tag_include, tag_exclude)) =
            self.split_masks(include, exclude)
        else {
            return;
        };

        let tags = &self.tags;
        let table_indices = archetype_cached_tables(
            &mut self.query_cache,
            self.tables.iter().map(|table| table.mask),
            component_include,
        );
        let tables = &mut self.tables;

        for &table_index in table_indices {
            let table = &mut tables[table_index];
            if table.mask & component_exclude != 0 {
                continue;
            }

            let mut table_changed = false;
            for column in &table.columns {
                let column_mask = 1u64 << column.component_index;
                if component_include & column_mask != 0
                    && tick_is_newer(column.peak_changed, since_tick)
                {
                    table_changed = true;
                }
            }
            if !table_changed {
                continue;
            }

            for index in 0..table.entity_indices.len() {
                let entity = table.entity_indices[index];
                if (tag_include != 0 || tag_exclude != 0)
                    && !tags_match(tags, entity, tag_include, tag_exclude)
                {
                    continue;
                }

                let mut changed = false;
                for column in &table.columns {
                    let column_mask = 1u64 << column.component_index;
                    if component_include & column_mask != 0
                        && tick_is_newer(column.changed[index], since_tick)
                    {
                        changed = true;
                    }
                }
                if changed {
                    f(entity, table, index);
                }
            }
        }
    }

    pub fn query_entities(&self, mask: u64) -> impl Iterator<Item = Entity> + '_ {
        debug_assert_eq!(
            mask & !self.registry.all_components_mask(),
            0,
            "query_entities takes component masks only"
        );
        self.tables
            .iter()
            .filter(move |table| table.mask & mask == mask)
            .flat_map(|table| table.entity_indices.iter().copied())
    }

    pub fn query_entities_changed(&self, mask: u64) -> impl Iterator<Item = Entity> + '_ {
        self.query_entities_changed_since(mask, self.last_tick)
    }

    pub fn query_entities_changed_since(
        &self,
        mask: u64,
        since_tick: u32,
    ) -> impl Iterator<Item = Entity> + '_ {
        debug_assert_eq!(
            mask & !self.registry.all_components_mask(),
            0,
            "changed queries take component masks only"
        );
        self.tables
            .iter()
            .filter(move |table| {
                table.mask & mask == mask
                    && table.columns.iter().any(|column| {
                        mask & (1u64 << column.component_index) != 0
                            && tick_is_newer(column.peak_changed, since_tick)
                    })
            })
            .flat_map(move |table| {
                table
                    .entity_indices
                    .iter()
                    .enumerate()
                    .filter(move |(index, _)| {
                        table.columns.iter().any(|column| {
                            mask & (1u64 << column.component_index) != 0
                                && tick_is_newer(column.changed[*index], since_tick)
                        })
                    })
                    .map(|(_, &entity)| entity)
            })
    }

    #[cfg(not(target_family = "wasm"))]
    pub fn par_for_each_mut<F>(&mut self, include: u64, exclude: u64, f: F)
    where
        F: Fn(Entity, &mut DynComponentArrays, usize) + Send + Sync,
    {
        use crate::rayon::prelude::*;

        let Some((component_include, component_exclude, tag_include, tag_exclude)) =
            self.split_masks(include, exclude)
        else {
            return;
        };

        let tags = &self.tags;
        self.tables
            .par_iter_mut()
            .filter(|table| {
                table.mask & component_include == component_include
                    && table.mask & component_exclude == 0
            })
            .for_each(|table| {
                for index in 0..table.entity_indices.len() {
                    let entity = table.entity_indices[index];
                    if (tag_include != 0 || tag_exclude != 0)
                        && !tags_match(tags, entity, tag_include, tag_exclude)
                    {
                        continue;
                    }
                    f(entity, table, index);
                }
            });
    }

    pub fn step(&mut self) {
        for slot in &mut self.event_slots {
            (slot.update)(slot.data.as_mut());
        }
        self.last_tick = self.current_tick;
        self.current_tick = self.current_tick.wrapping_add(1);
    }

    fn event_slot_index<T: Send + Sync + 'static>(&mut self) -> usize {
        if let Some(&index) = self.events_by_type.get(&TypeId::of::<T>()) {
            return index;
        }
        let index = self.event_slots.len();
        self.event_slots.push(EventSlot {
            type_id: TypeId::of::<T>(),
            data: Box::new(EventChannel::<T>::new()),
            update: event_update::<T>,
        });
        self.events_by_type.insert(TypeId::of::<T>(), index);
        index
    }

    fn event_channel<T: Send + Sync + 'static>(&self) -> Option<&EventChannel<T>> {
        let index = *self.events_by_type.get(&TypeId::of::<T>())?;
        debug_assert_eq!(self.event_slots[index].type_id, TypeId::of::<T>());
        Some(
            self.event_slots[index]
                .data
                .downcast_ref::<EventChannel<T>>()
                .expect("event channel type mismatch"),
        )
    }

    pub fn send<T: Send + Sync + 'static>(&mut self, event: T) {
        let index = self.event_slot_index::<T>();
        self.event_slots[index]
            .data
            .downcast_mut::<EventChannel<T>>()
            .expect("event channel type mismatch")
            .send(event);
    }

    /// Everything still buffered for `T`, oldest first. An unregistered event
    /// type reads as empty, which is indistinguishable from registered and
    /// empty.
    pub fn read_events<T: Send + Sync + 'static>(&self) -> &[T] {
        self.event_channel::<T>()
            .map(|channel| channel.events.as_slice())
            .unwrap_or(&[])
    }

    pub fn read_events_since<T: Send + Sync + 'static>(&self, cursor: u64) -> &[T] {
        self.event_channel::<T>()
            .map(|channel| channel.events_since(cursor))
            .unwrap_or(&[])
    }

    pub fn event_sequence<T: Send + Sync + 'static>(&self) -> u64 {
        self.event_channel::<T>()
            .map(|channel| channel.sequence())
            .unwrap_or(0)
    }

    pub fn trim_events<T: Send + Sync + 'static>(&mut self, up_to_sequence: u64) {
        let index = self.event_slot_index::<T>();
        self.event_slots[index]
            .data
            .downcast_mut::<EventChannel<T>>()
            .expect("event channel type mismatch")
            .trim(up_to_sequence);
    }

    pub fn clear_events<T: Send + Sync + 'static>(&mut self) {
        let index = self.event_slot_index::<T>();
        self.event_slots[index]
            .data
            .downcast_mut::<EventChannel<T>>()
            .expect("event channel type mismatch")
            .clear();
    }

    pub fn insert_resource<T: Send + Sync + 'static>(&mut self, value: T) {
        self.resources.insert(TypeId::of::<T>(), Box::new(value));
    }

    pub fn resource<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.resources
            .get(&TypeId::of::<T>())
            .and_then(|value| value.downcast_ref::<T>())
    }

    pub fn resource_mut<T: Send + Sync + 'static>(&mut self) -> Option<&mut T> {
        self.resources
            .get_mut(&TypeId::of::<T>())
            .and_then(|value| value.downcast_mut::<T>())
    }

    pub fn remove_resource<T: Send + Sync + 'static>(&mut self) -> Option<T> {
        self.resources
            .remove(&TypeId::of::<T>())
            .and_then(|value| value.downcast::<T>().ok())
            .map(|value| *value)
    }

    pub fn queue_spawn_entities(&mut self, mask: u64, count: usize) {
        self.command_buffer
            .push(DynCommand::SpawnEntities { mask, count });
    }

    pub fn queue_despawn_entity(&mut self, entity: Entity) {
        self.command_buffer.push(DynCommand::DespawnEntity(entity));
    }

    pub fn queue_despawn_entities(&mut self, entities: Vec<Entity>) {
        self.command_buffer
            .push(DynCommand::DespawnEntities(entities));
    }

    pub fn queue_add_components(&mut self, entity: Entity, mask: u64) {
        self.command_buffer
            .push(DynCommand::AddComponents(entity, mask));
    }

    pub fn queue_remove_components(&mut self, entity: Entity, mask: u64) {
        self.command_buffer
            .push(DynCommand::RemoveComponents(entity, mask));
    }

    pub fn queue_add_tag(&mut self, key: TagKey, entity: Entity) {
        self.command_buffer.push(DynCommand::AddTag(entity, key));
    }

    pub fn queue_remove_tag(&mut self, key: TagKey, entity: Entity) {
        self.command_buffer.push(DynCommand::RemoveTag(entity, key));
    }

    /// Queues a typed component write. The value is boxed with the command;
    /// registration happens at queue time so apply order cannot depend on it.
    pub fn queue_set<T: Send + Sync + Default + 'static>(&mut self, entity: Entity, value: T) {
        let key = self.component_key::<T>();
        self.command_buffer
            .push(DynCommand::Closure(Box::new(move |world| {
                world.set_keyed(key, entity, value);
            })));
    }

    /// Queues an arbitrary deferred mutation.
    pub fn queue(&mut self, command: impl FnOnce(&mut DynWorld) + Send + 'static) {
        self.command_buffer
            .push(DynCommand::Closure(Box::new(command)));
    }

    pub fn command_count(&self) -> usize {
        self.command_buffer.len()
    }

    pub fn clear_commands(&mut self) {
        self.command_buffer.clear();
    }

    pub fn apply_commands(&mut self) {
        let commands = std::mem::take(&mut self.command_buffer);
        for command in commands {
            match command {
                DynCommand::SpawnEntities { mask, count } => {
                    self.spawn_entities(mask, count);
                }
                DynCommand::DespawnEntity(entity) => {
                    self.despawn_entities(&[entity]);
                }
                DynCommand::DespawnEntities(entities) => {
                    self.despawn_entities(&entities);
                }
                DynCommand::AddComponents(entity, mask) => {
                    self.add_components(entity, mask);
                }
                DynCommand::RemoveComponents(entity, mask) => {
                    self.remove_components(entity, mask);
                }
                DynCommand::AddTag(entity, key) => {
                    self.add_tag(key, entity);
                }
                DynCommand::RemoveTag(entity, key) => {
                    self.remove_tag(key, entity);
                }
                DynCommand::Closure(command) => {
                    command(self);
                }
            }
        }
    }

    /// The lazy typed tier: resolves or registers `T` and returns its key.
    pub fn component_key<T: Send + Sync + Default + 'static>(&mut self) -> ComponentKey<T> {
        self.registry.register::<T>()
    }

    /// Typed read. Unregistered types read as absent.
    pub fn get<T: Send + Sync + Default + 'static>(&self, entity: Entity) -> Option<&T> {
        let &component_index = self.registry.components_by_type.get(&TypeId::of::<T>())?;
        let key = self.registry.key_for::<T>(component_index);
        self.get_keyed(key, entity)
    }

    pub fn get_mut<T: Send + Sync + Default + 'static>(
        &mut self,
        entity: Entity,
    ) -> Option<&mut T> {
        let key = self.component_key::<T>();
        self.get_mut_keyed(key, entity)
    }

    pub fn set<T: Send + Sync + Default + 'static>(&mut self, entity: Entity, value: T) {
        let key = self.component_key::<T>();
        self.set_keyed(key, entity, value);
    }

    pub fn remove<T: Send + Sync + Default + 'static>(&mut self, entity: Entity) -> bool {
        let key = self.component_key::<T>();
        self.remove_components(entity, key.mask)
    }

    pub fn has<T: Send + Sync + Default + 'static>(&self, entity: Entity) -> bool {
        self.get::<T>(entity).is_some()
    }

    /// Spawns one entity carrying the bundle's components, set to the given
    /// values. Bundle types register lazily.
    pub fn spawn<B: Bundle>(&mut self, bundle: B) -> Entity {
        let mask = B::component_mask(self);
        let entity = self.spawn_entities(mask, 1)[0];
        bundle.write(self, entity);
        entity
    }

    /// Starts a typed query. Component types in the tuple register lazily,
    /// borrow mutability comes from the tuple (`&T` or `&mut T`), and
    /// mutable elements stamp change ticks per visited entity.
    pub fn query<Q: QueryTuple>(&mut self) -> DynQuery<'_, Q> {
        let include = Q::component_mask(self);
        DynQuery {
            world: self,
            include,
            exclude: 0,
            changed_mask: 0,
            marker: PhantomData,
        }
    }
}

mod sealed {
    pub trait SealedElement {}
    pub trait SealedQueryTuple {}
    pub trait SealedBundle {}
}

/// One element of a typed query tuple, `&T` or `&mut T`.
pub trait QueryElement: sealed::SealedElement {
    type Fetch<'table>;
    type Item<'item>;
    fn component_mask(world: &mut DynWorld) -> u64;
    fn fetch<'table>(slot: &'table mut ColumnSlot, current_tick: u32) -> Self::Fetch<'table>;
    fn changed_newer(fetch: &Self::Fetch<'_>, index: usize, since_tick: u32) -> bool;
    fn item<'fetch>(fetch: &'fetch mut Self::Fetch<'_>, index: usize) -> Self::Item<'fetch>;
}

impl<T: Send + Sync + Default + 'static> sealed::SealedElement for &T {}

impl<T: Send + Sync + Default + 'static> QueryElement for &T {
    type Fetch<'table> = (&'table [T], &'table [u32]);
    type Item<'item> = &'item T;

    fn component_mask(world: &mut DynWorld) -> u64 {
        world.component_key::<T>().mask
    }

    fn fetch<'table>(slot: &'table mut ColumnSlot, _current_tick: u32) -> Self::Fetch<'table> {
        (
            column_vec::<T>(slot.data.as_ref()).as_slice(),
            slot.changed.as_slice(),
        )
    }

    fn changed_newer(fetch: &Self::Fetch<'_>, index: usize, since_tick: u32) -> bool {
        tick_is_newer(fetch.1[index], since_tick)
    }

    fn item<'fetch>(fetch: &'fetch mut Self::Fetch<'_>, index: usize) -> Self::Item<'fetch> {
        &fetch.0[index]
    }
}

impl<T: Send + Sync + Default + 'static> sealed::SealedElement for &mut T {}

impl<T: Send + Sync + Default + 'static> QueryElement for &mut T {
    type Fetch<'table> = (&'table mut [T], &'table mut [u32], u32);
    type Item<'item> = &'item mut T;

    fn component_mask(world: &mut DynWorld) -> u64 {
        world.component_key::<T>().mask
    }

    fn fetch<'table>(slot: &'table mut ColumnSlot, current_tick: u32) -> Self::Fetch<'table> {
        slot.peak_changed = current_tick;
        (
            column_vec_mut::<T>(slot.data.as_mut()).as_mut_slice(),
            slot.changed.as_mut_slice(),
            current_tick,
        )
    }

    fn changed_newer(fetch: &Self::Fetch<'_>, index: usize, since_tick: u32) -> bool {
        tick_is_newer(fetch.1[index], since_tick)
    }

    fn item<'fetch>(fetch: &'fetch mut Self::Fetch<'_>, index: usize) -> Self::Item<'fetch> {
        fetch.1[index] = fetch.2;
        &mut fetch.0[index]
    }
}

/// A tuple of query elements. Implemented for tuples of `&T` and `&mut T`
/// up to four elements; all component types in one tuple must be distinct.
pub trait QueryTuple: sealed::SealedQueryTuple {
    type Fetch<'table>;
    type Item<'item>;
    const ELEMENT_COUNT: usize;
    fn component_mask(world: &mut DynWorld) -> u64;
    fn element_masks(world: &mut DynWorld) -> [u64; 4];
    fn fetch<'table>(
        table_mask: u64,
        columns: &'table mut [ColumnSlot],
        element_masks: &[u64; 4],
        current_tick: u32,
    ) -> Self::Fetch<'table>;
    fn changed_newer(
        fetch: &Self::Fetch<'_>,
        index: usize,
        element_masks: &[u64; 4],
        changed_mask: u64,
        since_tick: u32,
    ) -> bool;
    fn item<'fetch>(fetch: &'fetch mut Self::Fetch<'_>, index: usize) -> Self::Item<'fetch>;
}

macro_rules! impl_query_tuple {
    ($count:expr, $(($element:ident, $position:tt)),+) => {
        impl<$($element: QueryElement),+> sealed::SealedQueryTuple for ($($element,)+) {}

        impl<$($element: QueryElement),+> QueryTuple for ($($element,)+) {
            type Fetch<'table> = ($($element::Fetch<'table>,)+);
            type Item<'item> = ($($element::Item<'item>,)+);
            const ELEMENT_COUNT: usize = $count;

            fn component_mask(world: &mut DynWorld) -> u64 {
                let mut mask = 0;
                $(
                    let element_mask = $element::component_mask(world);
                    assert_eq!(
                        mask & element_mask,
                        0,
                        "query tuples must not repeat a component type"
                    );
                    mask |= element_mask;
                )+
                mask
            }

            fn element_masks(world: &mut DynWorld) -> [u64; 4] {
                let mut masks = [0u64; 4];
                $(
                    masks[$position] = $element::component_mask(world);
                )+
                masks
            }

            #[allow(non_snake_case)]
            fn fetch<'table>(
                table_mask: u64,
                columns: &'table mut [ColumnSlot],
                element_masks: &[u64; 4],
                current_tick: u32,
            ) -> Self::Fetch<'table> {
                let positions = [$(column_position(table_mask, element_masks[$position]),)+];
                let slots = columns
                    .get_disjoint_mut(positions)
                    .expect("query tuple columns must be distinct");
                let [$($element,)+] = slots;
                ($($element::fetch($element, current_tick),)+)
            }

            fn changed_newer(
                fetch: &Self::Fetch<'_>,
                index: usize,
                element_masks: &[u64; 4],
                changed_mask: u64,
                since_tick: u32,
            ) -> bool {
                let mut newer = false;
                $(
                    if changed_mask & element_masks[$position] != 0
                        && $element::changed_newer(&fetch.$position, index, since_tick)
                    {
                        newer = true;
                    }
                )+
                newer
            }

            fn item<'fetch>(fetch: &'fetch mut Self::Fetch<'_>, index: usize) -> Self::Item<'fetch> {
                ($($element::item(&mut fetch.$position, index),)+)
            }
        }
    };
}

impl_query_tuple!(1, (A, 0));
impl_query_tuple!(2, (A, 0), (B, 1));
impl_query_tuple!(3, (A, 0), (B, 1), (C, 2));
impl_query_tuple!(4, (A, 0), (B, 1), (C, 2), (D, 3));

/// A typed query in progress. Filters compose before `for_each` runs it.
pub struct DynQuery<'world, Q: QueryTuple> {
    world: &'world mut DynWorld,
    include: u64,
    exclude: u64,
    changed_mask: u64,
    marker: PhantomData<Q>,
}

impl<'world, Q: QueryTuple> DynQuery<'world, Q> {
    pub fn with<T: Send + Sync + Default + 'static>(mut self) -> Self {
        self.include |= self.world.component_key::<T>().mask;
        self
    }

    pub fn without<T: Send + Sync + Default + 'static>(mut self) -> Self {
        self.exclude |= self.world.component_key::<T>().mask;
        self
    }

    pub fn with_mask(mut self, mask: u64) -> Self {
        self.include |= mask;
        self
    }

    pub fn without_mask(mut self, mask: u64) -> Self {
        self.exclude |= mask;
        self
    }

    pub fn with_tag(mut self, key: TagKey) -> Self {
        self.include |= key.mask;
        self
    }

    pub fn without_tag(mut self, key: TagKey) -> Self {
        self.exclude |= key.mask;
        self
    }

    /// Only visit entities whose `T` changed since the last step. `T` must be
    /// one of the tuple's components.
    pub fn changed<T: Send + Sync + Default + 'static>(mut self) -> Self {
        let mask = self.world.component_key::<T>().mask;
        self.changed_mask |= mask;
        self
    }

    pub fn for_each(self, mut f: impl for<'item> FnMut(Entity, Q::Item<'item>)) {
        let element_masks = Q::element_masks(self.world);
        let tuple_mask = element_masks.iter().fold(0, |mask, element| mask | element);
        assert_eq!(
            self.changed_mask & !tuple_mask,
            0,
            "changed filters must name components present in the query tuple"
        );

        let Some((component_include, component_exclude, tag_include, tag_exclude)) =
            self.world.split_masks(self.include, self.exclude)
        else {
            return;
        };

        let since_tick = self.world.last_tick;
        let current_tick = self.world.current_tick;
        let changed_mask = self.changed_mask;

        let tags = &self.world.tags;
        let table_indices = archetype_cached_tables(
            &mut self.world.query_cache,
            self.world.tables.iter().map(|table| table.mask),
            component_include,
        );
        let tables = &mut self.world.tables;

        for &table_index in table_indices {
            let table = &mut tables[table_index];
            if table.mask & component_exclude != 0 {
                continue;
            }

            let table_mask = table.mask;
            let entity_indices = &table.entity_indices;
            let mut fetch = Q::fetch(table_mask, &mut table.columns, &element_masks, current_tick);

            for (index, &entity) in entity_indices.iter().enumerate() {
                if (tag_include != 0 || tag_exclude != 0)
                    && !tags_match(tags, entity, tag_include, tag_exclude)
                {
                    continue;
                }
                if changed_mask != 0
                    && !Q::changed_newer(&fetch, index, &element_masks, changed_mask, since_tick)
                {
                    continue;
                }
                f(entity, Q::item(&mut fetch, index));
            }
        }
    }
}

fn tags_match(tags: &[SparseTagSet], entity: Entity, tag_include: u64, tag_exclude: u64) -> bool {
    for (tag_index, tag_set) in tags.iter().enumerate() {
        let tag_mask = 1u64 << (63 - tag_index as u32);
        if tag_include & tag_mask != 0 && !tag_set.contains(entity) {
            return false;
        }
        if tag_exclude & tag_mask != 0 && tag_set.contains(entity) {
            return false;
        }
    }
    true
}

/// A set of components spawned together. Implemented for tuples of
/// registered component types up to eight elements.
pub trait Bundle: sealed::SealedBundle {
    fn component_mask(world: &mut DynWorld) -> u64;
    fn write(self, world: &mut DynWorld, entity: Entity);
}

macro_rules! impl_bundle {
    ($($element:ident),+) => {
        impl<$($element: Send + Sync + Default + 'static),+> sealed::SealedBundle for ($($element,)+) {}

        impl<$($element: Send + Sync + Default + 'static),+> Bundle for ($($element,)+) {
            fn component_mask(world: &mut DynWorld) -> u64 {
                let mut mask = 0;
                $(mask |= world.component_key::<$element>().mask;)+
                mask
            }

            #[allow(non_snake_case)]
            fn write(self, world: &mut DynWorld, entity: Entity) {
                let ($($element,)+) = self;
                $(world.set(entity, $element);)+
            }
        }
    };
}

impl_bundle!(A);
impl_bundle!(A, B);
impl_bundle!(A, B, C);
impl_bundle!(A, B, C, D);
impl_bundle!(A, B, C, D, E);
impl_bundle!(A, B, C, D, E, F);
impl_bundle!(A, B, C, D, E, F, G);
impl_bundle!(A, B, C, D, E, F, G, H);

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default, Clone, Debug, PartialEq)]
    pub struct Position {
        pub x: f32,
        pub y: f32,
    }

    #[derive(Default, Clone, Debug, PartialEq)]
    pub struct Velocity {
        pub x: f32,
        pub y: f32,
    }

    #[derive(Default, Clone, Debug, PartialEq)]
    pub struct Health {
        pub value: f32,
    }

    #[derive(Debug, Clone, PartialEq)]
    pub struct PingEvent {
        pub value: u32,
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
    fn test_register_is_idempotent_and_orders_bits() {
        let mut world = DynWorld::new();
        let position = world.register::<Position>();
        let velocity = world.register::<Velocity>();
        let position_again = world.register::<Position>();

        assert_eq!(position.mask, 1);
        assert_eq!(velocity.mask, 2);
        assert_eq!(position.mask, position_again.mask);
        assert_eq!(world.registry.all_components_mask(), 0b11);
    }

    #[test]
    fn test_spawn_get_set_keyed() {
        let mut world = DynWorld::new();
        let position = world.register::<Position>();
        let velocity = world.register::<Velocity>();

        let entity = world.spawn_entities(position.mask | velocity.mask, 1)[0];
        assert_eq!(
            world.get_keyed(position, entity),
            Some(&Position { x: 0.0, y: 0.0 })
        );

        world.set_keyed(position, entity, Position { x: 1.0, y: 2.0 });
        assert_eq!(world.get_keyed(position, entity).unwrap().x, 1.0);

        world.get_mut_keyed(velocity, entity).unwrap().x = 5.0;
        assert_eq!(world.get_keyed(velocity, entity).unwrap().x, 5.0);
    }

    #[test]
    fn test_set_adds_missing_component_by_migration() {
        let mut world = DynWorld::new();
        let position = world.register::<Position>();
        let health = world.register::<Health>();

        let entity = world.spawn_entities(position.mask, 1)[0];
        world.set_keyed(position, entity, Position { x: 3.0, y: 0.0 });
        world.set_keyed(health, entity, Health { value: 10.0 });

        assert_eq!(
            world.component_mask(entity),
            Some(position.mask | health.mask)
        );
        assert_eq!(
            world.get_keyed(position, entity).unwrap().x,
            3.0,
            "migration must preserve existing component values"
        );
        assert_eq!(world.get_keyed(health, entity).unwrap().value, 10.0);
    }

    #[test]
    fn test_despawn_refuses_stale_and_double() {
        let mut world = DynWorld::new();
        let position = world.register::<Position>();

        let entity = world.spawn_entities(position.mask, 1)[0];
        assert_eq!(world.despawn_entities(&[entity]).len(), 1);
        assert!(world.despawn_entities(&[entity]).is_empty());

        let reused = world.spawn_entities(position.mask, 1)[0];
        assert_eq!(reused.id, entity.id);
        assert_eq!(reused.generation, entity.generation + 1);
        assert!(world.get_keyed(position, entity).is_none());
        assert!(!world.add_components(entity, position.mask));
        assert!(world.is_alive(reused));
        assert!(!world.is_alive(entity));
    }

    #[test]
    fn test_tags_and_structural_log() {
        let mut world = DynWorld::new();
        let position = world.register::<Position>();
        let boss = world.register_tag();

        let entity = world.spawn_entities(position.mask, 1)[0];
        world.add_tag(boss, entity);
        assert!(world.has_tag(boss, entity));
        assert_eq!(world.query_tag(boss).count(), 1);

        let kinds: Vec<StructuralChangeKind> = world
            .structural_changes_since(0)
            .iter()
            .map(|change| change.kind)
            .collect();
        assert_eq!(
            kinds,
            vec![
                StructuralChangeKind::Spawned,
                StructuralChangeKind::TagsAdded
            ]
        );

        world.despawn_entities(&[entity]);
        assert!(!world.has_tag(boss, entity));
    }

    #[test]
    fn test_typed_tier_and_bundles() {
        let mut world = DynWorld::new();

        let entity = world.spawn((Position { x: 1.0, y: 2.0 }, Velocity { x: 3.0, y: 4.0 }));

        assert_eq!(world.get::<Position>(entity).unwrap().x, 1.0);
        assert_eq!(world.get::<Velocity>(entity).unwrap().y, 4.0);
        assert!(world.get::<Health>(entity).is_none());
        assert!(world.has::<Position>(entity));

        world.set(entity, Health { value: 50.0 });
        assert_eq!(world.get::<Health>(entity).unwrap().value, 50.0);

        assert!(world.remove::<Health>(entity));
        assert!(world.get::<Health>(entity).is_none());
    }

    #[test]
    fn test_typed_query_iterates_and_stamps() {
        let mut world = DynWorld::new();
        let moving = world.spawn((Position::default(), Velocity { x: 2.0, y: 0.0 }));
        let still = world.spawn((Position { x: 9.0, y: 9.0 },));

        world.step();

        let mut visited = Vec::new();
        world
            .query::<(&mut Position, &Velocity)>()
            .for_each(|entity, (position, velocity)| {
                position.x += velocity.x;
                visited.push(entity);
            });

        assert_eq!(visited, vec![moving]);
        assert_eq!(world.get::<Position>(moving).unwrap().x, 2.0);
        assert_eq!(world.get::<Position>(still).unwrap().x, 9.0);

        let position_mask = world.component_key::<Position>().mask;
        let changed: Vec<Entity> = world.query_entities_changed(position_mask).collect();
        assert_eq!(
            changed,
            vec![moving],
            "mutable query elements must stamp change ticks"
        );
    }

    #[test]
    fn test_typed_query_filters() {
        let mut world = DynWorld::new();
        let selected = world.register_tag();

        let plain = world.spawn((Position::default(),));
        let tagged = world.spawn((Position::default(),));
        let armored = world.spawn((Position::default(), Health { value: 1.0 }));
        world.add_tag(selected, tagged);

        let mut with_tag = Vec::new();
        world
            .query::<(&Position,)>()
            .with_tag(selected)
            .for_each(|entity, _| with_tag.push(entity));
        assert_eq!(with_tag, vec![tagged]);

        let mut without_health = Vec::new();
        world
            .query::<(&Position,)>()
            .without::<Health>()
            .for_each(|entity, _| without_health.push(entity));
        assert_eq!(without_health.len(), 2);
        assert!(!without_health.contains(&armored));
        assert!(without_health.contains(&plain));
    }

    #[test]
    fn test_typed_query_changed_filter() {
        let mut world = DynWorld::new();
        let first = world.spawn((Position::default(), Velocity::default()));
        let second = world.spawn((Position::default(), Velocity::default()));

        world.step();
        world.get_mut::<Position>(first).unwrap().x = 1.0;

        let mut visited = Vec::new();
        world
            .query::<(&Position, &Velocity)>()
            .changed::<Position>()
            .for_each(|entity, _| visited.push(entity));

        assert_eq!(visited, vec![first]);
        let _ = second;
    }

    #[test]
    #[should_panic(expected = "must not repeat a component type")]
    fn test_typed_query_rejects_repeated_component() {
        let mut world = DynWorld::new();
        world.spawn((Position::default(),));
        world
            .query::<(&mut Position, &Position)>()
            .for_each(|_, _| {});
    }

    #[test]
    fn test_events_cursor_and_step() {
        let mut world = DynWorld::new();

        world.send(PingEvent { value: 1 });
        world.send(PingEvent { value: 2 });

        assert_eq!(world.read_events::<PingEvent>().len(), 2);
        let mut cursor = 0;
        assert_eq!(world.read_events_since::<PingEvent>(cursor).len(), 2);
        cursor = world.event_sequence::<PingEvent>();
        assert!(world.read_events_since::<PingEvent>(cursor).is_empty());

        world.step();
        assert_eq!(world.read_events::<PingEvent>().len(), 2);
        world.step();
        assert!(world.read_events::<PingEvent>().is_empty());
        assert_eq!(world.event_sequence::<PingEvent>(), 2);
    }

    #[test]
    fn test_resources() {
        let mut world = DynWorld::new();
        world.insert_resource(0.016f32);
        assert_eq!(world.resource::<f32>(), Some(&0.016));
        *world.resource_mut::<f32>().unwrap() = 0.033;
        assert_eq!(world.remove_resource::<f32>(), Some(0.033));
        assert!(world.resource::<f32>().is_none());
    }

    #[test]
    fn test_commands() {
        let mut world = DynWorld::new();
        let position = world.register::<Position>();

        let entity = world.spawn_entities(position.mask, 1)[0];
        world.queue_set(entity, Position { x: 7.0, y: 0.0 });
        world.queue_spawn_entities(position.mask, 2);
        world.queue_despawn_entity(entity);

        assert_eq!(world.command_count(), 3);
        assert_eq!(world.get_keyed(position, entity).unwrap().x, 0.0);

        world.apply_commands();

        assert!(world.get_keyed(position, entity).is_none());
        assert_eq!(world.entity_count(), 2);
    }

    #[test]
    fn test_mask_iteration_with_tags() {
        let mut world = DynWorld::new();
        let position = world.register::<Position>();
        let enemy = world.register_tag();

        let entities = world.spawn_entities(position.mask, 3);
        world.add_tag(enemy, entities[0]);
        world.add_tag(enemy, entities[2]);

        let mut count = 0;
        world.for_each(position.mask | enemy.mask, 0, |_entity, _table, _index| {
            count += 1;
        });
        assert_eq!(count, 2);

        count = 0;
        world.for_each_mut(position.mask, enemy.mask, |_entity, table, index| {
            table.column_mut(position)[index].x = 4.0;
            count += 1;
        });
        assert_eq!(count, 1);
        assert_eq!(world.get_keyed(position, entities[1]).unwrap().x, 4.0);
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn test_par_for_each_mut() {
        let mut world = DynWorld::new();
        let position = world.register::<Position>();
        let entities = world.spawn_entities(position.mask, 100);

        world.par_for_each_mut(position.mask, 0, |_entity, table, index| {
            table.column_mut(position)[index].x = 1.0;
        });

        for &entity in &entities {
            assert_eq!(world.get_keyed(position, entity).unwrap().x, 1.0);
        }
    }

    #[derive(Default, Clone)]
    struct ModelEntity {
        mask: u64,
        position: Option<f32>,
        position_changed: bool,
        boss: bool,
    }

    #[test]
    fn test_property_dyn_world_matches_model() {
        for seed in [11u64, 71, 3131] {
            let mut rng = Lcg(seed);
            let mut world = DynWorld::new();
            let position = world.register::<Position>();
            let velocity = world.register::<Velocity>();
            let health = world.register::<Health>();
            let boss = world.register_tag();
            let component_masks = [position.mask, velocity.mask, health.mask];

            let mut model: HashMap<Entity, ModelEntity> = HashMap::new();
            let mut handles: Vec<Entity> = Vec::new();
            let mut pending_pings: Vec<u32> = Vec::new();
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

            for _ in 0..3000 {
                match rng.next() % 12 {
                    0 | 1 => {
                        let mask = random_mask(&mut rng);
                        let entity = world.spawn_entities(mask, 1)[0];
                        model.insert(
                            entity,
                            ModelEntity {
                                mask,
                                position: (mask & position.mask != 0).then_some(0.0),
                                position_changed: mask & position.mask != 0,
                                ..Default::default()
                            },
                        );
                        handles.push(entity);
                    }
                    2 => {
                        if let Some(entity) = pick(&mut rng, &handles) {
                            let despawned = world.despawn_entities(&[entity]);
                            let was_live = model.remove(&entity).is_some();
                            assert_eq!(despawned.len() == 1, was_live);
                        }
                    }
                    3 => {
                        if let Some(entity) = pick(&mut rng, &handles) {
                            let mask = random_mask(&mut rng);
                            let accepted = world.add_components(entity, mask);
                            match model.get_mut(&entity) {
                                Some(model_entity) => {
                                    assert!(accepted);
                                    let migrated = mask & !model_entity.mask != 0;
                                    if mask & position.mask != 0
                                        && model_entity.mask & position.mask == 0
                                    {
                                        model_entity.position = Some(0.0);
                                    }
                                    model_entity.mask |= mask;
                                    if migrated && model_entity.mask & position.mask != 0 {
                                        model_entity.position_changed = true;
                                    }
                                }
                                None => assert!(!accepted),
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
                                    let migrated = mask & model_entity.mask != 0;
                                    if mask & position.mask != 0 {
                                        model_entity.position = None;
                                    }
                                    model_entity.mask &= !mask;
                                    if migrated && model_entity.mask & position.mask != 0 {
                                        model_entity.position_changed = true;
                                    }
                                }
                                None => assert!(!accepted),
                            }
                        }
                    }
                    5 => {
                        if let Some(entity) = pick(&mut rng, &handles) {
                            let value = (rng.next() % 1000) as f32;
                            world.set_keyed(position, entity, Position { x: value, y: 0.0 });
                            match model.get_mut(&entity) {
                                Some(model_entity) => {
                                    model_entity.mask |= position.mask;
                                    model_entity.position = Some(value);
                                    model_entity.position_changed = true;
                                }
                                None => assert!(world.get_keyed(position, entity).is_none()),
                            }
                        }
                    }
                    6 => {
                        if let Some(entity) = pick(&mut rng, &handles) {
                            world.add_tag(boss, entity);
                            if let Some(model_entity) = model.get_mut(&entity) {
                                model_entity.boss = true;
                            }
                            assert_eq!(
                                world.has_tag(boss, entity),
                                model.get(&entity).map(|m| m.boss).unwrap_or(false)
                            );
                        }
                    }
                    7 => {
                        if let Some(entity) = pick(&mut rng, &handles) {
                            let removed = world.remove_tag(boss, entity);
                            let expected = match model.get_mut(&entity) {
                                Some(model_entity) => {
                                    let had = model_entity.boss;
                                    model_entity.boss = false;
                                    had
                                }
                                None => false,
                            };
                            assert_eq!(removed, expected);
                        }
                    }
                    8 => {
                        if let Some(entity) = pick(&mut rng, &handles) {
                            world.queue_set(entity, Health { value: 1.0 });
                            let live = model.contains_key(&entity);
                            world.apply_commands();
                            if live {
                                let model_entity = model.get_mut(&entity).unwrap();
                                let migrated = model_entity.mask & health.mask == 0;
                                model_entity.mask |= health.mask;
                                if migrated && model_entity.mask & position.mask != 0 {
                                    model_entity.position_changed = true;
                                }
                            }
                        }
                    }
                    9 => {
                        let value = rng.next() as u32;
                        world.send(PingEvent { value });
                        pending_pings.push(value);
                        total_pings += 1;
                    }
                    _ => {
                        let changed: std::collections::HashSet<Entity> =
                            world.query_entities_changed(position.mask).collect();
                        let expected: std::collections::HashSet<Entity> = model
                            .iter()
                            .filter(|(_, model_entity)| {
                                model_entity.mask & position.mask != 0
                                    && model_entity.position_changed
                            })
                            .map(|(&entity, _)| entity)
                            .collect();
                        assert_eq!(
                            changed, expected,
                            "changed-query set diverged with seed {seed}"
                        );

                        world.step();
                        for model_entity in model.values_mut() {
                            model_entity.position_changed = false;
                        }

                        let buffered: Vec<u32> = world
                            .read_events::<PingEvent>()
                            .iter()
                            .map(|ping| ping.value)
                            .collect();
                        assert_eq!(buffered, pending_pings);
                        assert_eq!(world.event_sequence::<PingEvent>(), total_pings);
                        pending_pings.clear();
                    }
                }
            }

            assert_eq!(world.entity_count(), model.len());
            for (&entity, model_entity) in &model {
                assert_eq!(world.component_mask(entity), Some(model_entity.mask));
                assert_eq!(
                    world.get_keyed(position, entity).map(|p| p.x),
                    model_entity.position
                );
                assert_eq!(world.has_tag(boss, entity), model_entity.boss);
                assert!(world.is_alive(entity));
            }
            for &handle in &handles {
                if !model.contains_key(&handle) {
                    assert_eq!(world.component_mask(handle), None);
                    assert!(!world.is_alive(handle));
                    assert!(!world.has_tag(boss, handle));
                }
            }
            for mask in component_masks {
                let expected = model
                    .values()
                    .filter(|model_entity| model_entity.mask & mask == mask)
                    .count();
                assert_eq!(world.query_entities(mask).count(), expected);
            }
        }
    }

    mod differential {
        use super::*;

        crate::ecs! {
            StaticWorld {
                position: Position => DIFF_POSITION,
                velocity: Velocity => DIFF_VELOCITY,
                health: Health => DIFF_HEALTH,
            }
            Tags {
                boss => DIFF_BOSS,
            }
            DiffResources {
                _unused: f32,
            }
        }

        /// Drives the macro-generated world and the dynamic world with one
        /// seeded op stream and requires identical observable state. The
        /// static world, hardened by its own property suite, acts as the
        /// executable specification for the dynamic one. Allocators evolve
        /// identically, so handles are comparable directly.
        #[test]
        fn test_differential_dyn_world_matches_static_world() {
            for seed in [5u64, 555, 314159] {
                let mut rng = Lcg(seed);

                let mut static_world = StaticWorld::default();
                let mut dyn_world = DynWorld::new();
                let position = dyn_world.register::<Position>();
                let velocity = dyn_world.register::<Velocity>();
                let health = dyn_world.register::<Health>();
                let boss = dyn_world.register_tag();

                assert_eq!(position.mask, DIFF_POSITION);
                assert_eq!(velocity.mask, DIFF_VELOCITY);
                assert_eq!(health.mask, DIFF_HEALTH);

                let mut handles: Vec<Entity> = Vec::new();

                let random_mask = |rng: &mut Lcg| {
                    let mut mask = 0;
                    for component in [DIFF_POSITION, DIFF_VELOCITY, DIFF_HEALTH] {
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

                static_world.step();
                dyn_world.step();

                for _ in 0..3000 {
                    match rng.next() % 10 {
                        0 | 1 => {
                            let mask = random_mask(&mut rng);
                            let static_entity = static_world.spawn_entities(mask, 1)[0];
                            let dyn_entity = dyn_world.spawn_entities(mask, 1)[0];
                            assert_eq!(
                                static_entity, dyn_entity,
                                "allocators must evolve identically"
                            );
                            handles.push(static_entity);
                        }
                        2 => {
                            if let Some(entity) = pick(&mut rng, &handles) {
                                let static_despawned = static_world.despawn_entities(&[entity]);
                                let dyn_despawned = dyn_world.despawn_entities(&[entity]);
                                assert_eq!(static_despawned, dyn_despawned);
                            }
                        }
                        3 => {
                            if let Some(entity) = pick(&mut rng, &handles) {
                                let mask = random_mask(&mut rng);
                                assert_eq!(
                                    static_world.add_components(entity, mask),
                                    dyn_world.add_components(entity, mask)
                                );
                            }
                        }
                        4 => {
                            if let Some(entity) = pick(&mut rng, &handles) {
                                let mask = random_mask(&mut rng);
                                assert_eq!(
                                    static_world.remove_components(entity, mask),
                                    dyn_world.remove_components(entity, mask)
                                );
                            }
                        }
                        5 => {
                            if let Some(entity) = pick(&mut rng, &handles) {
                                let value = (rng.next() % 1000) as f32;
                                static_world.set_position(entity, Position { x: value, y: 0.0 });
                                dyn_world.set_keyed(
                                    position,
                                    entity,
                                    Position { x: value, y: 0.0 },
                                );
                            }
                        }
                        6 => {
                            if let Some(entity) = pick(&mut rng, &handles) {
                                static_world.add_boss(entity);
                                dyn_world.add_tag(boss, entity);
                            }
                        }
                        7 => {
                            if let Some(entity) = pick(&mut rng, &handles) {
                                assert_eq!(
                                    static_world.remove_boss(entity),
                                    dyn_world.remove_tag(boss, entity)
                                );
                            }
                        }
                        8 => {
                            if let Some(entity) = pick(&mut rng, &handles) {
                                let value = (rng.next() % 1000) as f32;
                                static_world
                                    .queue_set_position(entity, Position { x: value, y: 0.0 });
                                dyn_world.queue_set(entity, Position { x: value, y: 0.0 });
                                static_world.apply_commands();
                                dyn_world.apply_commands();
                            }
                        }
                        _ => {
                            let static_changed: std::collections::HashSet<Entity> =
                                static_world.query_entities_changed(DIFF_POSITION).collect();
                            let dyn_changed: std::collections::HashSet<Entity> =
                                dyn_world.query_entities_changed(DIFF_POSITION).collect();
                            assert_eq!(
                                static_changed, dyn_changed,
                                "changed sets diverged with seed {seed}"
                            );

                            static_world.step();
                            dyn_world.step();
                        }
                    }
                }

                assert_eq!(static_world.entity_count(), dyn_world.entity_count());
                for &handle in &handles {
                    assert_eq!(
                        static_world.component_mask(handle),
                        dyn_world.component_mask(handle),
                        "masks diverged for {handle:?} with seed {seed}"
                    );
                    assert_eq!(
                        static_world.get_position(handle),
                        dyn_world.get_keyed(position, handle),
                        "position diverged for {handle:?} with seed {seed}"
                    );
                    assert_eq!(
                        static_world.has_boss(handle),
                        dyn_world.has_tag(boss, handle)
                    );
                    assert_eq!(static_world.is_alive(handle), dyn_world.is_alive(handle));
                }
                for mask in [
                    DIFF_POSITION,
                    DIFF_VELOCITY,
                    DIFF_HEALTH,
                    DIFF_POSITION | DIFF_VELOCITY,
                ] {
                    assert_eq!(
                        static_world.query_entities(mask).count(),
                        dyn_world.query_entities(mask).count()
                    );
                }
            }
        }
    }
}
