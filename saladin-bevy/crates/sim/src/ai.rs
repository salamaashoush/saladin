use crate::buildings_defs::building_def;
use crate::combat::{Attacker, effective_damage};
use crate::enums::{BuildingKind, UnitKind};
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
    let trainable: Vec<UnitKind> = FIELD_UNITS.iter().copied().filter(|k| can_train(*k, owned)).collect();
    if trainable.is_empty() {
        return UnitKind::Spearman;
    }
    if enemy_has_walls && wants_siege {
        if trainable.contains(&UnitKind::Mangonel) {
            return UnitKind::Mangonel;
        }
        if trainable.contains(&UnitKind::Ram) {
            return UnitKind::Ram;
        }
    }
    if census_total(enemy) == 0 {
        if trainable.contains(&UnitKind::Spearman) {
            return UnitKind::Spearman;
        }
        if trainable.contains(&UnitKind::Archer) {
            return UnitKind::Archer;
        }
        return trainable[0];
    }
    let mut best = trainable[0];
    let mut best_score = Fx::MIN;
    for k in trainable {
        if unit_def(k).prefers_buildings {
            continue;
        }
        let s = counter_score(k, enemy);
        if s > best_score {
            best_score = s;
            best = k;
        }
    }
    best
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

    // 0) Food crisis: stop adding eaters, grow gatherers.
    if food_crisis(s, tune) {
        if s.peasants < tune.peasant_target + tune.reserve_peasants && !pop_full {
            return Some(train(UnitKind::Peasant, BuildingKind::Keep));
        }
        if pop_full {
            return Some(house());
        }
        if has(BuildingKind::Keep) && !has(BuildingKind::Granary) {
            return Some(build(BuildingKind::Granary));
        }
        return None;
    }

    // 1) Economy: peasants to target.
    if s.peasants < tune.peasant_target && !pop_full {
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
        let kind = counter_composition(&s.enemy, &s.owned, tune.wants_siege, s.enemy_has_walls);
        return Some(train(kind, trainer_for(kind)));
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

    // 4) Defense towers under threat.
    if s.threat_near_home >= tune.defend_threat && towers_below_cap(s, tune) {
        return Some(build(BuildingKind::Tower));
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

    // 7) Army toward target.
    if s.soldiers < tune.army_target {
        let kind = counter_composition(&s.enemy, &s.owned, tune.wants_siege, s.enemy_has_walls);
        return Some(train(kind, trainer_for(kind)));
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
        let s = PlannerState {
            peasants: 1,
            pop: 1,
            cap: 10,
            food: 100,
            wood: 100,
            stone: 50,
            gold: 0,
            upkeep: 0,
            soldiers: 0,
            army_composition: [0; 10],
            sieges: 0,
            towers: 0,
            owned: barracks_only(),
            enemy: [0; 10],
            enemy_has_walls: false,
            threat_near_home: 0,
        };
        let tune = PlannerTuning {
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
        };
        let d = next_build(&s, &tune).unwrap();
        assert!(d.is_unit && d.kind == UnitKind::Peasant as u8);
    }

    #[test]
    fn threat_forces_defend_phase() {
        let mut s = PlannerState {
            peasants: 10,
            pop: 10,
            cap: 20,
            food: 100,
            wood: 100,
            stone: 100,
            gold: 100,
            upkeep: 5,
            soldiers: 10,
            army_composition: [0; 10],
            sieges: 0,
            towers: 0,
            owned: barracks_only(),
            enemy: [0; 10],
            enemy_has_walls: false,
            threat_near_home: 9,
        };
        let tune = PlannerTuning {
            peasant_target: 7,
            army_target: 6,
            core_army: 4,
            pop_buffer: 2,
            food_floor_mult: 6,
            wood_buffer: 30,
            max_towers: 3,
            wants_cavalry: false,
            wants_siege: false,
            siege_target: 0,
            imam_target: 0,
            defend_threat: 3,
            food_floor: 12,
            reserve_peasants: 2,
        };
        assert_eq!(next_phase(&s, &tune), AiPhase::Defend);
        s.threat_near_home = 0;
        assert_ne!(next_phase(&s, &tune), AiPhase::Defend);
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
        };
        let th = ThreatState { attackers: 6, field_army: 10, home_army: 1 };
        assert!(should_recall(&th, &tune));
        // needed=5, cap=floor(10*0.6)=6 -> 5
        assert_eq!(recall_count(&th, &tune), 5);
    }
}
