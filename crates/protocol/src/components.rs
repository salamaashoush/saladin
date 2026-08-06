use bevy_ecs::prelude::*;
use saladin_sim::{
    AiDifficulty, AiPhase, BuildingKind, Faction, Fx, GatherState, MORALE_MAX, MatchStatus,
    ResourceType, Stance, Stockpile, UnitKind, V2, unit_def,
};
use serde::{Deserialize, Serialize};

/// Stable, deterministic game id. Bevy `Entity` ids are NOT identical across
/// lockstep clients, so cross-references (targets, keep, garrison host) use this
/// instead. The `GameIndex` resource maps it back to an `Entity`.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GameId(pub u64);

/// The owning player's stable id (0..N for humans, high ids for bots).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Owner(pub u64);

/// The match an entity belongs to.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MatchId(pub u64);

/// Hot positional state — written every move tick. `facing` is a render hint
/// (radians) recomputed from movement; it does not affect the sim.
#[derive(Component, Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Pos {
    pub pos: V2,
    pub facing: Fx,
}

/// What a unit was last told to do. Kept as a `u8` (not an enum) because it
/// rides in `Unit` and is hashed; the constants are the vocabulary.
pub const ORDER_NONE: u8 = 0;
pub const ORDER_MOVE: u8 = 1;
pub const ORDER_ATTACK_MOVE: u8 = 2;
pub const ORDER_ATTACK: u8 = 3;
pub const ORDER_STOP: u8 = 4;

/// 16-way facing: `heading` counts sixteenths of a turn counter-clockwise from
/// +X. Integer by construction — no trig anywhere in the sim.
pub const HEADINGS: u8 = 16;

fn v2_zero() -> V2 {
    V2::ZERO
}

/// A mobile unit: ownership + movement intent + gather/combat state.
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct Unit {
    pub kind: UnitKind,
    pub target: V2,
    pub has_target: bool,
    pub speed: Fx,
    pub gather_state: GatherState,
    pub target_node: u64,
    pub carrying: i32,
    pub carry_type: ResourceType,
    pub harvest_timer: Fx,
    pub hp: i32,
    pub attack_target: u64,
    /// Combat ticks until the next blow. An `Fx` decremented by `COMBAT_DT`
    /// rounded every attack_rate UP to the next tick — a Spearman's declared
    /// 1.0 s was really 1.2 s — and cost a fixed-point subtract per unit per
    /// combat tick for the privilege.
    #[serde(default)]
    pub attack_cd: i32,
    pub stance: Stance,
    pub morale: Fx,
    pub routing: bool,
    pub home: V2,
    pub garrisoned_in: u64,
    /// The building this unit is working on (construction/repair). Separate
    /// from `target_node` on purpose: `retarget` zeroes the node, and a builder
    /// must keep its site across a re-order.
    pub job_site: u64,
    pub path: Vec<V2>,
    pub path_idx: usize,
    #[serde(default)]
    pub heading: u8,
    /// `ORDER_*` — what the player/AI last asked for, as opposed to where the
    /// unit happens to be walking this tick.
    #[serde(default)]
    pub order: u8,
    #[serde(default = "v2_zero")]
    pub order_target: V2,
    /// Where the standing order was issued from. `home` cannot serve: Move,
    /// SetStance and the rout all overwrite it, so it is three things at once.
    #[serde(default = "v2_zero")]
    pub anchor: V2,
    #[serde(default)]
    pub engage_slot: u8,
    #[serde(default)]
    pub charge_cd: i32,
    #[serde(default)]
    pub rally_cd: i32,
    #[serde(default)]
    pub setup_timer: Fx,
    /// Fraction of this unit's ration actually issued last supply tick.
    #[serde(default)]
    pub ration: Fx,
}

impl Unit {
    /// A fresh unit of `kind` standing at `pos`, at full health and idle. Every
    /// construction site spreads from this (`..Unit::new(kind, pos)`) so a new
    /// field lands in one place instead of thirty.
    pub fn new(kind: UnitKind, pos: V2) -> Unit {
        let def = unit_def(kind);
        Unit {
            kind,
            target: pos,
            has_target: false,
            speed: def.speed,
            gather_state: GatherState::Idle,
            target_node: 0,
            carrying: 0,
            carry_type: ResourceType::Wood,
            harvest_timer: Fx::ZERO,
            hp: def.max_hp,
            attack_target: 0,
            attack_cd: 0,
            stance: Stance::Aggressive,
            morale: MORALE_MAX,
            routing: false,
            home: pos,
            garrisoned_in: 0,
            job_site: 0,
            path: Vec::new(),
            path_idx: 0,
            heading: 0,
            order: ORDER_NONE,
            order_target: pos,
            anchor: pos,
            engage_slot: 0,
            charge_cd: 0,
            rally_cd: 0,
            setup_timer: Fx::ZERO,
            ration: Fx::ONE,
        }
    }
}

