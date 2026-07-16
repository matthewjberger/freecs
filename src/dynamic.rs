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
//! By default nothing here uses `unsafe`: columns are erased as whole `Vec<T>`
//! values behind `Box<dyn Any + Send + Sync>`, so `Drop` and thread-safety come
//! from the vec itself. The opt-in `raw_storage` feature swaps that one storage
//! type for a contiguous byte buffer reached through pointer casts, dropping the
//! per-access downcast for extra speed. It is off by default, changes no public
//! API and no observable behavior (the whole test suite runs identically under
//! both, and the raw path is `miri`-verified), and confines every `unsafe` to a
//! single [`RawColumn`](self) type. Leave it off for the safety guarantee; turn
//! it on when you want the storage-bound paths (spawn, migration, fragmented
//! iteration) as fast as they go.
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
//! the match, and single-component queries can skip the tuple entirely
//! (`world.query::<&mut Position>()`). Both query forms filter by
//! `changed::<T>()` (mutated since the last step) and `added::<T>()` (gained
//! since the last step, surviving table migrations). On a shared borrow,
//! [`DynWorld::query_ref`] runs read-only tuples as a real [`Iterator`],
//! with `single()` and `iter_combinations()` on top:
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
//! Events buffer for two frames, so per-frame handlers consume through a
//! cursor: `world.consume_events::<T>(&mut cursor)` yields each event exactly
//! once per consumer, while `read_events` re-reads the whole buffer.
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

/// Type-erased boxed storage for events and resources, which stay `Box<dyn
/// Any>` regardless of the column storage backend.
type BoxedAny = Box<dyn Any + Send + Sync>;

/// A fast, non-cryptographic hasher for `TypeId` keys, used by `raw_storage`
/// to resolve a component type to its column index. Every typed `set`, `get`,
/// and `remove` does one such lookup, so the default `SipHash` over a 16-byte
/// `TypeId` is a real per-operation cost; this mixes the id's words directly.
/// Only the value distribution matters here, never resistance to collisions.
#[cfg(feature = "raw_storage")]
#[derive(Default)]
pub struct TypeIdHasher(u64);

#[cfg(feature = "raw_storage")]
impl std::hash::Hasher for TypeIdHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        let mut hash = self.0;
        for &byte in bytes {
            hash = (hash ^ byte as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
        self.0 = hash;
    }

    fn write_u64(&mut self, value: u64) {
        self.0 = (self.0 ^ value).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }

    fn write_u128(&mut self, value: u128) {
        self.write_u64(value as u64);
        self.write_u64((value >> 64) as u64);
    }
}

/// `TypeId`-keyed map used for the registry's type-to-index lookups. Uses the
/// fast [`TypeIdHasher`] under `raw_storage` and the standard hasher otherwise,
/// so the default build's public types are unchanged.
#[cfg(feature = "raw_storage")]
type TypeIdMap<V> = HashMap<TypeId, V, std::hash::BuildHasherDefault<TypeIdHasher>>;
#[cfg(not(feature = "raw_storage"))]
type TypeIdMap<V> = HashMap<TypeId, V>;

/// A type-erased component column. With the default (safe) storage it is a
/// boxed `Vec<T>` reached through `Any` downcasts. With the opt-in
/// `raw_storage` feature it is a hand-rolled contiguous buffer reached through
/// pointer casts that are checked once at registration, dropping the per-access
/// downcast, and its freed allocations are recycled through a thread-local
/// pool instead of returned to the system allocator.
///
/// The public API is byte-for-byte identical across both backends. Observable
/// behavior is identical too, with two deliberate exceptions: `raw_storage`
/// disables per-row change detection (`changed::<T>()`, `added::<T>()`, and the
/// `for_each_mut_changed` family match nothing) and the structural-change log
/// (`structural_changes_since` and `structural_sequence` return empty/zero).
/// Both drop per-entity bookkeeping the safe backend pays on every mutation and
/// migration. [`HierarchyIndex`] stays correct by rebuilding from a scan.
pub struct ErasedColumn {
    #[cfg(not(feature = "raw_storage"))]
    storage: Box<dyn Any + Send + Sync>,
    #[cfg(feature = "raw_storage")]
    storage: raw_storage::RawColumn,
}

impl ErasedColumn {
    fn new<T: Send + Sync + Default + 'static>() -> Self {
        ErasedColumn {
            #[cfg(not(feature = "raw_storage"))]
            storage: Box::new(Vec::<T>::new()),
            #[cfg(feature = "raw_storage")]
            storage: raw_storage::RawColumn::new::<T>(),
        }
    }

    #[inline]
    fn slice<T: 'static>(&self) -> &[T] {
        #[cfg(not(feature = "raw_storage"))]
        {
            self.storage
                .downcast_ref::<Vec<T>>()
                .expect("column type does not match its registered component")
                .as_slice()
        }
        #[cfg(feature = "raw_storage")]
        {
            self.storage.slice::<T>()
        }
    }

    #[inline]
    fn slice_mut<T: 'static>(&mut self) -> &mut [T] {
        #[cfg(not(feature = "raw_storage"))]
        {
            self.storage
                .downcast_mut::<Vec<T>>()
                .expect("column type does not match its registered component")
                .as_mut_slice()
        }
        #[cfg(feature = "raw_storage")]
        {
            self.storage.slice_mut::<T>()
        }
    }

    fn push<T: Send + Sync + Default + 'static>(&mut self, value: T) {
        #[cfg(not(feature = "raw_storage"))]
        {
            self.storage
                .downcast_mut::<Vec<T>>()
                .expect("column type does not match its registered component")
                .push(value);
        }
        #[cfg(feature = "raw_storage")]
        {
            self.storage.push::<T>(value);
        }
    }

    fn extend_clone<T: Send + Sync + Default + Clone + 'static>(
        &mut self,
        count: usize,
        value: &T,
    ) {
        #[cfg(not(feature = "raw_storage"))]
        {
            let column = self
                .storage
                .downcast_mut::<Vec<T>>()
                .expect("column type does not match its registered component");
            column.reserve(count);
            for _ in 0..count {
                column.push(value.clone());
            }
        }
        #[cfg(feature = "raw_storage")]
        {
            self.storage.reserve(count);
            for _ in 0..count {
                self.storage.push::<T>(value.clone());
            }
        }
    }

    fn push_defaults<T: Send + Sync + Default + 'static>(&mut self, count: usize) {
        #[cfg(not(feature = "raw_storage"))]
        {
            let column = self
                .storage
                .downcast_mut::<Vec<T>>()
                .expect("column type does not match its registered component");
            column.reserve(count);
            for _ in 0..count {
                column.push(T::default());
            }
        }
        #[cfg(feature = "raw_storage")]
        {
            self.storage.reserve(count);
            for _ in 0..count {
                self.storage.push::<T>(T::default());
            }
        }
    }

    fn swap_remove<T: Send + Sync + Default + 'static>(&mut self, index: usize) {
        #[cfg(not(feature = "raw_storage"))]
        {
            self.storage
                .downcast_mut::<Vec<T>>()
                .expect("column type does not match its registered component")
                .swap_remove(index);
        }
        #[cfg(feature = "raw_storage")]
        {
            let _ = std::marker::PhantomData::<T>;
            self.storage.swap_remove(index);
        }
    }

    fn move_row<T: Send + Sync + Default + 'static>(
        &mut self,
        index: usize,
        destination: &mut ErasedColumn,
    ) {
        let value = std::mem::take(&mut self.slice_mut::<T>()[index]);
        destination.push::<T>(value);
    }

    fn swap_remove_into<T: Send + Sync + Default + 'static>(
        &mut self,
        index: usize,
        destination: &mut ErasedColumn,
    ) {
        #[cfg(not(feature = "raw_storage"))]
        {
            let value = self
                .storage
                .downcast_mut::<Vec<T>>()
                .expect("column type does not match its registered component")
                .swap_remove(index);
            destination.push::<T>(value);
        }
        #[cfg(feature = "raw_storage")]
        {
            let _ = std::marker::PhantomData::<T>;
            self.storage
                .swap_remove_into(index, &mut destination.storage);
        }
    }

    /// Swap-removes one row by raw byte move, no type parameter and no vtable
    /// hop. Sound because `RawColumn` carries the element size, alignment, and
    /// drop glue captured at registration.
    #[cfg(feature = "raw_storage")]
    fn swap_remove_raw(&mut self, index: usize) {
        self.storage.swap_remove(index);
    }

    /// Moves one row's bytes into `destination` and swap-removes the source
    /// slot, no type parameter and no vtable hop.
    #[cfg(feature = "raw_storage")]
    fn swap_remove_into_raw(&mut self, index: usize, destination: &mut ErasedColumn) {
        self.storage
            .swap_remove_into(index, &mut destination.storage);
    }

    fn len<T: 'static>(&self) -> usize {
        #[cfg(not(feature = "raw_storage"))]
        {
            self.storage
                .downcast_ref::<Vec<T>>()
                .expect("column type does not match its registered component")
                .len()
        }
        #[cfg(feature = "raw_storage")]
        {
            let _ = std::marker::PhantomData::<T>;
            self.storage.len()
        }
    }
}

/// The contiguous storage backend used when the `raw_storage` feature is on.
/// Every `unsafe` in that backend is contained in this module. The invariant
/// upheld by [`ErasedColumn`] is narrow: each method is called with the exact
/// `T` the column was registered for, so size, alignment, and drop glue always
/// match the bytes it holds.
#[cfg(feature = "raw_storage")]
mod raw_storage {
    use std::alloc::{self, Layout};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::ptr::{self, NonNull};

    /// A thread-local free list of raw column allocations, bucketed by
    /// element size and alignment. Dropping a column returns its buffer here
    /// instead of to the system allocator, so the next column of the same
    /// shape reuses it. This mirrors a chunk pool: repeated spawn/despawn
    /// cycles stop paying for `alloc`/`dealloc` round trips. Buffers are
    /// deallocated when the pool itself drops at thread exit, so nothing
    /// leaks.
    type PooledBuffer = (NonNull<u8>, usize);

    struct BufferPool {
        buckets: HashMap<(usize, usize), Vec<PooledBuffer>>,
    }

    const MAX_POOLED_PER_BUCKET: usize = 64;

    impl Drop for BufferPool {
        fn drop(&mut self) {
            for ((item_size, item_align), bucket) in self.buckets.drain() {
                for (pointer, capacity) in bucket {
                    if item_size != 0 && capacity != 0 {
                        let layout = Layout::from_size_align(item_size * capacity, item_align)
                            .expect("pooled column layout overflow");
                        unsafe { alloc::dealloc(pointer.as_ptr(), layout) };
                    }
                }
            }
        }
    }

    thread_local! {
        static POOL: RefCell<BufferPool> = RefCell::new(BufferPool {
            buckets: HashMap::new(),
        });
    }

    /// Pulls a buffer for `(item_size, item_align)` that holds at least
    /// `required` elements when one exists, otherwise the largest buffer in
    /// the bucket (the caller grows it), otherwise `None`.
    fn pool_take(
        item_size: usize,
        item_align: usize,
        required: usize,
    ) -> Option<(NonNull<u8>, usize)> {
        POOL.with(|pool| {
            let mut pool = pool.borrow_mut();
            let bucket = pool.buckets.get_mut(&(item_size, item_align))?;
            if bucket.is_empty() {
                return None;
            }
            let chosen = bucket
                .iter()
                .position(|&(_, capacity)| capacity >= required)
                .unwrap_or(bucket.len() - 1);
            Some(bucket.swap_remove(chosen))
        })
    }

    /// Returns a buffer to the pool, or deallocates it when the bucket is
    /// already at capacity.
    fn pool_return(pointer: NonNull<u8>, item_size: usize, item_align: usize, capacity: usize) {
        POOL.with(|pool| {
            let mut pool = pool.borrow_mut();
            let bucket = pool.buckets.entry((item_size, item_align)).or_default();
            if bucket.len() < MAX_POOLED_PER_BUCKET {
                bucket.push((pointer, capacity));
            } else {
                let layout = Layout::from_size_align(item_size * capacity, item_align)
                    .expect("pooled column layout overflow");
                unsafe { alloc::dealloc(pointer.as_ptr(), layout) };
            }
        });
    }

    pub(crate) struct RawColumn {
        pointer: NonNull<u8>,
        len: usize,
        capacity: usize,
        item_size: usize,
        item_align: usize,
        drop_fn: Option<unsafe fn(*mut u8)>,
    }

    // A column only ever holds a registered component, and every registered
    // component is `Send + Sync + 'static`, so the erased bytes are too.
    unsafe impl Send for RawColumn {}
    unsafe impl Sync for RawColumn {}

    unsafe fn drop_in_place_as<T>(pointer: *mut u8) {
        unsafe { ptr::drop_in_place(pointer.cast::<T>()) }
    }

    impl RawColumn {
        pub(crate) fn new<T: 'static>() -> Self {
            let layout = Layout::new::<T>();
            RawColumn {
                pointer: NonNull::new(layout.align() as *mut u8).expect("alignment is never zero"),
                len: 0,
                capacity: 0,
                item_size: layout.size(),
                item_align: layout.align(),
                drop_fn: std::mem::needs_drop::<T>()
                    .then_some(drop_in_place_as::<T> as unsafe fn(*mut u8)),
            }
        }

        #[inline]
        pub(crate) fn len(&self) -> usize {
            self.len
        }

        #[inline]
        fn element_pointer(&self, index: usize) -> *mut u8 {
            unsafe { self.pointer.as_ptr().add(index * self.item_size) }
        }

        pub(crate) fn slice<T: 'static>(&self) -> &[T] {
            unsafe { std::slice::from_raw_parts(self.pointer.as_ptr().cast::<T>(), self.len) }
        }

        pub(crate) fn slice_mut<T: 'static>(&mut self) -> &mut [T] {
            unsafe { std::slice::from_raw_parts_mut(self.pointer.as_ptr().cast::<T>(), self.len) }
        }

        fn layout_for(&self, capacity: usize) -> Layout {
            Layout::from_size_align(self.item_size * capacity, self.item_align)
                .expect("component column layout overflow")
        }

        pub(crate) fn reserve(&mut self, additional: usize) {
            if self.item_size == 0 {
                return;
            }
            let required = self.len + additional;
            if required <= self.capacity {
                return;
            }
            let new_capacity = required.max(self.capacity * 2).max(4);
            let new_layout = self.layout_for(new_capacity);
            if self.capacity == 0 {
                if let Some((pooled_pointer, pooled_capacity)) =
                    pool_take(self.item_size, self.item_align, new_capacity)
                {
                    if pooled_capacity >= new_capacity {
                        self.pointer = pooled_pointer;
                        self.capacity = pooled_capacity;
                        return;
                    }
                    let old_layout = self.layout_for(pooled_capacity);
                    let grown = unsafe {
                        alloc::realloc(pooled_pointer.as_ptr(), old_layout, new_layout.size())
                    };
                    self.pointer = NonNull::new(grown)
                        .unwrap_or_else(|| alloc::handle_alloc_error(new_layout));
                    self.capacity = new_capacity;
                    return;
                }
                let fresh = unsafe { alloc::alloc(new_layout) };
                self.pointer =
                    NonNull::new(fresh).unwrap_or_else(|| alloc::handle_alloc_error(new_layout));
                self.capacity = new_capacity;
            } else {
                let old_layout = self.layout_for(self.capacity);
                let grown =
                    unsafe { alloc::realloc(self.pointer.as_ptr(), old_layout, new_layout.size()) };
                self.pointer =
                    NonNull::new(grown).unwrap_or_else(|| alloc::handle_alloc_error(new_layout));
                self.capacity = new_capacity;
            }
        }

        pub(crate) fn push<T: 'static>(&mut self, value: T) {
            if self.item_size == 0 {
                std::mem::forget(value);
                self.len += 1;
                return;
            }
            if self.len == self.capacity {
                self.reserve(1);
            }
            unsafe { ptr::write(self.element_pointer(self.len).cast::<T>(), value) }
            self.len += 1;
        }

        pub(crate) fn swap_remove(&mut self, index: usize) {
            assert!(index < self.len, "column swap_remove index out of bounds");
            if let Some(drop_fn) = self.drop_fn {
                unsafe { drop_fn(self.element_pointer(index)) }
            }
            self.len -= 1;
            if index != self.len && self.item_size != 0 {
                unsafe {
                    ptr::copy_nonoverlapping(
                        self.element_pointer(self.len),
                        self.element_pointer(index),
                        self.item_size,
                    )
                }
            }
        }

        pub(crate) fn swap_remove_into(&mut self, index: usize, destination: &mut RawColumn) {
            assert!(
                index < self.len,
                "column swap_remove_into index out of bounds"
            );
            destination.reserve(1);
            if self.item_size != 0 {
                unsafe {
                    ptr::copy_nonoverlapping(
                        self.element_pointer(index),
                        destination.element_pointer(destination.len),
                        self.item_size,
                    )
                }
            }
            destination.len += 1;
            self.len -= 1;
            if index != self.len && self.item_size != 0 {
                unsafe {
                    ptr::copy_nonoverlapping(
                        self.element_pointer(self.len),
                        self.element_pointer(index),
                        self.item_size,
                    )
                }
            }
        }
    }

    impl Drop for RawColumn {
        fn drop(&mut self) {
            if let Some(drop_fn) = self.drop_fn {
                for index in 0..self.len {
                    unsafe { drop_fn(self.element_pointer(index)) }
                }
            }
            if self.item_size != 0 && self.capacity != 0 {
                pool_return(self.pointer, self.item_size, self.item_align, self.capacity);
            }
        }
    }
}

fn column_new<T: Send + Sync + Default + 'static>() -> ErasedColumn {
    ErasedColumn::new::<T>()
}

fn column_vec<T: 'static>(column: &ErasedColumn) -> &[T] {
    column.slice::<T>()
}

fn column_vec_mut<T: 'static>(column: &mut ErasedColumn) -> &mut [T] {
    column.slice_mut::<T>()
}

fn column_push_default<T: Send + Sync + Default + 'static>(
    column: &mut ErasedColumn,
    count: usize,
) {
    column.push_defaults::<T>(count);
}

fn column_len_of<T: Send + Sync + Default + 'static>(column: &ErasedColumn) -> usize {
    column.len::<T>()
}

fn column_swap_remove<T: Send + Sync + Default + 'static>(column: &mut ErasedColumn, index: usize) {
    column.swap_remove::<T>(index);
}

fn column_move_row<T: Send + Sync + Default + 'static>(
    source: &mut ErasedColumn,
    index: usize,
    destination: &mut ErasedColumn,
) {
    source.move_row::<T>(index, destination);
}

fn column_swap_remove_into<T: Send + Sync + Default + 'static>(
    source: &mut ErasedColumn,
    index: usize,
    destination: &mut ErasedColumn,
) {
    source.swap_remove_into::<T>(index, destination);
}

/// The per-type operations a column needs, as a plain record of function
/// pointers captured at registration. This is the vtable, visible as data.
#[derive(Clone, Copy)]
pub struct ComponentInfo {
    pub type_id: TypeId,
    pub type_name: &'static str,
    pub mask: u64,
    pub new_column: fn() -> ErasedColumn,
    pub push_default: fn(&mut ErasedColumn, usize),
    pub swap_remove: fn(&mut ErasedColumn, usize),
    pub move_row: fn(&mut ErasedColumn, usize, &mut ErasedColumn),
    pub swap_remove_into: fn(&mut ErasedColumn, usize, &mut ErasedColumn),
    pub column_len: fn(&ErasedColumn) -> usize,
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
    pub components_by_type: TypeIdMap<u32>,
    pub tag_count: u32,
    pub tags_by_type: TypeIdMap<u32>,
    #[cfg(feature = "snapshot")]
    pub codecs: Vec<Option<ComponentCodec>>,
    /// One-entry cache of the most recently resolved component type. A hot
    /// loop of `set`/`remove` over one component type hits this on every call
    /// after the first, resolving through a `TypeId` equality instead of a map
    /// probe. Never wrong: a miss just falls through to the map.
    #[cfg(feature = "raw_storage")]
    recent_component: Option<(TypeId, u32)>,
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
            components_by_type: TypeIdMap::default(),
            tag_count: 0,
            tags_by_type: TypeIdMap::default(),
            #[cfg(feature = "snapshot")]
            codecs: Vec::new(),
            #[cfg(feature = "raw_storage")]
            recent_component: None,
        }
    }

    /// Registers `T` if it is not already registered and returns its key.
    /// Idempotent per type. `Default` is required because archetype migration
    /// moves values with `mem::take`, and `Send + Sync` because columns are
    /// shared across threads by the parallel iteration paths.
    pub fn register<T: Send + Sync + Default + 'static>(&mut self) -> ComponentKey<T> {
        let type_id = TypeId::of::<T>();
        #[cfg(feature = "raw_storage")]
        if let Some((cached_id, component_index)) = self.recent_component
            && cached_id == type_id
        {
            return self.key_for(component_index);
        }
        if let Some(&component_index) = self.components_by_type.get(&type_id) {
            #[cfg(feature = "raw_storage")]
            {
                self.recent_component = Some((type_id, component_index));
            }
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
            swap_remove_into: column_swap_remove_into::<T>,
            column_len: column_len_of::<T>,
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
            encode_value: encode_value_postcard::<T>,
            apply_value: apply_value_postcard::<T>,
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

    /// Finds a component by its registered type name, the full path
    /// `std::any::type_name` reports at registration. Linear scan over the
    /// schema; resolve once and hold the record when calling per frame.
    pub fn component_by_name(&self, name: &str) -> Option<&ComponentInfo> {
        self.components.iter().find(|info| info.type_name == name)
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

    /// How many of the 64 mask bits are still free. Components and tags
    /// share the budget, components from bit 0 up and tags from bit 63 down,
    /// so this is the number of registrations of either kind left before
    /// `register` or `register_tag` panics.
    pub fn remaining_bits(&self) -> u32 {
        64 - self.components.len() as u32 - self.tag_count
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

/// One erased column plus its tick columns. `data` always holds the `Vec<T>`
/// of the registered component; a hand-swapped wrong-type box panics on the
/// next typed access rather than misbehaving. `changed` restamps on every
/// mutable access; `added` stamps when the component arrives on the entity
/// and rides along through table migrations, which is what the `added`
/// query filters compare against.
pub struct ColumnSlot {
    pub component_index: u32,
    pub data: ErasedColumn,
    pub changed: Vec<u32>,
    pub peak_changed: u32,
    pub added: Vec<u32>,
    pub peak_added: u32,
}

impl ColumnSlot {
    /// Grows the tick columns to match `count` freshly pushed rows. Under
    /// `raw_storage` the tick columns are never materialized, so change
    /// detection is disabled and this is a no-op.
    #[inline]
    fn track_extend(&mut self, count: usize, tick: u32) {
        #[cfg(not(feature = "raw_storage"))]
        {
            let filled_length = self.changed.len() + count;
            self.changed.resize(filled_length, tick);
            self.added.resize(filled_length, tick);
            self.peak_changed = tick;
            self.peak_added = tick;
        }
        #[cfg(feature = "raw_storage")]
        {
            let _ = (count, tick);
        }
    }

    /// Pushes one tick row: `tick` for the changed column, `added_value` for
    /// the added column. No-op under `raw_storage`.
    #[inline]
    fn track_push(&mut self, tick: u32, added_value: u32) {
        #[cfg(not(feature = "raw_storage"))]
        {
            self.changed.push(tick);
            self.added.push(added_value);
            self.peak_changed = tick;
            self.peak_added = tick;
        }
        #[cfg(feature = "raw_storage")]
        {
            let _ = (tick, added_value);
        }
    }

    /// Swap-removes one tick row, mirroring the data column's swap-remove.
    /// No-op under `raw_storage`.
    #[inline]
    fn track_swap_remove(&mut self, index: usize) {
        #[cfg(not(feature = "raw_storage"))]
        {
            self.changed.swap_remove(index);
            self.added.swap_remove(index);
        }
        #[cfg(feature = "raw_storage")]
        {
            let _ = index;
        }
    }

    /// The added tick carried by a row during migration. Only the safe
    /// backend threads this through; `raw_storage` migrations move bytes
    /// without touching tick columns.
    #[cfg(not(feature = "raw_storage"))]
    #[inline]
    fn carried_added(&self, index: usize) -> u32 {
        self.added.get(index).copied().unwrap_or(0)
    }
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
        column_vec::<T>(&self.columns[position].data)
    }

    /// Mutable raw column access. Does not stamp change ticks, and costs a
    /// popcount and a downcast per call, so hoist it outside per-entity
    /// loops. Use the typed query tier when change detection matters.
    #[inline]
    pub fn column_mut<T: 'static>(&mut self, key: ComponentKey<T>) -> &mut [T] {
        let position = column_position(self.mask, key.mask);
        column_vec_mut::<T>(&mut self.columns[position].data)
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
            column_vec_mut::<A>(&mut first_slot.data),
            column_vec::<B>(&second_slot.data),
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
            column_vec_mut::<A>(&mut first_slot.data),
            column_vec_mut::<B>(&mut second_slot.data),
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
    Closure(Box<dyn FnOnce(&mut DynWorld) + Send + Sync>),
}

struct EventSlot {
    type_id: TypeId,
    data: BoxedAny,
    update: fn(&mut (dyn Any + Send + Sync)),
}

fn event_update<T: Send + Sync + 'static>(data: &mut (dyn Any + Send + Sync)) {
    data.downcast_mut::<EventChannel<T>>()
        .expect("event channel type mismatch")
        .update();
}

/// The type-erased event channels one container owns. [`DynWorld`] and
/// [`DynEcs`] both embed one, so world-local and group-shared events run on
/// identical machinery: sequence-numbered channels, two-frame buffering
/// driven by [`update`](Self::update), and exactly-once consumption through
/// per-consumer cursors.
#[derive(Default)]
pub struct EventBus {
    slots: Vec<EventSlot>,
    by_type: HashMap<TypeId, usize>,
}

impl EventBus {
    fn slot_index<T: Send + Sync + 'static>(&mut self) -> usize {
        if let Some(&index) = self.by_type.get(&TypeId::of::<T>()) {
            return index;
        }
        let index = self.slots.len();
        self.slots.push(EventSlot {
            type_id: TypeId::of::<T>(),
            data: Box::new(EventChannel::<T>::new()),
            update: event_update::<T>,
        });
        self.by_type.insert(TypeId::of::<T>(), index);
        index
    }

    fn channel<T: Send + Sync + 'static>(&self) -> Option<&EventChannel<T>> {
        let index = *self.by_type.get(&TypeId::of::<T>())?;
        debug_assert_eq!(self.slots[index].type_id, TypeId::of::<T>());
        Some(
            self.slots[index]
                .data
                .downcast_ref::<EventChannel<T>>()
                .expect("event channel type mismatch"),
        )
    }

    fn channel_mut<T: Send + Sync + 'static>(&mut self) -> &mut EventChannel<T> {
        let index = self.slot_index::<T>();
        self.slots[index]
            .data
            .downcast_mut::<EventChannel<T>>()
            .expect("event channel type mismatch")
    }

    pub fn send<T: Send + Sync + 'static>(&mut self, event: T) {
        self.channel_mut::<T>().send(event);
    }

    pub fn read<T: Send + Sync + 'static>(&self) -> &[T] {
        self.channel::<T>()
            .map(|channel| channel.events.as_slice())
            .unwrap_or(&[])
    }

    pub fn read_since<T: Send + Sync + 'static>(&self, cursor: u64) -> &[T] {
        self.channel::<T>()
            .map(|channel| channel.events_since(cursor))
            .unwrap_or(&[])
    }

    pub fn consume<T: Send + Sync + 'static>(&self, cursor: &mut u64) -> &[T] {
        match self.channel::<T>() {
            Some(channel) => channel.consume(cursor),
            None => &[],
        }
    }

    pub fn sequence<T: Send + Sync + 'static>(&self) -> u64 {
        self.channel::<T>()
            .map(|channel| channel.sequence())
            .unwrap_or(0)
    }

    pub fn trim<T: Send + Sync + 'static>(&mut self, up_to_sequence: u64) {
        self.channel_mut::<T>().trim(up_to_sequence);
    }

    pub fn clear<T: Send + Sync + 'static>(&mut self) {
        self.channel_mut::<T>().clear();
    }

    pub fn channel_count(&self) -> usize {
        self.slots.len()
    }

    /// Advances every channel one frame, expiring events past their
    /// two-frame window. The containers call this from their `step`.
    pub fn update(&mut self) {
        for slot in &mut self.slots {
            (slot.update)(&mut *slot.data);
        }
    }
}

/// The type-keyed resource singletons one container owns. [`DynWorld`] and
/// [`DynEcs`] both embed one, so world-local and group-shared resources use
/// identical machinery; the containers add the expect and scope forms.
#[derive(Default)]
pub struct ResourceMap {
    pub entries: HashMap<TypeId, BoxedAny>,
}

impl ResourceMap {
    pub fn insert<T: Send + Sync + 'static>(&mut self, value: T) {
        self.entries.insert(TypeId::of::<T>(), Box::new(value));
    }

    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.entries
            .get(&TypeId::of::<T>())
            .and_then(|value| value.downcast_ref::<T>())
    }

    pub fn get_mut<T: Send + Sync + 'static>(&mut self) -> Option<&mut T> {
        self.entries
            .get_mut(&TypeId::of::<T>())
            .and_then(|value| value.downcast_mut::<T>())
    }

    pub fn remove<T: Send + Sync + 'static>(&mut self) -> Option<T> {
        self.entries
            .remove(&TypeId::of::<T>())
            .and_then(|value| value.downcast::<T>().ok())
            .map(|value| *value)
    }
}

/// Access to a resource map for the host scope methods on
/// [`ResourceHostExt`]. [`DynWorld`] and [`DynEcs`] implement it over
/// their own maps. A host that wraps either in a larger state struct
/// implements it by delegating to the wrapped map, and the scope methods
/// then lend the whole host to the closure, which the inherent scope
/// methods structurally cannot do.
///
/// Implementations must return the same map from every call. The scopes
/// call it once to take the resource and once to reinsert it, and debug
/// builds verify the reinserted resource is reachable afterward, so a
/// host that routes calls to different maps fails loudly instead of
/// silently losing the resource.
pub trait ResourceHost {
    fn resource_map_mut(&mut self) -> &mut ResourceMap;

    /// The same map, shared, for reading a resource behind a `&self`, such as
    /// a run condition that checks a state. Must return the same map as
    /// [`resource_map_mut`](Self::resource_map_mut).
    fn resource_map(&self) -> &ResourceMap;
}

/// The host scope methods, blanket-implemented for every
/// [`ResourceHost`]: `host.resource_scope(...)` takes the resource out of
/// the host's map, runs the closure with the host and the resource as
/// independent borrows, then puts the resource back, even when the
/// closure panics. This is how an engine state struct holding a
/// [`DynWorld`] or [`DynEcs`] plus its own fields lends the whole wrapper
/// to systems. On [`DynWorld`] and [`DynEcs`] the inherent methods take
/// precedence, which is harmless because they delegate here. Panics if
/// the resource is not present.
///
/// ```rust
/// use freecs::dynamic::{DynWorld, ResourceHost, ResourceHostExt, ResourceMap};
///
/// struct Engine {
///     ecs: DynWorld,
///     frames: u32,
/// }
///
/// impl ResourceHost for Engine {
///     fn resource_map_mut(&mut self) -> &mut ResourceMap {
///         &mut self.ecs.resources
///     }
///     fn resource_map(&self) -> &ResourceMap {
///         &self.ecs.resources
///     }
/// }
///
/// struct Score(u32);
///
/// let mut engine = Engine { ecs: DynWorld::new(), frames: 0 };
/// engine.ecs.insert_resource(Score(0));
///
/// engine.resource_scope(|engine, score: &mut Score| {
///     engine.frames += 1;
///     score.0 += 1;
/// });
///
/// assert_eq!(engine.frames, 1);
/// ```
pub trait ResourceHostExt: ResourceHost + Sized {
    fn resource_scope<R: Send + Sync + 'static, T>(
        &mut self,
        f: impl FnOnce(&mut Self, &mut R) -> T,
    ) -> T {
        let mut resource = self.resource_map_mut().remove::<R>().unwrap_or_else(|| {
            panic!(
                "resource_scope requires {} to be present",
                std::any::type_name::<R>()
            )
        });
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(self, &mut resource)));
        self.resource_map_mut().insert(resource);
        debug_assert!(
            self.resource_map_mut()
                .entries
                .contains_key(&TypeId::of::<R>()),
            "ResourceHost returned a different map across calls, so {} was reinserted into a map the host no longer exposes",
            std::any::type_name::<R>()
        );
        match result {
            Ok(value) => value,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    /// The tuple form of [`resource_scope`](Self::resource_scope), with
    /// the same presence and distinctness checks before anything is
    /// removed and the same reinsertion on panic.
    fn resources_scope<B: ResourceBundle, T>(
        &mut self,
        f: impl FnOnce(&mut Self, &mut B) -> T,
    ) -> T {
        let mut bundle = B::take(self.resource_map_mut());
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(self, &mut bundle)));
        bundle.put(self.resource_map_mut());
        debug_assert!(
            B::contains_all(self.resource_map_mut()),
            "ResourceHost returned a different map across calls, so the bundle was reinserted into a map the host no longer exposes"
        );
        match result {
            Ok(value) => value,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }
}

impl<H: ResourceHost> ResourceHostExt for H {}

impl ResourceHost for DynWorld {
    fn resource_map_mut(&mut self) -> &mut ResourceMap {
        &mut self.resources
    }
    fn resource_map(&self) -> &ResourceMap {
        &self.resources
    }
}

impl ResourceHost for DynEcs {
    fn resource_map_mut(&mut self) -> &mut ResourceMap {
        &mut self.resources
    }
    fn resource_map(&self) -> &ResourceMap {
        &self.resources
    }
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
    pub added_scratch: Vec<bool>,
    pub current_tick: u32,
    pub last_tick: u32,
    pub structural_log: Vec<StructuralChange>,
    pub structural_sequence: u64,
    pub tags: Vec<SparseTagSet>,
    command_buffer: Vec<DynCommand>,
    pub events: EventBus,
    pub resources: ResourceMap,
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
            added_scratch: Vec::new(),
            current_tick: 0,
            last_tick: 0,
            structural_log: Vec::new(),
            structural_sequence: 0,
            tags: Vec::new(),
            command_buffer: Vec::new(),
            events: EventBus::default(),
            resources: ResourceMap::default(),
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

