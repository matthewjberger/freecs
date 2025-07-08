use freecs::{EntityId, ecs, table_has_components};
use macroquad::prelude::*;

ecs! {
    World {
        position: Position => POSITION,
        rotation: Rotation => ROTATION,
        velocity: Velocity => VELOCITY,
        player: Player => PLAYER,
        wall: Wall => WALL,
        wall_type: WallType => WALL_TYPE,

    }
    Resources {
        delta_time: f32,
        screen_width: f32,
        screen_height: f32,
        fov: f32,
        ray_count: usize,
        max_distance: f32,
        wall_height: f32,
        move_speed: f32,
        rotate_speed: f32,
        mouse_sensitivity: f32,
        show_minimap: bool,
        show_raycast_debug: bool,
    }
}

#[derive(Default, Debug, Clone, Copy)]
struct Position {
    x: f32,
    y: f32,
}

#[derive(Default, Debug, Clone, Copy)]
struct Rotation {
    angle: f32,
}

#[derive(Default, Debug, Clone, Copy)]
struct Velocity {
    x: f32,
    y: f32,
}

#[derive(Default, Debug, Clone, Copy)]
struct Player;

#[derive(Default, Debug, Clone, Copy)]
struct Wall;

#[derive(Default, Debug, Clone, Copy)]
struct WallType {
    texture_id: u8,
    height: f32,
}

struct GameParams {
    fov: f32,
    ray_count: usize,
    max_distance: f32,
    wall_height: f32,
    move_speed: f32,
    rotate_speed: f32,
    mouse_sensitivity: f32,
    show_minimap: bool,
    show_raycast_debug: bool,
}

impl Default for GameParams {
    fn default() -> Self {
        Self {
            fov: 60.0,
            ray_count: 320,
            max_distance: 20.0,
            wall_height: 1.0,
            move_speed: 5.0,
            rotate_speed: 2.0,
            mouse_sensitivity: 0.002,
            show_minimap: true,
            show_raycast_debug: false,
        }
    }
}

const MAP_SIZE: usize = 16;
const CELL_SIZE: f32 = 1.0;

