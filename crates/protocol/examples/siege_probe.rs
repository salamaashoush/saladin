//! Throwaway measurement harness for the SIEGE audit. Not a test.
//! `cargo run --release -p saladin-protocol --example siege_probe`

use bevy_app::prelude::*;
use bevy_ecs::prelude::Entity;
use saladin_protocol::*;
use saladin_sim::*;

const SEED: u32 = 1;

fn build_app() -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed: SEED });
    app
}

fn spawn_player(app: &mut App, id: u64) {
    app.world_mut().spawn((
        GameId(9000 + id),
        MatchId(1),
        Player {
            player_id: id,
            name: "P".into(),
            faction: Faction::Ayyubid,
            stock: Stockpile { wood: 9000, stone: 9000, food: 9000, gold: 9000 },
            color: 0,
            online: true,
            keep: 0,
            defeated: false,
            slot: id as u8,
            tech_mask: 0,
            hunger: 0,
        },
    ));
}

fn spawn_building(app: &mut App, id: u64, owner: u64, kind: BuildingKind, pos: V2) {
    let def = building_def(kind);
    app.world_mut().spawn((
        GameId(id),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Building::new(kind, def.max_hp, pos),
    ));
}

fn spawn_unit(app: &mut App, id: u64, owner: u64, kind: UnitKind, pos: V2) {
    let def = unit_def(kind);
    app.world_mut().spawn((
        GameId(id),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Unit {
            speed: def.speed,
            hp: def.max_hp,
            ..Unit::new(kind, pos)
        },
    ));
}

fn bhp(app: &mut App, id: u64) -> Option<i32> {
    let w = app.world_mut();
    let mut q = w.query::<(&GameId, &Building)>();
    q.iter(w).find(|(g, _)| g.0 == id).map(|(_, b)| b.hp)
}

fn uhp(app: &mut App, id: u64) -> Option<i32> {
    let w = app.world_mut();
    let mut q = w.query::<(&GameId, &Unit)>();
    q.iter(w).find(|(g, _)| g.0 == id).map(|(_, u)| u.hp)
}

fn upos(app: &mut App, id: u64) -> Option<V2> {
    let w = app.world_mut();
    let mut q = w.query::<(&GameId, &Pos, &Unit)>();
    q.iter(w).find(|(g, _, _)| g.0 == id).map(|(_, p, _)| p.pos)
}

fn umorale(app: &mut App, id: u64) -> Option<(Fx, bool)> {
    let w = app.world_mut();
    let mut q = w.query::<(&GameId, &Unit)>();
    q.iter(w).find(|(g, _)| g.0 == id).map(|(_, u)| (u.morale, u.routing))
}

fn alive(app: &mut App, owner: u64) -> usize {
    let w = app.world_mut();
    let mut q = w.query::<(&Owner, &Unit)>();
    q.iter(w).filter(|(o, u)| o.0 == owner && u.garrisoned_in == 0).count()
}

fn set_attack(app: &mut App, id: u64, target: u64) {
    let w = app.world_mut();
    let mut q = w.query::<(&GameId, &mut Unit)>();
    if let Some((_, mut u)) = q.iter_mut(w).find(|(g, _)| g.0 == id) {
        u.attack_target = target;
    }
}

fn cmd(app: &mut App, c: PlayerCommand) {
    app.world_mut().resource_mut::<CommandQueue>().0.push(c);
}

/// A block of `n`x`n` tiles that are all buildable AND passable.
fn find_flat_block(n: i32) -> (i32, i32) {
    for cy in 20..300 {
        for cx in 20..300 {
            if (0..n).all(|dx| {
                (0..n).all(|dy| {
                    is_passable(SEED, cx + dx, cy + dy) && is_buildable_tile(SEED, cx + dx, cy + dy)
                })
            }) {
                return (cx, cy);
            }
        }
    }
    panic!("no flat block");
}

fn c(t: i32) -> Fx {
    Fx::from_num(t) + fx!("0.5")
}

fn secs(ticks: i32) -> f64 {
    ticks as f64 * 0.05
}

// ── PART 1: the static table ────────────────────────────────────────────────

