//! What the RENDERER thinks it is drawing, as JSON.
//!
//! `{"query": "state"}` describes the simulation. Nothing in it can tell you
//! that a unit has no mesh, that a razed building left its root behind, or that
//! two things are stacked on one tile — and those are what a rendering bug
//! looks like from the outside. This answers the render half: one row per
//! drawn thing, plus the sums that make a leak obvious.
//!
//! `{"query": "clock"}` freezes and advances the animation clock. Every pose in
//! `animate_units` is a function of wall time, so two screenshots of one
//! unchanged world are never the same image and a pixel diff means nothing.
//! Paused, they are identical; advanced by an exact amount, they are
//! reproducible — which is what makes a baseline worth keeping.

use bevy::prelude::*;
use bevy::time::Virtual;
use saladin_protocol::devctl::ClientAsk;
use serde_json::{Value, json};

use crate::render::sync::{
    AnimState, AnimalNode, BuildingSelRing, FishNode, HpBar, RallyFlag, RenderMap, RenderRoot,
    UnitBody,
};

pub fn answer(world: &mut World, ask: ClientAsk) {
    match ask.query.clone().as_str() {
        "render" => {
            let v = inventory(world);
            ask.ok(v);
        }
        "clock" => {
            let v = clock(world, &ask);
            ask.ok(v);
        }
        other => ask.err(format!("unknown query: {other} (render-side names: render, clock)")),
    }
}

/// When the world should stop of its own accord. Asking over the socket and
/// then pausing cannot be exact — frames keep rendering between the poll and
/// the request being served, and one frame of slip is a different pose and a
/// different image.
#[derive(Resource, Default)]
pub struct FreezeAt(pub Option<u64>);

/// Pause the instant the sim reaches the tick asked for, and not a frame later.
pub fn freeze_at_tick(
    at: Res<FreezeAt>,
    tick: Res<saladin_protocol::Tick>,
    mut time: ResMut<Time<Virtual>>,
    mut done: ResMut<FreezeDone>,
) {
    if let Some(want) = at.0
        && tick.0 >= want
        && !time.is_paused()
    {
        time.pause();
        done.0 = true;
    }
}

/// Set once a `pause_at` has fired, so a caller polling `clock` can tell the
/// difference between "not yet" and "stopped".
#[derive(Resource, Default)]
pub struct FreezeDone(pub bool);

/// Freeze or step the animation clock. Pausing stops the fixed-update sim too,
/// which is the point: a frame you can diff is a frame nothing is moving in.
fn clock(world: &mut World, ask: &ClientAsk) -> Value {
    if let Some(at) = ask.req.get("pause_at").and_then(|v| v.as_u64()) {
        world.resource_mut::<FreezeAt>().0 = Some(at);
        world.resource_mut::<FreezeDone>().0 = false;
    }
    let tick = world.resource::<saladin_protocol::Tick>().0;
    let armed = world.resource::<FreezeAt>().0;
    let fired = world.resource::<FreezeDone>().0;
    let mut time = world.resource_mut::<Time<Virtual>>();
    let mut disarm = false;
    if let Some(pause) = ask.flag("pause") {
        if pause {
            time.pause();
        } else {
            time.unpause();
            // only an explicit unpause disarms. Doing it on any reply where the
            // clock happens to be running means a caller POLLING for the freeze
            // cancels the very thing it is waiting for, and waits forever.
            disarm = true;
        }
    }
    let advance = ask.number("advance", 0.0);
    if advance > 0.0 {
        time.advance_by(std::time::Duration::from_secs_f32(advance));
    }
    let paused = time.is_paused();
    let elapsed = time.elapsed_secs();
    if disarm {
        world.resource_mut::<FreezeAt>().0 = None;
        world.resource_mut::<FreezeDone>().0 = false;
    }
    json!({
        "paused": paused,
        "frozen": fired && paused,
        "pause_at": armed,
        "tick": tick,
        "elapsed": elapsed,
    })
}

fn xyz(v: Vec3) -> Value {
    json!([v.x, v.y, v.z])
}

