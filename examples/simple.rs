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

pub fn component_mask(world: &World, entity: EntityId) -> Option<u32> {
    location_get(&world.entity_locations, entity)
        .map(|(table_index, _)| world.tables[table_index].mask)
}

pub fn total_entities(world: &World) -> usize {
    world
        .tables
        .iter()
        .map(|table| table.entity_indices.len())
        .sum()
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

#[cfg(test)]
mod tests {
    use super::*;
    use rayon::*;
    use std::collections::HashSet;

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
        systems::run_systems(&mut world);

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

    // TODO: Ensure generational indices wrap at u32::MAX
    #[ignore]
    #[test]
    fn test_wrapping_generational_indices_at_u32_max() {
        let mut world = World::default();

        // Create an initial entity with Position
        let entity_a1 = spawn_entities(&mut world, POSITION, 1)[0];
        assert_eq!(
            entity_a1.generation, 0,
            "First use of ID should have generation 0"
        );

        // Store the ID for later reuse
        let id = entity_a1.id;

        // Create another entity with the same ID (entity A3)
        let entity_a2 = spawn_entities(&mut world, POSITION, 1)[0];
        assert_eq!(entity_a2.id, id, "Should reuse the same ID again");
        assert_eq!(
            entity_a2.generation, 2,
            "Third use of ID should have generation 2"
        );

        // Test wrapping behavior of generations
        // Force generation to maximum value
        let max_gen = u32::MAX;
        for _ in 0..max_gen - 2 {
            // -2 because we already used 2 generations
            despawn_entities(&mut world, &[entity_a2]);
            let entity = spawn_entities(&mut world, POSITION, 1)[0];
            assert_eq!(entity.id, id, "Should continue to reuse the same ID");
        }

        // Get the entity with maximum generation
        let entity_max = spawn_entities(&mut world, POSITION, 1)[0];
        assert_eq!(
            entity_max.generation,
            u32::MAX,
            "Should reach maximum generation"
        );

        // Test wrapping to zero
        despawn_entities(&mut world, &[entity_max]);
        let entity_wrapped = spawn_entities(&mut world, POSITION, 1)[0];
        assert_eq!(
            entity_wrapped.id, id,
            "Should still use same ID after generation wrap"
        );
        assert_eq!(entity_wrapped.generation, 0, "Generation should wrap to 0");

        // Verify that old reference with max generation is invalid
        assert!(
            get_component::<Position>(&world, entity_max, POSITION).is_none(),
            "Max generation reference should be invalid after wrap"
        );
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
}