fn part1() {
    println!("=== PART 1: hits and seconds to fell each structure ===\n");
    let engines = [UnitKind::Ram, UnitKind::Mangonel];
    println!(
        "{:<15} {:>5} {:>6} {:>6} | {:>5} {:>6} | {:>5} {:>6} | {:>5} {:>6}",
        "structure", "hp", "sres", "armor", "ram/h", "ram s", "man/h", "man s", "M+ram", "M+man"
    );
    let masonry = set_tech(0, Tech::Masonry);
    for &k in BuildingKind::ALL {
        let d = building_def(k);
        let mut cells: Vec<String> = Vec::new();
        for &e in &engines {
            let ed = unit_def(e);
            let atk = Attacker {
                attack: Fx::from_num(ed.attack),
                damage_type: ed.damage_type,
                bonus_vs_armor: ed.bonus_vs_armor,
            };
            let per = building_damage(&atk, d);
            let hits = (d.max_hp + per - 1) / per;
            let t = (hits - 1) as f64 * ed.attack_rate.to_num::<f64>();
            cells.push(format!("{hits:>5} {t:>6.1}"));
        }
        let mut m_cells: Vec<String> = Vec::new();
        for &e in &engines {
            let ed = unit_def(e);
            let ed_m = effective_building_def(k, masonry);
            let atk = Attacker {
                attack: Fx::from_num(ed.attack),
                damage_type: ed.damage_type,
                bonus_vs_armor: ed.bonus_vs_armor,
            };
            let per = building_damage(&atk, &ed_m);
            let hits = (ed_m.max_hp + per - 1) / per;
            m_cells.push(format!("{hits:>5}"));
        }
        println!(
            "{:<15} {:>5} {:>6.2} {:>6} | {} | {} | {:>5} {:>6}",
            d.label,
            d.max_hp,
            d.siege_resist.to_num::<f64>(),
            format!("{:?}", d.armor_class),
            cells[0],
            cells[1],
            m_cells[0],
            m_cells[1],
        );
    }

    println!("\n-- what a NON-siege unit does to a Wall (420 hp, Stone) --");
    for &u in UnitKind::ALL {
        let d = unit_def(u);
        if d.attack <= 0 {
            continue;
        }
        let atk = Attacker {
            attack: Fx::from_num(d.attack),
            damage_type: d.damage_type,
            bonus_vs_armor: d.bonus_vs_armor,
        };
        let wall = building_def(BuildingKind::Wall);
        let per = building_damage(&atk, wall);
        let hits = (wall.max_hp + per - 1) / per;
        let t = (hits - 1) as f64 * d.attack_rate.to_num::<f64>();
        println!(
            "  {:<14} {:>3} dmg/hit  {:>4} hits  {:>6.1}s alone   ({:>5.1}s with 10 of them)",
            d.label,
            per,
            hits,
            t,
            t / 10.0
        );
    }

    println!("\n-- cost of the breach --");
    let ram = unit_def(UnitKind::Ram);
    let man = unit_def(UnitKind::Mangonel);
    let wall = building_def(BuildingKind::Wall);
    println!(
        "  Ram {}w vs Wall {}w+{}s -> one ram pays for itself after {} segments",
        ram.cost.wood,
        wall.cost.wood,
        wall.cost.stone,
        ram.cost.wood / wall.cost.wood.max(1)
    );
    println!(
        "  Mangonel {}w+{}g, range {} vs Tower range {} / Watchtower range {}",
        man.cost.wood,
        man.cost.gold,
        man.range.to_num::<f64>(),
        building_def(BuildingKind::Tower).range.to_num::<f64>(),
        building_def(BuildingKind::Watchtower).range.to_num::<f64>()
    );
    println!(
        "  Ram aggro_range = {} (it NEVER auto-acquires), Mangonel aggro_range = {}",
        ram.aggro_range.to_num::<f64>(),
        man.aggro_range.to_num::<f64>()
    );
}

// ── PART 2: a real ram against a real wall ──────────────────────────────────

