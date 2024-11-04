use freecs::{has_components, world};
use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::Mutex;

// Define data structures for queries
#[derive(Debug)]
struct EntityStats {
    pub _entity: EntityId,
    pub _health: f32,
    pub _distance_from_origin: f32,
}

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

pub fn main() {
    let mut world = World::default();
    world.resources.delta_time = 0.016;
    let entities = spawn_test_entities(&mut world, 1000);
    println!("Spawned {} entities", total_entities(&world));
    run_game_loop(&mut world);
    despawn_entities(&mut world, &entities);
}

fn spawn_test_entities(world: &mut World, count: usize) -> Vec<EntityId> {
    (0..count)
        .map(|index| {
            let mask = if index % 3 == 0 {
                POSITION | VELOCITY | HEALTH
            } else if index % 2 == 0 {
                POSITION | HEALTH
            } else {
                POSITION | VELOCITY
            };

            let entity = spawn_entities(world, mask, 1)[0];

            if let Some(pos) = get_component_mut::<Position>(world, entity, POSITION) {
                pos.x = (index as f32) * 2.0;
                pos.y = (index as f32) * 1.5;
            }
            if let Some(vel) = get_component_mut::<Velocity>(world, entity, VELOCITY) {
                vel.x = 1.0;
                vel.y = 0.5;
            }
            if let Some(health) = get_component_mut::<Health>(world, entity, HEALTH) {
                health.value = 100.0;
            }

            entity
        })
        .collect()
}

fn run_game_loop(world: &mut World) {
    // Parallel queries for entity data
    let entities_stats = query_entity_stats(world);
    println!("Found {} entities with stats", entities_stats.len());

    // Parralel spatial queries
    let nearby_entities = query_entities_in_radius(world, 10.0);
    println!("Found {} entities near origin", nearby_entities.len());

    // Run systems that can access the IDs of the entities they are processing
    systems::run_systems(world);

    // Run a system that tracks component interaction
    // by populating a concurrent hashmap with the results
    // of a parallel spatial query
    systems::run_interaction_system(world);
}

// Parallel query example: Collect entity stats
fn query_entity_stats(world: &World) -> Vec<EntityStats> {
    world
        .tables
        .par_iter() // Parallel iteration over tables
        .filter(|table| has_components!(table, POSITION | HEALTH))
        .flat_map(|table| {
            // Create parallel iterator over components we want
            table
                .entity_indices
                .par_iter()
                .zip(table.position.par_iter())
                .zip(table.health.par_iter())
                .map(|((entity, pos), health)| {
                    let distance = (pos.x * pos.x + pos.y * pos.y).sqrt();
                    EntityStats {
                        _entity: *entity,
                        _health: health.value,
                        _distance_from_origin: distance,
                    }
                })
        })
        .collect()
}

// Spatial query example: Find entities within radius
fn query_entities_in_radius(world: &World, radius: f32) -> Vec<EntityId> {
    world
        .tables
        .par_iter()
        .filter(|table| has_components!(table, POSITION))
        .flat_map(|table| {
            table
                .entity_indices
                .par_iter()
                .zip(table.position.par_iter())
                .filter(|(_, pos)| {
                    let distance = (pos.x * pos.x + pos.y * pos.y).sqrt();
                    distance <= radius
                })
                .map(|(entity, _)| *entity)
        })
        .collect()
}

mod systems {
    use super::*;

    // Run systems in parallel over tables
    pub fn run_systems(world: &mut World) {
        let delta_time = world.resources.delta_time;
        world.tables.par_iter_mut().for_each(|table| {
            if has_components!(table, POSITION | VELOCITY | HEALTH) {
                // Parallel movement system with access to entity IDs
                movement_system(table, delta_time);

                // Parallel spatial system
                damage_system(table);
            }
        });
    }

    // Movement system that knows the ID of the entity it is processing
    #[inline]
    fn movement_system(table: &mut ComponentArrays, dt: f32) {
        table
            .entity_indices
            .par_iter()
            .zip(table.position.par_iter_mut())
            .zip(table.velocity.par_iter())
            .for_each(|((entity, pos), vel)| {
                pos.x += vel.x * dt;
                pos.y += vel.y * dt;
                if pos.x.abs() > 100.0 || pos.y.abs() > 100.0 {
                    println!("Entity {} moved outside bounds!", entity.id);
                }
            });
    }

    // Damage entities when they go too far away from the origin
    #[inline]
    fn damage_system(table: &mut ComponentArrays) {
        table
            .entity_indices
            .par_iter()
            .zip(table.position.par_iter())
            .zip(table.health.par_iter_mut())
            .for_each(|((entity, pos), health)| {
                let distance = (pos.x * pos.x + pos.y * pos.y).sqrt();
                if distance > 50.0 {
                    health.value *= 0.99;
                    if health.value < 50.0 {
                        println!("Warning: Entity {} health critical!", entity.id);
                    }
                }
            });
    }

    // System that tracks entity interactions
    pub fn run_interaction_system(world: &mut World) {
        // Create a thread-safe HashMap to track entity interactions
        let interactions = Mutex::new(HashMap::new());

        world.tables.par_iter().for_each(|table| {
            if has_components!(table, POSITION | HEALTH) {
                // Find entities that are close to each other
                for first_entity in 0..table.entity_indices.len() {
                    for second_entity in (first_entity + 1)..table.entity_indices.len() {
                        let pos1 = &table.position[first_entity];
                        let pos2 = &table.position[second_entity];
                        let delta_x = pos2.x - pos1.x;
                        let delta_y = pos2.y - pos1.y;
                        let distance = (delta_x * delta_x + delta_y * delta_y).sqrt();

                        if distance < 5.0 {
                            let mut interactions = interactions.lock().unwrap();
                            interactions.insert(
                                (
                                    table.entity_indices[first_entity],
                                    table.entity_indices[second_entity],
                                ),
                                distance,
                            );
                        }
                    }
                }
            }
        });

        // Process recorded interactions
        let interactions = interactions.into_inner().unwrap();
        for ((entity1, entity2), distance) in interactions {
            println!(
                "Entities {} and {} are interacting at distance {}",
                entity1.id, entity2.id, distance
            );
        }
    }
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
