//! `PlayerCommand` and fixed point over the wire. Hand-written rather than
//! derived: `Fx` serializes as raw bits, and a coordinate has to be readable in
//! a shell pipeline.

use crate::PlayerCommand;
use saladin_sim::{AiDifficulty, BuildingKind, Faction, Fx, ResourceType, Stance, UnitKind, V2};
use serde_json::{Map, Value, json};

// ── PlayerCommand <-> JSON ───────────────────────────────────────────────────

/// Every command name the parser accepts. Kept beside `command_to_json`, whose
/// exhaustive match is what breaks the build when `PlayerCommand` grows.
pub const COMMAND_NAMES: &[&str] = &[
    "Join",
    "AddAi",
    "Move",
    "SetStance",
    "Train",
    "Build",
    "Gather",
    "Attack",
    "SetRally",
    "Garrison",
    "Ungarrison",
    "Demolish",
    "PlaceWall",
    "MarketTrade",
    "MarketBuy",
    "StartResearch",
    "AutoGather",
    "Pause",
    "Resume",
    "Repair",
    "CancelSite",
    "UpgradeBuilding",
    "TrainAt",
    "CancelTrain",
    "GroupMove",
    "AttackMove",
    "GroupAttack",
    "Stop",
    "Embark",
    "Disembark",
];

/// `{"Train": {"player_id": 1, "kind": "Spearman"}}` — serde's externally
/// tagged shape, written out by hand because `Fx` serializes as raw bits and a
/// coordinate has to be readable in a shell pipeline.
pub fn command_to_json(cmd: &PlayerCommand) -> Value {
    use PlayerCommand::*;
    let (name, body) = match cmd {
        Join { player_id, name, faction, match_id } => (
            "Join",
            json!({"player_id": player_id, "name": name, "faction": faction, "match_id": match_id}),
        ),
        AddAi { player_id, host, difficulty, faction, match_id } => (
            "AddAi",
            json!({"player_id": player_id, "host": host, "difficulty": difficulty, "faction": faction, "match_id": match_id}),
        ),
        Move { player_id, unit, target } => {
            ("Move", json!({"player_id": player_id, "unit": unit, "target": v2_json(*target)}))
        }
        SetStance { player_id, unit, stance } => {
            ("SetStance", json!({"player_id": player_id, "unit": unit, "stance": stance}))
        }
        Train { player_id, kind } => ("Train", json!({"player_id": player_id, "kind": kind})),
        Build { player_id, kind, pos, facing, builders } => (
            "Build",
            json!({"player_id": player_id, "kind": kind, "pos": v2_json(*pos), "facing": facing, "builders": builders}),
        ),
        Gather { player_id, unit, node } => {
            ("Gather", json!({"player_id": player_id, "unit": unit, "node": node}))
        }
        Attack { player_id, unit, target } => {
            ("Attack", json!({"player_id": player_id, "unit": unit, "target": target}))
        }
        SetRally { player_id, building, target } => (
            "SetRally",
            json!({"player_id": player_id, "building": building, "target": v2_json(*target)}),
        ),
        Garrison { player_id, unit, building } => {
            ("Garrison", json!({"player_id": player_id, "unit": unit, "building": building}))
        }
        Ungarrison { player_id, building } => {
            ("Ungarrison", json!({"player_id": player_id, "building": building}))
        }
        Demolish { player_id, building } => {
            ("Demolish", json!({"player_id": player_id, "building": building}))
        }
        PlaceWall { player_id, tiles, builders } => (
            "PlaceWall",
            json!({"player_id": player_id, "tiles": tiles, "builders": builders}),
        ),
        MarketTrade { player_id, res, amount } => {
            ("MarketTrade", json!({"player_id": player_id, "res": res, "amount": amount}))
        }
        MarketBuy { player_id, res, amount } => {
            ("MarketBuy", json!({"player_id": player_id, "res": res, "amount": amount}))
        }
        StartResearch { player_id, building, tech } => (
            "StartResearch",
            json!({"player_id": player_id, "building": building, "tech": tech}),
        ),
        AutoGather { player_id } => ("AutoGather", json!({ "player_id": player_id })),
        Pause { player_id } => ("Pause", json!({ "player_id": player_id })),
        Resume { player_id } => ("Resume", json!({ "player_id": player_id })),
        Repair { player_id, unit, building } => {
            ("Repair", json!({"player_id": player_id, "unit": unit, "building": building}))
        }
        CancelSite { player_id, building } => {
            ("CancelSite", json!({"player_id": player_id, "building": building}))
        }
        UpgradeBuilding { player_id, building } => {
            ("UpgradeBuilding", json!({"player_id": player_id, "building": building}))
        }
        TrainAt { player_id, building, kind } => {
            ("TrainAt", json!({"player_id": player_id, "building": building, "kind": kind}))
        }
        CancelTrain { player_id, building } => {
            ("CancelTrain", json!({"player_id": player_id, "building": building}))
        }
        GroupMove { player_id, units, target, formation } => (
            "GroupMove",
            json!({"player_id": player_id, "units": units, "target": v2_json(*target), "formation": formation}),
        ),
        AttackMove { player_id, units, target, formation } => (
            "AttackMove",
            json!({"player_id": player_id, "units": units, "target": v2_json(*target), "formation": formation}),
        ),
        GroupAttack { player_id, units, target } => (
            "GroupAttack",
            json!({"player_id": player_id, "units": units, "target": target}),
        ),
        Stop { player_id, units } => ("Stop", json!({"player_id": player_id, "units": units})),
        Embark { player_id, units, boat } => {
            ("Embark", json!({"player_id": player_id, "units": units, "boat": boat}))
        }
        Disembark { player_id, boat, target } => (
            "Disembark",
            json!({"player_id": player_id, "boat": boat, "target": v2_json(*target)}),
        ),
    };
    json!({ name: body })
}

