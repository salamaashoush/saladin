//! Read-only projections of the match.
//!
//! Every handler here takes `&World` and every query goes through
//! `World::try_query`, which needs no registration pass — a component nothing
//! has ever spawned simply reads as an empty list. That is not fastidiousness:
//! the ordinary `world.query()` wants `&mut World`, and a read that can mutate
//! is a read that can desync a lockstep peer.

use super::wire::{at, fx_from, fx_json, v2_from, v2_json};
use crate::components::*;
use crate::{MatchStats, MatchStatuses, StateHash, Tick, WorldConfig};
use bevy_ecs::prelude::*;
use saladin_sim::{
    ALL_TECHS, Fx, UnitKind, V2, building_def, dist2, match_simulates, operational, seed_base,
    seed_preset, tech_bit, unit_def,
};
use serde_json::{Map, Value, json};

pub(super) fn query(world: &mut World, v: &Value, req: &Map<String, Value>, reply: super::Reply) {
    let Some(name) = v.as_str() else {
        return reply.err("query takes a name");
    };
    match name {
        "tick" => reply.ok(tick_report(world)),
        "feedback" => reply.ok(json!({ "feedback": super::drain_feedback(world) })),
        "state" => match scope(req) {
            Ok(scope) => reply.ok(capture(world, &scope)),
            Err(e) => reply.err(e),
        },
        "probe" => match probe(world, req) {
            Ok(v) => reply.ok(v),
            Err(e) => reply.err(e),
        },
        "path" => match path(world, req) {
            Ok(v) => reply.ok(v),
            Err(e) => reply.err(e),
        },
        "terrain" => match terrain(world, req) {
            Ok(v) => reply.ok(v),
            Err(e) => reply.err(e),
        },
        "invariants" => reply.ok(invariants(world)),
        "planner" => match planner(world, req) {
            Ok(v) => reply.ok(v),
            Err(e) => reply.err(e),
        },
        "gather" => match gather_probe(world, req) {
            Ok(v) => reply.ok(v),
            Err(e) => reply.err(e),
        },
        // anything else belongs to whoever owns the renderer, if there is one
        other if world.resource::<super::DevctlLink>().renders => {
            super::ask_host(world, other, req, reply)
        }
        other => reply.err(format!(
            "unknown query: {other} \
             (expected one of: tick, state, probe, path, terrain, planner, gather, \
             invariants, feedback; render-side queries need a client)"
        )),
    }
}

pub(super) fn tick_report(world: &World) -> Value {
    let link = *world.resource::<super::DevctlLink>();
    let paused = world.resource::<MatchStatuses>().0.values().any(|s| !match_simulates(*s));
    json!({
        "tick": world.resource::<Tick>().0,
        "hash": world.resource::<StateHash>().0,
        "seed": world.resource::<WorldConfig>().seed,
        "paused": paused,
        "submit_tick": link.submit_tick,
        "may_step": link.may_step,
        "renders": link.renders,
    })
}

// ── scoping ──────────────────────────────────────────────────────────────────

const KINDS: [&str; 5] = ["units", "buildings", "nodes", "players", "matches"];

struct Scope {
    kinds: Vec<String>,
    player: Option<u64>,
    /// Centre and SQUARED radius — the sim's own range test, no sqrt.
    near: Option<(V2, Fx)>,
}

impl Scope {
    fn wants(&self, kind: &str) -> bool {
        self.kinds.iter().any(|k| k == kind)
    }

    fn admits(&self, owner: Option<u64>, pos: Option<V2>) -> bool {
        if let (Some(want), Some(o)) = (self.player, owner)
            && want != o
        {
            return false;
        }
        match (self.near, pos) {
            (Some((c, r2)), Some(p)) => dist2(c, p) <= r2,
            (Some(_), None) => false,
            _ => true,
        }
    }
}

fn scope(req: &Map<String, Value>) -> Result<Scope, String> {
    let kinds = match req.get("kinds") {
        None | Some(Value::Null) => KINDS.iter().map(|k| k.to_string()).collect(),
        Some(Value::Array(a)) => {
            let mut out = Vec::with_capacity(a.len());
            for v in a {
                let k = v.as_str().ok_or("kinds takes strings")?;
                if !KINDS.contains(&k) {
                    return Err(format!("unknown kind: {k} (expected one of: {})", KINDS.join(", ")));
                }
                out.push(k.to_string());
            }
            out
        }
        Some(_) => return Err(format!("kinds takes an array of: {}", KINDS.join(", "))),
    };
    let player = match req.get("player") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_u64().ok_or("player takes a player id")?),
    };
    let near = match req.get("near") {
        None | Some(Value::Null) => None,
        Some(Value::Object(n)) => {
            let pos = v2_from(at(n, "pos")?).map_err(|e| format!("near.pos: {e}"))?;
            let radius = fx_from(at(n, "radius")?).map_err(|e| format!("near.radius: {e}"))?;
            Some((pos, radius * radius))
        }
        Some(_) => return Err("near takes {\"pos\": [x, y], \"radius\": r}".into()),
    };
    Ok(Scope { kinds, player, near })
}

