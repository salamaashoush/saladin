//! The AI economy stall, pinned down.
//!
//! On seed 48514 every peasant a Hard bot owned sat in `GatherState::ToResource`
//! from t=349 to the end of the match: nothing harvested, wood frozen at 1, food
//! at 0, and the army starved from eleven soldiers to none.
//!
//! The cause was a disagreement between the search that CHOOSES a node and the
//! search that APPROACHES it. `node_reachable` is an uncapped terrain-region
//! test and said yes; `nearest_reachable_passable_grid` was a flood capped at
//! 1024 tiles, a ridge forced its BFS the long way round, and it ran out about
//! two tiles short — so it handed back the tile the walker was already standing
//! on. The walk then looked hopeless, the node was thrown away, and the nearest
//! sibling node had exactly the same defect, so the two alternated forever.
//!
//! These tests hold the fix at three levels: the raw geometry at the stall site,
//! one gatherer on that ground, and a whole bot economy over eight minutes.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::*;

const STALL_SEED: u32 = 48514;

fn build(seed: u32) -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed });
    app
}

fn hard_bot(seed: u32) -> App {
    let mut app = build(seed);
    scatter_world_nodes(app.world_mut(), 1);
    app.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::AddAi {
        player_id: 1,
        host: 1,
        difficulty: AiDifficulty::Hard,
        faction: Faction::Ayyubid,
        match_id: 1,
    });
    step(app.world_mut());
    app
}

fn peasant(pos: V2, state: GatherState, node: u64) -> Unit {
    let def = unit_def(UnitKind::Peasant);
    Unit {
        speed: def.speed,
        gather_state: state,
        target_node: node,
        hp: def.max_hp,
        stance: Stance::Defensive,
        ..Unit::new(UnitKind::Peasant, pos)
    }
}

fn hauled(app: &mut App) -> u64 {
    app.world_mut().resource_mut::<MatchStats>().of(1).gathered
}

/// The exact geometry that defeated the old flood: a walker in a pocket at
/// (131.5, 151.5) and a food node at (129.02, 153.01) less than three tiles
/// away, with a ridge between them.
#[test]
fn the_capped_flood_and_the_region_disagree_at_the_stall_site() {
    let seed = compose_seed(STALL_SEED, 0);
    let pass = |x: i32, y: i32| is_passable(seed, x, y);
    let from = V2::new(fx!("131.5"), fx!("151.5"));
    let node = V2::new(fx!("129.022"), fx!("153.008"));

    // the old bound, and why it lied: it stops looking and reports the tile the
    // walker stands on as the closest it can get
    let mut flood = Flood::new();
    let capped = nearest_reachable_passable_grid(&mut flood, &pass, from, node, 1024).unwrap();
    assert!(capped.truncated, "1024 tiles was never enough here — the flag has to say so");
    assert!(
        dist(capped.at, node) >= dist(from, node),
        "the capped flood gets no closer than the start tile: that is the whole bug"
    );

    // and the region test, which is what the walker is actually entitled to
    let approach = approach_tile(seed, &pass, from, node, 4).expect("same landmass, so there is one");
    assert!(
        dist(approach, node) <= harvest_reach(0),
        "the approach tile has to be inside harvest reach, got {}",
        dist(approach, node).to_num::<f32>()
    );
}

