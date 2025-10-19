use macroquad::prelude::*;
use std::f32::consts::PI;

freecs::ecs! {
    World {
        position: Position => POSITION,
        velocity: Velocity => VELOCITY,
        rotation: Rotation => ROTATION,
        shape: Shape => SHAPE,
        player: Player => PLAYER,
        asteroid: Asteroid => ASTEROID,
        bullet: Bullet => BULLET,
        lifetime: Lifetime => LIFETIME,
    }
    Resources {
        score: Score,
        game_state: GameState,
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Position(Vec2);

#[derive(Debug, Clone, Copy, Default)]
pub struct Velocity(Vec2);

#[derive(Debug, Clone, Copy, Default)]
pub struct Rotation {
    angle: f32,
    speed: f32,
}

#[derive(Debug, Clone, Default)]
pub struct Shape(Vec<Vec2>, f32);

#[derive(Debug, Clone, Copy, Default)]
pub struct Player {
    thrusting: bool,
    shoot_cooldown: f32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Asteroid(AsteroidSize);

#[derive(Debug, Clone, Copy, Default)]
enum AsteroidSize {
    #[default]
    Large,
    Medium,
    Small,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Bullet;

#[derive(Debug, Clone, Copy, Default)]
pub struct Lifetime(f32);

#[derive(Debug, Default)]
struct Score(u32);

#[derive(Debug, Default)]
enum GameState {
    #[default]
    Playing,
    GameOver,
}

fn ship_shape() -> Shape {
    Shape(
        vec![
            Vec2::new(0.0, -10.0),
            Vec2::new(-7.0, 10.0),
            Vec2::new(0.0, 5.0),
            Vec2::new(7.0, 10.0),
        ],
        1.0,
    )
}

fn asteroid_shape(scale: f32) -> Shape {
    Shape(
        (0..12)
            .map(|index| {
                let angle = 2.0 * PI * index as f32 / 12.0;
                let radius = 1.0 + (index as f32 * 0.1).sin() * 0.3;
                Vec2::new(angle.cos() * radius, angle.sin() * radius)
            })
            .collect(),
        scale,
    )
}

fn spawn_player(world: &mut World) {
    let entity = world.spawn_entities(POSITION | VELOCITY | ROTATION | SHAPE | PLAYER, 1)[0];
    world.set_position(
        entity,
        Position(Vec2::new(screen_width() / 2.0, screen_height() / 2.0)),
    );
    world.set_velocity(entity, Velocity(Vec2::ZERO));
    world.set_rotation(
        entity,
        Rotation {
            angle: 0.0,
            speed: 0.0,
        },
    );
    world.set_shape(entity, ship_shape());
    world.set_player(
        entity,
        Player {
            thrusting: false,
            shoot_cooldown: 0.0,
        },
    );
}

fn spawn_asteroid(world: &mut World, position: Vec2, size: AsteroidSize) {
    let scale = match size {
        AsteroidSize::Large => 40.0,
        AsteroidSize::Medium => 25.0,
        AsteroidSize::Small => 15.0,
    };

    let _entities = world.spawn_batch(
        POSITION | VELOCITY | ROTATION | SHAPE | ASTEROID,
        1,
        |table, idx| {
            table.position[idx] = Position(position);
            table.velocity[idx] = Velocity(Vec2::new(
                rand::gen_range(-100.0, 100.0),
                rand::gen_range(-100.0, 100.0),
            ));
            table.rotation[idx] = Rotation {
                angle: rand::gen_range(0.0, 2.0 * PI),
                speed: rand::gen_range(-2.0, 2.0),
            };
            table.shape[idx] = asteroid_shape(scale);
            table.asteroid[idx] = Asteroid(size);
        },
    );
}

fn spawn_bullet(world: &mut World, position: Vec2, angle: f32) {
    let direction = angle - PI / 2.0;
    let entity = world.spawn_entities(POSITION | VELOCITY | SHAPE | BULLET | LIFETIME, 1)[0];
    world.set_position(entity, Position(position));
    world.set_velocity(
        entity,
        Velocity(Vec2::new(direction.cos(), direction.sin()) * 500.0),
    );
    world.set_shape(entity, Shape(vec![Vec2::ZERO], 3.0));
    world.set_bullet(entity, Bullet);
    world.set_lifetime(entity, Lifetime(1.0));
}

fn player_input_system(world: &mut World) {
    let delta_time = get_frame_time();
    let thrusting = is_key_down(KeyCode::W);

    world
        .query_mut()
        .with(PLAYER | VELOCITY | ROTATION | POSITION)
        .iter(|_entity, table, idx| {
            if is_key_down(KeyCode::A) {
                table.rotation[idx].angle -= 5.0 * delta_time;
            }
            if is_key_down(KeyCode::D) {
                table.rotation[idx].angle += 5.0 * delta_time;
            }

            table.player[idx].thrusting = thrusting;
            table.player[idx].shoot_cooldown =
                (table.player[idx].shoot_cooldown - delta_time).max(0.0);

            if thrusting {
                let angle = table.rotation[idx].angle - PI / 2.0;
                table.velocity[idx].0 += Vec2::new(angle.cos(), angle.sin()) * 300.0 * delta_time;
                if table.velocity[idx].0.length() > 400.0 {
                    table.velocity[idx].0 = table.velocity[idx].0.normalize() * 400.0;
                }
            }

            table.velocity[idx].0 *= 0.99;
        });

    if is_key_down(KeyCode::Space) {
        let mut bullets_to_spawn = Vec::new();

        world
            .query()
            .with(PLAYER | POSITION | ROTATION)
            .iter(|_entity, table, idx| {
                if table.player[idx].shoot_cooldown <= 0.0 {
                    let angle = table.rotation[idx].angle - PI / 2.0;
                    let spawn_position =
                        table.position[idx].0 + Vec2::new(angle.cos(), angle.sin()) * 15.0;
                    bullets_to_spawn.push((spawn_position, table.rotation[idx].angle));
                }
            });

        for (position, angle) in bullets_to_spawn {
            spawn_bullet(world, position, angle);
        }

        world.iter_player_mut(|_entity, player| {
            if player.shoot_cooldown <= 0.0 {
                player.shoot_cooldown = 0.1;
            }
        });
    }
}

fn movement_system(world: &mut World) {
    let delta_time = get_frame_time();
    let (screen_w, screen_h) = (screen_width(), screen_height());

    world
        .query_mut()
        .with(POSITION | VELOCITY)
        .iter(|_entity, table, idx| {
            table.position[idx].0 += table.velocity[idx].0 * delta_time;
            if table.position[idx].0.x < 0.0 {
                table.position[idx].0.x = screen_w;
            } else if table.position[idx].0.x > screen_w {
                table.position[idx].0.x = 0.0;
            }
            if table.position[idx].0.y < 0.0 {
                table.position[idx].0.y = screen_h;
            } else if table.position[idx].0.y > screen_h {
                table.position[idx].0.y = 0.0;
            }
        });

    world.iter_rotation_mut(|_entity, rotation| {
        rotation.angle += rotation.speed * delta_time;
    });
}

fn lifetime_system(world: &mut World) {
    let delta_time = get_frame_time();
    let mut entities_to_remove = Vec::new();

    world.iter_lifetime_mut(|entity, lifetime| {
        lifetime.0 -= delta_time;
        if lifetime.0 <= 0.0 {
            entities_to_remove.push(entity);
        }
    });

    for entity in entities_to_remove {
        world.queue_despawn_entity(entity);
    }

    world.apply_commands();
}

fn collision_system(world: &mut World) {
    let mut collisions = Vec::new();
    let mut player_hit = false;

    let mut bullets = Vec::new();
    world
        .query()
        .with(BULLET | POSITION)
        .iter(|entity, table, idx| {
            bullets.push((entity, table.position[idx].0));
        });

    let mut asteroids = Vec::new();
    world
        .query()
        .with(ASTEROID | POSITION | SHAPE)
        .iter(|entity, table, idx| {
            asteroids.push((
                entity,
                table.asteroid[idx],
                table.position[idx].0,
                table.shape[idx].1,
            ));
        });

    for (bullet, bullet_position) in &bullets {
        for (asteroid, asteroid_data, asteroid_position, shape_radius) in &asteroids {
            if (*bullet_position - *asteroid_position).length() < *shape_radius {
                collisions.push((*bullet, *asteroid, *asteroid_data, *asteroid_position));
            }
        }
    }

    let mut players = Vec::new();
    world
        .query()
        .with(PLAYER | POSITION)
        .iter(|entity, table, idx| {
            players.push((entity, table.position[idx].0));
        });

    for (_player, player_position) in &players {
        for (_asteroid, _asteroid_data, asteroid_position, shape_radius) in &asteroids {
            if (*player_position - *asteroid_position).length() < shape_radius + 10.0 {
                player_hit = true;
                break;
            }
        }
        if player_hit {
            break;
        }
    }

    if player_hit {
        world.resources.game_state = GameState::GameOver;
    }

    for (bullet, asteroid, asteroid_data, position) in collisions {
        world.queue_despawn_entity(bullet);
        world.queue_despawn_entity(asteroid);
        award_points_and_spawn_fragments(world, asteroid_data, position);
    }

    world.apply_commands();
}

fn award_points_and_spawn_fragments(world: &mut World, asteroid: Asteroid, position: Vec2) {
    let (points, new_size) = match asteroid.0 {
        AsteroidSize::Large => (20, Some(AsteroidSize::Medium)),
        AsteroidSize::Medium => (50, Some(AsteroidSize::Small)),
        AsteroidSize::Small => (100, None),
    };
    world.resources.score.0 += points;
    if let Some(size) = new_size {
        spawn_asteroid(world, position, size);
        spawn_asteroid(world, position, size);
    }
}

fn render_system(world: &World) {
    clear_background(BLACK);

    world
        .query()
        .with(POSITION | ROTATION | SHAPE)
        .iter(|entity, table, idx| {
            let position = &table.position[idx];
            let rotation = &table.rotation[idx];
            let shape = &table.shape[idx];

            let points: Vec<_> = shape
                .0
                .iter()
                .map(|point| {
                    let (sine, cosine) = rotation.angle.sin_cos();
                    position.0
                        + Vec2::new(
                            point.x * cosine - point.y * sine,
                            point.x * sine + point.y * cosine,
                        ) * shape.1
                })
                .collect();

            for index in 0..points.len() {
                let (start, end) = (points[index], points[(index + 1) % points.len()]);
                draw_line(start.x, start.y, end.x, end.y, 2.0, WHITE);
            }

            if let Some(player) = world.get_player(entity) {
                if player.thrusting {
                    render_thrust(position, rotation);
                }
            }
        });

    world
        .query()
        .with(BULLET | POSITION)
        .iter(|_entity, table, idx| {
            draw_circle(table.position[idx].0.x, table.position[idx].0.y, 3.0, WHITE);
        });

    draw_text(
        &format!("Score: {}", world.resources.score.0),
        10.0,
        30.0,
        30.0,
        WHITE,
    );

    if let GameState::GameOver = world.resources.game_state {
        let text = "GAME OVER";
        let font_size = 60.0;
        let dimensions = measure_text(text, None, font_size as u16, 1.0);
        draw_text(
            text,
            screen_width() / 2.0 - dimensions.width / 2.0,
            screen_height() / 2.0,
            font_size,
            RED,
        );
    }
}

fn render_thrust(position: &Position, rotation: &Rotation) {
    let angle = rotation.angle + PI / 2.0;
    let base_position = position.0 + Vec2::new(angle.cos(), angle.sin()) * 5.0;

    for index in 0..3 {
        let offset = (index as f32 - 1.0) * 0.3;
        let flame_angle = angle + offset;
        let length = rand::gen_range(10.0, 20.0);
        let (sine, cosine) = flame_angle.sin_cos();

        let tip = base_position + Vec2::new(cosine, sine) * length;
        let left =
            base_position + Vec2::new((flame_angle - 0.5).cos(), (flame_angle - 0.5).sin()) * 5.0;
        let right =
            base_position + Vec2::new((flame_angle + 0.5).cos(), (flame_angle + 0.5).sin()) * 5.0;

        let color = if index == 1 { YELLOW } else { ORANGE };
        draw_triangle(base_position, left, tip, color);
        draw_triangle(base_position, right, tip, color);
    }
}

#[macroquad::main("Asteroids")]
async fn main() {
    let mut world = World::default();
    spawn_player(&mut world);
    for _ in 0..5 {
        spawn_asteroid(
            &mut world,
            Vec2::new(
                rand::gen_range(0.0, screen_width()),
                rand::gen_range(0.0, screen_height()),
            ),
            AsteroidSize::Large,
        );
    }

    loop {
        match world.resources.game_state {
            GameState::Playing => {
                player_input_system(&mut world);
                movement_system(&mut world);
                lifetime_system(&mut world);
                collision_system(&mut world);
            }
            GameState::GameOver => {
                if is_key_pressed(KeyCode::R) {
                    world = World::default();
                    spawn_player(&mut world);
                    for _ in 0..5 {
                        spawn_asteroid(
                            &mut world,
                            Vec2::new(
                                rand::gen_range(0.0, screen_width()),
                                rand::gen_range(0.0, screen_height()),
                            ),
                            AsteroidSize::Large,
                        );
                    }
                }
            }
        }

        render_system(&world);
        if let GameState::GameOver = world.resources.game_state {
            draw_text("Press R to restart", 10.0, 60.0, 20.0, WHITE);
        }

        next_frame().await
    }
}
