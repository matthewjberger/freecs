// Needs query performance optimization
// Maybe setup query batching / caching, chunk-based processing

use freecs::has_components;
use rayon::prelude::*;
use std::time::{Duration, Instant};

#[repr(u32)]
#[allow(clippy::upper_case_acronyms)]
#[allow(non_camel_case_types)]
pub enum Component {
    POSITION,
    VELOCITY,
    HEALTH,
}

pub const ALL: u32 = 0;
pub const POSITION: u32 = 1 << (Component::POSITION as u32);
pub const VELOCITY: u32 = 1 << (Component::VELOCITY as u32);
pub const HEALTH: u32 = 1 << (Component::HEALTH as u32);
const COMPONENT_COUNT: usize = 3;

#[derive(
    Default, Clone, Copy, Debug, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize,
)]
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

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct EntityAllocator {
    next_id: u32,
    free_ids: Vec<(u32, u32)>,
}

#[derive(Copy, Clone, Default, serde::Serialize, serde::Deserialize)]
struct EntityLocation {
    generation: u32,
    table_index: u16,
    array_index: u16,
    allocated: bool,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct EntityLocations {
    locations: Vec<EntityLocation>,
}

#[derive(Default)]
pub struct Resources {
    pub delta_time: f32,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct ComponentArrays {
    pub position: Vec<Position>,
    pub velocity: Vec<Velocity>,
    pub health: Vec<Health>,
    pub entity_indices: Vec<EntityId>,
    pub mask: u32,
}

#[derive(Copy, Clone, Default, serde::Serialize, serde::Deserialize)]
struct TableEdges {
    add_edges: [Option<usize>; COMPONENT_COUNT],
    remove_edges: [Option<usize>; COMPONENT_COUNT],
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct World {
    pub entity_locations: EntityLocations,
    pub tables: Vec<ComponentArrays>,
    pub allocator: EntityAllocator,
    table_edges: Vec<TableEdges>,
    pending_despawns: Vec<EntityId>,
    #[serde(skip)]
    pub resources: Resources,
}

fn get_component_index(mask: u32) -> Option<usize> {
    match mask {
        POSITION => Some(0),
        VELOCITY => Some(1),
        HEALTH => Some(2),
        _ => None,
    }
}

fn get_or_create_table(world: &mut World, mask: u32) -> usize {
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

    for comp_mask in [POSITION, VELOCITY, HEALTH] {
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

fn move_entity(
    world: &mut World,
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

fn remove_from_table(arrays: &mut ComponentArrays, index: usize) -> Option<EntityId> {
    let last_index = arrays.entity_indices.len() - 1;
    let mut swapped_entity = None;

    if index < last_index {
        swapped_entity = Some(arrays.entity_indices[last_index]);
    }

    if arrays.mask & POSITION != 0 {
        arrays.position.swap_remove(index);
    }
    if arrays.mask & VELOCITY != 0 {
        arrays.velocity.swap_remove(index);
    }
    if arrays.mask & HEALTH != 0 {
        arrays.health.swap_remove(index);
    }
    arrays.entity_indices.swap_remove(index);

    swapped_entity
}

pub fn spawn_entities(world: &mut World, mask: u32, count: usize) -> Vec<EntityId> {
    let mut entities = Vec::with_capacity(count);
    let table_index = get_or_create_table(world, mask);

    world.tables[table_index].entity_indices.reserve(count);
    if mask & POSITION != 0 {
        world.tables[table_index].position.reserve(count);
    }
    if mask & VELOCITY != 0 {
        world.tables[table_index].velocity.reserve(count);
    }
    if mask & HEALTH != 0 {
        world.tables[table_index].health.reserve(count);
    }

    for _ in 0..count {
        let entity = create_entity(world);
        add_to_table(
            &mut world.tables[table_index],
            entity,
            (
                if mask & POSITION != 0 {
                    Some(Position::default())
                } else {
                    None
                },
                if mask & VELOCITY != 0 {
                    Some(Velocity::default())
                } else {
                    None
                },
                if mask & HEALTH != 0 {
                    Some(Health::default())
                } else {
                    None
                },
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

pub fn add_components(world: &mut World, entity: EntityId, mask: u32) -> bool {
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

pub fn remove_components(world: &mut World, entity: EntityId, mask: u32) -> bool {
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

pub fn query_entities(world: &World, mask: u32) -> Vec<EntityId> {
    use rayon::prelude::*;
    let total_capacity = world
        .tables
        .par_iter()
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

pub fn query_first_entity(world: &World, mask: u32) -> Option<EntityId> {
    world
        .tables
        .iter()
        .find(|table| table.mask & mask == mask)
        .and_then(|table| table.entity_indices.first().copied())
}

pub fn get_component<T: 'static>(world: &World, entity: EntityId, mask: u32) -> Option<&T> {
    let (table_index, array_index) = location_get(&world.entity_locations, entity)?;

    // Early return if entity is despawned
    if !world.entity_locations.locations[entity.id as usize].allocated {
        return None;
    }

    let table = &world.tables[table_index];

    let table = &world.tables[table_index];
    if table.mask & mask == 0 {
        return None;
    }
    if mask == POSITION && std::any::TypeId::of::<T>() == std::any::TypeId::of::<Position>() {
        return Some(unsafe { &*(&table.position[array_index] as *const Position as *const T) });
    }
    if mask == VELOCITY && std::any::TypeId::of::<T>() == std::any::TypeId::of::<Velocity>() {
        return Some(unsafe { &*(&table.velocity[array_index] as *const Velocity as *const T) });
    }
    if mask == HEALTH && std::any::TypeId::of::<T>() == std::any::TypeId::of::<Health>() {
        return Some(unsafe { &*(&table.health[array_index] as *const Health as *const T) });
    }
    None
}

pub fn get_component_mut<T: 'static>(
    world: &mut World,
    entity: EntityId,
    mask: u32,
) -> Option<&mut T> {
    let (table_index, array_index) = location_get(&world.entity_locations, entity)?;
    let table = &mut world.tables[table_index];
    if table.mask & mask == 0 {
        return None;
    }
    if mask == POSITION && std::any::TypeId::of::<T>() == std::any::TypeId::of::<Position>() {
        return Some(unsafe {
            &mut *(&mut table.position[array_index] as *mut Position as *mut T)
        });
    }
    if mask == VELOCITY && std::any::TypeId::of::<T>() == std::any::TypeId::of::<Velocity>() {
        return Some(unsafe {
            &mut *(&mut table.velocity[array_index] as *mut Velocity as *mut T)
        });
    }
    if mask == HEALTH && std::any::TypeId::of::<T>() == std::any::TypeId::of::<Health>() {
        return Some(unsafe { &mut *(&mut table.health[array_index] as *mut Health as *mut T) });
    }
    None
}

pub fn despawn_entities(world: &mut World, entities: &[EntityId]) -> Vec<EntityId> {
    let mut despawned = Vec::with_capacity(entities.len());

    // Just mark entities as despawned, defer actual cleanup
    for &entity in entities {
        let id = entity.id as usize;
        if id < world.entity_locations.locations.len() {
            let loc = &mut world.entity_locations.locations[id];
            if loc.allocated && loc.generation == entity.generation {
                loc.allocated = false;
                loc.generation = loc.generation.wrapping_add(1);
                world.allocator.free_ids.push((entity.id, loc.generation));
                world.pending_despawns.push(entity);
                despawned.push(entity);
            }
        }
    }

    // Only cleanup if we have a lot of pending despawns
    if world.pending_despawns.len() > 10_000 {
        cleanup_pending_despawns(world);
    }

    despawned
}

fn cleanup_pending_despawns(world: &mut World) {
    let mut table_ops: Vec<Vec<usize>> = vec![Vec::new(); world.tables.len()];

    // Group indices by table
    for &entity in &world.pending_despawns {
        if let Some((table_index, array_index)) = location_get(&world.entity_locations, entity) {
            table_ops[table_index].push(array_index);
        }
    }

    // Process each table
    for (table_idx, mut indices) in table_ops.into_iter().enumerate() {
        if indices.is_empty() {
            continue;
        }

        // Sort indices in reverse order
        indices.sort_unstable_by(|a, b| b.cmp(a));

        let table = &mut world.tables[table_idx];

        // Process in chunks for better cache utilization
        for chunk in indices.chunks(1024) {
            for &idx in chunk {
                let last_idx = table.entity_indices.len() - 1;

                // Move the last entity to this position if needed
                if idx < last_idx {
                    let moved_entity = table.entity_indices[last_idx];
                    if world.entity_locations.locations[moved_entity.id as usize].allocated {
                        location_insert(
                            &mut world.entity_locations,
                            moved_entity,
                            (table_idx, idx),
                        );
                    }
                }

                // Remove components
                if table.mask & POSITION != 0 {
                    table.position.swap_remove(idx);
                }
                if table.mask & VELOCITY != 0 {
                    table.velocity.swap_remove(idx);
                }
                if table.mask & HEALTH != 0 {
                    table.health.swap_remove(idx);
                }
                table.entity_indices.swap_remove(idx);
            }
        }
    }

    world.pending_despawns.clear();
}

fn create_entity(world: &mut World) -> EntityId {
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
    components: (Option<Position>, Option<Velocity>, Option<Health>),
) {
    let (position, velocity, health) = components;
    if arrays.mask & POSITION != 0 {
        arrays.position.push(position.unwrap_or_default());
    }
    if arrays.mask & VELOCITY != 0 {
        arrays.velocity.push(velocity.unwrap_or_default());
    }
    if arrays.mask & HEALTH != 0 {
        arrays.health.push(health.unwrap_or_default());
    }
    arrays.entity_indices.push(entity);
}

fn get_components(
    arrays: &ComponentArrays,
    index: usize,
) -> (Option<Position>, Option<Velocity>, Option<Health>) {
    (
        if arrays.mask & POSITION != 0 {
            Some(arrays.position[index].clone())
        } else {
            None
        },
        if arrays.mask & VELOCITY != 0 {
            Some(arrays.velocity[index].clone())
        } else {
            None
        },
        if arrays.mask & HEALTH != 0 {
            Some(arrays.health[index].clone())
        } else {
            None
        },
    )
}

fn location_get(locations: &EntityLocations, entity: EntityId) -> Option<(usize, usize)> {
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

fn location_insert(locations: &mut EntityLocations, entity: EntityId, location: (usize, usize)) {
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

    pub fn run_systems(world: &mut World) {
        let delta_time = world.resources.delta_time;
        world.tables.par_iter_mut().for_each(|table| {
            if has_components!(table, POSITION | VELOCITY | HEALTH) {
                update_positions_system(&mut table.position, &table.velocity, delta_time);
            }
            if has_components!(table, HEALTH) {
                health_system(&mut table.health);
            }
        });
    }

    #[inline]
    pub fn update_positions_system(positions: &mut [Position], velocities: &[Velocity], dt: f32) {
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
            health.value *= 0.98;
        });
    }
}

use components::*;

struct BenchmarkResults {
    spawn_time: Duration,
    query_time: Duration,
    add_component_time: Duration,
    remove_component_time: Duration,
    despawn_time: Duration,
    system_time: Duration,
    final_table_count: usize,
    final_entity_count: usize,
}

pub fn main() {
    let results = run_benchmark();
    print_results(&results);
}

fn run_benchmark() -> BenchmarkResults {
    let mut world = World::default();
    world.resources.delta_time = 0.016;

    let start = Instant::now();
    let entities: Vec<_> = (0..1_000_000)
        .map(|i| {
            let mask = if i % 2 == 0 {
                POSITION | VELOCITY
            } else {
                POSITION | VELOCITY | HEALTH
            };
            spawn_entities(&mut world, mask, 1)[0]
        })
        .collect();
    let spawn_time = start.elapsed();

    let start = Instant::now();
    for _ in 0..100 {
        let _ = query_entities(&world, POSITION | VELOCITY);
        let _ = query_entities(&world, POSITION | VELOCITY | HEALTH);
        let _ = query_first_entity(&world, HEALTH);
    }
    let query_time = start.elapsed();

    let start = Instant::now();
    for &entity in entities.iter().take(500_000) {
        add_components(&mut world, entity, HEALTH);
    }
    let add_component_time = start.elapsed();

    let start = Instant::now();
    for &entity in entities.iter().take(500_000) {
        remove_components(&mut world, entity, HEALTH);
    }
    let remove_component_time = start.elapsed();

    let start = Instant::now();
    for _ in 0..100 {
        systems::run_systems(&mut world);
    }
    let system_time = start.elapsed();

    let start = Instant::now();
    despawn_entities(&mut world, &entities);
    let despawn_time = start.elapsed();

    BenchmarkResults {
        spawn_time,
        query_time,
        add_component_time,
        remove_component_time,
        system_time,
        despawn_time,
        final_table_count: world.tables.len(),
        final_entity_count: query_entities(&world, ALL).len(),
    }
}

fn print_results(results: &BenchmarkResults) {
    println!("ECS Benchmark Results");
    println!("--------------------");
    println!(
        "Spawn 1 million entities: {:?} ({:?} per entity)",
        results.spawn_time,
        results.spawn_time / 1_000_000
    );
    println!(
        "100 queries across 1 million entities: {:?} ({:?} per query)",
        results.query_time,
        results.query_time / 100
    );
    println!(
        "Add component to 500_000 entities: {:?} ({:?} per operation)",
        results.add_component_time,
        results.add_component_time / 500_000
    );
    println!(
        "Remove component from 500_0000 entities: {:?} ({:?} per operation)",
        results.remove_component_time,
        results.remove_component_time / 500_000
    );
    println!(
        "100 system iterations on 1 million entities: {:?} ({:?} per iteration)",
        results.system_time,
        results.system_time / 100
    );
    println!(
        "Despawn 1 million entities: {:?} ({:?} per entity)",
        results.despawn_time,
        results.despawn_time / 1_000_000
    );
    println!("Final table count: {}", results.final_table_count);
    println!("Final entity count: {}", results.final_entity_count);
}
