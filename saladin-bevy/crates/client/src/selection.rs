//! Selection state + the HUD-facing digest (port of the selection bookkeeping
//! in SaladinGame.ts: emitSelection / emitSelectedBuilding).

use crate::LocalPlayer;
use bevy::prelude::*;
use saladin_protocol::{Building, GameId, Owner, Unit};
use saladin_sim::{BuildingKind, building_def, unit_def};
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

/// The selected building's live garrison digest for the command card.
#[derive(Resource, Default)]
pub struct SelectedBuilding {
    pub id: Option<u64>,
    pub kind: BuildingKind,
    pub occupants: i32,
    pub garrison_cap: i32,
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
    q_buildings: Query<(&GameId, &Owner, &Building)>,
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
    match selection.building {
        Some(id) => {
            let found = q_buildings.iter().find(|(g, o, _)| g.0 == id && o.0 == local.0);
            match found {
                Some((_, _, b)) => {
                    let occupants =
                        q_units.iter().filter(|(_, _, u)| u.garrisoned_in == id).count() as i32;
                    sel_building.id = Some(id);
                    sel_building.kind = b.kind;
                    sel_building.occupants = occupants;
                    sel_building.garrison_cap = building_def(b.kind).garrison_cap;
                }
                None => {
                    selection.building = None;
                    sel_building.id = None;
                }
            }
        }
        None => sel_building.id = None,
    }
}
