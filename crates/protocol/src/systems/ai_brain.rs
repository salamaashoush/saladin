use crate::commands::{
    assign_builders, assign_idle_gatherers, build_context, build_with, garrison, group_attack,
    group_move, market_buy_cmd, market_trade, move_unit, repair, start_research, train,
    ungarrison, upgrade_building,
};
use crate::components::*;
use bevy_ecs::prelude::*;
use bevy_platform::collections::HashMap;
use saladin_sim::*;
use std::collections::HashSet as StdSet;

const AI_BRAIN_DT: Fx = saladin_sim::AI_BRAIN_DT;
const HOME_THREAT_RADIUS: Fx = saladin_sim::fx!("24"); // enemy combatants this close to home = a threat
const HOME_RADIUS: Fx = saladin_sim::fx!("18"); // own combat units this close to a building count as "home"
const SHORE_SCAN: i32 = 14; // tile radius around the keep that counts as "shore near"
// How far out a bot will plant a forward Storehouse. Past this the outpost is
// indefensible and the haul home costs more than the drop-off saves.
const REMOTE_CLUSTER_MAX: Fx = saladin_sim::fx!("48");
// Squared distance from its node inside which a gatherer counts as "practically
// there" and is left alone by an economic re-steer.
const NEARLY_THERE2: Fx = saladin_sim::fx!("9");
/// No `FormationShape` — march loose. A recall and a retreat are not a parade:
/// dressing the line costs the seconds the men are running for.
const FORMATION_LOOSE: u8 = u8::MAX;
/// Full-cost group paths one bot may lay per brain tick. The rest fall back to
/// the cheap expansion cap, exactly as the player's command batch does. The old
/// code ran one UNCAPPED A* per unit per wave; a squad shares one.
const ARMY_PATHS_PER_BRAIN_TICK: usize = 4;
/// Rungs one decision window may fall through when the ground for a building has
/// run out. Bounded: a probe costs a full placement scan.
const LADDER_RETRIES: usize = 3;
/// Enough phantom sites to satisfy any rung's count, for one window only.
const LADDER_SUPPRESS: i32 = 1000;

fn is_support(kind: UnitKind) -> bool {
    unit_def(kind).role == saladin_sim::UnitRole::Support
}

fn is_combat(kind: UnitKind) -> bool {
    unit_def(kind).attack > 0
}
fn is_siege(kind: UnitKind) -> bool {
    unit_def(kind).prefers_buildings
}

/// A hub whose aura works fields — it belongs among them, not beside the keep.
fn tends_fields(kind: BuildingKind) -> bool {
    building_def(kind).aura.is_some_and(|a| a.target == AuraTarget::Field)
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
    wave_launched: i32,
    fishing_blocked: bool,
    famine: bool,
}

