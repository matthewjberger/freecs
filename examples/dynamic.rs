use freecs::dynamic::DynWorld;

#[derive(Default, Clone, Debug, PartialEq)]
struct Position {
    x: f32,
    y: f32,
}

#[derive(Default, Clone, Debug)]
struct Velocity {
    x: f32,
    y: f32,
}

#[derive(Default, Clone, Debug)]
struct Health {
    value: f32,
}

#[derive(Debug, Clone)]
struct CollisionEvent {
    entity_a: freecs::Entity,
    entity_b: freecs::Entity,
}

struct Selected;

fn main() {
    let mut world = DynWorld::new();
    world.insert_resource(1.0f32);

    println!("=== Spawning with bundles, types register lazily ===");
    let player = world.spawn((
        Position { x: 0.0, y: 0.0 },
        Velocity { x: 1.0, y: 0.5 },
        Health { value: 100.0 },
    ));
    let enemy = world.spawn((Position { x: 4.0, y: 2.0 }, Velocity { x: -1.0, y: 0.0 }));
    let landmark = world.spawn((Position { x: 10.0, y: 10.0 },));
    world.add_tag_type::<Selected>(player);

    println!("player: {:?}", world.get::<Position>(player));
    println!("enemy:  {:?}", world.get::<Position>(enemy));

    println!("\n=== Typed queries, mutability from the tuple ===");
    let mut collision_cursor = 0;
    for frame in 0..3 {
        let delta_time = *world.res::<f32>();

        world
            .query::<(&mut Position, &Velocity)>()
            .for_each(|_entity, (position, velocity)| {
                position.x += velocity.x * delta_time;
                position.y += velocity.y * delta_time;
            });

        world
            .query::<&Position>()
            .changed::<Position>()
            .for_each(|entity, position| {
                println!(
                    "frame {frame}: redraw {entity}: ({:.1}, {:.1})",
                    position.x, position.y
                );
            });

        let collisions: Vec<(freecs::Entity, freecs::Entity)> = world
            .query_ref::<&Position>()
            .iter_combinations()
            .filter(|((_, a), (_, b))| {
                let delta_x = a.x - b.x;
                let delta_y = a.y - b.y;
                delta_x * delta_x + delta_y * delta_y < 4.0
            })
            .map(|((entity_a, _), (entity_b, _))| (entity_a, entity_b))
            .collect();
        for (entity_a, entity_b) in collisions {
            world.send(CollisionEvent { entity_a, entity_b });
        }

        for event in world.consume_events::<CollisionEvent>(&mut collision_cursor) {
            println!(
                "frame {frame}: collision between {} and {}",
                event.entity_a, event.entity_b
            );
        }

        world.step();
    }

    println!("\n=== Tag and filter queries ===");
    world
        .query::<(&Position,)>()
        .with_tag_type::<Selected>()
        .for_each(|entity, (position,)| {
            println!(
                "selected {entity} at ({:.1}, {:.1})",
                position.x, position.y
            );
        });

    world
        .query::<(&Position,)>()
        .without::<Velocity>()
        .for_each(|entity, _| {
            println!("static scenery: {entity}");
        });

    println!("\n=== Optional elements and read-only iterators ===");
    world
        .query::<(&Position, Option<&Health>)>()
        .for_each(|entity, (_position, health)| match health {
            Some(health) => println!("{entity} has {:.0} health", health.value),
            None => println!("{entity} is indestructible"),
        });

    let names: Vec<String> = world
        .query_ref::<(&Position, Option<&Velocity>)>()
        .iter()
        .map(|(entity, (position, velocity))| {
            let motion = if velocity.is_some() {
                "moving"
            } else {
                "still"
            };
            format!(
                "{entity} {motion} at ({:.1}, {:.1})",
                position.x, position.y
            )
        })
        .collect();
    println!("{}", names.join("\n"));

    println!("\n=== Resource scope ===");
    world.resource_scope(|world, delta_time: &mut f32| {
        *delta_time = 0.5;
        world
            .query::<(&mut Position, &Velocity)>()
            .for_each(|_entity, (position, velocity)| {
                position.x += velocity.x * *delta_time;
                position.y += velocity.y * *delta_time;
            });
    });
    println!("player: {:?}", world.get::<Position>(player));

    println!("\n=== Resources scope, the tuple form ===");
    world.insert_resource(0u32);
    world.resources_scope(|world, (frame_count, delta_time): &mut (u32, f32)| {
        world
            .query::<(&mut Position, &Velocity)>()
            .for_each(|_entity, (position, velocity)| {
                position.x += velocity.x * *delta_time;
                position.y += velocity.y * *delta_time;
            });
        *frame_count += 1;
    });
    println!(
        "player: {:?} after {} scoped frame(s)",
        world.get::<Position>(player),
        world.res::<u32>()
    );

    println!("\n=== Deferred commands ===");
    world.queue_despawn_entity(enemy);
    world.queue_set(player, Health { value: 250.0 });
    world.apply_commands();

    println!("enemy alive: {}", world.is_alive(enemy));
    let player_health = world.get::<Health>(player).map(|health| health.value);
    println!("player health: {player_health:?}");
    println!("entities remaining: {}", world.entity_count());
    let _ = landmark;
}
