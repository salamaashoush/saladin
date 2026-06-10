//! Save/load as a deterministic ECS snapshot (replaces the SpacetimeDB
//! mirror-table save system). The snapshot captures every sim row plus the
//! lockstep-critical resources (tick, id counter, rng, world seed); restoring
//! rebuilds the world bit-identically, so a loaded single-player match
//! continues exactly where it left off.

use crate::components::*;
use crate::{NextEntityId, SimRng, Tick, WorldConfig};
use bevy_ecs::prelude::*;
use saladin_sim::Rng;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct SaveRow {
    id: u64,
    match_id: u64,
    owner: Option<u64>,
    pos: Option<Pos>,
    unit: Option<Unit>,
    building: Option<Building>,
    node: Option<ResourceNode>,
    player: Option<Player>,
    bot: Option<Bot>,
    research: Option<Research>,
}

#[derive(Serialize, Deserialize)]
pub struct SaveGame {
    pub seed: u32,
    pub tick: u64,
    pub next_id: u64,
    rng: Rng,
    matches: Vec<MatchInfo>,
    rows: Vec<SaveRow>,
}

/// Capture the whole sim state. Render/UI state is derived, never saved.
pub fn snapshot(world: &mut World) -> SaveGame {
    let mut rows = Vec::new();
    {
        let mut q = world.query::<(
            &GameId,
            &MatchId,
            Option<&Owner>,
            Option<&Pos>,
            Option<&Unit>,
            Option<&Building>,
            Option<&ResourceNode>,
            Option<&Player>,
            Option<&Bot>,
            Option<&Research>,
        )>();
        for (g, m, owner, pos, unit, building, node, player, bot, research) in q.iter(world) {
            rows.push(SaveRow {
                id: g.0,
                match_id: m.0,
                owner: owner.map(|o| o.0),
                pos: pos.copied(),
                unit: unit.cloned(),
                building: building.copied(),
                node: node.copied(),
                player: player.cloned(),
                bot: bot.copied(),
                research: research.copied(),
            });
        }
    }
    rows.sort_by_key(|r| r.id);
    let matches: Vec<MatchInfo> = {
        let mut q = world.query::<&MatchInfo>();
        q.iter(world).cloned().collect()
    };
    SaveGame {
        seed: world.resource::<WorldConfig>().seed,
        tick: world.resource::<Tick>().0,
        next_id: world.resource::<NextEntityId>().0,
        rng: world.resource::<SimRng>().0,
        matches,
        rows,
    }
}

/// Replace the sim state with a snapshot. The caller is responsible for being
/// between lockstep ticks (single-player load at match start).
pub fn restore(world: &mut World, save: SaveGame) {
    // tear down whatever sim rows exist
    let old: Vec<Entity> = {
        let mut q = world.query_filtered::<Entity, bevy_ecs::query::Or<(With<GameId>, With<MatchInfo>)>>();
        q.iter(world).collect()
    };
    for e in old {
        world.despawn(e);
    }

    world.resource_mut::<WorldConfig>().seed = save.seed;
    world.resource_mut::<Tick>().0 = save.tick;
    world.resource_mut::<NextEntityId>().0 = save.next_id;
    world.resource_mut::<SimRng>().0 = save.rng;

    for m in save.matches {
        world.spawn(m);
    }
    for r in save.rows {
        let mut e = world.spawn((GameId(r.id), MatchId(r.match_id)));
        if let Some(o) = r.owner {
            e.insert(Owner(o));
        }
        if let Some(p) = r.pos {
            e.insert(p);
        }
        if let Some(u) = r.unit {
            e.insert(u);
        }
        if let Some(b) = r.building {
            e.insert(b);
        }
        if let Some(n) = r.node {
            e.insert(n);
        }
        if let Some(p) = r.player {
            e.insert(p);
        }
        if let Some(b) = r.bot {
            e.insert(b);
        }
        if let Some(rr) = r.research {
            e.insert(rr);
        }
    }
}

pub fn to_bytes(save: &SaveGame) -> Vec<u8> {
    bincode::serialize(save).expect("savegame serializes")
}

pub fn from_bytes(bytes: &[u8]) -> Option<SaveGame> {
    bincode::deserialize(bytes).ok()
}
