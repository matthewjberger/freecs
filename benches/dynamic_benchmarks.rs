use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use freecs::dynamic::DynWorld;
use std::hint::black_box;

#[derive(Default, Debug, Clone, Copy)]
pub struct Position {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct Velocity {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

fn spawned_world(count: usize) -> DynWorld {
    let mut world = DynWorld::new();
    let position = world.register::<Position>();
    let velocity = world.register::<Velocity>();
    world.spawn_batch(position.mask | velocity.mask, count, |table, index| {
        table.column_mut(position)[index] = Position {
            x: index as f32,
            y: index as f32 * 2.0,
            z: 0.0,
        };
        table.column_mut(velocity)[index] = Velocity {
            x: 0.1,
            y: 0.2,
            z: 0.3,
        };
    });
    world
}

fn bench_dynamic_spawn(c: &mut Criterion) {
    let mut group = c.benchmark_group("dynamic_spawn");
    for count in [1000, 10000] {
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::new("spawn_batch", count),
            &count,
            |bencher, &count| {
                bencher.iter(|| {
                    let world = spawned_world(count);
                    black_box(world);
                });
            },
        );
    }
    group.finish();
}

fn bench_dynamic_iteration(c: &mut Criterion) {
    let mut group = c.benchmark_group("dynamic_iteration");
    for count in [1000, 10000, 100000] {
        let mut world = spawned_world(count);
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(
            BenchmarkId::new("typed_query_mutation", count),
            &count,
            |bencher, _| {
                bencher.iter(|| {
                    world.query::<(&mut Position, &Velocity)>().for_each(
                        |_entity, (position, velocity)| {
                            position.x += velocity.x;
                            position.y += velocity.y;
                            position.z += velocity.z;
                        },
                    );
                });
            },
        );

        let position = world.register::<Position>();
        let velocity = world.register::<Velocity>();
        group.bench_with_input(
            BenchmarkId::new("table_columns_mutation", count),
            &count,
            |bencher, _| {
                bencher.iter(|| {
                    world.for_each_tables_mut(position.mask | velocity.mask, 0, |table| {
                        let (positions, velocities) = table.columns_pair(position, velocity);
                        for (position_value, velocity_value) in positions.iter_mut().zip(velocities)
                        {
                            position_value.x += velocity_value.x;
                            position_value.y += velocity_value.y;
                            position_value.z += velocity_value.z;
                        }
                    });
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("query_ref_iterator_read", count),
            &count,
            |bencher, _| {
                bencher.iter(|| {
                    let mut sum = 0.0;
                    for (_entity, (position_value, velocity_value)) in
                        world.query_ref::<(&Position, &Velocity)>().iter()
                    {
                        sum += position_value.x + position_value.y + position_value.z;
                        sum += velocity_value.x;
                    }
                    black_box(sum);
                });
            },
        );
    }
    group.finish();
}

fn bench_dynamic_access(c: &mut Criterion) {
    let mut group = c.benchmark_group("dynamic_access");
    let mut world = spawned_world(10000);
    let position = world.register::<Position>();
    let entities = world.get_all_entities();

    group.bench_function("get_keyed", |bencher| {
        bencher.iter(|| {
            let mut sum = 0.0;
            for &entity in &entities {
                sum += world.get_keyed(position, entity).unwrap().x;
            }
            black_box(sum);
        });
    });

    group.bench_function("get_typed_lazy", |bencher| {
        bencher.iter(|| {
            let mut sum = 0.0;
            for &entity in &entities {
                sum += world.get::<Position>(entity).unwrap().x;
            }
            black_box(sum);
        });
    });

    group.finish();
}

fn bench_dynamic_migrations(c: &mut Criterion) {
    let mut group = c.benchmark_group("dynamic_migrations");
    for count in [100, 1000] {
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::new("add_component", count),
            &count,
            |bencher, &count| {
                bencher.iter(|| {
                    let mut world = DynWorld::new();
                    let position = world.register::<Position>();
                    let velocity = world.register::<Velocity>();
                    let entities = world.spawn_entities(position.mask, count);
                    for &entity in &entities {
                        world.add_components(entity, velocity.mask);
                    }
                    black_box(world);
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("remove_component", count),
            &count,
            |bencher, &count| {
                bencher.iter(|| {
                    let mut world = DynWorld::new();
                    let position = world.register::<Position>();
                    let velocity = world.register::<Velocity>();
                    let entities = world.spawn_entities(position.mask | velocity.mask, count);
                    for &entity in &entities {
                        world.remove_components(entity, velocity.mask);
                    }
                    black_box(world);
                });
            },
        );
    }
    group.finish();
}

fn bench_dynamic_tags(c: &mut Criterion) {
    let mut group = c.benchmark_group("dynamic_tags");
    let entity_count = 10000usize;

    let mut world = spawned_world(entity_count);
    let position = world.register::<Position>();
    let boss = world.register_tag();
    let entities = world.get_all_entities();
    for &entity in entities.iter().take(entity_count / 2) {
        world.add_tag(boss, entity);
    }

    group.throughput(Throughput::Elements(entity_count as u64));

    group.bench_function("has_tag_lookup", |bencher| {
        bencher.iter(|| {
            let mut count = 0;
            for &entity in &entities {
                if world.has_tag(boss, entity) {
                    count += 1;
                }
            }
            black_box(count);
        });
    });

    group.bench_function("typed_query_with_tag", |bencher| {
        bencher.iter(|| {
            let mut sum = 0.0;
            world
                .query::<(&Position,)>()
                .with_tag(boss)
                .for_each(|_entity, (position_value,)| {
                    sum += position_value.x;
                });
            black_box(sum);
        });
    });

    group.bench_function("mask_query_without_tag", |bencher| {
        bencher.iter(|| {
            let mut count = 0;
            world.for_each(position.mask | boss.mask, 0, |_entity, _table, _index| {
                count += 1;
            });
            black_box(count);
        });
    });

    group.finish();
}

fn bench_dynamic_change_detection(c: &mut Criterion) {
    let mut group = c.benchmark_group("dynamic_change_detection");
    let entity_count = 10000usize;

    let mut world = spawned_world(entity_count);
    let position = world.register::<Position>();
    let entities = world.get_all_entities();
    world.step();
    for &entity in entities.iter().take(entity_count / 10) {
        world.get_mut_keyed(position, entity).unwrap().x += 1.0;
    }
    world.step();

    group.throughput(Throughput::Elements(entity_count as u64));
    group.bench_function("detect_changed_components", |bencher| {
        bencher.iter(|| {
            let count = world.query_entities_changed(position.mask).count();
            black_box(count);
        });
    });
    group.finish();
}

#[derive(Debug, Clone)]
struct DamageEvent {
    entity: freecs::Entity,
    amount: f32,
}

fn bench_dynamic_events(c: &mut Criterion) {
    let mut group = c.benchmark_group("dynamic_events");
    group.throughput(Throughput::Elements(1000));

    group.bench_function("send_events_1000", |bencher| {
        bencher.iter(|| {
            let mut world = spawned_world(1000);
            let entities = world.get_all_entities();
            for &entity in &entities {
                world.send(DamageEvent {
                    entity,
                    amount: 10.0,
                });
            }
            black_box(world);
        });
    });

    group.bench_function("read_events_since_cursor", |bencher| {
        let mut world = spawned_world(1000);
        let entities = world.get_all_entities();
        for &entity in &entities {
            world.send(DamageEvent {
                entity,
                amount: 10.0,
            });
        }
        bencher.iter(|| {
            let mut total = 0.0;
            let mut id_sum = 0u32;
            for event in world.read_events_since::<DamageEvent>(0) {
                total += black_box(event).amount;
                id_sum = id_sum.wrapping_add(event.entity.id);
            }
            black_box((total, id_sum));
        });
    });

    group.finish();
}

fn bench_dynamic_commands(c: &mut Criterion) {
    let mut group = c.benchmark_group("dynamic_commands");
    for count in [100, 1000] {
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::new("queue_component_sets", count),
            &count,
            |bencher, &count| {
                bencher.iter(|| {
                    let mut world = spawned_world(count);
                    let entities = world.get_all_entities();
                    for &entity in &entities {
                        world.queue_set(
                            entity,
                            Position {
                                x: 1.0,
                                y: 2.0,
                                z: 3.0,
                            },
                        );
                    }
                    world.apply_commands();
                    black_box(world);
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("queue_despawns", count),
            &count,
            |bencher, &count| {
                bencher.iter(|| {
                    let mut world = spawned_world(count);
                    let entities = world.get_all_entities();
                    for &entity in &entities {
                        world.queue_despawn_entity(entity);
                    }
                    world.apply_commands();
                    black_box(world);
                });
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_dynamic_spawn,
    bench_dynamic_iteration,
    bench_dynamic_access,
    bench_dynamic_migrations,
    bench_dynamic_tags,
    bench_dynamic_change_detection,
    bench_dynamic_events,
    bench_dynamic_commands
);
criterion_main!(benches);
