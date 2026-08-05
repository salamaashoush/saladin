//! AI brain behavior tests: research starts, scouting, threat recall plumbing,
//! market trading, garrison defense, and cross-world bot determinism.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{
    AiDifficulty, BuildingKind, Faction, Fx, Stance, Stockpile,
    UnitKind, V2, ZERO, building_def, unit_def,
};

fn build() -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed: 1 });
    scatter_world_nodes(app.world_mut(), 1);
    app
}

fn cmd(app: &mut App, c: PlayerCommand) {
    app.world_mut().resource_mut::<CommandQueue>().0.push(c);
}

#[test]
fn bot_with_blacksmith_starts_research() {
    let mut app = build();
    cmd(
        &mut app,
        PlayerCommand::AddAi {
            player_id: 1000,
            host: 1,
            difficulty: AiDifficulty::Normal,
            faction: Faction::Crusader,
            match_id: 1,
        },
    );
    step(app.world_mut());

    // hand the bot a blacksmith + a full warchest so research is affordable now
    let keep_pos = {
        let world = app.world_mut();
        let mut q = world.query::<(&Pos, &Building)>();
        q.iter(world).find(|(_, b)| b.kind == BuildingKind::Keep).map(|(p, _)| p.pos).unwrap()
    };
    let smith_pos = V2::new(keep_pos.x + saladin_sim::Fx::from_num(4), keep_pos.y);
    let def = building_def(BuildingKind::Blacksmith);
    app.world_mut().spawn((
        GameId(5000),
        Owner(1000),
        MatchId(1),
        Pos { pos: smith_pos, facing: ZERO },
        Building::new(BuildingKind::Blacksmith, def.max_hp, smith_pos),
    ));
    {
        let world = app.world_mut();
        let mut q = world.query::<&mut Player>();
        for mut p in q.iter_mut(world) {
            p.stock = Stockpile { wood: 2000, stone: 2000, food: 2000, gold: 2000 };
        }
    }

    // a few brain windows (brain runs every 20 base ticks)
    for _ in 0..200 {
        step(app.world_mut());
    }
    let world = app.world_mut();
    let mut rq = world.query::<&Research>();
    assert!(rq.iter(world).count() >= 1, "Normal bot should start a Blacksmith tech");
}

#[test]
fn hard_bot_sends_a_scout() {
    let mut app = build();
    cmd(
        &mut app,
        PlayerCommand::Join { player_id: 1, name: "Saladin".into(), faction: Faction::Ayyubid, match_id: 1 },
    );
    cmd(
        &mut app,
        PlayerCommand::AddAi {
            player_id: 1000,
            host: 1,
            difficulty: AiDifficulty::Hard,
            faction: Faction::Crusader,
            match_id: 1,
        },
    );
    for _ in 0..40 {
        step(app.world_mut());
    }
    let world = app.world_mut();
    let mut bq = world.query::<&Bot>();
    let scout = bq.iter(world).next().unwrap().scout_id;
    assert_ne!(scout, 0, "Hard bot should dispatch a scout toward the enemy keep");
}

fn keep_pos_of(app: &mut App, owner: u64) -> V2 {
    let world = app.world_mut();
    let mut q = world.query::<(&Pos, &Owner, &Building)>();
    q.iter(world)
        .find(|(_, o, b)| o.0 == owner && b.kind == BuildingKind::Keep)
        .map(|(p, _, _)| p.pos)
        .unwrap()
}

fn spawn_unit_row(app: &mut App, id: u64, owner: u64, kind: UnitKind, pos: V2, stance: Stance) {
    app.world_mut().spawn((
        GameId(id),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Unit {
            speed: unit_def(kind).speed,
            hp: unit_def(kind).max_hp,
            stance,
            ..Unit::new(kind, pos)
        },
    ));
}

