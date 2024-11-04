use freecs::{has_components, world};
use rayon::prelude::*;
use std::time::{Duration, Instant};

world! {
    World {
        components {
            position: Position => POSITION,
            velocity: Velocity => VELOCITY,
            health: Health => HEALTH,
        },
        Resources {
            delta_time: f32
        }
    }
}

#[derive(Default)]
struct StressTestConfig {
    num_entities: usize,
    num_frames: usize,
    entity_batch_size: usize,
}

#[derive(Default)]
struct BenchmarkResults {
    parallel_query_time: Duration,
    sequential_query_time: Duration,
    parallel_update_time: Duration,
    sequential_update_time: Duration,
    mass_deletion_time: Duration,
    mass_spawn_time: Duration,
    mass_modify_time: Duration,
}

#[derive(Default)]
struct Metrics {
    spawn_time: Duration,
    system_time: Duration,
    cleanup_time: Duration,
    frame_times: Vec<Duration>,
    frame_count: usize,
    total_entities_processed: usize,
    benchmarks: Vec<BenchmarkResults>,
}

impl Metrics {
    fn print_summary(&self) {
        println!("\nPerformance Summary");
        println!("-----------------");
        println!("Initial spawn time: {:?}", self.spawn_time);

        if !self.benchmarks.is_empty() {
            let avg_results = self.average_benchmarks();
            println!(
                "\nBenchmark Results (Averaged over {} runs)",
                self.benchmarks.len()
            );
            println!(
                "Parallel vs Sequential Query Time: {:?} vs {:?} ({}x speedup)",
                avg_results.parallel_query_time,
                avg_results.sequential_query_time,
                avg_results.sequential_query_time.as_secs_f64()
                    / avg_results.parallel_query_time.as_secs_f64()
            );
            println!(
                "Parallel vs Sequential Update Time: {:?} vs {:?} ({}x speedup)",
                avg_results.parallel_update_time,
                avg_results.sequential_update_time,
                avg_results.sequential_update_time.as_secs_f64()
                    / avg_results.parallel_update_time.as_secs_f64()
            );
            println!("Mass Operations:");
            println!("  Deletion: {:?}", avg_results.mass_deletion_time);
            println!("  Spawn: {:?}", avg_results.mass_spawn_time);
            println!("  Modification: {:?}", avg_results.mass_modify_time);
        }

        if !self.frame_times.is_empty() {
            let total_time: Duration = self.frame_times.iter().sum();
            let avg_frame_time = total_time.div_f64(self.frame_times.len() as f64);
            let max_frame_time = *self.frame_times.iter().max().unwrap();
            let min_frame_time = *self.frame_times.iter().min().unwrap();

            println!("\nFrame Statistics");
            println!(
                "Total entities processed: {}",
                self.total_entities_processed
            );
            println!("Total frames: {}", self.frame_count);
            println!("Total simulation time: {:?}", total_time);
            println!(
                "Frame times - Avg: {:?}, Min: {:?}, Max: {:?}",
                avg_frame_time, min_frame_time, max_frame_time
            );

            let fps = self.frame_count as f64 / total_time.as_secs_f64();
            println!("Average FPS: {:.2}", fps);

            let mean_frame_secs = avg_frame_time.as_secs_f64();
            let variance = self
                .frame_times
                .iter()
                .map(|t| {
                    let diff = t.as_secs_f64() - mean_frame_secs;
                    diff * diff
                })
                .sum::<f64>()
                / self.frame_times.len() as f64;
            println!("Frame time std dev: {:.6}s", variance.sqrt());

            let entities_per_second =
                self.total_entities_processed as f64 / total_time.as_secs_f64();
            println!("Entity updates/second: {:.2}", entities_per_second);
        }
    }

    fn average_benchmarks(&self) -> BenchmarkResults {
        let count = self.benchmarks.len() as f64;
        BenchmarkResults {
            parallel_query_time: self
                .benchmarks
                .iter()
                .map(|b| b.parallel_query_time)
                .sum::<Duration>()
                .div_f64(count),
            sequential_query_time: self
                .benchmarks
                .iter()
                .map(|b| b.sequential_query_time)
                .sum::<Duration>()
                .div_f64(count),
            parallel_update_time: self
                .benchmarks
                .iter()
                .map(|b| b.parallel_update_time)
                .sum::<Duration>()
                .div_f64(count),
            sequential_update_time: self
                .benchmarks
                .iter()
                .map(|b| b.sequential_update_time)
                .sum::<Duration>()
                .div_f64(count),
            mass_deletion_time: self
                .benchmarks
                .iter()
                .map(|b| b.mass_deletion_time)
                .sum::<Duration>()
                .div_f64(count),
            mass_spawn_time: self
                .benchmarks
                .iter()
                .map(|b| b.mass_spawn_time)
                .sum::<Duration>()
                .div_f64(count),
            mass_modify_time: self
                .benchmarks
                .iter()
                .map(|b| b.mass_modify_time)
                .sum::<Duration>()
                .div_f64(count),
        }
    }
}