impl Default for Unit {
    fn default() -> Self {
        Unit::new(UnitKind::Peasant, V2::ZERO)
    }
}

/// Where a structure is in its life. `Damaged` is derived (`Complete` with
/// `hp < max_hp`), not a variant: construction and repair are the same loop.
/// The enum and the construction math live in `saladin_sim` — `operational()`
/// is the one gate every capability check runs through.
pub use saladin_sim::{BuildState, QUEUE_CAP, SITE_HP_PCT, site_start_hp};

/// A static structure.
#[derive(Component, Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Building {
    pub kind: BuildingKind,
    pub hp: i32,
    pub cooldown: Fx,
    pub rally: V2,
    pub state: BuildState,
    /// Labour banked toward finishing a `Site` or an `Upgrading` structure.
    pub work: Fx,
    /// Peasants assigned this tick — recounted from `Unit::job_site`, never
    /// incremented, so a builder that dies mid-job cannot leak a worker.
    pub builders: i32,
    /// What an `Upgrading` structure is becoming; equal to `kind` otherwise.
    pub target_kind: BuildingKind,
    /// Pending `UnitKind as u8` production slots, oldest first.
    pub queue: [u8; QUEUE_CAP],
    pub queue_len: u8,
    /// Work banked toward the unit at the head of the queue.
    pub train_work: Fx,
}

impl Building {
    /// A finished structure, standing at full health.
    pub fn new(kind: BuildingKind, hp: i32, rally: V2) -> Building {
        Building {
            kind,
            hp,
            cooldown: Fx::ZERO,
            rally,
            state: BuildState::Complete,
            work: Fx::ZERO,
            builders: 0,
            target_kind: kind,
            queue: [0; QUEUE_CAP],
            queue_len: 0,
            train_work: Fx::ZERO,
        }
    }

    /// A founded but unbuilt structure: a real target from the tick it is sited.
    pub fn site(kind: BuildingKind, max_hp: i32, rally: V2) -> Building {
        Building { state: BuildState::Site, ..Building::new(kind, site_start_hp(max_hp), rally) }
    }

    pub fn complete(&self) -> bool {
        self.state != BuildState::Site
    }

    pub fn queued(&self) -> &[u8] {
        &self.queue[..self.queue_len as usize]
    }
}

/// A harvestable resource node (position lives in `Pos`).
#[derive(Component, Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ResourceNode {
    pub res_type: ResourceType,
    pub remaining: i32,
    /// Natural maximum — regrowth never carries a node past it.
    #[serde(default)]
    pub cap: i32,
    /// Regained per economy tick. Zero means a finite deposit: a felled wood,
    /// a mined-out seam and a hunted herd stay gone.
    #[serde(default)]
    pub regen: i32,
}

impl ResourceNode {
    /// A finite deposit: timber, ore, a wild herd.
    pub fn deposit(res_type: ResourceType, amount: i32) -> ResourceNode {
        ResourceNode { res_type, remaining: amount, cap: amount, regen: 0 }
    }

    /// A stock that grows back: a sown field, a tended fishery.
    pub fn renewable(res_type: ResourceType, remaining: i32, cap: i32, regen: i32) -> ResourceNode {
        ResourceNode { res_type, remaining, cap, regen }
    }
}

/// Links a node to the structure that produces it — a farm's standing crop.
/// When the building falls, the field goes with it.
#[derive(Component, Clone, Copy, Debug, Serialize, Deserialize)]
pub struct FieldOf(pub u64);

/// Where a field is in its season. Rides beside the `ResourceNode` on the same
/// row, so a crop's stage is sim state a peer can desync on and the renderer
/// can read.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Crop {
    /// LATCHED at full growth, not derived from `remaining` — a reaper drawing
    /// the field down must not un-ripen it at the first sheaf.
    pub ripe: bool,
    /// Economy ticks the crop has stood uncut. Past `FARM_RIPE_GRACE` it lodges.
    pub standing: i32,
}

