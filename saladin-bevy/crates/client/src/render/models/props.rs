//! Small decorative prop meshes (port of src/game/meshes/props.ts +
//! src/game/Vegetation.ts templates). Vertex-colored merged primitives, one
//! mesh per prop kind, intended for instanced scattering.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, MeshBuilder, Meshable, PrimitiveTopology};
use bevy::prelude::*;
use saladin_sim::{ResourceType, resource_def};

use std::f32::consts::{FRAC_PI_2, TAU};

/// Index into `prop_meshes()` per decoration kind (must stay in sync with
/// `vegetation::mesh_index`).
pub const PROP_SHRUB: usize = 0;
pub const PROP_DUNE_GRASS: usize = 1;
pub const PROP_ROCK: usize = 2;
pub const PROP_BOULDER: usize = 3;
pub const PROP_REEDS: usize = 4;
pub const PROP_PALM: usize = 5;
pub const PROP_PINE: usize = 6;

/// Mesh templates for the vegetation/prop instancer, indexed by placement.
pub fn prop_meshes() -> Vec<Mesh> {
    vec![shrub(), dune_grass(), rock(), boulder(), reeds(), palm(), pine()]
}

/// Resource-node mesh variants by type. Several meshes per kind; the renderer
/// picks one per node — biome-aware via the `TREE_*`/`FOOD_*` index groups —
/// so groves and herds read as natural variety while every variant still
/// instances.
pub fn resource_node_meshes(res: ResourceType) -> Vec<Mesh> {
    let _ = resource_def(res);
    match res {
        ResourceType::Stone => vec![stone_cluster_a(), stone_cluster_b(), stone_cluster_c()],
        ResourceType::Food => {
            vec![deer(), boar(), berry_bush(), deer_grazing(), deer_carcass(), boar_carcass()]
        }
        ResourceType::Gold => vec![gold_vein_a(), gold_vein_b()],
        _ => vec![tree_broadleaf(), tree_conifer(), tree_broadleaf_tall(), tree_olive(), tree_palm()],
    }
}

/// Wood-node variant indices (into the `ResourceType::Wood` mesh vec).
pub const TREE_BROADLEAF: usize = 0;
pub const TREE_CONIFER: usize = 1;
pub const TREE_BROADLEAF_TALL: usize = 2;
pub const TREE_OLIVE: usize = 3;
pub const TREE_PALM: usize = 4;

/// Food-node variant indices.
pub const FOOD_DEER: usize = 0;
pub const FOOD_BOAR: usize = 1;
pub const FOOD_BERRY: usize = 2;
pub const FOOD_DEER_GRAZING: usize = 3;
pub const FOOD_DEER_CARCASS: usize = 4;
pub const FOOD_BOAR_CARCASS: usize = 5;

/// Coastal food nodes sit on water tiles — render them as a fish school with
/// a ripple ring instead of a land animal standing on the sea.
pub fn fish_node_mesh() -> Mesh {
    let silver = lin(0xa8c4cc);
    let dark = lin(0x5e7e8c);
    let ripple = lin(0xc8e8ee);
    let mut parts = vec![
        // Two faint concentric ripple rings lying on the water.
        part(
            torus_ring(0.42, 0.025),
            ripple,
            Transform::from_xyz(0.0, 0.05, 0.0),
        ),
        part(
            torus_ring(0.65, 0.02),
            ripple,
            Transform::from_xyz(0.0, 0.04, 0.0),
        ),
    ];
    // A few fish backs arcing out of the water around the center.
    for (i, &(dx, dz, yaw, s)) in
        [(0.0f32, 0.0f32, 0.4f32, 1.0f32), (0.35, 0.2, 2.6, 0.8), (-0.3, 0.25, 4.4, 0.85), (-0.1, -0.35, 1.6, 0.7)]
            .iter()
            .enumerate()
    {
        let c = if i % 2 == 0 { silver } else { dark };
        let body = Transform::from_xyz(dx, 0.1 * s, dz)
            * Transform::from_rotation(Quat::from_rotation_y(yaw))
            * Transform::from_scale(Vec3::new(0.32 * s, 0.5 * s, 1.05 * s));
        parts.push(part(octahedron(0.22), c, body));
        // tail fin
        let tail = Transform::from_xyz(dx, 0.08 * s, dz)
            * Transform::from_rotation(Quat::from_rotation_y(yaw))
            * Transform::from_xyz(0.0, 0.06, -0.26 * s)
            * Transform::from_rotation(Quat::from_rotation_x(0.9))
            * Transform::from_scale(Vec3::new(0.5, 1.0, 0.35));
        parts.push(part(cone(0.12 * s, 0.18 * s, 4), c, tail));
    }
    merge(parts)
}

