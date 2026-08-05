//! Selection state + the HUD-facing digest (port of the selection bookkeeping
//! in SaladinGame.ts: emitSelection / emitSelectedBuilding).

use crate::LocalPlayer;
use bevy::prelude::*;
use saladin_protocol::{Building, GameId, Owner, Player, Pos, Unit};
use saladin_sim::{
    BuildState, BuildingKind, Faction, Fx, FormationShape, FULL_RATION, ResourceCost, UnitKind, V2,
    building_def, dist, draws_rations, effective_building_def, unit_def,
};

/// The selected unit ids, held SORTED. This was a `HashSet` seeded per process,
/// so the Attack/Gather/Garrison loops iterated in an order that changed between
/// runs of the same build — and the garrison loop decrements its free-slot count
/// as it goes, so which of your men actually got into the tower was decided by
/// hash order. Nothing downstream may depend on that.
#[derive(Resource, Default)]
pub struct Selection {
    units: Vec<u64>,
    pub building: Option<u64>,
}

impl Selection {
    pub fn ids(&self) -> &[u64] {
        &self.units
    }
    pub fn contains(&self, id: &u64) -> bool {
        self.units.binary_search(id).is_ok()
    }
    pub fn insert(&mut self, id: u64) -> bool {
        match self.units.binary_search(&id) {
            Ok(_) => false,
            Err(i) => {
                self.units.insert(i, id);
                true
            }
        }
    }
    pub fn clear(&mut self) {
        self.units.clear();
    }
    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }
    pub fn len(&self) -> usize {
        self.units.len()
    }
    pub fn iter(&self) -> std::slice::Iter<'_, u64> {
        self.units.iter()
    }
    pub fn set(&mut self, ids: impl IntoIterator<Item = u64>) {
        self.units.clear();
        self.units.extend(ids);
        self.units.sort_unstable();
        self.units.dedup();
    }
}

impl<'a> IntoIterator for &'a Selection {
    type Item = &'a u64;
    type IntoIter = std::slice::Iter<'a, u64>;
    fn into_iter(self) -> Self::IntoIter {
        self.units.iter()
    }
}

/// Per-kind tally + averages over the selected units, recomputed each frame for
/// the command card. Cheap relative to rendering; keeps the HUD reactive.
#[derive(Resource, Default)]
pub struct SelectionInfo {
    pub total: usize,
    pub by_kind: [u32; UnitKind::ALL.len()],
    pub has_combat: bool,
    pub avg_hp: f32,
    pub avg_morale: f32,
    pub routing: u32,
    /// Men in the selection on short rations, and the worst ration among them.
    pub short: u32,
    pub worst_ration: f32,
}

/// Shape a group order marches in. Held on the client because it rides in the
/// command rather than in sim state; `FormationShape as u8` is what ships.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
pub struct FormationPick(pub FormationShape);

impl Default for FormationPick {
    /// Box reproduces the old client's grid spread, but at a pitch the units
    /// can actually hold (the hardcoded 0.85 was tighter than two Rams
    /// touching, so engines arrived interpenetrating).
    fn default() -> Self {
        FormationPick(FormationShape::Box)
    }
}