// ── the capture ──────────────────────────────────────────────────────────────

fn capture(world: &World, scope: &Scope) -> Value {
    let mut out = Map::new();
    out.insert("tick".into(), json!(world.resource::<Tick>().0));
    out.insert("hash".into(), json!(world.resource::<StateHash>().0));
    out.insert("seed".into(), json!(world.resource::<WorldConfig>().seed));
    if scope.wants("matches") {
        out.insert("matches".into(), json!(matches(world)));
    }
    if scope.wants("players") {
        out.insert("players".into(), json!(players(world, scope)));
    }
    if scope.wants("units") {
        out.insert("units".into(), json!(units(world, scope)));
    }
    if scope.wants("buildings") {
        out.insert("buildings".into(), json!(buildings(world, scope)));
    }
    if scope.wants("nodes") {
        out.insert("nodes".into(), json!(nodes(world, scope)));
    }
    Value::Object(out)
}

/// Rows come out sorted by `GameId` — an agent diffs two captures, and archetype
/// order is not stable enough to diff against.
fn by_id(mut rows: Vec<(u64, Value)>) -> Vec<Value> {
    rows.sort_by_key(|(id, _)| *id);
    rows.into_iter().map(|(_, v)| v).collect()
}

fn matches(world: &World) -> Vec<Value> {
    let Some(mut q) = world.try_query::<&MatchInfo>() else { return Vec::new() };
    let rows: Vec<(u64, Value)> = q
        .iter(world)
        .map(|m| {
            (
                m.match_id,
                json!({
                    "match_id": m.match_id,
                    "name": m.name,
                    "host": m.host,
                    "status": m.status,
                    "seed": m.seed,
                    "seed_base": seed_base(m.seed),
                    "preset": seed_preset(m.seed),
                }),
            )
        })
        .collect();
    by_id(rows)
}

fn players(world: &World, scope: &Scope) -> Vec<Value> {
    let Some(mut q) = world.try_query::<(Entity, &Player, &MatchId)>() else { return Vec::new() };
    let stats = world.resource::<MatchStats>();
    let rows: Vec<(u64, Value)> = q
        .iter(world)
        .filter(|(_, p, _)| scope.player.is_none_or(|want| want == p.player_id))
        .map(|(e, p, m)| {
            let tally = stats.0.get(&p.player_id).copied().unwrap_or_default();
            let (pop, cap) = population(world, p.player_id);
            (
                p.player_id,
                json!({
                    "player_id": p.player_id,
                    "match": m.0,
                    "name": p.name,
                    "faction": p.faction,
                    "stock": {
                        "wood": p.stock.wood,
                        "stone": p.stock.stone,
                        "food": p.stock.food,
                        "gold": p.stock.gold,
                    },
                    "color": p.color,
                    "online": p.online,
                    "keep": p.keep,
                    "defeated": p.defeated,
                    "slot": p.slot,
                    "hunger": p.hunger,
                    "pop": pop,
                    "pop_cap": cap,
                    "tech_mask": p.tech_mask,
                    "techs": ALL_TECHS
                        .iter()
                        .filter(|t| p.tech_mask & tech_bit(**t) != 0)
                        .map(|t| format!("{t:?}"))
                        .collect::<Vec<_>>(),
                    "bot": world.get::<Bot>(e).map(|b| json!({
                        "host": b.host,
                        "difficulty": b.difficulty,
                        "phase": b.phase,
                        "scout": b.scout_id,
                        "wave_launched": b.wave_launched,
                        "famine": b.famine,
                    })),
                    "research": research(world, p.player_id),
                    "stats": {
                        "trained": tally.trained,
                        "lost": tally.lost,
                        "gathered": tally.gathered,
                    },
                }),
            )
        })
        .collect();
    by_id(rows)
}

fn research(world: &World, owner: u64) -> Vec<Value> {
    let Some(mut q) = world.try_query::<&Research>() else { return Vec::new() };
    let mut rows: Vec<&Research> = q.iter(world).filter(|r| r.owner == owner).collect();
    rows.sort_by_key(|r| r.tech);
    rows.iter()
        .map(|r| {
            json!({
                "tech": r.tech,
                "name": saladin_sim::Tech::from_u8(r.tech).map(|t| format!("{t:?}")),
                "progress": fx_json(r.progress),
                "done": r.done,
            })
        })
        .collect()
}

