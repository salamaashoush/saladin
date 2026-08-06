//! Selection state + the HUD-facing digest (port of the selection bookkeeping
//! in SaladinGame.ts: emitSelection / emitSelectedBuilding).

use crate::LocalPlayer;
use bevy::prelude::*;
use saladin_protocol::{Building, Crop, FieldOf, GameId, Owner, Player, Pos, ResourceNode, Unit};
use saladin_sim::{
    AuraTarget, BuildState, BuildingKind, FARM_RIPE_GRACE, Faction, Fx, FormationShape,
    FULL_RATION, ResourceCost, UnitKind, V2, building_def, dist, draws_rations,
    effective_building_def, operational, unit_def,
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
    /// Passengers riding in the selected hulls, and the berths they have. A
    /// laden ferry and an empty one are otherwise the same row on the card, and
    /// unloading is a click on nearby ground with no other tell.
    pub aboard: u32,
    pub berths: u32,
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

/// The standing crop on a selected farm, read off its `FieldOf` node. `cap` IS
/// the soil the sim computed when the plot was sited, so the card can name the
/// ground without asking the terrain a second time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CropInfo {
    pub remaining: i32,
    pub cap: i32,
    pub ripe: bool,
    pub lodging: bool,
    /// Hands in the field — the farm's own committed crew.
    pub hands: i32,
    /// A friendly farm hub covers this plot: faster growth, double the grace.
    pub tended: bool,
}

impl CropInfo {
    pub fn fill(&self) -> f32 {
        if self.cap > 0 { (self.remaining as f32 / self.cap as f32).clamp(0.0, 1.0) } else { 0.0 }
    }

    /// Thin / Fair / Rich, straight off the yield the soil bought. A farm that
    /// carries half again what its neighbour does is a different farm, and this
    /// is the only place the fertility overlay's promise is ever cashed.
    ///
    /// The bands are NOT equal thirds of `FARM_CAP_MIN..FARM_CAP_MAX`. That span
    /// is what `field_cap` could return; real ground lives in the bottom quarter
    /// of it. Measured over every plot a farm can be sown on within a town radius
    /// (12 seeds x 4 presets, 26159 plots): caps run 70..190 but p33 is 85 and
    /// p67 is 101, so equal thirds called four plots in five "Thin" and said
    /// "Rich" of one in 24 — the same collapse the flat `regen` had, one layer up.
    pub fn soil_word(&self) -> &'static str {
        let span = saladin_sim::FARM_CAP_MAX - saladin_sim::FARM_CAP_MIN;
        let over = self.cap - saladin_sim::FARM_CAP_MIN;
        match () {
            _ if over * 8 < span => "Thin",
            _ if over * 4 < span => "Fair",
            _ => "Rich",
        }
    }
}

/// Everything the command card renders for the selected building. The card can
/// only show what this carries, so it carries the whole lifecycle: health,
/// construction progress, the crew, the production queue, the upgrade on offer
/// and — for a farm — the season standing in its furrows.
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
    /// Set only on a standing farm whose field has been sown.
    pub crop: Option<CropInfo>,
    /// Where the farm hub tending this plot stands — the ring is drawn on IT,
    /// not on the farm, so the player sees which granary reaches him and how far.
    pub hub: Option<(BuildingKind, V2)>,
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
            crop: None,
            hub: None,
        }
    }
}

impl SelectedBuilding {
    /// A standing structure below full health — the derived `Damaged` state.
    pub fn damaged(&self) -> bool {
        self.state == BuildState::Complete && self.hp < self.max_hp
    }

    /// A plot that grows a crop. `min_fertility` is what the sim's own
    /// `wants_work` gates on, so the card and the labour system agree by
    /// construction rather than by a matching `kind == Farm` in two places.
    pub fn tends_a_field(&self) -> bool {
        building_def(self.kind).min_fertility > Fx::ZERO && operational(self.state)
    }

