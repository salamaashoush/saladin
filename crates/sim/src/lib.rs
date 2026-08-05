//! Deterministic, engine-agnostic simulation core for Saladin. No Bevy, no
//! floats: all gameplay math is fixed-point so every client re-simulates to a
//! bit-identical state under lockstep. The Bevy client and the headless server
//! both depend on this crate.

pub mod ai;
pub mod biomes;
pub mod build_panel;
pub mod buildings;
pub mod buildings_defs;
pub mod climate;
pub mod combat;
pub mod constants;
pub mod content;
pub mod economy;
pub mod elevation;
pub mod enums;
pub mod garrison;
pub mod match_state;
pub mod math;
pub mod morale;
pub mod noise;
pub mod pathfinding;
pub mod plates;
pub mod presets;
pub mod research;
pub mod rng;
pub mod spatial;
pub mod tech;
pub mod terrain;
pub mod units;
pub mod worldgrid;

pub use ai::{
    AiPhase, AssaultIntel, BuildAction, BuildDecision, Census, FIELD_UNITS, PlannerState, PlannerTuning,
    SquadRole, TacticalTarget, TacticalTuning, ThreatState, TradeDecision, army_power,
    counter_composition, counter_score, count_own_kind, dynamic_army_target,
    dynamic_peasant_target, eats_food, food_crisis, mustered, next_army_kind, next_build,
    next_phase, next_trade, raid_quota, ranked_counters, recall_count, should_assault,
    should_recall, should_retreat, squad_role, target_for_role,
};
pub use content::{
    AiDifficulty, AiProfile, MATCH_PRESETS, MAX_AI_OPPONENTS, MatchPreset, PLAYER_COLORS,
    ResourceDef, TIMBER_SLOPE_MAX, ai_name, ai_profile, alloc_slot, enemy_faction, faction_label,
    node_kinds, planner_tuning, resource_def, spawn_corner, tactical_tuning,
};
pub use match_state::{MatchStatus, match_simulates};
pub use biomes::{
    Biome, BiomeDef, biome_buildable, biome_def,
    biome_is_fresh_water, biome_is_water, biome_passable, fish_density,
    game_density, gold_density, motherlode_density, move_cost_mul, rock_density, tree_density,
};
pub use climate::{CLIMATES, ClimateArchetype, climate_archetype, highland, whittaker};
pub use plates::{PlateSample, Plates};
pub use pathfinding::{
    AStar, MAX_EXPANSIONS, find_path_grid, line_of_sight, nearest_passable_grid,
    nearest_reachable_passable_grid,
};
pub use terrain::{
    FAIR_MIN_FOOD, FAIR_MIN_STONE, FAIR_MIN_WOOD, FAIR_RADIUS, ScatterRule, ScatteredNode,
    TerrainSample, compose_seed, fair_start_nodes, find_land_near, is_coastal, is_land,
    NodeSite, belt_at, fertility_at, is_passable, node_reachable, node_site, ore_at, passable_grid,
    region_at, slope_at,
    region_grid, surface_height, temp_at, world_climate, move_cost_at, move_cost_grid,
    dominant_region, buildable_grid, find_keep_site, is_buildable_tile, is_water_tile, sample_terrain, scatter_nodes,
    seed_base, seed_bias, seed_preset, start_point,
};
pub use buildings::{
    BUILD_SLOPE_MAX, FOUNDATION_RELIEF, Occupant, PlaceError, Tile, accepts, build_rate,
    cancel_refund, check_build, check_place, composes_with_walls, demolish_refund,
    find_buildable_near, footprint_relief, soil_quality, gate_blocks, hp_step, operational,
    place_error_text, repair_charge, site_start_hp, work_step,
    footprint_center, footprint_tiles, has_passable_approach, occupancy_set,
    tile_key,
};
pub use buildings_defs::{
    ACCEPTS_ALL, ACCEPTS_FOOD, BUILD_CATEGORIES, AuraTarget, BuildCategory, BuildingDef, WorkAura,
    building_def, res_bit,
};
pub use combat::{
    Attacker, CombatAct, DEFENSIVE_LEASH, acquire_target, building_damage, combat_action,
    effective_damage,
};
pub use constants::*;
pub use economy::{
    FOOD_RESERVE_PER_POP, GATHER_PRIORITY, ResourceCost, Stockpile, TradeResult, UpkeepResult,
    apply_upkeep, apply_upkeep_default, balanced_gather_types, food_low, harvest_reach, market_buy,
    market_sale,
};
pub use elevation::{ELEV_BONUS_MAX, ELEV_BONUS_SPAN, elevation, elevation_at, elevation_range_bonus};
pub use enums::*;
pub use garrison::{
    GarrisonOccupant, can_garrison, can_host_garrison, garrison_fire_power, garrison_free_slots,
};
pub use morale::{
    MORALE_MAX, MORALE_MIN, RALLY_THRESHOLD, ROUT_THRESHOLD, has_rallied, is_routing,
    morale_after_hit, morale_recover, should_rout,
};
pub use presets::{MAP_PRESETS, MapBias, MapPreset, NEUTRAL_BIAS, bias_of, map_preset_by_id, map_preset_by_index};
pub use spatial::{CELL_COUNT, CELL_SIZE, CELLS_PER_ROW, cell_coords, cell_of, cells_in_radius, surrounding_cells};
pub use build_panel::{BuildRowState, BuildStatus, build_panel_state, build_row_state};
pub use research::{
    ALL_TECHS, building_hp_delta, ResearchProgressRow, ResearchRowState, ResearchStatus, Tech, UpgradeDef,
    effective_building_def, effective_unit_def, has_tech, research_panel_state, set_tech, tech_bit,
    techs_in_mask, upgrade_def,
};
pub use tech::{all_prereqs, has_prereq, has_prereq_all};
pub use math::{
    Fnv1a, Fx, Located, ONE, StepResult, V2, ZERO, dist, dist2, fx_sqrt, nearest_index,
    nearest_within, step_toward,
};
pub use rng::{Rng, hash2, hash2_u32};
pub use units::{UnitDef, unit_def};
