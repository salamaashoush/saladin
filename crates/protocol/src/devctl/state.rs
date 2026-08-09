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
        other => {
            reply.err(format!("unknown query: {other} (expected one of: tick, state, feedback)"))
        }
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
                    // returns a share of `build_time`), so it needs no scaling.
                    "progress": fx_json(b.work),
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
