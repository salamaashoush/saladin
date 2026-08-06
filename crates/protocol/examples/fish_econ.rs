//! What a fishing boat is actually worth, measured — the sea's answer to
//! `farm_econ`. A farm banks a measured 1.36 food/s per hand, forever, for 50
//! wood. The sea must be STEADIER AND SAFER than the plough and NEVER RICHER
//! PER HAND, and this is the instrument that says whether it is.
//!
//! cargo run --release -p saladin-protocol --example fish_econ [seed]

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::*;

/// What a farm banks per hand, forever, once it is up (measured by `farm_econ`).
const FARM_PER_HAND: f64 = 1.36;

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

fn spawn_b(app: &mut App, id: u64, kind: BuildingKind, pos: V2) {
    let def = building_def(kind);
    app.world_mut().spawn((
        GameId(id),
        Owner(1),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Building::new(kind, def.max_hp, pos),
    ));
}

fn food(app: &mut App) -> i32 {
    let w = app.world_mut();
    let mut q = w.query::<&Player>();
    q.iter(w).find(|p| p.player_id == 1).unwrap().stock.food
}

fn school_left(app: &mut App) -> i32 {
    let w = app.world_mut();
    let mut q = w.query::<(&GameId, &ResourceNode)>();
    q.iter(w).find(|(g, _)| g.0 == 20).map(|(_, n)| n.remaining).unwrap_or(-1)
}

fn idle_boats(app: &mut App) -> usize {
    let w = app.world_mut();
    let mut q = w.query::<&Unit>();
    q.iter(w)
        .filter(|u| unit_def(u.kind).afloat() && u.gather_state == GatherState::Idle)
        .count()
}

/// A shore tile with `len` tiles of open water straight out from it, all in one
/// body: a hut on the beach and a fishing ground however far out we ask for.
fn fishing_line(seed: u32, len: i32) -> Option<(V2, (i32, i32))> {
    for ty in 8..WORLD_SIZE - 8 {
        for tx in 8..WORLD_SIZE - 8 {
            if !is_passable(seed, tx, ty) {
                continue;
            }
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                if (1..=len).all(|k| is_sailable(seed, tx + dx * k, ty + dy * k)) {
                    return Some((center(tx, ty), (dx, dy)));
                }
            }
        }
    }
    None
}

struct Catch {
    per_s: f64,
    per_boat: f64,
    left: i32,
    idle: usize,
}

/// `boats` skiffs working one school `haul` tiles off a shore camp, for `secs`.
/// `tender` is the structure that berths them AND (when its aura reaches) tends
/// the water: the Fishing Hut reaches 6 tiles, the Harbour 13.
#[allow(clippy::too_many_arguments)]
fn run(
    seed: u32,
    shore: V2,
    dir: (i32, i32),
    haul: i32,
    cap: i32,
    regen: i32,
    tender: BuildingKind,
    boats: u64,
    secs: u32,
) -> Catch {
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let at = footprint_center(building_def(tender).footprint, shore.x, shore.y);
    spawn_b(&mut app, 10, tender, at);
    let school = V2::new(
        shore.x + Fx::from_num(dir.0 * haul),
        shore.y + Fx::from_num(dir.1 * haul),
    );
    app.world_mut().spawn((
        GameId(20),
        MatchId(1),
        Pos { pos: school, facing: ZERO },
        ResourceNode::renewable(ResourceType::Food, cap, cap, regen),
    ));
    let berth = berth_of(seed, building_def(tender).footprint, at).expect("shore berth");
    let def = unit_def(UnitKind::FishingSkiff);
    for i in 0..boats {
        let pos = berth;
        app.world_mut().spawn((
            GameId(30 + i),
            Owner(1),
            MatchId(1),
            Pos { pos, facing: ZERO },
            Unit { speed: def.speed, hp: def.max_hp, ..Unit::new(UnitKind::FishingSkiff, pos) },
        ));
    }
    step(app.world_mut());
    for i in 0..boats {
        app.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::Gather {
            player_id: 1,
            unit: 30 + i,
            node: 20,
        });
    }
    for _ in 0..secs * 20 {
        step(app.world_mut());
    }
    let banked = food(&mut app) as f64;
    Catch {
        per_s: banked / secs as f64,
        per_boat: banked / secs as f64 / boats as f64,
        left: school_left(&mut app),
        idle: idle_boats(&mut app),
    }
}