#[test]
fn bot_sells_glut_at_its_market_for_gold() {
    let mut app = build();
    cmd(
        &mut app,
        PlayerCommand::AddAi {
            player_id: 1000,
            host: 1,
            difficulty: AiDifficulty::Normal,
            faction: Faction::Crusader,
            match_id: 1,
        },
    );
    step(app.world_mut());

    // hand the bot a Market and a deep wood glut with an empty purse
    let keep_pos = keep_pos_of(&mut app, 1000);
    let mpos = V2::new(keep_pos.x + saladin_sim::Fx::from_num(5), keep_pos.y);
    let def = building_def(BuildingKind::Market);
    app.world_mut().spawn((
        GameId(6000),
        Owner(1000),
        MatchId(1),
        Pos { pos: mpos, facing: ZERO },
        Building::new(BuildingKind::Market, def.max_hp, mpos),
    ));
    {
        let world = app.world_mut();
        let mut q = world.query::<&mut Player>();
        for mut p in q.iter_mut(world) {
            p.stock = Stockpile { wood: 2000, stone: 50, food: 800, gold: 0 };
        }
    }

    let mut earned = false;
    for _ in 0..20 {
        for _ in 0..20 {
            step(app.world_mut());
        }
        let world = app.world_mut();
        let mut q = world.query::<&Player>();
        if q.iter(world).any(|p| p.player_id == 1000 && p.stock.gold > 0) {
            earned = true;
            break;
        }
    }
    assert!(earned, "a gold-poor bot with a market and a wood glut must sell for gold");
}

#[test]
fn bot_garrisons_shooters_under_threat_and_releases_after() {
    let mut app = build();
    cmd(
        &mut app,
        PlayerCommand::Join { player_id: 1, name: "Foe".into(), faction: Faction::Crusader, match_id: 1 },
    );
    cmd(
        &mut app,
        PlayerCommand::AddAi {
            player_id: 1000,
            host: 1,
            difficulty: AiDifficulty::Normal,
            faction: Faction::Ayyubid,
            match_id: 1,
        },
    );
    step(app.world_mut());

    let keep_pos = keep_pos_of(&mut app, 1000);
    // the bot's archers stand at home
    for i in 0..3 {
        let pos = V2::new(keep_pos.x + saladin_sim::Fx::from_num(3 + i), keep_pos.y);
        spawn_unit_row(&mut app, 7000 + i as u64, 1000, UnitKind::Archer, pos, Stance::Defensive);
    }
    // enemy knights camp inside the threat radius but outside aggro/keep fire
    for i in 0..4 {
        let pos = V2::new(keep_pos.x + saladin_sim::Fx::from_num(15), keep_pos.y + saladin_sim::Fx::from_num(i));
        spawn_unit_row(&mut app, 8000 + i as u64, 1, UnitKind::Knight, pos, Stance::HoldGround);
    }

    for _ in 0..200 {
        step(app.world_mut());
    }
    {
        let world = app.world_mut();
        let mut q = world.query::<(&Owner, &Unit)>();
        let sheltered = q
            .iter(world)
            .filter(|(o, u)| o.0 == 1000 && u.kind == UnitKind::Archer && u.garrisoned_in != 0)
            .count();
        assert!(sheltered > 0, "a defending bot should garrison its shooters");
    }

    // threat clears -> the bot empties its shelters
    {
        let world = app.world_mut();
        let mut q = world.query::<(bevy_ecs::entity::Entity, &Owner, &Unit)>();
        let knights: Vec<bevy_ecs::entity::Entity> =
            q.iter(world).filter(|(_, o, u)| o.0 == 1 && u.kind == UnitKind::Knight).map(|(e, _, _)| e).collect();
        for e in knights {
            world.despawn(e);
        }
    }
    for _ in 0..100 {
        step(app.world_mut());
    }
    let world = app.world_mut();
    let mut q = world.query::<(&Owner, &Unit)>();
    let still_in = q.iter(world).filter(|(o, u)| o.0 == 1000 && u.garrisoned_in != 0).count();
    assert_eq!(still_in, 0, "all-clear should ungarrison every shelter");
}