fn part2() {
    println!("\n=== PART 2: a live siege engine against a live wall ===\n");
    let (bx, by) = find_flat_block(12);
    for (label, engine, stand) in [("Ram", UnitKind::Ram, 1), ("Mangonel", UnitKind::Mangonel, 6)] {
        let mut app = build_app();
        spawn_player(&mut app, 1);
        spawn_player(&mut app, 2);
        spawn_building(&mut app, 100, 2, BuildingKind::Wall, V2::new(c(bx + 5), c(by + 5)));
        spawn_unit(&mut app, 10, 1, engine, V2::new(c(bx + 5), c(by + 5 - stand)));
        set_attack(&mut app, 10, 100);
        let mut t = 0;
        while bhp(&mut app, 100).is_some() && t < 6000 {
            step(app.world_mut());
            t += 1;
        }
        println!(
            "  1 {label} vs 1 Wall segment: {} ticks = {:.1}s",
            t,
            secs(t)
        );

        // and against the Keep
        let mut app = build_app();
        spawn_player(&mut app, 1);
        spawn_player(&mut app, 2);
        spawn_building(&mut app, 100, 2, BuildingKind::Keep, V2::new(c(bx + 5), c(by + 5)));
        spawn_unit(&mut app, 10, 1, engine, V2::new(c(bx + 5), c(by + 5 - stand - 1)));
        set_attack(&mut app, 10, 100);
        let mut t = 0;
        while bhp(&mut app, 100).is_some() && t < 20000 {
            step(app.world_mut());
            t += 1;
        }
        let hp_left = bhp(&mut app, 100);
        println!(
            "  1 {label} vs the Keep (1500 hp, and it SHOOTS BACK at range 8): {} ticks = {:.1}s, engine hp {:?}, keep hp {:?}",
            t,
            secs(t),
            uhp(&mut app, 10),
            hp_left
        );
    }

    // 10 spearmen chewing a wall with no siege at all
    let mut app = build_app();
    spawn_player(&mut app, 1);
    spawn_player(&mut app, 2);
    spawn_building(&mut app, 100, 2, BuildingKind::Wall, V2::new(c(bx + 5), c(by + 5)));
    for i in 0..10 {
        spawn_unit(
            &mut app,
            10 + i as u64,
            1,
            UnitKind::Spearman,
            V2::new(c(bx + 2 + i % 4), c(by + 3)),
        );
        set_attack(&mut app, 10 + i as u64, 100);
    }
    let mut t = 0;
    while bhp(&mut app, 100).is_some() && t < 20000 {
        step(app.world_mut());
        t += 1;
    }
    println!("  10 Spearmen vs 1 Wall segment (no siege): {} ticks = {:.1}s", t, secs(t));
}

// ── PART 3: does a wall stop anybody? ───────────────────────────────────────

/// A solid `r`-radius square ring of Wall around (cx, cy), one tile optionally
/// swapped for a Gatehouse on the south face. Returns the ids placed.
fn box_walls(app: &mut App, owner: u64, cx: i32, cy: i32, r: i32, gate: bool) {
    let mut id = 200;
    for dx in -r..=r {
        for dy in -r..=r {
            if dx.abs() != r && dy.abs() != r {
                continue;
            }
            let kind = if gate && dx == 0 && dy == -r {
                BuildingKind::Gatehouse
            } else {
                BuildingKind::Wall
            };
            spawn_building(app, id, owner, kind, V2::new(c(cx + dx), c(cy + dy)));
            id += 1;
        }
    }
}

