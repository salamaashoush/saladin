//! The cost curve, measured rather than asserted.
//!
//! Two numbers per building kind, from a cold start on a real map with a real
//! bot working a real economy: the tick a player can first AFFORD it, and the
//! tick a crew could have it STANDING. Only the second one is the price, and
//! before construction existed the two were the same number.
//!
//! It also prints the prereq graph, so a kind that is cheap but gated deep shows
//! up as exactly that. The bug this exists to catch is a rung the ladder cannot
//! climb in order: a Blacksmith affordable at t259 whose own prerequisite
//! Barracks is not affordable until t395.
//!
//! cargo run --release -p saladin-protocol --example build_audit6

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::*;

const TICKS: u32 = 9000;

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

fn stock_of(app: &mut App, owner: u64) -> Stockpile {
    let w = app.world_mut();
    let mut q = w.query::<&Player>();
    q.iter(w).find(|p| p.player_id == owner).map(|p| p.stock).unwrap_or(Stockpile {
        wood: 0,
        stone: 0,
        food: 0,
        gold: 0,
    })
}

/// Seconds a full crew needs on `kind`, at the diminishing-returns curve the
/// construction loop actually uses.
fn crew_seconds(build_time: Fx, builders: i32) -> Fx {
    let rate = build_rate(builders);
    if rate <= Fx::ZERO { Fx::ZERO } else { build_time / rate }
}

fn prereq_line(kind: BuildingKind) -> String {
    let all = all_prereqs(building_def(kind));
    if all.is_empty() {
        "-".into()
    } else {
        all.iter().map(|k| building_kind_label(*k)).collect::<Vec<_>>().join(" + ")
    }
}

fn building_kind_label(k: BuildingKind) -> &'static str {
    match k {
        BuildingKind::Keep => "Keep",
        BuildingKind::Barracks => "Barracks",
        BuildingKind::Tower => "Tower",
        BuildingKind::Wall => "Wall",
        BuildingKind::Gatehouse => "Gatehouse",
        BuildingKind::House => "House",
        BuildingKind::Stable => "Stable",
        BuildingKind::Blacksmith => "Blacksmith",
        BuildingKind::Market => "Market",
        BuildingKind::Granary => "Granary",
        BuildingKind::FishingHut => "FishingHut",
        BuildingKind::SiegeWorkshop => "SiegeWorkshop",
        BuildingKind::Watchtower => "Watchtower",
        BuildingKind::Farm => "Farm",
        BuildingKind::Storehouse => "Storehouse",
        BuildingKind::Mosque => "Mosque",
        BuildingKind::Harbour => "Harbour",
    }
}

const ALL_KINDS: [BuildingKind; 16] = [
    BuildingKind::Keep,
    BuildingKind::Barracks,
    BuildingKind::Tower,
    BuildingKind::Wall,
    BuildingKind::Gatehouse,
    BuildingKind::House,
    BuildingKind::Stable,
    BuildingKind::Blacksmith,
    BuildingKind::Market,
    BuildingKind::Granary,
    BuildingKind::FishingHut,
    BuildingKind::SiegeWorkshop,
    BuildingKind::Watchtower,
    BuildingKind::Farm,
    BuildingKind::Storehouse,
    BuildingKind::Mosque,
];

/// A tick count as seconds, to one decimal, in integer math - `Fx`'s own
/// Display prints every bit of an I32F32 and the table becomes unreadable.
fn secs(t: u32) -> String {
    format!("{}.{}", t / 20, (t % 20) / 2)
}

/// Pass 1: a player who gathers and never spends. This is the honest cost
/// curve — a bot that spends every coin the tick it earns it may never hold a
/// Blacksmith's price at any single instant, which says nothing about the cost.
fn time_to_affordable(seed: u32) -> [Option<u32>; 16] {
    let mut app = build_app(seed);
    scatter_world_nodes(app.world_mut(), 1);
    cmd(&mut app, PlayerCommand::Join {
        player_id: 1,
        name: "S".into(),
        faction: Faction::Ayyubid,
        match_id: 1,
    });
    step(app.world_mut());
    cmd(&mut app, PlayerCommand::AutoGather { player_id: 1 });

    let mut out: [Option<u32>; 16] = [None; 16];
    for t in 0..TICKS {
        step(app.world_mut());
        let stock = stock_of(&mut app, 1);
        for k in ALL_KINDS {
            let i = k as usize;
            if out[i].is_none() && stock.can_afford(&building_def(k).cost) {
                out[i] = Some(t);
            }
        }
    }
    out
}

