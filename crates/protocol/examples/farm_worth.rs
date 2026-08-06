//! AUDIT: what a field is actually worth, per second, against what it competes
//! with. Answers three questions with measurements rather than formulas:
//!   1. food/s a single field delivers at each crew size (and with a hub)
//!   2. food/s the SAME peasants deliver hunting a wild herd, and for how long
//!   3. fields needed to keep one soldier in rations
//!
//! cargo run --release -p saladin-protocol --example farm_worth [seed] [secs]

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::*;

fn center(tx: i32, ty: i32) -> V2 {
    V2::new(Fx::from_num(tx) + fx!("0.5"), Fx::from_num(ty) + fx!("0.5"))
}

fn build_app(seed: u32) -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed });
    app
}

fn spawn_player(app: &mut App, id: u64) {
    app.world_mut().spawn((
        GameId(900 + id),
        MatchId(1),
        Player {
            player_id: id,
            name: "P".into(),
            faction: Faction::Ayyubid,
            stock: Stockpile { wood: 9000, stone: 9000, food: 0, gold: 9000 },
            color: 0,
            online: true,
            keep: 0,
            defeated: false,
            slot: 0,
            tech_mask: 0,
            hunger: 0,
        },
    ));
}

fn spawn_b(app: &mut App, id: u64, owner: u64, kind: BuildingKind, pos: V2) {
    let def = building_def(kind);
    app.world_mut().spawn((
        GameId(id),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Building::new(kind, def.max_hp, pos),
    ));
}

fn spawn_peasants(app: &mut App, owner: u64, at: V2, n: u64, first: u64) -> Vec<u64> {
    let def = unit_def(UnitKind::Peasant);
    (0..n)
        .map(|i| {
            let id = first + i;
            let pos = V2::new(at.x + Fx::from_num(2 + i as i32), at.y + fx!("2"));
            app.world_mut().spawn((
                GameId(id),
                Owner(owner),
                MatchId(1),
                Pos { pos, facing: ZERO },
                Unit { speed: def.speed, hp: def.max_hp, ..Unit::new(UnitKind::Peasant, pos) },
            ));
            id
        })
        .collect()
}

fn food(app: &mut App) -> i32 {
    let w = app.world_mut();
    let mut q = w.query::<&Player>();
    q.iter(w).find(|p| p.player_id == 1).unwrap().stock.food
}

fn block(seed: u32, lo: Fx, hi: Fx) -> Option<(i32, i32, Fx)> {
    for cy in 20..WORLD_SIZE - 24 {
        for cx in 20..WORLD_SIZE - 24 {
            if !(-9..4).all(|dx| (-9..4).all(|dy| is_buildable_tile(seed, cx + dx, cy + dy))) {
                continue;
            }
            let c = center(cx, cy);
            let q = soil_quality(seed, 2, c.x, c.y);
            if q >= lo && q <= hi {
                return Some((cx, cy, q));
            }
        }
    }
    None
}

/// EXACTLY `spawn::spawn_field` (which is pub(crate)). Hand-rolling this without
/// the `Crop` row makes `season` skip the node entirely: it creeps on plain node
/// regen, never latches ripe, and so can never be reaped — which reads as "farms
/// deliver nothing" and is a measurement bug, not a game one.
fn sow(app: &mut App, building: u64, pos: V2, cap: i32) {
    let id = app.world_mut().resource_mut::<NextEntityId>().alloc();
    app.world_mut().spawn((
        GameId(id),
        Owner(1),
        MatchId(1),
        FieldOf(building),
        Crop::default(),
        Pos { pos, facing: ZERO },
        ResourceNode::renewable(ResourceType::Food, cap / FARM_SOW_DIVISOR, cap, FARM_REGEN_IDLE),
    ));
}

/// One farm, `hands` peasants ordered onto its field, `secs` of game time.
/// Returns food banked.
fn run_farm(seed: u32, bx: i32, by: i32, hands: u64, granary: bool, secs: u32) -> i32 {
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let farm_at = center(bx, by);
    spawn_b(&mut app, 10, 1, BuildingKind::Keep, center(bx - 6, by));
    spawn_b(&mut app, 11, 1, BuildingKind::Farm, farm_at);
    if granary {
        spawn_b(&mut app, 12, 1, BuildingKind::Granary, center(bx + 4, by));
    }
    let ids: Vec<u64> = {
        let w = app.world_mut();
        let mut q = w.query::<(&GameId, &Building)>();
        q.iter(w).filter(|(_, b)| b.kind == BuildingKind::Farm).map(|(g, _)| g.0).collect()
    };
    for id in ids {
        let soil = soil_quality(seed, 2, farm_at.x, farm_at.y);
        sow(&mut app, id, farm_at, field_cap(soil));
    }
    let peas = spawn_peasants(&mut app, 1, farm_at, hands, 40);
    step(app.world_mut());
    let field = {
        let w = app.world_mut();
        let mut q = w.query::<(&GameId, &FieldOf)>();
        q.iter(w).map(|(g, _)| g.0).next().unwrap()
    };
    for p in &peas {
        app.world_mut()
            .resource_mut::<CommandQueue>()
            .0
            .push(PlayerCommand::Gather { player_id: 1, unit: *p, node: field });
    }
    let trace = std::env::var("TRACE").is_ok();
    for t in 0..secs * 20 {
        step(app.world_mut());
        if trace && t % 400 == 0 {
            let banked = food(&mut app);
            let w = app.world_mut();
            let (rem, ripe, standing) = {
                let mut q = w.query::<(&ResourceNode, &FieldOf, Option<&Crop>)>();
                q.iter(w)
                    .map(|(n, _, c)| {
                        (n.remaining, c.map(|c| c.ripe).unwrap_or(false), c.map(|c| c.standing).unwrap_or(0))
                    })
                    .next()
                    .unwrap_or((-1, false, -1))
            };
            let builders = {
                let mut q = w.query::<&Building>();
                q.iter(w).find(|b| b.kind == BuildingKind::Farm).map(|b| b.builders).unwrap_or(-1)
            };
            let states: Vec<String> = {
                let mut q = w.query::<&Unit>();
                q.iter(w)
                    .filter(|u| u.kind == UnitKind::Peasant)
                    .map(|u| format!("{:?}/js{}/c{}", u.gather_state, u.job_site, u.carrying))
                    .collect()
            };
            println!(
                "    t={:>4}s banked={banked:>5} field={rem:>3} ripe={ripe} standing={standing:>3} builders={builders} {}",
                t / 20,
                states.join(" ")
            );
        }
    }
    food(&mut app)
}

