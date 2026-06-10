use super::{building_occupancy, clamp_world, match_ctl, tech_mask_of};
use crate::components::*;
use crate::{NextEntityId, WorldConfig};
use bevy_ecs::prelude::*;
use saladin_sim::*;

/// Scatter every resource node kind across the map for `match_id` from the
/// seeded pure worldgen — call once at match start (mirrors the SpacetimeDB
/// `scatterNodes`). Reproducible: every client lays the same forest.
pub fn scatter_world_nodes(world: &mut World, match_id: u64) {
    let seed = world.resource::<WorldConfig>().seed;
    let mut nodes = scatter_nodes(seed, &node_kinds());
    // fair starts: top every spawn slot up to the guaranteed minimum of
    // wood/stone/food within FAIR_RADIUS — mirrored across players
    let extra = fair_start_nodes(seed, &nodes, MAX_PLAYERS, TREE_WOOD, STONE_YIELD, FOOD_YIELD);
    nodes.extend(extra);
    for n in nodes {
        let id = world.resource_mut::<NextEntityId>().alloc();
        world.spawn((
            GameId(id),
            MatchId(match_id),
            Pos { pos: n.pos, facing: Fx::ZERO },
            ResourceNode { res_type: n.res_type, remaining: n.yield_ },
        ));
    }
}

/// Eight fixed directions for the deterministic peasant start-cluster (the TS
/// version used cos/sin; replaced for determinism).
pub(crate) const DIRS8: [(Fx, Fx); 8] = [
    (saladin_sim::fx!("1"), saladin_sim::fx!("0")),
    (saladin_sim::fx!("0.7"), saladin_sim::fx!("0.7")),
    (saladin_sim::fx!("0"), saladin_sim::fx!("1")),
    (saladin_sim::fx!("-0.7"), saladin_sim::fx!("0.7")),
    (saladin_sim::fx!("-1"), saladin_sim::fx!("0")),
    (saladin_sim::fx!("-0.7"), saladin_sim::fx!("-0.7")),
    (saladin_sim::fx!("0"), saladin_sim::fx!("-1")),
    (saladin_sim::fx!("0.7"), saladin_sim::fx!("-0.7")),
];

