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
//! thread-safety come from the vec itself, and a type must exist at the
//! registration call site.
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
//! Query tuples take up to eight elements of `&T`, `&mut T`, `Option<&T>`,
//! or `Option<&mut T>`; optional elements yield `None` instead of narrowing
//! the match. On a shared borrow, [`DynWorld::query_ref`] runs read-only
//! tuples as a real [`Iterator`]:
//!
//! ```rust
//! # use freecs::dynamic::DynWorld;
//! # #[derive(Default, Clone, Debug, PartialEq)]
//! # struct Position { x: f32, y: f32 }
//! # #[derive(Default, Clone, Debug)]
//! # struct Velocity { x: f32, y: f32 }
//! # let mut world = DynWorld::new();
//! # world.spawn((Position { x: 1.0, y: 2.0 }, Velocity { x: 1.0, y: 2.0 }));
//! let total: f32 = world
//!     .query_ref::<(&Position, Option<&Velocity>)>()
//!     .iter()
//!     .map(|(_entity, (position, velocity))| {
//!         position.x + velocity.map_or(0.0, |velocity| velocity.x)
//!     })
//!     .sum();
//! # assert_eq!(total, 2.0);
//! ```
//!
//! Tags can be named by marker types (`world.add_tag_type::<Selected>(entity)`,
//! `.with_tag_type::<Selected>()`), registering lazily like components, or
//! held as [`TagKey`] values; both forms are the same sparse sets underneath.
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

impl<T> std::fmt::Debug for ComponentKey<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ComponentKey")
            .field("component_index", &self.component_index)
            .field("mask", &self.mask)
            .field("registry_id", &self.registry_id)
            .finish()
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
    pub tags_by_type: HashMap<TypeId, u32>,
    #[cfg(feature = "snapshot")]
    pub codecs: Vec<Option<ComponentCodec>>,
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
            tags_by_type: HashMap::new(),
            #[cfg(feature = "snapshot")]
            codecs: Vec::new(),
        }
    }

    /// Registers `T` if it is not already registered and returns its key.
    /// Idempotent per type. `Default` is required because archetype migration
    /// moves values with `mem::take`, and `Send + Sync` because columns are
    /// shared across threads by the parallel iteration paths.
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
        #[cfg(feature = "snapshot")]
        self.codecs.push(None);
        self.key_for(component_index)
    }

    /// Registers `T` with a snapshot codec, so worlds carrying it can be
    /// serialized. The codec encodes whole columns with postcard; register
    /// through [`register_codec`](Self::register_codec) instead to supply a
    /// custom byte format.
    #[cfg(feature = "snapshot")]
    pub fn register_serde<T>(&mut self) -> ComponentKey<T>
    where
        T: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + Default + 'static,
    {
        self.register_codec::<T>(ComponentCodec {
            encode_column: encode_column_postcard::<T>,
            decode_column: decode_column_postcard::<T>,
        })
    }

    /// Registers `T` with an explicit snapshot codec.
    #[cfg(feature = "snapshot")]
    pub fn register_codec<T: Send + Sync + Default + 'static>(
        &mut self,
        codec: ComponentCodec,
    ) -> ComponentKey<T> {
        let key = self.register::<T>();
        self.codecs[key.component_index as usize] = Some(codec);
        key
    }

    pub fn register_tag(&mut self) -> TagKey {
        assert!(
            (self.components.len() + self.tag_count as usize) < 64,
            "components plus tags must fit in a u64 mask"
        );
        let tag_index = self.tag_count;
        self.tag_count += 1;
        self.tag_key_for(tag_index)
    }

    /// Registers a tag identified by the marker type `T` if it is not
    /// already registered and returns its key. Idempotent per type; the tag
    /// is an ordinary sparse-set tag underneath, `T` is only its name.
    pub fn register_tag_type<T: 'static>(&mut self) -> TagKey {
        if let Some(&tag_index) = self.tags_by_type.get(&TypeId::of::<T>()) {
            return self.tag_key_for(tag_index);
        }
        let key = self.register_tag();
        self.tags_by_type.insert(TypeId::of::<T>(), key.tag_index);
        key
    }

    /// Resolves the marker type `T`'s tag key without registering.
    pub fn lookup_tag_type<T: 'static>(&self) -> Option<TagKey> {
        let &tag_index = self.tags_by_type.get(&TypeId::of::<T>())?;
        Some(self.tag_key_for(tag_index))
    }

    fn tag_key_for(&self, tag_index: u32) -> TagKey {
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
    /// Typed view of one column. Costs a popcount and a downcast per call,
    /// so hoist it outside per-entity loops; inside a hot loop, resolve
    /// columns once per table via [`columns_pair`](Self::columns_pair) or the
    /// typed query tier instead.
    #[inline]
    pub fn column<T: 'static>(&self, key: ComponentKey<T>) -> &[T] {
        let position = column_position(self.mask, key.mask);
        column_vec::<T>(self.columns[position].data.as_ref())
    }

    /// Mutable raw column access. Does not stamp change ticks, and costs a
    /// popcount and a downcast per call, so hoist it outside per-entity
    /// loops. Use the typed query tier when change detection matters.
    #[inline]
    pub fn column_mut<T: 'static>(&mut self, key: ComponentKey<T>) -> &mut [T] {
        let position = column_position(self.mask, key.mask);
        column_vec_mut::<T>(self.columns[position].data.as_mut())
    }

    pub fn has_component(&self, mask: u64) -> bool {
        self.mask & mask != 0
    }

    /// Stamps every row of the masked columns as changed at `tick`, the
    /// bulk opt-in for whole-column raw writes: after filling a column
    /// through `column_mut` or `columns_pair` inside a table loop, one call
    /// here makes the pass visible to tick-diffing consumers at zero
    /// per-row cost during the write. Pass the world's `current_tick()`.
    pub fn mark_columns_changed(&mut self, mask: u64, tick: u32) {
        let mut remaining = self.mask & mask;
        while remaining != 0 {
            let component_mask = remaining & remaining.wrapping_neg();
            remaining &= remaining - 1;
            let position = column_position(self.mask, component_mask);
            let column = &mut self.columns[position];
            column.changed.fill(tick);
            column.peak_changed = tick;
        }
    }

    /// Disjoint mutable and shared column slices in one call, for hoisting
    /// column access out of per-entity loops. Panics if the two components
    /// are the same or either is absent from this table.
    #[inline]
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

    /// Two disjoint mutable column slices in one call, the mut-and-mut
    /// counterpart of [`columns_pair`](Self::columns_pair).
    #[inline]
    pub fn columns_pair_mut<A: 'static, B: 'static>(
        &mut self,
        first: ComponentKey<A>,
        second: ComponentKey<B>,
    ) -> (&mut [A], &mut [B]) {
        let first_position = column_position(self.mask, first.mask);
        let second_position = column_position(self.mask, second.mask);
        let [first_slot, second_slot] = self
            .columns
            .get_disjoint_mut([first_position, second_position])
            .expect("columns_pair_mut components must be distinct");
        (
            column_vec_mut::<A>(first_slot.data.as_mut()).as_mut_slice(),
            column_vec_mut::<B>(second_slot.data.as_mut()).as_mut_slice(),
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
    /// When true, a live handle this world has never stored gets a row
    /// inserted on `add_components`/`set`, gated by the retired-generation
    /// check. Grouped worlds under [`DynEcs`] need this; a standalone world
    /// leaves it false so unknown handles are refused outright.
    pub insert_missing_rows: bool,
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
        let tag_count = registry.tag_count as usize;
        let mut world = Self {
            registry,
            allocator: EntityAllocator::default(),
            insert_missing_rows: false,
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
        };
        while world.tags.len() < tag_count {
            world.tags.push(SparseTagSet::default());
        }
        world
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

    /// The lazy typed tier for tags: resolves or registers the marker type
    /// `T`'s tag and returns its key.
    pub fn tag_key<T: 'static>(&mut self) -> TagKey {
        let key = self.registry.register_tag_type::<T>();
        while self.tags.len() < self.registry.tag_count as usize {
            self.tags.push(SparseTagSet::default());
        }
        key
    }

    /// Resolves the marker type `T`'s tag key without registering. Returns
    /// `None` for marker types this world has never seen.
    pub fn lookup_tag_key<T: 'static>(&self) -> Option<TagKey> {
        self.registry.lookup_tag_type::<T>()
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
        let mut allocator = std::mem::take(&mut self.allocator);
        let entities = self.spawn_entities_in(&mut allocator, mask, count);
        self.allocator = allocator;
        entities
    }

    /// Spawns through an external allocator, the grouped-worlds form used by
    /// [`DynEcs`].
    pub fn spawn_entities_in(
        &mut self,
        allocator: &mut EntityAllocator,
        mask: u64,
        count: usize,
    ) -> Vec<Entity> {
        let table_index = self.get_or_create_table(mask);
        let current_tick = self.current_tick;

        let mut entities = Vec::new();
        allocator.allocate_batch(count, &mut entities);

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
        let mut allocator = std::mem::take(&mut self.allocator);
        let despawned = self.despawn_entities_in(&mut allocator, entities);
        self.allocator = allocator;
        despawned
    }

    /// Despawns every entity carrying at least one of the bundle's component
    /// types, so one call clears several kinds of entity at once:
    /// `world.despawn_with_any::<(Projectile, VisualEffect)>()`. Types
    /// register lazily; a type nothing carries despawns nothing. Returns the
    /// despawned entities.
    pub fn despawn_with_any<B: Bundle>(&mut self) -> Vec<Entity> {
        let mask = B::component_mask(self);
        let entities: Vec<Entity> = self
            .tables
            .iter()
            .filter(|table| table.mask & mask != 0)
            .flat_map(|table| table.entity_indices.iter().copied())
            .collect();
        self.despawn_entities(&entities)
    }

    /// Despawns through an external allocator, the grouped-worlds form used
    /// by [`DynEcs`].
    pub fn despawn_entities_in(
        &mut self,
        allocator: &mut EntityAllocator,
        entities: &[Entity],
    ) -> Vec<Entity> {
        let mut despawned = Vec::with_capacity(entities.len());
        for &entity in entities {
            if allocator.deallocate(entity) {
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
            return self.insert_missing_rows && self.insert_row(entity, mask);
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

    /// Creates a row for a live handle this world has never stored. Refuses
    /// stale handles via the generation the despawn broadcast retired.
    fn insert_row(&mut self, entity: Entity, mask: u64) -> bool {
        if let Some(location) = self.entity_locations.get(entity.id)
            && (location.allocated || location.generation != entity.generation)
        {
            return false;
        }

        let table_index = self.get_or_create_table(mask);
        let current_tick = self.current_tick;
        let start_index = self.tables[table_index].entity_indices.len();
        {
            let table = &mut self.tables[table_index];
            for column in &mut table.columns {
                let info = &self.registry.components[column.component_index as usize];
                (info.push_default)(column.data.as_mut(), 1);
                column.changed.push(current_tick);
                column.peak_changed = current_tick;
            }
            table.entity_indices.push(entity);
        }
        insert_location(
            &mut self.entity_locations,
            entity,
            (table_index, start_index),
        );
        self.record_structural(entity, StructuralChangeKind::Spawned, mask);
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

    #[inline]
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

    #[inline]
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

    /// Explicitly stamps change ticks for the masked components on one
    /// entity. This is the opt-in for raw-tier writes: table access through
    /// [`for_each_tables_mut`](Self::for_each_tables_mut), `column_mut`, and
    /// `columns_pair` does not stamp, so follow such writes with this call
    /// when downstream consumers diff by ticks. Returns false if the entity
    /// is missing or its table lacks every masked component.
    pub fn mark_changed(&mut self, entity: Entity, mask: u64) -> bool {
        let Some((table_index, array_index)) = get_location(&self.entity_locations, entity) else {
            return false;
        };
        let current_tick = self.current_tick;
        let table = &mut self.tables[table_index];
        let present = table.mask & mask & self.registry.all_components_mask();
        if present == 0 {
            return false;
        }
        let mut remaining = present;
        while remaining != 0 {
            let component_mask = remaining & remaining.wrapping_neg();
            remaining &= remaining - 1;
            let position = column_position(table.mask, component_mask);
            let column = &mut table.columns[position];
            column.changed[array_index] = current_tick;
            column.peak_changed = current_tick;
        }
        true
    }

    pub fn set_keyed<T: 'static>(&mut self, key: ComponentKey<T>, entity: Entity, value: T) {
        self.check_key(key.registry_id);
        let current_tick = self.current_tick;
        if let Some((table_index, array_index)) = get_location(&self.entity_locations, entity) {
            let table = &mut self.tables[table_index];
            if table.mask & key.mask != 0 {
                let position = column_position(table.mask, key.mask);
                let column = &mut table.columns[position];
                column_vec_mut::<T>(column.data.as_mut())[array_index] = value;
                column.changed[array_index] = current_tick;
                column.peak_changed = current_tick;
                return;
            }
        }
        if self.add_components(entity, key.mask)
            && let Some((table_index, array_index)) = get_location(&self.entity_locations, entity)
        {
            let table = &mut self.tables[table_index];
            let position = column_position(table.mask, key.mask);
            let column = &mut table.columns[position];
            column_vec_mut::<T>(column.data.as_mut())[array_index] = value;
            column.changed[array_index] = current_tick;
            column.peak_changed = current_tick;
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

    /// Adds the marker type `T`'s tag to an entity, registering the tag on
    /// first use.
    pub fn add_tag_type<T: 'static>(&mut self, entity: Entity) {
        let key = self.tag_key::<T>();
        self.add_tag(key, entity);
    }

    /// Removes the marker type `T`'s tag from an entity. Unregistered marker
    /// types remove nothing.
    pub fn remove_tag_type<T: 'static>(&mut self, entity: Entity) -> bool {
        match self.lookup_tag_key::<T>() {
            Some(key) => self.remove_tag(key, entity),
            None => false,
        }
    }

    /// Whether an entity carries the marker type `T`'s tag. Unregistered
    /// marker types read as absent.
    pub fn has_tag_type<T: 'static>(&self, entity: Entity) -> bool {
        match self.lookup_tag_key::<T>() {
            Some(key) => self.has_tag(key, entity),
            None => false,
        }
    }

    /// Iterates entities carrying the marker type `T`'s tag. Unregistered
    /// marker types match nothing.
    pub fn query_tag_type<T: 'static>(&self) -> impl Iterator<Item = Entity> + '_ {
        self.lookup_tag_key::<T>()
            .into_iter()
            .flat_map(|key| self.tags[key.tag_index as usize].iter())
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

    /// Entity-granular iteration with tag filtering, mirroring the static
    /// worlds' shape. Calling `column`/`column_mut` inside the closure pays a
    /// downcast per entity; prefer the typed query tier, or
    /// [`for_each_tables_mut`](Self::for_each_tables_mut) with columns
    /// hoisted, for hot loops.
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

    /// Takes `R` out of the world, runs the closure with the world and the
    /// resource as independent borrows, then puts the resource back. This is
    /// the take/put pattern for systems that mutate both a resource and the
    /// world in one pass; the resource is absent from the world inside the
    /// closure and is reinserted even when the closure panics, before the
    /// panic resumes. Panics if `R` is not present.
    pub fn resource_scope<R: Send + Sync + 'static, T>(
        &mut self,
        f: impl FnOnce(&mut DynWorld, &mut R) -> T,
    ) -> T {
        let mut resource = self
            .remove_resource::<R>()
            .expect("resource_scope requires the resource to be present");
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(self, &mut resource)));
        self.insert_resource(resource);
        match result {
            Ok(value) => value,
            Err(panic) => std::panic::resume_unwind(panic),
        }
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

    pub fn queue_add_tag_type<T: 'static>(&mut self, entity: Entity) {
        let key = self.tag_key::<T>();
        self.queue_add_tag(key, entity);
    }

    pub fn queue_remove_tag_type<T: 'static>(&mut self, entity: Entity) {
        let key = self.tag_key::<T>();
        self.queue_remove_tag(key, entity);
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

    /// Resolves `T`'s key without registering. Returns `None` for types this
    /// world has never seen.
    pub fn lookup_key<T: Send + Sync + Default + 'static>(&self) -> Option<ComponentKey<T>> {
        let &component_index = self.registry.components_by_type.get(&TypeId::of::<T>())?;
        Some(self.registry.key_for::<T>(component_index))
    }

    /// Typed read. Unregistered types read as absent.
    pub fn get<T: Send + Sync + Default + 'static>(&self, entity: Entity) -> Option<&T> {
        let key = self.lookup_key::<T>()?;
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
    /// borrow mutability comes from the tuple (`&T`, `&mut T`, and their
    /// `Option` forms), and mutable elements stamp change ticks per visited
    /// entity.
    pub fn query<Q: QueryTuple>(&mut self) -> DynQuery<'_, Q> {
        let include = Q::component_mask(self);
        DynQuery {
            world: self,
            include,
            exclude: 0,
            changed_mask: 0,
            include_tag_sets: [None; 4],
            exclude_tag_sets: [None; 4],
            marker: PhantomData,
        }
    }

    /// Starts a read-only typed query on a shared world borrow. Tuples take
    /// `&T` and `Option<&T>` elements only, nothing registers, and a
    /// component type this world has never seen matches no entities. Unlike
    /// [`query`](Self::query), the result is a real [`Iterator`], so it
    /// composes with adapters and its items can be collected and outlive the
    /// iteration.
    pub fn query_ref<Q: ReadQueryTuple>(&self) -> DynQueryRef<'_, Q> {
        DynQueryRef {
            world: self,
            include: 0,
            exclude: 0,
            changed_mask: 0,
            include_tag_sets: [None; 4],
            exclude_tag_sets: [None; 4],
            dead: false,
            marker: PhantomData,
        }
    }
}

