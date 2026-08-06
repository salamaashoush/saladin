//! THROWAWAY AUDIT HARNESS — what a farm is actually worth, measured.
//!
//! cargo run --release -p saladin-protocol --example farm_econ [seed]

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

fn field_state(app: &mut App) -> Option<(i32, i32, i32)> {
    let w = app.world_mut();
    let mut q = w.query::<(&ResourceNode, &FieldOf)>();
    q.iter(w).map(|(n, _)| (n.remaining, n.cap, n.regen)).next()
}

/// A clear buildable block whose 2x2 farm soil lands in [lo, hi].
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

struct Run {
    banked: i32,
    field_alive: bool,
    field_left: i32,
    death_tick: Option<u32>,
}

/// Raise a farm instantly, hand it `hands` peasants, run `secs`.
fn run_farm(seed: u32, bx: i32, by: i32, hands: u64, granary: bool, secs: u32) -> Run {
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let farm_at = center(bx, by);
    // keep 6 tiles away — a realistic short haul
    spawn_b(&mut app, 10, 1, BuildingKind::Keep, center(bx - 6, by));
    spawn_b(&mut app, 11, 1, BuildingKind::Farm, farm_at);
    if granary {
        spawn_b(&mut app, 12, 1, BuildingKind::Granary, center(bx + 4, by));
    }
    // sow it exactly as construction would
    let ids: Vec<u64> = {
        let w = app.world_mut();
        let mut q = w.query::<(&GameId, &Building)>();
        q.iter(w).filter(|(_, b)| b.kind == BuildingKind::Farm).map(|(g, _)| g.0).collect()
    };
    for id in ids {
        let soil = soil_quality(seed, 2, farm_at.x, farm_at.y);
        let regen = Fx::ONE + soil * Fx::from_num(FARM_REGEN_MAX);
        sow(&mut app, id, farm_at, regen.to_num::<i32>().max(1));
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
    let mut death = None;
    for t in 0..secs * 20 {
        step(app.world_mut());
        if death.is_none() && field_state(&mut app).is_none() {
            death = Some(t);
        }
    }
    let fs = field_state(&mut app);
    Run {
        banked: food(&mut app),
        field_alive: fs.is_some(),
        field_left: fs.map(|f| f.0).unwrap_or(0),
        death_tick: death,
    }
}

/// The field spawn, copied out of `finish_building` (which is pub(crate)).
fn sow(app: &mut App, building: u64, pos: V2, regen: i32) {
    let id = app.world_mut().resource_mut::<NextEntityId>().alloc();
    app.world_mut().spawn((
        GameId(id),
        Owner(1),
        MatchId(1),
        FieldOf(building),
        Pos { pos, facing: ZERO },
        ResourceNode::renewable(ResourceType::Food, FARM_STORE / 3, FARM_STORE, regen),
    ));
}

/// One wild herd of FOOD_YIELD, `hands` peasants, no regen.
fn run_wild(seed: u32, bx: i32, by: i32, hands: u64, secs: u32) -> (i32, Option<u32>) {
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let at = center(bx, by);
    spawn_b(&mut app, 10, 1, BuildingKind::Keep, center(bx - 6, by));
    let id = app.world_mut().resource_mut::<NextEntityId>().alloc();
    app.world_mut().spawn((
        GameId(id),
        MatchId(1),
        Pos { pos: at, facing: ZERO },
        ResourceNode::deposit(ResourceType::Food, FOOD_YIELD),
    ));
    let peas = spawn_peasants(&mut app, 1, at, hands, 40);
    step(app.world_mut());
    for p in &peas {
        app.world_mut()
            .resource_mut::<CommandQueue>()
            .0
            .push(PlayerCommand::Gather { player_id: 1, unit: *p, node: id });
    }
    let mut death = None;
    for t in 0..secs * 20 {
        step(app.world_mut());
        if death.is_none() {
            let w = app.world_mut();
            let mut q = w.query::<&GameId>();
            if !q.iter(w).any(|g| g.0 == id) {
                death = Some(t);
            }
        }
    }
    (food(&mut app), death)
}

fn main() {
    let base: u32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(11);
    let seed = compose_seed(base, 1);

    println!("== 1. THE REGEN TABLE (what the soil buys) ==");
    println!(
        "{:>6} {:>6} {:>8} {:>10} {:>12} {:>12}",
        "soil", "regen", "food/s", "+granary", "empty->full", "w/granary"
    );
    for pct in [22, 30, 40, 50, 60, 70, 85, 100] {
        let soil = Fx::from_num(pct) / Fx::from_num(100);
        let regen = (Fx::ONE + soil * Fx::from_num(FARM_REGEN_MAX)).to_num::<i32>().max(1);
        let fps = Fx::from_num(regen) / Fx::from_num(2);
        let fps_g = Fx::from_num(regen + 3) / Fx::from_num(2);
        let refill = Fx::from_num(FARM_STORE) / fps;
        let refill_g = Fx::from_num(FARM_STORE) / fps_g;
        println!(
            "{:>5}% {:>6} {:>8.2} {:>10.2} {:>10.0}s {:>10.0}s",
            pct, regen, fps, fps_g, refill, refill_g
        );
    }

    println!("\n== 2. WHAT SOIL REALLY EXISTS (fertility of buildable land, 6 seeds) ==");
    for b in [11u32, 48514, 7, 1234, 99, 20250] {
        let s = compose_seed(b, 1);
        let mut hist = [0u32; 10];
        let mut n = 0u32;
        let mut farmable = 0u32;
        let mut y = 24;
        while y < WORLD_SIZE - 24 {
            let mut x = 24;
            while x < WORLD_SIZE - 24 {
                if is_buildable_tile(s, x, y) {
                    let f = fertility_at(s, Fx::from_num(x) + fx!("0.5"), Fx::from_num(y) + fx!("0.5"));
                    let bucket = (f * Fx::from_num(10)).to_num::<usize>().min(9);
                    hist[bucket] += 1;
                    n += 1;
                    if f >= FARM_MIN_FERTILITY {
                        farmable += 1;
                    }
                }
                x += 2;
            }
            y += 2;
        }
        let pct = |v: u32| if n == 0 { 0.0 } else { 100.0 * v as f64 / n as f64 };
        println!(
            "seed {:>6}: buildable {:>6}  farmable {:>5.1}%  buckets {:?}",
            b,
            n,
            pct(farmable),
            hist.iter().map(|v| pct(*v) as i32).collect::<Vec<_>>()
        );
    }

    println!("\n== 3. A FARM WITH HANDS ON IT (300 s) ==");
    let Some((bx, by, soil)) = block(seed, fx!("0.30"), fx!("0.9")) else {
        println!("no fertile block on seed {seed}");
        return;
    };
    println!("field at ({bx},{by}) soil {soil:.3}, keep 6 tiles west");
    println!("{:>6} {:>8} {:>9} {:>10} {:>12} {:>10}", "hands", "granary", "banked", "food/s", "per-hand/s", "field");
    for granary in [false, true] {
        for hands in [1u64, 2, 3, 5] {
            let r = run_farm(seed, bx, by, hands, granary, 300);
            let fps = r.banked as f64 / 300.0;
            let tag = match (r.field_alive, r.death_tick) {
                (true, _) => format!("alive {}", r.field_left),
                (false, Some(t)) => format!("DIED @{}s", t / 20),
                _ => "gone".into(),
            };
            println!(
                "{:>6} {:>8} {:>9} {:>10.2} {:>12.2} {:>10}",
                hands,
                granary,
                r.banked,
                fps,
                fps / hands as f64,
                tag
            );
        }
    }

    println!("\n== 4. THE SAME HANDS ON A WILD HERD (FOOD_YIELD {FOOD_YIELD}, 300 s) ==");
    println!("{:>6} {:>9} {:>10} {:>12} {:>12}", "hands", "banked", "food/s", "per-hand/s", "exhausted");
    for hands in [1u64, 2, 3, 5] {
        let (banked, death) = run_wild(seed, bx, by, hands, 300);
        println!(
            "{:>6} {:>9} {:>10.2} {:>12.2} {:>12}",
            hands,
            banked,
            banked as f64 / 300.0,
            banked as f64 / 300.0 / hands as f64,
            death.map(|t| format!("{}s", t / 20)).unwrap_or_else(|| "no".into())
        );
    }

    println!("\n== 5. COST RECOVERY ==");
    let farm_cost = building_def(BuildingKind::Farm).cost;
    let gran_cost = building_def(BuildingKind::Granary).cost;
    println!("farm  {:?}  build_time {}s", farm_cost, building_def(BuildingKind::Farm).build_time);
    println!("gran  {:?}  build_time {}s  aura r={} mult={} regen={}",
        gran_cost,
        building_def(BuildingKind::Granary).build_time,
        building_def(BuildingKind::Granary).aura.unwrap().radius,
        building_def(BuildingKind::Granary).aura.unwrap().harvest_mult,
        building_def(BuildingKind::Granary).aura.unwrap().regen);

    println!("\n== 6. SUPPLY: THE ROAD, NOT THE ROLL ==");
    println!("a garrison inside {SUPPLY_RADIUS} tiles of a drop-off draws NOTHING");
    println!("FIELD_RATION {FIELD_RATION} food/man/economy tick per unit of strain, capped at {MAX_STRAIN}");
    for d in [40, 68, 136, 250] {
        let st = strain(Fx::from_num(d));
        println!(
            "  {d:>3} tiles out: strain {:>5}, one man {:>7} food/s, a field of regen r feeds r/{:.3} men",
            st,
            man_draw(st) / Fx::from_num(2),
            (man_draw(st) / Fx::from_num(2)).to_num::<f32>().max(0.0001)
        );
    }

    println!("\n== 7a. IS A DEAD FIELD EVER RE-SOWN? ==");
    {
        let mut app = build_app(seed);
        spawn_player(&mut app, 1);
        let at = center(bx, by);
        spawn_b(&mut app, 10, 1, BuildingKind::Keep, center(bx - 6, by));
        spawn_b(&mut app, 11, 1, BuildingKind::Farm, at);
        sow(&mut app, 11, at, 3);
        let peas = spawn_peasants(&mut app, 1, at, 1, 40);
        step(app.world_mut());
        let field = {
            let w = app.world_mut();
            let mut q = w.query::<(&GameId, &FieldOf)>();
            q.iter(w).map(|(g, _)| g.0).next().unwrap()
        };
        app.world_mut()
            .resource_mut::<CommandQueue>()
            .0
            .push(PlayerCommand::Gather { player_id: 1, unit: peas[0], node: field });
        for _ in 0..20 * 900 {
            step(app.world_mut());
        }
        let farms_up = {
            let w = app.world_mut();
            let mut q = w.query::<&Building>();
            q.iter(w).filter(|b| b.kind == BuildingKind::Farm && b.complete()).count()
        };
        println!(
            "after 900 s: farms standing {}, fields alive {}, food banked {}",
            farms_up,
            field_state(&mut app).is_some() as i32,
            food(&mut app)
        );
    }

    println!("\n== 7b. THE SAME FLAW ON A FISHERY? ==");
    {
        // a water tile with a buildable land tile beside it
        let mut spot = None;
        'outer: for y in 20..WORLD_SIZE - 20 {
            for x in 20..WORLD_SIZE - 20 {
                if !is_water_tile(seed, x, y) {
                    continue;
                }
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (lx, ly) = (x + dx, y + dy);
                    if is_buildable_tile(seed, lx, ly)
                        && (-4..5).all(|a| (-4..5).all(|b| is_passable(seed, lx + a, ly + b) || is_water_tile(seed, lx + a, ly + b)))
                    {
                        spot = Some((x, y, lx, ly));
                        break 'outer;
                    }
                }
            }
        }
        match spot {
            None => println!("no shore on this seed"),
            Some((wx, wy, lx, ly)) => {
                for hut in [false, true] {
                    let mut app = build_app(seed);
                    spawn_player(&mut app, 1);
                    spawn_b(&mut app, 10, 1, BuildingKind::Keep, center(lx + 3, ly + 3));
                    if hut {
                        spawn_b(&mut app, 11, 1, BuildingKind::FishingHut, center(lx, ly));
                    }
                    let id = app.world_mut().resource_mut::<NextEntityId>().alloc();
                    app.world_mut().spawn((
                        GameId(id),
                        MatchId(1),
                        Pos { pos: center(wx, wy), facing: ZERO },
                        ResourceNode::deposit(ResourceType::Food, FOOD_YIELD),
                    ));
                    let peas = spawn_peasants(&mut app, 1, center(lx, ly), 1, 40);
                    step(app.world_mut());
                    app.world_mut()
                        .resource_mut::<CommandQueue>()
                        .0
                        .push(PlayerCommand::Gather { player_id: 1, unit: peas[0], node: id });
                    let mut death = None;
                    for t in 0..20 * 600 {
                        step(app.world_mut());
                        if death.is_none() {
                            let w = app.world_mut();
                            let mut q = w.query::<&GameId>();
                            if !q.iter(w).any(|g| g.0 == id) {
                                death = Some(t / 20);
                            }
                        }
                    }
                    println!(
                        "  hut {:>5}: banked {:>6} in 600 s ({:.2}/s), fishery {}",
                        hut,
                        food(&mut app),
                        food(&mut app) as f64 / 600.0,
                        death.map(|t| format!("DIED @{t}s")).unwrap_or_else(|| "alive".into())
                    );
                }
            }
        }
    }

    println!("\n== 7c. WHAT ONE PEASANT DRAWS (unlimited node, by haul distance) ==");
    println!("{:>6} {:>10} {:>12} {:>12}", "tiles", "food/s", "round trip", "vs regen@0.35");
    for d in [2i32, 4, 6, 8, 12, 16, 20] {
        let mut app = build_app(seed);
        spawn_player(&mut app, 1);
        let at = center(bx, by);
        spawn_b(&mut app, 10, 1, BuildingKind::Keep, center(bx - d, by));
        let id = app.world_mut().resource_mut::<NextEntityId>().alloc();
        app.world_mut().spawn((
            GameId(id),
            MatchId(1),
            Pos { pos: at, facing: ZERO },
            ResourceNode::deposit(ResourceType::Food, 1_000_000),
        ));
        let peas = spawn_peasants(&mut app, 1, at, 1, 40);
        step(app.world_mut());
        app.world_mut()
            .resource_mut::<CommandQueue>()
            .0
            .push(PlayerCommand::Gather { player_id: 1, unit: peas[0], node: id });
        for _ in 0..20 * 200 {
            step(app.world_mut());
        }
        let f = food(&mut app) as f64;
        println!(
            "{:>6} {:>10.2} {:>10.1}s {:>12}",
            d,
            f / 200.0,
            if f > 0.0 { 8.0 / (f / 200.0) } else { 0.0 },
            if f / 200.0 > 1.5 { "OUTDRAWS" } else { "ok" }
        );
    }

    println!("\n== 8. A REAL BOT ON A REAL MAP (hard, 900 s) ==");
    {
        let bseed = compose_seed(48514, 0);
        let mut app = build_app(bseed);
        scatter_world_nodes(app.world_mut(), 1);
        app.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::AddAi {
            player_id: 1,
            host: 1,
            difficulty: AiDifficulty::Hard,
            faction: Faction::Ayyubid,
            match_id: 1,
        });
        step(app.world_mut());
        println!(
            "{:>5} {:>6} {:>6} {:>7} {:>7} {:>9} {:>8}",
            "t", "farms", "fields", "crop", "gran", "food", "soldiers"
        );
        for t in 0..20 * 900 {
            step(app.world_mut());
            if t % 2000 != 1999 {
                continue;
            }
            let w = app.world_mut();
            let (mut farms, mut grans) = (0, 0);
            {
                let mut q = w.query::<(&Owner, &Building)>();
                for (o, b) in q.iter(w) {
                    if o.0 != 1 || !operational(b.state) {
                        continue;
                    }
                    if b.kind == BuildingKind::Farm {
                        farms += 1;
                    }
                    if b.kind == BuildingKind::Granary {
                        grans += 1;
                    }
                }
            }
            let (nf, crop) = {
                let mut q = w.query::<(&ResourceNode, &FieldOf)>();
                q.iter(w).fold((0, 0), |(c, s), (n, _)| (c + 1, s + n.remaining))
            };
            let fd = { let mut q = w.query::<&Player>(); q.iter(w).find(|p| p.player_id == 1).map(|p| p.stock.food).unwrap_or(0) };
            let sold = {
                let mut q = w.query::<(&Owner, &Unit)>();
                q.iter(w).filter(|(o, u)| o.0 == 1 && unit_def(u.kind).attack > 0 && u.kind != UnitKind::Peasant).count()
            };
            let field_ids: Vec<u64> = {
                let mut q = w.query::<(&GameId, &FieldOf)>();
                q.iter(w).map(|(g, _)| g.0).collect()
            };
            let per: Vec<i32> = {
                let mut q = w.query::<(&ResourceNode, &FieldOf)>();
                q.iter(w).map(|(n, _)| n.remaining).collect()
            };
            let (on_field, gathering) = {
                let mut q = w.query::<(&Owner, &Unit)>();
                q.iter(w).filter(|(o, u)| o.0 == 1 && u.kind == UnitKind::Peasant).fold(
                    (0, 0),
                    |(a, b), (_, u)| {
                        let busy = u.gather_state != GatherState::Idle
                            && u.gather_state != GatherState::Constructing;
                        (a + field_ids.contains(&u.target_node) as i32, b + busy as i32)
                    },
                )
            };
            println!(
                "{:>5} {:>6} {:>6} {:>7} {:>7} {:>9} {:>8}   hands-on-fields {on_field}/{gathering}  per-field {per:?}",
                (t + 1) / 20, farms, nf, crop, grans, fd, sold
            );
        }
    }

    println!("\n== 8b. WHERE THE BOT'S FOOD ACTUALLY COMES FROM (hard, 900 s) ==");
    {
        use bevy_platform::collections::HashMap;
        let bseed = compose_seed(48514, 0);
        let mut app = build_app(bseed);
        scatter_world_nodes(app.world_mut(), 1);
        app.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::AddAi {
            player_id: 1,
            host: 1,
            difficulty: AiDifficulty::Hard,
            faction: Faction::Ayyubid,
            match_id: 1,
        });
        step(app.world_mut());
        let mut prev: HashMap<u64, i32> = HashMap::new();
        let mut hauled_from_fields: i64 = 0;
        let mut regen_applied: i64 = 0;
        let mut regen_capacity: i64 = 0;
        for _ in 0..20 * 900 {
            step(app.world_mut());
            let w = app.world_mut();
            let now: Vec<(u64, i32, i32, i32)> = {
                let mut q = w.query::<(&GameId, &ResourceNode, &FieldOf)>();
                q.iter(w).map(|(g, n, _)| (g.0, n.remaining, n.cap, n.regen)).collect()
            };
            for (id, rem, _cap, _regen) in &now {
                if let Some(p) = prev.get(id) {
                    let d = rem - p;
                    if d < 0 {
                        hauled_from_fields += (-d) as i64;
                    } else {
                        regen_applied += d as i64;
                    }
                }
            }
            prev.clear();
            for (id, rem, _, _) in &now {
                prev.insert(*id, *rem);
            }
        }
        // regen capacity: sample the standing fields at the end
        {
            let w = app.world_mut();
            let mut q = w.query::<(&ResourceNode, &FieldOf)>();
            for (n, _) in q.iter(w) {
                regen_capacity += n.regen as i64;
            }
        }
        let total = { let w = app.world_mut(); w.resource::<MatchStats>().0.get(&1).map(|s| s.gathered).unwrap_or(0) };
        println!("food hauled OUT of fields over 900 s : {hauled_from_fields}");
        println!("regen actually ABSORBED by fields    : {regen_applied}");
        println!("regen per economy tick still standing: {regen_capacity} (x450 ticks = {} if never capped)", regen_capacity * 450);
        println!("total of EVERY resource gathered     : {total}");
    }

    println!("\n== 9. THE BOT'S LAYOUT: DOES THE GRANARY REACH ANYTHING? ==");
    {
        let bseed = compose_seed(48514, 0);
        let mut app = build_app(bseed);
        scatter_world_nodes(app.world_mut(), 1);
        app.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::AddAi {
            player_id: 1,
            host: 1,
            difficulty: AiDifficulty::Hard,
            faction: Faction::Ayyubid,
            match_id: 1,
        });
        step(app.world_mut());
        for _ in 0..20 * 900 {
            step(app.world_mut());
        }
        let w = app.world_mut();
        let mut grans: Vec<V2> = Vec::new();
        let mut farms: Vec<V2> = Vec::new();
        {
            let mut q = w.query::<(&Owner, &Pos, &Building)>();
            for (o, p, b) in q.iter(w) {
                if o.0 != 1 || !operational(b.state) {
                    continue;
                }
                if b.kind == BuildingKind::Granary {
                    grans.push(p.pos);
                }
                if b.kind == BuildingKind::Farm {
                    farms.push(p.pos);
                }
            }
        }
        let fields: Vec<V2> = {
            let mut q = w.query::<(&Pos, &FieldOf)>();
            q.iter(w).map(|(p, _)| p.pos).collect()
        };
        let covered = fields.iter().filter(|f| grans.iter().any(|g| dist(*g, **f) <= GRANARY_RANGE)).count();
        println!("granaries {}, farms {}, living fields {}", grans.len(), farms.len(), fields.len());
        println!("living fields inside a granary aura: {covered}/{}", fields.len());
        if let Some(g) = grans.first() {
            let mut ds: Vec<f64> = farms.iter().map(|f| dist(*g, *f).to_num::<f64>()).collect();
            ds.sort_by(|a, b| a.partial_cmp(b).unwrap());
            println!("granary->farm distances: {:?} (aura reaches {GRANARY_RANGE})",
                ds.iter().map(|d| (d * 10.0).round() / 10.0).collect::<Vec<_>>());
        }
        // farm spread: how far apart the bot actually puts them
        let mut spread = 0.0f64;
        for a in &farms {
            for b in &farms {
                spread = spread.max(dist(*a, *b).to_num::<f64>());
            }
        }
        println!("widest farm-to-farm span: {spread:.1} tiles (TOWN_RADIUS {TOWN_RADIUS})");
    }

    println!("\n== 10. THE FREE FOOD ON THE MAP (what farming competes with) ==");
    {
        let bseed = compose_seed(48514, 0);
        let mut app = build_app(bseed);
        scatter_world_nodes(app.world_mut(), 1);
        let w = app.world_mut();
        let mut land_food = 0i64;
        let mut water_food = 0i64;
        let mut nland = 0;
        let mut nwater = 0;
        {
            let mut q = w.query::<(&Pos, &ResourceNode)>();
            for (p, n) in q.iter(w) {
                if n.res_type != ResourceType::Food {
                    continue;
                }
                if is_passable(bseed, p.pos.x.to_num::<i32>(), p.pos.y.to_num::<i32>()) {
                    land_food += n.remaining as i64;
                    nland += 1;
                } else {
                    water_food += n.remaining as i64;
                    nwater += 1;
                }
            }
        }
        println!("wild herds  : {nland} nodes, {land_food} food");
        println!("fisheries   : {nwater} nodes, {water_food} food");
        println!("total wild  : {} food; a 10-man army eats {} food per 900 s", land_food + water_food, 10 * 450);
        println!("one farm delivers ~78 food before its field dies (measured above)");
    }

    extra();
    extra();

    println!("\n== 13. CAN A FARM EVER SURVIVE? (best real soil, 1 peasant, 400 s) ==");
    for (lo, hi, tag) in [(fx!("0.24"), fx!("0.30"), "poor"), (fx!("0.34"), fx!("0.40"), "median"), (fx!("0.55"), fx!("0.99"), "best")] {
        let Some((cx, cy, q)) = block(seed, lo, hi) else { println!("  {tag}: no such block"); continue };
        for granary in [false, true] {
            let mut row = String::new();
            for d in [2i32, 4, 6, 8, 12] {
                let mut app = build_app(seed);
                spawn_player(&mut app, 1);
                let at = center(cx, cy);
                spawn_b(&mut app, 10, 1, BuildingKind::Keep, center(cx - d, cy));
                spawn_b(&mut app, 11, 1, BuildingKind::Farm, at);
                if granary {
                    spawn_b(&mut app, 12, 1, BuildingKind::Granary, center(cx + 3, cy + 3));
                }
                let regen = (Fx::ONE + q * Fx::from_num(FARM_REGEN_MAX)).to_num::<i32>().max(1);
                sow(&mut app, 11, at, regen);
                let peas = spawn_peasants(&mut app, 1, at, 1, 40);
                step(app.world_mut());
                let fid = { let w = app.world_mut(); let mut qq = w.query::<(&GameId, &FieldOf)>(); qq.iter(w).map(|(g, _)| g.0).next().unwrap() };
                app.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::Gather { player_id: 1, unit: peas[0], node: fid });
                let mut death = None;
                for t in 0..20 * 400 {
                    step(app.world_mut());
                    if death.is_none() && field_state(&mut app).is_none() { death = Some(t / 20); }
                }
                row += &match death { Some(t) => format!(" d{d}:DEAD@{t}s"), None => format!(" d{d}:LIVES({})", food(&mut app)) };
            }
            println!("  soil {q:.2} ({tag}) regen {} granary {granary:>5}:{row}", (Fx::ONE + q * Fx::from_num(FARM_REGEN_MAX)).to_num::<i32>().max(1));
        }
    }

    println!("\n== 7. GRANARY REACH vs TOWN ==");
    println!("GRANARY_RANGE {GRANARY_RANGE}  TOWN_RADIUS {TOWN_RADIUS}  farm footprint 2");
    let r = GRANARY_RANGE.to_num::<f64>();
    println!("area in reach ~{:.0} tiles; 2x2 farms need 3-tile pitch to leave lanes -> ~{} farms max",
        std::f64::consts::PI * r * r,
        ((std::f64::consts::PI * r * r) / 9.0) as i32);
}