    /// How many of this world's 64 mask bits are still free for components
    /// and tags combined. Lazy registration spends them silently, so budget
    /// checks belong here rather than at the panic.
    pub fn remaining_bits(&self) -> u32 {
        self.registry.remaining_bits()
    }

    /// A point-in-time census of this world's storage and bookkeeping. See
    /// [`WorldStats`] for the fields.
    pub fn stats(&self) -> WorldStats {
        WorldStats {
            entity_count: self.entity_count(),
            table_count: self.tables.len(),
            empty_table_count: self
                .tables
                .iter()
                .filter(|table| table.entity_indices.is_empty())
                .count(),
            largest_table_rows: self
                .tables
                .iter()
                .map(|table| table.entity_indices.len())
                .max()
                .unwrap_or(0),
            table_rows: self
                .tables
                .iter()
                .map(|table| (table.mask, table.entity_indices.len()))
                .collect(),
            component_count: self.registry.components.len(),
            tag_count: self.registry.tag_count as usize,
            remaining_mask_bits: self.remaining_bits(),
            structural_log_entries: self.structural_log.len(),
            query_cache_entries: self.query_cache.len(),
            resource_count: self.resources.entries.len(),
            event_channels: self.events.channel_count(),
            pending_commands: self.command_count(),
        }
    }

    /// Drops empty archetype tables and rebuilds every structure that
    /// references table positions: entity locations remap in place with
    /// retired-generation stamps untouched, the mask lookup rebuilds, the
    /// archetype edge caches reset to refill lazily, and the query cache
    /// clears. Explicit and opt-in; call at loading screens or level
    /// boundaries, not per frame. Returns how many tables were dropped.
    pub fn compact(&mut self) -> usize {
        let mut remap: Vec<Option<usize>> = Vec::with_capacity(self.tables.len());
        let mut kept = 0usize;
        for table in &self.tables {
            if table.entity_indices.is_empty() {
                remap.push(None);
            } else {
                remap.push(Some(kept));
                kept += 1;
            }
        }
        let dropped = self.tables.len() - kept;
        if dropped == 0 {
            return 0;
        }

        self.tables.retain(|table| !table.entity_indices.is_empty());
        for location in &mut self.entity_locations.locations {
            if location.allocated
                && let Some(new_index) = remap[location.table_index as usize]
            {
                location.table_index = new_index as u32;
            }
        }
        self.table_lookup.clear();
        for (table_index, table) in self.tables.iter().enumerate() {
            self.table_lookup.insert(table.mask, table_index);
        }
        self.table_edges.clear();
        self.table_edges
            .resize_with(self.tables.len(), ArchetypeEdges::default);
        self.query_cache.clear();
        dropped
    }

    /// The components an entity currently carries, as registry records with
    /// their type names, masks, and vtables. Dead or rowless entities yield
    /// nothing. This is the inspection surface for editors and tooling; pair
    /// it with [`ComponentRegistry::component_by_name`] to go the other way.
    pub fn entity_components(&self, entity: Entity) -> impl Iterator<Item = &ComponentInfo> + '_ {
        let mask = self.component_mask(entity).unwrap_or(0);
        self.registry
            .components
            .iter()
            .filter(move |info| info.mask & mask != 0)
    }

    /// Delegates to [`ComponentRegistry::component_by_name`].
    pub fn component_by_name(&self, name: &str) -> Option<&ComponentInfo> {
        self.registry.component_by_name(name)
    }

    fn check_key(&self, registry_id: u32) {
        debug_assert_eq!(
            registry_id, self.registry.registry_id,
            "key was minted by a different registry than this world's"
        );
    }

    fn record_structural(&mut self, entity: Entity, kind: StructuralChangeKind, mask: u64) {
        // raw_storage does not maintain the structural-change log: its only
        // in-crate consumer, HierarchyIndex, rebuilds from a scan instead, so
        // spawn and migration skip this per-entity push entirely.
        #[cfg(feature = "raw_storage")]
        {
            let _ = (entity, kind, mask);
        }
        #[cfg(not(feature = "raw_storage"))]
        {
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
                    added: Vec::new(),
                    peak_added: self.current_tick,
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
                (info.push_default)(&mut column.data, count);
                column.track_extend(count, current_tick);
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
            (info.swap_remove)(&mut column.data, array_index);
            column.track_swap_remove(array_index);
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

    /// Direct children of a parent: every entity whose [`ChildOf`] link
    /// points at it. A full scan of `ChildOf` carriers on each call, pull
    /// not push; cache user-side when it measures hot.
    pub fn children(&self, parent: Entity) -> Vec<Entity> {
        self.query_ref::<&ChildOf>()
            .iter()
            .filter(|(_entity, child_of)| child_of.0 == parent)
            .map(|(entity, _child_of)| entity)
            .collect()
    }

    /// Despawns an entity and every descendant reachable through
    /// [`ChildOf`] links, breadth-first over on-demand scans. Link cycles
    /// are tolerated, each entity despawns once. Returns the despawned
    /// entities. In a [`DynEcs`] group, despawn through
    /// [`DynEcs::despawn_recursive`] instead, so retirement broadcasts into
    /// every member world.
    pub fn despawn_recursive(&mut self, root: Entity) -> Vec<Entity> {
        let mut pending = vec![root];
        let mut to_despawn: Vec<Entity> = Vec::new();
        while let Some(parent) = pending.pop() {
            if to_despawn.contains(&parent) {
                continue;
            }
            to_despawn.push(parent);
            pending.extend(self.children(parent));
        }
        self.despawn_entities(&to_despawn)
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
    ) -> (usize, usize) {
        let tick = self.current_tick;
        let swapped_entity;
        {
            let [source, destination] = self
                .tables
                .get_disjoint_mut([from_table, to_table])
                .expect("migration source and destination must differ");

            let mut gained = destination.mask & !source.mask;
            while gained != 0 {
                let component_mask = gained & gained.wrapping_neg();
                gained &= gained - 1;

                let destination_position = column_position(destination.mask, component_mask);
                let destination_column = &mut destination.columns[destination_position];
                let info = &self.registry.components[destination_column.component_index as usize];
                (info.push_default)(&mut destination_column.data, 1);
                destination_column.track_push(tick, tick);
            }

            let shared = source.mask & destination.mask;
            let mut bits = shared;
            while bits != 0 {
                let component_mask = bits & bits.wrapping_neg();
                bits &= bits - 1;

                let source_position = column_position(source.mask, component_mask);
                let destination_position = column_position(destination.mask, component_mask);
                #[cfg(not(feature = "raw_storage"))]
                {
                    let carried_added = source.columns[source_position].carried_added(from_index);
                    let info = &self.registry.components
                        [source.columns[source_position].component_index as usize];
                    (info.swap_remove_into)(
                        &mut source.columns[source_position].data,
                        from_index,
                        &mut destination.columns[destination_position].data,
                    );
                    source.columns[source_position].track_swap_remove(from_index);
                    let destination_column = &mut destination.columns[destination_position];
                    destination_column.track_push(tick, carried_added);
                }
                #[cfg(feature = "raw_storage")]
                source.columns[source_position].data.swap_remove_into_raw(
                    from_index,
                    &mut destination.columns[destination_position].data,
                );
            }

            let mut removed = source.mask & !destination.mask;
            while removed != 0 {
                let component_mask = removed & removed.wrapping_neg();
                removed &= removed - 1;

                let source_position = column_position(source.mask, component_mask);
                #[cfg(not(feature = "raw_storage"))]
                {
                    let info = &self.registry.components
                        [source.columns[source_position].component_index as usize];
                    (info.swap_remove)(&mut source.columns[source_position].data, from_index);
                    source.columns[source_position].track_swap_remove(from_index);
                }
                #[cfg(feature = "raw_storage")]
                source.columns[source_position]
                    .data
                    .swap_remove_raw(from_index);
            }

            destination.entity_indices.push(entity);

            let last_index = source.entity_indices.len() - 1;
            swapped_entity = if from_index < last_index {
                Some(source.entity_indices[last_index])
            } else {
                None
            };
            source.entity_indices.swap_remove(from_index);
        }

        let new_index = self.tables[to_table].entity_indices.len() - 1;
        insert_location(&mut self.entity_locations, entity, (to_table, new_index));
        if let Some(swapped) = swapped_entity
            && let Some(location) = self.entity_locations.get_mut(swapped.id)
            && location.allocated
        {
            location.array_index = from_index as u32;
        }
        (to_table, new_index)
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
                (info.push_default)(&mut column.data, 1);
                column.track_push(current_tick, current_tick);
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
        Some(&column_vec::<T>(&table.columns[position].data)[array_index])
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
        if let Some(cell) = column.changed.get_mut(array_index) {
            *cell = current_tick;
        }
        column.peak_changed = current_tick;
        Some(&mut column_vec_mut::<T>(&mut column.data)[array_index])
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
            if let Some(cell) = column.changed.get_mut(array_index) {
                *cell = current_tick;
            }
            column.peak_changed = current_tick;
        }
        true
    }

    /// Resolves the table an entity moves to when `mask` is added, creating
    /// and caching the edge on first use.
    #[cfg(feature = "raw_storage")]
    fn resolve_add_target(&mut self, table_index: usize, mask: u64) -> usize {
        let current_mask = self.tables[table_index].mask;
        let cached = if mask.count_ones() == 1 {
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
        cached.unwrap_or_else(|| {
            let new_index = self.get_or_create_table(current_mask | mask);
            self.table_edges[table_index]
                .multi_add_cache
                .insert(mask, new_index);
            new_index
        })
    }

    pub fn set_keyed<T: 'static>(&mut self, key: ComponentKey<T>, entity: Entity, value: T) {
        self.check_key(key.registry_id);
        let current_tick = self.current_tick;

        // Under raw_storage change detection is off and record_structural is a
        // no-op, so the migration path collapses to: locate once, migrate
        // reusing the location move_entity returns, write the value in place.
        // No tick stamps, no structural push, no second location lookup.
        #[cfg(feature = "raw_storage")]
        {
            let _ = current_tick;
            if let Some((table_index, array_index)) = get_location(&self.entity_locations, entity) {
                if self.tables[table_index].mask & key.mask != 0 {
                    let position = column_position(self.tables[table_index].mask, key.mask);
                    column_vec_mut::<T>(&mut self.tables[table_index].columns[position].data)
                        [array_index] = value;
                    return;
                }
                let target = self.resolve_add_target(table_index, key.mask);
                let (new_table, new_index) =
                    self.move_entity(entity, table_index, array_index, target);
                let position = column_position(self.tables[new_table].mask, key.mask);
                column_vec_mut::<T>(&mut self.tables[new_table].columns[position].data)
                    [new_index] = value;
                return;
            }
            if self.add_components(entity, key.mask)
                && let Some((table_index, array_index)) =
                    get_location(&self.entity_locations, entity)
            {
                let position = column_position(self.tables[table_index].mask, key.mask);
                column_vec_mut::<T>(&mut self.tables[table_index].columns[position].data)
                    [array_index] = value;
            }
        }

        #[cfg(not(feature = "raw_storage"))]
        {
            if let Some((table_index, array_index)) = get_location(&self.entity_locations, entity) {
                let table = &mut self.tables[table_index];
                if table.mask & key.mask != 0 {
                    let position = column_position(table.mask, key.mask);
                    let column = &mut table.columns[position];
                    column_vec_mut::<T>(&mut column.data)[array_index] = value;
                    if let Some(cell) = column.changed.get_mut(array_index) {
                        *cell = current_tick;
                    }
                    column.peak_changed = current_tick;
                    return;
                }
            }
            if self.add_components(entity, key.mask)
                && let Some((table_index, array_index)) =
                    get_location(&self.entity_locations, entity)
            {
                let table = &mut self.tables[table_index];
                let position = column_position(table.mask, key.mask);
                let column = &mut table.columns[position];
                column_vec_mut::<T>(&mut column.data)[array_index] = value;
                if let Some(cell) = column.changed.get_mut(array_index) {
                    *cell = current_tick;
                }
                column.peak_changed = current_tick;
            }
        }
    }

    /// Appends `count` clones of `value` to one column in one table, resolving
    /// the column once. The batch spawn path grows every column this way, so a
    /// bundle spawn writes each component once rather than a default followed
    /// by an overwrite.
    pub fn extend_column<T: Send + Sync + Default + Clone + 'static>(
        &mut self,
        table_index: usize,
        count: usize,
        value: &T,
    ) {
        let key = self.component_key::<T>();
        let table = &mut self.tables[table_index];
        let position = column_position(table.mask, key.mask);
        let column = &mut table.columns[position];
        column.data.extend_clone::<T>(count, value);
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
                        && column
                            .changed
                            .get(index)
                            .is_some_and(|&value| tick_is_newer(value, since_tick))
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
                                && column
                                    .changed
                                    .get(*index)
                                    .is_some_and(|&value| tick_is_newer(value, since_tick))
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
        self.events.update();
        self.last_tick = self.current_tick;
        self.current_tick = self.current_tick.wrapping_add(1);
    }

    pub fn send<T: Send + Sync + 'static>(&mut self, event: T) {
        self.events.send(event);
    }

    /// Everything still buffered for `T`, oldest first. An unregistered event
    /// type reads as empty, which is indistinguishable from registered and
    /// empty.
    pub fn read_events<T: Send + Sync + 'static>(&self) -> &[T] {
        self.events.read::<T>()
    }

    pub fn read_events_since<T: Send + Sync + 'static>(&self, cursor: u64) -> &[T] {
        self.events.read_since::<T>(cursor)
    }

    /// The exactly-once read: yields events sent after the cursor and
    /// advances it past them, so a handler calling this every frame sees
    /// each event once. Events stay buffered for two frames, so
    /// [`read_events`](Self::read_events) re-delivers on the second frame;
    /// keep one `u64` cursor per consumer and reach for this by default.
    pub fn consume_events<T: Send + Sync + 'static>(&self, cursor: &mut u64) -> &[T] {
        self.events.consume::<T>(cursor)
    }

    pub fn event_sequence<T: Send + Sync + 'static>(&self) -> u64 {
        self.events.sequence::<T>()
    }

    pub fn trim_events<T: Send + Sync + 'static>(&mut self, up_to_sequence: u64) {
        self.events.trim::<T>(up_to_sequence);
    }

    pub fn clear_events<T: Send + Sync + 'static>(&mut self) {
        self.events.clear::<T>();
    }

    pub fn insert_resource<T: Send + Sync + 'static>(&mut self, value: T) {
        self.resources.insert(value);
    }

    /// Inserts several resources at once from a tuple, each replacing any
    /// existing resource of its type. Equivalent to one
    /// [`insert_resource`](Self::insert_resource) per element, so
    /// `world.insert_resources((DeltaTime(0.016), Score(0)))` stands in for
    /// two calls.
    pub fn insert_resources<B: ResourceBundle>(&mut self, bundle: B) {
        bundle.put(&mut self.resources);
    }

    pub fn resource<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.resources.get::<T>()
    }

    pub fn resource_mut<T: Send + Sync + 'static>(&mut self) -> Option<&mut T> {
        self.resources.get_mut::<T>()
    }

    /// [`resource`](Self::resource) for resources that must exist: panics
    /// with the type name instead of returning `Option`, so call sites for
    /// engine-style singletons stay free of `unwrap`.
    pub fn res<T: Send + Sync + 'static>(&self) -> &T {
        self.resource::<T>()
            .unwrap_or_else(|| panic!("res requires {} to be present", std::any::type_name::<T>()))
    }

    pub fn res_mut<T: Send + Sync + 'static>(&mut self) -> &mut T {
        self.resource_mut::<T>().unwrap_or_else(|| {
            panic!(
                "res_mut requires {} to be present",
                std::any::type_name::<T>()
            )
        })
    }

    pub fn remove_resource<T: Send + Sync + 'static>(&mut self) -> Option<T> {
        self.resources.remove::<T>()
    }

    /// Takes `R` out of the world, runs the closure with the world and the
    /// resource as independent borrows, then puts the resource back. This is
    /// the take/put pattern for systems that mutate both a resource and the
    /// world in one pass; the resource is absent from the world inside the
    /// closure and is reinserted even when the closure panics, before the
    /// panic resumes. Panics if `R` is not present. For several resources at
    /// once, use [`resources_scope`](Self::resources_scope).
    ///
    /// A host that wraps this world in a larger state struct cannot lend
    /// that wrapper through this method, because the closure receives only
    /// the `DynWorld`. Implement [`ResourceHost`] on the wrapper and
    /// import [`ResourceHostExt`], whose scope methods lend the host
    /// itself to the closure.
    pub fn resource_scope<R: Send + Sync + 'static, T>(
        &mut self,
        f: impl FnOnce(&mut DynWorld, &mut R) -> T,
    ) -> T {
        ResourceHostExt::resource_scope(self, f)
    }

    /// The tuple form of [`resource_scope`](Self::resource_scope): takes
    /// every resource in the tuple out of the world, runs the closure with
    /// the world and the tuple as independent borrows, and puts them all
    /// back, even when the closure panics. Destructure the tuple in the
    /// closure parameter. Panics before touching anything if a resource is
    /// missing or a type repeats.
    ///
    /// ```rust
    /// # use freecs::dynamic::DynWorld;
    /// # #[derive(Default, Clone, Debug, PartialEq)]
    /// # struct Position { x: f32, y: f32 }
    /// struct DeltaTime(f32);
    /// struct Score(u32);
    ///
    /// let mut world = DynWorld::new();
    /// world.insert_resource(DeltaTime(0.5));
    /// world.insert_resource(Score(0));
    /// # world.spawn((Position { x: 1.0, y: 0.0 },));
    ///
    /// world.resources_scope(|world, (delta_time, score): &mut (DeltaTime, Score)| {
    ///     score.0 += world
    ///         .query_ref::<(&Position,)>()
    ///         .iter()
    ///         .filter(|(_entity, (position,))| position.x * delta_time.0 > 0.25)
    ///         .count() as u32;
    /// });
    ///
    /// assert_eq!(world.resource::<Score>().unwrap().0, 1);
    /// assert_eq!(world.resource::<DeltaTime>().unwrap().0, 0.5);
    /// ```
    pub fn resources_scope<B: ResourceBundle, T>(
        &mut self,
        f: impl FnOnce(&mut DynWorld, &mut B) -> T,
    ) -> T {
        ResourceHostExt::resources_scope(self, f)
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

    /// Queues a bundle spawn and returns the entity handle immediately, so
    /// other queued commands and systems can reference it before it has
    /// rows. The handle is allocated now, alive with no components, and
    /// gains the bundle's components when
    /// [`apply_commands`](Self::apply_commands) runs.
    pub fn queue_spawn<B: Bundle + Send + Sync + 'static>(&mut self, bundle: B) -> Entity {
        let entity = self.allocator.allocate();
        self.queue(move |world| {
            if !world.is_alive(entity) {
                return;
            }
            let mask = B::component_mask(world);
            if !world.contains_entity(entity) {
                world.insert_row(entity, mask);
            }
            bundle.write(world, entity);
        });
        entity
    }

    /// Queues an arbitrary deferred mutation.
    pub fn queue(&mut self, command: impl FnOnce(&mut DynWorld) + Send + Sync + 'static) {
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

    /// The typed bulk spawn: `count` entities each carrying a clone of the
    /// bundle. For per-entity initialization at batch speed, use the keyed
    /// [`spawn_batch`](Self::spawn_batch) instead.
    pub fn spawn_bundles<B: CloneBundle>(&mut self, bundle: B, count: usize) -> Vec<Entity> {
        let mask = B::component_mask(self);
        let table_index = self.get_or_create_table(mask);
        let current_tick = self.current_tick;
        let start_index = self.tables[table_index].entity_indices.len();

        let mut entities = Vec::new();
        let mut allocator = std::mem::take(&mut self.allocator);
        allocator.allocate_batch(count, &mut entities);
        self.allocator = allocator;

        bundle.spawn_extend(self, table_index, count);

        {
            let table = &mut self.tables[table_index];
            for column in &mut table.columns {
                column.track_extend(count, current_tick);
            }
            table.entity_indices.extend_from_slice(&entities);
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

    /// Starts a typed query. Component types register lazily, borrow
    /// mutability comes from the tuple (`&T`, `&mut T`, and their `Option`
    /// forms), and mutable elements stamp change ticks per visited entity.
    /// Single-component queries can skip the tuple: `query::<&mut Position>()`.
    ///
    /// This method takes `&mut self` even for all-shared tuples, because
    /// lazy registration and the query cache mutate the world; when you only
    /// have `&world`, use [`query_ref`](Self::query_ref), which is also the
    /// form that returns a real iterator.
    pub fn query<Q: QueryTuple>(&mut self) -> DynQuery<'_, Q> {
        let include = Q::component_mask(self);
        DynQuery {
            world: self,
            include,
            exclude: 0,
            changed_mask: 0,
            added_mask: 0,
            element_masks: None,
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
            added_mask: 0,
            resolved_masks: None,
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
/// The group keeps its own lifecycle log, the same two-log split as the
/// macro's multi-world form: this log records handle allocation (`Spawned`),
/// handle death anywhere (`Despawned`), and group tag flips, while each
/// member world's own structural log records that world's row history. Sync
/// world contents from the world logs and entity lifetime or group tags from
/// this one; a consumer merging both will see one entity spawn twice.
///
/// ```rust
/// use freecs::dynamic::{ComponentRegistry, DynEcs};
/// use freecs::StructuralChangeKind;
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
///
/// // "Entity died anywhere" is one stream on the group, not a per-world
/// // question. Consumers keep a cursor and the owner trims consumed entries.
/// let mut cursor = 0;
/// let kinds: Vec<StructuralChangeKind> = ecs
///     .structural_changes_since(cursor)
///     .iter()
///     .map(|change| change.kind)
///     .collect();
/// assert_eq!(kinds, vec![
///     StructuralChangeKind::Spawned,
///     StructuralChangeKind::TagsAdded,
///     StructuralChangeKind::Despawned,
/// ]);
/// cursor = ecs.structural_sequence();
/// ecs.trim_structural_log(cursor);
/// assert!(ecs.structural_changes_since(0).is_empty());
/// ```
#[derive(Default)]
pub struct DynEcs {
    pub allocator: EntityAllocator,
    pub worlds: Vec<DynWorld>,
    pub tags: Vec<SparseTagSet>,
    pub structural_log: Vec<StructuralChange>,
    pub structural_sequence: u64,
    pub type_routes: HashMap<TypeId, usize>,
    pub tag_type_indices: HashMap<TypeId, usize>,
    pub tag_type_names: Vec<Option<String>>,
    pub resources: ResourceMap,
    pub events: EventBus,
}

impl DynEcs {
    pub fn new() -> Self {
        Self::default()
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

    pub fn structural_sequence(&self) -> u64 {
        self.structural_sequence
    }

    /// The group-level lifecycle log: handle allocation (`Spawned` with mask
    /// 0), handle death anywhere (`Despawned` with mask 0), and group tag
    /// flips (`TagsAdded`/`TagsRemoved` carrying the tag index in the mask
    /// field, since group tags have no mask bits). Row-level history lives in
    /// each member world's own structural log, where an entity is `Spawned`
    /// with a component mask when its first components arrive there.
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

    /// Adds a world built from the given registry and returns its index.
    /// Grouped worlds insert rows for live handles they have never stored,
    /// which is what lets an entity gain components per world lazily.
    pub fn add_world(&mut self, registry: ComponentRegistry) -> usize {
        for info in &registry.components {
            for (index, world) in self.worlds.iter().enumerate() {
                assert!(
                    world.registry.component_by_name(info.type_name).is_none(),
                    "{} is already registered in member world {index};                      a component type must live in exactly one member world",
                    info.type_name
                );
            }
        }
        let mut world = DynWorld::from_registry(registry);
        world.insert_missing_rows = true;
        self.worlds.push(world);
        self.worlds.len() - 1
    }

    /// Allocates a handle with no rows anywhere. Give it components through
    /// any member world's `set`/`add_components`.
    pub fn spawn(&mut self) -> Entity {
        let entity = self.allocator.allocate();
        self.record_structural(entity, StructuralChangeKind::Spawned, 0);
        entity
    }

    /// Spawns one group entity carrying the bundle, with each component
    /// routed to the member world that registered its type, so a bundle can
    /// span worlds. Panics like [`set`](Self::set) if a component type is
    /// registered nowhere.
    pub fn spawn_with<B: Bundle>(&mut self, bundle: B) -> Entity {
        let entity = self.spawn();
        bundle.write_group(self, entity);
        entity
    }

    /// Which member world holds `T`, scanning members in index order and
    /// caching the answer in [`type_routes`](Self::type_routes). Returns
    /// `None` when no member world has registered `T`; group-typed access
    /// never registers lazily, because only a schema decides where a type
    /// lives.
    pub fn route<T: Send + Sync + Default + 'static>(&mut self) -> Option<usize> {
        if let Some(&index) = self.type_routes.get(&TypeId::of::<T>()) {
            return Some(index);
        }
        let index = route_world_scan(
            &self.worlds,
            |world| world.lookup_key::<T>().is_some(),
            std::any::type_name::<T>(),
        )?;
        self.type_routes.insert(TypeId::of::<T>(), index);
        Some(index)
    }

    fn route_ref<T: Send + Sync + Default + 'static>(&self) -> Option<usize> {
        if let Some(&index) = self.type_routes.get(&TypeId::of::<T>()) {
            return Some(index);
        }
        route_world_scan(
            &self.worlds,
            |world| world.lookup_key::<T>().is_some(),
            std::any::type_name::<T>(),
        )
    }

    /// Reads `T` from whichever member world holds it, no world index
    /// required.
    pub fn get<T: Send + Sync + Default + 'static>(&self, entity: Entity) -> Option<&T> {
        let index = self.route_ref::<T>()?;
        self.worlds[index].get::<T>(entity)
    }

    /// The mutable form of [`get`](Self::get); stamps change ticks exactly
    /// like the member world's accessor.
    pub fn get_mut<T: Send + Sync + Default + 'static>(
        &mut self,
        entity: Entity,
    ) -> Option<&mut T> {
        let index = self.route::<T>()?;
        self.worlds[index].get_mut::<T>(entity)
    }

    /// Writes `T` on the member world that registered it, adding the
    /// component if the entity lacks it. Panics if no member world has
    /// registered `T`: group-typed access never picks a world for a new
    /// type, that is a schema decision.
    pub fn set<T: Send + Sync + Default + 'static>(&mut self, entity: Entity, value: T) {
        let Some(index) = self.route::<T>() else {
            panic!(
                "{} is not registered in any member world; add it to a member schema first",
                std::any::type_name::<T>()
            );
        };
        self.worlds[index].set(entity, value);
    }

    /// Whether the entity carries `T` in whichever member world holds it.
    pub fn has<T: Send + Sync + Default + 'static>(&self, entity: Entity) -> bool {
        self.route_ref::<T>()
            .is_some_and(|index| self.worlds[index].has::<T>(entity))
    }

    /// Removes `T` from the member world that holds it. Returns false when
    /// the type is registered nowhere or the entity lacks it.
    pub fn remove<T: Send + Sync + Default + 'static>(&mut self, entity: Entity) -> bool {
        match self.route::<T>() {
            Some(index) => self.worlds[index].remove::<T>(entity),
            None => false,
        }
    }

    /// A typed query against the first member world where every required
    /// element of the tuple is registered; optional elements do not
    /// constrain the routing. Panics when no member world qualifies: a
    /// mutable query cannot pick a world to register types in, and a tuple
    /// spanning member worlds runs through
    /// [`query_join`](Self::query_join) instead.
    pub fn query<Q: QueryTuple>(&mut self) -> DynQuery<'_, Q> {
        let index = self
            .worlds
            .iter()
            .position(|world| Q::routing_match(world))
            .expect(
                "no member world registers every required component of the query tuple; \
                 tuples spanning member worlds run through query_join",
            );
        self.worlds[index].query::<Q>()
    }

    /// A typed query whose tuple may span member worlds, joined by entity.
    /// One world drives the iteration at full slice speed, the world
    /// holding every mutable element; the other worlds resolve their
    /// elements per entity at `get` speed, read-only, skipping entities
    /// that lack a required foreign component. Mutable elements in two
    /// different worlds panic at [`for_each`](DynJoin::for_each): mutate
    /// your own state, read theirs, or co-locate the types in one schema
    /// when a hot loop needs slice speed for everything. A tuple that
    /// resolves to a single world degenerates to a plain scan of it.
    pub fn query_join<Q: QueryTuple>(&mut self) -> DynJoin<'_, Q> {
        DynJoin {
            ecs: self,
            include_tag_types: [None; 4],
            exclude_tag_types: [None; 4],
            changed_lookups: [None; 4],
            added_lookups: [None; 4],
            marker: PhantomData,
        }
    }

    /// The read-only cross-world join, a real [`Iterator`] on `&self`:
    /// items borrow the group, the driver world walks its tables, and
    /// foreign elements resolve per entity, read-only like every join
    /// element. Unresolvable routing or filters degrade to an empty
    /// iterator, matching [`query_ref`](Self::query_ref).
    pub fn query_join_ref<Q: ReadQueryTuple>(&self) -> DynJoinRef<'_, Q> {
        DynJoinRef {
            ecs: self,
            include_tag_types: [None; 4],
            exclude_tag_types: [None; 4],
            changed_lookups: [None; 4],
            added_lookups: [None; 4],
            marker: PhantomData,
        }
    }

    /// The read-only routed query. When no member world registers every
    /// required element the query is empty rather than a panic, matching
    /// [`DynWorld::query_ref`]'s graceful degradation.
    pub fn query_ref<Q: ReadQueryTuple>(&self) -> DynQueryRef<'_, Q> {
        match self.worlds.iter().position(|world| Q::routing_match(world)) {
            Some(index) => self.worlds[index].query_ref::<Q>(),
            None => {
                let mut query = self.worlds[0].query_ref::<Q>();
                query.dead = true;
                query
            }
        }
    }

    /// Advances the group frame: expires group events past their two-frame
    /// window and steps every member world, so one call at frame end drives
    /// group-level and world-level event lifetimes and change windows
    /// together. This call replaces per-member stepping; call either this
    /// or the members' own `step`s each frame, never both, or change
    /// windows and event expiry advance twice per frame.
    pub fn step(&mut self) {
        self.events.update();
        for world in &mut self.worlds {
            world.step();
        }
    }

    /// Sends a group-level event, the shared channel for events that cross
    /// member-world (and plugin) boundaries; world-local events stay on
    /// [`DynWorld::send`]. Same two-frame buffer, expired by
    /// [`step`](Self::step).
    pub fn send<T: Send + Sync + 'static>(&mut self, event: T) {
        self.events.send(event);
    }

    /// Everything still buffered for `T` at the group level, oldest first.
    pub fn read_events<T: Send + Sync + 'static>(&self) -> &[T] {
        self.events.read::<T>()
    }

    pub fn read_events_since<T: Send + Sync + 'static>(&self, cursor: u64) -> &[T] {
        self.events.read_since::<T>(cursor)
    }

    /// The exactly-once group read: yields events sent after the cursor and
    /// advances it past them. Keep one `u64` cursor per consumer.
    pub fn consume_events<T: Send + Sync + 'static>(&self, cursor: &mut u64) -> &[T] {
        self.events.consume::<T>(cursor)
    }

    pub fn event_sequence<T: Send + Sync + 'static>(&self) -> u64 {
        self.events.sequence::<T>()
    }

    pub fn clear_events<T: Send + Sync + 'static>(&mut self) {
        self.events.clear::<T>();
    }

    /// Inserts a group-level resource, the home for state shared across
    /// member worlds and plugins; world-local resources stay on
    /// [`DynWorld::insert_resource`].
    pub fn insert_resource<T: Send + Sync + 'static>(&mut self, value: T) {
        self.resources.insert(value);
    }

    /// Inserts several group-level resources at once from a tuple, each
    /// replacing any existing resource of its type. Equivalent to one
    /// [`insert_resource`](Self::insert_resource) per element.
    pub fn insert_resources<B: ResourceBundle>(&mut self, bundle: B) {
        bundle.put(&mut self.resources);
    }

    pub fn resource<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.resources.get::<T>()
    }

    pub fn resource_mut<T: Send + Sync + 'static>(&mut self) -> Option<&mut T> {
        self.resources.get_mut::<T>()
    }

    /// [`resource`](Self::resource) for resources that must exist: panics
    /// with the type name instead of returning `Option`.
    pub fn res<T: Send + Sync + 'static>(&self) -> &T {
        self.resource::<T>()
            .unwrap_or_else(|| panic!("res requires {} to be present", std::any::type_name::<T>()))
    }

    /// The mutable form of [`res`](Self::res).
    pub fn res_mut<T: Send + Sync + 'static>(&mut self) -> &mut T {
        match self.resource_mut::<T>() {
            Some(resource) => resource,
            None => panic!(
                "res_mut requires {} to be present",
                std::any::type_name::<T>()
            ),
        }
    }

    pub fn remove_resource<T: Send + Sync + 'static>(&mut self) -> Option<T> {
        self.resources.remove::<T>()
    }

    /// Takes a group resource out, runs the closure with the group and the
    /// resource as independent borrows, then puts it back, even when the
    /// closure panics. Panics if `R` is not present.
    ///
    /// The closure receives the bare `DynEcs`, so a host that wraps the
    /// group in its own state struct implements [`ResourceHost`] and
    /// imports [`ResourceHostExt`], whose scope methods lend the host
    /// itself to the closure.
    pub fn resource_scope<R: Send + Sync + 'static, T>(
        &mut self,
        f: impl FnOnce(&mut DynEcs, &mut R) -> T,
    ) -> T {
        ResourceHostExt::resource_scope(self, f)
    }

    /// The tuple form of [`resource_scope`](Self::resource_scope), same
    /// semantics as [`DynWorld::resources_scope`].
    pub fn resources_scope<B: ResourceBundle, T>(
        &mut self,
        f: impl FnOnce(&mut DynEcs, &mut B) -> T,
    ) -> T {
        ResourceHostExt::resources_scope(self, f)
    }

    /// [`add_world`](Self::add_world) with the index asserted against the
    /// constant a schema pairs with this member, replacing the hand-written
    /// add-then-assert dance. Panics when members register out of
    /// declaration order.
    pub fn add_world_at(&mut self, expected_index: usize, registry: ComponentRegistry) -> usize {
        let index = self.add_world(registry);
        assert_eq!(
            index, expected_index,
            "member world registered out of declaration order"
        );
        index
    }

    pub fn spawn_count(&mut self, count: usize) -> Vec<Entity> {
        let mut entities = Vec::new();
        self.allocator.allocate_batch(count, &mut entities);
        for &entity in &entities {
            self.record_structural(entity, StructuralChangeKind::Spawned, 0);
        }
        entities
    }

    /// Spawns entities with rows in one member world. The handles land in
    /// the group lifecycle log as `Spawned` with mask 0; the component mask
    /// lands in that world's own structural log.
    pub fn spawn_entities(&mut self, world_index: usize, mask: u64, count: usize) -> Vec<Entity> {
        let entities = self.worlds[world_index].spawn_entities_in(&mut self.allocator, mask, count);
        for &entity in &entities {
            self.record_structural(entity, StructuralChangeKind::Spawned, 0);
        }
        entities
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
        self.record_structural(entity, StructuralChangeKind::Despawned, 0);
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

    /// Despawns an entity and every descendant reachable through [`ChildOf`]
    /// links in any member world, breadth-first over on-demand scans. This
    /// is the grouped form of [`DynWorld::despawn_recursive`]: each entity
    /// dies through the group, so retirement broadcasts into every member
    /// world, group tags drop, and the lifecycle log records each death.
    /// Link cycles are tolerated, each entity despawns once. Returns the
    /// despawned entities.
    pub fn despawn_recursive(&mut self, root: Entity) -> Vec<Entity> {
        let mut pending = vec![root];
        let mut to_despawn: Vec<Entity> = Vec::new();
        while let Some(parent) = pending.pop() {
            if to_despawn.contains(&parent) {
                continue;
            }
            to_despawn.push(parent);
            for world in &self.worlds {
                pending.extend(world.children(parent));
            }
        }
        self.despawn_entities(&to_despawn)
    }

    /// Registers a group-level tag and returns its index. Group tags have no
    /// mask bit; they filter queries by set reference.
    pub fn register_tag(&mut self) -> usize {
        self.tags.push(SparseTagSet::default());
        self.tag_type_names.push(None);
        self.tags.len() - 1
    }

    pub fn add_tag(&mut self, tag_index: usize, entity: Entity) {
        if self.allocator.is_alive(entity) && self.tags[tag_index].insert(entity) {
            self.record_structural(entity, StructuralChangeKind::TagsAdded, tag_index as u64);
        }
    }

    pub fn remove_tag(&mut self, tag_index: usize, entity: Entity) -> bool {
        let removed = self.tags[tag_index].remove(entity);
        if removed {
            self.record_structural(entity, StructuralChangeKind::TagsRemoved, tag_index as u64);
        }
        removed
    }

    pub fn has_tag(&self, tag_index: usize, entity: Entity) -> bool {
        self.tags[tag_index].contains(entity)
    }

    pub fn query_tag(&self, tag_index: usize) -> impl Iterator<Item = Entity> + '_ {
        self.tags[tag_index].iter()
    }

    /// The group-level census. See [`EcsStats`] for the fields.
    pub fn stats(&self) -> EcsStats {
        EcsStats {
            live_entities: self
                .allocator
                .slots
                .iter()
                .filter(|slot| slot.alive)
                .count(),
            free_ids: self.allocator.free_ids.len(),
            group_tag_count: self.tags.len(),
            group_structural_log_entries: self.structural_log.len(),
            group_resource_count: self.resources.entries.len(),
            group_event_channels: self.events.channel_count(),
            worlds: self.worlds.iter().map(|world| world.stats()).collect(),
        }
    }

    /// [`DynWorld::compact`] over every member world. Returns the total
    /// number of tables dropped.
    pub fn compact(&mut self) -> usize {
        self.worlds.iter_mut().map(|world| world.compact()).sum()
    }

    /// The group tag index for marker type `T`, registering the tag on
    /// first use. Group tags are the natural home for entity-scoped
    /// markers: they consume no member world's mask bits and need no world
    /// index to touch.
    pub fn tag_type_index<T: 'static>(&mut self) -> usize {
        if let Some(&index) = self.tag_type_indices.get(&TypeId::of::<T>()) {
            return index;
        }
        let name = std::any::type_name::<T>();
        let index = match self.scan_tag_type_names(name) {
            Some(index) => index,
            None => {
                let index = self.register_tag();
                self.tag_type_names[index] = Some(name.to_string());
                index
            }
        };
        self.tag_type_indices.insert(TypeId::of::<T>(), index);
        index
    }

    /// The group tag index for marker type `T` if it has been used, without
    /// registering it. Falls back to the persisted type names, so marker
    /// tags restored by [`DynEcs::from_snapshot`] resolve here before the
    /// `TypeId` map is rebuilt.
    pub fn lookup_tag_type<T: 'static>(&self) -> Option<usize> {
        if let Some(&index) = self.tag_type_indices.get(&TypeId::of::<T>()) {
            return Some(index);
        }
        self.scan_tag_type_names(std::any::type_name::<T>())
    }

    fn scan_tag_type_names(&self, name: &str) -> Option<usize> {
        self.tag_type_names
            .iter()
            .position(|stored| stored.as_deref() == Some(name))
    }

    /// Adds the marker type `T`'s group tag to an entity, registering the
    /// tag on first use.
    pub fn add_tag_type<T: 'static>(&mut self, entity: Entity) {
        let index = self.tag_type_index::<T>();
        self.add_tag(index, entity);
    }

    /// Removes the marker type `T`'s group tag from an entity. Unregistered
    /// marker types remove nothing.
    pub fn remove_tag_type<T: 'static>(&mut self, entity: Entity) -> bool {
        match self.lookup_tag_type::<T>() {
            Some(index) => self.remove_tag(index, entity),
            None => false,
        }
    }

    /// Whether an entity carries the marker type `T`'s group tag.
    /// Unregistered marker types read as absent.
    pub fn has_tag_type<T: 'static>(&self, entity: Entity) -> bool {
        match self.lookup_tag_type::<T>() {
            Some(index) => self.has_tag(index, entity),
            None => false,
        }
    }

    /// Iterates entities carrying the marker type `T`'s group tag.
    /// Unregistered marker types match nothing.
    pub fn query_tag_type<T: 'static>(&self) -> impl Iterator<Item = Entity> + '_ {
        self.lookup_tag_type::<T>()
            .into_iter()
            .flat_map(|index| self.tags[index].iter())
    }

    /// The marker type `T`'s group tag set, for composing into per-world
    /// typed queries with `with_tag_set`/`without_tag_set`. `None` until
    /// the tag's first use.
    pub fn tag_set_type<T: 'static>(&self) -> Option<&SparseTagSet> {
        self.lookup_tag_type::<T>().map(|index| &self.tags[index])
    }
}