pub fn command_from_json(v: &Value) -> Result<PlayerCommand, String> {
    let Value::Object(outer) = v else {
        return Err("cmd takes an object: {\"Train\": {...}}".into());
    };
    if outer.len() != 1 {
        return Err("cmd takes exactly one PlayerCommand variant".into());
    }
    let (name, body) = outer.iter().next().expect("length checked");
    let empty = Map::new();
    let m = match body {
        Value::Object(m) => m,
        Value::Null => &empty,
        _ => return Err(format!("{name} takes an object of its fields")),
    };
    let p = || u64_at(m, "player_id");
    use PlayerCommand as C;
    let cmd = match name.as_str() {
        "Join" => C::Join {
            player_id: p()?,
            name: str_at(m, "name")?.to_string(),
            faction: enum_at::<Faction>(m, "faction")?,
            match_id: u64_or(m, "match_id", 1)?,
        },
        "AddAi" => C::AddAi {
            player_id: p()?,
            host: u64_at(m, "host")?,
            difficulty: enum_at::<AiDifficulty>(m, "difficulty")?,
            faction: enum_at::<Faction>(m, "faction")?,
            match_id: u64_or(m, "match_id", 1)?,
        },
        "Move" => C::Move { player_id: p()?, unit: u64_at(m, "unit")?, target: v2_at(m, "target")? },
        "SetStance" => C::SetStance {
            player_id: p()?,
            unit: u64_at(m, "unit")?,
            stance: enum_at::<Stance>(m, "stance")?,
        },
        "Train" => C::Train { player_id: p()?, kind: enum_at::<UnitKind>(m, "kind")? },
        "Build" => C::Build {
            player_id: p()?,
            kind: enum_at::<BuildingKind>(m, "kind")?,
            pos: v2_at(m, "pos")?,
            facing: u64_or(m, "facing", 0)? as u8,
            builders: ids_at(m, "builders")?,
        },
        "Gather" => {
            C::Gather { player_id: p()?, unit: u64_at(m, "unit")?, node: u64_at(m, "node")? }
        }
        "Attack" => {
            C::Attack { player_id: p()?, unit: u64_at(m, "unit")?, target: u64_at(m, "target")? }
        }
        "SetRally" => C::SetRally {
            player_id: p()?,
            building: u64_at(m, "building")?,
            target: v2_at(m, "target")?,
        },
        "Garrison" => C::Garrison {
            player_id: p()?,
            unit: u64_at(m, "unit")?,
            building: u64_at(m, "building")?,
        },
        "Ungarrison" => C::Ungarrison { player_id: p()?, building: u64_at(m, "building")? },
        "Demolish" => C::Demolish { player_id: p()?, building: u64_at(m, "building")? },
        "PlaceWall" => {
            C::PlaceWall { player_id: p()?, tiles: tiles_at(m)?, builders: ids_at(m, "builders")? }
        }
        "MarketTrade" => C::MarketTrade {
            player_id: p()?,
            res: enum_at::<ResourceType>(m, "res")?,
            amount: i32_at(m, "amount")?,
        },
        "MarketBuy" => C::MarketBuy {
            player_id: p()?,
            res: enum_at::<ResourceType>(m, "res")?,
            amount: i32_at(m, "amount")?,
        },
        "StartResearch" => C::StartResearch {
            player_id: p()?,
            building: u64_at(m, "building")?,
            tech: tech_at(m)?,
        },
        "AutoGather" => C::AutoGather { player_id: p()? },
        "Pause" => C::Pause { player_id: p()? },
        "Resume" => C::Resume { player_id: p()? },
        "Repair" => {
            C::Repair { player_id: p()?, unit: u64_at(m, "unit")?, building: u64_at(m, "building")? }
        }
        "CancelSite" => C::CancelSite { player_id: p()?, building: u64_at(m, "building")? },
        "UpgradeBuilding" => {
            C::UpgradeBuilding { player_id: p()?, building: u64_at(m, "building")? }
        }
        "TrainAt" => C::TrainAt {
            player_id: p()?,
            building: u64_at(m, "building")?,
            kind: enum_at::<UnitKind>(m, "kind")?,
        },
        "CancelTrain" => C::CancelTrain { player_id: p()?, building: u64_at(m, "building")? },
        "GroupMove" => C::GroupMove {
            player_id: p()?,
            units: ids_at(m, "units")?,
            target: v2_at(m, "target")?,
            formation: u64_or(m, "formation", 0)? as u8,
        },
        "AttackMove" => C::AttackMove {
            player_id: p()?,
            units: ids_at(m, "units")?,
            target: v2_at(m, "target")?,
            formation: u64_or(m, "formation", 0)? as u8,
        },
        "GroupAttack" => C::GroupAttack {
            player_id: p()?,
            units: ids_at(m, "units")?,
            target: u64_at(m, "target")?,
        },
        "Stop" => C::Stop { player_id: p()?, units: ids_at(m, "units")? },
        "Embark" => {
            C::Embark { player_id: p()?, units: ids_at(m, "units")?, boat: u64_at(m, "boat")? }
        }
        "Disembark" => C::Disembark {
            player_id: p()?,
            boat: u64_at(m, "boat")?,
            target: v2_at(m, "target")?,
        },
        other => {
            return Err(format!(
                "unknown PlayerCommand variant: {other} (expected one of: {})",
                COMMAND_NAMES.join(", ")
            ));
        }
    };
    Ok(cmd)
}

