//! The kept combat/army/siege instrument. Every balance or tactics claim in
//! this project should be a line printed here first and a test second.
//!
//! cargo run --release -p saladin-protocol --example tactics_bench -- <mode>
//!   roster  unit table, cost efficiency, REAL vs NOMINAL attack cadence,
//!           and the role-signature collision check
//!   duels   pairwise equal-resource win matrix + time to decision
//!   gap     does a fight start at all? casualties vs starting separation
//!   shape   40v40 time-to-decision, army footprint, group-order arrival skew
//!           and naive slot-crossing count
//!   siege   ram/mangonel hits-to-fell for every structure, garrison vs field
//!   perf    ms per combat tick, packed-idle and packed-melee
//!   all     every mode in order

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::*;
use std::time::Instant;

/// The WHOLE roster, always — a hand-written list here silently stops showing
/// new kinds, which is how a diagnostic starts lying.
const KINDS: &[UnitKind] = UnitKind::ALL;

fn f(x: Fx) -> f64 {
    x.to_num::<f64>()
}

fn cost_total(c: &ResourceCost) -> i32 {
    c.wood + c.stone + c.food + c.gold
}

// ── arena ────────────────────────────────────────────────────────────────────

fn arena(seed: u32) -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed });
    app
}

/// The lowest-left `w x h` block of passable, flat-enough ground — every fight
/// happens on the same patch so terrain never explains a result.
fn flat_block(seed: u32, w: i32, h: i32) -> (i32, i32) {
    for cy in 24..(WORLD_SIZE - h - 8) {
        for cx in 24..(WORLD_SIZE - w - 8) {
            let ok = (0..w).all(|dx| (0..h).all(|dy| is_passable(seed, cx + dx, cy + dy)));
            if ok {
                let e0 = elevation_at(seed, Fx::from_num(cx), Fx::from_num(cy));
                let flat = (0..w).step_by(4).all(|dx| {
                    (0..h).step_by(4).all(|dy| {
                        let e = elevation_at(seed, Fx::from_num(cx + dx), Fx::from_num(cy + dy));
                        (e - e0).abs() < fx!("0.04")
                    })
                });
                if flat {
                    return (cx, cy);
                }
            }
        }
    }
    for cy in 24..(WORLD_SIZE - h - 8) {
        for cx in 24..(WORLD_SIZE - w - 8) {
            if (0..w).all(|dx| (0..h).all(|dy| is_passable(seed, cx + dx, cy + dy))) {
                return (cx, cy);
            }
        }
    }
    panic!("no {w}x{h} passable block on seed {seed}");
}

fn tile(x: i32, y: i32) -> V2 {
    V2::new(Fx::from_num(x) + fx!("0.5"), Fx::from_num(y) + fx!("0.5"))
}

fn put(app: &mut App, id: u64, owner: u64, kind: UnitKind, pos: V2) {
    app.world_mut().spawn((
        GameId(id),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: Fx::ZERO },
        Unit::new(kind, pos),
    ));
}

fn put_building(app: &mut App, id: u64, owner: u64, kind: BuildingKind, pos: V2) {
    let def = building_def(kind);
    app.world_mut().spawn((
        GameId(id),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: Fx::ZERO },
        Building::new(kind, def.max_hp, pos),
    ));
}

fn alive(app: &mut App, owner: u64) -> usize {
    let world = app.world_mut();
    let mut q = world.query::<(&Owner, &Unit)>();
    q.iter(world).filter(|(o, _)| o.0 == owner).count()
}

/// Run until one side is wiped out or `max_ticks` elapses. Returns
/// (ticks_used, survivors_a, survivors_b).
fn fight(app: &mut App, max_ticks: usize) -> (usize, usize, usize) {
    for t in 0..max_ticks {
        step(app.world_mut());
        if t.is_multiple_of(20) {
            let (a, b) = (alive(app, 1), alive(app, 2));
            if a == 0 || b == 0 {
                return (t + 1, a, b);
            }
        }
    }
    (max_ticks, alive(app, 1), alive(app, 2))
}

