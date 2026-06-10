use crate::MatchStatuses;
use crate::components::{MatchId, Player, Research};
use bevy_ecs::prelude::*;
use bevy_platform::collections::HashMap;
use saladin_sim::{Fx, RESEARCH_DT, Tech, set_tech, upgrade_def};

/// Research progress — runs every research tick (1 s). Advances each in-flight
/// tech by one tick of its research time; on completion flips the owner's
/// `tech_mask` bit (combat reads one number) and marks the row done. Ported from
/// the SpacetimeDB `researchSystem` reducer.
pub fn research(
    statuses: Res<MatchStatuses>,
    mut q_research: Query<(&mut Research, &MatchId)>,
    mut q_players: Query<(Entity, &mut Player)>,
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
        if let Some(&pe) = player_ent.get(&r.owner) {
            if let Ok((_, mut p)) = q_players.get_mut(pe) {
                p.tech_mask = set_tech(p.tech_mask, tech);
            }
        }
        r.progress = Fx::ONE;
        r.done = true;
    }
}
