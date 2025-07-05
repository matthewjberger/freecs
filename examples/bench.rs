use freecs::{ecs, table_has_components, EntityId};
use std::time::{Duration, Instant};
use rayon::prelude::*;

// Define the ECS world and components - testing with many components
ecs! {
    World {
        position: Position => POSITION,
        velocity: Velocity => VELOCITY,
        health: Health => HEALTH,
        transform: Transform => TRANSFORM,
        render: Render => RENDER,
        physics: Physics => PHYSICS,
        ai: AI => AI,
        inventory: Inventory => INVENTORY,
        component_a: ComponentA => COMPONENT_A,
        component_b: ComponentB => COMPONENT_B,
        component_c: ComponentC => COMPONENT_C,
        component_d: ComponentD => COMPONENT_D,
        component_e: ComponentE => COMPONENT_E,
        component_f: ComponentF => COMPONENT_F,
        component_g: ComponentG => COMPONENT_G,
        component_h: ComponentH => COMPONENT_H,
        component_i: ComponentI => COMPONENT_I,
        component_j: ComponentJ => COMPONENT_J,
        component_k: ComponentK => COMPONENT_K,
        component_l: ComponentL => COMPONENT_L,
        component_m: ComponentM => COMPONENT_M,
        component_n: ComponentN => COMPONENT_N,
        component_o: ComponentO => COMPONENT_O,
        component_p: ComponentP => COMPONENT_P,
    }
    Resources {
        delta_time: f32,
        frame_count: u64,
    }
}

// Component definitions
#[derive(Default, Debug, Clone)]
pub struct Position {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Default, Debug, Clone)]
pub struct Velocity {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Default, Debug, Clone)]
pub struct Health {
    pub value: f32,
    pub max_value: f32,
}