/// A rectangular body of troops: `n` of `kind` in rows `wide` across, growing
/// from (x, y) in the `dy` direction.
struct Body {
    owner: u64,
    kind: UnitKind,
    n: usize,
    x: i32,
    y: i32,
    wide: i32,
    dy: i32,
}

fn block(app: &mut App, next_id: &mut u64, b: Body) {
    for i in 0..b.n {
        let col = i as i32 % b.wide;
        let row = i as i32 / b.wide;
        put(app, *next_id, b.owner, b.kind, tile(b.x + col, b.y + row * b.dy));
        *next_id += 1;
    }
}

// ── roster ───────────────────────────────────────────────────────────────────

/// Replay the cooldown the combat loop actually counts down: an integer number
/// of combat ticks, rounded to the NEAREST tick.
fn real_cadence(rate: Fx) -> (i32, f64) {
    let ticks = ((rate + fx!("0.1")) / COMBAT_DT).to_num::<i32>().max(1);
    (ticks, ticks as f64 * f(COMBAT_DT))
}

/// What the old `Fx` cooldown delivered: subtract COMBAT_DT until it hits zero,
/// which rounds every rate UP to the next whole tick.
fn legacy_cadence(rate: Fx) -> f64 {
    let mut cd = rate;
    for ticks in 1..500 {
        cd = (cd - COMBAT_DT).max(Fx::ZERO);
        if cd <= Fx::ZERO {
            return ticks as f64 * f(COMBAT_DT);
        }
    }
    -1.0
}

fn roster() {
    println!("== ROSTER ==");
    println!(
        "{:<13} {:>4} {:>4} {:>6} {:>6} {:>6} {:>5} {:>7} {:>6} {:>6} {:>7}",
        "unit", "hp", "atk", "dmg_t", "armor", "range", "aggro", "rate", "speed", "cost", "dps/100"
    );
    for &k in KINDS {
        let d = unit_def(k);
        let c = cost_total(&d.cost).max(1);
        let (_, secs) = real_cadence(d.attack_rate);
        let dps = if secs > 0.0 { d.attack as f64 / secs } else { 0.0 };
        println!(
            "{:<13} {:>4} {:>4} {:>6} {:>6} {:>6.1} {:>5.1} {:>7.2} {:>6.2} {:>6} {:>7.2}",
            d.label,
            d.max_hp,
            d.attack,
            format!("{:?}", d.damage_type),
            format!("{:?}", d.armor_class),
            f(d.range),
            f(d.aggro_range),
            f(d.attack_rate),
            f(d.speed),
            c,
            dps * 100.0 / c as f64
        );
    }

    println!("\n== ATTACK CADENCE: declared vs what the cooldown loop delivers ==");
    println!(
        "{:<13} {:>11} {:>7} {:>10} {:>9} {:>12} {:>9}",
        "unit", "attack_rate", "ticks", "real secs", "error", "Fx cooldown", "was"
    );
    let mut worst = 0.0f64;
    let mut worst_old = 0.0f64;
    for &k in KINDS {
        let d = unit_def(k);
        if d.attack <= 0 {
            continue;
        }
        let (t, secs) = real_cadence(d.attack_rate);
        let err = (secs / f(d.attack_rate) - 1.0) * 100.0;
        let old = legacy_cadence(d.attack_rate);
        let old_err = (old / f(d.attack_rate) - 1.0) * 100.0;
        worst = worst.max(err.abs());
        worst_old = worst_old.max(old_err.abs());
        println!(
            "{:<13} {:>11.2} {:>7} {:>10.2} {:>8.1}% {:>12.2} {:>8.1}%",
            d.label,
            f(d.attack_rate),
            t,
            secs,
            err,
            old,
            old_err
        );
    }
    println!("  worst error now {worst:.1}%, was {worst_old:.1}% (0.0% = the rate lands on a combat tick)");
    let off: Vec<&str> = KINDS
        .iter()
        .filter(|&&k| {
            let d = unit_def(k);
            d.attack > 0 && (real_cadence(d.attack_rate).1 - f(d.attack_rate)).abs() > 1e-9
        })
        .map(|&k| unit_def(k).label)
        .collect();
    println!("  rates that are NOT a multiple of COMBAT_DT ({:.1} s): {:?}", f(COMBAT_DT), off);

    println!("\n== ROLE SIGNATURES (two kinds sharing one is a design collision) ==");
    let sig = |k: UnitKind| -> Vec<i64> {
        let d = unit_def(k);
        let mut v = vec![d.role as i64, d.damage_type as i64, d.armor_class as i64];
        v.extend(d.bonus_vs_armor.iter().map(|b| b.to_bits()));
        for flag in [
            d.ranged,
            d.arcs,
            d.brace,
            d.splash > Fx::ZERO,
            d.charge_mult > Fx::ONE,
            d.garrisonable,
            d.prefers_buildings,
            d.morale_aura > Fx::ZERO,
            d.rally_aura > Fx::ZERO,
        ] {
            v.push(flag as i64);
        }
        v
    };
    for (i, &a) in KINDS.iter().enumerate() {
        for &b in &KINDS[i + 1..] {
            if sig(a) == sig(b) {
                println!("  COLLISION {:<13} == {:<13} {:?}", unit_def(a).label, unit_def(b).label, sig(a));
            }
        }
    }
    println!(
        "  (signature = role, damage, armour, bonus, ranged, arcs, brace, splash, charge, garrison, siege, sustain, discipline)"
    );

    println!("\n== FACTION ROSTERS ==");
    for fac in [Faction::Ayyubid, Faction::Crusader] {
        let list: Vec<&str> = faction_roster(fac).iter().map(|&k| unit_def(k).label).collect();
        println!("  {:?} ({}): {}", fac, list.len(), list.join(", "));
    }
    for b in [BuildingKind::Barracks, BuildingKind::Stable, BuildingKind::SiegeWorkshop, BuildingKind::Mosque] {
        for fac in [Faction::Ayyubid, Faction::Crusader] {
            let list: Vec<&str> =
                roster_for(b, fac).iter().map(|&k| unit_def(k).label).collect();
            println!("  {:<14} {:?}: {}", hall_label(b, fac), fac, list.join(", "));
        }
    }
}

