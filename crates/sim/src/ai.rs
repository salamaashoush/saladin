use crate::buildings_defs::building_def;
use crate::combat::{Attacker, effective_damage};
use crate::constants::{MARKET_BUY_RATE, MARKET_RATE};
use crate::constants::COMBAT_DT;
use crate::enums::{BuildingKind, Faction, ResourceType, UnitKind, UnitRole};
use crate::roster::fields_unit;
use crate::math::{Fx, V2};
use crate::units::unit_def;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Pure strategic AI planner. Holdings + a census in, decisions out — runs
/// deterministically and is byte-for-byte testable. The brain system gathers a
/// snapshot, calls these, and executes via the SAME owner-parameterized helpers
/// a human's commands use. No cheats.

/// A tally of units by `UnitKind` (index == kind).
pub type Census = [i32; UnitKind::ALL.len()];

/// An empty tally. Written as a const because `[0; 10]` stops compiling the
/// moment the roster grows — which is exactly what it should do.
pub const EMPTY_CENSUS: Census = [0; UnitKind::ALL.len()];

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
    pub faction: Faction,
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
    /// Soil good enough to sow within building reach of the keep.
    pub farmland_near: bool,
    /// Farms carrying a STANDING FIELD. A plot whose crop is gone is not a farm,
    /// it is fifty wood of scenery — counting it kept the bot at its farm target
    /// while its food economy shrank underneath it.
    pub farms: i32,
    /// Fields carrying a harvest big enough to answer a famine with. NOT every
    /// ripe field: `Crop.ripe` latches down to an empty plot, and a ripe crop
    /// stops growing, so a stripped field would read as "the harvest is in" for
    /// the whole grace and the whole bleed after it (see `harvest_standing`).
    pub fields_ripe: i32,
    /// Standing enemy defensive structures (towers/watchtowers) — weigh into
    /// the assault go/no-go alongside their field army.
    pub enemy_towers: i32,
    /// Unfinished sites by `BuildingKind` index. Build time now EXCEEDS a
    /// decision window, so without this the ladder re-sites the same hall every
    /// window until the bot is broke.
    pub sites_in_flight: [i32; 16],
    /// Own COMPLETE buildings below the repair threshold.
    pub damaged: i32,
    /// Peasants already on a job site.
    pub builders_busy: i32,
    pub storehouses: i32,
    /// Own finished Towers that could become Watchtowers.
    pub upgradable_towers: i32,
    /// A worked resource cluster too far from the town to haul from — the
    /// reason to plant a Storehouse.
    pub remote_cluster: Option<V2>,
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
    /// Fields the bot works toward once its economy is standing.
    pub farm_target: i32,
    /// Peasants the bot parks on each standing field to work it.
    pub farm_hands: i32,
    /// Below this gold, sell a glut resource at the market for a war chest.
    pub gold_floor: i32,
    /// Wood/stone above this is a glut the market may sell down.
    pub sell_threshold: i32,
    /// Whether the bot plants forward drop-offs at all.
    pub wants_expansion: bool,
    pub storehouse_target: i32,
    /// Repair anything below this percentage of its health.
    pub repair_threshold: i32,
    /// Peasants pulled onto each new job site.
    pub builders_per_site: i32,
}

/// Food crisis: the larder is at/under the floor while an army eats from it.
pub fn food_crisis(s: &PlannerState, tune: &PlannerTuning) -> bool {
    s.upkeep > 0 && s.food <= tune.food_floor
}

/// How a bot spreads its peasants over its fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldLabour {
    /// Hands kept on each standing field.
    pub per_field: i32,
    /// Hands in fields ACROSS THE WHOLE TOWN. The binding half: a hand in a
    /// field is wood, stone and an army everywhere else, and a bot that answers
    /// "how many hands do I leave in the fields" with "all of them" piles up
    /// food it has nothing to spend on. Measured, that is exactly what happened
    /// — twelve of thirteen peasants in the wheat, six wood in the yard, and a
    /// build ladder frozen for eleven minutes.
    pub budget: i32,
}

