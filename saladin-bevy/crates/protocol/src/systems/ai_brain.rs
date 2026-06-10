use crate::commands::{assign_idle_gatherers, build, path_to, start_research, train};
use crate::components::*;
use bevy_ecs::prelude::*;
use bevy_platform::collections::HashMap;
use saladin_sim::*;
use std::collections::HashSet as StdSet;

const AI_BRAIN_DT: Fx = saladin_sim::AI_BRAIN_DT;
const HOME_THREAT_RADIUS: Fx = saladin_sim::fx!("24"); // enemy combatants this close to home = a threat
const HOME_RADIUS: Fx = saladin_sim::fx!("18"); // own combat units this close to a building count as "home"

fn is_combat(kind: UnitKind) -> bool {
    unit_def(kind).attack > 0
}
fn is_siege(kind: UnitKind) -> bool {
    unit_def(kind).prefers_buildings
}

struct BotSnap {
    entity: Entity,
    player_id: u64,
    difficulty: AiDifficulty,
    decision_cd: Fx,
    wave_timer: Fx,
    threat_timer: Fx,
    scout_id: u64,
    faction: Faction,
    match_id: u64,
    defeated: bool,
}

#[derive(Clone, Copy)]
struct USnap {
    id: u64,
    entity: Entity,
    pos: V2,
    owner: u64,
    kind: UnitKind,
    routing: bool,
    match_id: u64,
    gather_state: GatherState,
    target_node: u64,
    garrisoned: bool,
}

