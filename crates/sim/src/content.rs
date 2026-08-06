use crate::ai::{PlannerTuning, TacticalTuning};
use crate::biomes::{game_density, gold_density, motherlode_density, rock_density, tree_density};
use crate::constants::{
    FOOD_NODES, FOOD_YIELD, GOLD_NODES, GOLD_YIELD, MOTHERLODE_GOLD_NODES, MOTHERLODE_STONE_NODES,
    PLACER_NODES, SPAWN_MARGIN, STONE_NODES, STONE_YIELD, TREE_COUNT, TREE_WOOD, WORLD_SIZE,
};
use crate::biomes::Biome;
use crate::enums::{Faction, ResourceType};
use crate::math::{Fx, V2};
use crate::research::Tech;
use crate::terrain::{NodeSite, ScatterRule};
use serde::{Deserialize, Serialize};

// ── resources ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
pub struct ResourceDef {
    pub label: &'static str,
    pub color: u32,
    pub icon: &'static str,
}

pub const RESOURCE_DEFS: [ResourceDef; 4] = [
    ResourceDef { label: "Wood", color: 0x4b7f2f, icon: "🪵" },
    ResourceDef { label: "Stone", color: 0x9a9a9a, icon: "🪨" },
    ResourceDef { label: "Food", color: 0xc9a227, icon: "🍞" },
    ResourceDef { label: "Gold", color: 0xffd24a, icon: "🪙" },
];

pub fn resource_def(r: ResourceType) -> &'static ResourceDef {
    &RESOURCE_DEFS[r as usize]
}

// ── where resources actually come from ───────────────────────────────────────
// A biome label alone puts gold wherever the ground is rocky. These rules read
// the geology, climate and RELIEF layers instead, so a deposit sits where its
// formation process would have put it — and, since `slope` is the same rise the
// camera draws and the pathfinder charges for, where the eye expects it.

/// Above this rise per tile topsoil creeps downhill faster than a stand can
/// root; timber fades out to nothing by `TIMBER_SLOPE_MAX`. Both track the
/// client's bare-rock ramp (`terrain::ROCK_LO/HI`, 0.26..0.75) — the last tree
/// has to be gone before the ground under it renders as a rock face.
const TIMBER_SLOPE_T: Fx = crate::fx!("0.22");
pub const TIMBER_SLOPE_MAX: Fx = crate::fx!("0.62");
/// The rise per tile that reads as a bare scarp face.
const SCARP_REF: Fx = crate::fx!("0.35");
/// Where the soil has slid off and left the bedrock quarryable whatever the
/// biome label says — this is what puts outcrops on the foothill flanks, just
/// below where the render starts stripping the ground to rock.
pub(crate) const SCREE_T: Fx = crate::fx!("0.18");
const SCREE_GAIN: Fx = crate::fx!("1.25");
/// How hard broken ground puts a herd off it.
const HERD_SLOPE_PEN: Fx = crate::fx!("2.1");
const HERD_SLOPE_FLOOR: Fx = crate::fx!("0.12");

/// Timber: the biome sets the stand, the rain sets how thick it grows, the
/// slope decides whether it grows at all.
fn timber(s: &NodeSite) -> Fx {
    let taper =
        ((TIMBER_SLOPE_MAX - s.slope) / (TIMBER_SLOPE_MAX - TIMBER_SLOPE_T)).clamp(Fx::ZERO, Fx::ONE);
    tree_density(s.biome) * (crate::fx!("0.6") + s.precip * crate::fx!("0.85")) * taper
}

/// Building stone: exposed rock. Scarps, foothills and mineralized high ground
/// quarry best; a flat meadow yields almost none.
fn quarry(s: &NodeSite) -> Fx {
    let scarp = (s.slope / SCARP_REF).min(Fx::ONE);
    let exposed = rock_density(s.biome) + (s.slope - SCREE_T).max(Fx::ZERO) * SCREE_GAIN;
    exposed
        * (crate::fx!("0.2")
            + scarp * crate::fx!("1.05")
            + s.ore * crate::fx!("0.45")
            + s.height * crate::fx!("0.25"))
}

/// Game herds follow the grazing: fertile, watered, and flat enough to stand on.
fn herds(s: &NodeSite) -> Fx {
    let flat = (Fx::ONE - s.slope * HERD_SLOPE_PEN).max(HERD_SLOPE_FLOOR);
    game_density(s.biome)
        * (crate::fx!("0.45") + s.fertility * crate::fx!("1.15") + s.precip * crate::fx!("0.4"))
        * flat
}