/// A group of dynamic worlds over one shared entity allocator, the dynamic
/// counterpart of the macro's multi-world form. Each world carries its own
/// registry and full 64-bit mask space, so the group's component budget is
/// 64 per world rather than 64 total. One entity can hold rows in any
/// combination of worlds; despawning retires it everywhere and broadcasts
/// the bumped generation, so stale handles are refused in every world,
/// including worlds that never stored the entity.
///
/// Group tags live outside any world's mask space as plain sparse sets.
/// Filter per-world queries by them with
/// [`with_tag_set`](DynQuery::with_tag_set), or check membership directly.
///
/// ```rust
/// use freecs::dynamic::{ComponentRegistry, DynEcs};
///
/// #[derive(Default, Clone, Debug)]
/// struct Position { x: f32 }
///
/// #[derive(Default, Clone, Debug)]
/// struct Sprite { id: u32 }
///
/// let mut ecs = DynEcs::new();
/// let core = ecs.add_world(ComponentRegistry::new());
/// let render = ecs.add_world(ComponentRegistry::new());
/// let selected = ecs.register_tag();
///
/// let entity = ecs.spawn();
/// ecs.worlds[core].set(entity, Position { x: 1.0 });
/// ecs.worlds[render].set(entity, Sprite { id: 7 });
/// ecs.add_tag(selected, entity);
///
/// assert_eq!(ecs.worlds[core].get::<Position>(entity).unwrap().x, 1.0);
/// assert!(ecs.despawn(entity));
/// assert!(ecs.worlds[render].get::<Sprite>(entity).is_none());
/// assert!(!ecs.has_tag(selected, entity));
/// ```
#[derive(Default)]
pub struct DynEcs {
    pub allocator: EntityAllocator,
    pub worlds: Vec<DynWorld>,
    pub tags: Vec<SparseTagSet>,
}

impl DynEcs {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a world built from the given registry and returns its index.
    /// Grouped worlds insert rows for live handles they have never stored,
    /// which is what lets an entity gain components per world lazily.
    pub fn add_world(&mut self, registry: ComponentRegistry) -> usize {
        let mut world = DynWorld::from_registry(registry);
        world.insert_missing_rows = true;
        self.worlds.push(world);
        self.worlds.len() - 1
    }

    /// Allocates a handle with no rows anywhere. Give it components through
    /// any member world's `set`/`add_components`.
    pub fn spawn(&mut self) -> Entity {
        self.allocator.allocate()
    }

    pub fn spawn_count(&mut self, count: usize) -> Vec<Entity> {
        let mut entities = Vec::new();
        self.allocator.allocate_batch(count, &mut entities);
        entities
    }

    /// Spawns entities with rows in one member world.
    pub fn spawn_entities(&mut self, world_index: usize, mask: u64, count: usize) -> Vec<Entity> {
        self.worlds[world_index].spawn_entities_in(&mut self.allocator, mask, count)
    }

    pub fn is_alive(&self, entity: Entity) -> bool {
        self.allocator.is_alive(entity)
    }

    /// Despawns the entity across every world, dropping its group tags.
    /// Returns false for stale or already-despawned handles. Retirement
    /// broadcasts the bumped generation into every world's location table,
    /// 16 bytes per despawned id per world, which is what makes stale writes
    /// refusable everywhere.
    pub fn despawn(&mut self, entity: Entity) -> bool {
        if !self.allocator.deallocate(entity) {
            return false;
        }
        for world in &mut self.worlds {
            world.retire_entity(entity);
        }
        for tag_set in &mut self.tags {
            tag_set.remove(entity);
        }
        true
    }

    pub fn despawn_entities(&mut self, entities: &[Entity]) -> Vec<Entity> {
        let mut despawned = Vec::with_capacity(entities.len());
        for &entity in entities {
            if self.despawn(entity) {
                despawned.push(entity);
            }
        }
        despawned
    }

    /// Registers a group-level tag and returns its index. Group tags have no
    /// mask bit; they filter queries by set reference.
    pub fn register_tag(&mut self) -> usize {
        self.tags.push(SparseTagSet::default());
        self.tags.len() - 1
    }

    pub fn add_tag(&mut self, tag_index: usize, entity: Entity) {
        if self.allocator.is_alive(entity) {
            self.tags[tag_index].insert(entity);
        }
    }

    pub fn remove_tag(&mut self, tag_index: usize, entity: Entity) -> bool {
        self.tags[tag_index].remove(entity)
    }

    pub fn has_tag(&self, tag_index: usize, entity: Entity) -> bool {
        self.tags[tag_index].contains(entity)
    }

    pub fn query_tag(&self, tag_index: usize) -> impl Iterator<Item = Entity> + '_ {
        self.tags[tag_index].iter()
    }

    /// Steps every member world: event expiry and change-detection ticks.
    pub fn step(&mut self) {
        for world in &mut self.worlds {
            world.step();
        }
    }
}

#[cfg(feature = "snapshot")]
mod snapshot {
    use super::*;

    /// Column byte codec for one component type, plain function pointers
    /// like the rest of the registry's vtable. The built-in pair encodes the
    /// whole `Vec<T>` with postcard; any byte format works as long as encode
    /// and decode agree.
    #[derive(Clone, Copy)]
    pub struct ComponentCodec {
        pub encode_column: fn(&(dyn Any + Send + Sync)) -> Result<Vec<u8>, SnapshotError>,
        pub decode_column: fn(&[u8]) -> Result<ErasedColumn, SnapshotError>,
    }

    pub(super) fn encode_column_postcard<T>(
        column: &(dyn Any + Send + Sync),
    ) -> Result<Vec<u8>, SnapshotError>
    where
        T: serde::Serialize + Send + Sync + Default + 'static,
    {
        postcard::to_allocvec(column_vec::<T>(column))
            .map_err(|error| SnapshotError::Codec(error.to_string()))
    }

    pub(super) fn decode_column_postcard<T>(bytes: &[u8]) -> Result<ErasedColumn, SnapshotError>
    where
        T: serde::de::DeserializeOwned + Send + Sync + Default + 'static,
    {
        let column: Vec<T> =
            postcard::from_bytes(bytes).map_err(|error| SnapshotError::Codec(error.to_string()))?;
        Ok(Box::new(column))
    }

    #[derive(Debug, Clone, PartialEq)]
    pub enum SnapshotError {
        /// A component present in a table has no registered codec.
        MissingCodec(&'static str),
        /// The registry's component names or order do not match the snapshot.
        SchemaMismatch { expected: String, found: String },
        /// A column failed to encode or decode.
        Codec(String),
    }