/// How far a drawn thing may sit from the row it is drawing before it is a bug
/// rather than an ease. `interpolate` chases the sim position over a few
/// frames, and a unit at full speed covers well under a tile in that time.
const DRIFT_MAX: f32 = 3.0;

/// The highest the drawn water ever reaches (`WATERLINE_Y`). Anything afloat is
/// clamped to it; a hull below it is under the sea and a hull well above it is
/// standing on the beach.
const WATERLINE_Y: f32 = -0.015;

fn inventory(world: &mut World) -> Value {
    let ids: Vec<u64> = world.resource::<RenderMap>().0.keys().copied().collect();
    let sim: std::collections::HashSet<u64> = {
        let mut q = world.query::<&saladin_protocol::GameId>();
        q.iter(world).map(|g| g.0).collect()
    };
    // where the SIM says each drawn thing is, and what it is
    let seed = world.resource::<saladin_protocol::WorldConfig>().seed;
    let mut placed: std::collections::HashMap<u64, (Vec2, bool, bool)> = Default::default();
    let mut adrift: Vec<(u64, Vec2, saladin_sim::BuildingKind)> = Vec::new();
    {
        let mut q = world.query::<(
            &saladin_protocol::GameId,
            &saladin_protocol::Pos,
            Option<&saladin_protocol::Unit>,
            Option<&saladin_protocol::Building>,
        )>();
        for (g, p, u, b) in q.iter(world) {
            let afloat = u.is_some_and(|u| saladin_sim::unit_def(u.kind).afloat());
            let at = Vec2::new(p.pos.x.to_num::<f32>(), p.pos.y.to_num::<f32>());
            placed.insert(g.0, (at, u.is_some(), afloat));
            // A hall standing in the sea. `check_place` refuses this outright,
            // so in a real match it cannot happen — which is exactly why it is
            // worth checking: it means something bypassed the placement rules.
            if let Some(b) = b
                && !saladin_sim::is_buildable_tile(
                    seed,
                    p.pos.x.to_num::<i32>(),
                    p.pos.y.to_num::<i32>(),
                )
            {
                adrift.push((g.0, at, b.kind));
            }
        }
    }
    let mut problems: Vec<Value> = Vec::new();
    let mut note = |id: u64, rule: &str, detail: String| {
        problems.push(json!({ "id": id, "rule": rule, "detail": detail }));
    };
    for (id, at, kind) in adrift {
        note(
            id,
            "building on ground it could never be founded on",
            format!("{kind:?} at {:.0},{:.0}", at.x, at.y),
        );
    }

    // A root whose sim row is gone is a leak: it keeps drawing, it keeps its
    // meshes alive, and nothing will ever despawn it.
    let orphans: Vec<u64> = {
        let mut out: Vec<u64> = ids.iter().copied().filter(|id| !sim.contains(id)).collect();
        out.sort_unstable();
        out
    };

    let mut roots: Vec<(u64, Value)> = Vec::new();
    let mut mapped: Vec<(u64, Entity)> =
        world.resource::<RenderMap>().0.iter().map(|(k, v)| (*k, *v)).collect();
    mapped.sort_by_key(|(id, _)| *id);
    for (id, e) in mapped {
        let Ok(tf) = world.query::<&Transform>().get(world, e) else {
            roots.push((id, json!({ "id": id, "problem": "root has no transform" })));
            continue;
        };
        let pos = tf.translation;
        let vis = world.get::<Visibility>(e).map(|v| format!("{v:?}"));
        let anim = world.get::<AnimState>(e).map(|a| {
            json!({
                "kind": format!("{:?}", a.kind),
                "moving": a.moving,
                "combat": a.combat,
                "routing": a.routing,
                "harvest": a.harvest,
            })
        });
        // rig parts are children; a unit with none is an invisible unit
        let mut parts: Vec<String> = Vec::new();
        let mut meshes = 0usize;
        if let Some(kids) = world.get::<Children>(e).map(|c| c.iter().collect::<Vec<_>>()) {
            for k in kids {
                if let Some(b) = world.get::<UnitBody>(k) {
                    parts.push(format!("{:?}", b.group));
                }
                if world.get::<Mesh3d>(k).is_some() {
                    meshes += 1;
                }
            }
        }
        if world.get::<Mesh3d>(e).is_some() {
            meshes += 1;
        }
        if let Some((at, is_unit, afloat)) = placed.get(&id).copied() {
            let drift = Vec2::new(pos.x, pos.z).distance(at);
            if drift > DRIFT_MAX {
                note(
                    id,
                    "drawn away from the row it draws",
                    format!("{drift:.1} tiles from the sim at {at:?}"),
                );
            }
            if afloat && (pos.y - WATERLINE_Y).abs() > 0.35 {
                note(id, "hull off the waterline", format!("y {:.3}", pos.y));
            }
            if is_unit && !afloat && pos.y < WATERLINE_Y - 0.25 {
                note(id, "unit drawn under the sea", format!("y {:.3}", pos.y));
            }
        }
        if meshes == 0 {
            note(id, "drawing nothing", "no mesh anywhere in its tree".into());
        }
        if !sim.contains(&id) {
            note(id, "root outliving its row", "nothing will ever despawn it".into());
        }
        roots.push((
            id,
            json!({
                "id": id,
                "pos": xyz(pos),
                "yaw": tf.rotation.to_euler(EulerRot::YXZ).0,
                "scale": xyz(tf.scale),
                "visibility": vis,
                "meshes": meshes,
                "parts": parts,
                "anim": anim,
                "in_sim": sim.contains(&id),
            }),
        ));
    }

    // things with no mesh anywhere in their tree draw nothing at all
    let invisible: Vec<u64> = roots
        .iter()
        .filter(|(_, v)| v["meshes"].as_u64() == Some(0))
        .map(|(id, _)| *id)
        .collect();

    // A row the sim has and the renderer does not is a thing you cannot see.
    // Nodes and players are not all drawn, so only units and buildings count.
    {
        let drawn: std::collections::HashSet<u64> =
            world.resource::<RenderMap>().0.keys().copied().collect();
        let mut q = world.query_filtered::<
            &saladin_protocol::GameId,
            bevy::ecs::query::Or<(
                With<saladin_protocol::Unit>,
                With<saladin_protocol::Building>,
            )>,
        >();
        let mut undrawn: Vec<u64> =
            q.iter(world).map(|g| g.0).filter(|id| !drawn.contains(id)).collect();
        undrawn.sort_unstable();
        for id in undrawn.iter().take(16) {
            note(*id, "row with nothing drawing it", "the sim has it, the screen does not".into());
        }
    }

    let count = |world: &mut World, name: &str| -> usize {
        match name {
            "hp_bars" => world.query::<&HpBar>().iter(world).count(),
            "sel_rings" => world.query::<&BuildingSelRing>().iter(world).count(),
            "rally_flags" => world.query::<&RallyFlag>().iter(world).count(),
            "fish" => world.query::<&FishNode>().iter(world).count(),
            "animals" => world.query::<&AnimalNode>().iter(world).count(),
            _ => 0,
        }
    };
    let tallies = json!({
        "roots": roots.len(),
        "sim_rows": sim.len(),
        "orphans": orphans.len(),
        "invisible": invisible.len(),
        "hp_bars": count(world, "hp_bars"),
        "sel_rings": count(world, "sel_rings"),
        "rally_flags": count(world, "rally_flags"),
        "fish": count(world, "fish"),
        "animals": count(world, "animals"),
        "meshes_loaded": world.resource::<Assets<Mesh>>().len(),
        "materials_loaded": world.resource::<Assets<StandardMaterial>>().len(),
        "render_roots_total": world.query::<&RenderRoot>().iter(world).count(),
    });

    json!({
        "tally": tallies,
        "clean": problems.is_empty(),
        "problems": problems,
        "orphans": orphans,
        "invisible": invisible,
        "roots": roots.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
    })
}
