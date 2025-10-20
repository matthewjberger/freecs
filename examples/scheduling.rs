use freecs::{Schedule, ecs};

ecs! {
    World {
        position: Position => POSITION,
        velocity: Velocity => VELOCITY,
        health: Health => HEALTH,
    }
    Events {
        damage: DamageEvent,
    }
    Resources {
        delta_time: f32,
        frame_count: u32,
    }
}

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

#[derive(Debug, Clone)]
pub struct DamageEvent {
    pub entity: freecs::Entity,
    pub amount: f32,
}

fn physics_system(world: &mut World) {
    let dt = world.resources.delta_time;

    let updates: Vec<(freecs::Entity, Velocity)> = world
        .query_entities(POSITION | VELOCITY)
        .into_iter()
        .filter_map(|entity| world.get_velocity(entity).map(|vel| (entity, *vel)))
        .collect();

    for (entity, vel) in updates {
        if let Some(pos) = world.get_position_mut(entity) {
            pos.x += vel.x * dt;
            pos.y += vel.y * dt;
        }
    }
}

fn damage_system(world: &mut World) {
    for event in world.collect_damage() {
        if let Some(health) = world.get_health_mut(event.entity) {
            health.value -= event.amount;
            println!(
                "Entity {:?} took {} damage, health: {}",
                event.entity, event.amount, health.value
            );
        }
    }
}

fn health_decay_system(world: &mut World) {
    let dt = world.resources.delta_time;

    let entities: Vec<freecs::Entity> = world.query_entities(HEALTH).into_iter().collect();

    for entity in entities {
        if let Some(health) = world.get_health_mut(entity) {
            health.value -= 1.0 * dt;
        }
    }
}

fn frame_counter_system(world: &mut World) {
    world.resources.frame_count += 1;
    if world.resources.frame_count % 60 == 0 {
        println!("Frame {}", world.resources.frame_count);
    }
}

fn main() {
    println!("=== System Scheduling Example ===\n");

    let mut world = World::default();
    world.resources.delta_time = 0.016;

    let entities = EntityBuilder::new()
        .with_position(Position { x: 0.0, y: 0.0 })
        .with_velocity(Velocity { x: 10.0, y: 5.0 })
        .with_health(Health { value: 100.0 })
        .spawn(&mut world, 3);

    println!("Created {} entities\n", entities.len());

    let mut schedule = Schedule::new();
    schedule
        .add_system_mut(frame_counter_system)
        .add_system_mut(physics_system)
        .add_system_mut(damage_system)
        .add_system_mut(health_decay_system);

    world.send_damage(DamageEvent {
        entity: entities[0],
        amount: 25.0,
    });

    println!("Running 5 frames with scheduler:\n");
    for _ in 0..5 {
        schedule.run(&mut world);
        world.step();
    }

    println!("\nFinal positions:");
    for (index, &entity) in entities.iter().enumerate() {
        if let Some(pos) = world.get_position(entity) {
            if let Some(health) = world.get_health(entity) {
                println!(
                    "  Entity {}: pos=({:.2}, {:.2}), health={:.2}",
                    index, pos.x, pos.y, health.value
                );
            }
        }
    }

    println!("\n✓ Scheduler executed all systems in order");
    println!("✓ Systems ran {} times", world.resources.frame_count);
}
