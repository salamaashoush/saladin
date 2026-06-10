use crate::buildings_defs::building_def;
use crate::combat::{Attacker, effective_damage};
use crate::constants::{MARKET_BUY_RATE, MARKET_RATE};
use crate::enums::{BuildingKind, ResourceType, UnitKind};
use crate::math::{Fx, V2};
use crate::units::unit_def;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Pure strategic AI planner. Holdings + a census in, decisions out — runs
/// deterministically and is byte-for-byte testable. The brain system gathers a
/// snapshot, calls these, and executes via the SAME owner-parameterized helpers
/// a human's commands use. No cheats.

/// A tally of units by `UnitKind` (index == kind).
pub type Census = [i32; 10];

pub fn census_total(c: &Census) -> i32 {
    c.iter().map(|n| (*n).max(0)).sum()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum AiPhase {
    Boot = 0,
    Economy = 1,
    Expand = 2,
    Military = 3,
    Tech = 4,
    Siege = 5,
    Assault = 6,
    Defend = 7,
}

/// The planner's view of one bot, filled from a per-tick scan.
#[derive(Clone, Debug)]
pub struct PlannerState {
    pub peasants: i32,
    pub pop: i32,
    pub cap: i32,
    pub food: i32,
    pub wood: i32,
    pub stone: i32,
    pub gold: i32,
    pub upkeep: i32,
    pub soldiers: i32,
    pub army_composition: Census,
    pub sieges: i32,
    pub towers: i32,
    pub owned: HashSet<BuildingKind>,
    pub enemy: Census,
    pub enemy_has_walls: bool,
    pub threat_near_home: i32,
    /// Open water within building reach of the keep — enables a Fishing Hut.
    pub shore_near: bool,
    /// Standing enemy defensive structures (towers/watchtowers) — weigh into
    /// the assault go/no-go alongside their field army.
    pub enemy_towers: i32,
}

/// Tuning the planner reads — decision QUALITY + cadence, never a handicap.
#[derive(Clone, Copy, Debug)]
pub struct PlannerTuning {
    pub peasant_target: i32,
    pub army_target: i32,
    pub core_army: i32,
    pub pop_buffer: i32,
    pub food_floor_mult: i32,
    pub wood_buffer: i32,
    pub max_towers: i32,
    pub wants_cavalry: bool,
    pub wants_siege: bool,
    pub siege_target: i32,
    pub imam_target: i32,
    pub defend_threat: i32,
    pub food_floor: i32,
    pub reserve_peasants: i32,
    /// Grow the army goal to enemy strength + this margin (0 = static target).
    pub army_match_margin: i32,
    /// Ceiling on the grown army goal.
    pub army_cap: i32,
    /// Ceiling on the grown peasant goal.
    pub peasant_cap: i32,
    /// How many of the top counter kinds the army mixes (1 = monoculture).
    pub mix_size: i32,
    pub wants_market: bool,
    pub wants_fishing: bool,
    /// Below this gold, sell a glut resource at the market for a war chest.
    pub gold_floor: i32,
    /// Wood/stone above this is a glut the market may sell down.
    pub sell_threshold: i32,
}

/// Food crisis: the larder is at/under the floor while an army eats from it.
pub fn food_crisis(s: &PlannerState, tune: &PlannerTuning) -> bool {
    s.upkeep > 0 && s.food <= tune.food_floor
}

/// A trained kind that draws food upkeep (combat units + the Imam).
pub fn eats_food(kind: UnitKind) -> bool {
    unit_def(kind).attack > 0 || kind == UnitKind::Imam
}

/// Combat units a bot can field, in rough tech order (Peasants/Imams excluded).
pub const FIELD_UNITS: [UnitKind; 8] = [
    UnitKind::Spearman,
    UnitKind::Archer,
    UnitKind::Crossbowman,
    UnitKind::Knight,
    UnitKind::HorseArcher,
    UnitKind::Mamluk,
    UnitKind::Mangonel,
    UnitKind::Ram,
];

/// The training hall a unit kind needs.
fn trainer_for(kind: UnitKind) -> BuildingKind {
    for k in [BuildingKind::Keep, BuildingKind::Barracks, BuildingKind::Stable, BuildingKind::SiegeWorkshop] {
        if building_def(k).trains.contains(&kind) {
            return k;
        }
    }
    BuildingKind::Barracks
}

fn can_train(kind: UnitKind, owned: &HashSet<BuildingKind>) -> bool {
    if !owned.contains(&trainer_for(kind)) {
        return false;
    }
    match unit_def(kind).requires {
        None => true,
        Some(req) => owned.contains(&req),
    }
}

/// DPS-weighted score of how well attacker `a` answers the enemy mix, using the
/// SAME damage matrix the live combat loop uses.
pub fn counter_score(a: UnitKind, enemy: &Census) -> Fx {
    let adef = unit_def(a);
    if adef.attack <= 0 || adef.attack_rate <= Fx::ZERO {
        return Fx::ZERO;
    }
    let atk = Attacker {
        attack: Fx::from_num(adef.attack),
        damage_type: adef.damage_type,
        bonus_vs_armor: adef.bonus_vs_armor,
    };
    let mut score = Fx::ZERO;
    let mut total = 0;
    for ek in UnitKind::ALL {
        let n = enemy[*ek as usize];
        if n <= 0 {
            continue;
        }
        let edef = unit_def(*ek);
        total += n;
        let dmg = effective_damage(&atk, edef.armor_class);
        score += (Fx::from_num(dmg) / adef.attack_rate) * Fx::from_num(n);
    }
    if total == 0 { Fx::ZERO } else { score / Fx::from_num(total) }
}

/// Best trainable unit to add next against the enemy mix. Ties break toward the
/// lower `UnitKind` (FIELD_UNITS order + strict `>`), so deterministic.
pub fn counter_composition(
    enemy: &Census,
    owned: &HashSet<BuildingKind>,
    wants_siege: bool,
    enemy_has_walls: bool,
) -> UnitKind {
    next_army_kind(enemy, &[0; 10], owned, wants_siege, enemy_has_walls, 1, i32::MAX)
}

/// Non-siege trainable counters ranked by score, best first. Ties break toward
/// FIELD_UNITS order (stable sort over a tech-ordered scan), so deterministic.
pub fn ranked_counters(enemy: &Census, owned: &HashSet<BuildingKind>) -> Vec<(UnitKind, Fx)> {
    let mut v: Vec<(UnitKind, Fx)> = FIELD_UNITS
        .iter()
        .copied()
        .filter(|k| can_train(*k, owned) && !unit_def(*k).prefers_buildings)
        .map(|k| (k, counter_score(k, enemy)))
        .collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v
}

/// The next unit to train: a score-weighted MIX of the top `mix_size` counters,
/// picking whichever the current army is furthest below its target share of.
/// Kinds whose gold cost exceeds `gold` are skipped when an affordable
/// candidate exists — a bot with no gold engine must never deadlock its
/// training on a cavalry pick it can't pay for.
pub fn next_army_kind(
    enemy: &Census,
    own: &Census,
    owned: &HashSet<BuildingKind>,
    wants_siege: bool,
    enemy_has_walls: bool,
    mix_size: i32,
    gold: i32,
) -> UnitKind {
    if enemy_has_walls && wants_siege {
        if can_train(UnitKind::Mangonel, owned) && unit_def(UnitKind::Mangonel).cost.gold <= gold {
            return UnitKind::Mangonel;
        }
        if can_train(UnitKind::Ram, owned) {
            return UnitKind::Ram;
        }
    }
    let ranked = ranked_counters(enemy, owned);
    if ranked.is_empty() {
        return UnitKind::Spearman;
    }
    if census_total(enemy) == 0 {
        // nothing to counter yet: cheap line infantry
        for k in [UnitKind::Spearman, UnitKind::Archer] {
            if ranked.iter().any(|(r, _)| *r == k) {
                return k;
            }
        }
        return ranked[0].0;
    }
    let mix: Vec<(UnitKind, Fx)> = ranked
        .iter()
        .copied()
        .take(mix_size.max(1) as usize)
        .filter(|(_, s)| *s > Fx::ZERO)
        .collect();
    let mix = if mix.is_empty() { vec![ranked[0]] } else { mix };
    let affordable_exists = mix.iter().any(|(k, _)| unit_def(*k).cost.gold <= gold);
    let score_total: Fx = mix.iter().map(|(_, s)| *s).sum();
    let own_total: i32 = mix.iter().map(|(k, _)| own[*k as usize].max(0)).sum();
    // largest deficit: target share (score/total) minus current share
    let mut best = mix[0].0;
    let mut best_deficit = Fx::MIN;
    for (k, s) in &mix {
        if affordable_exists && unit_def(*k).cost.gold > gold {
            continue;
        }
        let target_share = if score_total > Fx::ZERO { *s / score_total } else { Fx::ZERO };
        let current_share = if own_total > 0 {
            Fx::from_num(own[*k as usize].max(0)) / Fx::from_num(own_total)
        } else {
            Fx::ZERO
        };
        let deficit = target_share - current_share;
        if deficit > best_deficit {
            best_deficit = deficit;
            best = *k;
        }
    }
    best
}

/// Army goal grown to answer the enemy's actual strength, within the cap.
pub fn dynamic_army_target(s: &PlannerState, tune: &PlannerTuning) -> i32 {
    if tune.army_match_margin <= 0 {
        return tune.army_target;
    }
    let matched = census_total(&s.enemy) + tune.army_match_margin;
    tune.army_target.max(matched).min(tune.army_cap.max(tune.army_target))
}

/// Economy goal scales with military ambition: half the extra mouths over the
/// base army target become extra gatherers, within the cap.
pub fn dynamic_peasant_target(s: &PlannerState, tune: &PlannerTuning) -> i32 {
    let extra = (dynamic_army_target(s, tune) - tune.army_target).max(0);
    (tune.peasant_target + extra / 2).min(tune.peasant_cap.max(tune.peasant_target))
}

/// One market order: sell a glut or buy a shortage. `buy` spends gold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TradeDecision {
    pub res: ResourceType,
    pub amount: i32,
    pub buy: bool,
}

