use freecs::{ecs, Entity, Schedule};
use macroquad::prelude::*;
use std::collections::HashMap;

ecs! {
    GameWorld {
        position: Position => POSITION,
        velocity: Velocity => VELOCITY,
        tower: Tower => TOWER,
        enemy: Enemy => ENEMY,
        projectile: Projectile => PROJECTILE,
        grid_cell: GridCell => GRID_CELL,
        grid_position: GridPosition => GRID_POSITION,
        health_bar: HealthBar => HEALTH_BAR,
        visual_effect: VisualEffect => VISUAL_EFFECT,
        range_indicator: RangeIndicator => RANGE_INDICATOR,
        money_popup: MoneyPopup => MONEY_POPUP,
    }
    Tags {
        basic_enemy => BASIC_ENEMY,
        tank_enemy => TANK_ENEMY,
        fast_enemy => FAST_ENEMY,
        flying_enemy => FLYING_ENEMY,
        healer_enemy => HEALER_ENEMY,
        basic_tower => BASIC_TOWER,
        frost_tower => FROST_TOWER,
        cannon_tower => CANNON_TOWER,
        sniper_tower => SNIPER_TOWER,
        poison_tower => POISON_TOWER,
        path_cell => PATH_CELL,
    }
    Events {
        enemy_spawned: EnemySpawnedEvent,
        enemy_died: EnemyDiedEvent,
        enemy_reached_end: EnemyReachedEndEvent,
        projectile_hit: ProjectileHitEvent,
        tower_placed: TowerPlacedEvent,
        tower_sold: TowerSoldEvent,
        wave_completed: WaveCompletedEvent,
        wave_started: WaveStartedEvent,
    }
    GameResources {
        money: u32,
        lives: u32,
        wave: u32,
        game_state: GameState,
        selected_tower_type: TowerType,
        spawn_timer: f32,
        enemies_to_spawn: Vec<EnemySpawnInfo>,
        mouse_grid_pos: Option<(i32, i32)>,
        path: Vec<Vec2>,
        wave_announce_timer: f32,
        game_speed: f32,
        current_hp: u32,
        max_hp: u32,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TowerType {
    #[default]
    Basic,
    Frost,
    Cannon,
    Sniper,
    Poison,
}

impl TowerType {
    fn cost(&self) -> u32 {
        match self {
            TowerType::Basic => 60,
            TowerType::Frost => 120,
            TowerType::Cannon => 200,
            TowerType::Sniper => 180,
            TowerType::Poison => 150,
        }
    }

    fn damage(&self) -> f32 {
        match self {
            TowerType::Basic => 15.0,
            TowerType::Frost => 8.0,
            TowerType::Cannon => 50.0,
            TowerType::Sniper => 80.0,
            TowerType::Poison => 5.0,
        }
    }

    fn range(&self) -> f32 {
        match self {
            TowerType::Basic => 100.0,
            TowerType::Frost => 80.0,
            TowerType::Cannon => 120.0,
            TowerType::Sniper => 180.0,
            TowerType::Poison => 90.0,
        }
    }

    fn fire_rate(&self) -> f32 {
        match self {
            TowerType::Basic => 0.5,
            TowerType::Frost => 1.0,
            TowerType::Cannon => 2.0,
            TowerType::Sniper => 3.0,
            TowerType::Poison => 0.8,
        }
    }

    fn color(&self) -> Color {
        match self {
            TowerType::Basic => GREEN,
            TowerType::Frost => Color::new(0.2, 0.6, 1.0, 1.0),
            TowerType::Cannon => RED,
            TowerType::Sniper => DARKGRAY,
            TowerType::Poison => Color::new(0.6, 0.2, 0.8, 1.0),
        }
    }

    fn projectile_speed(&self) -> f32 {
        match self {
            TowerType::Basic => 300.0,
            TowerType::Frost => 200.0,
            TowerType::Cannon => 250.0,
            TowerType::Sniper => 500.0,
            TowerType::Poison => 250.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum GameState {
    #[default]
    WaitingForWave,
    WaveInProgress,
    GameOver,
    Victory,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EnemyType {
    Normal,
    Fast,
    Tank,
    Flying,
    Shielded,
    Healer,
    Boss,
}

impl EnemyType {
    fn base_health(&self) -> f32 {
        match self {
            EnemyType::Normal => 50.0,
            EnemyType::Fast => 30.0,
            EnemyType::Tank => 150.0,
            EnemyType::Flying => 40.0,
            EnemyType::Shielded => 80.0,
            EnemyType::Healer => 60.0,
            EnemyType::Boss => 500.0,
        }
    }

    fn health(&self, wave: u32) -> f32 {
        let health_multiplier = 1.0 + (wave as f32 - 1.0) * 0.5;
        self.base_health() * health_multiplier
    }

    fn speed(&self) -> f32 {
        match self {
            EnemyType::Normal => 40.0,
            EnemyType::Fast => 80.0,
            EnemyType::Tank => 20.0,
            EnemyType::Flying => 60.0,
            EnemyType::Shielded => 30.0,
            EnemyType::Healer => 35.0,
            EnemyType::Boss => 15.0,
        }
    }

    fn value(&self, wave: u32) -> u32 {
        let base = match self {
            EnemyType::Normal => 10,
            EnemyType::Fast => 15,
            EnemyType::Tank => 30,
            EnemyType::Flying => 20,
            EnemyType::Shielded => 25,
            EnemyType::Healer => 40,
            EnemyType::Boss => 100,
        };
        base + wave * 2
    }

    fn shield(&self) -> f32 {
        match self {
            EnemyType::Shielded => 50.0,
            EnemyType::Boss => 100.0,
            _ => 0.0,
        }
    }

    fn color(&self) -> Color {
        match self {
            EnemyType::Normal => RED,
            EnemyType::Fast => ORANGE,
            EnemyType::Tank => DARKGRAY,
            EnemyType::Flying => SKYBLUE,
            EnemyType::Shielded => Color::new(0.5, 0.0, 0.8, 1.0),
            EnemyType::Healer => Color::new(0.2, 0.8, 0.3, 1.0),
            EnemyType::Boss => Color::new(0.6, 0.0, 0.6, 1.0),
        }
    }

    fn size(&self) -> f32 {
        match self {
            EnemyType::Normal => 15.0,
            EnemyType::Fast => 12.0,
            EnemyType::Tank => 20.0,
            EnemyType::Flying => 15.0,
            EnemyType::Shielded => 18.0,
            EnemyType::Healer => 16.0,
            EnemyType::Boss => 30.0,
        }
    }
}

impl Default for EnemyType {
    fn default() -> Self {
        EnemyType::Normal
    }
}

#[derive(Debug, Clone)]
pub struct EnemySpawnInfo {
    enemy_type: EnemyType,
    spawn_time: f32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Position(pub Vec2);

#[derive(Debug, Clone, Copy, Default)]
pub struct Velocity(pub Vec2);

#[derive(Debug, Clone, Copy, Default)]
pub struct Tower {
    pub tower_type: TowerType,
    pub cooldown: f32,
    pub target: Option<freecs::Entity>,
    pub fire_animation: f32,
    pub tracking_time: f32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Enemy {
    pub health: f32,
    pub max_health: f32,
    pub shield_health: f32,
    pub max_shield: f32,
    pub speed: f32,
    pub path_index: usize,
    pub path_progress: f32,
    pub value: u32,
    pub enemy_type: EnemyType,
    pub slow_duration: f32,
    pub poison_duration: f32,
    pub poison_damage: f32,
    pub is_flying: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Projectile {
    pub damage: f32,
    pub target: freecs::Entity,
    pub speed: f32,
    pub tower_type: TowerType,
    pub start_position: Vec2,
    pub arc_height: f32,
    pub flight_progress: f32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct GridCell {
    pub x: i32,
    pub y: i32,
    pub occupied: bool,
    pub is_path: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct GridPosition {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HealthBar {
    pub enemy_entity: freecs::Entity,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EffectType {
    Explosion,
    PoisonBubble,
    DeathParticle,
}

impl Default for EffectType {
    fn default() -> Self {
        EffectType::Explosion
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct VisualEffect {
    pub effect_type: EffectType,
    pub lifetime: f32,
    pub age: f32,
    pub velocity: Vec2,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RangeIndicator {
    pub tower_entity: freecs::Entity,
    pub visible: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MoneyPopup {
    pub lifetime: f32,
    pub amount: i32,
}

#[derive(Debug, Clone)]
pub struct EnemySpawnedEvent {
    pub entity: Entity,
    pub enemy_type: EnemyType,
}

#[derive(Debug, Clone)]
pub struct EnemyDiedEvent {
    pub entity: Entity,
    pub position: Vec2,
    pub reward: u32,
    pub enemy_type: EnemyType,
}

#[derive(Debug, Clone)]
pub struct EnemyReachedEndEvent {
    pub entity: Entity,
    pub damage: u32,
}

#[derive(Debug, Clone)]
pub struct ProjectileHitEvent {
    pub projectile: Entity,
    pub target: Entity,
    pub position: Vec2,
    pub damage: f32,
    pub tower_type: TowerType,
}

#[derive(Debug, Clone)]
pub struct TowerPlacedEvent {
    pub entity: Entity,
    pub tower_type: TowerType,
    pub grid_x: i32,
    pub grid_y: i32,
    pub cost: u32,
}

#[derive(Debug, Clone)]
pub struct TowerSoldEvent {
    pub entity: Entity,
    pub tower_type: TowerType,
    pub grid_x: i32,
    pub grid_y: i32,
    pub refund: u32,
}

#[derive(Debug, Clone)]
pub struct WaveCompletedEvent {
    pub wave: u32,
}

#[derive(Debug, Clone)]
pub struct WaveStartedEvent {
    pub wave: u32,
    pub enemy_count: usize,
}

const GRID_SIZE: i32 = 12;
const TILE_SIZE: f32 = 40.0;
const BASE_WIDTH: f32 = 1024.0;
const BASE_HEIGHT: f32 = 768.0;

fn get_scale() -> f32 {
    (screen_width() / BASE_WIDTH).min(screen_height() / BASE_HEIGHT)
}

fn get_offset() -> Vec2 {
    let scale = get_scale();
    let scaled_width = BASE_WIDTH * scale;
    let scaled_height = BASE_HEIGHT * scale;
    Vec2::new(
        (screen_width() - scaled_width) / 2.0,
        (screen_height() - scaled_height) / 2.0,
    )
}

fn grid_to_base(grid_x: i32, grid_y: i32) -> Vec2 {
    let num_cells = (GRID_SIZE + 1) as f32;
    let grid_width = num_cells * TILE_SIZE;
    let grid_height = num_cells * TILE_SIZE;
    let grid_offset_x = (BASE_WIDTH - grid_width) / 2.0;
    let grid_offset_y = (BASE_HEIGHT - grid_height) / 2.0;

    let tile_x = (grid_x + GRID_SIZE / 2) as f32;
    let tile_y = (grid_y + GRID_SIZE / 2) as f32;

    Vec2::new(
        grid_offset_x + (tile_x + 0.5) * TILE_SIZE,
        grid_offset_y + (tile_y + 0.5) * TILE_SIZE,
    )
}

fn grid_to_screen(grid_x: i32, grid_y: i32) -> Vec2 {
    let base_pos = grid_to_base(grid_x, grid_y);
    let scale = get_scale();
    let offset = get_offset();
    Vec2::new(
        offset.x + base_pos.x * scale,
        offset.y + base_pos.y * scale,
    )
}

fn screen_to_grid(screen_pos: Vec2) -> Option<(i32, i32)> {
    let scale = get_scale();
    let offset = get_offset();

    let num_cells = (GRID_SIZE + 1) as f32;
    let grid_width = num_cells * TILE_SIZE;
    let grid_height = num_cells * TILE_SIZE;
    let grid_offset_x = (BASE_WIDTH - grid_width) / 2.0;
    let grid_offset_y = (BASE_HEIGHT - grid_height) / 2.0;

    let local_x = (screen_pos.x - offset.x) / scale;
    let local_y = (screen_pos.y - offset.y) / scale;

    let rel_x = local_x - grid_offset_x;
    let rel_y = local_y - grid_offset_y;

    if rel_x < 0.0 || rel_y < 0.0 || rel_x >= grid_width || rel_y >= grid_height {
        return None;
    }

    let tile_x = (rel_x / TILE_SIZE).floor() as i32;
    let tile_y = (rel_y / TILE_SIZE).floor() as i32;

    let grid_x = tile_x - GRID_SIZE / 2;
    let grid_y = tile_y - GRID_SIZE / 2;

    Some((grid_x, grid_y))
}

fn initialize_grid(world: &mut GameWorld) {
    for x in -GRID_SIZE / 2..=GRID_SIZE / 2 {
        for y in -GRID_SIZE / 2..=GRID_SIZE / 2 {
            let entity = world.spawn_entities(GRID_CELL, 1)[0];
            world.set_grid_cell(
                entity,
                GridCell {
                    x,
                    y,
                    occupied: false,
                    is_path: false,
                },
            );
        }
    }
}

fn create_path(world: &mut GameWorld) {
    let path = vec![
        Vec2::new(-6.0, 0.0),
        Vec2::new(-3.0, 0.0),
        Vec2::new(-3.0, -4.0),
        Vec2::new(3.0, -4.0),
        Vec2::new(3.0, 2.0),
        Vec2::new(-1.0, 2.0),
        Vec2::new(-1.0, 5.0),
        Vec2::new(6.0, 5.0),
    ];

    let num_cells = (GRID_SIZE + 1) as f32;
    let grid_width = num_cells * TILE_SIZE;
    let grid_height = num_cells * TILE_SIZE;
    let grid_offset_x = (BASE_WIDTH - grid_width) / 2.0;
    let grid_offset_y = (BASE_HEIGHT - grid_height) / 2.0;

    let screen_path: Vec<Vec2> = path.iter().map(|&p| {
        Vec2::new(
            grid_offset_x + (p.x + GRID_SIZE as f32 / 2.0 + 0.5) * TILE_SIZE,
            grid_offset_y + (p.y + GRID_SIZE as f32 / 2.0 + 0.5) * TILE_SIZE,
        )
    }).collect();

    world.resources.path = screen_path;

    let mut cells_to_mark = Vec::new();

    for index in 0..path.len() - 1 {
        let start = path[index];
        let end = path[index + 1];
        let steps = 20;

        for step in 0..=steps {
            let t = step as f32 / steps as f32;
            let pos = start + (end - start) * t;
            let grid_x = pos.x.round() as i32;
            let grid_y = pos.y.round() as i32;
            cells_to_mark.push((grid_x, grid_y));
        }
    }

    let grid_entities: Vec<_> = world.query_entities(GRID_CELL).collect();
    for entity in grid_entities {
        if let Some(cell) = world.get_grid_cell_mut(entity) {
            for &(grid_x, grid_y) in &cells_to_mark {
                if cell.x == grid_x && cell.y == grid_y {
                    cell.is_path = true;
                    cell.occupied = true;
                    world.add_path_cell(entity);
                    break;
                }
            }
        }
    }
}

fn spawn_tower(
    world: &mut GameWorld,
    grid_x: i32,
    grid_y: i32,
    tower_type: TowerType,
) -> freecs::Entity {
    let position = grid_to_base(grid_x, grid_y);

    let entities = EntityBuilder::new()
        .with_position(Position(position))
        .with_grid_position(GridPosition { x: grid_x, y: grid_y })
        .with_tower(Tower {
            tower_type,
            cooldown: 0.0,
            target: None,
            fire_animation: 0.0,
            tracking_time: 0.0,
        })
        .spawn(world, 1);

    let entity = entities[0];

    match tower_type {
        TowerType::Basic => world.add_basic_tower(entity),
        TowerType::Frost => world.add_frost_tower(entity),
        TowerType::Cannon => world.add_cannon_tower(entity),
        TowerType::Sniper => world.add_sniper_tower(entity),
        TowerType::Poison => world.add_poison_tower(entity),
    }

    let cost = tower_type.cost();
    world.resources.money -= cost;

    world.send_tower_placed(TowerPlacedEvent {
        entity,
        tower_type,
        grid_x,
        grid_y,
        cost,
    });

    spawn_range_indicator(world, entity);

    entity
}

fn spawn_range_indicator(world: &mut GameWorld, tower_entity: freecs::Entity) {
    let entity = world.spawn_entities(RANGE_INDICATOR, 1)[0];
    world.set_range_indicator(
        entity,
        RangeIndicator {
            tower_entity,
            visible: false,
        },
    );
}

fn spawn_enemy(world: &mut GameWorld, enemy_type: EnemyType) -> freecs::Entity {
    let start_pos = world.resources.path[0];
    let health = enemy_type.health(world.resources.wave);
    let shield = enemy_type.shield();

    let entities = EntityBuilder::new()
        .with_position(Position(start_pos))
        .with_velocity(Velocity(Vec2::ZERO))
        .with_enemy(Enemy {
            health,
            max_health: health,
            shield_health: shield,
            max_shield: shield,
            speed: enemy_type.speed(),
            path_index: 0,
            path_progress: 0.0,
            value: enemy_type.value(world.resources.wave),
            enemy_type,
            slow_duration: 0.0,
            poison_duration: 0.0,
            poison_damage: 0.0,
            is_flying: enemy_type == EnemyType::Flying,
        })
        .spawn(world, 1);

    let entity = entities[0];
    world.set_health_bar(entity, HealthBar { enemy_entity: entity });

    match enemy_type {
        EnemyType::Normal => world.add_basic_enemy(entity),
        EnemyType::Tank => world.add_tank_enemy(entity),
        EnemyType::Fast => world.add_fast_enemy(entity),
        EnemyType::Flying => world.add_flying_enemy(entity),
        EnemyType::Healer => world.add_healer_enemy(entity),
        _ => world.add_basic_enemy(entity),
    }

    world.send_enemy_spawned(EnemySpawnedEvent { entity, enemy_type });

    entity
}

fn spawn_projectile(
    world: &mut GameWorld,
    from: Vec2,
    target: freecs::Entity,
    tower_type: TowerType,
) -> freecs::Entity {
    let arc_height = if tower_type == TowerType::Cannon { 50.0 } else { 0.0 };

    EntityBuilder::new()
        .with_position(Position(from))
        .with_velocity(Velocity(Vec2::ZERO))
        .with_projectile(Projectile {
            damage: tower_type.damage(),
            target,
            speed: tower_type.projectile_speed(),
            tower_type,
            start_position: from,
            arc_height,
            flight_progress: 0.0,
        })
        .spawn(world, 1)[0]
}

fn spawn_visual_effect(world: &mut GameWorld, position: Vec2, effect_type: EffectType, velocity: Vec2, lifetime: f32) {
    EntityBuilder::new()
        .with_position(Position(position))
        .with_visual_effect(VisualEffect {
            effect_type,
            lifetime,
            age: 0.0,
            velocity,
        })
        .spawn(world, 1);
}

fn spawn_money_popup(world: &mut GameWorld, position: Vec2, amount: i32) {
    EntityBuilder::new()
        .with_position(Position(position))
        .with_money_popup(MoneyPopup {
            lifetime: 0.0,
            amount,
        })
        .spawn(world, 1);
}

fn can_place_tower_at(world: &GameWorld, x: i32, y: i32) -> bool {
    let mut has_tower = false;
    world
        .query()
        .with(TOWER | GRID_POSITION)
        .iter(|_entity, table, index| {
            if table.grid_position[index].x == x && table.grid_position[index].y == y {
                has_tower = true;
            }
        });

    if has_tower {
        return false;
    }

    let mut can_place = false;
    world
        .query()
        .with(GRID_CELL)
        .iter(|_entity, table, index| {
            if table.grid_cell[index].x == x && table.grid_cell[index].y == y && !table.grid_cell[index].occupied {
                can_place = true;
            }
        });
    can_place
}

fn mark_cell_occupied(world: &mut GameWorld, x: i32, y: i32) {
    world
        .query_mut()
        .with(GRID_CELL)
        .iter(|_entity, table, index| {
            if table.grid_cell[index].x == x && table.grid_cell[index].y == y {
                table.grid_cell[index].occupied = true;
            }
        });
}

fn plan_wave(world: &mut GameWorld) {
    world.resources.wave += 1;
    let wave = world.resources.wave;
    let mut spawns = Vec::new();

    let enemy_count = 5 + wave * 2;

    let enemy_types = match wave {
        1..=2 => vec![(EnemyType::Normal, 1.0)],
        3..=4 => vec![(EnemyType::Normal, 0.7), (EnemyType::Fast, 0.3)],
        5..=6 => vec![
            (EnemyType::Normal, 0.5),
            (EnemyType::Fast, 0.3),
            (EnemyType::Tank, 0.2),
        ],
        7..=8 => vec![
            (EnemyType::Normal, 0.3),
            (EnemyType::Fast, 0.3),
            (EnemyType::Tank, 0.2),
            (EnemyType::Flying, 0.2),
        ],
        9..=10 => vec![
            (EnemyType::Normal, 0.2),
            (EnemyType::Fast, 0.2),
            (EnemyType::Tank, 0.2),
            (EnemyType::Flying, 0.2),
            (EnemyType::Shielded, 0.2),
        ],
        11..=12 => vec![
            (EnemyType::Fast, 0.2),
            (EnemyType::Tank, 0.2),
            (EnemyType::Flying, 0.2),
            (EnemyType::Shielded, 0.2),
            (EnemyType::Healer, 0.2),
        ],
        13..=14 => vec![
            (EnemyType::Tank, 0.2),
            (EnemyType::Flying, 0.2),
            (EnemyType::Shielded, 0.2),
            (EnemyType::Healer, 0.2),
            (EnemyType::Boss, 0.2),
        ],
        _ => vec![
            (EnemyType::Tank, 0.15),
            (EnemyType::Flying, 0.2),
            (EnemyType::Shielded, 0.2),
            (EnemyType::Healer, 0.2),
            (EnemyType::Boss, 0.25),
        ],
    };

    let spawn_interval = match wave {
        1..=3 => 1.0,
        4..=6 => 0.8,
        7..=9 => 0.6,
        _ => 0.5,
    };

    let mut spawn_time = 0.0;

    for _ in 0..enemy_count {
        let roll: f32 = rand::gen_range(0.0, 1.0);
        let mut cumulative = 0.0;
        let mut selected_type = EnemyType::Normal;

        for (enemy_type, probability) in &enemy_types {
            cumulative += probability;
            if roll < cumulative {
                selected_type = *enemy_type;
                break;
            }
        }

        spawns.push(EnemySpawnInfo {
            enemy_type: selected_type,
            spawn_time,
        });
        spawn_time += spawn_interval;
    }

    world.resources.enemies_to_spawn = spawns.clone();
    world.resources.spawn_timer = 0.0;
    world.resources.game_state = GameState::WaveInProgress;
    world.resources.wave_announce_timer = 3.0;

    world.send_wave_started(WaveStartedEvent {
        wave,
        enemy_count: spawns.len(),
    });
}

fn input_system(world: &mut GameWorld) {
    let mouse_pos = Vec2::new(mouse_position().0, mouse_position().1);
    world.resources.mouse_grid_pos = screen_to_grid(mouse_pos);

    let left_clicked = is_mouse_button_pressed(MouseButton::Left);
    let right_clicked = is_mouse_button_pressed(MouseButton::Right);

    if left_clicked {
        if let Some((grid_x, grid_y)) = world.resources.mouse_grid_pos {
            if can_place_tower_at(world, grid_x, grid_y) {
                let tower_type = world.resources.selected_tower_type;
                if world.resources.money >= tower_type.cost() {
                    let cost = tower_type.cost();
                    spawn_tower(world, grid_x, grid_y, tower_type);
                    mark_cell_occupied(world, grid_x, grid_y);
                    let pos = grid_to_base(grid_x, grid_y);
                    spawn_money_popup(world, pos, -(cost as i32));
                }
            }
        }
    }

    if right_clicked {
        if let Some((grid_x, grid_y)) = world.resources.mouse_grid_pos {
            let mut tower_entity = None;
            world
                .query()
                .with(TOWER | GRID_POSITION)
                .iter(|entity, table, index| {
                    if table.grid_position[index].x == grid_x && table.grid_position[index].y == grid_y {
                        tower_entity = Some(entity);
                    }
                });

            if let Some(tower_entity) = tower_entity {
                sell_tower(world, tower_entity, grid_x, grid_y);
            }
        }
    }

    if is_key_pressed(KeyCode::Key1) {
        world.resources.selected_tower_type = TowerType::Basic;
    } else if is_key_pressed(KeyCode::Key2) {
        world.resources.selected_tower_type = TowerType::Frost;
    } else if is_key_pressed(KeyCode::Key3) {
        world.resources.selected_tower_type = TowerType::Cannon;
    } else if is_key_pressed(KeyCode::Key4) {
        world.resources.selected_tower_type = TowerType::Sniper;
    } else if is_key_pressed(KeyCode::Key5) {
        world.resources.selected_tower_type = TowerType::Poison;
    }

    if is_key_pressed(KeyCode::LeftBracket) {
        world.resources.game_speed = (world.resources.game_speed - 0.5).max(0.5);
    } else if is_key_pressed(KeyCode::RightBracket) {
        world.resources.game_speed = (world.resources.game_speed + 0.5).min(3.0);
    } else if is_key_pressed(KeyCode::Backslash) {
        world.resources.game_speed = 1.0;
    }

    if is_key_pressed(KeyCode::P) {
        match world.resources.game_state {
            GameState::WaveInProgress => world.resources.game_state = GameState::Paused,
            GameState::Paused => world.resources.game_state = GameState::WaveInProgress,
            _ => {}
        }
    }

    if is_key_pressed(KeyCode::R) {
        if matches!(world.resources.game_state, GameState::GameOver | GameState::Victory) {
            restart_game(world);
        }
    }
}

fn wave_spawning_system(world: &mut GameWorld, delta_time: f32) {
    if world.resources.game_state != GameState::WaveInProgress {
        return;
    }

    world.resources.spawn_timer += delta_time;

    let current_time = world.resources.spawn_timer;
    let mut spawns_to_process = Vec::new();

    for (index, spawn_info) in world.resources.enemies_to_spawn.iter().enumerate() {
        if spawn_info.spawn_time <= current_time {
            spawns_to_process.push((index, spawn_info.enemy_type));
        }
    }

    for (_index, enemy_type) in spawns_to_process.iter() {
        spawn_enemy(world, *enemy_type);
    }

    for &(index, _) in spawns_to_process.iter().rev() {
        world.resources.enemies_to_spawn.remove(index);
    }

    let enemy_count = world.query_entities(ENEMY).count();

    if world.resources.enemies_to_spawn.is_empty() && enemy_count == 0 {
        world.send_wave_completed(WaveCompletedEvent {
            wave: world.resources.wave,
        });

        if world.resources.wave >= 20 {
            world.resources.game_state = GameState::Victory;
        } else {
            plan_wave(world);
        }
    }
}

fn enemy_movement_system(world: &mut GameWorld, delta_time: f32) {
    let path = world.resources.path.clone();
    let mut enemies_to_remove = Vec::new();
    let mut hp_damage = 0;

    let mut enemy_positions = Vec::new();
    world
        .query()
        .with(ENEMY | POSITION)
        .iter(|entity, table, index| {
            enemy_positions.push((entity, table.position[index].0, table.enemy[index].enemy_type));
        });

    for (healer_entity, healer_pos, enemy_type) in &enemy_positions {
        if *enemy_type == EnemyType::Healer {
            for (other_entity, other_pos, _) in &enemy_positions {
                if healer_entity != other_entity {
                    let distance = (*other_pos - *healer_pos).length();
                    if distance < 60.0 {
                        if let Some(enemy) = world.get_enemy_mut(*other_entity) {
                            enemy.health = (enemy.health + 10.0 * delta_time).min(enemy.max_health);
                        }
                    }
                }
            }
        }
    }

    let enemy_entities: Vec<_> = world.query_entities(ENEMY | POSITION).collect();
    for entity in enemy_entities {
        let enemy = world.get_enemy(entity).unwrap();
        let mut path_index = enemy.path_index;
        let mut path_progress = enemy.path_progress;

        let speed_multiplier = if enemy.slow_duration > 0.0 { 0.5 } else { 1.0 };
        let speed = enemy.speed * speed_multiplier;

        path_progress += speed * delta_time;

        if path_index < path.len() - 1 {
            let current = path[path_index];
            let next = path[path_index + 1];
            let segment_length = (next - current).length();

            if path_progress >= segment_length {
                path_progress -= segment_length;
                path_index += 1;

                if path_index >= path.len() - 1 {
                    enemies_to_remove.push(entity);
                    hp_damage += 1;
                    world.send_enemy_reached_end(EnemyReachedEndEvent {
                        entity,
                        damage: 1,
                    });
                    continue;
                }
            }

            let current = path[path_index];
            let next = path[path_index + 1];
            let direction = (next - current).normalize();
            let base_position = current + direction * path_progress;

            let mut poison_death = false;

            if let Some(enemy) = world.get_enemy_mut(entity) {
                enemy.path_index = path_index;
                enemy.path_progress = path_progress;

                if enemy.slow_duration > 0.0 {
                    enemy.slow_duration -= delta_time;
                }

                if enemy.poison_duration > 0.0 {
                    enemy.poison_duration -= delta_time;
                    enemy.health -= enemy.poison_damage * delta_time;

                    if enemy.health <= 0.0 {
                        poison_death = true;
                    }
                }
            }

            if poison_death {
                enemies_to_remove.push(entity);
            } else {
                if let Some(pos) = world.get_position_mut(entity) {
                    pos.0 = base_position;
                }
            }
        }
    }

    if hp_damage > 0 {
        if world.resources.current_hp >= hp_damage {
            world.resources.current_hp -= hp_damage;
        } else {
            world.resources.current_hp = 0;
        }

        if world.resources.current_hp == 0 {
            world.resources.current_hp = world.resources.max_hp;
            world.resources.lives = world.resources.lives.saturating_sub(1);

            if world.resources.lives == 0 {
                world.resources.game_state = GameState::GameOver;
            }
        }
    }

    for entity in enemies_to_remove {
        if let Some(enemy) = world.get_enemy(entity) {
            world.resources.money += enemy.value;
        }
        world.queue_despawn_entity(entity);
    }

    world.apply_commands();
}

fn tower_targeting_system(world: &mut GameWorld) {
    let mut enemy_data = Vec::new();
    world
        .query()
        .with(ENEMY | POSITION)
        .iter(|entity, table, index| {
            enemy_data.push((entity, table.position[index].0, table.enemy[index].is_flying));
        });

    let tower_entities: Vec<_> = world.query_entities(TOWER | POSITION).collect();
    for tower_entity in tower_entities {
        let tower_data = world.get_tower(tower_entity).unwrap();
        let tower_pos = world.get_position(tower_entity).unwrap().0;
        let range = tower_data.tower_type.range();
        let range_squared = range * range;

        let mut closest_enemy = None;
        let mut closest_distance = f32::MAX;

        for &(enemy_entity, enemy_pos, _is_flying) in &enemy_data {
            let distance_squared = (enemy_pos - tower_pos).length_squared();
            if distance_squared <= range_squared && distance_squared < closest_distance {
                closest_distance = distance_squared;
                closest_enemy = Some(enemy_entity);
            }
        }

        if let Some(tower) = world.get_tower_mut(tower_entity) {
            tower.target = closest_enemy;
            if tower.target.is_some() {
                tower.tracking_time += get_frame_time();
            } else {
                tower.tracking_time = 0.0;
            }
        }
    }
}

fn tower_shooting_system(world: &mut GameWorld, delta_time: f32) {
    let mut projectiles_to_spawn = Vec::new();

    let tower_entities: Vec<_> = world.query_entities(TOWER | POSITION).collect();
    for entity in tower_entities {
        let tower_pos = world.get_position(entity).unwrap().0;

        if let Some(tower) = world.get_tower_mut(entity) {
            tower.cooldown -= delta_time;

            if tower.fire_animation > 0.0 {
                tower.fire_animation -= delta_time * 3.0;
            }

            if tower.cooldown <= 0.0 && tower.target.is_some() {
                let can_fire = if tower.tower_type == TowerType::Sniper {
                    tower.tracking_time >= 2.0
                } else {
                    true
                };

                if can_fire {
                    projectiles_to_spawn.push((tower_pos, tower.target.unwrap(), tower.tower_type));
                    tower.cooldown = tower.tower_type.fire_rate();
                    tower.fire_animation = 1.0;
                    tower.tracking_time = 0.0;
                }
            }
        }
    }

    for (from, target, tower_type) in projectiles_to_spawn {
        spawn_projectile(world, from, target, tower_type);

        if tower_type == TowerType::Cannon {
            for _ in 0..6 {
                let offset = Vec2::new(
                    rand::gen_range(-5.0, 5.0),
                    rand::gen_range(-5.0, 5.0),
                );
                spawn_visual_effect(
                    world,
                    from + offset,
                    EffectType::Explosion,
                    Vec2::ZERO,
                    0.3,
                );
            }
        }
    }
}

fn projectile_movement_system(world: &mut GameWorld, delta_time: f32) {
    let mut projectiles_to_remove = Vec::new();
    let mut hits = Vec::new();

    let mut enemy_positions = HashMap::new();
    world
        .query()
        .with(ENEMY | POSITION)
        .iter(|entity, table, index| {
            enemy_positions.insert(entity, table.position[index].0);
        });

    let projectile_entities: Vec<_> = world.query_entities(PROJECTILE | POSITION).collect();
    for projectile_entity in projectile_entities {
        let mut projectile_data = *world.get_projectile(projectile_entity).unwrap();
        let old_pos = world.get_position(projectile_entity).unwrap().0;

        if let Some(&target_pos) = enemy_positions.get(&projectile_data.target) {
            let total_distance = (target_pos - projectile_data.start_position).length();
            let distance_to_target = (target_pos - old_pos).length();

            let new_pos = if projectile_data.arc_height > 0.0 {
                projectile_data.flight_progress += (projectile_data.speed * delta_time) / total_distance;
                projectile_data.flight_progress = projectile_data.flight_progress.min(1.0);

                let horizontal_pos = projectile_data.start_position
                    + (target_pos - projectile_data.start_position) * projectile_data.flight_progress;
                horizontal_pos
            } else {
                let direction = (target_pos - old_pos).normalize();
                old_pos + direction * projectile_data.speed * delta_time
            };

            if distance_to_target < 10.0 || projectile_data.flight_progress >= 1.0 {
                hits.push((projectile_data.target, projectile_data.damage, projectile_data.tower_type, target_pos));
                projectiles_to_remove.push(projectile_entity);
                world.send_projectile_hit(ProjectileHitEvent {
                    projectile: projectile_entity,
                    target: projectile_data.target,
                    position: target_pos,
                    damage: projectile_data.damage,
                    tower_type: projectile_data.tower_type,
                });
            } else {
                if let Some(projectile) = world.get_projectile_mut(projectile_entity) {
                    projectile.flight_progress = projectile_data.flight_progress;
                }

                if let Some(pos) = world.get_position_mut(projectile_entity) {
                    pos.0 = new_pos;
                }
            }
        } else {
            projectiles_to_remove.push(projectile_entity);
        }
    }

    for (enemy_entity, damage, tower_type, hit_pos) in hits {
        match tower_type {
            TowerType::Frost => {
                if let Some(enemy) = world.get_enemy_mut(enemy_entity) {
                    enemy.slow_duration = 2.0;
                }
                apply_damage_to_enemy(world, enemy_entity, damage);
            }
            TowerType::Poison => {
                if let Some(enemy) = world.get_enemy_mut(enemy_entity) {
                    enemy.poison_duration = 3.0;
                    enemy.poison_damage = 5.0;
                }
                apply_damage_to_enemy(world, enemy_entity, damage);

                for _ in 0..3 {
                    let velocity = Vec2::new(
                        rand::gen_range(-20.0, 20.0),
                        rand::gen_range(-20.0, 20.0),
                    );
                    spawn_visual_effect(
                        world,
                        hit_pos,
                        EffectType::PoisonBubble,
                        velocity,
                        2.0,
                    );
                }
            }
            TowerType::Cannon => {
                for _ in 0..8 {
                    let velocity = Vec2::new(
                        rand::gen_range(-30.0, 30.0),
                        rand::gen_range(-30.0, 30.0),
                    );
                    spawn_visual_effect(
                        world,
                        hit_pos,
                        EffectType::Explosion,
                        velocity,
                        0.5,
                    );
                }

                for (&enemy_entity, &enemy_pos) in &enemy_positions {
                    let distance = (enemy_pos - hit_pos).length();
                    if distance < 60.0 {
                        let damage_falloff = 1.0 - (distance / 60.0);
                        apply_damage_to_enemy(world, enemy_entity, damage * damage_falloff);
                    }
                }
            }
            _ => {
                apply_damage_to_enemy(world, enemy_entity, damage);
            }
        }
    }

    for entity in projectiles_to_remove {
        world.queue_despawn_entity(entity);
    }

    world.apply_commands();
}

fn apply_damage_to_enemy(world: &mut GameWorld, enemy_entity: freecs::Entity, damage: f32) {
    let mut should_remove = false;
    let mut death_pos = Vec2::ZERO;
    let mut money_earned = 0;
    let mut enemy_type = EnemyType::Normal;

    if let Some(enemy) = world.get_enemy_mut(enemy_entity) {
        let was_alive = enemy.health > 0.0;

        if enemy.shield_health > 0.0 {
            let shield_damage = damage.min(enemy.shield_health);
            enemy.shield_health -= shield_damage;
            let remaining_damage = damage - shield_damage;
            if remaining_damage > 0.0 {
                enemy.health -= remaining_damage;
            }
        } else {
            enemy.health -= damage;
        }

        if was_alive && enemy.health <= 0.0 {
            money_earned = enemy.value;
            enemy_type = enemy.enemy_type;
            should_remove = true;
        }
    }

    if should_remove {
        if let Some(pos) = world.get_position(enemy_entity) {
            death_pos = pos.0;
        }

        world.send_enemy_died(EnemyDiedEvent {
            entity: enemy_entity,
            position: death_pos,
            reward: money_earned,
            enemy_type,
        });

        world.queue_despawn_entity(enemy_entity);
    }
}

fn visual_effects_system(world: &mut GameWorld, delta_time: f32) {
    let mut effects_to_remove = Vec::new();

    world
        .query_mut()
        .with(VISUAL_EFFECT | POSITION)
        .iter(|entity, table, index| {
            table.visual_effect[index].age += delta_time;

            if table.visual_effect[index].age >= table.visual_effect[index].lifetime {
                effects_to_remove.push(entity);
            } else {
                let velocity = table.visual_effect[index].velocity;
                table.position[index].0 += velocity * delta_time;
            }
        });

    for entity in effects_to_remove {
        world.queue_despawn_entity(entity);
    }

    world.apply_commands();
}

fn update_money_popups(world: &mut GameWorld, delta_time: f32) {
    let mut popups_to_remove = Vec::new();

    world
        .query_mut()
        .with(MONEY_POPUP | POSITION)
        .iter(|entity, table, index| {
            table.money_popup[index].lifetime += delta_time;

            if table.money_popup[index].lifetime > 2.0 {
                popups_to_remove.push(entity);
            } else {
                table.position[index].0.y -= delta_time * 30.0;
            }
        });

    for entity in popups_to_remove {
        world.queue_despawn_entity(entity);
    }

    world.apply_commands();
}


fn sell_tower(world: &mut GameWorld, tower_entity: freecs::Entity, grid_x: i32, grid_y: i32) {
    if let Some(tower) = world.get_tower(tower_entity) {
        let tower_type = tower.tower_type;
        let refund = (tower.tower_type.cost() as f32 * 0.7) as u32;
        world.resources.money += refund;

        let position = grid_to_base(grid_x, grid_y);
        spawn_money_popup(world, position, refund as i32);

        world.send_tower_sold(TowerSoldEvent {
            entity: tower_entity,
            tower_type,
            grid_x,
            grid_y,
            refund,
        });

        world
            .query_mut()
            .with(GRID_CELL)
            .iter(|_entity, table, index| {
                if table.grid_cell[index].x == grid_x && table.grid_cell[index].y == grid_y {
                    table.grid_cell[index].occupied = false;
                }
            });

        let range_indicators_to_remove: Vec<_> = world.query_entities(RANGE_INDICATOR)
            .into_iter()
            .filter_map(|range_entity| {
                world.get_range_indicator(range_entity)
                    .filter(|indicator| indicator.tower_entity == tower_entity)
                    .map(|_| range_entity)
            })
            .collect();

        for range_entity in range_indicators_to_remove {
            world.queue_despawn_entity(range_entity);
        }

        world.queue_despawn_entity(tower_entity);
        world.apply_commands();
    }
}

fn restart_game(world: &mut GameWorld) {
    let towers_to_remove: Vec<_> = world.query_entities(TOWER).into_iter().collect();
    for entity in towers_to_remove {
        world.queue_despawn_entity(entity);
    }

    let enemies_to_remove: Vec<_> = world.query_entities(ENEMY).collect();
    for entity in enemies_to_remove {
        world.queue_despawn_entity(entity);
    }

    let projectiles_to_remove: Vec<_> = world.query_entities(PROJECTILE).collect();
    for entity in projectiles_to_remove {
        world.queue_despawn_entity(entity);
    }

    let effects_to_remove: Vec<_> = world.query_entities(VISUAL_EFFECT).collect();
    for entity in effects_to_remove {
        world.queue_despawn_entity(entity);
    }

    let money_popups_to_remove: Vec<_> = world.query_entities(MONEY_POPUP).into_iter().collect();
    for entity in money_popups_to_remove {
        world.queue_despawn_entity(entity);
    }

    let range_indicators_to_remove: Vec<_> = world.query_entities(RANGE_INDICATOR).into_iter().collect();
    for entity in range_indicators_to_remove {
        world.queue_despawn_entity(entity);
    }

    world.apply_commands();

    world.resources.money = 200;
    world.resources.lives = 1;
    world.resources.wave = 0;
    world.resources.current_hp = 20;
    world.resources.max_hp = 20;
    world.resources.game_state = GameState::WaitingForWave;
    world.resources.game_speed = 1.0;
    world.resources.spawn_timer = 0.0;
    world.resources.enemies_to_spawn.clear();
    world.resources.wave_announce_timer = 0.0;
}

fn render_grid(world: &GameWorld) {
    let scale = get_scale();
    let offset = get_offset();

    for entity in world.query_entities(GRID_CELL) {
        if let Some(cell) = world.get_grid_cell(entity) {
            let base_pos = grid_to_base(cell.x, cell.y);
            let pos = Vec2::new(offset.x + base_pos.x * scale, offset.y + base_pos.y * scale);

            let path_start = Vec2::new(
                offset.x + world.resources.path[0].x * scale,
                offset.y + world.resources.path[0].y * scale,
            );
            let path_end = Vec2::new(
                offset.x + world.resources.path.last().unwrap().x * scale,
                offset.y + world.resources.path.last().unwrap().y * scale,
            );

            let is_start = (pos - path_start).length() < TILE_SIZE * scale / 2.0;
            let is_end = (pos - path_end).length() < TILE_SIZE * scale / 2.0;

            let color = if is_start {
                ORANGE
            } else if is_end {
                BLUE
            } else if cell.is_path {
                Color::new(0.5, 0.3, 0.1, 1.0)
            } else {
                Color::new(0.1, 0.3, 0.1, 1.0)
            };

            draw_rectangle(
                pos.x - TILE_SIZE * scale / 2.0 + scale,
                pos.y - TILE_SIZE * scale / 2.0 + scale,
                (TILE_SIZE - 2.0) * scale,
                (TILE_SIZE - 2.0) * scale,
                color,
            );
        }
    }

    if let Some((grid_x, grid_y)) = world.resources.mouse_grid_pos {
        if can_place_tower_at(world, grid_x, grid_y) {
            let tower_type = world.resources.selected_tower_type;
            if world.resources.money >= tower_type.cost() {
                let pos = grid_to_screen(grid_x, grid_y);
                draw_rectangle(
                    pos.x - TILE_SIZE * scale / 2.0 + scale,
                    pos.y - TILE_SIZE * scale / 2.0 + scale,
                    (TILE_SIZE - 2.0) * scale,
                    (TILE_SIZE - 2.0) * scale,
                    Color::new(tower_type.color().r, tower_type.color().g, tower_type.color().b, 0.3),
                );

                draw_circle_lines(pos.x, pos.y, tower_type.range() * scale, 2.0, Color::new(tower_type.color().r, tower_type.color().g, tower_type.color().b, 0.5));
            }
        }
    }
}

fn render_towers(world: &GameWorld) {
    let scale = get_scale();
    let offset = get_offset();

    world
        .query()
        .with(TOWER | POSITION)
        .iter(|_entity, table, index| {
            let tower = &table.tower[index];
            let pos = &table.position[index];
            let screen_pos = Vec2::new(
                offset.x + pos.0.x * scale,
                offset.y + pos.0.y * scale,
            );
            let size = (20.0 + tower.fire_animation * 4.0) * scale;
            draw_circle(screen_pos.x, screen_pos.y, size / 2.0, tower.tower_type.color());
            draw_circle_lines(screen_pos.x, screen_pos.y, size / 2.0, 2.0, BLACK);

            if tower.tower_type == TowerType::Sniper {
                if let Some(target_entity) = tower.target {
                    if let Some(target_pos) = world.get_position(target_entity) {
                        let target_screen_pos = Vec2::new(
                            offset.x + target_pos.0.x * scale,
                            offset.y + target_pos.0.y * scale,
                        );
                        draw_line(screen_pos.x, screen_pos.y, target_screen_pos.x, target_screen_pos.y, 2.0, RED);
                    }
                }
            }
        });

    if let Some((grid_x, grid_y)) = world.resources.mouse_grid_pos {
        let mut tower_data = None;
        world
            .query()
            .with(TOWER | GRID_POSITION | POSITION)
            .iter(|_entity, table, index| {
                if table.grid_position[index].x == grid_x && table.grid_position[index].y == grid_y {
                    tower_data = Some((table.tower[index], table.position[index]));
                }
            });

        if let Some((tower, pos)) = tower_data {
            let screen_pos = Vec2::new(
                offset.x + pos.0.x * scale,
                offset.y + pos.0.y * scale,
            );
            draw_circle_lines(screen_pos.x, screen_pos.y, tower.tower_type.range() * scale, 2.0, tower.tower_type.color());

            if let Some(target_entity) = tower.target {
                if let Some(target_pos) = world.get_position(target_entity) {
                    let target_screen_pos = Vec2::new(
                        offset.x + target_pos.0.x * scale,
                        offset.y + target_pos.0.y * scale,
                    );
                    draw_line(screen_pos.x, screen_pos.y, target_screen_pos.x, target_screen_pos.y, 2.0, RED);
                }
            }
        }
    }
}

fn render_enemies(world: &GameWorld) {
    let scale = get_scale();
    let offset = get_offset();

    world
        .query()
        .with(ENEMY | POSITION)
        .iter(|_entity, table, index| {
            let enemy = &table.enemy[index];
            let pos = &table.position[index];
            let screen_pos = Vec2::new(
                offset.x + pos.0.x * scale,
                offset.y + pos.0.y * scale,
            );
            let size = enemy.enemy_type.size() * scale;
            draw_circle(screen_pos.x, screen_pos.y, size, enemy.enemy_type.color());
            draw_circle_lines(screen_pos.x, screen_pos.y, size, 2.0, BLACK);

            if enemy.shield_health > 0.0 {
                let shield_alpha = enemy.shield_health / enemy.max_shield;
                draw_circle_lines(
                    screen_pos.x,
                    screen_pos.y,
                    size + 3.0 * scale,
                    2.0,
                    Color::new(0.5, 0.5, 1.0, shield_alpha),
                );
            }

            let health_percent = enemy.health / enemy.max_health;
            let bar_width = size * 2.0;
            let bar_height = 4.0 * scale;
            let bar_y = screen_pos.y - size - 10.0 * scale;

            draw_rectangle(
                screen_pos.x - bar_width / 2.0,
                bar_y,
                bar_width,
                bar_height,
                BLACK,
            );

            let health_color = if health_percent > 0.5 {
                GREEN
            } else if health_percent > 0.25 {
                YELLOW
            } else {
                RED
            };

            draw_rectangle(
                screen_pos.x - bar_width / 2.0,
                bar_y,
                bar_width * health_percent,
                bar_height,
                health_color,
            );
        });
}

fn render_projectiles(world: &GameWorld) {
    let scale = get_scale();
    let offset = get_offset();

    world
        .query()
        .with(PROJECTILE | POSITION)
        .iter(|_entity, table, index| {
            let projectile = &table.projectile[index];
            let pos = &table.position[index];
            let screen_pos = Vec2::new(
                offset.x + pos.0.x * scale,
                offset.y + pos.0.y * scale,
            );
            let color = match projectile.tower_type {
                TowerType::Basic => YELLOW,
                TowerType::Frost => SKYBLUE,
                TowerType::Cannon => ORANGE,
                TowerType::Sniper => LIGHTGRAY,
                TowerType::Poison => Color::new(0.5, 0.0, 0.8, 1.0),
            };

            let size = match projectile.tower_type {
                TowerType::Cannon => 8.0,
                TowerType::Sniper => 10.0,
                _ => 5.0,
            } * scale;

            draw_circle(screen_pos.x, screen_pos.y, size, color);
        });
}

fn render_visual_effects(world: &GameWorld) {
    let scale = get_scale();
    let offset = get_offset();

    world
        .query()
        .with(VISUAL_EFFECT | POSITION)
        .iter(|_entity, table, index| {
            let effect = &table.visual_effect[index];
            let pos = &table.position[index];
            let screen_pos = Vec2::new(
                offset.x + pos.0.x * scale,
                offset.y + pos.0.y * scale,
            );
            let progress = effect.age / effect.lifetime;
            let alpha = 1.0 - progress;

            match effect.effect_type {
                EffectType::Explosion => {
                    let size = (1.0 - progress) * 10.0 * scale;
                    draw_circle(
                        screen_pos.x,
                        screen_pos.y,
                        size,
                        Color::new(1.0, 0.5, 0.0, alpha),
                    );
                }
                EffectType::PoisonBubble => {
                    let size = 5.0 * (1.0 + progress * 0.5) * scale;
                    draw_circle(
                        screen_pos.x,
                        screen_pos.y,
                        size,
                        Color::new(0.5, 0.0, 0.8, alpha * 0.6),
                    );
                }
                EffectType::DeathParticle => {
                    let size = (1.0 - progress) * 5.0 * scale;
                    draw_circle(
                        screen_pos.x,
                        screen_pos.y,
                        size,
                        Color::new(1.0, 0.0, 0.0, alpha),
                    );
                }
            }
        });
}

fn render_money_popups(world: &GameWorld) {
    let scale = get_scale();
    let offset = get_offset();

    world
        .query()
        .with(MONEY_POPUP | POSITION)
        .iter(|_entity, table, index| {
            let popup = &table.money_popup[index];
            let pos = &table.position[index];
            let screen_pos = Vec2::new(
                offset.x + pos.0.x * scale,
                offset.y + pos.0.y * scale,
            );
            let progress = popup.lifetime / 2.0;
            let alpha = 1.0 - progress.min(1.0);
            let text_scale = 1.0 + progress * 0.5;

            let text = if popup.amount > 0 {
                format!("+${}", popup.amount)
            } else {
                format!("-${}", -popup.amount)
            };

            let color = if popup.amount > 0 {
                Color::new(0.0, 1.0, 0.0, alpha)
            } else {
                Color::new(1.0, 0.0, 0.0, alpha)
            };

            draw_text(
                &text,
                screen_pos.x - 20.0 * scale,
                screen_pos.y,
                20.0 * scale * text_scale,
                color,
            );
        });
}

fn enemy_died_event_handler(world: &mut GameWorld) {
    for event in world.collect_enemy_died() {
        world.resources.money += event.reward;

        for _ in 0..6 {
            let velocity = Vec2::new(
                rand::gen_range(-40.0, 40.0),
                rand::gen_range(-40.0, 40.0),
            );
            spawn_visual_effect(
                world,
                event.position,
                EffectType::DeathParticle,
                velocity,
                0.8,
            );
        }

        if event.reward > 0 {
            spawn_money_popup(world, event.position, event.reward as i32);
        }
    }
}

fn health_bar_update_system(world: &mut GameWorld) {
    world.for_each_mut_changed(ENEMY, 0, |_entity, _table, _idx| {
    });
}

fn render_ui(world: &GameWorld) {
    let money_text = format!("Money: ${}", world.resources.money);
    draw_text(&money_text, 10.0, 30.0, 30.0, GREEN);

    let lives_text = format!("Lives: {}", world.resources.lives);
    draw_text(&lives_text, 10.0, 60.0, 25.0, RED);

    let hp_text = format!("HP: {}/{}", world.resources.current_hp, world.resources.max_hp);
    draw_text(&hp_text, 10.0, 90.0, 25.0, YELLOW);

    let wave_text = format!("Wave: {}", world.resources.wave);
    draw_text(&wave_text, screen_width() - 150.0, 30.0, 30.0, SKYBLUE);

    let speed_text = format!("Speed: {}x", world.resources.game_speed);
    draw_text(&speed_text, screen_width() - 150.0, 60.0, 20.0, WHITE);

    let total_hp = (world.resources.lives - 1) * world.resources.max_hp + world.resources.current_hp;
    let max_total_hp = world.resources.lives * world.resources.max_hp;
    let health_percentage = total_hp as f32 / max_total_hp as f32;

    let bar_width = 200.0;
    let bar_height = 20.0;
    let bar_x = 10.0;
    let bar_y = 100.0;

    draw_rectangle(bar_x, bar_y, bar_width, bar_height, BLACK);

    let health_color = if health_percentage > 0.5 {
        GREEN
    } else if health_percentage > 0.25 {
        YELLOW
    } else {
        RED
    };

    draw_rectangle(
        bar_x,
        bar_y,
        bar_width * health_percentage,
        bar_height,
        health_color,
    );

    let tower_ui_y = 140.0;
    let tower_types = [
        (TowerType::Basic, "1"),
        (TowerType::Frost, "2"),
        (TowerType::Cannon, "3"),
        (TowerType::Sniper, "4"),
        (TowerType::Poison, "5"),
    ];

    for (index, (tower_type, key)) in tower_types.iter().enumerate() {
        let x = 10.0 + index as f32 * 60.0;
        let is_selected = world.resources.selected_tower_type == *tower_type;
        let can_afford = world.resources.money >= tower_type.cost();

        let color = if is_selected {
            tower_type.color()
        } else if can_afford {
            Color::new(
                tower_type.color().r * 0.7,
                tower_type.color().g * 0.7,
                tower_type.color().b * 0.7,
                1.0,
            )
        } else {
            DARKGRAY
        };

        draw_rectangle(x, tower_ui_y, 50.0, 50.0, color);
        draw_rectangle_lines(x, tower_ui_y, 50.0, 50.0, 2.0, BLACK);

        draw_text(key, x + 5.0, tower_ui_y + 20.0, 20.0, BLACK);
        draw_text(&format!("${}", tower_type.cost()), x + 5.0, tower_ui_y + 45.0, 15.0, BLACK);
    }

    if world.resources.wave_announce_timer > 0.0 {
        let alpha = if world.resources.wave_announce_timer < 1.0 {
            world.resources.wave_announce_timer
        } else {
            1.0
        };

        let text = format!("WAVE {}", world.resources.wave);
        let text_size = 60.0;
        let text_dims = measure_text(&text, None, text_size as u16, 1.0);
        draw_text(
            &text,
            screen_width() / 2.0 - text_dims.width / 2.0,
            screen_height() / 2.0 - 100.0,
            text_size,
            Color::new(1.0, 0.8, 0.0, alpha),
        );
    }

    match world.resources.game_state {
        GameState::WaitingForWave => {
            let text = "Press SPACE to start wave";
            let text_size = 40.0;
            let text_dims = measure_text(text, None, text_size as u16, 1.0);
            draw_text(
                text,
                screen_width() / 2.0 - text_dims.width / 2.0,
                screen_height() / 2.0,
                text_size,
                WHITE,
            );
        }
        GameState::Paused => {
            let text = "PAUSED - Press P to resume";
            let text_size = 50.0;
            let text_dims = measure_text(text, None, text_size as u16, 1.0);
            draw_text(
                text,
                screen_width() / 2.0 - text_dims.width / 2.0,
                screen_height() / 2.0,
                text_size,
                YELLOW,
            );
        }
        GameState::GameOver => {
            let text = "GAME OVER - Press R to restart";
            let text_size = 50.0;
            let text_dims = measure_text(text, None, text_size as u16, 1.0);
            draw_text(
                text,
                screen_width() / 2.0 - text_dims.width / 2.0,
                screen_height() / 2.0,
                text_size,
                RED,
            );
        }
        GameState::Victory => {
            let text = "VICTORY! Press R to restart";
            let text_size = 50.0;
            let text_dims = measure_text(text, None, text_size as u16, 1.0);
            draw_text(
                text,
                screen_width() / 2.0 - text_dims.width / 2.0,
                screen_height() / 2.0,
                text_size,
                GREEN,
            );
        }
        _ => {}
    }

    let controls_text = "Controls: 1-5: Tower Type | Left Click: Place | Right Click: Sell | [/]: Speed | P: Pause | R: Restart";
    draw_text(controls_text, 10.0, screen_height() - 10.0, 15.0, LIGHTGRAY);
}

fn wave_spawning_system_wrapper(world: &mut GameWorld) {
    let delta_time = get_frame_time() * world.resources.game_speed;
    wave_spawning_system(world, delta_time);
}

fn enemy_movement_system_wrapper(world: &mut GameWorld) {
    let delta_time = get_frame_time() * world.resources.game_speed;
    enemy_movement_system(world, delta_time);
}

fn tower_shooting_system_wrapper(world: &mut GameWorld) {
    let delta_time = get_frame_time() * world.resources.game_speed;
    tower_shooting_system(world, delta_time);
}

fn projectile_movement_system_wrapper(world: &mut GameWorld) {
    let delta_time = get_frame_time() * world.resources.game_speed;
    projectile_movement_system(world, delta_time);
}

fn visual_effects_system_wrapper(world: &mut GameWorld) {
    let delta_time = get_frame_time() * world.resources.game_speed;
    visual_effects_system(world, delta_time);
}

fn update_money_popups_wrapper(world: &mut GameWorld) {
    let delta_time = get_frame_time() * world.resources.game_speed;
    update_money_popups(world, delta_time);
}

#[macroquad::main("Tower Defense")]
async fn main() {
    let mut world = GameWorld::default();

    world.resources.money = 200;
    world.resources.lives = 1;
    world.resources.wave = 0;
    world.resources.current_hp = 20;
    world.resources.max_hp = 20;
    world.resources.game_state = GameState::WaitingForWave;
    world.resources.game_speed = 1.0;
    world.resources.selected_tower_type = TowerType::Basic;

    initialize_grid(&mut world);
    create_path(&mut world);

    let mut game_schedule = Schedule::new();
    game_schedule
        .add_system_mut(wave_spawning_system_wrapper)
        .add_system_mut(enemy_movement_system_wrapper)
        .add_system_mut(tower_targeting_system)
        .add_system_mut(tower_shooting_system_wrapper)
        .add_system_mut(projectile_movement_system_wrapper)
        .add_system_mut(visual_effects_system_wrapper)
        .add_system_mut(update_money_popups_wrapper)
        .add_system_mut(enemy_died_event_handler)
        .add_system_mut(health_bar_update_system);

    let mut render_schedule = Schedule::new();
    render_schedule
        .add_system(render_grid)
        .add_system(render_towers)
        .add_system(render_enemies)
        .add_system(render_projectiles)
        .add_system(render_visual_effects)
        .add_system(render_money_popups)
        .add_system(render_ui);

    loop {
        clear_background(Color::new(0.05, 0.05, 0.05, 1.0));

        input_system(&mut world);

        if world.resources.game_state != GameState::Paused {
            game_schedule.run(&mut world);
        }

        if world.resources.wave_announce_timer > 0.0 {
            world.resources.wave_announce_timer -= get_frame_time();
        }

        if is_key_pressed(KeyCode::Space) {
            if world.resources.game_state == GameState::WaitingForWave {
                plan_wave(&mut world);
            }
        }

        render_schedule.run(&mut world);

        world.step();
        next_frame().await;
    }
}