/// A shore's fishery depends on the water it faces: a lake teems, a river runs
/// thin, the open sea sits in between.
fn fishery(s: &NodeSite) -> Fx {
    let Some(water) = s.adjacent_water else { return Fx::ZERO };
    match water {
        Biome::Lake => crate::fx!("0.85"),
        Biome::River => crate::fx!("0.45"),
        Biome::ShallowWater => crate::fx!("0.62"),
        Biome::DeepWater => crate::fx!("0.3"),
        _ => Fx::ZERO,
    }
}

/// Gold veins live in mineralized rock — the orogenic belts and volcanic arcs
/// the plate field marks out, not any old hillside.
fn vein(s: &NodeSite) -> Fx {
    gold_density(s.biome) * (crate::fx!("0.25") + s.ore * crate::fx!("1.8"))
}

/// Placer gold: grains weathered out of the highlands and dropped in the gravel
/// of a channel below them. Cheap, safe, early - and it runs out.
fn placer(s: &NodeSite) -> Fx {
    if !matches!(s.biome, Biome::Wadi | Biome::Sand | Biome::Marsh | Biome::Oasis)
        && !s.adjacent_water.is_some_and(crate::biomes::biome_is_fresh_water)
    {
        return Fx::ZERO;
    }
    (s.ore * crate::fx!("1.1")).min(crate::fx!("0.7"))
}

/// The exploration prize: rich deposits in the remote high country, where the
/// rock is both mineralized and broken enough to have exposed them.
fn motherlode(s: &NodeSite) -> Fx {
    let scarp = (s.slope / SCARP_REF).min(Fx::ONE);
    motherlode_density(s.biome)
        * (crate::fx!("0.15") + s.ore * crate::fx!("1.4") + scarp * crate::fx!("0.45"))
}

/// Scatter rules for all node kinds. Food splits between fisheries and herds.
pub fn node_kinds() -> Vec<ScatterRule> {
    let food_fish = (FOOD_NODES as f64 * 0.4).round() as i32;
    let food_game = FOOD_NODES - food_fish;
    // stone/gold land as tight AoE-style deposits (4-5 / 5-7 nodes per
    // patch); trees clump via the grove mask; animals roam as singles
    vec![
        ScatterRule { res_type: ResourceType::Wood, count: TREE_COUNT, yield_: TREE_WOOD, density: timber, coastal_only: false, clustered: true, patch: (1, 1) },
        ScatterRule { res_type: ResourceType::Stone, count: STONE_NODES, yield_: STONE_YIELD, density: quarry, coastal_only: false, clustered: false, patch: (4, 5) },
        ScatterRule { res_type: ResourceType::Food, count: food_game, yield_: FOOD_YIELD, density: herds, coastal_only: false, clustered: false, patch: (1, 1) },
        ScatterRule { res_type: ResourceType::Food, count: food_fish, yield_: FOOD_YIELD, density: fishery, coastal_only: true, clustered: false, patch: (1, 1) },
        ScatterRule { res_type: ResourceType::Gold, count: GOLD_NODES, yield_: GOLD_YIELD, density: vein, coastal_only: false, clustered: false, patch: (5, 7) },
        ScatterRule { res_type: ResourceType::Gold, count: PLACER_NODES, yield_: GOLD_YIELD / 2, density: placer, coastal_only: false, clustered: false, patch: (2, 3) },
        ScatterRule { res_type: ResourceType::Gold, count: MOTHERLODE_GOLD_NODES, yield_: GOLD_YIELD * 2, density: motherlode, coastal_only: false, clustered: false, patch: (6, 8) },
        ScatterRule { res_type: ResourceType::Stone, count: MOTHERLODE_STONE_NODES, yield_: STONE_YIELD * 2, density: motherlode, coastal_only: false, clustered: false, patch: (5, 7) },
    ]
}

// ── factions ─────────────────────────────────────────────────────────────────

pub fn faction_label(f: Faction) -> &'static str {
    match f {
        Faction::Ayyubid => "Ayyubid",
        Faction::Crusader => "Crusader",
    }
}

pub fn enemy_faction(f: Faction) -> Faction {
    match f {
        Faction::Ayyubid => Faction::Crusader,
        Faction::Crusader => Faction::Ayyubid,
    }
}