/// What the owner is housing and what he could house. Recomputed rather than
/// stored, exactly as `pop_room` does it — one number, one authority.
fn population(world: &World, owner: u64) -> (i32, i32) {
    let pop = match world.try_query::<(&Owner, &Unit)>() {
        Some(mut q) => q
            .iter(world)
            .filter(|(o, _)| o.0 == owner)
            .map(|(_, u)| unit_def(u.kind).pop_cost)
            .sum(),
        None => 0,
    };
    let cap = match world.try_query::<(&Owner, &Building)>() {
        Some(mut q) => q
            .iter(world)
            .filter(|(o, b)| o.0 == owner && operational(b.state))
            .map(|(_, b)| building_def(b.kind).pop)
            .sum(),
        None => 0,
    };
    (pop, cap)
}

fn order_name(order: u8) -> &'static str {
    match order {
        ORDER_MOVE => "Move",
        ORDER_ATTACK_MOVE => "AttackMove",
        ORDER_ATTACK => "Attack",
        ORDER_STOP => "Stop",
        _ => "None",
    }
}

fn units(world: &World, scope: &Scope) -> Vec<Value> {
    let Some(mut q) = world.try_query::<(&GameId, &Owner, &MatchId, &Pos, &Unit)>() else {
        return Vec::new();
    };
    let rows: Vec<(u64, Value)> = q
        .iter(world)
        .filter(|(_, o, _, p, _)| scope.admits(Some(o.0), Some(p.pos)))
        .map(|(g, o, m, p, u)| {
            let def = unit_def(u.kind);
            (
                g.0,
                json!({
                    "id": g.0,
                    "owner": o.0,
                    "match": m.0,
                    "kind": u.kind,
                    "role": def.role,
                    "domain": format!("{:?}", def.domain),
                    "pos": v2_json(p.pos),
                    "hp": u.hp,
                    "max_hp": def.max_hp,
                    "speed": fx_json(u.speed),
                    "stance": u.stance,
                    "order": order_name(u.order),
                    "order_target": v2_json(u.order_target),
                    "target": v2_json(u.target),
                    "has_target": u.has_target,
                    "heading": u.heading,
                    "morale": fx_json(u.morale),
                    "routing": u.routing,
                    "ration": fx_json(u.ration),
                    "gather_state": u.gather_state,
                    "target_node": u.target_node,
                    "carrying": u.carrying,
                    "carry_type": u.carry_type,
                    "job_site": u.job_site,
                    "garrisoned_in": u.garrisoned_in,
                    "attack_target": u.attack_target,
                    "path_len": u.path.len(),
                }),
            )
        })
        .collect();
    by_id(rows)
}

fn buildings(world: &World, scope: &Scope) -> Vec<Value> {
    let Some(mut q) = world.try_query::<(&GameId, &Owner, &MatchId, &Pos, &Building)>() else {
        return Vec::new();
    };
    let rows: Vec<(u64, Value)> = q
        .iter(world)
        .filter(|(_, o, _, p, _)| scope.admits(Some(o.0), Some(p.pos)))
        .map(|(g, o, m, p, b)| {
            let def = building_def(b.kind);
            (
                g.0,
                json!({
                    "id": g.0,
                    "owner": o.0,
                    "match": m.0,
                    "kind": b.kind,
                    "pos": v2_json(p.pos),
                    "footprint": def.footprint,
                    "hp": b.hp,
                    "max_hp": def.max_hp,
                    "state": b.state,
                    "complete": b.complete(),
                    // `work` IS the fraction of the job done (`work_step`
                    // returns a share of `build_time`), so it needs no
                    // scaling — but a finished building banks no more of it,
                    // and reporting a topped-out keep as 0.00 reads as broken.
                    "progress": fx_json(match b.state {
                        saladin_sim::BuildState::Complete => Fx::ONE,
                        _ => b.work,
                    }),
                    "builders": b.builders,
                    "target_kind": b.target_kind,
                    "queue": b.queued()
                        .iter()
                        .map(|k| match UnitKind::from_u8(*k) {
                            Some(k) => json!(k),
                            None => json!(k),
                        })
                        .collect::<Vec<_>>(),
                    "train_work": fx_json(b.train_work),
                    "rally": v2_json(b.rally),
                    "cooldown": fx_json(b.cooldown),
                }),
            )
        })
        .collect();
    by_id(rows)
}

fn nodes(world: &World, scope: &Scope) -> Vec<Value> {
    let Some(mut q) = world.try_query::<(Entity, &GameId, &MatchId, &Pos, &ResourceNode)>() else {
        return Vec::new();
    };
    let rows: Vec<(u64, Value)> = q
        .iter(world)
        .filter(|(_, _, _, p, _)| scope.admits(None, Some(p.pos)))
        .map(|(e, g, m, p, n)| {
            let field = world.get::<FieldOf>(e);
            let crop = world.get::<Crop>(e);
            (
                g.0,
                json!({
                    "id": g.0,
                    "match": m.0,
                    "pos": v2_json(p.pos),
                    "res": n.res_type,
                    "remaining": n.remaining,
                    "cap": n.cap,
                    "regen": n.regen,
                    "reapable": reapable(n, field.is_some(), crop),
                    "field_of": field.map(|f| f.0),
                    "crop": crop.map(|c| json!({ "ripe": c.ripe, "standing": c.standing })),
                }),
            )
        })
        .collect();
    by_id(rows)
}