#[derive(Default, Debug, Clone)]
pub struct Transform {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

#[derive(Default, Debug, Clone)]
pub struct Render {
    pub mesh_id: u32,
    pub material_id: u32,
    pub visible: bool,
    pub layer: u8,
}

#[derive(Default, Debug, Clone)]
pub struct Physics {
    pub mass: f32,
    pub drag: f32,
    pub acceleration: [f32; 3],
    pub forces: [f32; 3],
}

#[derive(Default, Debug, Clone)]
pub struct AI {
    pub state: u8,
    pub target_id: u32,
    pub decision_timer: f32,
    pub path: Vec<[f32; 3]>,
}

#[derive(Default, Debug, Clone)]
pub struct Inventory {
    pub items: Vec<u32>,
    pub capacity: u32,
    pub weight: f32,
}

// Additional test components with varying sizes to stress test
#[derive(Default, Debug, Clone)]
pub struct ComponentA {
    pub value: f32,
}

#[derive(Default, Debug, Clone)]
pub struct ComponentB {
    pub data: [u8; 16],
}

#[derive(Default, Debug, Clone)]
pub struct ComponentC {
    pub matrix: [[f32; 4]; 4],
}

#[derive(Default, Debug, Clone)]
pub struct ComponentD {
    pub flags: u64,
    pub counter: u32,
}

#[derive(Default, Debug, Clone)]
pub struct ComponentE {
    pub position: [f64; 3],
    pub velocity: [f64; 3],
}

#[derive(Default, Debug, Clone)]
pub struct ComponentF {
    pub name: String,
    pub id: u64,
}

#[derive(Default, Debug, Clone)]
pub struct ComponentG {
    pub buffer: Vec<u8>,
}

#[derive(Default, Debug, Clone)]
pub struct ComponentH {
    pub small_data: u8,
}

#[derive(Default, Debug, Clone)]
pub struct ComponentI {
    pub medium_data: [f32; 8],
}

#[derive(Default, Debug, Clone)]
pub struct ComponentJ {
    pub large_data: [u64; 32],
}

#[derive(Default, Debug, Clone)]
pub struct ComponentK {
    pub texture_coords: [[f32; 2]; 4],
}

#[derive(Default, Debug, Clone)]
pub struct ComponentL {
    pub animation_frame: u32,
    pub animation_speed: f32,
    pub loop_count: u32,
}

#[derive(Default, Debug, Clone)]
pub struct ComponentM {
    pub network_id: u64,
    pub last_sync: f64,
}

#[derive(Default, Debug, Clone)]
pub struct ComponentN {
    pub audio_source: u32,
    pub volume: f32,
    pub pitch: f32,
}

#[derive(Default, Debug, Clone)]
pub struct ComponentO {
    pub collision_layers: u32,
    pub collision_mask: u32,
}

#[derive(Default, Debug, Clone)]
pub struct ComponentP {
    pub script_id: u32,
    pub variables: Vec<f32>,
}

// System implementations
fn movement_system(positions: &mut [Position], velocities: &[Velocity], dt: f32) {
    positions
        .iter_mut()
        .zip(velocities.iter())
        .for_each(|(pos, vel)| {
            pos.x += vel.x * dt;
            pos.y += vel.y * dt;
            pos.z += vel.z * dt;
        });
}

fn physics_system(
    positions: &mut [Position],
    velocities: &mut [Velocity],
    physics: &mut [Physics],
    dt: f32,
) {
    positions
        .iter_mut()
        .zip(velocities.iter_mut())
        .zip(physics.iter_mut())
        .for_each(|((pos, vel), phys)| {
            // Apply forces to acceleration
            phys.acceleration[0] += phys.forces[0] / phys.mass;
            phys.acceleration[1] += phys.forces[1] / phys.mass;
            phys.acceleration[2] += phys.forces[2] / phys.mass;

            // Apply acceleration to velocity
            vel.x += phys.acceleration[0] * dt;
            vel.y += phys.acceleration[1] * dt;
            vel.z += phys.acceleration[2] * dt;

            // Apply drag
            let drag_factor = 1.0 - phys.drag * dt;
            vel.x *= drag_factor;
            vel.y *= drag_factor;
            vel.z *= drag_factor;

            // Update position
            pos.x += vel.x * dt;
            pos.y += vel.y * dt;
            pos.z += vel.z * dt;

            // Reset forces
            phys.forces = [0.0; 3];
        });
}

fn health_system(health: &mut [Health]) {
    health.iter_mut().for_each(|h| {
        h.value = (h.value - 0.1).max(0.0).min(h.max_value);
    });
}

fn render_system(positions: &[Position], transforms: &mut [Transform], renders: &mut [Render]) {
    positions
        .iter()
        .zip(transforms.iter_mut())
        .zip(renders.iter_mut())
        .for_each(|((pos, transform), render)| {
            // Update transform from position
            transform.position[0] = pos.x;
            transform.position[1] = pos.y;
            transform.position[2] = pos.z;

            // Frustum culling simulation
            let distance = (pos.x * pos.x + pos.y * pos.y + pos.z * pos.z).sqrt();
            render.visible = distance < 100.0;

            // Layer-based culling
            if distance > 50.0 {
                render.layer = 1;
            }
        });
}

fn ai_system(ai: &mut [AI], positions: &[Position], dt: f32) {
    ai.iter_mut()
        .zip(positions.iter())
        .for_each(|(ai_comp, pos)| {
            ai_comp.decision_timer -= dt;
            
            if ai_comp.decision_timer <= 0.0 {
                // Simple AI state machine
                ai_comp.state = (ai_comp.state + 1) % 4;
                ai_comp.decision_timer = 1.0;
                
                // Update path based on current position
                if ai_comp.path.len() < 5 {
                    ai_comp.path.push([pos.x + 10.0, pos.y, pos.z + 10.0]);
                }
            }
        });
}

// Parallel system implementations
fn parallel_movement_system(positions: &mut [Position], velocities: &[Velocity], dt: f32) {
    positions
        .par_iter_mut()
        .zip(velocities.par_iter())
        .for_each(|(pos, vel)| {
            pos.x += vel.x * dt;
            pos.y += vel.y * dt;
            pos.z += vel.z * dt;
        });
}

fn parallel_physics_system(
    positions: &mut [Position],
    velocities: &mut [Velocity],
    physics: &mut [Physics],
    dt: f32,
) {
    positions
        .par_iter_mut()
        .zip(velocities.par_iter_mut())
        .zip(physics.par_iter_mut())
        .for_each(|((pos, vel), phys)| {
            // Apply forces
            phys.acceleration[0] += phys.forces[0] / phys.mass;
            phys.acceleration[1] += phys.forces[1] / phys.mass;
            phys.acceleration[2] += phys.forces[2] / phys.mass;

            // Update velocity
            vel.x += phys.acceleration[0] * dt;
            vel.y += phys.acceleration[1] * dt;
            vel.z += phys.acceleration[2] * dt;

            // Apply drag
            let drag_factor = 1.0 - phys.drag * dt;
            vel.x *= drag_factor;
            vel.y *= drag_factor;
            vel.z *= drag_factor;

            // Update position
            pos.x += vel.x * dt;
            pos.y += vel.y * dt;
            pos.z += vel.z * dt;

            phys.forces = [0.0; 3];
        });
}

// Benchmark result structure
#[derive(Debug)]
pub struct BenchmarkResult {
    pub name: String,
    pub entity_count: usize,
    pub iterations: usize,
    pub total_time_ms: f64,
    pub avg_time_ms: f64,
    pub min_time_ms: f64,
    pub max_time_ms: f64,
    pub entities_per_second: f64,
    pub throughput_gb_per_sec: f64,
}

impl BenchmarkResult {
    pub fn new(name: String, entity_count: usize, times: &[Duration], data_size_bytes: usize) -> Self {
        let total_time = times.iter().sum::<Duration>();
        let total_time_ms = total_time.as_secs_f64() * 1000.0;
        let avg_time_ms = total_time_ms / times.len() as f64;
        let min_time_ms = times.iter().min().unwrap().as_secs_f64() * 1000.0;
        let max_time_ms = times.iter().max().unwrap().as_secs_f64() * 1000.0;
        let iterations = times.len();

        let entities_per_second = if total_time_ms > 0.0 {
            (entity_count * iterations) as f64 / (total_time_ms / 1000.0)
        } else {
            0.0
        };

        let throughput_gb_per_sec = if total_time_ms > 0.0 {
            let total_bytes = data_size_bytes * iterations;
            (total_bytes as f64) / (1024.0 * 1024.0 * 1024.0) / (total_time_ms / 1000.0)
        } else {
            0.0
        };

        Self {
            name,
            entity_count,
            iterations,
            total_time_ms,
            avg_time_ms,
            min_time_ms,
            max_time_ms,
            entities_per_second,
            throughput_gb_per_sec,
        }
    }