/// Can this node be worked RIGHT NOW?
///
/// A GROWING CROP CANNOT BE CUT. That single gate is what makes a field a season
/// rather than a bucket: draw can no longer outrun growth, because until the
/// harvest is in there is nothing to take. Every other deposit is takeable while
/// anything is left in it.
pub fn reapable(n: &ResourceNode, is_field: bool, crop: Option<&Crop>) -> bool {
    n.remaining > 0 && (!is_field || crop.is_none_or(|c| c.ripe))
}

/// A player (human or bot) — its own entity carrying the stockpile + faction.
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct Player {
    pub player_id: u64,
    pub name: String,
    pub faction: Faction,
    pub stock: Stockpile,
    pub color: u8,
    pub online: bool,
    pub keep: u64,
    pub defeated: bool,
    pub slot: u8,
    pub tech_mask: u64,
    /// Consecutive starving economy ticks — drives the grace/ramp escalation.
    #[serde(default)]
    pub hunger: i32,
}

/// AI driver state attached to a bot player entity.
#[derive(Component, Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Bot {
    pub host: u64,
    pub difficulty: AiDifficulty,
    pub decision_cd: Fx,
    pub wave_timer: Fx,
    pub phase: AiPhase,
    pub scout_id: u64,
    pub threat_timer: Fx,
    /// Soldier count when the last assault wave launched — the retreat baseline.
    #[serde(default)]
    pub wave_launched: i32,
    /// Seconds before the bot probes the shoreline again. A waterside site scan
    /// walks the whole ring perimeter, so a bot that cannot place a hut must not
    /// re-ask every window — but it must ASK AGAIN: a LATCH here meant one
    /// blocked probe (a peasant standing on the only legal tile, a tightened
    /// siting rule) disabled fishing for that bot for the rest of the match.
    #[serde(default)]
    pub waterside_cd: Fx,
    /// Latch for the famine steer's hysteresis: entered at the food cushion,
    /// left at half again. Without it the whole workforce changes trade every
    /// time the larder crosses one number.
    #[serde(default)]
    pub famine: bool,
}

/// One in-flight/completed research, attached to a player entity (one per tech).
#[derive(Component, Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Research {
    pub owner: u64,
    pub tech: u8,
    pub progress: Fx,
    pub done: bool,
}

/// Marker for an entity whose owning player has been defeated / should despawn
/// at end of tick (deferred cleanup keeps sim mutation ordered).
#[derive(Component, Clone, Copy, Debug)]
pub struct Despawn;

/// One match's lifecycle row (mirrors the SpacetimeDB `match` table). Systems
/// simulate only `Active` matches, so `Paused` freezes one in place.
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct MatchInfo {
    pub match_id: u64,
    pub name: String,
    pub host: u64,
    pub status: MatchStatus,
    pub seed: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sim code copies a `Building` out of the world (`save::snapshot`, the
    /// combat snapshot, every `world.get`). A heap field here breaks all three.
    #[test]
    fn a_building_row_stays_copy() {
        fn assert_copy<T: Copy>() {}
        assert_copy::<Building>();
    }

    #[test]
    fn a_founded_site_is_unfinished_and_worth_raiding() {
        let b = Building::site(BuildingKind::Barracks, 1000, V2::ZERO);
        assert_eq!(b.state, BuildState::Site);
        assert!(!b.complete());
        assert_eq!(b.hp, 100, "a site must be a real, frail target");
        assert_eq!(b.work, Fx::ZERO);
        assert_eq!(b.builders, 0);
        assert_eq!(b.target_kind, BuildingKind::Barracks);
        assert!(b.queued().is_empty());
        assert_eq!(site_start_hp(4), 1, "even the frailest site has a hit point");
    }

    #[test]
    fn a_finished_building_is_complete() {
        let b = Building::new(BuildingKind::Keep, 1500, V2::ZERO);
        assert_eq!(b.state, BuildState::Complete);
        assert!(b.complete());
        assert_eq!(b.hp, 1500);
        assert_eq!(b.target_kind, b.kind);
        assert_eq!(b.train_work, Fx::ZERO);
    }

    #[test]
    fn the_queue_reads_only_its_filled_slots() {
        let mut b = Building::new(BuildingKind::Barracks, 100, V2::ZERO);
        b.queue[0] = UnitKind::Spearman as u8;
        b.queue[1] = UnitKind::Archer as u8;
        b.queue_len = 2;
        assert_eq!(b.queued(), &[UnitKind::Spearman as u8, UnitKind::Archer as u8]);
        assert!(b.queue_len as usize <= QUEUE_CAP);
    }
}