    impl std::fmt::Display for SnapshotError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                SnapshotError::MissingCodec(type_name) => {
                    write!(formatter, "component {type_name} has no snapshot codec")
                }
                SnapshotError::SchemaMismatch { expected, found } => write!(
                    formatter,
                    "registry schema mismatch: snapshot has {expected}, registry has {found}"
                ),
                SnapshotError::Codec(message) => write!(formatter, "codec error: {message}"),
            }
        }
    }

    impl std::error::Error for SnapshotError {}

    /// One archetype table's serialized form: the mask, the entity handles,
    /// and one postcard-or-custom byte payload per column in ascending bit
    /// order.
    #[derive(serde::Serialize, serde::Deserialize)]
    pub struct DynTableSnapshot {
        pub mask: u64,
        pub entities: Vec<Entity>,
        pub columns: Vec<Vec<u8>>,
    }

    /// A serializable image of a [`DynWorld`]: schema names for validation,
    /// allocator state, tables, tag memberships, and tick counters. Events,
    /// pending commands, and the structural log are transient and not
    /// captured. Serialize this with any serde format.
    #[derive(serde::Serialize, serde::Deserialize)]
    pub struct DynWorldSnapshot {
        pub component_types: Vec<String>,
        pub allocator: EntityAllocator,
        pub tables: Vec<DynTableSnapshot>,
        pub tags: Vec<Vec<Entity>>,
        pub current_tick: u32,
        pub last_tick: u32,
    }

    /// A serializable image of a [`DynEcs`]: the shared allocator, one world
    /// snapshot per member, and group tag memberships.
    #[derive(serde::Serialize, serde::Deserialize)]
    pub struct DynEcsSnapshot {
        pub allocator: EntityAllocator,
        pub worlds: Vec<DynWorldSnapshot>,
        pub tags: Vec<Vec<Entity>>,
    }

    /// Rebuilds the retirement stamps a despawn broadcast would have left,
    /// from allocator liveness: dead ids stamp the next generation, live ids
    /// stamp their current one, so stale-handle refusal survives a restore
    /// even for entities that never had a row in this world.
    fn stamp_retirements(world: &mut DynWorld, slots: &[crate::EntitySlot]) {
        for (id, slot) in slots.iter().enumerate() {
            let id = id as u32;
            let expected_generation = if slot.alive {
                slot.generation
            } else {
                slot.generation.wrapping_add(1)
            };
            let needs_stamp = match world.entity_locations.get(id) {
                None => true,
                Some(location) => !location.allocated,
            };
            if needs_stamp {
                world.entity_locations.ensure_slot(id, expected_generation);
            }
        }
    }

    impl DynWorld {
        /// Captures the world. Fails with [`SnapshotError::MissingCodec`] if
        /// any component stored in a table was registered without a codec.
        pub fn snapshot(&self) -> Result<DynWorldSnapshot, SnapshotError> {
            let mut tables = Vec::with_capacity(self.tables.len());
            for table in &self.tables {
                let mut columns = Vec::with_capacity(table.columns.len());
                for column in &table.columns {
                    let info = &self.registry.components[column.component_index as usize];
                    let codec = self.registry.codecs[column.component_index as usize]
                        .as_ref()
                        .ok_or(SnapshotError::MissingCodec(info.type_name))?;
                    columns.push((codec.encode_column)(column.data.as_ref())?);
                }
                tables.push(DynTableSnapshot {
                    mask: table.mask,
                    entities: table.entity_indices.clone(),
                    columns,
                });
            }

            Ok(DynWorldSnapshot {
                component_types: self
                    .registry
                    .components
                    .iter()
                    .map(|info| info.type_name.to_string())
                    .collect(),
                allocator: EntityAllocator {
                    next_id: self.allocator.next_id,
                    free_ids: self.allocator.free_ids.clone(),
                    slots: self.allocator.slots.clone(),
                },
                tables,
                tags: self
                    .tags
                    .iter()
                    .map(|tag_set| tag_set.iter().collect())
                    .collect(),
                current_tick: self.current_tick,
                last_tick: self.last_tick,
            })
        }

        /// Rebuilds a world from a snapshot over a registry with the same
        /// registration order. The registry may have additional components
        /// appended after the snapshot's schema; masks stay stable because
        /// bits are assigned in registration order. Every restored slot is
        /// stamped with the restored `current_tick`, so change-detection
        /// consumers see the whole world as changed on load.
        pub fn from_snapshot(
            registry: ComponentRegistry,
            snapshot: &DynWorldSnapshot,
        ) -> Result<DynWorld, SnapshotError> {
            for (index, expected) in snapshot.component_types.iter().enumerate() {
                let found = registry
                    .components
                    .get(index)
                    .map(|info| info.type_name)
                    .unwrap_or("<unregistered>");
                if expected != found {
                    return Err(SnapshotError::SchemaMismatch {
                        expected: expected.clone(),
                        found: found.to_string(),
                    });
                }
            }

            let mut world = DynWorld::from_registry(registry);
            world.allocator = EntityAllocator {
                next_id: snapshot.allocator.next_id,
                free_ids: snapshot.allocator.free_ids.clone(),
                slots: snapshot.allocator.slots.clone(),
            };
            world.current_tick = snapshot.current_tick;
            world.last_tick = snapshot.last_tick;

            for table_snapshot in &snapshot.tables {
                let table_index = world.get_or_create_table(table_snapshot.mask);
                let table = &mut world.tables[table_index];
                table.entity_indices = table_snapshot.entities.clone();

                let mut column_payloads = table_snapshot.columns.iter();
                for column in &mut table.columns {
                    let info = &world.registry.components[column.component_index as usize];
                    let codec = world.registry.codecs[column.component_index as usize]
                        .as_ref()
                        .ok_or(SnapshotError::MissingCodec(info.type_name))?;
                    let payload = column_payloads.next().ok_or_else(|| {
                        SnapshotError::Codec("missing column payload".to_string())
                    })?;
                    column.data = (codec.decode_column)(payload)?;
                    column.changed = vec![snapshot.current_tick; table_snapshot.entities.len()];
                    column.peak_changed = snapshot.current_tick;
                }

                for (array_index, &entity) in table_snapshot.entities.iter().enumerate() {
                    insert_location(
                        &mut world.entity_locations,
                        entity,
                        (table_index, array_index),
                    );
                }
            }

            stamp_retirements(&mut world, &snapshot.allocator.slots);

            for (tag_index, tag_entities) in snapshot.tags.iter().enumerate() {
                while world.tags.len() <= tag_index {
                    world.tags.push(SparseTagSet::default());
                }
                for &entity in tag_entities {
                    world.tags[tag_index].insert(entity);
                }
            }

            Ok(world)
        }
    }

    impl DynEcs {
        pub fn snapshot(&self) -> Result<DynEcsSnapshot, SnapshotError> {
            let mut worlds = Vec::with_capacity(self.worlds.len());
            for world in &self.worlds {
                worlds.push(world.snapshot()?);
            }
            Ok(DynEcsSnapshot {
                allocator: EntityAllocator {
                    next_id: self.allocator.next_id,
                    free_ids: self.allocator.free_ids.clone(),
                    slots: self.allocator.slots.clone(),
                },
                worlds,
                tags: self
                    .tags
                    .iter()
                    .map(|tag_set| tag_set.iter().collect())
                    .collect(),
            })
        }

        /// Rebuilds a group from a snapshot and one registry per member
        /// world, in the same order the worlds were added.
        pub fn from_snapshot(
            registries: Vec<ComponentRegistry>,
            snapshot: &DynEcsSnapshot,
        ) -> Result<DynEcs, SnapshotError> {
            if registries.len() != snapshot.worlds.len() {
                return Err(SnapshotError::SchemaMismatch {
                    expected: format!("{} worlds", snapshot.worlds.len()),
                    found: format!("{} registries", registries.len()),
                });
            }

            let mut ecs = DynEcs::new();
            ecs.allocator = EntityAllocator {
                next_id: snapshot.allocator.next_id,
                free_ids: snapshot.allocator.free_ids.clone(),
                slots: snapshot.allocator.slots.clone(),
            };
            for (registry, world_snapshot) in registries.into_iter().zip(&snapshot.worlds) {
                let mut world = DynWorld::from_snapshot(registry, world_snapshot)?;
                world.insert_missing_rows = true;
                stamp_retirements(&mut world, &snapshot.allocator.slots);
                ecs.worlds.push(world);
            }
            for tag_entities in &snapshot.tags {
                let tag_index = ecs.register_tag();
                for &entity in tag_entities {
                    ecs.tags[tag_index].insert(entity);
                }
            }
            Ok(ecs)
        }
    }
}

#[cfg(feature = "snapshot")]
pub use snapshot::{
    ComponentCodec, DynEcsSnapshot, DynTableSnapshot, DynWorldSnapshot, SnapshotError,
};

#[cfg(feature = "snapshot")]
use snapshot::{decode_column_postcard, encode_column_postcard};

mod sealed {
    pub trait SealedElement {}
    pub trait SealedQueryTuple {}
    pub trait SealedBundle {}
}

/// One element of a typed query tuple: `&T`, `&mut T`, `Option<&T>`, or
/// `Option<&mut T>`. Optional elements do not constrain which entities the
/// query visits; they yield `None` on entities missing the component.
pub trait QueryElement: sealed::SealedElement {
    type Fetch<'table>;
    type Item<'item>;
    const REQUIRED: bool;
    fn component_mask(world: &mut DynWorld) -> u64;
    fn fetch<'table>(
        slot: Option<&'table mut ColumnSlot>,
        current_tick: u32,
    ) -> Self::Fetch<'table>;
    fn changed_newer(fetch: &Self::Fetch<'_>, index: usize, since_tick: u32) -> bool;
    fn item<'fetch>(fetch: &'fetch mut Self::Fetch<'_>, index: usize) -> Self::Item<'fetch>;
    fn stamp_peaks(fetch: &mut Self::Fetch<'_>);
}

impl<T: Send + Sync + Default + 'static> sealed::SealedElement for &T {}

impl<T: Send + Sync + Default + 'static> QueryElement for &T {
    type Fetch<'table> = (&'table [T], &'table [u32]);
    type Item<'item> = &'item T;
    const REQUIRED: bool = true;

    fn component_mask(world: &mut DynWorld) -> u64 {
        world.component_key::<T>().mask
    }

    fn fetch<'table>(
        slot: Option<&'table mut ColumnSlot>,
        _current_tick: u32,
    ) -> Self::Fetch<'table> {
        let slot = slot.expect("required query element column missing");
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

    fn stamp_peaks(_fetch: &mut Self::Fetch<'_>) {}
}

impl<T: Send + Sync + Default + 'static> sealed::SealedElement for &mut T {}

impl<T: Send + Sync + Default + 'static> QueryElement for &mut T {
    type Fetch<'table> = (&'table mut [T], &'table mut [u32], u32, &'table mut u32);
    type Item<'item> = &'item mut T;
    const REQUIRED: bool = true;

    fn component_mask(world: &mut DynWorld) -> u64 {
        world.component_key::<T>().mask
    }

    fn fetch<'table>(
        slot: Option<&'table mut ColumnSlot>,
        current_tick: u32,
    ) -> Self::Fetch<'table> {
        let slot = slot.expect("required query element column missing");
        (
            column_vec_mut::<T>(slot.data.as_mut()).as_mut_slice(),
            slot.changed.as_mut_slice(),
            current_tick,
            &mut slot.peak_changed,
        )
    }

    fn changed_newer(fetch: &Self::Fetch<'_>, index: usize, since_tick: u32) -> bool {
        tick_is_newer(fetch.1[index], since_tick)
    }

    fn item<'fetch>(fetch: &'fetch mut Self::Fetch<'_>, index: usize) -> Self::Item<'fetch> {
        fetch.1[index] = fetch.2;
        &mut fetch.0[index]
    }

    fn stamp_peaks(fetch: &mut Self::Fetch<'_>) {
        *fetch.3 = fetch.2;
    }
}

impl<T: Send + Sync + Default + 'static> sealed::SealedElement for Option<&T> {}

impl<T: Send + Sync + Default + 'static> QueryElement for Option<&T> {
    type Fetch<'table> = Option<(&'table [T], &'table [u32])>;
    type Item<'item> = Option<&'item T>;
    const REQUIRED: bool = false;

    fn component_mask(world: &mut DynWorld) -> u64 {
        world.component_key::<T>().mask
    }

    fn fetch<'table>(
        slot: Option<&'table mut ColumnSlot>,
        current_tick: u32,
    ) -> Self::Fetch<'table> {
        slot.map(|slot| <&T as QueryElement>::fetch(Some(slot), current_tick))
    }

    fn changed_newer(fetch: &Self::Fetch<'_>, index: usize, since_tick: u32) -> bool {
        fetch
            .as_ref()
            .is_some_and(|fetch| tick_is_newer(fetch.1[index], since_tick))
    }

    fn item<'fetch>(fetch: &'fetch mut Self::Fetch<'_>, index: usize) -> Self::Item<'fetch> {
        fetch.as_ref().map(|fetch| &fetch.0[index])
    }

    fn stamp_peaks(_fetch: &mut Self::Fetch<'_>) {}
}

impl<T: Send + Sync + Default + 'static> sealed::SealedElement for Option<&mut T> {}

impl<T: Send + Sync + Default + 'static> QueryElement for Option<&mut T> {
    type Fetch<'table> = Option<(&'table mut [T], &'table mut [u32], u32, &'table mut u32)>;
    type Item<'item> = Option<&'item mut T>;
    const REQUIRED: bool = false;

    fn component_mask(world: &mut DynWorld) -> u64 {
        world.component_key::<T>().mask
    }

    fn fetch<'table>(
        slot: Option<&'table mut ColumnSlot>,
        current_tick: u32,
    ) -> Self::Fetch<'table> {
        slot.map(|slot| <&mut T as QueryElement>::fetch(Some(slot), current_tick))
    }

    fn changed_newer(fetch: &Self::Fetch<'_>, index: usize, since_tick: u32) -> bool {
        fetch
            .as_ref()
            .is_some_and(|fetch| tick_is_newer(fetch.1[index], since_tick))
    }

    fn item<'fetch>(fetch: &'fetch mut Self::Fetch<'_>, index: usize) -> Self::Item<'fetch> {
        fetch.as_mut().map(|fetch| {
            fetch.1[index] = fetch.2;
            &mut fetch.0[index]
        })
    }

    fn stamp_peaks(fetch: &mut Self::Fetch<'_>) {
        if let Some(fetch) = fetch {
            *fetch.3 = fetch.2;
        }
    }
}