fn torus_ring(major: f32, minor: f32) -> Mesh {
    Torus { minor_radius: minor, major_radius: major }
        .mesh()
        .minor_resolution(4)
        .major_resolution(18)
        .build()
}

// ── trees (Wood nodes) ───────────────────────────────────────────────────────

const TRUNK: u32 = 0x6b4a2b;
const TRUNK_DARK: u32 = 0x553a20;

/// Round broadleaf: trunk under a clump of overlapping leaf blobs in two
/// greens — reads as an oak/sycamore instead of a traffic cone.
fn tree_broadleaf() -> Mesh {
    let leaf = lin(0x4a7a33);
    let leaf_lo = lin(0x3d6628);
    let leaf_hi = lin(0x5d9440);
    merge(vec![
        part(frustum(0.09, 0.16, 0.8, 6), lin(TRUNK), xyz(0.0, 0.4, 0.0)),
        part(icosahedron(0.52), leaf, xyz(0.0, 1.18, 0.0)),
        part(icosahedron(0.36), leaf_lo, xyz(0.34, 0.95, 0.16)),
        part(icosahedron(0.32), leaf_lo, xyz(-0.3, 0.98, -0.18)),
        part(icosahedron(0.3), leaf_hi, xyz(0.06, 1.55, 0.05)),
    ])
}

/// Taller, narrower broadleaf for grove variety.
fn tree_broadleaf_tall() -> Mesh {
    let leaf = lin(0x456f2f);
    let leaf_hi = lin(0x558b3a);
    merge(vec![
        part(frustum(0.08, 0.14, 1.0, 6), lin(TRUNK_DARK), xyz(0.0, 0.5, 0.0)),
        part(icosahedron(0.42), leaf, xyz(0.0, 1.3, 0.0)),
        part(icosahedron(0.34), leaf, xyz(0.22, 1.05, -0.14)),
        part(icosahedron(0.3), leaf_hi, xyz(-0.05, 1.68, 0.08)),
    ])
}

/// Conifer: stacked irregular tiers, visible trunk, dark forest green.
fn tree_conifer() -> Mesh {
    let mut parts = vec![part(frustum(0.07, 0.13, 0.6, 6), lin(TRUNK_DARK), xyz(0.0, 0.3, 0.0))];
    for (r, h, y, c) in [
        (0.52f32, 0.75f32, 0.78f32, 0x2f5d2a),
        (0.4, 0.65, 1.2, 0x356b30),
        (0.27, 0.58, 1.6, 0x3c7a36),
    ] {
        parts.push(part(cone(r, h, 7), lin(c), xyz(0.0, y, 0.0)));
    }
    merge(parts)
}

/// Olive/scrub tree: short forked trunk, wide flat grey-green canopy.
fn tree_olive() -> Mesh {
    let leaf = lin(0x6e7f4a);
    let leaf_lo = lin(0x5d6e3c);
    merge(vec![
        part(
            frustum(0.07, 0.13, 0.55, 5),
            lin(TRUNK),
            at_rot(0.05, 0.28, 0.0, Quat::from_rotation_z(0.18)),
        ),
        part(
            frustum(0.06, 0.09, 0.45, 5),
            lin(TRUNK),
            at_rot(-0.12, 0.32, 0.05, Quat::from_rotation_z(-0.35)),
        ),
        part(squashed(icosahedron(0.44), 0.62), leaf, xyz(0.0, 0.78, 0.0)),
        part(squashed(icosahedron(0.3), 0.6), leaf_lo, xyz(0.32, 0.68, 0.12)),
        part(squashed(icosahedron(0.26), 0.65), leaf_lo, xyz(-0.3, 0.7, -0.12)),
    ])
}

/// Harvestable palm: taller than the cosmetic prop, fuller crown, date
/// clusters under the fronds.
fn tree_palm() -> Mesh {
    let green = lin(0x3f8f49);
    let green_dk = lin(0x357a3e);
    let mut parts = vec![part(frustum(0.07, 0.12, 2.0, 5), lin(0x7a5a32), at_rot(0.0, 1.0, 0.0, Quat::from_rotation_z(0.06)))];
    let n = 7;
    for i in 0..n {
        let ang = i as f32 / n as f32 * TAU;
        let c = if i % 2 == 0 { green } else { green_dk };
        let tf = Transform::from_xyz(0.12, 2.0, 0.0)
            * Transform::from_rotation(Quat::from_rotation_y(ang))
            * Transform::from_rotation(Quat::from_rotation_x(-0.55))
            * Transform::from_xyz(0.0, 0.0, 0.55)
            * Transform::from_rotation(Quat::from_rotation_x(FRAC_PI_2));
        parts.push(part(cone(0.18, 1.1, 3), c, tf));
    }
    parts.push(part(icosahedron(0.15), green, xyz(0.12, 2.02, 0.0)));
    // date clusters
    parts.push(part(sphere_uv(0.09), lin(0xb07a2a), xyz(0.26, 1.88, 0.1)));
    parts.push(part(sphere_uv(0.07), lin(0x9a6a24), xyz(-0.02, 1.86, -0.16)));
    merge(parts)
}