const AI_NAMES: [[&str; 8]; 2] = [
    [
        "Al-Afdal", "Al-Adil", "Taqi al-Din", "Gökböri", "Al-Mashtub", "Qaymaz al-Najmi",
        "Husam al-Din", "Badr al-Din",
    ],
    [
        "Reynald de Châtillon", "Guy de Lusignan", "Raymond of Tripoli", "Conrad of Montferrat",
        "Balian of Ibelin", "Gérard de Ridefort", "Humphrey of Toron", "Joscelin of Courtenay",
    ],
];

pub fn ai_name(faction: Faction, index: usize) -> &'static str {
    AI_NAMES[faction as usize][index % 8]
}

// ── AI difficulty profiles ───────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum AiDifficulty {
    Easy = 0,
    Normal = 1,
    Hard = 2,
}

/// Per-difficulty AI tuning: cadence + planner + tactical knobs + research order.
/// Difficulty is decision QUALITY and CADENCE only — never a resource/vision cheat.
#[derive(Clone, Copy, Debug)]
pub struct AiProfile {
    pub label: &'static str,
    pub decision_interval: Fx,
    pub wave_size: i32,
    pub wave_interval: Fx,
    pub first_wave_delay: Fx,
    // planner
    pub peasant_target: i32,
    pub army_target: i32,
    pub core_army: i32,
    pub pop_buffer: i32,
    pub food_floor_mult: i32,
    pub wood_buffer: i32,
    pub max_towers: i32,
    pub wants_cavalry: bool,
    pub wants_siege: bool,
    pub siege_target: i32,
    pub imam_target: i32,
    pub defend_threat: i32,
    pub food_floor: i32,
    pub reserve_peasants: i32,
    pub army_match_margin: i32,
    pub army_cap: i32,
    pub peasant_cap: i32,
    pub mix_size: i32,
    pub wants_market: bool,
    pub wants_fishing: bool,
    pub farm_target: i32,
    pub farm_hands: i32,
    pub gold_floor: i32,
    pub sell_threshold: i32,
    pub wants_expansion: bool,
    pub storehouse_target: i32,
    pub repair_threshold: i32,
    pub builders_per_site: i32,
    // tactical
    pub recall_margin: i32,
    pub recall_fraction: Fx,
    pub raid_fraction: Fx,
    pub scouts: bool,
    pub defend_react_delay: Fx,
    pub raid_react_delay: Fx,
    pub advantage_margin_pct: i32,
    pub retreat_pct: i32,
    pub research: &'static [Tech],
}

