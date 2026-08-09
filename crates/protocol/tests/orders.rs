//! Army orders: the group verbs, what they cost, and that they stay lockstep.
//!
//! Every number quoted here was measured with `tactics_bench shape` before it
//! became an assertion. The shipped alternative to all of it was one full-map
//! A* per unit inside the exclusive command pass — 200 men measured 28 ms in a
//! SINGLE tick against a 50 ms budget, paid at the same moment by every peer.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::*;

fn arena(seed: u32) -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed });
    app
}

fn flat_block(seed: u32, w: i32, h: i32) -> (i32, i32) {
    for cy in 24..(WORLD_SIZE - h - 8) {
        for cx in 24..(WORLD_SIZE - w - 8) {
            if (0..w).all(|dx| (0..h).all(|dy| is_passable(seed, cx + dx, cy + dy))) {
                let e0 = elevation_at(seed, Fx::from_num(cx), Fx::from_num(cy));
                let flat = (0..w).step_by(3).all(|dx| {
                    (0..h).step_by(3).all(|dy| {
                        (elevation_at(seed, Fx::from_num(cx + dx), Fx::from_num(cy + dy)) - e0).abs()
                            < fx!("0.04")
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

fn push(app: &mut App, cmd: PlayerCommand) {
    app.world_mut().resource_mut::<CommandQueue>().0.push(cmd);
}

fn unit_of(app: &mut App, id: u64) -> Unit {
    let world = app.world_mut();
    let mut q = world.query::<(&GameId, &Unit)>();
    q.iter(world).find(|(g, _)| g.0 == id).map(|(_, u)| u.clone()).expect("unit alive")
}

fn pos_of(app: &mut App, id: u64) -> V2 {
    let world = app.world_mut();
    let mut q = world.query::<(&GameId, &Pos)>();
    q.iter(world).find(|(g, _)| g.0 == id).map(|(_, p)| p.pos).expect("unit alive")
}

fn alive(app: &mut App, id: u64) -> bool {
    let world = app.world_mut();
    let mut q = world.query::<&GameId>();
    q.iter(world).any(|g| g.0 == id)
}

// ── the wire ─────────────────────────────────────────────────────────────────

/// bincode encodes the VARIANT INDEX. A variant inserted rather than appended
/// silently renumbers every later one, so a Pause would decode as garbage
/// instead of failing. These indices are the wire contract for v7.
#[test]
fn the_new_verbs_are_appended_not_inserted() {
    let index_of = |c: &PlayerCommand| {
        let bytes = bincode::serialize(c).expect("encodes");
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    };
    let at = tile(10, 10);
    // the variants that existed before this workflow keep the numbers they had
    assert_eq!(
        index_of(&PlayerCommand::Join {
            player_id: 1,
            name: "a".into(),
            faction: Faction::Ayyubid,
            match_id: 1
        }),
        0
    );
    assert_eq!(index_of(&PlayerCommand::Move { player_id: 1, unit: 2, target: at }), 2);
    assert_eq!(index_of(&PlayerCommand::Pause { player_id: 1 }), 17);
    assert_eq!(index_of(&PlayerCommand::CancelTrain { player_id: 1, building: 2 }), 23);
    // and the four new ones land after every one of them
    let units = vec![7u64, 9];
    assert_eq!(
        index_of(&PlayerCommand::GroupMove {
            player_id: 1,
            units: units.clone(),
            target: at,
            formation: 0
        }),
        24
    );
    assert_eq!(
        index_of(&PlayerCommand::AttackMove {
            player_id: 1,
            units: units.clone(),
            target: at,
            formation: 3
        }),
        25
    );
    assert_eq!(
        index_of(&PlayerCommand::GroupAttack { player_id: 1, units: units.clone(), target: 4 }),
        26
    );
    assert_eq!(index_of(&PlayerCommand::Stop { player_id: 1, units }), 27);
}

#[test]
fn every_new_verb_survives_the_wire() {
    let at = V2::new(fx!("12.25"), fx!("200.75"));
    let units: Vec<u64> = (1..=64).collect();
    let cmds = vec![
        PlayerCommand::GroupMove { player_id: 3, units: units.clone(), target: at, formation: 2 },
        PlayerCommand::AttackMove { player_id: 3, units: units.clone(), target: at, formation: 255 },
        PlayerCommand::GroupAttack { player_id: 3, units: units.clone(), target: 900 },
        PlayerCommand::Stop { player_id: 3, units: units.clone() },
    ];
    // through the SAME envelope the relay uses, not just the bare command
    let msg = net_msg::Msg::Submit { tick: 41, player_id: 3, cmds: cmds.clone() };
    let back = net_msg::decode(&net_msg::encode(&msg)).expect("decodes");
    let net_msg::Msg::Submit { tick, player_id, cmds: got } = back else { panic!("wrong frame") };
    assert_eq!((tick, player_id), (41, 3));
    assert_eq!(got.len(), cmds.len());
    match &got[0] {
        PlayerCommand::GroupMove { units: u, target, formation, .. } => {
            assert_eq!(u, &units);
            assert_eq!(*target, at);
            assert_eq!(*formation, 2);
        }
        other => panic!("got {other:?}"),
    }
    match &got[2] {
        PlayerCommand::GroupAttack { target, .. } => assert_eq!(*target, 900),
        other => panic!("got {other:?}"),
    }
}

/// The whole point of a group verb on the wire: one click is one message.
#[test]
fn one_click_is_one_message_not_two_hundred() {
    let ids: Vec<u64> = (1..=200).collect();
    let at = tile(60, 60);
    let group = net_msg::encode(&net_msg::Msg::Submit {
        tick: 1,
        player_id: 1,
        cmds: vec![PlayerCommand::GroupMove {
            player_id: 1,
            units: ids.clone(),
            target: at,
            formation: 3,
        }],
    });
    let apiece = net_msg::encode(&net_msg::Msg::Submit {
        tick: 1,
        player_id: 1,
        cmds: ids
            .iter()
            .map(|u| PlayerCommand::Move { player_id: 1, unit: *u, target: at })
            .collect(),
    });
    assert!(
        group.len() * 3 < apiece.len(),
        "one group order is {} bytes against {} for the same click one man at a time",
        group.len(),
        apiece.len()
    );
}

// ── the order ────────────────────────────────────────────────────────────────

/// Slots used to go out in id order and 402 of 435 measured pairs crossed on the
/// way to the line. Sorting men and slots by one march key does not fix it —
/// measured, it removed NONE, because a selected block is rarely aligned to the
/// line it is sent to. Trading places between crossing pairs does.
#[test]
fn a_group_order_gives_every_man_his_own_place_and_they_do_not_cross() {
    let seed = 48514;
    let (cx, cy) = flat_block(seed, 40, 40);
    let mut app = arena(seed);
    let mut starts: Vec<(u64, V2)> = Vec::new();
    for i in 0..30u64 {
        let p = tile(cx + 2 + (i as i32 % 8), cy + 2 + (i as i32 / 8));
        put(&mut app, 1 + i, 1, UnitKind::Spearman, p);
        starts.push((1 + i, p));
    }
    let goal = tile(cx + 18, cy + 32);
    push(
        &mut app,
        PlayerCommand::GroupMove {
            player_id: 1,
            units: starts.iter().map(|(g, _)| *g).collect(),
            target: goal,
            formation: FormationShape::Box as u8,
        },
    );
    step(app.world_mut());

    let places: Vec<(u64, V2)> =
        starts.iter().map(|(g, _)| (*g, unit_of(&mut app, *g).home)).collect();
    for i in 0..places.len() {
        assert!(unit_of(&mut app, places[i].0).has_target, "man {} was not sent", places[i].0);
        for j in i + 1..places.len() {
            assert!(
                dist2(places[i].1, places[j].1) > fx!("0.01"),
                "two men were sent to the same spot"
            );
        }
    }
    // no pair of walks crosses
    let cross = |a: V2, b: V2, c: V2, d: V2| {
        let s = |p: V2, q: V2, r: V2| (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x);
        let (d1, d2, d3, d4) = (s(c, d, a), s(c, d, b), s(a, b, c), s(a, b, d));
        ((d1 > Fx::ZERO) != (d2 > Fx::ZERO)) && ((d3 > Fx::ZERO) != (d4 > Fx::ZERO))
    };
    let mut crossings = 0;
    for i in 0..starts.len() {
        for j in i + 1..starts.len() {
            if cross(starts[i].1, places[i].1, starts[j].1, places[j].1) {
                crossings += 1;
            }
        }
    }
    assert_eq!(crossings, 0, "{crossings} of 435 pairs cross on the way to their places");
}

/// A column marches at the pace of its slowest man, and gets its own pace back
/// the moment the march is over. The combined-arms arrival spread was 21.8 s
/// one man at a time and 7.7 s as a group.
#[test]
fn a_column_marches_at_the_pace_of_its_engine_and_no_slower_afterwards() {
    let seed = 48514;
    let (cx, cy) = flat_block(seed, 30, 30);
    let mut app = arena(seed);
    put(&mut app, 1, 1, UnitKind::Knight, tile(cx + 2, cy + 2));
    put(&mut app, 2, 1, UnitKind::Ram, tile(cx + 3, cy + 2));
    let ram_speed = unit_def(UnitKind::Ram).speed;
    let knight_speed = unit_def(UnitKind::Knight).speed;
    assert!(knight_speed > ram_speed, "the roster no longer has a fast horse and a slow engine");

    push(
        &mut app,
        PlayerCommand::GroupMove {
            player_id: 1,
            units: vec![1, 2],
            target: tile(cx + 20, cy + 20),
            formation: FormationShape::Line as u8,
        },
    );
    step(app.world_mut());
    assert_eq!(unit_of(&mut app, 1).speed, ram_speed, "the horse outran the engine");

    // they arrive together, not a minute apart
    let mut arrived: Vec<(u64, usize)> = Vec::new();
    for t in 0..2000 {
        step(app.world_mut());
        for id in [1u64, 2] {
            if !unit_of(&mut app, id).has_target && !arrived.iter().any(|(x, _)| *x == id) {
                arrived.push((id, t));
            }
        }
        if arrived.len() == 2 {
            break;
        }
    }
    assert_eq!(arrived.len(), 2, "the column never arrived");
    step(app.world_mut());
    let skew = arrived.iter().map(|(_, t)| *t).max().unwrap()
        - arrived.iter().map(|(_, t)| *t).min().unwrap();
    assert!(skew <= 20, "the column arrived {skew} ticks apart");
    // and the horse is a horse again
    assert_eq!(unit_of(&mut app, 1).speed, knight_speed, "the horse kept the column pace");
}

/// A Stop is a real order — and it is SPENT once served. Left standing it would
/// veto every later `has_target` write in the tree, which is HoldGround applied
/// to the whole game rather than a halt.
#[test]
fn a_stop_halts_everything_and_the_man_can_be_put_back_to_work() {
    let seed = 48514;
    let (cx, cy) = flat_block(seed, 24, 24);
    let mut app = arena(seed);
    put(&mut app, 1, 1, UnitKind::Peasant, tile(cx + 2, cy + 2));
    // a harmless quarry: a spearman beside him would simply kill him
    put(&mut app, 2, 2, UnitKind::Peasant, tile(cx + 18, cy + 18));
    app.world_mut().spawn((
        GameId(50),
        MatchId(1),
        Pos { pos: tile(cx + 8, cy + 8), facing: Fx::ZERO },
        ResourceNode { res_type: ResourceType::Wood, remaining: 500, cap: 500, regen: 0 },
    ));

    // the hunter has to be someone who can actually swing: `combat` skips a
    // worker on exactly that test, so a target set on one would never be
    // cleared again
    put(&mut app, 3, 1, UnitKind::Spearman, tile(cx + 3, cy + 2));
    push(&mut app, PlayerCommand::Move { player_id: 1, unit: 1, target: tile(cx + 20, cy + 20) });
    push(&mut app, PlayerCommand::Attack { player_id: 1, unit: 3, target: 2 });
    push(&mut app, PlayerCommand::Attack { player_id: 1, unit: 1, target: 2 });
    step(app.world_mut());
    assert_ne!(unit_of(&mut app, 3).attack_target, 0);
    let worker = unit_of(&mut app, 1);
    assert_eq!(worker.attack_target, 0, "a worker locked onto a quarry he can never strike");
    assert!(worker.has_target, "...and did not even march on it");

    push(&mut app, PlayerCommand::Stop { player_id: 1, units: vec![1, 3] });
    step(app.world_mut());
    assert_eq!(unit_of(&mut app, 3).attack_target, 0, "a stopped hunter is still hunting");
    let u = unit_of(&mut app, 1);
    assert!(!u.has_target, "a stopped man is still walking");
    assert!(u.path.is_empty(), "a stopped man kept his path");
    assert_eq!(u.attack_target, 0, "a stopped man is still hunting");
    assert_eq!(u.gather_state, GatherState::Idle, "a stopped man is still gathering");
    let stood = pos_of(&mut app, 1);
    for _ in 0..40 {
        step(app.world_mut());
    }
    assert!(dist2(stood, pos_of(&mut app, 1)) < fx!("0.5"), "a stopped man wandered off");

    // and he can be employed again
    push(&mut app, PlayerCommand::Gather { player_id: 1, unit: 1, node: 50 });
    for _ in 0..60 {
        step(app.world_mut());
    }
    assert!(
        dist2(stood, pos_of(&mut app, 1)) > fx!("1"),
        "a stopped man could never be put back to work"
    );
}

/// An attack-move both fights what turns up AND finishes the march. A plain
/// move order does the first (an aggressive man diverts on his own) and not the
/// second — measured, it ends the run standing over the body. The resume is the
/// difference between the two verbs, and it is deliberately NOT given to
/// `ORDER_MOVE`: the brain writes movement directly in half a dozen places
/// without touching `order`, so a stale `order_target` would drag those units
/// back to wherever they were last sent by hand.
#[test]
fn an_attack_move_finishes_its_march_and_a_move_order_does_not() {
    let seed = 48514;
    let (cx, cy) = flat_block(seed, 40, 40);
    let goal = tile(cx + 34, cy + 4);

    // a lone enemy two tiles off the road, well outside anyone's notice from
    // the start line
    let run = |attack_move: bool| {
        let mut app = arena(seed);
        for i in 0..4u64 {
            put(&mut app, 1 + i, 1, UnitKind::Spearman, tile(cx + 2, cy + 2 + i as i32));
        }
        put(&mut app, 99, 2, UnitKind::Peasant, tile(cx + 18, cy + 7));
        let units = vec![1, 2, 3, 4];
        let cmd = if attack_move {
            PlayerCommand::AttackMove {
                player_id: 1,
                units,
                target: goal,
                formation: FormationShape::Line as u8,
            }
        } else {
            PlayerCommand::GroupMove {
                player_id: 1,
                units,
                target: goal,
                formation: FormationShape::Line as u8,
            }
        };
        push(&mut app, cmd);
        for _ in 0..3000 {
            step(app.world_mut());
            if !alive(&mut app, 99) && dist2(pos_of(&mut app, 1), goal) < fx!("36") {
                break;
            }
        }
        let dead = !alive(&mut app, 99);
        let there = dist2(pos_of(&mut app, 1), goal) < fx!("36");
        (dead, there)
    };

    let (killed, arrived) = run(true);
    assert!(killed, "an attack-move walked past a man standing beside the road");
    assert!(arrived, "an attack-move never resumed its march after the fight");
    let (also_killed, never_arrived) = run(false);
    assert!(also_killed, "an aggressive man ignored an enemy beside his road");
    assert!(!never_arrived, "a plain move now resumes too — say so in the docs above");
}

/// The march has to resume even when the fight happens ON the road: a body that
/// wins its first contact used to stand where it stopped, which is how a 40v40
/// mirror froze at 20v20 for 230 seconds.
#[test]
fn an_attack_move_resumes_after_it_wins() {
    let seed = 48514;
    let (cx, cy) = flat_block(seed, 40, 40);
    let mut app = arena(seed);
    for i in 0..6u64 {
        put(&mut app, 1 + i, 1, UnitKind::Spearman, tile(cx + 2, cy + 2 + i as i32));
    }
    put(&mut app, 99, 2, UnitKind::Peasant, tile(cx + 14, cy + 4));
    let goal = tile(cx + 34, cy + 4);
    push(
        &mut app,
        PlayerCommand::AttackMove {
            player_id: 1,
            units: (1..=6).collect(),
            target: goal,
            formation: FormationShape::Line as u8,
        },
    );
    let mut killed_at = None;
    for t in 0..3000 {
        step(app.world_mut());
        if killed_at.is_none() && !alive(&mut app, 99) {
            killed_at = Some(t);
        }
    }
    assert!(killed_at.is_some(), "the attack-move never reached the man in its way");
    let ahead = (1..=6u64).filter(|id| dist2(pos_of(&mut app, *id), goal) < fx!("64")).count();
    assert!(ahead >= 5, "only {ahead} of 6 resumed the march after the fight");
}

/// A group attack sends the whole selection at one quarry through one path,
/// instead of one full-map A* per man.
#[test]
fn a_group_attack_commits_the_whole_selection() {
    let seed = 48514;
    let (cx, cy) = flat_block(seed, 30, 30);
    let mut app = arena(seed);
    for i in 0..8u64 {
        put(&mut app, 1 + i, 1, UnitKind::Spearman, tile(cx + 2 + i as i32 % 4, cy + 2 + i as i32 / 4));
    }
    put(&mut app, 99, 2, UnitKind::Peasant, tile(cx + 20, cy + 20));
    push(
        &mut app,
        PlayerCommand::GroupAttack { player_id: 1, units: (1..=8).collect(), target: 99 },
    );
    step(app.world_mut());
    for id in 1..=8u64 {
        assert_eq!(unit_of(&mut app, id).attack_target, 99, "man {id} was not committed");
    }
    // an order at someone else's unit does nothing at all
    push(
        &mut app,
        PlayerCommand::GroupAttack { player_id: 2, units: (1..=8).collect(), target: 99 },
    );
    step(app.world_mut());
    assert_eq!(unit_of(&mut app, 1).attack_target, 99, "an enemy re-ordered my army");

    for _ in 0..2000 {
        step(app.world_mut());
        if !alive(&mut app, 99) {
            break;
        }
    }
    assert!(!alive(&mut app, 99), "eight men never reached one peasant");
}

/// `Unit.speed` is a duplicate of `unit_def(kind).speed`, which is the ONLY
/// reason a march pace can be restored from the unit table when the march ends.
/// If a tech ever changes speed, that restore silently strips it — so this is
/// the tripwire for it.
#[test]
fn no_research_changes_how_fast_a_unit_walks() {
    let all: u64 = ALL_TECHS.iter().fold(0u64, |m, t| m | tech_bit(*t));
    for k in UnitKind::ALL {
        assert_eq!(
            effective_unit_def(*k, all).speed,
            unit_def(*k).speed,
            "{k:?} changes speed with research; movement's march-pace restore must learn the mask"
        );
    }
}

// ── lockstep ─────────────────────────────────────────────────────────────────

fn twin(seed: u32) -> (App, App) {
    (arena(seed), arena(seed))
}

/// Two worlds, the same batch, hashes compared EVERY tick. Group orders write
/// order/anchor/slot/march-pace state on every member at once, which is exactly
/// the shape of change that desyncs quietly.
#[test]
fn group_orders_are_bit_identical_on_two_peers() {
    let seed = 48514;
    let (cx, cy) = flat_block(seed, 40, 40);
    let (mut a, mut b) = twin(seed);
    let kinds = [UnitKind::Spearman, UnitKind::Archer, UnitKind::Knight, UnitKind::Ram];
    for app in [&mut a, &mut b] {
        for i in 0..24u64 {
            let k = kinds[i as usize % kinds.len()];
            put(app, 1 + i, 1, k, tile(cx + 2 + (i as i32 % 6), cy + 2 + (i as i32 / 6)));
        }
        for i in 0..12u64 {
            put(app, 100 + i, 2, UnitKind::Spearman, tile(cx + 26 + (i as i32 % 4), cy + 26));
        }
    }
    // the SAME ordered batch on both, including a selection listed backwards on
    // one peer: the group must be built from GameId order, not click order
    let mine: Vec<u64> = (1..=24).collect();
    let backwards: Vec<u64> = (1..=24).rev().collect();
    let script: Vec<(usize, PlayerCommand, PlayerCommand)> = vec![
        (
            2,
            PlayerCommand::GroupMove {
                player_id: 1,
                units: mine.clone(),
                target: tile(cx + 20, cy + 20),
                formation: FormationShape::Box as u8,
            },
            PlayerCommand::GroupMove {
                player_id: 1,
                units: backwards.clone(),
                target: tile(cx + 20, cy + 20),
                formation: FormationShape::Box as u8,
            },
        ),
        (
            60,
            PlayerCommand::AttackMove {
                player_id: 1,
                units: mine.clone(),
                target: tile(cx + 28, cy + 28),
                formation: FormationShape::Wedge as u8,
            },
            PlayerCommand::AttackMove {
                player_id: 1,
                units: backwards.clone(),
                target: tile(cx + 28, cy + 28),
                formation: FormationShape::Wedge as u8,
            },
        ),
        (
            200,
            PlayerCommand::GroupAttack { player_id: 1, units: mine.clone(), target: 104 },
            PlayerCommand::GroupAttack { player_id: 1, units: backwards.clone(), target: 104 },
        ),
        (
            400,
            PlayerCommand::Stop { player_id: 1, units: mine.clone() },
            PlayerCommand::Stop { player_id: 1, units: backwards.clone() },
        ),
    ];

    for t in 0..900usize {
        for (at, ca, cb) in &script {
            if *at == t {
                push(&mut a, ca.clone());
                push(&mut b, cb.clone());
            }
        }
        step(a.world_mut());
        step(b.world_mut());
        assert_eq!(
            a.world().resource::<StateHash>().0,
            b.world().resource::<StateHash>().0,
            "desync at tick {t}"
        );
    }
    // and it was a real match, not two frozen worlds
    let world = a.world_mut();
    let mut q = world.query::<&Owner>();
    assert!(q.iter(world).count() < 36, "nobody died in 45 seconds of fighting");
}

/// The whole point of hashing the order layer: a march pace or a standing order
/// that differs between peers has to CHANGE the hash, or the desync surfaces
/// ticks later as drifted positions and the real tick is unrecoverable.
#[test]
fn a_peer_that_ordered_differently_hashes_differently() {
    let seed = 48514;
    let (cx, cy) = flat_block(seed, 30, 30);
    let (mut a, mut b) = twin(seed);
    for app in [&mut a, &mut b] {
        for i in 0..6u64 {
            put(app, 1 + i, 1, UnitKind::Spearman, tile(cx + 2 + i as i32, cy + 2));
        }
    }
    let ids: Vec<u64> = (1..=6).collect();
    push(
        &mut a,
        PlayerCommand::GroupMove {
            player_id: 1,
            units: ids.clone(),
            target: tile(cx + 20, cy + 20),
            formation: FormationShape::Line as u8,
        },
    );
    push(
        &mut b,
        PlayerCommand::GroupMove {
            player_id: 1,
            units: ids.clone(),
            target: tile(cx + 20, cy + 20),
            formation: FormationShape::Column as u8,
        },
    );
    step(a.world_mut());
    step(b.world_mut());
    assert_ne!(
        a.world().resource::<StateHash>().0,
        b.world().resource::<StateHash>().0,
        "two different formations hashed the same"
    );
}

/// The mirror of `naval.rs::a_hull_handed_a_leg_across_land_still_never_stands_on_it`.
///
/// A land leg is only SAMPLED when it is laid — twice a tile — so a leg that
/// crosses a one-tile river diagonally can be sampled on both banks and read as
/// clear. The man then walks over the water and a ford stops being the only way
/// across. Found by the devctl soak on seeds 4 and 6: a Spearman and a peasant
/// standing mid-River with legal paths in hand.
#[test]
fn a_walker_handed_a_leg_across_a_river_never_stands_in_it() {
    use saladin_sim::{Biome, WORLD_SIZE, sample_terrain};
    let seed = saladin_sim::compose_seed(2, 2); // River Valley: rivers to cross

    // a river tile with dry ground on both sides along one axis
    let half = saladin_sim::fx!("0.5");
    let wet = |tx: i32, ty: i32| {
        matches!(
            sample_terrain(seed, Fx::from_num(tx) + half, Fx::from_num(ty) + half).biome,
            Biome::River
        ) && !is_passable(seed, tx, ty)
    };
    let mut crossing = None;
    'find: for ty in 24..WORLD_SIZE - 24 {
        for tx in 24..WORLD_SIZE - 24 {
            if wet(tx, ty)
                && (1..=3).all(|d| is_passable(seed, tx - d, ty))
                && (1..=3).all(|d| is_passable(seed, tx + d, ty))
            {
                crossing = Some((tx, ty));
                break 'find;
            }
        }
    }
    let (rx, ry) = crossing.expect("River Valley has a river with banks");
    let from = V2::new(Fx::from_num(rx - 3) + half, Fx::from_num(ry) + half);
    let to = V2::new(Fx::from_num(rx + 3) + half, Fx::from_num(ry) + half);

    let mut app = arena(seed);
    // straight at the far bank, through the water — no pathfinder involved
    put(&mut app, 1, 1, UnitKind::Spearman, from);
    {
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &mut Unit)>();
        let (_, mut u) = q.iter_mut(world).find(|(g, _)| g.0 == 1).expect("the man");
        u.path = vec![to];
        u.path_idx = 0;
        u.target = to;
        u.has_target = true;
    }

    let mut checked = 0;
    for _ in 0..400 {
        step(app.world_mut());
        let p = pos_of(&mut app, 1);
        let (tx, ty) = (p.x.to_num::<i32>(), p.y.to_num::<i32>());
        assert!(is_passable(seed, tx, ty), "the man forded the river at {tx},{ty}");
        checked += 1;
    }
    assert!(checked >= 400);
}