fn at_rot(x: f32, y: f32, z: f32, q: Quat) -> Transform {
    Transform::from_xyz(x, y, z).with_rotation(q)
}

fn squashed(m: Mesh, y: f32) -> Mesh {
    m.transformed_by(Transform::from_scale(Vec3::new(1.0, y, 1.0)))
}

// ── stone outcrops ───────────────────────────────────────────────────────────

/// Weathered grey cluster: one big slab leaning, two companions, pebbles.
fn stone_cluster_a() -> Mesh {
    merge(vec![
        part(
            dodecahedron(0.42),
            lin(0x878a8e),
            Transform::from_xyz(-0.08, 0.26, 0.0)
                .with_rotation(Quat::from_rotation_z(0.22))
                .with_scale(Vec3::new(1.15, 0.78, 0.95)),
        ),
        part(
            icosahedron(0.28),
            lin(0x75787d),
            Transform::from_xyz(0.38, 0.16, 0.18).with_scale(Vec3::new(1.0, 0.72, 1.0)),
        ),
        part(
            dodecahedron(0.2),
            lin(0x91959b),
            Transform::from_xyz(0.12, 0.1, -0.38).with_scale(Vec3::new(1.0, 0.6, 1.0)),
        ),
        part(icosahedron(0.1), lin(0x7e8187), xyz(-0.42, 0.06, 0.3)),
    ])
}

/// Tilted strata: three slabby blocks at stepped angles, cooler grey.
fn stone_cluster_b() -> Mesh {
    merge(vec![
        part(
            dodecahedron(0.38),
            lin(0x7d8287),
            Transform::from_xyz(0.0, 0.3, -0.1)
                .with_rotation(Quat::from_euler(EulerRot::XYZ, 0.3, 0.4, 0.0))
                .with_scale(Vec3::new(1.3, 0.9, 0.7)),
        ),
        part(
            dodecahedron(0.3),
            lin(0x6d7176),
            Transform::from_xyz(0.22, 0.18, 0.3)
                .with_rotation(Quat::from_euler(EulerRot::XYZ, 0.15, 1.1, 0.0))
                .with_scale(Vec3::new(1.2, 0.7, 0.8)),
        ),
        part(
            icosahedron(0.16),
            lin(0x8a8e94),
            Transform::from_xyz(-0.38, 0.1, 0.22).with_scale(Vec3::new(1.0, 0.7, 1.0)),
        ),
    ])
}

/// Single warm-grey boulder half-sunk with rubble at its foot.
fn stone_cluster_c() -> Mesh {
    merge(vec![
        part(
            icosahedron(0.46),
            lin(0x84827e),
            Transform::from_xyz(0.0, 0.24, 0.0)
                .with_rotation(Quat::from_rotation_y(0.7))
                .with_scale(Vec3::new(1.1, 0.66, 1.0)),
        ),
        part(
            dodecahedron(0.18),
            lin(0x747371),
            Transform::from_xyz(0.42, 0.1, -0.2).with_scale(Vec3::new(1.0, 0.65, 1.0)),
        ),
        part(icosahedron(0.12), lin(0x8d8c8a), xyz(-0.4, 0.07, -0.3)),
        part(icosahedron(0.09), lin(0x7c7b78), xyz(0.3, 0.05, 0.4)),
    ])
}

// ── gold veins ───────────────────────────────────────────────────────────────

