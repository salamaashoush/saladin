//! THROWAWAY audit harness (AUDIT 1 — combat math). Prints the full unit table,
//! DPS/effective-DPS/cost-efficiency, and runs N-vs-N head-to-heads on a flat
//! land patch reporting survivors, ticks-to-decide and rout counts.
//!
//! cargo run --release -p saladin-protocol --example combat_audit [mode]
//!   mode: table | duels | trace

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::*;

const KINDS: [UnitKind; 10] = [
    UnitKind::Peasant,
    UnitKind::Spearman,
    UnitKind::Archer,
    UnitKind::Knight,
    UnitKind::HorseArcher,
    UnitKind::Mamluk,
    UnitKind::Crossbowman,
    UnitKind::Ram,
    UnitKind::Mangonel,
    UnitKind::Imam,
];

const ARMORS: [ArmorClass; 4] =
    [ArmorClass::Unarmored, ArmorClass::Leather, ArmorClass::Mail, ArmorClass::Stone];

fn f(x: Fx) -> f64 {
    x.to_num::<f64>()
}

fn dmg(kind: UnitKind, armor: ArmorClass) -> i32 {
    let d = unit_def(kind);
    let atk = Attacker {
        attack: Fx::from_num(d.attack),
        damage_type: d.damage_type,
        bonus_vs_armor: d.bonus_vs_armor,
    };
    effective_damage(&atk, armor)
}

/// Attacks per second the combat loop ACTUALLY delivers: cooldown ticks down by
/// COMBAT_DT (0.2 s) each combat tick, so the real cadence is
/// ceil(attack_rate / 0.2) * 0.2 seconds between blows.
fn real_period(rate: Fx) -> f64 {
    let r = f(rate);
    let dt = f(COMBAT_DT);
    if r <= 0.0 {
        return 0.0;
    }
    (r / dt).ceil() * dt
}

fn cost_total(c: &ResourceCost) -> i32 {
    c.wood + c.stone + c.food + c.gold
}

/// Replay the EXACT cooldown loop the sim runs: cd = (cd - DT).max(0); fire when
/// cd <= 0. Reports ticks between blows for the real Fx arithmetic.
fn cadence(rate: Fx) -> (i32, f64) {
    let mut cd = rate;
    let mut ticks = 0;
    loop {
        cd = (cd - COMBAT_DT).max(Fx::ZERO);
        ticks += 1;
        if cd <= Fx::ZERO {
            return (ticks, ticks as f64 * 0.2);
        }
        if ticks > 200 {
            return (ticks, -1.0);
        }
    }
}

