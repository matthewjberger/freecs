use freecs::Schedule;
use freecs::dynamic::{
    ChildOf, ComponentRegistry, DynEcs, DynWorld, ResourceHost, ResourceHostExt, ResourceMap,
};

// Components are plain structs. Default is the only requirement; there is
// no derive macro and no registration ceremony, first use registers them.
#[derive(Default, Clone, Debug)]
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
struct Burning {
    lift: f32,
}

// Resources and events are also plain types; Default is not required.
struct DeltaTime(f32);
struct Score(u32);

#[derive(Clone, Debug)]
struct Damage {
    amount: f32,
}

// Marker tags are zero-sized types. Tags live in sparse sets outside the
// archetype tables, so toggling one never migrates the entity.
struct Player;
struct Frozen;
struct Selected;

// An engine that wraps the world in its own state struct implements
// ResourceHost, and the free-function scope forms in freecs::dynamic then
// lend the whole wrapper to the closure, which the inherent scope methods
// cannot do.
struct Engine {
    world: DynWorld,
    frames: u32,
}

impl ResourceHost for Engine {
    fn resource_map_mut(&mut self) -> &mut ResourceMap {
        &mut self.world.resources
    }
}

// A system is a plain function over the world. resource_scope takes the
// resource out for the closure, so the resource and the world are
// independent borrows and there is no borrow juggling.
fn movement(world: &mut DynWorld) {
    world.resource_scope(|world, delta_time: &mut DeltaTime| {
        world
            .query::<(&mut Position, &Velocity)>()
            .for_each(|_entity, (position, velocity)| {
                position.x += velocity.x * delta_time.0;
                position.y += velocity.y * delta_time.0;
            });
    });
}

// resources_scope is the tuple form for several resources at once. The
// take is all-or-nothing and everything is restored even on panic.
fn scoring(world: &mut DynWorld) {
    world.resources_scope(|world, (delta_time, score): &mut (DeltaTime, Score)| {
        score.0 += world
            .query_ref::<&Position>()
            .iter()
            .filter(|(_entity, position)| position.x * delta_time.0 > 0.25)
            .count() as u32;
    });
}

// Read-only systems can take &World and slot in with push_readonly.
fn report(world: &DynWorld) {
    for (entity, position) in world.query_ref::<&Position>().iter() {
        println!("{entity} is at ({}, {})", position.x, position.y);
    }
}