// ── duels ────────────────────────────────────────────────────────────────────

fn fighters() -> Vec<UnitKind> {
    KINDS.iter().copied().filter(|&k| unit_def(k).attack > 0).collect()
}

/// One equal-resource duel: both sides get as many bodies as `budget` buys.
fn duel(seed: u32, a: UnitKind, b: UnitKind, budget: i32, max_ticks: usize) -> (usize, usize, usize, usize, usize) {
    let na = (budget / cost_total(&unit_def(a).cost).max(1)).clamp(1, 40) as usize;
    let nb = (budget / cost_total(&unit_def(b).cost).max(1)).clamp(1, 40) as usize;
    let (cx, cy) = flat_block(seed, 24, 32);
    let mut app = arena(seed);
    let mut id = 1u64;
    // both blocks grow AWAY from the contact line, so the front ranks are the
    // same distance apart whatever the two sides' body counts are
    block(&mut app, &mut id, Body { owner: 1, kind: a, n: na, x: cx + 2, y: cy + 14, wide: 8, dy: -1 });
    block(&mut app, &mut id, Body { owner: 2, kind: b, n: nb, x: cx + 2, y: cy + 17, wide: 8, dy: 1 });
    let (ticks, sa, sb) = fight(&mut app, max_ticks);
    (na, nb, sa, sb, ticks)
}