static MAP: [[u8; MAP_SIZE]; MAP_SIZE] = [
    [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1],
    [1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1],
    [1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
];

mod systems {
    use super::*;

    pub fn run_systems(world: &mut World) {
        let resources = &world.resources;
        let delta_time = resources.delta_time;

        for table in &mut world.tables {
            if table_has_components!(table, POSITION | ROTATION | VELOCITY | PLAYER) {
                handle_player_input(table, resources, delta_time);
                update_player_movement(table, resources, delta_time);
            }
        }
    }

    fn handle_player_input(table: &mut ComponentArrays, resources: &Resources, dt: f32) {
        for i in 0..table.entity_indices.len() {
            let mut new_velocity = Velocity::default();
            let angle = table.rotation[i].angle;

            if is_key_down(KeyCode::W) {
                new_velocity.x = angle.cos() * resources.move_speed;
                new_velocity.y = angle.sin() * resources.move_speed;
            }
            if is_key_down(KeyCode::S) {
                new_velocity.x = -angle.cos() * resources.move_speed;
                new_velocity.y = -angle.sin() * resources.move_speed;
            }
            if is_key_down(KeyCode::A) {
                new_velocity.x = (angle - std::f32::consts::FRAC_PI_2).cos() * resources.move_speed;
                new_velocity.y = (angle - std::f32::consts::FRAC_PI_2).sin() * resources.move_speed;
            }
            if is_key_down(KeyCode::D) {
                new_velocity.x = (angle + std::f32::consts::FRAC_PI_2).cos() * resources.move_speed;
                new_velocity.y = (angle + std::f32::consts::FRAC_PI_2).sin() * resources.move_speed;
            }

            if is_key_down(KeyCode::Left) {
                table.rotation[i].angle -= resources.rotate_speed * dt;
            }
            if is_key_down(KeyCode::Right) {
                table.rotation[i].angle += resources.rotate_speed * dt;
            }

            let mouse_delta = mouse_wheel().1;
            if mouse_delta != 0.0 {
                table.rotation[i].angle += mouse_delta * resources.mouse_sensitivity;
            }

            table.velocity[i] = new_velocity;
        }
    }

    fn update_player_movement(table: &mut ComponentArrays, _resources: &Resources, dt: f32) {
        for i in 0..table.entity_indices.len() {
            let new_x = table.position[i].x + table.velocity[i].x * dt;
            let new_y = table.position[i].y + table.velocity[i].y * dt;

            if !is_wall_at(new_x, table.position[i].y) {
                table.position[i].x = new_x;
            }
            if !is_wall_at(table.position[i].x, new_y) {
                table.position[i].y = new_y;
            }
        }
    }
}

fn is_wall_at(x: f32, y: f32) -> bool {
    let map_x = (x / CELL_SIZE) as usize;
    let map_y = (y / CELL_SIZE) as usize;

    if map_x >= MAP_SIZE || map_y >= MAP_SIZE {
        return true;
    }

    MAP[map_y][map_x] != 0
}

fn raycast(start_x: f32, start_y: f32, angle: f32, max_distance: f32) -> (f32, u8, bool) {
    let ray_dir_x = angle.cos();
    let ray_dir_y = angle.sin();

    let map_x = start_x as i32;
    let map_y = start_y as i32;

    let delta_distance_x = (1.0 / ray_dir_x).abs();
    let delta_distance_y = (1.0 / ray_dir_y).abs();

    let step_x = if ray_dir_x < 0.0 { -1 } else { 1 };
    let step_y = if ray_dir_y < 0.0 { -1 } else { 1 };

    let side_distance_x = if ray_dir_x < 0.0 {
        (start_x - map_x as f32) * delta_distance_x
    } else {
        (map_x as f32 + 1.0 - start_x) * delta_distance_x
    };

    let side_distance_y = if ray_dir_y < 0.0 {
        (start_y - map_y as f32) * delta_distance_y
    } else {
        (map_y as f32 + 1.0 - start_y) * delta_distance_y
    };

    let mut current_x = map_x;
    let mut current_y = map_y;
    let mut side_distance_x = side_distance_x;
    let mut side_distance_y = side_distance_y;
    let mut hit = false;
    let mut side = false;

    while !hit {
        if side_distance_x < side_distance_y {
            side_distance_x += delta_distance_x;
            current_x += step_x;
            side = false;
        } else {
            side_distance_y += delta_distance_y;
            current_y += step_y;
            side = true;
        }

        if current_x < 0
            || current_x >= MAP_SIZE as i32
            || current_y < 0
            || current_y >= MAP_SIZE as i32
        {
            break;
        }

        if MAP[current_y as usize][current_x as usize] != 0 {
            hit = true;
        }
    }

    if !hit {
        return (max_distance, 0, side);
    }

    let perp_wall_dist = if !side {
        (current_x as f32 - start_x + (1.0 - step_x as f32) / 2.0) / ray_dir_x
    } else {
        (current_y as f32 - start_y + (1.0 - step_y as f32) / 2.0) / ray_dir_y
    };

    let wall_type = MAP[current_y as usize][current_x as usize];
    (perp_wall_dist.min(max_distance), wall_type, side)
}

fn spawn_level(world: &mut World) {
    for y in 0..MAP_SIZE {
        for x in 0..MAP_SIZE {
            if MAP[y][x] != 0 {
                let entity = world.spawn_entities(POSITION | WALL | WALL_TYPE, 1)[0];
                if let Some(pos) = world.get_component_mut::<Position>(entity, POSITION) {
                    pos.x = x as f32 * CELL_SIZE + CELL_SIZE / 2.0;
                    pos.y = y as f32 * CELL_SIZE + CELL_SIZE / 2.0;
                }
                if let Some(wall_type) = world.get_component_mut::<WallType>(entity, WALL_TYPE) {
                    wall_type.texture_id = MAP[y][x];
                    wall_type.height = 1.0;
                }
            }
        }
    }
}

fn spawn_player(world: &mut World) -> EntityId {
    let entity = world.spawn_entities(POSITION | ROTATION | VELOCITY | PLAYER, 1)[0];
    if let Some(pos) = world.get_component_mut::<Position>(entity, POSITION) {
        pos.x = 2.0;
        pos.y = 2.0;
    }
    if let Some(rot) = world.get_component_mut::<Rotation>(entity, ROTATION) {
        rot.angle = 0.0;
    }
    if let Some(vel) = world.get_component_mut::<Velocity>(entity, VELOCITY) {
        vel.x = 0.0;
        vel.y = 0.0;
    }
    entity
}

fn render_3d_view(world: &World) {
    let player_entity = world.query_first_entity(POSITION | ROTATION | PLAYER);
    if player_entity.is_none() {
        return;
    }

    let player_pos = world.get_position(player_entity.unwrap()).unwrap();
    let player_rot = world.get_rotation(player_entity.unwrap()).unwrap();

    let ray_angle_step = world.resources.fov.to_radians() / world.resources.ray_count as f32;

    for i in 0..world.resources.ray_count {
        let ray_angle =
            player_rot.angle - world.resources.fov.to_radians() / 2.0 + i as f32 * ray_angle_step;
        let (distance, wall_type, side) = raycast(
            player_pos.x,
            player_pos.y,
            ray_angle,
            world.resources.max_distance,
        );

        let wall_height = if distance > 0.0 {
            (world.resources.screen_height / distance) * world.resources.wall_height
        } else {
            world.resources.screen_height
        };

        let wall_top = (world.resources.screen_height - wall_height) / 2.0;

        let x = i as f32 * (world.resources.screen_width / world.resources.ray_count as f32);

        let color = if side {
            match wall_type {
                1 => DARKGRAY,
                2 => GRAY,
                3 => LIGHTGRAY,
                _ => WHITE,
            }
        } else {
            match wall_type {
                1 => RED,
                2 => GREEN,
                3 => BLUE,
                _ => YELLOW,
            }
        };

        draw_rectangle(
            x,
            wall_top,
            world.resources.screen_width / world.resources.ray_count as f32,
            wall_height,
            color,
        );

        if world.resources.show_raycast_debug {
            draw_line(
                player_pos.x * 10.0,
                player_pos.y * 10.0,
                (player_pos.x + ray_angle.cos() * distance) * 10.0,
                (player_pos.y + ray_angle.sin() * distance) * 10.0,
                1.0,
                RED,
            );
        }
    }
}

fn render_minimap(world: &World) {
    if !world.resources.show_minimap {
        return;
    }

    let minimap_size = 200.0;
    let minimap_scale = minimap_size / (MAP_SIZE as f32 * CELL_SIZE);
    let minimap_x = world.resources.screen_width - minimap_size - 10.0;
    let minimap_y = 10.0;

    draw_rectangle(minimap_x, minimap_y, minimap_size, minimap_size, BLACK);
    draw_rectangle_lines(minimap_x, minimap_y, minimap_size, minimap_size, 2.0, WHITE);

    for y in 0..MAP_SIZE {
        for x in 0..MAP_SIZE {
            if MAP[y][x] != 0 {
                let rect_x = minimap_x + x as f32 * CELL_SIZE * minimap_scale;
                let rect_y = minimap_y + y as f32 * CELL_SIZE * minimap_scale;
                let rect_size = CELL_SIZE * minimap_scale;

                draw_rectangle(rect_x, rect_y, rect_size, rect_size, GRAY);
            }
        }
    }

    let player_entity = world.query_first_entity(POSITION | ROTATION | PLAYER);
    if let Some(player_entity) = player_entity {
        let player_position = world.get_position(player_entity).unwrap();
        let player_rotation = world.get_rotation(player_entity).unwrap();

        let player_x = minimap_x + player_position.x * minimap_scale;
        let player_y = minimap_y + player_position.y * minimap_scale;

        draw_circle(player_x, player_y, 3.0, RED);

        let direction_x = player_x + player_rotation.angle.cos() * 10.0;
        let direction_y = player_y + player_rotation.angle.sin() * 10.0;
        draw_line(player_x, player_y, direction_x, direction_y, 2.0, YELLOW);
    }
}

fn draw_ui(_params: &mut GameParams, world: &World) {
    let player_entity = world.query_first_entity(POSITION | ROTATION | PLAYER);
    if let Some(player_entity) = player_entity {
        let player_pos = world.get_position(player_entity).unwrap();
        let player_rot = world.get_rotation(player_entity).unwrap();

        draw_text(
            &format!("Position: ({:.2}, {:.2})", player_pos.x, player_pos.y),
            10.0,
            30.0,
            20.0,
            WHITE,
        );
        draw_text(
            &format!("Rotation: {:.2}°", player_rot.angle.to_degrees()),
            10.0,
            60.0,
            20.0,
            WHITE,
        );
    }

    draw_text(
        "Controls: WASD to move, Arrow keys/Mouse to look",
        10.0,
        90.0,
        16.0,
        WHITE,
    );
    draw_text(
        "M: Toggle minimap, R: Toggle raycast debug",
        10.0,
        110.0,
        16.0,
        WHITE,
    );
}

#[macroquad::main("Wolfenstein")]
async fn main() {
    let mut params = GameParams::default();
    let mut world = World::default();

    world.resources.screen_width = screen_width();
    world.resources.screen_height = screen_height();
    world.resources.fov = params.fov;
    world.resources.ray_count = params.ray_count;
    world.resources.max_distance = params.max_distance;
    world.resources.wall_height = params.wall_height;
    world.resources.move_speed = params.move_speed;
    world.resources.rotate_speed = params.rotate_speed;
    world.resources.mouse_sensitivity = params.mouse_sensitivity;
    world.resources.show_minimap = params.show_minimap;
    world.resources.show_raycast_debug = params.show_raycast_debug;

    spawn_level(&mut world);
    let _player_entity = spawn_player(&mut world);

    println!("Wolfenstein 3D Style Example");
    println!("Controls: WASD to move, Arrow keys/Mouse to look");
    println!("M: Toggle minimap, R: Toggle raycast debug");

    loop {
        world.resources.delta_time = get_frame_time();
        world.resources.screen_width = screen_width();
        world.resources.screen_height = screen_height();

        if is_key_pressed(KeyCode::M) {
            params.show_minimap = !params.show_minimap;
            world.resources.show_minimap = params.show_minimap;
        }

        if is_key_pressed(KeyCode::R) {
            params.show_raycast_debug = !params.show_raycast_debug;
            world.resources.show_raycast_debug = params.show_raycast_debug;
        }

        systems::run_systems(&mut world);

        clear_background(BLACK);

        render_3d_view(&world);
        render_minimap(&world);
        draw_ui(&mut params, &world);

        next_frame().await;
    }
}
