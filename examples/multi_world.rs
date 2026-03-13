use freecs::{Entity, Schedule, ecs};

#[derive(Default, Debug, Clone)]
pub struct Position {
    pub x: f32,
    pub y: f32,
}

#[derive(Default, Debug, Clone)]
pub struct Velocity {
    pub x: f32,
    pub y: f32,
}

#[derive(Default, Debug, Clone)]
pub struct Sprite {
    pub id: u32,
}

#[derive(Default, Debug, Clone)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

#[derive(Debug, Clone)]
pub struct CollisionEvent {
    pub entity_a: Entity,
    pub entity_b: Entity,
}

ecs! {
    GameEcs {
        CoreWorld {
            position: Position => POSITION,
            velocity: Velocity => VELOCITY,
        }
        RenderWorld {
            sprite: Sprite => SPRITE,
            color: Color => COLOR,
        }
    }
    Tags { player => PLAYER }
    Events { collision: CollisionEvent }
    GameResources { delta_time: f32 }
}

fn physics_system(ecs: &mut GameEcs) {
    let dt = ecs.resources.delta_time;
    ecs.core_world
        .for_each_mut(POSITION | VELOCITY, 0, |_entity, table, idx| {
            table.position[idx].x += table.velocity[idx].x * dt;
            table.position[idx].y += table.velocity[idx].y * dt;
        });
}

fn render_system(ecs: &GameEcs) {
    let GameEcs {
        core_world,
        render_world,
        player,
        ..
    } = ecs;

    core_world.for_each(POSITION, 0, |entity, table, idx| {
        let tag = if player.contains(&entity) {
            " [PLAYER]"
        } else {
            ""
        };
        if let Some(sprite) = render_world.get_sprite(entity) {
            println!(
                "  Entity {:?}: pos=({:.1}, {:.1}), sprite={}{tag}",
                entity, table.position[idx].x, table.position[idx].y, sprite.id,
            );
        } else {
            println!(
                "  Entity {:?}: pos=({:.1}, {:.1}), no sprite{tag}",
                entity, table.position[idx].x, table.position[idx].y,
            );
        }
    });
}

fn main() {
    let mut ecs = GameEcs::default();
    ecs.resources.delta_time = 1.0 / 60.0;

    let entities = EntityBuilder::new()
        .with_position(Position { x: 0.0, y: 0.0 })
        .with_velocity(Velocity { x: 60.0, y: 30.0 })
        .with_sprite(Sprite { id: 1 })
        .with_color(Color {
            r: 1.0,
            g: 0.0,
            b: 0.0,
        })
        .spawn(&mut ecs, 1);
    ecs.add_player(entities[0]);

    let bg_entity = ecs.spawn();
    ecs.core_world
        .set_position(bg_entity, Position { x: 100.0, y: 50.0 });

    let mut schedule: Schedule<GameEcs> = Schedule::new();
    schedule.add_system_mut(physics_system);
    schedule.add_system(render_system);

    println!("Multi-world ECS: {} worlds, {} entities", 2, 2);
    println!(
        "Mask independence: POSITION={}, SPRITE={} (both bit 0)",
        POSITION, SPRITE
    );
    println!();

    for frame in 0..3 {
        println!("--- Frame {frame} ---");
        schedule.run(&mut ecs);
        ecs.step();
        println!();
    }
}
