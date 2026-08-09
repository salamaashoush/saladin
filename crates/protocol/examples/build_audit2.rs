//! Part 2 of the felt-experience audit: exploits and edge cases.

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
fn set_stock(app: &mut App, id: u64, s: Stockpile) {
    let w = app.world_mut();
    let mut q = w.query::<&mut Player>();
    if let Some(mut p) = q.iter_mut(w).find(|p| p.player_id == id) {
        p.stock = s;
    }
}
fn stock(app: &mut App, id: u64) -> Stockpile {
    let w = app.world_mut();
    let mut q = w.query::<&Player>();
    q.iter(w).find(|p| p.player_id == id).map(|p| p.stock).unwrap()
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

fn main() {
    let seed = compose_seed(48514, 0);
    let mut app = build_app(seed);
    scatter_world_nodes(app.world_mut(), 1);
    cmd(&mut app, PlayerCommand::Join { player_id: 1, name: "H".into(), faction: Faction::Ayyubid, match_id: 1 });
    cmd(&mut app, PlayerCommand::Join { player_id: 2, name: "E".into(), faction: Faction::Crusader, match_id: 1 });
    step(app.world_mut());
    let k1 = keep_pos(&mut app, 1);
    let k2 = keep_pos(&mut app, 2);
    println!("P1 keep {:?}  P2 keep {:?}  separation {:.1} tiles",
        (k1.x.to_num::<f32>(), k1.y.to_num::<f32>()),
        (k2.x.to_num::<f32>(), k2.y.to_num::<f32>()),
        dist(k1, k2).to_num::<f32>());
    set_stock(&mut app, 1, Stockpile { wood: 100000, stone: 100000, food: 10000, gold: 10000 });

    // ── EXPLOIT A: wall-crawl the town radius across the map ────────────────
    println!("\n=== A: TOWN RADIUS BYPASS via wall crawl (TOWN_RADIUS = {}) ===", TOWN_RADIUS.to_num::<f32>());
    // Bresenham-ish straight line from our keep toward the enemy keep
    let (x0, y0) = (k1.x.to_num::<i32>(), k1.y.to_num::<i32>());
    let (x1, y1) = (k2.x.to_num::<i32>(), k2.y.to_num::<i32>());
    let steps = (x1 - x0).abs().max((y1 - y0).abs());
    let mut tiles = Vec::new();
    for i in 3..=steps {
        let tx = x0 + (x1 - x0) * i / steps;
        let ty = y0 + (y1 - y0) * i / steps;
        tiles.push((tx, ty));
    }
    println!("  one PlaceWall command with {} tiles from my keep straight at the enemy keep", tiles.len());
    let before = stock(&mut app, 1);
    cmd(&mut app, PlayerCommand::PlaceWall { player_id: 1, tiles, builders: vec![] });
    step(app.world_mut());
    let (n, far) = {
        let w = app.world_mut();
        let mut q = w.query::<(&Owner, &Building, &Pos)>();
        let v: Vec<Fx> = q.iter(w).filter(|(o, b, _)| o.0 == 1 && b.kind == BuildingKind::Wall)
            .map(|(_, _, p)| dist(p.pos, k1)).collect();
        (v.len(), v.iter().fold(Fx::ZERO, |a, b| a.max(*b)))
    };
    println!("  -> {} wall segments in ONE tick, furthest {:.1} tiles from my keep (cost {}w {}s)",
        n, far.to_num::<f32>(), before.wood - stock(&mut app, 1).wood, before.stone - stock(&mut app, 1).stone);
    // now build something aggressive at the far end
    let farthest = {
        let w = app.world_mut();
        let mut q = w.query::<(&Owner, &Building, &Pos)>();
        let mut best = (Fx::ZERO, V2::new(Fx::ZERO, Fx::ZERO));
        for (o, b, p) in q.iter(w) {
            if o.0 == 1 && b.kind == BuildingKind::Wall {
                let d = dist(p.pos, k1);
                if d > best.0 { best = (d, p.pos); }
            }
        }
        best.1
    };
    println!("  farthest wall at {:?}, {:.1} tiles from the ENEMY keep",
        (farthest.x.to_num::<f32>(), farthest.y.to_num::<f32>()), dist(farthest, k2).to_num::<f32>());
    // ring-scan for a legal Watchtower spot beside it
    let own: Vec<V2> = {
        let w = app.world_mut();
        let mut q = w.query::<(&Owner, &Building, &Pos)>();
        q.iter(w).filter(|(o, _, _)| o.0 == 1).map(|(_, _, p)| p.pos).collect()
    };
    let occ = {
        let w = app.world_mut();
        let mut q = w.query::<(&Building, &Pos)>();
        let items: Vec<Occupant> = q.iter(w).map(|(b, p)| Occupant { kind: b.kind, pos: p.pos }).collect();
        let mut s = occupancy_set(&items, true);
        let mut nq = w.query::<(&Pos, &ResourceNode)>();
        for (p, _) in nq.iter(w) { s.insert(tile_key(p.pos.x.to_num::<i32>(), p.pos.y.to_num::<i32>())); }
        s
    };
    let mut found = None;
    'scan: for r in 1..12i32 {
        for dy in -r..=r { for dx in -r..=r {
            if dx.abs().max(dy.abs()) != r { continue; }
            let x = farthest.x.floor() + Fx::from_num(dx) + fx!("0.5");
            let y = farthest.y.floor() + Fx::from_num(dy) + fx!("0.5");
            if check_place(seed, BuildingKind::Barracks, x, y, |tx, ty| occ.contains(&tile_key(tx, ty)), |_, _| true, &own).is_ok() {
                found = Some(V2::new(x, y)); break 'scan;
            }
        }}
    }
    match found {
        Some(p) => {
            cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Barracks, pos: p, facing: 0, builders: vec![] });
            cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Watchtower, pos: V2::new(p.x + fx!("3"), p.y), facing: 0, builders: vec![] });
            step(app.world_mut());
            println!("  -> FORWARD BASE at {:.1} tiles from the enemy keep: barracks={} (needs no prereq at all)",
                dist(p, k2).to_num::<f32>(), count(&mut app, 1, BuildingKind::Barracks));
        }
        None => println!("  no legal spot found near the far wall"),
    }

    // ── EXPLOIT B: build on top of your own (and enemy) units ───────────────
    println!("\n=== B: buildings placed ON TOP of standing units ===");
    let mut app = build_app(seed);
    scatter_world_nodes(app.world_mut(), 1);
    cmd(&mut app, PlayerCommand::Join { player_id: 1, name: "H".into(), faction: Faction::Ayyubid, match_id: 1 });
    step(app.world_mut());
    set_stock(&mut app, 1, Stockpile { wood: 10000, stone: 10000, food: 10000, gold: 10000 });
    let k1 = keep_pos(&mut app, 1);
    // find a legal House spot, park a peasant on it, then build there
    let own: Vec<V2> = {
        let w = app.world_mut();
        let mut q = w.query::<(&Owner, &Building, &Pos)>();
        q.iter(w).filter(|(o, _, _)| o.0 == 1).map(|(_, _, p)| p.pos).collect()
    };
    let occ = {
        let w = app.world_mut();
        let mut q = w.query::<(&Building, &Pos)>();
        let items: Vec<Occupant> = q.iter(w).map(|(b, p)| Occupant { kind: b.kind, pos: p.pos }).collect();
        let mut s = occupancy_set(&items, true);
        let mut nq = w.query::<(&Pos, &ResourceNode)>();
        for (p, _) in nq.iter(w) { s.insert(tile_key(p.pos.x.to_num::<i32>(), p.pos.y.to_num::<i32>())); }
        s
    };
    let mut spot = None;
    'hs: for r in 3..20i32 {
        for dy in -r..=r { for dx in -r..=r {
            if dx.abs().max(dy.abs()) != r { continue; }
            let x = k1.x.floor() + Fx::from_num(dx) + fx!("0.5");
            let y = k1.y.floor() + Fx::from_num(dy) + fx!("0.5");
            if check_place(seed, BuildingKind::House, x, y, |tx, ty| occ.contains(&tile_key(tx, ty)), |_, _| true, &own).is_ok() {
                spot = Some(V2::new(x, y)); break 'hs;
            }
        }}
    }
    let upos = spot.expect("house spot");
    let uid = {
        let w = app.world_mut();
        let mut q = w.query::<(&GameId, &Owner, &Unit, &mut Pos)>();
        let (g, _, _, mut p) = q.iter_mut(w).find(|(_, o, _, _)| o.0 == 1).unwrap();
        let id = g.0;
        p.pos = upos;
        id
    };
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::House, pos: upos, facing: 0, builders: vec![] });
    step(app.world_mut());
    let inside = {
        let w = app.world_mut();
        let mut q = w.query::<(&GameId, &Pos)>();
        q.iter(w).find(|(g, _)| g.0 == uid).map(|(_, p)| p.pos)
    };
    println!("  built a House on the tile a peasant was standing on: house={} peasant now at {:?} (never pushed out)",
        count(&mut app, 1, BuildingKind::House),
        inside.map(|p| (p.x.to_num::<f32>(), p.y.to_num::<f32>())));

    // ── C: gatehouse is passable to EVERYONE ────────────────────────────────
    println!("\n=== C: gatehouse ownership ===");
    println!("  occupancy_set(items, include_passable=false) is built from ALL buildings, not per-owner:");
    println!("  -> an enemy army's A* walks straight through YOUR gatehouse. It is a hole in your wall, not a gate.");

    // ── D: repeated demolish/rebuild ────────────────────────────────────────
    println!("\n=== D: demolish/rebuild churn (the only 'repair') ===");
    step(app.world_mut()); // GameIndex is rebuilt at the top of a tick
    let hid = {
        let w = app.world_mut();
        let mut q = w.query::<(&GameId, &Owner, &Building)>();
        q.iter(w).find(|(_, o, b)| o.0 == 1 && b.kind == BuildingKind::House).map(|(g, _, _)| g.0).unwrap()
    };
    {
        let w = app.world_mut();
        let mut q = w.query::<(&GameId, &mut Building)>();
        if let Some((_, mut b)) = q.iter_mut(w).find(|(g, _)| g.0 == hid) { b.hp = 3; }
    }
    let before = stock(&mut app, 1);
    cmd(&mut app, PlayerCommand::Demolish { player_id: 1, building: hid });
    step(app.world_mut());
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::House, pos: upos, facing: 0, builders: vec![] });
    step(app.world_mut());
    let hp = {
        let w = app.world_mut();
        let mut q = w.query::<(&Owner, &Building)>();
        q.iter(w).find(|(o, b)| o.0 == 1 && b.kind == BuildingKind::House).map(|(_, b)| b.hp).unwrap()
    };
    println!("  3 hp house -> Demolish -> Build on the same tile, 2 ticks (100 ms) total:");
    println!("  -> back to {hp} hp for a NET cost of {} wood. Full repair, instant, no builder, no downtime.",
        before.wood - stock(&mut app, 1).wood);

    // ── E: tech ladder is rentable ──────────────────────────────────────────
    println!("\n=== E: the tech ladder is rentable ===");
    for (parent, child) in [
        (BuildingKind::Barracks, BuildingKind::Stable),
        (BuildingKind::Barracks, BuildingKind::Blacksmith),
        (BuildingKind::Blacksmith, BuildingKind::SiegeWorkshop),
        (BuildingKind::Tower, BuildingKind::Watchtower),
    ] {
        let pd = building_def(parent);
        println!("  {:<14} unlocks {:<14}  rent it for {}w {}s (build {}w {}s, demolish refunds 50%) then raze it",
            pd.label, building_def(child).label,
            (pd.cost.wood + 1) / 2, (pd.cost.stone + 1) / 2, pd.cost.wood, pd.cost.stone);
    }

    // ── F: keep loss ────────────────────────────────────────────────────────
    println!("\n=== F: losing the keep ===");
    println!("  Keep buildable = {} (cannot be rebuilt)", building_def(BuildingKind::Keep).buildable);
    let trainers: Vec<&str> = BuildingKind::ALL.iter()
        .filter(|k| building_def(**k).trains.contains(&UnitKind::Peasant))
        .map(|k| building_def(*k).label).collect();
    println!("  buildings that train Peasants: {trainers:?}");
    let all_drop: Vec<&str> = BuildingKind::ALL.iter()
        .filter(|k| **k == BuildingKind::Keep).map(|k| building_def(*k).label).collect();
    println!("  buildings that accept wood/stone/gold: {all_drop:?}  (food-only: Granary, Fishing Hut, Farm)");

    // ── G: build-bar tab occupancy ──────────────────────────────────────────
    println!("\n=== G: build bar ===");
    for c in BUILD_CATEGORIES.iter() {
        println!("  tab {:<9} {} entries: {:?}", c.label, c.kinds.len(),
            c.kinds.iter().map(|k| building_def(*k).label).collect::<Vec<_>>());
    }

    // ── H: what a Tower is worth ────────────────────────────────────────────
    println!("\n=== H: defence math ===");
    for k in [BuildingKind::Keep, BuildingKind::Tower, BuildingKind::Watchtower] {
        let d = building_def(k);
        let dps = d.attack as f32 / d.attack_rate.to_num::<f32>();
        println!("  {:<12} atk {:>2} rate {:.1}s range {:>2} -> {:.1} dps solo, garrison {} archers (+{} per volley = {:.1} dps)",
            d.label, d.attack, d.attack_rate.to_num::<f32>(), d.range.to_num::<i32>(), dps,
            d.garrison_cap, d.garrison_cap * unit_def(UnitKind::Archer).attack,
            (d.attack + d.garrison_cap * unit_def(UnitKind::Archer).attack) as f32 / d.attack_rate.to_num::<f32>());
    }
    let sp = unit_def(UnitKind::Spearman);
    println!("  a Spearman ({} hp, atk {}) needs {:.0}s of solo Tower fire to die; the tower needs {:.0}s to fall to one spearman",
        sp.max_hp, sp.attack,
        sp.max_hp as f32 / (building_def(BuildingKind::Tower).attack as f32 / building_def(BuildingKind::Tower).attack_rate.to_num::<f32>()),
        building_def(BuildingKind::Tower).max_hp as f32 / (sp.attack as f32 / sp.attack_rate.to_num::<f32>()));

    // ── I: garrison who ─────────────────────────────────────────────────────
    println!("\n=== I: who may garrison ===");
    let can: Vec<&str> = UnitKind::ALL.iter().filter(|u| can_garrison(unit_def(**u))).map(|u| unit_def(*u).label).collect();
    let cant: Vec<&str> = UnitKind::ALL.iter().filter(|u| !can_garrison(unit_def(**u))).map(|u| unit_def(*u).label).collect();
    println!("  can: {can:?}\n  cannot: {cant:?}");
    println!("  -> peasants cannot hide in the Keep. There is no 'ring the town bell' at all.");
}
