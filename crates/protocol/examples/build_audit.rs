//! Throwaway felt-experience harness for the building-system audit.
//! Plays a realistic opening and prints a timeline. Not a test — run with
//! `cargo run -p saladin-protocol --example build_audit`.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::*;

fn build_app(seed: u32) -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed });
    app
}

fn cmd(app: &mut App, c: PlayerCommand) {
    app.world_mut().resource_mut::<CommandQueue>().0.push(c);
}

fn stock(app: &mut App, id: u64) -> Stockpile {
    let w = app.world_mut();
    let mut q = w.query::<&Player>();
    q.iter(w).find(|p| p.player_id == id).map(|p| p.stock).unwrap()
}

fn set_stock(app: &mut App, id: u64, s: Stockpile) {
    let w = app.world_mut();
    let mut q = w.query::<&mut Player>();
    if let Some(mut p) = q.iter_mut(w).find(|p| p.player_id == id) {
        p.stock = s;
    }
}

fn keep_pos(app: &mut App, id: u64) -> V2 {
    let w = app.world_mut();
    let mut q = w.query::<(&Owner, &Building, &Pos)>();
    q.iter(w).find(|(o, b, _)| o.0 == id && b.kind == BuildingKind::Keep).map(|(_, _, p)| p.pos).unwrap()
}

fn count(app: &mut App, id: u64, kind: BuildingKind) -> usize {
    let w = app.world_mut();
    let mut q = w.query::<(&Owner, &Building)>();
    q.iter(w).filter(|(o, b)| o.0 == id && b.kind == kind).count()
}

fn units(app: &mut App, id: u64, kind: UnitKind) -> usize {
    let w = app.world_mut();
    let mut q = w.query::<(&Owner, &Unit)>();
    q.iter(w).filter(|(o, u)| o.0 == id && u.kind == kind).count()
}

fn all_buildings(app: &mut App, id: u64) -> Vec<(u64, BuildingKind, i32, V2, V2)> {
    let w = app.world_mut();
    let mut q = w.query::<(&GameId, &Owner, &Building, &Pos)>();
    let mut v: Vec<_> = q
        .iter(w)
        .filter(|(_, o, _, _)| o.0 == id)
        .map(|(g, _, b, p)| (g.0, b.kind, b.hp, p.pos, b.rally))
        .collect();
    v.sort_by_key(|t| t.0);
    v
}

fn secs(t: u64) -> String {
    format!("t{t} ({}s)", t as f64 * 0.05)
}

/// Find a free buildable spot near the keep by scanning outward.
fn free_spot(app: &mut App, seed: u32, id: u64, kind: BuildingKind, from: V2, start_r: i32) -> Option<V2> {
    let own: Vec<V2> = {
        let w = app.world_mut();
        let mut q = w.query::<(&Owner, &Building, &Pos)>();
        q.iter(w).filter(|(o, _, _)| o.0 == id).map(|(_, _, p)| p.pos).collect()
    };
    let occ = {
        let w = app.world_mut();
        let mut q = w.query::<(&Building, &Pos)>();
        let items: Vec<Occupant> = q.iter(w).map(|(b, p)| Occupant { kind: b.kind, pos: p.pos }).collect();
        let mut s = occupancy_set(&items, true);
        let mut nq = w.query::<(&Pos, &ResourceNode)>();
        for (p, _) in nq.iter(w) {
            s.insert(tile_key(p.pos.x.to_num::<i32>(), p.pos.y.to_num::<i32>()));
        }
        s
    };
    let bx = from.x.floor().to_num::<i32>();
    let by = from.y.floor().to_num::<i32>();
    for r in start_r..40 {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs().max(dy.abs()) != r {
                    continue;
                }
                let x = Fx::from_num(bx + dx) + fx!("0.5");
                let y = Fx::from_num(by + dy) + fx!("0.5");
                if check_place(seed, kind, x, y, |tx, ty| occ.contains(&tile_key(tx, ty)), |_, _| true, &own).is_ok() {
                    return Some(V2::new(x, y));
                }
            }
        }
    }
    None
}