/// Dark host rock studded with bright nuggets — ore in stone, not a floating
/// gem.
fn gold_vein_a() -> Mesh {
    let rock = lin(0x6f655a);
    let rock_dk = lin(0x5e554b);
    let gold = lin(0xe8c34a);
    let gold_dk = lin(0xc9a132);
    merge(vec![
        part(
            dodecahedron(0.42),
            rock,
            Transform::from_xyz(0.0, 0.27, 0.0)
                .with_rotation(Quat::from_rotation_y(0.4))
                .with_scale(Vec3::new(1.1, 0.75, 1.0)),
        ),
        part(
            icosahedron(0.26),
            rock_dk,
            Transform::from_xyz(0.36, 0.14, 0.22).with_scale(Vec3::new(1.0, 0.7, 1.0)),
        ),
        part(octahedron(0.16), gold, xyz(0.1, 0.52, 0.16)),
        part(octahedron(0.12), gold_dk, xyz(-0.24, 0.44, -0.1)),
        part(octahedron(0.11), gold, xyz(0.3, 0.32, -0.22)),
        part(octahedron(0.1), gold_dk, xyz(0.42, 0.26, 0.3)),
        part(octahedron(0.09), gold, xyz(-0.1, 0.32, 0.36)),
    ])
}

fn gold_vein_b() -> Mesh {
    let rock = lin(0x68625a);
    let gold = lin(0xddb83f);
    merge(vec![
        part(
            icosahedron(0.4),
            rock,
            Transform::from_xyz(-0.1, 0.24, 0.0)
                .with_rotation(Quat::from_rotation_y(1.2))
                .with_scale(Vec3::new(1.2, 0.7, 0.9)),
        ),
        part(
            dodecahedron(0.22),
            rock,
            Transform::from_xyz(0.34, 0.12, -0.18).with_scale(Vec3::new(1.0, 0.65, 1.0)),
        ),
        part(octahedron(0.15), gold, xyz(-0.05, 0.48, 0.12)),
        part(octahedron(0.11), gold, xyz(-0.34, 0.34, -0.14)),
        part(octahedron(0.1), gold, xyz(0.34, 0.28, -0.1)),
        part(octahedron(0.09), gold, xyz(0.12, 0.36, -0.3)),
    ])
}

// ── game animals + forage (Food nodes) ───────────────────────────────────────

const HIDE_DEER: u32 = 0x8f6a42;
const HIDE_DEER_DK: u32 = 0x6e5132;

/// Standing deer: tan spheroid body, raised neck + head, antlers, dark legs.
fn deer() -> Mesh {
    deer_pose(0.55)
}

/// Grazing deer: same body, head lowered to the grass.
fn deer_grazing() -> Mesh {
    deer_pose(-0.35)
}

fn deer_pose(neck_pitch: f32) -> Mesh {
    let hide = lin(HIDE_DEER);
    let hide_dk = lin(HIDE_DEER_DK);
    let antler = lin(0xb3a380);
    let mut parts = vec![
        // slim barrel body, slightly deeper than wide
        part(
            sphere_uv(0.26),
            hide,
            Transform::from_xyz(0.0, 0.58, -0.05).with_scale(Vec3::new(0.72, 0.85, 1.75)),
        ),
        // chest shoulder mass
        part(
            sphere_uv(0.2),
            hide,
            Transform::from_xyz(0.0, 0.6, 0.3).with_scale(Vec3::new(0.75, 0.9, 1.0)),
        ),
        // rump patch
        part(sphere_uv(0.1), lin(0xe8ddc8), xyz(0.0, 0.6, -0.5)),
    ];
    // neck + head pivot at the chest
    let pivot = Transform::from_xyz(0.0, 0.68, 0.4) * Transform::from_rotation(Quat::from_rotation_x(neck_pitch));
    parts.push(part(cyl8(0.055, 0.08, 0.42), hide, pivot * Transform::from_xyz(0.0, 0.18, 0.0)));
    let head_at = pivot * Transform::from_xyz(0.0, 0.42, 0.04);
    parts.push(part(
        sphere_uv(0.1),
        hide,
        head_at * Transform::from_scale(Vec3::new(0.9, 0.9, 1.35)),
    ));
    parts.push(part(cyl8(0.03, 0.045, 0.12), hide_dk, head_at * Transform::from_xyz(0.0, -0.02, 0.15) * Transform::from_rotation(Quat::from_rotation_x(1.35))));
    // ears
    for sx in [-1.0f32, 1.0] {
        parts.push(part(cone(0.035, 0.1, 4), hide_dk, head_at * Transform::from_xyz(sx * 0.09, 0.09, -0.02)));
    }
    // antlers: thick main beam sweeping back + two tines each side
    for sx in [-1.0f32, 1.0] {
        let base = head_at * Transform::from_xyz(sx * 0.05, 0.1, 0.0);
        let beam = base
            * Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.55, 0.0, sx * 0.55));
        parts.push(part(cyl8(0.018, 0.026, 0.34), antler, beam * Transform::from_xyz(0.0, 0.15, 0.0)));
        parts.push(part(
            cyl8(0.014, 0.018, 0.2),
            antler,
            beam * Transform::from_xyz(0.0, 0.2, 0.0)
                * Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, 0.9, 0.0, sx * 0.5))
                * Transform::from_xyz(0.0, 0.08, 0.0),
        ));
        parts.push(part(
            cyl8(0.012, 0.016, 0.16),
            antler,
            beam * Transform::from_xyz(0.0, 0.3, 0.0)
                * Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.5, 0.0, sx * 0.7))
                * Transform::from_xyz(0.0, 0.06, 0.0),
        ));
    }
    // long slim legs
    for sz in [-1.0f32, 1.0] {
        for sx in [-1.0f32, 1.0] {
            parts.push(part(
                cyl8(0.022, 0.032, 0.55),
                hide_dk,
                xyz(sx * 0.12, 0.27, sz * 0.3 - 0.05),
            ));
        }
    }
    merge(parts)
}