fn main() {
    let mut world = DynWorld::new();

    // Spawning takes bundles of component values; types register lazily.
    let player = world.spawn((Position { x: 1.0, y: 2.0 }, Velocity { x: 3.0, y: 0.0 }));
    world.add_tag_type::<Player>(player);
    world.spawn_bundles((Position::default(), Velocity { x: 1.0, y: 0.0 }), 8);

    // Deferred spawns hand back the handle immediately; the components
    // arrive when apply_commands runs at a safe point.
    let reserved = world.queue_spawn((Position::default(),));
    world.apply_commands();
    assert!(world.is_alive(reserved));

    // Component access: get / get_mut / set / remove / has. set adds the
    // component when the entity lacks it, and mutation stamps change ticks.
    world.set(player, Velocity { x: 4.0, y: 0.0 });
    if let Some(position) = world.get_mut::<Position>(player) {
        position.x += 0.5;
    }
    assert!(world.has::<Velocity>(player));

    // Resources insert by value and read back typed; expect_resource
    // panics with the type name for engine-style singletons.
    world.insert_resource(DeltaTime(0.5));
    world.insert_resource(Score(0));
    assert_eq!(world.expect_resource::<DeltaTime>().0, 0.5);

    // Host scopes: ResourceHostExt gives any ResourceHost the scopes as
    // methods, and the closure receives the host itself, so a system can
    // mutate engine state, world state, and the resource in one pass.
    let mut engine = Engine {
        world: DynWorld::new(),
        frames: 0,
    };
    engine.world.insert_resource(Score(0));
    engine.resource_scope(|engine, score: &mut Score| {
        engine.frames += 1;
        score.0 += 1;
    });
    assert_eq!(engine.frames, 1);
    assert_eq!(engine.world.expect_resource::<Score>().0, 1);

    // step() ends a frame: it expires old events and opens the next
    // change-detection window, so the systems below read as "this frame".
    world.step();

    // Systems compose into a schedule; push_if gates on a condition and
    // push_readonly takes the &World form.
    let mut schedule = Schedule::new();
    schedule
        .push("movement", movement)
        .push_if(
            "scoring",
            |world: &DynWorld| world.entity_count() > 0,
            scoring,
        )
        .push_readonly("report", report);
    schedule.run(&mut world);

    // Marker tags: add, test, and filter queries by them.
    world.add_tag_type::<Frozen>(reserved);
    assert!(world.has_tag_type::<Frozen>(reserved));
    world
        .query::<&mut Position>()
        .without_tag_type::<Frozen>()
        .for_each(|_entity, position| position.y += 1.0);

    // Change windows: changed matches mutation since the last step, added
    // matches component arrival. Both work on either query form.
    let moved = world
        .query_ref::<&Position>()
        .changed::<Position>()
        .iter()
        .count();
    println!("{moved} entities moved this frame");

    // Read queries are real iterators: single() is the exactly-one read
    // (here narrowed by tag), iter_combinations() yields each unordered
    // pair once.
    if let Some((entity, velocity)) = world
        .query_ref::<&Velocity>()
        .with_tag_type::<Player>()
        .single()
    {
        println!("the player is {entity} moving at {}", velocity.x);
    }
    let mut near_pairs = 0;
    for ((_a, a), (_b, b)) in world.query_ref::<&Position>().iter_combinations() {
        if (a.x - b.x).abs() < 1.0 {
            near_pairs += 1;
        }
    }
    println!("{near_pairs} pairs are close together");

    // Heavy passes go parallel without leaving the typed tier; tables run
    // concurrently, rows within a table stay sequential.
    #[cfg(not(target_family = "wasm"))]
    world
        .query::<(&mut Position, &Velocity)>()
        .par_for_each(|_entity, (position, velocity)| {
            position.x += velocity.x;
        });

    // Events buffer for two frames; consume_events with a per-consumer
    // cursor delivers each event exactly once.
    world.send(Damage { amount: 10.0 });
    let mut damage_cursor = 0;
    for event in world.consume_events::<Damage>(&mut damage_cursor) {
        println!("took {} damage", event.amount);
    }
    assert!(
        world
            .consume_events::<Damage>(&mut damage_cursor)
            .is_empty()
    );

    // Hierarchies are plain ChildOf links; children() scans on demand and
    // despawn_recursive follows the links breadth-first.
    let parent = world.spawn((Position::default(),));
    let child = world.spawn((Position::default(), ChildOf(parent)));
    assert_eq!(world.children(parent), vec![child]);
    world.despawn_recursive(parent);
    assert!(!world.is_alive(child));

    // Grouped worlds: members declared with explicit registries share one
    // entity allocator, and spawn_with routes each component of a bundle
    // to the member world that registered its type.
    let mut core = ComponentRegistry::new();
    core.register::<Position>();
    let mut effects = ComponentRegistry::new();
    effects.register::<Burning>();
    let mut ecs = DynEcs::new();
    ecs.add_world_at(0, core);
    ecs.add_world_at(1, effects);
    let torch = ecs.spawn_with((Position { x: 0.0, y: 0.0 }, Burning { lift: 2.0 }));

    // Group-typed access needs no world index; routing finds the owner.
    ecs.get_mut::<Position>(torch).unwrap().x = 1.0;
    ecs.add_tag_type::<Selected>(torch);

    // Cross-world tuples run through query_join with the same filter
    // vocabulary: one world drives at slice speed, the others resolve
    // per entity, and mutation stays in the driver by rule.
    ecs.worlds[0].step();
    ecs.get_mut::<Position>(torch).unwrap().y = 5.0;
    ecs.query_join::<(&mut Position, &Burning)>()
        .with_tag_type::<Selected>()
        .changed::<Position>()
        .for_each(|_entity, (position, burning)| {
            position.y += burning.lift;
        });
    assert_eq!(ecs.get::<Position>(torch).unwrap().y, 7.0);
    println!("the selected torch rose to y = 7");
}
