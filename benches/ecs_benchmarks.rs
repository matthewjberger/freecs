use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use freecs::ecs;
use rand::seq::SliceRandom;
use std::hint::black_box;

ecs! {
    World {
        position: Position => POSITION,
        velocity: Velocity => VELOCITY,
        health: Health => HEALTH,
        damage: Damage => DAMAGE,
        sprite: Sprite => SPRITE,
    }
    Tags {
        player => PLAYER,
        enemy => ENEMY,
        friendly => FRIENDLY,
        dead => DEAD,
    }
    Events {
        damage_event: DamageEvent,
        heal_event: HealEvent,
        collision_event: CollisionEvent,
    }
    Resources {
        frame_count: u64,
        delta_time: f32,
    }
}

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

#[derive(Default, Debug, Clone, Copy)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct Damage {
    pub value: f32,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct Sprite {
    pub texture_id: u32,
    pub layer: u32,
}

#[derive(Debug, Clone)]
pub struct DamageEvent {
    pub entity: freecs::Entity,
    pub amount: f32,
}

#[derive(Debug, Clone)]
pub struct HealEvent {
    pub entity: freecs::Entity,
    pub amount: f32,
}

#[derive(Debug, Clone)]
pub struct CollisionEvent {
    pub entity_a: freecs::Entity,
    pub entity_b: freecs::Entity,
}