fn duels(seed: u32) {
    let ks = fighters();
    let budget = 1200;
    println!("== EQUAL-RESOURCE DUELS ({budget} resources a side, 240 s cap) ==");
    println!("  row beats column when the row's survivors are the ones left standing");
    print!("{:<13}", "");
    for &b in &ks {
        print!("{:>7}", &unit_def(b).label[..unit_def(b).label.len().min(6)]);
    }
    println!("{:>8}", "record");
    let mut wins = vec![0i32; ks.len()];
    let mut losses = vec![0i32; ks.len()];
    let mut rows: Vec<String> = Vec::new();
    for (i, &a) in ks.iter().enumerate() {
        let mut line = format!("{:<13}", unit_def(a).label);
        for (j, &b) in ks.iter().enumerate() {
            if i == j {
                line.push_str(&format!("{:>7}", "-"));
                continue;
            }
            let (_, _, sa, sb, _) = duel(seed, a, b, budget, 4800);
            if sa > sb {
                wins[i] += 1;
                losses[j] += 1;
            } else if sb > sa {
                losses[i] += 1;
                wins[j] += 1;
            }
            line.push_str(&format!("{:>7}", format!("{sa}-{sb}")));
        }
        rows.push(line);
    }
    for (i, line) in rows.iter().enumerate() {
        println!("{line}{:>8}", format!("{}W{}L", wins[i], losses[i]));
    }
    let dominant: Vec<&str> =
        ks.iter().enumerate().filter(|(i, _)| losses[*i] == 0).map(|(_, &k)| unit_def(k).label).collect();
    let useless: Vec<&str> =
        ks.iter().enumerate().filter(|(i, _)| wins[*i] == 0).map(|(_, &k)| unit_def(k).label).collect();
    println!("\n  never loses: {:?}", dominant);
    println!("  never wins:  {:?}", useless);

    println!("\n== TIME TO DECISION (mirror matches) ==");
    for &k in &ks {
        let (na, _, sa, sb, ticks) = duel(seed, k, k, budget, 4800);
        let decided = sa == 0 || sb == 0;
        println!(
            "  {:<13} {na}v{na}  {:>5.1} s  {}  survivors {sa}/{sb}",
            unit_def(k).label,
            ticks as f64 / 20.0,
            if decided { "DECIDED" } else { "UNRESOLVED" }
        );
    }
}

// ── gap ──────────────────────────────────────────────────────────────────────

fn gap(seed: u32) {
    println!("== DOES A FIGHT EVEN START? 20v20, 240 s, varying separation ==");
    println!("{:<24} {:>5} {:>9} {:>9} {:>9}", "pairing", "gap", "A left", "B left", "contact");
    let pairs = [
        (UnitKind::Spearman, UnitKind::Spearman),
        (UnitKind::Archer, UnitKind::Spearman),
        (UnitKind::Knight, UnitKind::Spearman),
        (UnitKind::Crossbowman, UnitKind::HorseArcher),
    ];
    for (a, b) in pairs {
        for g in [4i32, 6, 7, 8, 12, 20] {
            let (cx, cy) = flat_block(seed, 32, 40);
            let mut app = arena(seed);
            let mut id = 1u64;
            block(&mut app, &mut id, Body { owner: 1, kind: a, n: 20, x: cx + 2, y: cy + 2, wide: 10, dy: 1 });
            block(&mut app, &mut id, Body { owner: 2, kind: b, n: 20, x: cx + 2, y: cy + 4 + g, wide: 10, dy: 1 });
            let (_, sa, sb) = fight(&mut app, 4800);
            println!(
                "{:<24} {:>5} {:>9} {:>9} {:>9}",
                format!("{} v {}", unit_def(a).label, unit_def(b).label),
                g,
                sa,
                sb,
                if sa < 20 || sb < 20 { "yes" } else { "NEVER" }
            );
        }
    }
}

// ── shape ────────────────────────────────────────────────────────────────────