const AI_PROFILES: [AiProfile; 3] = [
    // Easy
    AiProfile {
        label: "Easy",
        decision_interval: crate::fx!("2.0"),
        wave_size: 4,
        wave_interval: crate::fx!("45"),
        first_wave_delay: crate::fx!("60"),
        peasant_target: 7,
        army_target: 6,
        core_army: 4,
        pop_buffer: 2,
        food_floor_mult: 6,
        wood_buffer: 30,
        max_towers: 1,
        wants_cavalry: false,
        wants_siege: false,
        siege_target: 0,
        imam_target: 0,
        defend_threat: 4,
        food_floor: 12,
        reserve_peasants: 2,
        army_match_margin: 0,
        army_cap: 6,
        peasant_cap: 7,
        mix_size: 1,
        wants_market: false,
        wants_fishing: false,
        farm_target: 2,
        farm_hands: 1,
        gold_floor: 0,
        sell_threshold: 999_999,
        wants_expansion: false,
        storehouse_target: 0,
        repair_threshold: 0,
        builders_per_site: 1,
        recall_margin: 2,
        recall_fraction: crate::fx!("0.34"),
        raid_fraction: crate::fx!("0"),
        scouts: false,
        defend_react_delay: crate::fx!("6"),
        raid_react_delay: crate::fx!("9999"),
        advantage_margin_pct: -1,
        retreat_pct: 0,
        research: &[],
    },
    // Normal
    AiProfile {
        label: "Normal",
        decision_interval: crate::fx!("1.0"),
        wave_size: 6,
        wave_interval: crate::fx!("35"),
        first_wave_delay: crate::fx!("45"),
        peasant_target: 9,
        army_target: 10,
        core_army: 6,
        pop_buffer: 3,
        food_floor_mult: 6,
        wood_buffer: 40,
        max_towers: 2,
        wants_cavalry: true,
        wants_siege: true,
        siege_target: 1,
        imam_target: 1,
        defend_threat: 3,
        food_floor: 16,
        reserve_peasants: 3,
        army_match_margin: 2,
        army_cap: 18,
        peasant_cap: 14,
        mix_size: 2,
        wants_market: true,
        wants_fishing: true,
        farm_target: 4,
        farm_hands: 2,
        gold_floor: 60,
        sell_threshold: 250,
        wants_expansion: true,
        storehouse_target: 1,
        repair_threshold: 60,
        builders_per_site: 2,
        recall_margin: 1,
        recall_fraction: crate::fx!("0.5"),
        raid_fraction: crate::fx!("0.25"),
        scouts: false,
        defend_react_delay: crate::fx!("3"),
        raid_react_delay: crate::fx!("120"),
        advantage_margin_pct: 0,
        retreat_pct: 25,
        research: &[Tech::ArmorMail, Tech::SharpenedBlades, Tech::FletchedArrows],
    },
    // Hard
    AiProfile {
        label: "Hard",
        decision_interval: crate::fx!("0.6"),
        wave_size: 8,
        wave_interval: crate::fx!("25"),
        first_wave_delay: crate::fx!("30"),
        peasant_target: 11,
        army_target: 14,
        core_army: 9,
        pop_buffer: 4,
        food_floor_mult: 6,
        wood_buffer: 50,
        max_towers: 3,
        wants_cavalry: true,
        wants_siege: true,
        siege_target: 2,
        imam_target: 1,
        defend_threat: 3,
        food_floor: 20,
        reserve_peasants: 4,
        army_match_margin: 4,
        army_cap: 26,
        peasant_cap: 18,
        mix_size: 3,
        wants_market: true,
        wants_fishing: true,
        farm_target: 7,
        farm_hands: 3,
        gold_floor: 100,
        sell_threshold: 200,
        wants_expansion: true,
        storehouse_target: 2,
        repair_threshold: 75,
        builders_per_site: 3,
        recall_margin: 0,
        recall_fraction: crate::fx!("0.6"),
        raid_fraction: crate::fx!("0.34"),
        scouts: true,
        defend_react_delay: crate::fx!("1"),
        raid_react_delay: crate::fx!("75"),
        advantage_margin_pct: 10,
        retreat_pct: 35,
        research: &[
            Tech::ArmorMail,
            Tech::SharpenedBlades,
            Tech::FletchedArrows,
            Tech::ArmorPlate,
            Tech::Conscription,
            Tech::SiegeEngineering,
            Tech::Masonry,
        ],
    },
];

pub fn ai_profile(d: AiDifficulty) -> &'static AiProfile {
    &AI_PROFILES[d as usize]
}

pub fn planner_tuning(p: &AiProfile) -> PlannerTuning {
    PlannerTuning {
        peasant_target: p.peasant_target,
        army_target: p.army_target,
        core_army: p.core_army,
        pop_buffer: p.pop_buffer,
        food_floor_mult: p.food_floor_mult,
        wood_buffer: p.wood_buffer,
        max_towers: p.max_towers,
        wants_cavalry: p.wants_cavalry,
        wants_siege: p.wants_siege,
        siege_target: p.siege_target,
        imam_target: p.imam_target,
        defend_threat: p.defend_threat,
        food_floor: p.food_floor,
        reserve_peasants: p.reserve_peasants,
        army_match_margin: p.army_match_margin,
        army_cap: p.army_cap,
        peasant_cap: p.peasant_cap,
        mix_size: p.mix_size,
        wants_market: p.wants_market,
        wants_fishing: p.wants_fishing,
        farm_target: p.farm_target,
        farm_hands: p.farm_hands,
        gold_floor: p.gold_floor,
        sell_threshold: p.sell_threshold,
        wants_expansion: p.wants_expansion,
        storehouse_target: p.storehouse_target,
        repair_threshold: p.repair_threshold,
        builders_per_site: p.builders_per_site,
    }
}

pub fn tactical_tuning(p: &AiProfile) -> TacticalTuning {
    TacticalTuning {
        defend_threat: p.defend_threat,
        recall_margin: p.recall_margin,
        recall_fraction: p.recall_fraction,
        raid_fraction: p.raid_fraction,
        scouts: p.scouts,
        defend_react_delay: p.defend_react_delay,
        raid_react_delay: p.raid_react_delay,
        advantage_margin_pct: p.advantage_margin_pct,
        retreat_pct: p.retreat_pct,
    }
}

