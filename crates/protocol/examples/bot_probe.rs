//! Why the build ladder stopped. Runs one bot on a real map and prints, every
//! 25 seconds, the numbers `next_build` actually branches on plus what the
//! planner decided — so a stalled ladder shows WHICH rung it is stuck on
//! instead of just an empty column in the cost table.
//!
//! cargo run --release -p saladin-protocol --example bot_probe [difficulty] [secs]

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::*;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let diff = match args.first().map(|s| s.as_str()) {
        Some("easy") => AiDifficulty::Easy,
        Some("normal") => AiDifficulty::Normal,
        _ => AiDifficulty::Hard,
    };
    let secs: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(600);
    let ticks = secs * 20;
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
        difficulty: diff,
        faction: Faction::Ayyubid,
        match_id: 1,
    });
    step(app.world_mut());

    println!("{diff:?} bot, seed {seed}, {secs}s");
    println!(
        "{:>5} {:>5} {:>5} {:>5} {:>5} {:>5} {:>4} {:>4} {:>4}  {:<28} {}",
        "t", "wood", "stone", "food", "gold", "peas", "sold", "pop", "cap", "standing", "sites"
    );
    for t in 0..ticks {
        step(app.world_mut());
        if t % 500 != 499 {
            continue;
        }
        let w = app.world_mut();
        let stock =
            { let mut q = w.query::<&Player>(); q.iter(w).find(|p| p.player_id == 1).map(|p| p.stock).unwrap_or_default() };
        let (mut peas, mut sold, mut pop) = (0, 0, 0);
        let mut states = [0i32; 5];
        let mut jobs = 0;
        {
            let mut q = w.query::<(&Owner, &Unit)>();
            for (o, u) in q.iter(w) {
                if o.0 != 1 {
                    continue;
                }
                pop += 1;
                if u.kind == UnitKind::Peasant {
                    peas += 1;
                    states[u.gather_state as usize] += 1;
                    if u.job_site != 0 {
                        jobs += 1;
                    }
                }
                if unit_def(u.kind).attack > 0 {
                    sold += 1;
                }
            }
        }
        let (mut standing, mut sites, mut cap) = (Vec::new(), Vec::new(), 0);
        {
            let mut q = w.query::<(&Owner, &Building)>();
            for (o, b) in q.iter(w) {
                if o.0 != 1 {
                    continue;
                }
                let name = building_def(b.kind).label;
                if operational(b.state) {
                    cap += building_def(b.kind).pop;
                    standing.push(name);
                } else {
                    sites.push(name);
                }
            }
        }
        standing.sort_unstable();
        sites.sort_unstable();
        println!(
            "{:>5} {:>5} {:>5} {:>5} {:>5} {:>5} {:>4} {:>4} {:>4}  {:<28} {}",
            t / 20,
            stock.wood,
            stock.stone,
            stock.food,
            stock.gold,
            peas,
            sold,
            pop,
            cap,
            standing.join(","),
            sites.join(",")
        );
        let hauled = w.resource_mut::<MatchStats>().of(1).gathered;
        // Living fields, not plots: a farm that carries no crop is scenery, and
        // counting scenery is what let the bot sit at its farm target while its
        // food economy shrank underneath it.
        let (mut fields, mut ripe, mut standing_crop) = (0, 0, 0);
        {
            let mut q = w.query::<(&FieldOf, &ResourceNode, Option<&Crop>)>();
            for (_, n, c) in q.iter(w) {
                fields += 1;
                standing_crop += n.remaining;
                if c.is_some_and(|c| c.ripe) {
                    ripe += 1;
                }
            }
        }
        println!("        peasant states idle={} tores={} harv={} tostk={} constr={}  job_site!=0: {}  hauled={hauled}  fields={fields} ripe={ripe} crop={standing_crop}", states[0], states[1], states[2], states[3], states[4], jobs);
        if std::env::var("PROBE_UNITS").is_ok() {
            let nodes: Vec<(u64, V2, i32)> = {
                let mut q = w.query::<(&GameId, &Pos, &ResourceNode)>();
                q.iter(w).map(|(g, p, n)| (g.0, p.pos, n.remaining)).collect()
            };
            println!("        nodes alive: {}", nodes.len());
            {
                let mut q = w.query::<(&Owner, &Pos, &Building)>();
                let mut bs: Vec<String> = q
                    .iter(w)
                    .filter(|(o, ..)| o.0 == 1)
                    .map(|(_, p, b)| {
                        format!(
                            "{}@({:.1},{:.1}){}",
                            building_def(b.kind).label,
                            p.pos.x.to_num::<f32>(),
                            p.pos.y.to_num::<f32>(),
                            if operational(b.state) { "" } else { "[site]" }
                        )
                    })
                    .collect();
                bs.sort();
                println!("        buildings: {}", bs.join(" "));
            }
            let targets: Vec<u64> = {
                let mut q = w.query::<(&Owner, &Unit)>();
                let mut v: Vec<u64> = q
                    .iter(w)
                    .filter(|(o, u)| o.0 == 1 && u.kind == UnitKind::Peasant)
                    .map(|(_, u)| u.target_node)
                    .collect();
                v.sort_unstable();
                v.dedup();
                v
            };
            for t in targets {
                let mut q = w.query::<(&GameId, &Pos, &ResourceNode)>();
                if let Some((_, p, n)) = q.iter(w).find(|(g, ..)| g.0 == t) {
                    let (tx, ty) = (p.pos.x.to_num::<i32>(), p.pos.y.to_num::<i32>());
                    let ring: Vec<String> = (-2i32..=2)
                        .map(|dy| {
                            (-2i32..=2)
                                .map(|dx| if is_passable(seed, tx + dx, ty + dy) { '.' } else { '#' })
                                .collect()
                        })
                        .collect();
                    println!(
                        "        node {t} {:?} rem={} at({:.1},{:.1}) own_tile_passable={} ring5={:?}",
                        n.res_type,
                        n.remaining,
                        p.pos.x.to_num::<f32>(),
                        p.pos.y.to_num::<f32>(),
                        is_passable(seed, tx, ty),
                        ring
                    );
                }
            }
            let mut q = w.query::<(&GameId, &Owner, &Pos, &Unit)>();
            let mut rows: Vec<String> = q
                .iter(w)
                .filter(|(_, o, _, u)| o.0 == 1 && u.kind == UnitKind::Peasant)
                .map(|(g, _, p, u)| {
                    let n = nodes.iter().find(|(id, ..)| *id == u.target_node);
                    format!(
                        "          u{} {:?} at({:.1},{:.1}) node={} {} tgt={} path={} idx={} carry={}",
                        g.0,
                        u.gather_state,
                        p.pos.x.to_num::<f32>(),
                        p.pos.y.to_num::<f32>(),
                        u.target_node,
                        match n {
                            Some((_, np, rem)) => format!(
                                "@({:.1},{:.1}) rem={} d={:.1}",
                                np.x.to_num::<f32>(),
                                np.y.to_num::<f32>(),
                                rem,
                                dist(p.pos, *np).to_num::<f32>()
                            ),
                            None => "GONE".into(),
                        },
                        u.has_target,
                        u.path.len(),
                        u.path_idx,
                        u.carrying
                    )
                })
                .collect();
            rows.sort();
            for r in rows {
                println!("{r}");
            }
        }
    }
}
