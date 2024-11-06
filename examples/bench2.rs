use freecs::world;
use std::collections::HashMap;
use std::time::Instant;

world! {
    World {
        components {
            position: Position => POSITION,
            velocity: Velocity => VELOCITY,
            health: Health => HEALTH,
            damage: Damage => DAMAGE,
            armor: Armor => ARMOR,
            status: Status => STATUS,
            lifetime: Lifetime => LIFETIME,
            collision: Collision => COLLISION,
            ai: AI => AI,
            player: Player => PLAYER,
            weapon: Weapon => WEAPON,
            inventory: Inventory => INVENTORY,
        },
        Resources {
            delta_time: f32
        }
    }
}

const ENTITY_COUNT: usize = 100_000;
const OPERATIONS_PER_FRAME: usize = 10_000;
const FRAMES_TO_SIMULATE: usize = 100;

struct OperationStats {
    count: usize,
    total_time_ms: f64,
    min_time_ms: f64,
    max_time_ms: f64,
    avg_time_ms: f64,
    table_changes: i32,
    success_rate: f64,
}

impl OperationStats {
    fn new() -> Self {
        OperationStats {
            count: 0,
            total_time_ms: 0.0,
            min_time_ms: f64::INFINITY,
            max_time_ms: 0.0,
            avg_time_ms: 0.0,
            table_changes: 0,
            success_rate: 0.0,
        }
    }

    fn update(&mut self, time_ms: f64, success: bool, table_delta: i32) {
        self.count += 1;
        self.total_time_ms += time_ms;
        self.min_time_ms = self.min_time_ms.min(time_ms);
        self.max_time_ms = self.max_time_ms.max(time_ms);
        self.avg_time_ms = self.total_time_ms / self.count as f64;
        self.table_changes += table_delta;
        self.success_rate = (self.success_rate * (self.count - 1) as f64
            + if success { 1.0 } else { 0.0 })
            / self.count as f64;
    }
}

struct BenchmarkStats {
    frame_times: Vec<f64>,
    operation_stats: HashMap<String, OperationStats>,
    initial_tables: usize,
    final_tables: usize,
    initial_entities: usize,
    final_entities: usize,
}

fn main() {
    let mut world = World::default();
    let mut stats = BenchmarkStats {
        frame_times: Vec::with_capacity(FRAMES_TO_SIMULATE),
        operation_stats: HashMap::new(),
        initial_tables: 0,
        final_tables: 0,
        initial_entities: 0,
        final_entities: 0,
    };

    println!("\nStarting ECS Benchmark");
    println!("----------------------");
    println!("Initial spawn of {} entities...", ENTITY_COUNT);

    let start = Instant::now();
    let mut entities = spawn_initial_entities(&mut world, ENTITY_COUNT);
    let spawn_time = start.elapsed();

    stats.initial_tables = world.tables.len();
    stats.initial_entities = total_entities(&world);

    println!("Initial state:");
    println!(
        "  Spawn time: {:?} ({:?} per entity)",
        spawn_time,
        spawn_time / ENTITY_COUNT as u32
    );
    println!("  Tables: {}", world.tables.len());
    println!("  Entities: {}", total_entities(&world));
    println!(
        "\nRunning {} frames with {} operations per frame...",
        FRAMES_TO_SIMULATE, OPERATIONS_PER_FRAME
    );

    let benchmark_start = Instant::now();
    for frame in 0..FRAMES_TO_SIMULATE {
        let frame_start = Instant::now();

        for op_idx in 0..OPERATIONS_PER_FRAME {
            let operation = (frame * op_idx) % 4;
            let tables_before = world.tables.len();
            let op_start = Instant::now();

            match operation {
                0 => {
                    let entity_idx = (frame * op_idx) % entities.len();
                    let entity = entities[entity_idx];
                    let components = deterministic_component_mask(frame, op_idx);
                    let success = add_components(&mut world, entity, components);
                    let time = op_start.elapsed().as_secs_f64() * 1000.0;
                    let table_delta = world.tables.len() as i32 - tables_before as i32;
                    stats
                        .operation_stats
                        .entry("add_components".to_string())
                        .or_insert_with(OperationStats::new)
                        .update(time, success, table_delta);
                }
                1 => {
                    let entity_idx = (frame * op_idx) % entities.len();
                    let entity = entities[entity_idx];
                    let components = deterministic_component_mask(frame + 1, op_idx);
                    let success = remove_components(&mut world, entity, components);
                    let time = op_start.elapsed().as_secs_f64() * 1000.0;
                    let table_delta = world.tables.len() as i32 - tables_before as i32;
                    stats
                        .operation_stats
                        .entry("remove_components".to_string())
                        .or_insert_with(OperationStats::new)
                        .update(time, success, table_delta);
                }
                2 => {
                    let components = deterministic_component_mask(frame + 2, op_idx);
                    let new_entities = spawn_entities(&mut world, components, 1);
                    entities.extend(new_entities);
                    let time = op_start.elapsed().as_secs_f64() * 1000.0;
                    let table_delta = world.tables.len() as i32 - tables_before as i32;
                    stats
                        .operation_stats
                        .entry("spawn".to_string())
                        .or_insert_with(OperationStats::new)
                        .update(time, true, table_delta);
                }
                3 => {
                    let entity_idx = (frame * op_idx) % entities.len();
                    let entity = entities[entity_idx];
                    let remove_mask = deterministic_component_mask(frame + 3, op_idx);
                    let add_mask = deterministic_component_mask(frame + 4, op_idx);
                    let success1 = remove_components(&mut world, entity, remove_mask);
                    let success2 = add_components(&mut world, entity, add_mask);
                    let time = op_start.elapsed().as_secs_f64() * 1000.0;
                    let table_delta = world.tables.len() as i32 - tables_before as i32;
                    stats
                        .operation_stats
                        .entry("replace".to_string())
                        .or_insert_with(OperationStats::new)
                        .update(time, success1 && success2, table_delta);
                }
                _ => unreachable!(),
            }
        }

        stats
            .frame_times
            .push(frame_start.elapsed().as_secs_f64() * 1000.0);

        if frame % 10 == 0 {
            println!(
                "Frame {}: {} tables, {} entities",
                frame,
                world.tables.len(),
                total_entities(&world)
            );
        }
    }

    stats.final_tables = world.tables.len();
    stats.final_entities = total_entities(&world);
    let total_time = benchmark_start.elapsed();

    println!("\nBenchmark Results");
    println!("----------------");
    println!("Total time: {:?}", total_time);
    println!(
        "Avg frame time: {:?}",
        total_time / FRAMES_TO_SIMULATE as u32
    );
    println!("\nEntity Stats:");
    println!("  Initial: {}", stats.initial_entities);
    println!("  Final: {}", stats.final_entities);
    println!(
        "  Delta: {}",
        stats.final_entities as i32 - stats.initial_entities as i32
    );
    println!("\nTable Stats:");
    println!("  Initial: {}", stats.initial_tables);
    println!("  Final: {}", stats.final_tables);
    println!(
        "  Delta: {}",
        stats.final_tables as i32 - stats.initial_tables as i32
    );

    println!("\nFrame Times:");
    let avg_frame_time = stats.frame_times.iter().sum::<f64>() / stats.frame_times.len() as f64;
    let min_frame_time = stats
        .frame_times
        .iter()
        .fold(f64::INFINITY, |a, &b| a.min(b));
    let max_frame_time = stats
        .frame_times
        .iter()
        .fold(0.0, |a: f32, &b| a.max(b as f32));
    println!("  Average: {:.3} ms", avg_frame_time);
    println!("  Min: {:.3} ms", min_frame_time);
    println!("  Max: {:.3} ms", max_frame_time);

    println!("\nOperation Statistics:");
    for (op_name, stats) in stats.operation_stats.iter() {
        println!("\n{}:", op_name);
        println!("  Count: {}", stats.count);
        println!("  Avg time: {:.3} ms", stats.avg_time_ms);
        println!("  Min time: {:.3} ms", stats.min_time_ms);
        println!("  Max time: {:.3} ms", stats.max_time_ms);
        println!("  Success rate: {:.1}%", stats.success_rate * 100.0);
        println!("  Net table changes: {}", stats.table_changes);
        println!(
            "  Operations/sec: {:.0}",
            1000.0 * stats.count as f64 / stats.total_time_ms
        );
    }
}