#[test]
fn dueling_hard_bots_stay_in_lockstep() {
    let run = || {
        let mut app = build();
        for (id, faction) in [(1000, Faction::Ayyubid), (1001, Faction::Crusader)] {
            cmd(
                &mut app,
                PlayerCommand::AddAi { player_id: id, host: 1, difficulty: AiDifficulty::Hard, faction, match_id: 1 },
            );
        }
        app
    };
    let mut a = run();
    let mut b = run();
    for i in 0..600 {
        step(a.world_mut());
        step(b.world_mut());
        if i % 100 == 0 {
            let ha = a.world().resource::<StateHash>().0;
            let hb = b.world().resource::<StateHash>().0;
            assert_eq!(ha, hb, "bot worlds diverged at tick {i}");
        }
    }
    let ha = a.world().resource::<StateHash>().0;
    let hb = b.world().resource::<StateHash>().0;
    assert_eq!(ha, hb, "bot worlds diverged after 600 ticks");
}

/// A building system a bot cannot operate is not finished. The bot sites, mans
/// and RAISES structures through the same commands a human uses — no cheat, no
/// instant hall — and a site it cannot finish would wedge its own build ladder.
#[test]
fn a_bot_raises_the_buildings_it_pays_for() {
    let mut app = build();
    cmd(
        &mut app,
        PlayerCommand::AddAi {
            player_id: 1000,
            host: 1,
            difficulty: AiDifficulty::Hard,
            faction: Faction::Crusader,
            match_id: 1,
        },
    );
    // ten minutes of game time: long enough for the ladder to reach a hall
    for _ in 0..12_000 {
        step(app.world_mut());
    }
    let world = app.world_mut();
    let mut q = world.query::<(&Owner, &Building)>();
    let mine: Vec<&Building> = q.iter(world).filter(|(o, _)| o.0 == 1000).map(|(_, b)| b).collect();
    let raised = mine.iter().filter(|b| b.complete() && b.kind != BuildingKind::Keep).count();
    let sites = mine.iter().filter(|b| !b.complete()).count();
    assert!(raised >= 2, "the bot finished nothing it paid for ({raised} up, {sites} sited)");
    assert!(
        mine.iter().any(|b| b.kind == BuildingKind::House || b.kind == BuildingKind::Farm),
        "the bot never grew its economy: {:?}",
        mine.iter().map(|b| b.kind).collect::<Vec<_>>()
    );
}

/// The bot's view of a structure now carries hp and state. Without both, repair
/// is literally inexpressible and a hole in the ground counts as a finished
/// building — so this is the fixture that proves the snapshot widened.
#[test]
fn a_bot_mends_what_a_raider_broke() {
    let mut app = build();
    cmd(
        &mut app,
        PlayerCommand::AddAi {
            player_id: 1000,
            host: 1,
            difficulty: AiDifficulty::Hard,
            faction: Faction::Crusader,
            match_id: 1,
        },
    );
    step(app.world_mut());
    let keep_pos = {
        let world = app.world_mut();
        let mut q = world.query::<(&Pos, &Building)>();
        q.iter(world).find(|(_, b)| b.kind == BuildingKind::Keep).map(|(p, _)| p.pos).unwrap()
    };
    let at = V2::new(keep_pos.x + Fx::from_num(5), keep_pos.y);
    let def = building_def(BuildingKind::Barracks);
    let wrecked = def.max_hp / 5;
    app.world_mut().spawn((
        GameId(5000),
        Owner(1000),
        MatchId(1),
        Pos { pos: at, facing: ZERO },
        Building { hp: wrecked, ..Building::new(BuildingKind::Barracks, def.max_hp, at) },
    ));
    {
        let world = app.world_mut();
        let mut q = world.query::<&mut Player>();
        for mut p in q.iter_mut(world) {
            p.stock = Stockpile { wood: 4000, stone: 4000, food: 4000, gold: 4000 };
        }
    }

    let mut best = wrecked;
    let mut crewed = false;
    for _ in 0..3000 {
        step(app.world_mut());
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &Building)>();
        if let Some((_, b)) = q.iter(world).find(|(g, _)| g.0 == 5000) {
            best = best.max(b.hp);
        }
        // Masonry ALSO raises a standing building's health, so healing alone
        // would not prove a repair: the proof is a peasant put on the job.
        let mut uq = world.query::<&Unit>();
        crewed |= uq.iter(world).any(|u| u.job_site == 5000);
        if crewed && best >= def.max_hp {
            break;
        }
    }
    assert!(crewed, "the bot never sent a single hand to its wrecked barracks");
    assert!(
        best > wrecked,
        "the bot crewed the barracks but it never healed ({wrecked} -> {best} of {})",
        def.max_hp
    );
}