/// The single best market move this window, if any. Famine rescue first (gold
/// into food), then war-chest building (deepest glut into gold) — the only
/// gold engine a bot without mines has, and cavalry/siege/tech all cost gold.
pub fn next_trade(s: &PlannerState, tune: &PlannerTuning) -> Option<TradeDecision> {
    if !s.owned.contains(&BuildingKind::Market) {
        return None;
    }
    if food_crisis(s, tune) && s.gold >= MARKET_BUY_RATE {
        let want = (s.upkeep * tune.food_floor_mult).max(20);
        let amount = want.min(s.gold / MARKET_BUY_RATE);
        return Some(TradeDecision { res: ResourceType::Food, amount, buy: true });
    }
    if s.gold >= tune.gold_floor {
        return None;
    }
    let (res, bal) = if s.wood >= s.stone {
        (ResourceType::Wood, s.wood)
    } else {
        (ResourceType::Stone, s.stone)
    };
    let spare = bal - tune.sell_threshold;
    if spare >= MARKET_RATE {
        return Some(TradeDecision { res, amount: spare.min(60), buy: false });
    }
    // a deep food pile beyond any famine cushion is tradeable too
    let cushion = tune.food_floor * 8 + s.upkeep * tune.food_floor_mult * 4;
    let fspare = s.food - cushion;
    if fspare >= MARKET_RATE * 10 {
        return Some(TradeDecision { res: ResourceType::Food, amount: fspare.min(40), buy: false });
    }
    None
}

