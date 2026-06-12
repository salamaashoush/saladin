//! Baked-model loading: GLB files produced by `scripts/bake_models/` parsed
//! straight into vertex-colored `Mesh`es at startup. Every lookup falls back
//! to the procedural builders when the file is missing (assets/models is
//! gitignored, like assets/voices), so a clean checkout still renders.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use bevy::prelude::*;
use saladin_sim::BuildingKind;
use std::path::PathBuf;

fn models_root() -> PathBuf {
    // cwd-relative like the rest of the asset tree; shot.sh and cargo run
    // both run from the repo root
    PathBuf::from("assets/models")
}

/// Parse a single-mesh GLB (positions/normals/COLOR_0/indices) into a Mesh.
/// Vertex colors are linear floats, same convention as the procedural path.
fn load_glb_mesh(path: &std::path::Path) -> Option<Mesh> {
    let bytes = std::fs::read(path).ok()?;
    let glb = gltf::Gltf::from_slice(&bytes).ok()?;
    let blob = glb.blob.as_deref()?;
    let primitive_mesh = glb.meshes().next()?;
    let prim = primitive_mesh.primitives().next()?;
    let reader = prim.reader(|buffer| match buffer.source() {
        gltf::buffer::Source::Bin => Some(blob),
        gltf::buffer::Source::Uri(_) => None,
    });

    let positions: Vec<[f32; 3]> = reader.read_positions()?.collect();
    let normals: Vec<[f32; 3]> = reader.read_normals()?.collect();
    let indices: Vec<u32> = reader.read_indices()?.into_u32().collect();

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    if let Some(colors) = reader.read_colors(0) {
        let colors: Vec<[f32; 4]> = colors.into_rgba_f32().collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    }
    mesh.insert_indices(Indices::U32(indices));
    Some(mesh)
}

fn baked(category: &str, name: &str) -> Option<Mesh> {
    let path = models_root().join(category).join(format!("{name}.glb"));
    let mesh = load_glb_mesh(&path);
    if mesh.is_none() && path.exists() {
        warn!("failed to parse baked model {}", path.display());
    }
    mesh
}

pub fn building_kind_name(kind: BuildingKind) -> &'static str {
    match kind {
        BuildingKind::Keep => "keep",
        BuildingKind::Barracks => "barracks",
        BuildingKind::Tower => "tower",
        BuildingKind::Wall => "wall",
        BuildingKind::Gatehouse => "gatehouse",
        BuildingKind::House => "house",
        BuildingKind::Stable => "stable",
        BuildingKind::Blacksmith => "blacksmith",
        BuildingKind::Market => "market",
        BuildingKind::Granary => "granary",
        BuildingKind::FishingHut => "fishing_hut",
        BuildingKind::SiegeWorkshop => "siege_workshop",
        BuildingKind::Watchtower => "watchtower",
    }
}

fn faction_name(faction: saladin_sim::Faction) -> &'static str {
    match faction {
        saladin_sim::Faction::Ayyubid => "ayyubid",
        saladin_sim::Faction::Crusader => "crusader",
    }
}

/// Baked faction-variant building mesh, falling back to the unsuffixed bake,
/// then to the procedural builder.
pub fn building_mesh(kind: BuildingKind, faction: saladin_sim::Faction) -> Mesh {
    let name = building_kind_name(kind);
    baked("buildings", &format!("{name}_{}", faction_name(faction)))
        .or_else(|| baked("buildings", name))
        .unwrap_or_else(|| super::buildings::building_mesh(kind))
}

/// Baked faction wall arm, or the procedural fallback.
pub fn wall_arm_mesh(faction: saladin_sim::Faction) -> Mesh {
    baked("buildings", &format!("wall_arm_{}", faction_name(faction)))
        .or_else(|| baked("buildings", "wall_arm"))
        .unwrap_or_else(super::buildings::build_wall_arm)
}

/// Per-`ResourceType` node variant names, index-aligned with the
/// `TREE_*`/`FOOD_*` constants and the procedural variant vecs in props.rs.
fn node_variant_names(res: saladin_sim::ResourceType) -> &'static [&'static str] {
    use saladin_sim::ResourceType;
    match res {
        ResourceType::Wood => {
            &["tree_broadleaf", "tree_conifer", "tree_broadleaf_tall", "tree_olive", "tree_palm"]
        }
        ResourceType::Stone => &["stone_a", "stone_b", "stone_c"],
        ResourceType::Food => &[
            "food_deer",
            "food_boar",
            "food_berry",
            "food_deer_grazing",
            "food_deer_carcass",
            "food_boar_carcass",
        ],
        ResourceType::Gold => &["gold_a", "gold_b"],
    }
}

/// Baked node variants, falling back per-variant to the procedural set.
pub fn resource_node_meshes(res: saladin_sim::ResourceType) -> Vec<Mesh> {
    let mut procedural = super::props::resource_node_meshes(res);
    node_variant_names(res)
        .iter()
        .enumerate()
        .map(|(i, name)| {
            baked("props", name).unwrap_or_else(|| {
                std::mem::replace(&mut procedural[i], Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()))
            })
        })
        .collect()
}