fn part3() {
    println!("\n=== PART 3: can an attacker get through a wall line? ===\n");
    let (bx, by) = find_flat_block(24);
    let cx = bx + 12;
    let cy = by + 12;
    let r = 4; // 9x9 ring: the interior is 4 tiles deep, well past melee reach

    // --- 3a: a target sealed deep inside a solid ring, attacker ordered to kill
    let mut app = build_app();
    spawn_player(&mut app, 1);
    spawn_player(&mut app, 2);
    box_walls(&mut app, 2, cx, cy, r, false);
    spawn_unit(&mut app, 300, 2, UnitKind::Peasant, V2::new(c(cx), c(cy)));
    spawn_unit(&mut app, 400, 1, UnitKind::Spearman, V2::new(c(cx), c(cy - r - 4)));
    set_attack(&mut app, 400, 300);
    let mut t = 0;
    while uhp(&mut app, 300).is_some() && t < 4000 {
        step(app.world_mut());
        t += 1;
    }
    let p = upos(&mut app, 400);
    let inside = p
        .map(|p| {
            (p.x.to_num::<f64>() - c(cx).to_num::<f64>()).abs() < r as f64 - 0.5
                && (p.y.to_num::<f64>() - c(cy).to_num::<f64>()).abs() < r as f64 - 0.5
        })
        .unwrap_or(false);
    println!(
        "  3a. peasant sealed in a SOLID 9x9 wall ring, one Spearman ordered to kill it:\n      after {:.1}s peasant alive={}  attacker at {:?}  INSIDE THE RING={}",
        secs(t),
        uhp(&mut app, 300).is_some(),
        p.map(|p| (p.x.to_num::<f32>(), p.y.to_num::<f32>())),
        inside
    );
    let walls_left = {
        let w = app.world_mut();
        let mut q = w.query::<&Building>();
        q.iter(w).count()
    };
    println!("      walls still standing: {walls_left} (the ring is 32 segments)");

    // --- 3b: does an explicit MOVE order respect the ring?
    let mut app = build_app();
    spawn_player(&mut app, 1);
    spawn_player(&mut app, 2);
    box_walls(&mut app, 2, cx, cy, r, false);
    spawn_unit(&mut app, 400, 1, UnitKind::Spearman, V2::new(c(cx), c(cy - r - 4)));
    cmd(&mut app, PlayerCommand::Move { player_id: 1, unit: 400, target: V2::new(c(cx), c(cy)) });
    for _ in 0..900 {
        step(app.world_mut());
    }
    let p = upos(&mut app, 400).unwrap();
    println!(
        "  3b. same ring, explicit MOVE order into the middle: 45s later at {:?} (start y={:.1})",
        (p.x.to_num::<f32>(), p.y.to_num::<f32>()),
        c(cy - r - 4).to_num::<f64>()
    );

    // --- 3c: a BREACH. Knock one segment out, does anyone find the hole?
    let mut app = build_app();
    spawn_player(&mut app, 1);
    spawn_player(&mut app, 2);
    box_walls(&mut app, 2, cx, cy, r, false);
    // remove the middle of the south face
    {
        let w = app.world_mut();
        let hole = tile_key(cx, cy - r);
        let mut q = w.query::<(Entity, &Pos, &Building)>();
        let victim: Vec<Entity> = q
            .iter(w)
            .filter(|(_, p, _)| {
                tile_key(p.pos.x.to_num::<i32>(), p.pos.y.to_num::<i32>()) == hole
            })
            .map(|(e, _, _)| e)
            .collect();
        for e in victim {
            w.despawn(e);
        }
    }
    spawn_unit(&mut app, 300, 2, UnitKind::Peasant, V2::new(c(cx), c(cy)));
    spawn_unit(&mut app, 400, 1, UnitKind::Spearman, V2::new(c(cx + 3), c(cy - r - 4)));
    cmd(&mut app, PlayerCommand::Move { player_id: 1, unit: 400, target: V2::new(c(cx), c(cy)) });
    for _ in 0..900 {
        step(app.world_mut());
    }
    let p = upos(&mut app, 400).unwrap();
    println!(
        "  3c. ONE segment removed from the south face (a breach), MOVE order to the middle:\n      45s later at {:?} -> the pathfinder {} the hole",
        (p.x.to_num::<f32>(), p.y.to_num::<f32>()),
        if (p.y.to_num::<f64>() - c(cy).to_num::<f64>()).abs() < 1.0 { "FOUND" } else { "MISSED" }
    );

    // --- 3d: the gatehouse, owner vs enemy, each probed alone
    for (who, owner) in [("the wall owner", 2u64), ("the enemy", 1u64)] {
        let mut app = build_app();
        spawn_player(&mut app, 1);
        spawn_player(&mut app, 2);
        box_walls(&mut app, 2, cx, cy, r, true);
        spawn_unit(&mut app, 400, owner, UnitKind::Spearman, V2::new(c(cx), c(cy - r - 4)));
        cmd(&mut app, PlayerCommand::Move { player_id: owner, unit: 400, target: V2::new(c(cx), c(cy)) });
        for _ in 0..900 {
            step(app.world_mut());
        }
        let p = upos(&mut app, 400).unwrap();
        let inside = (p.y.to_num::<f64>() - c(cy).to_num::<f64>()).abs() < 1.0;
        println!(
            "  3d. ring WITH a gatehouse in the south face, MOVE order to the middle by {who}: at {:?} inside={inside}",
            (p.x.to_num::<f32>(), p.y.to_num::<f32>())
        );
    }

    // --- 3e: the same gate, but with an ATTACK order (the combat pursuit path)
    let mut app = build_app();
    spawn_player(&mut app, 1);
    spawn_player(&mut app, 2);
    box_walls(&mut app, 2, cx, cy, r, true);
    spawn_unit(&mut app, 300, 2, UnitKind::Peasant, V2::new(c(cx), c(cy)));
    spawn_unit(&mut app, 400, 1, UnitKind::Spearman, V2::new(c(cx), c(cy - r - 4)));
    set_attack(&mut app, 400, 300);
    let mut t = 0;
    while uhp(&mut app, 300).is_some() && t < 4000 {
        step(app.world_mut());
        t += 1;
    }
    let p = upos(&mut app, 400);
    println!(
        "  3e. same gated ring, ATTACK order on the peasant inside: peasant alive={} after {:.1}s, attacker at {:?}",
        uhp(&mut app, 300).is_some(),
        secs(t),
        p.map(|p| (p.x.to_num::<f32>(), p.y.to_num::<f32>()))
    );

    // --- 3f: can a defender shoot OVER its own wall? does a wall block fire?
    let mut app = build_app();
    spawn_player(&mut app, 1);
    spawn_player(&mut app, 2);
    // a single wall segment between an archer and a spearman, 2 tiles apart
    spawn_building(&mut app, 100, 2, BuildingKind::Wall, V2::new(c(cx), c(cy)));
    spawn_unit(&mut app, 500, 2, UnitKind::Archer, V2::new(c(cx), c(cy + 1)));
    spawn_unit(&mut app, 600, 1, UnitKind::Spearman, V2::new(c(cx), c(cy - 1)));
    for _ in 0..200 {
        step(app.world_mut());
    }
    println!(
        "  3f. archer and spearman on opposite sides of ONE wall segment, 2 tiles apart:\n      archer hp {:?}, spearman hp {:?} after 10s (line of sight is not modelled)",
        uhp(&mut app, 500),
        uhp(&mut app, 600)
    );
}