/// Wild boar: stocky dark body, bristle ridge, snout + tusks, stub legs.
fn boar() -> Mesh {
    let hide = lin(0x4f3a28);
    let hide_dk = lin(0x3a2b1d);
    let snout = lin(0x8a6a55);
    let tusk = lin(0xe8e0cc);
    let mut parts = vec![
        part(
            sphere_uv(0.3),
            hide,
            Transform::from_xyz(0.0, 0.36, -0.04).with_scale(Vec3::new(0.95, 0.9, 1.6)),
        ),
        // bristle ridge along the spine
        part(boxy(0.1, 0.1, 0.6), hide_dk, xyz(0.0, 0.62, -0.06)),
        // head
        part(
            sphere_uv(0.17),
            hide,
            Transform::from_xyz(0.0, 0.38, 0.42).with_scale(Vec3::new(0.9, 0.9, 1.1)),
        ),
        part(cyl8(0.06, 0.07, 0.12), snout, at_rot(0.0, 0.34, 0.58, Quat::from_rotation_x(FRAC_PI_2))),
    ];
    for sx in [-1.0f32, 1.0] {
        parts.push(part(cone(0.035, 0.08, 4), hide_dk, xyz(sx * 0.1, 0.52, 0.4)));
        parts.push(part(
            cone(0.018, 0.09, 4),
            tusk,
            at_rot(sx * 0.08, 0.3, 0.56, Quat::from_euler(EulerRot::XYZ, -0.5, 0.0, sx * 0.6)),
        ));
        for sz in [-1.0f32, 1.0] {
            parts.push(part(cyl8(0.035, 0.045, 0.3), hide_dk, xyz(sx * 0.14, 0.14, sz * 0.3)));
        }
    }
    merge(parts)
}

/// Slaughtered deer lying on its side — shown once the first harvest tick
/// lands (AoE-style: the animal stops wandering and becomes a carcass).
fn deer_carcass() -> Mesh {
    let hide = lin(HIDE_DEER);
    let hide_dk = lin(HIDE_DEER_DK);
    let antler = lin(0xb3a380);
    let mut parts = vec![
        // body flopped on its side
        part(
            sphere_uv(0.26),
            hide,
            Transform::from_xyz(0.0, 0.2, -0.05)
                .with_rotation(Quat::from_rotation_z(1.35))
                .with_scale(Vec3::new(0.72, 0.85, 1.75)),
        ),
        // neck stretched out flat, head on the ground
        part(
            cyl8(0.05, 0.08, 0.4),
            hide,
            at_rot(0.1, 0.1, 0.42, Quat::from_euler(EulerRot::XYZ, 1.35, 0.0, -0.2)),
        ),
        part(
            sphere_uv(0.1),
            hide,
            Transform::from_xyz(0.16, 0.08, 0.62).with_scale(Vec3::new(0.9, 0.8, 1.35)),
        ),
        part(sphere_uv(0.09), lin(0xe8ddc8), xyz(-0.06, 0.22, -0.5)),
    ];
    // antlers flat on the ground
    for sz in [-1.0f32, 1.0] {
        parts.push(part(
            cyl8(0.016, 0.024, 0.3),
            antler,
            at_rot(0.22, 0.05, 0.66 + sz * 0.06, Quat::from_euler(EulerRot::XYZ, sz * 0.5, 0.0, -1.2)),
        ));
    }
    // stiff legs sticking out sideways
    for (i, sz) in [-1.0f32, -0.35, 0.35, 1.0].into_iter().enumerate() {
        let lift = if i % 2 == 0 { 0.12 } else { 0.26 };
        parts.push(part(
            cyl8(0.022, 0.03, 0.45),
            hide_dk,
            at_rot(0.2, lift, sz * 0.3 - 0.05, Quat::from_rotation_z(-1.25)),
        ));
    }
    merge(parts)
}

