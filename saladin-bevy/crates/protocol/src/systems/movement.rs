use crate::MatchStatuses;
use crate::components::{MatchId, Pos, Unit};
use bevy_ecs::prelude::*;
use saladin_sim::{ARRIVE_EPS, MOVE_DT, step_toward};

/// Integrate every active mover one base tick toward its target, advancing along
/// its path on arrival. Garrisoned units are off the field; paused matches
/// freeze in place. Ported from the SpacetimeDB `moveUnits` reducer; the
/// spatial-grid `cell` is maintained by the index system, not here.
pub fn movement(statuses: Res<MatchStatuses>, mut q: Query<(&mut Pos, &mut Unit, &MatchId)>) {
    for (mut pos, mut u, mid) in &mut q {
        if u.garrisoned_in != 0 || !u.has_target || !statuses.simulates(mid.0) {
            continue;
        }
        let step = u.speed * MOVE_DT;
        let r = step_toward(pos.pos, u.target, step, ARRIVE_EPS);
        // facing is a render hint; derive it from the heading without trig later
        // on the client. Sim stores position only.
        pos.pos = r.pos;
        if !r.arrived {
            continue;
        }
        let next = u.path_idx + 1;
        if next < u.path.len() {
            u.path_idx = next;
            u.target = u.path[next];
        } else {
            u.has_target = false;
        }
    }
}