// ── the instruments ──────────────────────────────────────────────────────────

/// Would this placement be accepted, and if not, why? A DRY RUN: it asks the
/// command's own gathering (`BuildContext::check`), so it can never answer
/// differently from the order it stands in for, and it costs nothing — no tick,
/// no stockpile, no site to cancel. Probing by issuing a real Build and reading
/// the refusal works, but it churns the world and only answers once a tick.
fn probe(world: &World, req: &Map<String, Value>) -> Result<Value, String> {
    let player = at(req, "player")?.as_u64().ok_or("player takes a player id")?;
    let kind: saladin_sim::BuildingKind = serde_json::from_value(at(req, "kind")?.clone())
        .map_err(|e| format!("field \"kind\": {e}"))?;
    let seed = world.resource::<WorldConfig>().seed;
    let ctx = crate::commands::build_context(world, player)
        .ok_or_else(|| format!("no player {player} in this match"))?;

    let one = |pos: V2| {
        let verdict = ctx.check(seed, kind, pos);
        json!({
            "pos": v2_json(pos),
            "ok": verdict.is_ok(),
            "error": verdict.err().map(|e| format!("{e:?}")),
            "text": verdict.err().map(saladin_sim::place_error_text),
        })
    };
    match req.get("pos") {
        Some(v) => Ok(json!({ "results": [one(v2_from(v).map_err(|e| format!("pos: {e}"))?)] })),
        // a whole square at once: an agent siting a base asks about a REGION,
        // and one request beats sixty round trips
        None => {
            let c = v2_from(at(req, "near")?).map_err(|e| format!("near: {e}"))?;
            let r = at(req, "radius")?.as_i64().ok_or("radius takes tiles")?.clamp(0, 24) as i32;
            let (cx, cy) = (c.x.to_num::<i32>(), c.y.to_num::<i32>());
            let half = saladin_sim::fx!("0.5");
            let mut out = Vec::new();
            for dy in -r..=r {
                for dx in -r..=r {
                    out.push(one(V2::new(
                        Fx::from_num(cx + dx) + half,
                        Fx::from_num(cy + dy) + half,
                    )));
                }
            }
            Ok(json!({ "results": out }))
        }
    }
}

/// Can this unit walk there, and by what route? The closure is built exactly as
/// `movement` and `construction` build theirs — same domain, same occupancy,
/// same gates — so a disagreement between what the placement rules allow and
/// what a builder can actually reach shows up here as a fact rather than as a
/// foundation stuck at zero work.
///
/// Allocates its own `AStar` and `Flood` rather than borrowing `PathScratch`:
/// a query that writes a sim resource is a query that can desync a peer, and a
/// few megabytes once per debug request is not a hot path.
fn path(world: &World, req: &Map<String, Value>) -> Result<Value, String> {
    use saladin_sim::{
        AStar, Domain, Flood, MAX_EXPANSIONS, approach_tile, domain_passable, gate_blocks,
        move_cost_at, nearest_reachable_passable_grid, reach_budget, unit_def,
    };
    let seed = world.resource::<WorldConfig>().seed;
    let to = v2_from(at(req, "to")?).map_err(|e| format!("to: {e}"))?;

    let (from, owner, domain, unit) = match req.get("unit") {
        Some(v) => {
            let id = v.as_u64().ok_or("unit takes a game id")?;
            let mut q = world
                .try_query::<(&GameId, &Owner, &Pos, &Unit)>()
                .ok_or("this world has no units")?;
            let row = q
                .iter(world)
                .find(|(g, ..)| g.0 == id)
                .ok_or_else(|| format!("no unit {id}"))?;
            (row.2.pos, row.1.0, unit_def(row.3.kind).domain, Some(id))
        }
        None => (
            v2_from(at(req, "from")?).map_err(|e| format!("from: {e}"))?,
            req.get("player").and_then(|v| v.as_u64()).unwrap_or(0),
            match req.get("domain").and_then(|v| v.as_str()) {
                Some("Sea") => Domain::Sea,
                _ => Domain::Land,
            },
            None,
        ),
    };

    let (occ, gates) = crate::commands::occupancy_and_gates(world, false);
    let passable = |tx: i32, ty: i32| {
        let k = saladin_sim::tile_key(tx, ty);
        domain_passable(seed, domain, tx, ty)
            && !occ.contains(&k)
            && !gate_blocks(&gates, k, owner)
    };
    let mut flood = Flood::default();
    let reach = nearest_reachable_passable_grid(
        &mut flood,
        &passable,
        from,
        to,
        reach_budget(saladin_sim::dist(from, to)),
    );
    let truncated = reach.as_ref().map(|r| r.truncated);
    let mut astar = AStar::default();
    let cost = |tx: i32, ty: i32| move_cost_at(seed, tx, ty);
    let mut route_to = |s: V2| {
        astar.find_path_costed_in(
            &passable,
            &cost,
            from.x,
            from.y,
            s.x,
            s.y,
            MAX_EXPANSIONS,
            domain.smoothing(),
        )
    };
    // `walk_to`'s two-step, and for the same reason: the tidy approach tile can
    // be a dead end, and an instrument that reports the dead end as "no route"
    // sends its reader hunting a bug the game does not have. This one did,
    // for an hour.
    let tidy = approach_tile(seed, &passable, from, to, 3);
    let mut snap = tidy;
    let mut route = tidy.map(&mut route_to).unwrap_or_default();
    if route.is_empty()
        && let Some(r) = reach
    {
        snap = Some(r.at);
        route = route_to(r.at);
    }
    Ok(json!({
        "unit": unit,
        "from": v2_json(from),
        "to": v2_json(to),
        "domain": format!("{domain:?}"),
        "reachable": !route.is_empty(),
        "snap": snap.map(v2_json),
        "truncated": truncated,
        "legs": route.len(),
        "route": route.iter().map(|p| v2_json(*p)).collect::<Vec<_>>(),
    }))
}