/// The read-only half of [`QueryElement`], `&T` or `Option<&T>` only. Shared
/// fetches are `Copy` and items borrow the world rather than the fetch, which
/// is what lets [`DynQueryRef::iter`] hand out a real `Iterator`.
pub trait ReadQueryElement: QueryElement {
    type ReadFetch<'table>: Copy;
    fn lookup_mask(world: &DynWorld) -> Option<u64>;
    fn read_fetch<'table>(slot: Option<&'table ColumnSlot>) -> Self::ReadFetch<'table>;
    fn read_changed_newer(fetch: Self::ReadFetch<'_>, index: usize, since_tick: u32) -> bool;
    fn read_item<'table>(fetch: Self::ReadFetch<'table>, index: usize) -> Self::Item<'table>;
}

impl<T: Send + Sync + Default + 'static> ReadQueryElement for &T {
    type ReadFetch<'table> = (&'table [T], &'table [u32]);

    fn lookup_mask(world: &DynWorld) -> Option<u64> {
        world.lookup_key::<T>().map(|key| key.mask)
    }

    fn read_fetch<'table>(slot: Option<&'table ColumnSlot>) -> Self::ReadFetch<'table> {
        let slot = slot.expect("required query element column missing");
        (
            column_vec::<T>(slot.data.as_ref()).as_slice(),
            slot.changed.as_slice(),
        )
    }

    fn read_changed_newer(fetch: Self::ReadFetch<'_>, index: usize, since_tick: u32) -> bool {
        tick_is_newer(fetch.1[index], since_tick)
    }

    fn read_item<'table>(fetch: Self::ReadFetch<'table>, index: usize) -> Self::Item<'table> {
        &fetch.0[index]
    }
}

impl<T: Send + Sync + Default + 'static> ReadQueryElement for Option<&T> {
    type ReadFetch<'table> = Option<(&'table [T], &'table [u32])>;

    fn lookup_mask(world: &DynWorld) -> Option<u64> {
        world.lookup_key::<T>().map(|key| key.mask)
    }

    fn read_fetch<'table>(slot: Option<&'table ColumnSlot>) -> Self::ReadFetch<'table> {
        slot.map(|slot| <&T as ReadQueryElement>::read_fetch(Some(slot)))
    }

    fn read_changed_newer(fetch: Self::ReadFetch<'_>, index: usize, since_tick: u32) -> bool {
        fetch.is_some_and(|fetch| tick_is_newer(fetch.1[index], since_tick))
    }

    fn read_item<'table>(fetch: Self::ReadFetch<'table>, index: usize) -> Self::Item<'table> {
        fetch.map(|fetch| &fetch.0[index])
    }
}

/// A tuple of query elements. Implemented for tuples of `&T`, `&mut T`,
/// `Option<&T>`, and `Option<&mut T>` up to eight elements; all component
/// types in one tuple must be distinct. Only the non-optional elements
/// constrain which entities the query visits.
pub trait QueryTuple: sealed::SealedQueryTuple {
    type Fetch<'table>;
    type Item<'item>;
    fn component_mask(world: &mut DynWorld) -> u64;
    fn element_masks(world: &mut DynWorld) -> [u64; 8];
    fn fetch<'table>(
        table_mask: u64,
        columns: &'table mut [ColumnSlot],
        element_masks: &[u64; 8],
        current_tick: u32,
    ) -> Self::Fetch<'table>;
    fn changed_newer(
        fetch: &Self::Fetch<'_>,
        index: usize,
        element_masks: &[u64; 8],
        changed_mask: u64,
        since_tick: u32,
    ) -> bool;
    fn item<'fetch>(fetch: &'fetch mut Self::Fetch<'_>, index: usize) -> Self::Item<'fetch>;
    fn stamp_peaks(fetch: &mut Self::Fetch<'_>);
}

/// The read-only half of [`QueryTuple`], tuples of `&T` and `Option<&T>`
/// only. Resolves masks without registering, fetches through shared borrows,
/// and hands out items that borrow the world, so results can outlive the
/// iteration.
pub trait ReadQueryTuple: QueryTuple {
    type ReadFetch<'table>: Copy;
    fn lookup_masks(world: &DynWorld) -> Option<([u64; 8], u64)>;
    fn read_fetch<'table>(
        table_mask: u64,
        columns: &'table [ColumnSlot],
        element_masks: &[u64; 8],
    ) -> Self::ReadFetch<'table>;
    fn read_changed_newer(
        fetch: Self::ReadFetch<'_>,
        index: usize,
        element_masks: &[u64; 8],
        changed_mask: u64,
        since_tick: u32,
    ) -> bool;
    fn read_item<'table>(fetch: Self::ReadFetch<'table>, index: usize) -> Self::Item<'table>;
}

fn required_mask(elements: &[(u64, bool)]) -> u64 {
    let mut seen = 0;
    let mut required = 0;
    for &(mask, is_required) in elements {
        assert_eq!(
            seen & mask,
            0,
            "query tuples must not repeat a component type"
        );
        seen |= mask;
        if is_required {
            required |= mask;
        }
    }
    required
}

fn lookup_masks_from(elements: &[(Option<u64>, bool)]) -> Option<([u64; 8], u64)> {
    let mut masks = [0u64; 8];
    let mut seen = 0;
    let mut required = 0;
    for (position, &(mask, is_required)) in elements.iter().enumerate() {
        match mask {
            Some(mask) => {
                assert_eq!(
                    seen & mask,
                    0,
                    "query tuples must not repeat a component type"
                );
                seen |= mask;
                if is_required {
                    required |= mask;
                }
                masks[position] = mask;
            }
            None => {
                if is_required {
                    return None;
                }
            }
        }
    }
    Some((masks, required))
}

fn distribute_slots<const COUNT: usize>(
    columns: &mut [ColumnSlot],
    positions: [Option<usize>; COUNT],
) -> [Option<&mut ColumnSlot>; COUNT] {
    let mut slots = [const { None }; COUNT];
    for (column_index, slot) in columns.iter_mut().enumerate() {
        if let Some(element_index) = positions
            .iter()
            .position(|position| *position == Some(column_index))
        {
            slots[element_index] = Some(slot);
        }
    }
    slots
}

macro_rules! impl_query_tuple {
    ($(($element:ident, $position:tt)),+) => {
        impl<$($element: QueryElement),+> sealed::SealedQueryTuple for ($($element,)+) {}

        impl<$($element: QueryElement),+> QueryTuple for ($($element,)+) {
            type Fetch<'table> = ($($element::Fetch<'table>,)+);
            type Item<'item> = ($($element::Item<'item>,)+);

            fn component_mask(world: &mut DynWorld) -> u64 {
                let elements = [$(($element::component_mask(world), $element::REQUIRED),)+];
                required_mask(&elements)
            }

            fn element_masks(world: &mut DynWorld) -> [u64; 8] {
                let mut masks = [0u64; 8];
                $(
                    masks[$position] = $element::component_mask(world);
                )+
                masks
            }

            #[allow(non_snake_case)]
            fn fetch<'table>(
                table_mask: u64,
                columns: &'table mut [ColumnSlot],
                element_masks: &[u64; 8],
                current_tick: u32,
            ) -> Self::Fetch<'table> {
                let positions = [$(
                    if table_mask & element_masks[$position] != 0 {
                        Some(column_position(table_mask, element_masks[$position]))
                    } else {
                        None
                    },
                )+];
                let [$($element,)+] = distribute_slots(columns, positions);
                ($($element::fetch($element, current_tick),)+)
            }

            fn changed_newer(
                fetch: &Self::Fetch<'_>,
                index: usize,
                element_masks: &[u64; 8],
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

            fn stamp_peaks(fetch: &mut Self::Fetch<'_>) {
                $($element::stamp_peaks(&mut fetch.$position);)+
            }
        }
    };
}

impl_query_tuple!((A, 0));
impl_query_tuple!((A, 0), (B, 1));
impl_query_tuple!((A, 0), (B, 1), (C, 2));
impl_query_tuple!((A, 0), (B, 1), (C, 2), (D, 3));
impl_query_tuple!((A, 0), (B, 1), (C, 2), (D, 3), (E, 4));
impl_query_tuple!((A, 0), (B, 1), (C, 2), (D, 3), (E, 4), (F, 5));
impl_query_tuple!((A, 0), (B, 1), (C, 2), (D, 3), (E, 4), (F, 5), (G, 6));
impl_query_tuple!(
    (A, 0),
    (B, 1),
    (C, 2),
    (D, 3),
    (E, 4),
    (F, 5),
    (G, 6),
    (H, 7)
);

macro_rules! impl_read_query_tuple {
    ($(($element:ident, $position:tt)),+) => {
        impl<$($element: ReadQueryElement),+> ReadQueryTuple for ($($element,)+) {
            type ReadFetch<'table> = ($($element::ReadFetch<'table>,)+);

            fn lookup_masks(world: &DynWorld) -> Option<([u64; 8], u64)> {
                let elements = [$(($element::lookup_mask(world), $element::REQUIRED),)+];
                lookup_masks_from(&elements)
            }

            fn read_fetch<'table>(
                table_mask: u64,
                columns: &'table [ColumnSlot],
                element_masks: &[u64; 8],
            ) -> Self::ReadFetch<'table> {
                ($(
                    $element::read_fetch(
                        if table_mask & element_masks[$position] != 0 {
                            Some(&columns[column_position(table_mask, element_masks[$position])])
                        } else {
                            None
                        },
                    ),
                )+)
            }

            fn read_changed_newer(
                fetch: Self::ReadFetch<'_>,
                index: usize,
                element_masks: &[u64; 8],
                changed_mask: u64,
                since_tick: u32,
            ) -> bool {
                let mut newer = false;
                $(
                    if changed_mask & element_masks[$position] != 0
                        && $element::read_changed_newer(fetch.$position, index, since_tick)
                    {
                        newer = true;
                    }
                )+
                newer
            }

            fn read_item<'table>(
                fetch: Self::ReadFetch<'table>,
                index: usize,
            ) -> Self::Item<'table> {
                ($($element::read_item(fetch.$position, index),)+)
            }
        }
    };
}

impl_read_query_tuple!((A, 0));
impl_read_query_tuple!((A, 0), (B, 1));
impl_read_query_tuple!((A, 0), (B, 1), (C, 2));
impl_read_query_tuple!((A, 0), (B, 1), (C, 2), (D, 3));
impl_read_query_tuple!((A, 0), (B, 1), (C, 2), (D, 3), (E, 4));
impl_read_query_tuple!((A, 0), (B, 1), (C, 2), (D, 3), (E, 4), (F, 5));
impl_read_query_tuple!((A, 0), (B, 1), (C, 2), (D, 3), (E, 4), (F, 5), (G, 6));
impl_read_query_tuple!(
    (A, 0),
    (B, 1),
    (C, 2),
    (D, 3),
    (E, 4),
    (F, 5),
    (G, 6),
    (H, 7)
);

/// A typed query in progress. Filters compose before `for_each` runs it.
pub struct DynQuery<'world, Q: QueryTuple> {
    world: &'world mut DynWorld,
    include: u64,
    exclude: u64,
    changed_mask: u64,
    include_tag_sets: [Option<&'world SparseTagSet>; 4],
    exclude_tag_sets: [Option<&'world SparseTagSet>; 4],
    marker: PhantomData<Q>,
}

fn push_tag_set<'world>(
    slots: &mut [Option<&'world SparseTagSet>; 4],
    tag_set: &'world SparseTagSet,
) {
    for slot in slots.iter_mut() {
        if slot.is_none() {
            *slot = Some(tag_set);
            return;
        }
    }
    panic!("a query supports at most four tag-set filters per side");
}

fn tag_sets_match(
    include: &[Option<&SparseTagSet>; 4],
    exclude: &[Option<&SparseTagSet>; 4],
    entity: Entity,
) -> bool {
    for tag_set in include.iter().flatten() {
        if !tag_set.contains(entity) {
            return false;
        }
    }
    for tag_set in exclude.iter().flatten() {
        if tag_set.contains(entity) {
            return false;
        }
    }
    true
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

    /// Filter by the marker type `T`'s tag, registering it on first use.
    pub fn with_tag_type<T: 'static>(mut self) -> Self {
        self.include |= self.world.tag_key::<T>().mask;
        self
    }

    pub fn without_tag_type<T: 'static>(mut self) -> Self {
        self.exclude |= self.world.tag_key::<T>().mask;
        self
    }

    /// Filter by membership in an external tag set, the grouped-worlds form:
    /// [`DynEcs`] tags live outside any single world's mask space, so they
    /// filter by set reference instead of by mask bit.
    pub fn with_tag_set(mut self, tag_set: &'world SparseTagSet) -> Self {
        push_tag_set(&mut self.include_tag_sets, tag_set);
        self
    }

    pub fn without_tag_set(mut self, tag_set: &'world SparseTagSet) -> Self {
        push_tag_set(&mut self.exclude_tag_sets, tag_set);
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

            let has_row_filters = tag_include != 0
                || tag_exclude != 0
                || changed_mask != 0
                || self.include_tag_sets.iter().any(Option::is_some)
                || self.exclude_tag_sets.iter().any(Option::is_some);

            if has_row_filters {
                let mut visited = false;
                for (index, &entity) in entity_indices.iter().enumerate() {
                    if (tag_include != 0 || tag_exclude != 0)
                        && !tags_match(tags, entity, tag_include, tag_exclude)
                    {
                        continue;
                    }
                    if !tag_sets_match(&self.include_tag_sets, &self.exclude_tag_sets, entity) {
                        continue;
                    }
                    if changed_mask != 0
                        && !Q::changed_newer(
                            &fetch,
                            index,
                            &element_masks,
                            changed_mask,
                            since_tick,
                        )
                    {
                        continue;
                    }
                    visited = true;
                    f(entity, Q::item(&mut fetch, index));
                }
                if visited {
                    Q::stamp_peaks(&mut fetch);
                }
            } else {
                for (index, &entity) in entity_indices.iter().enumerate() {
                    f(entity, Q::item(&mut fetch, index));
                }
                if !entity_indices.is_empty() {
                    Q::stamp_peaks(&mut fetch);
                }
            }
        }
    }
}