#[cfg(feature = "snapshot")]
mod snapshot {
    use super::*;

    /// Column byte codec for one component type, plain function pointers
    /// like the rest of the registry's vtable. The built-in pair encodes the
    /// whole `Vec<T>` with postcard; any byte format works as long as encode
    /// and decode agree.
    /// Encodes one entity's component as codec bytes; `None` when absent.
    pub type EncodeValueFn = fn(&DynWorld, Entity) -> Option<Result<Vec<u8>, SnapshotError>>;

    #[derive(Clone, Copy)]
    pub struct ComponentCodec {
        pub encode_column: fn(&(dyn Any + Send + Sync)) -> Result<Vec<u8>, SnapshotError>,
        pub decode_column: fn(&[u8]) -> Result<ErasedColumn, SnapshotError>,
        pub encode_value: EncodeValueFn,
        pub apply_value: fn(&mut DynWorld, Entity, &[u8]) -> Result<(), SnapshotError>,
    }

    pub(super) fn encode_value_postcard<T>(
        world: &DynWorld,
        entity: Entity,
    ) -> Option<Result<Vec<u8>, SnapshotError>>
    where
        T: serde::Serialize + Send + Sync + Default + 'static,
    {
        world.get::<T>(entity).map(|value| {
            postcard::to_allocvec(value).map_err(|error| SnapshotError::Codec(error.to_string()))
        })
    }

    pub(super) fn apply_value_postcard<T>(
        world: &mut DynWorld,
        entity: Entity,
        bytes: &[u8],
    ) -> Result<(), SnapshotError>
    where
        T: serde::de::DeserializeOwned + Send + Sync + Default + 'static,
    {
        let value: T =
            postcard::from_bytes(bytes).map_err(|error| SnapshotError::Codec(error.to_string()))?;
        world.set(entity, value);
        Ok(())
    }

    pub(super) fn encode_column_postcard<T>(
        column: &(dyn Any + Send + Sync),
    ) -> Result<Vec<u8>, SnapshotError>
    where
        T: serde::Serialize + Send + Sync + Default + 'static,
    {
        let column = column
            .downcast_ref::<ErasedColumn>()
            .expect("snapshot column codec received a value that is not an ErasedColumn");
        postcard::to_allocvec(column_vec::<T>(column))
            .map_err(|error| SnapshotError::Codec(error.to_string()))
    }

    pub(super) fn decode_column_postcard<T>(bytes: &[u8]) -> Result<ErasedColumn, SnapshotError>
    where
        T: serde::de::DeserializeOwned + Send + Sync + Default + 'static,
    {
        let values: Vec<T> =
            postcard::from_bytes(bytes).map_err(|error| SnapshotError::Codec(error.to_string()))?;
        let mut column = ErasedColumn::new::<T>();
        for value in values {
            column.push::<T>(value);
        }
        Ok(column)
    }

    #[derive(Debug, Clone, PartialEq)]
    pub enum SnapshotError {
        /// A component present in a table has no registered codec.
        MissingCodec(&'static str),
        /// The registry's component names or order do not match the snapshot.
        SchemaMismatch { expected: String, found: String },
        /// A column failed to encode or decode.
        Codec(String),
        /// No registered component carries the requested type name.
        UnknownComponent(String),
        /// A value write named an entity that is not alive.
        DeadEntity,
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
                SnapshotError::UnknownComponent(name) => {
                    write!(formatter, "no registered component is named {name}")
                }
                SnapshotError::DeadEntity => {
                    write!(formatter, "the entity is not alive")
                }
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
        pub tag_type_names: Vec<Option<String>>,
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
        fn value_codec(&self, name: &str) -> Result<&ComponentCodec, SnapshotError> {
            let info = self
                .registry
                .component_by_name(name)
                .ok_or_else(|| SnapshotError::UnknownComponent(name.to_string()))?;
            self.registry.codecs[info.mask.trailing_zeros() as usize]
                .as_ref()
                .ok_or(SnapshotError::MissingCodec(info.type_name))
        }

        /// Sets one component on an entity from codec bytes, resolved by
        /// registered type name, adding the component when the entity lacks
        /// it and stamping change ticks like any `set`. The wire format is
        /// whatever the component's codec chose, postcard for
        /// `register_serde` components; together with
        /// [`Self::get_component_by_name`] this is the value-level door for
        /// editors and protocols, no per-component dispatch required.
        /// Standalone worlds reject dead entities with
        /// [`SnapshotError::DeadEntity`]; a grouped member world defers
        /// liveness to its group, so route grouped writes through
        /// [`DynEcs::set_component_by_name`], which checks it.
        pub fn set_component_by_name(
            &mut self,
            entity: Entity,
            name: &str,
            bytes: &[u8],
        ) -> Result<(), SnapshotError> {
            if !self.insert_missing_rows && !self.is_alive(entity) {
                return Err(SnapshotError::DeadEntity);
            }
            let apply = self.value_codec(name)?.apply_value;
            apply(self, entity, bytes)
        }

        /// One component's codec bytes for an entity, resolved by registered
        /// type name. `Ok(None)` when the entity does not carry it.
        pub fn get_component_by_name(
            &self,
            entity: Entity,
            name: &str,
        ) -> Result<Option<Vec<u8>>, SnapshotError> {
            let encode = self.value_codec(name)?.encode_value;
            encode(self, entity).transpose()
        }

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
                    columns.push((codec.encode_column)(&column.data)?);
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
                    let decoded_rows = (info.column_len)(&column.data);
                    if decoded_rows != table_snapshot.entities.len() {
                        return Err(SnapshotError::Codec(format!(
                            "column {} decoded {decoded_rows} rows for {} entities",
                            info.type_name,
                            table_snapshot.entities.len()
                        )));
                    }
                    column.changed = vec![snapshot.current_tick; table_snapshot.entities.len()];
                    column.peak_changed = snapshot.current_tick;
                    column.added = vec![snapshot.current_tick; table_snapshot.entities.len()];
                    column.peak_added = snapshot.current_tick;
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

    /// Where a delta stream stands: the structural sequence and change tick
    /// a capture starts from. Take the first cursor right after the full
    /// snapshot that seeds a replica, then chain each delta's `to` cursor
    /// into the next capture.
    #[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize)]
    pub struct DeltaCursor {
        pub sequence: u64,
        pub tick: u32,
    }

    /// A serialized change-set for one world since a [`DeltaCursor`]:
    /// the structural entries in order, then one codec payload per changed
    /// component value, reflecting end-of-window state. Apply with
    /// [`DynWorld::apply_delta`] to a replica seeded from a snapshot of the
    /// same lineage; deltas must apply in unbroken cursor order, the same
    /// trust boundary snapshots carry.
    #[derive(serde::Serialize, serde::Deserialize)]
    pub struct DynWorldDelta {
        pub since: DeltaCursor,
        pub to: DeltaCursor,
        pub structural: Vec<StructuralChange>,
        pub values: Vec<(Entity, u32, Vec<u8>)>,
    }

    /// The group form: the group's own structural window (handle lifecycle
    /// and group tags) plus one [`DynWorldDelta`] per member.
    #[derive(serde::Serialize, serde::Deserialize)]
    pub struct DynEcsDelta {
        pub group_since: u64,
        pub group_to: u64,
        pub group_structural: Vec<StructuralChange>,
        pub tag_type_names: Vec<Option<String>>,
        pub worlds: Vec<DynWorldDelta>,
    }

    /// The group cursor: the group sequence plus one world cursor per
    /// member.
    #[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
    pub struct DynEcsDeltaCursor {
        pub group_sequence: u64,
        pub worlds: Vec<DeltaCursor>,
    }

    fn structural_window(
        log: &[StructuralChange],
        latest_sequence: u64,
        since_sequence: u64,
    ) -> Result<Vec<StructuralChange>, SnapshotError> {
        let start = log.partition_point(|change| change.sequence <= since_sequence);
        let window = &log[start..];
        match window.first() {
            Some(first) => {
                if first.sequence != since_sequence + 1 {
                    return Err(SnapshotError::Codec(format!(
                        "structural log gap: delta cursor at {since_sequence}, oldest retained \
                         entry is {}; reseed the replica from a full snapshot",
                        first.sequence
                    )));
                }
            }
            None => {
                if latest_sequence > since_sequence {
                    return Err(SnapshotError::Codec(format!(
                        "structural log gap: delta cursor at {since_sequence}, log trimmed \
                         through {latest_sequence}; reseed the replica from a full snapshot",
                    )));
                }
            }
        }
        Ok(window.to_vec())
    }

    impl DynWorld {
        /// The cursor a delta stream starts from, taken right after the
        /// full snapshot that seeds the replica. Fences the change window
        /// with [`increment_tick`](Self::increment_tick), so writes made
        /// after this call land in the first delta; without the fence,
        /// same-tick writes would silently miss it.
        pub fn delta_cursor(&mut self) -> DeltaCursor {
            let cursor = DeltaCursor {
                sequence: self.structural_sequence,
                tick: self.current_tick,
            };
            self.increment_tick();
            cursor
        }

        /// Captures everything that changed since the cursor as a
        /// serialized change-set, then fences the change window with
        /// [`increment_tick`](Self::increment_tick) so later writes land in
        /// the next delta. Fails with a gap error when the structural log
        /// was trimmed or overflowed past the cursor (reseed from a full
        /// snapshot), and with [`SnapshotError::MissingCodec`] when a
        /// component without a codec changed inside the window.
        pub fn delta_since(
            &mut self,
            cursor: &DeltaCursor,
        ) -> Result<DynWorldDelta, SnapshotError> {
            let structural = structural_window(
                &self.structural_log,
                self.structural_sequence,
                cursor.sequence,
            )?;

            let mut values = Vec::new();
            for (component_index, info) in self.registry.components.iter().enumerate() {
                let mut changed = self
                    .query_entities_changed_since(info.mask, cursor.tick)
                    .peekable();
                match &self.registry.codecs[component_index] {
                    Some(codec) => {
                        for entity in changed {
                            if let Some(bytes) = (codec.encode_value)(self, entity) {
                                values.push((entity, component_index as u32, bytes?));
                            }
                        }
                    }
                    None => {
                        if changed.peek().is_some() {
                            return Err(SnapshotError::MissingCodec(info.type_name));
                        }
                    }
                }
            }

            let to = DeltaCursor {
                sequence: self.structural_sequence,
                tick: self.current_tick,
            };
            self.increment_tick();
            Ok(DynWorldDelta {
                since: *cursor,
                to,
                structural,
                values,
            })
        }

        /// Replays a delta onto a replica: structural entries in order
        /// (spawns revive the exact handle, despawns retire it, component
        /// and tag changes reapply), then the changed component values
        /// through their codecs. The replica must be seeded from a snapshot
        /// of the same lineage and receive every delta in cursor order;
        /// like snapshots, delta payloads are a trust boundary.
        pub fn apply_delta(&mut self, delta: &DynWorldDelta) -> Result<(), SnapshotError> {
            for change in &delta.structural {
                match change.kind {
                    StructuralChangeKind::Spawned => {
                        self.allocator.revive(change.entity);
                        if change.mask != 0 {
                            self.insert_row(change.entity, change.mask);
                        }
                    }
                    StructuralChangeKind::Despawned => {
                        self.despawn_entities(&[change.entity]);
                    }
                    StructuralChangeKind::ComponentsAdded => {
                        self.add_components(change.entity, change.mask);
                    }
                    StructuralChangeKind::ComponentsRemoved => {
                        self.remove_components(change.entity, change.mask);
                    }
                    StructuralChangeKind::TagsAdded => {
                        let tag_index = change.mask.leading_zeros();
                        while self.tags.len() <= tag_index as usize {
                            self.tags.push(SparseTagSet::default());
                        }
                        let key = self.registry.tag_key_for(tag_index);
                        self.add_tag(key, change.entity);
                    }
                    StructuralChangeKind::TagsRemoved => {
                        let tag_index = change.mask.leading_zeros();
                        if (tag_index as usize) < self.tags.len() {
                            let key = self.registry.tag_key_for(tag_index);
                            self.remove_tag(key, change.entity);
                        }
                    }
                }
            }

            for (entity, component_index, bytes) in &delta.values {
                let info = self
                    .registry
                    .components
                    .get(*component_index as usize)
                    .ok_or_else(|| {
                        SnapshotError::UnknownComponent(format!(
                            "component index {component_index}"
                        ))
                    })?;
                let codec = self.registry.codecs[*component_index as usize]
                    .as_ref()
                    .ok_or(SnapshotError::MissingCodec(info.type_name))?;
                (codec.apply_value)(self, *entity, bytes)?;
            }
            Ok(())
        }
    }

    impl DynEcs {
        /// The group cursor a delta stream starts from, fencing every
        /// member's change window like [`DynWorld::delta_cursor`].
        pub fn delta_cursor(&mut self) -> DynEcsDeltaCursor {
            DynEcsDeltaCursor {
                group_sequence: self.structural_sequence,
                worlds: self.worlds.iter_mut().map(DynWorld::delta_cursor).collect(),
            }
        }

        /// [`DynWorld::delta_since`] across the whole group: the group's
        /// structural window (handle lifecycle and group tags) plus one
        /// world delta per member, each fenced.
        pub fn delta_since(
            &mut self,
            cursor: &DynEcsDeltaCursor,
        ) -> Result<DynEcsDelta, SnapshotError> {
            if cursor.worlds.len() != self.worlds.len() {
                return Err(SnapshotError::SchemaMismatch {
                    expected: format!("{} member cursors", self.worlds.len()),
                    found: format!("{} member cursors", cursor.worlds.len()),
                });
            }
            let group_structural = structural_window(
                &self.structural_log,
                self.structural_sequence,
                cursor.group_sequence,
            )?;
            let mut worlds = Vec::with_capacity(self.worlds.len());
            for (world, world_cursor) in self.worlds.iter_mut().zip(&cursor.worlds) {
                worlds.push(world.delta_since(world_cursor)?);
            }
            Ok(DynEcsDelta {
                group_since: cursor.group_sequence,
                group_to: self.structural_sequence,
                group_structural,
                tag_type_names: self.tag_type_names.clone(),
                worlds,
            })
        }

        /// Replays a group delta: group handle lifecycle and group tags in
        /// order, then each member's delta.
        pub fn apply_delta(&mut self, delta: &DynEcsDelta) -> Result<(), SnapshotError> {
            if delta.worlds.len() != self.worlds.len() {
                return Err(SnapshotError::SchemaMismatch {
                    expected: format!("{} member deltas", self.worlds.len()),
                    found: format!("{} member deltas", delta.worlds.len()),
                });
            }
            for (index, name) in delta.tag_type_names.iter().enumerate() {
                while self.tag_type_names.len() <= index {
                    self.tags.push(SparseTagSet::default());
                    self.tag_type_names.push(None);
                }
                if name.is_some() && self.tag_type_names[index].is_none() {
                    self.tag_type_names[index] = name.clone();
                }
            }
            for change in &delta.group_structural {
                match change.kind {
                    StructuralChangeKind::Spawned => {
                        self.allocator.revive(change.entity);
                    }
                    StructuralChangeKind::Despawned => {
                        self.despawn(change.entity);
                    }
                    StructuralChangeKind::TagsAdded => {
                        let tag_index = change.mask as usize;
                        while self.tags.len() <= tag_index {
                            self.tags.push(SparseTagSet::default());
                            self.tag_type_names.push(None);
                        }
                        self.tags[tag_index].insert(change.entity);
                    }
                    StructuralChangeKind::TagsRemoved => {
                        if (change.mask as usize) < self.tags.len() {
                            self.tags[change.mask as usize].remove(change.entity);
                        }
                    }
                    StructuralChangeKind::ComponentsAdded
                    | StructuralChangeKind::ComponentsRemoved => {}
                }
            }
            for (world, world_delta) in self.worlds.iter_mut().zip(&delta.worlds) {
                world.apply_delta(world_delta)?;
            }
            Ok(())
        }
    }

    impl DynEcs {
        /// [`DynWorld::set_component_by_name`] routed to the member world
        /// whose registry carries the name.
        pub fn set_component_by_name(
            &mut self,
            entity: Entity,
            name: &str,
            bytes: &[u8],
        ) -> Result<(), SnapshotError> {
            if !self.is_alive(entity) {
                return Err(SnapshotError::DeadEntity);
            }
            let index = self
                .worlds
                .iter()
                .position(|world| world.registry.component_by_name(name).is_some())
                .ok_or_else(|| SnapshotError::UnknownComponent(name.to_string()))?;
            self.worlds[index].set_component_by_name(entity, name, bytes)
        }

        /// [`DynWorld::get_component_by_name`] routed to the member world
        /// whose registry carries the name.
        pub fn get_component_by_name(
            &self,
            entity: Entity,
            name: &str,
        ) -> Result<Option<Vec<u8>>, SnapshotError> {
            let index = self
                .worlds
                .iter()
                .position(|world| world.registry.component_by_name(name).is_some())
                .ok_or_else(|| SnapshotError::UnknownComponent(name.to_string()))?;
            self.worlds[index].get_component_by_name(entity, name)
        }

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
                tag_type_names: self.tag_type_names.clone(),
            })
        }

        /// Rebuilds a group from a snapshot and one registry per member
        /// world, in the same order the worlds were added. Snapshots do not
        /// carry structural logs, so the restored group and its worlds start
        /// with empty logs; treat a load as a full-sync boundary and rely on
        /// the restored slots being stamped changed.
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
                if let Some(name) = snapshot.tag_type_names.get(tag_index) {
                    ecs.tag_type_names[tag_index] = name.clone();
                }
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
    ComponentCodec, DeltaCursor, DynEcsDelta, DynEcsDeltaCursor, DynEcsSnapshot, DynTableSnapshot,
    DynWorldDelta, DynWorldSnapshot, EncodeValueFn, SnapshotError,
};

#[cfg(feature = "snapshot")]
use snapshot::{
    apply_value_postcard, decode_column_postcard, encode_column_postcard, encode_value_postcard,
};

mod sealed {
    pub trait SealedElement {}
    pub trait SealedQueryTuple {}
    pub trait SealedBundle {}
    pub trait SealedResourceBundle {}
}

/// One element of a typed query tuple: `&T`, `&mut T`, `Option<&T>`, or
/// `Option<&mut T>`. Optional elements do not constrain which entities the
/// query visits; they yield `None` on entities missing the component.
pub trait QueryElement: sealed::SealedElement {
    type Fetch<'table>;
    type Item<'item>;
    const REQUIRED: bool;
    const MUTABLE: bool;
    fn component_mask(world: &mut DynWorld) -> u64;
    fn route_registered(world: &DynWorld) -> bool;
    fn foreign_item<'world>(world: &'world DynWorld, entity: Entity) -> Option<Self::Item<'world>>;
    fn fetch<'table>(
        slot: Option<&'table mut ColumnSlot>,
        current_tick: u32,
    ) -> Self::Fetch<'table>;
    fn changed_newer(fetch: &Self::Fetch<'_>, index: usize, since_tick: u32) -> bool;
    fn item<'fetch>(fetch: &'fetch mut Self::Fetch<'_>, index: usize) -> Self::Item<'fetch>;
    fn stamp_peaks(fetch: &mut Self::Fetch<'_>);
    type ParFetch<'table>: Send;
    fn par_fetch<'table>(
        slot: Option<&'table mut ColumnSlot>,
        current_tick: u32,
    ) -> Self::ParFetch<'table>;
    fn par_split<'table>(
        fetch: Self::ParFetch<'table>,
        mid: usize,
    ) -> (Self::ParFetch<'table>, Self::ParFetch<'table>);
    fn par_item<'fetch>(fetch: &'fetch mut Self::ParFetch<'_>, index: usize) -> Self::Item<'fetch>;
    fn mark_changed_all(fetch: &mut Self::Fetch<'_>);
    fn item_marked<'fetch>(fetch: &'fetch mut Self::Fetch<'_>, index: usize) -> Self::Item<'fetch>;
    fn slice_iter<'fetch>(
        fetch: Self::ParFetch<'fetch>,
    ) -> impl Iterator<Item = Self::Item<'fetch>> + 'fetch;
    /// Row access without bounds checks, the `raw_storage` fast-path
    /// counterpart of [`par_item`](Self::par_item). Change stamping is
    /// already disabled under `raw_storage`, so this only reads or hands out
    /// the element at `index`.
    ///
    /// # Safety
    /// `index` must be less than the fetched column's length.
    #[cfg(feature = "raw_storage")]
    unsafe fn par_item_unchecked<'fetch>(
        fetch: &'fetch mut Self::ParFetch<'_>,
        index: usize,
    ) -> Self::Item<'fetch>;
}

impl<T: Send + Sync + Default + 'static> sealed::SealedElement for &T {}

impl<T: Send + Sync + Default + 'static> QueryElement for &T {
    type Fetch<'table> = (&'table [T], &'table [u32]);
    type Item<'item> = &'item T;
    const REQUIRED: bool = true;
    const MUTABLE: bool = false;

    fn component_mask(world: &mut DynWorld) -> u64 {
        world.component_key::<T>().mask
    }

    fn route_registered(world: &DynWorld) -> bool {
        world.lookup_key::<T>().is_some()
    }

    fn foreign_item<'world>(world: &'world DynWorld, entity: Entity) -> Option<Self::Item<'world>> {
        world.get::<T>(entity)
    }

    fn fetch<'table>(
        slot: Option<&'table mut ColumnSlot>,
        _current_tick: u32,
    ) -> Self::Fetch<'table> {
        let slot = slot.expect("required query element column missing");
        (column_vec::<T>(&slot.data), slot.changed.as_slice())
    }

    fn changed_newer(fetch: &Self::Fetch<'_>, index: usize, since_tick: u32) -> bool {
        fetch
            .1
            .get(index)
            .is_some_and(|&value| tick_is_newer(value, since_tick))
    }

    fn item<'fetch>(fetch: &'fetch mut Self::Fetch<'_>, index: usize) -> Self::Item<'fetch> {
        &fetch.0[index]
    }

    fn stamp_peaks(_fetch: &mut Self::Fetch<'_>) {}

    type ParFetch<'table> = (&'table [T], &'table [u32]);

    fn par_fetch<'table>(
        slot: Option<&'table mut ColumnSlot>,
        _current_tick: u32,
    ) -> Self::ParFetch<'table> {
        let slot = slot.expect("required query element column missing");
        (column_vec::<T>(&slot.data), slot.changed.as_slice())
    }

    fn par_split<'table>(
        fetch: Self::ParFetch<'table>,
        mid: usize,
    ) -> (Self::ParFetch<'table>, Self::ParFetch<'table>) {
        let (left_data, right_data) = fetch.0.split_at(mid);
        let (left_changed, right_changed) = fetch.1.split_at(mid.min(fetch.1.len()));
        ((left_data, left_changed), (right_data, right_changed))
    }

    fn par_item<'fetch>(fetch: &'fetch mut Self::ParFetch<'_>, index: usize) -> Self::Item<'fetch> {
        &fetch.0[index]
    }

    fn mark_changed_all(_fetch: &mut Self::Fetch<'_>) {}

    fn item_marked<'fetch>(fetch: &'fetch mut Self::Fetch<'_>, index: usize) -> Self::Item<'fetch> {
        &fetch.0[index]
    }

    fn slice_iter<'fetch>(
        fetch: Self::ParFetch<'fetch>,
    ) -> impl Iterator<Item = Self::Item<'fetch>> + 'fetch {
        IntoIterator::into_iter(fetch.0)
    }

    #[cfg(feature = "raw_storage")]
    unsafe fn par_item_unchecked<'fetch>(
        fetch: &'fetch mut Self::ParFetch<'_>,
        index: usize,
    ) -> Self::Item<'fetch> {
        unsafe { fetch.0.get_unchecked(index) }
    }
}

impl<T: Send + Sync + Default + 'static> sealed::SealedElement for &mut T {}

impl<T: Send + Sync + Default + 'static> QueryElement for &mut T {
    type Fetch<'table> = (&'table mut [T], &'table mut [u32], u32, &'table mut u32);
    type Item<'item> = &'item mut T;
    const REQUIRED: bool = true;
    const MUTABLE: bool = true;

    fn component_mask(world: &mut DynWorld) -> u64 {
        world.component_key::<T>().mask
    }

    fn route_registered(world: &DynWorld) -> bool {
        world.lookup_key::<T>().is_some()
    }

    fn foreign_item<'world>(
        _world: &'world DynWorld,
        _entity: Entity,
    ) -> Option<Self::Item<'world>> {
        unreachable!("cross-world queries mutate only driver-world components")
    }

    fn fetch<'table>(
        slot: Option<&'table mut ColumnSlot>,
        current_tick: u32,
    ) -> Self::Fetch<'table> {
        let slot = slot.expect("required query element column missing");
        let ColumnSlot {
            data,
            changed,
            peak_changed,
            ..
        } = slot;
        (
            column_vec_mut::<T>(data),
            changed.as_mut_slice(),
            current_tick,
            peak_changed,
        )
    }

    fn changed_newer(fetch: &Self::Fetch<'_>, index: usize, since_tick: u32) -> bool {
        fetch
            .1
            .get(index)
            .is_some_and(|&value| tick_is_newer(value, since_tick))
    }

    fn item<'fetch>(fetch: &'fetch mut Self::Fetch<'_>, index: usize) -> Self::Item<'fetch> {
        if let Some(cell) = fetch.1.get_mut(index) {
            *cell = fetch.2;
        }
        &mut fetch.0[index]
    }

    fn stamp_peaks(fetch: &mut Self::Fetch<'_>) {
        *fetch.3 = fetch.2;
    }

    type ParFetch<'table> = (&'table mut [T], &'table mut [u32], u32);

    fn par_fetch<'table>(
        slot: Option<&'table mut ColumnSlot>,
        current_tick: u32,
    ) -> Self::ParFetch<'table> {
        let slot = slot.expect("required query element column missing");
        #[cfg(not(feature = "raw_storage"))]
        {
            slot.peak_changed = current_tick;
        }
        let ColumnSlot { data, changed, .. } = slot;
        (
            column_vec_mut::<T>(data),
            changed.as_mut_slice(),
            current_tick,
        )
    }

    fn par_split<'table>(
        fetch: Self::ParFetch<'table>,
        mid: usize,
    ) -> (Self::ParFetch<'table>, Self::ParFetch<'table>) {
        let (left_data, right_data) = fetch.0.split_at_mut(mid);
        let (left_changed, right_changed) = fetch.1.split_at_mut(mid.min(fetch.1.len()));
        (
            (left_data, left_changed, fetch.2),
            (right_data, right_changed, fetch.2),
        )
    }

    fn par_item<'fetch>(fetch: &'fetch mut Self::ParFetch<'_>, index: usize) -> Self::Item<'fetch> {
        if let Some(cell) = fetch.1.get_mut(index) {
            *cell = fetch.2;
        }
        &mut fetch.0[index]
    }

    fn mark_changed_all(fetch: &mut Self::Fetch<'_>) {
        fetch.1.fill(fetch.2);
    }

    fn item_marked<'fetch>(fetch: &'fetch mut Self::Fetch<'_>, index: usize) -> Self::Item<'fetch> {
        &mut fetch.0[index]
    }

    fn slice_iter<'fetch>(
        fetch: Self::ParFetch<'fetch>,
    ) -> impl Iterator<Item = Self::Item<'fetch>> + 'fetch {
        let (data, changed, tick) = fetch;
        changed.fill(tick);
        IntoIterator::into_iter(data)
    }

    #[cfg(feature = "raw_storage")]
    unsafe fn par_item_unchecked<'fetch>(
        fetch: &'fetch mut Self::ParFetch<'_>,
        index: usize,
    ) -> Self::Item<'fetch> {
        unsafe { fetch.0.get_unchecked_mut(index) }
    }
}