/// Peasants a bot commits to its fields. Spread before stacked: `per_field` is
/// filled a layer at a time across every farm, so the diminishing tending curve
/// is answered the way the player answers it.
///
/// A famine with a harvest standing swells both numbers. Cutting a ripe crop is
/// food already grown and already paid for; it comes in before the war chest is
/// spent on somebody else's grain (see `next_trade`).
pub fn field_labour(s: &PlannerState, tune: &PlannerTuning) -> FieldLabour {
    let surge = food_crisis(s, tune) && s.fields_ripe > 0;
    let per_field = if surge { tune.farm_hands + 1 } else { tune.farm_hands }.max(1);
    let budget = if surge { s.peasants * 2 / 3 } else { s.peasants / 2 };
    FieldLabour { per_field, budget: budget.max(0) }
}

/// A trained kind that draws rations. ROLE, not `attack > 0`: the muster roll
/// must not change the day a peasant is given a knife.
pub fn eats_food(kind: UnitKind) -> bool {
    unit_def(kind).draws_rations()
}

/// Combat units a bot can field, in rough tech order (workers and support
/// excluded). Faction filters this, it does not shorten it.
pub const FIELD_UNITS: [UnitKind; 10] = [
    UnitKind::Spearman,
    UnitKind::Archer,
    UnitKind::Crossbowman,
    UnitKind::Naffatun,
    UnitKind::Sergeant,
    UnitKind::Knight,
    UnitKind::HorseArcher,
    UnitKind::Mamluk,
    UnitKind::Mangonel,
    UnitKind::Ram,
];

/// The training hall a unit kind needs.
fn trainer_for(kind: UnitKind) -> BuildingKind {
    for k in [
        BuildingKind::Keep,
        BuildingKind::Barracks,
        BuildingKind::Stable,
        BuildingKind::SiegeWorkshop,
        BuildingKind::Mosque,
    ] {
        if building_def(k).trains.contains(&kind) {
            return k;
        }
    }
    BuildingKind::Barracks
}

fn can_train(kind: UnitKind, owned: &HashSet<BuildingKind>, faction: Faction) -> bool {
    if !fields_unit(kind, faction) {
        return false;
    }
    if !owned.contains(&trainer_for(kind)) {
        return false;
    }
    match unit_def(kind).requires {
        None => true,
        Some(req) => owned.contains(&req),
    }
}

/// Damage per second `a` lands on the enemy MIX, at the cadence the engine can
/// actually deliver (`attack_ticks`, not the nominal `attack_rate` — eight kinds
/// disagreed by up to 20%). With no enemy on the board, its raw output.
pub fn counter_dps(a: UnitKind, enemy: &Census) -> Fx {
    let adef = unit_def(a);
    if adef.attack <= 0 || adef.attack_ticks <= 0 {
        return Fx::ZERO;
    }
    let cadence = Fx::from_num(adef.attack_ticks) * COMBAT_DT;
    let atk = Attacker {
        attack: Fx::from_num(adef.attack),
        damage_type: adef.damage_type,
        bonus_vs_armor: adef.bonus_vs_armor,
    };
    let mut dmg_sum = Fx::ZERO;
    let mut total = 0;
    for ek in UnitKind::ALL {
        let n = enemy[*ek as usize];
        if n <= 0 {
            continue;
        }
        total += n;
        dmg_sum += Fx::from_num(effective_damage(&atk, unit_def(*ek).armor_class)) * Fx::from_num(n);
    }
    let per_hit = if total == 0 {
        Fx::from_num(effective_damage(&atk, crate::enums::ArmorClass::Leather))
    } else {
        dmg_sum / Fx::from_num(total)
    };
    per_hit / cadence
}

/// Absolute fighting strength of one of `a` against the enemy mix: what it deals
/// times how long it lives. Scaled down by 100 so a tower's static contribution
/// stays a readable number.
pub fn unit_power(a: UnitKind, enemy: &Census) -> Fx {
    let d = unit_def(a);
    counter_dps(a, enemy) * Fx::from_num(d.max_hp) / Fx::from_num(100)
}