fn dps(host: BuildingKind, garrison: &[UnitKind], target: UnitKind) -> (f64, i32, f64, f64) {
    let bdef = building_def(host);
    let occ: Vec<GarrisonOccupant> = garrison
        .iter()
        .map(|k| {
            let d = unit_def(*k);
            GarrisonOccupant { attack: d.attack, ranged: d.ranged }
        })
        .collect();
    let gfire = garrison_fire_power(&occ, bdef);
    let total = bdef.attack + gfire;
    let (range, rate) = if bdef.attack > 0 {
        (bdef.range, bdef.attack_rate)
    } else {
        let mut r = Fx::ZERO;
        let mut rr = Fx::MAX;
        for k in garrison {
            let d = unit_def(*k);
            if d.ranged && d.attack > 0 {
                r = r.max(d.range);
                rr = rr.min(d.attack_rate);
            }
        }
        (r, rr)
    };
    let atk = Attacker::new(Fx::from_num(total), bdef.damage_type);
    let dmg = effective_damage(&atk, unit_def(target).armor_class);
    let rate_f = rate.to_num::<f64>().max(0.0001);
    (dmg as f64 / rate_f, dmg, range.to_num::<f64>(), rate_f)
}

fn part4() {
    println!("\n=== PART 4: what defending is worth ===\n");
    let target = UnitKind::Spearman;
    let tdef = unit_def(target);
    println!("  (target = Spearman: {} hp, {:?} armor)\n", tdef.max_hp, tdef.armor_class);

    println!("  -- garrisoned volleys (ONE target, summed attack, one hit) --");
    for (host, n) in [
        (BuildingKind::Wall, 2),
        (BuildingKind::Tower, 5),
        (BuildingKind::Watchtower, 8),
        (BuildingKind::Keep, 10),
        (BuildingKind::Gatehouse, 3),
        (BuildingKind::House, 3),
    ] {
        let g: Vec<UnitKind> = (0..n).map(|_| UnitKind::Archer).collect();
        let (d, per, range, rate) = dps(host, &g, target);
        let empty = dps(host, &[], target);
        let frac = per as f64 / tdef.max_hp as f64;
        let morale_drop = frac * 1.5;
        println!(
            "  {:<12} empty {:>3} dmg/{:.1}s = {:>5.1} dps | +{} archers {:>4} dmg/{:.1}s = {:>5.1} dps  range {:.0}  one volley takes {:.0}% hp and {:.0}% morale{}",
            building_def(host).label,
            empty.1,
            empty.3,
            empty.0,
            n,
            per,
            rate,
            d,
            range,
            frac * 100.0,
            morale_drop * 100.0,
            if morale_drop >= 0.75 { "  <- ONE VOLLEY ROUTS" } else { "" }
        );
    }

    println!("\n  -- the same archers standing in the open --");
    let a = unit_def(UnitKind::Archer);
    let atk = Attacker { attack: Fx::from_num(a.attack), damage_type: a.damage_type, bonus_vs_armor: a.bonus_vs_armor };
    let one = effective_damage(&atk, tdef.armor_class);
    let one_dps = one as f64 / a.attack_rate.to_num::<f64>();
    for n in [2, 5, 8, 10] {
        println!(
            "  {n:>2} archers in the field: {:>5.1} dps ({} dmg each per {:.1}s), range {:.0}, and they can be killed",
            one_dps * n as f64,
            one,
            a.attack_rate.to_num::<f64>(),
            a.range.to_num::<f64>()
        );
    }

    println!("\n  -- elevation --");
    println!(
        "  elevation_range_bonus is RANGE ONLY, +-{:.0}% max, no damage/accuracy term",
        ELEV_BONUS_MAX.to_num::<f64>() * 100.0
    );
    println!(
        "  a Watchtower on the high ground reaches {:.1} instead of {:.1}; a Mangonel out-ranges it at 8 either way when firing DOWNHILL is not modelled for the attacker",
        building_def(BuildingKind::Watchtower).range.to_num::<f64>() * 1.25,
        building_def(BuildingKind::Watchtower).range.to_num::<f64>()
    );

    // live: 8 spearmen storm a garrisoned tower vs 5 archers in the open
    let (bx, by) = find_flat_block(14);
    for (label, fortified) in [("5 archers IN a Tower", true), ("5 archers in the OPEN", false)] {
        let mut app = build_app();
        spawn_player(&mut app, 1);
        spawn_player(&mut app, 2);
        let tx = c(bx + 7);
        let ty = c(by + 7);
        if fortified {
            spawn_building(&mut app, 100, 2, BuildingKind::Tower, V2::new(tx, ty));
        }
        for i in 0..5u64 {
            spawn_unit(&mut app, 500 + i, 2, UnitKind::Archer, V2::new(tx, ty + fx!("0.4")));
            if fortified {
                cmd(&mut app, PlayerCommand::Garrison { player_id: 2, unit: 500 + i, building: 100 });
            }
        }
        for i in 0..8u64 {
            spawn_unit(
                &mut app,
                600 + i,
                1,
                UnitKind::Spearman,
                V2::new(tx + Fx::from_num(i as i32 % 4) - fx!("2"), ty - fx!("6")),
            );
            set_attack(&mut app, 600 + i, if fortified { 100 } else { 500 + i % 5 });
        }
        let mut t = 0;
        while t < 4000 && alive(&mut app, 1) > 0 && (alive(&mut app, 2) > 0 || fortified) {
            step(app.world_mut());
            t += 1;
            if fortified && bhp(&mut app, 100).is_none() {
                break;
            }
        }
        println!(
            "\n  8 Spearmen assault {label}: after {:.1}s -> attackers left {}, defenders left {}, tower hp {:?}",
            secs(t),
            alive(&mut app, 1),
            {
                let w = app.world_mut();
                let mut q = w.query::<(&Owner, &Unit)>();
                q.iter(w).filter(|(o, _)| o.0 == 2).count()
            },
            bhp(&mut app, 100)
        );
    }
}