/// Slaughtered boar on its side.
fn boar_carcass() -> Mesh {
    let hide = lin(0x4f3a28);
    let hide_dk = lin(0x3a2b1d);
    let tusk = lin(0xe8e0cc);
    let mut parts = vec![
        part(
            sphere_uv(0.3),
            hide,
            Transform::from_xyz(0.0, 0.22, -0.04)
                .with_rotation(Quat::from_rotation_z(1.35))
                .with_scale(Vec3::new(0.9, 0.95, 1.6)),
        ),
        part(
            sphere_uv(0.17),
            hide,
            Transform::from_xyz(0.1, 0.13, 0.42)
                .with_rotation(Quat::from_rotation_z(1.1))
                .with_scale(Vec3::new(0.9, 0.9, 1.1)),
        ),
        part(cyl8(0.06, 0.07, 0.12), lin(0x8a6a55), at_rot(0.14, 0.1, 0.58, Quat::from_rotation_x(FRAC_PI_2))),
    ];
    for sz in [-1.0f32, 1.0] {
        parts.push(part(
            cone(0.018, 0.09, 4),
            tusk,
            at_rot(0.18, 0.08, 0.56 + sz * 0.05, Quat::from_euler(EulerRot::XYZ, sz * 0.6, 0.0, -1.0)),
        ));
    }
    for (i, sz) in [-1.0f32, -0.35, 0.35, 1.0].into_iter().enumerate() {
        let lift = if i % 2 == 0 { 0.1 } else { 0.24 };
        parts.push(part(
            cyl8(0.03, 0.04, 0.3),
            hide_dk,
            at_rot(0.22, lift, sz * 0.28, Quat::from_rotation_z(-1.3)),
        ));
    }
    merge(parts)
}

/// Berry bush: low leaf clump dotted with red berries.
fn berry_bush() -> Mesh {
    let leaf = lin(0x4a6f2e);
    let leaf_hi = lin(0x5d8138);
    let berry = lin(0xb33326);
    let mut parts = vec![
        part(squashed(icosahedron(0.4), 0.7), leaf, xyz(0.0, 0.26, 0.0)),
        part(squashed(icosahedron(0.28), 0.75), leaf_hi, xyz(0.3, 0.2, 0.14)),
        part(squashed(icosahedron(0.24), 0.7), leaf, xyz(-0.28, 0.18, -0.12)),
    ];
    for &(x, y, z) in &[
        (0.12f32, 0.5f32, 0.18f32),
        (-0.18, 0.44, 0.2),
        (0.3, 0.4, -0.1),
        (-0.06, 0.52, -0.2),
        (0.42, 0.3, 0.22),
        (-0.38, 0.32, 0.06),
        (0.0, 0.46, 0.34),
    ] {
        parts.push(part(sphere_uv(0.045), berry, xyz(x, y, z)));
    }
    merge(parts)
}

fn sphere_uv(r: f32) -> Mesh {
    Sphere::new(r).mesh().uv(10, 8)
}

fn cyl8(r_top: f32, r_bot: f32, h: f32) -> Mesh {
    frustum(r_top, r_bot, h, 6)
}

fn boxy(w: f32, h: f32, d: f32) -> Mesh {
    Mesh::from(Cuboid::new(w, h, d))
}

fn lin(hex: u32) -> [f32; 4] {
    let c = Color::srgb_u8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8).to_linear();
    [c.red, c.green, c.blue, 1.0]
}

fn xyz(x: f32, y: f32, z: f32) -> Transform {
    Transform::from_xyz(x, y, z)
}

fn part(prim: Mesh, color: [f32; 4], tf: Transform) -> Mesh {
    let mut m = prim.transformed_by(tf);
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![color; n]);
    m
}

fn merge(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        let _ = base.merge(&p);
    }
    base
}

fn cone(r: f32, h: f32, seg: u32) -> Mesh {
    Cone::new(r, h).mesh().resolution(seg).build()
}

fn cylinder(r: f32, h: f32, seg: u32) -> Mesh {
    Cylinder::new(r, h).mesh().resolution(seg).build()
}

fn frustum(r_top: f32, r_bottom: f32, h: f32, seg: u32) -> Mesh {
    ConicalFrustum { radius_top: r_top, radius_bottom: r_bottom, height: h }
        .mesh()
        .resolution(seg)
        .build()
}

