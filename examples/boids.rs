use freecs::{ecs, has_components};
use macroquad::prelude::*;

ecs! {
    World {
        position: Position => POSITION,
        velocity: Velocity => VELOCITY,
        boid: Boid => BOID,
        color: BoidColor => COLOR,
    }
    Resources {
        delta_time: f32,
        alignment_weight: f32,
        cohesion_weight: f32,
        separation_weight: f32,
        visual_range: f32,
        min_speed: f32,
        max_speed: f32,
    }
}

#[derive(Default, Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
struct Position {
    x: f32,
    y: f32,
}

#[derive(Default, Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
struct Velocity {
    x: f32,
    y: f32,
}

#[derive(Default, Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
struct Boid;

#[derive(Default, Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
struct BoidColor {
    r: f32,
    g: f32,
    b: f32,
}

struct BoidParams {
    alignment_weight: f32,
    cohesion_weight: f32,
    separation_weight: f32,
    visual_range: f32,
    min_speed: f32,
    max_speed: f32,
    show_debug: bool,
    paused: bool,
    spawn_count: usize,
}

impl Default for BoidParams {
    fn default() -> Self {
        Self {
            alignment_weight: 0.5,
            cohesion_weight: 0.3,
            separation_weight: 0.4,
            visual_range: 50.0,
            min_speed: 100.0,
            max_speed: 300.0,
            show_debug: false,
            paused: false,
            spawn_count: 1000,
        }
    }
}

mod systems {
    use super::*;
    use rayon::prelude::*;

    pub fn run_systems(world: &mut World) {
        world.tables.par_iter_mut().for_each(|table| {
            if has_components!(table, POSITION | VELOCITY | BOID) {
                process_boids(table, &world.resources);
                update_positions(table, world.resources.delta_time);
                wrap_positions(table);
            }
        });
    }

    fn process_boids(table: &mut ComponentArrays, resources: &Resources) {
        // Create spatial grid
        let mut grid = SpatialGrid::new(screen_width(), screen_height(), resources.visual_range);

        // Fill grid with current positions and velocities
        for i in 0..table.entity_indices.len() {
            grid.insert(
                table.entity_indices[i],
                table.position[i],
                table.velocity[i],
            );
        }

        // Process boids using spatial grid
        table
            .velocity
            .par_iter_mut()
            .enumerate()
            .for_each(|(i, vel)| {
                let pos = table.position[i];
                let mut alignment = Velocity::default();
                let mut cohesion = Position::default();
                let mut separation = Velocity::default();
                let mut neighbors = 0;

                // Only check nearby boids using spatial grid
                for (other_entity, other_pos, other_vel) in
                    grid.get_nearby_boids(pos, resources.visual_range)
                {
                    if table.entity_indices[i] == *other_entity {
                        continue;
                    }

                    let dx = other_pos.x - pos.x;
                    let dy = other_pos.y - pos.y;
                    let dist_sq = dx * dx + dy * dy;

                    if dist_sq < resources.visual_range * resources.visual_range {
                        alignment.x += other_vel.x;
                        alignment.y += other_vel.y;

                        cohesion.x += other_pos.x;
                        cohesion.y += other_pos.y;

                        if dist_sq > 0.0 {
                            let factor = 1.0 / dist_sq.sqrt();
                            separation.x -= dx * factor;
                            separation.y -= dy * factor;
                        }

                        neighbors += 1;
                    }
                }

                // Rest of the boid logic remains the same...
                if neighbors > 0 {
                    let inv_neighbors = 1.0 / neighbors as f32;

                    alignment.x *= inv_neighbors * resources.alignment_weight;
                    alignment.y *= inv_neighbors * resources.alignment_weight;

                    cohesion.x = (cohesion.x * inv_neighbors - pos.x) * resources.cohesion_weight;
                    cohesion.y = (cohesion.y * inv_neighbors - pos.y) * resources.cohesion_weight;

                    vel.x += alignment.x + cohesion.x + separation.x * resources.separation_weight;
                    vel.y += alignment.y + cohesion.y + separation.y * resources.separation_weight;

                    let speed = (vel.x * vel.x + vel.y * vel.y).sqrt();
                    if speed > resources.max_speed {
                        let factor = resources.max_speed / speed;
                        vel.x *= factor;
                        vel.y *= factor;
                    } else if speed < resources.min_speed {
                        let factor = resources.min_speed / speed;
                        vel.x *= factor;
                        vel.y *= factor;
                    }
                }
            });
    }

