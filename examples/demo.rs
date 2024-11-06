use freecs::{has_components, world};
use macroquad::prelude::*;
use rayon::prelude::*;

world! {
    World {
        components {
            position: Position3D => POSITION,
            rotation: Rotation => ROTATION,
            scale: Scale => SCALE,
            velocity: Velocity => VELOCITY,
            gravity: Gravity => GRAVITY,
        },
        Resources {
            delta_time: f32
        }
    }
}

use components::*;
mod components {
    use super::*;

    #[derive(Default, Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
    pub struct Position3D {
        pub x: f32,
        pub y: f32,
        pub z: f32,
    }

    #[derive(Default, Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
    pub struct Rotation {
        pub x: f32,
        pub y: f32,
        pub z: f32,
    }

    #[derive(Default, Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
    pub struct Scale {
        pub x: f32,
        pub y: f32,
        pub z: f32,
    }

    #[derive(Default, Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
    pub struct Velocity {
        pub x: f32,
        pub y: f32,
        pub z: f32,
    }

    #[derive(Default, Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
    pub struct Gravity(pub f32);
}

mod systems {
    use super::*;
    use freecs::has_components;

    pub fn run_systems(world: &mut World, dt: f32) {
        let delta_time = world.resources.delta_time;
        world.tables.par_iter_mut().for_each(|table| {
            if has_components!(table, SCALE) {
                scale_system_parallel(&mut table.scale);
            }

            if has_components!(table, POSITION | VELOCITY) {
                movement_system_parallel(&mut table.position, &table.velocity, delta_time);
                bounce_system_parallel(&mut table.position, &mut table.velocity);
            }

            if has_components!(table, VELOCITY | GRAVITY) {
                gravity_system_parallel(&mut table.velocity, &table.gravity, dt);
            }

            if has_components!(table, ROTATION) {
                rotation_system_parallel(&mut table.rotation, dt);
            }
        });
    }

    #[inline]
    fn scale_system_parallel(scales: &mut [Scale]) {
        let time = macroquad::time::get_time() as f32;
        scales.par_iter_mut().for_each(|scale| {
            scale.x = (4.0 * time).sin() + 2.0;
            scale.y = (4.0 * time).sin() + 2.0;
            scale.z = (4.0 * time).sin() + 2.0;
        });
    }

    #[inline]
    fn movement_system_parallel(positions: &mut [Position3D], velocities: &[Velocity], dt: f32) {
        positions
            .par_iter_mut()
            .zip(velocities.par_iter())
            .for_each(|(pos, vel)| {
                pos.x += vel.x * dt;
                pos.y += vel.y * dt;
                pos.z += vel.z * dt;
            });
    }

    #[inline]
    fn gravity_system_parallel(velocities: &mut [Velocity], gravities: &[Gravity], dt: f32) {
        velocities
            .par_iter_mut()
            .zip(gravities.par_iter())
            .for_each(|(vel, gravity)| {
                vel.y -= gravity.0 * dt;
            });
    }

    #[inline]
    fn rotation_system_parallel(rotations: &mut [Rotation], dt: f32) {
        rotations.par_iter_mut().for_each(|rot| {
            rot.y += dt * 0.5;
            rot.x += dt * 0.3;
            rot.z += dt * 0.2;
        });
    }

    #[inline]
    fn bounce_system_parallel(positions: &mut [Position3D], velocities: &mut [Velocity]) {
        positions
            .par_iter_mut()
            .zip(velocities.par_iter_mut())
            .for_each(|(pos, vel)| {
                // Ground bounce
                if pos.y < 0.0 {
                    pos.y = 0.0;
                    vel.y = vel.y.abs() * 0.8;
                }

                // Wall bounces - now using GRID_SIZE constant
                const BOUNDS: f32 = 40.0; // Matches GRID_SIZE
                if pos.x.abs() > BOUNDS {
                    pos.x = pos.x.signum() * BOUNDS;
                    vel.x = -vel.x * 0.8;
                }
                if pos.z.abs() > BOUNDS {
                    pos.z = pos.z.signum() * BOUNDS;
                    vel.z = -vel.z * 0.8;
                }
            });
    }
}

