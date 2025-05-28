use freecs::{ecs, table_has_components};
use macroquad::prelude::*;

ecs! {
    World {
        position: Position => POSITION,
        rotation: Rotation => ROTATION,
        velocity: Velocity => VELOCITY,
        player: Player => PLAYER,
        thrust: Thrust => THRUST,
        projectile: Projectile => PROJECTILE,
        asteroid: Asteroid => ASTEROID,
        radius: Radius => RADIUS,
        lifetime: Lifetime => LIFETIME,
    }
    Resources {
        delta_time: f32,
        last_shot_time: f32,
        score: u32
    }
}

use components::*;
mod components {
    #[derive(Default, Debug, Clone, Copy)]
    pub struct Position {
        pub x: f32,
        pub y: f32,
    }

    #[derive(Default, Debug, Clone, Copy)]
    pub struct Rotation {
        pub radians: f32,
    }

    #[derive(Default, Debug, Clone, Copy)]
    pub struct Velocity {
        pub x: f32,
        pub y: f32,
    }

    #[derive(Default, Debug, Clone, Copy)]
    pub struct Player;

    #[derive(Default, Debug, Clone, Copy)]
    pub struct Thrust {
        pub power: f32,
    }

    #[derive(Default, Debug, Clone, Copy)]
    pub struct Projectile;

    #[derive(Default, Debug, Clone, Copy)]
    pub struct Asteroid;

    #[derive(Default, Debug, Clone, Copy)]
    pub struct Radius {
        pub value: f32,
    }

    #[derive(Default, Debug, Clone, Copy)]
    pub struct Lifetime {
        pub remaining: f32,
    }
}

const ASTEROID_SCORE: u32 = 100;

mod systems {
    use super::*;
    use std::f32::consts::PI;

    const PROJECTILE_SPEED: f32 = 500.0;
    const PROJECTILE_LIFETIME: f32 = 1.0;
    const SHOOT_COOLDOWN: f32 = 0.15;

    pub fn run_systems(world: &mut World, dt: f32) {
        handle_input(world, dt);

        let screen_w = screen_width();
        let screen_h = screen_height();

        world.tables.iter_mut().for_each(|table| {
            if table_has_components!(table, POSITION | VELOCITY) {
                movement_system(table, dt);
            }

            if table_has_components!(table, POSITION) {
                wrap_position_system(table, screen_w, screen_h);
            }

            if table_has_components!(table, PLAYER | VELOCITY) {
                damping_system(table);
            }

            if table_has_components!(table, LIFETIME) {
                lifetime_system(table, dt);
            }
        });

        handle_collisions(world);
    }

    fn handle_input(world: &mut World, dt: f32) {
        if is_key_down(KeyCode::Space) {
            let current_time = get_time() as f32;
            if current_time - world.resources.last_shot_time >= SHOOT_COOLDOWN {
                spawn_projectile(world);
                world.resources.last_shot_time = current_time;
            }
        }

        for table in &mut world.tables {
            if table_has_components!(table, PLAYER | ROTATION | VELOCITY | THRUST) {
                for i in 0..table.entity_indices.len() {
                    let rot = &mut table.rotation[i];
                    let vel = &mut table.velocity[i];
                    let thrust = &table.thrust[i];

                    const ROTATION_SPEED: f32 = 5.0;
                    if is_key_down(KeyCode::Left) {
                        rot.radians -= ROTATION_SPEED * dt;
                    }
                    if is_key_down(KeyCode::Right) {
                        rot.radians += ROTATION_SPEED * dt;
                    }

                    while rot.radians < 0.0 {
                        rot.radians += 2.0 * PI;
                    }
                    while rot.radians >= 2.0 * PI {
                        rot.radians -= 2.0 * PI;
                    }

                    if is_key_down(KeyCode::Up) {
                        vel.x += rot.radians.cos() * thrust.power;
                        vel.y += rot.radians.sin() * thrust.power;
                    }
                }
            }
        }
    }