fn seg_cross(p1: (f64, f64), p2: (f64, f64), p3: (f64, f64), p4: (f64, f64)) -> bool {
    let d = |a: (f64, f64), b: (f64, f64), c: (f64, f64)| {
        (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
    };
    let (d1, d2, d3, d4) = (d(p3, p4, p1), d(p3, p4, p2), d(p1, p2, p3), d(p1, p2, p4));
    ((d1 > 0.0) != (d2 > 0.0)) && ((d3 > 0.0) != (d4 > 0.0))
}

fn shape(seed: u32) {
    // 40v40 mirror: does it finish, and what does the field look like after?
    let (cx, cy) = flat_block(seed, 32, 32);
    let mut app = arena(seed);
    let mut id = 1u64;
    block(&mut app, &mut id, Body { owner: 1, kind: UnitKind::Spearman, n: 40, x: cx + 2, y: cy + 2, wide: 10, dy: 1 });
    block(&mut app, &mut id, Body { owner: 2, kind: UnitKind::Spearman, n: 40, x: cx + 2, y: cy + 10, wide: 10, dy: 1 });
    println!("== 40 v 40 MIRROR ==");
    let mut decided_at = None;
    for t in 0..4800usize {
        step(app.world_mut());
        if t.is_multiple_of(200) || t == 4799 {
            let (a, b) = (alive(&mut app, 1), alive(&mut app, 2));
            let world = app.world_mut();
            let mut q = world.query::<(&Owner, &Pos, &Unit)>();
            let (mut targeting, mut idle, mut ma, mut mb, mut na, mut nb) = (0, 0, 0.0f64, 0.0f64, 0, 0);
            let (mut ca, mut cb) = ((0.0f64, 0.0f64), (0.0f64, 0.0f64));
            for (o, p, u) in q.iter(world) {
                if u.attack_target != 0 {
                    targeting += 1;
                } else {
                    idle += 1;
                }
                if o.0 == 1 {
                    ma += f(u.morale);
                    na += 1;
                    ca = (ca.0 + f(p.pos.x), ca.1 + f(p.pos.y));
                } else {
                    mb += f(u.morale);
                    nb += 1;
                    cb = (cb.0 + f(p.pos.x), cb.1 + f(p.pos.y));
                }
            }
            let sep = if na > 0 && nb > 0 {
                let (ax, ay) = (ca.0 / na as f64, ca.1 / na as f64);
                let (bx, by) = (cb.0 / nb as f64, cb.1 / nb as f64);
                ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt()
            } else {
                0.0
            };
            println!(
                "  t={:>5.0}s  {a:>3} v {b:>3}  targeting={targeting:>3} idle={idle:>3}  morale {:.2}/{:.2}  centres {sep:.1} tiles apart",
                t as f64 / 20.0,
                if na > 0 { ma / na as f64 } else { 0.0 },
                if nb > 0 { mb / nb as f64 } else { 0.0 },
            );
            if decided_at.is_none() && (a == 0 || b == 0) {
                decided_at = Some(t);
            }
        }
    }
    match decided_at {
        Some(t) => println!("  DECIDED after {:.1} s", t as f64 / 20.0),
        None => println!("  NEVER DECIDED in 240 s"),
    }

    // how much ground does a 40-man block occupy once it has converged?
    let world = app.world_mut();
    let mut q = world.query::<(&Owner, &Pos)>();
    let pts: Vec<(f64, f64)> = q.iter(world).filter(|(o, _)| o.0 == 1).map(|(_, p)| (f(p.pos.x), f(p.pos.y))).collect();
    if pts.len() > 1 {
        let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for (x, y) in &pts {
            x0 = x0.min(*x);
            x1 = x1.max(*x);
            y0 = y0.min(*y);
            y1 = y1.max(*y);
        }
        println!(
            "  survivors of side A occupy {:.2} x {:.2} tiles = {:.2} tiles^2 for {} men",
            x1 - x0,
            y1 - y0,
            (x1 - x0) * (y1 - y0),
            pts.len()
        );
    }

    // ── the same order, one man at a time and then as a group ───────────────
    println!("\n== GROUP ORDER: 30 men, 30 tiles, open ground ==");
    for mixed in [true, false] {
        println!("  {} kinds", if mixed { "four" } else { "one" });
        for shape in [None, Some(FormationShape::Box), Some(FormationShape::Line)] {
            march_order(seed, 30, 30, shape, false, mixed).report();
        }
    }

    // ── what one click COSTS the tick it lands on ───────────────────────────
    // A wall across the route so the A* is a real one, not a straight line.
    println!("\n== ONE CLICK, 200 MEN, AROUND A WALL ==");
    for shape in [None, Some(FormationShape::Box)] {
        march_order(seed, 200, 30, shape, true, true).report();
    }
}

struct OrderResult {
    men: usize,
    shape: Option<FormationShape>,
    msgs: usize,
    order_ms: f64,
    crossings: usize,
    naive_cross: usize,
    pairs: usize,
    arrived: usize,
    skew: f64,
}

impl OrderResult {
    fn report(&self) {
        let how = match self.shape {
            Some(s) => format!("GroupMove {s:?}"),
            None => "per-unit Move".into(),
        };
        println!(
            "    {:<20} {:>3} men {:>4} msgs  order tick {:>6.2} ms  crossings {:>5} (id-order {:>5}) of {:<5}  {} arrived, skew {:.1} s",
            how, self.men, self.msgs, self.order_ms, self.crossings, self.naive_cross, self.pairs, self.arrived, self.skew
        );
    }
}

/// Issue ONE march order `men` strong over `dist` tiles — as `men` separate
/// `Move` commands when `shape` is None, as a single `GroupMove` otherwise —
/// and measure what the tick it lands on costs, how many of the men cross each
/// other reaching their places, and how far apart they arrive.
fn march_order(
    seed: u32,
    men: usize,
    dist: i32,
    shape: Option<FormationShape>,
    wall: bool,
    mixed: bool,
) -> OrderResult {
    let (cx, cy) = flat_block(seed, 40, dist + 10);
    let mut app = arena(seed);
    let kinds: &[UnitKind] = if mixed {
        &[UnitKind::Spearman, UnitKind::Archer, UnitKind::Knight, UnitKind::Ram]
    } else {
        &[UnitKind::Spearman]
    };
    let wide = if men > 60 { 14 } else { 8 };
    let mut starts: Vec<(u64, (f64, f64))> = Vec::new();
    for i in 0..men {
        let kind = kinds[i % kinds.len()];
        let p = tile(cx + 2 + (i as i32 % wide), cy + 2 + (i as i32 / wide));
        put(&mut app, 1 + i as u64, 1, kind, p);
        starts.push((1 + i as u64, (f(p.x), f(p.y))));
    }
    let rows = (men as i32 + wide - 1) / wide;
    if wall {
        // a solid line with ONE gap, ten tiles ahead of the block: the path has
        // to be found, not guessed
        let wy = cy + rows + 10;
        let mut bid = 100_000u64;
        for dx in 0..40 {
            if dx == 30 {
                continue;
            }
            put_building(&mut app, bid, 2, BuildingKind::Wall, tile(cx - 4 + dx, wy));
            bid += 1;
        }
    }
    let goal = tile(cx + 18, cy + 2 + dist);
    let ids: Vec<u64> = starts.iter().map(|(g, _)| *g).collect();
    let msgs = match shape {
        Some(shape) => {
            app.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::GroupMove {
                player_id: 1,
                units: ids.clone(),
                target: goal,
                formation: shape as u8,
            });
            1
        }
        None => {
            for uid in &ids {
                app.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::Move {
                    player_id: 1,
                    unit: *uid,
                    target: goal,
                });
            }
            ids.len()
        }
    };
    let t0 = Instant::now();
    step(app.world_mut());
    let order_ms = t0.elapsed().as_secs_f64() * 1000.0;

    // where each man was actually SENT: `home` is his own place in the order
    let places: Vec<(u64, (f64, f64))> = {
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &Unit)>();
        q.iter(world).map(|(g, u)| (g.0, (f(u.home.x), f(u.home.y)))).collect()
    };
    let place_of = |id: u64| places.iter().find(|(g, _)| *g == id).map(|(_, p)| *p).unwrap();
    // the same set of places handed out in plain id order — what a formation
    // costs when the men are not sorted the way the slots are
    let mut naive: Vec<(f64, f64)> = places.iter().map(|(_, p)| *p).collect();
    naive.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let (mut crossings, mut pairs, mut naive_cross) = (0usize, 0usize, 0usize);
    for i in 0..starts.len() {
        for j in i + 1..starts.len() {
            let (a, b) = (place_of(starts[i].0), place_of(starts[j].0));
            // two men going to the same spot cannot be said to cross
            if (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6 {
                continue;
            }
            pairs += 1;
            if seg_cross(starts[i].1, a, starts[j].1, b) {
                crossings += 1;
            }
            if seg_cross(starts[i].1, naive[i], starts[j].1, naive[j]) {
                naive_cross += 1;
            }
        }
    }

    let mut arrived: Vec<(u64, usize)> = Vec::new();
    for t in 0..4000 {
        step(app.world_mut());
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &Unit)>();
        for (g, u) in q.iter(world) {
            if !u.has_target && !arrived.iter().any(|(x, _)| *x == g.0) {
                arrived.push((g.0, t));
            }
        }
        if arrived.len() == men {
            break;
        }
    }
    let skew = if arrived.len() >= 2 {
        let first = arrived.iter().map(|(_, t)| *t).min().unwrap();
        let last = arrived.iter().map(|(_, t)| *t).max().unwrap();
        (last - first) as f64 / 20.0
    } else {
        f64::NAN
    };
    OrderResult {
        men,
        shape,
        msgs,
        order_ms,
        crossings,
        naive_cross,
        pairs,
        arrived: arrived.len(),
        skew,
    }
}