/// Baked fish school, or the procedural fallback.
pub fn fish_node_mesh() -> Mesh {
    baked("props", "fish_school").unwrap_or_else(super::props::fish_node_mesh)
}

/// Baked cosmetic decoration meshes, per-index fallback to the procedural
/// set (index order = props.rs PROP_* constants).
pub fn prop_meshes() -> Vec<Mesh> {
    const NAMES: [&str; 8] = [
        "prop_shrub",
        "prop_dune_grass",
        "prop_rock",
        "prop_boulder",
        "prop_reeds",
        "prop_palm",
        "prop_pine",
        "prop_flowers",
    ];
    let mut procedural = super::props::prop_meshes();
    NAMES
        .iter()
        .enumerate()
        .map(|(i, name)| {
            baked("props", name).unwrap_or_else(|| {
                std::mem::replace(&mut procedural[i], Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()))
            })
        })
        .collect()
}

/// Peasant hand tools [axe, pick, sickle] — parented onto the right hand and
/// swapped per activity. Missing bakes → no tools shown (graceful).
pub fn tool_meshes() -> Vec<Mesh> {
    ["tool_axe", "tool_pick", "tool_sickle"]
        .iter()
        .filter_map(|name| baked("props", name))
        .collect()
}

/// Ruin landmark meshes (render-only secrets scattered by vegetation.rs).
/// No procedural fallback — missing bakes simply mean no landmarks.
pub fn landmark_meshes() -> Vec<Mesh> {
    ["ruin_columns", "ruin_arch", "ruin_circle"]
        .iter()
        .filter_map(|name| baked("props", name))
        .collect()
}

fn unit_kind_name(kind: saladin_sim::UnitKind) -> &'static str {
    use saladin_sim::UnitKind;
    match kind {
        UnitKind::Peasant => "peasant",
        UnitKind::Spearman => "spearman",
        UnitKind::Archer => "archer",
        UnitKind::Knight => "knight",
        UnitKind::HorseArcher => "horse_archer",
        UnitKind::Mamluk => "mamluk",
        UnitKind::Crossbowman => "crossbowman",
        UnitKind::Ram => "ram",
        UnitKind::Mangonel => "mangonel",
        UnitKind::Imam => "imam",
    }
}

fn rig_group(node_name: &str) -> Option<super::RigGroup> {
    use super::RigGroup as G;
    // Blender suffixes duplicate names ("Body.001") when several units share
    // a bake scene — match on the stem
    Some(match node_name.split('.').next().unwrap_or(node_name) {
        "Body" => G::Body,
        "LegL" => G::LegL,
        "LegR" => G::LegR,
        "ArmL" => G::ArmL,
        "ArmR" => G::ArmR,
        "WheelFL" => G::WheelFL,
        "WheelFR" => G::WheelFR,
        "WheelBL" => G::WheelBL,
        "WheelBR" => G::WheelBR,
        _ => return None,
    })
}

/// Parse a multi-node rig GLB: every node named after a RigGroup becomes a
/// part; node translation = joint pivot, mesh verts already pivot-relative.
fn load_glb_rig(path: &std::path::Path) -> Option<Vec<super::units::RigPart>> {
    let bytes = std::fs::read(path).ok()?;
    let glb = gltf::Gltf::from_slice(&bytes).ok()?;
    let blob = glb.blob.as_deref()?;
    let mut parts = Vec::new();
    for node in glb.nodes() {
        let Some(mesh) = node.mesh() else { continue };
        let Some(group) = node.name().and_then(rig_group) else { continue };
        let (t, _, _) = node.transform().decomposed();
        let prim = mesh.primitives().next()?;
        let reader = prim.reader(|buffer| match buffer.source() {
            gltf::buffer::Source::Bin => Some(blob),
            gltf::buffer::Source::Uri(_) => None,
        });
        let positions: Vec<[f32; 3]> = reader.read_positions()?.collect();
        let normals: Vec<[f32; 3]> = reader.read_normals()?.collect();
        let indices: Vec<u32> = reader.read_indices()?.into_u32().collect();
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        if let Some(colors) = reader.read_colors(0) {
            let colors: Vec<[f32; 4]> = colors.into_rgba_f32().collect();
            m.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        }
        m.insert_indices(Indices::U32(indices));
        parts.push(super::units::RigPart { group, pivot: Vec3::new(t[0], t[1], t[2]), mesh: m });
    }
    if parts.is_empty() { None } else { Some(parts) }
}

/// Baked unit rig for the faction, falling back to the unsuffixed bake
/// (shared siege engines), then to the procedural rig.
pub fn unit_rig(kind: saladin_sim::UnitKind, faction: saladin_sim::Faction) -> Vec<super::units::RigPart> {
    let name = unit_kind_name(kind);
    let dir = models_root().join("units");
    let rig = load_glb_rig(&dir.join(format!("{name}_{}.glb", faction_name(faction))))
        .or_else(|| load_glb_rig(&dir.join(format!("{name}.glb"))));
    rig.unwrap_or_else(|| super::units::unit_rig(kind))
}
