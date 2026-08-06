//! Army-control audit: what a 30-unit move order actually does.
//!
//! Measures, on a real map, (1) arrival spread and arrival-time skew for a
//! massed move with the client's formation offsets, (2) the same order with no
//! offsets, (3) 30 units squeezing through a 2-tile gap in a wall, and (4)
//! whether the separation pass can shove a unit inside a standing structure.
//!
//! cargo run --release -p saladin-protocol --example army_audit

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::*;

fn app_for(seed: u32) -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed });
    app.world_mut().spawn((
        GameId(900),
        MatchId(1),
        Player {
            player_id: 1,
            name: "P".into(),
            faction: Faction::Ayyubid,
            stock: Stockpile { wood: 9999, stone: 9999, food: 9999, gold: 9999 },
            color: 0,
            online: true,
            keep: 0,
            defeated: false,
            slot: 0,
            tech_mask: 0,
            hunger: 0,
        },
    ));
    app
}

fn spawn_unit(app: &mut App, id: u64, kind: UnitKind, pos: V2) {
    let def = unit_def(kind);
    app.world_mut().spawn((
        GameId(id),
        Owner(1),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Unit {
            speed: def.speed,
            hp: def.max_hp,
            ..Unit::new(kind, pos)
        },
    ));
}

fn spawn_wall(app: &mut App, id: u64, pos: V2) {
    let def = building_def(BuildingKind::Wall);
    app.world_mut().spawn((
        GameId(id),
        Owner(2),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Building::new(BuildingKind::Wall, def.max_hp, pos),
    ));
}

/// Open rectangle of passable land, w x h tiles, scanned deterministically.
fn open_block(seed: u32, w: i32, h: i32) -> (i32, i32) {
    for cy in 8..(WORLD_SIZE - h - 8) {
        for cx in 8..(WORLD_SIZE - w - 8) {
            if (0..w).all(|dx| (0..h).all(|dy| is_passable(seed, cx + dx, cy + dy))) {
                return (cx, cy);
            }
        }
    }
    panic!("no {w}x{h} open block");
}

/// Port of the client's `formation(n)` (crates/client/src/input.rs) — the ONLY
/// spreading a massed move order gets today.
fn formation(n: usize) -> Vec<(f32, f32)> {
    let cols = (n as f32).sqrt().ceil().max(1.0) as usize;
    let rows = n.div_ceil(cols);
    let s = 0.85_f32;
    (0..n)
        .map(|i| {
            let c = (i % cols) as f32;
            let r = (i / cols) as f32;
            ((c - (cols as f32 - 1.0) / 2.0) * s, (r - (rows as f32 - 1.0) / 2.0) * s)
        })
        .collect()
}

struct Row {
    id: u64,
    pos: V2,
    has_target: bool,
}

fn snapshot(app: &mut App) -> Vec<Row> {
    let w = app.world_mut();
    let mut q = w.query::<(&GameId, &Pos, &Unit)>();
    let mut v: Vec<Row> =
        q.iter(w).map(|(g, p, u)| Row { id: g.0, pos: p.pos, has_target: u.has_target }).collect();
    v.sort_by_key(|r| r.id);
    v
}

fn f(v: Fx) -> f64 {
    v.to_num::<f64>()
}

fn stats(rows: &[Row], goal: (f64, f64)) -> (f64, f64, f64, f64) {
    // centroid, mean/max distance from the goal, max pairwise spread
    let n = rows.len() as f64;
    let (mut sx, mut sy) = (0.0, 0.0);
    for r in rows {
        sx += f(r.pos.x);
        sy += f(r.pos.y);
    }
    let (cx, cy) = (sx / n, sy / n);
    let mut sum = 0.0;
    let mut max_goal: f64 = 0.0;
    for r in rows {
        let d = ((f(r.pos.x) - goal.0).powi(2) + (f(r.pos.y) - goal.1).powi(2)).sqrt();
        sum += d;
        max_goal = max_goal.max(d);
    }
    let mut max_pair: f64 = 0.0;
    for i in 0..rows.len() {
        for j in (i + 1)..rows.len() {
            let d = ((f(rows[i].pos.x) - f(rows[j].pos.x)).powi(2)
                + (f(rows[i].pos.y) - f(rows[j].pos.y)).powi(2))
            .sqrt();
            max_pair = max_pair.max(d);
        }
    }
    let _ = (cx, cy);
    (sum / n, max_goal, max_pair, n)
}