/// One structure's lifecycle row, for the planner's site/damage/upgrade view.
#[derive(Clone, Copy)]
struct Lifecycle {
    id: u64,
    pos: V2,
    owner: u64,
    kind: BuildingKind,
    state: BuildState,
    hp: i32,
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
    garrisoned_in: u64,
    job_site: u64,
    /// Nothing to walk to and nothing to hit — the state a wave that has won
    /// its road fight sits in until somebody points it at the objective again.
    idle: bool,
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
                wave_launched: b.wave_launched,
                fishing_blocked: b.fishing_blocked,
                famine: b.famine,
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
                garrisoned_in: u.garrisoned_in,
                job_site: u.job_site,
                idle: !u.has_target && u.attack_target == 0,
            })
            .collect()
    };
    let buildings: Vec<(u64, V2, u64, BuildingKind, u64)> = {
        let mut q = world.query::<(&GameId, &Pos, &Owner, &Building, &MatchId)>();
        q.iter(world).map(|(g, p, o, b, m)| (g.0, p.pos, o.0, b.kind, m.0)).collect()
    };
    // lifecycle of every structure, for the planner's site/damage view
    let mut lifecycles: Vec<Lifecycle> = {
        let mut q = world.query::<(&GameId, &Pos, &Owner, &Building)>();
        q.iter(world)
            .map(|(g, p, o, b)| Lifecycle {
                id: g.0,
                pos: p.pos,
                owner: o.0,
                kind: b.kind,
                state: b.state,
                hp: b.hp,
            })
            .collect()
    };
    lifecycles.sort_by_key(|l| l.id);
    let tech_of: HashMap<u64, u64> = {
        let mut q = world.query::<&Player>();
        q.iter(world).map(|p| (p.player_id, p.tech_mask)).collect()
    };
    // node id → resource type, for the committed re-steer (carry_type lags — it
    // holds the last DEPOSITED load — so steering keys off the target NODE).
    let node_type: HashMap<u64, ResourceType> = {
        let mut q = world.query::<(&GameId, &ResourceNode)>();
        q.iter(world).map(|(g, n)| (g.0, n.res_type)).collect()
    };
    let node_pos: Vec<(u64, V2)> = {
        let mut q = world.query::<(&GameId, &Pos, &ResourceNode)>();
        q.iter(world).map(|(g, p, _)| (g.0, p.pos)).collect()
    };
    let node_at: HashMap<u64, V2> = node_pos.iter().copied().collect();
    // The farms that actually carry a standing field, and whether a harvest
    // worth planning around stands on it. A plot without a crop is fifty wood of
    // scenery, not a farm; and a plot reaped down to two sheaves is not a
    // harvest, however long `ripe` stays latched over it (see
    // `harvest_standing`).
    let field_of: HashMap<u64, bool> = {
        let mut q = world.query::<(&FieldOf, &ResourceNode, Option<&Crop>)>();
        q.iter(world)
            .map(|(f, n, c)| {
                (f.0, c.is_some_and(|c| c.ripe) && harvest_standing(n.remaining, n.cap))
            })
            .collect()
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
        let mut army_comp: Census = saladin_sim::EMPTY_CENSUS;
        let (mut peasants, mut soldiers, mut sieges, mut pop) = (0, 0, 0, 0);
        for u in &units {
            if u.owner != owner {
                continue;
            }
            pop += 1;
            if u.kind == UnitKind::Peasant {
                peasants += 1;
            }
            if is_combat(u.kind) || is_support(u.kind) {
                army_comp[u.kind as usize] += 1;
            }
            if is_combat(u.kind) {
                soldiers += 1;
            }
            if is_siege(u.kind) {
                sieges += 1;
            }
        }

        // what the bot can DO, not what it has paid for: a foundation houses
        // nobody, fires nothing and unlocks nothing until a peasant finishes it
        let mut owned: StdSet<BuildingKind> = StdSet::new();
        let mut towers = 0;
        let mut cap = 0;
        for l in &lifecycles {
            if l.owner != owner || !operational(l.state) {
                continue;
            }
            owned.insert(l.kind);
            if l.kind == BuildingKind::Tower {
                towers += 1;
            }
            cap += building_def(l.kind).pop;
        }

        // enemy census + threat + walls
        let mut enemy: Census = saladin_sim::EMPTY_CENSUS;
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

        // standing enemy towers weigh into the assault go/no-go
        let enemy_towers = buildings
            .iter()
            .filter(|(_, _, o, k, m)| {
                *m == bot.match_id
                    && faction_of.get(o).copied() == Some(saladin_sim::enemy_faction(bot.faction))
                    && matches!(*k, BuildingKind::Tower | BuildingKind::Watchtower)
            })
            .count() as i32;

        // open water within building reach of the keep enables a Fishing Hut
        let shore_near = !bot.fishing_blocked && {
            let seed = world.resource::<crate::WorldConfig>().seed;
            let (kx, ky) = (keep_pos.x.to_num::<i32>(), keep_pos.y.to_num::<i32>());
            let mut found = false;
            'scan: for dy in -SHORE_SCAN..=SHORE_SCAN {
                for dx in -SHORE_SCAN..=SHORE_SCAN {
                    if is_water_tile(seed, kx + dx, ky + dy) {
                        found = true;
                        break 'scan;
                    }
                }
            }
            found
        };

        // soil worth sowing within building reach of the keep, and the fields
        // already standing on it
        // A FARM IS ITS FIELD. Counting plots rather than crops let a bot sit at
        // its farm target while the food economy under it shrank, and it is the
        // reason a granary was sited off the back of foundations that fed
        // nobody. Sites are excluded here because the ladder adds
        // `sites_in_flight[Farm]` on top.
        let (mut farms, mut fields_ripe) = (0, 0);
        let mut farm_sum = V2::new(Fx::ZERO, Fx::ZERO);
        for l in &lifecycles {
            if l.owner != owner || !operational(l.state) {
                continue;
            }
            let Some(&ripe) = field_of.get(&l.id) else { continue };
            farms += 1;
            fields_ripe += i32::from(ripe);
            farm_sum = V2::new(farm_sum.x + l.pos.x, farm_sum.y + l.pos.y);
        }
        // A hub belongs among the fields it hubs, not on the first clear tile out
        // from the keep — measured, spiralling from the keep covered two of nine.
        let farm_centroid = (farms > 0)
            .then(|| V2::new(farm_sum.x / Fx::from_num(farms), farm_sum.y / Fx::from_num(farms)));
        let farmland_near = {
            let seed = world.resource::<crate::WorldConfig>().seed;
            let mut found = false;
            'soil: for dy in -SHORE_SCAN..=SHORE_SCAN {
                for dx in -SHORE_SCAN..=SHORE_SCAN {
                    let p = V2::new(keep_pos.x + Fx::from_num(dx), keep_pos.y + Fx::from_num(dy));
                    if saladin_sim::fertility_at(seed, p.x, p.y) >= saladin_sim::FARM_MIN_FERTILITY {
                        found = true;
                        break 'soil;
                    }
                }
            }
            found
        };

        // sites rising, structures hurt, towers worth raising
        let mask = tech_of.get(&owner).copied().unwrap_or(0);
        let mut sites_in_flight = [0i32; 16];
        let (mut damaged, mut storehouses, mut upgradable_towers) = (0, 0, 0);
        let mut worst: Option<(i32, u64)> = None;
        let mut oldest_tower = 0u64;
        for l in &lifecycles {
            if l.owner != owner {
                continue;
            }
            match l.state {
                BuildState::Site => sites_in_flight[l.kind as usize] += 1,
                _ => {
                    let max_hp = saladin_sim::effective_building_def(l.kind, mask).max_hp.max(1);
                    if tune.repair_threshold > 0 && l.hp * 100 < max_hp * tune.repair_threshold {
                        damaged += 1;
                        let pct = l.hp * 100 / max_hp;
                        if worst.is_none_or(|(bp, _)| pct < bp) {
                            worst = Some((pct, l.id));
                        }
                    }
                    if l.kind == BuildingKind::Storehouse {
                        storehouses += 1;
                    }
                    if l.kind == BuildingKind::Tower && l.state == BuildState::Complete && oldest_tower == 0 {
                        oldest_tower = l.id;
                    }
                    if l.kind == BuildingKind::Tower && l.state == BuildState::Complete {
                        upgradable_towers += 1;
                    }
                }
            }
        }
        let builders_busy = units
            .iter()
            .filter(|u| u.owner == owner && u.gather_state == GatherState::Constructing)
            .count() as i32;
        // the nearest resource cluster no drop-off can serve — the reason to
        // plant a Storehouse. Own buildings are the anchors; TOWN_RADIUS is the
        // reach a town has without one.
        let remote_cluster = if tune.wants_expansion {
            let reach2 = TOWN_RADIUS * TOWN_RADIUS;
            let far2 = REMOTE_CLUSTER_MAX * REMOTE_CLUSTER_MAX;
            // ties break on the lowest GameId, never on ECS iteration order:
            // this feeds a build decision, so an order-dependent pick would
            // desync two peers that agree on everything else.
            let mut best: Option<(Fx, u64, V2)> = None;
            for (id, npos) in &node_pos {
                let d = dist2(*npos, keep_pos);
                if d > far2 {
                    continue;
                }
                if let Some((bd, bid, _)) = best {
                    if d > bd || (d == bd && *id >= bid) {
                        continue;
                    }
                }
                if owned_b_pos.iter().any(|b| dist2(*npos, *b) <= reach2) {
                    continue;
                }
                best = Some((d, *id, *npos));
            }
            best.map(|(_, _, p)| p)
        } else {
            None
        };

        let state = PlannerState {
            faction: bot.faction,
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
            shore_near,
            farmland_near,
            farms,
            fields_ripe,
            enemy_towers,
            sites_in_flight,
            damaged,
            builders_busy,
            storehouses,
            upgradable_towers,
            remote_cluster,
        };

        // ── economy: steer gatherers to what the bot is short of ──────────────
        // Each steer pulls a BOUNDED number of hands off one trade and puts
        // exactly those hands on another. The old shape — idle a few, then let
        // one blanket bias reassign everyone still idle — could not express two
        // wants at once: a famine bias overrode the wood steer entirely, so a
        // starving bot quarried fourteen hundred stone it could not eat while
        // sitting on twelve wood, four short of the forty-five a field costs.
        let upkeep_food = soldiers * FOOD_PER_UNIT;
        let crisis = food_crisis(&state, &tune);
        let cushion = 40 + upkeep_food * tune.food_floor_mult * 2;
        // Enter at the cushion, leave at half again: a bare threshold makes the
        // whole workforce change trade on every crossing, and a peasant that is
        // walking is a peasant that is not gathering.
        let food_emergency =
            crisis || stock.food <= cushion || (bot.famine && stock.food <= cushion + cushion / 2);
        let food_surplus = !food_emergency && upkeep_food == 0 && stock.food > cushion + 200;
        let scarce_build = if stock.wood <= stock.stone { ResourceType::Wood } else { ResourceType::Stone };
        let on_food = units
            .iter()
            .filter(|u| {
                u.owner == owner
                    && u.kind == UnitKind::Peasant
                    && node_type.get(&u.target_node) == Some(&ResourceType::Food)
            })
            .count() as i32;
        // Half the workforce on food, not all of it: the larder is ultimately
        // paid for with the wood and stone the other half brings in.
        let want_food = (peasants / 2).max(2);

        // Pull peasants OFF a resource and idle them so they reassign to `want`.
        // Skips the scout, idle ones, loads in transit, and anyone whose target
        // node already matches `want`. `moved` keeps a second steer in the same
        // brain tick from re-tasking a hand the first one just re-tasked — the
        // `units` snapshot is from before either.
        let mut moved: StdSet<u64> = StdSet::new();
        let steer_to = |world: &mut World,
                            moved: &mut StdSet<u64>,
                            want: ResourceType,
                            from: Option<&[ResourceType]>,
                            max: i32| {
            let mut n = 0;
            for u in &units {
                if n >= max {
                    break;
                }
                if u.owner != owner
                    || u.kind != UnitKind::Peasant
                    || u.id == bot.scout_id
                    || u.garrisoned_in != 0
                    || u.gather_state == GatherState::Idle
                    || u.gather_state == GatherState::ToStockpile
                    || moved.contains(&u.id)
                {
                    continue;
                }
                if u.gather_state == GatherState::Harvesting {
                    continue; // hands already on the node: never interrupt a swing
                }
                // A hand on a job is `staff_jobs`'s to allocate. Idling one here
                // left its `job_site` booked at a site nobody was raising and a
                // field nobody was working, and the gather loop walked it back
                // the moment it dropped its load.
                if u.gather_state == GatherState::Constructing {
                    continue;
                }
                let nt = if u.target_node == 0 { None } else { node_type.get(&u.target_node).copied() };
                if nt == Some(want) {
                    continue; // already working the wanted resource
                }
                // practically there — re-steering now throws away the whole walk
                if node_at.get(&u.target_node).is_some_and(|p| dist2(u.pos, *p) <= NEARLY_THERE2) {
                    continue;
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
                    moved.insert(u.id);
                    n += 1;
                }
            }
            n > 0
        };
        if food_emergency && on_food < want_food {
            if steer_to(world, &mut moved, ResourceType::Food, None, want_food - on_food) {
                assign_idle_gatherers(world, owner, Some(ResourceType::Food));
            }
        } else if food_surplus
            && steer_to(world, &mut moved, scarce_build, Some(&[ResourceType::Food]), 3)
        {
            assign_idle_gatherers(world, owner, Some(scarce_build));
        }
        if !food_surplus && (stock.wood - stock.stone).abs() > 80 {
            let glut = if stock.wood > stock.stone { ResourceType::Wood } else { ResourceType::Stone };
            if steer_to(world, &mut moved, scarce_build, Some(&[glut]), 3) {
                assign_idle_gatherers(world, owner, Some(scarce_build));
            }
        }
        // Whoever is still idle — new peasants, and anyone whose node ran out —
        // takes the balanced mix, biased only while the larder is genuinely
        // short of its half.
        let idle_bias = if food_emergency && on_food < want_food {
            Some(ResourceType::Food)
        } else if food_surplus {
            Some(scarce_build)
        } else {
            None
        };
        // Jobs first, THEN the balancer: a hand `staff_jobs` sends home from an
        // over-staffed field is idle for the rest of this tick, and the balancer
        // is what gives it a node. Reversed, it stood in the yard for a second.
        staff_jobs(
            world,
            owner,
            &units,
            &lifecycles,
            &field_of,
            tune.builders_per_site,
            field_labour(&state, &tune),
        );
        assign_idle_gatherers(world, owner, idle_bias);

        // ── phase + one macro decision per profile-paced window ───────────────
        let phase = next_phase(&state, &tune);
        let mut fishing_blocked = bot.fishing_blocked;
        let mut decision_cd = bot.decision_cd - AI_BRAIN_DT;
        if decision_cd <= Fx::ZERO {
            decision_cd = prof.decision_interval;
            // A rung whose ground has run out is suppressed and the NEXT
            // decision taken in the same window. Without this an affordable
            // building with nowhere legal to stand wedges the whole ladder
            // behind it forever — measured on a soil-poor start, a bot sat on
            // 872 wood and 800 stone asking for an eighth farm it could not
            // site, and never bought the market standing one rung below.
            let mut plan_state = state.clone();
            for _ in 0..LADDER_RETRIES {
                let Some(plan) = next_build(&plan_state, &tune) else { break };
                let mut blocked = None;
                match plan.action {
                    BuildAction::Train => {
                        if let Some(kind) = UnitKind::from_u8(plan.kind) {
                            train(world, owner, kind);
                        }
                    }
                    BuildAction::Build => {
                        if let Some(kind) = BuildingKind::from_u8(plan.kind) {
                            // Defensive towers keep a wood reserve; structural
                            // buildings just need to be affordable (build()
                            // re-checks the rest).
                            let reserve_ok = kind != BuildingKind::Tower
                                || stock.wood >= building_def(kind).cost.wood + tune.wood_buffer;
                            let anchor = match kind {
                                BuildingKind::Storehouse => {
                                    state.remote_cluster.unwrap_or(keep_pos)
                                }
                                _ if tends_fields(kind) => farm_centroid.unwrap_or(keep_pos),
                                _ => keep_pos,
                            };
                            if reserve_ok {
                                let placed = place_near(world, owner, kind, anchor);
                                if placed.is_none() && stock.can_afford(&building_def(kind).cost) {
                                    blocked = Some(kind);
                                    // a shoreline is the one kind of ground that
                                    // never comes back, so that one stays latched
                                    if kind == BuildingKind::FishingHut {
                                        fishing_blocked = true;
                                    }
                                }
                            }
                        }
                    }
                    // Raising a tower in place and mending a battered hall are
                    // the SAME order a human gives: pay, then send hands.
                    BuildAction::Upgrade => {
                        if oldest_tower != 0 {
                            upgrade_building(world, owner, oldest_tower);
                        }
                    }
                    BuildAction::Repair => {
                        if let Some((_, id)) = worst {
                            let pos =
                                lifecycles.iter().find(|l| l.id == id).map(|l| l.pos).unwrap_or(keep_pos);
                            let mut taken: StdSet<u64> =
                                units.iter().filter(|u| u.job_site != 0).map(|u| u.id).collect();
                            let crew =
                                spare_hands(&units, owner, pos, tune.builders_per_site, &mut taken);
                            assign_builders(world, owner, id, &crew);
                        }
                    }
                }
                let Some(kind) = blocked else { break };
                // `rising` is what every rung reads, count-based ones included,
                // so one bump suppresses the kind whatever shape its rung has.
                plan_state.sites_in_flight[kind as usize] += LADDER_SUPPRESS;
            }
            // market: one order per window through the SAME validated command
            // path a human uses — famine rescue (gold into food) or war-chest
            // building (glut into gold; cavalry, siege and tech all cost gold).
            if let Some(t) = next_trade(&state, &tune) {
                if t.buy {
                    market_buy_cmd(world, owner, t.res, t.amount);
                } else {
                    market_trade(world, owner, t.res, t.amount);
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
            idle: bool,
        }
        let army: Vec<FieldUnit> = units
            .iter()
            .filter(|u| {
                u.owner == owner
                    && (is_combat(u.kind) || is_support(u.kind))
                    && !u.routing
                    && u.garrisoned_in == 0
            })
            .map(|u| FieldUnit {
                entity: u.entity,
                id: u.id,
                pos: u.pos,
                kind: u.kind,
                role: squad_role(u.kind),
                at_home: owned_b_pos.iter().any(|b| dist(u.pos, *b) <= HOME_RADIUS),
                idle: u.idle,
            })
            .collect();

        let mut army_paths = ARMY_PATHS_PER_BRAIN_TICK;

        // ── defensive recall: pull part of the field army home under sustained
        //    attack. Closest field units come back first. Units at home stay.
        let field_count = army.iter().filter(|a| !a.at_home).count() as i32;
        let th = ThreatState {
            attackers: threat,
            field_army: field_count,
            home_army: army.len() as i32 - field_count,
        };
        let defending = threat_timer >= tac.defend_react_delay && threat >= tac.defend_threat;
        let under_attack = threat_timer >= tac.defend_react_delay && should_recall(&th, &tac);

        // ── garrison: while defending, shooters man the keep/towers (volleys
        //    stack and the garrison survives the host); all-clear empties them.
        let hosts: Vec<(u64, V2, i32)> = buildings
            .iter()
            .filter(|(_, _, o, k, m)| {
                *o == owner && *m == bot.match_id && building_def(*k).garrison_cap > 0
            })
            .map(|(id, p, _, k, _)| (*id, *p, building_def(*k).garrison_cap))
            .collect();
        if defending {
            let mut free: Vec<(u64, V2, i32)> = hosts
                .iter()
                .map(|(id, p, cap)| {
                    let occ = units.iter().filter(|u| u.garrisoned_in == *id).count() as i32;
                    (*id, *p, cap - occ)
                })
                .filter(|(_, _, f)| *f > 0)
                .collect();
            let mut shooters: Vec<(u64, V2)> = army
                .iter()
                .filter(|a| {
                    a.at_home
                        && can_garrison(unit_def(a.kind))
                        && unit_def(a.kind).range >= saladin_sim::fx!("3")
                })
                .map(|a| (a.id, a.pos))
                .collect();
            shooters.sort_by_key(|(id, _)| *id);
            for (uid, upos) in shooters {
                let Some(h) =
                    free.iter_mut().filter(|(_, _, f)| *f > 0).min_by_key(|(_, p, _)| dist2(upos, *p))
                else {
                    break;
                };
                let host_id = h.0;
                h.2 -= 1;
                garrison(world, owner, uid, host_id);
            }
        } else if threat == 0 {
            let occupied: Vec<u64> = hosts
                .iter()
                .filter(|(id, _, _)| units.iter().any(|u| u.owner == owner && u.garrisoned_in == *id))
                .map(|(id, _, _)| *id)
                .collect();
            for b in occupied {
                ungarrison(world, owner, b);
            }
        }

        if under_attack {
            let n = recall_count(&th, &tac);
            let mut by_closest: Vec<&FieldUnit> = army.iter().filter(|a| !a.at_home).collect();
            by_closest.sort_by_key(|a| (dist(a.pos, keep_pos), a.id));
            let recalls: Vec<(Entity, u64)> =
                by_closest.iter().take(n.max(0) as usize).map(|a| (a.entity, a.id)).collect();
            if !recalls.is_empty() {
                let ids: Vec<u64> = recalls.iter().map(|(_, id)| *id).collect();
                group_move(world, owner, &ids, keep_pos, FORMATION_LOOSE, &mut army_paths);
                for (e, _) in recalls {
                    if let Some(mut u) = world.get_mut::<Unit>(e) {
                        u.stance = Stance::Defensive;
                    }
                }
            }
        }

        // ── assault: muster to wave_size, then march squads onto role targets ──
        // Hold while Defending or recalling; commit a full wave at once. Siege
        // leads onto fortifications, the main body besieges the keep, and the
        // fastest raider-class units peel off to harass the enemy economy.
        let mut wave_timer = bot.wave_timer - AI_BRAIN_DT;
        let mut wave_launched = bot.wave_launched;
        // Power gate: only commit a wave with a real strength edge over the
        // defender's field army + towers. A double-muster overrides the gate
        // so a turtling stalemate still breaks.
        let overwhelming = soldiers >= prof.wave_size * 2;
        let strong_enough =
            overwhelming || should_assault(&army_comp, &enemy, enemy_towers, tac.advantage_margin_pct);
        let wants_assault = phase != AiPhase::Defend
            && !under_attack
            && mustered(soldiers, prof.wave_size)
            && strong_enough
            && wave_timer <= Fx::ZERO;
        // A wave in the field that has run out of things to hit is a wave that
        // has FORGOTTEN its objective: it killed what it met on the road, the
        // named target went with `attack_target = 0`, and it stands there until
        // the next muster interval. Re-pointing only the idle men keeps a siege
        // pressed without re-pathing the whole army every second.
        let recommit = !wants_assault
            && wave_launched > 0
            && !under_attack
            && army.iter().any(|a| !a.at_home && a.idle);
        let mut launched = false;
        if wants_assault || recommit {
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

                // Squads, not a crowd of singletons. Men sharing an objective
                // march as ONE group order: one path instead of N, formation
                // slots so they do not stack on a single tile, and the column
                // paces itself to its slowest man so the ram arrives with its
                // escort. `ORDER_ATTACK` is also what keeps the wave committed —
                // an aggro pickup is leashed to `home`, a named target is not.
                let mut squads: Vec<(u64, Vec<u64>)> = Vec::new();
                let mut stances: Vec<Entity> = Vec::new();
                for a in &army {
                    // a re-commit moves the men who have stopped, and nobody else
                    if !wants_assault && (a.at_home || !a.idle) {
                        continue;
                    }
                    // A raider not picked for the raid marches as Main so the
                    // assault keeps its punch.
                    let eff_role = if raid_set.contains(&a.id) {
                        SquadRole::Raider
                    } else if a.role == SquadRole::Raider {
                        SquadRole::Main
                    } else {
                        a.role
                    };
                    let Some(t) = target_for_role(eff_role, a.pos, &intel).or(intel.keep) else {
                        continue;
                    };
                    stances.push(a.entity);
                    match squads.iter_mut().find(|(id, _)| *id == t.id) {
                        Some((_, members)) => members.push(a.id),
                        None => squads.push((t.id, vec![a.id])),
                    }
                }
                squads.sort_unstable_by_key(|(id, _)| *id);
                for (target, members) in &squads {
                    group_attack(world, owner, members, *target, &mut army_paths);
                }
                for e in stances {
                    if let Some(mut u) = world.get_mut::<Unit>(e) {
                        u.stance = Stance::Aggressive;
                    }
                }
                if wants_assault {
                    wave_timer = prof.wave_interval;
                    wave_launched = soldiers;
                    launched = true;
                }
            }
        }

        // ── retreat: a wave bled below the threshold breaks off and regroups
        //    at the keep instead of trickling into the meat grinder.
        let field_units: Vec<(Entity, u64)> =
            army.iter().filter(|a| !a.at_home).map(|a| (a.entity, a.id)).collect();
        if !launched && wave_launched > 0 {
            if field_units.is_empty() {
                wave_launched = 0; // wave resolved (won, died, or walked home)
            } else if should_retreat(wave_launched, soldiers, tac.retreat_pct) {
                let ids: Vec<u64> = field_units.iter().map(|(_, id)| *id).collect();
                group_move(world, owner, &ids, keep_pos, FORMATION_LOOSE, &mut army_paths);
                for (e, _) in field_units {
                    if let Some(mut u) = world.get_mut::<Unit>(e) {
                        u.stance = Stance::Defensive;
                    }
                }
                wave_launched = 0;
                wave_timer = prof.wave_interval; // regroup before the next muster
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
                .filter(|u| u.owner == owner && u.kind == UnitKind::Peasant && u.garrisoned_in == 0)
                .min_by_key(|u| u.id)
                .map(|u| u.id);
            if let (Some(tpos), Some(id)) = (target, best) {
                // the same order a human gives; `move_unit` already clears the
                // job_site a scout would otherwise keep booked at a site it is
                // walking away from, forever
                move_unit(world, owner, id, tpos);
                scout_id = id;
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
            b.wave_launched = wave_launched;
            b.fishing_blocked = fishing_blocked;
            b.famine = food_emergency;
        }
    }
}

/// Try to place `kind` on a clear spot spiralling out from the keep. Eight
/// rays cover ordinary structures; a shoreline building needs the FULL ring
/// perimeter — its one valid waterside tile is rarely on a ray.
fn place_near(world: &mut World, owner: u64, kind: BuildingKind, keep: V2) -> Option<u64> {
    // ONE context for the whole probe: a shoreline scan asks ~800 questions and
    // each one used to re-walk every building and every resource node on the map.
    let mut ctx = build_context(world, owner)?;
    let mut try_at =
        |world: &mut World, pos: V2| build_with(world, &mut ctx, owner, kind, pos, 0).ok();
    if let Some(id) = try_at(world, keep) {
        return Some(id);
    }
    // A field needs soil and a hut needs a shore; both are scarce enough that
    // the eight-spoke scan below walks straight past them.
    if building_def(kind).requires_water || building_def(kind).min_fertility > Fx::ZERO {
        for r in 2..=SHORE_SCAN {
            for (dx, dy) in ring_perimeter(r) {
                let pos = V2::new(keep.x + Fx::from_num(dx), keep.y + Fx::from_num(dy));
                if let Some(id) = try_at(world, pos) {
                    return Some(id);
                }
            }
        }
        return None;
    }
    for r in 3..26 {
        for (dx, dy) in [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)] {
            let pos = V2::new(keep.x + Fx::from_num(dx * r), keep.y + Fx::from_num(dy * r));
            if let Some(id) = try_at(world, pos) {
                return Some(id);
            }
        }
    }
    None
}

