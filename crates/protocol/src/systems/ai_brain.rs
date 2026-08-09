use crate::commands::{
    assign_builders, assign_idle_gatherers, build_context, build_with, disembark, embark,
    garrison, group_attack, group_move, market_buy_cmd, market_trade, move_unit, repair,
    start_research, train, ungarrison, upgrade_building,
};
use crate::components::*;
use bevy_ecs::prelude::*;
use bevy_platform::collections::HashMap;
use saladin_sim::*;
use std::collections::HashSet as StdSet;

const AI_BRAIN_DT: Fx = saladin_sim::AI_BRAIN_DT;
const HOME_THREAT_RADIUS: Fx = saladin_sim::fx!("24"); // enemy combatants this close to home = a threat
const HOME_RADIUS: Fx = saladin_sim::fx!("18"); // own combat units this close to a building count as "home"
// Tile radius around the keep the bot looks for a shoreline in. 14 missed
// several percent of starts outright — a Highlands keep sits up to ~18 tiles
// from the nearest water — and 20 is still inside TOWN_RADIUS on the diagonal,
// so nothing it finds is refused for being outside the town.
const SHORE_SCAN: i32 = 20;
/// How far out a bot looks for ground it can SOW. Deliberately narrower than
/// `SHORE_SCAN`: a soil probe is re-run every decision window forever, a shore
/// probe once behind a cooldown.
const SOIL_SCAN: i32 = 14;
/// How far out a skiff is worth sending. Also the fleet cap: three boats over
/// one school is 60 wood of queue.
const FISHERY_RANGE: Fx = saladin_sim::fx!("40");
/// Seconds before a bot probes the shoreline again after a waterside site was
/// refused. This used to be a LATCH, so one blocked probe — a peasant standing
/// on the only legal tile — disabled fishing for that bot for the whole match.
const WATERSIDE_RETRY: Fx = saladin_sim::fx!("45");
/// Open sea this close to a stranded cluster means a barge can put men on it.
const OFFSHORE_COAST_SCAN: i32 = 6;
/// A hull this close to its quay is loading, and this close to the far shore is
/// landing.
const BERTH_NEAR: Fx = saladin_sim::fx!("2.5");
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
    waterside_cd: Fx,
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
                waterside_cd: b.waterside_cd,
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
    // Hulls paid for and still in a hall's queue. A skiff takes ten seconds to
    // build and a Hard bot decides every 0.6 — without this the fleet rung
    // re-orders the same boat until the first one splashes, and a three-boat
    // target buys six.
    let mut queued_hulls: HashMap<u64, (i32, i32)> = HashMap::new();
    {
        let mut q = world.query::<(&Owner, &Building)>();
        for (o, b) in q.iter(world) {
            for k in b.queued() {
                let Some(def) = UnitKind::from_u8(*k).map(unit_def) else { continue };
                if !def.afloat() {
                    continue;
                }
                let e = queued_hulls.entry(o.0).or_insert((0, 0));
                if def.cargo_cap > 0 {
                    e.1 += 1;
                } else {
                    e.0 += 1;
                }
            }
        }
    }
    // Every footprint on the map, for the one question the brain asks about
    // ground it does not own: where beside a berth can a column actually stand.
    let occupants: Vec<Occupant> =
        buildings.iter().map(|(_, p, _, k, _)| Occupant { kind: *k, pos: *p }).collect();
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
        // Hulls by trade, never by kind: a skiff carries a load and a barge
        // carries men, and the two rungs that build them ask exactly that.
        let (mut boats, mut ferries) = queued_hulls.get(&owner).copied().unwrap_or((0, 0));
        for u in &units {
            if u.owner != owner {
                continue;
            }
            let def = unit_def(u.kind);
            pop += def.pop_cost;
            if def.afloat() {
                if def.cargo_cap > 0 {
                    ferries += 1;
                } else {
                    boats += 1;
                }
            }
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

        let seed = world.resource::<crate::WorldConfig>().seed;

        // The fish a hut on this coast could actually put a boat on. "Any water
        // tile in the box" was the old gate: it bought a 40-wood drop-off beside
        // a puddle with nothing in it, and it could not tell a bot how many
        // hulls that water was worth.
        // ...and only while it can still change a decision. Once the hut stands
        // and its fleet is out, `min(skiff_target, fisheries)` cannot move a rung
        // whatever the count is, and this is a per-bot per-brain-tick scan.
        let counts_fish = tune.wants_fishing
            && (!owned.contains(&BuildingKind::FishingHut) || boats < tune.skiff_target);
        let home_water = counts_fish.then(|| nearest_water(seed, keep_pos, SHORE_SCAN)).flatten();
        let mut fish_near: Vec<V2> = Vec::new();
        if let Some(water) = home_water {
            let far2 = FISHERY_RANGE * FISHERY_RANGE;
            for (id, npos) in &node_pos {
                if node_type.get(id) != Some(&ResourceType::Food) {
                    continue;
                }
                if !is_sailable(seed, npos.x.to_num::<i32>(), npos.y.to_num::<i32>()) {
                    continue;
                }
                if dist2(*npos, keep_pos) > far2 || !sea_reachable(seed, water, *npos) {
                    continue;
                }
                fish_near.push(*npos);
            }
        }
        // Only counted and summed below, both order-independent — but the list
        // is ordered anyway so that a future reader that picks ONE of these
        // cannot pick a different one on a different peer.
        fish_near.sort_unstable_by(|a, b| a.x.cmp(&b.x).then(a.y.cmp(&b.y)));
        let fisheries = fish_near.len() as i32;
        let fishery_centroid = (fisheries > 0).then(|| {
            let mut sum = V2::new(Fx::ZERO, Fx::ZERO);
            for p in &fish_near {
                sum = V2::new(sum.x + p.x, sum.y + p.y);
            }
            V2::new(sum.x / Fx::from_num(fisheries), sum.y / Fx::from_num(fisheries))
        });

        // The enemy the bot must reach, and whether it can WALK there. An enemy
        // across water is an enemy a land army can never touch, which is what
        // turns a ferry from a convenience into the win condition.
        let enemy_keep = buildings
            .iter()
            .filter(|(_, _, o, k, m)| {
                *m == bot.match_id
                    && *k == BuildingKind::Keep
                    && faction_of.get(o).copied() == Some(saladin_sim::enemy_faction(bot.faction))
            })
            .min_by_key(|(id, p, _, _, _)| (dist2(keep_pos, *p), *id))
            .map(|(_, p, _, _, _)| *p);
        let enemy_by_land =
            enemy_keep.is_none_or(|p| saladin_sim::node_reachable(seed, keep_pos, p));

        // A cluster on the bot's water but off its land. `remote_cluster` cannot
        // see one — everything past the beach fails `node_reachable`, so nothing
        // over there is ever a candidate — and it is the economic half of the
        // reason a Harbour exists.
        // Only while there is no quay: past that the cluster gates nothing, and
        // this walks every node on the map.
        let offshore_cluster = if tune.barge_target > 0
            && !lifecycles.iter().any(|l| l.owner == owner && l.kind == BuildingKind::Harbour)
        {
            let mut best: Option<(Fx, u64, V2)> = None;
            for (id, npos) in &node_pos {
                if is_sailable(seed, npos.x.to_num::<i32>(), npos.y.to_num::<i32>()) {
                    continue; // a fishery is the skiff's job, not the barge's
                }
                if saladin_sim::node_reachable(seed, keep_pos, *npos) {
                    continue;
                }
                let d = dist2(*npos, keep_pos);
                if let Some((bd, bid, _)) = best
                    && (d > bd || (d == bd && *id >= bid))
                {
                    continue;
                }
                // ...and a barge has to be able to get there. An inland pocket
                // behind a cliff reads the same as an island until you ask.
                if !sea_touches(seed, *npos, OFFSHORE_COAST_SCAN) {
                    continue;
                }
                best = Some((d, *id, *npos));
            }
            best.map(|(_, _, p)| p)
        } else {
            None
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
            let mut found = false;
            'soil: for dy in -SOIL_SCAN..=SOIL_SCAN {
                for dx in -SOIL_SCAN..=SOIL_SCAN {
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
        let mut sites_in_flight = [0i32; BuildingKind::ALL.len()];
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
            campaign_food: saladin_sim::campaign_reserve(soldiers),
            soldiers,
            army_composition: army_comp,
            sieges,
            towers,
            owned: owned.clone(),
            enemy,
            enemy_has_walls,
            threat_near_home: threat,
            fisheries,
            fishery_centroid,
            offshore_cluster,
            boats,
            ferries,
            enemy_by_land,
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
        // Bread musters the army and bread carries it. The bot prices both off
        // `campaign_reserve`, which is the ONE supply rate spent over a full
        // campaign — the old estimate multiplied a per-head poll tax by a
        // difficulty knob and had the bot hoarding four times what it needed.
        let crisis = food_crisis(&state, &tune);
        // The LOW mark: hands go back on food when the larder cannot raise a
        // couple of men. It is deliberately far below `food_cushion`, the high
        // mark — a narrow band between them makes the whole workforce change
        // trade every few seconds, and a peasant that is walking is a peasant
        // that is not gathering.
        let cushion = tune.food_floor * 2;
        // Enter at the cushion, leave at half again: a bare threshold makes the
        // whole workforce change trade on every crossing, and a peasant that is
        // walking is a peasant that is not gathering.
        let food_emergency =
            crisis || stock.food <= cushion || (bot.famine && stock.food <= cushion + cushion / 2);
        // A pile past the war chest is hands that should be on timber. The same
        // mark `next_trade` sells at and `field_labour` staffs to, so the three
        // cannot disagree about what a glut is. No longer gated on owning no
        // army: bread is the army currency, so "soldiers == 0" stopped meaning
        // "food has no use".
        let food_surplus = !food_emergency && stock.food > food_cushion(&state, &tune);
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
        let mut waterside_cd = (bot.waterside_cd - AI_BRAIN_DT).max(Fx::ZERO);
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
            // A refused shoreline is suppressed the same way any other blocked
            // rung is, and for the same reason — except it comes BACK, because a
            // waterside site is refused for reasons that pass (a peasant on the
            // one legal tile) as often as for reasons that do not.
            if waterside_cd > Fx::ZERO {
                for k in [BuildingKind::FishingHut, BuildingKind::Harbour] {
                    plan_state.sites_in_flight[k as usize] += LADDER_SUPPRESS;
                }
            }
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
                            // WHERE a building goes is half of what it does. A
                            // Fishing Hut was anchored on the keep, which is the
                            // one place its drop-off is worth nothing, because
                            // the keep already accepts food.
                            let anchor = match kind {
                                BuildingKind::Storehouse => {
                                    state.remote_cluster.unwrap_or(keep_pos)
                                }
                                // A hut is sited on the SCHOOLS it can reach,
                                // not on the average position of every fish in
                                // forty tiles: scattered fisheries average out
                                // to open water with nothing in it.
                                BuildingKind::FishingHut => {
                                    shore_anchor(seed, keep_pos, SHORE_SCAN, false, |c| {
                                        let n = fish_near
                                            .iter()
                                            .filter(|f| dist(**f, c) <= FISHING_HUT_RANGE)
                                            .count() as i32;
                                        (-n, dist2(c, keep_pos))
                                    })
                                    .unwrap_or(keep_pos)
                                }
                                BuildingKind::Harbour => {
                                    let aim = offshore_cluster
                                        .or(enemy_keep)
                                        .or(fishery_centroid)
                                        .unwrap_or(keep_pos);
                                    shore_anchor(seed, keep_pos, SHORE_SCAN, true, |c| {
                                        (0, dist2(c, aim))
                                    })
                                    .unwrap_or(keep_pos)
                                }
                                _ if tends_fields(kind) => farm_centroid.unwrap_or(keep_pos),
                                _ => keep_pos,
                            };
                            if reserve_ok {
                                let placed = place_near(world, owner, kind, anchor);
                                if placed.is_none() && stock.can_afford(&building_def(kind).cost) {
                                    blocked = Some(kind);
                                    if building_def(kind).requires_water {
                                        waterside_cd = WATERSIDE_RETRY;
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
            /// The landmass the man is standing on. An order across water is not
            /// a slow order, it is an A* that walks its whole expansion budget
            /// and then hands back nothing — so who can reach what is asked HERE,
            /// with an O(1) read off a cached grid, and never by trying.
            region: u16,
        }
        // A unit on a tile with no region at all is never filtered out, exactly
        // as `node_reachable` refuses to over-filter a walker on odd ground.
        let home_region = region_at(seed, keep_pos.x, keep_pos.y);
        let enemy_region = enemy_keep.map(|p| region_at(seed, p.x, p.y));
        let marches = |r: u16| enemy_region.is_none_or(|e| r == u16::MAX || r == e);
        let returns = |r: u16| r == u16::MAX || r == home_region;
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
                region: region_at(seed, u.pos.x, u.pos.y),
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

        let mut wave_timer = bot.wave_timer - AI_BRAIN_DT;
        let mut wave_launched = bot.wave_launched;
        // Power gate: only commit a wave with a real strength edge over the
        // defender's field army + towers. A double-muster overrides the gate
        // so a turtling stalemate still breaks.
        let overwhelming = soldiers >= prof.wave_size * 2;
        let strong_enough =
            overwhelming || should_assault(&army_comp, &enemy, enemy_towers, tac.advantage_margin_pct);

        // ── the crossing: an army that cannot walk to the enemy sails at him ──
        // Crude by design: muster on the quay, fill the hold, cross, put the
        // party on the nearest legal enemy shore and hand STRAIGHT OFF to the
        // land assault below. No escort, no beach selection, no withdrawal by
        // sea. It will lose to a competent human every time — and it is the
        // difference between an island match resolving and two bots staring at
        // each other across the water until the clock runs out.
        let crossing = !enemy_by_land
            && ferries > 0
            && phase != AiPhase::Defend
            && !under_attack
            && mustered(soldiers, prof.wave_size)
            && strong_enough;
        // A hold with men in it finishes its crossing whatever the muster gate
        // says NOW. The gate falls the moment the wave takes casualties, and a
        // laden barge left floating is a landing party deleted from the match.
        let afloat: Vec<u64> = units
            .iter()
            .filter(|u| u.owner == owner && unit_def(u.kind).cargo_cap > 0 && u.garrisoned_in == 0)
            .map(|u| u.id)
            .collect();
        let laden = !afloat.is_empty()
            && units.iter().any(|u| u.garrisoned_in != 0 && afloat.contains(&u.garrisoned_in));
        let quay = lifecycles
            .iter()
            .find(|l| l.owner == owner && l.kind == BuildingKind::Harbour && operational(l.state))
            .and_then(|h| berth_of(seed, building_def(BuildingKind::Harbour).footprint, h.pos));

        let mut hulls: Vec<(u64, V2, i32, bool)> = units
            .iter()
            .filter(|u| u.owner == owner && unit_def(u.kind).cargo_cap > 0 && u.garrisoned_in == 0)
            .map(|u| (u.id, u.pos, unit_def(u.kind).cargo_cap, u.idle))
            .collect();
        hulls.sort_unstable_by_key(|(id, _, _, _)| *id);

        // The water this crossing happens on. It is the quay's while the quay
        // stands — but a LADEN hull has to finish whatever happens ashore. The
        // party aboard is already paid for, it is off the muster roll and out of
        // the fight, and a barge with nowhere left to report back to is six men
        // deleted from the match. So a razed harbour falls back to the hull's own
        // water, lowest id first so every peer picks the same body.
        let anchor = quay.or_else(|| hulls.first().map(|(_, p, _, _)| *p));
        if (crossing || laden)
            && let Some(ekeep) = enemy_keep
            && let Some(anchor) = anchor
        {
            let body = water_region_at(seed, anchor.x, anchor.y);
            let landing = landing_water(seed, ekeep, body, LANDING_SCAN);
            let occ = occupancy_set(&occupants, false);
            // The men and the hull are sent to the SAME point. A hull at its
            // berth and a column at the quay beside it end up four tiles apart
            // across a corner, which is outside a gangplank, and the barge then
            // sits full of nobody forever. With no quay there is no muster at
            // all: a hold cannot be filled from a shore that has no jetty, and
            // the only thing left to do is land what is already aboard.
            //
            // And the muster is DRY GROUND or it is nothing. Falling back to the
            // berth itself put the column on a water tile: every called man
            // marched to it, arrived, and stood in the sea for the rest of the
            // match. A quay with no standing room beside it cannot be loaded
            // from, and pretending otherwise deletes the men who try.
            let muster = quay.and_then(|b| quay_spot(seed, &occ, b, keep_pos));

            // The men still on the wrong side of the water, nearest the muster
            // first — the same order on every peer, because it decides who
            // boards.
            let rally = muster.unwrap_or(anchor);
            let mut party: Vec<(Fx, u64, bool, V2)> = army
                .iter()
                .filter(|a| !marches(a.region))
                .map(|a| (dist2(a.pos, rally), a.id, a.idle, a.pos))
                .collect();
            party.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

            // Call up exactly the boarding party — the hold that is standing
            // empty, no more. The rest of the army holds the island; dragging it
            // all onto one quay is how a bot loses its home while it is away.
            //
            // ONE ORDER PER MAN, not a group march: `lay_march` routes from the
            // GROUP CENTROID, and a knot of men whose centroid falls on a
            // building footprint gets an empty route and NOBODY moves. Measured,
            // that was six men re-issued the same doomed order every second for
            // the rest of the match while a seventh walked in alone.
            let berths_free: i32 = hulls
                .iter()
                .map(|(bid, _, cap, _)| {
                    (*cap - units.iter().filter(|u| u.garrisoned_in == *bid).count() as i32).max(0)
                })
                .sum();
            let called: Vec<u64> = match muster {
                Some(_) if crossing => party
                    .iter()
                    .filter(|(d2, _, idle, _)| *idle && *d2 > EMBARK_RANGE * EMBARK_RANGE)
                    .take(berths_free.max(0) as usize)
                    .map(|(_, id, _, _)| *id)
                    .collect(),
                _ => Vec::new(),
            };
            for id in called {
                move_unit(world, owner, id, rally);
            }

            for (bid, bpos, cap, bidle) in &hulls {
                let aboard = units.iter().filter(|u| u.garrisoned_in == *bid).count() as i32;
                // A hold that waits to be full waits forever on a bot with five
                // soldiers; a hold that sails with one shuttles all match.
                // A hold that waits for the quay that no longer exists waits
                // forever: with no muster left, whatever is aboard IS the party.
                let want = if muster.is_some() { (*cap).min(soldiers).max(1) } else { 1 };
                if aboard >= want {
                    let Some(l) = landing else { continue };
                    if dist(*bpos, l) <= BERTH_NEAR {
                        disembark(world, owner, *bid, ekeep);
                        // A landing IS a launch: without this the party stands
                        // on the beach until the next muster interval, because
                        // `recommit` only moves men from a wave that went out.
                        wave_launched = wave_launched.max(soldiers);
                    } else if *bidle {
                        move_unit(world, owner, *bid, l);
                    }
                } else if muster.is_none() {
                    continue; // an empty hull with no quay has nothing to do
                } else if dist(*bpos, rally) > BERTH_NEAR {
                    if *bidle {
                        move_unit(world, owner, *bid, rally);
                    }
                } else if crossing {
                    // whoever is within a gangplank of THIS hull, closest first
                    let mut reach: Vec<(Fx, u64)> = party
                        .iter()
                        .map(|(_, id, _, p)| (dist2(*p, *bpos), *id))
                        .filter(|(d2, _)| *d2 <= EMBARK_RANGE * EMBARK_RANGE)
                        .collect();
                    reach.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
                    let take: Vec<u64> = reach
                        .into_iter()
                        .take((want - aboard).max(0) as usize)
                        .map(|(_, id)| id)
                        .collect();
                    party.retain(|(_, id, _, _)| !take.contains(id));
                    embark(world, owner, &take, *bid);
                }
            }
        }

        // ── assault: muster to wave_size, then march squads onto role targets ──
        // Hold while Defending or recalling; commit a full wave at once. Siege
        // leads onto fortifications, the main body besieges the keep, and the
        // fastest raider-class units peel off to harass the enemy economy.
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
            && army.iter().any(|a| !a.at_home && a.idle && marches(a.region));
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
                    // A march order across water is an A* that spends its whole
                    // budget and returns nothing, every brain tick, for the rest
                    // of the match. The men on the wrong side wait for a hull.
                    if !marches(a.region) {
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
        // A party put ashore on the far island cannot walk home, so it is not
        // part of a retreat: ordering it to would buy a failed path per man.
        let field_units: Vec<(Entity, u64)> = army
            .iter()
            .filter(|a| !a.at_home && returns(a.region))
            .map(|a| (a.entity, a.id))
            .collect();
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
        // A scout cannot swim: sending one at an enemy across water buys a failed
        // full-budget A* and a peasant that never leaves the yard.
        if tac.scouts && enemy_by_land && !scout_alive && !launched {
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
            b.waterside_cd = waterside_cd;
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
    //
    // The two radii are SEPARATE on purpose. A bot at its soil limit re-probes
    // for a farm it cannot place every decision window, so this loop's width is
    // a per-window cost paid forever — widening it to reach a Highlands
    // shoreline cost 60% of the AI's whole budget on a mainland map, measured.
    if building_def(kind).requires_water || building_def(kind).min_fertility > Fx::ZERO {
        let scan = if building_def(kind).requires_water { SHORE_SCAN } else { SOIL_SCAN };
        for r in 2..=scan {
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
    // A farm still going UP is field labour too — its crew stands in the plot,
    // not in the woods, and converts to a tending crew the moment the plot tops
    // out. Counting only finished fields let a bot raising four farms at once
    // hire a full crew for each and then hand every one of them to the crop
    // together: 13 of 14 peasants in the fields for a beat, measured.
    //
    // Counted, but never STRIPPED: taking hands off a foundation to stay under
    // the tending budget just stops the farm being built, which is how an island
    // bot lost its economy and never crossed. Sites bound what the finished
    // fields may hire; they are not themselves thinned.
    let raising: i32 = jobs
        .iter()
        .filter(|j| j.owner == owner && j.kind == BuildingKind::Farm && !operational(j.state))
        .map(|j| crew_of(j.id))
        .sum();
    let mut committed: i32 = crews.iter().map(|(_, _, c)| *c).sum::<i32>() + raising;
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

/// How far out from an enemy keep the brain looks for water to land beside.
const LANDING_SCAN: i32 = 24;
/// How far from a berth a column will stand to wait for it.
const QUAY_SCAN: i32 = 5;

/// The best tile on the FIRST square ring out from `at` that satisfies `ok` —
/// `score` orders the ring (lower wins) and `tile_key` breaks its ties, so every
/// peer answers with the same tile.
fn ring_pick<A: Fn(i32, i32) -> bool, S: Fn(V2) -> Fx>(
    at: V2,
    scan: i32,
    ok: A,
    score: S,
) -> Option<V2> {
    let (cx, cy) = (at.x.to_num::<i32>(), at.y.to_num::<i32>());
    let half = saladin_sim::fx!("0.5");
    for r in 0..=scan {
        let mut best: Option<(Fx, i32, V2)> = None;
        for (dx, dy) in ring_perimeter(r) {
            let (tx, ty) = (cx + dx, cy + dy);
            if tx < 0 || ty < 0 || tx >= WORLD_SIZE || ty >= WORLD_SIZE || !ok(tx, ty) {
                continue;
            }
            let c = V2::new(Fx::from_num(tx) + half, Fx::from_num(ty) + half);
            let (s, k) = (score(c), tile_key(tx, ty));
            match best {
                Some((bs, bk, _)) if s > bs || (s == bs && k >= bk) => {}
                _ => best = Some((s, k, c)),
            }
        }
        if best.is_some() {
            return best.map(|(_, _, p)| p);
        }
    }
    None
}

/// The water this coast opens onto: nearest sailable tile to `at`.
fn nearest_water(seed: u32, at: V2, scan: i32) -> Option<V2> {
    ring_pick(at, scan, |tx, ty| is_sailable(seed, tx, ty), |c| dist2(c, at))
}

/// Open sea on the map's MAIN body within `r` of `at` — whether a barge could
/// ever put men on this ground. An inland pocket behind a cliff reads exactly
/// like an island until you ask.
fn sea_touches(seed: u32, at: V2, r: i32) -> bool {
    let ocean = main_water_body(seed);
    let (cx, cy) = (at.x.to_num::<i32>(), at.y.to_num::<i32>());
    let half = saladin_sim::fx!("0.5");
    for dy in -r..=r {
        for dx in -r..=r {
            let (tx, ty) = (cx + dx, cy + dy);
            if tx < 0 || ty < 0 || tx >= WORLD_SIZE || ty >= WORLD_SIZE {
                continue;
            }
            if is_sailable(seed, tx, ty)
                && water_region_at(seed, Fx::from_num(tx) + half, Fx::from_num(ty) + half) == ocean
            {
                return true;
            }
        }
    }
    false
}

/// Where a waterside building belongs: buildable ground inside `scan` of the
/// keep with water against it, ranked by `score` (lower wins, ties on tile key).
/// This is the whole fix for a Fishing Hut planted beside the Keep — a hut's
/// only working function is being a drop-off near the fish, and the Keep already
/// accepts food.
fn shore_anchor<S: Fn(V2) -> (i32, Fx)>(
    seed: u32,
    keep: V2,
    scan: i32,
    sea_only: bool,
    score: S,
) -> Option<V2> {
    let ocean = main_water_body(seed);
    let (cx, cy) = (keep.x.to_num::<i32>(), keep.y.to_num::<i32>());
    let half = saladin_sim::fx!("0.5");
    let mut best: Option<((i32, Fx), i32, V2)> = None;
    for dy in -scan..=scan {
        for dx in -scan..=scan {
            let (tx, ty) = (cx + dx, cy + dy);
            if tx < 0 || ty < 0 || tx >= WORLD_SIZE || ty >= WORLD_SIZE {
                continue;
            }
            if !is_buildable_tile(seed, tx, ty) {
                continue;
            }
            let waterside = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(ox, oy)| {
                let (nx, ny) = (tx + ox, ty + oy);
                is_sailable(seed, nx, ny)
                    && (!sea_only
                        || water_region_at(seed, Fx::from_num(nx) + half, Fx::from_num(ny) + half)
                            == ocean)
            });
            if !waterside {
                continue;
            }
            let c = V2::new(Fx::from_num(tx) + half, Fx::from_num(ty) + half);
            let (s, k) = (score(c), tile_key(tx, ty));
            match best {
                Some((bs, bk, _)) if s > bs || (s == bs && k >= bk) => {}
                _ => best = Some((s, k, c)),
            }
        }
    }
    best.map(|(_, _, p)| p)
}

/// Water beside the enemy, on the body the hull is already floating in: where a
/// crossing ends. `disembark` finds the beach from there.
fn landing_water(seed: u32, target: V2, body: u16, scan: i32) -> Option<V2> {
    ring_pick(
        target,
        scan,
        |tx, ty| {
            let half = saladin_sim::fx!("0.5");
            is_sailable(seed, tx, ty)
                && water_region_at(seed, Fx::from_num(tx) + half, Fx::from_num(ty) + half) == body
        },
        |c| dist2(c, target),
    )
}

/// Dry ground beside a berth for a column to wait on, ties toward `toward` (the
/// town) so the muster forms on the landward side of the quay.
fn quay_spot(
    seed: u32,
    occ: &std::collections::HashSet<i32>,
    berth: V2,
    toward: V2,
) -> Option<V2> {
    ring_pick(
        berth,
        QUAY_SCAN,
        |tx, ty| is_passable(seed, tx, ty) && !occ.contains(&tile_key(tx, ty)),
        |c| dist2(c, toward),
    )
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