impl<T: Send + Sync + Default + 'static> sealed::SealedElement for Option<&T> {}

impl<T: Send + Sync + Default + 'static> QueryElement for Option<&T> {
    type Fetch<'table> = Option<(&'table [T], &'table [u32])>;
    type Item<'item> = Option<&'item T>;
    const REQUIRED: bool = false;
    const MUTABLE: bool = false;

    fn component_mask(world: &mut DynWorld) -> u64 {
        world.component_key::<T>().mask
    }

    fn route_registered(world: &DynWorld) -> bool {
        world.lookup_key::<T>().is_some()
    }

    fn foreign_item<'world>(world: &'world DynWorld, entity: Entity) -> Option<Self::Item<'world>> {
        Some(world.get::<T>(entity))
    }

    fn fetch<'table>(
        slot: Option<&'table mut ColumnSlot>,
        current_tick: u32,
    ) -> Self::Fetch<'table> {
        slot.map(|slot| <&T as QueryElement>::fetch(Some(slot), current_tick))
    }

    fn changed_newer(fetch: &Self::Fetch<'_>, index: usize, since_tick: u32) -> bool {
        fetch.as_ref().is_some_and(|fetch| {
            fetch
                .1
                .get(index)
                .is_some_and(|&value| tick_is_newer(value, since_tick))
        })
    }

    fn item<'fetch>(fetch: &'fetch mut Self::Fetch<'_>, index: usize) -> Self::Item<'fetch> {
        fetch.as_ref().map(|fetch| &fetch.0[index])
    }

    fn stamp_peaks(_fetch: &mut Self::Fetch<'_>) {}

    type ParFetch<'table> = Option<(&'table [T], &'table [u32])>;

    fn par_fetch<'table>(
        slot: Option<&'table mut ColumnSlot>,
        current_tick: u32,
    ) -> Self::ParFetch<'table> {
        slot.map(|slot| <&T as QueryElement>::par_fetch(Some(slot), current_tick))
    }

    fn par_split<'table>(
        fetch: Self::ParFetch<'table>,
        mid: usize,
    ) -> (Self::ParFetch<'table>, Self::ParFetch<'table>) {
        match fetch {
            Some(inner) => {
                let (left, right) = <&T as QueryElement>::par_split(inner, mid);
                (Some(left), Some(right))
            }
            None => (None, None),
        }
    }

    fn par_item<'fetch>(fetch: &'fetch mut Self::ParFetch<'_>, index: usize) -> Self::Item<'fetch> {
        fetch.as_ref().map(|inner| &inner.0[index])
    }

    fn mark_changed_all(_fetch: &mut Self::Fetch<'_>) {}

    fn item_marked<'fetch>(fetch: &'fetch mut Self::Fetch<'_>, index: usize) -> Self::Item<'fetch> {
        fetch.as_ref().map(|inner| &inner.0[index])
    }

    fn slice_iter<'fetch>(
        _fetch: Self::ParFetch<'fetch>,
    ) -> impl Iterator<Item = Self::Item<'fetch>> + 'fetch {
        std::iter::from_fn(|| {
            unreachable!("optional query elements never use the all-required fast path")
        })
    }

    #[cfg(feature = "raw_storage")]
    unsafe fn par_item_unchecked<'fetch>(
        fetch: &'fetch mut Self::ParFetch<'_>,
        index: usize,
    ) -> Self::Item<'fetch> {
        fetch
            .as_ref()
            .map(|inner| unsafe { inner.0.get_unchecked(index) })
    }
}

impl<T: Send + Sync + Default + 'static> sealed::SealedElement for Option<&mut T> {}

impl<T: Send + Sync + Default + 'static> QueryElement for Option<&mut T> {
    type Fetch<'table> = Option<(&'table mut [T], &'table mut [u32], u32, &'table mut u32)>;
    type Item<'item> = Option<&'item mut T>;
    const REQUIRED: bool = false;
    const MUTABLE: bool = true;

    fn component_mask(world: &mut DynWorld) -> u64 {
        world.component_key::<T>().mask
    }

    fn route_registered(world: &DynWorld) -> bool {
        world.lookup_key::<T>().is_some()
    }

    fn foreign_item<'world>(
        _world: &'world DynWorld,
        _entity: Entity,
    ) -> Option<Self::Item<'world>> {
        unreachable!("cross-world queries mutate only driver-world components")
    }

    fn fetch<'table>(
        slot: Option<&'table mut ColumnSlot>,
        current_tick: u32,
    ) -> Self::Fetch<'table> {
        slot.map(|slot| <&mut T as QueryElement>::fetch(Some(slot), current_tick))
    }

    fn changed_newer(fetch: &Self::Fetch<'_>, index: usize, since_tick: u32) -> bool {
        fetch.as_ref().is_some_and(|fetch| {
            fetch
                .1
                .get(index)
                .is_some_and(|&value| tick_is_newer(value, since_tick))
        })
    }

    fn item<'fetch>(fetch: &'fetch mut Self::Fetch<'_>, index: usize) -> Self::Item<'fetch> {
        fetch.as_mut().map(|fetch| {
            if let Some(cell) = fetch.1.get_mut(index) {
                *cell = fetch.2;
            }
            &mut fetch.0[index]
        })
    }

    fn stamp_peaks(fetch: &mut Self::Fetch<'_>) {
        if let Some(fetch) = fetch {
            *fetch.3 = fetch.2;
        }
    }

    type ParFetch<'table> = Option<(&'table mut [T], &'table mut [u32], u32)>;

    fn par_fetch<'table>(
        slot: Option<&'table mut ColumnSlot>,
        current_tick: u32,
    ) -> Self::ParFetch<'table> {
        slot.map(|slot| <&mut T as QueryElement>::par_fetch(Some(slot), current_tick))
    }

    fn par_split<'table>(
        fetch: Self::ParFetch<'table>,
        mid: usize,
    ) -> (Self::ParFetch<'table>, Self::ParFetch<'table>) {
        match fetch {
            Some(inner) => {
                let (left, right) = <&mut T as QueryElement>::par_split(inner, mid);
                (Some(left), Some(right))
            }
            None => (None, None),
        }
    }

    fn par_item<'fetch>(fetch: &'fetch mut Self::ParFetch<'_>, index: usize) -> Self::Item<'fetch> {
        fetch.as_mut().map(|inner| {
            if let Some(cell) = inner.1.get_mut(index) {
                *cell = inner.2;
            }
            &mut inner.0[index]
        })
    }

    fn mark_changed_all(fetch: &mut Self::Fetch<'_>) {
        if let Some(inner) = fetch {
            inner.1.fill(inner.2);
        }
    }

    fn item_marked<'fetch>(fetch: &'fetch mut Self::Fetch<'_>, index: usize) -> Self::Item<'fetch> {
        fetch.as_mut().map(|inner| &mut inner.0[index])
    }

    fn slice_iter<'fetch>(
        _fetch: Self::ParFetch<'fetch>,
    ) -> impl Iterator<Item = Self::Item<'fetch>> + 'fetch {
        std::iter::from_fn(|| {
            unreachable!("optional query elements never use the all-required fast path")
        })
    }

    #[cfg(feature = "raw_storage")]
    unsafe fn par_item_unchecked<'fetch>(
        fetch: &'fetch mut Self::ParFetch<'_>,
        index: usize,
    ) -> Self::Item<'fetch> {
        fetch
            .as_mut()
            .map(|inner| unsafe { inner.0.get_unchecked_mut(index) })
    }
}

/// The read-only half of [`QueryElement`], `&T` or `Option<&T>` only. Shared
/// fetches are `Copy` and items borrow the world rather than the fetch, which
/// is what lets [`DynQueryRef::iter`] hand out a real `Iterator`.
pub trait ReadQueryElement: QueryElement {
    type ReadFetch<'table>: Copy;
    fn lookup_mask(world: &DynWorld) -> Option<u64>;
    fn read_fetch<'table>(slot: Option<&'table ColumnSlot>) -> Self::ReadFetch<'table>;
    fn placeholder_read_fetch<'table>() -> Self::ReadFetch<'table>;
    fn read_changed_newer(fetch: Self::ReadFetch<'_>, index: usize, since_tick: u32) -> bool;
    fn read_added_newer(fetch: Self::ReadFetch<'_>, index: usize, since_tick: u32) -> bool;
    fn read_item<'table>(fetch: Self::ReadFetch<'table>, index: usize) -> Self::Item<'table>;
}

impl<T: Send + Sync + Default + 'static> ReadQueryElement for &T {
    type ReadFetch<'table> = (&'table [T], &'table [u32], &'table [u32]);

    fn lookup_mask(world: &DynWorld) -> Option<u64> {
        world.lookup_key::<T>().map(|key| key.mask)
    }

    fn read_fetch<'table>(slot: Option<&'table ColumnSlot>) -> Self::ReadFetch<'table> {
        let slot = slot.expect("required query element column missing");
        (
            column_vec::<T>(&slot.data),
            slot.changed.as_slice(),
            slot.added.as_slice(),
        )
    }

    fn placeholder_read_fetch<'table>() -> Self::ReadFetch<'table> {
        (&[], &[], &[])
    }

    fn read_changed_newer(fetch: Self::ReadFetch<'_>, index: usize, since_tick: u32) -> bool {
        fetch
            .1
            .get(index)
            .is_some_and(|&value| tick_is_newer(value, since_tick))
    }

    fn read_added_newer(fetch: Self::ReadFetch<'_>, index: usize, since_tick: u32) -> bool {
        fetch
            .2
            .get(index)
            .is_some_and(|&value| tick_is_newer(value, since_tick))
    }

    fn read_item<'table>(fetch: Self::ReadFetch<'table>, index: usize) -> Self::Item<'table> {
        &fetch.0[index]
    }
}

impl<T: Send + Sync + Default + 'static> ReadQueryElement for Option<&T> {
    type ReadFetch<'table> = Option<(&'table [T], &'table [u32], &'table [u32])>;

    fn lookup_mask(world: &DynWorld) -> Option<u64> {
        world.lookup_key::<T>().map(|key| key.mask)
    }

    fn read_fetch<'table>(slot: Option<&'table ColumnSlot>) -> Self::ReadFetch<'table> {
        slot.map(|slot| <&T as ReadQueryElement>::read_fetch(Some(slot)))
    }

    fn placeholder_read_fetch<'table>() -> Self::ReadFetch<'table> {
        None
    }

    fn read_changed_newer(fetch: Self::ReadFetch<'_>, index: usize, since_tick: u32) -> bool {
        fetch.is_some_and(|fetch| {
            fetch
                .1
                .get(index)
                .is_some_and(|&value| tick_is_newer(value, since_tick))
        })
    }

    fn read_added_newer(fetch: Self::ReadFetch<'_>, index: usize, since_tick: u32) -> bool {
        fetch.is_some_and(|fetch| {
            fetch
                .2
                .get(index)
                .is_some_and(|&value| tick_is_newer(value, since_tick))
        })
    }

    fn read_item<'table>(fetch: Self::ReadFetch<'table>, index: usize) -> Self::Item<'table> {
        fetch.map(|fetch| &fetch.0[index])
    }
}

/// A tuple of query elements. Implemented for tuples of `&T`, `&mut T`,
/// `Option<&T>`, and `Option<&mut T>` up to eight elements; all component
/// types in one tuple must be distinct. Only the non-optional elements
/// constrain which entities the query visits.
pub(crate) fn route_world_scan(
    worlds: &[DynWorld],
    registered: impl Fn(&DynWorld) -> bool,
    type_name: &str,
) -> Option<usize> {
    let mut found: Option<usize> = None;
    for (index, world) in worlds.iter().enumerate() {
        if registered(world) {
            match found {
                None => found = Some(index),
                Some(first) => panic!(
                    "{type_name} is registered in member worlds {first} and {index}; \
                     a component type must live in exactly one member world"
                ),
            }
        }
    }
    found
}

/// The row filters one cross-world join carries: group tag sets to
/// include and exclude, and driver-world changed/added mask lookups. The
/// lookups resolve inside the join, after the tuple's driver-local
/// elements have registered, so a lazily-registered tuple element is a
/// valid filter target.
pub struct JoinFilters<'sets> {
    pub include_sets: [Option<&'sets SparseTagSet>; 4],
    pub exclude_sets: [Option<&'sets SparseTagSet>; 4],
    pub changed_lookups: [Option<JoinMaskLookup>; 4],
    pub added_lookups: [Option<JoinMaskLookup>; 4],
}

/// One query-tuple element's routing facts for a cross-world join: which
/// member world holds its type, whether the element narrows the match, and
/// whether it writes (writers must share one world, the driver).
#[derive(Clone, Copy)]
pub struct JoinRoute {
    pub world: Option<usize>,
    pub required: bool,
    pub mutable: bool,
    pub type_name: &'static str,
}

pub trait QueryTuple: sealed::SealedQueryTuple {
    type Fetch<'table>;
    type Item<'item>;
    fn component_mask(world: &mut DynWorld) -> u64;
    fn element_masks(world: &mut DynWorld) -> [u64; 8];
    fn routing_match(world: &DynWorld) -> bool;
    fn join_routes(worlds: &[DynWorld]) -> [Option<JoinRoute>; 8];
    fn join_for_each<F: for<'item> FnMut(Entity, Self::Item<'item>)>(
        driver: &mut DynWorld,
        element_worlds: &[Option<&DynWorld>; 8],
        filters: &JoinFilters<'_>,
        f: F,
    );
    #[cfg(not(target_family = "wasm"))]
    fn join_par_for_each<F: for<'item> Fn(Entity, Self::Item<'item>) + Send + Sync>(
        driver: &mut DynWorld,
        element_worlds: &[Option<&DynWorld>; 8],
        filters: &JoinFilters<'_>,
        f: F,
    );
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
    type ParFetch<'table>: Send;
    fn par_fetch<'table>(
        table_mask: u64,
        columns: &'table mut [ColumnSlot],
        element_masks: &[u64; 8],
        current_tick: u32,
    ) -> Self::ParFetch<'table>;
    fn par_split<'table>(
        fetch: Self::ParFetch<'table>,
        mid: usize,
    ) -> (Self::ParFetch<'table>, Self::ParFetch<'table>);
    fn par_item<'fetch>(fetch: &'fetch mut Self::ParFetch<'_>, index: usize) -> Self::Item<'fetch>;
    /// Row access without bounds checks, used by the `raw_storage` fast path.
    ///
    /// # Safety
    /// `index` must be less than every fetched column's length.
    #[cfg(feature = "raw_storage")]
    unsafe fn par_item_unchecked<'fetch>(
        fetch: &'fetch mut Self::ParFetch<'_>,
        index: usize,
    ) -> Self::Item<'fetch>;
    fn mark_changed_all(fetch: &mut Self::Fetch<'_>);
    fn item_marked<'fetch>(fetch: &'fetch mut Self::Fetch<'_>, index: usize) -> Self::Item<'fetch>;
    const ALL_REQUIRED: bool;
    fn fast_for_each<FN>(fetch: Self::ParFetch<'_>, entities: &[Entity], f: &mut FN)
    where
        FN: for<'item> FnMut(Entity, Self::Item<'item>);
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
    fn read_added_newer(
        fetch: Self::ReadFetch<'_>,
        index: usize,
        element_masks: &[u64; 8],
        added_mask: u64,
        since_tick: u32,
    ) -> bool;
    fn read_item<'table>(fetch: Self::ReadFetch<'table>, index: usize) -> Self::Item<'table>;
    fn join_lookup(
        driver: &DynWorld,
        element_worlds: &[Option<&DynWorld>; 8],
    ) -> Option<([u64; 8], u64)>;
    fn join_read_fetch<'table>(
        table_mask: u64,
        columns: &'table [ColumnSlot],
        element_masks: &[u64; 8],
        element_worlds: &[Option<&DynWorld>; 8],
    ) -> Self::ReadFetch<'table>;
    fn join_read_item<'world>(
        fetch: Self::ReadFetch<'world>,
        element_worlds: &[Option<&'world DynWorld>; 8],
        entity: Entity,
        index: usize,
    ) -> Option<Self::Item<'world>>;
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

#[cfg(not(feature = "raw_storage"))]
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

/// Resolves each element's column by direct index rather than scanning every
/// column of the table. A query tuple's components are distinct, so their
/// `column_position`s are distinct in-bounds indices, which is exactly the
/// condition for handing out disjoint mutable borrows from the raw pointer.
#[cfg(feature = "raw_storage")]
fn distribute_slots<const COUNT: usize>(
    columns: &mut [ColumnSlot],
    positions: [Option<usize>; COUNT],
) -> [Option<&mut ColumnSlot>; COUNT] {
    let base = columns.as_mut_ptr();
    let length = columns.len();
    std::array::from_fn(|element_index| {
        positions[element_index].map(|position| {
            debug_assert!(position < length, "query column position out of bounds");
            unsafe { &mut *base.add(position) }
        })
    })
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

            fn routing_match(world: &DynWorld) -> bool {
                let mut matched = true;
                $(
                    if $element::REQUIRED && !$element::route_registered(world) {
                        matched = false;
                    }
                )+
                matched
            }

            fn join_routes(worlds: &[DynWorld]) -> [Option<JoinRoute>; 8] {
                let mut routes = [None; 8];
                $(
                    routes[$position] = Some(JoinRoute {
                        world: route_world_scan(
                            worlds,
                            |world| $element::route_registered(world),
                            std::any::type_name::<$element>(),
                        ),
                        required: $element::REQUIRED,
                        mutable: $element::MUTABLE,
                        type_name: std::any::type_name::<$element>(),
                    });
                )+
                routes
            }

            #[allow(non_snake_case)]
            fn join_for_each<FN: for<'item> FnMut(Entity, Self::Item<'item>)>(
                driver: &mut DynWorld,
                element_worlds: &[Option<&DynWorld>; 8],
                filters: &JoinFilters<'_>,
                mut f: FN,
            ) {
                let mut element_masks = [0u64; 8];
                let mut local_include = 0u64;
                $(
                    if element_worlds[$position].is_none() {
                        element_masks[$position] = $element::component_mask(driver);
                        if $element::REQUIRED {
                            local_include |= element_masks[$position];
                        }
                    }
                )+
                let mut changed_mask = 0u64;
                for lookup in filters.changed_lookups.iter().flatten() {
                    changed_mask |= lookup(driver)
                        .expect("changed filters on query_join must name driver-world components");
                }
                let mut added_mask = 0u64;
                for lookup in filters.added_lookups.iter().flatten() {
                    added_mask |= lookup(driver)
                        .expect("added filters on query_join must name driver-world components");
                }
                let local_tuple_mask = element_masks
                    .iter()
                    .fold(0, |mask, element| mask | element);
                assert_eq!(
                    (changed_mask | added_mask) & !local_tuple_mask,
                    0,
                    "changed and added filters must name components present in the query tuple"
                );
                let since_tick = driver.last_tick;
                let current_tick = driver.current_tick;
                let mut added_scratch = std::mem::take(&mut driver.added_scratch);
                let table_indices = archetype_cached_tables(
                    &mut driver.query_cache,
                    driver.tables.iter().map(|table| table.mask),
                    local_include,
                );
                let tables = &mut driver.tables;
                for &table_index in table_indices {
                    let table = &mut tables[table_index];
                    let table_mask = table.mask;
                    let DynComponentArrays {
                        entity_indices,
                        columns,
                        ..
                    } = table;
                    let positions = [$(
                        if element_worlds[$position].is_none()
                            && table_mask & element_masks[$position] != 0
                        {
                            Some(column_position(table_mask, element_masks[$position]))
                        } else {
                            None
                        },
                    )+];
                    if added_mask != 0 {
                        added_scratch.clear();
                        added_scratch.resize(entity_indices.len(), false);
                        for column in columns.iter() {
                            if added_mask & (1u64 << column.component_index) == 0 {
                                continue;
                            }
                            for (row, &added_tick) in column.added.iter().enumerate() {
                                if tick_is_newer(added_tick, since_tick) {
                                    added_scratch[row] = true;
                                }
                            }
                        }
                    }
                    let [$($element,)+] = distribute_slots(columns, positions);
                    $(
                        let mut $element = if element_worlds[$position].is_none() {
                            Some(<$element as QueryElement>::fetch($element, current_tick))
                        } else {
                            None
                        };
                    )+
                    let mut visited = false;
                    'rows: for (row_index, &entity) in entity_indices.iter().enumerate() {
                        if !tag_sets_match(&filters.include_sets, &filters.exclude_sets, entity) {
                            continue 'rows;
                        }
                        if added_mask != 0 && !added_scratch[row_index] {
                            continue 'rows;
                        }
                        if changed_mask != 0 {
                            let mut newer = false;
                            $(
                                if !newer
                                    && changed_mask & element_masks[$position] != 0
                                    && let Some(fetch) = &$element
                                    && <$element as QueryElement>::changed_newer(
                                        fetch, row_index, since_tick,
                                    )
                                {
                                    newer = true;
                                }
                            )+
                            if !newer {
                                continue 'rows;
                            }
                        }
                        $crate::paste::paste! {
                            $(
                                let [<foreign_ $position>] = match element_worlds[$position] {
                                    Some(world) => {
                                        match <$element as QueryElement>::foreign_item(
                                            world, entity,
                                        ) {
                                            Some(item) => Some(item),
                                            None => continue 'rows,
                                        }
                                    }
                                    None => None,
                                };
                            )+
                            $(
                                let [<item_ $position>] = match [<foreign_ $position>] {
                                    Some(item) => item,
                                    None => <$element as QueryElement>::item(
                                        $element.as_mut().expect("local positions carry a fetch"),
                                        row_index,
                                    ),
                                };
                            )+
                            visited = true;
                            f(entity, ($([<item_ $position>],)+));
                        }
                    }
                    if visited {
                        $(
                            if let Some(fetch) = &mut $element {
                                <$element as QueryElement>::stamp_peaks(fetch);
                            }
                        )+
                    }
                }
                driver.added_scratch = added_scratch;
            }

            #[cfg(not(target_family = "wasm"))]
            #[allow(non_snake_case)]
            fn join_par_for_each<FN: for<'item> Fn(Entity, Self::Item<'item>) + Send + Sync>(
                driver: &mut DynWorld,
                element_worlds: &[Option<&DynWorld>; 8],
                filters: &JoinFilters<'_>,
                f: FN,
            ) {
                use crate::rayon::prelude::*;

                let mut element_masks = [0u64; 8];
                let mut local_include = 0u64;
                $(
                    if element_worlds[$position].is_none() {
                        element_masks[$position] = $element::component_mask(driver);
                        if $element::REQUIRED {
                            local_include |= element_masks[$position];
                        }
                    }
                )+
                let mut changed_mask = 0u64;
                for lookup in filters.changed_lookups.iter().flatten() {
                    changed_mask |= lookup(driver)
                        .expect("changed filters on query_join must name driver-world components");
                }
                let mut added_mask = 0u64;
                for lookup in filters.added_lookups.iter().flatten() {
                    added_mask |= lookup(driver)
                        .expect("added filters on query_join must name driver-world components");
                }
                let local_tuple_mask = element_masks
                    .iter()
                    .fold(0, |mask, element| mask | element);
                assert_eq!(
                    (changed_mask | added_mask) & !local_tuple_mask,
                    0,
                    "changed and added filters must name components present in the query tuple"
                );
                let since_tick = driver.last_tick;
                let current_tick = driver.current_tick;
                driver
                    .tables
                    .par_iter_mut()
                    .filter(|table| {
                        table.mask & local_include == local_include
                            && !table.entity_indices.is_empty()
                    })
                    .for_each(|table| {
                        let table_mask = table.mask;
                        let DynComponentArrays {
                            entity_indices,
                            columns,
                            ..
                        } = table;
                        let added_scratch: Vec<bool> = if added_mask != 0 {
                            let mut scratch = vec![false; entity_indices.len()];
                            for column in columns.iter() {
                                if added_mask & (1u64 << column.component_index) == 0 {
                                    continue;
                                }
                                for (row, &added_tick) in column.added.iter().enumerate() {
                                    if tick_is_newer(added_tick, since_tick) {
                                        scratch[row] = true;
                                    }
                                }
                            }
                            scratch
                        } else {
                            Vec::new()
                        };
                        let positions = [$(
                            if element_worlds[$position].is_none()
                                && table_mask & element_masks[$position] != 0
                            {
                                Some(column_position(table_mask, element_masks[$position]))
                            } else {
                                None
                            },
                        )+];
                        let [$($element,)+] = distribute_slots(columns, positions);
                        $(
                            let mut $element = if element_worlds[$position].is_none() {
                                Some(<$element as QueryElement>::fetch($element, current_tick))
                            } else {
                                None
                            };
                        )+
                        let mut visited = false;
                        'rows: for (row_index, &entity) in entity_indices.iter().enumerate() {
                            if !tag_sets_match(
                                &filters.include_sets,
                                &filters.exclude_sets,
                                entity,
                            ) {
                                continue 'rows;
                            }
                            if added_mask != 0 && !added_scratch[row_index] {
                                continue 'rows;
                            }
                            if changed_mask != 0 {
                                let mut newer = false;
                                $(
                                    if !newer
                                        && changed_mask & element_masks[$position] != 0
                                        && let Some(fetch) = &$element
                                        && <$element as QueryElement>::changed_newer(
                                            fetch, row_index, since_tick,
                                        )
                                    {
                                        newer = true;
                                    }
                                )+
                                if !newer {
                                    continue 'rows;
                                }
                            }
                            $crate::paste::paste! {
                                $(
                                    let [<foreign_ $position>] = match element_worlds[$position] {
                                        Some(world) => {
                                            match <$element as QueryElement>::foreign_item(
                                                world, entity,
                                            ) {
                                                Some(item) => Some(item),
                                                None => continue 'rows,
                                            }
                                        }
                                        None => None,
                                    };
                                )+
                                $(
                                    let [<item_ $position>] = match [<foreign_ $position>] {
                                        Some(item) => item,
                                        None => <$element as QueryElement>::item(
                                            $element
                                                .as_mut()
                                                .expect("local positions carry a fetch"),
                                            row_index,
                                        ),
                                    };
                                )+
                                visited = true;
                                f(entity, ($([<item_ $position>],)+));
                            }
                        }
                        if visited {
                            $(
                                if let Some(fetch) = &mut $element {
                                    <$element as QueryElement>::stamp_peaks(fetch);
                                }
                            )+
                        }
                    });
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

            type ParFetch<'table> = ($($element::ParFetch<'table>,)+);

            #[allow(non_snake_case)]
            fn par_fetch<'table>(
                table_mask: u64,
                columns: &'table mut [ColumnSlot],
                element_masks: &[u64; 8],
                current_tick: u32,
            ) -> Self::ParFetch<'table> {
                let positions = [$(
                    if table_mask & element_masks[$position] != 0 {
                        Some(column_position(table_mask, element_masks[$position]))
                    } else {
                        None
                    },
                )+];
                let [$($element,)+] = distribute_slots(columns, positions);
                ($($element::par_fetch($element, current_tick),)+)
            }

            fn par_split<'table>(
                fetch: Self::ParFetch<'table>,
                mid: usize,
            ) -> (Self::ParFetch<'table>, Self::ParFetch<'table>) {
                let pairs = ($($element::par_split(fetch.$position, mid),)+);
                (($(pairs.$position.0,)+), ($(pairs.$position.1,)+))
            }

            fn par_item<'fetch>(fetch: &'fetch mut Self::ParFetch<'_>, index: usize) -> Self::Item<'fetch> {
                ($($element::par_item(&mut fetch.$position, index),)+)
            }

            #[cfg(feature = "raw_storage")]
            unsafe fn par_item_unchecked<'fetch>(
                fetch: &'fetch mut Self::ParFetch<'_>,
                index: usize,
            ) -> Self::Item<'fetch> {
                ($(unsafe { $element::par_item_unchecked(&mut fetch.$position, index) },)+)
            }

            fn mark_changed_all(fetch: &mut Self::Fetch<'_>) {
                $($element::mark_changed_all(&mut fetch.$position);)+
            }

            fn item_marked<'fetch>(fetch: &'fetch mut Self::Fetch<'_>, index: usize) -> Self::Item<'fetch> {
                ($($element::item_marked(&mut fetch.$position, index),)+)
            }

            const ALL_REQUIRED: bool = true $(&& $element::REQUIRED)+;

            #[allow(non_snake_case)]
            fn fast_for_each<FN>(fetch: Self::ParFetch<'_>, entities: &[Entity], f: &mut FN)
            where
                FN: for<'item> FnMut(Entity, Self::Item<'item>),
            {
                #[cfg(feature = "raw_storage")]
                {
                    let mut fetch = fetch;
                    for index in 0..entities.len() {
                        let entity = unsafe { *entities.get_unchecked(index) };
                        f(entity, unsafe { Self::par_item_unchecked(&mut fetch, index) });
                    }
                }
                #[cfg(not(feature = "raw_storage"))]
                {
                    let mut iterators = ($($element::slice_iter(fetch.$position),)+);
                    for &entity in entities {
                        f(entity, ($(iterators.$position.next().unwrap(),)+));
                    }
                }
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

/// Splits one archetype's rows into halves down to a chunk size and runs the
/// halves on the rayon pool, so a single large archetype uses every core. The
/// `ParFetch` split hands each task a disjoint sub-slice of every column, so
/// no `unsafe` and no aliasing. Peaks are stamped eagerly by `par_fetch`; only
/// the unfiltered query path reaches here, where every row is visited.
#[cfg(not(target_family = "wasm"))]
fn par_query_rows<Q, F>(entities: &[Entity], fetch: Q::ParFetch<'_>, f: &F)
where
    Q: QueryTuple,
    F: for<'item> Fn(Entity, Q::Item<'item>) + Send + Sync,
{
    const PARALLEL_ROW_CHUNK: usize = 1024;
    let length = entities.len();
    if length <= PARALLEL_ROW_CHUNK {
        let mut fetch = fetch;
        #[cfg(feature = "raw_storage")]
        for index in 0..length {
            let entity = unsafe { *entities.get_unchecked(index) };
            f(entity, unsafe { Q::par_item_unchecked(&mut fetch, index) });
        }
        #[cfg(not(feature = "raw_storage"))]
        for (index, &entity) in entities.iter().enumerate() {
            f(entity, Q::par_item(&mut fetch, index));
        }
    } else {
        let middle = length / 2;
        let (left_fetch, right_fetch) = Q::par_split(fetch, middle);
        let (left_entities, right_entities) = entities.split_at(middle);
        crate::rayon::join(
            || par_query_rows::<Q, F>(left_entities, left_fetch, f),
            || par_query_rows::<Q, F>(right_entities, right_fetch, f),
        );
    }
}

macro_rules! impl_bare_element_query {
    ($($element:ty),+) => {
        $(
            impl<'element, T: Send + Sync + Default + 'static> sealed::SealedQueryTuple
                for $element
            {
            }

            impl<'element, T: Send + Sync + Default + 'static> QueryTuple for $element {
                type Fetch<'table> = <$element as QueryElement>::Fetch<'table>;
                type Item<'item> = <$element as QueryElement>::Item<'item>;

                fn component_mask(world: &mut DynWorld) -> u64 {
                    let elements = [(
                        <$element as QueryElement>::component_mask(world),
                        <$element as QueryElement>::REQUIRED,
                    )];
                    required_mask(&elements)
                }

                fn element_masks(world: &mut DynWorld) -> [u64; 8] {
                    let mut masks = [0u64; 8];
                    masks[0] = <$element as QueryElement>::component_mask(world);
                    masks
                }

                fn routing_match(world: &DynWorld) -> bool {
                    !<$element as QueryElement>::REQUIRED
                        || <$element as QueryElement>::route_registered(world)
                }

                fn join_routes(worlds: &[DynWorld]) -> [Option<JoinRoute>; 8] {
                    let mut routes = [None; 8];
                    routes[0] = Some(JoinRoute {
                        world: route_world_scan(
                            worlds,
                            |world| <$element as QueryElement>::route_registered(world),
                            std::any::type_name::<$element>(),
                        ),
                        required: <$element as QueryElement>::REQUIRED,
                        mutable: <$element as QueryElement>::MUTABLE,
                        type_name: std::any::type_name::<$element>(),
                    });
                    routes
                }

                fn join_for_each<FN: for<'item> FnMut(Entity, Self::Item<'item>)>(
                    driver: &mut DynWorld,
                    element_worlds: &[Option<&DynWorld>; 8],
                    filters: &JoinFilters<'_>,
                    mut f: FN,
                ) {
                    debug_assert!(
                        element_worlds[0].is_none(),
                        "a single-element tuple always drives its own world"
                    );
                    let mut element_masks = [0u64; 8];
                    element_masks[0] = <$element as QueryElement>::component_mask(driver);
                    let local_include = if <$element as QueryElement>::REQUIRED {
                        element_masks[0]
                    } else {
                        0
                    };
                    let mut changed_mask = 0u64;
                    for lookup in filters.changed_lookups.iter().flatten() {
                        changed_mask |= lookup(driver).expect(
                            "changed filters on query_join must name driver-world components",
                        );
                    }
                    let mut added_mask = 0u64;
                    for lookup in filters.added_lookups.iter().flatten() {
                        added_mask |= lookup(driver).expect(
                            "added filters on query_join must name driver-world components",
                        );
                    }
                    assert_eq!(
                        (changed_mask | added_mask) & !element_masks[0],
                        0,
                        "changed and added filters must name components present in the query tuple"
                    );
                    let since_tick = driver.last_tick;
                    let current_tick = driver.current_tick;
                    let mut added_scratch = std::mem::take(&mut driver.added_scratch);
                    let table_indices = archetype_cached_tables(
                        &mut driver.query_cache,
                        driver.tables.iter().map(|table| table.mask),
                        local_include,
                    );
                    let tables = &mut driver.tables;
                    for &table_index in table_indices {
                        let table = &mut tables[table_index];
                        let table_mask = table.mask;
                        let DynComponentArrays {
                            entity_indices,
                            columns,
                            ..
                        } = table;
                        let positions = [if table_mask & element_masks[0] != 0 {
                            Some(column_position(table_mask, element_masks[0]))
                        } else {
                            None
                        }];
                        if added_mask != 0 {
                            added_scratch.clear();
                            added_scratch.resize(entity_indices.len(), false);
                            for column in columns.iter() {
                                if added_mask & (1u64 << column.component_index) == 0 {
                                    continue;
                                }
                                for (row, &added_tick) in column.added.iter().enumerate() {
                                    if tick_is_newer(added_tick, since_tick) {
                                        added_scratch[row] = true;
                                    }
                                }
                            }
                        }
                        let [slot] = distribute_slots(columns, positions);
                        let mut fetch = <$element as QueryElement>::fetch(slot, current_tick);
                        let mut visited = false;
                        'rows: for (row_index, &entity) in entity_indices.iter().enumerate() {
                            if !tag_sets_match(
                                &filters.include_sets,
                                &filters.exclude_sets,
                                entity,
                            ) {
                                continue 'rows;
                            }
                            if added_mask != 0 && !added_scratch[row_index] {
                                continue 'rows;
                            }
                            if changed_mask != 0
                                && !<$element as QueryElement>::changed_newer(
                                    &fetch, row_index, since_tick,
                                )
                            {
                                continue 'rows;
                            }
                            let item = <$element as QueryElement>::item(&mut fetch, row_index);
                            visited = true;
                            f(entity, item);
                        }
                        if visited {
                            <$element as QueryElement>::stamp_peaks(&mut fetch);
                        }
                    }
                    driver.added_scratch = added_scratch;
                }

                #[cfg(not(target_family = "wasm"))]
                fn join_par_for_each<
                    FN: for<'item> Fn(Entity, Self::Item<'item>) + Send + Sync,
                >(
                    driver: &mut DynWorld,
                    element_worlds: &[Option<&DynWorld>; 8],
                    filters: &JoinFilters<'_>,
                    f: FN,
                ) {
                    use crate::rayon::prelude::*;

                    debug_assert!(
                        element_worlds[0].is_none(),
                        "a single-element tuple always drives its own world"
                    );
                    let mut element_masks = [0u64; 8];
                    element_masks[0] = <$element as QueryElement>::component_mask(driver);
                    let local_include = if <$element as QueryElement>::REQUIRED {
                        element_masks[0]
                    } else {
                        0
                    };
                    let mut changed_mask = 0u64;
                    for lookup in filters.changed_lookups.iter().flatten() {
                        changed_mask |= lookup(driver).expect(
                            "changed filters on query_join must name driver-world components",
                        );
                    }
                    let mut added_mask = 0u64;
                    for lookup in filters.added_lookups.iter().flatten() {
                        added_mask |= lookup(driver).expect(
                            "added filters on query_join must name driver-world components",
                        );
                    }
                    assert_eq!(
                        (changed_mask | added_mask) & !element_masks[0],
                        0,
                        "changed and added filters must name components present in the query tuple"
                    );
                    let since_tick = driver.last_tick;
                    let current_tick = driver.current_tick;
                    driver
                        .tables
                        .par_iter_mut()
                        .filter(|table| {
                            table.mask & local_include == local_include
                                && !table.entity_indices.is_empty()
                        })
                        .for_each(|table| {
                            let table_mask = table.mask;
                            let DynComponentArrays {
                                entity_indices,
                                columns,
                                ..
                            } = table;
                            let added_scratch: Vec<bool> = if added_mask != 0 {
                                let mut scratch = vec![false; entity_indices.len()];
                                for column in columns.iter() {
                                    if added_mask & (1u64 << column.component_index) == 0 {
                                        continue;
                                    }
                                    for (row, &added_tick) in column.added.iter().enumerate() {
                                        if tick_is_newer(added_tick, since_tick) {
                                            scratch[row] = true;
                                        }
                                    }
                                }
                                scratch
                            } else {
                                Vec::new()
                            };
                            let positions = [if table_mask & element_masks[0] != 0 {
                                Some(column_position(table_mask, element_masks[0]))
                            } else {
                                None
                            }];
                            let [slot] = distribute_slots(columns, positions);
                            let mut fetch =
                                <$element as QueryElement>::fetch(slot, current_tick);
                            let mut visited = false;
                            'rows: for (row_index, &entity) in
                                entity_indices.iter().enumerate()
                            {
                                if !tag_sets_match(
                                    &filters.include_sets,
                                    &filters.exclude_sets,
                                    entity,
                                ) {
                                    continue 'rows;
                                }
                                if added_mask != 0 && !added_scratch[row_index] {
                                    continue 'rows;
                                }
                                if changed_mask != 0
                                    && !<$element as QueryElement>::changed_newer(
                                        &fetch, row_index, since_tick,
                                    )
                                {
                                    continue 'rows;
                                }
                                let item =
                                    <$element as QueryElement>::item(&mut fetch, row_index);
                                visited = true;
                                f(entity, item);
                            }
                            if visited {
                                <$element as QueryElement>::stamp_peaks(&mut fetch);
                            }
                        });
                }

                fn fetch<'table>(
                    table_mask: u64,
                    columns: &'table mut [ColumnSlot],
                    element_masks: &[u64; 8],
                    current_tick: u32,
                ) -> Self::Fetch<'table> {
                    let position = if table_mask & element_masks[0] != 0 {
                        Some(column_position(table_mask, element_masks[0]))
                    } else {
                        None
                    };
                    let [slot] = distribute_slots(columns, [position]);
                    <$element as QueryElement>::fetch(slot, current_tick)
                }

                fn changed_newer(
                    fetch: &Self::Fetch<'_>,
                    index: usize,
                    element_masks: &[u64; 8],
                    changed_mask: u64,
                    since_tick: u32,
                ) -> bool {
                    changed_mask & element_masks[0] != 0
                        && <$element as QueryElement>::changed_newer(fetch, index, since_tick)
                }

                fn item<'fetch>(
                    fetch: &'fetch mut Self::Fetch<'_>,
                    index: usize,
                ) -> Self::Item<'fetch> {
                    <$element as QueryElement>::item(fetch, index)
                }

                fn stamp_peaks(fetch: &mut Self::Fetch<'_>) {
                    <$element as QueryElement>::stamp_peaks(fetch);
                }

                type ParFetch<'table> = <$element as QueryElement>::ParFetch<'table>;

                fn par_fetch<'table>(
                    table_mask: u64,
                    columns: &'table mut [ColumnSlot],
                    element_masks: &[u64; 8],
                    current_tick: u32,
                ) -> Self::ParFetch<'table> {
                    let position = if table_mask & element_masks[0] != 0 {
                        Some(column_position(table_mask, element_masks[0]))
                    } else {
                        None
                    };
                    let [slot] = distribute_slots(columns, [position]);
                    <$element as QueryElement>::par_fetch(slot, current_tick)
                }

                fn par_split<'table>(
                    fetch: Self::ParFetch<'table>,
                    mid: usize,
                ) -> (Self::ParFetch<'table>, Self::ParFetch<'table>) {
                    <$element as QueryElement>::par_split(fetch, mid)
                }

                fn par_item<'fetch>(
                    fetch: &'fetch mut Self::ParFetch<'_>,
                    index: usize,
                ) -> Self::Item<'fetch> {
                    <$element as QueryElement>::par_item(fetch, index)
                }

                #[cfg(feature = "raw_storage")]
                unsafe fn par_item_unchecked<'fetch>(
                    fetch: &'fetch mut Self::ParFetch<'_>,
                    index: usize,
                ) -> Self::Item<'fetch> {
                    unsafe { <$element as QueryElement>::par_item_unchecked(fetch, index) }
                }

                fn mark_changed_all(fetch: &mut Self::Fetch<'_>) {
                    <$element as QueryElement>::mark_changed_all(fetch);
                }

                fn item_marked<'fetch>(
                    fetch: &'fetch mut Self::Fetch<'_>,
                    index: usize,
                ) -> Self::Item<'fetch> {
                    <$element as QueryElement>::item_marked(fetch, index)
                }

                const ALL_REQUIRED: bool = <$element as QueryElement>::REQUIRED;

                fn fast_for_each<FN>(fetch: Self::ParFetch<'_>, entities: &[Entity], f: &mut FN)
                where
                    FN: for<'item> FnMut(Entity, Self::Item<'item>),
                {
                    #[cfg(feature = "raw_storage")]
                    {
                        let mut fetch = fetch;
                        for index in 0..entities.len() {
                            let entity = unsafe { *entities.get_unchecked(index) };
                            f(entity, unsafe {
                                <$element as QueryElement>::par_item_unchecked(&mut fetch, index)
                            });
                        }
                    }
                    #[cfg(not(feature = "raw_storage"))]
                    {
                        let mut iterator = <$element as QueryElement>::slice_iter(fetch);
                        for &entity in entities {
                            f(entity, iterator.next().unwrap());
                        }
                    }
                }
            }
        )+
    };
}