#[macroquad::main("freecs - Free ECS")]
async fn main() {
    let mut world = World::default();

    // Inject resources for systems to use
    world.resources.delta_time = 0.016;

    // Set up camera with a wider view
    let mut camera = Camera3D {
        position: vec3(80.0, 80.0, 80.0),
        target: vec3(0.0, 0.0, 0.0),
        up: vec3(0.0, 1.0, 0.0),
        ..Default::default()
    };

    // Configure lattice with wider distribution
    const LATTICE_SIZE: i32 = 10;
    const GRID_SIZE: f32 = 40.0; // Total size of the grid
    const START_HEIGHT: f32 = 50.0;
    const TOTAL_ENTITIES: i32 = LATTICE_SIZE * LATTICE_SIZE * LATTICE_SIZE;

    println!(
        "Pre-spawning {}x{}x{} = {} cubes...",
        LATTICE_SIZE, LATTICE_SIZE, LATTICE_SIZE, TOTAL_ENTITIES
    );

    let spawn_start = std::time::Instant::now();

    // Pre-spawn all entities at once
    let entities = spawn_entities(
        &mut world,
        POSITION | ROTATION | SCALE | VELOCITY,
        TOTAL_ENTITIES as usize,
    );

    // The gravity component is added separately to
    // demonstrate adding components after initial creation
    for entity in entities.iter() {
        add_components(&mut world, *entity, GRAVITY);
    }

    // Initialize all entities in the pre-spawned batch
    for (idx, entity) in entities.iter().enumerate() {
        // Convert linear index to 3D coordinates
        let x = (idx as i32 / (LATTICE_SIZE * LATTICE_SIZE)) % LATTICE_SIZE;
        let y = (idx as i32 / LATTICE_SIZE) % LATTICE_SIZE;
        let z = idx as i32 % LATTICE_SIZE;

        // Calculate normalized position (0 to 1)
        let nx = x as f32 / (LATTICE_SIZE - 1) as f32;
        let ny = y as f32 / (LATTICE_SIZE - 1) as f32;
        let nz = z as f32 / (LATTICE_SIZE - 1) as f32;

        // Map to full grid size (-GRID_SIZE to +GRID_SIZE)
        let px = (nx * 2.0 - 1.0) * GRID_SIZE;
        let py = START_HEIGHT + (ny * GRID_SIZE * 0.5);
        let pz = (nz * 2.0 - 1.0) * GRID_SIZE;

        if let Some(position) = get_component_mut::<Position3D>(&mut world, *entity, POSITION) {
            *position = Position3D {
                x: px,
                y: py,
                z: pz,
            };
        }

        // Random initial velocity scaled with position
        if let Some(velocity) = get_component_mut::<Velocity>(&mut world, *entity, VELOCITY) {
            *velocity = Velocity {
                x: rand::gen_range(-2.0, 2.0) * (px / GRID_SIZE),
                y: 0.0,
                z: rand::gen_range(-2.0, 2.0) * (pz / GRID_SIZE),
            };
        }

        if let Some(rotation) = get_component_mut::<Rotation>(&mut world, *entity, ROTATION) {
            *rotation = Rotation {
                x: rand::gen_range(-1.0, 1.0),
                y: rand::gen_range(-1.0, 1.0),
                z: rand::gen_range(-1.0, 1.0),
            };
        }

        // Scale varies with height
        let scale_factor = 0.4 + (ny * 0.2);
        if let Some(scale) = get_component_mut::<Scale>(&mut world, *entity, SCALE) {
            *scale = Scale {
                x: scale_factor,
                y: scale_factor,
                z: scale_factor,
            };
        }

        if let Some(gravity) = get_component_mut::<Gravity>(&mut world, *entity, GRAVITY) {
            *gravity = Gravity(9.81 * (1.0 + ny * 0.2));
        }
    }

    println!(
        "Pre-spawned {} cubes in {:?}",
        TOTAL_ENTITIES,
        spawn_start.elapsed()
    );

    let mut fps_last_time = std::time::Instant::now();
    let mut fps_frame_count = 0;
    let mut current_fps = 0.0;
    let mut frame_times = Vec::with_capacity(100);
    loop {
        let frame_start = std::time::Instant::now();
        let dt = get_frame_time();

        // FPS calculation
        fps_frame_count += 1;
        let fps_elapsed = fps_last_time.elapsed().as_secs_f32();
        if fps_elapsed >= 1.0 {
            current_fps = fps_frame_count as f32 / fps_elapsed;
            fps_frame_count = 0;
            fps_last_time = std::time::Instant::now();
        }

        // Check for escape key to exit
        if is_key_pressed(KeyCode::Escape) {
            break;
        }

        // Update systems
        let systems_start = std::time::Instant::now();
        systems::run_systems(&mut world, dt);
        let systems_time = systems_start.elapsed();

        // Camera rotation
        let rot_speed = 0.5;
        if is_key_down(KeyCode::Left) {
            camera.position = vec3(
                camera.position.x * f32::cos(rot_speed * dt)
                    + camera.position.z * f32::sin(rot_speed * dt),
                camera.position.y,
                -camera.position.x * f32::sin(rot_speed * dt)
                    + camera.position.z * f32::cos(rot_speed * dt),
            );
        }
        if is_key_down(KeyCode::Right) {
            camera.position = vec3(
                camera.position.x * f32::cos(-rot_speed * dt)
                    + camera.position.z * f32::sin(-rot_speed * dt),
                camera.position.y,
                -camera.position.x * f32::sin(-rot_speed * dt)
                    + camera.position.z * f32::cos(-rot_speed * dt),
            );
        }

        // Render
        clear_background(BLACK);
        set_camera(&camera);

        // Draw ground grid
        draw_grid(20, 5.0, GRAY, DARKGRAY); // Increased grid size and spacing

        // Draw entities with culling
        let view_distance = 200.0; // Increased view distance for larger scene
        let distance_squared = view_distance * view_distance;

        for table in &world.tables {
            if has_components!(table, POSITION | ROTATION | SCALE) {
                for i in 0..table.entity_indices.len() {
                    let pos = &table.position[i];
                    let scale = &table.scale[i];

                    // Simple distance culling
                    let dx = pos.x - camera.position.x;
                    let dy = pos.y - camera.position.y;
                    let dz = pos.z - camera.position.z;
                    let dist_sq = dx * dx + dy * dy + dz * dz;

                    if dist_sq > distance_squared {
                        continue;
                    }

                    // Color based on height and position
                    let color = Color::from_rgba(
                        ((pos.y / START_HEIGHT) * 255.0) as u8,
                        100,
                        ((pos.z / GRID_SIZE + 0.5) * 255.0) as u8,
                        255,
                    );

                    draw_cube(
                        Vec3::new(pos.x, pos.y, pos.z),
                        Vec3::new(scale.x, scale.y, scale.z),
                        None,
                        color,
                    );

                    // Only draw wireframes for closer cubes
                    if dist_sq < distance_squared / 4.0 {
                        draw_cube_wires(
                            Vec3::new(pos.x, pos.y, pos.z),
                            Vec3::new(scale.x, scale.y, scale.z),
                            BLACK,
                        );
                    }
                }
            }
        }

        // Performance UI
        set_default_camera();
        draw_text(&format!("FPS: {:.1}", current_fps), 10.0, 30.0, 20.0, GREEN);
        draw_text(
            &format!("Entities: {}", world.next_entity_id),
            10.0,
            70.0,
            20.0,
            WHITE,
        );
        draw_text(
            &format!("Systems update time: {:.2?}", systems_time),
            10.0,
            90.0,
            20.0,
            WHITE,
        );

        frame_times.push(frame_start.elapsed());
        if frame_times.len() >= 100 {
            let avg_frame_time =
                frame_times.iter().sum::<std::time::Duration>() / frame_times.len() as u32;
            println!("Avg frame time over 100 frames: {:?}", avg_frame_time);
            frame_times.clear();
        }

        next_frame().await;
    }
}