/// Tower -> Watchtower is a decision, not a second purchase. A bot that cannot
/// take it simply never owns a heavy tower, because the Watchtower is off the
/// build bar entirely.
///
/// The fixture holds the bot at its tower cap under sustained pressure, which is
/// exactly the state the defense rung is written for: with nowhere left to plant
/// a picket, the only way to harden the line is to RAISE the one already
/// standing on the ground it is defending.
#[test]
fn a_bot_raises_its_tower_into_a_watchtower() {
    let mut app = build();
    cmd(
        &mut app,
        PlayerCommand::AddAi {
            player_id: 1000,
            host: 1,
            difficulty: AiDifficulty::Easy,
            faction: Faction::Crusader,
            match_id: 1,
        },
    );
    step(app.world_mut());
    let keep_pos = {
        let world = app.world_mut();
        let mut q = world.query::<(&Pos, &Building)>();
        q.iter(world).find(|(_, b)| b.kind == BuildingKind::Keep).map(|(p, _)| p.pos).unwrap()
    };
    // an Easy bot caps at ONE tower, so the defense rung has only the upgrade
    // left the moment that tower stands
    let def = building_def(BuildingKind::Tower);
    let at = V2::new(keep_pos.x + Fx::from_num(6), keep_pos.y + Fx::from_num(2));
    app.world_mut().spawn((
        GameId(5000),
        Owner(1000),
        MatchId(1),
        Pos { pos: at, facing: ZERO },
        Building::new(BuildingKind::Tower, def.max_hp, at),
    ));
    // a raiding party parked just inside threat range and out of tower reach
    let camp = V2::new(keep_pos.x + Fx::from_num(20), keep_pos.y);
    for i in 0..4u64 {
        spawn_unit_row(
            &mut app,
            6000 + i,
            1001,
            UnitKind::Spearman,
            V2::new(camp.x + Fx::from_num(i as i32), camp.y),
            Stance::Defensive,
        );
    }
    {
        let world = app.world_mut();
        let mut q = world.query::<&mut Player>();
        for mut p in q.iter_mut(world) {
            p.stock = Stockpile { wood: 6000, stone: 6000, food: 6000, gold: 6000 };
        }
    }

    let mut reached = false;
    for _ in 0..8000 {
        step(app.world_mut());
        // the siege never lifts: the raiders are a standing threat, not a fight
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &mut Unit)>();
        for (g, mut u) in q.iter_mut(world) {
            if (6000..6004).contains(&g.0) {
                u.hp = unit_def(UnitKind::Spearman).max_hp;
            }
        }
        let mut q = world.query::<(&GameId, &Building)>();
        if q.iter(world).any(|(g, b)| g.0 == 5000 && b.kind == BuildingKind::Watchtower) {
            reached = true;
            break;
        }
    }
    assert!(reached, "the bot never upgraded a tower, so it can never own a watchtower");
    // the upgrade happened IN PLACE: same row, same id, same owner
    let world = app.world_mut();
    let mut q = world.query::<(&GameId, &Owner, &Building)>();
    let (_, o, b) = q.iter(world).find(|(g, _, _)| g.0 == 5000).expect("the tower kept its id");
    assert_eq!(o.0, 1000, "the upgrade changed hands");
    assert_eq!(b.kind, BuildingKind::Watchtower);
    assert_eq!(b.state, saladin_sim::BuildState::Complete);
}