    fn spawn_projectile(world: &mut World) {
        let mut player_pos = Position::default();
        let mut player_rot = Rotation::default();

        for table in &world.tables {
            if table_has_components!(table, PLAYER | POSITION | ROTATION) {
                for i in 0..table.entity_indices.len() {
                    player_pos = table.position[i];
                    player_rot = table.rotation[i];
                }
            }
        }

        let projectile = spawn_entities(
            world,
            POSITION | VELOCITY | PROJECTILE | RADIUS | LIFETIME,
            1,
        )[0];

        if let Some(pos) = get_component_mut::<Position>(world, projectile, POSITION) {
            pos.x = player_pos.x + player_rot.radians.cos() * 20.0;
            pos.y = player_pos.y + player_rot.radians.sin() * 20.0;
        }

        if let Some(vel) = get_component_mut::<Velocity>(world, projectile, VELOCITY) {
            vel.x = player_rot.radians.cos() * PROJECTILE_SPEED;
            vel.y = player_rot.radians.sin() * PROJECTILE_SPEED;
        }

        if let Some(radius) = get_component_mut::<Radius>(world, projectile, RADIUS) {
            radius.value = 2.0;
        }

        if let Some(lifetime) = get_component_mut::<Lifetime>(world, projectile, LIFETIME) {
            lifetime.remaining = PROJECTILE_LIFETIME;
        }
    }

    fn movement_system(table: &mut ComponentArrays, dt: f32) {
        for (pos, vel) in table.position.iter_mut().zip(table.velocity.iter()) {
            pos.x += vel.x * dt;
            pos.y += vel.y * dt;
        }
    }

    fn wrap_position_system(table: &mut ComponentArrays, screen_width: f32, screen_height: f32) {
        for pos in table.position.iter_mut() {
            if pos.x < 0.0 {
                pos.x += screen_width;
            }
            if pos.x > screen_width {
                pos.x -= screen_width;
            }
            if pos.y < 0.0 {
                pos.y += screen_height;
            }
            if pos.y > screen_height {
                pos.y -= screen_height;
            }
        }
    }

    fn damping_system(table: &mut ComponentArrays) {
        const DAMPING: f32 = 0.999;
        for vel in table.velocity.iter_mut() {
            vel.x *= DAMPING;
            vel.y *= DAMPING;
        }
    }

    fn lifetime_system(table: &mut ComponentArrays, dt: f32) {
        for lifetime in table.lifetime.iter_mut() {
            lifetime.remaining -= dt;
        }
    }

    fn handle_collisions(world: &mut World) {
        let mut to_despawn = Vec::new();
        let mut projectile_positions = Vec::new();
        let mut asteroid_data = Vec::new();

        for table in &world.tables {
            if table_has_components!(table, PROJECTILE | POSITION | RADIUS | LIFETIME) {
                for i in 0..table.entity_indices.len() {
                    if table.lifetime[i].remaining > 0.0 {
                        projectile_positions.push((
                            table.entity_indices[i],
                            table.position[i],
                            table.radius[i].value,
                        ));
                    }
                }
            }
            if table_has_components!(table, ASTEROID | POSITION | RADIUS) {
                for i in 0..table.entity_indices.len() {
                    asteroid_data.push((
                        table.entity_indices[i],
                        table.position[i],
                        table.radius[i].value,
                    ));
                }
            }
        }

        let mut asteroids_destroyed = 0;

        for (proj_entity, proj_pos, proj_radius) in projectile_positions {
            for (ast_entity, ast_pos, ast_radius) in &asteroid_data {
                let dx = proj_pos.x - ast_pos.x;
                let dy = proj_pos.y - ast_pos.y;
                let distance = (dx * dx + dy * dy).sqrt();

                if distance < (proj_radius + ast_radius) {
                    to_despawn.push(proj_entity);
                    to_despawn.push(*ast_entity);
                    asteroids_destroyed += 1;
                }
            }
        }

        // Update score
        world.resources.score += asteroids_destroyed * ASTEROID_SCORE;

        // Despawn collided entities
        if !to_despawn.is_empty() {
            despawn_entities(world, &to_despawn);
        }
    }
}