fn table() {
    println!("== ATTACK CADENCE: data vs what the cooldown loop actually delivers ==");
    println!("{:<13} {:>10} {:>8} {:>10} {:>8}", "unit", "attack_rate", "ticks", "real secs", "slower");
    for &k in &KINDS {
        let d = unit_def(k);
        if d.attack <= 0 {
            continue;
        }
        let (t, secs) = cadence(d.attack_rate);
        println!(
            "{:<13} {:>10.2} {:>8} {:>10.2} {:>7.1}%",
            d.label,
            f(d.attack_rate),
            t,
            secs,
            (secs / f(d.attack_rate) - 1.0) * 100.0
        );
    }

    println!("\n== WHICH TECHS TOUCH WHICH UNIT (base -> fully upgraded) ==");
    let all: u64 = ALL_TECHS.iter().fold(0u64, |m, t| set_tech(m, *t));
    println!("{:<13} {:>16} {:>16} {:>26}", "unit", "base atk/hp/armor", "full atk/hp/armor", "techs applied");
    for &k in &KINDS {
        let b = unit_def(k);
        let e = effective_unit_def(k, all);
        let applied: Vec<&str> = ALL_TECHS
            .iter()
            .filter(|t| {
                let one = effective_unit_def(k, set_tech(0, **t));
                one.attack != b.attack || one.max_hp != b.max_hp || one.armor_class != b.armor_class
            })
            .map(|t| upgrade_def(*t).label)
            .collect();
        println!(
            "{:<13} {:>16} {:>16} {:>26}",
            b.label,
            format!("{}/{}/{:?}", b.attack, b.max_hp, b.armor_class),
            format!("{}/{}/{:?}", e.attack, e.max_hp, e.armor_class),
            applied.join(",")
        );
    }
    println!("  Mangonel `ranged` flag = {}  (no Shot event, no Fletched Arrows)", unit_def(UnitKind::Mangonel).ranged);

    println!("\n== ROSTER ==");
    println!(
        "{:<13} {:>4} {:>4} {:>6} {:>9} {:>5} {:>5} {:>5} {:>5} {:>6} {:>4} {:>4} {:>4} {:>4} {:>5} {:>16}",
        "unit", "hp", "atk", "dtype", "armor", "rng", "rate", "aggro", "spd", "train", "w", "s", "f", "g", "total", "trained by"
    );
    for &k in &KINDS {
        let d = unit_def(k);
        let trainer = BuildingKind::ALL
            .iter()
            .find(|b| building_def(**b).trains.contains(&k))
            .map(|b| building_def(*b).label)
            .unwrap_or("-");
        println!(
            "{:<13} {:>4} {:>4} {:>6} {:>9} {:>5.1} {:>5.1} {:>5.1} {:>5.1} {:>6.0} {:>4} {:>4} {:>4} {:>4} {:>5} {:>16}",
            d.label,
            d.max_hp,
            d.attack,
            format!("{:?}", d.damage_type),
            format!("{:?}", d.armor_class),
            f(d.range),
            f(d.attack_rate),
            f(d.aggro_range),
            f(d.speed),
            f(d.train_time),
            d.cost.wood,
            d.cost.stone,
            d.cost.food,
            d.cost.gold,
            cost_total(&d.cost),
            trainer
        );
    }

    println!("\n== EFFECTIVE DPS (nominal rate | real quantised rate) ==");
    print!("{:<13}", "unit");
    for a in ARMORS {
        print!(" {:>17}", format!("{:?}", a));
    }
    println!("  {:>8}", "period");
    for &k in &KINDS {
        let d = unit_def(k);
        if d.attack <= 0 {
            continue;
        }
        let p = real_period(d.attack_rate);
        print!("{:<13}", d.label);
        for a in ARMORS {
            let hit = dmg(k, a) as f64;
            print!(" {:>8.2}|{:>8.2}", hit / f(d.attack_rate), hit / p);
        }
        println!("  {:>8.2}", p);
    }

    println!("\n== COST EFFICIENCY (real DPS per 100 res | ehp per 100 res) ==");
    println!(
        "{:<13} {:>7} {:>8} {:>9} {:>9} {:>9} {:>9}",
        "unit", "cost", "ehp*", "dps/100 U", "dps/100 L", "dps/100 M", "ehp/100"
    );
    for &k in &KINDS {
        let d = unit_def(k);
        let c = cost_total(&d.cost).max(1) as f64;
        // effective hp vs a generic Slash 15-attack swing (armour value made concrete)
        let probe = Attacker::new(fx!("15"), DamageType::Slash);
        let taken = effective_damage(&probe, d.armor_class) as f64;
        let ehp = d.max_hp as f64 * (15.0 / taken);
        let p = real_period(d.attack_rate);
        let dps = |a: ArmorClass| if p > 0.0 { dmg(k, a) as f64 / p } else { 0.0 };
        println!(
            "{:<13} {:>7} {:>8.0} {:>9.2} {:>9.2} {:>9.2} {:>9.0}",
            d.label,
            cost_total(&d.cost),
            ehp,
            dps(ArmorClass::Unarmored) * 100.0 / c,
            dps(ArmorClass::Leather) * 100.0 / c,
            dps(ArmorClass::Mail) * 100.0 / c,
            ehp * 100.0 / c
        );
    }

    println!("\n== TIME-TO-KILL matrix (seconds for row to kill column, 1v1, real cadence) ==");
    print!("{:<13}", "atk \\ def");
    for &k in &KINDS {
        print!(" {:>7}", &unit_def(k).label[..unit_def(k).label.len().min(7)]);
    }
    println!();
    for &a in &KINDS {
        let da = unit_def(a);
        if da.attack <= 0 {
            continue;
        }
        print!("{:<13}", da.label);
        for &b in &KINDS {
            let db = unit_def(b);
            let hit = dmg(a, db.armor_class);
            let hits = (db.max_hp as f64 / hit as f64).ceil();
            print!(" {:>7.1}", hits * real_period(da.attack_rate));
        }
        println!();
    }
}