// ── PART 5: siege engines under fire ────────────────────────────────────────

fn part5() {
    println!("\n=== PART 5: siege engines under fire ===\n");
    let (bx, by) = find_flat_block(16);

    // a ram walking into a garrisoned tower's field of fire
    let mut app = build_app();
    spawn_player(&mut app, 1);
    spawn_player(&mut app, 2);
    let tx = c(bx + 8);
    let ty = c(by + 8);
    spawn_building(&mut app, 100, 2, BuildingKind::Tower, V2::new(tx, ty));
    for i in 0..5u64 {
        spawn_unit(&mut app, 500 + i, 2, UnitKind::Archer, V2::new(tx, ty + fx!("0.4")));
        cmd(&mut app, PlayerCommand::Garrison { player_id: 2, unit: 500 + i, building: 100 });
    }
    spawn_unit(&mut app, 700, 1, UnitKind::Ram, V2::new(tx, ty - fx!("9")));
    set_attack(&mut app, 700, 100);
    let mut t = 0;
    while t < 6000 && uhp(&mut app, 700).is_some() && bhp(&mut app, 100).is_some() {
        step(app.world_mut());
        t += 1;
    }
    println!(
        "  1 Ram (400 hp) walks 9 tiles into a Tower garrisoned with 5 archers:\n    after {:.1}s ram hp {:?}, tower hp {:?}",
        secs(t),
        uhp(&mut app, 700),
        bhp(&mut app, 100)
    );
    println!("    ram morale/routing: {:?}", umorale(&mut app, 700));

    // a mangonel outside tower range
    let mut app = build_app();
    spawn_player(&mut app, 1);
    spawn_player(&mut app, 2);
    spawn_building(&mut app, 100, 2, BuildingKind::Tower, V2::new(tx, ty));
    for i in 0..5u64 {
        spawn_unit(&mut app, 500 + i, 2, UnitKind::Archer, V2::new(tx, ty + fx!("0.4")));
        cmd(&mut app, PlayerCommand::Garrison { player_id: 2, unit: 500 + i, building: 100 });
    }
    spawn_unit(&mut app, 700, 1, UnitKind::Mangonel, V2::new(tx, ty - fx!("7.6")));
    set_attack(&mut app, 700, 100);
    let mut t = 0;
    while t < 12000 && uhp(&mut app, 700).is_some() && bhp(&mut app, 100).is_some() {
        step(app.world_mut());
        t += 1;
    }
    println!(
        "\n  1 Mangonel (range 8, 90 hp) vs the same Tower (range 7):\n    after {:.1}s mangonel hp {:?}, tower hp {:?}, mangonel pos {:?}",
        secs(t),
        uhp(&mut app, 700),
        bhp(&mut app, 100),
        upos(&mut app, 700).map(|p| (p.x.to_num::<f32>(), p.y.to_num::<f32>()))
    );

    // friendly fire / splash check: mangonel hits one target only
    println!(
        "\n  Mangonel splash radius: none (single target). Friendly fire: none. Minimum range: none."
    );
}

