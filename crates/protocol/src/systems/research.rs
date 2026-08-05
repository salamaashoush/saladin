use crate::MatchStatuses;
use crate::components::{Building, GameId, MatchId, Owner, Player, Research};
use bevy_ecs::prelude::*;
use bevy_platform::collections::HashMap;
use saladin_sim::{
    BuildState, Fx, RESEARCH_DT, Tech, building_hp_delta, effective_building_def, set_tech,
    upgrade_def,
};

/// Research progress — runs every research tick (1 s). Advances each in-flight
/// tech by one tick of its research time; on completion flips the owner's
/// `tech_mask` bit (combat reads one number) and marks the row done. Ported from
/// the SpacetimeDB `researchSystem` reducer.
pub fn research(
    statuses: Res<MatchStatuses>,
    mut q_research: Query<(&mut Research, &MatchId)>,
    mut q_players: Query<(Entity, &mut Player)>,
    mut q_buildings: Query<(Entity, &GameId, &Owner, &mut Building)>,
) {
    let player_ent: HashMap<u64, Entity> = q_players.iter().map(|(e, p)| (p.player_id, e)).collect();

    for (mut r, mid) in &mut q_research {
        if r.done || !statuses.simulates(mid.0) {
            continue;
        }
        let Some(tech) = Tech::from_u8(r.tech) else { continue };
        let up = upgrade_def(tech);
        let step = if up.research_time > Fx::ZERO { RESEARCH_DT / up.research_time } else { Fx::ONE };
        let progress = r.progress + step;
        if progress < Fx::ONE {
            r.progress = progress;
            continue;
        }
        if let Some(&pe) = player_ent.get(&r.owner)
            && let Ok((_, mut p)) = q_players.get_mut(pe)
        {
            let before = p.tech_mask;
            let after = set_tech(before, tech);
            p.tech_mask = after;
            harden(&mut q_buildings, r.owner, before, after);
        }
        r.progress = Fx::ONE;
        r.done = true;
    }
}

/// Masonry thickens the walls you ALREADY have. Max hp is derived from the tech
/// mask, so without this pass finishing a structural tech raises every ceiling
/// and lays no stone: every building you own would simply start reading as
/// damaged. A site is skipped — its hp is the labour banked so far, and the
/// extra course goes on when the crew gets to it.
///
/// GameId order, so two peers apply the same deltas in the same sequence.
fn harden(
    q: &mut Query<(Entity, &GameId, &Owner, &mut Building)>,
    owner: u64,
    before: u64,
    after: u64,
) {
    let mut rows: Vec<(u64, Entity)> =
        q.iter().filter(|(_, _, o, _)| o.0 == owner).map(|(e, g, _, _)| (g.0, e)).collect();
    rows.sort_unstable();
    for (_, e) in rows {
        let Ok((_, _, _, mut b)) = q.get_mut(e) else { continue };
        if b.state == BuildState::Site {
            continue;
        }
        let delta = building_hp_delta(before, after, b.kind);
        if delta == 0 {
            continue;
        }
        let cap = effective_building_def(b.kind, after).max_hp;
        b.hp = (b.hp + delta).clamp(1, cap);
    }
}
