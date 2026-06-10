use crate::MatchStatuses;
use crate::WorldConfig;
use crate::components::{Building, GameId, MatchId, Owner, Player, Pos, ResourceNode, Unit};
use bevy_ecs::prelude::*;
use bevy_platform::collections::HashMap;
use saladin_sim::{
    BuildingKind, ECONOMY_DT, FISH_REGEN_PER_TICK, FISHING_HUT_RANGE, FOOD_YIELD, ResourceType,
    apply_upkeep, dist, is_passable, unit_def,
};

/// Food upkeep — runs every economy tick (2 s). Only COMBAT units draw rations;
/// peasants/imams feed themselves, so a worker opening never starves and food
/// instead caps army size. A player whose larder runs dry starves and their
/// soldiers bleed hp until fed; a soldier that hits 0 hp dies. Ported from the
/// SpacetimeDB `economySystem` reducer.
pub fn economy(
    statuses: Res<MatchStatuses>,
    cfg: Res<WorldConfig>,
    mut commands: Commands,
    mut q_players: Query<(&GameId, &mut Player, &MatchId)>,
    mut q_units: Query<(Entity, &Owner, &mut Unit)>,
    q_buildings: Query<(&Pos, &Building)>,
    mut q_nodes: Query<(&Pos, &mut ResourceNode)>,
    mut stats: ResMut<crate::MatchStats>,
) {
    // Fishing huts tend their waters: every water food node in reach of ANY
    // hut regains a little each tick (capped at the natural school size).
    // Additive + clamped, so iteration order can never desync the lockstep.
    let huts: Vec<saladin_sim::V2> = q_buildings
        .iter()
        .filter(|(_, b)| b.kind == BuildingKind::FishingHut)
        .map(|(p, _)| p.pos)
        .collect();
    if !huts.is_empty() {
        for (np, mut n) in &mut q_nodes {
            if n.res_type != ResourceType::Food || n.remaining >= FOOD_YIELD {
                continue;
            }
            let on_water =
                !is_passable(cfg.seed, np.pos.x.to_num::<i32>(), np.pos.y.to_num::<i32>());
            if !on_water {
                continue;
            }
            if huts.iter().any(|h| dist(*h, np.pos) <= FISHING_HUT_RANGE) {
                n.remaining = (n.remaining + FISH_REGEN_PER_TICK).min(FOOD_YIELD);
            }
        }
    }
    // Combat-unit entities grouped by owner (read pass).
    let mut eaters: HashMap<u64, Vec<Entity>> = HashMap::new();
    for (e, owner, unit) in &q_units {
        if unit_def(unit.kind).attack > 0 {
            eaters.entry(owner.0).or_default().push(e);
        }
    }

    for (_gid, mut p, mid) in &mut q_players {
        if p.defeated || !statuses.simulates(mid.0) {
            continue;
        }
        let list = eaters.get(&p.player_id);
        let count = list.map(|v| v.len()).unwrap_or(0) as i32;
        let r = apply_upkeep(p.stock.food, count, p.hunger, ECONOMY_DT);
        if r.food != p.stock.food {
            p.stock.food = r.food;
        }
        // hunger escalates while the larder stays empty, resets the moment
        // the army is fed again
        let new_hunger = if r.starving { (p.hunger + 1).min(1 << 20) } else { 0 };
        if new_hunger != p.hunger {
            p.hunger = new_hunger;
        }
        if !r.starving {
            continue;
        }
        if let Some(list) = list {
            for &e in list {
                if let Ok((_, _, mut u)) = q_units.get_mut(e) {
                    // hunger breaks spirits first...
                    if r.morale_drain > saladin_sim::Fx::ZERO {
                        u.morale = (u.morale - r.morale_drain).max(saladin_sim::MORALE_MIN);
                    }
                    // ...and bodies only after the grace, ramping up
                    if r.hp_drain <= 0 {
                        continue;
                    }
                    let hp = (u.hp - r.hp_drain).max(0);
                    if hp == u.hp {
                        continue;
                    }
                    if hp <= 0 {
                        stats.of(p.player_id).lost += 1;
                        commands.entity(e).despawn();
                    } else {
                        u.hp = hp;
                    }
                }
            }
        }
    }
}