/// The ground as the sim sees it, drawn. One request answers "why can nothing
/// walk from here to there" that a thousand `path` calls only hint at, and an
/// ASCII block is readable by an agent and by a human over `nc` alike.
///
/// `player` overlays that owner's builder reach (`town_reach`), which is the
/// set the placement rules read — seeing the two together is how a placement
/// that disagrees with the pathfinder gets caught.
fn terrain(world: &World, req: &Map<String, Value>) -> Result<Value, String> {
    let c = v2_from(at(req, "near")?).map_err(|e| format!("near: {e}"))?;
    let r = at(req, "radius")?.as_i64().ok_or("radius takes tiles")?.clamp(1, 60) as i32;
    let seed = world.resource::<WorldConfig>().seed;
    let (cx, cy) = (c.x.to_num::<i32>(), c.y.to_num::<i32>());

    let occ = crate::commands::occupancy_and_gates(world, false).0;
    let solid = crate::commands::occupancy_and_gates(world, true).0;
    let nodes: std::collections::HashSet<i32> = match world.try_query::<(&Pos, &ResourceNode)>() {
        Some(mut q) => q
            .iter(world)
            .map(|(p, _)| saladin_sim::tile_key(p.pos.x.to_num::<i32>(), p.pos.y.to_num::<i32>()))
            .collect(),
        None => Default::default(),
    };
    let reach = match req.get("player").and_then(|v| v.as_u64()) {
        Some(p) => crate::commands::build_context(world, p).map(|ctx| ctx.reach_set().clone()),
        None => None,
    };

    let mut rows = Vec::with_capacity((r * 2 + 1) as usize);
    for ty in cy - r..=cy + r {
        let mut row = String::with_capacity((r * 2 + 1) as usize);
        for tx in cx - r..=cx + r {
            let key = saladin_sim::tile_key(tx, ty);
            let ch = if solid.contains(&key) {
                if occ.contains(&key) { 'B' } else { 'g' }
            } else if nodes.contains(&key) {
                'n'
            } else if saladin_sim::is_sailable(seed, tx, ty) {
                '~'
            } else if !saladin_sim::is_passable(seed, tx, ty) {
                '#'
            } else if reach.as_ref().is_some_and(|s| s.contains(&key)) {
                '+'
            } else if reach.is_some() {
                '-'
            } else {
                '.'
            };
            row.push(ch);
        }
        rows.push(row);
    }
    Ok(json!({
        "origin": [cx - r, cy - r],
        "size": r * 2 + 1,
        "rows": rows,
        "legend": {
            "B": "building (blocks a walker)",
            "g": "gatehouse (its owner walks through)",
            "n": "resource node",
            "~": "sailable water",
            "#": "impassable land",
            "+": "open, and inside this player's builder reach",
            "-": "open, but CUT OFF from this player's builders",
            ".": "open (no player asked about)"
        }
    }))
}

