use crate::components::*;
use crate::{GameIndex, MatchStatuses, SimSchedule, SimSet, StateHash, Tick, every};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

mod ai_brain;
mod combat;
mod construction;
mod economy;
mod gather;
pub(crate) use gather::{node_reach, workable};
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
            construction::construction.in_set(SimSet::Gather).run_if(every(4)),
            combat::combat.in_set(SimSet::Combat).run_if(every(4)),
            reap_orphan_fields.in_set(SimSet::Combat).run_if(every(4)),
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

/// A farm's standing crop belongs to the farm. Razing one takes its field with
/// it, whether the building was demolished, burned down or wiped with its
/// owner — one reaper covers every route instead of three call sites.
fn reap_orphan_fields(
    mut commands: Commands,
    fields: Query<(Entity, &FieldOf)>,
    buildings: Query<&GameId, With<Building>>,
) {
    if fields.is_empty() {
        return;
    }
    let alive: bevy_platform::collections::HashSet<u64> = buildings.iter().map(|g| g.0).collect();
    for (e, f) in &fields {
        if !alive.contains(&f.0) {
            commands.entity(e).despawn();
        }
    }
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

/// Per-row digest. FNV-1a mixes one BYTE at a time, which was affordable while
/// a unit contributed five fields; at thirty-five fields x 20k units it was
/// 5.6M byte-mixes a tick. This does the same job in one multiply-xorshift per
/// field. The hash VALUE is not stable across builds and never needs to be —
/// the only invariant is that every peer computes the same one.
#[derive(Default)]
struct RowHash(u64);

impl RowHash {
    #[inline]
    fn w(&mut self, v: u64) {
        let mut x = v ^ self.0;
        x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
        x ^= x >> 33;
        x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        self.0 = x ^ (x >> 29);
    }

    #[inline]
    fn fx(&mut self, v: saladin_sim::Fx) {
        self.w(v.to_bits() as u64);
    }

    #[inline]
    fn v2(&mut self, v: saladin_sim::V2) {
        self.fx(v.x);
        self.fx(v.y);
    }
}

/// Fold the whole simulation state into one checksum, compared across the
/// lockstep group to detect desync the instant it happens. Each row hashes to
/// its own digest and the digests COMBINE COMMUTATIVELY (a sum of well-mixed
/// per-row hashes), so no sort or collection is needed — O(N), zero allocation,
/// independent of ECS iteration order by construction.
#[allow(clippy::type_complexity)]
fn state_hash(
    mut hash: ResMut<StateHash>,
    q: Query<(
        &GameId,
        Option<&Pos>,
        Option<&Unit>,
        Option<&Building>,
        Option<&ResourceNode>,
        Option<&Crop>,
        Option<&Player>,
        Option<&Research>,
    )>,
) {
    let mut acc: u64 = 0;
    for (id, pos, unit, bld, node, crop, player, research) in &q {
        let mut f = RowHash::default();
        f.w(id.0);
        if let Some(p) = pos {
            f.v2(p.pos);
        }
        // EVERY field a system writes belongs here. The combat/morale/order
        // layer was invisible until now: a peer could rout, garrison or
        // re-target differently and the hash still matched, so the desync only
        // surfaced ticks later as drifted positions — by then unrecoverable.
        // The narrow fields are packed into words so thirty-odd of them cost a
        // handful of mixes.
        if let Some(u) = unit {
            f.w(u.hp as u32 as u64 | (u.carrying as u32 as u64) << 32);
            f.w(u.gather_state as u64
                | (u.carry_type as u64) << 8
                | (u.stance as u64) << 16
                | (u.has_target as u64) << 24
                | (u.routing as u64) << 32
                | (u.heading as u64) << 40
                | (u.order as u64) << 48
                | (u.engage_slot as u64) << 56);
            f.w(u.charge_cd as u32 as u64 | (u.rally_cd as u32 as u64) << 32);
            f.w(u.attack_cd as u32 as u64);
            f.w(u.job_site);
            f.w(u.target_node);
            f.w(u.attack_target);
            f.w(u.garrisoned_in);
            // `speed` is a column pace while a group order is running, so it is
            // live sim state, not a copy of the unit table
            f.fx(u.speed);
            f.fx(u.harvest_timer);
            f.fx(u.morale);
            f.fx(u.setup_timer);
            f.fx(u.ration);
            f.v2(u.target);
            f.v2(u.home);
            f.v2(u.order_target);
            f.v2(u.anchor);
            // the path itself is sim state (a peer that pathed differently is
            // already desynced); digest it instead of walking every waypoint
            f.w(u.path_idx as u64 | (u.path.len() as u64) << 32);
            if let Some(first) = u.path.first() {
                f.v2(*first);
            }
            if let Some(last) = u.path.last() {
                f.v2(*last);
            }
        }
        if let Some(b) = bld {
            f.w(b.hp as u32 as u64 | (b.builders as u32 as u64) << 32);
            // what an upgrade is becoming, and where its output walks: both are
            // command-driven sim state, so both are desyncs waiting to happen
            f.w(b.kind as u64
                | (b.state as u64) << 8
                | (b.target_kind as u64) << 16
                | (b.queue_len as u64) << 24);
            f.fx(b.cooldown);
            f.fx(b.work);
            f.fx(b.train_work);
            f.v2(b.rally);
            for k in b.queued() {
                f.w(*k as u64);
            }
        }
        if let Some(n) = node {
            f.w(n.remaining as u64);
            // cap and regen VARY per field (the soil sets them at sowing) and
            // were invisible here: two peers could disagree about a farm's
            // yield and pass the desync check until the drift leaked into
            // `remaining` minutes later.
            f.w(n.cap as u32 as u64 | (n.regen as u32 as u64) << 32);
        }
        if let Some(c) = crop {
            f.w(c.ripe as u64 | (c.standing as u32 as u64) << 32);
        }
        // the stockpile, the tech tree and research progress ARE sim state: a
        // desync in any of them was invisible while this query demanded a Pos
        if let Some(p) = player {
            f.w(p.stock.wood as u32 as u64 | (p.stock.stone as u32 as u64) << 32);
            f.w(p.stock.food as u32 as u64 | (p.stock.gold as u32 as u64) << 32);
            f.w(p.tech_mask);
            f.w(p.hunger as u32 as u64 | (p.defeated as u64) << 32);
        }
        if let Some(r) = research {
            f.w(r.tech as u64 | (r.done as u64) << 8);
            f.fx(r.progress);
        }
        // golden-ratio mix before the commutative sum so weak per-row deltas
        // can't cancel each other out
        acc = acc.wrapping_add(f.0.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    }
    hash.0 = acc;
}