/// One massed move; returns (first arrival tick, last arrival tick, still moving).
fn run_move(label: &str, seed: u32, n: usize, spread: bool, kind: UnitKind, dist: i32) {
    let (bx, by) = open_block(seed, dist + 14, 14);
    let mut app = app_for(seed);
    // staging block: 6 wide, sqrt-ish deep, one unit per tile
    for i in 0..n {
        let x = Fx::from_num(bx + 2 + (i % 6) as i32) + fx!("0.5");
        let y = Fx::from_num(by + 4 + (i / 6) as i32) + fx!("0.5");
        spawn_unit(&mut app, 1 + i as u64, kind, V2::new(x, y));
    }
    step(app.world_mut());
    let gx = (bx + dist) as f32 + 0.5;
    let gy = (by + 7) as f32 + 0.5;
    let offs = formation(n);
    for i in 0..n {
        let (ox, oy) = if spread { offs[i] } else { (0.0, 0.0) };
        app.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::Move {
            player_id: 1,
            unit: 1 + i as u64,
            target: V2::new(Fx::from_num(gx + ox), Fx::from_num(gy + oy)),
        });
    }
    let mut arrived: Vec<Option<u32>> = vec![None; n];
    let mut t = 0u32;
    while t < 2000 {
        step(app.world_mut());
        t += 1;
        let rows = snapshot(&mut app);
        for (i, r) in rows.iter().enumerate() {
            if arrived[i].is_none() && !r.has_target {
                arrived[i] = Some(t);
            }
        }
        if arrived.iter().all(|a| a.is_some()) {
            break;
        }
    }
    let rows = snapshot(&mut app);
    let (mean_d, max_d, max_pair, _) = stats(&rows, (gx as f64, gy as f64));
    let done: Vec<u32> = arrived.iter().filter_map(|a| *a).collect();
    let first = done.iter().copied().min().unwrap_or(0);
    let last = done.iter().copied().max().unwrap_or(0);
    println!(
        "{label:<34} n={n} arrive first/last tick {first}/{last} (skew {} ticks = {:.1}s)  \
         stuck={}  mean dist to click {mean_d:.2}  worst {max_d:.2}  widest pair {max_pair:.2}",
        last - first,
        (last - first) as f64 / 20.0,
        n - done.len(),
    );
    // how many pairs are still interpenetrating at rest
    let r2 = f(unit_def(kind).radius) * 2.0;
    let mut overlap = 0;
    for i in 0..rows.len() {
        for j in (i + 1)..rows.len() {
            let d = ((f(rows[i].pos.x) - f(rows[j].pos.x)).powi(2)
                + (f(rows[i].pos.y) - f(rows[j].pos.y)).powi(2))
            .sqrt();
            if d < r2 {
                overlap += 1;
            }
        }
    }
    println!("{:<34} overlapping pairs at rest: {overlap} (min sep {r2:.2})", "");
}

/// 30 units through a 2-tile gap in a 24-tile wall.
fn run_gap(seed: u32, n: usize, gap: i32) {
    let (bx, by) = open_block(seed, 44, 30);
    let mut app = app_for(seed);
    let wall_x = bx + 20;
    let gy0 = by + 14;
    let mut wid = 5000;
    for y in (by + 2)..(by + 28) {
        if (gy0..gy0 + gap).contains(&y) {
            continue;
        }
        spawn_wall(&mut app, wid, V2::new(Fx::from_num(wall_x) + fx!("0.5"), Fx::from_num(y) + fx!("0.5")));
        wid += 1;
    }
    for i in 0..n {
        let x = Fx::from_num(bx + 2 + (i % 6) as i32) + fx!("0.5");
        let y = Fx::from_num(by + 12 + (i / 6) as i32) + fx!("0.5");
        spawn_unit(&mut app, 1 + i as u64, UnitKind::Spearman, V2::new(x, y));
    }
    step(app.world_mut());
    let gx = (bx + 40) as f32 + 0.5;
    let gy = (by + 15) as f32 + 0.5;
    let offs = formation(n);
    for i in 0..n {
        let (ox, oy) = offs[i];
        app.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::Move {
            player_id: 1,
            unit: 1 + i as u64,
            target: V2::new(Fx::from_num(gx + ox), Fx::from_num(gy + oy)),
        });
    }
    let mut arrived: Vec<Option<u32>> = vec![None; n];
    let mut t = 0u32;
    let mut inside_wall = 0;
    while t < 3000 {
        step(app.world_mut());
        t += 1;
        let rows = snapshot(&mut app);
        for (i, r) in rows.iter().enumerate() {
            if arrived[i].is_none() && !r.has_target {
                arrived[i] = Some(t);
            }
            // standing on a wall tile => the pass shoved someone into a structure
            let tx = r.pos.x.to_num::<i32>();
            let ty = r.pos.y.to_num::<i32>();
            if tx == wall_x && !(gy0..gy0 + gap).contains(&ty) {
                inside_wall += 1;
            }
        }
        if arrived.iter().all(|a| a.is_some()) {
            break;
        }
    }
    let done: Vec<u32> = arrived.iter().filter_map(|a| *a).collect();
    let first = done.iter().copied().min().unwrap_or(0);
    let last = done.iter().copied().max().unwrap_or(0);
    let rows = snapshot(&mut app);
    let mut past = 0;
    for r in &rows {
        if f(r.pos.x) > wall_x as f64 + 1.0 {
            past += 1;
        }
    }
    println!(
        "{n} units through a {gap}-tile gap: first/last {first}/{last} (skew {:.1}s), stuck {}, \
         through the gap {past}/{n}, unit-ticks standing ON a wall tile {inside_wall}",
        (last - first) as f64 / 20.0,
        n - done.len(),
    );
}

