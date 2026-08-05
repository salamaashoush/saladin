//! The three rules the war overhaul rests on that nothing was checking:
//! a faction may only field its OWN roster, a named target is reached however
//! far away it is, and a rallied man is posted at the flag rather than the hall
//! door. Each of these failed on the code that shipped the overhaul.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::*;
use saladin_protocol::components::ORDER_ATTACK;

fn build() -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed: 1 });
    app
}

fn cmd(app: &mut App, c: PlayerCommand) {
    app.world_mut().resource_mut::<CommandQueue>().0.push(c);
}

fn spawn_player(app: &mut App, id: u64, faction: Faction) {
    app.world_mut().spawn((
        GameId(900 + id),
        MatchId(1),
        Player {
            player_id: id,
            name: "P".into(),
            faction,
            stock: Stockpile { wood: 5000, stone: 5000, food: 5000, gold: 5000 },
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
        Unit { speed: def.speed, hp: def.max_hp, ..Unit::new(kind, pos) },
    ));
}

fn queue_len(app: &mut App, building: u64) -> u8 {
    let w = app.world_mut();
    let mut q = w.query::<(&GameId, &Building)>();
    q.iter(w).find(|(g, _)| g.0 == building).map(|(_, b)| b.queue_len).unwrap_or(0)
}

fn pos_of(app: &mut App, id: u64) -> Option<V2> {
    let w = app.world_mut();
    let mut q = w.query::<(&GameId, &Pos)>();
    q.iter(w).find(|(g, _)| g.0 == id).map(|(_, p)| p.pos)
}

/// A long strip of walkable ground, so a march is never a pathfinding story.
fn strip(seed: u32, len: i32) -> (i32, i32) {
    for cy in 20..220 {
        for cx in 20..220 {
            if (0..len).all(|dx| (0..8).all(|dy| is_passable(seed, cx + dx, cy + dy))) {
                return (cx, cy);
            }
        }
    }
    panic!("no {len}-tile strip on seed {seed}");
}

/// Exclusivity has to live in the SIM. `BuildingDef.trains` is the UNION of both
/// rosters — it has to be, the discriminant is an index into `UNIT_DEFS` — so a
/// HUD-only filter means a hand-built packet trains a Crusader Mamluk and the
/// whole faction design is decoration.
#[test]
fn a_hall_refuses_an_order_its_owners_faction_cannot_field() {
    for faction in [Faction::Ayyubid, Faction::Crusader] {
        let mut app = build();
        spawn_player(&mut app, 1, faction);
        // EVERY structure, so a refusal is never a missing prerequisite
        let mut halls: Vec<(u64, BuildingKind)> = Vec::new();
        for (i, &b) in BuildingKind::ALL.iter().enumerate() {
            let id = i as u64 + 1;
            spawn_building(&mut app, id, 1, b, V2::new(Fx::from_num(40 + i as i32), Fx::from_num(40)));
            if !building_def(b).trains.is_empty() {
                halls.push((id, b));
            }
        }
        // plenty of housing, so a refusal is never just a population cap
        for i in 0..8 {
            spawn_building(
                &mut app,
                500 + i,
                1,
                BuildingKind::House,
                V2::new(Fx::from_num(80 + i as i32), Fx::from_num(80)),
            );
        }
        step(app.world_mut());

        for (bid, kind) in &halls {
            let allowed = roster_for(*kind, faction);
            for &k in building_def(*kind).trains {
                cmd(&mut app, PlayerCommand::TrainAt { player_id: 1, building: *bid, kind: k });
                step(app.world_mut());
                let queued = queue_len(&mut app, *bid);
                if allowed.contains(&k) {
                    assert_eq!(
                        queued, 1,
                        "{faction:?} {kind:?} refused its OWN {}",
                        unit_def(k).label
                    );
                    cmd(&mut app, PlayerCommand::CancelTrain { player_id: 1, building: *bid });
                    step(app.world_mut());
                } else {
                    assert_eq!(
                        queued,
                        0,
                        "{faction:?} {kind:?} queued off-roster {}",
                        unit_def(k).label
                    );
                }
            }
        }
    }
}

/// Cancelling an off-roster order must not refund what was never paid.
#[test]
fn a_refused_order_costs_nothing() {
    let mut app = build();
    spawn_player(&mut app, 1, Faction::Crusader);
    spawn_building(&mut app, 1, 1, BuildingKind::Stable, V2::new(Fx::from_num(40), Fx::from_num(40)));
    spawn_building(&mut app, 2, 1, BuildingKind::House, V2::new(Fx::from_num(60), Fx::from_num(60)));
    step(app.world_mut());
    let before = {
        let w = app.world_mut();
        let mut q = w.query::<&Player>();
        q.iter(w).find(|p| p.player_id == 1).map(|p| p.stock).unwrap()
    };
    cmd(&mut app, PlayerCommand::TrainAt { player_id: 1, building: 1, kind: UnitKind::Mamluk });
    step(app.world_mut());
    let after = {
        let w = app.world_mut();
        let mut q = w.query::<&Player>();
        q.iter(w).find(|p| p.player_id == 1).map(|p| p.stock).unwrap()
    };
    assert_eq!(before.gold, after.gold, "a refused order still charged gold");
    assert_eq!(before.food, after.food, "a refused order still charged food");
}

/// The aggressive leash is for an AGGRO pickup — one scout must not drag an army
/// across the map. It was measured against `home`, which only a group order
/// keeps current, so a man told to kill something thirty tiles off dropped the
/// order the moment he was past the leash and walked back. He never arrived.
#[test]
fn a_named_target_is_reached_however_far_away_it_is() {
    for (label, group) in [("Attack", false), ("GroupAttack", true)] {
        let mut app = build();
        spawn_player(&mut app, 1, Faction::Ayyubid);
        spawn_player(&mut app, 2, Faction::Crusader);
        let (cx, cy) = strip(1, 40);
        let fy = Fx::from_num(cy + 4);
        let me = V2::new(Fx::from_num(cx + 2), fy);
        let foe = V2::new(Fx::from_num(cx + 34), fy);
        spawn_unit(&mut app, 1, 1, UnitKind::Spearman, me);
        spawn_unit(&mut app, 2, 2, UnitKind::Peasant, foe);
        step(app.world_mut());

        if group {
            cmd(&mut app, PlayerCommand::GroupAttack { player_id: 1, units: vec![1], target: 2 });
        } else {
            cmd(&mut app, PlayerCommand::Attack { player_id: 1, unit: 1, target: 2 });
        }
        // 32 tiles at ~2 tiles/s, with room to spare
        for _ in 0..900 {
            step(app.world_mut());
        }
        assert!(
            pos_of(&mut app, 2).is_none(),
            "{label}: the quarry 32 tiles away is still alive — the attacker turned back at the leash"
        );
    }
}

/// The same rule from the other side: a man who picks a fight up by AGGRO while
/// posted at home is still leashed, or one scout drags the whole garrison off.
#[test]
fn an_aggro_pickup_is_still_leashed_to_the_ground_it_was_posted_on() {
    let mut app = build();
    spawn_player(&mut app, 1, Faction::Ayyubid);
    spawn_player(&mut app, 2, Faction::Crusader);
    let (cx, cy) = strip(1, 40);
    let fy = Fx::from_num(cy + 4);
    let post = V2::new(Fx::from_num(cx + 2), fy);
    // 25 tiles out: past the leash, but the runner is inside aggro range of it
    let strayed = V2::new(Fx::from_num(cx + 27), fy);
    let bait = V2::new(Fx::from_num(cx + 31), fy);
    spawn_unit(&mut app, 1, 1, UnitKind::Spearman, strayed);
    spawn_unit(&mut app, 2, 2, UnitKind::Peasant, bait);
    {
        let w = app.world_mut();
        let mut q = w.query::<(&GameId, &mut Unit)>();
        for (g, mut u) in q.iter_mut(w) {
            if g.0 == 1 {
                u.home = post;
                u.stance = Stance::Aggressive;
            }
        }
    }
    for _ in 0..300 {
        step(app.world_mut());
    }
    let back = pos_of(&mut app, 1).expect("the spearman lives");
    assert!(
        dist(back, post) < dist(strayed, post),
        "an aggro pickup ignored the leash: {} tiles from post, started at {}",
        dist(back, post),
        dist(strayed, post)
    );
}

/// A rally flag is where a man is POSTED. `home` stayed the hall door, so both
/// leashes measured a rallied unit against the building it was sent away from
/// and it walked back to it the moment it saw an enemy.
#[test]
fn a_rallied_unit_is_posted_at_the_flag_not_at_the_hall_door() {
    let mut app = build();
    spawn_player(&mut app, 1, Faction::Ayyubid);
    let (cx, cy) = strip(1, 30);
    let hall = V2::new(Fx::from_num(cx + 3), Fx::from_num(cy + 4));
    let flag = V2::new(Fx::from_num(cx + 20), Fx::from_num(cy + 4));
    spawn_building(&mut app, 1, 1, BuildingKind::Barracks, hall);
    spawn_building(&mut app, 2, 1, BuildingKind::House, V2::new(hall.x, hall.y + fx!("12")));
    spawn_building(&mut app, 3, 1, BuildingKind::House, V2::new(hall.x, hall.y + fx!("14")));
    step(app.world_mut());
    cmd(&mut app, PlayerCommand::SetRally { player_id: 1, building: 1, target: flag });
    cmd(&mut app, PlayerCommand::TrainAt { player_id: 1, building: 1, kind: UnitKind::Spearman });
    let ticks = (unit_def(UnitKind::Spearman).train_time.to_num::<i64>() as usize + 2) * 20;
    for _ in 0..ticks {
        step(app.world_mut());
    }
    let homes: Vec<V2> = {
        let w = app.world_mut();
        let mut q = w.query::<&Unit>();
        q.iter(w).map(|u| u.home).collect()
    };
    assert_eq!(homes.len(), 1, "exactly one spearman was trained");
    assert!(
        dist(homes[0], flag) < dist(homes[0], hall),
        "a rallied unit is homed at the hall door {:?}, not at the flag {flag:?}",
        homes[0]
    );
}

/// The bot must play through the SAME commands a human uses. It wrote `Unit`
/// rows straight — no `order`, no `order_target`, no formation, no shared path —
/// which is a SECOND command layer, and it is why an assault wave forgot its
/// objective and marched home the moment it was past the aggressive leash.
#[test]
fn a_bot_assault_goes_out_as_a_real_group_order() {
    let mut app = build();
    scatter_world_nodes(app.world_mut(), 1);
    cmd(
        &mut app,
        PlayerCommand::AddAi {
            player_id: 1,
            host: 1,
            difficulty: AiDifficulty::Hard,
            faction: Faction::Ayyubid,
            match_id: 1,
        },
    );
    cmd(
        &mut app,
        PlayerCommand::AddAi {
            player_id: 2,
            host: 1,
            difficulty: AiDifficulty::Hard,
            faction: Faction::Crusader,
            match_id: 1,
        },
    );
    step(app.world_mut());

    // hand bot 1 a mustered army beside its keep and a full warchest, so the
    // wave gate is the only thing left to satisfy
    let keep = {
        let w = app.world_mut();
        let mut q = w.query::<(&Owner, &Pos, &Building)>();
        q.iter(w)
            .find(|(o, _, b)| o.0 == 1 && b.kind == BuildingKind::Keep)
            .map(|(_, p, _)| p.pos)
            .expect("bot 1 has a keep")
    };
    for i in 0..12u64 {
        let pos = V2::new(keep.x + Fx::from_num(2 + (i % 4) as i32), keep.y + Fx::from_num(2 + (i / 4) as i32));
        spawn_unit(&mut app, 7000 + i, 1, UnitKind::Spearman, pos);
    }
    {
        let w = app.world_mut();
        let mut q = w.query::<&mut Player>();
        for mut p in q.iter_mut(w) {
            p.stock = Stockpile { wood: 4000, stone: 4000, food: 4000, gold: 4000 };
        }
    }
    // brain runs every 20 ticks; the wave timer needs a few windows
    for _ in 0..900 {
        step(app.world_mut());
    }
    let orders: Vec<(u8, u64)> = {
        let w = app.world_mut();
        let mut q = w.query::<(&GameId, &Owner, &Unit)>();
        q.iter(w)
            .filter(|(g, o, _)| o.0 == 1 && g.0 >= 7000 && g.0 < 7012)
            .map(|(_, _, u)| (u.order, u.attack_target))
            .collect()
    };
    assert!(!orders.is_empty(), "the planted army was wiped before it marched");
    let commanded = orders.iter().filter(|(o, _)| *o != ORDER_NONE).count();
    assert!(
        commanded * 2 >= orders.len(),
        "only {commanded} of {} planted soldiers ever received a real order — the bot is still \
         writing Unit rows directly",
        orders.len()
    );
    assert!(
        orders.iter().any(|(o, t)| *o == ORDER_ATTACK && *t != 0),
        "no soldier is under a named attack order: {orders:?}"
    );
}

/// Two bots at war, two worlds, tick-by-tick. The assault path now runs through
/// the group-order machinery (shared A*, formation slots, march-at-slowest);
/// none of it may leak iteration order into the tick.
#[test]
fn two_bots_at_war_stay_in_lockstep() {
    let war = || {
        let mut app = build();
        scatter_world_nodes(app.world_mut(), 1);
        for (id, f) in [(1u64, Faction::Ayyubid), (2u64, Faction::Crusader)] {
            app.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::AddAi {
                player_id: id,
                host: 1,
                difficulty: AiDifficulty::Hard,
                faction: f,
                match_id: 1,
            });
        }
        step(app.world_mut());
        app
    };
    let (mut a, mut b) = (war(), war());
    for t in 0..5_000 {
        step(a.world_mut());
        step(b.world_mut());
        let ha = a.world().resource::<StateHash>().0;
        let hb = b.world().resource::<StateHash>().0;
        assert_eq!(ha, hb, "desync at tick {t}");
    }
}