fn spawn_initial_entities(world: &mut World, count: usize) -> Vec<EntityId> {
    let mut entities = Vec::with_capacity(count);

    for i in 0..count {
        let components = match i % 6 {
            0 => POSITION | VELOCITY | HEALTH,
            1 => POSITION | HEALTH | ARMOR,
            2 => POSITION | VELOCITY | DAMAGE | WEAPON,
            3 => POSITION | AI | COLLISION | STATUS,
            4 => POSITION | PLAYER | INVENTORY | WEAPON | ARMOR,
            5 => POSITION | LIFETIME | STATUS | AI | COLLISION,
            _ => unreachable!(),
        };

        let entity = spawn_entities(world, components, 1)[0];
        entities.push(entity);
    }

    entities
}

fn deterministic_component_mask(frame: usize, op_idx: usize) -> u32 {
    let seed = frame
        .wrapping_mul(1664525)
        .wrapping_add(op_idx)
        .wrapping_mul(1013904223);
    let component_count = (seed % 5) + 1; // 1-5 components
    let mut mask = POSITION; // Always include position

    let possible_components = [
        VELOCITY, HEALTH, DAMAGE, ARMOR, STATUS, LIFETIME, COLLISION, AI, PLAYER, WEAPON, INVENTORY,
    ];

    for i in 0..component_count {
        let idx = ((seed as u32 >> (i * 8)) % possible_components.len() as u32) as usize;
        mask |= possible_components[idx];
    }

    mask
}

#[derive(Default, Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Position {
    x: f32,
    y: f32,
}

#[derive(Default, Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Velocity {
    x: f32,
    y: f32,
}

#[derive(Default, Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Health {
    value: f32,
}

#[derive(Default, Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Damage {
    value: f32,
}

#[derive(Default, Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Armor {
    value: f32,
}

#[derive(Default, Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Status {
    stunned: bool,
    poisoned: bool,
}

#[derive(Default, Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Lifetime {
    remaining: f32,
}

#[derive(Default, Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Collision {
    radius: f32,
}

#[derive(Default, Clone, Debug, serde::Serialize, serde::Deserialize)]
struct AI {
    state: u32,
}

#[derive(Default, Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Player {
    id: u32,
}

#[derive(Default, Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Weapon {
    damage: f32,
}

#[derive(Default, Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Inventory {
    capacity: u32,
}