    /// Whether sending hands here would achieve anything. A standing farm always
    /// says yes: the crew tends the season in and reaps it when it comes.
    pub fn wants_work(&self) -> bool {
        self.state != BuildState::Complete || self.damaged() || self.tends_a_field()
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
    q_fields: Query<(&FieldOf, &ResourceNode, Option<&Crop>)>,
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
    // Second pass, own units only: a host is named by GameId, so the count is
    // not knowable until the live selection is settled above.
    let (mut aboard, mut berths) = (0u32, 0u32);
    for (g, o, u) in &q_units {
        if o.0 != local.0 {
            continue;
        }
        if u.garrisoned_in != 0 {
            aboard += selection.contains(&u.garrisoned_in) as u32;
        } else if selection.contains(&g.0) {
            berths += unit_def(u.kind).cargo_cap.max(0) as u32;
        }
    }
    info.aboard = aboard;
    info.berths = berths;

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

    // the season standing in the furrows. A hub tends only its OWNER's fields,
    // which is the same rule the economy tick applies
    let mut hub: Option<(Fx, u64, BuildingKind, V2)> = None;
    if base.min_fertility > Fx::ZERO {
        for (hg, o, hp, hb) in &q_buildings {
            if o.0 != local.0 || !operational(hb.state) {
                continue;
            }
            let d = dist(hp.pos, p.pos);
            if !building_def(hb.kind)
                .aura
                .is_some_and(|a| a.target == AuraTarget::Field && d <= a.radius)
            {
                continue;
            }
            // nearest wins, lowest id breaks a tie — two granaries must not
            // trade the ring back and forth on query order
            if hub.is_none_or(|(bd, bid, ..)| d < bd || (d == bd && hg.0 < bid)) {
                hub = Some((d, hg.0, hb.kind, hp.pos));
            }
        }
    }
    sel_building.hub = hub.map(|(_, _, k, at)| (k, at));
    sel_building.crop = q_fields.iter().find(|(f, ..)| f.0 == id).map(|(_, n, c)| {
        let grace = if sel_building.hub.is_some() { FARM_RIPE_GRACE * 2 } else { FARM_RIPE_GRACE };
        CropInfo {
            remaining: n.remaining,
            cap: n.cap.max(1),
            ripe: c.is_some_and(|c| c.ripe),
            lodging: c.is_some_and(|c| c.ripe && c.standing > grace),
            hands: b.builders,
            tended: sel_building.hub.is_some(),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(state: BuildState, hp: i32, max_hp: i32) -> SelectedBuilding {
        SelectedBuilding { id: Some(1), state, hp, max_hp, ..default() }
    }

    fn farm(state: BuildState, hp: i32) -> SelectedBuilding {
        let d = building_def(BuildingKind::Farm);
        SelectedBuilding {
            id: Some(1),
            kind: BuildingKind::Farm,
            target_kind: BuildingKind::Farm,
            state,
            hp,
            max_hp: d.max_hp,
            ..default()
        }
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

    /// A STANDING FARM always wants hands — that is the whole labour loop, and
    /// the card's Send Farmhands button is the only place a player can see it.
    /// The client's answer has to be the sim's (`construction::wants_work`), or
    /// the button offers an order the sim throws away.
    #[test]
    fn a_whole_farm_still_wants_hands_and_a_whole_keep_does_not() {
        let max = building_def(BuildingKind::Farm).max_hp;
        assert!(farm(BuildState::Complete, max).wants_work(), "a whole farm takes farmhands");
        assert!(farm(BuildState::Complete, max).tends_a_field());
        assert!(!farm(BuildState::Complete, max).damaged(), "a whole farm is not hurt");
        // a foundation is not a field yet: the sim sows only on completion
        assert!(!farm(BuildState::Site, 10).tends_a_field());
        assert!(farm(BuildState::Site, 10).wants_work());
        // and nothing else in the game grew a standing labour appetite
        for &kind in BuildingKind::ALL {
            if building_def(kind).min_fertility > Fx::ZERO {
                continue;
            }
            let whole = SelectedBuilding {
                id: Some(1),
                kind,
                state: BuildState::Complete,
                hp: 100,
                max_hp: 100,
                ..default()
            };
            assert!(!whole.wants_work(), "{kind:?} started asking for hands it cannot use");
        }
    }

    /// Thin / Fair / Rich is read off `cap`, and the crop bar is the crop —
    /// never the health bar wearing a second hat.
    #[test]
    fn the_crop_digest_reads_the_field_not_the_building() {
        let c = CropInfo { remaining: 51, cap: 102, hands: 2, ..default() };
        assert_eq!(c.fill(), 0.5);
        assert_eq!(CropInfo { cap: saladin_sim::FARM_CAP_MIN, ..c }.soil_word(), "Thin");
        assert_eq!(CropInfo { cap: saladin_sim::FARM_CAP_MAX, ..c }.soil_word(), "Rich");
        assert_eq!(CropInfo { cap: 0, ..c }.fill(), 0.0, "an unsown plot is not a full one");
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