/// Power a defensive tower adds to the defender side of the assault gate.
pub const TOWER_POWER: Fx = crate::fx!("12");

/// HP-weighted counter-DPS of `mine` against `vs` — the strength estimate both
/// sides of the assault go/no-go use. Durable units count for more than glass.
pub fn army_power(mine: &Census, vs: &Census) -> Fx {
    let mut total = Fx::ZERO;
    for k in UnitKind::ALL {
        let n = mine[*k as usize];
        if n <= 0 {
            continue;
        }
        let dps = counter_score(*k, vs);
        if dps <= Fx::ZERO {
            continue;
        }
        let durability = Fx::from_num(unit_def(*k).max_hp + 100) / Fx::from_num(200);
        total += dps * durability * Fx::from_num(n);
    }
    total
}

/// Launch only with a real strength edge: my power must beat the defender's
/// field army + static defenses by `margin_pct` percent. Negative margin
/// disables the gate (Easy attacks on muster, as before).
pub fn should_assault(mine: &Census, enemy: &Census, enemy_towers: i32, margin_pct: i32) -> bool {
    if margin_pct < 0 {
        return true;
    }
    let my = army_power(mine, enemy);
    let their = army_power(enemy, mine) + Fx::from_num(enemy_towers) * TOWER_POWER;
    if their <= Fx::ZERO {
        return true;
    }
    my * Fx::from_num(100) >= their * Fx::from_num(100 + margin_pct)
}