/// The Storehouse is the whole answer to a town that cannot grow past a 28-tile
/// disc. A bot that never plants one leaves every quarry and ore belt on the map
/// as decoration.
#[test]
fn a_hard_bot_expands_with_a_storehouse() {
    let mut app = build();
    cmd(
        &mut app,
        PlayerCommand::AddAi {
            player_id: 1000,
            host: 1,
            difficulty: AiDifficulty::Hard,
            faction: Faction::Crusader,
            match_id: 1,
        },
    );
    step(app.world_mut());
    {
        let world = app.world_mut();
        let mut q = world.query::<&mut Player>();
        for mut p in q.iter_mut(world) {
            p.stock = Stockpile { wood: 6000, stone: 6000, food: 6000, gold: 6000 };
        }
    }
    let mut sited = false;
    for _ in 0..12_000 {
        step(app.world_mut());
        let world = app.world_mut();
        let mut q = world.query::<(&Owner, &Building)>();
        if q.iter(world).any(|(o, b)| o.0 == 1000 && b.kind == BuildingKind::Storehouse) {
            sited = true;
            break;
        }
    }
    assert!(sited, "a Hard bot never planted an outpost, so its town can never leave the disc");
}

/// Two bots playing the WHOLE new lifecycle against each other — siting, manning,
/// finishing, queueing, repairing — with the desync detector reading every tick.
/// This is the one that would catch an ECS-iteration-order slip in any of it.
#[test]
fn dueling_bots_stay_in_lockstep_through_the_whole_lifecycle() {
    let run = || {
        let mut app = build();
        for (id, faction) in [(1000, Faction::Ayyubid), (1001, Faction::Crusader)] {
            cmd(
                &mut app,
                PlayerCommand::AddAi { player_id: id, host: 1, difficulty: AiDifficulty::Hard, faction, match_id: 1 },
            );
        }
        app
    };
    let mut a = run();
    let mut b = run();
    for i in 0..3000 {
        step(a.world_mut());
        step(b.world_mut());
        let ha = a.world().resource::<StateHash>().0;
        let hb = b.world().resource::<StateHash>().0;
        assert_eq!(ha, hb, "bot worlds diverged at tick {i}");
    }
    // and the run actually exercised construction rather than idling
    let world = a.world_mut();
    let mut q = world.query::<&Building>();
    let raised = q.iter(world).filter(|b| b.complete() && b.kind != BuildingKind::Keep).count();
    assert!(raised >= 2, "the bots raised nothing in 3000 ticks, so lockstep proved little");
}