    pub fn print(&self) {
        println!(
            "{:<35} | {:>8} | {:>8.3}ms | {:>8.3}ms | {:>8.3}ms | {:>12.0} | {:>8.3} GB/s",
            self.name,
            self.entity_count,
            self.avg_time_ms,
            self.min_time_ms,
            self.max_time_ms,
            self.entities_per_second,
            self.throughput_gb_per_sec
        );
    }
}

// Benchmark implementations
fn benchmark_entity_creation(entity_count: usize, iterations: usize) -> BenchmarkResult {
    let mut times = Vec::new();

    // Warmup
    for _ in 0..3 {
        let mut world = World::default();
        let start = Instant::now();
        let _entities = world.spawn_entities(POSITION | VELOCITY, entity_count);
        times.push(start.elapsed());
    }
    times.clear();

    // Actual benchmark
    for _ in 0..iterations {
        let mut world = World::default();
        let start = Instant::now();
        let _entities = world.spawn_entities(POSITION | VELOCITY, entity_count);
        times.push(start.elapsed());
    }

    let data_size = entity_count * (std::mem::size_of::<Position>() + std::mem::size_of::<Velocity>());
    BenchmarkResult::new("Entity Creation".to_string(), entity_count, &times, data_size)
}

fn benchmark_sequential_movement_system(entity_count: usize, iterations: usize) -> BenchmarkResult {
    let mut world = World::default();
    let _entities = world.spawn_entities(POSITION | VELOCITY, entity_count);

    // Initialize velocities
    for table in &mut world.tables {
        if table_has_components!(table, VELOCITY) {
            for (i, vel) in table.velocity.iter_mut().enumerate() {
                vel.x = (i as f32 * 0.1) % 10.0;
                vel.y = (i as f32 * 0.05) % 5.0;
                vel.z = (i as f32 * 0.02) % 2.0;
            }
        }
    }

    let mut times = Vec::new();
    for _ in 0..iterations {
        let start = Instant::now();
        for table in &mut world.tables {
            if table_has_components!(table, POSITION | VELOCITY) {
                movement_system(&mut table.position, &table.velocity, 0.016);
            }
        }
        times.push(start.elapsed());
    }

    let data_size = entity_count * (std::mem::size_of::<Position>() + std::mem::size_of::<Velocity>());
    BenchmarkResult::new("Sequential Movement System".to_string(), entity_count, &times, data_size)
}

fn benchmark_parallel_movement_system(entity_count: usize, iterations: usize) -> BenchmarkResult {
    let mut world = World::default();
    let _entities = world.spawn_entities(POSITION | VELOCITY, entity_count);

    // Initialize velocities
    for table in &mut world.tables {
        if table_has_components!(table, VELOCITY) {
            for (i, vel) in table.velocity.iter_mut().enumerate() {
                vel.x = (i as f32 * 0.1) % 10.0;
                vel.y = (i as f32 * 0.05) % 5.0;
                vel.z = (i as f32 * 0.02) % 2.0;
            }
        }
    }

    let mut times = Vec::new();
    for _ in 0..iterations {
        let start = Instant::now();
        world.tables.par_iter_mut().for_each(|table| {
            if table_has_components!(table, POSITION | VELOCITY) {
                parallel_movement_system(&mut table.position, &table.velocity, 0.016);
            }
        });
        times.push(start.elapsed());
    }

    let data_size = entity_count * (std::mem::size_of::<Position>() + std::mem::size_of::<Velocity>());
    BenchmarkResult::new("Parallel Movement System".to_string(), entity_count, &times, data_size)
}

fn benchmark_sequential_physics_system(entity_count: usize, iterations: usize) -> BenchmarkResult {
    let mut world = World::default();
    let _entities = world.spawn_entities(POSITION | VELOCITY | PHYSICS, entity_count);

    // Initialize physics components
    for table in &mut world.tables {
        if table_has_components!(table, PHYSICS) {
            for (i, phys) in table.physics.iter_mut().enumerate() {
                phys.mass = 1.0 + (i as f32 * 0.1) % 5.0;
                phys.drag = 0.01 + (i as f32 * 0.001) % 0.1;
                phys.acceleration = [0.0, -9.81, 0.0];
                phys.forces = [
                    (i as f32 * 0.5) % 100.0 - 50.0,
                    (i as f32 * 0.3) % 50.0,
                    (i as f32 * 0.2) % 25.0 - 12.5
                ];
            }
        }
        if table_has_components!(table, VELOCITY) {
            for (i, vel) in table.velocity.iter_mut().enumerate() {
                vel.x = (i as f32 * 0.1) % 10.0;
                vel.y = 0.0;
                vel.z = (i as f32 * 0.05) % 5.0;
            }
        }
    }

    let mut times = Vec::new();
    for _ in 0..iterations {
        let start = Instant::now();
        for table in &mut world.tables {
            if table_has_components!(table, POSITION | VELOCITY | PHYSICS) {
                physics_system(
                    &mut table.position,
                    &mut table.velocity,
                    &mut table.physics,
                    0.016,
                );
            }
        }
        times.push(start.elapsed());
    }

    let data_size = entity_count * (
        std::mem::size_of::<Position>() + 
        std::mem::size_of::<Velocity>() + 
        std::mem::size_of::<Physics>()
    );
    BenchmarkResult::new("Sequential Physics System".to_string(), entity_count, &times, data_size)
}

fn benchmark_parallel_physics_system(entity_count: usize, iterations: usize) -> BenchmarkResult {
    let mut world = World::default();
    let _entities = world.spawn_entities(POSITION | VELOCITY | PHYSICS, entity_count);

    // Initialize physics components
    for table in &mut world.tables {
        if table_has_components!(table, PHYSICS) {
            for (i, phys) in table.physics.iter_mut().enumerate() {
                phys.mass = 1.0 + (i as f32 * 0.1) % 5.0;
                phys.drag = 0.01 + (i as f32 * 0.001) % 0.1;
                phys.acceleration = [0.0, -9.81, 0.0];
                phys.forces = [
                    (i as f32 * 0.5) % 100.0 - 50.0,
                    (i as f32 * 0.3) % 50.0,
                    (i as f32 * 0.2) % 25.0 - 12.5
                ];
            }
        }
        if table_has_components!(table, VELOCITY) {
            for (i, vel) in table.velocity.iter_mut().enumerate() {
                vel.x = (i as f32 * 0.1) % 10.0;
                vel.y = 0.0;
                vel.z = (i as f32 * 0.05) % 5.0;
            }
        }
    }

    let mut times = Vec::new();
    for _ in 0..iterations {
        let start = Instant::now();
        world.tables.par_iter_mut().for_each(|table| {
            if table_has_components!(table, POSITION | VELOCITY | PHYSICS) {
                parallel_physics_system(
                    &mut table.position,
                    &mut table.velocity,
                    &mut table.physics,
                    0.016,
                );
            }
        });
        times.push(start.elapsed());
    }

    let data_size = entity_count * (
        std::mem::size_of::<Position>() + 
        std::mem::size_of::<Velocity>() + 
        std::mem::size_of::<Physics>()
    );
    BenchmarkResult::new("Parallel Physics System".to_string(), entity_count, &times, data_size)
}

fn benchmark_multi_component_query(entity_count: usize, iterations: usize) -> BenchmarkResult {
    let mut world = World::default();
    
    // Create entities with different component combinations to test query performance
    let quarter = entity_count / 4;
    let _e1 = world.spawn_entities(POSITION, quarter);
    let _e2 = world.spawn_entities(POSITION | VELOCITY, quarter);
    let _e3 = world.spawn_entities(POSITION | VELOCITY | HEALTH, quarter);
    let _e4 = world.spawn_entities(POSITION | VELOCITY | HEALTH | PHYSICS, quarter);

    let mut times = Vec::new();
    for _ in 0..iterations {
        let start = Instant::now();
        let _r1 = world.query_entities(POSITION);
        let _r2 = world.query_entities(POSITION | VELOCITY);
        let _r3 = world.query_entities(POSITION | VELOCITY | HEALTH);
        let _r4 = world.query_entities(VELOCITY | PHYSICS);
        times.push(start.elapsed());
    }

    let data_size = entity_count * std::mem::size_of::<EntityId>();
    BenchmarkResult::new("Multi-Component Queries".to_string(), entity_count, &times, data_size)
}

fn benchmark_component_transitions(entity_count: usize, iterations: usize) -> BenchmarkResult {
    let mut world = World::default();
    let entities = world.spawn_entities(POSITION | VELOCITY, entity_count);

    let mut times = Vec::new();
    for iteration in 0..iterations {
        let start = Instant::now();
        
        // Add various components to different entities to create more archetype diversity
        for (i, &entity) in entities.iter().enumerate() {
            match i % 8 {
                0 => { world.add_components(entity, HEALTH | COMPONENT_A | COMPONENT_B); },
                1 => { world.add_components(entity, PHYSICS | COMPONENT_C | COMPONENT_D); },
                2 => { world.add_components(entity, AI | COMPONENT_E | COMPONENT_F); },
                3 => { world.add_components(entity, INVENTORY | COMPONENT_G | COMPONENT_H); },
                4 => { world.add_components(entity, TRANSFORM | COMPONENT_I | COMPONENT_J); },
                5 => { world.add_components(entity, RENDER | COMPONENT_K | COMPONENT_L); },
                6 => { world.add_components(entity, COMPONENT_M | COMPONENT_N | COMPONENT_O); },
                7 => { world.add_components(entity, COMPONENT_P | HEALTH | PHYSICS); },
                _ => {}
            }
        }
        
        // Remove some components to test removal transitions too
        for (i, &entity) in entities.iter().enumerate() {
            if i % 4 == 0 {
                world.remove_components(entity, VELOCITY | COMPONENT_A);
            }
        }
        
        times.push(start.elapsed());

        // Reset for next iteration (except last one)
        if iteration < iterations - 1 {
            for (i, &entity) in entities.iter().enumerate() {
                match i % 8 {
                    0 => { world.remove_components(entity, HEALTH | COMPONENT_A | COMPONENT_B); },
                    1 => { world.remove_components(entity, PHYSICS | COMPONENT_C | COMPONENT_D); },
                    2 => { world.remove_components(entity, AI | COMPONENT_E | COMPONENT_F); },
                    3 => { world.remove_components(entity, INVENTORY | COMPONENT_G | COMPONENT_H); },
                    4 => { world.remove_components(entity, TRANSFORM | COMPONENT_I | COMPONENT_J); },
                    5 => { world.remove_components(entity, RENDER | COMPONENT_K | COMPONENT_L); },
                    6 => { world.remove_components(entity, COMPONENT_M | COMPONENT_N | COMPONENT_O); },
                    7 => { world.remove_components(entity, COMPONENT_P | HEALTH | PHYSICS); },
                    _ => {}
                }
                if i % 4 == 0 {
                    world.add_components(entity, VELOCITY | COMPONENT_A);
                }
            }
        }
    }

    let data_size = entity_count * (
        std::mem::size_of::<Position>() + 
        std::mem::size_of::<Velocity>() + 
        // Add average size for the various components that get added/removed
        (std::mem::size_of::<Health>() + std::mem::size_of::<Physics>() + 
         std::mem::size_of::<AI>() + std::mem::size_of::<Inventory>() +
         std::mem::size_of::<ComponentA>() + std::mem::size_of::<ComponentB>() +
         std::mem::size_of::<ComponentC>() + std::mem::size_of::<ComponentD>()) / 2
    );
    BenchmarkResult::new("Component Transitions".to_string(), entity_count, &times, data_size)
}

fn benchmark_full_game_simulation(entity_count: usize, iterations: usize) -> BenchmarkResult {
    let mut world = World::default();
    
    // Create different entity types with varying component combinations
    let players = (entity_count / 10).max(1);
    let npcs = (entity_count / 5).max(1);
    let projectiles = (entity_count / 3).max(1);
    let remaining = entity_count.saturating_sub(players + npcs + projectiles);
    let environment = remaining.max(1);

    println!("Creating entities: players={}, npcs={}, projectiles={}, environment={}, total={}", 
             players, npcs, projectiles, environment, players + npcs + projectiles + environment);

    // Players: Full feature set with many components
    let _player_entities = world.spawn_entities(
        POSITION | VELOCITY | HEALTH | TRANSFORM | RENDER | PHYSICS | AI | INVENTORY | 
        COMPONENT_A | COMPONENT_B | COMPONENT_C | COMPONENT_D | COMPONENT_E, 
        players
    );

    // NPCs: AI and basic physics with some additional components
    let _npc_entities = world.spawn_entities(
        POSITION | VELOCITY | HEALTH | TRANSFORM | RENDER | AI | 
        COMPONENT_F | COMPONENT_G | COMPONENT_H | COMPONENT_I, 
        npcs
    );

    // Projectiles: Simple physics with minimal components
    let _projectile_entities = world.spawn_entities(
        POSITION | VELOCITY | PHYSICS | RENDER | COMPONENT_J | COMPONENT_K, 
        projectiles
    );

    // Environment: Static objects with various component combinations
    let _env_entities = world.spawn_entities(
        POSITION | TRANSFORM | RENDER | COMPONENT_L | COMPONENT_M | 
        COMPONENT_N | COMPONENT_O | COMPONENT_P, 
        environment
    );

    // Initialize all components with realistic data
    initialize_game_world(&mut world);

    let mut times = Vec::new();
    for _ in 0..iterations {
        let start = Instant::now();
        
        // Simulate a full game frame with multiple systems
        run_game_frame(&mut world, 0.016);
        
        times.push(start.elapsed());
    }

    let total_component_size = entity_count * (
        std::mem::size_of::<Position>() + 
        std::mem::size_of::<Velocity>() + 
        std::mem::size_of::<Health>() + 
        std::mem::size_of::<Transform>() + 
        std::mem::size_of::<Render>() + 
        std::mem::size_of::<Physics>() + 
        std::mem::size_of::<AI>() + 
        std::mem::size_of::<Inventory>() +
        // Add sizes for new components (estimated average since not all entities have all)
        (std::mem::size_of::<ComponentA>() + std::mem::size_of::<ComponentB>() + 
         std::mem::size_of::<ComponentC>() + std::mem::size_of::<ComponentD>() + 
         std::mem::size_of::<ComponentE>() + std::mem::size_of::<ComponentF>() + 
         std::mem::size_of::<ComponentG>() + std::mem::size_of::<ComponentH>() + 
         std::mem::size_of::<ComponentI>() + std::mem::size_of::<ComponentJ>() + 
         std::mem::size_of::<ComponentK>() + std::mem::size_of::<ComponentL>() + 
         std::mem::size_of::<ComponentM>() + std::mem::size_of::<ComponentN>() + 
         std::mem::size_of::<ComponentO>() + std::mem::size_of::<ComponentP>()) / 4
    ) / 3; // Rough average since not all entities have all components

    BenchmarkResult::new("Full Game Simulation".to_string(), entity_count, &times, total_component_size)
}

fn initialize_game_world(world: &mut World) {
    for table in &mut world.tables {
        // Initialize physics
        if table_has_components!(table, PHYSICS) {
            for (i, phys) in table.physics.iter_mut().enumerate() {
                phys.mass = 1.0 + (i as f32 * 0.1) % 5.0;
                phys.drag = 0.01 + (i as f32 * 0.001) % 0.1;
                phys.acceleration = [0.0, -9.81, 0.0];
            }
        }

        // Initialize velocities
        if table_has_components!(table, VELOCITY) {
            for (i, vel) in table.velocity.iter_mut().enumerate() {
                vel.x = (i as f32 * 0.1) % 10.0 - 5.0;
                vel.y = (i as f32 * 0.05) % 5.0;
                vel.z = (i as f32 * 0.02) % 2.0 - 1.0;
            }
        }

        // Initialize health
        if table_has_components!(table, HEALTH) {
            for health in &mut table.health {
                health.value = 100.0;
                health.max_value = 100.0;
            }
        }

        // Initialize AI
        if table_has_components!(table, AI) {
            for (i, ai) in table.ai.iter_mut().enumerate() {
                ai.state = (i % 4) as u8;
                ai.target_id = (i % 1000) as u32; // Safe target ID
                ai.decision_timer = (i as f32 * 0.1) % 2.0;
            }
        }

        // Initialize inventory
        if table_has_components!(table, INVENTORY) {
            for (i, inv) in table.inventory.iter_mut().enumerate() {
                inv.capacity = 20;
                inv.items = vec![(i % 10) as u32; (i % 5) + 1];
                inv.weight = inv.items.len() as f32 * 0.5;
            }
        }

        // Initialize render components
        if table_has_components!(table, RENDER) {
            for (i, render) in table.render.iter_mut().enumerate() {
                render.mesh_id = (i % 100) as u32;
                render.material_id = (i % 20) as u32;
                render.visible = true;
                render.layer = (i % 8) as u8;
            }
        }
    }
}

fn run_game_frame(world: &mut World, dt: f32) {
    // Physics system
    for table in &mut world.tables {
        if table_has_components!(table, POSITION | VELOCITY | PHYSICS) {
            physics_system(
                &mut table.position,
                &mut table.velocity,
                &mut table.physics,
                dt,
            );
        }
    }

    // Movement system
    for table in &mut world.tables {
        if table_has_components!(table, POSITION | VELOCITY) {
            movement_system(&mut table.position, &table.velocity, dt);
        }
    }

    // AI system
    for table in &mut world.tables {
        if table_has_components!(table, AI) && table_has_components!(table, POSITION) {
            ai_system(&mut table.ai, &table.position, dt);
        }
    }

    // Health system
    for table in &mut world.tables {
        if table_has_components!(table, HEALTH) {
            health_system(&mut table.health);
        }
    }

    // Render system
    for table in &mut world.tables {
        if table_has_components!(table, POSITION | TRANSFORM | RENDER) {
            render_system(&table.position, &mut table.transform, &mut table.render);
        }
    }
}

fn print_header() {
    println!("🚀 FreECS Performance Benchmark Suite");
    println!("===========================================");
    println!(
        "{:<35} | {:<8} | {:<10} | {:<10} | {:<10} | {:<12} | {:<10} | {:<8} | {:<8}",
        "Benchmark", "Entities", "Avg (ms)", "Min (ms)", "Max (ms)", "Entities/sec", "Throughput", "Memory", "Per Ent"
    );
    println!("{}", "=".repeat(140));
}

fn run_benchmark_suite(entity_count: usize, iterations: usize) {
     // Original benchmarks
     benchmark_entity_creation(entity_count, iterations).print();
     benchmark_sequential_movement_system(entity_count, iterations).print();
     benchmark_parallel_movement_system(entity_count, iterations).print();
     benchmark_sequential_physics_system(entity_count, iterations).print();
     benchmark_parallel_physics_system(entity_count, iterations).print();
     benchmark_multi_component_query(entity_count, iterations).print();
     benchmark_component_transitions(entity_count, iterations).print();
     benchmark_full_game_simulation(entity_count, iterations).print();
     
     // New benchmarks
     benchmark_entity_despawning(entity_count, iterations / 4).print(); // Fewer iterations since it recreates world each time
     
     if entity_count <= 10_000 { // Only run fragmentation test on smaller entity counts
         benchmark_table_fragmentation(entity_count, iterations).print();
     }
     
     let memory_result = benchmark_memory_usage(entity_count, iterations);
     memory_result.print();
}

fn benchmark_entity_despawning(entity_count: usize, iterations: usize) -> BenchmarkResult {
    let mut times = Vec::new();
    
    for _ in 0..iterations {
        // Setup entities for each iteration
        let mut world = World::default();
        let entities = world.spawn_entities(POSITION | VELOCITY | HEALTH, entity_count);
        
        let start = Instant::now();
        world.despawn_entities( &entities);
        times.push(start.elapsed());
    }

    let data_size = entity_count * (std::mem::size_of::<Position>() + std::mem::size_of::<Velocity>() + std::mem::size_of::<Health>());
    BenchmarkResult::new("Entity Despawning".to_string(), entity_count, &times, data_size)
}

fn benchmark_table_fragmentation(entity_count: usize, iterations: usize) -> BenchmarkResult {
    let mut world = World::default();
    
    // Create maximum fragmentation: each entity gets a unique component combination
    let components = [POSITION, VELOCITY, HEALTH, PHYSICS, AI, RENDER, TRANSFORM, INVENTORY];
    for i in 0..entity_count {
        let mask = components.iter().enumerate()
            .filter(|(idx, _)| (i >> idx) & 1 == 1)
            .map(|(_, &comp)| comp)
            .fold(POSITION, |acc, comp| acc | comp); // Always include POSITION
        world.spawn_entities(mask, 1);
    }

    let mut times = Vec::new();
    for _ in 0..iterations {
        let start = Instant::now();
        // Query across all fragmented tables
        let _results = world.query_entities(POSITION);
        times.push(start.elapsed());
    }

    let data_size = entity_count * std::mem::size_of::<EntityId>();
    BenchmarkResult::new(format!("Fragmented Query ({} tables)", world.tables.len()), entity_count, &times, data_size)
}

// Enhanced BenchmarkResult to track memory
#[derive(Debug)]
pub struct MemoryBenchmarkResult {
    pub base: BenchmarkResult,
    pub memory_used_mb: f64,
    pub memory_per_entity_bytes: f64,
}

impl MemoryBenchmarkResult {
    pub fn new(name: String, entity_count: usize, times: &[Duration], data_size_bytes: usize, world: &World) -> Self {
        let base = BenchmarkResult::new(name, entity_count, times, data_size_bytes);
        let memory_used = calculate_world_memory_usage(world);
        let memory_used_mb = memory_used as f64 / (1024.0 * 1024.0);
        let memory_per_entity_bytes = if entity_count > 0 { memory_used as f64 / entity_count as f64 } else { 0.0 };
        
        Self { base, memory_used_mb, memory_per_entity_bytes }
    }

