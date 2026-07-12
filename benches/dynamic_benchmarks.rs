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

criterion_group!(
    benches,
    bench_dynamic_spawn,
    bench_dynamic_iteration,
    bench_dynamic_access
);
criterion_main!(benches);