/// Everything the command card renders for the selected building. The card can
/// only show what this carries, so it carries the whole lifecycle: health,
/// construction progress, the crew, the production queue and the upgrade on
/// offer — not just a garrison tally.
#[derive(Resource)]
pub struct SelectedBuilding {
    pub id: Option<u64>,
    pub kind: BuildingKind,
    /// Who raised it — the roster it sells and the name it wears both depend
    /// on this, so the card cannot be drawn without it.
    pub faction: Faction,
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
            faction: Faction::Ayyubid,
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

#[allow(clippy::too_many_arguments)]
pub fn publish_selection(
    local: Res<LocalPlayer>,
    mut selection: ResMut<Selection>,
    mut groups: ResMut<ControlGroups>,
    mut info: ResMut<SelectionInfo>,
    mut sel_building: ResMut<SelectedBuilding>,
    q_units: Query<(&GameId, &Owner, &Unit)>,
    q_buildings: Query<(&GameId, &Owner, &Pos, &Building)>,
    q_players: Query<&Player>,
) {
    // prune ids whose entities died, drop garrisoned units from the live selection
    let mut live: Vec<u64> = Vec::with_capacity(selection.len());
    let mut mine: Vec<u64> = Vec::new();
    let mut by_kind = [0u32; UnitKind::ALL.len()];
    let (mut hp_sum, mut hp_n) = (0.0_f32, 0u32);
    let (mut mor_sum, mut mor_n) = (0.0_f32, 0u32);
    let mut routing = 0u32;
    let mut has_combat = false;
    let (mut short, mut worst) = (0u32, 1.0_f32);

    for (g, o, u) in &q_units {
        if o.0 != local.0 {
            continue;
        }
        mine.push(g.0);
        if !selection.contains(&g.0) || u.garrisoned_in != 0 {
            continue;
        }
        live.push(g.0);
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
        if draws_rations(u.kind) && u.ration < FULL_RATION {
            short += 1;
            worst = worst.min(u.ration.to_num::<f32>());
        }
        if def.max_hp > 0 {
            hp_sum += u.hp as f32 / def.max_hp as f32;
            hp_n += 1;
        }
    }
    selection.set(live);
    // a wiped control group used to clear the selection and select nothing
    mine.sort_unstable();
    for g in groups.0.iter_mut() {
        g.retain(|id| mine.binary_search(id).is_ok());
    }
    info.total = selection.len();
    info.by_kind = by_kind;
    info.has_combat = has_combat;
    info.avg_hp = if hp_n > 0 { hp_sum / hp_n as f32 } else { 1.0 };
    info.avg_morale = if mor_n > 0 { mor_sum / mor_n as f32 } else { 1.0 };
    info.routing = routing;
    info.short = short;
    info.worst_ration = if short > 0 { worst } else { 1.0 };

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
    let me = q_players.iter().find(|p| p.player_id == local.0);
    let mask = me.map(|p| p.tech_mask).unwrap_or(0);
    let base = building_def(b.kind);
    let eff = effective_building_def(b.kind, mask);
    sel_building.id = Some(id);
    sel_building.kind = b.kind;
    sel_building.faction = me.map(|p| p.faction).unwrap_or(Faction::Ayyubid);
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

    /// The order units come out in decides which of them enters a tower (the
    /// garrison loop spends its free slots as it walks the selection), so the
    /// same click has to give the same answer twice.
    #[test]
    fn a_selection_reads_back_sorted_whatever_order_it_was_filled_in() {
        let fills: [&[u64]; 3] = [&[9, 3, 7, 1], &[1, 3, 7, 9], &[7, 9, 1, 3]];
        for f in fills {
            let mut s = Selection::default();
            for id in f {
                s.insert(*id);
            }
            assert_eq!(s.ids(), &[1, 3, 7, 9], "filled {f:?}");
        }
        let mut s = Selection::default();
        s.insert(4);
        assert!(!s.insert(4), "a second click on the same man is not a second man");
        assert_eq!(s.len(), 1);
        s.set([8, 2, 8, 5]);
        assert_eq!(s.ids(), &[2, 5, 8]);
        assert!(s.contains(&5) && !s.contains(&6));
    }

    /// `by_kind` was a hardcoded `[u32; 10]` indexed by discriminant, so
    /// selecting a Sergeant (kind 10) panicked.
    #[test]
    fn the_per_kind_tally_has_a_slot_for_every_kind() {
        let info = SelectionInfo::default();
        for &k in UnitKind::ALL {
            assert!((k as usize) < info.by_kind.len(), "{k:?} has no tally slot");
        }
    }
}