/// A wave that has bled below `retreat_pct` percent of its launch strength
/// breaks off instead of feeding the rest in. Zero disables.
pub fn should_retreat(launched: i32, alive: i32, retreat_pct: i32) -> bool {
    retreat_pct > 0 && launched > 0 && alive * 100 < launched * retreat_pct
}

/// Transition phase from live state. Threat always wins.
pub fn next_phase(s: &PlannerState, tune: &PlannerTuning) -> AiPhase {
    if s.threat_near_home >= tune.defend_threat {
        return AiPhase::Defend;
    }
    let has = |k: BuildingKind| s.owned.contains(&k);
    let has_barracks = has(BuildingKind::Barracks);
    let economy_ready = s.peasants >= tune.peasant_target;
    let tech_complete = (!tune.wants_cavalry || has(BuildingKind::Stable))
        && (!tune.wants_siege || (has(BuildingKind::Blacksmith) && has(BuildingKind::SiegeWorkshop)));

    if !has_barracks && !economy_ready {
        return AiPhase::Boot;
    }
    if !economy_ready {
        return AiPhase::Economy;
    }
    if !has_barracks {
        return AiPhase::Expand;
    }
    if !tech_complete {
        return AiPhase::Tech;
    }
    if s.soldiers >= tune.army_target {
        if tune.wants_siege && s.sieges < tune.siege_target {
            return AiPhase::Siege;
        }
        return AiPhase::Assault;
    }
    AiPhase::Military
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildDecision {
    /// `UnitKind` when `is_unit`, else `BuildingKind` — both as their u8 value.
    pub kind: u8,
    pub is_unit: bool,
    pub trainer: Option<BuildingKind>,
}

const fn house() -> BuildDecision {
    BuildDecision { kind: BuildingKind::House as u8, is_unit: false, trainer: None }
}
fn train(kind: UnitKind, trainer: BuildingKind) -> BuildDecision {
    BuildDecision { kind: kind as u8, is_unit: true, trainer: Some(trainer) }
}
fn build(kind: BuildingKind) -> BuildDecision {
    BuildDecision { kind: kind as u8, is_unit: false, trainer: None }
}

pub fn count_own_kind(census: &Census, kind: UnitKind) -> i32 {
    census[kind as usize]
}

fn towers_below_cap(s: &PlannerState, tune: &PlannerTuning) -> bool {
    s.towers < tune.max_towers
}

/// The single best macro action to take next. One per call.
pub fn next_build(s: &PlannerState, tune: &PlannerTuning) -> Option<BuildDecision> {
    let has = |k: BuildingKind| s.owned.contains(&k);
    let pop_headroom = s.cap - s.pop;
    let pop_full = pop_headroom <= 0;
    let peasant_goal = dynamic_peasant_target(s, tune);
    let army_goal = dynamic_army_target(s, tune);
    let pick_army = || {
        let kind = next_army_kind(
            &s.enemy,
            &s.army_composition,
            &s.owned,
            tune.wants_siege,
            s.enemy_has_walls,
            tune.mix_size,
            s.gold,
        );
        train(kind, trainer_for(kind))
    };

    // 0) Food crisis: stop adding eaters, grow the food economy.
    if food_crisis(s, tune) {
        if s.peasants < peasant_goal + tune.reserve_peasants && !pop_full {
            return Some(train(UnitKind::Peasant, BuildingKind::Keep));
        }
        if pop_full {
            return Some(house());
        }
        if tune.wants_fishing && s.shore_near && !has(BuildingKind::FishingHut) {
            return Some(build(BuildingKind::FishingHut));
        }
        if has(BuildingKind::Keep) && !has(BuildingKind::Granary) {
            return Some(build(BuildingKind::Granary));
        }
        return None; // next_trade may still buy food with gold
    }

    // 1) Economy: peasants to the (growing) target.
    if s.peasants < peasant_goal && !pop_full {
        return Some(train(UnitKind::Peasant, BuildingKind::Keep));
    }

    // 2) Pop headroom.
    if pop_headroom <= tune.pop_buffer {
        return Some(house());
    }

    // 3) Tech tree, in order. Barracks first.
    if !has(BuildingKind::Barracks) {
        return Some(build(BuildingKind::Barracks));
    }

    // 3a) Defensive core while teching.
    let tech_complete = (!tune.wants_cavalry || has(BuildingKind::Stable))
        && (!tune.wants_siege || (has(BuildingKind::Blacksmith) && has(BuildingKind::SiegeWorkshop)));
    if !tech_complete && s.soldiers < tune.core_army && !pop_full {
        return Some(pick_army());
    }

    // 3b) Economy infrastructure: the Market is the gold engine (cavalry,
    // siege and tech all cost gold), a Granary shortens food hauls, and a
    // shoreline Fishing Hut makes food self-sustaining.
    if tune.wants_market && !has(BuildingKind::Market) {
        return Some(build(BuildingKind::Market));
    }
    if tune.wants_fishing && s.shore_near && !has(BuildingKind::FishingHut) {
        return Some(build(BuildingKind::FishingHut));
    }
    if !has(BuildingKind::Granary) && s.peasants >= 6 {
        return Some(build(BuildingKind::Granary));
    }

    if tune.wants_cavalry && !has(BuildingKind::Stable) {
        return Some(build(BuildingKind::Stable));
    }
    if tune.wants_siege && !has(BuildingKind::Blacksmith) {
        return Some(build(BuildingKind::Blacksmith));
    }
    if tune.wants_siege && has(BuildingKind::Blacksmith) && !has(BuildingKind::SiegeWorkshop) {
        return Some(build(BuildingKind::SiegeWorkshop));
    }

    // 4) Defense under threat: towers to cap, then upgrade to a Watchtower.
    if s.threat_near_home >= tune.defend_threat {
        if towers_below_cap(s, tune) {
            return Some(build(BuildingKind::Tower));
        }
        if has(BuildingKind::Tower) && !has(BuildingKind::Watchtower) {
            return Some(build(BuildingKind::Watchtower));
        }
    }

    if pop_full {
        return Some(house());
    }

    // 5) Imam support once an army forms.
    if tune.imam_target > 0
        && s.soldiers >= 2
        && count_own_kind(&s.army_composition, UnitKind::Imam) < tune.imam_target
    {
        return Some(train(UnitKind::Imam, BuildingKind::Keep));
    }

    // 6) Siege toward target.
    if tune.wants_siege
        && has(BuildingKind::SiegeWorkshop)
        && s.sieges < tune.siege_target
        && (s.soldiers >= 2 || s.enemy_has_walls)
    {
        let siege = if s.enemy_has_walls && can_train(UnitKind::Mangonel, &s.owned) {
            UnitKind::Mangonel
        } else if can_train(UnitKind::Ram, &s.owned) {
            UnitKind::Ram
        } else {
            UnitKind::Mangonel
        };
        return Some(train(siege, BuildingKind::SiegeWorkshop));
    }

    // 7) Army toward the (enemy-tracking) target.
    if s.soldiers < army_goal {
        return Some(pick_army());
    }

    // 8) Top up towers with spare wood.
    if towers_below_cap(s, tune) {
        return Some(build(BuildingKind::Tower));
    }

    None
}

// ── tactical layer ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SquadRole {
    Main = 0,
    Siege = 1,
    Raider = 2,
}

