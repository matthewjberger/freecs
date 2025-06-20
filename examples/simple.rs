use freecs::{ecs, table_has_components};
use rayon::prelude::*;

ecs! {
    World {
        position: Position => POSITION,
        velocity: Velocity => VELOCITY,
        health: Health => HEALTH,
        node: Node => NODE,
    }
    Resources {
        delta_time: f32
    }
}

pub fn main() {
    let mut world = World::default();

    // Spawn entities with components
    let entity = spawn_entities(&mut world, POSITION | VELOCITY, 1)[0];
    println!(
        "Spawned {} with position and velocity",
        get_all_entities(&world).len()
    );

    // Read a component
    let position = get_component::<Position>(&world, entity, POSITION);
    println!("Position: {:?}", position);

    // Mutate a component
    if let Some(position) = get_component_mut::<Position>(&mut world, entity, POSITION) {
        position.x += 1.0;
    }

    // Get an entity's component mask
    println!(
        "Component mask before adding health component: {:b}",
        component_mask(&world, entity).unwrap()
    );

    // Add a new component to an entity
    add_components(&mut world, entity, HEALTH);

    println!(
        "Component mask after adding health component: {:b}",
        component_mask(&world, entity).unwrap()
    );

    // Query all entities
    let entities = get_all_entities(&world);
    println!("All entities: {entities:?}");

    // Query all entities with a specific component
    let players = query_entities(&world, POSITION | VELOCITY | HEALTH);
    println!("Player entities: {players:?}");

    // Query the first entity with a specific component,
    // returning early instead of checking remaining entities
    let first_player_entity = query_first_entity(&world, POSITION | VELOCITY | HEALTH);
    println!("First player entity : {first_player_entity:?}");

    // Remove a component from an entity
    remove_components(&mut world, entity, HEALTH);

    // This runs the systems once in parallel
    // Not part of the library's public API, but a demonstration of how to run systems
    systems::run_systems(&mut world);

    // Despawn entities, freeing their table slots for reuse
    despawn_entities(&mut world, &[entity]);

    // Create a new world to populate
    let mut new_world = World::default();

    // Spawn all entities at once
    let [root, child1, child2] = spawn_entities(&mut new_world, POSITION | NODE, 3)[..] else {
        panic!("Failed to spawn entities");
    };

    // Set up hierarchy
    if let Some(root_node) = get_component_mut::<Node>(&mut new_world, root, NODE) {
        root_node.id = root;
        root_node.parent = None;
        root_node.children = vec![child1, child2];
    }

    if let Some(child1_node) = get_component_mut::<Node>(&mut new_world, child1, NODE) {
        child1_node.id = child1;
        child1_node.parent = Some(root);
        child1_node.children = vec![child2];
    }

    if let Some(child2_node) = get_component_mut::<Node>(&mut new_world, child2, NODE) {
        child2_node.id = child2;
        child2_node.parent = Some(child1);
        child2_node.children = vec![];
    }
}

use components::*;
mod components {
    use super::*;

    #[derive(Default, Debug, Clone, PartialEq)]
    pub struct Node {
        pub id: EntityId,
        pub parent: Option<EntityId>,
        pub children: Vec<EntityId>,
    }

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
    pub struct Health {
        pub value: f32,
    }
}

mod systems {
    use super::*;

    // Systems are functions that iterate over
    // the component tables and transform component data.
    // This function invokes two systems in parallel
    // for each table in the world filtered by component mask.
    pub fn run_systems(world: &mut World) {
        let delta_time = world.resources.delta_time;
        world.tables.par_iter_mut().for_each(|table| {
            if table_has_components!(table, POSITION | VELOCITY | HEALTH) {
                update_positions_system(&mut table.position, &table.velocity, delta_time);
            }
            if table_has_components!(table, HEALTH) {
                health_system(&mut table.health);
            }
        });
    }

    // The system itself can also access components in parallel
    #[inline]
    pub fn update_positions_system(positions: &mut [Position], velocities: &[Velocity], dt: f32) {
        positions
            .par_iter_mut()
            .zip(velocities.par_iter())
            .for_each(|(pos, vel)| {
                pos.x += vel.x * dt;
                pos.y += vel.y * dt;
            });
    }

    #[inline]
    pub fn health_system(health: &mut [Health]) {
        health.par_iter_mut().for_each(|health| {
            health.value *= 0.98; // gradually decline health value
        });
    }
}