impl_bare_element_query!(
    &'element T,
    &'element mut T,
    Option<&'element T>,
    Option<&'element mut T>
);

macro_rules! impl_bare_element_read_query {
    ($($element:ty),+) => {
        $(
            impl<'element, T: Send + Sync + Default + 'static> ReadQueryTuple for $element {
                type ReadFetch<'table> = <$element as ReadQueryElement>::ReadFetch<'table>;

                fn lookup_masks(world: &DynWorld) -> Option<([u64; 8], u64)> {
                    let elements = [(
                        <$element as ReadQueryElement>::lookup_mask(world),
                        <$element as QueryElement>::REQUIRED,
                    )];
                    lookup_masks_from(&elements)
                }

                fn read_fetch<'table>(
                    table_mask: u64,
                    columns: &'table [ColumnSlot],
                    element_masks: &[u64; 8],
                ) -> Self::ReadFetch<'table> {
                    <$element as ReadQueryElement>::read_fetch(
                        if table_mask & element_masks[0] != 0 {
                            Some(&columns[column_position(table_mask, element_masks[0])])
                        } else {
                            None
                        },
                    )
                }

                fn read_changed_newer(
                    fetch: Self::ReadFetch<'_>,
                    index: usize,
                    element_masks: &[u64; 8],
                    changed_mask: u64,
                    since_tick: u32,
                ) -> bool {
                    changed_mask & element_masks[0] != 0
                        && <$element as ReadQueryElement>::read_changed_newer(
                            fetch, index, since_tick,
                        )
                }

                fn read_added_newer(
                    fetch: Self::ReadFetch<'_>,
                    index: usize,
                    element_masks: &[u64; 8],
                    added_mask: u64,
                    since_tick: u32,
                ) -> bool {
                    added_mask & element_masks[0] != 0
                        && <$element as ReadQueryElement>::read_added_newer(
                            fetch, index, since_tick,
                        )
                }

                fn read_item<'table>(
                    fetch: Self::ReadFetch<'table>,
                    index: usize,
                ) -> Self::Item<'table> {
                    <$element as ReadQueryElement>::read_item(fetch, index)
                }

                fn join_lookup(
                    driver: &DynWorld,
                    element_worlds: &[Option<&DynWorld>; 8],
                ) -> Option<([u64; 8], u64)> {
                    let mut masks = [0u64; 8];
                    let mut required = 0u64;
                    if element_worlds[0].is_none() {
                        match <$element as ReadQueryElement>::lookup_mask(driver) {
                            Some(mask) => {
                                masks[0] = mask;
                                if <$element as QueryElement>::REQUIRED {
                                    required |= mask;
                                }
                            }
                            None => {
                                if <$element as QueryElement>::REQUIRED {
                                    return None;
                                }
                            }
                        }
                    }
                    Some((masks, required))
                }

                fn join_read_fetch<'table>(
                    table_mask: u64,
                    columns: &'table [ColumnSlot],
                    element_masks: &[u64; 8],
                    element_worlds: &[Option<&DynWorld>; 8],
                ) -> Self::ReadFetch<'table> {
                    if element_worlds[0].is_none() {
                        <$element as ReadQueryElement>::read_fetch(
                            if element_masks[0] != 0 && table_mask & element_masks[0] != 0 {
                                Some(&columns[column_position(table_mask, element_masks[0])])
                            } else {
                                None
                            },
                        )
                    } else {
                        <$element as ReadQueryElement>::placeholder_read_fetch()
                    }
                }

                fn join_read_item<'world>(
                    fetch: Self::ReadFetch<'world>,
                    element_worlds: &[Option<&'world DynWorld>; 8],
                    entity: Entity,
                    index: usize,
                ) -> Option<Self::Item<'world>> {
                    match element_worlds[0] {
                        Some(world) => <$element as QueryElement>::foreign_item(world, entity),
                        None => Some(<$element as ReadQueryElement>::read_item(fetch, index)),
                    }
                }
            }
        )+
    };
}

impl_bare_element_read_query!(&'element T, Option<&'element T>);

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

            fn read_added_newer(
                fetch: Self::ReadFetch<'_>,
                index: usize,
                element_masks: &[u64; 8],
                added_mask: u64,
                since_tick: u32,
            ) -> bool {
                let mut newer = false;
                $(
                    if added_mask & element_masks[$position] != 0
                        && $element::read_added_newer(fetch.$position, index, since_tick)
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

            fn join_lookup(
                driver: &DynWorld,
                element_worlds: &[Option<&DynWorld>; 8],
            ) -> Option<([u64; 8], u64)> {
                let mut masks = [0u64; 8];
                let mut required = 0u64;
                $(
                    if element_worlds[$position].is_none() {
                        match $element::lookup_mask(driver) {
                            Some(mask) => {
                                masks[$position] = mask;
                                if $element::REQUIRED {
                                    required |= mask;
                                }
                            }
                            None => {
                                if $element::REQUIRED {
                                    return None;
                                }
                            }
                        }
                    }
                )+
                Some((masks, required))
            }

            fn join_read_fetch<'table>(
                table_mask: u64,
                columns: &'table [ColumnSlot],
                element_masks: &[u64; 8],
                element_worlds: &[Option<&DynWorld>; 8],
            ) -> Self::ReadFetch<'table> {
                ($(
                    if element_worlds[$position].is_none() {
                        $element::read_fetch(
                            if element_masks[$position] != 0
                                && table_mask & element_masks[$position] != 0
                            {
                                Some(&columns
                                    [column_position(table_mask, element_masks[$position])])
                            } else {
                                None
                            },
                        )
                    } else {
                        $element::placeholder_read_fetch()
                    },
                )+)
            }

            #[allow(non_snake_case)]
            fn join_read_item<'world>(
                fetch: Self::ReadFetch<'world>,
                element_worlds: &[Option<&'world DynWorld>; 8],
                entity: Entity,
                index: usize,
            ) -> Option<Self::Item<'world>> {
                $crate::paste::paste! {
                    $(
                        let [<item_ $position>] = match element_worlds[$position] {
                            Some(world) => {
                                match <$element as QueryElement>::foreign_item(world, entity) {
                                    Some(item) => item,
                                    None => return None,
                                }
                            }
                            None => $element::read_item(fetch.$position, index),
                        };
                    )+
                    Some(($([<item_ $position>],)+))
                }
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
/// Like everything else in the crate, the fields are plain data: the filter
/// methods are conveniences over them, and writing them directly is fine.
/// [`DynWorld::query`] seeds `include` with the tuple's required components;
/// a hand-built query whose `include` misses one panics at fetch rather than
/// misbehaving.
pub struct DynQuery<'world, Q: QueryTuple> {
    pub world: &'world mut DynWorld,
    pub include: u64,
    pub exclude: u64,
    pub changed_mask: u64,
    pub added_mask: u64,
    pub include_tag_sets: [Option<&'world SparseTagSet>; 4],
    pub exclude_tag_sets: [Option<&'world SparseTagSet>; 4],
    pub element_masks: Option<[u64; 8]>,
    pub marker: PhantomData<Q>,
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
    /// Registration permanently consumes one of the world's 64 mask bits,
    /// even when nothing carries the tag yet; on a shared borrow,
    /// [`DynQueryRef::with_tag_type`] looks up without registering.
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

    /// Only visit entities that gained `T` since the last step, whether by
    /// spawn or by a component add; mutating `T` does not retrigger this, and
    /// the added stamp rides along through table migrations. `T` must be one
    /// of the tuple's components.
    pub fn added<T: Send + Sync + Default + 'static>(mut self) -> Self {
        let mask = self.world.component_key::<T>().mask;
        self.added_mask |= mask;
        self
    }

    /// Freezes this query's resolved masks into a reusable
    /// [`PreparedQuery`], registering the tuple's types now. Tag-set
    /// references cannot be captured (they borrow the world); apply those
    /// at run time on the rehydrated query.
    pub fn prepare(self) -> PreparedQuery<Q> {
        assert!(
            self.include_tag_sets.iter().all(Option::is_none)
                && self.exclude_tag_sets.iter().all(Option::is_none),
            "prepared queries cannot capture tag-set references; \
             apply with_tag_set/without_tag_set on the rehydrated query"
        );
        PreparedQuery {
            element_masks: match self.element_masks {
                Some(masks) => masks,
                None => Q::element_masks(self.world),
            },
            include: self.include,
            exclude: self.exclude,
            changed_mask: self.changed_mask,
            added_mask: self.added_mask,
            marker: PhantomData,
        }
    }

    pub fn for_each(self, mut f: impl for<'item> FnMut(Entity, Q::Item<'item>)) {
        let element_masks = match self.element_masks {
            Some(masks) => masks,
            None => Q::element_masks(self.world),
        };
        let tuple_mask = element_masks.iter().fold(0, |mask, element| mask | element);
        assert_eq!(
            (self.changed_mask | self.added_mask) & !tuple_mask,
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
        let added_mask = self.added_mask;

        let has_row_filters = tag_include != 0
            || tag_exclude != 0
            || changed_mask != 0
            || added_mask != 0
            || self.include_tag_sets.iter().any(Option::is_some)
            || self.exclude_tag_sets.iter().any(Option::is_some);

        let tags = &self.world.tags;
        let table_indices = archetype_cached_tables(
            &mut self.world.query_cache,
            self.world.tables.iter().map(|table| table.mask),
            component_include,
        );
        let added_scratch = &mut self.world.added_scratch;
        let tables = &mut self.world.tables;

        for &table_index in table_indices {
            let table = &mut tables[table_index];
            if table.mask & component_exclude != 0 {
                continue;
            }

            if added_mask != 0 {
                added_scratch.clear();
                added_scratch.resize(table.entity_indices.len(), false);
                for column in &table.columns {
                    if added_mask & (1u64 << column.component_index) == 0 {
                        continue;
                    }
                    for (row, &added_tick) in column.added.iter().enumerate() {
                        if tick_is_newer(added_tick, since_tick) {
                            added_scratch[row] = true;
                        }
                    }
                }
            }

            let table_mask = table.mask;
            let entity_indices = &table.entity_indices;

            if has_row_filters {
                let mut fetch =
                    Q::fetch(table_mask, &mut table.columns, &element_masks, current_tick);
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
                    if added_mask != 0 && !added_scratch[index] {
                        continue;
                    }
                    visited = true;
                    f(entity, Q::item(&mut fetch, index));
                }
                if visited {
                    Q::stamp_peaks(&mut fetch);
                }
            } else if Q::ALL_REQUIRED && !entity_indices.is_empty() {
                let slice_fetch =
                    Q::par_fetch(table_mask, &mut table.columns, &element_masks, current_tick);
                Q::fast_for_each(slice_fetch, entity_indices.as_slice(), &mut f);
            } else {
                let mut fetch =
                    Q::fetch(table_mask, &mut table.columns, &element_masks, current_tick);
                Q::mark_changed_all(&mut fetch);
                for (index, &entity) in entity_indices.iter().enumerate() {
                    f(entity, Q::item_marked(&mut fetch, index));
                }
                if !entity_indices.is_empty() {
                    Q::stamp_peaks(&mut fetch);
                }
            }
        }
    }

    /// The parallel form of [`for_each`](Self::for_each): matching tables run
    /// concurrently, and an unfiltered query also splits the rows within each
    /// table across the pool, so one large archetype still uses every core.
    /// Filtered (tag/changed/added) queries stay table-granular.
    /// Same filter set and stamping semantics; the closure is `Fn` because
    /// tables run on worker threads, and the `added` filter builds one
    /// scratch buffer per table task.
    #[cfg(not(target_family = "wasm"))]
    pub fn par_for_each<F>(self, f: F)
    where
        F: for<'item> Fn(Entity, Q::Item<'item>) + Send + Sync,
    {
        use crate::rayon::prelude::*;

        let element_masks = match self.element_masks {
            Some(masks) => masks,
            None => Q::element_masks(self.world),
        };
        let tuple_mask = element_masks.iter().fold(0, |mask, element| mask | element);
        assert_eq!(
            (self.changed_mask | self.added_mask) & !tuple_mask,
            0,
            "changed and added filters must name components present in the query tuple"
        );

        let Some((component_include, component_exclude, tag_include, tag_exclude)) =
            self.world.split_masks(self.include, self.exclude)
        else {
            return;
        };

        let since_tick = self.world.last_tick;
        let current_tick = self.world.current_tick;
        let changed_mask = self.changed_mask;
        let added_mask = self.added_mask;
        let include_tag_sets = self.include_tag_sets;
        let exclude_tag_sets = self.exclude_tag_sets;

        let has_row_filters = tag_include != 0
            || tag_exclude != 0
            || changed_mask != 0
            || added_mask != 0
            || include_tag_sets.iter().any(Option::is_some)
            || exclude_tag_sets.iter().any(Option::is_some);

        let tags = &self.world.tags;
        self.world
            .tables
            .par_iter_mut()
            .filter(|table| {
                table.mask & component_include == component_include
                    && table.mask & component_exclude == 0
                    && !table.entity_indices.is_empty()
            })
            .for_each(|table| {
                let added_scratch: Vec<bool> = if added_mask != 0 {
                    let mut scratch = vec![false; table.entity_indices.len()];
                    for column in &table.columns {
                        if added_mask & (1u64 << column.component_index) == 0 {
                            continue;
                        }
                        for (row, &added_tick) in column.added.iter().enumerate() {
                            if tick_is_newer(added_tick, since_tick) {
                                scratch[row] = true;
                            }
                        }
                    }
                    scratch
                } else {
                    Vec::new()
                };

                let table_mask = table.mask;
                let DynComponentArrays {
                    entity_indices,
                    columns,
                    ..
                } = table;

                if has_row_filters {
                    let mut fetch = Q::fetch(table_mask, columns, &element_masks, current_tick);
                    let mut visited = false;
                    for (index, &entity) in entity_indices.iter().enumerate() {
                        if (tag_include != 0 || tag_exclude != 0)
                            && !tags_match(tags, entity, tag_include, tag_exclude)
                        {
                            continue;
                        }
                        if !tag_sets_match(&include_tag_sets, &exclude_tag_sets, entity) {
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
                        if added_mask != 0 && !added_scratch[index] {
                            continue;
                        }
                        visited = true;
                        f(entity, Q::item(&mut fetch, index));
                    }
                    if visited {
                        Q::stamp_peaks(&mut fetch);
                    }
                } else {
                    let fetch = Q::par_fetch(table_mask, columns, &element_masks, current_tick);
                    par_query_rows::<Q, F>(entity_indices.as_slice(), fetch, &f);
                }
            });
    }
}

/// A read-only typed query in progress, from [`DynWorld::query_ref`].
/// Filters compose before [`iter`](Self::iter) runs it. The fields are the
/// plain query description and writing them directly is fine; `iter`
/// resolves the tuple's masks itself, so `include` only carries extra
/// filters.
pub struct DynQueryRef<'world, Q: ReadQueryTuple> {
    pub world: &'world DynWorld,
    pub include: u64,
    pub exclude: u64,
    pub changed_mask: u64,
    pub added_mask: u64,
    pub include_tag_sets: [Option<&'world SparseTagSet>; 4],
    pub exclude_tag_sets: [Option<&'world SparseTagSet>; 4],
    pub resolved_masks: Option<([u64; 8], u64)>,
    pub dead: bool,
    pub marker: PhantomData<Q>,
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

    /// Only visit entities that gained `T` since the last step, whether by
    /// spawn or by a component add; mutating `T` does not retrigger this.
    /// `T` must be one of the tuple's components.
    pub fn added<T: Send + Sync + Default + 'static>(mut self) -> Self {
        match self.world.lookup_key::<T>() {
            Some(key) => self.added_mask |= key.mask,
            None => self.dead = true,
        }
        self
    }

    /// Freezes this query's resolved masks into a reusable
    /// [`PreparedQueryRef`]. Tag-set references cannot be captured; apply
    /// those at run time on the rehydrated query.
    pub fn prepare(self) -> PreparedQueryRef<Q> {
        assert!(
            self.include_tag_sets.iter().all(Option::is_none)
                && self.exclude_tag_sets.iter().all(Option::is_none),
            "prepared queries cannot capture tag-set references; \
             apply with_tag_set/without_tag_set on the rehydrated query"
        );
        PreparedQueryRef {
            resolved_masks: if self.dead {
                None
            } else {
                match self.resolved_masks {
                    Some(resolved) => Some(resolved),
                    None => Q::lookup_masks(self.world),
                }
            },
            include: self.include,
            exclude: self.exclude,
            changed_mask: self.changed_mask,
            added_mask: self.added_mask,
            marker: PhantomData,
        }
    }

    /// Runs the query as an iterator of `(Entity, items)`. Items borrow the
    /// world, not the iterator, so they survive collection.
    pub fn iter(self) -> DynQueryRefIter<'world, Q> {
        let mut done = self.dead;
        let mut element_masks = [0u64; 8];
        let mut include = self.include;
        let resolved = match self.resolved_masks {
            Some(resolved) => Some(resolved),
            None => Q::lookup_masks(self.world),
        };
        match resolved {
            Some((masks, required)) => {
                element_masks = masks;
                include |= required;
            }
            None => done = true,
        }

        if !done {
            let tuple_mask = element_masks.iter().fold(0, |mask, element| mask | element);
            assert_eq!(
                (self.changed_mask | self.added_mask) & !tuple_mask,
                0,
                "changed and added filters must name components present in the query tuple"
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
            added_mask: self.added_mask,
            since_tick: self.world.last_tick,
            cached_tables,
            table_index: 0,
            row_index: 0,
            current: None,
            done,
        }
    }

    /// The exactly-one match: `Some` when precisely one entity matches,
    /// `None` for zero or several. The get-the-player call; take the entity
    /// and go back through `get_mut` when you need to write.
    pub fn single(self) -> Option<(Entity, Q::Item<'world>)> {
        let mut matches = self.iter();
        let first = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        Some(first)
    }

    /// Every unordered pair of distinct matches, for pairwise logic like
    /// collision tests. Items are shared borrows of the world, so pairs are
    /// `Copy` and this is a real [`Iterator`]. Matches are collected once up
    /// front, one `(Entity, item)` per match.
    pub fn iter_combinations(self) -> DynQueryCombinations<'world, Q>
    where
        Q::Item<'world>: Copy,
    {
        DynQueryCombinations {
            items: self.iter().collect(),
            first: 0,
            second: 1,
        }
    }
}

/// The iterator behind [`DynQueryRef::iter_combinations`]: yields each
/// unordered pair of distinct matches exactly once, in match order.
pub struct DynQueryCombinations<'world, Q: ReadQueryTuple> {
    pub items: Vec<(Entity, Q::Item<'world>)>,
    pub first: usize,
    pub second: usize,
}

impl<'world, Q: ReadQueryTuple> Iterator for DynQueryCombinations<'world, Q>
where
    Q::Item<'world>: Copy,
{
    type Item = ((Entity, Q::Item<'world>), (Entity, Q::Item<'world>));

    fn next(&mut self) -> Option<Self::Item> {
        while self.second >= self.items.len() {
            if self.first + 2 >= self.items.len() {
                return None;
            }
            self.first += 1;
            self.second = self.first + 1;
        }
        let pair = (self.items[self.first], self.items[self.second]);
        self.second += 1;
        Some(pair)
    }
}

/// The iterator behind [`DynQueryRef::iter`]. Walks matching tables in
/// order, resolving columns once per table. When a `&mut` query path has
/// already cached this include mask's table list, the iterator reuses it
/// instead of scanning every table; the cache stays valid because table
/// registration appends to matching entries.
pub struct DynQueryRefIter<'world, Q: ReadQueryTuple> {
    pub world: &'world DynWorld,
    pub element_masks: [u64; 8],
    pub include: u64,
    pub exclude: u64,
    pub tag_include: u64,
    pub tag_exclude: u64,
    pub include_tag_sets: [Option<&'world SparseTagSet>; 4],
    pub exclude_tag_sets: [Option<&'world SparseTagSet>; 4],
    pub changed_mask: u64,
    pub added_mask: u64,
    pub since_tick: u32,
    pub cached_tables: Option<&'world [usize]>,
    pub table_index: usize,
    pub row_index: usize,
    pub current: Option<(&'world [Entity], Q::ReadFetch<'world>)>,
    pub done: bool,
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
                    if self.added_mask != 0
                        && !Q::read_added_newer(
                            fetch,
                            index,
                            &self.element_masks,
                            self.added_mask,
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
    fn write_group(self, ecs: &mut DynEcs, entity: Entity);
}

/// A [`Bundle`] whose components are `Clone`, so a whole batch of freshly
/// spawned rows can be filled column by column from one bundle value. Sealed
/// through [`Bundle`]; every clonable tuple bundle implements it.
pub trait CloneBundle: Bundle + Clone {
    fn spawn_extend(&self, world: &mut DynWorld, table_index: usize, count: usize);
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

            #[allow(non_snake_case)]
            fn write_group(self, ecs: &mut DynEcs, entity: Entity) {
                let ($($element,)+) = self;
                $(ecs.set(entity, $element);)+
            }
        }

        impl<$($element: Send + Sync + Default + Clone + 'static),+> CloneBundle for ($($element,)+) {
            #[allow(non_snake_case)]
            fn spawn_extend(&self, world: &mut DynWorld, table_index: usize, count: usize) {
                let ($($element,)+) = self;
                $(world.extend_column(table_index, count, $element);)+
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

/// A cross-world typed query in progress, from [`DynEcs::query_join`].
/// The fields are plain data like every query builder in this crate. Tag
/// filters name group marker tags; `changed`/`added` filters name
/// driver-world components and panic when the type routes elsewhere, since
/// only the driver's ticks are walked in place.
/// Resolves one filter type's mask against the join's driver world.
pub type JoinMaskLookup = fn(&DynWorld) -> Option<u64>;

pub struct DynJoin<'ecs, Q: QueryTuple> {
    pub ecs: &'ecs mut DynEcs,
    pub include_tag_types: [Option<TypeId>; 4],
    pub exclude_tag_types: [Option<TypeId>; 4],
    pub changed_lookups: [Option<JoinMaskLookup>; 4],
    pub added_lookups: [Option<JoinMaskLookup>; 4],
    pub marker: PhantomData<Q>,
}

fn push_join_slot<T: Copy>(slots: &mut [Option<T>; 4], value: T) {
    for slot in slots.iter_mut() {
        if slot.is_none() {
            *slot = Some(value);
            return;
        }
    }
    panic!("a join filter family holds at most four entries");
}

fn join_mask_of<T: Send + Sync + Default + 'static>(world: &DynWorld) -> Option<u64> {
    world.lookup_key::<T>().map(|key| key.mask)
}

impl<Q: QueryTuple> DynJoin<'_, Q> {
    /// Only visit entities carrying the marker type `T`'s group tag. A tag
    /// nothing has used yet matches nothing.
    pub fn with_tag_type<T: 'static>(mut self) -> Self {
        push_join_slot(&mut self.include_tag_types, TypeId::of::<T>());
        self
    }

    /// Skip entities carrying the marker type `T`'s group tag.
    pub fn without_tag_type<T: 'static>(mut self) -> Self {
        push_join_slot(&mut self.exclude_tag_types, TypeId::of::<T>());
        self
    }

    /// Only visit entities whose `T` changed since the driver world's last
    /// step. `T` must be one of the tuple's driver-world components.
    pub fn changed<T: Send + Sync + Default + 'static>(mut self) -> Self {
        push_join_slot(&mut self.changed_lookups, join_mask_of::<T>);
        self
    }

    /// Only visit entities that gained `T` since the driver world's last
    /// step. `T` must be one of the tuple's driver-world components.
    pub fn added<T: Send + Sync + Default + 'static>(mut self) -> Self {
        push_join_slot(&mut self.added_lookups, join_mask_of::<T>);
        self
    }

    /// Runs the join. Routing, the driver rule, and the borrow split are
    /// documented on [`DynEcs::query_join`].
    pub fn for_each(self, f: impl for<'item> FnMut(Entity, Q::Item<'item>)) {
        let routes = Q::join_routes(&self.ecs.worlds);

        let mut driver: Option<usize> = None;
        for route in routes.iter().flatten() {
            if route.required && route.world.is_none() {
                panic!(
                    "{} is not registered in any member world; add it to a member schema first",
                    route.type_name
                );
            }
            if route.mutable
                && let Some(world) = route.world
            {
                match driver {
                    None => driver = Some(world),
                    Some(existing) => assert_eq!(
                        existing, world,
                        "cross-world queries mutate only one world's components; \
                         {} lives in member world {world}, but another mutable element \
                         already fixed the driver to member world {existing}",
                        route.type_name
                    ),
                }
            }
        }
        let driver = driver.unwrap_or_else(|| {
            let mut counts = vec![0usize; self.ecs.worlds.len()];
            for route in routes.iter().flatten() {
                if let Some(world) = route.world {
                    counts[world] += 1;
                }
            }
            counts
                .iter()
                .copied()
                .enumerate()
                .max_by_key(|&(index, count)| (count, usize::MAX - index))
                .map(|(index, _)| index)
                .expect("query_join requires at least one member world")
        });

        let mut include_sets: [Option<&SparseTagSet>; 4] = [None; 4];
        let mut exclude_sets: [Option<&SparseTagSet>; 4] = [None; 4];
        for (slot, type_id) in include_sets.iter_mut().zip(self.include_tag_types.iter()) {
            if let Some(type_id) = type_id {
                match self.ecs.tag_type_indices.get(type_id) {
                    Some(&index) => *slot = Some(&self.ecs.tags[index]),
                    None => return,
                }
            }
        }
        for (slot, type_id) in exclude_sets.iter_mut().zip(self.exclude_tag_types.iter()) {
            if let Some(type_id) = type_id
                && let Some(&index) = self.ecs.tag_type_indices.get(type_id)
            {
                *slot = Some(&self.ecs.tags[index]);
            }
        }

        let (left, rest) = self.ecs.worlds.split_at_mut(driver);
        let (driver_world, right) = rest
            .split_first_mut()
            .expect("the driver index is within the member list");
        let left: &[DynWorld] = left;
        let right: &[DynWorld] = right;

        let mut element_worlds: [Option<&DynWorld>; 8] = [None; 8];
        for (position, route) in routes.iter().enumerate() {
            if let Some(route) = route
                && let Some(world) = route.world
                && world != driver
            {
                element_worlds[position] = Some(if world < driver {
                    &left[world]
                } else {
                    &right[world - driver - 1]
                });
            }
        }

        let filters = JoinFilters {
            include_sets,
            exclude_sets,
            changed_lookups: self.changed_lookups,
            added_lookups: self.added_lookups,
        };
        Q::join_for_each(driver_world, &element_worlds, &filters, f);
    }

    /// The parallel form of [`for_each`](Self::for_each): driver tables run
    /// concurrently, rows within a table sequentially, foreign worlds are
    /// shared across threads read-only, and the `added` filter builds one
    /// scratch buffer per table task. Same routing rules and stamping.
    #[cfg(not(target_family = "wasm"))]
    pub fn par_for_each<F>(self, f: F)
    where
        F: for<'item> Fn(Entity, Q::Item<'item>) + Send + Sync,
    {
        let routes = Q::join_routes(&self.ecs.worlds);

        let mut driver: Option<usize> = None;
        for route in routes.iter().flatten() {
            if route.required && route.world.is_none() {
                panic!(
                    "{} is not registered in any member world; add it to a member schema first",
                    route.type_name
                );
            }
            if route.mutable
                && let Some(world) = route.world
            {
                match driver {
                    None => driver = Some(world),
                    Some(existing) => assert_eq!(
                        existing, world,
                        "cross-world queries mutate only one world's components; \
                         {} lives in member world {world}, but another mutable element \
                         already fixed the driver to member world {existing}",
                        route.type_name
                    ),
                }
            }
        }
        let driver = driver.unwrap_or_else(|| {
            let mut counts = vec![0usize; self.ecs.worlds.len()];
            for route in routes.iter().flatten() {
                if let Some(world) = route.world {
                    counts[world] += 1;
                }
            }
            counts
                .iter()
                .copied()
                .enumerate()
                .max_by_key(|&(index, count)| (count, usize::MAX - index))
                .map(|(index, _)| index)
                .expect("query_join requires at least one member world")
        });

        let mut include_sets: [Option<&SparseTagSet>; 4] = [None; 4];
        let mut exclude_sets: [Option<&SparseTagSet>; 4] = [None; 4];
        for (slot, type_id) in include_sets.iter_mut().zip(self.include_tag_types.iter()) {
            if let Some(type_id) = type_id {
                match self.ecs.tag_type_indices.get(type_id) {
                    Some(&index) => *slot = Some(&self.ecs.tags[index]),
                    None => return,
                }
            }
        }
        for (slot, type_id) in exclude_sets.iter_mut().zip(self.exclude_tag_types.iter()) {
            if let Some(type_id) = type_id
                && let Some(&index) = self.ecs.tag_type_indices.get(type_id)
            {
                *slot = Some(&self.ecs.tags[index]);
            }
        }

        let (left, rest) = self.ecs.worlds.split_at_mut(driver);
        let (driver_world, right) = rest
            .split_first_mut()
            .expect("the driver index is within the member list");
        let left: &[DynWorld] = left;
        let right: &[DynWorld] = right;

        let mut element_worlds: [Option<&DynWorld>; 8] = [None; 8];
        for (position, route) in routes.iter().enumerate() {
            if let Some(route) = route
                && let Some(world) = route.world
                && world != driver
            {
                element_worlds[position] = Some(if world < driver {
                    &left[world]
                } else {
                    &right[world - driver - 1]
                });
            }
        }

        let filters = JoinFilters {
            include_sets,
            exclude_sets,
            changed_lookups: self.changed_lookups,
            added_lookups: self.added_lookups,
        };
        Q::join_par_for_each(driver_world, &element_worlds, &filters, f);
    }
}