    fn update_positions(table: &mut ComponentArrays, dt: f32) {
        table
            .position
            .par_iter_mut()
            .zip(table.velocity.par_iter())
            .for_each(|(pos, vel)| {
                pos.x += vel.x * dt;
                pos.y += vel.y * dt;
            });
    }

    fn wrap_positions(table: &mut ComponentArrays) {
        let screen_w = screen_width();
        let screen_h = screen_height();

        table.position.par_iter_mut().for_each(|pos| {
            if pos.x < 0.0 {
                pos.x += screen_w;
            }
            if pos.x > screen_w {
                pos.x -= screen_w;
            }
            if pos.y < 0.0 {
                pos.y += screen_h;
            }
            if pos.y > screen_h {
                pos.y -= screen_h;
            }
        });
    }
}

fn spawn_boids(world: &mut World, count: usize) {
    let entities = spawn_entities(world, POSITION | VELOCITY | BOID | COLOR, count);

    for entity in entities {
        if let Some(pos) = get_component_mut::<Position>(world, entity, POSITION) {
            pos.x = rand::gen_range(0.0, screen_width());
            pos.y = rand::gen_range(0.0, screen_height());
        }

        if let Some(vel) = get_component_mut::<Velocity>(world, entity, VELOCITY) {
            let angle = rand::gen_range(0.0, std::f32::consts::PI * 2.0);
            let speed = rand::gen_range(100.0, 200.0);
            vel.x = angle.cos() * speed;
            vel.y = angle.sin() * speed;
        }

        if let Some(color) = get_component_mut::<BoidColor>(world, entity, COLOR) {
            color.r = rand::gen_range(0.5, 1.0);
            color.g = rand::gen_range(0.5, 1.0);
            color.b = rand::gen_range(0.5, 1.0);
        }
    }
}

fn render_boids(world: &World) {
    for table in &world.tables {
        if has_components!(table, POSITION | VELOCITY | COLOR) {
            for i in 0..table.entity_indices.len() {
                let pos = &table.position[i];
                let vel = &table.velocity[i];
                let color = &table.color[i];

                let angle = vel.y.atan2(vel.x);

                draw_triangle(
                    Vec2::new(pos.x, pos.y),
                    Vec2::new(
                        pos.x - 8.0 * (angle + 2.0).cos(),
                        pos.y - 8.0 * (angle + 2.0).sin(),
                    ),
                    Vec2::new(
                        pos.x - 8.0 * (angle - 2.0).cos(),
                        pos.y - 8.0 * (angle - 2.0).sin(),
                    ),
                    Color::new(color.r, color.g, color.b, 1.0),
                );
            }
        }
    }
}

fn draw_ui(params: &mut BoidParams, world: &mut World) {
    let panel_width = 250.0;
    let screen_w = screen_width();

    draw_rectangle(
        screen_w - panel_width,
        0.0,
        panel_width,
        280.0,
        Color::new(0.0, 0.0, 0.0, 0.7),
    );

    let x = screen_w - panel_width + 10.0;
    let mut y = 20.0;
    let step = 25.0;

    let draw_param = |y: f32, text: &str| {
        draw_text(text, x, y, 20.0, WHITE);
    };

    draw_param(
        y,
        &format!("Entities: {}", query_entities(world, ALL).len()),
    );
    y += step;
    draw_param(y, &format!("FPS: {:.1}", get_fps()));
    y += step;

    draw_param(y, "[Space] Pause");
    y += step;
    draw_param(y, "[+/-] Add/Remove 1000 boids");
    y += step;
    draw_param(y, "[D] Toggle debug view");
    y += step;

    y += step;
    draw_param(y, "Parameters (use arrows):");
    y += step;
    draw_param(y, &format!("Alignment: {:.2}", params.alignment_weight));
    y += step;
    draw_param(y, &format!("Cohesion: {:.2}", params.cohesion_weight));
    y += step;
    draw_param(y, &format!("Separation: {:.2}", params.separation_weight));
    y += step;
    draw_param(y, &format!("Visual Range: {:.0}", params.visual_range));
    y += step;
    draw_param(
        y,
        &format!("Speed: {:.0}-{:.0}", params.min_speed, params.max_speed),
    );

    if is_key_pressed(KeyCode::Space) {
        params.paused = !params.paused;
    }
    if is_key_pressed(KeyCode::D) {
        params.show_debug = !params.show_debug;
    }

    let speed = if is_key_down(KeyCode::LeftShift) {
        0.01
    } else {
        0.001
    };

    if is_key_down(KeyCode::Left) {
        params.alignment_weight = (params.alignment_weight - speed).max(0.0);
    }
    if is_key_down(KeyCode::Right) {
        params.alignment_weight = (params.alignment_weight + speed).min(1.0);
    }
    if is_key_down(KeyCode::Down) {
        params.cohesion_weight = (params.cohesion_weight - speed).max(0.0);
    }
    if is_key_down(KeyCode::Up) {
        params.cohesion_weight = (params.cohesion_weight + speed).min(1.0);
    }

    if is_key_pressed(KeyCode::Equal) {
        params.spawn_count += 1000;
        spawn_boids(world, 1000);
    }
    if is_key_pressed(KeyCode::Minus) {
        params.spawn_count = params.spawn_count.saturating_sub(1000);
        // Note: Currently no despawn functionality
    }
}