// ── field readers ────────────────────────────────────────────────────────────

pub(crate) fn at<'a>(m: &'a Map<String, Value>, k: &str) -> Result<&'a Value, String> {
    m.get(k).ok_or_else(|| format!("missing field \"{k}\""))
}

fn u64_at(m: &Map<String, Value>, k: &str) -> Result<u64, String> {
    at(m, k)?.as_u64().ok_or_else(|| format!("field \"{k}\" takes a non-negative integer"))
}

fn u64_or(m: &Map<String, Value>, k: &str, default: u64) -> Result<u64, String> {
    match m.get(k) {
        None | Some(Value::Null) => Ok(default),
        Some(_) => u64_at(m, k),
    }
}

fn i32_at(m: &Map<String, Value>, k: &str) -> Result<i32, String> {
    let n = at(m, k)?.as_i64().ok_or_else(|| format!("field \"{k}\" takes an integer"))?;
    i32::try_from(n).map_err(|_| format!("field \"{k}\" is out of range"))
}

fn str_at<'a>(m: &'a Map<String, Value>, k: &str) -> Result<&'a str, String> {
    at(m, k)?.as_str().ok_or_else(|| format!("field \"{k}\" takes a string"))
}

fn enum_at<T: serde::de::DeserializeOwned>(m: &Map<String, Value>, k: &str) -> Result<T, String> {
    serde_json::from_value(at(m, k)?.clone()).map_err(|e| format!("field \"{k}\": {e}"))
}