/// Everything about the world that must never be true, checked in one pass.
///
/// A soak run asks this every few hundred ticks; each answer is a bug with its
/// row already named, which is the difference between "the bots did something
/// odd on seed 31" and a repro. Cheap enough to ask often: one walk of the
/// units, one of the buildings, one of the nodes.
fn invariants(world: &World) -> Value {
    use saladin_sim::{Domain, WORLD_SIZE, is_passable, is_sailable, unit_def};
    let seed = world.resource::<WorldConfig>().seed;
    let mut bad: Vec<Value> = Vec::new();
    let mut note = |rule: &str, id: u64, what: String| {
        bad.push(json!({ "rule": rule, "id": id, "detail": what }));
    };

    let mut alive: std::collections::HashSet<u64> = Default::default();
    let mut seen: std::collections::HashSet<u64> = Default::default();
    if let Some(mut q) = world.try_query::<&GameId>() {
        for g in q.iter(world) {
            alive.insert(g.0);
            if !seen.insert(g.0) {
                note("duplicate GameId", g.0, "two rows share one id".into());
            }
        }
    }
    let players: std::collections::HashSet<u64> = match world.try_query::<&Player>() {
        Some(mut q) => q.iter(world).map(|p| p.player_id).collect(),
        None => Default::default(),
    };

    if let Some(mut q) = world.try_query::<(&GameId, &Owner, &Pos, &Unit)>() {
        for (g, o, p, u) in q.iter(world) {
            let (tx, ty) = (p.pos.x.to_num::<i32>(), p.pos.y.to_num::<i32>());
            if tx < 0 || ty < 0 || tx >= WORLD_SIZE || ty >= WORLD_SIZE {
                note("unit off the map", g.0, format!("at {tx},{ty}"));
            } else if unit_def(u.kind).domain == Domain::Sea {
                if !is_sailable(seed, tx, ty) {
                    note("hull aground", g.0, format!("{:?} at {tx},{ty}", u.kind));
                }
            } else if u.garrisoned_in == 0 && !is_passable(seed, tx, ty) {
                let biome = saladin_sim::sample_terrain(
                    seed,
                    Fx::from_num(tx) + saladin_sim::fx!("0.5"),
                    Fx::from_num(ty) + saladin_sim::fx!("0.5"),
                )
                .biome;
                note(
                    "walker on impassable ground",
                    g.0,
                    format!(
                        "{:?} at {tx},{ty} on {biome:?} (order {}, routing {}, path {})",
                        u.kind,
                        u.order,
                        u.routing,
                        u.path.len()
                    ),
                );
            }
            if u.hp <= 0 {
                note("dead unit still standing", g.0, format!("hp {}", u.hp));
            }
            if !players.contains(&o.0) {
                note("unit with no player", g.0, format!("owner {}", o.0));
            }
            // A HAULER legitimately remembers the node it just emptied — the
            // whole crew that drew one to zero carries its id home and picks a
            // new one on arrival. Only a hand still walking TO a node, or a
            // passenger of a hull that no longer exists, is a dangling
            // reference. A checker that cries wolf gets ignored.
            let heading_out = matches!(
                u.gather_state,
                saladin_sim::GatherState::ToResource | saladin_sim::GatherState::Harvesting
            );
            for (what, target) in [
                ("attack_target", u.attack_target),
                ("target_node", if heading_out { u.target_node } else { 0 }),
                ("job_site", u.job_site),
                ("garrisoned_in", u.garrisoned_in),
            ] {
                if target != 0 && !alive.contains(&target) {
                    note("order points at a dead row", g.0, format!("{what} = {target}"));
                }
            }
        }
    }

    if let Some(mut q) = world.try_query::<(&GameId, &Owner, &Building)>() {
        for (g, o, b) in q.iter(world) {
            if b.hp <= 0 {
                note("razed building still standing", g.0, format!("hp {}", b.hp));
            }
            if b.builders < 0 {
                note("negative crew", g.0, format!("builders {}", b.builders));
            }
            if b.queue_len as usize > saladin_sim::QUEUE_CAP {
                note("queue past its cap", g.0, format!("len {}", b.queue_len));
            }
            if !players.contains(&o.0) {
                note("building with no player", g.0, format!("owner {}", o.0));
            }
        }
    }

    if let Some(mut q) = world.try_query::<(&GameId, &ResourceNode)>() {
        for (g, n) in q.iter(world) {
            if n.remaining < 0 {
                note("node drawn below zero", g.0, format!("remaining {}", n.remaining));
            }
            if n.cap > 0 && n.remaining > n.cap {
                note("node past its cap", g.0, format!("{} of {}", n.remaining, n.cap));
            }
        }
    }
    if let Some(mut q) = world.try_query::<(&GameId, &FieldOf)>() {
        for (g, f) in q.iter(world) {
            if !alive.contains(&f.0) {
                note("field outliving its farm", g.0, format!("farm {}", f.0));
            }
        }
    }
    if let Some(mut q) = world.try_query::<&Player>() {
        for p in q.iter(world) {
            for (res, v) in
                [("wood", p.stock.wood), ("stone", p.stock.stone), ("food", p.stock.food), ("gold", p.stock.gold)]
            {
                if v < 0 {
                    note("stockpile below zero", p.player_id, format!("{res} {v}"));
                }
            }
        }
    }

    json!({
        "tick": world.resource::<Tick>().0,
        "hash": world.resource::<StateHash>().0,
        "clean": bad.is_empty(),
        "violations": bad,
    })
}