    pub fn print(&self) {
        println!(
            "{:<35} | {:>8} | {:>8.3}ms | {:>8.3}ms | {:>8.3}ms | {:>12.0} | {:>8.3} GB/s | {:>8.1} MB | {:>6.0} B/ent",
            self.base.name,
            self.base.entity_count,
            self.base.avg_time_ms,
            self.base.min_time_ms,
            self.base.max_time_ms,
            self.base.entities_per_second,
            self.base.throughput_gb_per_sec,
            self.memory_used_mb,
            self.memory_per_entity_bytes
        );
    }
}

fn calculate_world_memory_usage(world: &World) -> usize {
    let mut total = 0;
    
    // Entity locations
    total += world.entity_locations.locations.len() * std::mem::size_of::<EntityLocation>();
    
    // Tables
    for table in &world.tables {
        total += table.entity_indices.capacity() * std::mem::size_of::<EntityId>();
        total += table.position.capacity() * std::mem::size_of::<Position>();
        total += table.velocity.capacity() * std::mem::size_of::<Velocity>();
        total += table.health.capacity() * std::mem::size_of::<Health>();
        total += table.physics.capacity() * std::mem::size_of::<Physics>();
        total += table.transform.capacity() * std::mem::size_of::<Transform>();
        total += table.render.capacity() * std::mem::size_of::<Render>();
        total += table.ai.capacity() * std::mem::size_of::<AI>();
        total += table.inventory.capacity() * std::mem::size_of::<Inventory>();
        // Add other components...
        total += table.component_a.capacity() * std::mem::size_of::<ComponentA>();
        total += table.component_b.capacity() * std::mem::size_of::<ComponentB>();
        // ... other components
    }
    
    // Table lookup HashMap
    total += world.table_lookup.capacity() * (std::mem::size_of::<u64>() + std::mem::size_of::<usize>());
    
    total
}

fn benchmark_memory_usage(entity_count: usize, iterations: usize) -> MemoryBenchmarkResult {
    let mut world = World::default();
    let mut times = Vec::new();
    
    for _ in 0..iterations {
        let start = Instant::now();
        let _entities = world.spawn_entities(POSITION | VELOCITY | HEALTH | PHYSICS, entity_count / iterations);
        times.push(start.elapsed());
    }

    let data_size = entity_count * (std::mem::size_of::<Position>() + std::mem::size_of::<Velocity>() + std::mem::size_of::<Health>() + std::mem::size_of::<Physics>());
    MemoryBenchmarkResult::new("Memory Usage".to_string(), entity_count, &times, data_size, &world)
}

fn main() {
    print_header();

    let test_configs = vec![
        (1_000, 100),
        (10_000, 50), 
        (100_000, 25),
        (1_000_000, 10),
    ];

    for (entity_count, iterations) in test_configs {
        println!(
            "\n📊 Testing with {} entities ({} iterations)",
            entity_count, iterations
        );
        println!("{}", "-".repeat(120));
        run_benchmark_suite(entity_count, iterations);
    }

    println!("\n✅ Benchmark completed!");
    println!("\n📈 Performance Analysis:");
    println!("• Sequential vs Parallel: Compare movement system performance");
    println!("• Memory Throughput: GB/s indicates cache efficiency"); 
    println!("• Component Transitions: Table movement overhead with {} total components", 16);
    println!("• Full Game Simulation: Real-world mixed workload with diverse archetypes");
    
    if cfg!(debug_assertions) {
        println!("\n⚠️  RUNNING IN DEBUG MODE");
        println!("💡 Run with --release flag for accurate production performance measurements");
        println!("💡 Performance in release mode will be 5-10x faster");
    } else {
        println!("\n✅ Running in release mode - results are production-representative");
        println!("💡 Testing with 16 different component types to stress test component scaling");
    }
}
