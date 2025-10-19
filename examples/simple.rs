use freecs::{Entity, ecs};

ecs! {
    World {
        position: Position => POSITION,
        velocity: Velocity => VELOCITY,
        health: Health => HEALTH,
    }
    Tags {
        player => PLAYER,
        enemy => ENEMY,
    }
    Events {
        collision: CollisionEvent,
    }
    Resources {
        delta_time: f32,
        frame_count: u32,
    }
}

pub fn main() {
    let mut world = World::default();
    world.resources.delta_time = 0.016;
    world.resources.frame_count = 0;

    println!("=== Entity Creation ===");

    let player = EntityBuilder::new()
        .with_position(Position { x: 0.0, y: 0.0 })
        .with_velocity(Velocity { x: 5.0, y: 0.0 })
        .with_health(Health { value: 100.0 })
        .spawn(&mut world, 1)[0];
    world.add_player(player);
    println!("Created player entity: {:?}", player);

    let entities = world.spawn_batch(POSITION | VELOCITY | HEALTH, 3, |table, idx| {
        table.position[idx] = Position {
            x: idx as f32 * 10.0,
            y: 0.0,
        };
        table.velocity[idx] = Velocity { x: -2.0, y: 1.0 };
        table.health[idx] = Health { value: 50.0 };
    });
    for entity in &entities {
        world.add_enemy(*entity);
    }
    println!("Created {} enemy entities", entities.len());

    println!("\n=== Component Access ===");

    if let Some(pos) = world.get_position(player) {
        println!("Player position: ({}, {})", pos.x, pos.y);
    }

    if let Some(health) = world.get_health_mut(player) {
        health.value += 10.0;
        println!("Player health increased to: {}", health.value);
    }

    world.set_velocity(player, Velocity { x: 10.0, y: 5.0 });
    println!("Player velocity updated");

    println!("\n=== Querying ===");

    let all_entities: Vec<Entity> = world.query_entities(POSITION | VELOCITY).collect();
    println!(
        "Entities with position and velocity: {}",
        all_entities.len()
    );

    for entity in world.query_entities(POSITION) {
        if world.has_enemy(entity) {
            println!("  Enemy at position: {:?}", world.get_position(entity));
        }
    }

    println!("\n=== Systems ===");

    for frame in 0..3 {
        world.resources.frame_count = frame;
        println!("Frame {}", frame);
        systems::run_systems(&mut world);
        world.step();
    }

    println!("\n=== Query Builder API ===");

    for entity in world.query_entities(POSITION | VELOCITY) {
        let pos = world.get_position(entity).unwrap();
        let is_player = world.has_player(entity);
        let tag = if is_player { "PLAYER" } else { "ENEMY" };
        println!(
            "  [{}] Entity {:?} at ({:.1}, {:.1})",
            tag, entity, pos.x, pos.y
        );
    }

    println!("\n=== Command Buffers ===");

    let entities_to_despawn: Vec<Entity> = world
        .query_entities(HEALTH)
        .filter(|&entity| world.get_health(entity).map_or(false, |h| h.value <= 0.0))
        .collect();

    if entities_to_despawn.is_empty() {
        println!("No entities to despawn (all have health > 0)");
    } else {
        println!(
            "Despawning {} entities with health <= 0",
            entities_to_despawn.len()
        );
        for entity in entities_to_despawn {
            world.queue_despawn_entity(entity);
        }
    }
    world.apply_commands();

    println!("\n=== Event Processing ===");

    let event_count = world.len_collision();
    if event_count > 0 {
        println!("Processing {} collision events", event_count);
        for event in world.collect_collision() {
            println!(
                "  Collision between {:?} and {:?}",
                event.entity_a, event.entity_b
            );
        }
    } else {
        println!("No collision events detected");
    }

    println!("\n=== Final State ===");
    println!("Total entities: {}", world.get_all_entities().len());
    println!(
        "Entities with health: {}",
        world.query_entities(HEALTH).count()
    );
}

use components::*;
mod components {
    #[derive(Default, Debug, Clone, Copy)]
    pub struct Position {
        pub x: f32,
        pub y: f32,
    }

    #[derive(Default, Debug, Clone, Copy)]
    pub struct Velocity {
        pub x: f32,
        pub y: f32,
    }

    #[derive(Default, Debug, Clone, Copy)]
    pub struct Health {
        pub value: f32,
    }
}

#[derive(Debug, Clone)]
pub struct CollisionEvent {
    pub entity_a: Entity,
    pub entity_b: Entity,
}

mod systems {
    use super::*;

    pub fn run_systems(world: &mut World) {
        physics_system(world);
        collision_system(world);
        damage_system(world);
        health_decay_system(world);
    }

    fn physics_system(world: &mut World) {
        let dt = world.resources.delta_time;

        world.for_each_mut(POSITION | VELOCITY, 0, |_entity, table, idx| {
            table.position[idx].x += table.velocity[idx].x * dt;
            table.position[idx].y += table.velocity[idx].y * dt;
        });
    }

    fn collision_system(world: &mut World) {
        let entities: Vec<(Entity, Position)> = world
            .query_entities(POSITION)
            .filter_map(|e| world.get_position(e).map(|p| (e, *p)))
            .collect();

        for idx_a in 0..entities.len() {
            for idx_b in (idx_a + 1)..entities.len() {
                let (entity_a, pos_a) = entities[idx_a];
                let (entity_b, pos_b) = entities[idx_b];

                let dx = pos_a.x - pos_b.x;
                let dy = pos_a.y - pos_b.y;
                let distance_squared = dx * dx + dy * dy;

                if distance_squared < 4.0 {
                    world.send_collision(CollisionEvent { entity_a, entity_b });
                }
            }
        }
    }

    fn damage_system(world: &mut World) {
        let collision_events = world.collect_collision();

        for event in collision_events {
            if let Some(health) = world.get_health_mut(event.entity_a) {
                health.value -= 5.0;
            }
            if let Some(health) = world.get_health_mut(event.entity_b) {
                health.value -= 5.0;
            }
        }
    }

    fn health_decay_system(world: &mut World) {
        world.for_each_health_mut(|health| {
            health.value *= 0.99;
        });
    }
}