pub(crate) fn spawn_building(world: &mut World, owner: u64, kind: BuildingKind, pos: V2, match_id: u64) -> u64 {
    let mask = tech_mask_of(world, owner);
    let def = effective_building_def(kind, mask);
    let id = world.resource_mut::<NextEntityId>().alloc();
    world.spawn((
        GameId(id),
        Owner(owner),
        MatchId(match_id),
        Pos { pos, facing: Fx::ZERO },
        Building { kind, hp: def.max_hp, cooldown: Fx::ZERO, rally: pos },
    ));
    id
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_unit(
    world: &mut World,
    owner: u64,
    kind: UnitKind,
    pos: V2,
    match_id: u64,
    gather_state: GatherState,
    target_node: u64,
) -> u64 {
    let mask = tech_mask_of(world, owner);
    let def = effective_unit_def(kind, mask);
    let id = world.resource_mut::<NextEntityId>().alloc();
    world.spawn((
        GameId(id),
        Owner(owner),
        MatchId(match_id),
        Pos { pos, facing: Fx::ZERO },
        Unit {
            kind,
            target: pos,
            has_target: false,
            speed: def.speed,
            gather_state,
            target_node,
            carrying: 0,
            carry_type: ResourceType::Wood,
            harvest_timer: Fx::ZERO,
            hp: def.max_hp,
            attack_target: 0,
            attack_cooldown: Fx::ZERO,
            stance: Stance::Aggressive,
            morale: MORALE_MAX,
            routing: false,
            home: pos,
            garrisoned_in: 0,
            path: vec![],
            path_idx: 0,
        },
    ));
    id
}

/// Spawn a skirmish bot: found its base, then attach the `Bot` driver component.
pub(crate) fn spawn_ai(
    world: &mut World,
    player_id: u64,
    host: u64,
    difficulty: AiDifficulty,
    faction: Faction,
    match_id: u64,
) {
    let name = ai_name(faction, (player_id as usize) % 8);
    found_player(world, player_id, name, faction, match_id);
    let pe = {
        let mut q = world.query::<(Entity, &Player)>();
        q.iter(world).find(|(_, p)| p.player_id == player_id).map(|(e, _)| e)
    };
    if let Some(e) = pe {
        let prof = ai_profile(difficulty);
        world.entity_mut(e).insert(Bot {
            host,
            difficulty,
            decision_cd: Fx::ZERO,
            wave_timer: prof.first_wave_delay,
            phase: AiPhase::Boot,
            scout_id: 0,
            threat_timer: Fx::ZERO,
        });
    }
}

pub(crate) fn found_player(world: &mut World, player_id: u64, name: &str, faction: Faction, match_id: u64) {
    let seed = world.resource::<WorldConfig>().seed;
    match_ctl::ensure_match(world, match_id, player_id);

    // stable slot among players already in this match
    let used: Vec<i32> = {
        let mut q = world.query::<(&Player, &MatchId)>();
        q.iter(world).filter(|(_, m)| m.0 == match_id).map(|(p, _)| p.slot as i32).collect()
    };
    let slot = alloc_slot(&used, MAX_PLAYERS as i32).max(0);
    let keep_fp = building_def(BuildingKind::Keep).footprint;
    // validated site on the dominant landmass with open ground around it (a
    // keep against cliffs/water strands the deposit economy); the occupancy
    // pass then only needs to dodge other players' buildings
    let site = find_keep_site(seed, slot as usize, keep_fp);

    let occ = building_occupancy(world, true);
    let passable = |tx: i32, ty: i32| is_passable(seed, tx, ty) && !occ.contains(&tile_key(tx, ty));
    let base = find_buildable_near(site.x, site.y, keep_fp, passable);

    let keep_id = spawn_building(world, player_id, BuildingKind::Keep, base, match_id);

    let player_ent_id = world.resource_mut::<NextEntityId>().alloc();
    world.spawn((
        GameId(player_ent_id),
        MatchId(match_id),
        Player {
            player_id,
            name: name.to_string(),
            faction,
            stock: Stockpile { wood: START_WOOD, stone: START_STONE, food: START_FOOD, gold: START_GOLD },
            color: (slot as usize % PLAYER_COLORS.len()) as u8,
            online: true,
            keep: keep_id,
            defeated: false,
            slot: slot as u8,
            tech_mask: 0,
        },
    ));

    // nodes for gather assignment
    let nodes: Vec<(u64, V2, ResourceType)> = {
        let mut q = world.query::<(&GameId, &Pos, &ResourceNode, &MatchId)>();
        q.iter(world).filter(|(_, _, _, m)| m.0 == match_id).map(|(g, p, n, _)| (g.0, p.pos, n.res_type)).collect()
    };
    let available: Vec<ResourceType> = {
        let mut seen = Vec::new();
        for (_, _, rt) in &nodes {
            if !seen.contains(rt) {
                seen.push(*rt);
            }
        }
        seen
    };
    let types = balanced_gather_types(&available, START_PEASANTS as usize);

    for i in 0..START_PEASANTS as usize {
        let (dx, dy) = DIRS8[i % DIRS8.len()];
        let px = clamp_world(base.x + dx * SPAWN_CLUSTER);
        let py = clamp_world(base.y + dy * SPAWN_CLUSTER);
        let pos = V2::new(px, py);
        // nearest node of the assigned type
        let (state, node) = if let Some(&want) = types.get(i) {
            let typed: Vec<(u64, V2)> =
                nodes.iter().filter(|(_, _, rt)| *rt == want).map(|(id, p, _)| (*id, *p)).collect();
            let pool: Vec<(u64, V2)> =
                if typed.is_empty() { nodes.iter().map(|(id, p, _)| (*id, *p)).collect() } else { typed };
            let mut best: Option<u64> = None;
            let mut best_d = Fx::MAX;
            for (id, p) in &pool {
                if !node_reachable(seed, pos, *p) {
                    continue;
                }
                let d = dist2(pos, *p);
                if d < best_d {
                    best_d = d;
                    best = Some(*id);
                }
            }
            match best {
                Some(id) => (GatherState::ToResource, id),
                None => (GatherState::Idle, 0),
            }
        } else {
            (GatherState::Idle, 0)
        };
        spawn_unit(world, player_id, UnitKind::Peasant, pos, match_id, state, node);
    }
}