// ── siege ────────────────────────────────────────────────────────────────────

fn siege(seed: u32) {
    println!("== HITS TO FELL (a single engine, no upgrades) ==");
    println!("{:<14} {:>7} {:>8} {:>7} {:>10} {:>8} {:>10}", "structure", "hp", "resist", "ram", "ram secs", "mangonel", "mang secs");
    for &bk in BuildingKind::ALL {
        let bd = building_def(bk);
        let row = |uk: UnitKind| -> (i32, f64) {
            let ud = unit_def(uk);
            let atk = Attacker {
                attack: Fx::from_num(ud.attack),
                damage_type: ud.damage_type,
                bonus_vs_armor: ud.bonus_vs_armor,
            };
            let d = building_damage(&atk, bd).max(1);
            let hits = (bd.max_hp + d - 1) / d;
            let (_, secs) = real_cadence(ud.attack_rate);
            (hits, hits as f64 * secs)
        };
        let (rh, rs) = row(UnitKind::Ram);
        let (mh, ms) = row(UnitKind::Mangonel);
        println!(
            "{:<14} {:>7} {:>8.2} {:>7} {:>10.1} {:>8} {:>10.1}",
            bd.label,
            bd.max_hp,
            f(bd.siege_resist),
            rh,
            rs,
            mh,
            ms
        );
    }

    println!("\n== DOES AN ENGINE ENGAGE THE WALL IN FRONT OF IT UNAIDED? ==");
    for uk in [UnitKind::Ram, UnitKind::Mangonel, UnitKind::Spearman] {
        let (cx, cy) = flat_block(seed, 16, 16);
        let mut app = arena(seed);
        for i in 0..5i32 {
            put_building(&mut app, 100 + i as u64, 2, BuildingKind::Wall, tile(cx + 4 + i, cy + 8));
        }
        put(&mut app, 1, 1, uk, tile(cx + 6, cy + 6));
        let mut hit_at = None;
        let full = building_def(BuildingKind::Wall).max_hp;
        for t in 0..2400 {
            step(app.world_mut());
            let world = app.world_mut();
            let mut q = world.query::<&Building>();
            if q.iter(world).any(|b| b.hp < full) {
                hit_at = Some(t);
                break;
            }
        }
        println!(
            "  {:<13} aggro_range={:>4.1}  {}",
            unit_def(uk).label,
            f(unit_def(uk).aggro_range),
            match hit_at {
                Some(t) => format!("first blow at {:.1} s", t as f64 / 20.0),
                None => "NEVER TOUCHED THE WALL in 120 s".into(),
            }
        );
    }

    println!("\n== GARRISON vs THE SAME TROOPS IN THE FIELD ==");
    let host = BuildingKind::Tower;
    let bd = building_def(host);
    println!("  host {} attack={} range={:.1} rate={:.2}", bd.label, bd.attack, f(bd.range), f(bd.attack_rate));
    for uk in [UnitKind::Archer, UnitKind::Crossbowman, UnitKind::Spearman] {
        let ud = unit_def(uk);
        let occ = vec![GarrisonOccupant { attack: ud.attack, ranged: ud.ranged }; 4];
        let power = garrison_fire_power(&occ, bd);
        let host_atk = Attacker {
            attack: Fx::from_num(bd.attack + power),
            damage_type: bd.damage_type,
            bonus_vs_armor: [Fx::ONE; 4],
        };
        let field_atk = Attacker {
            attack: Fx::from_num(ud.attack),
            damage_type: ud.damage_type,
            bonus_vs_armor: ud.bonus_vs_armor,
        };
        let victim = ArmorClass::Mail;
        let tower_hit = effective_damage(&host_atk, victim);
        let field_hit = effective_damage(&field_atk, victim) * 4;
        let (_, tsecs) = real_cadence(if bd.attack_rate > Fx::ZERO { bd.attack_rate } else { ud.attack_rate });
        let (_, fsecs) = real_cadence(ud.attack_rate);
        println!(
            "  4x {:<13} in the tower: {:>4} per volley every {:.1} s = {:>5.1} dps vs Mail | in the field: {:>4} every {:.1} s = {:>5.1} dps ({:?} {:.1}x vs Mail)",
            ud.label,
            tower_hit,
            tsecs,
            tower_hit as f64 / tsecs,
            field_hit,
            fsecs,
            field_hit as f64 / fsecs,
            ud.damage_type,
            f(ud.bonus_vs_armor[ArmorClass::Mail as usize]),
        );
    }
}