/// The `n` peasants nearest `at` that are not already on a job — ties on the
/// lowest GameId, never on iteration order. `taken` stops one pass handing the
/// same hand to two sites off a stale snapshot.
fn spare_hands(units: &[USnap], owner: u64, at: V2, n: i32, taken: &mut StdSet<u64>) -> Vec<u64> {
    let mut cand: Vec<(Fx, u64)> = units
        .iter()
        .filter(|u| {
            u.owner == owner
                && u.kind == UnitKind::Peasant
                && u.garrisoned_in == 0
                && u.gather_state != GatherState::Constructing
                && !taken.contains(&u.id)
        })
        .map(|u| (dist2(u.pos, at), u.id))
        .collect();
    cand.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    let picked: Vec<u64> = cand.into_iter().take(n.max(0) as usize).map(|(_, id)| id).collect();
    for id in &picked {
        taken.insert(*id);
    }
    picked
}

/// Keep every paid-for site manned and every standing field worked — one loop,
/// because they are one mechanic: a farmhand IS a builder standing on a farm.
///
/// An unmanned site never finishes, and the planner's `sites_in_flight` gate
/// would then wedge the whole build ladder behind it — so the first half is not
/// a nicety. The second half is the labour allocation the player makes every
/// minute of the match, and it is BUDGETED in both directions. `crew_up` keeps a
/// farm crew forever, so without a ceiling the hands that raise the last farm
/// never come home: measured, twelve of thirteen peasants standing in the wheat,
/// six wood in the yard, and a build ladder frozen for eleven minutes on a food
/// pile the bot had nothing to spend on.
fn staff_jobs(
    world: &mut World,
    owner: u64,
    units: &[USnap],
    jobs: &[Lifecycle],
    fields: &HashMap<u64, bool>,
    want: i32,
    lab: FieldLabour,
) {
    let hands = units.iter().filter(|u| u.owner == owner && u.kind == UnitKind::Peasant).count();
    if hands <= 2 {
        return; // never strip the economy bare to raise a wall
    }
    let crew_of = |site: u64| {
        units.iter().filter(|u| u.owner == owner && u.job_site == site).count() as i32
    };
    let mut taken: StdSet<u64> =
        units.iter().filter(|u| u.job_site != 0).map(|u| u.id).collect();
    for j in jobs {
        if j.owner != owner || j.state == BuildState::Complete {
            continue;
        }
        let short = want.max(1) - crew_of(j.id);
        if short <= 0 {
            continue;
        }
        for u in spare_hands(units, owner, j.pos, short, &mut taken) {
            repair(world, owner, u, j.id);
        }
    }

    // `jobs` is GameId-sorted, so the fields are walked in the same order on
    // every peer and a budget that runs out cuts the same farm off everywhere.
    let mut crews: Vec<(u64, V2, i32)> = jobs
        .iter()
        .filter(|j| j.owner == owner && operational(j.state) && fields.contains_key(&j.id))
        .map(|j| (j.id, j.pos, crew_of(j.id)))
        .collect();
    if crews.is_empty() {
        return;
    }
    let mut committed: i32 = crews.iter().map(|(_, _, c)| *c).sum();
    // Over budget — send the surplus back to the woods. Deepest crew first, so
    // a town that has just lost peasants thins its fields evenly instead of
    // abandoning one.
    //
    // `sent_home` is load-bearing: `units` is a snapshot taken before this pass,
    // so a hand released here still reads `job_site == site` on the next lap and
    // the same man would be picked until the counters ran out. That thinned the
    // fields ONE hand per brain tick however far over budget the town was — a
    // farm finishing with a six-hand crew sat 50% over its ration for three
    // seconds, which is the breach `a_bot_works_the_fields...` measures.
    let mut sent_home: StdSet<u64> = StdSet::default();
    while committed > lab.budget {
        let Some(slot) = crews
            .iter_mut()
            .filter(|(_, _, c)| *c > 0)
            .max_by_key(|(id, _, c)| (*c, std::cmp::Reverse(*id)))
        else {
            break;
        };
        let site = slot.0;
        // the newest hand leaves first: the man who has walked furthest into the
        // season stays with the crop
        let Some(u) = units
            .iter()
            .filter(|u| u.owner == owner && u.job_site == site && !sent_home.contains(&u.id))
            .max_by_key(|u| u.id)
            .map(|u| (u.entity, u.id))
        else {
            slot.2 = 0;
            continue;
        };
        if let Some(mut unit) = world.get_mut::<Unit>(u.0) {
            unit.job_site = 0;
            unit.target_node = 0;
            unit.gather_state = GatherState::Idle;
            unit.has_target = false;
        }
        sent_home.insert(u.1);
        taken.remove(&u.1);
        slot.2 -= 1;
        committed -= 1;
    }
    // Under budget — spread a LAYER AT A TIME across every field before
    // thickening any one of them. The tending curve diminishes, so three hands
    // over three fields beat three on one, and the bot answers that the way the
    // player does.
    for layer in 1..=lab.per_field {
        for (id, pos, crew) in crews.iter_mut() {
            if committed >= lab.budget {
                return;
            }
            if *crew >= layer {
                continue;
            }
            for u in spare_hands(units, owner, *pos, 1, &mut taken) {
                repair(world, owner, u, *id);
                *crew += 1;
                committed += 1;
            }
        }
    }
}

/// Every tile on the square ring of radius `r`, in deterministic scan order.
fn ring_perimeter(r: i32) -> Vec<(i32, i32)> {
    let mut v = Vec::with_capacity((8 * r) as usize);
    for dx in -r..=r {
        v.push((dx, -r));
        v.push((dx, r));
    }
    for dy in (-r + 1)..r {
        v.push((-r, dy));
        v.push((r, dy));
    }
    v
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
        // What a siege train marches AT — the things that shoot back and the
        // door. A plain wall segment is what an engine breaks THROUGH on its way
        // there; listing it here sent every ram at the nearest five wood of
        // masonry while the tower behind it kept firing. Walls stay in
        // `buildings`, so a defence-free town still gives the engines a target.
        if matches!(*kind, BuildingKind::Gatehouse | BuildingKind::Tower | BuildingKind::Watchtower) {
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
