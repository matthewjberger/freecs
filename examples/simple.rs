use freecs::{Entity, ecs};

ecs! {
    World {
        position: Position => POSITION,
        velocity: Velocity => VELOCITY,
        health: Health => HEALTH,
    }
    Resources {
        delta_time: f32
    }
}

pub fn main() {
    let mut world = World::default();

    // Spawn entities with components
    let _entity = world.spawn_entities(POSITION | VELOCITY, 1)[0];

    // Or use the entity builder
    let entity = EntityBuilder::new()
        .with_position(Position { x: 1.0, y: 2.0 })
        .spawn(&mut world, 1)[0];

    // Read components using the generated methods
    let position = world.get_position(entity);
    println!("Position: {:?}", position);

    // Set components (adds if not present)
    world.set_position(entity, Position { x: 1.0, y: 2.0 });

    // Mutate a component
    if let Some(position) = world.get_position_mut(entity) {
        position.x += 1.0;
    }

    // Get an entity's component mask
    let _component_mask = world.component_mask(entity).unwrap();

    // Add a new component to an entity
    world.add_components(entity, HEALTH);

    // Or use the generated add method
    world.add_health(entity);

    // Query all entities
    let _entities = world.get_all_entities();

    // Query all entities with a specific component
    let _players = world.query_entities(POSITION | VELOCITY | HEALTH);

    // Query the first entity with a specific component,
    // returning early instead of checking remaining entities
    let _first_player_entity = world.query_first_entity(POSITION | VELOCITY | HEALTH);

    // Remove a component from an entity
    world.remove_components(entity, HEALTH);

    // Or use the generated remove method
    world.remove_health(entity);

    // Check if entity has components
    if world.entity_has_position(entity) {
        println!("Entity has position component");
    }

    // Systems are functions that transform component data
    systems::run_systems(&mut world);

    // Despawn entities, freeing their table slots for reuse
    world.despawn_entities(&[entity]);
}

use components::*;
mod components {
    #[derive(Default, Debug, Clone, Copy)]
    pub struct Position {
        pub x: f32,
        pub y: f32,
    }

    #[derive(Default, Debug, Clone, Copy)]
    pub struct Velocity {
        pub x: f32,
        pub y: f32,
    }

    #[derive(Default, Debug, Clone, Copy)]
    pub struct Health {
        pub value: f32,
    }
}

mod systems {
    use super::*;

    pub fn run_systems(world: &mut World) {
        // Systems use queries and component accessors
        example_system(world);
        update_positions_system(world);
        health_system(world);
    }

    fn example_system(world: &mut World) {
        for entity in world.query_entities(POSITION | VELOCITY) {
            if let Some(position) = world.get_position_mut(entity) {
                position.x += 1.0;
            }
        }
    }

    fn update_positions_system(world: &mut World) {
        let dt = world.resources.delta_time;

        // Collect entities with their velocities first to avoid borrow conflicts
        let updates: Vec<(Entity, Velocity)> = world
            .query_entities(POSITION | VELOCITY)
            .into_iter()
            .filter_map(|entity| world.get_velocity(entity).map(|vel| (entity, *vel)))
            .collect();

        // Now update positions
        for (entity, vel) in updates {
            if let Some(pos) = world.get_position_mut(entity) {
                pos.x += vel.x * dt;
                pos.y += vel.y * dt;
            }
        }
    }

    fn health_system(world: &mut World) {
        for entity in world.query_entities(HEALTH) {
            if let Some(health) = world.get_health_mut(entity) {
                health.value *= 0.98;
            }
        }
    }
}