pub fn main() {
    let config = StressTestConfig {
        num_entities: 5_000_000,
        num_frames: 1000,
        entity_batch_size: 10_000,
    };

    println!("Starting comprehensive stress test");
    println!("Entities: {}", config.num_entities);
    println!("Frames: {}", config.num_frames);
    println!("Batch size: {}", config.entity_batch_size);

    let mut metrics = Metrics::default();
    run_stress_test(config, &mut metrics);
    metrics.print_summary();
}

fn run_benchmark_suite(world: &mut World) -> BenchmarkResults {
    let mut results = BenchmarkResults::default();

    // Test 1: Parallel vs Sequential Query
    {
        let start = Instant::now();
        let parallel_results: usize = world
            .tables
            .par_iter()
            .filter(|table| has_components!(table, POSITION | HEALTH))
            .map(|table| {
                table
                    .position
                    .par_iter()
                    .zip(table.health.par_iter())
                    .filter(|(pos, health)| {
                        let dist = (pos.x * pos.x + pos.y * pos.y).sqrt();
                        dist > 100.0 && health.value > 50.0
                    })
                    .count()
            })
            .sum();
        results.parallel_query_time = start.elapsed();

        let start = Instant::now();
        let sequential_results: usize = world
            .tables
            .iter()
            .filter(|table| has_components!(table, POSITION | HEALTH))
            .map(|table| {
                table
                    .position
                    .iter()
                    .zip(table.health.iter())
                    .filter(|(pos, health)| {
                        let dist = (pos.x * pos.x + pos.y * pos.y).sqrt();
                        dist > 100.0 && health.value > 50.0
                    })
                    .count()
            })
            .sum();
        results.sequential_query_time = start.elapsed();

        assert_eq!(
            parallel_results, sequential_results,
            "Query results mismatch!"
        );
    }

    // Rest of benchmark suite remains the same...
    // Test 2: Parallel vs Sequential Update
    {
        let start = Instant::now();
        world.tables.par_iter_mut().for_each(|table| {
            if has_components!(table, POSITION | VELOCITY) {
                table
                    .position
                    .par_iter_mut()
                    .zip(table.velocity.par_iter())
                    .for_each(|(pos, vel)| {
                        pos.x += vel.x;
                        pos.y += vel.y;
                    });
            }
        });
        results.parallel_update_time = start.elapsed();

        let start = Instant::now();
        for table in &mut world.tables {
            if has_components!(table, POSITION | VELOCITY) {
                for (pos, vel) in table.position.iter_mut().zip(table.velocity.iter()) {
                    pos.x += vel.x;
                    pos.y += vel.y;
                }
            }
        }
        results.sequential_update_time = start.elapsed();
    }

    // Test 3: Mass Entity Operations
    {
        // Mass spawn test
        let start = Instant::now();
        let new_entities = spawn_entities(world, POSITION | VELOCITY | HEALTH, 10_000);
        results.mass_spawn_time = start.elapsed();

        // Mass modify test
        let start = Instant::now();
        for &entity in &new_entities {
            if let Some(pos) = get_component_mut::<Position>(world, entity, POSITION) {
                pos.x = 100.0;
                pos.y = 100.0;
            }
            if let Some(health) = get_component_mut::<Health>(world, entity, HEALTH) {
                health.value = 50.0;
            }
        }
        results.mass_modify_time = start.elapsed();

        // Mass deletion test
        let start = Instant::now();
        despawn_entities(world, &new_entities);
        results.mass_deletion_time = start.elapsed();
    }

    results
}