// ── head-to-head ────────────────────────────────────────────────────────────

fn flat_patch(seed: u32) -> (i32, i32) {
    use std::sync::OnceLock;
    static MEMO: OnceLock<(i32, i32)> = OnceLock::new();
    if let Some(p) = MEMO.get() {
        return *p;
    }
    // the flattest 24x14 all-passable block we can find
    let mut best = None;
    let mut best_spread = Fx::MAX;
    for cy in (24..(WORLD_SIZE - 40)).step_by(3) {
        for cx in (24..(WORLD_SIZE - 40)).step_by(3) {
            let ok = (0..24).all(|dx| (0..14).all(|dy| is_passable(seed, cx + dx, cy + dy)));
            if !ok {
                continue;
            }
            let mut lo = Fx::MAX;
            let mut hi = Fx::MIN;
            for dx in 0..24 {
                for dy in 0..14 {
                    let e = elevation_at(seed, Fx::from_num(cx + dx), Fx::from_num(cy + dy));
                    lo = lo.min(e);
                    hi = hi.max(e);
                }
            }
            if hi - lo < best_spread {
                best_spread = hi - lo;
                best = Some((cx, cy));
            }
        }
    }
    let p = best.expect("no passable patch");
    println!("  patch {:?} elevation spread {:.4}", p, f(best_spread));
    let _ = MEMO.set(p);
    p
}

struct Result {
    a_left: i32,
    b_left: i32,
    ticks: u32,
    a_routs: u32,
    b_routs: u32,
    max_rout_a: i32,
    max_rout_b: i32,
}

fn duel(seed: u32, a_kind: UnitKind, na: i32, b_kind: UnitKind, nb: i32, trace: bool) -> Result {
    let (cx, cy) = flat_patch(seed);
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed });

    let mut id = 1u64;
    let spawn = |w: &mut bevy_ecs::world::World, kind: UnitKind, owner: u64, x: Fx, y: Fx, gid: u64| {
        let d = unit_def(kind);
        w.spawn((
            GameId(gid),
            Owner(owner),
            MatchId(1),
            Pos { pos: V2::new(x, y), facing: ZERO },
            Unit {
                speed: d.speed,
                hp: d.max_hp,
                ..Unit::new(kind, V2::new(x, y))
            },
        ));
    };

    // two lines facing each other. GAP tiles between the front ranks.
    let gap: i32 = std::env::var("GAP").ok().and_then(|s| s.parse().ok()).unwrap_or(6);
    let per_row = 10;
    let a_rows = (na + per_row - 1) / per_row;
    for i in 0..na {
        let x = Fx::from_num(cx + 2 + (i % per_row));
        let y = Fx::from_num(cy + 2 + (i / per_row));
        spawn(app.world_mut(), a_kind, 1, x, y, id);
        id += 1;
    }
    let b_y0 = cy + 2 + a_rows - 1 + gap;
    for i in 0..nb {
        let x = Fx::from_num(cx + 2 + (i % per_row));
        let y = Fx::from_num(b_y0 + (i / per_row));
        spawn(app.world_mut(), b_kind, 2, x, y, id);
        id += 1;
    }

    let mut ticks = 0u32;
    let (mut a_routs, mut b_routs) = (0u32, 0u32);
    let (mut max_ra, mut max_rb) = (0i32, 0i32);
    let mut seen_routing: std::collections::HashSet<u64> = std::collections::HashSet::new();
    loop {
        step(app.world_mut());
        ticks += 1;
        let w = app.world_mut();
        let (mut a, mut b, mut ra, mut rb) = (0, 0, 0, 0);
        {
            let mut q = w.query::<(&GameId, &Owner, &Unit)>();
            for (g, o, u) in q.iter(w) {
                if o.0 == 1 {
                    a += 1;
                    if u.routing {
                        ra += 1;
                        if seen_routing.insert(g.0) {
                            a_routs += 1;
                        }
                    }
                } else {
                    b += 1;
                    if u.routing {
                        rb += 1;
                        if seen_routing.insert(g.0) {
                            b_routs += 1;
                        }
                    }
                }
            }
        }
        max_ra = max_ra.max(ra);
        max_rb = max_rb.max(rb);
        if trace && ticks % 20 == 0 {
            // engagement shape: who has a target, who is walking, focus-fire depth
            let w = app.world_mut();
            let (mut targeted, mut moving, mut idle) = (0, 0, 0);
            let mut focus: std::collections::HashMap<u64, i32> = std::collections::HashMap::new();
            let mut sumx = 0.0;
            let mut sumy = 0.0;
            let mut cnt = 0.0;
            {
                let mut q = w.query::<(&Owner, &Pos, &Unit)>();
                for (o, p, u) in q.iter(w) {
                    if u.attack_target != 0 {
                        targeted += 1;
                        *focus.entry(u.attack_target).or_default() += 1;
                    } else if u.has_target {
                        moving += 1;
                    } else {
                        idle += 1;
                    }
                    if o.0 == 1 {
                        sumx += f(p.pos.x);
                        sumy += f(p.pos.y);
                        cnt += 1.0;
                    }
                }
            }
            let maxfocus = focus.values().copied().max().unwrap_or(0);
            let uniq = focus.len();
            // closest A-to-B pair, and mean morale per side
            let (mut pa, mut pb) = (Vec::new(), Vec::new());
            let (mut ma, mut mb) = (0.0, 0.0);
            {
                let mut q = w.query::<(&Owner, &Pos, &Unit)>();
                for (o, p, u) in q.iter(w) {
                    if o.0 == 1 {
                        pa.push(p.pos);
                        ma += f(u.morale);
                    } else {
                        pb.push(p.pos);
                        mb += f(u.morale);
                    }
                }
            }
            let mut mind = f64::MAX;
            for x in &pa {
                for y in &pb {
                    mind = mind.min(f(dist(*x, *y)));
                }
            }
            println!(
                "  t={:>4}s a={:<3} b={:<3} rout a/b={}/{} targeting={} moving={} idle={} tgts={} max_on_one={} minAB={:.1} morale a/b={:.2}/{:.2} Ac=({:.1},{:.1})",
                ticks / 20, a, b, ra, rb, targeted, moving, idle, uniq, maxfocus,
                if mind == f64::MAX { -1.0 } else { mind },
                if !pa.is_empty() { ma / pa.len() as f64 } else { 0.0 },
                if !pb.is_empty() { mb / pb.len() as f64 } else { 0.0 },
                if cnt > 0.0 { sumx / cnt } else { 0.0 },
                if cnt > 0.0 { sumy / cnt } else { 0.0 }
            );
        }
        if a == 0 || b == 0 || ticks > 20 * 240 {
            return Result {
                a_left: a,
                b_left: b,
                ticks,
                a_routs,
                b_routs,
                max_rout_a: max_ra,
                max_rout_b: max_rb,
            };
        }
    }
}

