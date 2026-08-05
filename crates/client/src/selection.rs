//! Selection state + the HUD-facing digest (port of the selection bookkeeping
//! in SaladinGame.ts: emitSelection / emitSelectedBuilding).

use crate::LocalPlayer;
use bevy::prelude::*;
use saladin_protocol::{Building, GameId, Owner, Player, Pos, Unit};
use saladin_sim::{
    BuildState, BuildingKind, Fx, ResourceCost, UnitKind, V2, building_def, dist,
    effective_building_def, unit_def,
};
use std::collections::HashSet;

#[derive(Resource, Default)]
pub struct Selection {
    pub units: HashSet<u64>,
    pub building: Option<u64>,
}

/// Per-kind tally + averages over the selected units, recomputed each frame for
/// the command card. Cheap relative to rendering; keeps the HUD reactive.
#[derive(Resource, Default)]
pub struct SelectionInfo {
    pub total: usize,
    pub by_kind: [u32; 10],
    pub has_combat: bool,
    pub avg_hp: f32,
    pub avg_morale: f32,
    pub routing: u32,
}

/// Everything the command card renders for the selected building. The card can
/// only show what this carries, so it carries the whole lifecycle: health,
/// construction progress, the crew, the production queue and the upgrade on
/// offer — not just a garrison tally.
#[derive(Resource)]
pub struct SelectedBuilding {
    pub id: Option<u64>,
    pub kind: BuildingKind,
    /// What an `Upgrading` structure is becoming; equal to `kind` otherwise.
    pub target_kind: BuildingKind,
    pub occupants: i32,
    pub garrison_cap: i32,
    pub pos: V2,
    pub hp: i32,
    pub max_hp: i32,
    pub state: BuildState,
    /// Fraction of the founding/upgrade job banked (0..1).
    pub work: f32,
    pub builders: i32,
    pub queue: Vec<UnitKind>,
    /// Fraction of the head-of-queue unit's training time banked (0..1).
    pub train_progress: f32,
    /// Set only when the flag was moved off the building itself.
    pub rally: Option<V2>,
    pub upgrade: Option<(BuildingKind, ResourceCost)>,
}

impl Default for SelectedBuilding {
    fn default() -> Self {
        SelectedBuilding {
            id: None,
            kind: BuildingKind::default(),
            target_kind: BuildingKind::default(),
            occupants: 0,
            garrison_cap: 0,
            pos: V2::new(Fx::ZERO, Fx::ZERO),
            hp: 0,
            max_hp: 1,
            state: BuildState::default(),
            work: 0.0,
            builders: 0,
            queue: Vec::new(),
            train_progress: 0.0,
            rally: None,
            upgrade: None,
        }
    }
}

impl SelectedBuilding {
    /// A standing structure below full health — the derived `Damaged` state.
    pub fn damaged(&self) -> bool {
        self.state == BuildState::Complete && self.hp < self.max_hp
    }

    /// Whether sending hands here would achieve anything.
    pub fn wants_work(&self) -> bool {
        self.state != BuildState::Complete || self.damaged()
    }
}

/// Saved control groups (Ctrl+1..9 store, 1..9 recall).
#[derive(Resource, Default)]
pub struct ControlGroups(pub [Vec<u64>; 10]);