pub const RAIDER_SPEED: Fx = crate::fx!("3.8");
const RAIDER_MAX_HP: i32 = 80;

pub fn squad_role(kind: UnitKind) -> SquadRole {
    let def = unit_def(kind);
    if def.attack <= 0 {
        return SquadRole::Main;
    }
    if def.prefers_buildings {
        return SquadRole::Siege;
    }
    if def.speed >= RAIDER_SPEED && def.max_hp <= RAIDER_MAX_HP {
        return SquadRole::Raider;
    }
    SquadRole::Main
}

#[derive(Clone, Copy, Debug)]
pub struct TacticalTarget {
    pub id: u64,
    pub pos: V2,
}

#[derive(Clone, Debug, Default)]
pub struct AssaultIntel {
    pub keep: Option<TacticalTarget>,
    pub defenses: Vec<TacticalTarget>,
    pub buildings: Vec<TacticalTarget>,
    pub gatherers: Vec<TacticalTarget>,
}

fn nearest(pos: V2, pts: &[TacticalTarget]) -> Option<TacticalTarget> {
    let mut best: Option<TacticalTarget> = None;
    let mut best_d = Fx::MAX;
    for p in pts {
        let dx = p.pos.x - pos.x;
        let dy = p.pos.y - pos.y;
        let d = dx * dx + dy * dy;
        if d < best_d {
            best_d = d;
            best = Some(*p);
        }
    }
    best
}