fn tech_at(m: &Map<String, Value>) -> Result<u8, String> {
    match at(m, "tech")? {
        Value::String(_) => Ok(enum_at::<saladin_sim::Tech>(m, "tech")? as u8),
        v => v
            .as_u64()
            .and_then(|n| u8::try_from(n).ok())
            .ok_or_else(|| "field \"tech\" takes a Tech name or index".to_string()),
    }
}

fn ids_at(m: &Map<String, Value>, k: &str) -> Result<Vec<u64>, String> {
    match m.get(k) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(a)) => a
            .iter()
            .map(|v| v.as_u64().ok_or_else(|| format!("field \"{k}\" takes game ids")))
            .collect(),
        Some(_) => Err(format!("field \"{k}\" takes an array of game ids")),
    }
}

fn tiles_at(m: &Map<String, Value>) -> Result<Vec<(i32, i32)>, String> {
    let Some(Value::Array(a)) = m.get("tiles") else {
        return Err("field \"tiles\" takes an array of [x, y] tiles".into());
    };
    a.iter()
        .map(|v| match v {
            Value::Array(p) if p.len() == 2 => {
                let x = p[0].as_i64().ok_or("tile x must be an integer")?;
                let y = p[1].as_i64().ok_or("tile y must be an integer")?;
                Ok((x as i32, y as i32))
            }
            _ => Err("each tile is [x, y]".to_string()),
        })
        .collect()
}

fn v2_at(m: &Map<String, Value>, k: &str) -> Result<V2, String> {
    v2_from(at(m, k)?).map_err(|e| format!("field \"{k}\": {e}"))
}

pub(crate) fn v2_from(v: &Value) -> Result<V2, String> {
    let (x, y) = match v {
        Value::Array(a) if a.len() == 2 => (&a[0], &a[1]),
        Value::Object(o) => (at(o, "x")?, at(o, "y")?),
        _ => return Err("a position is [x, y] or {\"x\": .., \"y\": ..}".into()),
    };
    Ok(V2::new(fx_from(x)?, fx_from(y)?))
}

/// A JSON number to `Fx` without a float in the path. `Fx::from_num(f64)` would
/// do it in one line and put an f64 in the protocol crate for the sake of one
/// dev command; serde_json hands back the literal decimal, so parse that.
pub(crate) fn fx_from(v: &Value) -> Result<Fx, String> {
    let text = match v {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        _ => return Err(format!("expected a number, got {v}")),
    };
    parse_fx(&text)
}

fn parse_fx(src: &str) -> Result<Fx, String> {
    let s = src.trim();
    let (neg, s) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
    let digits = |t: &str| t.bytes().all(|b| b.is_ascii_digit());
    if (whole.is_empty() && frac.is_empty()) || !digits(whole) || !digits(frac) {
        return Err(format!("not a plain decimal number: {src}"));
    }
    let w: i64 = if whole.is_empty() { 0 } else { whole.parse().map_err(|_| range(src))? };
    if w > i32::MAX as i64 {
        return Err(range(src));
    }
    let mut out = Fx::from_num(w);
    // I32F32 resolves ~9 decimal places; the rest is noise, not precision
    let frac = &frac[..frac.len().min(9)];
    if !frac.is_empty() {
        let num: i64 = frac.parse().map_err(|_| range(src))?;
        out += Fx::from_num(num) / Fx::from_num(10i64.pow(frac.len() as u32));
    }
    Ok(if neg { -out } else { out })
}

fn range(src: &str) -> String {
    format!("number out of range: {src}")
}

pub(crate) fn v2_json(v: V2) -> Value {
    json!([fx_json(v.x), fx_json(v.y)])
}