fn main() {
    part1();
    part2();
    part3();
    part4();
    part5();
    part6();
    part7();
}

// ── PART 6: the economics and the exploits ──────────────────────────────────

fn part6() {
    println!("\n=== PART 6: siege economics, sites-as-weapons, targeting ===\n");
    let (bx, by) = find_flat_block(30);
    let cx = bx + 15;
    let cy = by + 15;

    // 6a. a 20-tile wall line vs 3 rams: how long to open a 3-tile gap?
    let mut app = build_app();
    spawn_player(&mut app, 1);
    spawn_player(&mut app, 2);
    for i in 0..20i32 {
        spawn_building(
            &mut app,
            200 + i as u64,
            2,
            BuildingKind::Wall,
            V2::new(c(cx - 10 + i), c(cy)),
        );
    }
    for i in 0..3i32 {
        spawn_unit(
            &mut app,
            300 + i as u64,
            1,
            UnitKind::Ram,
            V2::new(c(cx - 1 + i), c(cy - 2)),
        );
        set_attack(&mut app, 300 + i as u64, 200 + (9 + i) as u64);
    }
    let mut t = 0;
    let mut gap = 0;
    while t < 4000 {
        step(app.world_mut());
        t += 1;
        gap = {
            let w = app.world_mut();
            let mut q = w.query::<&Building>();
            20 - q.iter(w).count() as i32
        };
        if gap >= 3 {
            break;
        }
    }
    let wall = building_def(BuildingKind::Wall);
    let ram = unit_def(UnitKind::Ram);
    println!(
        "  6a. 3 Rams vs a 20-segment wall line: a 3-tile gap in {:.1}s\n      wall line cost {}w {}s ({} s of peasant labour); the 3 rams cost {}w",
        secs(t),
        wall.cost.wood * 20,
        wall.cost.stone * 20,
        wall.build_time.to_num::<f64>() * 20.0,
        ram.cost.wood * 3
    );

    // 6b. Watchtower (range 9) vs Mangonel (range 8)
    let mut app = build_app();
    spawn_player(&mut app, 1);
    spawn_player(&mut app, 2);
    spawn_building(&mut app, 100, 2, BuildingKind::Watchtower, V2::new(c(cx), c(cy)));
    for i in 0..8u64 {
        spawn_unit(&mut app, 500 + i, 2, UnitKind::Archer, V2::new(c(cx), c(cy) + fx!("0.4")));
        cmd(&mut app, PlayerCommand::Garrison { player_id: 2, unit: 500 + i, building: 100 });
    }
    spawn_unit(&mut app, 700, 1, UnitKind::Mangonel, V2::new(c(cx), c(cy) - fx!("7.6")));
    set_attack(&mut app, 700, 100);
    let mut t = 0;
    while t < 8000 && uhp(&mut app, 700).is_some() && bhp(&mut app, 100).is_some() {
        step(app.world_mut());
        t += 1;
    }
    println!(
        "  6b. 1 Mangonel vs a Watchtower + 8 archers: after {:.1}s mangonel hp {:?}, tower hp {:?}",
        secs(t),
        uhp(&mut app, 700),
        bhp(&mut app, 100)
    );

    // 6c. a wall SITE blocks pathing the instant it is paid for
    let mut app = build_app();
    spawn_player(&mut app, 1);
    spawn_player(&mut app, 2);
    let site_hp = site_start_hp(building_def(BuildingKind::Wall).max_hp);
    {
        let w = app.world_mut();
        for dx in -1..=1i32 {
            for dy in -1..=1i32 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                w.spawn((
                    GameId((500 + (dx + 1) * 3 + (dy + 1)) as u64),
                    Owner(2),
                    MatchId(1),
                    Pos { pos: V2::new(c(cx + dx), c(cy + dy)), facing: ZERO },
                    Building::site(
                        BuildingKind::Wall,
                        building_def(BuildingKind::Wall).max_hp,
                        V2::new(c(cx + dx), c(cy + dy)),
                    ),
                ));
            }
        }
    }
    spawn_unit(&mut app, 400, 1, UnitKind::Knight, V2::new(c(cx), c(cy)));
    cmd(&mut app, PlayerCommand::Move { player_id: 1, unit: 400, target: V2::new(c(cx), c(cy - 6)) });
    for _ in 0..600 {
        step(app.world_mut());
    }
    let p = upos(&mut app, 400).unwrap();
    println!(
        "  6c. a Knight ringed by 8 unfinished wall SITES ({site_hp} hp each, {}w {}s total, paid this tick):\n      30s after a MOVE order out it is at {:?}; the centre it started on is {:?} -- a paid-for SITE walls people in on the tick it is founded",
        building_def(BuildingKind::Wall).cost.wood * 8,
        building_def(BuildingKind::Wall).cost.stone * 8,
        (p.x.to_num::<f32>(), p.y.to_num::<f32>()),
        (c(cx).to_num::<f32>(), c(cy).to_num::<f32>())
    );

    // 6d. siege targeting priority: nearest BUILDING wins, whatever it is
    let mut app = build_app();
    spawn_player(&mut app, 1);
    spawn_player(&mut app, 2);
    spawn_building(&mut app, 100, 2, BuildingKind::Watchtower, V2::new(c(cx), c(cy + 3)));
    spawn_building(&mut app, 101, 2, BuildingKind::Wall, V2::new(c(cx), c(cy)));
    spawn_building(&mut app, 102, 2, BuildingKind::Farm, V2::new(c(cx + 4), c(cy)));
    spawn_unit(&mut app, 700, 1, UnitKind::Mangonel, V2::new(c(cx), c(cy - 4)));
    for _ in 0..40 {
        step(app.world_mut());
    }
    let target = {
        let w = app.world_mut();
        let mut q = w.query::<(&GameId, &Unit)>();
        q.iter(w).find(|(g, _)| g.0 == 700).map(|(_, u)| u.attack_target)
    };
    println!(
        "  6d. a Mangonel with a Watchtower, a Wall and a Farm all in reach auto-picks id {:?} (100=Watchtower, 101=Wall, 102=Farm) -- nearest building, no threat weighting",
        target
    );

    // 6e. garrison invulnerability
    let mut app = build_app();
    spawn_player(&mut app, 1);
    spawn_player(&mut app, 2);
    spawn_building(&mut app, 100, 2, BuildingKind::Keep, V2::new(c(cx), c(cy)));
    for i in 0..10u64 {
        spawn_unit(&mut app, 500 + i, 2, UnitKind::Archer, V2::new(c(cx), c(cy) + fx!("0.4")));
        cmd(&mut app, PlayerCommand::Garrison { player_id: 2, unit: 500 + i, building: 100 });
    }
    for i in 0..20u64 {
        spawn_unit(
            &mut app,
            600 + i,
            1,
            UnitKind::Mamluk,
            V2::new(c(cx) + Fx::from_num(i as i32 % 5) - fx!("2"), c(cy) - fx!("6")),
        );
        set_attack(&mut app, 600 + i, 100);
    }
    let mut t = 0;
    while t < 8000 && alive(&mut app, 1) > 0 && bhp(&mut app, 100).is_some() {
        step(app.world_mut());
        t += 1;
    }
    println!(
        "  6e. 20 Mamluks (the best melee in the game) storm a Keep garrisoned with 10 archers:\n      after {:.1}s attackers left {}, keep hp {:?}, garrison losses 0 by construction",
        secs(t),
        alive(&mut app, 1),
        bhp(&mut app, 100)
    );
}