/// The read-only cross-world join builder, from
/// [`DynEcs::query_join_ref`]. Filter semantics mirror [`DynJoin`]'s,
/// except failures degrade to empty instead of panicking where the
/// read-only single-world path also degrades.
pub struct DynJoinRef<'ecs, Q: ReadQueryTuple> {
    pub ecs: &'ecs DynEcs,
    pub include_tag_types: [Option<TypeId>; 4],
    pub exclude_tag_types: [Option<TypeId>; 4],
    pub changed_lookups: [Option<JoinMaskLookup>; 4],
    pub added_lookups: [Option<JoinMaskLookup>; 4],
    pub marker: PhantomData<Q>,
}

impl<'ecs, Q: ReadQueryTuple> DynJoinRef<'ecs, Q> {
    /// Only visit entities carrying the marker type `T`'s group tag.
    pub fn with_tag_type<T: 'static>(mut self) -> Self {
        push_join_slot(&mut self.include_tag_types, TypeId::of::<T>());
        self
    }

    /// Skip entities carrying the marker type `T`'s group tag.
    pub fn without_tag_type<T: 'static>(mut self) -> Self {
        push_join_slot(&mut self.exclude_tag_types, TypeId::of::<T>());
        self
    }

    /// Only visit entities whose `T` changed since the driver world's last
    /// step. `T` must be one of the tuple's driver-world components; an
    /// unregistered `T` reads as an empty iterator.
    pub fn changed<T: Send + Sync + Default + 'static>(mut self) -> Self {
        push_join_slot(&mut self.changed_lookups, join_mask_of::<T>);
        self
    }

    /// Only visit entities that gained `T` since the driver world's last
    /// step, with the same rules as [`changed`](Self::changed).
    pub fn added<T: Send + Sync + Default + 'static>(mut self) -> Self {
        push_join_slot(&mut self.added_lookups, join_mask_of::<T>);
        self
    }

    /// Runs the join as an iterator of `(Entity, items)` borrowing the
    /// group.
    pub fn iter(self) -> DynJoinRefIter<'ecs, Q> {
        let dead_iterator = |ecs: &'ecs DynEcs| DynJoinRefIter {
            driver: &ecs.worlds[0],
            element_worlds: [None; 8],
            element_masks: [0u64; 8],
            include: 0,
            include_sets: [None; 4],
            exclude_sets: [None; 4],
            changed_mask: 0,
            added_mask: 0,
            since_tick: 0,
            added_scratch: Vec::new(),
            table_index: 0,
            row_index: 0,
            current: None,
            done: true,
            marker: PhantomData,
        };

        if self.ecs.worlds.is_empty() {
            panic!("query_join requires at least one member world");
        }

        let routes = Q::join_routes(&self.ecs.worlds);
        for route in routes.iter().flatten() {
            if route.required && route.world.is_none() {
                return dead_iterator(self.ecs);
            }
        }
        let mut counts = vec![0usize; self.ecs.worlds.len()];
        for route in routes.iter().flatten() {
            if let Some(world) = route.world {
                counts[world] += 1;
            }
        }
        let driver_index = counts
            .iter()
            .copied()
            .enumerate()
            .max_by_key(|&(index, count)| (count, usize::MAX - index))
            .map(|(index, _)| index)
            .expect("query_join requires at least one member world");

        let mut include_sets: [Option<&SparseTagSet>; 4] = [None; 4];
        let mut exclude_sets: [Option<&SparseTagSet>; 4] = [None; 4];
        for (slot, type_id) in include_sets.iter_mut().zip(self.include_tag_types.iter()) {
            if let Some(type_id) = type_id {
                match self.ecs.tag_type_indices.get(type_id) {
                    Some(&index) => *slot = Some(&self.ecs.tags[index]),
                    None => return dead_iterator(self.ecs),
                }
            }
        }
        for (slot, type_id) in exclude_sets.iter_mut().zip(self.exclude_tag_types.iter()) {
            if let Some(type_id) = type_id
                && let Some(&index) = self.ecs.tag_type_indices.get(type_id)
            {
                *slot = Some(&self.ecs.tags[index]);
            }
        }

        let driver = &self.ecs.worlds[driver_index];
        let mut element_worlds: [Option<&'ecs DynWorld>; 8] = [None; 8];
        for (position, route) in routes.iter().enumerate() {
            if let Some(route) = route
                && let Some(world) = route.world
                && world != driver_index
            {
                element_worlds[position] = Some(&self.ecs.worlds[world]);
            }
        }

        let Some((element_masks, include)) = Q::join_lookup(driver, &element_worlds) else {
            return dead_iterator(self.ecs);
        };

        let mut changed_mask = 0u64;
        for lookup in self.changed_lookups.iter().flatten() {
            match lookup(driver) {
                Some(mask) => changed_mask |= mask,
                None => return dead_iterator(self.ecs),
            }
        }
        let mut added_mask = 0u64;
        for lookup in self.added_lookups.iter().flatten() {
            match lookup(driver) {
                Some(mask) => added_mask |= mask,
                None => return dead_iterator(self.ecs),
            }
        }
        let local_tuple_mask = element_masks.iter().fold(0, |mask, element| mask | element);
        assert_eq!(
            (changed_mask | added_mask) & !local_tuple_mask,
            0,
            "changed and added filters must name components present in the query tuple"
        );

        DynJoinRefIter {
            driver,
            element_worlds,
            element_masks,
            include,
            include_sets,
            exclude_sets,
            changed_mask,
            added_mask,
            since_tick: driver.last_tick,
            added_scratch: Vec::new(),
            table_index: 0,
            row_index: 0,
            current: None,
            done: false,
            marker: PhantomData,
        }
    }
}

/// The iterator behind [`DynJoinRef::iter`]: walks the driver world's
/// matching tables in order, resolving foreign elements per entity. The
/// `added` filter recomputes its scratch per table into an iterator-owned
/// buffer.
pub struct DynJoinRefIter<'ecs, Q: ReadQueryTuple> {
    pub driver: &'ecs DynWorld,
    pub element_worlds: [Option<&'ecs DynWorld>; 8],
    pub element_masks: [u64; 8],
    pub include: u64,
    pub include_sets: [Option<&'ecs SparseTagSet>; 4],
    pub exclude_sets: [Option<&'ecs SparseTagSet>; 4],
    pub changed_mask: u64,
    pub added_mask: u64,
    pub since_tick: u32,
    pub added_scratch: Vec<bool>,
    pub table_index: usize,
    pub row_index: usize,
    pub current: Option<(&'ecs [Entity], Q::ReadFetch<'ecs>)>,
    pub done: bool,
    pub marker: PhantomData<Q>,
}

impl<'ecs, Q: ReadQueryTuple> Iterator for DynJoinRefIter<'ecs, Q> {
    type Item = (Entity, Q::Item<'ecs>);

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
                    if !tag_sets_match(&self.include_sets, &self.exclude_sets, entity) {
                        continue;
                    }
                    if self.added_mask != 0 && !self.added_scratch[index] {
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
                    match Q::join_read_item(fetch, &self.element_worlds, entity, index) {
                        Some(item) => return Some((entity, item)),
                        None => continue,
                    }
                }
                self.current = None;
            }

            let table = loop {
                let Some(table) = self.driver.tables.get(self.table_index) else {
                    self.done = true;
                    return None;
                };
                self.table_index += 1;
                if table.mask & self.include == self.include && !table.entity_indices.is_empty() {
                    break table;
                }
            };

            if self.added_mask != 0 {
                self.added_scratch.clear();
                self.added_scratch.resize(table.entity_indices.len(), false);
                for column in &table.columns {
                    if self.added_mask & (1u64 << column.component_index) == 0 {
                        continue;
                    }
                    for (row, &added_tick) in column.added.iter().enumerate() {
                        if tick_is_newer(added_tick, self.since_tick) {
                            self.added_scratch[row] = true;
                        }
                    }
                }
            }
            let fetch = Q::join_read_fetch(
                table.mask,
                &table.columns,
                &self.element_masks,
                &self.element_worlds,
            );
            self.current = Some((table.entity_indices.as_slice(), fetch));
            self.row_index = 0;
        }
    }
}

/// A mutable typed query resolved once and reused: element masks and
/// filter masks cached as plain copyable data, so hot systems skip the
/// per-call `TypeId` resolution. Build one by configuring a
/// [`DynQuery`] and calling [`prepare`](DynQuery::prepare); run it with
/// [`query`](Self::query). Prepared against one world's schema; running it
/// against a world with different mask assignments is a logic error that
/// the column machinery surfaces as a panic rather than wrong data.
#[derive(Clone, Copy)]
pub struct PreparedQuery<Q: QueryTuple> {
    pub element_masks: [u64; 8],
    pub include: u64,
    pub exclude: u64,
    pub changed_mask: u64,
    pub added_mask: u64,
    pub marker: PhantomData<Q>,
}

impl<Q: QueryTuple> PreparedQuery<Q> {
    /// Rehydrates the prepared masks into a runnable [`DynQuery`].
    /// Tag-set filters compose here at run time
    /// (`prepared.query(world).with_tag_set(...)`).
    pub fn query<'world>(&self, world: &'world mut DynWorld) -> DynQuery<'world, Q> {
        DynQuery {
            world,
            include: self.include,
            exclude: self.exclude,
            changed_mask: self.changed_mask,
            added_mask: self.added_mask,
            include_tag_sets: [None; 4],
            exclude_tag_sets: [None; 4],
            element_masks: Some(self.element_masks),
            marker: PhantomData,
        }
    }
}

/// The read-only prepared form, from [`DynQueryRef::prepare`]. A tuple
/// whose required elements were unregistered at prepare time stays dead,
/// matching `query_ref`'s graceful degradation.
#[derive(Clone, Copy)]
pub struct PreparedQueryRef<Q: ReadQueryTuple> {
    pub resolved_masks: Option<([u64; 8], u64)>,
    pub include: u64,
    pub exclude: u64,
    pub changed_mask: u64,
    pub added_mask: u64,
    pub marker: PhantomData<Q>,
}

impl<Q: ReadQueryTuple> PreparedQueryRef<Q> {
    /// Rehydrates the prepared masks into a runnable [`DynQueryRef`].
    pub fn query<'world>(&self, world: &'world DynWorld) -> DynQueryRef<'world, Q> {
        DynQueryRef {
            world,
            include: self.include,
            exclude: self.exclude,
            changed_mask: self.changed_mask,
            added_mask: self.added_mask,
            include_tag_sets: [None; 4],
            exclude_tag_sets: [None; 4],
            resolved_masks: self.resolved_masks,
            dead: self.resolved_masks.is_none(),
            marker: PhantomData,
        }
    }
}

/// A point-in-time census of one world's storage and bookkeeping, from
/// [`DynWorld::stats`]: table occupancy, schema budget, and the lengths of
/// every log and cache an editor overlay or a perf investigation reaches
/// for. Plain data, cheap to build, allocation proportional to the table
/// count.
#[derive(Clone, Debug, Default)]
pub struct WorldStats {
    pub entity_count: usize,
    pub table_count: usize,
    pub empty_table_count: usize,
    pub largest_table_rows: usize,
    pub table_rows: Vec<(u64, usize)>,
    pub component_count: usize,
    pub tag_count: usize,
    pub remaining_mask_bits: u32,
    pub structural_log_entries: usize,
    pub query_cache_entries: usize,
    pub resource_count: usize,
    pub event_channels: usize,
    pub pending_commands: usize,
}

/// The group-level census, from [`DynEcs::stats`]: allocator liveness,
/// group tags, logs, resources, and events, plus one [`WorldStats`] per
/// member world.
#[derive(Clone, Debug, Default)]
pub struct EcsStats {
    pub live_entities: usize,
    pub free_ids: usize,
    pub group_tag_count: usize,
    pub group_structural_log_entries: usize,
    pub group_resource_count: usize,
    pub group_event_channels: usize,
    pub worlds: Vec<WorldStats>,
}

/// A parent link for entity hierarchies: plain data, pull-maintained, no
/// hooks. Attach with `world.set(child, ChildOf(parent))`;
/// [`DynWorld::children`] and [`DynWorld::despawn_recursive`] scan it on
/// demand, [`HierarchyIndex`] answers from a synced map, and a link to a
/// despawned parent is just a link nothing resolves.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ChildOf(pub Entity);

/// A maintained child index over [`ChildOf`] links, for hierarchy-heavy
/// consumers: [`DynWorld::children`] scans every link carrier on demand,
/// while this answers from maps kept current by [`sync`](Self::sync).
///
/// Plain data owned by the consumer, no hooks: `sync` consumes the world's
/// structural log and change ticks, so every link write that stamps ticks
/// is picked up — spawns, `set`, migrations, and raw-tier writes followed
/// by [`DynWorld::mark_changed`] — and each sync costs proportional to what
/// changed since the last one. Reads reflect the last sync. In a
/// [`DynEcs`] group, sync against the member world holding the links and
/// despawn through the group using [`descendants`](Self::descendants).
pub struct HierarchyIndex {
    pub children: HashMap<Entity, Vec<Entity>>,
    pub parent_of: HashMap<Entity, Entity>,
    pub structural_cursor: u64,
    pub tick_cursor: u32,
}

impl Default for HierarchyIndex {
    fn default() -> Self {
        Self {
            children: HashMap::new(),
            parent_of: HashMap::new(),
            structural_cursor: 0,
            tick_cursor: u32::MAX,
        }
    }
}

impl HierarchyIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Brings the index up to date with the world, then fences the change
    /// window with [`DynWorld::increment_tick`] so writes made after this
    /// call land in the next sync. Despawns and `ChildOf` removals unlink
    /// through the structural log; new and rewritten links relink from the
    /// component's current value.
    pub fn sync(&mut self, world: &mut DynWorld) {
        let child_mask = world
            .lookup_key::<ChildOf>()
            .map(|key| key.mask)
            .unwrap_or(0);

        // raw_storage tracks neither structural changes nor ticks, so the
        // incremental unlink/relink path has nothing to diff against. Rebuild
        // the whole index from a scan of current links instead; the result is
        // identical, just not incremental.
        #[cfg(feature = "raw_storage")]
        {
            self.children.clear();
            self.parent_of.clear();
            if child_mask != 0 {
                let holders: Vec<Entity> = world.query_entities(child_mask).collect();
                for entity in holders {
                    if let Some(child_of) = world.get::<ChildOf>(entity) {
                        let parent = child_of.0;
                        self.parent_of.insert(entity, parent);
                        self.children.entry(parent).or_default().push(entity);
                    }
                }
            }
            world.increment_tick();
        }

        #[cfg(not(feature = "raw_storage"))]
        {
            let unlinks: Vec<Entity> = world
                .structural_changes_since(self.structural_cursor)
                .iter()
                .filter(|change| match change.kind {
                    StructuralChangeKind::Despawned => true,
                    StructuralChangeKind::ComponentsRemoved => change.mask & child_mask != 0,
                    _ => false,
                })
                .map(|change| change.entity)
                .collect();
            for entity in unlinks {
                self.unlink(entity);
            }

            if child_mask != 0 {
                let relinks: Vec<Entity> = world
                    .query_entities_changed_since(child_mask, self.tick_cursor)
                    .collect();
                for entity in relinks {
                    if let Some(child_of) = world.get::<ChildOf>(entity) {
                        self.relink(entity, child_of.0);
                    }
                }
            }

            self.structural_cursor = world.structural_sequence();
            self.tick_cursor = world.current_tick();
            world.increment_tick();
        }
    }

    fn unlink(&mut self, entity: Entity) {
        if let Some(parent) = self.parent_of.remove(&entity)
            && let Some(siblings) = self.children.get_mut(&parent)
        {
            siblings.retain(|&child| child != entity);
            if siblings.is_empty() {
                self.children.remove(&parent);
            }
        }
    }

    #[cfg(not(feature = "raw_storage"))]
    fn relink(&mut self, entity: Entity, parent: Entity) {
        if self.parent_of.get(&entity) == Some(&parent) {
            return;
        }
        self.unlink(entity);
        self.parent_of.insert(entity, parent);
        self.children.entry(parent).or_default().push(entity);
    }

    /// Children of a parent as of the last sync.
    pub fn children(&self, parent: Entity) -> &[Entity] {
        self.children.get(&parent).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Every entity reachable from the root through child edges as of the
    /// last sync, breadth-first, root excluded. Link cycles are tolerated.
    pub fn descendants(&self, root: Entity) -> Vec<Entity> {
        let mut pending = vec![root];
        let mut visited: Vec<Entity> = Vec::new();
        while let Some(parent) = pending.pop() {
            for &child in self.children(parent) {
                if child != root && !visited.contains(&child) {
                    visited.push(child);
                    pending.push(child);
                }
            }
        }
        visited
    }

    /// Despawns the root and its indexed descendants in one pass, eagerly
    /// unlinking them so the index is consistent before the next sync.
    /// Answers from the index, not a scan, so sync first if links changed
    /// since the last one. Returns the despawned entities.
    pub fn despawn_recursive(&mut self, world: &mut DynWorld, root: Entity) -> Vec<Entity> {
        let mut targets = self.descendants(root);
        targets.insert(0, root);
        for &entity in &targets {
            self.unlink(entity);
            self.children.remove(&entity);
        }
        world.despawn_entities(&targets)
    }
}

/// A tuple of resource types taken out of a [`ResourceMap`] together by
/// [`DynWorld::resources_scope`] and [`DynEcs::resources_scope`].
/// Implemented for tuples of up to eight distinct resource types; presence
/// and distinctness are checked before anything is removed, so a failed
/// take leaves the map untouched.
pub trait ResourceBundle: sealed::SealedResourceBundle + Sized {
    fn take(resources: &mut ResourceMap) -> Self;
    fn put(self, resources: &mut ResourceMap);
    fn contains_all(resources: &ResourceMap) -> bool;
}