/// March objective for one unit at `pos` given its role and the intel.
pub fn target_for_role(role: SquadRole, pos: V2, intel: &AssaultIntel) -> Option<TacticalTarget> {
    match role {
        SquadRole::Siege => {
            nearest(pos, &intel.defenses).or_else(|| nearest(pos, &intel.buildings)).or(intel.keep)
        }
        SquadRole::Raider => nearest(pos, &intel.gatherers),
        SquadRole::Main => {
            intel.keep.or_else(|| nearest(pos, &intel.buildings)).or_else(|| nearest(pos, &intel.gatherers))
        }
    }
}

/// How many of `army` to carve off as raiders.
pub fn raid_quota(raider_count: i32, raid_fraction: Fx) -> i32 {
    if raid_fraction <= Fx::ZERO || raider_count <= 0 {
        return 0;
    }
    let frac = (Fx::from_num(raider_count) * raid_fraction).floor().to_num::<i32>().max(1);
    frac.min(raider_count)
}

#[derive(Clone, Copy, Debug)]
pub struct ThreatState {
    pub attackers: i32,
    pub field_army: i32,
    pub home_army: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct TacticalTuning {
    pub defend_threat: i32,
    pub recall_margin: i32,
    pub recall_fraction: Fx,
    pub raid_fraction: Fx,
    pub scouts: bool,
    pub defend_react_delay: Fx,
    pub raid_react_delay: Fx,
    /// Required % power edge before a mustered wave launches (-1 = no gate).
    pub advantage_margin_pct: i32,
    /// Recall the wave when survivors drop below this % of launch strength
    /// (0 = fight to the death).
    pub retreat_pct: i32,
}

pub fn should_recall(th: &ThreatState, tune: &TacticalTuning) -> bool {
    if th.attackers < tune.defend_threat {
        return false;
    }
    if th.field_army <= 0 {
        return false;
    }
    th.attackers > th.home_army + tune.recall_margin
}

pub fn recall_count(th: &ThreatState, tune: &TacticalTuning) -> i32 {
    if !should_recall(th, tune) {
        return 0;
    }
    let needed = (th.attackers - th.home_army).max(0);
    let cap = (Fx::from_num(th.field_army) * tune.recall_fraction).floor().to_num::<i32>().max(1);
    needed.min(cap).min(th.field_army).max(1)
}

pub fn mustered(soldiers: i32, wave_size: i32) -> bool {
    soldiers >= wave_size
}

#[cfg(test)]
mod tests {
    use super::*;

    fn barracks_only() -> HashSet<BuildingKind> {
        let mut s = HashSet::new();
        s.insert(BuildingKind::Keep);
        s.insert(BuildingKind::Barracks);
        s
    }

    fn state(owned: HashSet<BuildingKind>) -> PlannerState {
        PlannerState {
            peasants: 10,
            pop: 10,
            cap: 20,
            food: 100,
            wood: 100,
            stone: 100,
            gold: 100,
            upkeep: 0,
            soldiers: 0,
            army_composition: [0; 10],
            sieges: 0,
            towers: 0,
            owned,
            enemy: [0; 10],
            enemy_has_walls: false,
            threat_near_home: 0,
            shore_near: false,
            enemy_towers: 0,
        }
    }