fn main() {
    let base: u32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(11);
    let seed = compose_seed(base, 0);
    let secs: u32 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(600);
    let Some((shore, dir)) = fishing_line(seed, 17) else {
        println!("seed {base} has no 17-tile straight sea run; try another");
        return;
    };
    println!(
        "seed {base} (composed {seed})  shore {:?} dir {:?}  {secs}s per run",
        shore, dir
    );
    println!(
        "inshore cap {FISH_INSHORE_CAP} regen {FISH_INSHORE_REGEN}   \
         offshore cap {FISH_OFFSHORE_CAP} regen {FISH_OFFSHORE_REGEN}   \
         hut r{FISHING_HUT_RANGE} x{}   harbour r{HARBOUR_RANGE} x{}",
        building_def(BuildingKind::FishingHut).aura.unwrap().regen,
        building_def(BuildingKind::Harbour).aura.unwrap().regen,
    );
    println!("the line to beat: a farm banks {FARM_PER_HAND:.2} food/s per hand, forever, for 50 wood\n");

    // The node's own flow, which is the ceiling a boat on station converges to.
    let flow = |regen: i32, mult: i32| Fx::from_num(regen * mult) / Fx::from_num(2);
    println!("== 0. THE CEILING (what the water itself produces, food/s) ==");
    println!(
        "  inshore  wild {:.2}  hut {:.2}  harbour {:.2}",
        flow(FISH_INSHORE_REGEN, 1).to_num::<f64>(),
        flow(FISH_INSHORE_REGEN, 2).to_num::<f64>(),
        flow(FISH_INSHORE_REGEN, 3).to_num::<f64>()
    );
    println!(
        "  offshore wild {:.2}  hut {:.2}  harbour {:.2}",
        flow(FISH_OFFSHORE_REGEN, 1).to_num::<f64>(),
        flow(FISH_OFFSHORE_REGEN, 2).to_num::<f64>(),
        flow(FISH_OFFSHORE_REGEN, 3).to_num::<f64>()
    );

    println!("\n== 1. ONE SKIFF, ONE SCHOOL, BY HAUL (food/s; 'left' = fish still in the water) ==");
    println!(
        "{:>9} {:>7} {:>10} {:>8} {:>7} {:>6}",
        "ground", "haul", "tender", "food/s", "left", "idle"
    );
    for (name, cap, regen) in [
        ("inshore", FISH_INSHORE_CAP, FISH_INSHORE_REGEN),
        ("offshore", FISH_OFFSHORE_CAP, FISH_OFFSHORE_REGEN),
    ] {
        for haul in [2, 4, 8, 16] {
            for tender in [BuildingKind::FishingHut, BuildingKind::Harbour] {
                let tended = Fx::from_num(haul) <= building_def(tender).aura.unwrap().radius;
                let c = run(seed, shore, dir, haul, cap, regen, tender, 1, secs);
                println!(
                    "{:>9} {:>7} {:>10} {:>8.2} {:>7} {:>6}",
                    name,
                    haul,
                    format!("{}{}", if tender == BuildingKind::Harbour { "harbour" } else { "hut" },
                            if tended { "" } else { " (far)" }),
                    c.per_s,
                    c.left,
                    c.idle
                );
            }
        }
    }

    println!("\n== 2. A SECOND BOAT ON ONE SCHOOL (the flow is the cap, not the hull) ==");
    println!("{:>9} {:>6} {:>8} {:>10} {:>7}", "ground", "boats", "food/s", "per boat", "left");
    for (name, cap, regen) in [
        ("inshore", FISH_INSHORE_CAP, FISH_INSHORE_REGEN),
        ("offshore", FISH_OFFSHORE_CAP, FISH_OFFSHORE_REGEN),
    ] {
        for boats in [1u64, 2, 3] {
            let c = run(seed, shore, dir, 4, cap, regen, BuildingKind::FishingHut, boats, secs);
            println!(
                "{:>9} {:>6} {:>8.2} {:>10.2} {:>7}",
                name, boats, c.per_s, c.per_boat, c.left
            );
        }
    }

    println!("\n== 3. THE VERDICT ==");
    let hut = run(seed, shore, dir, 4, FISH_INSHORE_CAP, FISH_INSHORE_REGEN, BuildingKind::FishingHut, 1, secs);
    let deep = run(seed, shore, dir, 12, FISH_OFFSHORE_CAP, FISH_OFFSHORE_REGEN, BuildingKind::Harbour, 1, secs);
    println!(
        "  tended inshore, 4-tile haul : {:.2} food/s per skiff  ({:+.0}% vs a farm hand)",
        hut.per_s,
        100.0 * (hut.per_s / FARM_PER_HAND - 1.0)
    );
    println!(
        "  tended offshore, 12-tile haul: {:.2} food/s per skiff  ({:+.0}% vs a farm hand)",
        deep.per_s,
        100.0 * (deep.per_s / FARM_PER_HAND - 1.0)
    );
    println!(
        "  cost: 40 wood of hut + 30 of hull = 70 wood for the first boat, 30 for each after;\n        \
         a farm is 50 wood and takes as many hands as you put on it."
    );

    println!("\n== 4. WHAT THE SEA IS ACTUALLY WORTH TO A TOWN ==");
    println!("(fisheries inside TOWN_RADIUS {TOWN_RADIUS} of each start, and the food/s they can sustain)");
    for preset in 0..4u8 {
        let s = compose_seed(base, preset);
        let nodes = scatter_nodes(s, &node_kinds());
        let wet: Vec<&ScatteredNode> = nodes
            .iter()
            .filter(|n| {
                n.res_type == ResourceType::Food
                    && is_sailable(s, n.pos.x.to_num::<i32>(), n.pos.y.to_num::<i32>())
            })
            .collect();
        let mut per_town = Vec::new();
        for slot in 0..MAX_PLAYERS {
            let start = start_point(s, slot);
            let (mut n, mut flow) = (0, 0i64);
            for nd in &wet {
                if dist(nd.pos, start) <= TOWN_RADIUS {
                    n += 1;
                    flow += nd.regen as i64;
                }
            }
            per_town.push((n, flow));
        }
        let tot: i32 = per_town.iter().map(|(n, _)| n).sum();
        let best = per_town.iter().map(|(_, f)| *f).max().unwrap_or(0);
        let map_flow: i64 = wet.iter().map(|n| n.regen as i64).sum();
        println!(
            "  preset {preset}: {} fisheries on the map ({:.0} food/s wild / {:.0} all-hutted); \
             in town: {:.1} avg, {} best-start ({:.1} food/s hutted)",
            wet.len(),
            map_flow as f64 / 2.0,
            map_flow as f64,
            tot as f64 / MAX_PLAYERS as f64,
            best,
            best as f64
        );
    }
}