/// The same peasants hunting wild herds. `herds` deposits of FOOD_YIELD each,
/// laid beside the keep. Returns (food banked, seconds until every herd was
/// stripped, or None if food was still coming in at the end).
fn run_wild(seed: u32, bx: i32, by: i32, hands: u64, herds: i32, secs: u32) -> (i32, Option<u32>) {
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let at = center(bx, by);
    spawn_b(&mut app, 10, 1, BuildingKind::Keep, center(bx - 6, by));
    let mut ids = Vec::new();
    for k in 0..herds {
        let id = app.world_mut().resource_mut::<NextEntityId>().alloc();
        let p = center(bx + (k % 4), by + (k / 4));
        app.world_mut().spawn((
            GameId(id),
            MatchId(1),
            Pos { pos: p, facing: ZERO },
            ResourceNode::deposit(ResourceType::Food, FOOD_YIELD),
        ));
        ids.push(id);
    }
    let peas = spawn_peasants(&mut app, 1, at, hands, 40);
    step(app.world_mut());
    for p in &peas {
        app.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::Gather {
            player_id: 1,
            unit: *p,
            node: ids[0],
        });
    }
    let mut dry = None;
    let mut last = 0;
    let mut flat_since: Option<u32> = None;
    for t in 0..secs * 20 {
        step(app.world_mut());
        if t % 20 == 0 {
            let now = food(&mut app);
            if now == last {
                if flat_since.is_none() {
                    flat_since = Some(t / 20);
                }
            } else {
                flat_since = None;
                last = now;
            }
            // 30 s with nothing coming in = the herds are gone
            if dry.is_none() && flat_since.is_some_and(|s| t / 20 - s >= 30) {
                dry = flat_since;
            }
        }
    }
    (food(&mut app), dry)
}

fn main() {
    let seed: u32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(48514);
    let secs: u32 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(600);
    let per = |banked: i32| banked as f32 / secs as f32;

    let (bx, by, soil) = block(seed, fx!("0.30"), fx!("0.36")).expect("no median-soil block");
    println!("seed {seed}, {secs}s per run, median soil {:.3} at ({bx},{by})", soil.to_num::<f32>());
    println!("FARM_STORE {FARM_STORE}  FARM_TEND_TIME {}  regen_idle {FARM_REGEN_IDLE}", FARM_TEND_TIME);

    println!("\n== ONE FIELD, food/s delivered to the stockpile ==");
    println!("{:>6} {:>8} {:>9} {:>9}", "hands", "food", "food/s", "per hand");
    for h in [0u64, 1, 2, 3, 4] {
        let b = run_farm(seed, bx, by, h, false, secs);
        let ph = if h > 0 { per(b) / h as f32 } else { 0.0 };
        println!("{h:>6} {b:>8} {:>9.3} {ph:>9.3}", per(b));
    }
    println!("\n== THE SAME, WITH A GRANARY HUB IN REACH ==");
    for h in [1u64, 2, 3] {
        let b = run_farm(seed, bx, by, h, true, secs);
        println!("{h:>6} {b:>8} {:>9.3}", per(b));
    }

    println!("\n== THE SAME HANDS HUNTING WILD HERDS (FOOD_YIELD {FOOD_YIELD}/herd) ==");
    println!("{:>6} {:>6} {:>8} {:>9} {:>12}", "hands", "herds", "food", "food/s", "dry after");
    for (h, n) in [(1u64, 4), (2, 4), (3, 4), (3, 16)] {
        let (b, dry) = run_wild(seed, bx, by, h, n, secs);
        let d = dry.map(|s| format!("{s}s")).unwrap_or_else(|| "still going".into());
        println!("{h:>6} {n:>6} {b:>8} {:>9.3} {d:>12}", per(b));
    }

    println!("\n== WHAT THAT FEEDS ==");
    println!("one soldier in supply draws {FOOD_PER_UNIT} food / economy tick ({ECONOMY_DT}s)");
    let per_soldier = FOOD_PER_UNIT as f32 / ECONOMY_DT.to_num::<f32>();
    println!("                          = {per_soldier:.3} food/s");
    let f3 = per(run_farm(seed, bx, by, 3, false, secs));
    println!("a 3-hand field delivers     {f3:.3} food/s");
    println!("=> one field feeds          {:.1} soldiers", f3 / per_soldier);
    println!("=> fields per soldier       {:.2}", per_soldier / f3);
}