    fn tuning() -> PlannerTuning {
        PlannerTuning {
            peasant_target: 7,
            army_target: 6,
            core_army: 4,
            pop_buffer: 2,
            food_floor_mult: 6,
            wood_buffer: 30,
            max_towers: 1,
            wants_cavalry: false,
            wants_siege: false,
            siege_target: 0,
            imam_target: 0,
            defend_threat: 4,
            food_floor: 12,
            reserve_peasants: 2,
            army_match_margin: 0,
            army_cap: 6,
            peasant_cap: 7,
            mix_size: 1,
            wants_market: false,
            wants_fishing: false,
            gold_floor: 0,
            sell_threshold: i32::MAX,
        }
    }

    #[test]
    fn counters_archers_with_cavalry_when_available() {
        let mut owned = barracks_only();
        owned.insert(BuildingKind::Stable);
        let mut enemy: Census = [0; 10];
        enemy[UnitKind::Archer as usize] = 5;
        let pick = counter_composition(&enemy, &owned, false, false);
        // a fast slasher (Knight/Mamluk) should out-DPS infantry vs leather archers
        assert!(matches!(pick, UnitKind::Knight | UnitKind::Mamluk | UnitKind::Spearman));
    }

    #[test]
    fn siege_chosen_against_walls() {
        let mut owned = barracks_only();
        owned.insert(BuildingKind::Blacksmith);
        owned.insert(BuildingKind::SiegeWorkshop);
        let enemy: Census = [0; 10];
        let pick = counter_composition(&enemy, &owned, true, true);
        assert!(matches!(pick, UnitKind::Mangonel | UnitKind::Ram));
    }

    #[test]
    fn opening_builds_peasants() {
        let mut s = state(barracks_only());
        s.peasants = 1;
        s.pop = 1;
        s.cap = 10;
        s.gold = 0;
        let d = next_build(&s, &tuning()).unwrap();
        assert!(d.is_unit && d.kind == UnitKind::Peasant as u8);
    }

    #[test]
    fn threat_forces_defend_phase() {
        let mut s = state(barracks_only());
        s.upkeep = 5;
        s.soldiers = 10;
        s.threat_near_home = 9;
        let mut tune = tuning();
        tune.max_towers = 3;
        tune.defend_threat = 3;
        assert_eq!(next_phase(&s, &tune), AiPhase::Defend);
        s.threat_near_home = 0;
        assert_ne!(next_phase(&s, &tune), AiPhase::Defend);
    }

    #[test]
    fn mix_spreads_across_top_counters() {
        let mut owned = barracks_only();
        owned.insert(BuildingKind::Stable);
        let mut enemy: Census = [0; 10];
        enemy[UnitKind::Spearman as usize] = 6;
        enemy[UnitKind::Archer as usize] = 6;
        // train up an army one pick at a time; with mix_size 3 the result must
        // not be a monoculture
        let mut own: Census = [0; 10];
        for _ in 0..12 {
            let k = next_army_kind(&enemy, &own, &owned, false, false, 3, i32::MAX);
            own[k as usize] += 1;
        }
        let kinds_used = own.iter().filter(|n| **n > 0).count();
        assert!(kinds_used >= 2, "mix produced a monoculture: {own:?}");
    }

    #[test]
    fn broke_bot_never_picks_a_gold_unit() {
        let mut owned = barracks_only();
        owned.insert(BuildingKind::Stable);
        let mut enemy: Census = [0; 10];
        enemy[UnitKind::Archer as usize] = 8;
        for _ in 0..8 {
            let k = next_army_kind(&enemy, &[0; 10], &owned, false, false, 3, 0);
            assert_eq!(unit_def(k).cost.gold, 0, "picked unaffordable {k:?} with 0 gold");
        }
    }

