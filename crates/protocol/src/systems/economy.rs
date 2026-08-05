use crate::MatchStatuses;
use crate::WorldConfig;
use crate::components::{
    Building, FieldOf, GameId, MatchId, Owner, Player, Pos, ResourceNode, Unit,
};
use bevy_ecs::prelude::*;
use bevy_platform::collections::HashMap;
use saladin_sim::{
    AuraTarget, ECONOMY_DT, FOOD_YIELD, ResourceType, V2, WorkAura, apply_upkeep, building_def,
    dist, is_passable, operational, unit_def,
};

/// Food upkeep — runs every economy tick (2 s). Only COMBAT units draw rations;
/// peasants/imams feed themselves, so a worker opening never starves and food
/// instead caps army size. A player whose larder runs dry starves and their
/// soldiers bleed hp until fed; a soldier that hits 0 hp dies. Ported from the
/// SpacetimeDB `economySystem` reducer.
#[allow(clippy::too_many_arguments)]
pub fn economy(
    statuses: Res<MatchStatuses>,
    cfg: Res<WorldConfig>,
    mut commands: Commands,
    mut q_players: Query<(&GameId, &mut Player, &MatchId)>,
    mut q_units: Query<(Entity, &Owner, &mut Unit)>,
    q_buildings: Query<(&GameId, &Pos, &Owner, &Building)>,
    mut q_nodes: Query<(&Pos, &mut ResourceNode, Option<&FieldOf>)>,
    mut stats: ResMut<crate::MatchStats>,
) {
    // Regrowth. A sown field comes back on its own — how fast is the soil's
    // doing — and a fishing hut tends the waters in its reach. Everything else
    // (timber, ore, wild herds) is finite and stays mined out.
    // Additive + clamped, so iteration order can never desync the lockstep.
    let auras: Vec<(u64, V2, WorkAura)> = q_buildings
        .iter()
        .filter(|(_, _, _, b)| operational(b.state))
        .filter_map(|(_, p, o, b)| building_def(b.kind).aura.map(|a| (o.0, p.pos, a)))
        .collect();
    // A field belongs to the farm that sowed it, so only that player's granary
    // may tend it. A wild fishery belongs to nobody: whoever plants the hut
    // tends the water, and the ground is contested on purpose.
    let farm_owner: HashMap<u64, u64> =
        q_buildings.iter().map(|(g, _, o, _)| (g.0, o.0)).collect();
    let tended = |at: V2, target: AuraTarget, node_owner: Option<u64>| -> i32 {
        auras
            .iter()
            .filter(|(o, p, a)| {
                a.target == target
                    && node_owner.is_none_or(|n| n == *o)
                    && dist(*p, at) <= a.radius
            })
            .map(|(_, _, a)| a.regen)
            .max()
            .unwrap_or(0)
    };
    for (np, mut n, field) in &mut q_nodes {
        // a sown field grows back to its own capacity, faster under a granary
        if n.regen > 0 && n.remaining < n.cap {
            let owner = field.and_then(|f| farm_owner.get(&f.0).copied());
            let extra = if auras.is_empty() {
                0
            } else {
                tended(np.pos, AuraTarget::Field, Some(owner.unwrap_or(0)))
            };
            n.remaining = (n.remaining + n.regen + extra).min(n.cap);
        }
        // a hut restocks the waters in its reach up to a natural school
        if n.res_type == ResourceType::Food
            && n.remaining < FOOD_YIELD
            && !auras.is_empty()
            && !is_passable(cfg.seed, np.pos.x.to_num::<i32>(), np.pos.y.to_num::<i32>())
        {
            let regen = tended(np.pos, AuraTarget::WaterFood, None);
            if regen > 0 {
                n.remaining = (n.remaining + regen).min(FOOD_YIELD);
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