fn duels(seed: u32) {
    let fighters = [
        UnitKind::Spearman,
        UnitKind::Archer,
        UnitKind::Knight,
        UnitKind::HorseArcher,
        UnitKind::Mamluk,
        UnitKind::Crossbowman,
    ];
    println!("\n== 20 v 20 EQUAL-COUNT (survivors, seconds, unique routs) ==");
    println!("{:<13} {:<13} {:>8} {:>8} {:>7} {:>10}", "A", "B", "A left", "B left", "secs", "routs A/B");
    for (i, &a) in fighters.iter().enumerate() {
        for &b in fighters.iter().skip(i + 1) {
            let r = duel(7, a, 20, b, 20, false);
            println!(
                "{:<13} {:<13} {:>8} {:>8} {:>7.0} {:>10}",
                unit_def(a).label,
                unit_def(b).label,
                r.a_left,
                r.b_left,
                r.ticks as f64 / 20.0,
                format!("{}/{}", r.a_routs, r.b_routs)
            );
        }
    }

    println!("\n== EQUAL-RESOURCE (~1800 total each side) ==");
    println!("{:<13} {:<13} {:>7} {:>7} {:>8} {:>8} {:>7}", "A", "B", "nA", "nB", "A left", "B left", "secs");
    for (i, &a) in fighters.iter().enumerate() {
        for &b in fighters.iter().skip(i + 1) {
            let ca = cost_total(&unit_def(a).cost).max(1);
            let cb = cost_total(&unit_def(b).cost).max(1);
            let na = (1800 / ca).clamp(1, 60);
            let nb = (1800 / cb).clamp(1, 60);
            let r = duel(7, a, na, b, nb, false);
            println!(
                "{:<13} {:<13} {:>7} {:>7} {:>8} {:>8} {:>7.0}",
                unit_def(a).label,
                unit_def(b).label,
                na,
                nb,
                r.a_left,
                r.b_left,
                r.ticks as f64 / 20.0
            );
        }
    }
    let _ = seed;
}