/// What a bot's brain SAW on its last beat, and what it concluded.
///
/// Not a reconstruction: `ai_brain` publishes this at the point the numbers are
/// computed. Tuning the gatherer steer means knowing whether the planner
/// thought food was short, which trade it called scarce, and how many hands its
/// budget allowed — inferring any of that from the world is guesswork that goes
/// stale the first time the planner changes.
fn planner(world: &World, req: &Map<String, Value>) -> Result<Value, String> {
    let dbg = world
        .get_resource::<crate::BotDebug>()
        .ok_or("devctl is not attached to this world, so no brain is publishing")?;
    let want = match req.get("player") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_u64().ok_or("player takes a player id")?),
    };
    let mut rows: Vec<(u64, Value)> = dbg
        .0
        .iter()
        .filter(|(id, _)| want.is_none_or(|w| w == **id))
        .map(|(id, t)| (*id, thoughts(*id, t)))
        .collect();
    rows.sort_by_key(|(id, _)| *id);
    if want.is_some() && rows.is_empty() {
        return Err("that player has no brain, or it has not had a beat yet".into());
    }
    Ok(json!({
        "tick": world.resource::<Tick>().0,
        "bots": rows.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
    }))
}

fn thoughts(id: u64, t: &crate::BotThoughts) -> Value {
    let s = &t.state;
    let kinds = |c: &saladin_sim::Census| {
        UnitKind::ALL
            .iter()
            .filter(|k| c[**k as usize] > 0)
            .map(|k| (format!("{k:?}"), json!(c[*k as usize])))
            .collect::<Map<String, Value>>()
    };
    json!({
        "player_id": id,
        "seen_at_tick": t.tick,
        "phase": t.phase,
        // what the steer branches on, in the order it branches
        "steer": {
            "crisis": t.crisis,
            "food_emergency": t.food_emergency,
            "food_surplus": t.food_surplus,
            "food_cushion": t.food_cushion,
            "scarce_build": t.scarce_build,
            "on_food": t.on_food,
            "want_food": t.want_food,
            "idle_bias": t.idle_bias,
            "field_hands_per_farm": t.labour.per_field,
            "field_hands_budget": t.labour.budget,
        },
        "stock": {
            "wood": s.wood, "stone": s.stone, "food": s.food, "gold": s.gold,
            "campaign_food": s.campaign_food,
        },
        "town": {
            "peasants": s.peasants, "pop": s.pop, "cap": s.cap,
            "farms": s.farms, "fields_ripe": s.fields_ripe,
            "farmland_near": s.farmland_near, "storehouses": s.storehouses,
            "damaged": s.damaged, "builders_busy": s.builders_busy,
            "owned": s.owned.iter().map(|k| format!("{k:?}")).collect::<Vec<_>>(),
            "sites_in_flight": saladin_sim::BuildingKind::ALL
                .iter()
                .filter(|k| s.sites_in_flight[**k as usize] > 0)
                .map(|k| (format!("{k:?}"), json!(s.sites_in_flight[*k as usize])))
                .collect::<Map<String, Value>>(),
        },
        "war": {
            "soldiers": s.soldiers, "sieges": s.sieges, "towers": s.towers,
            "army": kinds(&s.army_composition),
            "enemy": kinds(&s.enemy),
            "enemy_has_walls": s.enemy_has_walls,
            "enemy_towers": s.enemy_towers,
            "threat_near_home": s.threat_near_home,
            "enemy_by_land": s.enemy_by_land,
        },
        "sea": {
            "fisheries": s.fisheries,
            "fishery_centroid": s.fishery_centroid.map(v2_json),
            "offshore_cluster": s.offshore_cluster.map(v2_json),
            "boats": s.boats,
            "ferries": s.ferries,
        },
        // what it decided with all of the above
        "build": t.build.map(|b| json!({
            "action": format!("{:?}", b.action),
            "kind": if b.is_unit {
                UnitKind::from_u8(b.kind).map(|k| format!("{k:?}"))
            } else {
                saladin_sim::BuildingKind::from_u8(b.kind).map(|k| format!("{k:?}"))
            },
            "is_unit": b.is_unit,
            "trainer": b.trainer.map(|k| format!("{k:?}")),
        })),
        "trade": t.trade.map(|d| json!({
            "res": d.res, "amount": d.amount, "buy": d.buy,
        })),
        "targets": {
            "peasants": saladin_sim::dynamic_peasant_target(s, &t.tuning),
            "army": saladin_sim::dynamic_army_target(s, &t.tuning),
        },
    })
}