    #[test]
    fn army_target_tracks_enemy_strength() {
        let mut s = state(barracks_only());
        let mut tune = tuning();
        tune.army_match_margin = 3;
        tune.army_cap = 20;
        assert_eq!(dynamic_army_target(&s, &tune), 6); // empty enemy: base
        s.enemy[UnitKind::Spearman as usize] = 10;
        assert_eq!(dynamic_army_target(&s, &tune), 13); // 10 + 3
        s.enemy[UnitKind::Archer as usize] = 30;
        assert_eq!(dynamic_army_target(&s, &tune), 20); // capped
        assert!(dynamic_peasant_target(&s, &tune) >= tune.peasant_target);
    }

    #[test]
    fn trade_buys_food_in_famine_and_sells_glut_for_gold() {
        let mut owned = barracks_only();
        owned.insert(BuildingKind::Market);
        let mut s = state(owned);
        let mut tune = tuning();
        tune.gold_floor = 80;
        tune.sell_threshold = 150;

        // famine + gold -> buy food
        s.food = 5;
        s.upkeep = 8;
        s.gold = 50;
        let t = next_trade(&s, &tune).unwrap();
        assert!(t.buy && t.res == ResourceType::Food && t.amount > 0);

        // gold-poor + wood glut -> sell wood
        s.food = 500;
        s.upkeep = 0;
        s.gold = 10;
        s.wood = 400;
        s.stone = 60;
        let t = next_trade(&s, &tune).unwrap();
        assert!(!t.buy && t.res == ResourceType::Wood && t.amount > 0);

        // flush -> no trade
        s.gold = 200;
        assert_eq!(next_trade(&s, &tune), None);

        // no market -> no trade
        s.gold = 10;
        s.owned.remove(&BuildingKind::Market);
        assert_eq!(next_trade(&s, &tune), None);
    }

    #[test]
    fn assault_gate_demands_an_edge_and_retreat_triggers() {
        let mut mine: Census = [0; 10];
        let mut enemy: Census = [0; 10];
        mine[UnitKind::Spearman as usize] = 4;
        enemy[UnitKind::Spearman as usize] = 12;
        assert!(!should_assault(&mine, &enemy, 0, 10), "4 v 12 must not launch");
        mine[UnitKind::Spearman as usize] = 20;
        assert!(should_assault(&mine, &enemy, 0, 10), "20 v 12 should launch");
        // towers tip the balance back
        assert!(!should_assault(&mine, &enemy, 8, 10), "8 towers should deter");
        // negative margin = gate off (Easy)
        mine[UnitKind::Spearman as usize] = 1;
        assert!(should_assault(&mine, &enemy, 8, -1));

        assert!(should_retreat(10, 3, 40)); // 30% alive < 40%
        assert!(!should_retreat(10, 5, 40)); // 50% alive
        assert!(!should_retreat(10, 1, 0)); // disabled
    }

    #[test]
    fn planner_builds_market_then_fishing_hut() {
        let mut s = state(barracks_only());
        let mut tune = tuning();
        tune.wants_market = true;
        tune.wants_fishing = true;
        s.shore_near = true;
        let d = next_build(&s, &tune).unwrap();
        assert!(!d.is_unit && d.kind == BuildingKind::Market as u8);
        s.owned.insert(BuildingKind::Market);
        let d = next_build(&s, &tune).unwrap();
        assert!(!d.is_unit && d.kind == BuildingKind::FishingHut as u8);
        s.owned.insert(BuildingKind::FishingHut);
        let d = next_build(&s, &tune).unwrap();
        assert!(!d.is_unit && d.kind == BuildingKind::Granary as u8);
    }

    #[test]
    fn recall_scales_with_attack() {
        let tune = TacticalTuning {
            defend_threat: 3,
            recall_margin: 0,
            recall_fraction: crate::fx!("0.6"),
            raid_fraction: crate::fx!("0.3"),
            scouts: true,
            defend_react_delay: crate::fx!("1"),
            raid_react_delay: crate::fx!("75"),
            advantage_margin_pct: 10,
            retreat_pct: 35,
        };
        let th = ThreatState { attackers: 6, field_army: 10, home_army: 1 };
        assert!(should_recall(&th, &tune));
        // needed=5, cap=floor(10*0.6)=6 -> 5
        assert_eq!(recall_count(&th, &tune), 5);
    }
}
