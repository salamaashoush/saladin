//! Part 3: a scripted realistic opening. A "player" policy that builds the way
//! a genre veteran would, timed in ticks.

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
fn keep_pos(app: &mut App, id: u64) -> V2 {
    let w = app.world_mut();
    let mut q = w.query::<(&Owner, &Building, &Pos)>();
    q.iter(w).find(|(o, b, _)| o.0 == id && b.kind == BuildingKind::Keep).map(|(_, _, p)| p.pos).unwrap()
}
fn owned(app: &mut App, id: u64) -> Vec<(BuildingKind, V2)> {
    let w = app.world_mut();
    let mut q = w.query::<(&Owner, &Building, &Pos)>();
    q.iter(w).filter(|(o, _, _)| o.0 == id).map(|(_, b, p)| (b.kind, p.pos)).collect()
}
fn unit_counts(app: &mut App, id: u64) -> (usize, usize) {
    let w = app.world_mut();
    let mut q = w.query::<(&Owner, &Unit)>();
    let v: Vec<UnitKind> = q.iter(w).filter(|(o, _)| o.0 == id).map(|(_, u)| u.kind).collect();
    (v.iter().filter(|k| **k == UnitKind::Peasant).count(), v.iter().filter(|k| **k != UnitKind::Peasant).count())
}

fn site(app: &mut App, seed: u32, id: u64, kind: BuildingKind) -> Option<V2> {
    let own: Vec<V2> = owned(app, id).into_iter().map(|(_, p)| p).collect();
    let kp = own[0];
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
    for r in 3..25i32 {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs().max(dy.abs()) != r {
                    continue;
                }
                let x = kp.x.floor() + Fx::from_num(dx) + fx!("0.5");
                let y = kp.y.floor() + Fx::from_num(dy) + fx!("0.5");
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
    let mut app = build_app(seed);
    scatter_world_nodes(app.world_mut(), 1);
    cmd(&mut app, PlayerCommand::Join { player_id: 1, name: "H".into(), faction: Faction::Ayyubid, match_id: 1 });
    step(app.world_mut());
    let kp = keep_pos(&mut app, 1);
    println!("SCRIPTED OPENING, seed {seed}, keep at ({:.1},{:.1})", kp.x.to_num::<f32>(), kp.y.to_num::<f32>());
    println!("start: {:?}, 5 peasants, pop cap 8\n", stock(&mut app, 1));

    // policy queue: what a genre player wants, in order
    let mut want: Vec<(&str, Option<BuildingKind>, Option<UnitKind>)> = vec![
        ("peasant", None, Some(UnitKind::Peasant)),
        ("peasant", None, Some(UnitKind::Peasant)),
        ("peasant", None, Some(UnitKind::Peasant)),
        ("House", Some(BuildingKind::House), None),
        ("peasant", None, Some(UnitKind::Peasant)),
        ("peasant", None, Some(UnitKind::Peasant)),
        ("Farm", Some(BuildingKind::Farm), None),
        ("House", Some(BuildingKind::House), None),
        ("peasant", None, Some(UnitKind::Peasant)),
        ("peasant", None, Some(UnitKind::Peasant)),
        ("Barracks", Some(BuildingKind::Barracks), None),
        ("Spearman", None, Some(UnitKind::Spearman)),
        ("Spearman", None, Some(UnitKind::Spearman)),
        ("Spearman", None, Some(UnitKind::Spearman)),
        ("House", Some(BuildingKind::House), None),
        ("Blacksmith", Some(BuildingKind::Blacksmith), None),
        ("Stable", Some(BuildingKind::Stable), None),
        ("Market", Some(BuildingKind::Market), None),
        ("Tower", Some(BuildingKind::Tower), None),
        ("Knight", None, Some(UnitKind::Knight)),
        ("Knight", None, Some(UnitKind::Knight)),
        ("SiegeWorkshop", Some(BuildingKind::SiegeWorkshop), None),
    ];
    want.reverse();

    let mut clicks = 0u32;
    let mut idle_ticks = 0u32;
    let mut log: Vec<(u64, String)> = Vec::new();
    for t in 1..=6000u64 {
        // re-task idle peasants once a second so gathering never stalls
        if t % 20 == 0 {
            cmd(&mut app, PlayerCommand::AutoGather { player_id: 1 });
        }
        if let Some(&(label, bk, uk)) = want.last() {
            let s = stock(&mut app, 1);
            let afford = match (bk, uk) {
                (Some(k), _) => s.can_afford(&building_def(k).cost),
                (_, Some(u)) => s.can_afford(&unit_def(u).cost),
                _ => false,
            };
            if afford {
                let issued = if let Some(k) = bk {
                    match site(&mut app, seed, 1, k) {
                        Some(p) => {
                            cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: k, pos: p, facing: 0, builders: vec![] });
                            true
                        }
                        None => false,
                    }
                } else if let Some(u) = uk {
                    cmd(&mut app, PlayerCommand::Train { player_id: 1, kind: u });
                    true
                } else {
                    false
                };
                if issued {
                    let before = if let Some(k) = bk {
                        owned(&mut app, 1).iter().filter(|(kk, _)| *kk == k).count()
                    } else {
                        let (p, a) = unit_counts(&mut app, 1);
                        p + a
                    };
                    step(app.world_mut());
                    let after = if let Some(k) = bk {
                        owned(&mut app, 1).iter().filter(|(kk, _)| *kk == k).count()
                    } else {
                        let (p, a) = unit_counts(&mut app, 1);
                        p + a
                    };
                    if after > before {
                        clicks += 1;
                        log.push((t, label.to_string()));
                        want.pop();
                    }
                    continue;
                }
            } else {
                idle_ticks += 1;
            }
        }
        step(app.world_mut());
    }

    println!("build order actually achieved in 5 minutes ({} player actions):", clicks);
    let mut prev = 0u64;
    for (t, label) in &log {
        println!("  t{:<5} {:>6.1}s  (+{:>5.1}s wait)  {}", t, *t as f64 * 0.05, (*t - prev) as f64 * 0.05, label);
        prev = *t;
    }
    if !want.is_empty() {
        println!("  NOT REACHED in 5 min: {:?}", want.iter().rev().map(|w| w.0).collect::<Vec<_>>());
    }
    let (pe, ar) = unit_counts(&mut app, 1);
    println!("\n  after 300s: {} peasants, {} soldiers, stock {:?}", pe, ar, stock(&mut app, 1));
    let mut kinds: Vec<&str> = owned(&mut app, 1).iter().map(|(k, _)| building_def(*k).label).collect();
    kinds.sort();
    println!("  buildings: {kinds:?}");
    println!("  ticks spent unable to afford the next thing: {idle_ticks} ({:.0}s of the 300s = {:.0}%)",
        idle_ticks as f64 * 0.05, idle_ticks as f64 / 6000.0 * 100.0);
    println!("\n  -> total player CLICKS in the whole 5-minute opening: {clicks}");
    println!("  -> every one of those was instant. No build progress, no queue, no builder, nothing to watch.");
}