/// Why this hand is not gathering: every candidate node with the gate that
/// refused it.
///
/// `probe` for timber. The balancer walks four gates — domain, region
/// reachability, whether a stander can get close enough, and whether the node
/// can be cut at all — and a hand that fails all four on every node simply
/// stands still, which from outside is indistinguishable from a planner that
/// never asked it to work. This calls the SAME four helpers the balancer calls,
/// so a verdict here is the verdict there.
fn gather_probe(world: &World, req: &Map<String, Value>) -> Result<Value, String> {
    use saladin_sim::{Domain, Flood, MAX_EXPANSIONS, domain_passable, gate_blocks, unit_def};
    let id = at(req, "unit")?.as_u64().ok_or("unit takes a game id")?;
    let want = req.get("limit").and_then(|v| v.as_u64()).unwrap_or(12) as usize;
    let seed = world.resource::<WorldConfig>().seed;

    let (pos, owner, kind, mtch) = {
        let mut q = world
            .try_query::<(&GameId, &Owner, &Pos, &Unit, &MatchId)>()
            .ok_or("this world has no units")?;
        let row = q.iter(world).find(|(g, ..)| g.0 == id).ok_or_else(|| format!("no unit {id}"))?;
        (row.2.pos, row.1.0, row.3.kind, row.4.0)
    };
    let def = unit_def(kind);
    if def.carry <= 0 {
        return Err(format!("{kind:?} carries nothing, so it never gathers"));
    }
    let dom = def.domain;

    let (occ, gates) = crate::commands::occupancy_and_gates(world, false);
    let passable = |tx: i32, ty: i32| {
        let k = saladin_sim::tile_key(tx, ty);
        domain_passable(seed, dom, tx, ty) && !occ.contains(&k) && !gate_blocks(&gates, k, owner)
    };
    let mut flood = Flood::default();
    let flooded = flood.explore(&passable, pos, MAX_EXPANSIONS);

    let footprints: std::collections::HashMap<u64, i32> = {
        let sizes: std::collections::HashMap<u64, i32> = match world.try_query::<(&GameId, &Building)>() {
            Some(mut q) => q
                .iter(world)
                .map(|(g, b)| (g.0, saladin_sim::building_def(b.kind).footprint))
                .collect(),
            None => Default::default(),
        };
        match world.try_query::<(&GameId, &FieldOf)>() {
            Some(mut q) => q
                .iter(world)
                .map(|(g, f)| (g.0, sizes.get(&f.0).copied().unwrap_or(1)))
                .collect(),
            None => Default::default(),
        }
    };

    let mut rows: Vec<(Fx, Value, &'static str)> = Vec::new();
    if let Some(mut q) =
        world.try_query::<(&GameId, &Pos, &ResourceNode, &MatchId, Option<&FieldOf>, Option<&Crop>)>()
    {
        for (g, p, n, m, f, c) in q.iter(world) {
            if m.0 != mtch {
                continue;
            }
            let d = saladin_sim::dist(pos, p.pos);
            let gate = if crate::systems::node_domain(seed, p.pos) != dom {
                "wrong domain"
            } else if !reapable(n, f.is_some(), c) {
                if f.is_some() { "crop not ripe" } else { "drawn out" }
            } else if !match dom {
                Domain::Land => saladin_sim::node_reachable(seed, pos, p.pos),
                Domain::Sea => saladin_sim::sea_reachable(seed, pos, p.pos),
            } {
                "another region"
            } else if !crate::systems::workable(
                &flood,
                p.pos,
                crate::systems::node_reach(seed, p.pos, footprints.get(&g.0).copied()),
            ) {
                "no standing room the hand can reach"
            } else {
                "ok"
            };
            rows.push((
                d,
                json!({
                    "id": g.0,
                    "res": n.res_type,
                    "pos": v2_json(p.pos),
                    "dist": fx_json(d),
                    "remaining": n.remaining,
                    "gate": gate,
                }),
                gate,
            ));
        }
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    let mut tally: Map<String, Value> = Map::new();
    for (_, _, gate) in &rows {
        let n = tally.get(*gate).and_then(|v| v.as_u64()).unwrap_or(0) + 1;
        tally.insert((*gate).to_string(), json!(n));
    }
    let pick = rows.iter().find(|(_, _, g)| *g == "ok").map(|(_, v, _)| v["id"].clone());
    Ok(json!({
        "unit": id,
        "kind": format!("{kind:?}"),
        "domain": format!("{dom:?}"),
        "pos": v2_json(pos),
        // a flood that never started is the whole answer: the hand is standing
        // somewhere its own domain cannot be walked out of
        "flooded": flooded,
        "nearest_workable": pick,
        "gates": tally,
        "nodes": rows.into_iter().take(want).map(|(_, v, _)| v).collect::<Vec<_>>(),
    }))
}