/// Strategic skirmish AI. Under lockstep every client runs this identically (the
/// planner is deterministic over deterministic state), so bots need no network.
/// Full port of `aiBrain`: per-bot PlannerState → next_phase/next_build →
/// train/build/research, three-tier gatherer steering (idle bias + committed
/// re-steer), sustained-threat defensive recall, mustered assault waves with a
/// raider carve-off, and scouting on Hard.
pub fn ai_brain(world: &mut World) {
    // ── snapshots ────────────────────────────────────────────────────────────
    let bots: Vec<BotSnap> = {
        let mut q = world.query::<(Entity, &Player, &Bot, &MatchId)>();
        q.iter(world)
            .map(|(e, p, b, m)| BotSnap {
                entity: e,
                player_id: p.player_id,
                difficulty: b.difficulty,
                decision_cd: b.decision_cd,
                wave_timer: b.wave_timer,
                threat_timer: b.threat_timer,
                scout_id: b.scout_id,
                faction: p.faction,
                match_id: m.0,
                defeated: p.defeated,
            })
            .collect()
    };
    if bots.is_empty() {
        return;
    }

    let faction_of: HashMap<u64, Faction> = {
        let mut q = world.query::<&Player>();
        q.iter(world).map(|p| (p.player_id, p.faction)).collect()
    };
    let units: Vec<USnap> = {
        let mut q = world.query::<(Entity, &GameId, &Pos, &Owner, &MatchId, &Unit)>();
        q.iter(world)
            .map(|(e, g, p, o, m, u)| USnap {
                id: g.0,
                entity: e,
                pos: p.pos,
                owner: o.0,
                kind: u.kind,
                routing: u.routing,
                match_id: m.0,
                gather_state: u.gather_state,
                target_node: u.target_node,
                garrisoned: u.garrisoned_in != 0,
            })
            .collect()
    };
    let buildings: Vec<(u64, V2, u64, BuildingKind, u64)> = {
        let mut q = world.query::<(&GameId, &Pos, &Owner, &Building, &MatchId)>();
        q.iter(world).map(|(g, p, o, b, m)| (g.0, p.pos, o.0, b.kind, m.0)).collect()
    };
    // node id → resource type, for the committed re-steer (carry_type lags — it
    // holds the last DEPOSITED load — so steering keys off the target NODE).
    let node_type: HashMap<u64, ResourceType> = {
        let mut q = world.query::<(&GameId, &ResourceNode)>();
        q.iter(world).map(|(g, n)| (g.0, n.res_type)).collect()
    };

    let paused: StdSet<u64> = {
        let statuses = world.resource::<crate::MatchStatuses>();
        bots.iter().map(|b| b.match_id).filter(|&m| !statuses.simulates(m)).collect()
    };

    for bot in &bots {
        if bot.defeated || paused.contains(&bot.match_id) {
            continue;
        }
        let owner = bot.player_id;
        let prof = ai_profile(bot.difficulty);
        let tune = planner_tuning(prof);
        let tac = tactical_tuning(prof);

        // keep
        let Some(&(_, keep_pos, _, _, _)) =
            buildings.iter().find(|(_, _, o, k, _)| *o == owner && *k == BuildingKind::Keep)
        else {
            continue;
        };

        // Positions of every owned building — threat is measured against ALL of
        // them, so the bot reacts to a base raid even away from its keep.
        let owned_b_pos: Vec<V2> =
            buildings.iter().filter(|(_, _, o, _, _)| *o == owner).map(|(_, p, _, _, _)| *p).collect();

        // my census
        let mut army_comp: Census = [0; 10];
        let (mut peasants, mut soldiers, mut sieges, mut pop) = (0, 0, 0, 0);
        for u in &units {
            if u.owner != owner {
                continue;
            }
            pop += 1;
            if u.kind == UnitKind::Peasant {
                peasants += 1;
            }
            if is_combat(u.kind) || u.kind == UnitKind::Imam {
                army_comp[u.kind as usize] += 1;
            }
            if is_combat(u.kind) {
                soldiers += 1;
            }
            if is_siege(u.kind) {
                sieges += 1;
            }
        }

        let mut owned: StdSet<BuildingKind> = StdSet::new();
        let mut towers = 0;
        let mut cap = 0;
        for (_, _, o, k, _) in &buildings {
            if *o != owner {
                continue;
            }
            owned.insert(*k);
            if *k == BuildingKind::Tower {
                towers += 1;
            }
            cap += building_def(*k).pop;
        }

        // enemy census + threat + walls
        let mut enemy: Census = [0; 10];
        let mut threat = 0;
        for u in &units {
            if u.owner == owner || u.match_id != bot.match_id {
                continue;
            }
            let fac = faction_of.get(&u.owner).copied();
            if fac != Some(saladin_sim::enemy_faction(bot.faction)) {
                continue;
            }
            if !is_combat(u.kind) {
                continue;
            }
            enemy[u.kind as usize] += 1;
            if owned_b_pos.iter().any(|b| dist(u.pos, *b) <= HOME_THREAT_RADIUS) {
                threat += 1;
            }
        }
        let enemy_has_walls = buildings.iter().any(|(_, _, o, k, m)| {
            *o != owner
                && *m == bot.match_id
                && faction_of.get(o).copied() == Some(saladin_sim::enemy_faction(bot.faction))
                && (*k == BuildingKind::Wall || *k == BuildingKind::Gatehouse)
        });

        let stock = {
            let mut q = world.query::<&Player>();
            q.iter(world).find(|p| p.player_id == owner).map(|p| p.stock).unwrap_or_default()
        };

        let state = PlannerState {
            peasants,
            pop,
            cap,
            food: stock.food,
            wood: stock.wood,
            stone: stock.stone,
            gold: stock.gold,
            upkeep: soldiers,
            soldiers,
            army_composition: army_comp,
            sieges,
            towers,
            owned: owned.clone(),
            enemy,
            enemy_has_walls,
            threat_near_home: threat,
        };

        // ── economy: steer gatherers to what the bot is short of ──────────────
        // Two levers: (a) idle bias — what NEW idle peasants pick up; (b) committed
        // re-steer — pull a few peasants off the glut resource so a bias takes hold
        // even when everyone is locked onto fat nodes. Food emergency pulls all.
        let upkeep_food = soldiers * FOOD_PER_UNIT;
        let crisis = food_crisis(&state, &tune);
        let cushion = 40 + upkeep_food * tune.food_floor_mult * 2;
        let food_emergency = crisis || stock.food <= cushion;
        let food_surplus = !food_emergency && upkeep_food == 0 && stock.food > cushion + 200;
        let scarce_build = if stock.wood <= stock.stone { ResourceType::Wood } else { ResourceType::Stone };
        let idle_bias = if food_emergency {
            Some(ResourceType::Food)
        } else if food_surplus {
            Some(scarce_build)
        } else {
            None
        };

        // Pull peasants OFF a resource and idle them so they reassign to `want`.
        // Skips the scout, idle ones, loads in transit, and anyone whose target
        // node already matches `want`.
        let steer_to = |world: &mut World, want: ResourceType, from: Option<&[ResourceType]>, max: i32| {
            let mut n = 0;
            for u in &units {
                if n >= max {
                    break;
                }
                if u.owner != owner
                    || u.kind != UnitKind::Peasant
                    || u.id == bot.scout_id
                    || u.garrisoned
                    || u.gather_state == GatherState::Idle
                    || u.gather_state == GatherState::ToStockpile
                {
                    continue;
                }
                let nt = if u.target_node == 0 { None } else { node_type.get(&u.target_node).copied() };
                if nt == Some(want) {
                    continue; // already working the wanted resource
                }
                if let Some(from) = from {
                    match nt {
                        Some(t) if from.contains(&t) => {}
                        _ => continue, // only pull off the named glut resource(s)
                    }
                }
                if let Some(mut unit) = world.get_mut::<Unit>(u.entity) {
                    unit.gather_state = GatherState::Idle;
                    unit.target_node = 0;
                    n += 1;
                }
            }
        };
        if food_emergency {
            steer_to(world, ResourceType::Food, None, peasants);
        } else if food_surplus {
            steer_to(world, scarce_build, Some(&[ResourceType::Food]), 3);
        } else if (stock.wood - stock.stone).abs() > 80 {
            let glut = if stock.wood > stock.stone { ResourceType::Wood } else { ResourceType::Stone };
            steer_to(world, scarce_build, Some(&[glut]), 3);
        }
        assign_idle_gatherers(world, owner, idle_bias);

        // ── phase + one macro decision per profile-paced window ───────────────
        let phase = next_phase(&state, &tune);
        let mut decision_cd = bot.decision_cd - AI_BRAIN_DT;
        if decision_cd <= Fx::ZERO {
            decision_cd = prof.decision_interval;
            if let Some(plan) = next_build(&state, &tune) {
                if plan.is_unit {
                    if let Some(kind) = UnitKind::from_u8(plan.kind) {
                        train(world, owner, kind);
                    }
                } else if let Some(kind) = BuildingKind::from_u8(plan.kind) {
                    // Defensive towers keep a wood reserve; structural buildings
                    // just need to be affordable (build() re-checks the rest).
                    let reserve_ok = kind != BuildingKind::Tower
                        || stock.wood >= building_def(kind).cost.wood + tune.wood_buffer;
                    if reserve_ok {
                        place_near(world, owner, kind, keep_pos);
                    }
                }
            }
            // research: start the highest-priority Blacksmith tech the bot can
            // afford — through the SAME validation path a human uses (full cost,
            // full timer; no cheat). One start per decision window.
            if !prof.research.is_empty() && owned.contains(&BuildingKind::Blacksmith) {
                for &tech in prof.research {
                    if start_research(world, owner, tech as u8) {
                        break;
                    }
                }
            }
        }

        // ── threat timer: seconds of SUSTAINED threat near home ───────────────
        let threat_timer = if threat > 0 { bot.threat_timer + AI_BRAIN_DT } else { Fx::ZERO };

        // The fielded combat units classified by squad role; "home" units are the
        // standing garrison, the rest are the field army.
        struct FieldUnit {
            entity: Entity,
            id: u64,
            pos: V2,
            kind: UnitKind,
            role: SquadRole,
            at_home: bool,
        }
        let army: Vec<FieldUnit> = units
            .iter()
            .filter(|u| {
                u.owner == owner
                    && (is_combat(u.kind) || u.kind == UnitKind::Imam)
                    && !u.routing
                    && !u.garrisoned
            })
            .map(|u| FieldUnit {
                entity: u.entity,
                id: u.id,
                pos: u.pos,
                kind: u.kind,
                role: squad_role(u.kind),
                at_home: owned_b_pos.iter().any(|b| dist(u.pos, *b) <= HOME_RADIUS),
            })
            .collect();

        // ── defensive recall: pull part of the field army home under sustained
        //    attack. Closest field units come back first. Units at home stay.
        let field_count = army.iter().filter(|a| !a.at_home).count() as i32;
        let th = ThreatState {
            attackers: threat,
            field_army: field_count,
            home_army: army.len() as i32 - field_count,
        };
        let under_attack = threat_timer >= tac.defend_react_delay && should_recall(&th, &tac);
        if under_attack {
            let n = recall_count(&th, &tac);
            let mut by_closest: Vec<&FieldUnit> = army.iter().filter(|a| !a.at_home).collect();
            by_closest.sort_by_key(|a| dist(a.pos, keep_pos));
            let recalls: Vec<(Entity, V2)> =
                by_closest.iter().take(n.max(0) as usize).map(|a| (a.entity, a.pos)).collect();
            for (e, pos) in recalls {
                let path = path_to(world, pos, keep_pos);
                if let Some(mut u) = world.get_mut::<Unit>(e) {
                    u.attack_target = 0;
                    u.stance = Stance::Defensive;
                    u.gather_state = GatherState::Idle;
                    u.target_node = 0;
                    u.home = keep_pos;
                    if !path.is_empty() {
                        u.target = path[0];
                        u.path = path;
                        u.path_idx = 0;
                        u.has_target = true;
                    }
                }
            }
        }

        // ── assault: muster to wave_size, then march squads onto role targets ──
        // Hold while Defending or recalling; commit a full wave at once. Siege
        // leads onto fortifications, the main body besieges the keep, and the
        // fastest raider-class units peel off to harass the enemy economy.
        let mut wave_timer = bot.wave_timer - AI_BRAIN_DT;
        let wants_assault = phase != AiPhase::Defend
            && !under_attack
            && mustered(soldiers, prof.wave_size)
            && wave_timer <= Fx::ZERO;
        let mut launched = false;
        if wants_assault {
            let intel = assault_intel(&units, &buildings, &faction_of, owner, bot.faction, bot.match_id);
            if intel.keep.is_some() || !intel.buildings.is_empty() {
                let mut raiders: Vec<&FieldUnit> =
                    army.iter().filter(|a| a.role == SquadRole::Raider).collect();
                raiders.sort_by(|a, b| {
                    unit_def(b.kind).speed.cmp(&unit_def(a.kind).speed).then(a.id.cmp(&b.id))
                });
                let raids = raid_quota(raiders.len() as i32, tac.raid_fraction);
                let raid_set: StdSet<u64> =
                    raiders.iter().take(raids.max(0) as usize).map(|a| a.id).collect();

                let orders: Vec<(Entity, V2, SquadRole, bool)> =
                    army.iter().map(|a| (a.entity, a.pos, a.role, raid_set.contains(&a.id))).collect();
                for (e, pos, role, raiding) in orders {
                    // A raider not picked for the raid marches as Main so the
                    // assault keeps its punch.
                    let eff_role = if raiding {
                        SquadRole::Raider
                    } else if role == SquadRole::Raider {
                        SquadRole::Main
                    } else {
                        role
                    };
                    if let Some(t) = target_for_role(eff_role, pos, &intel).or(intel.keep) {
                        let path = path_to(world, pos, t.pos);
                        if let Some(mut u) = world.get_mut::<Unit>(e) {
                            u.attack_target = t.id;
                            u.stance = Stance::Aggressive;
                            u.gather_state = GatherState::Idle;
                            u.target_node = 0;
                            if !path.is_empty() {
                                u.target = path[0];
                                u.path = path;
                                u.path_idx = 0;
                                u.has_target = true;
                            }
                        }
                    }
                }
                wave_timer = prof.wave_interval;
                launched = true;
            }
        }

        // ── scouting (Hard): send the lowest-id peasant toward the nearest enemy
        //    keep once, so the bot reacts to the real map. Re-scout when it dies.
        let mut scout_id = bot.scout_id;
        let scout_alive = scout_id != 0 && units.iter().any(|u| u.id == scout_id && u.owner == owner);
        if tac.scouts && !scout_alive && !launched {
            let target = buildings
                .iter()
                .filter(|(_, _, o, k, m)| {
                    *m == bot.match_id
                        && *k == BuildingKind::Keep
                        && faction_of.get(o).copied() == Some(saladin_sim::enemy_faction(bot.faction))
                })
                .min_by_key(|(_, p, _, _, _)| dist2(keep_pos, *p))
                .map(|(_, p, _, _, _)| *p);
            let best = units
                .iter()
                .filter(|u| u.owner == owner && u.kind == UnitKind::Peasant && !u.garrisoned)
                .min_by_key(|u| u.id)
                .map(|u| (u.entity, u.pos, u.id));
            if let (Some(tpos), Some((e, pos, id))) = (target, best) {
                let path = path_to(world, pos, tpos);
                if let Some(mut u) = world.get_mut::<Unit>(e) {
                    u.gather_state = GatherState::Idle;
                    u.target_node = 0;
                    if !path.is_empty() {
                        u.target = path[0];
                        u.path = path;
                        u.path_idx = 0;
                        u.has_target = true;
                    }
                    scout_id = id;
                }
            }
        } else if scout_id != 0 && !scout_alive {
            scout_id = 0; // scout died — clear so a fresh one can go out later
        }

        if let Some(mut b) = world.get_mut::<Bot>(bot.entity) {
            b.decision_cd = decision_cd;
            b.wave_timer = wave_timer;
            b.phase = phase;
            b.threat_timer = threat_timer;
            b.scout_id = scout_id;
        }
    }
}