fn draw_debug(world: &World) {
    for table in &world.tables {
        if has_components!(table, POSITION | VELOCITY) {
            for i in 0..table.entity_indices.len() {
                let pos = &table.position[i];
                draw_circle_lines(
                    pos.x,
                    pos.y,
                    world.resources.visual_range,
                    0.5,
                    Color::new(0.2, 0.2, 0.2, 0.3),
                );
                let vel = &table.velocity[i];
                draw_line(
                    pos.x,
                    pos.y,
                    pos.x + vel.x * 0.2,
                    pos.y + vel.y * 0.2,
                    1.0,
                    Color::new(0.0, 1.0, 0.0, 0.3),
                );
            }
        }
    }
}

struct SpatialGrid {
    cells: Vec<Vec<(EntityId, Position, Velocity)>>,
    cell_size: f32,
    width: usize,
    height: usize,
}

impl SpatialGrid {
    fn new(screen_w: f32, screen_h: f32, cell_size: f32) -> Self {
        let width = (screen_w / cell_size).ceil() as usize;
        let height = (screen_h / cell_size).ceil() as usize;
        let cells = vec![Vec::new(); width * height];
        Self {
            cells,
            cell_size,
            width,
            height,
        }
    }

    fn insert(&mut self, entity: EntityId, pos: Position, vel: Velocity) {
        let idx = self.get_cell_index(pos.x, pos.y);
        if let Some(cell) = self.cells.get_mut(idx) {
            cell.push((entity, pos, vel));
        }
    }

    fn get_cell_index(&self, x: f32, y: f32) -> usize {
        let cell_x = (x / self.cell_size).floor() as usize;
        let cell_y = (y / self.cell_size).floor() as usize;
        (cell_x.min(self.width - 1)) + (cell_y.min(self.height - 1)) * self.width
    }

    fn get_nearby_boids(
        &self,
        pos: Position,
        range: f32,
    ) -> impl Iterator<Item = &(EntityId, Position, Velocity)> {
        let range_cells = (range / self.cell_size).ceil() as isize;
        let cell_x = (pos.x / self.cell_size).floor() as isize;
        let cell_y = (pos.y / self.cell_size).floor() as isize;

        let mut nearby = Vec::new();

        for dy in -range_cells as isize..=range_cells as isize {
            for dx in -range_cells as isize..=range_cells as isize {
                let x = cell_x + dx;
                let y = cell_y + dy;

                if x >= 0 && x < self.width as isize && y >= 0 && y < self.height as isize {
                    let idx = x as usize + y as usize * self.width;
                    if let Some(cell) = self.cells.get(idx) {
                        nearby.extend(cell.iter());
                    }
                }
            }
        }

        nearby.into_iter()
    }
}

#[macroquad::main("Boids")]
async fn main() {
    let mut world = World::default();
    let mut params = BoidParams::default();

    spawn_boids(&mut world, params.spawn_count);

    loop {
        clear_background(BLACK);

        world.resources.delta_time = if params.paused { 0.0 } else { get_frame_time() };
        world.resources.alignment_weight = params.alignment_weight;
        world.resources.cohesion_weight = params.cohesion_weight;
        world.resources.separation_weight = params.separation_weight;
        world.resources.visual_range = params.visual_range;
        world.resources.min_speed = params.min_speed;
        world.resources.max_speed = params.max_speed;

        systems::run_systems(&mut world);

        if params.show_debug {
            draw_debug(&world);
        }

        render_boids(&world);
        draw_ui(&mut params, &mut world);

        next_frame().await;
    }
}
