//! Does the bot actually SAIL? Two bots on an ARCHIPELAGO map whose starts are
//! on different islands — the one configuration a land army can never resolve.
//! Prints what each side built, launched, loaded and put ashore, and whether
//! anybody ever stood on the other island.
//!
//! cargo run --release -p saladin-protocol --example naval_war [difficulty] [secs] [seeds]
//! env: NW_SEEDS=1,4,7  NW_PRESET=3  NW_DETAIL=1

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::*;

fn show(seed: u32, p: V2) -> String {
    let (x, y) = (p.x.to_num::<i32>(), p.y.to_num::<i32>());
    format!("({:.3},{:.3}){}", p.x.to_num::<f32>(), p.y.to_num::<f32>(), if is_sailable(seed, x, y) { "~" } else { "#" })
}

#[derive(Default, Clone, Copy)]
struct Side {
    huts: i32,
    harbours: i32,
    skiffs: i32,
    barges: i32,
    aboard: i32,
    soldiers: i32,
    ashore: i32,
    fish: i32,
    keeps: i32,
    boat_on_land: i32,
    adrift: i32,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let diff = match args.first().map(|s| s.as_str()) {
        Some("easy") => AiDifficulty::Easy,
        Some("normal") => AiDifficulty::Normal,
        _ => AiDifficulty::Hard,
    };
    let secs: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1800);
    let preset: u8 =
        std::env::var("NW_PRESET").ok().and_then(|s| s.parse().ok()).unwrap_or(3);
    let seeds: Vec<u32> = match std::env::var("NW_SEEDS") {
        Ok(s) => s.split(',').filter_map(|x| x.trim().parse().ok()).collect(),
        Err(_) => {
            let n: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(8);
            // only the seeds whose first two slots land on DIFFERENT islands:
            // anything else is a land match wearing an archipelago costume
            (1..=200u32)
                .filter(|b| {
                    let seed = compose_seed(*b, preset);
                    let a = start_point(seed, 0);
                    let c = start_point(seed, 1);
                    region_at(seed, a.x, a.y) != region_at(seed, c.x, c.y)
                })
                .take(n as usize)
                .collect()
        }
    };

    let beach_watch = std::env::var("NW_BEACH").is_ok();
    println!("{diff:?} vs {diff:?}, preset {preset}, {secs}s, {} seeds", seeds.len());
    let mut resolved = 0;
    let mut ever_ashore = 0;
    let mut ever_barge = 0;
    let mut ever_harbour = 0;
    let mut ever_skiff = 0;
    for base in &seeds {
        let seed = compose_seed(*base, preset);
        let mut app = App::new();
        app.add_plugins(SimPlugin);
        app.finish();
        app.cleanup();
        app.world_mut().insert_resource(WorldConfig { seed });
        scatter_world_nodes(app.world_mut(), 1);
        for (id, f) in [(1u64, Faction::Ayyubid), (2u64, Faction::Crusader)] {
            app.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::AddAi {
                player_id: id,
                host: 1,
                difficulty: diff,
                faction: f,
                match_id: 1,
            });
        }
        step(app.world_mut());
        let home: [u16; 3] = {
            let w = app.world_mut();
            let mut q = w.query::<(&Owner, &Pos, &Building)>();
            let mut h = [u16::MAX; 3];
            for (o, p, b) in q.iter(w) {
                if b.kind == BuildingKind::Keep && o.0 <= 2 {
                    h[o.0 as usize] = region_at(seed, p.pos.x, p.pos.y);
                }
            }
            h
        };

        if std::env::var("NW_DETAIL").is_ok() {
            println!("   home regions p1={} p2={}", home[1], home[2]);
        }
        let mut peak = [Side::default(); 3];
        let mut done_at = None;
        let mut prev: std::collections::HashMap<u64, V2> = std::collections::HashMap::new();
        for t in 0..secs * 20 {
            step(app.world_mut());
            // A hull on dry ground is a broken invariant, and it is invisible in
            // a sampled count: check it EVERY tick and say where it happened.
            if beach_watch {
                let w = app.world_mut();
                let mut q = w.query::<(&GameId, &Owner, &Unit, &Pos)>();
                let bad: Option<String> = q
                    .iter(w)
                    .find(|(_, _, u, p)| {
                        unit_def(u.kind).domain == Domain::Sea
                            && !is_sailable(
                                seed,
                                p.pos.x.to_num::<i32>(),
                                p.pos.y.to_num::<i32>(),
                            )
                    })
                    .map(|(g, o, u, p)| {
                        format!(
                            "t{} p{} {:?} id{} at {} prev {} tgt {} order{} gs{:?} node{} path[{}] idx{} hastgt{}",
                            t,
                            o.0,
                            u.kind,
                            g.0,
                            show(seed, p.pos),
                            prev.get(&g.0).map(|q| show(seed, *q)).unwrap_or_default(),
                            show(seed, u.target),
                            u.order,
                            u.gather_state,
                            u.target_node,
                            u.path.iter().map(|w| show(seed, *w)).collect::<Vec<_>>().join(" "),
                            u.path_idx,
                            u.has_target,
                        )
                    });
                if let Some(msg) = bad {
                    println!("  BEACHED seed {base}: {msg}");
                    break;
                }
                prev.clear();
                for (g, _, u, p) in q.iter(w) {
                    if unit_def(u.kind).domain == Domain::Sea {
                        prev.insert(g.0, p.pos);
                    }
                }
            }
            if t % 200 != 199 {
                continue;
            }
            let w = app.world_mut();
            let mut cur = [Side::default(); 3];
            {
                let mut q = w.query::<(&Owner, &Unit, &Pos)>();
                for (o, u, p) in q.iter(w) {
                    if o.0 > 2 {
                        continue;
                    }
                    let s = &mut cur[o.0 as usize];
                    let d = unit_def(u.kind);
                    match u.kind {
                        UnitKind::FishingSkiff => s.skiffs += 1,
                        UnitKind::Barge => s.barges += 1,
                        _ => {}
                    }
                    if d.domain == Domain::Sea
                        && !is_sailable(seed, p.pos.x.to_num::<i32>(), p.pos.y.to_num::<i32>())
                    {
                        s.boat_on_land += 1;
                    }
                    if u.garrisoned_in != 0 && d.domain == Domain::Land && d.attack > 0 {
                        s.aboard += 1;
                    }
                    if d.attack > 0 && u.garrisoned_in == 0 {
                        s.soldiers += 1;
                        let r = region_at(seed, p.pos.x, p.pos.y);
                        // A man on a tile with NO region is a man on ground he
                        // cannot walk on. Before the sea existed that was a
                        // cliff and nobody looked; now it is a soldier standing
                        // in the ocean off his own beach, which is why this is
                        // counted separately from a real landing.
                        if r == u16::MAX {
                            s.adrift += 1;
                            if std::env::var("NW_DETAIL").is_ok() {
                                println!(
                                    "      ADRIFT p{} {:?} at {} order{} hastgt{} tgt {} path{} idx{}",
                                    o.0, u.kind, show(seed, p.pos), u.order, u.has_target,
                                    show(seed, u.target), u.path.len(), u.path_idx
                                );
                            }
                        } else if r != home[o.0 as usize] {
                            s.ashore += 1;
                        }
                    }
                    if u.kind == UnitKind::FishingSkiff && u.gather_state != GatherState::Idle {
                        s.fish += 1;
                    }
                }
            }
            {
                let mut q = w.query::<(&Owner, &Building)>();
                for (o, b) in q.iter(w) {
                    if o.0 > 2 || !operational(b.state) {
                        continue;
                    }
                    let s = &mut cur[o.0 as usize];
                    match b.kind {
                        BuildingKind::FishingHut => s.huts += 1,
                        BuildingKind::Harbour => s.harbours += 1,
                        BuildingKind::Keep => s.keeps += 1,
                        _ => {}
                    }
                }
            }
            for i in 1..3 {
                let (p, c) = (&mut peak[i], cur[i]);
                p.huts = p.huts.max(c.huts);
                p.harbours = p.harbours.max(c.harbours);
                p.skiffs = p.skiffs.max(c.skiffs);
                p.barges = p.barges.max(c.barges);
                p.aboard = p.aboard.max(c.aboard);
                p.soldiers = p.soldiers.max(c.soldiers);
                p.ashore = p.ashore.max(c.ashore);
                p.fish = p.fish.max(c.fish);
                p.boat_on_land = p.boat_on_land.max(c.boat_on_land);
                p.adrift = p.adrift.max(c.adrift);
            }
            if std::env::var("NW_DETAIL").is_ok() && t % 2000 == 199 {
                let w = app.world_mut();
                let mut sq = w.query::<&Player>();
                let st: Vec<String> = (1..=2u64)
                    .map(|id| {
                        sq.iter(w)
                            .find(|p| p.player_id == id)
                            .map(|p| format!("w{} s{} f{} g{}", p.stock.wood, p.stock.stone, p.stock.food, p.stock.gold))
                            .unwrap_or_default()
                    })
                    .collect();
                let mut bq = w.query::<(&Owner, &Building)>();
                let mut names: [Vec<&str>; 3] = Default::default();
                let mut popcap = [0i32; 3];
                for (o, b) in bq.iter(w) {
                    if o.0 <= 2 {
                        names[o.0 as usize].push(building_def(b.kind).label);
                        if operational(b.state) {
                            popcap[o.0 as usize] += building_def(b.kind).pop;
                        }
                    }
                }
                for i in 1..3 {
                    names[i].sort_unstable();
                }
                println!("      p1 [{}] cap{} | p2 [{}] cap{}", st[0], popcap[1], st[1], popcap[2]);
                println!("      p1 blds {:?}", names[1]);
                println!("      p2 blds {:?}", names[2]);
                println!(
                    "   t{:<5} p1 hut{} hbr{} skf{} brg{} abd{} sol{} ash{} | p2 hut{} hbr{} skf{} brg{} abd{} sol{} ash{}",
                    t / 20,
                    cur[1].huts, cur[1].harbours, cur[1].skiffs, cur[1].barges,
                    cur[1].aboard, cur[1].soldiers, cur[1].ashore,
                    cur[2].huts, cur[2].harbours, cur[2].skiffs, cur[2].barges,
                    cur[2].aboard, cur[2].soldiers, cur[2].ashore,
                );
            }
            if (cur[1].keeps == 0 || cur[2].keeps == 0) && done_at.is_none() {
                done_at = Some(t / 20);
                break;
            }
        }
        if done_at.is_some() {
            resolved += 1;
        }
        if peak[1].ashore + peak[2].ashore > 0 {
            ever_ashore += 1;
        }
        if peak[1].barges + peak[2].barges > 0 {
            ever_barge += 1;
        }
        if peak[1].harbours + peak[2].harbours > 0 {
            ever_harbour += 1;
        }
        if peak[1].skiffs + peak[2].skiffs > 0 {
            ever_skiff += 1;
        }
        println!(
            "seed {base:<4} {:>7} | p1 hut{} hbr{} skf{} brg{} fish{} abd{} ash{} beached{} adrift{} | p2 hut{} hbr{} skf{} brg{} fish{} abd{} ash{} beached{} adrift{}",
            done_at.map(|s| format!("won@{s}")).unwrap_or_else(|| "-".into()),
            peak[1].huts, peak[1].harbours, peak[1].skiffs, peak[1].barges,
            peak[1].fish, peak[1].aboard, peak[1].ashore, peak[1].boat_on_land, peak[1].adrift,
            peak[2].huts, peak[2].harbours, peak[2].skiffs, peak[2].barges,
            peak[2].fish, peak[2].aboard, peak[2].ashore, peak[2].boat_on_land, peak[2].adrift,
        );
    }
    let n = seeds.len();
    println!(
        "resolved {resolved}/{n}  landings {ever_ashore}/{n}  barges {ever_barge}/{n}  harbours {ever_harbour}/{n}  skiffs {ever_skiff}/{n}"
    );
}