/// A siege train marches at what shoots back, not at the cheapest masonry in
/// front of it. `intel.defenses` used to list plain wall segments, so
/// `target_for_role(Siege)` sent every engine at the nearest five wood of wall
/// while the tower behind it kept firing. Walls stay in `buildings` — a town
/// with no towers still gives the engines something to break.
#[test]
fn a_siege_train_marches_at_the_tower_not_the_nearest_wall() {
    let mut app = build();
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
    let enemy_keep = keep_pos_of(&mut app, 2);
    let f = |v: Fx, d: i32| v + Fx::from_num(d);

    // a wall line between us and them, and one tower further back
    let wall_def = building_def(BuildingKind::Wall);
    for i in 0..5 {
        let p = V2::new(f(enemy_keep.x, -8), f(enemy_keep.y, i - 2));
        app.world_mut().spawn((
            GameId(7000 + i as u64),
            Owner(2),
            MatchId(1),
            Pos { pos: p, facing: ZERO },
            Building::new(BuildingKind::Wall, wall_def.max_hp, p),
        ));
    }
    let tower_def = building_def(BuildingKind::Tower);
    let tower_pos = V2::new(f(enemy_keep.x, -3), enemy_keep.y);
    app.world_mut().spawn((
        GameId(7100),
        Owner(2),
        MatchId(1),
        Pos { pos: tower_pos, facing: ZERO },
        Building::new(BuildingKind::Tower, tower_def.max_hp, tower_pos),
    ));

    // a ram sitting right on top of the wall line: the nearest defence by a mile
    let ram_pos = V2::new(f(enemy_keep.x, -10), enemy_keep.y);
    spawn_unit_row(&mut app, 7200, 1, UnitKind::Ram, ram_pos, Stance::Aggressive);

    // the planner's own view of the enemy town, built the way the brain builds it
    let world = app.world_mut();
    let mut bq = world.query::<(&GameId, &Pos, &Owner, &Building, &MatchId)>();
    let mut intel = saladin_sim::AssaultIntel::default();
    for (g, p, o, b, _) in bq.iter(world) {
        if o.0 != 2 {
            continue;
        }
        let t = saladin_sim::TacticalTarget { id: g.0, pos: p.pos };
        intel.buildings.push(t);
        if b.kind == BuildingKind::Keep {
            intel.keep = Some(t);
        }
        if matches!(
            b.kind,
            BuildingKind::Gatehouse | BuildingKind::Tower | BuildingKind::Watchtower
        ) {
            intel.defenses.push(t);
        }
    }
    let picked = saladin_sim::target_for_role(saladin_sim::SquadRole::Siege, ram_pos, &intel)
        .expect("there is an enemy town to march at");
    assert_eq!(picked.id, 7100, "the ram picked {} instead of the tower", picked.id);

    // and squad_role really does class a ram as siege, or the above proves nothing
    assert_eq!(saladin_sim::squad_role(UnitKind::Ram), saladin_sim::SquadRole::Siege);
}

/// A unit the brain sends to war, home, or scouting must let go of its building
/// site. `job_site` is what `spare_hands` reads to decide a peasant is spoken
/// for, so a scout that keeps one books a crew slot at a foundation it is
/// walking away from for the rest of the match. `move_unit` — the command a
/// human's order goes through — has always cleared it; the brain's private
/// order path had drifted and did not.
#[test]
fn a_bot_order_releases_the_builder_it_takes() {
    let mut app = build();
    for (id, faction) in [(1u64, Faction::Ayyubid), (2, Faction::Crusader)] {
        cmd(
            &mut app,
            PlayerCommand::AddAi {
                player_id: id,
                host: 1,
                difficulty: AiDifficulty::Hard,
                faction,
                match_id: 1,
            },
        );
    }
    step(app.world_mut());

    // Every tick, book each of the bot's peasants onto a site, then let the
    // brain run. The tick it picks a scout is the tick under test.
    let mut checked = false;
    for _ in 0..1200 {
        {
            let world = app.world_mut();
            let mut q = world.query::<(&Owner, &mut Unit)>();
            for (o, mut u) in q.iter_mut(world) {
                if o.0 == 1 && u.kind == UnitKind::Peasant {
                    u.job_site = 4242;
                }
            }
        }
        step(app.world_mut());
        let world = app.world_mut();
        let mut bq = world.query::<(&Player, &Bot)>();
        let scout = bq.iter(world).find(|(p, _)| p.player_id == 1).map(|(_, b)| b.scout_id);
        let Some(scout) = scout.filter(|s| *s != 0) else { continue };
        let mut q = world.query::<(&GameId, &Unit)>();
        let u = q.iter(world).find(|(g, _)| g.0 == scout).map(|(_, u)| (u.job_site, u.gather_state));
        if let Some((job_site, state)) = u {
            assert_eq!(
                job_site, 0,
                "the scout marched off still holding site {job_site} (state {state:?})"
            );
            checked = true;
        }
        break;
    }
    assert!(checked, "no scout was ever sent, so the release path was never exercised");
}
