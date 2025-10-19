use freecs::{Entity, ecs};
use macroquad::prelude::*;

ecs! {
    World {
        position: Position => POSITION,
        velocity: Velocity => VELOCITY,
        paddle: Paddle => PADDLE,
        ball: Ball => BALL,
    }
    Resources {
        score_left: u32,
        score_right: u32,
        game_over: bool,
        winner: Option<String>,
    }
}

#[derive(Default, Debug, Clone, Copy)]
pub struct Position {
    x: f32,
    y: f32,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct Velocity {
    x: f32,
    y: f32,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct Paddle {
    width: f32,
    height: f32,
    speed: f32,
    _is_left: bool,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct Ball {
    radius: f32,
}

#[macroquad::main("Pong")]
async fn main() {
    let mut world = World::default();

    let left_paddle = EntityBuilder::new()
        .with_position(Position { x: 50.0, y: 300.0 })
        .with_paddle(Paddle {
            width: 20.0,
            height: 100.0,
            speed: 400.0,
            _is_left: true,
        })
        .spawn(&mut world, 1)[0];

    let right_paddle = EntityBuilder::new()
        .with_position(Position { x: 750.0, y: 300.0 })
        .with_paddle(Paddle {
            width: 20.0,
            height: 100.0,
            speed: 400.0,
            _is_left: false,
        })
        .spawn(&mut world, 1)[0];

    let ball = EntityBuilder::new()
        .with_position(Position { x: 400.0, y: 300.0 })
        .with_velocity(Velocity { x: 300.0, y: 200.0 })
        .with_ball(Ball { radius: 10.0 })
        .spawn(&mut world, 1)[0];

    loop {
        clear_background(BLACK);

        if world.resources.game_over {
            render_game_over(&world);
            if is_key_pressed(KeyCode::Space) {
                reset_game(&mut world, ball, left_paddle, right_paddle);
            }
        } else {
            paddle_input(&mut world, left_paddle, KeyCode::W, KeyCode::S);
            ai_paddle(&mut world, right_paddle, ball);

            update_positions(&mut world, get_frame_time());
            ball_collision(&mut world, ball);
            ball_paddle_collision(&mut world, ball, &[left_paddle, right_paddle]);
            check_score(&mut world, ball);

            render(&world);
        }

        next_frame().await;
    }
}

fn paddle_input(world: &mut World, paddle: Entity, up_key: KeyCode, down_key: KeyCode) {
    let paddle_data = *world.get_paddle(paddle).unwrap();
    let speed = paddle_data.speed * get_frame_time();

    let Some(pos) = world.get_position_mut(paddle) else {
        return;
    };

    if is_key_down(up_key) && pos.y > paddle_data.height / 2.0 {
        pos.y -= speed;
    }
    if is_key_down(down_key) && pos.y < screen_height() - paddle_data.height / 2.0 {
        pos.y += speed;
    }
}

fn ai_paddle(world: &mut World, paddle: Entity, ball: Entity) {
    let ball_pos = *world.get_position(ball).unwrap();
    let ball_vel = *world.get_velocity(ball).unwrap();
    let paddle_data = *world.get_paddle(paddle).unwrap();
    let speed = paddle_data.speed * get_frame_time() * 0.65;

    let Some(paddle_pos) = world.get_position_mut(paddle) else {
        return;
    };

    let target_y = if ball_vel.x > 0.0 && ball_pos.x > 400.0 {
        ball_pos.y + (rand::gen_range(-30.0, 30.0))
    } else {
        screen_height() / 2.0
    };

    let diff = target_y - paddle_pos.y;
    if diff.abs() > 10.0 {
        if diff > 0.0 && paddle_pos.y < screen_height() - paddle_data.height / 2.0 {
            paddle_pos.y += speed;
        } else if diff < 0.0 && paddle_pos.y > paddle_data.height / 2.0 {
            paddle_pos.y -= speed;
        }
    }
}

fn update_positions(world: &mut World, dt: f32) {
    world
        .query_mut()
        .with(POSITION | VELOCITY)
        .iter(|_entity, table, idx| {
            table.position[idx].x += table.velocity[idx].x * dt;
            table.position[idx].y += table.velocity[idx].y * dt;
        });
}

fn ball_collision(world: &mut World, ball: Entity) {
    let pos = *world.get_position(ball).unwrap();
    let ball_data = *world.get_ball(ball).unwrap();

    if pos.y - ball_data.radius <= 0.0 || pos.y + ball_data.radius >= screen_height() {
        if let Some(vel) = world.get_velocity_mut(ball) {
            vel.y = -vel.y;
        }
    }
}

fn ball_paddle_collision(world: &mut World, ball: Entity, paddles: &[Entity]) {
    let ball_pos = *world.get_position(ball).unwrap();
    let ball_data = *world.get_ball(ball).unwrap();

    for &paddle in paddles {
        let paddle_pos = *world.get_position(paddle).unwrap();
        let paddle_data = *world.get_paddle(paddle).unwrap();

        let paddle_left = paddle_pos.x - paddle_data.width / 2.0;
        let paddle_right = paddle_pos.x + paddle_data.width / 2.0;
        let paddle_top = paddle_pos.y - paddle_data.height / 2.0;
        let paddle_bottom = paddle_pos.y + paddle_data.height / 2.0;

        if ball_pos.x + ball_data.radius >= paddle_left
            && ball_pos.x - ball_data.radius <= paddle_right
            && ball_pos.y + ball_data.radius >= paddle_top
            && ball_pos.y - ball_data.radius <= paddle_bottom
        {
            if let Some(vel) = world.get_velocity_mut(ball) {
                vel.x = -vel.x;
                vel.x *= 1.05;
                vel.y += (ball_pos.y - paddle_pos.y) * 2.0;
            }
        }
    }
}

fn check_score(world: &mut World, ball: Entity) {
    let pos = world.get_position(ball).unwrap();

    if pos.x < 0.0 {
        world.resources.score_right += 1;
        if world.resources.score_right >= 5 {
            world.resources.game_over = true;
            world.resources.winner = Some("AI".to_string());
        } else {
            reset_ball(world, ball);
        }
    } else if pos.x > screen_width() {
        world.resources.score_left += 1;
        if world.resources.score_left >= 5 {
            world.resources.game_over = true;
            world.resources.winner = Some("Player".to_string());
        } else {
            reset_ball(world, ball);
        }
    }
}

fn reset_ball(world: &mut World, ball: Entity) {
    world.set_position(ball, Position { x: 400.0, y: 300.0 });
    if let Some(vel) = world.get_velocity_mut(ball) {
        vel.x = if vel.x > 0.0 { -300.0 } else { 300.0 };
        vel.y = 200.0;
    }
}

fn render(world: &World) {
    world
        .query()
        .with(POSITION | PADDLE)
        .iter(|_entity, table, idx| {
            let pos = &table.position[idx];
            let paddle = &table.paddle[idx];
            draw_rectangle(
                pos.x - paddle.width / 2.0,
                pos.y - paddle.height / 2.0,
                paddle.width,
                paddle.height,
                WHITE,
            );
        });

    world
        .query()
        .with(POSITION | BALL)
        .iter(|_entity, table, idx| {
            let pos = &table.position[idx];
            let ball = &table.ball[idx];
            draw_circle(pos.x, pos.y, ball.radius, WHITE);
        });

    draw_line(
        screen_width() / 2.0,
        0.0,
        screen_width() / 2.0,
        screen_height(),
        2.0,
        WHITE,
    );

    draw_text(
        &world.resources.score_left.to_string(),
        screen_width() / 2.0 - 50.0,
        50.0,
        50.0,
        WHITE,
    );
    draw_text(
        &world.resources.score_right.to_string(),
        screen_width() / 2.0 + 30.0,
        50.0,
        50.0,
        WHITE,
    );
}

fn render_game_over(world: &World) {
    if let Some(winner) = &world.resources.winner {
        let text = format!("{} wins!", winner);
        let text_size = 60.0;
        let text_dimensions = measure_text(&text, None, text_size as u16, 1.0);
        draw_text(
            &text,
            screen_width() / 2.0 - text_dimensions.width / 2.0,
            screen_height() / 2.0 - 50.0,
            text_size,
            WHITE,
        );

        let restart_text = "Press SPACE to restart";
        let restart_size = 30.0;
        let restart_dimensions = measure_text(restart_text, None, restart_size as u16, 1.0);
        draw_text(
            restart_text,
            screen_width() / 2.0 - restart_dimensions.width / 2.0,
            screen_height() / 2.0 + 50.0,
            restart_size,
            WHITE,
        );
    }

    draw_text(
        &world.resources.score_left.to_string(),
        screen_width() / 2.0 - 50.0,
        50.0,
        50.0,
        WHITE,
    );
    draw_text(
        &world.resources.score_right.to_string(),
        screen_width() / 2.0 + 30.0,
        50.0,
        50.0,
        WHITE,
    );
}

fn reset_game(world: &mut World, ball: Entity, left_paddle: Entity, right_paddle: Entity) {
    world.resources.score_left = 0;
    world.resources.score_right = 0;
    world.resources.game_over = false;
    world.resources.winner = None;

    world.set_position(left_paddle, Position { x: 50.0, y: 300.0 });
    world.set_position(right_paddle, Position { x: 750.0, y: 300.0 });
    world.set_position(ball, Position { x: 400.0, y: 300.0 });
    world.set_velocity(ball, Velocity { x: 300.0, y: 200.0 });
}