// ── perf ─────────────────────────────────────────────────────────────────────

fn perf(seed: u32) {
    println!("== COMBAT TICK COST ==");
    println!("{:<28} {:>7} {:>12} {:>12}", "scenario", "units", "ms/base tick", "ms/combat tk");
    for (label, owners) in [("packed idle (one side)", 1u64), ("packed melee (two sides)", 2u64)] {
        for n in [2000usize, 8000] {
            let (cx, cy) = flat_block(seed, 40, 40);
            let mut app = arena(seed);
            let mut id = 1u64;
            let mut placed = 0;
            'fill: for row in 0..40 {
                for col in 0..40 {
                    for _ in 0..8 {
                        if placed >= n {
                            break 'fill;
                        }
                        let owner = if owners == 1 { 1 } else { 1 + (placed as u64 % 2) };
                        put(&mut app, id, owner, UnitKind::Spearman, tile(cx + col, cy + row));
                        id += 1;
                        placed += 1;
                    }
                }
            }
            for _ in 0..8 {
                step(app.world_mut());
            }
            let t0 = Instant::now();
            const N: usize = 40;
            for _ in 0..N {
                step(app.world_mut());
            }
            let ms = t0.elapsed().as_secs_f64() * 1000.0 / N as f64;
            println!("{:<28} {:>7} {:>12.3} {:>12.3}", label, placed, ms, ms * 4.0);
        }
    }
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "all".into());
    let seed: u32 = std::env::var("TACTICS_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(1);
    println!("tactics_bench mode={mode} seed={seed}\n");
    let run = |m: &str| mode == m || mode == "all";
    if run("roster") {
        roster();
        println!();
    }
    if run("duels") {
        duels(seed);
        println!();
    }
    if run("gap") {
        gap(seed);
        println!();
    }
    if run("shape") {
        shape(seed);
        println!();
    }
    if run("siege") {
        siege(seed);
        println!();
    }
    if run("perf") {
        perf(seed);
        println!();
    }
}