/// Pass 2: a real bot on the same map, siting and RAISING through the same
/// commands a human uses.
fn bot_timeline(seed: u32) -> ([Option<u32>; 16], [Option<u32>; 16]) {
    let mut app = build_app(seed);
    scatter_world_nodes(app.world_mut(), 1);
    cmd(&mut app, PlayerCommand::AddAi {
        player_id: 1,
        host: 1,
        difficulty: AiDifficulty::Hard,
        faction: Faction::Ayyubid,
        match_id: 1,
    });
    step(app.world_mut());

    let (mut sited, mut standing): ([Option<u32>; 16], [Option<u32>; 16]) = ([None; 16], [None; 16]);
    for t in 0..TICKS {
        step(app.world_mut());
        let w = app.world_mut();
        let mut q = w.query::<(&Owner, &Building)>();
        for (o, b) in q.iter(w) {
            if o.0 != 1 {
                continue;
            }
            let i = b.kind as usize;
            if sited[i].is_none() {
                sited[i] = Some(t);
            }
            if standing[i].is_none() && b.complete() {
                standing[i] = Some(t);
            }
        }
    }
    (sited, standing)
}

fn main() {
    let seed = compose_seed(48514, 0);
    let affordable = time_to_affordable(seed);
    let (sited, standing) = bot_timeline(seed);

    println!("cold start, seed {seed}, {TICKS} ticks ({}s of play)", secs(TICKS));
    println!("afford = a saver's first instant holding the price");
    println!("sited/up = a Hard bot founding and finishing it\n");
    println!(
        "{:<14} {:>12} {:>6} {:>7} {:>7} {:>7} {:>7}  prereqs",
        "kind", "cost", "btime", "1 hand", "afford", "sited", "up"
    );
    println!("{}", "-".repeat(96));

    let mut ordered: Vec<BuildingKind> = ALL_KINDS.to_vec();
    ordered.sort_by_key(|k| (all_prereqs(building_def(*k)).len(), standing[*k as usize].unwrap_or(u32::MAX)));

    for k in ordered {
        let i = k as usize;
        let d = building_def(k);
        let cost = format!(
            "{}w{}s{}f{}g",
            d.cost.wood,
            if d.cost.stone > 0 { d.cost.stone.to_string() } else { "0".into() },
            d.cost.food,
            d.cost.gold
        );
        let one = crew_seconds(d.build_time, 1);
        let f = |o: Option<u32>| match o {
            Some(t) => secs(t),
            None => "-".into(),
        };
        println!(
            "{:<14} {:>12} {:>6} {:>7} {:>7} {:>7} {:>7}  {}",
            building_kind_label(k),
            cost,
            format!("{}s", d.build_time),
            format!("{one:.0}s"),
            f(affordable[i]),
            f(sited[i]),
            f(standing[i]),
            prereq_line(k)
        );
    }

    println!("\nprereq order check (a kind must never be affordable before its gate):");
    let mut bad = 0;
    for k in ALL_KINDS {
        let Some(mine) = affordable[k as usize] else { continue };
        for p in all_prereqs(building_def(k)) {
            match affordable[p as usize] {
                Some(t) if t <= mine => {}
                _ => {
                    bad += 1;
                    println!(
                        "  {} affordable at {}s but its gate {} is not",
                        building_kind_label(k),
                        secs(mine),
                        building_kind_label(p)
                    );
                }
            }
        }
    }
    if bad == 0 {
        println!("  clean: every gated kind priced above its own gate");
    }

    println!("\nwhat a crew is worth (barracks, {}s of labour):", building_def(BuildingKind::Barracks).build_time);
    let bt = building_def(BuildingKind::Barracks).build_time;
    for n in 1..=MAX_BUILDERS {
        let s = crew_seconds(bt, n);
        let solo = crew_seconds(bt, 1);
        println!("  {n} hands: {s:.1}s  ({:.2}x one hand)", solo / s.max(saladin_sim::fx!("0.001")));
    }
}
