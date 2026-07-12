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
    for frame in 0..3 {
        let delta_time = *world.resource::<f32>().unwrap();

        world
            .query::<(&mut Position, &Velocity)>()
            .for_each(|_entity, (position, velocity)| {
                position.x += velocity.x * delta_time;
                position.y += velocity.y * delta_time;
            });

        world
            .query::<(&Position,)>()
            .changed::<Position>()
            .for_each(|entity, (position,)| {
                println!(
                    "frame {frame}: redraw {entity}: ({:.1}, {:.1})",
                    position.x, position.y
                );
            });

        let mut collisions = Vec::new();
        world
            .query::<(&Position,)>()
            .for_each(|entity, (position,)| {
                let player_position = Position {
                    x: position.x,
                    y: position.y,
                };
                collisions.push((entity, player_position));
            });
        for index in 0..collisions.len() {
            for other in index + 1..collisions.len() {
                let (entity_a, ref a) = collisions[index];
                let (entity_b, ref b) = collisions[other];
                let delta_x = a.x - b.x;
                let delta_y = a.y - b.y;
                if delta_x * delta_x + delta_y * delta_y < 4.0 {
                    world.send(CollisionEvent { entity_a, entity_b });
                }
            }
        }

        let mut cursor = 0;
        for event in world.read_events_since::<CollisionEvent>(cursor) {
            println!(
                "frame {frame}: collision between {} and {}",
                event.entity_a, event.entity_b
            );
        }
        cursor = world.event_sequence::<CollisionEvent>();
        world.trim_events::<CollisionEvent>(cursor);

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
