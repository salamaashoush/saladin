use super::player_match;
use crate::WorldConfig;
use crate::components::*;
use bevy_ecs::prelude::*;
use saladin_sim::MatchStatus;

/// Spawn the lifecycle row for `match_id` if it does not exist yet. Joins create
/// it implicitly so every simulated match has a status to gate on.
pub(crate) fn ensure_match(world: &mut World, match_id: u64, host: u64) {
    let exists = {
        let mut q = world.query::<&MatchInfo>();
        q.iter(world).any(|m| m.match_id == match_id)
    };
    if exists {
        return;
    }
    let seed = world.resource::<WorldConfig>().seed;
    world.spawn(MatchInfo {
        match_id,
        name: format!("Match {match_id}"),
        host,
        status: MatchStatus::Active,
        seed,
    });
}

/// Pause/resume the caller's match. Only flips between Active and Paused — an
/// Ended match stays ended. Idempotent. Mirrors `pauseMatch`/`resumeMatch`.
pub(crate) fn set_match_status(world: &mut World, owner: u64, status: MatchStatus) {
    let Some(match_id) = player_match(world, owner) else { return };
    let mut q = world.query::<&mut MatchInfo>();
    if let Some(mut m) = q.iter_mut(world).find(|m| m.match_id == match_id) {
        let legal = matches!(
            (m.status, status),
            (MatchStatus::Active, MatchStatus::Paused) | (MatchStatus::Paused, MatchStatus::Active)
        );
        if legal {
            m.status = status;
        }
    }
}