/// VALUE of adding one `a` against the enemy mix — strength per resource spent.
/// The old score was cost-BLIND, so a planner comparing 16 Mamluks with 51
/// Spearmen preferred the Mamluks on a per-unit reading of the same pile of
/// wood.
pub fn counter_score(a: UnitKind, enemy: &Census) -> Fx {
    let d = unit_def(a);
    if d.attack <= 0 {
        return Fx::ZERO;
    }
    unit_power(a, enemy) / Fx::from_num(d.resource_cost())
}

/// Best trainable unit to add next against the enemy mix. Ties break toward the
/// lower `UnitKind` (FIELD_UNITS order + strict `>`), so deterministic.
pub fn counter_composition(
    enemy: &Census,
    owned: &HashSet<BuildingKind>,
    faction: Faction,
    wants_siege: bool,
    enemy_has_walls: bool,
) -> UnitKind {
    next_army_kind(enemy, &EMPTY_CENSUS, owned, faction, wants_siege, enemy_has_walls, 1, i32::MAX)
}

/// Non-siege trainable counters ranked by score, best first. Ties break toward
/// FIELD_UNITS order (stable sort over a tech-ordered scan), so deterministic.
pub fn ranked_counters(
    enemy: &Census,
    owned: &HashSet<BuildingKind>,
    faction: Faction,
) -> Vec<(UnitKind, Fx)> {
    let mut v: Vec<(UnitKind, Fx)> = FIELD_UNITS
        .iter()
        .copied()
        .filter(|k| can_train(*k, owned, faction) && unit_def(*k).role != UnitRole::Siege)
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
#[allow(clippy::too_many_arguments)]
pub fn next_army_kind(
    enemy: &Census,
    own: &Census,
    owned: &HashSet<BuildingKind>,
    faction: Faction,
    wants_siege: bool,
    enemy_has_walls: bool,
    mix_size: i32,
    gold: i32,
) -> UnitKind {
    if enemy_has_walls && wants_siege {
        if can_train(UnitKind::Mangonel, owned, faction)
            && unit_def(UnitKind::Mangonel).cost.gold <= gold
        {
            return UnitKind::Mangonel;
        }
        if can_train(UnitKind::Ram, owned, faction) {
            return UnitKind::Ram;
        }
    }
    let ranked = ranked_counters(enemy, owned, faction);
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
    // A harvest standing in the fields is food already grown and already paid
    // for. Cut it before buying somebody else's grain — `field_labour` has
    // already swelled the crew that is cutting it.
    if food_crisis(s, tune) && s.fields_ripe == 0 && s.gold >= MARKET_BUY_RATE {
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

/// Power a defensive tower adds to the defender side of the assault gate, in the
/// same units as `unit_power` (roughly one line infantryman).
pub const TOWER_POWER: Fx = crate::fx!("7");

/// HP-weighted counter-DPS of `mine` against `vs` — the strength estimate both
/// sides of the assault go/no-go use. Durable units count for more than glass.
pub fn army_power(mine: &Census, vs: &Census) -> Fx {
    let mut total = Fx::ZERO;
    for k in UnitKind::ALL {
        let n = mine[*k as usize];
        if n <= 0 {
            continue;
        }
        total += unit_power(*k, vs) * Fx::from_num(n);
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

/// What the executor should DO with the decision. The bot plays the whole
/// roster through the same commands a human uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BuildAction {
    Build = 0,
    Train = 1,
    Upgrade = 2,
    Repair = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildDecision {
    /// `UnitKind` when `is_unit`, else `BuildingKind` — both as their u8 value.
    /// For `Upgrade`/`Repair` it names the kind to act ON; the executor picks
    /// which of its buildings (lowest `GameId` wins, so peers agree).
    pub kind: u8,
    pub is_unit: bool,
    pub trainer: Option<BuildingKind>,
    pub action: BuildAction,
}

const fn house() -> BuildDecision {
    BuildDecision {
        kind: BuildingKind::House as u8,
        is_unit: false,
        trainer: None,
        action: BuildAction::Build,
    }
}
fn train(kind: UnitKind, trainer: BuildingKind) -> BuildDecision {
    BuildDecision {
        kind: kind as u8,
        is_unit: true,
        trainer: Some(trainer),
        action: BuildAction::Train,
    }
}
fn build(kind: BuildingKind) -> BuildDecision {
    BuildDecision { kind: kind as u8, is_unit: false, trainer: None, action: BuildAction::Build }
}
fn upgrade(kind: BuildingKind) -> BuildDecision {
    BuildDecision { kind: kind as u8, is_unit: false, trainer: None, action: BuildAction::Upgrade }
}
fn repair() -> BuildDecision {
    BuildDecision { kind: 0, is_unit: false, trainer: None, action: BuildAction::Repair }
}

pub fn count_own_kind(census: &Census, kind: UnitKind) -> i32 {
    census[kind as usize]
}

fn towers_below_cap(s: &PlannerState, tune: &PlannerTuning) -> bool {
    s.towers < tune.max_towers
}

/// The single best macro action to take next. One per call.
///
/// Every rung that sites a structure also checks `sites_in_flight`: a build now
/// takes longer than a decision window, so a ladder that only asked "do I own
/// one?" would found the same hall every window until the bot went broke. A
/// rung whose kind is already rising simply falls through to the next.
pub fn next_build(s: &PlannerState, tune: &PlannerTuning) -> Option<BuildDecision> {
    let has = |k: BuildingKind| s.owned.contains(&k);
    let rising = |k: BuildingKind| s.sites_in_flight.get(k as usize).copied().unwrap_or(0) > 0;
    let need = |k: BuildingKind| !has(k) && !rising(k);
    let pop_headroom = s.cap - s.pop;
    let pop_full = pop_headroom <= 0;
    let peasant_goal = dynamic_peasant_target(s, tune);
    let army_goal = dynamic_army_target(s, tune);
    let pick_army = || {
        let kind = next_army_kind(
            &s.enemy,
            &s.army_composition,
            &s.owned,
            s.faction,
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
        if pop_full && !rising(BuildingKind::House) {
            return Some(house());
        }
        // A field is the only food that grows back, so a starving bot sows
        // before it does anything else with wood — UNLESS a harvest is already
        // standing, in which case the answer is hands, not another foundation
        // that comes in a season too late.
        if s.fields_ripe == 0 {
            if s.farmland_near
                && s.farms + s.sites_in_flight[BuildingKind::Farm as usize] < tune.farm_target + 2
                && s.wood >= 45
            {
                return Some(build(BuildingKind::Farm));
            }
            if tune.wants_fishing && s.shore_near && need(BuildingKind::FishingHut) {
                return Some(build(BuildingKind::FishingHut));
            }
        }
        if s.farms > 0 && need(BuildingKind::Granary) {
            return Some(build(BuildingKind::Granary));
        }
        // Ground that cannot be sown and a shore that is not there: the market
        // IS the food economy, and `next_trade` cannot open one. Measured on a
        // soil-poor start, a bot stood at zero food holding 464 gold, 672 wood
        // and 1634 stone because a famine never let the ladder past a farm rung
        // it had nowhere to put.
        if tune.wants_market && need(BuildingKind::Market) {
            return Some(build(BuildingKind::Market));
        }
        return None; // next_trade may still buy food with gold
    }

    // 0a) Fix what is falling down before adding to it — construction and
    // repair are the same loop, so a bot that can build can mend.
    if tune.repair_threshold > 0 && s.damaged > 0 {
        return Some(repair());
    }

    // 1) Economy: peasants to the (growing) target.
    if s.peasants < peasant_goal && !pop_full {
        return Some(train(UnitKind::Peasant, BuildingKind::Keep));
    }

    // 2) Pop headroom.
    if pop_headroom <= tune.pop_buffer && !rising(BuildingKind::House) {
        return Some(house());
    }

    // 3) Tech tree, in order. Barracks first.
    if need(BuildingKind::Barracks) {
        return Some(build(BuildingKind::Barracks));
    }

    // 3a) Defensive core while teching.
    let tech_complete = (!tune.wants_cavalry || has(BuildingKind::Stable))
        && (!tune.wants_siege || (has(BuildingKind::Blacksmith) && has(BuildingKind::SiegeWorkshop)));
    if !tech_complete && has(BuildingKind::Barracks) && s.soldiers < tune.core_army && !pop_full {
        return Some(pick_army());
    }

    // 3b) Economy infrastructure: fields feed, the Granary hubs them, the
    // Market is the gold engine (cavalry, siege, tech and the Mosque all cost
    // gold), and a shoreline hut makes food self-sustaining.
    if s.farmland_near && s.farms + s.sites_in_flight[BuildingKind::Farm as usize] < tune.farm_target
    {
        return Some(build(BuildingKind::Farm));
    }
    if tune.wants_market && need(BuildingKind::Market) {
        return Some(build(BuildingKind::Market));
    }
    if tune.wants_fishing && s.shore_near && need(BuildingKind::FishingHut) {
        return Some(build(BuildingKind::FishingHut));
    }
    // a hub with nothing to hub is a wasted 50 wood
    if s.farms > 0 && need(BuildingKind::Granary) {
        return Some(build(BuildingKind::Granary));
    }

    if tune.wants_cavalry && need(BuildingKind::Blacksmith) {
        return Some(build(BuildingKind::Blacksmith));
    }
    if tune.wants_cavalry && has(BuildingKind::Blacksmith) && need(BuildingKind::Stable) {
        return Some(build(BuildingKind::Stable));
    }
    if tune.wants_siege && need(BuildingKind::Blacksmith) {
        return Some(build(BuildingKind::Blacksmith));
    }
    if tune.wants_siege && has(BuildingKind::Blacksmith) && need(BuildingKind::SiegeWorkshop) {
        return Some(build(BuildingKind::SiegeWorkshop));
    }

    // 3c) Expansion: a Storehouse at a worked cluster is the only way a town
    // reaches past its own radius. It sits BELOW the tech rungs: a cluster the
    // town cannot legally reach yet must never stall the whole ladder.
    if tune.wants_expansion
        && s.remote_cluster.is_some()
        && s.storehouses + s.sites_in_flight[BuildingKind::Storehouse as usize]
            < tune.storehouse_target
    {
        return Some(build(BuildingKind::Storehouse));
    }


    // 4) Defense under threat: towers to cap, then RAISE one where it already
    // stands rather than buying a second tower on new ground.
    if s.threat_near_home >= tune.defend_threat {
        if towers_below_cap(s, tune) && !rising(BuildingKind::Tower) {
            return Some(build(BuildingKind::Tower));
        }
        if s.upgradable_towers > 0 {
            return Some(upgrade(BuildingKind::Tower));
        }
    }

    if pop_full && !rising(BuildingKind::House) {
        return Some(house());
    }

    // 5) Imam support once an army forms — which needs the mosque that trains
    // them and steadies the ground they hold.
    let preacher = if s.faction == Faction::Crusader { UnitKind::Chaplain } else { UnitKind::Imam };
    if tune.imam_target > 0
        && s.soldiers >= 2
        && count_own_kind(&s.army_composition, preacher) < tune.imam_target
    {
        if need(BuildingKind::Mosque) {
            return Some(build(BuildingKind::Mosque));
        }
        if has(BuildingKind::Mosque) {
            return Some(train(preacher, BuildingKind::Mosque));
        }
    }

    // 6) Siege toward target.
    if tune.wants_siege
        && has(BuildingKind::SiegeWorkshop)
        && s.sieges < tune.siege_target
        && (s.soldiers >= 2 || s.enemy_has_walls)
    {
        let siege = if s.enemy_has_walls && can_train(UnitKind::Mangonel, &s.owned, s.faction) {
            UnitKind::Mangonel
        } else if can_train(UnitKind::Ram, &s.owned, s.faction) {
            UnitKind::Ram
        } else {
            UnitKind::Mangonel
        };
        return Some(train(siege, BuildingKind::SiegeWorkshop));
    }

    // 7) Army toward the (enemy-tracking) target.
    if s.soldiers < army_goal && has(BuildingKind::Barracks) {
        return Some(pick_army());
    }

    // 8) Spare wood: another picket, or raise the one standing.
    if towers_below_cap(s, tune) && !rising(BuildingKind::Tower) {
        return Some(build(BuildingKind::Tower));
    }
    if s.upgradable_towers > 0 {
        return Some(upgrade(BuildingKind::Tower));
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
            faction: Faction::Ayyubid,
            peasants: 10,
            pop: 10,
            cap: 20,
            food: 100,
            wood: 100,
            stone: 100,
            gold: 100,
            upkeep: 0,
            soldiers: 0,
            army_composition: EMPTY_CENSUS,
            sieges: 0,
            towers: 0,
            owned,
            enemy: EMPTY_CENSUS,
            enemy_has_walls: false,
            threat_near_home: 0,
            shore_near: false,
            farmland_near: false,
            farms: 0,
            fields_ripe: 0,
            enemy_towers: 0,
            sites_in_flight: [0; 16],
            damaged: 0,
            builders_busy: 0,
            storehouses: 0,
            upgradable_towers: 0,
            remote_cluster: None,
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
            farm_target: 0,
            farm_hands: 1,
            gold_floor: 0,
            sell_threshold: i32::MAX,
            wants_expansion: false,
            storehouse_target: 0,
            repair_threshold: 0,
            builders_per_site: 1,
        }
    }

    #[test]
    fn counters_archers_with_cavalry_when_available() {
        let mut owned = barracks_only();
        owned.insert(BuildingKind::Stable);
        let mut enemy: Census = EMPTY_CENSUS;
        enemy[UnitKind::Archer as usize] = 5;
        let pick = counter_composition(&enemy, &owned, Faction::Ayyubid, false, false);
        // a fast slasher (Knight/Mamluk) should out-DPS infantry vs leather archers
        assert!(matches!(pick, UnitKind::Knight | UnitKind::Mamluk | UnitKind::Spearman));
    }

    #[test]
    fn siege_chosen_against_walls() {
        let mut owned = barracks_only();
        owned.insert(BuildingKind::Blacksmith);
        owned.insert(BuildingKind::SiegeWorkshop);
        let enemy: Census = EMPTY_CENSUS;
        let pick = counter_composition(&enemy, &owned, Faction::Ayyubid, true, true);
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
        let mut enemy: Census = EMPTY_CENSUS;
        enemy[UnitKind::Spearman as usize] = 6;
        enemy[UnitKind::Archer as usize] = 6;
        // train up an army one pick at a time; with mix_size 3 the result must
        // not be a monoculture
        let mut own: Census = EMPTY_CENSUS;
        for _ in 0..12 {
            let k = next_army_kind(&enemy, &own, &owned, Faction::Ayyubid, false, false, 3, i32::MAX);
            own[k as usize] += 1;
        }
        let kinds_used = own.iter().filter(|n| **n > 0).count();
        assert!(kinds_used >= 2, "mix produced a monoculture: {own:?}");
    }

    #[test]
    fn broke_bot_never_picks_a_gold_unit() {
        let mut owned = barracks_only();
        owned.insert(BuildingKind::Stable);
        let mut enemy: Census = EMPTY_CENSUS;
        enemy[UnitKind::Archer as usize] = 8;
        for _ in 0..8 {
            let k = next_army_kind(&enemy, &EMPTY_CENSUS, &owned, Faction::Ayyubid, false, false, 3, 0);
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

    /// The measured failure this exists to stop: twelve of thirteen peasants
    /// tending wheat, six wood in the yard, and a build ladder frozen for the
    /// rest of the match.
    #[test]
    fn hands_in_the_fields_never_eat_the_whole_workforce() {
        let mut s = state(barracks_only());
        let mut tune = tuning();
        tune.farm_hands = 3;
        s.peasants = 13;
        s.farms = 7;
        let lab = field_labour(&s, &tune);
        assert_eq!(lab.per_field, 3);
        assert_eq!(lab.budget, 6, "fields took more than half the town");
        assert!(
            lab.budget < s.farms * lab.per_field,
            "the budget must BIND at the farm target, or it is not a budget"
        );
        // and it scales with the workforce rather than being a magic number
        s.peasants = 4;
        assert_eq!(field_labour(&s, &tune).budget, 2);
        s.peasants = 0;
        assert_eq!(field_labour(&s, &tune).budget, 0);
        // a profile that wants no crew at all still spreads one hand per field
        tune.farm_hands = 0;
        assert_eq!(field_labour(&s, &tune).per_field, 1);
    }

    #[test]
    fn a_famine_cuts_the_standing_harvest_before_it_buys_grain() {
        let mut owned = barracks_only();
        owned.insert(BuildingKind::Market);
        let mut s = state(owned);
        let mut tune = tuning();
        tune.farm_hands = 2;
        s.peasants = 12;
        s.food = 5;
        s.upkeep = 8;
        s.gold = 200;
        s.farms = 3;
        assert!(food_crisis(&s, &tune));

        // nothing ripe: the market is the only food there is
        let t = next_trade(&s, &tune).unwrap();
        assert!(t.buy && t.res == ResourceType::Food);
        let lean = field_labour(&s, &tune);

        // a harvest standing: swell the crew, keep the war chest
        s.fields_ripe = 2;
        assert!(
            next_trade(&s, &tune).is_none_or(|t| !t.buy),
            "bought grain with a crop standing in the field"
        );
        let surge = field_labour(&s, &tune);
        assert!(surge.per_field > lean.per_field, "famine did not put more hands on the crop");
        assert!(surge.budget > lean.budget);
        assert!(surge.budget <= s.peasants, "a surge is still not the whole town");

        // and once it is cut the market opens again
        s.fields_ripe = 0;
        assert!(next_trade(&s, &tune).unwrap().buy);
    }

    #[test]
    fn a_starving_bot_with_a_crop_in_reach_sows_no_new_foundation() {
        let mut s = state(barracks_only());
        let mut tune = tuning();
        tune.farm_target = 4;
        s.farmland_near = true;
        s.food = 5;
        s.upkeep = 8;
        s.wood = 200;
        s.peasants = 20; // past the peasant goal, so the ladder reaches the farm rung
        s.pop = 20;
        s.cap = 30; // and pop headroom, so it is not answered with a House
        assert_eq!(
            next_build(&s, &tune).map(|d| d.kind),
            Some(BuildingKind::Farm as u8),
            "a famine with no crop standing sows"
        );
        s.fields_ripe = 1;
        assert!(
            next_build(&s, &tune).map(|d| d.kind) != Some(BuildingKind::Farm as u8),
            "spent 45 wood on a season that arrives after the famine"
        );
    }

    /// The measured deadlock: soil the bot cannot sow, no shore, a famine that
    /// therefore never ends, and a ladder that never reaches the one rung that
    /// could feed it. It sat at zero food on 464 gold and 1634 stone.
    #[test]
    fn a_famine_on_ground_that_cannot_be_sown_still_reaches_the_market() {
        let mut s = state(barracks_only());
        let mut tune = tuning();
        tune.wants_market = true;
        tune.wants_fishing = true;
        tune.farm_target = 4;
        s.food = 5;
        s.upkeep = 8;
        s.wood = 200;
        s.stone = 200;
        s.gold = 400;
        s.peasants = 20;
        s.pop = 20;
        s.cap = 30;
        assert!(food_crisis(&s, &tune));
        assert_eq!(next_trade(&s, &tune), None, "no market, so no grain to buy");
        assert_eq!(
            next_build(&s, &tune).map(|d| d.kind),
            Some(BuildingKind::Market as u8),
            "starved holding a war chest it had no way to spend"
        );

        // but ground it CAN sow is still answered with a field first: a market
        // eats a war chest, a farm makes food
        s.farmland_near = true;
        assert_eq!(next_build(&s, &tune).map(|d| d.kind), Some(BuildingKind::Farm as u8));
        // ...and once the ladder has suppressed a farm rung it cannot place,
        // the market is what it falls through to
        s.sites_in_flight[BuildingKind::Farm as usize] = 1000;
        assert_eq!(next_build(&s, &tune).map(|d| d.kind), Some(BuildingKind::Market as u8));
        // a shore is still cheaper food than a merchant
        s.shore_near = true;
        assert_eq!(next_build(&s, &tune).map(|d| d.kind), Some(BuildingKind::FishingHut as u8));

        // and a bot that already trades does not found a second market
        let mut owned = barracks_only();
        owned.insert(BuildingKind::Market);
        let mut s2 = state(owned);
        s2.food = 5;
        s2.upkeep = 8;
        s2.peasants = 20;
        s2.pop = 20;
        s2.cap = 30;
        assert_eq!(next_build(&s2, &tune), None);
    }

    #[test]
    fn assault_gate_demands_an_edge_and_retreat_triggers() {
        let mut mine: Census = EMPTY_CENSUS;
        let mut enemy: Census = EMPTY_CENSUS;
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
        // a hub with nothing to hub is 50 wasted wood
        let d = next_build(&s, &tune).unwrap();
        assert!(d.kind != BuildingKind::Granary as u8, "granary with no farm");
        s.farms = 1;
        let d = next_build(&s, &tune).unwrap();
        assert!(!d.is_unit && d.kind == BuildingKind::Granary as u8);
    }

    #[test]
    fn the_ladder_never_re_sites_what_is_already_rising() {
        let mut owned = HashSet::new();
        owned.insert(BuildingKind::Keep);
        let mut s = state(owned);
        let tune = tuning();
        // with no barracks the ladder asks for one...
        let d = next_build(&s, &tune).unwrap();
        assert_eq!(d.kind, BuildingKind::Barracks as u8);
        assert_eq!(d.action, BuildAction::Build);
        // ...and stops asking the moment one is under construction
        s.sites_in_flight[BuildingKind::Barracks as usize] = 1;
        for _ in 0..6 {
            let Some(d) = next_build(&s, &tune) else { break };
            assert_ne!(d.kind, BuildingKind::Barracks as u8, "founded a second barracks");
        }
    }

    #[test]
    fn a_threatened_tower_is_raised_not_re_bought() {
        let mut s = state(barracks_only());
        let mut tune = tuning();
        tune.max_towers = 1;
        tune.defend_threat = 2;
        s.threat_near_home = 5;
        s.towers = 1;
        s.upgradable_towers = 1;
        let d = next_build(&s, &tune).unwrap();
        assert_eq!(d.action, BuildAction::Upgrade);
        assert_eq!(d.kind, BuildingKind::Tower as u8);
        // and a Watchtower is never founded outright
        assert!(!crate::buildings_defs::building_def(BuildingKind::Watchtower).buildable);
    }

    #[test]
    fn damage_is_mended_before_the_town_grows() {
        let mut s = state(barracks_only());
        let mut tune = tuning();
        tune.repair_threshold = 60;
        s.damaged = 1;
        assert_eq!(next_build(&s, &tune).unwrap().action, BuildAction::Repair);
        // an Easy bot with no repair budget carries on as before
        tune.repair_threshold = 0;
        assert_ne!(next_build(&s, &tune).unwrap().action, BuildAction::Repair);
    }

    #[test]
    fn a_worked_cluster_out_of_reach_earns_a_storehouse() {
        let mut s = state(barracks_only());
        let mut tune = tuning();
        tune.wants_expansion = true;
        tune.storehouse_target = 1;
        assert!(
            next_build(&s, &tune).map(|d| d.kind) != Some(BuildingKind::Storehouse as u8),
            "nothing remote to serve"
        );
        s.remote_cluster = Some(V2::new(crate::fx!("200"), crate::fx!("60")));
        let d = next_build(&s, &tune).unwrap();
        assert_eq!(d.kind, BuildingKind::Storehouse as u8);
        s.storehouses = 1;
        assert_ne!(
            next_build(&s, &tune).map(|d| d.kind),
            Some(BuildingKind::Storehouse as u8),
            "one is the target"
        );
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