macro_rules! impl_resource_bundle {
    ($($element:ident),+) => {
        impl<$($element: Send + Sync + 'static),+> sealed::SealedResourceBundle for ($($element,)+) {}

        impl<$($element: Send + Sync + 'static),+> ResourceBundle for ($($element,)+) {
            fn contains_all(resources: &ResourceMap) -> bool {
                true $(&& resources.entries.contains_key(&TypeId::of::<$element>()))+
            }

            fn take(resources: &mut ResourceMap) -> Self {
                let elements = [$((TypeId::of::<$element>(), std::any::type_name::<$element>()),)+];
                for (index, (type_id, name)) in elements.iter().enumerate() {
                    assert!(
                        !elements[..index].iter().any(|(seen, _)| seen == type_id),
                        "resource scopes must not repeat a resource type: {name}"
                    );
                    assert!(
                        resources.entries.contains_key(type_id),
                        "resources_scope requires {name} to be present"
                    );
                }
                ($(
                    resources
                        .remove::<$element>()
                        .expect("presence was checked before any removal"),
                )+)
            }

            #[allow(non_snake_case)]
            fn put(self, resources: &mut ResourceMap) {
                let ($($element,)+) = self;
                $(resources.insert($element);)+
            }
        }
    };
}

impl_resource_bundle!(A);
impl_resource_bundle!(A, B);
impl_resource_bundle!(A, B, C);
impl_resource_bundle!(A, B, C, D);
impl_resource_bundle!(A, B, C, D, E);
impl_resource_bundle!(A, B, C, D, E, F);
impl_resource_bundle!(A, B, C, D, E, F, G);
impl_resource_bundle!(A, B, C, D, E, F, G, H);

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
    #[cfg(not(feature = "raw_storage"))]
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
    #[cfg(not(feature = "raw_storage"))]
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
    #[cfg(not(feature = "raw_storage"))]
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
    #[cfg(not(feature = "raw_storage"))]
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
    #[cfg(not(feature = "raw_storage"))]
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
    #[cfg(not(feature = "raw_storage"))]
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
    #[cfg(not(feature = "raw_storage"))]
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
    #[cfg(not(feature = "raw_storage"))]
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

    #[cfg(not(feature = "raw_storage"))]
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
    #[cfg(not(feature = "raw_storage"))]
    fn test_filtered_mutable_query_keeps_changed_sets_exact() {
        let mut world = DynWorld::new();
        let position = world.register::<Position>();
        let boss = world.register_tag();
        let entities = world.spawn_entities(position.mask, 4);
        world.add_tag(boss, entities[1]);
        world.add_tag(boss, entities[3]);

        world.step();
        world
            .query::<(&mut Position,)>()
            .with_tag(boss)
            .for_each(|_entity, (value,)| value.x += 1.0);

        let changed: Vec<Entity> = world.query_entities_changed(position.mask).collect();
        assert_eq!(changed, vec![entities[1], entities[3]]);

        world.step();
        assert_eq!(world.query_entities_changed(position.mask).count(), 0);
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
    fn test_consume_events_delivers_exactly_once_per_cursor() {
        let mut world = DynWorld::new();
        world.send(PingEvent { value: 1 });
        world.send(PingEvent { value: 2 });

        let mut first_cursor = 0;
        let mut second_cursor = 0;

        let values: Vec<u32> = world
            .consume_events::<PingEvent>(&mut first_cursor)
            .iter()
            .map(|event| event.value)
            .collect();
        assert_eq!(values, vec![1, 2]);
        assert!(
            world
                .consume_events::<PingEvent>(&mut first_cursor)
                .is_empty()
        );

        world.step();
        world.send(PingEvent { value: 3 });

        let next: Vec<u32> = world
            .consume_events::<PingEvent>(&mut first_cursor)
            .iter()
            .map(|event| event.value)
            .collect();
        assert_eq!(next, vec![3], "the two-frame buffer must not re-deliver");

        let all: Vec<u32> = world
            .consume_events::<PingEvent>(&mut second_cursor)
            .iter()
            .map(|event| event.value)
            .collect();
        assert_eq!(
            all,
            vec![1, 2, 3],
            "independent consumers own their cursors"
        );

        struct NeverSent;
        let mut untouched = 0;
        assert!(world.consume_events::<NeverSent>(&mut untouched).is_empty());
        assert_eq!(untouched, 0);
    }

    #[test]
    fn test_res_returns_and_panics_with_type_name() {
        struct Score(u32);

        let mut world = DynWorld::new();
        world.insert_resource(Score(5));

        assert_eq!(world.res::<Score>().0, 5);
        world.res_mut::<Score>().0 += 1;
        assert_eq!(world.res::<Score>().0, 6);
    }

    #[test]
    #[should_panic(expected = "res requires")]
    fn test_res_panics_on_missing_resource() {
        struct Missing;

        let world = DynWorld::new();
        world.res::<Missing>();
    }

    #[test]
    fn test_spawn_bundles_clones_per_entity() {
        let mut world = DynWorld::new();
        let entities = world.spawn_bundles(
            (Position { x: 4.0, y: 0.0 }, Velocity { x: 1.0, y: 0.0 }),
            3,
        );

        assert_eq!(entities.len(), 3);
        for &entity in &entities {
            assert_eq!(world.get::<Position>(entity).unwrap().x, 4.0);
            assert_eq!(world.get::<Velocity>(entity).unwrap().x, 1.0);
        }
    }

    #[test]
    fn test_spawn_bundles_bulk_fill_stamps_every_row() {
        let mut world = DynWorld::new();
        let entities = world.spawn_bundles(
            (Position { x: 7.0, y: 2.0 }, Velocity { x: 3.0, y: 0.0 }),
            500,
        );

        assert_eq!(entities.len(), 500);
        assert_eq!(world.entity_count(), 500);

        let mut positions = 0;
        world
            .query::<(&Position, &Velocity)>()
            .for_each(|_entity, (position, velocity)| {
                assert_eq!(position.x, 7.0);
                assert_eq!(position.y, 2.0);
                assert_eq!(velocity.x, 3.0);
                positions += 1;
            });
        assert_eq!(positions, 500);
    }

    #[test]
    #[cfg(not(feature = "raw_storage"))]
    fn test_unfiltered_mutable_iteration_marks_changes() {
        let mut world = DynWorld::new();
        world.spawn_bundles(
            (Position { x: 1.0, y: 0.0 }, Velocity { x: 1.0, y: 0.0 }),
            200,
        );
        world.step();

        world
            .query::<(&mut Position, &Velocity)>()
            .for_each(|_entity, (position, velocity)| position.x += velocity.x);

        let mut changed = 0;
        world
            .query::<&Position>()
            .changed::<Position>()
            .for_each(|_entity, _position| changed += 1);
        assert_eq!(changed, 200);
    }

    #[test]
    fn test_column_storage_drops_every_value_exactly_once() {
        use std::sync::atomic::{AtomicIsize, Ordering};
        static LIVE: AtomicIsize = AtomicIsize::new(0);

        struct Tracked(u64);
        impl Default for Tracked {
            fn default() -> Self {
                LIVE.fetch_add(1, Ordering::Relaxed);
                Tracked(0)
            }
        }
        impl Clone for Tracked {
            fn clone(&self) -> Self {
                LIVE.fetch_add(1, Ordering::Relaxed);
                Tracked(self.0)
            }
        }
        impl Drop for Tracked {
            fn drop(&mut self) {
                LIVE.fetch_sub(1, Ordering::Relaxed);
            }
        }

        LIVE.store(0, Ordering::Relaxed);
        {
            let mut world = DynWorld::new();
            let mut entities: Vec<_> = (0..200)
                .map(|_| world.spawn((Tracked::default(),)))
                .collect();
            world.spawn_bundles((Tracked::default(), Position { x: 0.0, y: 0.0 }), 200);
            for &entity in &entities {
                world.set(entity, Velocity { x: 1.0, y: 0.0 });
            }
            for &entity in entities.iter().take(120) {
                world.queue_despawn_entity(entity);
            }
            world.apply_commands();
            entities.clear();
        }
        assert_eq!(
            LIVE.load(Ordering::Relaxed),
            0,
            "every Tracked value must be constructed and dropped exactly once"
        );
    }

    #[cfg(feature = "raw_storage")]
    #[test]
    fn test_raw_storage_applies_writes_with_change_detection_disabled() {
        let mut world = DynWorld::new();
        let position = world.register::<Position>();
        let entities: Vec<_> = (0..64)
            .map(|index| {
                world.spawn((Position {
                    x: index as f32,
                    y: 0.0,
                },))
            })
            .collect();

        world.step();
        world
            .query::<&mut Position>()
            .for_each(|_entity, position| {
                position.x += 1.0;
            });

        assert_eq!(
            world.query_entities_changed(position.mask).count(),
            0,
            "raw_storage intentionally disables per-row change detection"
        );

        for (index, &entity) in entities.iter().enumerate() {
            assert_eq!(
                world.get::<Position>(entity).unwrap().x,
                index as f32 + 1.0,
                "the mutable write itself must still land under raw_storage"
            );
        }
    }

    #[cfg(feature = "raw_storage")]
    #[test]
    fn test_raw_storage_buffer_pool_reuse_preserves_data() {
        for _ in 0..50 {
            let mut world = DynWorld::new();
            world.spawn_bundles(
                (Position { x: 3.0, y: 4.0 }, Velocity { x: 1.0, y: 2.0 }),
                500,
            );
            let total: f32 = {
                let mut sum = 0.0;
                world.query::<(&Position, &Velocity)>().for_each(
                    |_entity, (position, velocity)| {
                        sum += position.x + position.y + velocity.x + velocity.y;
                    },
                );
                sum
            };
            assert_eq!(
                total,
                (3.0 + 4.0 + 1.0 + 2.0) * 500.0,
                "columns recycled through the thread-local pool must read back exactly"
            );
        }
    }

    #[cfg(feature = "raw_storage")]
    #[test]
    fn test_raw_storage_pool_reuse_drops_every_value_exactly_once() {
        use std::sync::atomic::{AtomicIsize, Ordering};
        static LIVE: AtomicIsize = AtomicIsize::new(0);

        struct Tracked(u64);
        impl Default for Tracked {
            fn default() -> Self {
                LIVE.fetch_add(1, Ordering::Relaxed);
                Tracked(0)
            }
        }
        impl Clone for Tracked {
            fn clone(&self) -> Self {
                LIVE.fetch_add(1, Ordering::Relaxed);
                Tracked(self.0)
            }
        }
        impl Drop for Tracked {
            fn drop(&mut self) {
                LIVE.fetch_sub(1, Ordering::Relaxed);
            }
        }

        LIVE.store(0, Ordering::Relaxed);
        for _ in 0..30 {
            let mut world = DynWorld::new();
            let entities: Vec<_> = (0..100)
                .map(|_| world.spawn((Tracked::default(),)))
                .collect();
            for &entity in entities.iter().take(40) {
                world.set(entity, Velocity { x: 1.0, y: 0.0 });
            }
            let doomed: Vec<_> = entities.iter().skip(60).copied().collect();
            world.despawn_entities(&doomed);
        }
        assert_eq!(
            LIVE.load(Ordering::Relaxed),
            0,
            "recycling column buffers through the pool must not leak or double-drop values"
        );
    }

    #[test]
    fn test_migration_preserves_every_row_and_location() {
        let mut world = DynWorld::new();
        let entities: Vec<_> = (0..400)
            .map(|index| {
                world.spawn((Position {
                    x: index as f32,
                    y: 0.0,
                },))
            })
            .collect();

        for &entity in &entities {
            world.set(entity, Velocity { x: 1.0, y: 0.0 });
        }
        for (index, &entity) in entities.iter().enumerate() {
            assert_eq!(world.get::<Position>(entity).unwrap().x, index as f32);
            assert_eq!(world.get::<Velocity>(entity).unwrap().x, 1.0);
        }

        for &entity in &entities {
            world.remove::<Velocity>(entity);
        }
        for (index, &entity) in entities.iter().enumerate() {
            assert_eq!(world.get::<Position>(entity).unwrap().x, index as f32);
            assert!(world.get::<Velocity>(entity).is_none());
        }
        assert_eq!(world.entity_count(), 400);
    }

    #[test]
    fn test_all_required_fast_iteration_visits_every_row() {
        let mut world = DynWorld::new();
        world.spawn_bundles(
            (Position { x: 1.0, y: 0.0 }, Velocity { x: 2.0, y: 0.0 }),
            1500,
        );
        world.spawn_bundles((Position { x: 5.0, y: 0.0 },), 700);

        world
            .query::<(&mut Position, &Velocity)>()
            .for_each(|_entity, (position, velocity)| position.x += velocity.x);
        world
            .query::<&mut Position>()
            .for_each(|_entity, position| position.y += 1.0);

        let mut moved = 0;
        let mut still = 0;
        world.query::<(&Position, Option<&Velocity>)>().for_each(
            |_entity, (position, velocity)| {
                assert!((position.y - 1.0).abs() < 1e-3);
                match velocity {
                    Some(_) => {
                        assert!((position.x - 3.0).abs() < 1e-3);
                        moved += 1;
                    }
                    None => {
                        assert!((position.x - 5.0).abs() < 1e-3);
                        still += 1;
                    }
                }
            },
        );
        assert_eq!(moved, 1500);
        assert_eq!(still, 700);
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn test_par_for_each_covers_large_single_archetype() {
        let mut world = DynWorld::new();
        let count = 5000;
        world.spawn_bundles(
            (Position { x: 1.0, y: 0.0 }, Velocity { x: 2.0, y: 0.0 }),
            count,
        );

        world.query::<(&mut Position, &Velocity)>().par_for_each(
            |_entity, (position, velocity)| {
                position.x += velocity.x;
            },
        );

        let mut visited = 0;
        let mut wrong = 0;
        world.query::<&Position>().for_each(|_entity, position| {
            visited += 1;
            if (position.x - 3.0).abs() > 1e-3 {
                wrong += 1;
            }
        });
        assert_eq!(visited, count);
        assert_eq!(wrong, 0);
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn test_par_for_each_bare_and_optional_elements() {
        let mut world = DynWorld::new();
        world.spawn_bundles((Position { x: 5.0, y: 0.0 },), 3000);
        world.spawn_bundles(
            (Position { x: 5.0, y: 0.0 }, Velocity { x: 1.0, y: 0.0 }),
            3000,
        );

        world
            .query::<&mut Position>()
            .par_for_each(|_entity, position| position.x *= 2.0);
        world
            .query::<(&mut Position, Option<&Velocity>)>()
            .par_for_each(|_entity, (position, velocity)| {
                if let Some(velocity) = velocity {
                    position.x += velocity.x;
                }
            });

        let mut without_velocity = 0;
        let mut with_velocity = 0;
        world.query::<(&Position, Option<&Velocity>)>().for_each(
            |_entity, (position, velocity)| match velocity {
                None => {
                    assert!((position.x - 10.0).abs() < 1e-3);
                    without_velocity += 1;
                }
                Some(_) => {
                    assert!((position.x - 11.0).abs() < 1e-3);
                    with_velocity += 1;
                }
            },
        );
        assert_eq!(without_velocity, 3000);
        assert_eq!(with_velocity, 3000);
    }

    #[test]
    #[cfg(not(feature = "raw_storage"))]
    fn test_bare_element_queries_match_single_tuples() {
        let mut world = DynWorld::new();
        let plain = world.spawn((Position { x: 1.0, y: 0.0 },));
        let moving = world.spawn((Position { x: 2.0, y: 0.0 }, Velocity { x: 5.0, y: 0.0 }));

        world
            .query::<&mut Position>()
            .for_each(|_entity, position| position.x += 10.0);
        assert_eq!(world.get::<Position>(plain).unwrap().x, 11.0);
        assert_eq!(world.get::<Position>(moving).unwrap().x, 12.0);

        let mut velocities = Vec::new();
        world
            .query::<Option<&Velocity>>()
            .for_each(|entity, velocity| {
                velocities.push((entity, velocity.map(|velocity| velocity.x)));
            });
        velocities.sort_by_key(|(entity, _)| entity.id);
        assert_eq!(velocities, vec![(plain, None), (moving, Some(5.0))]);

        let total: f32 = world
            .query_ref::<&Position>()
            .iter()
            .map(|(_entity, position)| position.x)
            .sum();
        assert_eq!(total, 23.0);

        let with_velocity: Vec<Entity> = world
            .query_ref::<Option<&Velocity>>()
            .iter()
            .filter(|(_entity, velocity)| velocity.is_some())
            .map(|(entity, _)| entity)
            .collect();
        assert_eq!(with_velocity, vec![moving]);

        world.step();
        world.get_mut::<Position>(plain).unwrap().x = 0.0;
        let mut changed = Vec::new();
        world
            .query::<&Position>()
            .changed::<Position>()
            .for_each(|entity, _position| changed.push(entity));
        assert_eq!(changed, vec![plain]);
    }

    #[test]
    fn test_remaining_bits_counts_components_and_tags() {
        let mut world = DynWorld::new();
        assert_eq!(world.remaining_bits(), 64);
        world.register::<Position>();
        world.register::<Velocity>();
        world.register_tag();
        world.tag_key::<Health>();
        assert_eq!(world.remaining_bits(), 60);
    }

    #[test]
    #[cfg(not(feature = "raw_storage"))]
    fn test_added_filter_matches_spawns_and_component_adds_only() {
        let mut world = DynWorld::new();
        let velocity = world.register::<Velocity>();
        let veteran = world.spawn((Position::default(),));

        world.step();
        let fresh = world.spawn((Position::default(),));
        world.add_components(veteran, velocity.mask);
        let mut added_positions = Vec::new();
        world
            .query::<&Position>()
            .added::<Position>()
            .for_each(|entity, _position| added_positions.push(entity));
        assert_eq!(
            added_positions,
            vec![fresh],
            "a migration must carry the original added tick"
        );

        let mut added_velocities = Vec::new();
        world
            .query::<&Velocity>()
            .added::<Velocity>()
            .for_each(|entity, _velocity| added_velocities.push(entity));
        assert_eq!(added_velocities, vec![veteran]);

        world.step();
        world.get_mut::<Position>(fresh).unwrap().x = 5.0;
        let added_now: Vec<Entity> = world
            .query_ref::<&Position>()
            .added::<Position>()
            .iter()
            .map(|(entity, _position)| entity)
            .collect();
        assert!(
            added_now.is_empty(),
            "mutation must not retrigger the added filter"
        );
        let changed_now: Vec<Entity> = world
            .query_ref::<&Position>()
            .changed::<Position>()
            .iter()
            .map(|(entity, _position)| entity)
            .collect();
        assert_eq!(changed_now, vec![fresh]);
    }

    #[test]
    #[should_panic(expected = "added filters must name components")]
    fn test_added_filter_rejects_types_outside_tuple() {
        let mut world = DynWorld::new();
        world.register::<Velocity>();
        world.spawn((Position::default(),));
        world
            .query_ref::<&Position>()
            .added::<Velocity>()
            .iter()
            .count();
    }

    #[test]
    fn test_queue_spawn_returns_live_handle_before_apply() {
        let mut world = DynWorld::new();

        let entity = world.queue_spawn((Position { x: 3.0, y: 0.0 }, Velocity::default()));
        assert!(world.is_alive(entity));
        assert!(world.get::<Position>(entity).is_none());

        world.queue_set(entity, Health { value: 50.0 });
        world.apply_commands();

        assert_eq!(world.get::<Position>(entity).unwrap().x, 3.0);
        assert_eq!(world.get::<Health>(entity).unwrap().value, 50.0);
    }

    #[test]
    fn test_entity_components_and_component_by_name() {
        let mut world = DynWorld::new();
        let entity = world.spawn((Position::default(), Velocity::default()));
        world.register::<Health>();

        let mut names: Vec<&str> = world
            .entity_components(entity)
            .map(|info| info.type_name)
            .collect();
        names.sort_unstable();
        assert_eq!(names.len(), 2);
        assert!(names[0].ends_with("Position"));
        assert!(names[1].ends_with("Velocity"));

        let position_name = std::any::type_name::<Position>();
        let info = world.component_by_name(position_name).unwrap();
        assert_eq!(info.mask, world.lookup_key::<Position>().unwrap().mask);
        assert!(world.component_by_name("no::such::Component").is_none());

        let dead = world.spawn((Position::default(),));
        world.despawn_entities(&[dead]);
        assert_eq!(world.entity_components(dead).count(), 0);
    }

    #[test]
    fn test_iter_combinations_yields_each_pair_once() {
        let mut world = DynWorld::new();
        let first = world.spawn((Position { x: 1.0, y: 0.0 },));
        let second = world.spawn((Position { x: 2.0, y: 0.0 },));
        let third = world.spawn((Position { x: 3.0, y: 0.0 },));

        let pairs: Vec<(Entity, Entity)> = world
            .query_ref::<&Position>()
            .iter_combinations()
            .map(|((entity_a, _), (entity_b, _))| (entity_a, entity_b))
            .collect();
        assert_eq!(
            pairs,
            vec![(first, second), (first, third), (second, third)]
        );

        let total: f32 = world
            .query_ref::<&Position>()
            .iter_combinations()
            .map(|((_, a), (_, b))| a.x + b.x)
            .sum();
        assert_eq!(total, 12.0);

        world.despawn_entities(&[second, third]);
        assert_eq!(
            world.query_ref::<&Position>().iter_combinations().count(),
            0
        );
    }

    #[test]
    fn test_single_matches_exactly_one() {
        struct Player;

        let mut world = DynWorld::new();
        assert!(world.query_ref::<&Position>().single().is_none());

        let player = world.spawn((Position { x: 7.0, y: 0.0 },));
        world.add_tag_type::<Player>(player);
        let (entity, position) = world.query_ref::<&Position>().single().unwrap();
        assert_eq!(entity, player);
        assert_eq!(position.x, 7.0);

        world.spawn((Position::default(),));
        assert!(world.query_ref::<&Position>().single().is_none());
        assert!(
            world
                .query_ref::<&Position>()
                .with_tag_type::<Player>()
                .single()
                .is_some()
        );
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    #[cfg(not(feature = "raw_storage"))]
    fn test_typed_par_for_each_matches_sequential() {
        let mut world = DynWorld::new();
        let boss = world.register_tag();
        let position = world.register::<Position>();
        let velocity = world.register::<Velocity>();
        let entities = world.spawn_entities(position.mask, 100);
        world.spawn_entities(velocity.mask | position.mask, 50);
        for &entity in entities.iter().take(10) {
            world.add_tag(boss, entity);
        }

        world.step();
        world
            .query::<(&mut Position, Option<&Velocity>)>()
            .par_for_each(|_entity, (position_value, velocity)| {
                position_value.x += 1.0 + velocity.map_or(0.0, |velocity| velocity.x);
            });

        let mut total = 0.0;
        world
            .query_ref::<&Position>()
            .iter()
            .for_each(|(_entity, position_value)| total += position_value.x);
        assert_eq!(total, 150.0);

        assert_eq!(
            world.query_entities_changed(position.mask).count(),
            150,
            "parallel mutable elements stamp change ticks"
        );

        world.step();
        let counted = std::sync::atomic::AtomicUsize::new(0);
        world
            .query::<&Position>()
            .with_tag(boss)
            .par_for_each(|_entity, _position| {
                counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            });
        assert_eq!(counted.load(std::sync::atomic::Ordering::Relaxed), 10);
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    #[cfg(not(feature = "raw_storage"))]
    fn test_typed_par_for_each_added_filter() {
        let mut world = DynWorld::new();
        world.spawn((Position::default(),));
        world.step();
        let fresh = world.spawn((Position::default(),));

        let seen = std::sync::Mutex::new(Vec::new());
        world
            .query::<&Position>()
            .added::<Position>()
            .par_for_each(|entity, _position| {
                seen.lock().unwrap().push(entity);
            });
        assert_eq!(*seen.lock().unwrap(), vec![fresh]);
    }

    #[test]
    fn test_hierarchy_index_links_relinks_and_unlinks() {
        let mut world = DynWorld::new();
        let mut index = HierarchyIndex::new();

        let parent_a = world.spawn((Position::default(),));
        let parent_b = world.spawn((Position::default(),));
        let child = world.spawn((Position::default(), ChildOf(parent_a)));

        index.sync(&mut world);
        assert_eq!(index.children(parent_a), &[child]);
        assert_eq!(index.parent_of.get(&child), Some(&parent_a));

        world.set(child, ChildOf(parent_b));
        index.sync(&mut world);
        assert!(index.children(parent_a).is_empty());
        assert_eq!(index.children(parent_b), &[child]);

        world.remove::<ChildOf>(child);
        index.sync(&mut world);
        assert!(index.children(parent_b).is_empty());
        assert!(index.parent_of.is_empty());

        world.set(child, ChildOf(parent_b));
        index.sync(&mut world);
        world.despawn_entities(&[child]);
        index.sync(&mut world);
        assert!(index.children(parent_b).is_empty());

        index.sync(&mut world);
        assert!(index.parent_of.is_empty(), "an idle sync changes nothing");
    }

    #[test]
    fn test_hierarchy_index_matches_scan_oracle() {
        for seed in [11u64, 1111, 111111] {
            let mut rng = Lcg(seed);
            let mut world = DynWorld::new();
            let mut index = HierarchyIndex::new();
            let mut handles: Vec<Entity> = Vec::new();

            for _ in 0..1200 {
                match rng.next() % 10 {
                    0..=2 => {
                        handles.push(world.spawn((Position::default(),)));
                    }
                    3..=4 => {
                        if handles.len() >= 2 {
                            let child = handles[rng.next() as usize % handles.len()];
                            let parent = handles[rng.next() as usize % handles.len()];
                            if child != parent {
                                world.set(child, ChildOf(parent));
                            }
                        }
                    }
                    5 => {
                        if !handles.is_empty() {
                            let entity = handles[rng.next() as usize % handles.len()];
                            world.remove::<ChildOf>(entity);
                        }
                    }
                    6 => {
                        if !handles.is_empty() {
                            let victim = handles.remove(rng.next() as usize % handles.len());
                            world.despawn_entities(&[victim]);
                        }
                    }
                    _ => {
                        index.sync(&mut world);
                    }
                }
            }

            index.sync(&mut world);
            for &parent in &handles {
                let mut indexed: Vec<Entity> = index.children(parent).to_vec();
                let mut scanned = world.children(parent);
                indexed.sort_unstable_by_key(|entity| entity.id);
                scanned.sort_unstable_by_key(|entity| entity.id);
                assert_eq!(
                    indexed, scanned,
                    "index diverged from scan with seed {seed}"
                );
            }
            for (&child, &parent) in &index.parent_of {
                assert_eq!(
                    world.get::<ChildOf>(child).map(|link| link.0),
                    Some(parent),
                    "stale upward link with seed {seed}"
                );
            }
        }
    }

    #[test]
    #[cfg(not(feature = "raw_storage"))]
    fn test_hierarchy_index_raw_writes_need_mark_changed() {
        let mut world = DynWorld::new();
        let mut index = HierarchyIndex::new();

        let parent_a = world.spawn((Position::default(),));
        let parent_b = world.spawn((Position::default(),));
        let child = world.spawn((ChildOf(parent_a),));
        index.sync(&mut world);

        let child_of = world.register::<ChildOf>();
        world.for_each_tables_mut(child_of.mask, 0, |table| {
            for link in table.column_mut(child_of) {
                link.0 = parent_b;
            }
        });
        index.sync(&mut world);
        assert_eq!(
            index.children(parent_a),
            &[child],
            "an unstamped raw write is invisible by covenant"
        );

        world.mark_changed(child, child_of.mask);
        index.sync(&mut world);
        assert_eq!(index.children(parent_b), &[child]);
    }

    #[test]
    fn test_hierarchy_index_despawn_recursive() {
        let mut world = DynWorld::new();
        let mut index = HierarchyIndex::new();

        let root = world.spawn((Position::default(),));
        let child = world.spawn((ChildOf(root),));
        let grandchild = world.spawn((ChildOf(child),));
        let bystander = world.spawn((Position::default(),));
        index.sync(&mut world);

        assert_eq!(index.descendants(root), vec![child, grandchild]);
        let despawned = index.despawn_recursive(&mut world, root);
        assert_eq!(despawned.len(), 3);
        assert!(index.children(root).is_empty());
        assert!(index.parent_of.is_empty());
        assert!(world.is_alive(bystander));

        index.sync(&mut world);
        assert!(index.parent_of.is_empty());
    }

    #[test]
    fn test_despawn_recursive_follows_child_links() {
        let mut world = DynWorld::new();
        let root = world.spawn((Position::default(),));
        let child = world.spawn((Position::default(), ChildOf(root)));
        let grandchild = world.spawn((Position::default(), ChildOf(child)));
        let bystander = world.spawn((Position::default(),));

        assert_eq!(world.children(root), vec![child]);
        assert_eq!(world.children(child), vec![grandchild]);

        let despawned = world.despawn_recursive(root);
        assert_eq!(despawned.len(), 3);
        assert!(!world.is_alive(root));
        assert!(!world.is_alive(child));
        assert!(!world.is_alive(grandchild));
        assert!(world.is_alive(bystander));

        let cycle_a = world.spawn((ChildOf(bystander),));
        let cycle_b = world.spawn((ChildOf(cycle_a),));
        world.set(cycle_a, ChildOf(cycle_b));
        let despawned = world.despawn_recursive(cycle_a);
        assert_eq!(despawned.len(), 2, "link cycles must terminate");
        assert!(world.is_alive(bystander));
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
    fn test_resources_scope_takes_tuple_and_restores() {
        struct DeltaTime(f32);
        struct Score(u32);

        let mut world = DynWorld::new();
        world.insert_resource(DeltaTime(0.25));
        world.insert_resource(Score(10));

        let spawned =
            world.resources_scope(|world, (delta_time, score): &mut (DeltaTime, Score)| {
                assert!(world.resource::<DeltaTime>().is_none());
                assert!(world.resource::<Score>().is_none());
                delta_time.0 *= 2.0;
                score.0 += 1;
                world.spawn((Position { x: 3.0, y: 0.0 },))
            });

        assert_eq!(world.resource::<DeltaTime>().unwrap().0, 0.5);
        assert_eq!(world.resource::<Score>().unwrap().0, 11);
        assert_eq!(world.get::<Position>(spawned).unwrap().x, 3.0);
    }

    #[test]
    fn test_resources_scope_preserves_tuple_on_panic() {
        struct DeltaTime(f32);
        struct Score(u32);

        let mut world = DynWorld::new();
        world.insert_resource(DeltaTime(0.25));
        world.insert_resource(Score(10));

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            world.resources_scope(|_world, (_delta_time, _score): &mut (DeltaTime, Score)| {
                panic!("boom")
            });
        }));

        assert!(result.is_err());
        assert_eq!(world.resource::<DeltaTime>().unwrap().0, 0.25);
        assert_eq!(world.resource::<Score>().unwrap().0, 10);
    }

    #[test]
    #[should_panic(expected = "resources_scope requires")]
    fn test_resources_scope_missing_resource_leaves_world_untouched() {
        struct Present;
        struct Missing;

        let mut world = DynWorld::new();
        world.insert_resource(Present);

        world.resources_scope(|_world, (_present, _missing): &mut (Present, Missing)| {});
    }

    #[test]
    fn test_resources_scope_failed_take_removes_nothing() {
        struct Present(u32);
        struct Missing;

        let mut world = DynWorld::new();
        world.insert_resource(Present(1));

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            world.resources_scope(|_world, (_missing, _present): &mut (Missing, Present)| {});
        }));

        assert!(result.is_err());
        assert_eq!(world.resource::<Present>().unwrap().0, 1);
    }

    #[test]
    #[should_panic(expected = "must not repeat a resource type")]
    fn test_resources_scope_rejects_repeated_type() {
        struct Score;

        let mut world = DynWorld::new();
        world.insert_resource(Score);

        world.resources_scope(|_world, (_first, _second): &mut (Score, Score)| {});
    }

    #[test]
    #[should_panic(expected = "resource_scope requires")]
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
                        #[cfg(not(feature = "raw_storage"))]
                        {
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
                        }

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
    #[cfg(not(feature = "raw_storage"))]
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
    #[cfg(not(feature = "raw_storage"))]
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
    #[cfg(not(feature = "raw_storage"))]
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
    #[cfg(not(feature = "raw_storage"))]
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
    #[cfg(not(feature = "raw_storage"))]
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

    #[cfg(all(feature = "snapshot", not(feature = "raw_storage")))]
    #[test]
    fn test_component_values_by_name() {
        let mut registry = ComponentRegistry::new();
        registry.register_serde::<Position>();
        registry.register::<Velocity>();
        let mut world = DynWorld::from_registry(registry);

        let entity = world.spawn((Position { x: 1.0, y: 2.0 },));
        let name = std::any::type_name::<Position>();

        let bytes = world.get_component_by_name(entity, name).unwrap().unwrap();
        let decoded: Position = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, Position { x: 1.0, y: 2.0 });

        world.step();
        let replacement = postcard::to_allocvec(&Position { x: 7.0, y: 8.0 }).unwrap();
        world
            .set_component_by_name(entity, name, &replacement)
            .unwrap();
        assert_eq!(world.get::<Position>(entity).unwrap().x, 7.0);
        assert_eq!(
            world
                .query_entities_changed(world.lookup_key::<Position>().unwrap().mask)
                .count(),
            1,
            "value writes stamp change ticks like any set"
        );

        let bare = world.spawn((Velocity::default(),));
        world
            .set_component_by_name(bare, name, &replacement)
            .unwrap();
        assert_eq!(
            world.get::<Position>(bare).unwrap().x,
            7.0,
            "value writes add the component when absent"
        );

        assert!(matches!(
            world.get_component_by_name(entity, "no::such::Type"),
            Err(SnapshotError::UnknownComponent(_))
        ));
        assert!(matches!(
            world.set_component_by_name(entity, std::any::type_name::<Velocity>(), &[]),
            Err(SnapshotError::MissingCodec(_))
        ));
        let rowless = world.spawn((Velocity::default(),));
        assert_eq!(
            world.get_component_by_name(rowless, name).unwrap(),
            None,
            "an entity without the component reads as None"
        );

        let mut game_registry = ComponentRegistry::new();
        game_registry.register_serde::<Health>();
        let mut ecs = DynEcs::new();
        let mut core_registry = ComponentRegistry::new();
        core_registry.register_serde::<Position>();
        ecs.add_world_at(0, core_registry);
        ecs.add_world_at(1, game_registry);
        let grouped = ecs.spawn_with((Position::default(), Health { value: 3.0 }));
        let health_name = std::any::type_name::<Health>();
        let health_bytes = postcard::to_allocvec(&Health { value: 42.0 }).unwrap();
        ecs.set_component_by_name(grouped, health_name, &health_bytes)
            .unwrap();
        assert_eq!(ecs.get::<Health>(grouped).unwrap().value, 42.0);
        assert!(
            ecs.get_component_by_name(grouped, health_name)
                .unwrap()
                .is_some(),
            "group value reads route by name"
        );
    }

    #[cfg(feature = "snapshot")]
    #[test]
    fn test_dyn_ecs_marker_tags_survive_snapshots() {
        struct Selected;

        let mut registry = ComponentRegistry::new();
        registry.register_serde::<Position>();
        let mut ecs = DynEcs::new();
        ecs.add_world_at(0, registry);

        let first = ecs.spawn_with((Position::default(),));
        let second = ecs.spawn_with((Position::default(),));
        ecs.add_tag_type::<Selected>(first);
        let anonymous = ecs.register_tag();
        ecs.add_tag(anonymous, second);

        let snapshot = ecs.snapshot().unwrap();
        let mut registry = ComponentRegistry::new();
        registry.register_serde::<Position>();
        let mut restored = DynEcs::from_snapshot(vec![registry], &snapshot).unwrap();

        assert!(
            restored.has_tag_type::<Selected>(first),
            "marker membership resolves after a restore"
        );
        assert_eq!(
            restored.query_tag_type::<Selected>().collect::<Vec<_>>(),
            vec![first]
        );
        assert!(restored.tag_set_type::<Selected>().is_some());

        let set_count = restored.tags.len();
        restored.add_tag_type::<Selected>(second);
        assert_eq!(
            restored.tags.len(),
            set_count,
            "adding after a restore reuses the persisted set instead of splitting membership"
        );
        let mut members: Vec<Entity> = restored.query_tag_type::<Selected>().collect();
        members.sort_unstable_by_key(|entity| entity.id);
        assert_eq!(members, vec![first, second]);
        assert!(restored.has_tag(anonymous, second));
    }

    #[test]
    #[should_panic(expected = "must live in exactly one member world")]
    fn test_dyn_ecs_add_world_rejects_duplicate_types() {
        let mut first = ComponentRegistry::new();
        first.register::<Position>();
        let mut second = ComponentRegistry::new();
        second.register::<Position>();

        let mut ecs = DynEcs::new();
        ecs.add_world(first);
        ecs.add_world(second);
    }

    #[test]
    #[should_panic(expected = "must live in exactly one member world")]
    fn test_dyn_ecs_route_detects_post_add_duplicates() {
        let mut ecs = DynEcs::new();
        ecs.add_world(ComponentRegistry::new());
        ecs.add_world(ComponentRegistry::new());
        ecs.worlds[0].register::<Position>();
        ecs.worlds[1].register::<Position>();

        let entity = ecs.spawn();
        let _ = ecs.get::<Position>(entity);
    }

    #[test]
    #[should_panic(expected = "must name components present in the query tuple")]
    fn test_query_join_filters_must_name_tuple_components() {
        let mut core_registry = ComponentRegistry::new();
        core_registry.register::<Position>();
        core_registry.register::<Velocity>();
        let mut game_registry = ComponentRegistry::new();
        game_registry.register::<Health>();

        let mut ecs = DynEcs::new();
        ecs.add_world_at(0, core_registry);
        ecs.add_world_at(1, game_registry);
        ecs.spawn_with((Position::default(), Health::default()));

        ecs.query_join::<(&mut Position, &Health)>()
            .changed::<Velocity>()
            .for_each(|_entity, (_position, _health)| {});
    }

    #[test]
    fn test_query_join_changed_accepts_lazily_registered_elements() {
        let mut core_registry = ComponentRegistry::new();
        core_registry.register::<Position>();

        let mut ecs = DynEcs::new();
        ecs.add_world_at(0, core_registry);
        ecs.spawn_with((Position::default(),));

        let mut visited = 0;
        ecs.query_join::<(&mut Position, Option<&mut Velocity>)>()
            .changed::<Velocity>()
            .for_each(|_entity, (_position, _velocity)| visited += 1);
        assert_eq!(
            visited, 0,
            "a lazily-registered tuple element is a valid filter target and never panics"
        );
    }

    #[cfg(feature = "snapshot")]
    #[test]
    fn test_set_component_by_name_rejects_dead_entities() {
        let mut registry = ComponentRegistry::new();
        registry.register_serde::<Position>();
        let mut world = DynWorld::from_registry(registry);
        let entity = world.spawn((Position::default(),));
        world.despawn_entities(&[entity]);

        let name = std::any::type_name::<Position>();
        let bytes = postcard::to_allocvec(&Position::default()).unwrap();
        assert!(matches!(
            world.set_component_by_name(entity, name, &bytes),
            Err(SnapshotError::DeadEntity)
        ));

        let mut registry = ComponentRegistry::new();
        registry.register_serde::<Position>();
        let mut ecs = DynEcs::new();
        ecs.add_world_at(0, registry);
        let grouped = ecs.spawn_with((Position::default(),));
        ecs.despawn(grouped);
        assert!(matches!(
            ecs.set_component_by_name(grouped, name, &bytes),
            Err(SnapshotError::DeadEntity)
        ));

        let live = ecs.spawn_with((Position::default(),));
        assert!(
            ecs.set_component_by_name(live, name, &bytes).is_ok(),
            "grouped members still accept writes for live group handles"
        );
    }

    #[cfg(feature = "snapshot")]
    #[test]
    fn test_from_snapshot_rejects_length_mismatched_columns() {
        let mut registry = ComponentRegistry::new();
        registry.register_serde::<Position>();
        let mut world = DynWorld::from_registry(registry);
        world.spawn((Position::default(),));
        world.spawn((Position::default(),));

        let mut snapshot = world.snapshot().unwrap();
        let truncated: Vec<Position> = vec![Position::default()];
        snapshot.tables[0].columns[0] = postcard::to_allocvec(&truncated).unwrap();

        let mut registry = ComponentRegistry::new();
        registry.register_serde::<Position>();
        let result = DynWorld::from_snapshot(registry, &snapshot);
        assert!(
            matches!(result, Err(SnapshotError::Codec(message)) if message.contains("decoded 1 rows for 2 entities"))
        );
    }

    #[test]
    fn test_dyn_ecs_group_marker_tags() {
        struct Selected;
        struct Locked;

        let mut core_registry = ComponentRegistry::new();
        core_registry.register::<Position>();
        let mut ecs = DynEcs::new();
        ecs.add_world_at(0, core_registry);

        let first = ecs.spawn_with((Position { x: 1.0, y: 0.0 },));
        let second = ecs.spawn_with((Position { x: 2.0, y: 0.0 },));

        assert!(!ecs.has_tag_type::<Selected>(first));
        assert_eq!(ecs.query_tag_type::<Selected>().count(), 0);
        assert!(!ecs.remove_tag_type::<Selected>(first));

        ecs.clear_structural_log();
        ecs.add_tag_type::<Selected>(first);
        ecs.add_tag_type::<Locked>(second);
        assert!(ecs.has_tag_type::<Selected>(first));
        assert!(!ecs.has_tag_type::<Selected>(second));
        assert_eq!(
            ecs.query_tag_type::<Selected>().collect::<Vec<_>>(),
            vec![first]
        );
        assert_eq!(
            ecs.structural_changes_since(0)
                .iter()
                .filter(|change| change.kind == StructuralChangeKind::TagsAdded)
                .count(),
            2,
            "marker tags land in the group structural log"
        );

        let selected_positions: Vec<Entity> = ecs.worlds[0]
            .query_ref::<&Position>()
            .with_tag_set(ecs.tag_set_type::<Selected>().unwrap())
            .iter()
            .map(|(entity, _position)| entity)
            .collect();
        assert_eq!(
            selected_positions,
            vec![first],
            "group marker sets compose into per-world typed queries"
        );

        assert!(ecs.remove_tag_type::<Selected>(first));
        assert!(!ecs.has_tag_type::<Selected>(first));

        ecs.add_tag_type::<Selected>(second);
        ecs.despawn(second);
        assert!(
            !ecs.has_tag_type::<Selected>(second),
            "despawn drops group marker tags"
        );

        assert!(
            ecs.worlds[0].remaining_bits() == 63,
            "group marker tags spend no member-world mask bits"
        );
    }

    #[cfg(all(feature = "snapshot", not(feature = "raw_storage")))]
    fn assert_worlds_equivalent(left: &DynWorld, right: &DynWorld, context: &str) {
        let mut left_entities: Vec<Entity> = left
            .tables
            .iter()
            .flat_map(|table| table.entity_indices.iter().copied())
            .collect();
        let mut right_entities: Vec<Entity> = right
            .tables
            .iter()
            .flat_map(|table| table.entity_indices.iter().copied())
            .collect();
        left_entities.sort_unstable_by_key(|entity| entity.id);
        right_entities.sort_unstable_by_key(|entity| entity.id);
        assert_eq!(
            left_entities, right_entities,
            "entity sets diverge: {context}"
        );

        for &entity in &left_entities {
            assert_eq!(
                left.component_mask(entity),
                right.component_mask(entity),
                "masks diverge for {entity}: {context}"
            );
            for info in &left.registry.components {
                let name = info.type_name;
                let left_bytes = left.get_component_by_name(entity, name).unwrap();
                let right_bytes = right.get_component_by_name(entity, name).unwrap();
                assert_eq!(
                    left_bytes, right_bytes,
                    "{name} diverges for {entity}: {context}"
                );
            }
        }
        for (index, (left_set, right_set)) in left.tags.iter().zip(&right.tags).enumerate() {
            let mut left_members: Vec<Entity> = left_set.iter().collect();
            let mut right_members: Vec<Entity> = right_set.iter().collect();
            left_members.sort_unstable_by_key(|entity| entity.id);
            right_members.sort_unstable_by_key(|entity| entity.id);
            assert_eq!(
                left_members, right_members,
                "tag {index} diverges: {context}"
            );
        }
    }

    #[cfg(all(feature = "snapshot", not(feature = "raw_storage")))]
    #[test]
    fn test_world_deltas_replicate_op_stream() {
        let mut registry = ComponentRegistry::new();
        registry.register_serde::<Position>();
        registry.register_serde::<Velocity>();
        registry.register_serde::<Health>();
        let mut source = DynWorld::from_registry(registry.clone());

        let seed_a = source.spawn((Position { x: 1.0, y: 0.0 }, Health { value: 5.0 }));
        source.spawn((Velocity { x: 2.0, y: 0.0 },));

        let snapshot = source.snapshot().unwrap();
        let mut replica = DynWorld::from_snapshot(registry, &snapshot).unwrap();
        let mut cursor = source.delta_cursor();

        let mut rng = Lcg(2024);
        let mut handles = vec![seed_a];
        for round in 0..6 {
            for _ in 0..40 {
                match rng.next() % 8 {
                    0 | 1 => handles.push(source.spawn((Position {
                        x: rng.next() as f32 % 10.0,
                        y: 0.0,
                    },))),
                    2 => {
                        if let Some(&entity) = handles.last() {
                            source.set(
                                entity,
                                Health {
                                    value: (rng.next() % 100) as f32,
                                },
                            );
                        }
                    }
                    3 => {
                        if !handles.is_empty() {
                            let entity = handles[rng.next() as usize % handles.len()];
                            if let Some(position) = source.get_mut::<Position>(entity) {
                                position.x += 1.0;
                            }
                        }
                    }
                    4 => {
                        if !handles.is_empty() {
                            let entity = handles[rng.next() as usize % handles.len()];
                            source.remove::<Health>(entity);
                        }
                    }
                    5 => {
                        if !handles.is_empty() {
                            let entity = handles[rng.next() as usize % handles.len()];
                            source.add_tag_type::<u8>(entity);
                        }
                    }
                    6 => {
                        if !handles.is_empty() {
                            let entity = handles[rng.next() as usize % handles.len()];
                            source.remove_tag_type::<u8>(entity);
                        }
                    }
                    _ => {
                        if handles.len() > 2 {
                            let victim = handles.remove(rng.next() as usize % handles.len());
                            source.despawn_entities(&[victim]);
                        }
                    }
                }
            }
            let delta = source.delta_since(&cursor).unwrap();
            cursor = delta.to;
            let bytes = postcard::to_allocvec(&delta).unwrap();
            let decoded: DynWorldDelta = postcard::from_bytes(&bytes).unwrap();
            replica.apply_delta(&decoded).unwrap();
            assert_worlds_equivalent(&source, &replica, &format!("round {round}"));
        }

        let stale = handles[0];
        source.despawn_entities(&[stale]);
        let delta = source.delta_since(&cursor).unwrap();
        replica.apply_delta(&delta).unwrap();
        assert!(!replica.is_alive(stale) || replica.get::<Position>(stale).is_none());
    }

    #[cfg(all(feature = "snapshot", not(feature = "raw_storage")))]
    #[test]
    fn test_world_delta_detects_log_gaps() {
        let mut registry = ComponentRegistry::new();
        registry.register_serde::<Position>();
        let mut world = DynWorld::from_registry(registry);
        let cursor = world.delta_cursor();
        world.spawn((Position::default(),));
        world.trim_structural_log(world.structural_sequence);
        world.spawn((Position::default(),));
        world.trim_structural_log(world.structural_sequence);
        let result = world.delta_since(&cursor);
        assert!(
            matches!(result, Err(SnapshotError::Codec(message)) if message.contains("gap")),
            "a trimmed log past the cursor must be a loud gap"
        );
    }

    #[cfg(all(feature = "snapshot", not(feature = "raw_storage")))]
    #[test]
    fn test_group_deltas_replicate_across_members() {
        struct Marked;

        let mut core_registry = ComponentRegistry::new();
        core_registry.register_serde::<Position>();
        let mut game_registry = ComponentRegistry::new();
        game_registry.register_serde::<Health>();

        let mut source = DynEcs::new();
        source.add_world_at(0, core_registry.clone());
        source.add_world_at(1, game_registry.clone());
        let anchor = source.spawn_with((Position { x: 1.0, y: 0.0 }, Health { value: 9.0 }));

        let snapshot = source.snapshot().unwrap();
        let mut replica =
            DynEcs::from_snapshot(vec![core_registry, game_registry], &snapshot).unwrap();
        let mut cursor = source.delta_cursor();

        let newcomer = source.spawn_with((Position { x: 5.0, y: 0.0 }, Health { value: 1.0 }));
        source.add_tag_type::<Marked>(newcomer);
        source.get_mut::<Health>(anchor).unwrap().value = 77.0;
        let doomed = source.spawn_with((Position::default(),));
        source.despawn(doomed);

        let delta = source.delta_since(&cursor).unwrap();
        cursor = DynEcsDeltaCursor {
            group_sequence: delta.group_to,
            worlds: delta.worlds.iter().map(|world| world.to).collect(),
        };
        replica.apply_delta(&delta).unwrap();

        assert!(replica.is_alive(newcomer));
        assert!(!replica.is_alive(doomed));
        assert_eq!(replica.get::<Health>(anchor).unwrap().value, 77.0);
        assert_eq!(replica.get::<Position>(newcomer).unwrap().x, 5.0);
        assert!(replica.has_tag_type::<Marked>(newcomer));
        assert_worlds_equivalent(&source.worlds[0], &replica.worlds[0], "core");
        assert_worlds_equivalent(&source.worlds[1], &replica.worlds[1], "game");

        source.remove_tag_type::<Marked>(newcomer);
        source.get_mut::<Position>(newcomer).unwrap().x = 6.0;
        let second = source.delta_since(&cursor).unwrap();
        replica.apply_delta(&second).unwrap();
        assert!(!replica.has_tag_type::<Marked>(newcomer));
        assert_eq!(replica.get::<Position>(newcomer).unwrap().x, 6.0);
    }

    #[test]
    #[cfg(not(feature = "raw_storage"))]
    fn test_prepared_queries_match_direct_queries() {
        let mut world = DynWorld::new();
        let moving = world.spawn((Position { x: 1.0, y: 0.0 }, Velocity { x: 1.0, y: 0.0 }));
        world.spawn((Position { x: 2.0, y: 0.0 },));

        let prepared = world
            .query::<(&mut Position, &Velocity)>()
            .changed::<Position>()
            .prepare();
        let prepared_read = world.query_ref::<&Position>().prepare();

        world.step();
        world.get_mut::<Position>(moving).unwrap().x = 5.0;

        let mut visited = Vec::new();
        prepared
            .query(&mut world)
            .for_each(|entity, (position, velocity)| {
                position.x += velocity.x;
                visited.push(entity);
            });
        assert_eq!(visited, vec![moving]);
        assert_eq!(world.get::<Position>(moving).unwrap().x, 6.0);

        let total = prepared_read.query(&world).iter().count();
        assert_eq!(total, 2);

        let again = prepared_read.query(&world).iter().count();
        assert_eq!(again, 2, "prepared queries rerun without re-resolving");
    }

    #[test]
    fn test_query_join_ref_iterates_across_worlds() {
        struct Chosen;

        let mut core_registry = ComponentRegistry::new();
        core_registry.register::<Position>();
        let mut game_registry = ComponentRegistry::new();
        game_registry.register::<Health>();

        let mut ecs = DynEcs::new();
        ecs.add_world_at(0, core_registry);
        ecs.add_world_at(1, game_registry);

        let both = ecs.spawn_with((Position { x: 1.0, y: 0.0 }, Health { value: 10.0 }));
        ecs.spawn_with((Position { x: 2.0, y: 0.0 },));
        let tagged = ecs.spawn_with((Position { x: 3.0, y: 0.0 }, Health { value: 30.0 }));
        ecs.add_tag_type::<Chosen>(tagged);

        let pairs: Vec<(Entity, f32)> = ecs
            .query_join_ref::<(&Position, &Health)>()
            .iter()
            .map(|(entity, (position, health))| (entity, position.x + health.value))
            .collect();
        assert_eq!(pairs, vec![(both, 11.0), (tagged, 33.0)]);

        let chosen: Vec<Entity> = ecs
            .query_join_ref::<(&Position, &Health)>()
            .with_tag_type::<Chosen>()
            .iter()
            .map(|(entity, _items)| entity)
            .collect();
        assert_eq!(chosen, vec![tagged]);

        assert_eq!(
            ecs.query_join_ref::<(&Position, &Velocity)>()
                .iter()
                .count(),
            0,
            "an unregistered required element degrades to empty"
        );
    }

    struct HostWorld {
        world: DynWorld,
        frames: u32,
    }

    impl ResourceHost for HostWorld {
        fn resource_map_mut(&mut self) -> &mut ResourceMap {
            &mut self.world.resources
        }
        fn resource_map(&self) -> &ResourceMap {
            &self.world.resources
        }
    }

    struct ScopePoints(u32);
    struct ScopeLabel(&'static str);

    #[test]
    fn test_host_resource_scope_lends_the_wrapper() {
        let mut host = HostWorld {
            world: DynWorld::new(),
            frames: 0,
        };
        host.world.insert_resource(ScopePoints(0));
        let entity = host.world.spawn((Position::default(),));

        let seen = host.resource_scope(|host, points: &mut ScopePoints| {
            host.frames += 1;
            host.world.get_mut::<Position>(entity).unwrap().x = 2.0;
            points.0 += 5;
            points.0
        });

        assert_eq!(seen, 5);
        assert_eq!(host.frames, 1);
        assert_eq!(host.world.get::<Position>(entity).unwrap().x, 2.0);
        assert_eq!(host.world.resource::<ScopePoints>().unwrap().0, 5);
    }

    #[test]
    fn test_host_resource_scope_reinserts_on_panic() {
        let mut host = HostWorld {
            world: DynWorld::new(),
            frames: 0,
        };
        host.world.insert_resource(ScopePoints(3));

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            host.resource_scope(|_host, points: &mut ScopePoints| {
                points.0 += 1;
                panic!("scope body panics");
            });
        }));

        assert!(outcome.is_err());
        assert_eq!(host.world.resource::<ScopePoints>().unwrap().0, 4);
    }

    struct AlternatingHost {
        primary: ResourceMap,
        secondary: ResourceMap,
        calls: u32,
    }

    impl ResourceHost for AlternatingHost {
        fn resource_map_mut(&mut self) -> &mut ResourceMap {
            self.calls += 1;
            if self.calls % 2 == 1 {
                &mut self.primary
            } else {
                &mut self.secondary
            }
        }
        fn resource_map(&self) -> &ResourceMap {
            &self.primary
        }
    }

    #[test]
    #[should_panic(expected = "different map")]
    fn test_host_returning_different_maps_fails_loudly() {
        let mut host = AlternatingHost {
            primary: ResourceMap::default(),
            secondary: ResourceMap::default(),
            calls: 0,
        };
        host.primary.insert(ScopePoints(1));
        host.resource_scope(|_host, _points: &mut ScopePoints| {});
    }

    #[test]
    fn test_host_resources_scope_takes_the_tuple() {
        let mut host = HostWorld {
            world: DynWorld::new(),
            frames: 0,
        };
        host.world.insert_resource(ScopePoints(1));
        host.world.insert_resource(ScopeLabel("group"));

        host.resources_scope(|host, (points, label): &mut (ScopePoints, ScopeLabel)| {
            host.frames += 1;
            points.0 += 1;
            assert_eq!(label.0, "group");
        });

        assert_eq!(host.frames, 1);
        assert_eq!(host.world.resource::<ScopePoints>().unwrap().0, 2);
        assert!(host.world.resource::<ScopeLabel>().is_some());
    }

    #[test]
    #[cfg(not(feature = "raw_storage"))]
    fn test_query_join_ref_filters_and_windows() {
        let mut core_registry = ComponentRegistry::new();
        core_registry.register::<Position>();
        let mut game_registry = ComponentRegistry::new();
        game_registry.register::<Health>();

        let mut ecs = DynEcs::new();
        ecs.add_world_at(0, core_registry);
        ecs.add_world_at(1, game_registry);
        let still = ecs.spawn_with((Position { x: 1.0, y: 0.0 }, Health { value: 1.0 }));
        let moved = ecs.spawn_with((Position { x: 2.0, y: 0.0 }, Health { value: 2.0 }));

        ecs.worlds[0].step();
        ecs.get_mut::<Position>(moved).unwrap().x = 9.0;
        let changed: Vec<Entity> = ecs
            .query_join_ref::<(&Position, &Health)>()
            .changed::<Position>()
            .iter()
            .map(|(entity, _items)| entity)
            .collect();
        assert_eq!(changed, vec![moved]);

        ecs.worlds[0].step();
        let fresh = ecs.spawn_with((Position { x: 3.0, y: 0.0 }, Health { value: 3.0 }));
        let appeared: Vec<Entity> = ecs
            .query_join_ref::<(&Position, &Health)>()
            .added::<Position>()
            .iter()
            .map(|(entity, _items)| entity)
            .collect();
        assert_eq!(appeared, vec![fresh]);

        assert_eq!(
            ecs.query_join_ref::<(&Position, &Health)>()
                .changed::<Velocity>()
                .iter()
                .count(),
            0,
            "an unregistered filter type degrades the read join to empty"
        );
        let _ = still;
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn test_prepared_query_runs_parallel() {
        let mut world = DynWorld::new();
        world.spawn_bundles((Position::default(), Velocity { x: 1.0, y: 0.0 }), 64);
        let prepared = world.query::<(&mut Position, &Velocity)>().prepare();

        prepared
            .query(&mut world)
            .par_for_each(|_entity, (position, velocity)| {
                position.x += velocity.x;
            });
        let total: f32 = world
            .query_ref::<&Position>()
            .iter()
            .map(|(_entity, position)| position.x)
            .sum();
        assert_eq!(total, 64.0);
    }

    #[cfg(all(feature = "snapshot", not(feature = "raw_storage")))]
    #[test]
    fn test_group_delta_serializes_and_group_compact_runs() {
        let mut core_registry = ComponentRegistry::new();
        core_registry.register_serde::<Position>();
        core_registry.register_serde::<Velocity>();
        let mut source = DynEcs::new();
        source.add_world_at(0, core_registry.clone());
        let seed = source.spawn_with((Position::default(),));

        let snapshot = source.snapshot().unwrap();
        let mut replica = DynEcs::from_snapshot(vec![core_registry], &snapshot).unwrap();
        let cursor = source.delta_cursor();

        source.get_mut::<Position>(seed).unwrap().x = 4.0;
        let doomed = source.spawn_with((Position { x: 9.0, y: 0.0 }, Velocity::default()));
        source.despawn(doomed);

        let delta = source.delta_since(&cursor).unwrap();
        let bytes = postcard::to_allocvec(&delta).unwrap();
        let decoded: DynEcsDelta = postcard::from_bytes(&bytes).unwrap();
        replica.apply_delta(&decoded).unwrap();
        assert_eq!(replica.get::<Position>(seed).unwrap().x, 4.0);
        assert!(!replica.is_alive(doomed));

        assert!(
            source.worlds[0].stats().empty_table_count >= 1,
            "the despawned spawn's table sits empty"
        );
        let dropped = source.compact();
        assert!(dropped >= 1);
        assert_eq!(source.get::<Position>(seed).unwrap().x, 4.0);
        assert_eq!(source.stats().free_ids, 1);
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    #[cfg(not(feature = "raw_storage"))]
    fn test_query_join_par_for_each_matches_sequential() {
        let mut core_registry = ComponentRegistry::new();
        core_registry.register::<Position>();
        let mut game_registry = ComponentRegistry::new();
        game_registry.register::<Health>();

        let mut ecs = DynEcs::new();
        ecs.add_world_at(0, core_registry);
        ecs.add_world_at(1, game_registry);
        for index in 0..50 {
            let entity = ecs.spawn_with((Position {
                x: index as f32,
                y: 0.0,
            },));
            if index % 2 == 0 {
                ecs.set(entity, Health { value: 1.0 });
            }
        }

        ecs.worlds[0].step();
        ecs.query_join::<(&mut Position, &Health)>()
            .par_for_each(|_entity, (position, health)| {
                position.y += health.value;
            });

        let mut lifted = 0;
        ecs.query_join::<(&Position, &Health)>()
            .for_each(|_entity, (position, _health)| {
                if position.y == 1.0 {
                    lifted += 1;
                }
            });
        assert_eq!(lifted, 25);
        assert_eq!(
            ecs.worlds[0]
                .query_entities_changed(ecs.worlds[0].lookup_key::<Position>().unwrap().mask)
                .count(),
            25,
            "parallel join mutation stamps driver ticks"
        );
    }

    #[test]
    fn test_stats_and_compact() {
        let mut world = DynWorld::new();
        let mover = world.spawn((Position::default(), Velocity::default()));
        world.spawn((Position::default(),));
        world.insert_resource(7u32);
        world.send(3u8);

        let stats = world.stats();
        assert_eq!(stats.entity_count, 2);
        assert_eq!(stats.table_count, 2);
        assert_eq!(stats.empty_table_count, 0);
        assert_eq!(stats.component_count, 2);
        assert_eq!(stats.resource_count, 1);
        assert_eq!(stats.event_channels, 1);

        world.remove::<Velocity>(mover);
        assert_eq!(world.stats().empty_table_count, 1);

        let dropped = world.compact();
        assert_eq!(dropped, 1);
        assert_eq!(world.stats().table_count, 1);
        assert_eq!(world.get::<Position>(mover).unwrap().x, 0.0);
        world.set(mover, Velocity { x: 4.0, y: 0.0 });
        assert_eq!(world.get::<Velocity>(mover).unwrap().x, 4.0);
        assert_eq!(world.compact(), 0);

        let mut ecs = DynEcs::new();
        ecs.add_world(ComponentRegistry::new());
        let entity = ecs.spawn();
        ecs.add_tag_type::<u16>(entity);
        let group_stats = ecs.stats();
        assert_eq!(group_stats.live_entities, 1);
        assert_eq!(group_stats.group_tag_count, 1);
        assert_eq!(group_stats.worlds.len(), 1);
    }

    #[test]
    #[cfg(not(feature = "raw_storage"))]
    fn test_query_join_filters() {
        struct Cursed;

        let mut core_registry = ComponentRegistry::new();
        core_registry.register::<Position>();
        let mut game_registry = ComponentRegistry::new();
        game_registry.register::<Health>();

        let mut ecs = DynEcs::new();
        ecs.add_world_at(0, core_registry);
        ecs.add_world_at(1, game_registry);

        let cursed = ecs.spawn_with((Position { x: 1.0, y: 0.0 }, Health { value: 1.0 }));
        let plain = ecs.spawn_with((Position { x: 2.0, y: 0.0 }, Health { value: 2.0 }));
        ecs.add_tag_type::<Cursed>(cursed);

        let mut tagged = Vec::new();
        ecs.query_join::<(&Position, &Health)>()
            .with_tag_type::<Cursed>()
            .for_each(|entity, (_position, _health)| tagged.push(entity));
        assert_eq!(tagged, vec![cursed]);

        let mut untagged = Vec::new();
        ecs.query_join::<(&Position, &Health)>()
            .without_tag_type::<Cursed>()
            .for_each(|entity, (_position, _health)| untagged.push(entity));
        assert_eq!(untagged, vec![plain]);

        struct NeverUsed;
        let mut none = Vec::new();
        ecs.query_join::<(&Position, &Health)>()
            .with_tag_type::<NeverUsed>()
            .for_each(|entity, (_position, _health)| none.push(entity));
        assert!(none.is_empty(), "an unused include tag matches nothing");

        ecs.worlds[0].step();
        ecs.get_mut::<Position>(plain).unwrap().x = 9.0;
        let mut changed = Vec::new();
        ecs.query_join::<(&mut Position, &Health)>()
            .changed::<Position>()
            .for_each(|entity, (_position, _health)| changed.push(entity));
        assert_eq!(changed, vec![plain]);

        ecs.worlds[0].step();
        let fresh = ecs.spawn_with((Position { x: 5.0, y: 0.0 }, Health { value: 5.0 }));
        let mut appeared = Vec::new();
        ecs.query_join::<(&Position, &Health)>()
            .added::<Position>()
            .for_each(|entity, (_position, _health)| appeared.push(entity));
        assert_eq!(appeared, vec![fresh]);
    }

    #[test]
    #[should_panic(expected = "must name driver-world components")]
    fn test_query_join_changed_rejects_foreign_types() {
        let mut core_registry = ComponentRegistry::new();
        core_registry.register::<Position>();
        let mut game_registry = ComponentRegistry::new();
        game_registry.register::<Health>();

        let mut ecs = DynEcs::new();
        ecs.add_world_at(0, core_registry);
        ecs.add_world_at(1, game_registry);

        ecs.query_join::<(&mut Position, &Health)>()
            .changed::<Health>()
            .for_each(|_entity, (_position, _health)| {});
    }

    #[test]
    #[cfg(not(feature = "raw_storage"))]
    fn test_query_join_spans_member_worlds() {
        let mut core_registry = ComponentRegistry::new();
        core_registry.register::<Position>();
        let mut game_registry = ComponentRegistry::new();
        game_registry.register::<Health>();
        game_registry.register::<Velocity>();

        let mut ecs = DynEcs::new();
        ecs.add_world_at(0, core_registry);
        ecs.add_world_at(1, game_registry);

        let both = ecs.spawn_with((Position { x: 1.0, y: 0.0 }, Health { value: 10.0 }));
        let position_only = ecs.spawn_with((Position { x: 2.0, y: 0.0 },));
        let all_three = ecs.spawn_with((
            Position { x: 3.0, y: 0.0 },
            Health { value: 30.0 },
            Velocity { x: 9.0, y: 0.0 },
        ));
        ecs.spawn_with((Health { value: 99.0 },));

        ecs.worlds[0].step();
        let mut visited = Vec::new();
        ecs.query_join::<(&mut Position, &Health, Option<&Velocity>)>()
            .for_each(|entity, (position, health, velocity)| {
                position.x += health.value + velocity.map_or(0.0, |velocity| velocity.x);
                visited.push(entity);
            });
        assert_eq!(visited, vec![both, all_three]);
        assert_eq!(ecs.get::<Position>(both).unwrap().x, 11.0);
        assert_eq!(ecs.get::<Position>(all_three).unwrap().x, 42.0);
        assert_eq!(ecs.get::<Position>(position_only).unwrap().x, 2.0);
        assert_eq!(
            ecs.worlds[0]
                .query_entities_changed(ecs.worlds[0].lookup_key::<Position>().unwrap().mask)
                .count(),
            2,
            "join mutation stamps driver-world ticks per visited row"
        );

        let manual: Vec<(Entity, f32)> = {
            let mut pairs = Vec::new();
            for (entity, position) in ecs.worlds[0].query_ref::<&Position>().iter() {
                if let Some(health) = ecs.worlds[1].get::<Health>(entity) {
                    pairs.push((entity, position.x + health.value));
                }
            }
            pairs
        };
        let mut joined: Vec<(Entity, f32)> = Vec::new();
        ecs.query_join::<(&Position, &Health)>()
            .for_each(|entity, (position, health)| {
                joined.push((entity, position.x + health.value));
            });
        assert_eq!(
            joined, manual,
            "the join matches the hand-rolled routing loop"
        );

        let mut shared_only = Vec::new();
        ecs.query_join::<(&Health, &Position)>()
            .for_each(|entity, (_health, _position)| shared_only.push(entity));
        assert_eq!(
            shared_only.len(),
            2,
            "all-shared joins pick a driver themselves"
        );

        let mut degenerate = Vec::new();
        ecs.query_join::<(&Health, &Velocity)>()
            .for_each(|entity, (_health, _velocity)| degenerate.push(entity));
        assert_eq!(
            degenerate,
            vec![all_three],
            "a single-world tuple degenerates to a plain scan"
        );
    }

    #[test]
    #[should_panic(expected = "mutate only one world's components")]
    fn test_query_join_rejects_mutable_elements_in_two_worlds() {
        let mut core_registry = ComponentRegistry::new();
        core_registry.register::<Position>();
        let mut game_registry = ComponentRegistry::new();
        game_registry.register::<Health>();

        let mut ecs = DynEcs::new();
        ecs.add_world_at(0, core_registry);
        ecs.add_world_at(1, game_registry);

        ecs.query_join::<(&mut Position, &mut Health)>()
            .for_each(|_entity, (_position, _health)| {});
    }

    #[test]
    #[should_panic(expected = "not registered in any member world")]
    fn test_query_join_rejects_unregistered_required_types() {
        let mut ecs = DynEcs::new();
        ecs.add_world(ComponentRegistry::new());
        ecs.query_join::<(&Position,)>()
            .for_each(|_entity, (_position,)| {});
    }

    #[test]
    fn test_dyn_ecs_group_events_are_exactly_once_with_two_frame_expiry() {
        let mut ecs = DynEcs::new();
        ecs.add_world(ComponentRegistry::new());
        ecs.add_world(ComponentRegistry::new());

        ecs.send(42u32);
        let mut cursor = 0;
        assert_eq!(ecs.consume_events::<u32>(&mut cursor), &[42]);
        assert!(ecs.consume_events::<u32>(&mut cursor).is_empty());
        assert_eq!(
            ecs.read_events::<u32>(),
            &[42],
            "read_events re-reads inside the buffer window"
        );

        let core_tick = ecs.worlds[0].current_tick;
        ecs.step();
        assert_eq!(
            ecs.worlds[0].current_tick,
            core_tick.wrapping_add(1),
            "group step drives member ticks"
        );
        assert_eq!(ecs.read_events::<u32>(), &[42]);
        ecs.step();
        assert!(
            ecs.read_events::<u32>().is_empty(),
            "group events expire after two frames"
        );

        ecs.worlds[0].send(7u32);
        assert!(
            ecs.read_events::<u32>().is_empty(),
            "world-local channels stay separate from the group channel"
        );
    }

    #[test]
    fn test_dyn_ecs_group_resources_and_scopes() {
        struct Shared(u32);
        struct Delta(f32);

        let mut ecs = DynEcs::new();
        ecs.add_world(ComponentRegistry::new());

        assert!(ecs.resource::<Shared>().is_none());
        ecs.insert_resource(Shared(1));
        ecs.insert_resource(Delta(0.5));
        ecs.res_mut::<Shared>().0 += 1;
        assert_eq!(ecs.res::<Shared>().0, 2);

        let entity = ecs.spawn();
        ecs.resource_scope(|ecs, shared: &mut Shared| {
            shared.0 += u32::from(ecs.is_alive(entity));
        });
        assert_eq!(ecs.resource::<Shared>().unwrap().0, 3);

        ecs.resources_scope(|ecs, (shared, delta): &mut (Shared, Delta)| {
            shared.0 += u32::from(ecs.is_alive(entity)) + delta.0 as u32;
        });
        assert_eq!(ecs.resource::<Shared>().unwrap().0, 4);
        assert!(ecs.remove_resource::<Delta>().is_some());

        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ecs.resource_scope(|_ecs, _shared: &mut Shared| panic!("boom"));
        }));
        assert!(panic.is_err());
        assert_eq!(
            ecs.resource::<Shared>().unwrap().0,
            4,
            "the resource is restored even when the closure panics"
        );
    }

    #[test]
    #[cfg(not(feature = "raw_storage"))]
    fn test_dyn_ecs_routes_typed_access_to_owning_world() {
        let mut core_registry = ComponentRegistry::new();
        core_registry.register::<Position>();
        let mut game_registry = ComponentRegistry::new();
        game_registry.register::<Health>();

        let mut ecs = DynEcs::new();
        let core = ecs.add_world_at(0, core_registry);
        let game = ecs.add_world_at(1, game_registry);

        let entity = ecs.spawn_with((Position { x: 1.0, y: 0.0 }, Health { value: 10.0 }));
        assert_eq!(ecs.worlds[core].get::<Position>(entity).unwrap().x, 1.0);
        assert_eq!(ecs.worlds[game].get::<Health>(entity).unwrap().value, 10.0);
        assert!(ecs.worlds[core].get::<Health>(entity).is_none());

        assert_eq!(ecs.get::<Health>(entity).unwrap().value, 10.0);
        ecs.worlds[core].step();
        ecs.get_mut::<Position>(entity).unwrap().x = 2.0;
        ecs.set(entity, Health { value: 5.0 });
        assert!(ecs.has::<Position>(entity));
        assert_eq!(ecs.type_routes.len(), 2);

        assert_eq!(
            ecs.worlds[core].query_entities_changed(1).count(),
            1,
            "routed mutation stamps ticks in the owning world"
        );

        assert!(ecs.remove::<Health>(entity));
        assert!(!ecs.has::<Health>(entity));

        let visited: Vec<Entity> = ecs
            .query_ref::<&Position>()
            .iter()
            .map(|(entity, _position)| entity)
            .collect();
        assert_eq!(visited, vec![entity]);
        ecs.query::<&mut Position>()
            .for_each(|_entity, position| position.y = 7.0);
        assert_eq!(ecs.get::<Position>(entity).unwrap().y, 7.0);

        assert!(ecs.get::<Velocity>(entity).is_none());
        assert!(!ecs.remove::<Velocity>(entity));
        assert_eq!(
            ecs.query_ref::<&Velocity>().iter().count(),
            0,
            "unrouteable read queries are empty, not a panic"
        );
    }

    #[test]
    #[should_panic(expected = "not registered in any member world")]
    fn test_dyn_ecs_set_rejects_unregistered_types() {
        let mut ecs = DynEcs::new();
        ecs.add_world(ComponentRegistry::new());
        let entity = ecs.spawn();
        ecs.set(entity, Position::default());
    }

    #[test]
    #[should_panic(expected = "member world registered out of declaration order")]
    fn test_dyn_ecs_add_world_at_asserts_index() {
        let mut ecs = DynEcs::new();
        ecs.add_world_at(1, ComponentRegistry::new());
    }

    #[test]
    fn test_dyn_ecs_despawn_recursive_cascades_across_worlds() {
        let mut ecs = DynEcs::new();
        let core = ecs.add_world(ComponentRegistry::new());
        let render = ecs.add_world(ComponentRegistry::new());

        let root = ecs.spawn();
        ecs.worlds[core].set(root, Position::default());

        let child = ecs.spawn();
        ecs.worlds[core].set(child, ChildOf(root));

        let grandchild = ecs.spawn();
        ecs.worlds[render].set(grandchild, ChildOf(child));

        let bystander = ecs.spawn();
        ecs.worlds[core].set(bystander, Position::default());

        ecs.clear_structural_log();
        let despawned = ecs.despawn_recursive(root);

        assert_eq!(despawned.len(), 3);
        assert!(!ecs.is_alive(root));
        assert!(!ecs.is_alive(child));
        assert!(!ecs.is_alive(grandchild));
        assert!(ecs.is_alive(bystander));
        assert!(ecs.worlds[core].get::<Position>(root).is_none());
        assert!(ecs.worlds[render].get::<ChildOf>(grandchild).is_none());

        let deaths = ecs
            .structural_changes_since(0)
            .iter()
            .filter(|change| change.kind == StructuralChangeKind::Despawned)
            .count();
        assert_eq!(deaths, 3, "every cascade death lands in the lifecycle log");
    }

    #[test]
    fn test_dyn_ecs_lifecycle_log_records_handles_and_group_tags() {
        let mut ecs = DynEcs::new();
        let core = ecs.add_world(ComponentRegistry::new());
        let position = ecs.worlds[core].register::<Position>();
        let marked = ecs.register_tag();

        let solo = ecs.spawn();
        let pair = ecs.spawn_count(2);
        let rowed = ecs.spawn_entities(core, position.mask, 1)[0];

        ecs.add_tag(marked, solo);
        ecs.add_tag(marked, solo);
        assert!(ecs.remove_tag(marked, solo));
        assert!(!ecs.remove_tag(marked, solo));

        assert!(ecs.despawn(pair[0]));
        assert!(!ecs.despawn(pair[0]));

        let entries: Vec<(Entity, StructuralChangeKind, u64)> = ecs
            .structural_changes_since(0)
            .iter()
            .map(|change| (change.entity, change.kind, change.mask))
            .collect();
        assert_eq!(
            entries,
            vec![
                (solo, StructuralChangeKind::Spawned, 0),
                (pair[0], StructuralChangeKind::Spawned, 0),
                (pair[1], StructuralChangeKind::Spawned, 0),
                (rowed, StructuralChangeKind::Spawned, 0),
                (solo, StructuralChangeKind::TagsAdded, marked as u64),
                (solo, StructuralChangeKind::TagsRemoved, marked as u64),
                (pair[0], StructuralChangeKind::Despawned, 0),
            ]
        );

        let sequences: Vec<u64> = ecs
            .structural_changes_since(0)
            .iter()
            .map(|change| change.sequence)
            .collect();
        assert_eq!(sequences, (1..=7).collect::<Vec<u64>>());

        let cursor = sequences[3];
        assert_eq!(ecs.structural_changes_since(cursor).len(), 3);
        ecs.trim_structural_log(cursor);
        assert_eq!(ecs.structural_changes_since(0).len(), 3);
        assert_eq!(ecs.structural_sequence(), 7);

        ecs.clear_structural_log();
        assert!(ecs.structural_changes_since(0).is_empty());
        assert_eq!(ecs.structural_sequence(), 7);
    }

    #[test]
    fn test_dyn_ecs_lifecycle_log_capacity_backstop() {
        let mut ecs = DynEcs::new();
        let entity = ecs.spawn();
        let marked = ecs.register_tag();

        for _ in 0..(STRUCTURAL_LOG_CAPACITY / 2) {
            ecs.add_tag(marked, entity);
            ecs.remove_tag(marked, entity);
        }

        assert!(ecs.structural_log.len() < STRUCTURAL_LOG_CAPACITY);
        assert_eq!(
            ecs.structural_sequence(),
            STRUCTURAL_LOG_CAPACITY as u64 + 1
        );
        let tail = ecs.structural_changes_since(0);
        assert_eq!(tail.last().unwrap().sequence, ecs.structural_sequence());
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
                                #[cfg(not(feature = "raw_storage"))]
                                {
                                    let static_changed: std::collections::HashSet<Entity> =
                                        static_ecs
                                            .static_core
                                            .query_entities_changed(GROUP_POSITION)
                                            .collect();
                                    let dyn_changed: std::collections::HashSet<Entity> = dyn_ecs
                                        .worlds[core]
                                        .query_entities_changed(position.mask)
                                        .collect();
                                    assert_eq!(
                                        static_changed, dyn_changed,
                                        "core changed sets diverged with seed {seed}"
                                    );
                                }

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

                    let static_lifecycle: Vec<(u64, Entity, StructuralChangeKind)> = static_ecs
                        .structural_changes_since(0)
                        .iter()
                        .map(|change| (change.sequence, change.entity, change.kind))
                        .collect();
                    let dyn_lifecycle: Vec<(u64, Entity, StructuralChangeKind)> = dyn_ecs
                        .structural_changes_since(0)
                        .iter()
                        .map(|change| (change.sequence, change.entity, change.kind))
                        .collect();
                    assert_eq!(
                        static_lifecycle, dyn_lifecycle,
                        "group lifecycle logs diverged with seed {seed}"
                    );
                    assert_eq!(
                        static_ecs.structural_sequence(),
                        dyn_ecs.structural_sequence()
                    );
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
                            #[cfg(not(feature = "raw_storage"))]
                            {
                                let static_changed: std::collections::HashSet<Entity> =
                                    static_world.query_entities_changed(DIFF_POSITION).collect();
                                let dyn_changed: std::collections::HashSet<Entity> =
                                    dyn_world.query_entities_changed(DIFF_POSITION).collect();
                                assert_eq!(
                                    static_changed, dyn_changed,
                                    "changed sets diverged with seed {seed}"
                                );
                            }

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