#[macroquad::main("Asteroids")]
async fn main() {
    let mut world = World::default();

    // Initialize resources
    world.resources.last_shot_time = 0.0;
    world.resources.score = 0;

    // Spawn player
    let player = spawn_entities(
        &mut world,
        POSITION | ROTATION | VELOCITY | PLAYER | THRUST,
        1,
    )[0];

    if let Some(pos) = get_component_mut::<Position>(&mut world, player, POSITION) {
        pos.x = screen_width() / 2.0;
        pos.y = screen_height() / 2.0;
    }

    if let Some(thrust) = get_component_mut::<Thrust>(&mut world, player, THRUST) {
        thrust.power = 1.0;
    }

    // Spawn initial asteroids
    for _ in 0..10 {
        let asteroid = spawn_entities(&mut world, POSITION | VELOCITY | ASTEROID | RADIUS, 1)[0];

        if let Some(pos) = get_component_mut::<Position>(&mut world, asteroid, POSITION) {
            pos.x = rand::gen_range(0.0, screen_width());
            pos.y = rand::gen_range(0.0, screen_height());
        }

        if let Some(vel) = get_component_mut::<Velocity>(&mut world, asteroid, VELOCITY) {
            let angle = rand::gen_range(0.0, std::f32::consts::PI * 2.0);
            let speed = rand::gen_range(50.0, 100.0);
            vel.x = angle.cos() * speed;
            vel.y = angle.sin() * speed;
        }

        if let Some(radius) = get_component_mut::<Radius>(&mut world, asteroid, RADIUS) {
            radius.value = 20.0;
        }
    }

    loop {
        clear_background(BLACK);
        let dt = get_frame_time();

        systems::run_systems(&mut world, dt);

        // Render everything
        for table in &world.tables {
            // Render player
            if table_has_components!(table, PLAYER | POSITION | ROTATION) {
                for i in 0..table.entity_indices.len() {
                    let pos = &table.position[i];
                    let rot = &table.rotation[i];

                    let vertices = [
                        Vec2::new(15.0, 0.0),
                        Vec2::new(-15.0, -10.0),
                        Vec2::new(-15.0, 10.0),
                    ];

                    let transformed: Vec<Vec2> = vertices
                        .iter()
                        .map(|v| {
                            let x = v.x * rot.radians.cos() - v.y * rot.radians.sin() + pos.x;
                            let y = v.x * rot.radians.sin() + v.y * rot.radians.cos() + pos.y;
                            Vec2::new(x, y)
                        })
                        .collect();

                    draw_triangle_lines(transformed[0], transformed[1], transformed[2], 1.5, WHITE);

                    if is_key_down(KeyCode::Up) {
                        let flame = [
                            Vec2::new(-15.0, 0.0),
                            Vec2::new(-25.0, -5.0),
                            Vec2::new(-25.0, 5.0),
                        ];

                        let flame_transformed: Vec<Vec2> = flame
                            .iter()
                            .map(|v| {
                                let x = v.x * rot.radians.cos() - v.y * rot.radians.sin() + pos.x;
                                let y = v.x * rot.radians.sin() + v.y * rot.radians.cos() + pos.y;
                                Vec2::new(x, y)
                            })
                            .collect();

                        draw_triangle_lines(
                            flame_transformed[0],
                            flame_transformed[1],
                            flame_transformed[2],
                            1.5,
                            RED,
                        );
                    }
                }
            }

            // Render projectiles
            if table_has_components!(table, PROJECTILE | POSITION | LIFETIME) {
                for i in 0..table.entity_indices.len() {
                    if table.lifetime[i].remaining > 0.0 {
                        let pos = &table.position[i];
                        draw_circle(pos.x, pos.y, 2.0, YELLOW);
                    }
                }
            }

            // Render asteroids
            if table_has_components!(table, ASTEROID | POSITION | RADIUS) {
                for i in 0..table.entity_indices.len() {
                    let pos = &table.position[i];
                    let radius = table.radius[i].value;
                    draw_circle_lines(pos.x, pos.y, radius, 1.5, WHITE);
                }
            }
        }

        // Draw score
        draw_text(
            &format!("SCORE: {}", world.resources.score),
            20.0,
            40.0,
            30.0,
            WHITE,
        );

        next_frame().await;
    }
}