/// Does the separation pass respect building footprints? Ring 8 units around a
/// keep and let them push each other for 200 ticks.
fn run_sep_into_building(seed: u32) {
    let (bx, by) = open_block(seed, 20, 20);
    let mut app = app_for(seed);
    let kx = Fx::from_num(bx + 10) + fx!("0.5");
    let ky = Fx::from_num(by + 10) + fx!("0.5");
    let def = building_def(BuildingKind::Keep);
    app.world_mut().spawn((
        GameId(7000),
        Owner(1),
        MatchId(1),
        Pos { pos: V2::new(kx, ky), facing: ZERO },
        Building::new(BuildingKind::Keep, def.max_hp, V2::new(kx, ky)),
    ));
    // 24 peasants packed onto one tile just outside the keep's footprint
    let half = Fx::from_num(def.footprint) / Fx::from_num(2);
    for i in 0..24 {
        spawn_unit(&mut app, 1 + i, UnitKind::Peasant, V2::new(kx + half + fx!("0.3"), ky));
    }
    for _ in 0..300 {
        step(app.world_mut());
    }
    let rows = snapshot(&mut app);
    let mut inside = 0;
    for r in &rows {
        if (f(r.pos.x) - f(kx)).abs() < f(half) && (f(r.pos.y) - f(ky)).abs() < f(half) {
            inside += 1;
        }
    }
    println!(
        "separation vs a {}-tile Keep footprint: {inside}/24 units ended up INSIDE the building",
        def.footprint
    );
}

/// A real combined-arms column: knights, spearmen and a siege train, one click.
fn run_mixed(seed: u32) {
    let (bx, by) = open_block(seed, 60, 16);
    let mut app = app_for(seed);
    let mut kinds = Vec::new();
    for _ in 0..10 {
        kinds.push(UnitKind::Knight);
    }
    for _ in 0..14 {
        kinds.push(UnitKind::Spearman);
    }
    for _ in 0..3 {
        kinds.push(UnitKind::Ram);
    }
    for _ in 0..3 {
        kinds.push(UnitKind::Mangonel);
    }
    let n = kinds.len();
    for (i, k) in kinds.iter().enumerate() {
        let x = Fx::from_num(bx + 2 + (i % 6) as i32) + fx!("0.5");
        let y = Fx::from_num(by + 4 + (i / 6) as i32) + fx!("0.5");
        spawn_unit(&mut app, 1 + i as u64, *k, V2::new(x, y));
    }
    step(app.world_mut());
    let gx = (bx + 40) as f32 + 0.5;
    let gy = (by + 7) as f32 + 0.5;
    let offs = formation(n);
    for i in 0..n {
        app.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::Move {
            player_id: 1,
            unit: 1 + i as u64,
            target: V2::new(Fx::from_num(gx + offs[i].0), Fx::from_num(gy + offs[i].1)),
        });
    }
    let mut arrived: Vec<Option<u32>> = vec![None; n];
    let mut t = 0u32;
    while t < 2000 {
        step(app.world_mut());
        t += 1;
        let rows = snapshot(&mut app);
        for (i, r) in rows.iter().enumerate() {
            if arrived[i].is_none() && !r.has_target {
                arrived[i] = Some(t);
            }
        }
        if arrived.iter().all(|a| a.is_some()) {
            break;
        }
    }
    let done: Vec<u32> = arrived.iter().filter_map(|a| *a).collect();
    let first = *done.iter().min().unwrap();
    let last = *done.iter().max().unwrap();
    println!(
        "combined arms (10 knight / 14 spear / 3 ram / 3 mangonel), one click 40 tiles: \
         first arrival tick {first}, last {last} -- the cavalry stands alone for {:.1}s",
        (last - first) as f64 / 20.0
    );
}