// appended: water-food census + food in reach of a start

fn extra() {
    println!("\n== 11. ARE THERE ANY WATER FOOD NODES AT ALL? ==");
    for b in [11u32, 48514, 7, 1234, 99, 20250] {
        for preset in 0..4u8 {
            let s = compose_seed(b, preset);
            let nodes = scatter_nodes(s, &node_kinds());
            let (mut wet, mut dry) = (0, 0);
            for n in &nodes {
                if n.res_type != ResourceType::Food {
                    continue;
                }
                if is_passable(s, n.pos.x.to_num::<i32>(), n.pos.y.to_num::<i32>()) {
                    dry += 1;
                } else {
                    wet += 1;
                }
            }
            print!("  seed {b} p{preset}: wet {wet} dry {dry};");
        }
        println!();
    }

    println!("\n== 12. FOOD IN REACH OF A START (TOWN_RADIUS {TOWN_RADIUS}) ==");
    for b in [11u32, 48514, 7, 1234] {
        let s = compose_seed(b, 0);
        let mut nodes = scatter_nodes(s, &node_kinds());
        nodes.extend(fair_start_nodes(s, &nodes, MAX_PLAYERS, TREE_WOOD, STONE_YIELD, FOOD_YIELD));
        for slot in 0..2 {
            let start = start_point(s, slot);
            let mut food_near = 0i64;
            let mut n_near = 0;
            for nd in &nodes {
                if nd.res_type == ResourceType::Food && dist(nd.pos, start) <= TOWN_RADIUS {
                    food_near += nd.yield_ as i64;
                    n_near += 1;
                }
            }
            println!(
                "  seed {b} slot {slot}: {n_near} herds / {food_near} food inside the town; \
                 a 10-man army eats that in {}s",
                food_near / 5
            );
        }
    }
}