fn main() {
    let seed = compose_seed(48514, 0);
    println!("=== SEED {seed} (base 48514, preset 0) ===\n");
    let mut app = build_app(seed);
    scatter_world_nodes(app.world_mut(), 1);
    cmd(&mut app, PlayerCommand::Join { player_id: 1, name: "Human".into(), faction: Faction::Ayyubid, match_id: 1 });
    step(app.world_mut());

    let kp = keep_pos(&mut app, 1);
    println!("keep at {:?}  start stock {:?}", (kp.x.to_num::<f32>(), kp.y.to_num::<f32>()), stock(&mut app, 1));
    println!("start peasants: {}\n", units(&mut app, 1, UnitKind::Peasant));

    // ── PART 1: pure economy timeline, no player input ──────────────────────
    println!("--- PART 1: 5 minutes of pure gathering, zero player input ---");
    let mut milestones: Vec<(&str, ResourceCost, Option<u64>)> = vec![
        ("House      (40w)", building_def(BuildingKind::House).cost, None),
        ("Farm       (45w)", building_def(BuildingKind::Farm).cost, None),
        ("Barracks (70w20s)", building_def(BuildingKind::Barracks).cost, None),
        ("Market   (60w20s)", building_def(BuildingKind::Market).cost, None),
        ("Stable   (80w20s)", building_def(BuildingKind::Stable).cost, None),
        ("Blacksmith(60w40s)", building_def(BuildingKind::Blacksmith).cost, None),
        ("SiegeWs (100w40s)", building_def(BuildingKind::SiegeWorkshop).cost, None),
        ("Watchtwr (80w70s)", building_def(BuildingKind::Watchtower).cost, None),
    ];
    for t in 1..=6000u64 {
        step(app.world_mut());
        let s = stock(&mut app, 1);
        for m in milestones.iter_mut() {
            if m.2.is_none() && s.can_afford(&m.1) {
                m.2 = Some(t);
            }
        }
        if t % 600 == 0 {
            println!("  {:>18}  {:?}", secs(t), s);
        }
    }
    println!("\n  first affordable (no building placed, 5 idle-ish peasants):");
    for (label, _, at) in &milestones {
        match at {
            Some(t) => println!("    {label}  at {}", secs(*t)),
            None => println!("    {label}  NEVER in 5 min"),
        }
    }
    println!();

    // ── PART 2: instant construction ────────────────────────────────────────
    println!("--- PART 2: what construction feels like ---");
    let mut app = build_app(seed);
    scatter_world_nodes(app.world_mut(), 1);
    cmd(&mut app, PlayerCommand::Join { player_id: 1, name: "H".into(), faction: Faction::Ayyubid, match_id: 1 });
    step(app.world_mut());
    let kp = keep_pos(&mut app, 1);
    set_stock(&mut app, 1, Stockpile { wood: 5000, stone: 5000, food: 5000, gold: 5000 });

    let spot = free_spot(&mut app, seed, 1, BuildingKind::House, kp, 3).unwrap();
    let before = stock(&mut app, 1);
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::House, pos: spot, facing: 0, builders: vec![] });
    step(app.world_mut());
    let after = stock(&mut app, 1);
    let bs = all_buildings(&mut app, 1);
    let h = bs.iter().find(|b| b.1 == BuildingKind::House);
    println!(
        "  ONE tick after the Build command: house exists={} hp={}/{}  wood {} -> {}",
        h.is_some(),
        h.map(|b| b.2).unwrap_or(0),
        building_def(BuildingKind::House).max_hp,
        before.wood,
        after.wood
    );

    // spam: 12 buildings in ONE tick
    println!("\n  SPAM TEST: 12 Build commands queued in a single tick");
    let mut spots = Vec::new();
    let mut r = 5;
    for _ in 0..12 {
        if let Some(s) = free_spot(&mut app, seed, 1, BuildingKind::House, kp, r) {
            spots.push(s);
            r += 1;
        }
    }
    // Placing them all in one queue: occupancy is recomputed per command, so
    // they should all land. Distinct spots chosen above.
    let before = stock(&mut app, 1);
    for s in &spots {
        cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::House, pos: *s, facing: 0, builders: vec![] });
    }
    step(app.world_mut());
    println!(
        "    houses now: {} (was 1), wood {} -> {} in ONE 50ms tick",
        count(&mut app, 1, BuildingKind::House),
        before.wood,
        stock(&mut app, 1).wood
    );

    // spam the SAME tile 5x in one tick
    let s2 = free_spot(&mut app, seed, 1, BuildingKind::Barracks, kp, 6).unwrap();
    let before = stock(&mut app, 1);
    for _ in 0..5 {
        cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Barracks, pos: s2, facing: 0, builders: vec![] });
    }
    step(app.world_mut());
    println!(
        "    5x Build on the SAME tile: barracks={} wood spent={}",
        count(&mut app, 1, BuildingKind::Barracks),
        before.wood - stock(&mut app, 1).wood
    );

    // ── PART 3: broke ───────────────────────────────────────────────────────
    println!("\n--- PART 3: building with no wood ---");
    set_stock(&mut app, 1, Stockpile { wood: 0, stone: 0, food: 500, gold: 0 });
    let s3 = free_spot(&mut app, seed, 1, BuildingKind::House, kp, 9).unwrap();
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::House, pos: s3, facing: 0, builders: vec![] });
    let n = count(&mut app, 1, BuildingKind::House);
    step(app.world_mut());
    println!("  broke build: houses {} -> {} (silent no-op, no queue, no event)", n, count(&mut app, 1, BuildingKind::House));

    // ── PART 4: training ────────────────────────────────────────────────────
    println!("\n--- PART 4: training + rally ---");
    set_stock(&mut app, 1, Stockpile { wood: 5000, stone: 5000, food: 5000, gold: 5000 });
    let p0 = units(&mut app, 1, UnitKind::Peasant);
    for _ in 0..10 {
        cmd(&mut app, PlayerCommand::Train { player_id: 1, kind: UnitKind::Peasant });
    }
    step(app.world_mut());
    println!("  10x Train queued in ONE tick: peasants {} -> {} (pop cap gate)", p0, units(&mut app, 1, UnitKind::Peasant));

    let cap: i32 = {
        let w = app.world_mut();
        let mut q = w.query::<(&Owner, &Building)>();
        q.iter(w).filter(|(o, _)| o.0 == 1).map(|(_, b)| building_def(b.kind).pop).sum()
    };
    println!("  pop cap now {cap}, units {}", {
        let w = app.world_mut();
        let mut q = w.query::<(&Owner, &Unit)>();
        q.iter(w).filter(|(o, _)| o.0 == 1).count()
    });

    // rally: move the keep rally far away, train, watch the unit
    let keep_id = all_buildings(&mut app, 1).iter().find(|b| b.1 == BuildingKind::Keep).unwrap().0;
    let rally = V2::new(kp.x + fx!("12"), kp.y + fx!("12"));
    cmd(&mut app, PlayerCommand::SetRally { player_id: 1, building: keep_id, target: rally });
    step(app.world_mut());
    let before_ids: Vec<u64> = {
        let w = app.world_mut();
        let mut q = w.query::<(&GameId, &Owner, &Unit)>();
        q.iter(w).filter(|(_, o, _)| o.0 == 1).map(|(g, _, _)| g.0).collect()
    };
    cmd(&mut app, PlayerCommand::Train { player_id: 1, kind: UnitKind::Peasant });
    step(app.world_mut());
    let fresh: Option<(u64, V2, bool, GatherState)> = {
        let w = app.world_mut();
        let mut q = w.query::<(&GameId, &Owner, &Unit, &Pos)>();
        q.iter(w)
            .filter(|(g, o, _, _)| o.0 == 1 && !before_ids.contains(&g.0))
            .map(|(g, _, u, p)| (g.0, p.pos, u.has_target, u.gather_state))
            .next()
    };
    println!("  fresh unit after SetRally 12 tiles away: {fresh:?}");
    for _ in 0..400 {
        step(app.world_mut());
    }
    if let Some((id, _, _, _)) = fresh {
        let w = app.world_mut();
        let mut q = w.query::<(&GameId, &Unit, &Pos)>();
        if let Some((_, u, p)) = q.iter(w).find(|(g, _, _)| g.0 == id) {
            println!(
                "  20s later it is at {:?} (rally {:?}), state {:?}, has_target {}",
                (p.pos.x.to_num::<f32>(), p.pos.y.to_num::<f32>()),
                (rally.x.to_num::<f32>(), rally.y.to_num::<f32>()),
                u.gather_state,
                u.has_target
            );
        } else {
            println!("  fresh unit is GONE");
        }
    }

    // ── PART 5: damage & repair ─────────────────────────────────────────────
    println!("\n--- PART 5: damage is forever ---");
    let hid = all_buildings(&mut app, 1).iter().find(|b| b.1 == BuildingKind::House).unwrap().0;
    {
        let w = app.world_mut();
        let e = w.resource::<GameIndex>().get(hid).unwrap();
        w.get_mut::<Building>(e).unwrap().hp = 1;
    }
    for _ in 0..2000 {
        step(app.world_mut());
    }
    let hp = {
        let w = app.world_mut();
        let e = w.resource::<GameIndex>().get(hid).unwrap();
        w.get::<Building>(e).unwrap().hp
    };
    println!("  house set to 1 hp, 2000 ticks (100s) later: hp = {hp} / {}", building_def(BuildingKind::House).max_hp);
    let before = stock(&mut app, 1);
    cmd(&mut app, PlayerCommand::Demolish { player_id: 1, building: hid });
    step(app.world_mut());
    println!(
        "  demolishing that 1-hp house refunds {} wood (50% of {} full cost) -- selling a burning house pays the same as a pristine one",
        stock(&mut app, 1).wood - before.wood,
        building_def(BuildingKind::House).cost.wood
    );

    // ── PART 6: prereq dodge ────────────────────────────────────────────────
    println!("\n--- PART 6: prerequisite dodging ---");
    set_stock(&mut app, 1, Stockpile { wood: 5000, stone: 5000, food: 5000, gold: 5000 });
    let sp = free_spot(&mut app, seed, 1, BuildingKind::Stable, kp, 11).unwrap();
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Stable, pos: sp, facing: 0, builders: vec![] });
    step(app.world_mut());
    println!("  stable built (barracks exists): {}", count(&mut app, 1, BuildingKind::Stable) > 0);
    let bid = all_buildings(&mut app, 1).iter().find(|b| b.1 == BuildingKind::Barracks).unwrap().0;
    let before = stock(&mut app, 1);
    cmd(&mut app, PlayerCommand::Demolish { player_id: 1, building: bid });
    step(app.world_mut());
    println!(
        "  demolished the barracks (+{}w +{}s back). barracks left: {}",
        stock(&mut app, 1).wood - before.wood,
        stock(&mut app, 1).stone - before.stone,
        count(&mut app, 1, BuildingKind::Barracks)
    );
    let k0 = units(&mut app, 1, UnitKind::Knight);
    cmd(&mut app, PlayerCommand::Train { player_id: 1, kind: UnitKind::Knight });
    step(app.world_mut());
    println!("  train Knight with a Stable but NO Barracks: knights {} -> {}", k0, units(&mut app, 1, UnitKind::Knight));

    // ── PART 7: walls ───────────────────────────────────────────────────────
    println!("\n--- PART 7: walls ---");
    let mut app = build_app(seed);
    scatter_world_nodes(app.world_mut(), 1);
    cmd(&mut app, PlayerCommand::Join { player_id: 1, name: "H".into(), faction: Faction::Ayyubid, match_id: 1 });
    step(app.world_mut());
    let kp = keep_pos(&mut app, 1);
    set_stock(&mut app, 1, Stockpile { wood: 5000, stone: 5000, food: 5000, gold: 5000 });
    let bx = kp.x.floor().to_num::<i32>();
    let by = kp.y.floor().to_num::<i32>();
    // a DIAGONAL drag - what a player gets dragging corner to corner
    let diag: Vec<(i32, i32)> = (0..10).map(|i| (bx + 6 + i, by + 6 + i)).collect();
    let before = stock(&mut app, 1);
    cmd(&mut app, PlayerCommand::PlaceWall { player_id: 1, tiles: diag.clone(), builders: vec![] });
    step(app.world_mut());
    println!(
        "  diagonal 10-tile wall drag: {} segments placed for {}w {}s -- diagonal walls do NOT seal (units walk the gaps)",
        count(&mut app, 1, BuildingKind::Wall),
        before.wood - stock(&mut app, 1).wood,
        before.stone - stock(&mut app, 1).stone
    );
    // now a straight run and a tower absorbing one
    let line: Vec<(i32, i32)> = (0..8).map(|i| (bx + 6, by - 4 + i)).collect();
    cmd(&mut app, PlayerCommand::PlaceWall { player_id: 1, tiles: line.clone(), builders: vec![] });
    step(app.world_mut());
    let walls = count(&mut app, 1, BuildingKind::Wall);
    let before = stock(&mut app, 1);
    let tpos = V2::new(Fx::from_num(line[3].0) + fx!("0.5"), Fx::from_num(line[3].1) + fx!("0.5"));
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Tower, pos: tpos, facing: 0, builders: vec![] });
    step(app.world_mut());
    println!(
        "  tower dropped onto own wall: walls {} -> {}, towers {}, net wood {:+}, net stone {:+} (tower {}w{}s, wall refunded IN FULL)",
        walls,
        count(&mut app, 1, BuildingKind::Wall),
        count(&mut app, 1, BuildingKind::Tower),
        stock(&mut app, 1).wood - before.wood,
        stock(&mut app, 1).stone - before.stone,
        building_def(BuildingKind::Tower).cost.wood,
        building_def(BuildingKind::Tower).cost.stone,
    );

    // wall drag OUTSIDE the town radius, snaking
    println!("\n  town radius = {} tiles", TOWN_RADIUS.to_num::<f32>());
    let far: Vec<(i32, i32)> = (0..120).map(|i| (bx + 8 + i, by)).collect();
    let before_w = count(&mut app, 1, BuildingKind::Wall);
    cmd(&mut app, PlayerCommand::PlaceWall { player_id: 1, tiles: far, builders: vec![] });
    step(app.world_mut());
    let after_w = count(&mut app, 1, BuildingKind::Wall);
    let fx_far = {
        let w = app.world_mut();
        let mut q = w.query::<(&Owner, &Building, &Pos)>();
        q.iter(w)
            .filter(|(o, b, _)| o.0 == 1 && b.kind == BuildingKind::Wall)
            .map(|(_, _, p)| dist(p.pos, kp))
            .fold(Fx::ZERO, |a, b| a.max(b))
    };
    println!(
        "  120-tile wall drag heading away from the keep: {} segments placed, furthest is {:.1} tiles from the keep",
        after_w - before_w,
        fx_far.to_num::<f32>()
    );

    // ── PART 8: garrison ────────────────────────────────────────────────────
    println!("\n--- PART 8: garrison edges ---");
    let wall_id = all_buildings(&mut app, 1).iter().find(|b| b.1 == BuildingKind::Wall).unwrap().0;
    let punit = {
        let w = app.world_mut();
        let mut q = w.query::<(&GameId, &Owner, &Unit)>();
        q.iter(w).find(|(_, o, _)| o.0 == 1).map(|(g, _, _)| g.0).unwrap()
    };
    cmd(&mut app, PlayerCommand::Garrison { player_id: 1, unit: punit, building: wall_id });
    step(app.world_mut());
    let g = {
        let w = app.world_mut();
        let e = w.resource::<GameIndex>().get(punit).unwrap();
        w.get::<Unit>(e).unwrap().garrisoned_in
    };
    println!("  peasant (attack 0) garrisoned into a Wall from ANY distance: garrisoned_in = {g}");
    println!("    -> a peasant on a parapet contributes nothing (buildings fire only if def.attack>0; Wall attack = {})", building_def(BuildingKind::Wall).attack);
    // kill the wall
    {
        let w = app.world_mut();
        let e = w.resource::<GameIndex>().get(wall_id).unwrap();
        w.get_mut::<Building>(e).unwrap().hp = 1;
    }
    println!("  wall garrison_survives_death = {}", building_def(BuildingKind::Wall).garrison_survives_death);

    // ── PART 9: farm churn ──────────────────────────────────────────────────
    println!("\n--- PART 9: farms ---");
    let mut app = build_app(seed);
    scatter_world_nodes(app.world_mut(), 1);
    cmd(&mut app, PlayerCommand::Join { player_id: 1, name: "H".into(), faction: Faction::Ayyubid, match_id: 1 });
    step(app.world_mut());
    let kp = keep_pos(&mut app, 1);
    set_stock(&mut app, 1, Stockpile { wood: 5000, stone: 5000, food: 5000, gold: 5000 });
    match free_spot(&mut app, seed, 1, BuildingKind::Farm, kp, 4) {
        Some(fp) => {
            cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Farm, pos: fp, facing: 0, builders: vec![] });
            step(app.world_mut());
            let fid = all_buildings(&mut app, 1).iter().find(|b| b.1 == BuildingKind::Farm).map(|b| b.0);
            let field = {
                let w = app.world_mut();
                let mut q = w.query::<(&FieldOf, &ResourceNode)>();
                q.iter(w).next().map(|(f, n)| (f.0, n.remaining, n.cap, n.regen))
            };
            println!("  farm {:?} sown, field {:?} (id, remaining, cap, regen/2s)", fid, field);
            if let Some((_, _, _, regen)) = field {
                println!("    -> field refills {} food per 2s economy tick; {} to fill from 1/3", regen, {
                    let need = FARM_STORE - FARM_STORE / 3;
                    format!("{:.0}s", (need as f64 / regen.max(1) as f64) * 2.0)
                });
            }
            // demolish & rebuild churn
            if let Some(fid) = fid {
                let before = stock(&mut app, 1);
                cmd(&mut app, PlayerCommand::Demolish { player_id: 1, building: fid });
                step(app.world_mut());
                step(app.world_mut());
                step(app.world_mut());
                step(app.world_mut());
                let fields_left = {
                    let w = app.world_mut();
                    let mut q = w.query::<&FieldOf>();
                    q.iter(w).count()
                };
                println!(
                    "  demolish farm: +{}w refund, fields left {}",
                    stock(&mut app, 1).wood - before.wood,
                    fields_left
                );
            }
        }
        None => println!("  NO farmable soil within 40 tiles of the keep on this seed"),
    }

    // ── PART 10: what each building costs in gather-seconds ─────────────────
    println!("\n--- PART 10: cost table (1 peasant carries 8 per 1.2s harvest + walk) ---");
    for &k in BuildingKind::ALL {
        let d = building_def(k);
        if !d.buildable {
            continue;
        }
        println!(
            "  {:<15} {:>3}w {:>3}s {:>3}f {:>3}g  hp{:<5} fp{} pop{:<2} atk{:<3} garr{:<2} req={:?}",
            d.label, d.cost.wood, d.cost.stone, d.cost.food, d.cost.gold, d.max_hp, d.footprint, d.pop, d.attack, d.garrison_cap, d.requires
        );
    }
}