fn run_stress_test(config: StressTestConfig, metrics: &mut Metrics) {
    let mut world = World::default();
    world.resources.delta_time = 1.0 / 60.0;

    // Initial spawn
    let spawn_start = Instant::now();
    let entities = spawn_test_entities(&mut world, config.num_entities, config.entity_batch_size);
    metrics.spawn_time = spawn_start.elapsed();
    println!(
        "Spawned {} entities in {:?}",
        entities.len(),
        metrics.spawn_time
    );

    // Run simulation with periodic benchmarks
    metrics.frame_count = config.num_frames;
    for frame in 0..config.num_frames {
        let frame_start = Instant::now();

        // Run normal frame
        let processed = run_frame(&mut world, metrics);
        metrics.total_entities_processed += processed;

        // Run benchmark suite every 200 frames
        if frame % 200 == 0 {
            println!("\nRunning benchmark suite at frame {}", frame);
            let benchmark_results = run_benchmark_suite(&mut world);
            metrics.benchmarks.push(benchmark_results);
        }

        let frame_time = frame_start.elapsed();
        metrics.frame_times.push(frame_time);

        if frame % 100 == 0 {
            println!(
                "Frame {}/{}: {:?} ({:.2} FPS) - {} entities",
                frame + 1,
                config.num_frames,
                frame_time,
                1.0 / frame_time.as_secs_f64(),
                processed
            );
        }

        if frame % 60 == 0 {
            merge_tables(&mut world);
        }
    }

    // Cleanup
    let cleanup_start = Instant::now();
    despawn_entities(&mut world, &entities);
    metrics.cleanup_time = cleanup_start.elapsed();
}

fn spawn_test_entities(world: &mut World, total: usize, batch_size: usize) -> Vec<EntityId> {
    let mut entities = Vec::with_capacity(total);
    let mut spawned = 0;

    while spawned < total {
        let batch = total.saturating_sub(spawned).min(batch_size);

        for i in 0..batch {
            let mask = match (spawned + i) % 4 {
                0 => POSITION | VELOCITY | HEALTH,
                1 => POSITION | VELOCITY,
                2 => POSITION | HEALTH,
                _ => POSITION,
            };

            let entity = spawn_entities(world, mask, 1)[0];

            if let Some(pos) = get_component_mut::<Position>(world, entity, POSITION) {
                pos.x = ((spawned + i) as f32).cos() * 100.0;
                pos.y = ((spawned + i) as f32).sin() * 100.0;
            }
            if let Some(vel) = get_component_mut::<Velocity>(world, entity, VELOCITY) {
                vel.x = ((spawned + i) as f32 * 0.1).cos();
                vel.y = ((spawned + i) as f32 * 0.1).sin();
            }
            if let Some(health) = get_component_mut::<Health>(world, entity, HEALTH) {
                health.value = 100.0;
            }

            entities.push(entity);
        }

        spawned += batch;
    }

    entities
}

fn run_frame(world: &mut World, metrics: &mut Metrics) -> usize {
    let dt = world.resources.delta_time;
    let mut total_processed = 0;
    let mut entities_to_remove = Vec::new();

    let system_start = Instant::now();

    // Process each table
    world.tables.par_iter_mut().for_each(|table| {
        if has_components!(table, POSITION | VELOCITY) {
            table
                .position
                .par_iter_mut()
                .zip(table.velocity.par_iter())
                .for_each(|(pos, vel)| {
                    pos.x += vel.x * dt;
                    pos.y += vel.y * dt;
                    pos.x = pos.x.rem_euclid(1000.0);
                    pos.y = pos.y.rem_euclid(1000.0);
                });
        }

        if has_components!(table, POSITION | HEALTH) {
            table
                .health
                .par_iter_mut()
                .zip(table.position.par_iter())
                .zip(table.entity_indices.par_iter())
                .for_each(|((health, pos), _entity_id)| {
                    let distance = (pos.x * pos.x + pos.y * pos.y).sqrt();
                    if distance > 800.0 {
                        health.value -= 1.0;
                    }
                });
        }
    });

    // Count total processed entities and collect entities to remove
    for table in &world.tables {
        total_processed += table.entity_indices.len();

        if has_components!(table, HEALTH) {
            for (health, &entity_id) in table.health.iter().zip(table.entity_indices.iter()) {
                if health.value <= 0.0 {
                    entities_to_remove.push(entity_id);
                }
            }
        }
    }

    metrics.system_time += system_start.elapsed();

    // Remove dead entities
    if !entities_to_remove.is_empty() {
        despawn_entities(world, &entities_to_remove);
    }

    total_processed
}

use components::*;
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