/// Try to place `kind` on a clear spot spiralling out from the keep.
fn place_near(world: &mut World, owner: u64, kind: BuildingKind, keep: V2) {
    if build(world, owner, kind, keep, 0) {
        return;
    }
    for r in 3..26 {
        for (dx, dy) in [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)] {
            let pos = V2::new(keep.x + Fx::from_num(dx * r), keep.y + Fx::from_num(dy * r));
            if build(world, owner, kind, pos, 0) {
                return;
            }
        }
    }
}

fn assault_intel(
    units: &[USnap],
    buildings: &[(u64, V2, u64, BuildingKind, u64)],
    faction_of: &HashMap<u64, Faction>,
    owner: u64,
    my_faction: Faction,
    match_id: u64,
) -> AssaultIntel {
    let is_enemy = |o: u64| o != owner && faction_of.get(&o).copied() == Some(saladin_sim::enemy_faction(my_faction));
    let mut intel = AssaultIntel::default();
    for (id, pos, o, kind, m) in buildings {
        if *m != match_id || !is_enemy(*o) {
            continue;
        }
        let t = TacticalTarget { id: *id, pos: *pos };
        intel.buildings.push(t);
        if *kind == BuildingKind::Keep {
            intel.keep = Some(t);
        }
        if matches!(*kind, BuildingKind::Wall | BuildingKind::Gatehouse | BuildingKind::Tower | BuildingKind::Watchtower) {
            intel.defenses.push(t);
        }
    }
    for u in units {
        if u.match_id != match_id || !is_enemy(u.owner) || u.kind != UnitKind::Peasant {
            continue;
        }
        intel.gatherers.push(TacticalTarget { id: u.id, pos: u.pos });
    }
    intel
}