/// A read-only typed query in progress, from [`DynWorld::query_ref`].
/// Filters compose before [`iter`](Self::iter) runs it.
pub struct DynQueryRef<'world, Q: ReadQueryTuple> {
    world: &'world DynWorld,
    include: u64,
    exclude: u64,
    changed_mask: u64,
    include_tag_sets: [Option<&'world SparseTagSet>; 4],
    exclude_tag_sets: [Option<&'world SparseTagSet>; 4],
    dead: bool,
    marker: PhantomData<Q>,
}

impl<'world, Q: ReadQueryTuple> DynQueryRef<'world, Q> {
    pub fn with<T: Send + Sync + Default + 'static>(mut self) -> Self {
        match self.world.lookup_key::<T>() {
            Some(key) => self.include |= key.mask,
            None => self.dead = true,
        }
        self
    }

    pub fn without<T: Send + Sync + Default + 'static>(mut self) -> Self {
        if let Some(key) = self.world.lookup_key::<T>() {
            self.exclude |= key.mask;
        }
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

    /// Filter by the marker type `T`'s tag. Unregistered marker types match
    /// no entities.
    pub fn with_tag_type<T: 'static>(mut self) -> Self {
        match self.world.lookup_tag_key::<T>() {
            Some(key) => self.include |= key.mask,
            None => self.dead = true,
        }
        self
    }

    pub fn without_tag_type<T: 'static>(mut self) -> Self {
        if let Some(key) = self.world.lookup_tag_key::<T>() {
            self.exclude |= key.mask;
        }
        self
    }

    pub fn with_tag_set(mut self, tag_set: &'world SparseTagSet) -> Self {
        push_tag_set(&mut self.include_tag_sets, tag_set);
        self
    }

    pub fn without_tag_set(mut self, tag_set: &'world SparseTagSet) -> Self {
        push_tag_set(&mut self.exclude_tag_sets, tag_set);
        self
    }

    /// Only visit entities whose `T` changed since the last step. `T` must be
    /// one of the tuple's components.
    pub fn changed<T: Send + Sync + Default + 'static>(mut self) -> Self {
        match self.world.lookup_key::<T>() {
            Some(key) => self.changed_mask |= key.mask,
            None => self.dead = true,
        }
        self
    }

    /// Runs the query as an iterator of `(Entity, items)`. Items borrow the
    /// world, not the iterator, so they survive collection.
    pub fn iter(self) -> DynQueryRefIter<'world, Q> {
        let mut done = self.dead;
        let mut element_masks = [0u64; 8];
        let mut include = self.include;
        match Q::lookup_masks(self.world) {
            Some((masks, required)) => {
                element_masks = masks;
                include |= required;
            }
            None => done = true,
        }

        if !done {
            let tuple_mask = element_masks.iter().fold(0, |mask, element| mask | element);
            assert_eq!(
                self.changed_mask & !tuple_mask,
                0,
                "changed filters must name components present in the query tuple"
            );
        }

        let mut component_include = 0;
        let mut component_exclude = 0;
        let mut tag_include = 0;
        let mut tag_exclude = 0;
        match self.world.split_masks(include, self.exclude) {
            Some((components_in, components_out, tags_in, tags_out)) => {
                component_include = components_in;
                component_exclude = components_out;
                tag_include = tags_in;
                tag_exclude = tags_out;
            }
            None => done = true,
        }

        let cached_tables = self
            .world
            .query_cache
            .get(&component_include)
            .map(|indices| indices.as_slice());

        DynQueryRefIter {
            world: self.world,
            element_masks,
            include: component_include,
            exclude: component_exclude,
            tag_include,
            tag_exclude,
            include_tag_sets: self.include_tag_sets,
            exclude_tag_sets: self.exclude_tag_sets,
            changed_mask: self.changed_mask,
            since_tick: self.world.last_tick,
            cached_tables,
            table_index: 0,
            row_index: 0,
            current: None,
            done,
        }
    }
}