/// One peasant, the real ground, the real node. Before the fix it marched at the
/// node for the rest of the match without ever swinging.
#[test]
fn a_gatherer_behind_the_ridge_reaches_its_node() {
    let seed = compose_seed(STALL_SEED, 0);
    let mut app = build(seed);
    let node_pos = V2::new(fx!("129.022"), fx!("153.008"));
    let start = V2::new(fx!("131.5"), fx!("151.5"));
    app.world_mut().spawn((
        GameId(900),
        MatchId(1),
        Player {
            player_id: 1,
            name: "P".into(),
            faction: Faction::Ayyubid,
            stock: Stockpile { wood: 0, stone: 0, food: 0, gold: 0 },
            color: 0,
            online: true,
            keep: 0,
            defeated: false,
            slot: 0,
            tech_mask: 0,
            hunger: 0,
        },
    ));
    let kdef = building_def(BuildingKind::Keep);
    app.world_mut().spawn((
        GameId(10),
        Owner(1),
        MatchId(1),
        Pos { pos: start, facing: ZERO },
        Building::new(BuildingKind::Keep, kdef.max_hp, start),
    ));
    app.world_mut().spawn((
        GameId(20),
        MatchId(1),
        Pos { pos: node_pos, facing: ZERO },
        ResourceNode::deposit(ResourceType::Food, 500),
    ));
    app.world_mut().spawn((
        GameId(30),
        Owner(1),
        MatchId(1),
        Pos { pos: start, facing: ZERO },
        peasant(start, GatherState::ToResource, 20),
    ));

    let mut nodes_seen: Vec<u64> = Vec::new();
    let mut harvested = false;
    for _ in 0..1200 {
        step(app.world_mut());
        let w = app.world_mut();
        let mut q = w.query::<(&GameId, &Unit)>();
        if let Some((_, u)) = q.iter(w).find(|(g, _)| g.0 == 30) {
            if nodes_seen.last() != Some(&u.target_node) {
                nodes_seen.push(u.target_node);
            }
            harvested |= u.carrying > 0 || u.gather_state == GatherState::Harvesting;
        }
    }
    assert!(harvested, "60 seconds behind a ridge and the peasant never worked the node");
    assert!(
        nodes_seen.len() <= 2,
        "the walker changed its mind about which node to work {} times: {nodes_seen:?}",
        nodes_seen.len() - 1
    );
}

/// The whole economy, eight minutes, on the seed that froze it. The sharp
/// assertion is the pin: `ToResource` for twenty seconds is fine if the walker
/// is crossing the map, and fatal if it is standing still. So the test measures
/// both — a long run with no ground covered is exactly the absorbing state that
/// froze fourteen peasants and the economy behind them.
#[test]
fn a_hard_bot_never_pins_its_workforce() {
    for base in [STALL_SEED, 90210, 7, 12345] {
        never_pins(base);
    }
}

fn never_pins(base: u32) {
    const PIN_TICKS: u32 = 400;
    const PIN_TILES: Fx = fx!("3");
    let mut app = hard_bot(compose_seed(base, 0));
    // per peasant: ticks in an unbroken ToResource run, and where it started it
    let mut run: bevy_platform::collections::HashMap<u64, (u32, V2)> = Default::default();
    let mut pins: Vec<(u64, u32, Fx)> = Vec::new();
    let mut at_200s = 0u64;
    for t in 0..8_000 {
        step(app.world_mut());
        if t == 4_000 {
            at_200s = hauled(&mut app);
        }
        if t % 20 != 0 {
            continue;
        }
        let w = app.world_mut();
        let mut q = w.query::<(&GameId, &Owner, &Pos, &Unit)>();
        let mut hits: Vec<(u64, u32, Fx)> = Vec::new();
        for (g, o, p, u) in q.iter(w) {
            if o.0 != 1 || u.kind != UnitKind::Peasant {
                continue;
            }
            if u.gather_state != GatherState::ToResource {
                run.insert(g.0, (0, p.pos));
                continue;
            }
            let e = run.entry(g.0).or_insert((0, p.pos));
            e.0 += 20;
            if e.0 >= PIN_TICKS {
                let moved = dist(p.pos, e.1);
                if moved < PIN_TILES {
                    hits.push((g.0, e.0, moved));
                }
                *e = (0, p.pos);
            }
        }
        pins.extend(hits);
    }
    assert!(
        pins.is_empty(),
        "seed {base}: peasants pinned in ToResource without covering ground: {:?}",
        &pins[..pins.len().min(6)]
    );

    let at_400s = hauled(&mut app);
    assert!(at_200s > 500, "seed {base}: the bot had hauled only {at_200s} by t=200s");
    // A plateau is the SECOND half hauling nothing. Stating that as a RATIO
    // between the halves punishes a FASTER first half: fortification stopped
    // peasants being shoved inside their own keep, seed 12345 went 1068 -> 1188
    // at t=200s and 1380 -> 1436 at t=400s — better at both samples — and the
    // ratio still read as a stall. An absolute floor plus a real second-half
    // haul catches every freeze the ratio caught (a frozen economy gains ~0)
    // without grading an improvement as a regression.
    assert!(at_400s > 1300, "seed {base}: only {at_400s} hauled in 400 s");
    assert!(
        at_400s - at_200s > 200,
        "seed {base}: the economy plateaued — {at_200s} hauled by t=200s, {at_400s} by t=400s"
    );

    // A stall does not have to look like a pin. Standing every hand down is the
    // same lost economy with a different label, so the employment rate is part
    // of the assertion.
    let w = app.world_mut();
    let mut q = w.query::<(&Owner, &Unit)>();
    let (mut hands, mut standing) = (0, 0);
    for (o, u) in q.iter(w) {
        if o.0 != 1 || u.kind != UnitKind::Peasant {
            continue;
        }
        hands += 1;
        if u.gather_state == GatherState::Idle {
            standing += 1;
        }
    }
    assert!(hands > 0, "seed {base}: the bot has no peasants left at all");
    assert!(
        standing * 4 <= hands,
        "seed {base}: {standing} of {hands} peasants are standing about doing nothing"
    );
}