// ── match setup ──────────────────────────────────────────────────────────────

pub const MAX_AI_OPPONENTS: usize = 7;

#[derive(Clone, Debug)]
pub struct MatchPreset {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub enemies: &'static [AiDifficulty],
}

pub const MATCH_PRESETS: [MatchPreset; 4] = [
    MatchPreset { id: "duel", label: "Duel", description: "1 v 1 — a single rival keep.", enemies: &[AiDifficulty::Normal] },
    MatchPreset {
        id: "skirmish",
        label: "Skirmish",
        description: "1 v 2 — outnumbered on open ground.",
        enemies: &[AiDifficulty::Normal, AiDifficulty::Easy],
    },
    MatchPreset {
        id: "last-stand",
        label: "Last Stand",
        description: "1 v 3 — every corner against you.",
        enemies: &[AiDifficulty::Hard, AiDifficulty::Normal, AiDifficulty::Easy],
    },
    MatchPreset {
        id: "free-for-all",
        label: "Free-for-All",
        description: "1 v 7 — the whole map against you.",
        enemies: &[
            AiDifficulty::Hard,
            AiDifficulty::Hard,
            AiDifficulty::Normal,
            AiDifficulty::Normal,
            AiDifficulty::Normal,
            AiDifficulty::Easy,
            AiDifficulty::Easy,
        ],
    },
];

// ── spawn slots ──────────────────────────────────────────────────────────────

/// One team colour per spawn slot (up to MAX_PLAYERS), spaced around the hue wheel.
pub const PLAYER_COLORS: [u32; 8] =
    [0x2e7d32, 0xb71c1c, 0x1565c0, 0x6a1b9a, 0xef6c00, 0x00838f, 0xf9a825, 0xad1457];

/// Lowest free slot in [0, max) not in `used`, else -1.
pub fn alloc_slot(used: &[i32], max: i32) -> i32 {
    for s in 0..max {
        if !used.contains(&s) {
            return s;
        }
    }
    -1
}

/// Deterministic spawn anchor per joining player index (8 well-spread starts:
/// four corners then four edge midpoints, interleaved so early joiners take
/// opposite corners).
pub fn spawn_corner(index: usize) -> V2 {
    let lo = Fx::from_num(SPAWN_MARGIN);
    let hi = Fx::from_num(WORLD_SIZE - SPAWN_MARGIN);
    let mid = Fx::from_num(WORLD_SIZE) / Fx::from_num(2);
    let anchors = [
        V2::new(lo, lo),
        V2::new(hi, hi),
        V2::new(hi, lo),
        V2::new(lo, hi),
        V2::new(mid, lo),
        V2::new(mid, hi),
        V2::new(lo, mid),
        V2::new(hi, mid),
    ];
    anchors[index % anchors.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_kinds_split_food() {
        let n = node_kinds();
        assert_eq!(n.len(), 8); // 5 staples + placer gold + gold/stone motherlodes
        let food_total: i32 = n.iter().filter(|r| r.res_type == ResourceType::Food).map(|r| r.count).sum();
        assert_eq!(food_total, FOOD_NODES);
    }

    #[test]
    fn alloc_slot_fills_gaps() {
        assert_eq!(alloc_slot(&[0, 2], 8), 1);
        assert_eq!(alloc_slot(&[0, 1, 2], 8), 3);
        assert_eq!(alloc_slot(&[0, 1], 2), -1);
    }

    #[test]
    fn spawn_corners_are_distinct_and_spread() {
        let a = spawn_corner(0);
        let b = spawn_corner(1);
        assert_ne!(a, b);
        // first two are opposite corners
        assert!(a.x < b.x && a.y < b.y);
    }

    #[test]
    fn profiles_map_to_tuning() {
        let hard = ai_profile(AiDifficulty::Hard);
        let pt = planner_tuning(hard);
        assert_eq!(pt.army_target, 14);
        assert_eq!(pt.farm_hands, 3);
        let tt = tactical_tuning(hard);
        assert!(tt.scouts);
        // a better bot works its fields harder, difficulty by difficulty
        let hands: Vec<i32> = [AiDifficulty::Easy, AiDifficulty::Normal, AiDifficulty::Hard]
            .iter()
            .map(|d| planner_tuning(ai_profile(*d)).farm_hands)
            .collect();
        assert_eq!(hands, vec![1, 2, 3]);
    }
}