/// Pursuit vs a wall: an enemy stands behind an UNBROKEN wall line. Does the
/// combat pursuit path respect it?
fn run_pursuit_wall(seed: u32) {
    let (bx, by) = open_block(seed, 30, 24);
    let mut app = app_for(seed);
    app.world_mut().spawn((
        GameId(901),
        MatchId(1),
        Player {
            player_id: 2,
            name: "E".into(),
            faction: Faction::Crusader,
            stock: Stockpile::default(),
            color: 1,
            online: true,
            keep: 0,
            defeated: false,
            slot: 1,
            tech_mask: 0,
            hunger: 0,
        },
    ));
    let wall_x = bx + 10;
    let mut wid = 5000;
    for y in (by + 2)..(by + 22) {
        spawn_wall(&mut app, wid, V2::new(Fx::from_num(wall_x) + fx!("0.5"), Fx::from_num(y) + fx!("0.5")));
        wid += 1;
    }
    // attacker west of the wall, victim two tiles east of it — inside aggro range
    let ax = Fx::from_num(wall_x - 3) + fx!("0.5");
    let ay = Fx::from_num(by + 12) + fx!("0.5");
    spawn_unit(&mut app, 1, UnitKind::Knight, V2::new(ax, ay));
    let vx = Fx::from_num(wall_x + 2) + fx!("0.5");
    app.world_mut().spawn((
        GameId(2),
        Owner(2),
        MatchId(1),
        Pos { pos: V2::new(vx, ay), facing: ZERO },
        Unit {
            speed: Fx::ZERO,
            hp: 100000,
            stance: Stance::HoldGround,
            ..Unit::new(UnitKind::Peasant, V2::new(vx, ay))
        },
    ));
    let mut crossed = false;
    let mut on_wall = 0;
    for _ in 0..400 {
        step(app.world_mut());
        let rows = snapshot(&mut app);
        let a = rows.iter().find(|r| r.id == 1).unwrap();
        if a.pos.x.to_num::<i32>() == wall_x {
            on_wall += 1;
        }
        if f(a.pos.x) > wall_x as f64 + 1.0 {
            crossed = true;
        }
    }
    println!(
        "combat pursuit vs an UNBROKEN 20-tile wall: attacker walked THROUGH it = {crossed} \
         ({on_wall} ticks standing on a wall tile)"
    );
}

/// Formation slots are handed out in sorted-GameId order, not spatial order.
/// Count how many pairs must swap sides to reach their slot.
fn run_slot_crossing(seed: u32, n: usize) {
    let (bx, by) = open_block(seed, 60, 16);
    let mut app = app_for(seed);
    // spatial order is the REVERSE of id order along y
    for i in 0..n {
        let x = Fx::from_num(bx + 2) + fx!("0.5");
        let y = Fx::from_num(by + 2 + (n - 1 - i) as i32) + fx!("0.5");
        spawn_unit(&mut app, 1 + i as u64, UnitKind::Spearman, V2::new(x, y));
    }
    step(app.world_mut());
    let start = snapshot(&mut app);
    let gx = (bx + 40) as f32 + 0.5;
    let gy = (by + 8) as f32 + 0.5;
    let offs = formation(n);
    for i in 0..n {
        let (ox, oy) = offs[i];
        app.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::Move {
            player_id: 1,
            unit: 1 + i as u64,
            target: V2::new(Fx::from_num(gx + ox), Fx::from_num(gy + oy)),
        });
    }
    let mut widest: f64 = 0.0;
    for _ in 0..600 {
        step(app.world_mut());
        let rows = snapshot(&mut app);
        let (_, _, pair, _) = stats(&rows, (gx as f64, gy as f64));
        widest = widest.max(pair);
    }
    let end = snapshot(&mut app);
    let mut crossings = 0;
    for i in 0..n {
        for j in (i + 1)..n {
            let s = f(start[i].pos.y) - f(start[j].pos.y);
            let e = f(end[i].pos.y) - f(end[j].pos.y);
            if s * e < 0.0 {
                crossings += 1;
            }
        }
    }
    println!(
        "formation slots are assigned by sorted GameId: {crossings}/{} pairs had to swap sides \
         en route (widest the column ever got: {widest:.1} tiles)",
        n * (n - 1) / 2
    );
}