// ── PART 7: what a wall line costs the combat tick ──────────────────────────

fn part7() {
    println!("\n=== PART 7: perf — what a long wall does to the combat tick ===\n");
    let (bx, by) = find_flat_block(40);
    for walls in [0usize, 200, 800] {
        let mut app = build_app();
        spawn_player(&mut app, 1);
        spawn_player(&mut app, 2);
        let mut id = 100_000u64;
        for i in 0..walls as i32 {
            let (wx, wy) = (bx + i % 40, by + i / 40);
            spawn_building(&mut app, id, 2, BuildingKind::Wall, V2::new(c(wx), c(wy)));
            id += 1;
        }
        // 1200 soldiers in a real melee, so morale recovery is live
        for i in 0..600i32 {
            spawn_unit(
                &mut app,
                1_000 + i as u64,
                1,
                UnitKind::Spearman,
                V2::new(c(bx + 60 + i % 20) + fx!("0.1"), c(by + i / 20)),
            );
            spawn_unit(
                &mut app,
                2_000 + i as u64,
                2,
                UnitKind::Spearman,
                V2::new(c(bx + 60 + i % 20) + fx!("0.6"), c(by + i / 20)),
            );
        }
        // warm up caches
        for _ in 0..8 {
            step(app.world_mut());
        }
        let t0 = std::time::Instant::now();
        for _ in 0..200 {
            step(app.world_mut());
        }
        let el = t0.elapsed();
        println!(
            "  1200 soldiers fighting + {walls:>3} wall segments: 200 ticks in {:>7.1} ms ({:>6.0} ticks/s)",
            el.as_secs_f64() * 1000.0,
            200.0 / el.as_secs_f64()
        );
    }
}
