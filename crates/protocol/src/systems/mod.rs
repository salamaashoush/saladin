use crate::components::*;
use crate::{GameIndex, MatchStatuses, SimSchedule, SimSet, StateHash, Tick, every};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use saladin_sim::Fnv1a;

mod ai_brain;
mod combat;
mod economy;
mod gather;
mod movement;
mod research;
mod separation;

/// Register every simulation system on `SimSchedule`, fully chained so parallel
/// execution can't reorder mutations between clients. Sub-rate systems gate on
/// the tick counter (base tick = 50 ms): gather/combat @200 ms (every 4),
/// brain/research @1 s (every 20), economy @2 s (every 40).
pub fn register(app: &mut App) {
    app.init_resource::<combat::CombatScratch>();
    app.init_resource::<separation::SepScratch>();
    app.add_systems(
        SimSchedule,
        (
            crate::commands::apply_commands.in_set(SimSet::Index),
            advance_tick.in_set(SimSet::Index),
            // the index is only read by gather (every 4 ticks) — rebuild on its cadence
            maintain_index.in_set(SimSet::Index).run_if(every(4)),
            maintain_match_statuses.in_set(SimSet::Index),
            movement::movement.in_set(SimSet::Movement),
            separation::separation.in_set(SimSet::Movement).run_if(every(2)),
            gather::gather.in_set(SimSet::Gather).run_if(every(4)),
            combat::combat.in_set(SimSet::Combat).run_if(every(4)),
            economy::economy.in_set(SimSet::Economy).run_if(every(40)),
            research::research.in_set(SimSet::Research).run_if(every(20)),
            ai_brain::ai_brain.in_set(SimSet::Brain).run_if(every(20)),
            state_hash.in_set(SimSet::Cleanup),
        )
            .chain(),
    );
}

fn advance_tick(mut tick: ResMut<Tick>) {
    tick.0 += 1;
}

/// Rebuild the `GameId → Entity` index each tick. O(N) but deterministic and
/// simple; replace with incremental maintenance once entity counts demand it.
fn maintain_index(q: Query<(Entity, &GameId)>, mut index: ResMut<GameIndex>) {
    index.0.clear();
    for (e, id) in &q {
        index.0.insert(id.0, e);
    }
}

/// Rebuild the `match_id → status` snapshot so the sub-rate systems can skip
/// entities in Paused/Ended matches without querying `MatchInfo` per row.
fn maintain_match_statuses(q: Query<&MatchInfo>, mut statuses: ResMut<MatchStatuses>) {
    statuses.0.clear();
    for m in &q {
        statuses.0.insert(m.match_id, m.status);
    }
}

/// Fold the whole simulation state into one checksum, compared across the
/// lockstep group to detect desync the instant it happens. Each row hashes to
/// its own FNV-1a digest and the digests COMBINE COMMUTATIVELY (a sum of
/// well-mixed per-row hashes), so no sort or collection is needed — O(N),
/// zero allocation, independent of ECS iteration order by construction.
fn state_hash(
    mut hash: ResMut<StateHash>,
    q: Query<(&GameId, &Pos, Option<&Unit>, Option<&Building>, Option<&ResourceNode>)>,
) {
    let mut acc: u64 = 0;
    for (id, pos, unit, bld, node) in &q {
        let mut f = Fnv1a::default();
        f.write_u64(id.0);
        f.write_v2(pos.pos);
        if let Some(u) = unit {
            f.write_u64(u.hp as u64);
            f.write_v2(u.target);
            f.write_u64(u.has_target as u64);
            f.write_u64(u.gather_state as u64);
        }
        if let Some(b) = bld {
            f.write_u64(b.hp as u64);
        }
        if let Some(n) = node {
            f.write_u64(n.remaining as u64);
        }
        // golden-ratio mix before the commutative sum so weak per-row deltas
        // can't cancel each other out
        acc = acc.wrapping_add(f.0.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    }
    hash.0 = acc;
}
