//! Follow ONE stalled gatherer tick by tick and re-derive, outside the sim,
//! exactly what `gather::move_patch` decides for it. Diagnostic scaffolding for
//! the AI-economy stall hunt.
//!
//! cargo run --release -p saladin-protocol --example stall_probe [secs]

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::*;

struct Patch {
    snap: V2,
    path: Vec<V2>,
    stuck: bool,
}

fn move_patch(
    astar: &mut AStar,
    flood: &mut Flood,
    seed: u32,
    occ: &std::collections::HashSet<i32>,
    from: V2,
    to: V2,
    reach: Fx,
) -> Patch {
    let passable = |tx: i32, ty: i32| {
        let k = tile_key(tx, ty);
        is_passable(seed, tx, ty) && !occ.contains(&k)
    };
    let snap = approach_tile(seed, &passable, from, to, 4).unwrap_or_else(|| {
        nearest_reachable_passable_grid(flood, &passable, from, to, reach_budget(dist(from, to)))
            .map(|r| r.at)
            .unwrap_or_else(|| nearest_passable_grid(&passable, to.x, to.y))
    });
    let cost = |tx: i32, ty: i32| move_cost_at(seed, tx, ty);
    let path =
        astar.find_path_costed(&passable, &cost, from.x, from.y, snap.x, snap.y, MAX_EXPANSIONS);
    let stuck = dist(from, to) > reach + Fx::ONE && dist2(snap, to) >= dist2(from, to);
    Patch { snap, path, stuck }
}

fn f(v: Fx) -> f32 {
    v.to_num::<f32>()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let secs: u32 = args.first().and_then(|s| s.parse().ok()).unwrap_or(500);
    let base: u32 = std::env::var("PROBE_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(48514);
    let seed = compose_seed(base, 0);

    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed });
    scatter_world_nodes(app.world_mut(), 1);
    app.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::AddAi {
        player_id: 1,
        host: 1,
        difficulty: AiDifficulty::Hard,
        faction: Faction::Ayyubid,
        match_id: 1,
    });
    step(app.world_mut());

    // how many times each peasant's target_node changes — retarget thrash count
    let mut last_node: std::collections::HashMap<u64, u64> = Default::default();
    let mut retargets: std::collections::HashMap<u64, u32> = Default::default();
    let mut retargets_recent: std::collections::HashMap<u64, u32> = Default::default();

    for t in 0..secs * 20 {
        step(app.world_mut());
        if t % 4 != 0 {
            continue;
        }
        let w = app.world_mut();
        let mut q = w.query::<(&GameId, &Owner, &Unit)>();
        for (g, o, u) in q.iter(w) {
            if o.0 != 1 || u.kind != UnitKind::Peasant {
                continue;
            }
            let prev = last_node.insert(g.0, u.target_node);
            if let Some(p) = prev {
                if p != u.target_node {
                    *retargets.entry(g.0).or_default() += 1;
                    if t >= (secs * 20).saturating_sub(1200) {
                        *retargets_recent.entry(g.0).or_default() += 1;
                    }
                }
            }
        }
    }

    let w = app.world_mut();
    println!("== retargets per peasant over {secs}s (and in the last 60s) ==");
    let mut ids: Vec<u64> = retargets.keys().copied().collect();
    let mut all: Vec<u64> = last_node.keys().copied().collect();
    all.sort_unstable();
    ids.sort_unstable();
    for id in &all {
        println!(
            "  u{id}: {} retargets total, {} in the last 60s",
            retargets.get(id).copied().unwrap_or(0),
            retargets_recent.get(id).copied().unwrap_or(0)
        );
    }

    // node + building snapshot
    let occupants: Vec<Occupant> = {
        let mut q = w.query::<(&Pos, &Building)>();
        q.iter(w).map(|(p, b)| Occupant { kind: b.kind, pos: p.pos }).collect()
    };
    let occ = occupancy_set(&occupants, false);
    let nodes: Vec<(u64, V2, i32, ResourceType)> = {
        let mut q = w.query::<(&GameId, &Pos, &ResourceNode)>();
        q.iter(w).map(|(g, p, n)| (g.0, p.pos, n.remaining, n.res_type)).collect()
    };

    // pick the stalled peasants
    let peas: Vec<(u64, V2, GatherState, u64, bool, V2, usize, usize)> = {
        let mut q = w.query::<(&GameId, &Owner, &Pos, &Unit)>();
        let mut v: Vec<_> = q
            .iter(w)
            .filter(|(_, o, _, u)| o.0 == 1 && u.kind == UnitKind::Peasant)
            .map(|(g, _, p, u)| {
                (g.0, p.pos, u.gather_state, u.target_node, u.has_target, u.target, u.path.len(), u.path_idx)
            })
            .collect();
        v.sort_by_key(|x| x.0);
        v
    };

    let mut astar = AStar::new();
    let mut flood = Flood::new();
    println!("\n== per-peasant move_patch re-derivation at t={secs}s ==");
    for (id, pos, gs, node, has_t, tgt, plen, pidx) in &peas {
        let n = nodes.iter().find(|(nid, ..)| nid == node);
        match n {
            Some((_, npos, rem, rt)) => {
                let reach = harvest_reach(0);
                let p = move_patch(&mut astar, &mut flood, seed, &occ, *pos, *npos, reach);
                let reachable = node_reachable(seed, *pos, *npos);
                println!(
                    "u{id} {gs:?} at({:.2},{:.2}) node={node} {rt:?} rem={rem} @({:.2},{:.2}) d={:.2} reach={:.2}\n\
                     \x20    has_target={has_t} target=({:.2},{:.2}) path_len={plen} idx={pidx}\n\
                     \x20    node_reachable={reachable} snap=({:.2},{:.2}) d(snap,node)={:.2} path={:?} stuck={}",
                    f(pos.x), f(pos.y), f(npos.x), f(npos.y), f(dist(*pos, *npos)), f(reach),
                    f(tgt.x), f(tgt.y),
                    f(p.snap.x), f(p.snap.y), f(dist(p.snap, *npos)),
                    p.path.iter().map(|q| (f(q.x), f(q.y))).collect::<Vec<_>>(),
                    p.stuck,
                );
            }
            None => println!("u{id} {gs:?} at({:.2},{:.2}) node={node} GONE", f(pos.x), f(pos.y)),
        }
    }

    // follow the first stalled one for 40 more AI ticks
    let victim = peas
        .iter()
        .find(|(_, pos, gs, node, ..)| {
            *gs == GatherState::ToResource
                && nodes
                    .iter()
                    .find(|(nid, ..)| nid == node)
                    .is_some_and(|(_, np, ..)| dist(*pos, *np) > harvest_reach(0) + Fx::ONE)
        })
        .map(|x| x.0);
    let Some(victim) = victim else {
        println!("\nno stalled peasant found");
        return;
    };
    println!("\n== following u{victim} for 200 ticks (10 s) ==");
    for t in 0..200 {
        step(app.world_mut());
        let w = app.world_mut();
        let mut q = w.query::<(&GameId, &Pos, &Unit)>();
        if let Some((_, p, u)) = q.iter(w).find(|(g, ..)| g.0 == victim) {
            if t % 4 == 0 || t < 12 {
                println!(
                    "  t+{t:<3} pos=({:.3},{:.3}) {:?} node={} has_target={} target=({:.3},{:.3}) path={} idx={} carry={}",
                    f(p.pos.x), f(p.pos.y), u.gather_state, u.target_node, u.has_target,
                    f(u.target.x), f(u.target.y), u.path.len(), u.path_idx, u.carrying
                );
            }
        }
    }
}