/// Fixed point out as a plain number. Output only — nothing reads this back
/// into the sim, so the f64 cannot reach gameplay math.
pub(crate) fn fx_json(v: Fx) -> f64 {
    v.to_num::<f64>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_commands() -> Vec<PlayerCommand> {
        use PlayerCommand::*;
        let at = V2::new(Fx::from_num(12), saladin_sim::fx!("30.5"));
        vec![
            Join { player_id: 1, name: "You".into(), faction: Faction::Ayyubid, match_id: 1 },
            AddAi {
                player_id: 1000,
                host: 1,
                difficulty: AiDifficulty::Hard,
                faction: Faction::Crusader,
                match_id: 1,
            },
            Move { player_id: 1, unit: 7, target: at },
            SetStance { player_id: 1, unit: 7, stance: Stance::HoldGround },
            Train { player_id: 1, kind: UnitKind::Spearman },
            Build {
                player_id: 1,
                kind: BuildingKind::Farm,
                pos: at,
                facing: 2,
                builders: vec![7, 8],
            },
            Gather { player_id: 1, unit: 7, node: 42 },
            Attack { player_id: 1, unit: 7, target: 42 },
            SetRally { player_id: 1, building: 3, target: at },
            Garrison { player_id: 1, unit: 7, building: 3 },
            Ungarrison { player_id: 1, building: 3 },
            Demolish { player_id: 1, building: 3 },
            PlaceWall { player_id: 1, tiles: vec![(4, 5), (4, 6)], builders: vec![7] },
            MarketTrade { player_id: 1, res: ResourceType::Wood, amount: 100 },
            MarketBuy { player_id: 1, res: ResourceType::Stone, amount: 50 },
            StartResearch { player_id: 1, building: 3, tech: 2 },
            AutoGather { player_id: 1 },
            Pause { player_id: 1 },
            Resume { player_id: 1 },
            Repair { player_id: 1, unit: 7, building: 3 },
            CancelSite { player_id: 1, building: 3 },
            UpgradeBuilding { player_id: 1, building: 3 },
            TrainAt { player_id: 1, building: 3, kind: UnitKind::Archer },
            CancelTrain { player_id: 1, building: 3 },
            GroupMove { player_id: 1, units: vec![7, 8], target: at, formation: 1 },
            AttackMove { player_id: 1, units: vec![7, 8], target: at, formation: 0 },
            GroupAttack { player_id: 1, units: vec![7, 8], target: 42 },
            Stop { player_id: 1, units: vec![7, 8] },
            Embark { player_id: 1, units: vec![7, 8], boat: 9 },
            Disembark { player_id: 1, boat: 9, target: at },
        ]
    }

    /// `command_to_json`'s match is exhaustive, so a new `PlayerCommand` variant
    /// breaks the build there; this is what makes the PARSER keep up with it.
    #[test]
    fn every_command_round_trips_through_json() {
        let samples = sample_commands();
        assert_eq!(samples.len(), COMMAND_NAMES.len(), "one sample per command variant");
        for cmd in &samples {
            let wire = command_to_json(cmd);
            let name = wire.as_object().unwrap().keys().next().unwrap().clone();
            assert!(COMMAND_NAMES.contains(&name.as_str()), "{name} missing from COMMAND_NAMES");
            let back = command_from_json(&wire).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(
                format!("{back:?}"),
                format!("{cmd:?}"),
                "{name} did not survive the round trip"
            );
        }
    }

    #[test]
    fn a_coordinate_parses_without_a_float() {
        assert_eq!(parse_fx("12").unwrap(), Fx::from_num(12));
        assert_eq!(parse_fx("30.5").unwrap(), saladin_sim::fx!("30.5"));
        assert_eq!(parse_fx("-0.25").unwrap(), saladin_sim::fx!("-0.25"));
        assert_eq!(parse_fx("+7").unwrap(), Fx::from_num(7));
        assert!(parse_fx("1e5").is_err());
        assert!(parse_fx("twelve").is_err());
        assert!(parse_fx("").is_err());
    }

    #[test]
    fn a_bad_request_is_a_value_not_a_panic() {
        assert!(command_from_json(&json!({"Nonsense": {}})).is_err());
        assert!(command_from_json(&json!({"Train": {"player_id": 1}})).is_err());
        assert!(command_from_json(&json!({"Train": {"player_id": 1, "kind": "Spear"}})).is_err());
        assert!(command_from_json(&json!({"Move": {"player_id": 1, "unit": 1}})).is_err());
        assert!(command_from_json(&json!([1, 2])).is_err());
        assert!(
            command_from_json(&json!({"Train": {"player_id": -1, "kind": "Archer"}})).is_err()
        );
    }
}
