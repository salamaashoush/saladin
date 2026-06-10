use crate::MatchStatuses;
use crate::components::{GameId, MatchId, Owner, Player, Unit};
use bevy_ecs::prelude::*;
use bevy_platform::collections::HashMap;
use saladin_sim::{ECONOMY_DT, apply_upkeep, unit_def};

/// Food upkeep — runs every economy tick (2 s). Only COMBAT units draw rations;
/// peasants/imams feed themselves, so a worker opening never starves and food
/// instead caps army size. A player whose larder runs dry starves and their
/// soldiers bleed hp until fed; a soldier that hits 0 hp dies. Ported from the
/// SpacetimeDB `economySystem` reducer.
pub fn economy(
    statuses: Res<MatchStatuses>,
    mut commands: Commands,
    mut q_players: Query<(&GameId, &mut Player, &MatchId)>,
    mut q_units: Query<(Entity, &Owner, &mut Unit)>,
    mut stats: ResMut<crate::MatchStats>,
) {
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
        let r = apply_upkeep(p.stock.food, count, ECONOMY_DT);
        if r.food != p.stock.food {
            p.stock.food = r.food;
        }
        if !r.starving || r.hp_drain <= 0 {
            continue;
        }
        if let Some(list) = list {
            for &e in list {
                if let Ok((_, _, mut u)) = q_units.get_mut(e) {
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