/// Faceted convex polyhedron with verts pushed to radius `r` and flat normals
/// (stand-in for THREE's Icosahedron/Dodecahedron/Octahedron geometries).
fn polyhedron(verts: &[[f32; 3]], faces: &[[usize; 3]], r: f32) -> Mesh {
    let mut pos: Vec<[f32; 3]> = Vec::with_capacity(faces.len() * 3);
    let mut nrm: Vec<[f32; 3]> = Vec::with_capacity(faces.len() * 3);
    for f in faces {
        let mut p = [
            Vec3::from(verts[f[0]]).normalize() * r,
            Vec3::from(verts[f[1]]).normalize() * r,
            Vec3::from(verts[f[2]]).normalize() * r,
        ];
        let mut n = (p[1] - p[0]).cross(p[2] - p[0]).normalize();
        // Faces of an origin-centered convex solid must point away from origin.
        if n.dot(p[0] + p[1] + p[2]) < 0.0 {
            p.swap(1, 2);
            n = -n;
        }
        for v in p {
            pos.push([v.x, v.y, v.z]);
            nrm.push([n.x, n.y, n.z]);
        }
    }
    let count = pos.len();
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0]; count]);
    m.insert_indices(Indices::U32((0..count as u32).collect()));
    m
}

fn icosahedron(r: f32) -> Mesh {
    const T: f32 = 1.618_034;
    const V: [[f32; 3]; 12] = [
        [-1.0, T, 0.0],
        [1.0, T, 0.0],
        [-1.0, -T, 0.0],
        [1.0, -T, 0.0],
        [0.0, -1.0, T],
        [0.0, 1.0, T],
        [0.0, -1.0, -T],
        [0.0, 1.0, -T],
        [T, 0.0, -1.0],
        [T, 0.0, 1.0],
        [-T, 0.0, -1.0],
        [-T, 0.0, 1.0],
    ];
    const F: [[usize; 3]; 20] = [
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    polyhedron(&V, &F, r)
}

fn dodecahedron(r: f32) -> Mesh {
    const T: f32 = 1.618_034;
    const S: f32 = 1.0 / T;
    const V: [[f32; 3]; 20] = [
        [-1.0, -1.0, -1.0],
        [-1.0, -1.0, 1.0],
        [-1.0, 1.0, -1.0],
        [-1.0, 1.0, 1.0],
        [1.0, -1.0, -1.0],
        [1.0, -1.0, 1.0],
        [1.0, 1.0, -1.0],
        [1.0, 1.0, 1.0],
        [0.0, -S, -T],
        [0.0, -S, T],
        [0.0, S, -T],
        [0.0, S, T],
        [-S, -T, 0.0],
        [-S, T, 0.0],
        [S, -T, 0.0],
        [S, T, 0.0],
        [-T, 0.0, -S],
        [T, 0.0, -S],
        [-T, 0.0, S],
        [T, 0.0, S],
    ];
    const F: [[usize; 3]; 36] = [
        [3, 11, 7],
        [3, 7, 15],
        [3, 15, 13],
        [7, 19, 17],
        [7, 17, 6],
        [7, 6, 15],
        [17, 4, 8],
        [17, 8, 10],
        [17, 10, 6],
        [8, 0, 16],
        [8, 16, 2],
        [8, 2, 10],
        [0, 12, 1],
        [0, 1, 18],
        [0, 18, 16],
        [6, 10, 2],
        [6, 2, 13],
        [6, 13, 15],
        [2, 16, 18],
        [2, 18, 3],
        [2, 3, 13],
        [18, 1, 9],
        [18, 9, 11],
        [18, 11, 3],
        [4, 14, 12],
        [4, 12, 0],
        [4, 0, 8],
        [11, 9, 5],
        [11, 5, 19],
        [11, 19, 7],
        [19, 5, 14],
        [19, 14, 4],
        [19, 4, 17],
        [1, 12, 14],
        [1, 14, 5],
        [1, 5, 9],
    ];
    polyhedron(&V, &F, r)
}

fn octahedron(r: f32) -> Mesh {
    const V: [[f32; 3]; 6] =
        [[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]];
    const F: [[usize; 3]; 8] = [
        [0, 2, 4],
        [0, 4, 3],
        [0, 3, 5],
        [0, 5, 2],
        [1, 2, 5],
        [1, 5, 3],
        [1, 3, 4],
        [1, 4, 2],
    ];
    polyhedron(&V, &F, r)
}

// A two-lobe brush clump — big blob with a smaller offset companion.
fn shrub() -> Mesh {
    let c = lin(0x6e7d3a);
    let c_lo = lin(0x5c6a30);
    merge(vec![
        part(squashed(icosahedron(0.3), 0.8), c, xyz(0.0, 0.22, 0.0)),
        part(squashed(icosahedron(0.19), 0.85), c_lo, xyz(0.24, 0.13, 0.1)),
        part(squashed(icosahedron(0.14), 0.8), c_lo, xyz(-0.2, 0.1, -0.12)),
    ])
}

// A fan of a few thin blades — a sparse tuft of dune grass.
fn dune_grass() -> Mesh {
    let c = lin(0xc2b06a);
    let n = 4;
    let mut parts = Vec::new();
    for i in 0..n {
        let h = 0.5 + (i % 2) as f32 * 0.18;
        let ang = i as f32 / n as f32 * TAU;
        let tf = Transform::from_xyz(ang.cos() * 0.07, 0.0, ang.sin() * 0.07)
            * Transform::from_rotation(Quat::from_rotation_y(ang))
            * Transform::from_rotation(Quat::from_rotation_z(0.28))
            * Transform::from_xyz(0.0, 0.28, 0.0);
        parts.push(part(cone(0.05, h, 3), c, tf));
    }
    merge(parts)
}

// A faceted loose stone with a pebble companion, half-sunk so it sits in the
// ground instead of resting on it.
fn rock() -> Mesh {
    merge(vec![
        part(
            dodecahedron(0.28),
            lin(0x82807e),
            Transform::from_xyz(0.0, 0.13, 0.0)
                .with_rotation(Quat::from_rotation_y(0.5))
                .with_scale(Vec3::new(1.15, 0.6, 0.9)),
        ),
        part(icosahedron(0.12), lin(0x767472), xyz(0.26, 0.06, 0.14)),
    ])
}

// A big two-mass boulder for the high, cold biomes.
fn boulder() -> Mesh {
    merge(vec![
        part(
            dodecahedron(0.55),
            lin(0x9aa0a6),
            Transform::from_xyz(0.0, 0.4, 0.0).with_scale(Vec3::new(1.0, 0.8, 1.0)),
        ),
        part(icosahedron(0.3), lin(0x9aa0a6), xyz(0.45, 0.18, 0.12)),
    ])
}

// A clump of marsh reeds: stalks of varied height with darker seed heads.
fn reeds() -> Mesh {
    let stalk = lin(0x8a9a52);
    let head = lin(0x6b5a2e);
    let n = 5;
    let mut parts = Vec::new();
    for i in 0..n {
        let h = 0.7 + (i % 3) as f32 * 0.22;
        let dx = (i as f32 - (n - 1) as f32 / 2.0) * 0.09;
        let dz = (i % 2) as f32 * 0.06;
        parts.push(part(frustum(0.025, 0.04, h, 4), stalk, xyz(dx, h / 2.0, dz)));
        parts.push(part(cylinder(0.05, 0.16, 4), head, xyz(dx, h, dz)));
    }
    merge(parts)
}

// A palm: slim leaning trunk under a ring of drooping fronds + crown knob.
fn palm() -> Mesh {
    let green = lin(0x3f8f49);
    let mut parts = vec![part(frustum(0.06, 0.1, 1.6, 5), lin(0x7a5a32), xyz(0.0, 0.8, 0.0))];
    let n = 6;
    for i in 0..n {
        let ang = i as f32 / n as f32 * TAU;
        // Lay the frond outward, droop it, fan it around the crown.
        let tf = Transform::from_xyz(0.0, 1.6, 0.0)
            * Transform::from_rotation(Quat::from_rotation_y(ang))
            * Transform::from_rotation(Quat::from_rotation_x(-0.5))
            * Transform::from_xyz(0.0, 0.0, 0.45)
            * Transform::from_rotation(Quat::from_rotation_x(FRAC_PI_2));
        parts.push(part(cone(0.16, 0.9, 3), green, tf));
    }
    parts.push(part(icosahedron(0.13), green, xyz(0.0, 1.62, 0.0)));
    merge(parts)
}

// A small cosmetic conifer: stacked cone tiers over a stub trunk, each tier
// its own green so the silhouette reads as foliage layers.
fn pine() -> Mesh {
    let mut parts = vec![part(frustum(0.08, 0.11, 0.5, 5), lin(0x5b4127), xyz(0.0, 0.25, 0.0))];
    for (r, h, y, c) in [
        (0.5, 0.75, 0.62, 0x2a5c2b),
        (0.38, 0.65, 1.02, 0x316a31),
        (0.25, 0.55, 1.4, 0x3a7a38),
    ] {
        parts.push(part(cone(r, h, 7), lin(c), xyz(0.0, y, 0.0)));
    }
    merge(parts)
}