pub fn publish_selection(
    local: Res<LocalPlayer>,
    mut selection: ResMut<Selection>,
    mut info: ResMut<SelectionInfo>,
    mut sel_building: ResMut<SelectedBuilding>,
    q_units: Query<(&GameId, &Owner, &Unit)>,
    q_buildings: Query<(&GameId, &Owner, &Pos, &Building)>,
    q_players: Query<&Player>,
) {
    // prune ids whose entities died, drop garrisoned units from the live selection
    let mut live: HashSet<u64> = HashSet::new();
    let mut by_kind = [0u32; 10];
    let (mut hp_sum, mut hp_n) = (0.0_f32, 0u32);
    let (mut mor_sum, mut mor_n) = (0.0_f32, 0u32);
    let mut routing = 0u32;
    let mut has_combat = false;

    for (g, o, u) in &q_units {
        if !selection.units.contains(&g.0) || o.0 != local.0 || u.garrisoned_in != 0 {
            continue;
        }
        live.insert(g.0);
        by_kind[u.kind as usize] += 1;
        let def = unit_def(u.kind);
        if def.attack > 0 {
            has_combat = true;
            mor_sum += u.morale.to_num::<f32>();
            mor_n += 1;
            if u.routing {
                routing += 1;
            }
        }
        if def.max_hp > 0 {
            hp_sum += u.hp as f32 / def.max_hp as f32;
            hp_n += 1;
        }
    }
    selection.units = live;
    info.total = selection.units.len();
    info.by_kind = by_kind;
    info.has_combat = has_combat;
    info.avg_hp = if hp_n > 0 { hp_sum / hp_n as f32 } else { 1.0 };
    info.avg_morale = if mor_n > 0 { mor_sum / mor_n as f32 } else { 1.0 };
    info.routing = routing;

    // selected building digest (occupants derived from garrisoned_in)
    let Some(id) = selection.building else {
        sel_building.id = None;
        return;
    };
    let Some((_, _, p, b)) = q_buildings.iter().find(|(g, o, _, _)| g.0 == id && o.0 == local.0)
    else {
        selection.building = None;
        sel_building.id = None;
        return;
    };
    let mask = q_players.iter().find(|p| p.player_id == local.0).map(|p| p.tech_mask).unwrap_or(0);
    let base = building_def(b.kind);
    let eff = effective_building_def(b.kind, mask);
    sel_building.id = Some(id);
    sel_building.kind = b.kind;
    sel_building.target_kind = b.target_kind;
    sel_building.occupants = q_units.iter().filter(|(_, _, u)| u.garrisoned_in == id).count() as i32;
    sel_building.garrison_cap = base.garrison_cap;
    sel_building.pos = p.pos;
    sel_building.hp = b.hp;
    sel_building.max_hp = eff.max_hp.max(1);
    sel_building.state = b.state;
    sel_building.work = b.work.to_num::<f32>().clamp(0.0, 1.0);
    sel_building.builders = b.builders;
    sel_building.queue = b.queued().iter().filter_map(|k| UnitKind::from_u8(*k)).collect();
    sel_building.train_progress = match sel_building.queue.first() {
        Some(k) => {
            let t = unit_def(*k).train_time;
            if t > Fx::ZERO { (b.train_work / t).to_num::<f32>().clamp(0.0, 1.0) } else { 1.0 }
        }
        None => 0.0,
    };
    // a rally flag that never moved sits on the building; only report a real one
    sel_building.rally =
        (dist(b.rally, p.pos) > saladin_sim::fx!("1.2")).then_some(b.rally);
    sel_building.upgrade =
        base.upgrades_to.filter(|_| b.state == BuildState::Complete).map(|k| (k, base.upgrade_cost));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(state: BuildState, hp: i32, max_hp: i32) -> SelectedBuilding {
        SelectedBuilding { id: Some(1), state, hp, max_hp, ..default() }
    }

    /// `Damaged` is DERIVED, not a variant: a standing structure below full
    /// health and an unfinished one both want the same hammer.
    #[test]
    fn damage_is_derived_and_every_unfinished_job_wants_hands() {
        assert!(!row(BuildState::Complete, 500, 500).damaged());
        assert!(!row(BuildState::Complete, 500, 500).wants_work());
        assert!(row(BuildState::Complete, 200, 500).damaged());
        assert!(row(BuildState::Complete, 200, 500).wants_work());
        // a site is never "damaged" — it is simply not built yet
        assert!(!row(BuildState::Site, 50, 500).damaged());
        assert!(row(BuildState::Site, 50, 500).wants_work());
        assert!(row(BuildState::Upgrading, 500, 500).wants_work());
    }
}
