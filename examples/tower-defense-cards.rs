use freecs::{Entity, Schedule, ecs};
use macroquad::prelude::*;

ecs! {
    GameWorld {
        position: Position => POSITION,
        velocity: Velocity => VELOCITY,
        tower: Tower => TOWER,
        health: Health => HEALTH,
        speed: Speed => SPEED,
        path_follower: PathFollower => PATH_FOLLOWER,
        status_effects: StatusEffects => STATUS_EFFECTS,
        enemy_data: EnemyData => ENEMY_DATA,
        projectile: Projectile => PROJECTILE,
        grid_cell: GridCell => GRID_CELL,
        grid_position: GridPosition => GRID_POSITION,
        health_bar: HealthBar => HEALTH_BAR,
        visual_effect: VisualEffect => VISUAL_EFFECT,
        range_indicator: RangeIndicator => RANGE_INDICATOR,
        money_popup: MoneyPopup => MONEY_POPUP,
        card: Card => CARD,
        map_node: MapNode => MAP_NODE,
        heal_aura: HealAura => HEAL_AURA,
        player: Player => PLAYER,
        relic: Relic => RELIC,
        cooldown: Cooldown => COOLDOWN,
        targeting: Targeting => TARGETING,
        animation: Animation => ANIMATION,
    }
    Tags {
        enemy => ENEMY,
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
        tower_upgraded: TowerUpgradedEvent,
        wave_completed: WaveCompletedEvent,
        wave_started: WaveStartedEvent,
        damage: DamageEvent,
    }
    GameResources {
        economy: EconomyState,
        combat: CombatState,
        card_system: CardSystemState,
        ui: UIState,
        meta_game: MetaGameState,
        config: GameConfig,
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
    fn color(&self) -> Color {
        match self {
            TowerType::Basic => GREEN,
            TowerType::Frost => Color::new(0.2, 0.6, 1.0, 1.0),
            TowerType::Cannon => RED,
            TowerType::Sniper => DARKGRAY,
            TowerType::Poison => Color::new(0.6, 0.2, 0.8, 1.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum GameState {
    #[default]
    Map,
    WaitingForWave,
    WaveInProgress,
    GameOver,
    Victory,
    Paused,
    DeckView,
    Shop,
    Rest,
    Forge,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NodeType {
    Combat,
    Elite,
    Boss,
    Shop,
    Rest,
    Forge,
}

impl Default for NodeType {
    fn default() -> Self {
        NodeType::Combat
    }
}

impl NodeType {
    fn color(&self) -> Color {
        match self {
            NodeType::Combat => Color::new(0.8, 0.3, 0.3, 1.0),
            NodeType::Elite => Color::new(0.9, 0.5, 0.2, 1.0),
            NodeType::Boss => Color::new(0.6, 0.1, 0.6, 1.0),
            NodeType::Shop => Color::new(0.3, 0.7, 0.9, 1.0),
            NodeType::Rest => Color::new(0.3, 0.8, 0.3, 1.0),
            NodeType::Forge => Color::new(0.9, 0.5, 0.1, 1.0),
        }
    }

    fn icon(&self) -> &str {
        match self {
            NodeType::Combat => "Combat",
            NodeType::Elite => "Elite",
            NodeType::Boss => "Boss",
            NodeType::Shop => "Shop",
            NodeType::Rest => "Rest",
            NodeType::Forge => "Forge",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct MapNode {
    pub node_type: NodeType,
    pub layer: usize,
    pub position_in_layer: usize,
    pub connections: Vec<usize>,
    pub visited: bool,
    pub available: bool,
}

#[derive(Debug, Clone, Default)]
pub struct MapData {
    pub nodes: Vec<MapNode>,
    pub layers: usize,
    pub nodes_per_layer: Vec<usize>,
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
}

impl Default for EnemyType {
    fn default() -> Self {
        EnemyType::Normal
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct GridCoord {
    pub x: i32,
    pub y: i32,
}

impl GridCoord {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Position(pub Vec2);

#[derive(Debug, Clone, Copy, Default)]
pub struct Velocity(pub Vec2);

#[derive(Debug, Clone, Copy, Default)]
pub struct Health {
    pub current: f32,
    pub maximum: f32,
    pub shield_current: f32,
    pub shield_maximum: f32,
}

impl Health {
    pub fn new(health: f32) -> Self {
        Self {
            current: health,
            maximum: health,
            shield_current: 0.0,
            shield_maximum: 0.0,
        }
    }

    pub fn with_shield(health: f32, shield: f32) -> Self {
        Self {
            current: health,
            maximum: health,
            shield_current: shield,
            shield_maximum: shield,
        }
    }

    pub fn is_alive(&self) -> bool {
        self.current > 0.0
    }

    pub fn apply_damage(&mut self, damage: f32) {
        if self.shield_current > 0.0 {
            let shield_damage = damage.min(self.shield_current);
            self.shield_current -= shield_damage;
            let remaining_damage = damage - shield_damage;
            if remaining_damage > 0.0 {
                self.current -= remaining_damage;
            }
        } else {
            self.current -= damage;
        }
    }

    pub fn heal(&mut self, amount: f32) {
        self.current = (self.current + amount).min(self.maximum);
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Speed {
    pub base_speed: f32,
    pub current_speed: f32,
}

impl Speed {
    pub fn new(speed: f32) -> Self {
        Self {
            base_speed: speed,
            current_speed: speed,
        }
    }

    pub fn apply_slow(&mut self, factor: f32) {
        self.current_speed = self.base_speed * factor;
    }

    pub fn reset(&mut self) {
        self.current_speed = self.base_speed;
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PathFollower {
    pub path_index: usize,
    pub path_progress: f32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StatusEffects {
    pub slow_duration: f32,
    pub slow_factor: f32,
    pub poison_duration: f32,
    pub poison_damage_per_second: f32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EnemyData {
    pub enemy_type: EnemyType,
    pub reward_value: u32,
    pub is_flying: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Tower {
    pub tower_type: TowerType,
    pub level: u32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Cooldown {
    pub remaining: f32,
    pub duration: f32,
}

impl Cooldown {
    pub fn new(duration: f32) -> Self {
        Self {
            remaining: 0.0,
            duration,
        }
    }

    pub fn tick(&mut self, delta: f32) {
        self.remaining = (self.remaining - delta).max(0.0);
    }

    pub fn is_ready(&self) -> bool {
        self.remaining <= 0.0
    }

    pub fn reset(&mut self) {
        self.remaining = self.duration;
    }

    pub fn set_duration(&mut self, duration: f32) {
        self.duration = duration;
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Targeting {
    pub target_position: Option<Vec2>,
    pub tracking_time: f32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Animation {
    pub fire_animation: f32,
}

impl Animation {
    pub fn tick(&mut self, delta: f32, decay_rate: f32) {
        if self.fire_animation > 0.0 {
            self.fire_animation -= delta * decay_rate;
            self.fire_animation = self.fire_animation.max(0.0);
        }
    }

    pub fn trigger(&mut self) {
        self.fire_animation = 1.0;
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Projectile {
    pub damage: f32,
    pub target_position: Vec2,
    pub speed: f32,
    pub tower_type: TowerType,
    pub start_position: Vec2,
    pub arc_height: f32,
    pub flight_progress: f32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct GridCell {
    pub coord: GridCoord,
    pub occupied: bool,
    pub is_path: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct GridPosition {
    pub coord: GridCoord,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HealthBar {}

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
    pub tower_grid_coord: GridCoord,
    pub visible: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MoneyPopup {
    pub lifetime: f32,
    pub amount: i32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HealAura {
    pub cooldown: f32,
    pub heal_amount: f32,
    pub range: f32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Player {
    pub health: u32,
    pub max_health: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CardRarity {
    Common,
    Rare,
    Epic,
    Legendary,
}

impl Default for CardRarity {
    fn default() -> Self {
        CardRarity::Common
    }
}

impl CardRarity {
    fn color(&self) -> Color {
        match self {
            CardRarity::Common => Color::new(0.7, 0.7, 0.7, 1.0),
            CardRarity::Rare => Color::new(0.3, 0.5, 1.0, 1.0),
            CardRarity::Epic => Color::new(0.6, 0.2, 0.8, 1.0),
            CardRarity::Legendary => Color::new(1.0, 0.7, 0.0, 1.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CardState {
    InDeck,
    Drawing,
    InHand,
}

impl Default for CardState {
    fn default() -> Self {
        CardState::InDeck
    }
}

#[derive(Debug, Clone, Default)]
pub struct Card {
    pub card_index: usize,
    pub tower_pattern: Vec<Option<TowerType>>,
    pub cost: u32,
    pub rarity: CardRarity,
    pub name: String,
    pub in_hand: bool,
    pub card_state: CardState,
    pub draw_animation_progress: f32,
    pub hand_position_index: usize,
}

#[derive(Debug, Clone)]
pub struct EnemyCard {
    pub enemy_types: Vec<EnemyType>,
    pub count: usize,
    pub play_delay: f32,
}

#[derive(Debug, Clone)]
pub struct EnemyDeck {
    pub encounter_name: String,
    pub cards: Vec<EnemyCard>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum RelicType {
    #[default]
    CriticalStrike,
    DoubleTap,
    RapidFire,
    SniperNest,
    FrostAura,
    GoldenTouch,
    BulkDiscount,
    Recycler,
    Overcharge,
    PoisonCloud,
    CannonBoost,
    RangeExtender,
}

impl RelicType {
    fn name(&self) -> &str {
        match self {
            RelicType::CriticalStrike => "Critical Strike",
            RelicType::DoubleTap => "Double Tap",
            RelicType::RapidFire => "Rapid Fire",
            RelicType::SniperNest => "Sniper Nest",
            RelicType::FrostAura => "Frost Aura",
            RelicType::GoldenTouch => "Golden Touch",
            RelicType::BulkDiscount => "Bulk Discount",
            RelicType::Recycler => "Recycler",
            RelicType::Overcharge => "Overcharge",
            RelicType::PoisonCloud => "Poison Cloud",
            RelicType::CannonBoost => "Cannon Boost",
            RelicType::RangeExtender => "Range Extender",
        }
    }

    fn description(&self) -> &str {
        match self {
            RelicType::CriticalStrike => "Towers have 20% chance to deal double damage",
            RelicType::DoubleTap => "Towers fire an extra projectile",
            RelicType::RapidFire => "All towers attack 30% faster",
            RelicType::SniperNest => "Sniper towers gain +50% range",
            RelicType::FrostAura => "Frost towers slow radius increased by 50%",
            RelicType::GoldenTouch => "Gain 50% more money from kills",
            RelicType::BulkDiscount => "All cards cost 20% less",
            RelicType::Recycler => "Refund 50% when selling towers",
            RelicType::Overcharge => "Towers deal 25% more damage",
            RelicType::PoisonCloud => "Poison towers affect nearby enemies",
            RelicType::CannonBoost => "Cannon towers have +30% splash range",
            RelicType::RangeExtender => "All towers gain +20% range",
        }
    }

    fn cost(&self) -> u32 {
        match self {
            RelicType::GoldenTouch | RelicType::BulkDiscount => 150,
            RelicType::Overcharge | RelicType::RangeExtender => 200,
            RelicType::DoubleTap | RelicType::CriticalStrike => 180,
            RelicType::RapidFire => 160,
            RelicType::Recycler => 120,
            _ => 140,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Relic {
    pub relic_type: RelicType,
}

#[derive(Debug, Clone, Default)]
pub struct EconomyState {
    pub money: u32,
    pub owned_relics: Vec<RelicType>,
}

impl EconomyState {
    pub fn can_afford(&self, cost: u32) -> bool {
        self.money >= cost
    }

    pub fn spend(&mut self, cost: u32) -> Result<(), String> {
        if !self.can_afford(cost) {
            return Err(format!(
                "Insufficient funds: have {}, need {}",
                self.money, cost
            ));
        }
        self.money -= cost;
        Ok(())
    }

    pub fn earn(&mut self, amount: u32) {
        self.money += amount;
    }

    pub fn has_relic(&self, relic: RelicType) -> bool {
        self.owned_relics.contains(&relic)
    }
}

#[derive(Debug, Clone, Default)]
pub struct CombatState {
    pub lives: u32,
    pub wave: u32,
    pub game_state: GameState,
    pub game_speed: f32,
    pub spawn_timer: f32,
    pub wave_announce_timer: f32,
    pub path: Vec<Vec2>,
    pub enemy_deck: Vec<EnemyCard>,
    pub enemy_deck_play_timer: f32,
    pub current_encounter_name: String,
    pub victory_timer: f32,
    pub current_hp: u32,
    pub max_hp: u32,
}

impl CombatState {
    pub fn is_combat_active(&self) -> bool {
        matches!(
            self.game_state,
            GameState::WaitingForWave | GameState::WaveInProgress
        )
    }

    pub fn take_damage(&mut self, damage: u32) -> bool {
        if damage >= self.current_hp {
            if self.lives > 1 {
                self.lives -= 1;
                self.current_hp = self.max_hp;
                false
            } else {
                self.current_hp = 0;
                self.lives = 0;
                true
            }
        } else {
            self.current_hp -= damage;
            false
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CardSystemState {
    pub selected_card: Option<usize>,
    pub hand_size: usize,
    pub max_hand_size: usize,
}

impl CardSystemState {
    pub fn can_draw(&self) -> bool {
        self.hand_size < self.max_hand_size
    }

    pub fn is_card_selected(&self, index: usize) -> bool {
        self.selected_card == Some(index)
    }
}

#[derive(Debug, Clone, Default)]
pub struct UIState {
    pub mouse_grid_pos: Option<(i32, i32)>,
    pub selected_tower_type: TowerType,
}

#[derive(Debug, Clone, Default)]
pub struct MetaGameState {
    pub map_data: MapData,
    pub current_node: usize,
    pub previous_state: GameState,
    pub shop_offerings: Vec<ShopOffering>,
    pub rest_options: Vec<RestOption>,
    pub forge_selected_cards: Vec<usize>,
    pub forge_offered_cards: Vec<(String, Vec<Option<TowerType>>, CardRarity)>,
    pub forge_uses_remaining: u32,
}

#[derive(Debug, Clone)]
pub struct GameConfig {
    pub tower_configs: TowerConfigs,
    pub enemy_configs: EnemyConfigs,
    pub gameplay_constants: GameplayConstants,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            tower_configs: TowerConfigs::default(),
            enemy_configs: EnemyConfigs::default(),
            gameplay_constants: GameplayConstants::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TowerConfigs {
    pub basic: TowerStats,
    pub frost: TowerStats,
    pub cannon: TowerStats,
    pub sniper: TowerStats,
    pub poison: TowerStats,
}

impl Default for TowerConfigs {
    fn default() -> Self {
        Self {
            basic: TowerStats {
                cost: 60,
                base_damage: 15.0,
                base_range: 100.0,
                base_fire_rate: 0.5,
                projectile_speed: 300.0,
                upgrade_cost_multiplier: 0.5,
            },
            frost: TowerStats {
                cost: 120,
                base_damage: 8.0,
                base_range: 80.0,
                base_fire_rate: 1.0,
                projectile_speed: 200.0,
                upgrade_cost_multiplier: 0.5,
            },
            cannon: TowerStats {
                cost: 200,
                base_damage: 50.0,
                base_range: 120.0,
                base_fire_rate: 2.0,
                projectile_speed: 250.0,
                upgrade_cost_multiplier: 0.5,
            },
            sniper: TowerStats {
                cost: 180,
                base_damage: 80.0,
                base_range: 180.0,
                base_fire_rate: 3.0,
                projectile_speed: 500.0,
                upgrade_cost_multiplier: 0.5,
            },
            poison: TowerStats {
                cost: 150,
                base_damage: 5.0,
                base_range: 90.0,
                base_fire_rate: 0.8,
                projectile_speed: 250.0,
                upgrade_cost_multiplier: 0.5,
            },
        }
    }
}

impl TowerConfigs {
    pub fn get(&self, tower_type: TowerType) -> &TowerStats {
        match tower_type {
            TowerType::Basic => &self.basic,
            TowerType::Frost => &self.frost,
            TowerType::Cannon => &self.cannon,
            TowerType::Sniper => &self.sniper,
            TowerType::Poison => &self.poison,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TowerStats {
    pub cost: u32,
    pub base_damage: f32,
    pub base_range: f32,
    pub base_fire_rate: f32,
    pub projectile_speed: f32,
    pub upgrade_cost_multiplier: f32,
}

impl TowerStats {
    pub fn damage(&self, level: u32) -> f32 {
        self.base_damage * (1.0 + 0.25 * (level - 1) as f32)
    }

    pub fn range(&self, level: u32) -> f32 {
        self.base_range * (1.0 + 0.15 * (level - 1) as f32)
    }

    pub fn fire_rate(&self, level: u32) -> f32 {
        self.base_fire_rate * (1.0 - 0.1 * (level - 1) as f32).max(0.2)
    }

    pub fn upgrade_cost(&self, current_level: u32) -> u32 {
        (self.cost as f32 * self.upgrade_cost_multiplier * current_level as f32) as u32
    }
}

#[derive(Debug, Clone)]
pub struct EnemyConfigs {
    pub normal: EnemyStats,
    pub fast: EnemyStats,
    pub tank: EnemyStats,
    pub flying: EnemyStats,
    pub shielded: EnemyStats,
    pub healer: EnemyStats,
    pub boss: EnemyStats,
}

impl Default for EnemyConfigs {
    fn default() -> Self {
        Self {
            normal: EnemyStats {
                base_health: 50.0,
                speed: 40.0,
                base_value: 10,
                shield: 0.0,
                size: 15.0,
            },
            fast: EnemyStats {
                base_health: 30.0,
                speed: 80.0,
                base_value: 15,
                shield: 0.0,
                size: 12.0,
            },
            tank: EnemyStats {
                base_health: 150.0,
                speed: 20.0,
                base_value: 30,
                shield: 0.0,
                size: 20.0,
            },
            flying: EnemyStats {
                base_health: 40.0,
                speed: 60.0,
                base_value: 20,
                shield: 0.0,
                size: 15.0,
            },
            shielded: EnemyStats {
                base_health: 80.0,
                speed: 30.0,
                base_value: 25,
                shield: 50.0,
                size: 18.0,
            },
            healer: EnemyStats {
                base_health: 60.0,
                speed: 35.0,
                base_value: 40,
                shield: 0.0,
                size: 16.0,
            },
            boss: EnemyStats {
                base_health: 500.0,
                speed: 15.0,
                base_value: 100,
                shield: 100.0,
                size: 30.0,
            },
        }
    }
}

impl EnemyConfigs {
    pub fn get(&self, enemy_type: EnemyType) -> &EnemyStats {
        match enemy_type {
            EnemyType::Normal => &self.normal,
            EnemyType::Fast => &self.fast,
            EnemyType::Tank => &self.tank,
            EnemyType::Flying => &self.flying,
            EnemyType::Shielded => &self.shielded,
            EnemyType::Healer => &self.healer,
            EnemyType::Boss => &self.boss,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EnemyStats {
    pub base_health: f32,
    pub speed: f32,
    pub base_value: u32,
    pub shield: f32,
    pub size: f32,
}

impl EnemyStats {
    pub fn health_for_wave(&self, wave: u32) -> f32 {
        let health_multiplier = 1.0 + (wave as f32 * 0.15) + (wave as f32).ln() * 0.3;
        self.base_health * health_multiplier
    }

    pub fn value_for_wave(&self, wave: u32) -> u32 {
        self.base_value + wave * 2
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GameplayConstants {
    pub starting_money: u32,
    pub starting_lives: u32,
    pub starting_hp: u32,
    pub max_hand_size: usize,
    pub grid_size: i32,
    pub tile_size: f32,
    pub max_tower_level: u32,
    pub tower_sell_refund_percent: f32,
}

impl Default for GameplayConstants {
    fn default() -> Self {
        Self {
            starting_money: 500,
            starting_lives: 1,
            starting_hp: 20,
            max_hand_size: 5,
            grid_size: 12,
            tile_size: 40.0,
            max_tower_level: 4,
            tower_sell_refund_percent: 0.7,
        }
    }
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
pub struct TowerUpgradedEvent {
    pub entity: Entity,
    pub tower_type: TowerType,
    pub old_level: u32,
    pub new_level: u32,
    pub cost: u32,
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

#[derive(Debug, Clone)]
pub struct DamageEvent {
    pub target: Entity,
    pub damage: f32,
}

#[derive(Debug, Clone)]
pub enum ShopOffering {
    Card {
        name: String,
        pattern: Vec<Option<TowerType>>,
        rarity: CardRarity,
        cost: u32,
    },
    UpgradeCard {
        card_entity: Entity,
        current_rarity: CardRarity,
        cost: u32,
    },
    RemoveCard {
        cost: u32,
    },
    Heal {
        amount: u32,
        cost: u32,
    },
    MaxHealth {
        amount: u32,
        cost: u32,
    },
    Relic {
        relic_type: RelicType,
        cost: u32,
    },
}

#[derive(Debug, Clone)]
pub enum RestOption {
    Heal { amount: u32 },
    UpgradeCard,
    RemoveCard,
}

const GRID_SIZE: i32 = 12;
const TILE_SIZE: f32 = 40.0;
const BASE_WIDTH: f32 = 1024.0;
const BASE_HEIGHT: f32 = 768.0;

#[derive(Debug, Clone)]
pub enum GameCommand {
    PlaceTower {
        grid_x: i32,
        grid_y: i32,
        tower_type: TowerType,
    },
    UpgradeTower {
        tower_entity: Entity,
        grid_x: i32,
        grid_y: i32,
    },
    SellTower {
        tower_entity: Entity,
        grid_x: i32,
        grid_y: i32,
    },
    DrawCard {
        count: usize,
    },
    PlayCard {
        card_index: usize,
        grid_x: i32,
        grid_y: i32,
    },
    StartWave,
    PauseGame,
    ResumeGame,
    RestartGame,
    ChangeGameSpeed {
        speed: f32,
    },
    SelectCard {
        index: usize,
    },
    DeselectCard,
    PurchaseShopItem {
        index: usize,
    },
    SelectRestOption {
        index: usize,
    },
    NavigateToNode {
        node_index: usize,
    },
    ForgeSelectCard {
        card_index: usize,
    },
    ForgeChooseOffering {
        offering_index: usize,
    },
}

pub struct CommandExecutor;

impl CommandExecutor {
    pub fn execute(command: GameCommand, world: &mut GameWorld) -> Result<(), String> {
        match command {
            GameCommand::PlaceTower {
                grid_x,
                grid_y,
                tower_type,
            } => Self::execute_place_tower(world, grid_x, grid_y, tower_type),
            GameCommand::UpgradeTower {
                tower_entity,
                grid_x,
                grid_y,
            } => Self::execute_upgrade_tower(world, tower_entity, grid_x, grid_y),
            GameCommand::SellTower {
                tower_entity,
                grid_x,
                grid_y,
            } => Self::execute_sell_tower(world, tower_entity, grid_x, grid_y),
            GameCommand::DrawCard { count } => Self::execute_draw_card(world, count),
            GameCommand::PlayCard {
                card_index,
                grid_x,
                grid_y,
            } => Self::execute_play_card(world, card_index, grid_x, grid_y),
            GameCommand::StartWave => Self::execute_start_wave(world),
            GameCommand::ChangeGameSpeed { speed } => {
                world.resources.combat.game_speed = speed;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn execute_place_tower(
        world: &mut GameWorld,
        grid_x: i32,
        grid_y: i32,
        tower_type: TowerType,
    ) -> Result<(), String> {
        if !can_place_tower_at(world, grid_x, grid_y) {
            return Err("Cannot place tower at this position".to_string());
        }

        let cost = world.resources.config.tower_configs.get(tower_type).cost;
        world.resources.economy.spend(cost)?;

        spawn_tower(world, grid_x, grid_y, tower_type);
        Ok(())
    }

    fn execute_upgrade_tower(
        world: &mut GameWorld,
        tower_entity: Entity,
        grid_x: i32,
        grid_y: i32,
    ) -> Result<(), String> {
        let tower = world.get_tower(tower_entity).ok_or("Tower not found")?;

        let current_level = tower.level;
        let max_level = world.resources.config.gameplay_constants.max_tower_level;

        if current_level >= max_level {
            return Err("Tower is already at max level".to_string());
        }

        let upgrade_cost = world
            .resources
            .config
            .tower_configs
            .get(tower.tower_type)
            .upgrade_cost(current_level);
        world.resources.economy.spend(upgrade_cost)?;

        if let Some(tower_mut) = world.get_tower_mut(tower_entity) {
            tower_mut.level += 1;

            let position = grid_to_base(grid_x, grid_y);
            spawn_money_popup(world, position, -(upgrade_cost as i32));
        }

        Ok(())
    }

    fn execute_sell_tower(
        world: &mut GameWorld,
        tower_entity: Entity,
        grid_x: i32,
        grid_y: i32,
    ) -> Result<(), String> {
        let tower = world.get_tower(tower_entity).ok_or("Tower not found")?;

        let cost = world
            .resources
            .config
            .tower_configs
            .get(tower.tower_type)
            .cost;
        let refund_percent = world
            .resources
            .config
            .gameplay_constants
            .tower_sell_refund_percent;
        let refund = (cost as f32 * refund_percent) as u32;

        world.resources.economy.earn(refund);

        let position = grid_to_base(grid_x, grid_y);
        spawn_money_popup(world, position, refund as i32);

        world
            .query_mut()
            .with(GRID_CELL)
            .iter(|_entity, table, index| {
                if table.grid_cell[index].coord.x == grid_x
                    && table.grid_cell[index].coord.y == grid_y
                {
                    table.grid_cell[index].occupied = false;
                }
            });

        world.queue_despawn_entity(tower_entity);
        world.apply_commands();

        Ok(())
    }

    fn execute_draw_card(_world: &mut GameWorld, _count: usize) -> Result<(), String> {
        Ok(())
    }

    fn execute_play_card(
        _world: &mut GameWorld,
        _card_index: usize,
        _grid_x: i32,
        _grid_y: i32,
    ) -> Result<(), String> {
        Ok(())
    }

    fn execute_start_wave(world: &mut GameWorld) -> Result<(), String> {
        if world.resources.combat.game_state != GameState::WaitingForWave {
            return Err("Cannot start wave in current state".to_string());
        }

        world.resources.combat.wave += 1;
        world.resources.combat.game_state = GameState::WaveInProgress;
        world.resources.combat.wave_announce_timer = 3.0;

        Ok(())
    }
}

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
    Vec2::new(offset.x + base_pos.x * scale, offset.y + base_pos.y * scale)
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
                    coord: GridCoord::new(x, y),
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

    let screen_path: Vec<Vec2> = path
        .iter()
        .map(|&p| {
            Vec2::new(
                grid_offset_x + (p.x + GRID_SIZE as f32 / 2.0 + 0.5) * TILE_SIZE,
                grid_offset_y + (p.y + GRID_SIZE as f32 / 2.0 + 0.5) * TILE_SIZE,
            )
        })
        .collect();

    world.resources.combat.path = screen_path;

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
                if cell.coord.x == grid_x && cell.coord.y == grid_y {
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
    let fire_rate = world
        .resources
        .config
        .tower_configs
        .get(tower_type)
        .fire_rate(1);
    let cost = world.resources.config.tower_configs.get(tower_type).cost;

    let entities = EntityBuilder::new()
        .with_position(Position(position))
        .with_grid_position(GridPosition {
            coord: GridCoord::new(grid_x, grid_y),
        })
        .with_tower(Tower {
            tower_type,
            level: 1,
        })
        .with_cooldown(Cooldown::new(fire_rate))
        .with_targeting(Targeting {
            target_position: None,
            tracking_time: 0.0,
        })
        .with_animation(Animation {
            fire_animation: 0.0,
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

    world.send_tower_placed(TowerPlacedEvent {
        entity,
        tower_type,
        grid_x,
        grid_y,
        cost,
    });

    spawn_range_indicator(world, GridCoord::new(grid_x, grid_y));

    entity
}

fn spawn_range_indicator(world: &mut GameWorld, grid_coord: GridCoord) {
    let entity = world.spawn_entities(RANGE_INDICATOR, 1)[0];
    world.set_range_indicator(
        entity,
        RangeIndicator {
            tower_grid_coord: grid_coord,
            visible: false,
        },
    );
}

fn spawn_enemy(world: &mut GameWorld, enemy_type: EnemyType) -> freecs::Entity {
    let start_pos = world.resources.combat.path[0];
    let enemy_stats = world.resources.config.enemy_configs.get(enemy_type);
    let wave = world.resources.combat.wave;

    let health = enemy_stats.health_for_wave(wave);
    let shield = enemy_stats.shield;
    let speed_value = enemy_stats.speed;
    let value = enemy_stats.value_for_wave(wave);

    let entities = EntityBuilder::new()
        .with_position(Position(start_pos))
        .with_velocity(Velocity(Vec2::ZERO))
        .with_health(Health::with_shield(health, shield))
        .with_speed(Speed::new(speed_value))
        .with_path_follower(PathFollower {
            path_index: 0,
            path_progress: 0.0,
        })
        .with_status_effects(StatusEffects::default())
        .with_enemy_data(EnemyData {
            enemy_type,
            reward_value: value,
            is_flying: enemy_type == EnemyType::Flying,
        })
        .spawn(world, 1);

    let entity = entities[0];
    world.set_health_bar(entity, HealthBar {});
    world.add_enemy(entity);

    match enemy_type {
        EnemyType::Normal => world.add_basic_enemy(entity),
        EnemyType::Tank => world.add_tank_enemy(entity),
        EnemyType::Fast => world.add_fast_enemy(entity),
        EnemyType::Flying => world.add_flying_enemy(entity),
        EnemyType::Healer => {
            world.add_healer_enemy(entity);
            world.set_heal_aura(
                entity,
                HealAura {
                    cooldown: 0.0,
                    heal_amount: 30.0,
                    range: 60.0,
                },
            );
        }
        _ => world.add_basic_enemy(entity),
    }

    world.send_enemy_spawned(EnemySpawnedEvent { entity, enemy_type });

    entity
}

fn spawn_projectile(
    world: &mut GameWorld,
    from: Vec2,
    target_position: Vec2,
    tower_type: TowerType,
    level: u32,
) -> freecs::Entity {
    let arc_height = if tower_type == TowerType::Cannon {
        50.0
    } else {
        0.0
    };
    let tower_stats = world.resources.config.tower_configs.get(tower_type);
    let damage = tower_stats.damage(level);
    let speed = tower_stats.projectile_speed;

    EntityBuilder::new()
        .with_position(Position(from))
        .with_velocity(Velocity(Vec2::ZERO))
        .with_projectile(Projectile {
            damage,
            target_position,
            speed,
            tower_type,
            start_position: from,
            arc_height,
            flight_progress: 0.0,
        })
        .spawn(world, 1)[0]
}

fn spawn_visual_effect(
    world: &mut GameWorld,
    position: Vec2,
    effect_type: EffectType,
    velocity: Vec2,
    lifetime: f32,
) {
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

fn get_all_card_definitions() -> Vec<(&'static str, Vec<Option<TowerType>>, CardRarity)> {
    vec![
        (
            "Sniper",
            vec![
                None,
                None,
                None,
                None,
                Some(TowerType::Sniper),
                None,
                None,
                None,
                None,
            ],
            CardRarity::Common,
        ),
        (
            "Frost Line",
            vec![
                Some(TowerType::Frost),
                Some(TowerType::Frost),
                Some(TowerType::Frost),
                None,
                None,
                None,
                None,
                None,
                None,
            ],
            CardRarity::Common,
        ),
        (
            "Frost Corners",
            vec![
                Some(TowerType::Frost),
                None,
                Some(TowerType::Frost),
                None,
                None,
                None,
                Some(TowerType::Frost),
                None,
                Some(TowerType::Frost),
            ],
            CardRarity::Rare,
        ),
        (
            "Artillery Stack",
            vec![
                None,
                Some(TowerType::Cannon),
                None,
                None,
                Some(TowerType::Cannon),
                None,
                None,
                Some(TowerType::Cannon),
                None,
            ],
            CardRarity::Rare,
        ),
        (
            "Support Grid",
            vec![
                Some(TowerType::Basic),
                None,
                Some(TowerType::Basic),
                None,
                Some(TowerType::Cannon),
                None,
                Some(TowerType::Basic),
                None,
                Some(TowerType::Basic),
            ],
            CardRarity::Epic,
        ),
        (
            "Poison Triangle",
            vec![
                None,
                Some(TowerType::Poison),
                None,
                Some(TowerType::Poison),
                None,
                Some(TowerType::Poison),
                None,
                None,
                None,
            ],
            CardRarity::Rare,
        ),
        (
            "Sniper Nest",
            vec![
                Some(TowerType::Basic),
                Some(TowerType::Sniper),
                Some(TowerType::Basic),
                Some(TowerType::Basic),
                Some(TowerType::Sniper),
                Some(TowerType::Basic),
                None,
                None,
                None,
            ],
            CardRarity::Epic,
        ),
        (
            "Full House",
            vec![
                Some(TowerType::Basic),
                Some(TowerType::Frost),
                Some(TowerType::Basic),
                Some(TowerType::Cannon),
                Some(TowerType::Poison),
                Some(TowerType::Cannon),
                Some(TowerType::Basic),
                Some(TowerType::Frost),
                Some(TowerType::Basic),
            ],
            CardRarity::Legendary,
        ),
        (
            "Cross Formation",
            vec![
                None,
                Some(TowerType::Basic),
                None,
                Some(TowerType::Basic),
                Some(TowerType::Cannon),
                Some(TowerType::Basic),
                None,
                Some(TowerType::Basic),
                None,
            ],
            CardRarity::Common,
        ),
        (
            "Poison Wall",
            vec![
                Some(TowerType::Poison),
                Some(TowerType::Poison),
                Some(TowerType::Poison),
                Some(TowerType::Poison),
                Some(TowerType::Poison),
                Some(TowerType::Poison),
                None,
                None,
                None,
            ],
            CardRarity::Epic,
        ),
        (
            "Diagonal Strike",
            vec![
                Some(TowerType::Sniper),
                None,
                None,
                None,
                Some(TowerType::Sniper),
                None,
                None,
                None,
                Some(TowerType::Sniper),
            ],
            CardRarity::Rare,
        ),
        (
            "Frost Fortress",
            vec![
                Some(TowerType::Frost),
                Some(TowerType::Frost),
                Some(TowerType::Frost),
                Some(TowerType::Frost),
                Some(TowerType::Cannon),
                Some(TowerType::Frost),
                Some(TowerType::Frost),
                Some(TowerType::Frost),
                Some(TowerType::Frost),
            ],
            CardRarity::Legendary,
        ),
    ]
}

fn get_enemy_deck_for_encounter(layer: usize, is_elite: bool, is_boss: bool) -> EnemyDeck {
    if is_boss {
        EnemyDeck {
            encounter_name: "The Final Swarm".to_string(),
            cards: vec![
                EnemyCard {
                    enemy_types: vec![EnemyType::Normal, EnemyType::Normal, EnemyType::Fast],
                    count: 3,
                    play_delay: 2.0,
                },
                EnemyCard {
                    enemy_types: vec![EnemyType::Tank, EnemyType::Healer],
                    count: 2,
                    play_delay: 3.0,
                },
                EnemyCard {
                    enemy_types: vec![EnemyType::Flying, EnemyType::Flying, EnemyType::Fast],
                    count: 3,
                    play_delay: 2.5,
                },
                EnemyCard {
                    enemy_types: vec![EnemyType::Tank, EnemyType::Tank],
                    count: 2,
                    play_delay: 4.0,
                },
                EnemyCard {
                    enemy_types: vec![EnemyType::Healer, EnemyType::Normal, EnemyType::Normal],
                    count: 2,
                    play_delay: 3.0,
                },
            ],
        }
    } else if is_elite {
        let deck_choice = rand::gen_range(0, 3);
        match deck_choice {
            0 => EnemyDeck {
                encounter_name: "Aerial Assault".to_string(),
                cards: vec![
                    EnemyCard {
                        enemy_types: vec![EnemyType::Flying, EnemyType::Flying],
                        count: 2,
                        play_delay: 2.0,
                    },
                    EnemyCard {
                        enemy_types: vec![EnemyType::Fast, EnemyType::Flying],
                        count: 2,
                        play_delay: 2.5,
                    },
                    EnemyCard {
                        enemy_types: vec![EnemyType::Flying, EnemyType::Flying, EnemyType::Flying],
                        count: 2,
                        play_delay: 3.0,
                    },
                ],
            },
            1 => EnemyDeck {
                encounter_name: "Tank Battalion".to_string(),
                cards: vec![
                    EnemyCard {
                        enemy_types: vec![EnemyType::Tank],
                        count: 1,
                        play_delay: 2.0,
                    },
                    EnemyCard {
                        enemy_types: vec![EnemyType::Tank, EnemyType::Healer],
                        count: 2,
                        play_delay: 3.0,
                    },
                    EnemyCard {
                        enemy_types: vec![EnemyType::Tank, EnemyType::Tank],
                        count: 2,
                        play_delay: 4.0,
                    },
                ],
            },
            _ => EnemyDeck {
                encounter_name: "Speed Runners".to_string(),
                cards: vec![
                    EnemyCard {
                        enemy_types: vec![EnemyType::Fast, EnemyType::Fast, EnemyType::Fast],
                        count: 3,
                        play_delay: 1.5,
                    },
                    EnemyCard {
                        enemy_types: vec![EnemyType::Fast, EnemyType::Normal],
                        count: 2,
                        play_delay: 2.0,
                    },
                    EnemyCard {
                        enemy_types: vec![EnemyType::Fast, EnemyType::Fast],
                        count: 2,
                        play_delay: 1.8,
                    },
                ],
            },
        }
    } else {
        let difficulty_tier = (layer / 3).min(3);
        let deck_choice = rand::gen_range(0, 4);

        match (difficulty_tier, deck_choice) {
            (0, _) => EnemyDeck {
                encounter_name: "Scouting Party".to_string(),
                cards: vec![
                    EnemyCard {
                        enemy_types: vec![EnemyType::Normal, EnemyType::Normal],
                        count: 2,
                        play_delay: 2.0,
                    },
                    EnemyCard {
                        enemy_types: vec![EnemyType::Normal, EnemyType::Fast],
                        count: 2,
                        play_delay: 2.5,
                    },
                    EnemyCard {
                        enemy_types: vec![EnemyType::Normal],
                        count: 1,
                        play_delay: 1.5,
                    },
                ],
            },
            (1, 0) => EnemyDeck {
                encounter_name: "Mixed Squad".to_string(),
                cards: vec![
                    EnemyCard {
                        enemy_types: vec![EnemyType::Normal, EnemyType::Tank],
                        count: 2,
                        play_delay: 2.5,
                    },
                    EnemyCard {
                        enemy_types: vec![EnemyType::Fast, EnemyType::Fast],
                        count: 2,
                        play_delay: 2.0,
                    },
                    EnemyCard {
                        enemy_types: vec![EnemyType::Flying],
                        count: 1,
                        play_delay: 3.0,
                    },
                ],
            },
            (1, _) => EnemyDeck {
                encounter_name: "Reinforced Line".to_string(),
                cards: vec![
                    EnemyCard {
                        enemy_types: vec![EnemyType::Tank, EnemyType::Normal],
                        count: 2,
                        play_delay: 2.5,
                    },
                    EnemyCard {
                        enemy_types: vec![EnemyType::Healer, EnemyType::Normal, EnemyType::Normal],
                        count: 1,
                        play_delay: 3.0,
                    },
                    EnemyCard {
                        enemy_types: vec![EnemyType::Tank],
                        count: 1,
                        play_delay: 2.0,
                    },
                ],
            },
            (2, 0) => EnemyDeck {
                encounter_name: "Sky Raiders".to_string(),
                cards: vec![
                    EnemyCard {
                        enemy_types: vec![EnemyType::Flying, EnemyType::Fast],
                        count: 2,
                        play_delay: 2.0,
                    },
                    EnemyCard {
                        enemy_types: vec![EnemyType::Flying, EnemyType::Flying],
                        count: 2,
                        play_delay: 2.5,
                    },
                    EnemyCard {
                        enemy_types: vec![EnemyType::Tank, EnemyType::Flying],
                        count: 1,
                        play_delay: 3.0,
                    },
                ],
            },
            (2, _) => EnemyDeck {
                encounter_name: "Heavy Support".to_string(),
                cards: vec![
                    EnemyCard {
                        enemy_types: vec![EnemyType::Tank, EnemyType::Healer],
                        count: 2,
                        play_delay: 3.0,
                    },
                    EnemyCard {
                        enemy_types: vec![EnemyType::Fast, EnemyType::Fast, EnemyType::Healer],
                        count: 2,
                        play_delay: 2.5,
                    },
                    EnemyCard {
                        enemy_types: vec![EnemyType::Tank],
                        count: 1,
                        play_delay: 2.0,
                    },
                ],
            },
            _ => EnemyDeck {
                encounter_name: "Elite Vanguard".to_string(),
                cards: vec![
                    EnemyCard {
                        enemy_types: vec![EnemyType::Tank, EnemyType::Flying, EnemyType::Healer],
                        count: 2,
                        play_delay: 3.0,
                    },
                    EnemyCard {
                        enemy_types: vec![EnemyType::Fast, EnemyType::Flying],
                        count: 2,
                        play_delay: 2.0,
                    },
                    EnemyCard {
                        enemy_types: vec![EnemyType::Tank, EnemyType::Tank],
                        count: 1,
                        play_delay: 3.5,
                    },
                ],
            },
        }
    }
}

fn calculate_card_cost(
    pattern: &[Option<TowerType>],
    rarity: CardRarity,
    tower_configs: &TowerConfigs,
) -> u32 {
    let base_cost: u32 = pattern
        .iter()
        .filter_map(|&tower_type| tower_type.map(|t| tower_configs.get(t).cost))
        .sum();

    match rarity {
        CardRarity::Common => base_cost,
        CardRarity::Rare => (base_cost as f32 * 0.9) as u32,
        CardRarity::Epic => (base_cost as f32 * 0.85) as u32,
        CardRarity::Legendary => (base_cost as f32 * 0.75) as u32,
    }
}

fn create_card(
    world: &mut GameWorld,
    name: &str,
    pattern: Vec<Option<TowerType>>,
    rarity: CardRarity,
) {
    let discounted_cost =
        calculate_card_cost(&pattern, rarity, &world.resources.config.tower_configs);

    let entity = world.spawn_entities(CARD, 1)[0];
    world.set_card(
        entity,
        Card {
            card_index: 0,
            tower_pattern: pattern,
            cost: discounted_cost,
            rarity,
            name: name.to_string(),
            in_hand: false,
            card_state: CardState::InDeck,
            draw_animation_progress: 0.0,
            hand_position_index: 0,
        },
    );
}

fn create_starter_cards(world: &mut GameWorld) {
    let starter_cards = vec![
        (
            "Single Tower",
            vec![
                None,
                None,
                None,
                None,
                Some(TowerType::Basic),
                None,
                None,
                None,
                None,
            ],
            CardRarity::Common,
        ),
        (
            "Single Tower",
            vec![
                None,
                None,
                None,
                None,
                Some(TowerType::Basic),
                None,
                None,
                None,
                None,
            ],
            CardRarity::Common,
        ),
        (
            "Single Tower",
            vec![
                None,
                None,
                None,
                None,
                Some(TowerType::Basic),
                None,
                None,
                None,
                None,
            ],
            CardRarity::Common,
        ),
        (
            "Cross Formation",
            vec![
                None,
                Some(TowerType::Basic),
                None,
                Some(TowerType::Basic),
                Some(TowerType::Cannon),
                Some(TowerType::Basic),
                None,
                Some(TowerType::Basic),
                None,
            ],
            CardRarity::Common,
        ),
        (
            "Frost Line",
            vec![
                Some(TowerType::Frost),
                Some(TowerType::Frost),
                Some(TowerType::Frost),
                None,
                None,
                None,
                None,
                None,
                None,
            ],
            CardRarity::Common,
        ),
    ];

    for (name, pattern, rarity) in starter_cards {
        create_card(world, name, pattern, rarity);
    }
}

fn add_card_reward(world: &mut GameWorld, node_layer: usize) {
    let all_cards = get_all_card_definitions();

    let available_cards: Vec<_> = all_cards
        .iter()
        .filter(|(_, _, rarity)| match rarity {
            CardRarity::Common => true,
            CardRarity::Rare => node_layer >= 3,
            CardRarity::Epic => node_layer >= 7,
            CardRarity::Legendary => node_layer >= 12,
        })
        .collect();

    if !available_cards.is_empty() {
        let random_index = rand::gen_range(0, available_cards.len());
        let (name, pattern, rarity) = available_cards[random_index];
        create_card(world, name, pattern.clone(), *rarity);
    }
}

fn generate_map() -> MapData {
    let layers = 15;
    let mut map_data = MapData {
        nodes: Vec::new(),
        layers,
        nodes_per_layer: Vec::new(),
    };

    for layer in 0..layers {
        let nodes_in_layer = if layer == 0 {
            1
        } else if layer == layers - 1 {
            1
        } else {
            rand::gen_range(3, 6)
        };

        map_data.nodes_per_layer.push(nodes_in_layer);

        for position in 0..nodes_in_layer {
            let node_type = if layer == 0 {
                NodeType::Combat
            } else if layer == layers - 1 {
                NodeType::Boss
            } else if layer % 2 == 0 {
                if rand::gen_range(0, 100) < 20 {
                    NodeType::Elite
                } else {
                    NodeType::Combat
                }
            } else {
                let roll = rand::gen_range(0, 100);
                if roll < 40 {
                    NodeType::Shop
                } else if roll < 70 {
                    NodeType::Rest
                } else if roll < 85 {
                    NodeType::Forge
                } else {
                    NodeType::Combat
                }
            };

            let available = layer == 0;

            map_data.nodes.push(MapNode {
                node_type,
                layer,
                position_in_layer: position,
                connections: Vec::new(),
                visited: false,
                available,
            });
        }
    }

    let mut current_index = 0;
    for layer in 0..layers - 1 {
        let current_layer_size = map_data.nodes_per_layer[layer];
        let next_layer_size = map_data.nodes_per_layer[layer + 1];
        let next_layer_start = current_index + current_layer_size;

        for position in 0..current_layer_size {
            let node_idx = current_index + position;

            let connections_count = rand::gen_range(1, 3.min(next_layer_size + 1));

            let mut available_targets: Vec<usize> = (0..next_layer_size).collect();
            use macroquad::rand::ChooseRandom;
            available_targets.shuffle();

            for _ in 0..connections_count {
                if let Some(target_pos) = available_targets.pop() {
                    let target_idx = next_layer_start + target_pos;
                    if !map_data.nodes[node_idx].connections.contains(&target_idx) {
                        map_data.nodes[node_idx].connections.push(target_idx);
                    }
                }
            }
        }

        current_index += current_layer_size;
    }

    validate_map_connectivity(&mut map_data);
    map_data
}

fn validate_map_connectivity(map_data: &mut MapData) {
    use std::collections::VecDeque;

    let boss_index = map_data.nodes.len() - 1;

    let mut queue = VecDeque::new();
    let mut reachable = vec![false; map_data.nodes.len()];

    for (index, node) in map_data.nodes.iter().enumerate() {
        if node.layer == 0 {
            queue.push_back(index);
            reachable[index] = true;
        }
    }

    while let Some(current) = queue.pop_front() {
        for &next in &map_data.nodes[current].connections {
            if !reachable[next] {
                reachable[next] = true;
                queue.push_back(next);
            }
        }
    }

    if !reachable[boss_index] {
        let mut current_index = 0;
        for layer in 0..map_data.layers - 1 {
            let current_layer_size = map_data.nodes_per_layer[layer];
            let next_layer_size = map_data.nodes_per_layer[layer + 1];
            let next_layer_start = current_index + current_layer_size;

            for position in 0..current_layer_size {
                let node_idx = current_index + position;

                if reachable[node_idx] {
                    for target_pos in 0..next_layer_size {
                        let target_idx = next_layer_start + target_pos;
                        if !map_data.nodes[node_idx].connections.contains(&target_idx) {
                            map_data.nodes[node_idx].connections.push(target_idx);
                            break;
                        }
                    }
                }
            }

            current_index += current_layer_size;
        }
    }
}

fn draw_random_cards(world: &mut GameWorld, count: usize) {
    let mut deck_cards: Vec<_> = world
        .query_entities(CARD)
        .filter_map(|entity| {
            world.get_card(entity).and_then(|card| {
                if card.card_state == CardState::InDeck {
                    Some(entity)
                } else {
                    None
                }
            })
        })
        .collect();

    let mut drawn = 0;
    for _ in 0..count {
        if drawn >= count
            || world.resources.card_system.hand_size >= world.resources.card_system.max_hand_size
        {
            break;
        }

        if !deck_cards.is_empty() {
            let random_index = rand::gen_range(0, deck_cards.len());
            let card_entity = deck_cards.remove(random_index);

            let current_hand_size = world.resources.card_system.hand_size;
            if let Some(card) = world.get_card_mut(card_entity) {
                card.card_state = CardState::Drawing;
                card.draw_animation_progress = 0.0;
                card.hand_position_index = current_hand_size;
                world.resources.card_system.hand_size += 1;
                drawn += 1;
            }
        }
    }
}

fn update_card_animations(world: &mut GameWorld, delta_time: f32) {
    let card_entities: Vec<_> = world.query_entities(CARD).collect();

    for entity in card_entities {
        if let Some(card) = world.get_card_mut(entity) {
            if card.card_state == CardState::Drawing {
                card.draw_animation_progress += delta_time * 3.0;

                if card.draw_animation_progress >= 1.0 {
                    card.draw_animation_progress = 1.0;
                    card.card_state = CardState::InHand;
                    card.in_hand = true;
                }
            }
        }
    }
}

fn update_card_animations_wrapper(world: &mut GameWorld) {
    let delta_time = get_frame_time() * world.resources.combat.game_speed;
    update_card_animations(world, delta_time);
}

fn get_card_placements(
    center_x: i32,
    center_y: i32,
    pattern: &[Option<TowerType>],
) -> Vec<(i32, i32, TowerType)> {
    let mut placements = Vec::new();

    for row in 0..3 {
        for col in 0..3 {
            let pattern_index = row * 3 + col;
            if let Some(Some(tower_type)) = pattern.get(pattern_index) {
                let offset_x = col as i32 - 1;
                let offset_y = row as i32 - 1;
                placements.push((center_x + offset_x, center_y + offset_y, *tower_type));
            }
        }
    }

    placements
}

fn can_place_tower_at(world: &GameWorld, x: i32, y: i32) -> bool {
    let mut has_tower = false;
    world
        .query()
        .with(TOWER | GRID_POSITION)
        .iter(|_entity, table, index| {
            if table.grid_position[index].coord.x == x && table.grid_position[index].coord.y == y {
                has_tower = true;
            }
        });

    if has_tower {
        return false;
    }

    let mut can_place = false;
    world.query().with(GRID_CELL).iter(|_entity, table, index| {
        if table.grid_cell[index].coord.x == x
            && table.grid_cell[index].coord.y == y
            && !table.grid_cell[index].occupied
        {
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
            if table.grid_cell[index].coord.x == x && table.grid_cell[index].coord.y == y {
                table.grid_cell[index].occupied = true;
            }
        });
}

fn mouse_input_system(world: &mut GameWorld) {
    let mouse_pos = Vec2::new(mouse_position().0, mouse_position().1);
    world.resources.ui.mouse_grid_pos = screen_to_grid(mouse_pos);

    if is_mouse_button_pressed(MouseButton::Left) {
        if world.resources.combat.game_state == GameState::WaitingForWave
            && world.resources.combat.wave == 0
        {
            let button_width = 300.0;
            let button_height = 80.0;
            let button_x = (screen_width() - button_width) / 2.0;
            let button_y = screen_height() / 2.0 - button_height / 2.0;

            if mouse_pos.x >= button_x
                && mouse_pos.x <= button_x + button_width
                && mouse_pos.y >= button_y
                && mouse_pos.y <= button_y + button_height
            {
                let _ = CommandExecutor::execute(GameCommand::StartWave, world);
            }
        }
    }

    if world.resources.combat.game_state == GameState::WaitingForWave
        && world.resources.combat.wave > 0
        && world.resources.combat.wave < 5
    {
        let _ = CommandExecutor::execute(GameCommand::StartWave, world);
    }
}

fn card_interaction_system(world: &mut GameWorld) {
    let mouse_pos = Vec2::new(mouse_position().0, mouse_position().1);

    let left_clicked = is_mouse_button_pressed(MouseButton::Left);

    if left_clicked {
        let card_width = 120.0;
        let card_height = 140.0;
        let card_spacing = 10.0;
        let card_y = screen_height() - card_height - 40.0;

        let cards: Vec<_> = world
            .query_entities(CARD)
            .filter_map(|entity| {
                world.get_card(entity).and_then(|card| {
                    if card.in_hand {
                        Some((entity, card.clone()))
                    } else {
                        None
                    }
                })
            })
            .collect();

        let mut clicked_card = None;
        for (card_index, (card_entity, card)) in cards.iter().enumerate() {
            let card_x = 10.0 + card_index as f32 * (card_width + card_spacing);

            if mouse_pos.x >= card_x
                && mouse_pos.x <= card_x + card_width
                && mouse_pos.y >= card_y
                && mouse_pos.y <= card_y + card_height
            {
                if world.resources.economy.money >= card.cost {
                    clicked_card = Some((card_index, *card_entity));
                }
                break;
            }
        }

        if let Some((card_index, _card_entity)) = clicked_card {
            if world.resources.card_system.selected_card == Some(card_index) {
                world.resources.card_system.selected_card = None;
            } else {
                world.resources.card_system.selected_card = Some(card_index);
            }
        } else if let Some((grid_x, grid_y)) = world.resources.ui.mouse_grid_pos {
            if let Some(selected_card_index) = world.resources.card_system.selected_card {
                if let Some((card_entity, card)) = cards.get(selected_card_index) {
                    if world.resources.economy.money >= card.cost {
                        let mut all_placeable = true;
                        let placements = get_card_placements(grid_x, grid_y, &card.tower_pattern);

                        for (check_x, check_y, _tower_type) in &placements {
                            if !can_place_tower_at(world, *check_x, *check_y) {
                                all_placeable = false;
                                break;
                            }
                        }

                        if all_placeable && !placements.is_empty() {
                            for (place_x, place_y, tower_type) in placements {
                                spawn_tower(world, place_x, place_y, tower_type);
                                mark_cell_occupied(world, place_x, place_y);
                            }

                            world.resources.economy.money =
                                world.resources.economy.money.saturating_sub(card.cost);
                            let pos = grid_to_base(grid_x, grid_y);
                            spawn_money_popup(world, pos, -(card.cost as i32));
                            world.resources.card_system.selected_card = None;

                            if let Some(used_card) = world.get_card_mut(*card_entity) {
                                used_card.in_hand = false;
                                used_card.card_state = CardState::InDeck;
                                used_card.draw_animation_progress = 0.0;
                                world.resources.card_system.hand_size =
                                    world.resources.card_system.hand_size.saturating_sub(1);
                            }

                            draw_random_cards(world, 1);
                        }
                    }
                }
            }
        }
    }
}

fn tower_interaction_system(world: &mut GameWorld) {
    let right_clicked = is_mouse_button_pressed(MouseButton::Right);
    let upgrade_pressed =
        is_key_pressed(KeyCode::U) || is_mouse_button_pressed(MouseButton::Middle);

    if right_clicked {
        if let Some((grid_x, grid_y)) = world.resources.ui.mouse_grid_pos {
            let mut tower_entity = None;
            world
                .query()
                .with(TOWER | GRID_POSITION)
                .iter(|entity, table, index| {
                    if table.grid_position[index].coord.x == grid_x
                        && table.grid_position[index].coord.y == grid_y
                    {
                        tower_entity = Some(entity);
                    }
                });

            if let Some(tower_entity) = tower_entity {
                sell_tower(world, tower_entity, grid_x, grid_y);
            }
        }
    }

    if upgrade_pressed {
        if let Some((grid_x, grid_y)) = world.resources.ui.mouse_grid_pos {
            let mut tower_entity = None;
            world
                .query()
                .with(TOWER | GRID_POSITION)
                .iter(|entity, table, index| {
                    if table.grid_position[index].coord.x == grid_x
                        && table.grid_position[index].coord.y == grid_y
                    {
                        tower_entity = Some(entity);
                    }
                });

            if let Some(tower_entity) = tower_entity {
                upgrade_tower(world, tower_entity, grid_x, grid_y);
            }
        }
    }
}

fn keyboard_input_system(world: &mut GameWorld) {
    if is_key_pressed(KeyCode::LeftBracket) {
        world.resources.combat.game_speed = (world.resources.combat.game_speed - 0.5).max(0.5);
    } else if is_key_pressed(KeyCode::RightBracket) {
        world.resources.combat.game_speed = (world.resources.combat.game_speed + 0.5).min(3.0);
    } else if is_key_pressed(KeyCode::Backslash) {
        world.resources.combat.game_speed = 1.0;
    }

    if is_key_pressed(KeyCode::P) {
        match world.resources.combat.game_state {
            GameState::WaveInProgress => world.resources.combat.game_state = GameState::Paused,
            GameState::Paused => world.resources.combat.game_state = GameState::WaveInProgress,
            _ => {}
        }
    }

    if is_key_pressed(KeyCode::R) {
        if matches!(
            world.resources.combat.game_state,
            GameState::GameOver | GameState::Victory
        ) {
            restart_game(world);
        }
    }

    if is_key_pressed(KeyCode::D) {
        draw_random_cards(world, 1);
    }

    if is_key_pressed(KeyCode::V) {
        world.resources.meta_game.previous_state = world.resources.combat.game_state;
        world.resources.combat.game_state = GameState::DeckView;
    }
}

fn input_system(world: &mut GameWorld) {
    update_mouse_grid_position(world);
    mouse_input_system(world);
    keyboard_input_system(world);
    card_interaction_system(world);
    tower_interaction_system(world);
}

fn update_mouse_grid_position(world: &mut GameWorld) {
    let mouse_pos = Vec2::new(mouse_position().0, mouse_position().1);
    world.resources.ui.mouse_grid_pos = screen_to_grid(mouse_pos);
}

fn victory_timer_system(world: &mut GameWorld, delta_time: f32) {
    if world.resources.combat.game_state == GameState::Victory {
        world.resources.combat.victory_timer -= delta_time;
        if world.resources.combat.victory_timer <= 0.0 {
            world.resources.combat.game_state = GameState::Map;
        }
    }

    if world.resources.combat.wave_announce_timer > 0.0 {
        world.resources.combat.wave_announce_timer -= delta_time;
    }
}

fn enemy_deck_system(world: &mut GameWorld, delta_time: f32) {
    if world.resources.combat.game_state != GameState::WaveInProgress {
        return;
    }

    world.resources.combat.enemy_deck_play_timer += delta_time;

    let cards_to_play: Vec<_> = world
        .resources
        .combat
        .enemy_deck
        .iter()
        .filter(|card| card.play_delay <= world.resources.combat.enemy_deck_play_timer)
        .cloned()
        .collect();

    for card in &cards_to_play {
        for _ in 0..card.count {
            for enemy_type in &card.enemy_types {
                spawn_enemy(world, *enemy_type);
            }
        }
    }

    world
        .resources
        .combat
        .enemy_deck
        .retain(|card| card.play_delay > world.resources.combat.enemy_deck_play_timer);

    let enemy_count = world.query_entities(ENEMY).count();

    if world.resources.combat.enemy_deck.is_empty() && enemy_count == 0 {
        world.send_wave_completed(WaveCompletedEvent {
            wave: world.resources.combat.wave,
        });

        draw_random_cards(world, 2);

        if world.resources.combat.wave >= 5 {
            cleanup_combat_effects(world);

            let current_node = world.resources.meta_game.current_node;
            let (is_boss_node, node_layer) = if current_node != usize::MAX {
                let node = &world.resources.meta_game.map_data.nodes[current_node];
                (matches!(node.node_type, NodeType::Boss), node.layer)
            } else {
                (false, 0)
            };

            if current_node != usize::MAX {
                add_card_reward(world, node_layer);
            }

            if is_boss_node {
                world.resources.combat.game_state = GameState::Victory;
                world.resources.combat.victory_timer = 3.0;
            } else {
                world.resources.combat.game_state = GameState::Map;
            }
        } else {
            world.resources.combat.game_state = GameState::WaitingForWave;
        }
    }
}

fn enemy_movement_system(world: &mut GameWorld, delta_time: f32) {
    let path = world.resources.combat.path.clone();
    let mut enemies_to_remove = Vec::new();
    let mut hp_damage = 0;

    let mut healer_data = Vec::new();
    world
        .query()
        .with(HEAL_AURA | POSITION)
        .iter(|entity, table, index| {
            let heal_aura = table.heal_aura[index];
            let position = table.position[index].0;
            healer_data.push((entity, heal_aura, position));
        });

    for (healer_entity, mut heal_aura, healer_pos) in healer_data {
        heal_aura.cooldown -= delta_time;

        if heal_aura.cooldown <= 0.0 {
            let mut nearby_enemies = Vec::new();
            world
                .query()
                .with(ENEMY | POSITION | HEALTH)
                .iter(|entity, table, index| {
                    if entity != healer_entity {
                        let enemy_pos = table.position[index].0;
                        let distance = (enemy_pos - healer_pos).length();
                        if distance < heal_aura.range {
                            let health = &table.health[index];
                            if health.current < health.maximum {
                                nearby_enemies.push(entity);
                            }
                        }
                    }
                });

            if !nearby_enemies.is_empty() {
                let random_index = rand::gen_range(0, nearby_enemies.len());
                let target = nearby_enemies[random_index];

                if let Some(health) = world.get_health_mut(target) {
                    health.heal(heal_aura.heal_amount);
                }

                heal_aura.cooldown = 2.0;
            }
        }

        world.set_heal_aura(healer_entity, heal_aura);
    }

    let mut enemy_updates = Vec::new();
    world
        .query()
        .with(ENEMY | POSITION | PATH_FOLLOWER | SPEED | STATUS_EFFECTS)
        .iter(|entity, table, index| {
            enemy_updates.push((
                entity,
                table.path_follower[index],
                table.speed[index],
                table.status_effects[index],
            ));
        });

    for (entity, path_follower, speed_comp, status_effects) in enemy_updates {
        let mut path_index = path_follower.path_index;
        let mut path_progress = path_follower.path_progress;

        let speed_multiplier = if status_effects.slow_duration > 0.0 {
            0.5
        } else {
            1.0
        };
        let speed = speed_comp.base_speed * speed_multiplier;

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
                    world.send_enemy_reached_end(EnemyReachedEndEvent { entity, damage: 1 });
                    continue;
                }
            }

            let current = path[path_index];
            let next = path[path_index + 1];
            let direction = (next - current).normalize();
            let base_position = current + direction * path_progress;

            let mut poison_death = false;

            if let Some(path_follower_mut) = world.get_path_follower_mut(entity) {
                path_follower_mut.path_index = path_index;
                path_follower_mut.path_progress = path_progress;
            }

            let mut poison_damage_to_apply = 0.0;

            if let Some(effects_mut) = world.get_status_effects_mut(entity) {
                if effects_mut.slow_duration > 0.0 {
                    effects_mut.slow_duration -= delta_time;
                }

                if effects_mut.poison_duration > 0.0 {
                    effects_mut.poison_duration -= delta_time;
                    poison_damage_to_apply = effects_mut.poison_damage_per_second * delta_time;
                }
            }

            if poison_damage_to_apply > 0.0 {
                if let Some(health) = world.get_health_mut(entity) {
                    health.apply_damage(poison_damage_to_apply);

                    if !health.is_alive() {
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
        if let Some(player_entity) = world.query_entities(PLAYER).next() {
            if let Some(player) = world.get_player_mut(player_entity) {
                if player.health >= hp_damage {
                    player.health -= hp_damage;
                } else {
                    player.health = 0;
                }

                if player.health == 0 {
                    player.health = player.max_health;
                    world.resources.combat.lives = world.resources.combat.lives.saturating_sub(1);

                    if world.resources.combat.lives == 0 {
                        world.resources.combat.game_state = GameState::GameOver;
                    }
                }
            }
        }
    }

    for entity in enemies_to_remove {
        if let Some(enemy_data) = world.get_enemy_data(entity) {
            world.resources.economy.money += enemy_data.reward_value;
        }
        world.queue_despawn_entity(entity);
    }

    world.apply_commands();
}

fn tower_targeting_system(world: &mut GameWorld) {
    let mut enemy_data = Vec::new();
    world
        .query()
        .with(ENEMY | POSITION | ENEMY_DATA)
        .iter(|entity, table, index| {
            enemy_data.push((
                entity,
                table.position[index].0,
                table.enemy_data[index].is_flying,
            ));
        });

    let tower_entities: Vec<_> = world.query_entities(TOWER | POSITION).collect();
    for tower_entity in tower_entities {
        let tower_data = world.get_tower(tower_entity).unwrap();
        let tower_pos = world.get_position(tower_entity).unwrap().0;
        let tower_stats = world
            .resources
            .config
            .tower_configs
            .get(tower_data.tower_type);
        let mut range = tower_stats.range(tower_data.level);

        if world
            .resources
            .economy
            .owned_relics
            .contains(&RelicType::RangeExtender)
        {
            range *= 1.2;
        }

        if tower_data.tower_type == TowerType::Sniper
            && world
                .resources
                .economy
                .owned_relics
                .contains(&RelicType::SniperNest)
        {
            range *= 1.5;
        }

        let range_squared = range * range;

        let mut closest_enemy_pos = None;
        let mut closest_distance = f32::MAX;

        for &(_enemy_entity, enemy_pos, _is_flying) in &enemy_data {
            let distance_squared = (enemy_pos - tower_pos).length_squared();
            if distance_squared <= range_squared && distance_squared < closest_distance {
                closest_distance = distance_squared;
                closest_enemy_pos = Some(enemy_pos);
            }
        }

        if let Some(targeting) = world.get_targeting_mut(tower_entity) {
            targeting.target_position = closest_enemy_pos;
            if targeting.target_position.is_some() {
                targeting.tracking_time += get_frame_time();
            } else {
                targeting.tracking_time = 0.0;
            }
        }
    }
}

fn tower_shooting_system(world: &mut GameWorld, delta_time: f32) {
    let mut projectiles_to_spawn = Vec::new();

    let has_double_tap = world
        .resources
        .economy
        .owned_relics
        .contains(&RelicType::DoubleTap);
    let has_rapid_fire = world
        .resources
        .economy
        .owned_relics
        .contains(&RelicType::RapidFire);

    let tower_entities: Vec<_> = world
        .query_entities(TOWER | POSITION | COOLDOWN | ANIMATION | TARGETING)
        .collect();
    for entity in tower_entities {
        let tower_pos = world.get_position(entity).unwrap().0;
        let tower_type = world.get_tower(entity).unwrap().tower_type;
        let tower_level = world.get_tower(entity).unwrap().level;
        let target_position = world.get_targeting(entity).unwrap().target_position;
        let tracking_time = world.get_targeting(entity).unwrap().tracking_time;

        if let Some(cooldown) = world.get_cooldown_mut(entity) {
            cooldown.tick(delta_time);
        }

        if let Some(animation) = world.get_animation_mut(entity) {
            animation.tick(delta_time, 3.0);
        }

        let is_ready = world.get_cooldown(entity).unwrap().is_ready();

        if is_ready && target_position.is_some() {
            let can_fire = if tower_type == TowerType::Sniper {
                tracking_time >= 2.0
            } else {
                true
            };

            if can_fire {
                projectiles_to_spawn.push((
                    tower_pos,
                    target_position.unwrap(),
                    tower_type,
                    tower_level,
                ));

                if has_double_tap {
                    projectiles_to_spawn.push((
                        tower_pos,
                        target_position.unwrap(),
                        tower_type,
                        tower_level,
                    ));
                }

                let tower_stats = world.resources.config.tower_configs.get(tower_type);
                let mut fire_rate = tower_stats.fire_rate(tower_level);
                if has_rapid_fire {
                    fire_rate *= 0.7;
                }

                if let Some(cooldown_mut) = world.get_cooldown_mut(entity) {
                    cooldown_mut.set_duration(fire_rate);
                    cooldown_mut.reset();
                }

                if let Some(animation_mut) = world.get_animation_mut(entity) {
                    animation_mut.trigger();
                }

                if let Some(targeting_mut) = world.get_targeting_mut(entity) {
                    targeting_mut.tracking_time = 0.0;
                }
            }
        }
    }

    for (from, target, tower_type, level) in projectiles_to_spawn {
        spawn_projectile(world, from, target, tower_type, level);

        if tower_type == TowerType::Cannon {
            for _ in 0..6 {
                let offset = Vec2::new(rand::gen_range(-5.0, 5.0), rand::gen_range(-5.0, 5.0));
                spawn_visual_effect(world, from + offset, EffectType::Explosion, Vec2::ZERO, 0.3);
            }
        }
    }
}

fn projectile_movement_system(world: &mut GameWorld, delta_time: f32) {
    let mut projectiles_to_remove = Vec::new();
    let mut hits = Vec::new();

    let mut enemy_data = Vec::new();
    world
        .query()
        .with(ENEMY | POSITION)
        .iter(|entity, table, index| {
            enemy_data.push((entity, table.position[index].0));
        });

    let projectile_entities: Vec<_> = world.query_entities(PROJECTILE | POSITION).collect();
    for projectile_entity in projectile_entities {
        let mut projectile_comp = *world.get_projectile(projectile_entity).unwrap();
        let old_pos = world.get_position(projectile_entity).unwrap().0;

        let target_pos = projectile_comp.target_position;
        let total_distance = (target_pos - projectile_comp.start_position).length();
        let distance_to_target = (target_pos - old_pos).length();

        let new_pos = if projectile_comp.arc_height > 0.0 {
            projectile_comp.flight_progress +=
                (projectile_comp.speed * delta_time) / total_distance;
            projectile_comp.flight_progress = projectile_comp.flight_progress.min(1.0);

            let horizontal_pos = projectile_comp.start_position
                + (target_pos - projectile_comp.start_position) * projectile_comp.flight_progress;
            horizontal_pos
        } else {
            let direction = (target_pos - old_pos).normalize();
            old_pos + direction * projectile_comp.speed * delta_time
        };

        if distance_to_target < 10.0 || projectile_comp.flight_progress >= 1.0 {
            let mut hit_enemy = None;
            let mut closest_distance = f32::MAX;
            for &(enemy_entity, enemy_pos) in &enemy_data {
                let distance = (enemy_pos - target_pos).length();
                if distance < 20.0 && distance < closest_distance {
                    closest_distance = distance;
                    hit_enemy = Some((enemy_entity, enemy_pos));
                }
            }

            if let Some((enemy_entity, enemy_pos)) = hit_enemy {
                hits.push((
                    enemy_entity,
                    projectile_comp.damage,
                    projectile_comp.tower_type,
                    enemy_pos,
                ));
                world.send_projectile_hit(ProjectileHitEvent {
                    projectile: projectile_entity,
                    target: enemy_entity,
                    position: enemy_pos,
                    damage: projectile_comp.damage,
                    tower_type: projectile_comp.tower_type,
                });
            }
            projectiles_to_remove.push(projectile_entity);
        } else {
            if let Some(projectile) = world.get_projectile_mut(projectile_entity) {
                projectile.flight_progress = projectile_comp.flight_progress;
            }

            if let Some(pos) = world.get_position_mut(projectile_entity) {
                pos.0 = new_pos;
            }
        }
    }

    for (enemy_entity, damage, tower_type, hit_pos) in hits {
        if world.get_enemy_data(enemy_entity).is_none() {
            continue;
        }

        match tower_type {
            TowerType::Frost => {
                if let Some(effects) = world.get_status_effects_mut(enemy_entity) {
                    effects.slow_duration = 2.0;
                    effects.slow_factor = 0.5;
                }
                world.send_damage(DamageEvent {
                    target: enemy_entity,
                    damage,
                });
            }
            TowerType::Poison => {
                if let Some(effects) = world.get_status_effects_mut(enemy_entity) {
                    effects.poison_duration = 3.0;
                    effects.poison_damage_per_second = 5.0;
                }
                world.send_damage(DamageEvent {
                    target: enemy_entity,
                    damage,
                });

                for _ in 0..3 {
                    let velocity =
                        Vec2::new(rand::gen_range(-20.0, 20.0), rand::gen_range(-20.0, 20.0));
                    spawn_visual_effect(world, hit_pos, EffectType::PoisonBubble, velocity, 2.0);
                }
            }
            TowerType::Cannon => {
                for _ in 0..8 {
                    let velocity =
                        Vec2::new(rand::gen_range(-30.0, 30.0), rand::gen_range(-30.0, 30.0));
                    spawn_visual_effect(world, hit_pos, EffectType::Explosion, velocity, 0.5);
                }

                for &(enemy_entity, enemy_pos) in &enemy_data {
                    let distance = (enemy_pos - hit_pos).length();
                    if distance < 60.0 {
                        let damage_falloff = 1.0 - (distance / 60.0);
                        world.send_damage(DamageEvent {
                            target: enemy_entity,
                            damage: damage * damage_falloff,
                        });
                    }
                }
            }
            _ => {
                world.send_damage(DamageEvent {
                    target: enemy_entity,
                    damage,
                });
            }
        }
    }

    for entity in projectiles_to_remove {
        world.queue_despawn_entity(entity);
    }

    world.apply_commands();
}

fn damage_handler_system(world: &mut GameWorld) {
    let has_overcharge = world
        .resources
        .economy
        .owned_relics
        .contains(&RelicType::Overcharge);
    let has_critical_strike = world
        .resources
        .economy
        .owned_relics
        .contains(&RelicType::CriticalStrike);
    let has_golden_touch = world
        .resources
        .economy
        .owned_relics
        .contains(&RelicType::GoldenTouch);

    for event in world.collect_damage() {
        let mut should_remove = false;
        let mut death_pos = Vec2::ZERO;
        let mut money_earned = 0;
        let mut enemy_type = EnemyType::Normal;

        if let Some(health) = world.get_health_mut(event.target) {
            let was_alive = health.is_alive();

            let mut damage = event.damage;

            if has_overcharge {
                damage *= 1.25;
            }

            if has_critical_strike {
                if rand::gen_range(0.0, 1.0) < 0.2 {
                    damage *= 2.0;
                }
            }

            health.apply_damage(damage);

            if was_alive && !health.is_alive() {
                if let Some(enemy_data) = world.get_enemy_data(event.target) {
                    money_earned = enemy_data.reward_value;
                    if has_golden_touch {
                        money_earned = (money_earned as f32 * 1.5) as u32;
                    }
                    enemy_type = enemy_data.enemy_type;
                }
                should_remove = true;
            }
        }

        if should_remove {
            if let Some(pos) = world.get_position(event.target) {
                death_pos = pos.0;
            }

            world.send_enemy_died(EnemyDiedEvent {
                entity: event.target,
                position: death_pos,
                reward: money_earned,
                enemy_type,
            });

            world.queue_despawn_entity(event.target);
        }
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

fn upgrade_tower(
    world: &mut GameWorld,
    tower_entity: freecs::Entity,
    grid_x: i32,
    grid_y: i32,
) -> bool {
    if let Some(tower) = world.get_tower(tower_entity) {
        let current_level = tower.level;
        if current_level >= 4 {
            return false;
        }

        let tower_stats = world.resources.config.tower_configs.get(tower.tower_type);
        let upgrade_cost = tower_stats.upgrade_cost(current_level);
        if world.resources.economy.money < upgrade_cost {
            return false;
        }

        let tower_type = tower.tower_type;
        world.resources.economy.money -= upgrade_cost;

        if let Some(tower) = world.get_tower_mut(tower_entity) {
            tower.level += 1;
            let new_level = tower.level;

            world.send_tower_upgraded(TowerUpgradedEvent {
                entity: tower_entity,
                tower_type,
                old_level: current_level,
                new_level,
                cost: upgrade_cost,
            });

            let position = grid_to_base(grid_x, grid_y);
            spawn_money_popup(world, position, -(upgrade_cost as i32));

            return true;
        }
    }
    false
}

fn sell_tower(world: &mut GameWorld, tower_entity: freecs::Entity, grid_x: i32, grid_y: i32) {
    if let Some(tower) = world.get_tower(tower_entity) {
        let tower_type = tower.tower_type;
        let tower_stats = world.resources.config.tower_configs.get(tower_type);
        let refund = (tower_stats.cost as f32 * 0.7) as u32;
        world.resources.economy.money += refund;

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
                if table.grid_cell[index].coord.x == grid_x
                    && table.grid_cell[index].coord.y == grid_y
                {
                    table.grid_cell[index].occupied = false;
                }
            });

        let grid_coord = GridCoord::new(grid_x, grid_y);
        let range_indicators_to_remove: Vec<_> = world
            .query_entities(RANGE_INDICATOR)
            .into_iter()
            .filter_map(|range_entity| {
                world
                    .get_range_indicator(range_entity)
                    .filter(|indicator| indicator.tower_grid_coord == grid_coord)
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

    let range_indicators_to_remove: Vec<_> =
        world.query_entities(RANGE_INDICATOR).into_iter().collect();
    for entity in range_indicators_to_remove {
        world.queue_despawn_entity(entity);
    }

    world.apply_commands();

    world.resources.economy.money = 500;
    world.resources.combat.lives = 1;
    world.resources.combat.wave = 0;
    world.resources.combat.current_hp = 20;
    world.resources.combat.max_hp = 20;

    if let Some(player_entity) = world.query_entities(PLAYER).next() {
        if let Some(player) = world.get_player_mut(player_entity) {
            player.health = 20;
            player.max_health = 20;
        }
    }

    world.resources.combat.game_state = GameState::WaitingForWave;
    world.resources.combat.game_speed = 1.0;
    world.resources.combat.spawn_timer = 0.0;
    world.resources.combat.wave_announce_timer = 0.0;
}

fn render_grid(world: &GameWorld) {
    let scale = get_scale();
    let offset = get_offset();

    for entity in world.query_entities(GRID_CELL) {
        if let Some(cell) = world.get_grid_cell(entity) {
            let base_pos = grid_to_base(cell.coord.x, cell.coord.y);
            let pos = Vec2::new(offset.x + base_pos.x * scale, offset.y + base_pos.y * scale);

            let path_start = Vec2::new(
                offset.x + world.resources.combat.path[0].x * scale,
                offset.y + world.resources.combat.path[0].y * scale,
            );
            let path_end = Vec2::new(
                offset.x + world.resources.combat.path.last().unwrap().x * scale,
                offset.y + world.resources.combat.path.last().unwrap().y * scale,
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

    if let Some((grid_x, grid_y)) = world.resources.ui.mouse_grid_pos {
        if let Some(selected_card_index) = world.resources.card_system.selected_card {
            let cards_in_hand: Vec<_> = world
                .query_entities(CARD)
                .filter_map(|entity| {
                    world.get_card(entity).and_then(|card| {
                        if card.card_state == CardState::InHand {
                            Some((entity, card.clone()))
                        } else {
                            None
                        }
                    })
                })
                .collect();

            if let Some((_entity, card)) = cards_in_hand.get(selected_card_index) {
                let placements = get_card_placements(grid_x, grid_y, &card.tower_pattern);
                let mut all_placeable = true;

                for (check_x, check_y, _tower_type) in &placements {
                    if !can_place_tower_at(world, *check_x, *check_y) {
                        all_placeable = false;
                        break;
                    }
                }

                for (place_x, place_y, tower_type) in &placements {
                    let pos = grid_to_screen(*place_x, *place_y);

                    if all_placeable {
                        let tower_size = 20.0 * scale;
                        let tower_color = tower_type.color();
                        let preview_color =
                            Color::new(tower_color.r, tower_color.g, tower_color.b, 0.5);

                        draw_circle(pos.x, pos.y, tower_size / 2.0, preview_color);
                        draw_circle_lines(
                            pos.x,
                            pos.y,
                            tower_size / 2.0,
                            2.0,
                            Color::new(tower_color.r, tower_color.g, tower_color.b, 0.7),
                        );

                        let tower_stats = world.resources.config.tower_configs.get(*tower_type);
                        let range = tower_stats.range(1);
                        draw_circle_lines(
                            pos.x,
                            pos.y,
                            range * scale,
                            1.5,
                            Color::new(tower_color.r, tower_color.g, tower_color.b, 0.3),
                        );
                    } else {
                        let preview_color = Color::new(1.0, 0.0, 0.0, 0.3);
                        draw_rectangle(
                            pos.x - TILE_SIZE * scale / 2.0 + scale,
                            pos.y - TILE_SIZE * scale / 2.0 + scale,
                            (TILE_SIZE - 2.0) * scale,
                            (TILE_SIZE - 2.0) * scale,
                            preview_color,
                        );
                    }
                }
            }
        }
    }
}

fn render_towers(world: &GameWorld) {
    let scale = get_scale();
    let offset = get_offset();

    world
        .query()
        .with(TOWER | POSITION | ANIMATION | TARGETING)
        .iter(|_entity, table, index| {
            let tower = &table.tower[index];
            let animation = &table.animation[index];
            let targeting = &table.targeting[index];
            let pos = &table.position[index];
            let screen_pos = Vec2::new(offset.x + pos.0.x * scale, offset.y + pos.0.y * scale);

            let base_size = 20.0 + animation.fire_animation * 4.0;
            let size = base_size * (1.0 + 0.15 * (tower.level - 1) as f32) * scale;

            let color = tower.tower_type.color();
            let level_brightness = 1.0 + 0.2 * (tower.level - 1) as f32;
            let upgraded_color = Color::new(
                (color.r * level_brightness).min(1.0),
                (color.g * level_brightness).min(1.0),
                (color.b * level_brightness).min(1.0),
                color.a,
            );

            draw_circle(screen_pos.x, screen_pos.y, size / 2.0, upgraded_color);
            draw_circle_lines(screen_pos.x, screen_pos.y, size / 2.0, 2.0, BLACK);

            for ring in 1..tower.level {
                let ring_radius = size / 2.0 + ring as f32 * 3.0 * scale;
                draw_circle_lines(screen_pos.x, screen_pos.y, ring_radius, 1.5, upgraded_color);
            }

            if tower.tower_type == TowerType::Sniper {
                if let Some(target_pos) = targeting.target_position {
                    let target_screen_pos = Vec2::new(
                        offset.x + target_pos.x * scale,
                        offset.y + target_pos.y * scale,
                    );
                    draw_line(
                        screen_pos.x,
                        screen_pos.y,
                        target_screen_pos.x,
                        target_screen_pos.y,
                        2.0,
                        RED,
                    );
                }
            }
        });

    if let Some((grid_x, grid_y)) = world.resources.ui.mouse_grid_pos {
        let mut tower_data = None;
        world
            .query()
            .with(TOWER | GRID_POSITION | POSITION | TARGETING)
            .iter(|_entity, table, index| {
                if table.grid_position[index].coord.x == grid_x
                    && table.grid_position[index].coord.y == grid_y
                {
                    tower_data = Some((
                        table.tower[index],
                        table.position[index],
                        table.targeting[index],
                    ));
                }
            });

        if let Some((tower, pos, targeting)) = tower_data {
            let screen_pos = Vec2::new(offset.x + pos.0.x * scale, offset.y + pos.0.y * scale);

            let tower_stats = world.resources.config.tower_configs.get(tower.tower_type);
            let range = tower_stats.range(tower.level);
            draw_circle_lines(
                screen_pos.x,
                screen_pos.y,
                range * scale,
                2.0,
                tower.tower_type.color(),
            );

            if tower.level < 4 {
                let upgrade_cost = tower_stats.upgrade_cost(tower.level);
                let text = format!("U: Upgrade (${}) Lv{}", upgrade_cost, tower.level);
                let can_afford = world.resources.economy.money >= upgrade_cost;
                let text_color = if can_afford { GREEN } else { RED };
                draw_text(
                    &text,
                    screen_pos.x - 60.0 * scale,
                    screen_pos.y - 35.0 * scale,
                    20.0 * scale,
                    text_color,
                );
            } else {
                draw_text(
                    "MAX LEVEL",
                    screen_pos.x - 40.0 * scale,
                    screen_pos.y - 35.0 * scale,
                    20.0 * scale,
                    GOLD,
                );
            }

            if let Some(target_pos) = targeting.target_position {
                let target_screen_pos = Vec2::new(
                    offset.x + target_pos.x * scale,
                    offset.y + target_pos.y * scale,
                );
                draw_line(
                    screen_pos.x,
                    screen_pos.y,
                    target_screen_pos.x,
                    target_screen_pos.y,
                    2.0,
                    RED,
                );
            }
        }
    }
}

fn render_enemies(world: &GameWorld) {
    let scale = get_scale();
    let offset = get_offset();

    world
        .query()
        .with(ENEMY | POSITION | ENEMY_DATA | HEALTH)
        .iter(|_entity, table, index| {
            let enemy_data = &table.enemy_data[index];
            let health = &table.health[index];
            let pos = &table.position[index];
            let screen_pos = Vec2::new(offset.x + pos.0.x * scale, offset.y + pos.0.y * scale);
            let enemy_stats = world
                .resources
                .config
                .enemy_configs
                .get(enemy_data.enemy_type);
            let size = enemy_stats.size * scale;
            draw_circle(
                screen_pos.x,
                screen_pos.y,
                size,
                enemy_data.enemy_type.color(),
            );
            draw_circle_lines(screen_pos.x, screen_pos.y, size, 2.0, BLACK);

            if health.shield_current > 0.0 {
                let shield_alpha = health.shield_current / health.shield_maximum;
                draw_circle_lines(
                    screen_pos.x,
                    screen_pos.y,
                    size + 3.0 * scale,
                    2.0,
                    Color::new(0.5, 0.5, 1.0, shield_alpha),
                );
            }

            let health_percent = health.current / health.maximum;
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
            let screen_pos = Vec2::new(offset.x + pos.0.x * scale, offset.y + pos.0.y * scale);
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
            let screen_pos = Vec2::new(offset.x + pos.0.x * scale, offset.y + pos.0.y * scale);
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
            let screen_pos = Vec2::new(offset.x + pos.0.x * scale, offset.y + pos.0.y * scale);
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
        world.resources.economy.money += event.reward;

        for _ in 0..6 {
            let velocity = Vec2::new(rand::gen_range(-40.0, 40.0), rand::gen_range(-40.0, 40.0));
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
    world.for_each_mut_changed(ENEMY, 0, |_entity, _table, _idx| {});
}

fn get_pattern_bounds(pattern: &[Option<TowerType>]) -> (usize, usize, usize, usize) {
    let mut min_row = 3;
    let mut max_row = 0;
    let mut min_col = 3;
    let mut max_col = 0;

    for row in 0..3 {
        for col in 0..3 {
            let index = row * 3 + col;
            if let Some(Some(_)) = pattern.get(index) {
                min_row = min_row.min(row);
                max_row = max_row.max(row);
                min_col = min_col.min(col);
                max_col = max_col.max(col);
            }
        }
    }

    (min_row, max_row, min_col, max_col)
}

fn render_card_preview(
    card: &Card,
    display_x: f32,
    display_y: f32,
    display_width: f32,
    display_height: f32,
    is_selected: bool,
    can_afford: bool,
) {
    let card_color = if is_selected {
        Color::new(0.9, 0.9, 0.6, 1.0)
    } else if can_afford {
        Color::new(0.2, 0.2, 0.2, 1.0)
    } else {
        Color::new(0.15, 0.15, 0.15, 1.0)
    };

    draw_rectangle(
        display_x,
        display_y,
        display_width,
        display_height,
        card_color,
    );

    if !can_afford {
        draw_rectangle(
            display_x,
            display_y,
            display_width,
            display_height,
            Color::new(0.0, 0.0, 0.0, 0.6),
        );
    }

    let border_thickness = if is_selected { 4.0 } else { 3.0 };
    let border_color = if !can_afford {
        Color::new(0.5, 0.0, 0.0, 1.0)
    } else {
        card.rarity.color()
    };
    draw_rectangle_lines(
        display_x,
        display_y,
        display_width,
        display_height,
        border_thickness,
        border_color,
    );

    let name_y = display_y + 15.0 * (display_height / 160.0);
    let name_size = 14.0 * (display_height / 160.0);
    let name_color = if !can_afford {
        Color::new(0.5, 0.5, 0.5, 1.0)
    } else {
        card.rarity.color()
    };
    draw_text(&card.name, display_x + 5.0, name_y, name_size, name_color);

    let (min_row, max_row, min_col, max_col) = get_pattern_bounds(&card.tower_pattern);
    let pattern_rows = (max_row - min_row + 1) as f32;
    let pattern_cols = (max_col - min_col + 1) as f32;

    let available_width = display_width - 20.0;
    let available_height = (display_height * 0.5) - 20.0;

    let cell_size = (available_width / pattern_cols).min(available_height / pattern_rows);

    let grid_width = pattern_cols * cell_size;
    let grid_start_x = display_x + (display_width - grid_width) / 2.0;
    let grid_start_y = display_y + 30.0 * (display_height / 160.0);

    for row in min_row..=max_row {
        for col in min_col..=max_col {
            let pattern_index = row * 3 + col;
            let display_row = row - min_row;
            let display_col = col - min_col;
            let cell_x = grid_start_x + display_col as f32 * cell_size;
            let cell_y = grid_start_y + display_row as f32 * cell_size;

            draw_rectangle_lines(
                cell_x,
                cell_y,
                cell_size,
                cell_size,
                1.0 * (display_height / 160.0),
                Color::new(0.5, 0.5, 0.5, 0.5),
            );

            if let Some(Some(tower_type)) = card.tower_pattern.get(pattern_index) {
                let tower_size = cell_size * 0.6;
                let tower_color = if !can_afford {
                    let base_color = tower_type.color();
                    Color::new(
                        base_color.r * 0.4,
                        base_color.g * 0.4,
                        base_color.b * 0.4,
                        1.0,
                    )
                } else {
                    tower_type.color()
                };
                draw_circle(
                    cell_x + cell_size / 2.0,
                    cell_y + cell_size / 2.0,
                    tower_size / 2.0,
                    tower_color,
                );
            }
        }
    }

    let cost_text = format!("${}", card.cost);
    let text_color = if can_afford { GREEN } else { RED };
    let cost_size = 18.0 * (display_height / 160.0);
    let cost_y = display_y + display_height - 5.0;
    draw_text(&cost_text, display_x + 5.0, cost_y, cost_size, text_color);

    if !can_afford {
        let bg_width = measure_text(&cost_text, None, cost_size as u16, 1.0).width + 4.0;
        let bg_height = cost_size + 2.0;
        let bg_x = display_x + 3.0;
        let bg_y = cost_y - cost_size;
        draw_rectangle(
            bg_x,
            bg_y,
            bg_width,
            bg_height,
            Color::new(0.3, 0.0, 0.0, 0.8),
        );
        draw_text(&cost_text, display_x + 5.0, cost_y, cost_size, RED);
    }

    let rarity_text = match card.rarity {
        CardRarity::Common => "Common",
        CardRarity::Rare => "Rare",
        CardRarity::Epic => "Epic",
        CardRarity::Legendary => "Legendary",
    };
    let rarity_size = 16.0 * (display_height / 160.0);
    let rarity_y = display_y + display_height - 25.0 * (display_height / 160.0);
    let rarity_color = if !can_afford {
        Color::new(0.5, 0.5, 0.5, 1.0)
    } else {
        card.rarity.color()
    };
    draw_text(
        rarity_text,
        display_x + 5.0,
        rarity_y,
        rarity_size,
        rarity_color,
    );
}

fn render_cards(world: &GameWorld) {
    let card_width = 120.0;
    let card_height = 160.0;
    let card_spacing = 10.0;
    let card_y = screen_height() - card_height - 40.0;
    let mouse_pos = Vec2::new(mouse_position().0, mouse_position().1);
    let shift_held = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);

    let deck_x = screen_width() - card_width - 20.0;
    let deck_y = screen_height() - card_height - 40.0;

    let deck_count = world
        .query_entities(CARD)
        .filter(|entity| {
            if let Some(card) = world.get_card(*entity) {
                card.card_state == CardState::InDeck
            } else {
                false
            }
        })
        .count();

    draw_rectangle(
        deck_x,
        deck_y,
        card_width,
        card_height,
        Color::new(0.2, 0.2, 0.3, 1.0),
    );
    draw_rectangle_lines(
        deck_x,
        deck_y,
        card_width,
        card_height,
        3.0,
        Color::new(0.6, 0.6, 0.7, 1.0),
    );

    draw_rectangle(
        deck_x + 3.0,
        deck_y - 3.0,
        card_width,
        card_height,
        Color::new(0.15, 0.15, 0.25, 1.0),
    );
    draw_rectangle_lines(
        deck_x + 3.0,
        deck_y - 3.0,
        card_width,
        card_height,
        2.0,
        Color::new(0.5, 0.5, 0.6, 1.0),
    );

    let deck_text = format!("{}", deck_count);
    let text_size = 60.0;
    let text_dimensions = measure_text(&deck_text, None, text_size as u16, 1.0);
    draw_text(
        &deck_text,
        deck_x + (card_width - text_dimensions.width) / 2.0,
        deck_y + card_height / 2.0 + text_dimensions.height / 2.0,
        text_size,
        WHITE,
    );

    let all_cards: Vec<_> = world
        .query_entities(CARD)
        .filter_map(|entity| world.get_card(entity).map(|card| (entity, card.clone())))
        .collect();

    for (_entity, card) in all_cards.iter() {
        if card.card_state == CardState::Drawing {
            let start_x = deck_x;
            let start_y = deck_y;
            let target_x = 10.0 + card.hand_position_index as f32 * (card_width + card_spacing);
            let target_y = card_y;

            let progress = card.draw_animation_progress;
            let current_x = start_x + (target_x - start_x) * progress;
            let current_y = start_y + (target_y - start_y) * progress;

            let can_afford = world.resources.economy.money >= card.cost;
            render_card_preview(
                &card,
                current_x,
                current_y,
                card_width,
                card_height,
                false,
                can_afford,
            );
        }
    }

    let cards_in_hand: Vec<_> = all_cards
        .iter()
        .filter(|(_entity, card)| card.card_state == CardState::InHand)
        .collect();

    for (card_index, (_entity, card)) in cards_in_hand.iter().enumerate() {
        let card_x = 10.0 + card_index as f32 * (card_width + card_spacing);

        let is_hovered = mouse_pos.x >= card_x
            && mouse_pos.x <= card_x + card_width
            && mouse_pos.y >= card_y
            && mouse_pos.y <= card_y + card_height;

        if shift_held && is_hovered {
            continue;
        }

        let is_selected = world.resources.card_system.selected_card == Some(card_index);

        let scale = if is_hovered && !shift_held { 1.1 } else { 1.0 };
        let display_width = card_width * scale;
        let display_height = card_height * scale;
        let display_x = card_x - (display_width - card_width) / 2.0;
        let display_y = card_y - (display_height - card_height) / 2.0;

        let can_afford = world.resources.economy.money >= card.cost;

        render_card_preview(
            &card,
            display_x,
            display_y,
            display_width,
            display_height,
            is_selected,
            can_afford,
        );
    }

    let hand_info = format!(
        "Hand: {}/{} | Deck: {}",
        world.resources.card_system.hand_size,
        world.resources.card_system.max_hand_size,
        deck_count
    );
    draw_text(&hand_info, 10.0, screen_height() - 15.0, 20.0, WHITE);
}

fn render_card_preview_overlay(world: &GameWorld) {
    let shift_held = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);

    if !shift_held {
        return;
    }

    let card_width = 120.0;
    let card_height = 160.0;
    let card_spacing = 10.0;
    let card_y = screen_height() - card_height - 40.0;
    let mouse_pos = Vec2::new(mouse_position().0, mouse_position().1);

    let cards: Vec<_> = world
        .query_entities(CARD)
        .filter_map(|entity| {
            world.get_card(entity).and_then(|card| {
                if card.in_hand {
                    Some((entity, card.clone()))
                } else {
                    None
                }
            })
        })
        .collect();

    let mut hovered_card_index = None;

    for (card_index, (_entity, _card)) in cards.iter().enumerate() {
        let card_x = 10.0 + card_index as f32 * (card_width + card_spacing);

        let is_hovered = mouse_pos.x >= card_x
            && mouse_pos.x <= card_x + card_width
            && mouse_pos.y >= card_y
            && mouse_pos.y <= card_y + card_height;

        if is_hovered {
            hovered_card_index = Some(card_index);
            break;
        }
    }

    if let Some(hovered_index) = hovered_card_index {
        if let Some((_entity, card)) = cards.get(hovered_index) {
            draw_rectangle(
                0.0,
                0.0,
                screen_width(),
                screen_height(),
                Color::new(0.0, 0.0, 0.0, 0.8),
            );

            let preview_width = 400.0;
            let preview_height = 533.0;
            let preview_x = (screen_width() - preview_width) / 2.0;
            let preview_y = (screen_height() - preview_height) / 2.0;

            let can_afford = world.resources.economy.money >= card.cost;
            let is_selected = world.resources.card_system.selected_card == Some(hovered_index);

            render_card_preview(
                &card,
                preview_x,
                preview_y,
                preview_width,
                preview_height,
                is_selected,
                can_afford,
            );
        }
    }
}

fn get_node_screen_position(map_data: &MapData, node_index: usize) -> Vec2 {
    if node_index >= map_data.nodes.len() {
        return Vec2::new(0.0, 0.0);
    }

    let node = &map_data.nodes[node_index];
    let layer = node.layer;
    let position_in_layer = node.position_in_layer;
    let nodes_in_layer = map_data.nodes_per_layer[layer];

    let map_width = screen_width() * 0.8;
    let map_height = screen_height() * 0.8;
    let map_x = screen_width() * 0.1;
    let map_y = screen_height() * 0.1;

    let layer_spacing = map_height / (map_data.layers as f32 - 1.0).max(1.0);
    let y = map_y + layer as f32 * layer_spacing;

    let node_spacing = map_width / (nodes_in_layer as f32 + 1.0);
    let x = map_x + (position_in_layer as f32 + 1.0) * node_spacing;

    Vec2::new(x, y)
}

fn render_map(world: &GameWorld) {
    let map_data = &world.resources.meta_game.map_data;
    let current_node = world.resources.meta_game.current_node;

    for (node_index, node) in map_data.nodes.iter().enumerate() {
        let pos = get_node_screen_position(map_data, node_index);

        for &connection_idx in &node.connections {
            let target_pos = get_node_screen_position(map_data, connection_idx);
            let line_color = if node.visited {
                Color::new(0.6, 0.6, 0.6, 0.6)
            } else {
                Color::new(0.3, 0.3, 0.3, 0.4)
            };
            draw_line(pos.x, pos.y, target_pos.x, target_pos.y, 3.0, line_color);
        }
    }

    let mouse_pos = Vec2::new(mouse_position().0, mouse_position().1);

    for (node_index, node) in map_data.nodes.iter().enumerate() {
        let pos = get_node_screen_position(map_data, node_index);
        let node_radius = 30.0;

        let is_hovered = mouse_pos.distance(pos) <= node_radius;
        let is_current = current_node != usize::MAX && node_index == current_node;

        let node_color = if !node.available && !node.visited {
            Color::new(0.2, 0.2, 0.2, 0.5)
        } else if is_current {
            Color::new(1.0, 1.0, 0.5, 1.0)
        } else if node.visited {
            Color::new(0.5, 0.5, 0.5, 1.0)
        } else if is_hovered {
            let base = node.node_type.color();
            Color::new(base.r * 1.2, base.g * 1.2, base.b * 1.2, 1.0)
        } else {
            node.node_type.color()
        };

        draw_circle(pos.x, pos.y, node_radius, node_color);

        let border_color = if is_current {
            WHITE
        } else if node.available {
            Color::new(0.9, 0.9, 0.9, 1.0)
        } else {
            Color::new(0.4, 0.4, 0.4, 0.8)
        };
        let border_thickness = if is_current { 4.0 } else { 2.0 };

        draw_circle_lines(pos.x, pos.y, node_radius, border_thickness, border_color);

        let icon = node.node_type.icon();
        let text_size = 16.0;
        let text_dims = measure_text(icon, None, text_size as u16, 1.0);
        draw_text(
            icon,
            pos.x - text_dims.width / 2.0,
            pos.y + text_dims.height / 2.0 - 5.0,
            text_size,
            WHITE,
        );
    }

    draw_text("MAP - Click a node to travel", 20.0, 30.0, 30.0, WHITE);
    draw_text("Press 'V' to view deck", 20.0, 60.0, 20.0, GRAY);
}

fn render_deck_view(world: &GameWorld) {
    draw_rectangle(
        0.0,
        0.0,
        screen_width(),
        screen_height(),
        Color::new(0.1, 0.1, 0.1, 0.95),
    );

    let title = "DECK VIEW";
    let title_size = 50.0;
    let title_dims = measure_text(title, None, title_size as u16, 1.0);
    draw_text(
        title,
        (screen_width() - title_dims.width) / 2.0,
        60.0,
        title_size,
        WHITE,
    );

    let all_cards: Vec<_> = world
        .query_entities(CARD)
        .filter_map(|entity| world.get_card(entity).map(|card| card.clone()))
        .collect();

    let mut card_counts: Vec<(Card, usize)> = Vec::new();
    for card in all_cards.iter() {
        let mut found = false;
        for (existing_card, count) in card_counts.iter_mut() {
            if existing_card.name == card.name && existing_card.tower_pattern == card.tower_pattern
            {
                *count += 1;
                found = true;
                break;
            }
        }
        if !found {
            card_counts.push((card.clone(), 1));
        }
    }

    let card_width = 180.0;
    let card_height = 240.0;
    let card_spacing = 20.0;
    let cards_per_row = 5;

    let start_x = (screen_width()
        - (cards_per_row as f32 * (card_width + card_spacing) - card_spacing))
        / 2.0;
    let start_y = 120.0;

    for (index, (card, count)) in card_counts.iter().enumerate() {
        let row = index / cards_per_row;
        let col = index % cards_per_row;

        let x = start_x + col as f32 * (card_width + card_spacing);
        let y = start_y + row as f32 * (card_height + card_spacing);

        render_card_preview(&card, x, y, card_width, card_height, false, true);

        if *count > 1 {
            let count_text = format!("x{}", count);
            let count_size = 35.0;
            let count_dims = measure_text(&count_text, None, count_size as u16, 1.0);

            draw_rectangle(
                x + card_width - count_dims.width - 15.0,
                y + 5.0,
                count_dims.width + 10.0,
                count_dims.height + 10.0,
                Color::new(0.0, 0.0, 0.0, 0.8),
            );

            draw_text(
                &count_text,
                x + card_width - count_dims.width - 10.0,
                y + count_dims.height + 10.0,
                count_size,
                WHITE,
            );
        }
    }

    let info_text = format!(
        "Total Cards: {} | Unique: {} | Press ESC to close",
        all_cards.len(),
        card_counts.len()
    );
    let info_size = 25.0;
    let info_dims = measure_text(&info_text, None, info_size as u16, 1.0);
    draw_text(
        &info_text,
        (screen_width() - info_dims.width) / 2.0,
        screen_height() - 30.0,
        info_size,
        WHITE,
    );
}

fn deck_view_input_system(world: &mut GameWorld) {
    if world.resources.combat.game_state != GameState::DeckView {
        return;
    }

    if is_key_pressed(KeyCode::Escape) || is_key_pressed(KeyCode::V) {
        world.resources.combat.game_state = world.resources.meta_game.previous_state;
    }
}

fn render_shop_offering(
    world: &GameWorld,
    offering: &ShopOffering,
    _index: usize,
    x: f32,
    y: f32,
    card_width: f32,
    card_height: f32,
    mouse_pos: Vec2,
) {
    let is_hovered = mouse_pos.x >= x
        && mouse_pos.x <= x + card_width
        && mouse_pos.y >= y
        && mouse_pos.y <= y + card_height;

    let bg_color = if is_hovered {
        Color::new(0.3, 0.2, 0.4, 1.0)
    } else {
        Color::new(0.2, 0.15, 0.25, 1.0)
    };

    draw_rectangle(x, y, card_width, card_height, bg_color);
    draw_rectangle_lines(
        x,
        y,
        card_width,
        card_height,
        3.0,
        Color::new(0.6, 0.5, 0.7, 1.0),
    );

    match offering {
        ShopOffering::Card {
            name,
            cost,
            rarity,
            pattern,
        } => {
            draw_text(name, x + 10.0, y + 25.0, 20.0, WHITE);

            let rarity_text = format!("{:?}", rarity);
            draw_text(&rarity_text, x + 10.0, y + 50.0, 16.0, rarity.color());

            let grid_size = 3;
            let cell_size = 18.0;
            let grid_start_x = x + (card_width - (grid_size as f32 * cell_size)) / 2.0;
            let grid_start_y = y + 70.0;

            for grid_y in 0..grid_size {
                for grid_x in 0..grid_size {
                    let cell_x = grid_start_x + grid_x as f32 * cell_size;
                    let cell_y = grid_start_y + grid_y as f32 * cell_size;
                    let pattern_index = grid_y * grid_size + grid_x;

                    if let Some(Some(tower_type)) = pattern.get(pattern_index) {
                        draw_rectangle(
                            cell_x,
                            cell_y,
                            cell_size - 2.0,
                            cell_size - 2.0,
                            tower_type.color(),
                        );
                    } else {
                        draw_rectangle(
                            cell_x,
                            cell_y,
                            cell_size - 2.0,
                            cell_size - 2.0,
                            Color::new(0.1, 0.1, 0.1, 1.0),
                        );
                    }
                    draw_rectangle_lines(
                        cell_x,
                        cell_y,
                        cell_size - 2.0,
                        cell_size - 2.0,
                        1.0,
                        GRAY,
                    );
                }
            }

            let cost_text = format!("${}", cost);
            let can_afford = world.resources.economy.money >= *cost;
            let cost_color = if can_afford { YELLOW } else { RED };
            draw_text(
                &cost_text,
                x + 10.0,
                y + card_height - 15.0,
                24.0,
                cost_color,
            );
        }
        ShopOffering::UpgradeCard {
            card_entity,
            current_rarity,
            cost,
        } => {
            if let Some(card) = world.get_card(*card_entity) {
                draw_text(
                    "UPGRADE",
                    x + 10.0,
                    y + 25.0,
                    20.0,
                    Color::new(1.0, 0.8, 0.3, 1.0),
                );
                draw_text(&card.name, x + 10.0, y + 50.0, 18.0, WHITE);

                let current_text = format!("{:?}", current_rarity);
                let next_rarity = match current_rarity {
                    CardRarity::Common => CardRarity::Rare,
                    CardRarity::Rare => CardRarity::Epic,
                    CardRarity::Epic => CardRarity::Legendary,
                    CardRarity::Legendary => CardRarity::Legendary,
                };
                let next_text = format!("{:?}", next_rarity);

                draw_text(
                    &current_text,
                    x + 10.0,
                    y + 75.0,
                    16.0,
                    current_rarity.color(),
                );
                draw_text("→", x + 70.0, y + 75.0, 16.0, WHITE);
                draw_text(&next_text, x + 90.0, y + 75.0, 16.0, next_rarity.color());

                let grid_size = 3;
                let cell_size = 18.0;
                let grid_start_x = x + (card_width - (grid_size as f32 * cell_size)) / 2.0;
                let grid_start_y = y + 100.0;

                for grid_y in 0..grid_size {
                    for grid_x in 0..grid_size {
                        let cell_x = grid_start_x + grid_x as f32 * cell_size;
                        let cell_y = grid_start_y + grid_y as f32 * cell_size;
                        let pattern_index = grid_y * grid_size + grid_x;

                        if let Some(Some(tower_type)) = card.tower_pattern.get(pattern_index) {
                            draw_rectangle(
                                cell_x,
                                cell_y,
                                cell_size - 2.0,
                                cell_size - 2.0,
                                tower_type.color(),
                            );
                        } else {
                            draw_rectangle(
                                cell_x,
                                cell_y,
                                cell_size - 2.0,
                                cell_size - 2.0,
                                Color::new(0.1, 0.1, 0.1, 1.0),
                            );
                        }
                        draw_rectangle_lines(
                            cell_x,
                            cell_y,
                            cell_size - 2.0,
                            cell_size - 2.0,
                            1.0,
                            GRAY,
                        );
                    }
                }

                let cost_text = format!("${}", cost);
                let can_afford = world.resources.economy.money >= *cost;
                let cost_color = if can_afford { YELLOW } else { RED };
                draw_text(
                    &cost_text,
                    x + 10.0,
                    y + card_height - 15.0,
                    24.0,
                    cost_color,
                );
            }
        }
        ShopOffering::RemoveCard { cost } => {
            draw_text("Remove Card", x + 10.0, y + 30.0, 22.0, WHITE);
            draw_text("Remove a card", x + 10.0, y + 100.0, 16.0, LIGHTGRAY);
            draw_text("from your deck", x + 10.0, y + 120.0, 16.0, LIGHTGRAY);

            let cost_text = format!("${}", cost);
            let can_afford = world.resources.economy.money >= *cost;
            let cost_color = if can_afford { YELLOW } else { RED };
            draw_text(
                &cost_text,
                x + 10.0,
                y + card_height - 15.0,
                24.0,
                cost_color,
            );
        }
        ShopOffering::Heal { amount, cost } => {
            draw_text(
                &format!("Heal +{} HP", amount),
                x + 10.0,
                y + 30.0,
                22.0,
                WHITE,
            );
            draw_text("Restore health", x + 10.0, y + 100.0, 16.0, LIGHTGRAY);

            let cost_text = format!("${}", cost);
            let can_afford = world.resources.economy.money >= *cost;
            let cost_color = if can_afford { YELLOW } else { RED };
            draw_text(
                &cost_text,
                x + 10.0,
                y + card_height - 15.0,
                24.0,
                cost_color,
            );
        }
        ShopOffering::MaxHealth { amount, cost } => {
            draw_text(
                &format!("Max HP +{}", amount),
                x + 10.0,
                y + 30.0,
                22.0,
                WHITE,
            );
            draw_text("Permanently", x + 10.0, y + 100.0, 16.0, LIGHTGRAY);
            draw_text("increase max HP", x + 10.0, y + 120.0, 16.0, LIGHTGRAY);

            let cost_text = format!("${}", cost);
            let can_afford = world.resources.economy.money >= *cost;
            let cost_color = if can_afford { YELLOW } else { RED };
            draw_text(
                &cost_text,
                x + 10.0,
                y + card_height - 15.0,
                24.0,
                cost_color,
            );
        }
        ShopOffering::Relic { relic_type, cost } => {
            let name_size = 20.0;
            let name_dims = measure_text(relic_type.name(), None, name_size as u16, 1.0);
            draw_text(
                relic_type.name(),
                x + (card_width - name_dims.width) / 2.0,
                y + 35.0,
                name_size,
                Color::new(1.0, 0.8, 0.3, 1.0),
            );

            draw_rectangle(x + 15.0, y + 50.0, card_width - 30.0, 2.0, GOLD);

            let desc = relic_type.description();
            let words: Vec<&str> = desc.split_whitespace().collect();
            let mut line = String::new();
            let mut y_offset = 70.0;
            let desc_size = 14.0;

            for word in words {
                let test_line = if line.is_empty() {
                    word.to_string()
                } else {
                    format!("{} {}", line, word)
                };

                let test_dims = measure_text(&test_line, None, desc_size as u16, 1.0);
                if test_dims.width > card_width - 20.0 {
                    draw_text(&line, x + 10.0, y + y_offset, desc_size, LIGHTGRAY);
                    line = word.to_string();
                    y_offset += 18.0;
                } else {
                    line = test_line;
                }
            }
            if !line.is_empty() {
                draw_text(&line, x + 10.0, y + y_offset, desc_size, LIGHTGRAY);
            }

            let cost_text = format!("${}", cost);
            let can_afford = world.resources.economy.money >= *cost;
            let cost_color = if can_afford { YELLOW } else { RED };
            draw_text(
                &cost_text,
                x + 10.0,
                y + card_height - 15.0,
                24.0,
                cost_color,
            );
        }
    }
}

fn render_shop(world: &GameWorld) {
    draw_rectangle(
        0.0,
        0.0,
        screen_width(),
        screen_height(),
        Color::new(0.1, 0.05, 0.15, 1.0),
    );

    let title = "MERCHANT'S SHOP";
    let title_size = 60.0;
    let title_dims = measure_text(title, None, title_size as u16, 1.0);
    draw_text(
        title,
        (screen_width() - title_dims.width) / 2.0,
        50.0,
        title_size,
        Color::new(1.0, 0.8, 0.2, 1.0),
    );

    let money_text = format!("Gold: ${}", world.resources.economy.money);
    draw_text(
        &money_text,
        (screen_width() - 200.0) / 2.0,
        110.0,
        35.0,
        YELLOW,
    );

    let offerings = &world.resources.meta_game.shop_offerings;

    let card_width = 180.0;
    let card_height = 240.0;
    let spacing = 20.0;
    let mouse_pos = Vec2::new(mouse_position().0, mouse_position().1);

    let start_y = 160.0;
    let total_width = offerings.len() as f32 * card_width + (offerings.len() - 1) as f32 * spacing;
    let start_x = (screen_width() - total_width) / 2.0;

    for (index, offering) in offerings.iter().enumerate() {
        let x = start_x + index as f32 * (card_width + spacing);
        let y = start_y;
        render_shop_offering(
            world,
            offering,
            index,
            x,
            y,
            card_width,
            card_height,
            mouse_pos,
        );
    }

    let info = "Click card to purchase | ESC to leave";
    let info_size = 20.0;
    let info_dims = measure_text(info, None, info_size as u16, 1.0);
    draw_text(
        info,
        (screen_width() - info_dims.width) / 2.0,
        screen_height() - 30.0,
        info_size,
        LIGHTGRAY,
    );
}

fn shop_input_system(world: &mut GameWorld) {
    if world.resources.combat.game_state != GameState::Shop {
        return;
    }

    if is_key_pressed(KeyCode::Escape) {
        world.resources.combat.game_state = GameState::Map;
        return;
    }

    if is_mouse_button_pressed(MouseButton::Left) {
        let mouse_pos = Vec2::new(mouse_position().0, mouse_position().1);
        let offerings = world.resources.meta_game.shop_offerings.clone();

        let card_width = 180.0;
        let card_height = 240.0;
        let spacing = 20.0;
        let start_y = 160.0;
        let total_width =
            offerings.len() as f32 * card_width + (offerings.len() - 1) as f32 * spacing;
        let start_x = (screen_width() - total_width) / 2.0;

        for (index, offering) in offerings.iter().enumerate() {
            let x = start_x + index as f32 * (card_width + spacing);
            let y = start_y;

            if mouse_pos.x >= x
                && mouse_pos.x <= x + card_width
                && mouse_pos.y >= y
                && mouse_pos.y <= y + card_height
            {
                match offering {
                    ShopOffering::Card {
                        name,
                        pattern,
                        rarity,
                        cost,
                    } => {
                        if world.resources.economy.money >= *cost {
                            world.resources.economy.money -= cost;
                            create_card(world, name, pattern.clone(), *rarity);
                            world.resources.meta_game.shop_offerings.remove(index);
                        }
                    }
                    ShopOffering::Relic { relic_type, cost } => {
                        if world.resources.economy.money >= *cost {
                            world.resources.economy.money -= cost;
                            world.resources.economy.owned_relics.push(*relic_type);
                            world.resources.meta_game.shop_offerings.remove(index);
                        }
                    }
                    _ => {}
                }
                break;
            }
        }
    }
}

fn render_rest(world: &GameWorld) {
    draw_rectangle(
        0.0,
        0.0,
        screen_width(),
        screen_height(),
        Color::new(0.05, 0.15, 0.1, 1.0),
    );

    let title = "CAMPFIRE REST";
    let title_size = 60.0;
    let title_dims = measure_text(title, None, title_size as u16, 1.0);
    draw_text(
        title,
        (screen_width() - title_dims.width) / 2.0,
        60.0,
        title_size,
        Color::new(0.8, 1.0, 0.6, 1.0),
    );

    let subtitle = "Choose one option";
    let subtitle_size = 25.0;
    let subtitle_dims = measure_text(subtitle, None, subtitle_size as u16, 1.0);
    draw_text(
        subtitle,
        (screen_width() - subtitle_dims.width) / 2.0,
        110.0,
        subtitle_size,
        LIGHTGRAY,
    );

    let options = &world.resources.meta_game.rest_options;
    let card_width = 250.0;
    let card_height = 120.0;
    let spacing = 30.0;
    let start_y = 160.0;

    let mouse_pos = Vec2::new(mouse_position().0, mouse_position().1);

    for (index, option) in options.iter().enumerate() {
        let x = (screen_width() - card_width) / 2.0;
        let y = start_y + (index as f32 * (card_height + spacing));

        let is_hovered = mouse_pos.x >= x
            && mouse_pos.x <= x + card_width
            && mouse_pos.y >= y
            && mouse_pos.y <= y + card_height;

        let bg_color = if is_hovered {
            Color::new(0.2, 0.4, 0.25, 1.0)
        } else {
            Color::new(0.15, 0.3, 0.2, 1.0)
        };

        draw_rectangle(x, y, card_width, card_height, bg_color);
        draw_rectangle_lines(
            x,
            y,
            card_width,
            card_height,
            3.0,
            Color::new(0.5, 0.7, 0.6, 1.0),
        );

        match option {
            RestOption::Heal { amount } => {
                draw_text(
                    &format!("Heal +{} HP", amount),
                    x + 10.0,
                    y + 35.0,
                    28.0,
                    WHITE,
                );
                draw_text(
                    "Restore your health by resting",
                    x + 10.0,
                    y + 65.0,
                    18.0,
                    LIGHTGRAY,
                );
                draw_text("at the campfire", x + 10.0, y + 90.0, 18.0, LIGHTGRAY);
            }
            RestOption::UpgradeCard => {
                draw_text("Upgrade Random Card", x + 10.0, y + 35.0, 26.0, WHITE);
                draw_text("Increase power of a", x + 10.0, y + 65.0, 18.0, LIGHTGRAY);
                draw_text("random card's rarity", x + 10.0, y + 90.0, 18.0, LIGHTGRAY);
            }
            RestOption::RemoveCard => {
                draw_text("Purify Deck", x + 10.0, y + 35.0, 28.0, WHITE);
                draw_text("Remove a random card", x + 10.0, y + 65.0, 18.0, LIGHTGRAY);
                draw_text("from your deck", x + 10.0, y + 90.0, 18.0, LIGHTGRAY);
            }
        }
    }

    let info = "Click option to choose | ESC to skip rest";
    let info_size = 20.0;
    let info_dims = measure_text(info, None, info_size as u16, 1.0);
    draw_text(
        info,
        (screen_width() - info_dims.width) / 2.0,
        screen_height() - 30.0,
        info_size,
        LIGHTGRAY,
    );
}

fn rest_input_system(world: &mut GameWorld) {
    if world.resources.combat.game_state != GameState::Rest {
        return;
    }

    if is_key_pressed(KeyCode::Escape) {
        world.resources.combat.game_state = GameState::Map;
        return;
    }

    if is_mouse_button_pressed(MouseButton::Left) {
        let mouse_pos = Vec2::new(mouse_position().0, mouse_position().1);
        let options = world.resources.meta_game.rest_options.clone();

        let card_width = 250.0;
        let card_height = 120.0;
        let spacing = 30.0;
        let start_y = 160.0;

        for (index, option) in options.iter().enumerate() {
            let x = (screen_width() - card_width) / 2.0;
            let y = start_y + (index as f32 * (card_height + spacing));

            if mouse_pos.x >= x
                && mouse_pos.x <= x + card_width
                && mouse_pos.y >= y
                && mouse_pos.y <= y + card_height
            {
                match option {
                    RestOption::Heal { amount } => {
                        if let Some(player_entity) = world.query_entities(PLAYER).next() {
                            if let Some(player) = world.get_player_mut(player_entity) {
                                player.health = (player.health + amount).min(player.max_health);
                                world.resources.combat.current_hp =
                                    (world.resources.combat.current_hp + amount)
                                        .min(world.resources.combat.max_hp);
                            }
                        }
                    }
                    RestOption::UpgradeCard => {
                        let cards: Vec<_> = world.query_entities(CARD).collect();
                        if !cards.is_empty() {
                            let random_card_entity = cards[rand::gen_range(0, cards.len())];
                            if let Some(card) = world.get_card_mut(random_card_entity) {
                                card.rarity = match card.rarity {
                                    CardRarity::Common => CardRarity::Rare,
                                    CardRarity::Rare => CardRarity::Epic,
                                    CardRarity::Epic => CardRarity::Legendary,
                                    CardRarity::Legendary => CardRarity::Legendary,
                                };
                            }
                            if let Some(card) = world.get_card(random_card_entity) {
                                let new_cost = calculate_card_cost(
                                    &card.tower_pattern,
                                    card.rarity,
                                    &world.resources.config.tower_configs,
                                );
                                if let Some(card_mut) = world.get_card_mut(random_card_entity) {
                                    card_mut.cost = new_cost;
                                }
                            }
                        }
                    }
                    RestOption::RemoveCard => {
                        let cards: Vec<_> = world.query_entities(CARD).collect();
                        if !cards.is_empty() {
                            let random_card = cards[rand::gen_range(0, cards.len())];
                            world.queue_despawn_entity(random_card);
                            world.apply_commands();
                        }
                    }
                }

                world.resources.combat.game_state = GameState::Map;
                break;
            }
        }
    }
}

fn render_forge(world: &GameWorld) {
    clear_background(Color::new(0.05, 0.05, 0.08, 1.0));

    let title = "FORGE";
    let title_size = 50.0;
    let title_dims = measure_text(title, None, title_size as u16, 1.0);
    draw_text(
        title,
        (screen_width() - title_dims.width) / 2.0,
        60.0,
        title_size,
        GOLD,
    );

    let subtitle = if world.resources.meta_game.forge_offered_cards.is_empty() {
        format!(
            "Select 2 cards to sacrifice ({} uses remaining)",
            world.resources.meta_game.forge_uses_remaining
        )
    } else {
        "Choose 1 upgraded card".to_string()
    };
    let subtitle_size = 25.0;
    let subtitle_dims = measure_text(&subtitle, None, subtitle_size as u16, 1.0);
    draw_text(
        &subtitle,
        (screen_width() - subtitle_dims.width) / 2.0,
        110.0,
        subtitle_size,
        LIGHTGRAY,
    );

    if world.resources.meta_game.forge_offered_cards.is_empty() {
        let cards: Vec<_> = world
            .query_entities(CARD)
            .filter_map(|entity| world.get_card(entity).map(|card| (entity, card.clone())))
            .collect();

        let card_width = 120.0;
        let card_height = 140.0;
        let spacing = 15.0;
        let cards_per_row = 8;
        let start_y = 160.0;

        for (index, (_entity, card)) in cards.iter().enumerate() {
            let row = index / cards_per_row;
            let col = index % cards_per_row;
            let x = 50.0 + col as f32 * (card_width + spacing);
            let y = start_y + row as f32 * (card_height + spacing);

            let is_selected = world
                .resources
                .meta_game
                .forge_selected_cards
                .contains(&index);
            let border_color = if is_selected {
                GOLD
            } else {
                card.rarity.color()
            };
            draw_rectangle(
                x - 2.0,
                y - 2.0,
                card_width + 4.0,
                card_height + 4.0,
                border_color,
            );
            draw_rectangle(
                x,
                y,
                card_width,
                card_height,
                Color::new(0.1, 0.1, 0.15, 1.0),
            );

            let name_size = 14.0;
            draw_text(&card.name, x + 5.0, y + 20.0, name_size, WHITE);

            let grid_size = 3;
            let cell_size = 18.0;
            let grid_start_x = x + (card_width - (grid_size as f32 * cell_size)) / 2.0;
            let grid_start_y = y + 30.0;

            for grid_y in 0..grid_size {
                for grid_x in 0..grid_size {
                    let cell_x = grid_start_x + grid_x as f32 * cell_size;
                    let cell_y = grid_start_y + grid_y as f32 * cell_size;
                    let pattern_index = grid_y * grid_size + grid_x;

                    if let Some(Some(tower_type)) = card.tower_pattern.get(pattern_index) {
                        draw_rectangle(
                            cell_x,
                            cell_y,
                            cell_size - 2.0,
                            cell_size - 2.0,
                            tower_type.color(),
                        );
                    } else {
                        draw_rectangle(
                            cell_x,
                            cell_y,
                            cell_size - 2.0,
                            cell_size - 2.0,
                            Color::new(0.1, 0.1, 0.1, 1.0),
                        );
                    }
                }
            }

            let cost_text = format!("${}", card.cost);
            draw_text(&cost_text, x + 5.0, y + card_height - 10.0, 16.0, GOLD);
        }
    } else {
        let card_width = 140.0;
        let card_height = 160.0;
        let spacing = 30.0;
        let total_width = 3.0 * card_width + 2.0 * spacing;
        let start_x = (screen_width() - total_width) / 2.0;
        let start_y = 180.0;

        for (index, (name, pattern, rarity)) in world
            .resources
            .meta_game
            .forge_offered_cards
            .iter()
            .enumerate()
        {
            let x = start_x + index as f32 * (card_width + spacing);
            let y = start_y;

            let mouse_pos = Vec2::new(mouse_position().0, mouse_position().1);
            let is_hovered = mouse_pos.x >= x
                && mouse_pos.x <= x + card_width
                && mouse_pos.y >= y
                && mouse_pos.y <= y + card_height;

            let border_color = if is_hovered { GOLD } else { rarity.color() };
            draw_rectangle(
                x - 2.0,
                y - 2.0,
                card_width + 4.0,
                card_height + 4.0,
                border_color,
            );
            draw_rectangle(
                x,
                y,
                card_width,
                card_height,
                Color::new(0.1, 0.1, 0.15, 1.0),
            );

            let name_size = 14.0;
            draw_text(name, x + 5.0, y + 20.0, name_size, WHITE);

            let grid_size = 3;
            let cell_size = 20.0;
            let grid_start_x = x + (card_width - (grid_size as f32 * cell_size)) / 2.0;
            let grid_start_y = y + 40.0;

            for grid_y in 0..grid_size {
                for grid_x in 0..grid_size {
                    let cell_x = grid_start_x + grid_x as f32 * cell_size;
                    let cell_y = grid_start_y + grid_y as f32 * cell_size;
                    let pattern_index = grid_y * grid_size + grid_x;

                    if let Some(Some(tower_type)) = pattern.get(pattern_index) {
                        draw_rectangle(
                            cell_x,
                            cell_y,
                            cell_size - 2.0,
                            cell_size - 2.0,
                            tower_type.color(),
                        );
                    } else {
                        draw_rectangle(
                            cell_x,
                            cell_y,
                            cell_size - 2.0,
                            cell_size - 2.0,
                            Color::new(0.1, 0.1, 0.1, 1.0),
                        );
                    }
                }
            }

            let rarity_text = format!("{:?}", rarity);
            draw_text(
                &rarity_text,
                x + 5.0,
                y + card_height - 10.0,
                14.0,
                rarity.color(),
            );
        }
    }

    let info = "ESC: Return to Map";
    let info_size = 16.0;
    let info_dims = measure_text(info, None, info_size as u16, 1.0);
    draw_text(
        info,
        (screen_width() - info_dims.width) / 2.0,
        screen_height() - 30.0,
        info_size,
        LIGHTGRAY,
    );
}

fn forge_input_system(world: &mut GameWorld) {
    if world.resources.combat.game_state != GameState::Forge {
        return;
    }

    if is_key_pressed(KeyCode::Escape) {
        world.resources.combat.game_state = GameState::Map;
        return;
    }

    if is_mouse_button_pressed(MouseButton::Left) {
        let mouse_pos = Vec2::new(mouse_position().0, mouse_position().1);

        if world.resources.meta_game.forge_offered_cards.is_empty() {
            let cards: Vec<_> = world
                .query_entities(CARD)
                .filter_map(|entity| world.get_card(entity).map(|card| (entity, card.clone())))
                .collect();

            let card_width = 120.0;
            let card_height = 140.0;
            let spacing = 15.0;
            let cards_per_row = 8;
            let start_y = 160.0;

            for (index, (_entity, _card)) in cards.iter().enumerate() {
                let row = index / cards_per_row;
                let col = index % cards_per_row;
                let x = 50.0 + col as f32 * (card_width + spacing);
                let y = start_y + row as f32 * (card_height + spacing);

                if mouse_pos.x >= x
                    && mouse_pos.x <= x + card_width
                    && mouse_pos.y >= y
                    && mouse_pos.y <= y + card_height
                {
                    if world
                        .resources
                        .meta_game
                        .forge_selected_cards
                        .contains(&index)
                    {
                        world
                            .resources
                            .meta_game
                            .forge_selected_cards
                            .retain(|&i| i != index);
                    } else if world.resources.meta_game.forge_selected_cards.len() < 2 {
                        world.resources.meta_game.forge_selected_cards.push(index);
                    }

                    if world.resources.meta_game.forge_selected_cards.len() == 2 {
                        let card_definitions = get_all_card_definitions();
                        let mut offered_cards = Vec::new();
                        for _ in 0..3 {
                            let random_index = rand::gen_range(0, card_definitions.len());
                            let (name, pattern, _) = &card_definitions[random_index];
                            let upgraded_rarity = CardRarity::Epic;
                            offered_cards.push((
                                name.to_string(),
                                pattern.clone(),
                                upgraded_rarity,
                            ));
                        }
                        world.resources.meta_game.forge_offered_cards = offered_cards;
                    }
                    break;
                }
            }
        } else {
            let card_width = 140.0;
            let card_height = 160.0;
            let spacing = 30.0;
            let total_width = 3.0 * card_width + 2.0 * spacing;
            let start_x = (screen_width() - total_width) / 2.0;
            let start_y = 180.0;

            let offered_cards = world.resources.meta_game.forge_offered_cards.clone();
            for (index, (name, pattern, rarity)) in offered_cards.iter().enumerate() {
                let x = start_x + index as f32 * (card_width + spacing);
                let y = start_y;

                if mouse_pos.x >= x
                    && mouse_pos.x <= x + card_width
                    && mouse_pos.y >= y
                    && mouse_pos.y <= y + card_height
                {
                    let cards: Vec<_> = world.query_entities(CARD).collect();
                    let mut selected_indices: Vec<_> = world
                        .resources
                        .meta_game
                        .forge_selected_cards
                        .iter()
                        .cloned()
                        .collect();
                    selected_indices.sort_by(|a, b| b.cmp(a));

                    for card_index in selected_indices {
                        if card_index < cards.len() {
                            world.queue_despawn_entity(cards[card_index]);
                        }
                    }
                    world.apply_commands();

                    let card_entity = world.spawn_entities(CARD, 1)[0];
                    world.set_card(
                        card_entity,
                        Card {
                            card_index: 0,
                            tower_pattern: pattern.clone(),
                            cost: calculate_card_cost(
                                pattern,
                                *rarity,
                                &world.resources.config.tower_configs,
                            ),
                            rarity: *rarity,
                            name: name.clone(),
                            in_hand: false,
                            card_state: CardState::InDeck,
                            draw_animation_progress: 0.0,
                            hand_position_index: 0,
                        },
                    );

                    world.resources.meta_game.forge_uses_remaining -= 1;
                    world.resources.meta_game.forge_selected_cards.clear();
                    world.resources.meta_game.forge_offered_cards.clear();

                    if world.resources.meta_game.forge_uses_remaining == 0 {
                        world.resources.combat.game_state = GameState::Map;
                    }
                    break;
                }
            }
        }
    }
}

fn map_input_system(world: &mut GameWorld) {
    if world.resources.combat.game_state != GameState::Map {
        return;
    }

    if is_key_pressed(KeyCode::V) {
        world.resources.meta_game.previous_state = world.resources.combat.game_state;
        world.resources.combat.game_state = GameState::DeckView;
        return;
    }

    if is_mouse_button_pressed(MouseButton::Left) {
        let mouse_pos = Vec2::new(mouse_position().0, mouse_position().1);

        let mut clicked_node_index = None;
        let mut clicked_node_type = None;

        for (node_index, node) in world.resources.meta_game.map_data.nodes.iter().enumerate() {
            if !node.available
                || node.visited
                || node_index == world.resources.meta_game.current_node
            {
                continue;
            }

            let pos = get_node_screen_position(&world.resources.meta_game.map_data, node_index);
            let node_radius = 30.0;

            if mouse_pos.distance(pos) <= node_radius {
                clicked_node_index = Some(node_index);
                clicked_node_type = Some(node.node_type);
                break;
            }
        }

        if let Some(node_index) = clicked_node_index {
            world.resources.meta_game.current_node = node_index;
            world.resources.meta_game.map_data.nodes[node_index].visited = true;

            let connections = world.resources.meta_game.map_data.nodes[node_index]
                .connections
                .clone();
            for connection_idx in connections {
                world.resources.meta_game.map_data.nodes[connection_idx].available = true;
            }

            match clicked_node_type.unwrap() {
                NodeType::Combat | NodeType::Elite => {
                    world.resources.combat.game_state = GameState::WaitingForWave;
                    world.resources.combat.wave = 0;
                    let is_elite = clicked_node_type.unwrap() == NodeType::Elite;
                    setup_combat(world, is_elite, false);
                }
                NodeType::Boss => {
                    world.resources.combat.game_state = GameState::WaitingForWave;
                    world.resources.combat.wave = 0;
                    setup_combat(world, false, true);
                }
                NodeType::Shop => {
                    world.resources.meta_game.shop_offerings = generate_shop_offerings(world);
                    world.resources.combat.game_state = GameState::Shop;
                }
                NodeType::Rest => {
                    world.resources.meta_game.rest_options = generate_rest_options();
                    world.resources.combat.game_state = GameState::Rest;
                }
                NodeType::Forge => {
                    world.resources.meta_game.forge_selected_cards.clear();
                    world.resources.meta_game.forge_offered_cards.clear();
                    world.resources.meta_game.forge_uses_remaining = 2;
                    world.resources.combat.game_state = GameState::Forge;
                }
            }
        }
    }
}

fn cleanup_combat_effects(world: &mut GameWorld) {
    let effects_to_remove: Vec<_> = world.query_entities(VISUAL_EFFECT).collect();
    for entity in effects_to_remove {
        world.queue_despawn_entity(entity);
    }

    let popups_to_remove: Vec<_> = world.query_entities(MONEY_POPUP).collect();
    for entity in popups_to_remove {
        world.queue_despawn_entity(entity);
    }

    let towers_to_remove: Vec<_> = world.query_entities(TOWER).collect();
    for entity in towers_to_remove {
        world.queue_despawn_entity(entity);
    }

    let projectiles_to_remove: Vec<_> = world.query_entities(PROJECTILE).collect();
    for entity in projectiles_to_remove {
        world.queue_despawn_entity(entity);
    }

    let enemies_to_remove: Vec<_> = world.query_entities(ENEMY).collect();
    for entity in enemies_to_remove {
        world.queue_despawn_entity(entity);
    }

    world.apply_commands();
}

fn setup_combat(world: &mut GameWorld, is_elite: bool, is_boss: bool) {
    initialize_grid(world);
    create_path(world);

    let layer =
        world.resources.meta_game.map_data.nodes[world.resources.meta_game.current_node].layer;
    let enemy_deck = get_enemy_deck_for_encounter(layer, is_elite, is_boss);

    world.resources.combat.current_encounter_name = enemy_deck.encounter_name.clone();
    world.resources.combat.enemy_deck = enemy_deck.cards;
    world.resources.combat.enemy_deck_play_timer = 0.0;
}

fn generate_shop_offerings(world: &GameWorld) -> Vec<ShopOffering> {
    let mut offerings = Vec::new();

    let card_definitions = get_all_card_definitions();
    for _ in 0..3 {
        let random_index = rand::gen_range(0, card_definitions.len());
        let (name, pattern, rarity) = &card_definitions[random_index];
        let cost = calculate_card_cost(pattern, *rarity, &world.resources.config.tower_configs);
        offerings.push(ShopOffering::Card {
            name: name.to_string(),
            pattern: pattern.clone(),
            rarity: *rarity,
            cost,
        });
    }

    let all_relics = [
        RelicType::CriticalStrike,
        RelicType::DoubleTap,
        RelicType::RapidFire,
        RelicType::SniperNest,
        RelicType::FrostAura,
        RelicType::GoldenTouch,
        RelicType::BulkDiscount,
        RelicType::Recycler,
        RelicType::Overcharge,
        RelicType::PoisonCloud,
        RelicType::CannonBoost,
        RelicType::RangeExtender,
    ];

    let available_relics: Vec<_> = all_relics
        .iter()
        .filter(|r| !world.resources.economy.owned_relics.contains(r))
        .cloned()
        .collect();

    if !available_relics.is_empty() {
        let random_relic = available_relics[rand::gen_range(0, available_relics.len())];
        offerings.push(ShopOffering::Relic {
            relic_type: random_relic,
            cost: random_relic.cost(),
        });
    }

    offerings
}

fn generate_rest_options() -> Vec<RestOption> {
    vec![
        RestOption::Heal { amount: 15 },
        RestOption::UpgradeCard,
        RestOption::RemoveCard,
    ]
}

fn render_ui(world: &GameWorld) {
    let money_text = format!("Money: ${}", world.resources.economy.money);
    draw_text(&money_text, 10.0, 30.0, 30.0, GREEN);

    let lives_text = format!("Lives: {}", world.resources.combat.lives);
    draw_text(&lives_text, 10.0, 60.0, 25.0, RED);

    let (current_hp, max_hp) = if let Some(player_entity) = world.query_entities(PLAYER).next() {
        if let Some(player) = world.get_player(player_entity) {
            (player.health, player.max_health)
        } else {
            (
                world.resources.combat.current_hp,
                world.resources.combat.max_hp,
            )
        }
    } else {
        (
            world.resources.combat.current_hp,
            world.resources.combat.max_hp,
        )
    };

    let hp_text = format!("HP: {}/{}", current_hp, max_hp);
    draw_text(&hp_text, 10.0, 90.0, 25.0, YELLOW);

    if matches!(
        world.resources.combat.game_state,
        GameState::WaitingForWave | GameState::WaveInProgress
    ) {
        let wave_text = format!("Wave: {}/5", world.resources.combat.wave);
        draw_text(&wave_text, screen_width() - 180.0, 30.0, 30.0, SKYBLUE);

        let enemy_count = world.query_entities(ENEMY).count();
        let remaining_cards = world.resources.combat.enemy_deck.len();

        let enemies_text = format!("Enemies: {}", enemy_count);
        draw_text(
            &enemies_text,
            screen_width() - 180.0,
            60.0,
            22.0,
            Color::new(1.0, 0.6, 0.6, 1.0),
        );

        if remaining_cards > 0 {
            let spawning_text = format!("({} cards left)", remaining_cards);
            draw_text(
                &spawning_text,
                screen_width() - 180.0,
                85.0,
                18.0,
                LIGHTGRAY,
            );
        }
    }

    let speed_text = format!("Speed: {}x", world.resources.combat.game_speed);
    draw_text(&speed_text, screen_width() - 180.0, 110.0, 20.0, WHITE);

    let total_hp = (world.resources.combat.lives - 1) * max_hp + current_hp;
    let max_total_hp = world.resources.combat.lives * max_hp;
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

    if world.resources.combat.wave_announce_timer > 0.0 {
        let alpha = if world.resources.combat.wave_announce_timer < 1.0 {
            world.resources.combat.wave_announce_timer
        } else {
            1.0
        };

        let wave_text = format!("WAVE {}", world.resources.combat.wave);
        let text_size = 60.0;
        let text_dims = measure_text(&wave_text, None, text_size as u16, 1.0);
        draw_text(
            &wave_text,
            screen_width() / 2.0 - text_dims.width / 2.0,
            screen_height() / 2.0 - 100.0,
            text_size,
            Color::new(1.0, 0.8, 0.0, alpha),
        );

        if !world.resources.combat.current_encounter_name.is_empty() {
            let encounter_text_size = 30.0;
            let encounter_dims = measure_text(
                &world.resources.combat.current_encounter_name,
                None,
                encounter_text_size as u16,
                1.0,
            );
            draw_text(
                &world.resources.combat.current_encounter_name,
                screen_width() / 2.0 - encounter_dims.width / 2.0,
                screen_height() / 2.0 - 40.0,
                encounter_text_size,
                Color::new(0.9, 0.3, 0.3, alpha),
            );
        }
    }

    match world.resources.combat.game_state {
        GameState::WaitingForWave => {
            if world.resources.combat.wave == 0 {
                let button_width = 300.0;
                let button_height = 80.0;
                let button_x = (screen_width() - button_width) / 2.0;
                let button_y = screen_height() / 2.0 - button_height / 2.0;

                draw_rectangle(
                    button_x,
                    button_y,
                    button_width,
                    button_height,
                    Color::new(0.2, 0.6, 0.3, 1.0),
                );
                draw_rectangle_lines(
                    button_x,
                    button_y,
                    button_width,
                    button_height,
                    3.0,
                    Color::new(0.3, 0.9, 0.4, 1.0),
                );

                let text = "START WAVE";
                let text_size = 32.0;
                let text_dims = measure_text(text, None, text_size as u16, 1.0);
                draw_text(
                    text,
                    button_x + (button_width - text_dims.width) / 2.0,
                    button_y + (button_height + text_size) / 2.0 - 5.0,
                    text_size,
                    WHITE,
                );
            }
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
            draw_rectangle(
                0.0,
                0.0,
                screen_width(),
                screen_height(),
                Color::new(0.0, 0.0, 0.0, 0.7),
            );

            let title = "VICTORY!";
            let title_size = 80.0;
            let title_dims = measure_text(title, None, title_size as u16, 1.0);
            draw_text(
                title,
                screen_width() / 2.0 - title_dims.width / 2.0,
                screen_height() / 2.0 - 80.0,
                title_size,
                Color::new(1.0, 0.85, 0.0, 1.0),
            );

            let subtitle = "You have defeated the final boss!";
            let subtitle_size = 30.0;
            let subtitle_dims = measure_text(subtitle, None, subtitle_size as u16, 1.0);
            draw_text(
                subtitle,
                screen_width() / 2.0 - subtitle_dims.width / 2.0,
                screen_height() / 2.0,
                subtitle_size,
                Color::new(0.8, 1.0, 0.6, 1.0),
            );

            let restart = "Press R to restart";
            let restart_size = 25.0;
            let restart_dims = measure_text(restart, None, restart_size as u16, 1.0);
            draw_text(
                restart,
                screen_width() / 2.0 - restart_dims.width / 2.0,
                screen_height() / 2.0 + 60.0,
                restart_size,
                LIGHTGRAY,
            );
        }
        _ => {}
    }

    let controls_text = "Controls: Click Card then Grid to Place | Right Click: Sell | U/Mid Click: Upgrade | D: Draw Card | V: View Deck | Shift: Preview Card | [/]: Speed | P: Pause";
    draw_text(controls_text, 10.0, screen_height() - 10.0, 13.0, LIGHTGRAY);
}

fn enemy_deck_system_wrapper(world: &mut GameWorld) {
    let delta_time = get_frame_time() * world.resources.combat.game_speed;
    enemy_deck_system(world, delta_time);
}

fn enemy_movement_system_wrapper(world: &mut GameWorld) {
    let delta_time = get_frame_time() * world.resources.combat.game_speed;
    enemy_movement_system(world, delta_time);
}

fn tower_shooting_system_wrapper(world: &mut GameWorld) {
    let delta_time = get_frame_time() * world.resources.combat.game_speed;
    tower_shooting_system(world, delta_time);
}

fn projectile_movement_system_wrapper(world: &mut GameWorld) {
    let delta_time = get_frame_time() * world.resources.combat.game_speed;
    projectile_movement_system(world, delta_time);
}

fn visual_effects_system_wrapper(world: &mut GameWorld) {
    let delta_time = get_frame_time() * world.resources.combat.game_speed;
    visual_effects_system(world, delta_time);
}

fn update_money_popups_wrapper(world: &mut GameWorld) {
    let delta_time = get_frame_time() * world.resources.combat.game_speed;
    update_money_popups(world, delta_time);
}

fn victory_timer_system_wrapper(world: &mut GameWorld) {
    let delta_time = get_frame_time();
    victory_timer_system(world, delta_time);
}

#[macroquad::main("Tower Defense Cards")]
async fn main() {
    let mut world = GameWorld::default();

    world.resources.economy.money = 500;
    world.resources.combat.lives = 1;
    world.resources.combat.wave = 0;
    world.resources.combat.current_hp = 20;
    world.resources.combat.max_hp = 20;
    world.resources.combat.game_state = GameState::Map;
    world.resources.combat.game_speed = 1.0;
    world.resources.ui.selected_tower_type = TowerType::Basic;
    world.resources.card_system.hand_size = 0;
    world.resources.card_system.max_hand_size = 5;
    world.resources.meta_game.map_data = generate_map();
    world.resources.meta_game.current_node = usize::MAX;
    world.resources.meta_game.previous_state = GameState::Map;
    world.resources.meta_game.shop_offerings = Vec::new();
    world.resources.meta_game.rest_options = Vec::new();
    world.resources.combat.victory_timer = 0.0;
    world.resources.meta_game.forge_selected_cards = Vec::new();
    world.resources.meta_game.forge_offered_cards = Vec::new();
    world.resources.meta_game.forge_uses_remaining = 0;
    world.resources.economy.owned_relics = Vec::new();

    let player_entity = world.spawn_entities(PLAYER, 1)[0];
    world.set_player(
        player_entity,
        Player {
            health: 20,
            max_health: 20,
        },
    );

    initialize_grid(&mut world);
    create_path(&mut world);
    create_starter_cards(&mut world);
    draw_random_cards(&mut world, 3);

    let mut game_schedule = Schedule::new();
    game_schedule
        .add_system_mut(victory_timer_system_wrapper)
        .add_system_mut(enemy_deck_system_wrapper)
        .add_system_mut(enemy_movement_system_wrapper)
        .add_system_mut(tower_targeting_system)
        .add_system_mut(tower_shooting_system_wrapper)
        .add_system_mut(projectile_movement_system_wrapper)
        .add_system_mut(damage_handler_system)
        .add_system_mut(visual_effects_system_wrapper)
        .add_system_mut(update_money_popups_wrapper)
        .add_system_mut(enemy_died_event_handler)
        .add_system_mut(health_bar_update_system)
        .add_system_mut(update_card_animations_wrapper);

    let mut render_schedule = Schedule::new();
    render_schedule
        .add_system(render_grid)
        .add_system(render_towers)
        .add_system(render_enemies)
        .add_system(render_projectiles)
        .add_system(render_visual_effects)
        .add_system(render_money_popups)
        .add_system(render_cards)
        .add_system(render_ui)
        .add_system(render_card_preview_overlay);

    loop {
        clear_background(Color::new(0.05, 0.05, 0.05, 1.0));

        if world.resources.combat.game_state == GameState::DeckView {
            deck_view_input_system(&mut world);

            if world.resources.meta_game.previous_state == GameState::Map {
                render_map(&world);
            } else {
                render_schedule.run(&mut world);
            }

            render_deck_view(&world);
        } else if world.resources.combat.game_state == GameState::Shop {
            shop_input_system(&mut world);
            render_shop(&world);
        } else if world.resources.combat.game_state == GameState::Rest {
            rest_input_system(&mut world);
            render_rest(&world);
        } else if world.resources.combat.game_state == GameState::Forge {
            forge_input_system(&mut world);
            render_forge(&world);
        } else if world.resources.combat.game_state == GameState::Map {
            map_input_system(&mut world);
            render_map(&world);
        } else {
            input_system(&mut world);

            if world.resources.combat.game_state != GameState::Paused {
                game_schedule.run(&mut world);
            }

            render_schedule.run(&mut world);
        }

        world.step();
        next_frame().await;
    }
}