fn bench_spawn_entities(c: &mut Criterion) {
    let mut group = c.benchmark_group("spawn_entities");

    for count in [100, 1000, 10000] {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(
            BenchmarkId::new("spawn_individual", count),
            &count,
            |b, &count| {
                b.iter_batched(
                    || World::default(),
                    |mut world| {
                        for _ in 0..count {
                            world.spawn_entities(POSITION | VELOCITY, 1);
                        }
                        black_box(world);
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("spawn_batch", count),
            &count,
            |b, &count| {
                b.iter_batched(
                    || World::default(),
                    |mut world| {
                        world.spawn_batch(POSITION | VELOCITY, count, |_table, _idx| {});
                        black_box(world);
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("spawn_batch_initialized", count),
            &count,
            |b, &count| {
                b.iter_batched(
                    || World::default(),
                    |mut world| {
                        world.spawn_batch(POSITION | VELOCITY, count, |table, idx| {
                            table.position[idx] = Position {
                                x: 1.0,
                                y: 2.0,
                                z: 3.0,
                            };
                            table.velocity[idx] = Velocity {
                                x: 0.1,
                                y: 0.2,
                                z: 0.3,
                            };
                        });
                        black_box(world);
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

fn bench_iteration(c: &mut Criterion) {
    let mut group = c.benchmark_group("iteration");

    for count in [1000, 10000, 100000] {
        let mut world = World::default();
        world.spawn_batch(POSITION | VELOCITY, count, |table, idx| {
            table.position[idx] = Position {
                x: idx as f32,
                y: idx as f32 * 2.0,
                z: 0.0,
            };
            table.velocity[idx] = Velocity {
                x: 0.1,
                y: 0.2,
                z: 0.3,
            };
        });

        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(
            BenchmarkId::new("single_component_iter", count),
            &count,
            |b, _| {
                b.iter(|| {
                    let mut sum = 0.0;
                    world.iter_position(|_entity, pos| {
                        sum += pos.x + pos.y + pos.z;
                    });
                    black_box(sum);
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("two_component_query", count),
            &count,
            |b, _| {
                b.iter(|| {
                    let mut sum = 0.0;
                    world
                        .query()
                        .with(POSITION | VELOCITY)
                        .iter(|_entity, table, idx| {
                            sum += table.position[idx].x + table.velocity[idx].x;
                        });
                    black_box(sum);
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("two_component_mutation", count),
            &count,
            |b, _| {
                b.iter(|| {
                    world
                        .query_mut()
                        .with(POSITION | VELOCITY)
                        .iter(|_entity, table, idx| {
                            table.position[idx].x += table.velocity[idx].x;
                            table.position[idx].y += table.velocity[idx].y;
                            table.position[idx].z += table.velocity[idx].z;
                        });
                });
            },
        );
    }

    group.finish();
}

fn bench_complex_queries(c: &mut Criterion) {
    let mut group = c.benchmark_group("complex_queries");

    let entity_count = 10000;
    let mut world = World::default();

    let entities = world.spawn_batch(POSITION | VELOCITY | HEALTH, entity_count, |table, idx| {
        table.position[idx] = Position {
            x: idx as f32,
            y: 0.0,
            z: 0.0,
        };
        table.velocity[idx] = Velocity {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        };
        table.health[idx] = Health {
            current: 100.0,
            max: 100.0,
        };
    });

    for i in 0..entity_count / 2 {
        world.add_enemy(entities[i]);
    }
    for i in entity_count / 2..entity_count {
        world.add_friendly(entities[i]);
    }

    group.throughput(Throughput::Elements(entity_count as u64));

    group.bench_function("query_with_tag_filter", |b| {
        b.iter(|| {
            let mut sum = 0.0;
            world
                .query()
                .with(POSITION | VELOCITY | ENEMY)
                .iter(|_entity, table, idx| {
                    sum += table.position[idx].x;
                });
            black_box(sum);
        });
    });

    group.bench_function("query_without_tag", |b| {
        b.iter(|| {
            let mut sum = 0.0;
            world
                .query()
                .with(POSITION | VELOCITY)
                .without(DEAD)
                .iter(|_entity, table, idx| {
                    sum += table.position[idx].x;
                });
            black_box(sum);
        });
    });

    group.bench_function("query_multiple_tags", |b| {
        b.iter(|| {
            let mut sum = 0.0;
            world
                .query()
                .with(POSITION | VELOCITY | HEALTH | ENEMY)
                .without(DEAD | FRIENDLY)
                .iter(|_entity, table, idx| {
                    sum += table.position[idx].x + table.health[idx].current;
                });
            black_box(sum);
        });
    });

    group.finish();
}

fn bench_change_detection(c: &mut Criterion) {
    let mut group = c.benchmark_group("change_detection");

    let entity_count = 10000;
    let mut world = World::default();

    world.spawn_batch(POSITION | VELOCITY, entity_count, |table, idx| {
        table.position[idx] = Position {
            x: idx as f32,
            y: 0.0,
            z: 0.0,
        };
        table.velocity[idx] = Velocity {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        };
    });

    world
        .query_mut()
        .with(POSITION)
        .iter(|_entity, table, idx| {
            if idx % 10 == 0 {
                table.position[idx].x += 1.0;
            }
        });

    world.increment_tick();

    group.throughput(Throughput::Elements(entity_count as u64));

    group.bench_function("detect_changed_components", |b| {
        b.iter(|| {
            let mut count = 0;
            world.for_each_mut_changed(POSITION, 0, |_entity, _table, _idx| {
                count += 1;
            });
            black_box(count);
        });
    });

    group.finish();
}

fn bench_command_buffers(c: &mut Criterion) {
    let mut group = c.benchmark_group("command_buffers");

    for count in [100, 1000, 10000] {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(
            BenchmarkId::new("queue_component_sets", count),
            &count,
            |b, &count| {
                b.iter(|| {
                    let mut world = World::default();
                    let entities = world.spawn_batch(POSITION, count, |_table, _idx| {});

                    for &entity in &entities {
                        world.queue_set_position(
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
            BenchmarkId::new("queue_tag_adds", count),
            &count,
            |b, &count| {
                b.iter(|| {
                    let mut world = World::default();
                    let entities = world.spawn_batch(POSITION, count, |_table, _idx| {});

                    for &entity in &entities {
                        world.queue_add_enemy(entity);
                    }

                    world.apply_commands();
                    black_box(world);
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("queue_despawns", count),
            &count,
            |b, &count| {
                b.iter(|| {
                    let mut world = World::default();
                    let entities = world.spawn_batch(POSITION, count, |_table, _idx| {});

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

fn bench_sparse_sets(c: &mut Criterion) {
    let mut group = c.benchmark_group("sparse_sets_tags");

    let entity_count = 10000;
    let mut world = World::default();
    let entities = world.spawn_batch(POSITION | VELOCITY, entity_count, |_table, _idx| {});

    for &entity in &entities {
        world.add_player(entity);
    }

    group.throughput(Throughput::Elements(entity_count as u64));

    group.bench_function("add_tag", |b| {
        b.iter(|| {
            let mut temp_world = World::default();
            let temp_entities = temp_world.spawn_batch(POSITION, 1000, |_table, _idx| {});
            for &entity in &temp_entities {
                temp_world.add_enemy(entity);
            }
            black_box(temp_world);
        });
    });

    group.bench_function("remove_tag", |b| {
        b.iter(|| {
            let mut temp_world = World::default();
            let temp_entities = temp_world.spawn_batch(POSITION, 1000, |_table, _idx| {});
            for &entity in &temp_entities {
                temp_world.add_enemy(entity);
            }
            for &entity in &temp_entities {
                temp_world.remove_enemy(entity);
            }
            black_box(temp_world);
        });
    });

    group.bench_function("has_tag_lookup", |b| {
        b.iter(|| {
            let mut count = 0;
            for &entity in &entities {
                if world.has_player(entity) {
                    count += 1;
                }
            }
            black_box(count);
        });
    });

    group.finish();
}

fn bench_fragmentation_resistance(c: &mut Criterion) {
    let mut group = c.benchmark_group("fragmentation_resistance");

    group.bench_function("many_archetypes_iteration", |b| {
        b.iter(|| {
            let mut world = World::default();

            world.spawn_batch(POSITION, 1000, |_table, _idx| {});
            world.spawn_batch(POSITION | VELOCITY, 1000, |_table, _idx| {});
            world.spawn_batch(POSITION | VELOCITY | HEALTH, 1000, |_table, _idx| {});
            world.spawn_batch(POSITION | HEALTH, 1000, |_table, _idx| {});
            world.spawn_batch(VELOCITY | HEALTH, 1000, |_table, _idx| {});

            let mut sum = 0.0;
            world.query().with(POSITION).iter(|_entity, table, idx| {
                sum += table.position[idx].x;
            });

            black_box(sum);
        });
    });

    group.bench_function("tag_fragmentation_resistance", |b| {
        b.iter(|| {
            let mut world = World::default();
            let entities = world.spawn_batch(POSITION | VELOCITY, 5000, |_table, _idx| {});

            for i in 0..entities.len() {
                match i % 4 {
                    0 => world.add_player(entities[i]),
                    1 => world.add_enemy(entities[i]),
                    2 => world.add_friendly(entities[i]),
                    _ => {}
                }
            }

            let mut count = 0;
            world
                .query()
                .with(POSITION | PLAYER)
                .iter(|_entity, _table, _idx| {
                    count += 1;
                });

            black_box(count);
        });
    });

    group.finish();
}

fn bench_realistic_game_loop(c: &mut Criterion) {
    let mut group = c.benchmark_group("realistic_game_loop");

    group.bench_function("simple_physics_update", |b| {
        b.iter(|| {
            let mut world = World::default();
            world.spawn_batch(POSITION | VELOCITY, 5000, |table, idx| {
                table.position[idx] = Position {
                    x: idx as f32,
                    y: 0.0,
                    z: 0.0,
                };
                table.velocity[idx] = Velocity {
                    x: 1.0,
                    y: 1.0,
                    z: 0.0,
                };
            });

            let dt = 0.016;

            world
                .query_mut()
                .with(POSITION | VELOCITY)
                .iter(|_entity, table, idx| {
                    table.position[idx].x += table.velocity[idx].x * dt;
                    table.position[idx].y += table.velocity[idx].y * dt;
                    table.position[idx].z += table.velocity[idx].z * dt;
                });

            black_box(world);
        });
    });

    group.bench_function("combat_system_simulation", |b| {
        b.iter(|| {
            let mut world = World::default();
            let entities = world.spawn_batch(POSITION | HEALTH | DAMAGE, 1000, |table, idx| {
                table.position[idx] = Position {
                    x: idx as f32,
                    y: 0.0,
                    z: 0.0,
                };
                table.health[idx] = Health {
                    current: 100.0,
                    max: 100.0,
                };
                table.damage[idx] = Damage { value: 10.0 };
            });

            for i in 0..entities.len() / 2 {
                world.add_enemy(entities[i]);
            }
            for i in entities.len() / 2..entities.len() {
                world.add_friendly(entities[i]);
            }

            world
                .query_mut()
                .with(HEALTH | ENEMY)
                .iter(|_entity, table, idx| {
                    table.health[idx].current -= 5.0;
                });

            world
                .query_mut()
                .with(HEALTH | FRIENDLY)
                .iter(|_entity, table, idx| {
                    table.health[idx].current =
                        (table.health[idx].current + 2.0).min(table.health[idx].max);
                });

            black_box(world);
        });
    });

    group.finish();
}

fn bench_parallel_iteration(c: &mut Criterion) {
    let mut group = c.benchmark_group("parallel_iteration");

    for count in [10000, 100000, 500000] {
        let mut world = World::default();
        world.spawn_batch(POSITION | VELOCITY, count, |table, idx| {
            table.position[idx] = Position {
                x: idx as f32,
                y: idx as f32 * 2.0,
                z: 0.0,
            };
            table.velocity[idx] = Velocity {
                x: 0.1,
                y: 0.2,
                z: 0.3,
            };
        });

        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(
            BenchmarkId::new("serial_iteration", count),
            &count,
            |b, _| {
                b.iter(|| {
                    world
                        .query_mut()
                        .with(POSITION | VELOCITY)
                        .iter(|_entity, table, idx| {
                            table.position[idx].x += table.velocity[idx].x;
                            table.position[idx].y += table.velocity[idx].y;
                            table.position[idx].z += table.velocity[idx].z;
                        });
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("parallel_iteration", count),
            &count,
            |b, _| {
                b.iter(|| {
                    world.par_for_each_mut(POSITION | VELOCITY, 0, |_entity, table, idx| {
                        table.position[idx].x += table.velocity[idx].x;
                        table.position[idx].y += table.velocity[idx].y;
                        table.position[idx].z += table.velocity[idx].z;
                    });
                });
            },
        );
    }

    group.finish();
}

fn bench_entity_lookups(c: &mut Criterion) {
    let mut group = c.benchmark_group("entity_lookups");

    for count in [1000, 10000, 100000] {
        let mut world = World::default();
        let entities = world.spawn_batch(POSITION | VELOCITY | HEALTH, count, |table, idx| {
            table.position[idx] = Position {
                x: idx as f32,
                y: 0.0,
                z: 0.0,
            };
            table.velocity[idx] = Velocity {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            };
            table.health[idx] = Health {
                current: 100.0,
                max: 100.0,
            };
        });

        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(
            BenchmarkId::new("sequential_lookups", count),
            &count,
            |b, _| {
                b.iter(|| {
                    let mut sum = 0.0;
                    for &entity in &entities {
                        if let Some(pos) = world.get_position(entity) {
                            sum += pos.x;
                        }
                    }
                    black_box(sum);
                });
            },
        );

        group.bench_with_input(BenchmarkId::new("random_lookups", count), &count, |b, _| {
            let mut indices: Vec<usize> = (0..count).collect();
            let mut rng = rand::rng();
            indices.shuffle(&mut rng);
            b.iter(|| {
                let mut sum = 0.0;
                for &idx in &indices {
                    if let Some(pos) = world.get_position(entities[idx]) {
                        sum += pos.x;
                    }
                }
                black_box(sum);
            });
        });

        group.bench_with_input(
            BenchmarkId::new("mutable_lookups", count),
            &count,
            |b, _| {
                b.iter(|| {
                    for &entity in &entities {
                        if let Some(pos) = world.get_position_mut(entity) {
                            pos.x += 0.1;
                        }
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_archetype_migrations(c: &mut Criterion) {
    let mut group = c.benchmark_group("archetype_migrations");

    for count in [100, 1000, 5000] {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(
            BenchmarkId::new("add_component", count),
            &count,
            |b, &count| {
                b.iter_batched(
                    || {
                        let mut world = World::default();
                        let entities =
                            world.spawn_batch(POSITION | VELOCITY, count, |_table, _idx| {});
                        (world, entities)
                    },
                    |(mut world, entities)| {
                        for &entity in &entities {
                            world.queue_set_health(
                                entity,
                                Health {
                                    current: 100.0,
                                    max: 100.0,
                                },
                            );
                        }
                        world.apply_commands();
                        black_box(world);
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("remove_component", count),
            &count,
            |b, &count| {
                b.iter_batched(
                    || {
                        let mut world = World::default();
                        let entities =
                            world.spawn_batch(POSITION | VELOCITY | HEALTH, count, |table, idx| {
                                table.position[idx] = Position::default();
                                table.velocity[idx] = Velocity::default();
                                table.health[idx] = Health {
                                    current: 100.0,
                                    max: 100.0,
                                };
                            });
                        (world, entities)
                    },
                    |(mut world, entities)| {
                        for &entity in &entities {
                            world.queue_remove_components(entity, HEALTH);
                        }
                        world.apply_commands();
                        black_box(world);
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        group.bench_with_input(BenchmarkId::new("add_tag", count), &count, |b, &count| {
            b.iter_batched(
                || {
                    let mut world = World::default();
                    let entities = world.spawn_batch(POSITION, count, |_table, _idx| {});
                    (world, entities)
                },
                |(mut world, entities)| {
                    for &entity in &entities {
                        world.add_enemy(entity);
                    }
                    black_box(world);
                },
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

fn bench_large_scale(c: &mut Criterion) {
    let mut group = c.benchmark_group("large_scale");
    group.sample_size(100);

    for count in [500000, 1000000] {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(
            BenchmarkId::new("spawn_large", count),
            &count,
            |b, &count| {
                b.iter_batched(
                    || World::default(),
                    |mut world| {
                        world.spawn_batch(POSITION | VELOCITY, count, |table, idx| {
                            table.position[idx] = Position {
                                x: idx as f32,
                                y: 0.0,
                                z: 0.0,
                            };
                            table.velocity[idx] = Velocity {
                                x: 1.0,
                                y: 1.0,
                                z: 1.0,
                            };
                        });
                        black_box(world);
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("iterate_large", count),
            &count,
            |b, &count| {
                let mut world = World::default();
                world.spawn_batch(POSITION | VELOCITY, count, |table, idx| {
                    table.position[idx] = Position {
                        x: idx as f32,
                        y: 0.0,
                        z: 0.0,
                    };
                    table.velocity[idx] = Velocity {
                        x: 1.0,
                        y: 1.0,
                        z: 1.0,
                    };
                });

                b.iter(|| {
                    let mut sum = 0.0;
                    world
                        .query()
                        .with(POSITION | VELOCITY)
                        .iter(|_entity, table, idx| {
                            sum += table.position[idx].x + table.velocity[idx].x;
                        });
                    black_box(sum);
                });
            },
        );
    }

    group.finish();
}

fn bench_multiple_archetypes(c: &mut Criterion) {
    let mut group = c.benchmark_group("multiple_archetypes");

    group.bench_function("iterate_10_archetypes", |b| {
        let mut world = World::default();

        world.spawn_batch(POSITION, 1000, |_table, _idx| {});
        world.spawn_batch(POSITION | VELOCITY, 1000, |_table, _idx| {});
        world.spawn_batch(POSITION | HEALTH, 1000, |_table, _idx| {});
        world.spawn_batch(POSITION | VELOCITY | HEALTH, 1000, |_table, _idx| {});
        world.spawn_batch(VELOCITY | HEALTH, 1000, |_table, _idx| {});
        world.spawn_batch(
            POSITION | VELOCITY | HEALTH | DAMAGE,
            1000,
            |_table, _idx| {},
        );
        world.spawn_batch(VELOCITY, 1000, |_table, _idx| {});
        world.spawn_batch(HEALTH, 1000, |_table, _idx| {});
        world.spawn_batch(POSITION | DAMAGE, 1000, |_table, _idx| {});
        world.spawn_batch(VELOCITY | DAMAGE, 1000, |_table, _idx| {});

        b.iter(|| {
            let mut count = 0;
            world.query().with(POSITION).iter(|_entity, _table, _idx| {
                count += 1;
            });
            black_box(count);
        });
    });

    group.bench_function("query_across_archetypes", |b| {
        let mut world = World::default();

        world.spawn_batch(POSITION | VELOCITY, 2000, |_table, _idx| {});
        world.spawn_batch(POSITION | VELOCITY | HEALTH, 2000, |_table, _idx| {});
        world.spawn_batch(POSITION | VELOCITY | DAMAGE, 2000, |_table, _idx| {});
        world.spawn_batch(
            POSITION | VELOCITY | HEALTH | DAMAGE,
            2000,
            |_table, _idx| {},
        );
        world.spawn_batch(POSITION | VELOCITY | SPRITE, 2000, |_table, _idx| {});

        b.iter(|| {
            let mut sum = 0.0;
            world
                .query()
                .with(POSITION | VELOCITY)
                .iter(|_entity, table, idx| {
                    sum += table.position[idx].x + table.velocity[idx].x;
                });
            black_box(sum);
        });
    });

    group.finish();
}

fn bench_50_plus_archetypes(c: &mut Criterion) {
    let mut group = c.benchmark_group("archetype_scaling");

    group.bench_function("query_50_archetypes", |b| {
        let mut world = World::default();

        let component_masks = [
            POSITION,
            VELOCITY,
            HEALTH,
            DAMAGE,
            SPRITE,
            POSITION | VELOCITY,
            POSITION | HEALTH,
            POSITION | DAMAGE,
            POSITION | SPRITE,
            VELOCITY | HEALTH,
            VELOCITY | DAMAGE,
            VELOCITY | SPRITE,
            HEALTH | DAMAGE,
            HEALTH | SPRITE,
            DAMAGE | SPRITE,
            POSITION | VELOCITY | HEALTH,
            POSITION | VELOCITY | DAMAGE,
            POSITION | VELOCITY | SPRITE,
            POSITION | HEALTH | DAMAGE,
            POSITION | HEALTH | SPRITE,
            POSITION | DAMAGE | SPRITE,
            VELOCITY | HEALTH | DAMAGE,
            VELOCITY | HEALTH | SPRITE,
            VELOCITY | DAMAGE | SPRITE,
            HEALTH | DAMAGE | SPRITE,
            POSITION | VELOCITY | HEALTH | DAMAGE,
            POSITION | VELOCITY | HEALTH | SPRITE,
            POSITION | VELOCITY | DAMAGE | SPRITE,
            POSITION | HEALTH | DAMAGE | SPRITE,
            VELOCITY | HEALTH | DAMAGE | SPRITE,
            POSITION | VELOCITY | HEALTH | DAMAGE | SPRITE,
            POSITION | PLAYER,
            POSITION | ENEMY,
            POSITION | FRIENDLY,
            POSITION | DEAD,
            VELOCITY | PLAYER,
            VELOCITY | ENEMY,
            VELOCITY | FRIENDLY,
            VELOCITY | DEAD,
            HEALTH | PLAYER,
            HEALTH | ENEMY,
            HEALTH | FRIENDLY,
            HEALTH | DEAD,
            POSITION | VELOCITY | PLAYER,
            POSITION | VELOCITY | ENEMY,
            POSITION | VELOCITY | FRIENDLY,
            POSITION | VELOCITY | DEAD,
            POSITION | HEALTH | PLAYER,
            POSITION | HEALTH | ENEMY,
            POSITION | HEALTH | FRIENDLY,
        ];

        for mask in &component_masks {
            world.spawn_batch(*mask, 100, |_, _| {});
        }

        b.iter(|| {
            let mut count = 0;
            world.query().with(POSITION).iter(|_entity, _table, _idx| {
                count += 1;
            });
            black_box(count);
        });
    });

    group.finish();
}

fn bench_cold_cache(c: &mut Criterion) {
    let mut group = c.benchmark_group("cold_cache");

    group.bench_function("cold_cache_iteration_10k", |b| {
        b.iter_batched(
            || {
                let mut world = World::default();
                world.spawn_batch(POSITION | VELOCITY, 10000, |table, idx| {
                    table.position[idx] = Position {
                        x: idx as f32,
                        y: 0.0,
                        z: 0.0,
                    };
                    table.velocity[idx] = Velocity {
                        x: 1.0,
                        y: 1.0,
                        z: 1.0,
                    };
                });
                world
            },
            |world| {
                let mut sum = 0.0;
                world
                    .query()
                    .with(POSITION | VELOCITY)
                    .iter(|_entity, table, idx| {
                        sum += table.position[idx].x + table.velocity[idx].x;
                    });
                black_box(sum);
            },
            BatchSize::LargeInput,
        );
    });

    group.bench_function("cold_cache_iteration_100k", |b| {
        b.iter_batched(
            || {
                let mut world = World::default();
                world.spawn_batch(POSITION | VELOCITY, 100000, |table, idx| {
                    table.position[idx] = Position {
                        x: idx as f32,
                        y: 0.0,
                        z: 0.0,
                    };
                    table.velocity[idx] = Velocity {
                        x: 1.0,
                        y: 1.0,
                        z: 1.0,
                    };
                });
                world
            },
            |world| {
                let mut sum = 0.0;
                world
                    .query()
                    .with(POSITION | VELOCITY)
                    .iter(|_entity, table, idx| {
                        sum += table.position[idx].x + table.velocity[idx].x;
                    });
                black_box(sum);
            },
            BatchSize::LargeInput,
        );
    });

    group.bench_function("cold_cache_random_lookup_10k", |b| {
        b.iter_batched(
            || {
                let mut world = World::default();
                let entities = world.spawn_batch(POSITION | VELOCITY, 10000, |table, idx| {
                    table.position[idx] = Position {
                        x: idx as f32,
                        y: 0.0,
                        z: 0.0,
                    };
                    table.velocity[idx] = Velocity {
                        x: 1.0,
                        y: 1.0,
                        z: 1.0,
                    };
                });
                let mut indices: Vec<usize> = (0..10000).collect();
                let mut rng = rand::rng();
                indices.shuffle(&mut rng);
                (world, entities, indices)
            },
            |(world, entities, indices)| {
                let mut sum = 0.0;
                for &idx in &indices {
                    if let Some(pos) = world.get_position(entities[idx]) {
                        sum += pos.x;
                    }
                }
                black_box(sum);
            },
            BatchSize::LargeInput,
        );
    });

    group.finish();
}

fn bench_events(c: &mut Criterion) {
    let mut group = c.benchmark_group("events");

    for count in [1000, 10000, 100000] {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(
            BenchmarkId::new("send_events", count),
            &count,
            |b, &count| {
                b.iter(|| {
                    let mut world = World::default();
                    let entities = world.spawn_batch(HEALTH, count, |_table, _idx| {});

                    for &entity in &entities {
                        world.send_damage_event(DamageEvent {
                            entity,
                            amount: 10.0,
                        });
                    }
                    black_box(world);
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("collect_events", count),
            &count,
            |b, &count| {
                let mut world = World::default();
                let entities = world.spawn_batch(HEALTH, count, |_table, _idx| {});

                for &entity in &entities {
                    world.send_damage_event(DamageEvent {
                        entity,
                        amount: 10.0,
                    });
                }

                b.iter(|| {
                    let events = world.collect_damage_event();
                    black_box(events.len());
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("read_events", count),
            &count,
            |b, &count| {
                let mut world = World::default();
                let entities = world.spawn_batch(HEALTH, count, |_table, _idx| {});

                for &entity in &entities {
                    world.send_damage_event(DamageEvent {
                        entity,
                        amount: 10.0,
                    });
                }

                b.iter(|| {
                    let mut event_count = 0;
                    for _event in world.read_damage_event() {
                        event_count += 1;
                    }
                    black_box(event_count);
                });
            },
        );
    }

    group.bench_function("peek_event", |b| {
        let mut world = World::default();
        let entity = world.spawn_entities(HEALTH, 1)[0];
        world.send_damage_event(DamageEvent {
            entity,
            amount: 10.0,
        });

        b.iter(|| {
            let event = world.peek_damage_event();
            black_box(event);
        });
    });

    group.bench_function("drain_events", |b| {
        b.iter(|| {
            let mut world = World::default();
            let entities = world.spawn_batch(HEALTH, 10000, |_table, _idx| {});

            for &entity in &entities {
                world.send_damage_event(DamageEvent {
                    entity,
                    amount: 10.0,
                });
            }

            let events = world.drain_damage_event();
            black_box(events.count());
        });
    });

    group.bench_function("event_step_cleanup", |b| {
        let mut world = World::default();
        let entities = world.spawn_batch(HEALTH, 1000, |_table, _idx| {});

        for &entity in &entities {
            world.send_damage_event(DamageEvent {
                entity,
                amount: 10.0,
            });
        }

        b.iter(|| {
            world.step();
        });
    });

    group.bench_function("event_chain_realistic", |b| {
        b.iter(|| {
            let mut world = World::default();
            let entities = world.spawn_batch(HEALTH, 1000, |table, idx| {
                table.health[idx] = Health {
                    current: 100.0,
                    max: 100.0,
                };
            });

            for &entity in &entities[..500] {
                world.send_collision_event(CollisionEvent {
                    entity_a: entity,
                    entity_b: entities[500],
                });
            }

            for event in world.collect_collision_event() {
                world.send_damage_event(DamageEvent {
                    entity: event.entity_a,
                    amount: 10.0,
                });
            }

            for event in world.collect_damage_event() {
                if let Some(health) = world.get_health_mut(event.entity) {
                    health.current -= event.amount;
                    if health.current <= 0.0 {
                        world.send_heal_event(HealEvent {
                            entity: event.entity,
                            amount: 50.0,
                        });
                    }
                }
            }

            world.step();
            black_box(world);
        });
    });

    group.finish();
}

fn bench_scheduling(c: &mut Criterion) {
    let mut group = c.benchmark_group("scheduling");

    group.bench_function("schedule_creation", |b| {
        b.iter(|| {
            let schedule = freecs::Schedule::<World>::new();
            black_box(schedule);
        });
    });

    for count in [1, 10, 50, 100] {
        group.bench_with_input(
            BenchmarkId::new("add_systems", count),
            &count,
            |b, &count| {
                b.iter(|| {
                    let mut schedule = freecs::Schedule::<World>::new();
                    for _ in 0..count {
                        schedule.add_system(|world: &mut World| {
                            world.resources.frame_count += 1;
                        });
                    }
                    black_box(schedule);
                });
            },
        );
    }

    group.bench_function("run_schedule_vs_manual_10_systems", |b| {
        let mut world = World::default();
        world.spawn_batch(POSITION | VELOCITY, 1000, |_table, _idx| {});

        let mut schedule = freecs::Schedule::new();
        for _ in 0..10 {
            schedule.add_system(|world: &mut World| {
                world.resources.frame_count += 1;
            });
        }

        b.iter(|| {
            schedule.run(&mut world);
        });
    });

    group.bench_function("schedule_multi_frame_simulation", |b| {
        b.iter(|| {
            let mut world = World::default();
            world.spawn_batch(POSITION | VELOCITY | HEALTH, 1000, |table, idx| {
                table.position[idx] = Position {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                };
                table.velocity[idx] = Velocity {
                    x: 1.0,
                    y: 1.0,
                    z: 1.0,
                };
                table.health[idx] = Health {
                    current: 100.0,
                    max: 100.0,
                };
            });

            let mut schedule = freecs::Schedule::new();
            schedule.add_system(|world: &mut World| {
                world.resources.delta_time = 0.016;
            });

            schedule.add_system(|world: &mut World| {
                let dt = world.resources.delta_time;
                world
                    .query_mut()
                    .with(POSITION | VELOCITY)
                    .iter(|_entity, table, idx| {
                        table.position[idx].x += table.velocity[idx].x * dt;
                        table.position[idx].y += table.velocity[idx].y * dt;
                    });
            });

            schedule.add_system(|world: &mut World| {
                world.resources.frame_count += 1;
            });

            for _ in 0..60 {
                schedule.run(&mut world);
                world.step();
            }

            black_box(world);
        });
    });

    group.finish();
}

fn bench_entity_builder(c: &mut Criterion) {
    let mut group = c.benchmark_group("entity_builder");

    for count in [100, 1000, 10000] {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(
            BenchmarkId::new("builder_vs_manual_1_component", count),
            &count,
            |b, &count| {
                b.iter(|| {
                    let mut world = World::default();
                    let _entities = EntityBuilder::new()
                        .with_position(Position {
                            x: 1.0,
                            y: 2.0,
                            z: 3.0,
                        })
                        .spawn(&mut world, count);
                    black_box(world);
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("manual_spawn_set_1_component", count),
            &count,
            |b, &count| {
                b.iter(|| {
                    let mut world = World::default();
                    let entities = world.spawn_entities(POSITION, count);
                    for entity in entities {
                        world.set_position(
                            entity,
                            Position {
                                x: 1.0,
                                y: 2.0,
                                z: 3.0,
                            },
                        );
                    }
                    black_box(world);
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("builder_3_components", count),
            &count,
            |b, &count| {
                b.iter(|| {
                    let mut world = World::default();
                    let _entities = EntityBuilder::new()
                        .with_position(Position {
                            x: 1.0,
                            y: 2.0,
                            z: 3.0,
                        })
                        .with_velocity(Velocity {
                            x: 0.1,
                            y: 0.2,
                            z: 0.3,
                        })
                        .with_health(Health {
                            current: 100.0,
                            max: 100.0,
                        })
                        .spawn(&mut world, count);
                    black_box(world);
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("builder_5_components", count),
            &count,
            |b, &count| {
                b.iter(|| {
                    let mut world = World::default();
                    let _entities = EntityBuilder::new()
                        .with_position(Position {
                            x: 1.0,
                            y: 2.0,
                            z: 3.0,
                        })
                        .with_velocity(Velocity {
                            x: 0.1,
                            y: 0.2,
                            z: 0.3,
                        })
                        .with_health(Health {
                            current: 100.0,
                            max: 100.0,
                        })
                        .with_damage(Damage { value: 10.0 })
                        .with_sprite(Sprite {
                            texture_id: 1,
                            layer: 0,
                        })
                        .spawn(&mut world, count);
                    black_box(world);
                });
            },
        );
    }

    group.finish();
}

fn bench_single_component_apis(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_component_apis");

    let entity_count = 10000;
    let mut world = World::default();
    world.spawn_batch(POSITION | VELOCITY, entity_count, |table, idx| {
        table.position[idx] = Position {
            x: idx as f32,
            y: 0.0,
            z: 0.0,
        };
        table.velocity[idx] = Velocity {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        };
    });

    group.throughput(Throughput::Elements(entity_count as u64));

    group.bench_function("iter_component_vs_query", |b| {
        b.iter(|| {
            let mut sum = 0.0;
            world.iter_position(|_entity, pos| {
                sum += pos.x;
            });
            black_box(sum);
        });
    });

    group.bench_function("query_single_component", |b| {
        b.iter(|| {
            let mut sum = 0.0;
            world.query().with(POSITION).iter(|_entity, table, idx| {
                sum += table.position[idx].x;
            });
            black_box(sum);
        });
    });

    group.bench_function("for_each_component_mut", |b| {
        b.iter(|| {
            world.for_each_position_mut(|pos| {
                pos.x += 1.0;
            });
        });
    });

    group.bench_function("par_for_each_component_mut", |b| {
        b.iter(|| {
            world.par_for_each_position_mut(|pos| {
                pos.x += 1.0;
            });
        });
    });

    group.bench_function("par_for_each_mut_general", |b| {
        b.iter(|| {
            world.par_for_each_mut(POSITION, 0, |_entity, table, idx| {
                table.position[idx].x += 1.0;
            });
        });
    });

    group.finish();
}

fn bench_query_optimizations(c: &mut Criterion) {
    let mut group = c.benchmark_group("query_optimizations");

    let entity_count = 10000;
    let mut world = World::default();
    world.spawn_batch(POSITION | VELOCITY, entity_count, |table, idx| {
        table.position[idx] = Position {
            x: idx as f32,
            y: 0.0,
            z: 0.0,
        };
        table.velocity[idx] = Velocity {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        };
    });

    group.throughput(Throughput::Elements(entity_count as u64));

    group.bench_function("query_first_entity_early_exit", |b| {
        b.iter(|| {
            let entity = world.query_first_entity(POSITION | VELOCITY);
            black_box(entity);
        });
    });

    group.bench_function("query_entities_full_scan", |b| {
        b.iter(|| {
            let entities: Vec<_> = world
                .query_entities(POSITION | VELOCITY)
                .into_iter()
                .collect();
            black_box(entities[0]);
        });
    });

    group.bench_function("get_all_entities", |b| {
        b.iter(|| {
            let entities = world.get_all_entities();
            black_box(entities.len());
        });
    });

    group.bench_function("component_mask_lookup", |b| {
        let entity = world.query_first_entity(POSITION).unwrap();
        b.iter(|| {
            let mask = world.component_mask(entity);
            black_box(mask);
        });
    });

    group.finish();
}

fn bench_direct_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("direct_operations");

    for count in [100, 1000, 5000] {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(
            BenchmarkId::new("despawn_direct", count),
            &count,
            |b, &count| {
                b.iter_batched(
                    || {
                        let mut world = World::default();
                        let entities = world.spawn_batch(POSITION, count, |_table, _idx| {});
                        (world, entities)
                    },
                    |(mut world, entities)| {
                        world.despawn_entities(&entities);
                        black_box(world);
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("despawn_queued", count),
            &count,
            |b, &count| {
                b.iter_batched(
                    || {
                        let mut world = World::default();
                        let entities = world.spawn_batch(POSITION, count, |_table, _idx| {});
                        (world, entities)
                    },
                    |(mut world, entities)| {
                        for &entity in &entities {
                            world.queue_despawn_entity(entity);
                        }
                        world.apply_commands();
                        black_box(world);
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("add_components_direct", count),
            &count,
            |b, &count| {
                b.iter_batched(
                    || {
                        let mut world = World::default();
                        let entities = world.spawn_batch(POSITION, count, |_table, _idx| {});
                        (world, entities)
                    },
                    |(mut world, entities)| {
                        for &entity in &entities {
                            world.add_components(entity, VELOCITY);
                        }
                        black_box(world);
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("add_components_queued", count),
            &count,
            |b, &count| {
                b.iter_batched(
                    || {
                        let mut world = World::default();
                        let entities = world.spawn_batch(POSITION, count, |_table, _idx| {});
                        (world, entities)
                    },
                    |(mut world, entities)| {
                        for &entity in &entities {
                            world.queue_add_components(entity, VELOCITY);
                        }
                        world.apply_commands();
                        black_box(world);
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("set_component_direct", count),
            &count,
            |b, &count| {
                b.iter_batched(
                    || {
                        let mut world = World::default();
                        let entities = world.spawn_batch(POSITION, count, |_table, _idx| {});
                        (world, entities)
                    },
                    |(mut world, entities)| {
                        for &entity in &entities {
                            world.set_position(
                                entity,
                                Position {
                                    x: 1.0,
                                    y: 2.0,
                                    z: 3.0,
                                },
                            );
                        }
                        black_box(world);
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("set_component_queued", count),
            &count,
            |b, &count| {
                b.iter_batched(
                    || {
                        let mut world = World::default();
                        let entities = world.spawn_batch(POSITION, count, |_table, _idx| {});
                        (world, entities)
                    },
                    |(mut world, entities)| {
                        for &entity in &entities {
                            world.queue_set_position(
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
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

fn bench_resources(c: &mut Criterion) {
    let mut group = c.benchmark_group("resources");

    group.bench_function("resource_read", |b| {
        let world = World::default();
        b.iter(|| {
            let value = world.resources.delta_time;
            black_box(value);
        });
    });

    group.bench_function("resource_write", |b| {
        let mut world = World::default();
        b.iter(|| {
            world.resources.delta_time = 0.016;
        });
    });

    group.bench_function("resource_in_system", |b| {
        let mut world = World::default();
        world.spawn_batch(POSITION | VELOCITY, 1000, |table, idx| {
            table.position[idx] = Position {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            };
            table.velocity[idx] = Velocity {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            };
        });

        world.resources.delta_time = 0.016;

        b.iter(|| {
            let dt = world.resources.delta_time;
            world
                .query_mut()
                .with(POSITION | VELOCITY)
                .iter(|_entity, table, idx| {
                    table.position[idx].x += table.velocity[idx].x * dt;
                    table.position[idx].y += table.velocity[idx].y * dt;
                });
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_spawn_entities,
    bench_iteration,
    bench_complex_queries,
    bench_change_detection,
    bench_command_buffers,
    bench_sparse_sets,
    bench_fragmentation_resistance,
    bench_realistic_game_loop,
    bench_parallel_iteration,
    bench_entity_lookups,
    bench_archetype_migrations,
    bench_large_scale,
    bench_multiple_archetypes,
    bench_50_plus_archetypes,
    bench_cold_cache,
    bench_events,
    bench_scheduling,
    bench_entity_builder,
    bench_single_component_apis,
    bench_query_optimizations,
    bench_direct_operations,
    bench_resources,
);
criterion_main!(benches);