/// Two bots, two worlds, same seed: the gather rewrite reads terrain regions and
/// a retained flood buffer, and neither may leak iteration order or stale state
/// into the tick.
#[test]
fn two_bot_economies_stay_in_lockstep() {
    let seed = compose_seed(STALL_SEED, 0);
    let mut a = hard_bot(seed);
    let mut b = hard_bot(seed);
    for t in 0..3_000 {
        step(a.world_mut());
        step(b.world_mut());
        let ha = a.world().resource::<StateHash>().0;
        let hb = b.world().resource::<StateHash>().0;
        assert_eq!(ha, hb, "desync at tick {t}");
    }
}

/// The crowd price: idle hands spread over the nodes near them instead of every
/// one of them converging on the single closest.
#[test]
fn idle_gatherers_spread_over_nearby_nodes() {
    let seed = 1u32;
    let mut app = build(seed);
    let c = |t: i32| Fx::from_num(t) + fx!("0.5");
    let (cx, cy) = {
        let mut found = None;
        'outer: for y in 16..128 {
            for x in 16..128 {
                if (0..9).all(|dx| (0..9).all(|dy| is_passable(seed, x + dx, y + dy))) {
                    found = Some((x, y));
                    break 'outer;
                }
            }
        }
        found.expect("no land block")
    };
    app.world_mut().spawn((
        GameId(900),
        MatchId(1),
        Player {
            player_id: 1,
            name: "P".into(),
            faction: Faction::Ayyubid,
            stock: Stockpile { wood: 0, stone: 0, food: 200, gold: 0 },
            color: 0,
            online: true,
            keep: 0,
            defeated: false,
            slot: 0,
            tech_mask: 0,
            hunger: 0,
        },
    ));
    // four equally good wood nodes in a row, and six idle hands
    for i in 0..4 {
        app.world_mut().spawn((
            GameId(20 + i as u64),
            MatchId(1),
            Pos { pos: V2::new(c(cx + 2 * i), c(cy + 6)), facing: ZERO },
            ResourceNode::deposit(ResourceType::Wood, 500),
        ));
    }
    for i in 0..6 {
        let pos = V2::new(c(cx + 3), c(cy));
        app.world_mut().spawn((
            GameId(30 + i as u64),
            Owner(1),
            MatchId(1),
            Pos { pos, facing: ZERO },
            peasant(pos, GatherState::Idle, 0),
        ));
    }
    app.world_mut()
        .resource_mut::<CommandQueue>()
        .0
        .push(PlayerCommand::AutoGather { player_id: 1 });
    step(app.world_mut());

    let w = app.world_mut();
    let mut q = w.query::<(&Owner, &Unit)>();
    let mut per_node: bevy_platform::collections::HashMap<u64, i32> = Default::default();
    for (o, u) in q.iter(w) {
        if o.0 == 1 && u.target_node != 0 {
            *per_node.entry(u.target_node).or_insert(0) += 1;
        }
    }
    assert!(per_node.len() >= 3, "six hands landed on {} nodes", per_node.len());
    assert!(
        per_node.values().all(|&n| n <= 3),
        "one node took {} of six hands",
        per_node.values().max().copied().unwrap_or(0)
    );
}