fn trace() {
    println!("\n== 40 v 40 Spearman mirror (line-vs-line behaviour) ==");
    let r = duel(7, UnitKind::Spearman, 40, UnitKind::Spearman, 40, true);
    println!(
        "  result a={} b={} secs={:.0} unique routs a/b={}/{} peak routing a/b={}/{}",
        r.a_left,
        r.b_left,
        r.ticks as f64 / 20.0,
        r.a_routs,
        r.b_routs,
        r.max_rout_a,
        r.max_rout_b
    );

    println!("\n== 40 Knights vs 40 Spearmen ==");
    let r = duel(7, UnitKind::Knight, 40, UnitKind::Spearman, 40, true);
    println!(
        "  result a={} b={} secs={:.0} unique routs a/b={}/{} peak routing a/b={}/{}",
        r.a_left,
        r.b_left,
        r.ticks as f64 / 20.0,
        r.a_routs,
        r.b_routs,
        r.max_rout_a,
        r.max_rout_b
    );

    println!("\n== 40 Archers vs 40 Knights ==");
    let r = duel(7, UnitKind::Archer, 40, UnitKind::Knight, 40, true);
    println!(
        "  result a={} b={} secs={:.0} unique routs a/b={}/{} peak routing a/b={}/{}",
        r.a_left,
        r.b_left,
        r.ticks as f64 / 20.0,
        r.a_routs,
        r.b_routs,
        r.max_rout_a,
        r.max_rout_b
    );
}

/// Cost of the morale-support building scan: identical armies, N buildings.
fn perf_buildings() {
    use std::time::Instant;
    for nb in [0usize, 50, 200, 400] {
        let seed = 1u32;
        let mut app = App::new();
        app.add_plugins(SimPlugin);
        app.finish();
        app.cleanup();
        app.world_mut().insert_resource(WorldConfig { seed });
        let (cx, cy) = flat_patch(seed);
        let mut id = 1u64;
        // 4000 soldiers that are hurt (so the morale pass runs for all of them)
        let one_team = std::env::var("ONE_TEAM").is_ok();
        for i in 0..4000i32 {
            let owner = if one_team { 1 } else { 1 + (i % 2) as u64 };
            let x = Fx::from_num(cx) + Fx::from_num(i % 20) / Fx::from_num(4);
            let y = Fx::from_num(cy) + Fx::from_num(i / 20) / Fx::from_num(4);
            let d = unit_def(UnitKind::Spearman);
            app.world_mut().spawn((
                GameId(id),
                Owner(owner),
                MatchId(1),
                Pos { pos: V2::new(x, y), facing: ZERO },
                Unit {
                    speed: d.speed,
                    hp: d.max_hp,
                    stance: Stance::HoldGround,
                    morale: fx!("0.9"),
                    ..Unit::new(UnitKind::Spearman, V2::new(x, y))
                },
            ));
            id += 1;
        }
        // buildings scattered far away (no morale_radius hit -> full scan)
        for i in 0..nb {
            let x = Fx::from_num(20 + (i % 20) as i32 * 3);
            let y = Fx::from_num(20 + (i / 20) as i32 * 3);
            app.world_mut().spawn((
                GameId(id),
                Owner(1),
                MatchId(1),
                Pos { pos: V2::new(x, y), facing: ZERO },
                Building::new(BuildingKind::House, 250, V2::new(x, y)),
            ));
            id += 1;
        }
        // warm up, then time 20 combat ticks (every 4th base tick)
        for _ in 0..8 {
            step(app.world_mut());
        }
        let t0 = Instant::now();
        for _ in 0..80 {
            step(app.world_mut());
        }
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        println!("  buildings={:>4}  80 ticks (20 combat) in {:>8.1} ms  ({:.2} ms/combat tick)", nb, ms, ms / 20.0);
    }
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "table".into());
    match mode.as_str() {
        "duels" => duels(7),
        "trace" => trace(),
        "perf" => perf_buildings(),
        "all" => {
            table();
            duels(7);
            trace();
        }
        _ => table(),
    }
}