/// Does `StateHash` — the desync detector — actually see the army-control state
/// that rides on the wire?
fn run_hash_blindspots(seed: u32) {
    let (bx, by) = open_block(seed, 20, 20);
    let at = V2::new(Fx::from_num(bx + 5) + fx!("0.5"), Fx::from_num(by + 5) + fx!("0.5"));
    let probe = |mutate: &dyn Fn(&mut App)| -> u64 {
        let mut app = app_for(seed);
        for i in 0..8 {
            spawn_unit(&mut app, 1 + i, UnitKind::Spearman, V2::new(at.x + Fx::from_num(i as i32), at.y));
        }
        step(app.world_mut());
        mutate(&mut app);
        step(app.world_mut());
        app.world().resource::<StateHash>().0
    };
    let base = probe(&|_| {});
    let stance = probe(&|app| {
        for i in 0..8 {
            app.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::SetStance {
                player_id: 1,
                unit: 1 + i,
                stance: Stance::HoldGround,
            });
        }
    });
    let morale = probe(&|app| {
        let w = app.world_mut();
        let mut q = w.query::<&mut Unit>();
        for mut u in q.iter_mut(w) {
            u.morale = fx!("0.1");
            u.routing = true;
        }
    });
    let target = probe(&|app| {
        let w = app.world_mut();
        let mut q = w.query::<&mut Unit>();
        for mut u in q.iter_mut(w) {
            u.attack_target = 777;
            u.home = V2::new(Fx::ZERO, Fx::ZERO);
        }
    });
    println!("StateHash blind spots (identical hash == the desync detector is blind):");
    println!("  stance changed:            {}", if stance == base { "BLIND" } else { "seen" });
    println!("  morale + routing changed:  {}", if morale == base { "BLIND" } else { "seen" });
    println!("  attack_target/home changed:{}", if target == base { "BLIND" } else { "seen" });
}

fn main() {
    let seed = compose_seed(48514, 0);
    println!("seed {seed}\n");
    run_move("massed move, formation offsets", seed, 30, true, UnitKind::Spearman, 40);
    run_move("massed move, NO offsets (raw click)", seed, 30, false, UnitKind::Spearman, 40);
    run_move("mixed-speed check: knights", seed, 30, true, UnitKind::Knight, 40);
    println!();
    run_gap(seed, 30, 2);
    println!();
    run_sep_into_building(seed);
    run_gap(seed, 30, 1);
    println!();
    run_mixed(seed);
    println!();
    run_pursuit_wall(seed);
    println!();
    run_slot_crossing(seed, 30);
    println!();
    run_hash_blindspots(seed);
    println!();
    run_order_cost(seed);
}

/// What one big move order COSTS: `move_unit` rebuilds the whole building
/// occupancy set and runs a fresh A* per command, and `find_owned` scans every
/// entity per command. Time the tick the batch lands on.
pub fn run_order_cost(seed: u32) {
    for (units, buildings) in [(30usize, 40usize), (100, 80), (200, 120)] {
        let (bx, by) = open_block(seed, 58, 34);
        let mut app = app_for(seed);
        let mut id = 1u64;
        for i in 0..units {
            let x = Fx::from_num(bx + 2 + (i % 12) as i32) + fx!("0.5");
            let y = Fx::from_num(by + 2 + (i / 12) as i32) + fx!("0.5");
            spawn_unit(&mut app, id, UnitKind::Spearman, V2::new(x, y));
            id += 1;
        }
        // a real barrier between the army and its goal, so A* must SEARCH
        // instead of taking the clear-straight-line fast path
        for i in 0..buildings {
            let y = by + 2 + i as i32;
            if y >= by + 32 {
                break;
            }
            if (by + 16..by + 18).contains(&y) {
                continue; // the gap
            }
            spawn_wall(
                &mut app,
                9000 + i as u64,
                V2::new(Fx::from_num(bx + 30) + fx!("0.5"), Fx::from_num(y) + fx!("0.5")),
            );
        }
        step(app.world_mut());
        let t0 = std::time::Instant::now();
        step(app.world_mut());
        let quiet = t0.elapsed();
        let gx = (bx + 52) as f32 + 0.5;
        let gy = (by + 20) as f32 + 0.5;
        let offs = formation(units);
        for i in 0..units {
            app.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::Move {
                player_id: 1,
                unit: 1 + i as u64,
                target: V2::new(Fx::from_num(gx + offs[i].0), Fx::from_num(gy + offs[i].1)),
            });
        }
        let t1 = std::time::Instant::now();
        step(app.world_mut());
        let ordered = t1.elapsed();
        println!(
            "one move order for {units} units with {buildings} structures on the map: \
             quiet tick {:.2} ms -> order tick {:.2} ms ({:.0}x)",
            quiet.as_secs_f64() * 1000.0,
            ordered.as_secs_f64() * 1000.0,
            ordered.as_secs_f64() / quiet.as_secs_f64().max(1e-9)
        );
    }
}