/// The iterator behind [`DynQueryRef::iter`]. Walks matching tables in
/// order, resolving columns once per table. When a `&mut` query path has
/// already cached this include mask's table list, the iterator reuses it
/// instead of scanning every table; the cache stays valid because table
/// registration appends to matching entries.
pub struct DynQueryRefIter<'world, Q: ReadQueryTuple> {
    world: &'world DynWorld,
    element_masks: [u64; 8],
    include: u64,
    exclude: u64,
    tag_include: u64,
    tag_exclude: u64,
    include_tag_sets: [Option<&'world SparseTagSet>; 4],
    exclude_tag_sets: [Option<&'world SparseTagSet>; 4],
    changed_mask: u64,
    since_tick: u32,
    cached_tables: Option<&'world [usize]>,
    table_index: usize,
    row_index: usize,
    current: Option<(&'world [Entity], Q::ReadFetch<'world>)>,
    done: bool,
}

impl<'world, Q: ReadQueryTuple> Iterator for DynQueryRefIter<'world, Q> {
    type Item = (Entity, Q::Item<'world>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        loop {
            if let Some((entities, fetch)) = self.current {
                while self.row_index < entities.len() {
                    let index = self.row_index;
                    self.row_index += 1;
                    let entity = entities[index];
                    if (self.tag_include != 0 || self.tag_exclude != 0)
                        && !tags_match(&self.world.tags, entity, self.tag_include, self.tag_exclude)
                    {
                        continue;
                    }
                    if !tag_sets_match(&self.include_tag_sets, &self.exclude_tag_sets, entity) {
                        continue;
                    }
                    if self.changed_mask != 0
                        && !Q::read_changed_newer(
                            fetch,
                            index,
                            &self.element_masks,
                            self.changed_mask,
                            self.since_tick,
                        )
                    {
                        continue;
                    }
                    return Some((entity, Q::read_item(fetch, index)));
                }
                self.current = None;
            }

            loop {
                let table = if let Some(indices) = self.cached_tables {
                    let Some(&cached_index) = indices.get(self.table_index) else {
                        self.done = true;
                        return None;
                    };
                    self.table_index += 1;
                    &self.world.tables[cached_index]
                } else {
                    let Some(table) = self.world.tables.get(self.table_index) else {
                        self.done = true;
                        return None;
                    };
                    self.table_index += 1;
                    if table.mask & self.include != self.include {
                        continue;
                    }
                    table
                };
                if table.mask & self.exclude == 0 && !table.entity_indices.is_empty() {
                    self.row_index = 0;
                    self.current = Some((
                        table.entity_indices.as_slice(),
                        Q::read_fetch(table.mask, &table.columns, &self.element_masks),
                    ));
                    break;
                }
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
                $(
                    let element_mask = world.component_key::<$element>().mask;
                    assert_eq!(
                        mask & element_mask,
                        0,
                        "bundles must not repeat a component type"
                    );
                    mask |= element_mask;
                )+
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
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct Position {
        pub x: f32,
        pub y: f32,
    }

    #[derive(Default, Clone, Debug, PartialEq)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct Velocity {
        pub x: f32,
        pub y: f32,
    }

    #[derive(Default, Clone, Debug, PartialEq)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct Health {
        pub value: f32,
    }

    #[derive(Default, Debug, Clone, PartialEq)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    #[should_panic(expected = "must not repeat a component type")]
    fn test_typed_query_rejects_optional_repeat_of_component() {
        let mut world = DynWorld::new();
        world.spawn((Position::default(),));
        world
            .query::<(&mut Position, Option<&Position>)>()
            .for_each(|_, _| {});
    }

    #[test]
    fn test_optional_element_yields_none_on_missing_component() {
        let mut world = DynWorld::new();
        let plain = world.spawn((Position { x: 1.0, y: 0.0 },));
        let moving = world.spawn((Position { x: 2.0, y: 0.0 }, Velocity { x: 5.0, y: 0.0 }));

        let mut visited = Vec::new();
        world
            .query::<(&Position, Option<&Velocity>)>()
            .for_each(|entity, (position, velocity)| {
                visited.push((entity, position.x, velocity.map(|velocity| velocity.x)));
            });

        visited.sort_by(|left, right| left.1.total_cmp(&right.1));
        assert_eq!(visited, vec![(plain, 1.0, None), (moving, 2.0, Some(5.0))]);
    }

    #[test]
    fn test_optional_mut_element_stamps_only_present_rows() {
        let mut world = DynWorld::new();
        let velocity_key = world.register::<Velocity>();
        let plain = world.spawn((Position::default(),));
        let moving = world.spawn((Position::default(), Velocity::default()));

        world.step();
        world
            .query::<(&Position, Option<&mut Velocity>)>()
            .for_each(|_entity, (_position, velocity)| {
                if let Some(velocity) = velocity {
                    velocity.x += 1.0;
                }
            });

        assert_eq!(world.get::<Velocity>(moving).unwrap().x, 1.0);
        let changed: Vec<Entity> = world.query_entities_changed(velocity_key.mask).collect();
        assert_eq!(changed, vec![moving]);
        let _ = plain;
    }

    #[test]
    fn test_optional_only_tuple_visits_every_entity() {
        let mut world = DynWorld::new();
        world.spawn((Position::default(),));
        world.spawn((Velocity::default(),));
        world.spawn((Health::default(),));

        let mut count = 0;
        world
            .query::<(Option<&Position>, Option<&Velocity>)>()
            .for_each(|_entity, _items| count += 1);
        assert_eq!(count, 3);
    }

    #[test]
    fn test_changed_filter_on_optional_element() {
        let mut world = DynWorld::new();
        let still = world.spawn((Position::default(), Velocity::default()));
        let moving = world.spawn((Position::default(), Velocity::default()));
        let bare = world.spawn((Position::default(),));

        world.step();
        world.get_mut::<Velocity>(moving).unwrap().x = 3.0;

        let mut visited = Vec::new();
        world
            .query::<(&Position, Option<&Velocity>)>()
            .changed::<Velocity>()
            .for_each(|entity, _| visited.push(entity));

        assert_eq!(visited, vec![moving]);
        let _ = (still, bare);
    }

    #[test]
    fn test_query_tuple_arity_eight() {
        #[derive(Default, Clone, Debug, PartialEq)]
        struct C1(f32);
        #[derive(Default, Clone, Debug, PartialEq)]
        struct C2(f32);
        #[derive(Default, Clone, Debug, PartialEq)]
        struct C3(f32);
        #[derive(Default, Clone, Debug, PartialEq)]
        struct C4(f32);
        #[derive(Default, Clone, Debug, PartialEq)]
        struct C5(f32);
        #[derive(Default, Clone, Debug, PartialEq)]
        struct C6(f32);
        #[derive(Default, Clone, Debug, PartialEq)]
        struct C7(f32);
        #[derive(Default, Clone, Debug, PartialEq)]
        struct C8(f32);

        let mut world = DynWorld::new();
        let entity = world.spawn((
            C1(1.0),
            C2(2.0),
            C3(3.0),
            C4(4.0),
            C5(5.0),
            C6(6.0),
            C7(7.0),
            C8(8.0),
        ));

        let mut total = 0.0;
        world
            .query::<(&C1, &C2, &C3, &mut C4, &C5, &C6, &C7, &mut C8)>()
            .for_each(|seen, (c1, c2, c3, c4, c5, c6, c7, c8)| {
                assert_eq!(seen, entity);
                c4.0 += 10.0;
                c8.0 += 10.0;
                total = c1.0 + c2.0 + c3.0 + c4.0 + c5.0 + c6.0 + c7.0 + c8.0;
            });

        assert_eq!(total, 56.0);
        assert_eq!(world.get::<C4>(entity).unwrap().0, 14.0);
        assert_eq!(world.get::<C8>(entity).unwrap().0, 18.0);
    }

    #[test]
    fn test_query_ref_iterates_and_collects_borrows() {
        let mut world = DynWorld::new();
        let plain = world.spawn((Position { x: 1.0, y: 0.0 },));
        let moving = world.spawn((Position { x: 2.0, y: 0.0 }, Velocity { x: 5.0, y: 0.0 }));

        let mut collected: Vec<(Entity, &Position, Option<&Velocity>)> = world
            .query_ref::<(&Position, Option<&Velocity>)>()
            .iter()
            .map(|(entity, (position, velocity))| (entity, position, velocity))
            .collect();
        collected.sort_by(|left, right| left.1.x.total_cmp(&right.1.x));

        assert_eq!(collected.len(), 2);
        assert_eq!(collected[0], (plain, &Position { x: 1.0, y: 0.0 }, None));
        assert_eq!(
            collected[1],
            (
                moving,
                &Position { x: 2.0, y: 0.0 },
                Some(&Velocity { x: 5.0, y: 0.0 })
            )
        );

        let total: f32 = world
            .query_ref::<(&Position,)>()
            .iter()
            .map(|(_entity, (position,))| position.x)
            .sum();
        assert_eq!(total, 3.0);
    }

    #[test]
    fn test_query_ref_filters_match_for_each() {
        let mut world = DynWorld::new();
        let boss = world.register_tag();
        let tagged = world.spawn((Position { x: 1.0, y: 0.0 }, Velocity::default()));
        let untagged = world.spawn((Position { x: 2.0, y: 0.0 }, Velocity::default()));
        let frozen = world.spawn((Position { x: 3.0, y: 0.0 },));
        world.add_tag(boss, tagged);

        let with_velocity: Vec<Entity> = world
            .query_ref::<(&Position,)>()
            .with::<Velocity>()
            .iter()
            .map(|(entity, _)| entity)
            .collect();
        assert_eq!(with_velocity, vec![tagged, untagged]);

        let without_velocity: Vec<Entity> = world
            .query_ref::<(&Position,)>()
            .without::<Velocity>()
            .iter()
            .map(|(entity, _)| entity)
            .collect();
        assert_eq!(without_velocity, vec![frozen]);

        let tagged_only: Vec<Entity> = world
            .query_ref::<(&Position,)>()
            .with_tag(boss)
            .iter()
            .map(|(entity, _)| entity)
            .collect();
        assert_eq!(tagged_only, vec![tagged]);

        world.step();
        world.get_mut::<Position>(untagged).unwrap().x = 9.0;
        let changed: Vec<Entity> = world
            .query_ref::<(&Position,)>()
            .changed::<Position>()
            .iter()
            .map(|(entity, _)| entity)
            .collect();
        assert_eq!(changed, vec![untagged]);
    }

    #[test]
    fn test_query_ref_unregistered_types_match_nothing_and_do_not_register() {
        #[derive(Default, Clone, Debug)]
        struct Unseen;

        let mut world = DynWorld::new();
        world.spawn((Position::default(),));
        let registered = world.registry.components.len();

        assert_eq!(world.query_ref::<(&Unseen,)>().iter().count(), 0);
        assert_eq!(
            world
                .query_ref::<(&Position, Option<&Unseen>)>()
                .iter()
                .map(|(_entity, (_position, unseen))| {
                    assert!(unseen.is_none());
                })
                .count(),
            1
        );
        assert_eq!(
            world
                .query_ref::<(&Position,)>()
                .with::<Unseen>()
                .iter()
                .count(),
            0
        );
        assert_eq!(world.registry.components.len(), registered);
    }

    #[test]
    fn test_marker_tags_register_lazily_and_share_index_space() {
        struct Boss;
        struct Frozen;

        let mut world = DynWorld::new();
        let keyed = world.register_tag();
        let boss = world.tag_key::<Boss>();
        let boss_again = world.tag_key::<Boss>();

        assert_eq!(boss.tag_index, keyed.tag_index + 1);
        assert_eq!(boss, boss_again);
        assert!(world.lookup_tag_key::<Frozen>().is_none());

        let entity = world.spawn((Position::default(),));
        world.add_tag_type::<Boss>(entity);
        assert!(world.has_tag_type::<Boss>(entity));
        assert!(world.has_tag(boss, entity));
        assert!(!world.has_tag_type::<Frozen>(entity));

        assert_eq!(
            world.query_tag_type::<Boss>().collect::<Vec<_>>(),
            vec![entity]
        );
        assert_eq!(world.query_tag_type::<Frozen>().count(), 0);

        assert!(world.remove_tag_type::<Boss>(entity));
        assert!(!world.has_tag_type::<Boss>(entity));
        assert!(!world.remove_tag_type::<Frozen>(entity));
    }

    #[test]
    fn test_marker_tag_query_filters() {
        struct Boss;

        let mut world = DynWorld::new();
        let tagged = world.spawn((Position { x: 1.0, y: 0.0 },));
        let untagged = world.spawn((Position { x: 2.0, y: 0.0 },));
        world.add_tag_type::<Boss>(tagged);

        let mut with_tag = Vec::new();
        world
            .query::<(&Position,)>()
            .with_tag_type::<Boss>()
            .for_each(|entity, _| with_tag.push(entity));
        assert_eq!(with_tag, vec![tagged]);

        let mut without_tag = Vec::new();
        world
            .query::<(&Position,)>()
            .without_tag_type::<Boss>()
            .for_each(|entity, _| without_tag.push(entity));
        assert_eq!(without_tag, vec![untagged]);

        let with_tag_ref: Vec<Entity> = world
            .query_ref::<(&Position,)>()
            .with_tag_type::<Boss>()
            .iter()
            .map(|(entity, _)| entity)
            .collect();
        assert_eq!(with_tag_ref, vec![tagged]);
    }

    #[test]
    fn test_marker_tag_filters_on_unregistered_types() {
        struct Never;

        let mut world = DynWorld::new();
        world.spawn((Position::default(),));

        assert_eq!(
            world
                .query_ref::<(&Position,)>()
                .with_tag_type::<Never>()
                .iter()
                .count(),
            0
        );
        assert_eq!(
            world
                .query_ref::<(&Position,)>()
                .without_tag_type::<Never>()
                .iter()
                .count(),
            1
        );
        assert!(world.lookup_tag_key::<Never>().is_none());
    }

    #[test]
    fn test_marker_tag_commands() {
        struct Boss;

        let mut world = DynWorld::new();
        let entity = world.spawn((Position::default(),));

        world.queue_add_tag_type::<Boss>(entity);
        world.apply_commands();
        assert!(world.has_tag_type::<Boss>(entity));

        world.queue_remove_tag_type::<Boss>(entity);
        world.apply_commands();
        assert!(!world.has_tag_type::<Boss>(entity));
    }

    #[test]
    fn test_mark_changed_stamps_raw_writes() {
        let mut world = DynWorld::new();
        let position = world.register::<Position>();
        let entities = world.spawn_entities(position.mask, 3);

        world.step();
        world.for_each_tables_mut(position.mask, 0, |table| {
            table.column_mut(position)[1].x = 5.0;
        });
        assert_eq!(world.query_entities_changed(position.mask).count(), 0);

        assert!(world.mark_changed(entities[1], position.mask));
        let changed: Vec<Entity> = world.query_entities_changed(position.mask).collect();
        assert_eq!(changed, vec![entities[1]]);
    }

    #[test]
    fn test_mark_changed_rejects_missing_rows() {
        let mut world = DynWorld::new();
        let position = world.register::<Position>();
        let velocity = world.register::<Velocity>();
        let entity = world.spawn_entities(position.mask, 1)[0];
        let dead = world.spawn_entities(position.mask, 1)[0];
        world.despawn_entities(&[dead]);

        assert!(!world.mark_changed(entity, velocity.mask));
        assert!(!world.mark_changed(dead, position.mask));

        let boss = world.register_tag();
        assert!(!world.mark_changed(entity, boss.mask));
        assert!(world.mark_changed(entity, position.mask | boss.mask));
    }

    #[test]
    fn test_mark_columns_changed_bulk_stamps_one_table() {
        let mut world = DynWorld::new();
        let position = world.register::<Position>();
        let velocity = world.register::<Velocity>();
        let plain = world.spawn_entities(position.mask, 2);
        let moving = world.spawn_entities(position.mask | velocity.mask, 2);

        world.step();
        let current_tick = world.current_tick();
        world.for_each_tables_mut(position.mask | velocity.mask, 0, |table| {
            for value in table.column_mut(position) {
                value.x += 1.0;
            }
            table.mark_columns_changed(position.mask, current_tick);
        });

        let changed: Vec<Entity> = world.query_entities_changed(position.mask).collect();
        assert_eq!(changed, moving);
        assert_eq!(world.query_entities_changed(velocity.mask).count(), 0);
        let _ = plain;
    }

    #[test]
    fn test_despawn_with_any_clears_matching_kinds() {
        let mut world = DynWorld::new();
        let plain = world.spawn((Position::default(),));
        let moving = world.spawn((Position::default(), Velocity::default()));
        let hurt = world.spawn((Health::default(),));

        let despawned = world.despawn_with_any::<(Velocity, Health)>();

        assert_eq!(despawned.len(), 2);
        assert!(despawned.contains(&moving));
        assert!(despawned.contains(&hurt));
        assert!(world.is_alive(plain));
        assert!(!world.is_alive(moving));
        assert!(!world.is_alive(hurt));

        #[derive(Default, Clone)]
        struct NeverSpawned;
        assert!(world.despawn_with_any::<(NeverSpawned,)>().is_empty());
        assert!(world.is_alive(plain));
    }

    #[test]
    fn test_mutable_query_stamps_peak_only_when_rows_are_visited() {
        let mut world = DynWorld::new();
        let position = world.register::<Position>();
        let boss = world.register_tag();
        world.spawn_entities(position.mask, 2);

        world.step();
        world
            .query::<(&mut Position,)>()
            .with_tag(boss)
            .for_each(|_entity, _items| {});

        for table in &world.tables {
            if table.mask == position.mask {
                assert!(
                    !tick_is_newer(table.columns[0].peak_changed, world.last_tick),
                    "a query that visits no rows must not stamp the table peak"
                );
            }
        }

        world
            .query::<(&mut Position,)>()
            .for_each(|_entity, _items| {});
        for table in &world.tables {
            if table.mask == position.mask {
                assert!(tick_is_newer(
                    table.columns[0].peak_changed,
                    world.last_tick
                ));
            }
        }
    }

    #[test]
    fn test_query_ref_reuses_cached_table_lists() {
        let mut world = DynWorld::new();
        let position = world.register::<Position>();
        world.spawn_entities(position.mask, 2);
        let velocity = world.register::<Velocity>();
        world.spawn_entities(position.mask | velocity.mask, 3);

        world.for_each_mut(position.mask, 0, |_entity, _table, _index| {});
        assert!(world.query_cache.contains_key(&position.mask));

        assert_eq!(world.query_ref::<(&Position,)>().iter().count(), 5);

        world.spawn_entities(position.mask, 1);
        assert_eq!(
            world.query_ref::<(&Position,)>().iter().count(),
            6,
            "cache entries must stay current as tables grow"
        );
    }

    #[test]
    #[should_panic(expected = "bundles must not repeat a component type")]
    fn test_bundle_rejects_repeated_component() {
        let mut world = DynWorld::new();
        world.spawn((Position::default(), Position { x: 1.0, y: 0.0 }));
    }

    #[test]
    fn test_resource_scope_preserves_resource_on_panic() {
        struct Score {
            value: u32,
        }

        let mut world = DynWorld::new();
        world.insert_resource(Score { value: 7 });

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            world.resource_scope(|_world, _score: &mut Score| panic!("boom"));
        }));

        assert!(result.is_err());
        assert_eq!(world.resource::<Score>().unwrap().value, 7);
    }

    #[test]
    fn test_resource_scope_takes_and_restores() {
        struct Score {
            value: u32,
        }

        let mut world = DynWorld::new();
        world.insert_resource(Score { value: 1 });

        let spawned = world.resource_scope(|world, score: &mut Score| {
            assert!(world.resource::<Score>().is_none());
            score.value += 1;
            world.spawn((Position { x: 7.0, y: 0.0 },))
        });

        assert_eq!(world.resource::<Score>().unwrap().value, 2);
        assert_eq!(world.get::<Position>(spawned).unwrap().x, 7.0);
    }

    #[test]
    #[should_panic(expected = "resource_scope requires the resource to be present")]
    fn test_resource_scope_panics_on_missing_resource() {
        struct Missing;

        let mut world = DynWorld::new();
        world.resource_scope(|_world, _missing: &mut Missing| {});
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

    #[test]
    fn test_spawn_batch_initializes_rows() {
        let mut world = DynWorld::new();
        let position = world.register::<Position>();
        let velocity = world.register::<Velocity>();

        let entities = world.spawn_batch(position.mask | velocity.mask, 100, |table, index| {
            table.column_mut(position)[index] = Position {
                x: index as f32,
                y: 0.0,
            };
        });

        assert_eq!(entities.len(), 100);
        for (offset, &entity) in entities.iter().enumerate() {
            assert_eq!(world.get_keyed(position, entity).unwrap().x, offset as f32);
            assert_eq!(world.get_keyed(velocity, entity).unwrap().x, 0.0);
        }
    }

    #[test]
    fn test_for_each_mut_changed_visits_only_stamped_slots() {
        let mut world = DynWorld::new();
        let position = world.register::<Position>();
        let entities = world.spawn_entities(position.mask, 3);

        world.step();
        world.get_mut_keyed(position, entities[1]).unwrap().x = 5.0;

        let mut visited = Vec::new();
        world.for_each_mut_changed(position.mask, 0, |entity, _table, _index| {
            visited.push(entity);
        });
        assert_eq!(visited, vec![entities[1]]);

        world.step();
        visited.clear();
        world.for_each_mut_changed(position.mask, 0, |entity, _table, _index| {
            visited.push(entity);
        });
        assert!(visited.is_empty(), "a step must expire the changed window");
    }

    #[test]
    fn test_for_each_mut_changed_since_cursor() {
        let mut world = DynWorld::new();
        let position = world.register::<Position>();
        let entity = world.spawn_entities(position.mask, 1)[0];

        world.step();
        let cursor = world.last_tick();
        world.set_keyed(position, entity, Position { x: 1.0, y: 0.0 });
        world.step();
        world.step();

        let mut visited = Vec::new();
        world.for_each_mut_changed_since(position.mask, 0, cursor, |seen, _table, _index| {
            visited.push(seen);
        });
        assert_eq!(visited, vec![entity]);

        let cursor = world.current_tick();
        visited.clear();
        world.for_each_mut_changed_since(position.mask, 0, cursor, |seen, _table, _index| {
            visited.push(seen);
        });
        assert!(visited.is_empty());
    }

    #[test]
    fn test_changed_skips_untouched_tables() {
        let mut world = DynWorld::new();
        let position = world.register::<Position>();
        let velocity = world.register::<Velocity>();

        world.spawn_entities(position.mask, 3);
        let moving = world.spawn_entities(position.mask | velocity.mask, 1)[0];

        world.step();
        world.get_mut_keyed(position, moving).unwrap().x = 1.0;

        let changed: Vec<Entity> = world.query_entities_changed(position.mask).collect();
        assert_eq!(changed, vec![moving]);

        for table in &world.tables {
            let column = &table.columns[0];
            if table.mask == position.mask {
                assert!(!tick_is_newer(column.peak_changed, world.last_tick));
            }
        }
    }

    #[test]
    fn test_structural_log_capacity_backstop() {
        let mut world = DynWorld::new();
        let position = world.register::<Position>();
        let entity = world.spawn_entities(position.mask, 1)[0];

        for _ in 0..STRUCTURAL_LOG_CAPACITY {
            world.record_structural(entity, StructuralChangeKind::ComponentsAdded, position.mask);
        }

        assert_eq!(world.structural_log.len(), 1);
        assert_eq!(
            world.structural_sequence(),
            STRUCTURAL_LOG_CAPACITY as u64 + 1
        );
        let tail = world.structural_changes_since(0);
        assert_eq!(tail.len(), 1);
        assert_eq!(tail[0].sequence, world.structural_sequence());
    }

    #[test]
    fn test_structural_log_trim_and_clear() {
        let mut world = DynWorld::new();
        let position = world.register::<Position>();
        let entity = world.spawn_entities(position.mask, 1)[0];
        world.add_components(entity, position.mask);
        world.remove_components(entity, position.mask);

        let cursor = world.structural_changes_since(0)[0].sequence;
        world.trim_structural_log(cursor);
        assert_eq!(world.structural_changes_since(0).len(), 1);

        world.clear_structural_log();
        assert!(world.structural_changes_since(0).is_empty());
        assert_eq!(world.structural_sequence(), 2);
    }

    #[test]
    fn test_commands_full_surface() {
        let mut world = DynWorld::new();
        let position = world.register::<Position>();
        let velocity = world.register::<Velocity>();
        let boss = world.register_tag();

        let entity = world.spawn_entities(position.mask, 1)[0];

        world.queue_spawn_entities(velocity.mask, 3);
        world.queue_add_components(entity, velocity.mask);
        world.queue_add_tag(boss, entity);
        world.queue(move |world| {
            world.set_keyed(position, entity, Position { x: 9.0, y: 0.0 });
        });
        assert_eq!(world.command_count(), 4);

        world.apply_commands();

        assert_eq!(world.entity_count(), 4);
        assert!(world.get_keyed(velocity, entity).is_some());
        assert!(world.has_tag(boss, entity));
        assert_eq!(world.get_keyed(position, entity).unwrap().x, 9.0);

        world.queue_remove_components(entity, velocity.mask);
        world.queue_remove_tag(boss, entity);
        world.apply_commands();
        assert!(world.get_keyed(velocity, entity).is_none());
        assert!(!world.has_tag(boss, entity));

        world.queue_despawn_entity(entity);
        world.clear_commands();
        assert_eq!(world.command_count(), 0);
        assert!(world.is_alive(entity), "cleared commands must not apply");
    }

    #[test]
    fn test_events_trim_and_clear() {
        let mut world = DynWorld::new();
        for value in 0..6u32 {
            world.send(PingEvent { value });
        }

        world.trim_events::<PingEvent>(2);
        assert_eq!(world.read_events::<PingEvent>().len(), 4);
        assert_eq!(world.read_events::<PingEvent>()[0].value, 2);
        assert_eq!(world.event_sequence::<PingEvent>(), 6);

        world.clear_events::<PingEvent>();
        assert!(world.read_events::<PingEvent>().is_empty());
        assert_eq!(world.event_sequence::<PingEvent>(), 6);
    }

    #[test]
    fn test_query_mask_filters_and_empty_tag_fast_path() {
        let mut world = DynWorld::new();
        let velocity_key = world.register::<Velocity>();
        let boss = world.register_tag();

        let plain = world.spawn((Position { x: 1.0, y: 0.0 },));
        let fast = world.spawn((Position { x: 2.0, y: 0.0 }, Velocity::default()));

        let mut with_mask = Vec::new();
        world
            .query::<(&Position,)>()
            .with_mask(velocity_key.mask)
            .for_each(|entity, _| with_mask.push(entity));
        assert_eq!(with_mask, vec![fast]);

        let mut without_mask = Vec::new();
        world
            .query::<(&Position,)>()
            .without_mask(velocity_key.mask)
            .for_each(|entity, _| without_mask.push(entity));
        assert_eq!(without_mask, vec![plain]);

        let mut excluding_empty_tag = Vec::new();
        world
            .query::<(&Position,)>()
            .without_tag(boss)
            .for_each(|entity, _| excluding_empty_tag.push(entity));
        assert_eq!(
            excluding_empty_tag.len(),
            2,
            "excluding a tag nobody has excludes nothing"
        );

        let mut including_empty_tag = Vec::new();
        world
            .query::<(&Position,)>()
            .with_tag(boss)
            .for_each(|entity, _| including_empty_tag.push(entity));
        assert!(
            including_empty_tag.is_empty(),
            "including a tag nobody has matches nothing"
        );
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn test_par_for_each_mut_with_tag_masks() {
        let mut world = DynWorld::new();
        let position = world.register::<Position>();
        let boss = world.register_tag();

        let entities = world.spawn_entities(position.mask, 10);
        for &entity in &entities[..5] {
            world.add_tag(boss, entity);
        }

        world.par_for_each_mut(position.mask | boss.mask, 0, |_entity, table, index| {
            table.column_mut(position)[index].x = 7.0;
        });

        for (offset, &entity) in entities.iter().enumerate() {
            let expected = if offset < 5 { 7.0 } else { 0.0 };
            assert_eq!(world.get_keyed(position, entity).unwrap().x, expected);
        }
    }

    #[test]
    fn test_dyn_ecs_group_shares_entities_across_worlds() {
        let mut ecs = DynEcs::new();
        let core = ecs.add_world(ComponentRegistry::new());
        let render = ecs.add_world(ComponentRegistry::new());

        let entity = ecs.spawn();
        ecs.worlds[core].set(entity, Position { x: 1.0, y: 2.0 });
        ecs.worlds[render].set(entity, Health { value: 9.0 });

        assert_eq!(ecs.worlds[core].get::<Position>(entity).unwrap().x, 1.0);
        assert_eq!(ecs.worlds[render].get::<Health>(entity).unwrap().value, 9.0);
        assert!(ecs.worlds[core].get::<Health>(entity).is_none());

        let core_position = ecs.worlds[core].register::<Position>();
        let render_health = ecs.worlds[render].register::<Health>();
        assert_eq!(
            core_position.mask, render_health.mask,
            "each grouped world owns an independent 64-bit mask space"
        );
    }

    #[test]
    fn test_dyn_ecs_despawn_broadcasts_and_refuses_stale() {
        let mut ecs = DynEcs::new();
        let core = ecs.add_world(ComponentRegistry::new());
        let render = ecs.add_world(ComponentRegistry::new());

        let old = ecs.spawn();
        ecs.worlds[core].set(old, Position { x: 1.0, y: 0.0 });

        assert!(ecs.despawn(old));
        assert!(!ecs.despawn(old), "double despawn must be refused");

        assert!(
            !ecs.worlds[core].add_components(old, 1),
            "stale add must be refused in a world that stored the entity"
        );
        ecs.worlds[render].set(old, Health { value: 3.0 });
        assert!(
            ecs.worlds[render].get::<Health>(old).is_none(),
            "stale set must be refused in a world that never stored the entity"
        );

        let reused = ecs.spawn();
        assert_eq!(reused.id, old.id);
        assert_eq!(reused.generation, old.generation + 1);
        ecs.worlds[render].set(reused, Health { value: 5.0 });
        assert_eq!(ecs.worlds[render].get::<Health>(reused).unwrap().value, 5.0);
        assert!(ecs.worlds[render].get::<Health>(old).is_none());
    }

    #[test]
    fn test_dyn_ecs_group_tags_filter_queries() {
        let mut ecs = DynEcs::new();
        let core = ecs.add_world(ComponentRegistry::new());
        let selected = ecs.register_tag();

        let first = ecs.spawn();
        let second = ecs.spawn();
        ecs.worlds[core].set(first, Position { x: 1.0, y: 0.0 });
        ecs.worlds[core].set(second, Position { x: 2.0, y: 0.0 });
        ecs.add_tag(selected, first);

        let DynEcs { worlds, tags, .. } = &mut ecs;
        let mut visited = Vec::new();
        worlds[core]
            .query::<(&Position,)>()
            .with_tag_set(&tags[selected])
            .for_each(|entity, _| visited.push(entity));
        assert_eq!(visited, vec![first]);

        let mut excluded = Vec::new();
        worlds[core]
            .query::<(&Position,)>()
            .without_tag_set(&tags[selected])
            .for_each(|entity, _| excluded.push(entity));
        assert_eq!(excluded, vec![second]);

        assert!(ecs.despawn(first));
        assert!(!ecs.has_tag(selected, first), "despawn drops group tags");
    }

    #[test]
    fn test_dyn_ecs_spawn_entities_in_member_world() {
        let mut ecs = DynEcs::new();
        let core = ecs.add_world(ComponentRegistry::new());
        let position = ecs.worlds[core].register::<Position>();

        let entities = ecs.spawn_entities(core, position.mask, 3);
        assert_eq!(entities.len(), 3);
        for &entity in &entities {
            assert!(ecs.is_alive(entity));
            assert!(ecs.worlds[core].get_keyed(position, entity).is_some());
        }

        let next = ecs.spawn();
        assert_eq!(next.id, 3, "member spawns draw from the shared allocator");
    }

    #[cfg(feature = "snapshot")]
    mod snapshots {
        use super::*;

        fn build_registry() -> ComponentRegistry {
            let mut registry = ComponentRegistry::new();
            registry.register_serde::<Position>();
            registry.register_serde::<Velocity>();
            registry.register_serde::<Health>();
            registry.register_tag();
            registry
        }

        fn populated_world() -> (DynWorld, Vec<Entity>) {
            let mut world = DynWorld::from_registry(build_registry());
            let position = world.component_key::<Position>();
            let health = world.component_key::<Health>();
            let boss = TagKey {
                tag_index: 0,
                mask: 1 << 63,
                registry_id: world.registry.registry_id,
            };

            let mut entities = Vec::new();
            for index in 0..10 {
                let entity = world.spawn((
                    Position {
                        x: index as f32,
                        y: index as f32 * 2.0,
                    },
                    Velocity { x: 1.0, y: 0.0 },
                ));
                entities.push(entity);
            }
            world.set_keyed(health, entities[3], Health { value: 42.0 });
            world.despawn_entities(&[entities[5]]);
            world.add_tag(boss, entities[7]);
            world.step();
            let _ = position;
            (world, entities)
        }

        #[test]
        fn test_snapshot_round_trip_preserves_state() {
            let (world, entities) = populated_world();

            let snapshot = world.snapshot().unwrap();
            let bytes = postcard::to_allocvec(&snapshot).unwrap();
            let decoded: DynWorldSnapshot = postcard::from_bytes(&bytes).unwrap();
            let restored = DynWorld::from_snapshot(build_registry(), &decoded).unwrap();

            assert_eq!(restored.entity_count(), world.entity_count());
            for &entity in &entities {
                assert_eq!(restored.is_alive(entity), world.is_alive(entity));
                assert_eq!(
                    restored.component_mask(entity),
                    world.component_mask(entity)
                );
                assert_eq!(
                    restored.get::<Position>(entity).map(|p| (p.x, p.y)),
                    world.get::<Position>(entity).map(|p| (p.x, p.y))
                );
                assert_eq!(
                    restored.get::<Health>(entity).map(|h| h.value),
                    world.get::<Health>(entity).map(|h| h.value)
                );
            }
            assert_eq!(restored.tags[0].len(), 1);
            assert!(restored.tags[0].contains(entities[7]));

            let respawned = {
                let mut restored = restored;
                restored.spawn((Position::default(),))
            };
            assert_eq!(
                respawned.id, entities[5].id,
                "the restored allocator must recycle the despawned id"
            );
            assert_eq!(respawned.generation, entities[5].generation + 1);
        }

        #[test]
        fn test_snapshot_restored_world_stays_in_lockstep() {
            let (mut original, _) = populated_world();
            let snapshot = original.snapshot().unwrap();
            let mut restored = DynWorld::from_snapshot(build_registry(), &snapshot).unwrap();

            let mut rng = Lcg(99);
            let mut handles: Vec<Entity> = original.get_all_entities();
            for _ in 0..500 {
                match rng.next() % 5 {
                    0 => {
                        let first = original.spawn((Position { x: 1.0, y: 1.0 },));
                        let second = restored.spawn((Position { x: 1.0, y: 1.0 },));
                        assert_eq!(first, second);
                        handles.push(first);
                    }
                    1 => {
                        if !handles.is_empty() {
                            let entity = handles[rng.next() as usize % handles.len()];
                            assert_eq!(
                                original.despawn_entities(&[entity]),
                                restored.despawn_entities(&[entity])
                            );
                        }
                    }
                    2 => {
                        if !handles.is_empty() {
                            let entity = handles[rng.next() as usize % handles.len()];
                            let value = (rng.next() % 100) as f32;
                            original.set(entity, Health { value });
                            restored.set(entity, Health { value });
                        }
                    }
                    3 => {
                        if !handles.is_empty() {
                            let entity = handles[rng.next() as usize % handles.len()];
                            let velocity_mask = original.component_key::<Velocity>().mask;
                            assert_eq!(
                                original.remove_components(entity, velocity_mask),
                                restored.remove_components(entity, velocity_mask)
                            );
                        }
                    }
                    _ => {
                        original.step();
                        restored.step();
                    }
                }
            }

            assert_eq!(original.entity_count(), restored.entity_count());
            for &entity in &handles {
                assert_eq!(
                    original.component_mask(entity),
                    restored.component_mask(entity)
                );
                assert_eq!(
                    original.get::<Health>(entity).map(|h| h.value),
                    restored.get::<Health>(entity).map(|h| h.value)
                );
            }
        }

        #[test]
        fn test_snapshot_schema_mismatch_is_refused() {
            let (world, _) = populated_world();
            let snapshot = world.snapshot().unwrap();

            let mut wrong_order = ComponentRegistry::new();
            wrong_order.register_serde::<Velocity>();
            wrong_order.register_serde::<Position>();
            wrong_order.register_serde::<Health>();

            match DynWorld::from_snapshot(wrong_order, &snapshot) {
                Err(SnapshotError::SchemaMismatch { .. }) => {}
                Err(other) => panic!("expected schema mismatch, got {other:?}"),
                Ok(_) => panic!("expected schema mismatch, got a restored world"),
            }
        }

        #[test]
        fn test_snapshot_registry_may_extend_the_schema() {
            let (world, entities) = populated_world();
            let snapshot = world.snapshot().unwrap();

            let mut extended = build_registry();
            extended.register_serde::<PingEvent>();

            let restored = DynWorld::from_snapshot(extended, &snapshot).unwrap();
            assert_eq!(
                restored.get::<Position>(entities[0]).unwrap().x,
                0.0,
                "components appended after the snapshot schema must not shift masks"
            );
        }

        #[test]
        fn test_snapshot_missing_codec_is_refused() {
            let mut world = DynWorld::new();
            world.spawn((Position::default(),));

            match world.snapshot() {
                Err(SnapshotError::MissingCodec(_)) => {}
                other => panic!(
                    "expected missing codec for a plain-registered component, got {:?}",
                    other.map(|_| ())
                ),
            }
        }

        #[test]
        fn test_snapshot_restores_change_detection_as_all_changed() {
            let (world, _) = populated_world();
            let snapshot = world.snapshot().unwrap();
            let restored = DynWorld::from_snapshot(build_registry(), &snapshot).unwrap();

            let position_mask = 1u64;
            let changed = restored.query_entities_changed(position_mask).count();
            assert_eq!(
                changed,
                restored.entity_count(),
                "restored slots must read as changed so consumers resync"
            );
        }

        #[test]
        fn test_dyn_ecs_snapshot_round_trip() {
            let mut ecs = DynEcs::new();
            let core = ecs.add_world({
                let mut registry = ComponentRegistry::new();
                registry.register_serde::<Position>();
                registry
            });
            let render = ecs.add_world({
                let mut registry = ComponentRegistry::new();
                registry.register_serde::<Health>();
                registry
            });
            let selected = ecs.register_tag();

            let entity = ecs.spawn();
            ecs.worlds[core].set(entity, Position { x: 3.0, y: 4.0 });
            ecs.worlds[render].set(entity, Health { value: 7.0 });
            ecs.add_tag(selected, entity);
            let dead = ecs.spawn();
            ecs.despawn(dead);

            let snapshot = ecs.snapshot().unwrap();
            let bytes = postcard::to_allocvec(&snapshot).unwrap();
            let decoded: DynEcsSnapshot = postcard::from_bytes(&bytes).unwrap();

            let restored = DynEcs::from_snapshot(
                vec![
                    {
                        let mut registry = ComponentRegistry::new();
                        registry.register_serde::<Position>();
                        registry
                    },
                    {
                        let mut registry = ComponentRegistry::new();
                        registry.register_serde::<Health>();
                        registry
                    },
                ],
                &decoded,
            )
            .unwrap();

            assert!(restored.is_alive(entity));
            assert!(!restored.is_alive(dead));
            assert_eq!(
                restored.worlds[core].get::<Position>(entity).unwrap().x,
                3.0
            );
            assert_eq!(
                restored.worlds[render].get::<Health>(entity).unwrap().value,
                7.0
            );
            assert!(restored.has_tag(selected, entity));

            let mut restored = restored;
            assert!(
                !restored.worlds[core].add_components(dead, 1),
                "stale refusal must survive the round trip"
            );
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

        mod group_differential {
            use super::*;

            crate::ecs! {
                StaticEcs {
                    StaticCore {
                        core_position: Position => GROUP_POSITION,
                        core_velocity: Velocity => GROUP_VELOCITY,
                    }
                    StaticRender {
                        render_health: Health => GROUP_HEALTH,
                    }
                }
                Tags {
                    marked => GROUP_MARKED,
                }
                GroupResources {
                    _unused: f32,
                }
            }

            /// Drives the macro multi-world and a DynEcs group with one seeded op
            /// stream and requires identical observable state, the grouped
            /// counterpart of the single-world differential below.
            #[test]
            fn test_differential_dyn_ecs_matches_static_multi_world() {
                for seed in [21u64, 2121, 212121] {
                    let mut rng = Lcg(seed);

                    let mut static_ecs = StaticEcs::default();
                    let mut dyn_ecs = DynEcs::new();
                    let core = dyn_ecs.add_world(ComponentRegistry::new());
                    let render = dyn_ecs.add_world(ComponentRegistry::new());
                    let position = dyn_ecs.worlds[core].register::<Position>();
                    let velocity = dyn_ecs.worlds[core].register::<Velocity>();
                    let health = dyn_ecs.worlds[render].register::<Health>();
                    let marked = dyn_ecs.register_tag();

                    assert_eq!(position.mask, GROUP_POSITION);
                    assert_eq!(velocity.mask, GROUP_VELOCITY);
                    assert_eq!(health.mask, GROUP_HEALTH);

                    let mut handles: Vec<Entity> = Vec::new();
                    let pick = |rng: &mut Lcg, handles: &[Entity]| {
                        if handles.is_empty() {
                            None
                        } else {
                            Some(handles[rng.next() as usize % handles.len()])
                        }
                    };

                    static_ecs.step();
                    dyn_ecs.step();

                    for _ in 0..2500 {
                        match rng.next() % 9 {
                            0 | 1 => {
                                let static_entity = static_ecs.spawn();
                                let dyn_entity = dyn_ecs.spawn();
                                assert_eq!(static_entity, dyn_entity);
                                handles.push(static_entity);
                            }
                            2 => {
                                if let Some(entity) = pick(&mut rng, &handles) {
                                    assert_eq!(static_ecs.despawn(entity), dyn_ecs.despawn(entity));
                                }
                            }
                            3 => {
                                if let Some(entity) = pick(&mut rng, &handles) {
                                    let value = (rng.next() % 1000) as f32;
                                    static_ecs
                                        .static_core
                                        .set_core_position(entity, Position { x: value, y: 0.0 });
                                    dyn_ecs.worlds[core].set_keyed(
                                        position,
                                        entity,
                                        Position { x: value, y: 0.0 },
                                    );
                                }
                            }
                            4 => {
                                if let Some(entity) = pick(&mut rng, &handles) {
                                    let value = (rng.next() % 1000) as f32;
                                    static_ecs
                                        .static_render
                                        .set_render_health(entity, Health { value });
                                    dyn_ecs.worlds[render].set_keyed(
                                        health,
                                        entity,
                                        Health { value },
                                    );
                                }
                            }
                            5 => {
                                if let Some(entity) = pick(&mut rng, &handles) {
                                    assert_eq!(
                                        static_ecs
                                            .static_core
                                            .remove_components(entity, GROUP_POSITION),
                                        dyn_ecs.worlds[core]
                                            .remove_components(entity, position.mask)
                                    );
                                }
                            }
                            6 => {
                                if let Some(entity) = pick(&mut rng, &handles) {
                                    static_ecs.add_marked(entity);
                                    dyn_ecs.add_tag(marked, entity);
                                    assert_eq!(
                                        static_ecs.has_marked(entity),
                                        dyn_ecs.has_tag(marked, entity)
                                    );
                                }
                            }
                            7 => {
                                if let Some(entity) = pick(&mut rng, &handles) {
                                    assert_eq!(
                                        static_ecs.remove_marked(entity),
                                        dyn_ecs.remove_tag(marked, entity)
                                    );
                                }
                            }
                            _ => {
                                let static_changed: std::collections::HashSet<Entity> = static_ecs
                                    .static_core
                                    .query_entities_changed(GROUP_POSITION)
                                    .collect();
                                let dyn_changed: std::collections::HashSet<Entity> = dyn_ecs.worlds
                                    [core]
                                    .query_entities_changed(position.mask)
                                    .collect();
                                assert_eq!(
                                    static_changed, dyn_changed,
                                    "core changed sets diverged with seed {seed}"
                                );

                                static_ecs.step();
                                dyn_ecs.step();
                            }
                        }
                    }

                    assert_eq!(
                        static_ecs.static_core.entity_count(),
                        dyn_ecs.worlds[core].entity_count()
                    );
                    assert_eq!(
                        static_ecs.static_render.entity_count(),
                        dyn_ecs.worlds[render].entity_count()
                    );
                    for &handle in &handles {
                        assert_eq!(static_ecs.is_alive(handle), dyn_ecs.is_alive(handle));
                        assert_eq!(
                            static_ecs.static_core.component_mask(handle),
                            dyn_ecs.worlds[core].component_mask(handle)
                        );
                        assert_eq!(
                            static_ecs.static_render.component_mask(handle),
                            dyn_ecs.worlds[render].component_mask(handle)
                        );
                        assert_eq!(
                            static_ecs.static_core.get_core_position(handle),
                            dyn_ecs.worlds[core].get_keyed(position, handle)
                        );
                        assert_eq!(
                            static_ecs
                                .static_render
                                .get_render_health(handle)
                                .map(|h| h.value),
                            dyn_ecs.worlds[render]
                                .get_keyed(health, handle)
                                .map(|h| h.value)
                        );
                        assert_eq!(
                            static_ecs.has_marked(handle),
                            dyn_ecs.has_tag(marked, handle)
                        );
                    }
                    let expected_marked = static_ecs.query_marked().count();
                    assert_eq!(dyn_ecs.query_tag(marked).count(), expected_marked);
                }
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
