//! Two bots, one map, one faction each. The end-to-end question the whole war
//! overhaul rests on: do AI armies ever actually MEET and DECIDE anything?
//!
//! cargo run --release -p saladin-protocol --example war_probe [difficulty] [secs]
//! env: WAR_SEED, WAR_SWAP=1 (swap which side is which faction)

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
    let secs: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(900);
    let ticks = secs * 20;
    let base: u32 = std::env::var("WAR_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(48514);
    let swap = std::env::var("WAR_SWAP").is_ok();
    let seed = compose_seed(base, 0);

    let (f1, f2) = if swap {
        (Faction::Crusader, Faction::Ayyubid)
    } else {
        (Faction::Ayyubid, Faction::Crusader)
    };

    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed });
    scatter_world_nodes(app.world_mut(), 1);
    for (id, f) in [(1u64, f1), (2u64, f2)] {
        app.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::AddAi {
            player_id: id,
            host: 1,
            difficulty: diff,
            faction: f,
            match_id: 1,
        });
    }
    step(app.world_mut());

    println!("seed {base} preset 0, {diff:?} bots, p1={f1:?} p2={f2:?}, {secs}s");
    println!(
        "{:>5} | {:>4} {:>4} {:>5} {:>5} {:>4} | {:>4} {:>4} {:>5} {:>5} {:>4} | {:>6} {:>6}",
        "t", "p1s", "p1b", "p1trn", "p1los", "p1fd", "p2s", "p2b", "p2trn", "p2los", "p2fd",
        "contact", "roster"
    );

    let mut ever_contact = 0i32;
    for t in 0..ticks {
        step(app.world_mut());
        if t % 1200 != 1199 {
            continue;
        }
        let w = app.world_mut();
        let mut sold = [0i32; 3];
        let mut blds = [0i32; 3];
        let mut kinds: [std::collections::BTreeSet<&'static str>; 3] = Default::default();
        let mut positions: [Vec<V2>; 3] = Default::default();
        {
            let mut q = w.query::<(&Owner, &Unit, &Pos)>();
            for (o, u, p) in q.iter(w) {
                if o.0 > 2 {
                    continue;
                }
                if unit_def(u.kind).attack > 0 {
                    sold[o.0 as usize] += 1;
                    kinds[o.0 as usize].insert(unit_def(u.kind).label);
                    positions[o.0 as usize].push(p.pos);
                }
            }
        }
        {
            let mut q = w.query::<(&Owner, &Building)>();
            for (o, b) in q.iter(w) {
                if o.0 <= 2 && operational(b.state) {
                    blds[o.0 as usize] += 1;
                }
            }
        }
        // closest pair of enemy soldiers, in tiles
        let mut closest = i64::MAX;
        for a in &positions[1] {
            for b in &positions[2] {
                closest = closest.min(dist(*a, *b).to_num::<i64>());
            }
        }
        if closest <= 10 {
            ever_contact += 1;
        }
        let (k1, l1, k2, l2) = {
            let mut stats = w.resource_mut::<MatchStats>();
            let (k1, l1) = (stats.of(1).trained, stats.of(1).lost);
            let (k2, l2) = (stats.of(2).trained, stats.of(2).lost);
            (k1, l1, k2, l2)
        };
        let (mut f1s, mut f2s) = (0, 0);
        {
            let mut q = w.query::<&Player>();
            for p in q.iter(w) {
                if p.player_id == 1 {
                    f1s = p.stock.food;
                }
                if p.player_id == 2 {
                    f2s = p.stock.food;
                }
            }
        }
        if std::env::var("WAR_DETAIL").is_ok() {
            for owner in [1u64, 2u64] {
                let mut states = [0i32; 5];
                let (mut peas, mut jobs) = (0, 0);
                let mut keep = V2::ZERO;
                {
                    let mut q = w.query::<(&Owner, &Unit, &Pos)>();
                    for (o, u, _) in q.iter(w) {
                        if o.0 == owner && u.kind == UnitKind::Peasant {
                            peas += 1;
                            states[u.gather_state as usize] += 1;
                            if u.job_site != 0 {
                                jobs += 1;
                            }
                        }
                    }
                }
                let mut names: Vec<&str> = Vec::new();
                {
                    let mut q = w.query::<(&Owner, &Building, &Pos)>();
                    for (o, b, p) in q.iter(w) {
                        if o.0 == owner {
                            names.push(building_def(b.kind).label);
                            if b.kind == BuildingKind::Keep {
                                keep = p.pos;
                            }
                        }
                    }
                }
                names.sort_unstable();
                let stock = {
                    let mut q = w.query::<&Player>();
                    q.iter(w).find(|p| p.player_id == owner).map(|p| p.stock).unwrap_or_default()
                };
                let hauled = w.resource_mut::<MatchStats>().of(owner).gathered;
                println!(
                    "     p{owner}: keep({},{}) w{} s{} f{} g{} peas={peas} idle={} tores={} harv={} tostk={} constr={} jobs={jobs} hauled={hauled} [{}]",
                    keep.x.to_num::<i32>(), keep.y.to_num::<i32>(),
                    stock.wood, stock.stone, stock.food, stock.gold,
                    states[0], states[1], states[2], states[3], states[4],
                    names.join(",")
                );
            }
        }
        println!(
            "{:>5} | {:>4} {:>4} {:>5} {:>5} {:>4} | {:>4} {:>4} {:>5} {:>5} {:>4} | {:>6} {} / {}",
            t / 20,
            sold[1],
            blds[1],
            k1,
            l1,
            f1s,
            sold[2],
            blds[2],
            k2,
            l2,
            f2s,
            if closest == i64::MAX { -1 } else { closest },
            kinds[1].iter().copied().collect::<Vec<_>>().join(","),
            kinds[2].iter().copied().collect::<Vec<_>>().join(","),
        );
    }
    println!("samples with armies within 10 tiles: {ever_contact}");
}
