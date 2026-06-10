//! Detailed procedural building meshes (port of src/game/meshes/buildings.ts).
//! Geometry is authored at world scale matching `building_def(kind).footprint`
//! (1 tile = 1 world unit), exactly like the TS source. Colors are baked vertex
//! colors in the true TS material palette; the renderer applies a faint team
//! tint via the material, so team-cloth parts (banners, awnings, tower caps)
//! are baked near-white (TS untinted fallback 0xdddddd) to pick up the tint.

use std::f32::consts::{FRAC_PI_2, PI};

use bevy::mesh::{Mesh, MeshBuilder, Meshable};
use bevy::prelude::*;
use saladin_sim::BuildingKind;

// Shared palette so buildings read as one settlement, not a parts bin.
const STONE: u32 = 0x9c958a;
const STONE_DARK: u32 = 0x7d766b;
const TIMBER: u32 = 0x8a6a3a;
const TIMBER_DARK: u32 = 0x5a3a22;
const PLASTER: u32 = 0xcbb487;
const THATCH: u32 = 0x9a7a45;
const TEAM_CLOTH: u32 = 0xdddddd;
const SLIT_DARK: u32 = 0x2a2620;

fn srgb(hex: u32) -> [f32; 4] {
    let c = Color::srgb_u8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8).to_linear();
    [c.red, c.green, c.blue, 1.0]
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

fn xyz(x: f32, y: f32, z: f32) -> Transform {
    Transform::from_xyz(x, y, z)
}

fn rot_x(x: f32, y: f32, z: f32, rx: f32) -> Transform {
    Transform::from_xyz(x, y, z).with_rotation(Quat::from_rotation_x(rx))
}

fn rot_y(x: f32, y: f32, z: f32, ry: f32) -> Transform {
    Transform::from_xyz(x, y, z).with_rotation(Quat::from_rotation_y(ry))
}

fn rot_z(x: f32, y: f32, z: f32, rz: f32) -> Transform {
    Transform::from_xyz(x, y, z).with_rotation(Quat::from_rotation_z(rz))
}

fn cuboid(w: f32, h: f32, d: f32) -> Mesh {
    Mesh::from(Cuboid::new(w, h, d))
}

fn cyl(r: f32, h: f32, seg: u32) -> Mesh {
    Cylinder::new(r, h).mesh().resolution(seg).build()
}

/// Tapered drum (THREE.CylinderGeometry with rTop != rBottom).
fn frustum(r_top: f32, r_bottom: f32, h: f32, seg: u32) -> Mesh {
    ConicalFrustum { radius_top: r_top, radius_bottom: r_bottom, height: h }
        .mesh()
        .resolution(seg)
        .build()
}

fn cone(r: f32, h: f32, seg: u32) -> Mesh {
    Cone { radius: r, height: h }.mesh().resolution(seg).build()
}

fn sphere(r: f32) -> Mesh {
    Mesh::from(Sphere::new(r))
}

// A pennant on a pole — the cloth is a thin slab standing in for the TS
// swallow-tail ShapeGeometry. Team cloth so ownership is obvious after tint.
fn pennant(parts: &mut Vec<Mesh>, x: f32, y: f32, z: f32, h: f32) {
    let team = srgb(TEAM_CLOTH);
    parts.push(part(frustum(0.035, 0.045, h, 5), srgb(TIMBER_DARK), xyz(x, y, z)));
    parts.push(part(sphere(0.06), team, xyz(x, y + h / 2.0 + 0.04, z)));
    parts.push(part(cuboid(0.46, 0.21, 0.02), team, xyz(x + 0.25, y + h / 2.0 - 0.135, z)));
}

// Crenellated parapet ring stamped around a square top edge.
fn square_merlons(
    parts: &mut Vec<Mesh>,
    color: [f32; 4],
    cx: f32,
    top_y: f32,
    cz: f32,
    span: f32,
    size: f32,
    height: f32,
) {
    let half = span / 2.0 - size / 2.0;
    let steps = ((span / (size * 1.7)).round() as i32).max(2);
    for i in 0..=steps {
        let t = i as f32 / steps as f32 - 0.5;
        for (ox, oz) in [(t * span, -half), (t * span, half), (-half, t * span), (half, t * span)] {
            // Skip the inner duplicates on corners by only placing the perimeter band.
            if ox.abs() > half + 0.001 || oz.abs() > half + 0.001 {
                continue;
            }
            parts.push(part(
                cuboid(size, height, size),
                color,
                xyz(cx + ox, top_y + height / 2.0, cz + oz),
            ));
        }
    }
}

// Tall narrow recess that reads as an arrow loop / window when slightly inset.
fn arrow_slit(parts: &mut Vec<Mesh>, x: f32, y: f32, z: f32, ry: f32) {
    parts.push(part(cuboid(0.08, 0.4, 0.06), srgb(SLIT_DARK), rot_y(x, y, z, ry)));
}

/// Building mesh with baked stone/timber/roof vertex colors.
pub fn building_mesh(kind: BuildingKind) -> Mesh {
    match kind {
        BuildingKind::Keep => build_keep(),
        BuildingKind::Barracks => build_barracks(),
        BuildingKind::Tower => tower_core(false),
        BuildingKind::Watchtower => tower_core(true),
        BuildingKind::Wall => build_wall_pillar(),
        BuildingKind::Gatehouse => build_gatehouse(),
        BuildingKind::House => build_house(),
        BuildingKind::Stable => build_stable(),
        BuildingKind::Blacksmith => build_blacksmith(),
        BuildingKind::Market => build_market(),
        BuildingKind::Granary => build_granary(),
        BuildingKind::FishingHut => build_fishing_hut(),
        BuildingKind::SiegeWorkshop => build_siege_workshop(),
    }
}

// Walls are CONNECTIVITY-BASED: every segment is a square pillar on its tile
// center, and `update_wall_arms` hangs a half-tile arm toward each adjacent
// own structure. Corners, T-junctions, crosses and end-caps all read correctly
// without rotation guessing (the old averaged-angle slab left diagonal gaps at
// every corner).
fn build_wall_pillar() -> Mesh {
    let stone = srgb(STONE);
    let cap = srgb(0x847c71);
    let s = 0.56;
    let h = 0.74;
    let parts = vec![
        // battered foot for a heavier silhouette
        part(cuboid(s + 0.14, 0.18, s + 0.14), srgb(STONE_DARK), xyz(0.0, 0.09, 0.0)),
        part(cuboid(s, h, s), stone, xyz(0.0, h / 2.0, 0.0)),
        part(cuboid(s + 0.1, 0.1, s + 0.1), cap, xyz(0.0, h + 0.05, 0.0)),
        // single crowning merlon
        part(cuboid(0.3, 0.26, 0.3), stone, xyz(0.0, h + 0.23, 0.0)),
    ];
    merge(parts)
}

/// Half-tile crenellated wall run from inside the pillar to the +X tile edge;
/// the neighbour's mirrored arm meets it at the boundary.
pub fn build_wall_arm() -> Mesh {
    let stone = srgb(STONE);
    let cap = srgb(0x847c71);
    let h = 0.62;
    let tk = 0.42;
    let l = 0.56;
    let cx = 0.25;
    let mut parts = vec![
        part(cuboid(l, 0.18, tk + 0.1), srgb(STONE_DARK), xyz(cx, 0.09, 0.0)),
        part(cuboid(l, h, tk), stone, xyz(cx, h / 2.0, 0.0)),
        part(cuboid(l, 0.1, tk + 0.08), cap, xyz(cx, h + 0.05, 0.0)),
        // one merlon on the outer half of the run
        part(cuboid(0.2, 0.24, tk), stone, xyz(0.37, h + 0.22, 0.0)),
    ];
    arrow_slit(&mut parts, 0.3, h * 0.6, tk / 2.0 + 0.01, 0.0);
    merge(parts)
}

// Round watch tower: a tapered stone drum, arrow slits, a corbelled parapet
// with merlons and a conical team-tinted cap. `tall` is the Watchtower variant
// with an extra fighting tier and a banner.
fn tower_core(tall: bool) -> Mesh {
    let stone = srgb(STONE);
    let stone_dk = srgb(STONE_DARK);
    let team = srgb(TEAM_CLOTH);

    let body_h = if tall { 3.3 } else { 2.4 };
    let r = if tall { 0.52 } else { 0.5 };
    let mut parts = vec![
        part(frustum(r * 1.25, r * 1.4, 0.4, 8), stone_dk, xyz(0.0, 0.2, 0.0)),
        part(frustum(r, r * 1.18, body_h, 8), stone, xyz(0.0, 0.4 + body_h / 2.0, 0.0)),
    ];
    // String course separating tiers.
    if tall {
        parts.push(part(cyl(r * 1.06, 0.14, 8), stone_dk, xyz(0.0, 0.4 + body_h * 0.55, 0.0)));
    }
    // Arrow slits around the shaft on two levels.
    let levels: &[f32] = if tall { &[0.45, 0.72] } else { &[0.55] };
    for &ly in levels {
        for i in 0..4 {
            let a = (i as f32 / 4.0) * PI * 2.0 + 0.4;
            let y = 0.4 + body_h * ly;
            arrow_slit(&mut parts, a.cos() * r * 1.02, y, a.sin() * r * 1.02, -a);
        }
    }
    // Corbelled parapet ring (a slightly wider drum) + merlons.
    let top_y = 0.4 + body_h;
    parts.push(part(frustum(r * 1.3, r * 1.15, 0.26, 8), stone_dk, xyz(0.0, top_y + 0.13, 0.0)));
    let m_count = 8;
    for i in 0..m_count {
        let a = (i as f32 / m_count as f32) * PI * 2.0;
        parts.push(part(
            cuboid(0.2, 0.32, 0.16),
            stone,
            rot_y(a.cos() * r * 1.25, top_y + 0.42, a.sin() * r * 1.25, -a),
        ));
    }
    let roof_h = if tall { 0.95 } else { 0.75 };
    parts.push(part(
        cone(r * 1.35, roof_h, 8),
        team,
        xyz(0.0, top_y + if tall { 1.0 } else { 0.85 }, 0.0),
    ));
    parts.push(part(sphere(0.08), stone_dk, xyz(0.0, top_y + if tall { 1.55 } else { 1.3 }, 0.0)));
    if tall {
        pennant(&mut parts, r * 1.25, top_y + 0.6, 0.0, 0.8);
    }
    merge(parts)
}

// Gatehouse: two flanking stone pillars, a recessed arched gateway, battlement
// walkway with merlons and a faction banner over the arch.
fn build_gatehouse() -> Mesh {
    let stone = srgb(STONE);
    let stone_dk = srgb(STONE_DARK);
    let team = srgb(TEAM_CLOTH);

    let mut parts = Vec::new();
    for sx in [-0.38_f32, 0.38] {
        parts.push(part(cuboid(0.3, 1.45, 0.6), stone, xyz(sx, 0.725, 0.0)));
        square_merlons(&mut parts, stone, sx, 1.45, 0.0, 0.55, 0.16, 0.22);
        arrow_slit(&mut parts, sx, 1.05, 0.31, 0.0);
    }
    // Arched lintel above the passage (TS half-cylinder; full drum here — the
    // hidden lower half sits inside the dark passage recess).
    parts.push(part(cuboid(0.78, 0.26, 0.58), stone_dk, xyz(0.0, 1.28, 0.0)));
    parts.push(part(cyl(0.32, 0.58, 12), stone_dk, rot_x(0.0, 1.1, 0.0, FRAC_PI_2)));
    // Dark gateway recess so the passage reads as open.
    parts.push(part(cuboid(0.5, 1.0, 0.62), srgb(0x20201c), xyz(0.0, 0.5, 0.0)));
    // Faction banner draped over the arch.
    parts.push(part(cuboid(0.4, 0.34, 0.02), team, xyz(0.0, 1.18, 0.3)));
    // Battlement walkway joining the two towers.
    parts.push(part(cuboid(1.0, 0.14, 0.6), stone_dk, xyz(0.0, 1.55, 0.0)));
    square_merlons(&mut parts, stone, 0.0, 1.62, 0.0, 0.9, 0.18, 0.2);
    merge(parts)
}

// Levantine dwelling: flat-roofed plastered cube with a parapet, a low domed
// adjacent room, shuttered window and an awning over the door.
fn build_house() -> Mesh {
    let wall = srgb(PLASTER);
    let wall_warm = srgb(0xbfa377);
    let dark = srgb(TIMBER_DARK);
    let dome = srgb(0xd2bd8e);
    let team = srgb(TEAM_CLOTH);

    let mut parts = vec![
        // Main block, slightly off-square for a hand-built look.
        part(cuboid(1.6, 1.05, 1.7), wall, xyz(-0.1, 0.525, 0.0)),
        // Flat roof slab + low parapet so it reads as a usable rooftop.
        part(cuboid(1.66, 0.12, 1.76), wall_warm, xyz(-0.1, 1.11, 0.0)),
    ];
    for (w, d, x, z) in [
        (1.66, 0.1, -0.1, 0.86),
        (1.66, 0.1, -0.1, -0.86),
        (0.1, 1.76, 0.71, 0.0),
        (0.1, 1.76, -0.91, 0.0),
    ] {
        parts.push(part(cuboid(w, 0.18, d), wall_warm, xyz(x, 1.26, z)));
    }
    // A small lower annex with a domed cap (a second room / oven).
    parts.push(part(cuboid(0.7, 0.7, 0.7), wall_warm, xyz(0.95, 0.35, 0.4)));
    parts.push(part(sphere(0.38), dome, xyz(0.95, 0.7, 0.4)));
    // Door with a small awning, plus a shuttered window.
    parts.push(part(cuboid(0.4, 0.62, 0.12), dark, xyz(-0.1, 0.31, 0.86)));
    parts.push(part(cuboid(0.55, 0.05, 0.3), dark, rot_x(-0.1, 0.68, 0.98, 0.3)));
    parts.push(part(cuboid(0.3, 0.3, 0.06), dark, xyz(-0.55, 0.7, 0.86)));
    // Faction cloth hung on the rooftop pole.
    parts.push(part(cyl(0.03, 0.55, 5), dark, xyz(0.5, 1.6, 0.5)));
    parts.push(part(cuboid(0.4, 0.26, 0.02), team, xyz(0.71, 1.66, 0.5)));
    merge(parts)
}

// Barracks: a long timber-framed hall with exposed posts, a thatched pitched
// roof, a banner over a wide door and a training-yard weapon rack hint.
fn build_barracks() -> Mesh {
    let wall = srgb(0xb89a6a);
    let beam = srgb(TIMBER_DARK);
    let thatch = srgb(THATCH);
    let team = srgb(TEAM_CLOTH);

    let mut parts = vec![part(cuboid(2.0, 1.2, 2.0), wall, xyz(0.0, 0.6, 0.0))];
    // Exposed timber framing: corner posts + a mid rail.
    for sx in [-1.0_f32, 1.0] {
        for sz in [-1.0_f32, 1.0] {
            parts.push(part(cuboid(0.16, 1.2, 0.16), beam, xyz(sx * 0.92, 0.6, sz * 0.92)));
        }
    }
    for sz in [-1.0_f32, 1.0] {
        parts.push(part(cuboid(2.0, 0.12, 0.1), beam, xyz(0.0, 0.75, sz * 0.95)));
    }
    // Thatched gable roof from two slabs + ridge.
    for s in [-1.0_f32, 1.0] {
        parts.push(part(cuboid(2.2, 0.14, 1.25), thatch, rot_x(0.0, 1.55, s * 0.55, s * 0.5)));
    }
    parts.push(part(cuboid(2.25, 0.16, 0.18), beam, xyz(0.0, 1.92, 0.0)));
    // Gable ends filling the triangle (3-sided cone laid on its side, as in TS).
    for sz in [-1.0_f32, 1.0] {
        let rot = Quat::from_euler(
            EulerRot::XYZ,
            if sz < 0.0 { FRAC_PI_2 } else { -FRAC_PI_2 },
            0.0,
            PI,
        );
        parts.push(part(
            cone(0.9, 0.85, 3),
            wall,
            Transform::from_xyz(0.0, 1.55, sz).with_rotation(rot),
        ));
    }
    parts.push(part(cuboid(0.55, 0.85, 0.12), beam, xyz(0.0, 0.42, 1.01)));
    // Weapon rack: two upright spears against the wall.
    for sx in [0.6_f32, 0.78] {
        parts.push(part(cyl(0.02, 0.9, 4), beam, rot_z(sx, 0.45, 1.02, (sx - 0.69) * 1.5)));
    }
    // Banner on a roof pole.
    parts.push(part(cyl(0.04, 0.85, 5), beam, xyz(0.0, 2.4, 0.0)));
    parts.push(part(cuboid(0.55, 0.34, 0.02), team, xyz(0.28, 2.5, 0.0)));
    merge(parts)
}

// Open-fronted horse shed: timber barn, hay, a paddock rail and a horse hint.
fn build_stable() -> Mesh {
    let wall = srgb(0x9a7a4a);
    let roof = srgb(0x6b4a2b);
    let dark = srgb(TIMBER_DARK);
    let hay = srgb(0xc9a64a);
    let horse = srgb(0x5b4632);

    // Back wall + two stall partitions, leaving the front open.
    let mut parts = vec![part(cuboid(2.0, 0.95, 0.18), wall, xyz(0.0, 0.55, -0.78))];
    for x in [-1.0_f32, 0.0, 1.0] {
        parts.push(part(cuboid(0.16, 0.85, 1.4), wall, xyz(x, 0.5, -0.1)));
    }
    // Pitched plank roof (two slabs) over the stalls.
    for s in [-1.0_f32, 1.0] {
        parts.push(part(cuboid(2.15, 0.12, 1.0), roof, rot_x(0.0, 1.12, s * 0.42, s * 0.32)));
    }
    parts.push(part(cuboid(2.2, 0.12, 0.16), dark, xyz(0.0, 1.32, 0.0)));
    // Hay bale + loose pile.
    parts.push(part(cuboid(0.55, 0.36, 0.4), hay, xyz(-0.7, 0.18, 0.62)));
    parts.push(part(cone(0.32, 0.32, 6), hay, xyz(0.85, 0.16, 0.55)));
    // Horse hint: a low body block + neck + head poking from a stall.
    parts.push(part(cuboid(0.7, 0.32, 0.26), horse, xyz(0.35, 0.46, 0.1)));
    parts.push(part(cuboid(0.16, 0.3, 0.16), horse, rot_z(0.68, 0.66, 0.1, -0.4)));
    parts.push(part(cuboid(0.26, 0.16, 0.14), horse, xyz(0.82, 0.78, 0.1)));
    // Paddock rail at the front.
    for z in [0.78_f32, 0.96] {
        parts.push(part(cuboid(1.9, 0.06, 0.06), dark, xyz(0.0, 0.52 + (z - 0.78) * 1.1, z)));
    }
    for x in [-0.92_f32, 0.0, 0.92] {
        parts.push(part(cuboid(0.08, 0.72, 0.08), dark, xyz(x, 0.36, 0.9)));
    }
    pennant(&mut parts, 0.92, 1.55, -0.7, 0.7);
    merge(parts)
}

// Stone smithy: pitched roof, a tall venting chimney with ember glow, an anvil
// out front lit by the forge.
fn build_blacksmith() -> Mesh {
    let stone = srgb(STONE);
    let stone_dk = srgb(STONE_DARK);
    let roof = srgb(0x3a3a3a);
    let ember = srgb(0xd9531e);
    let metal = srgb(0x3c3c40);

    let mut parts = vec![part(cuboid(1.9, 1.05, 1.9), stone, xyz(0.0, 0.525, 0.0))];
    // Stone-trim quoins at the corners read as masonry.
    for sx in [-1.0_f32, 1.0] {
        for sz in [-1.0_f32, 1.0] {
            parts.push(part(cuboid(0.2, 1.05, 0.2), stone_dk, xyz(sx * 0.85, 0.525, sz * 0.85)));
        }
    }
    for s in [-1.0_f32, 1.0] {
        parts.push(part(cuboid(2.05, 0.12, 1.05), roof, rot_x(0.0, 1.18, s * 0.45, s * 0.34)));
    }
    parts.push(part(cuboid(0.4, 1.15, 0.4), stone_dk, xyz(0.62, 1.55, -0.55)));
    parts.push(part(cuboid(0.5, 0.14, 0.5), stone_dk, xyz(0.62, 2.16, -0.55)));
    parts.push(part(cuboid(0.26, 0.12, 0.26), ember, xyz(0.62, 2.24, -0.55)));
    // Forge mouth glowing under the roof.
    parts.push(part(cuboid(0.5, 0.42, 0.22), ember, xyz(-0.3, 0.42, 1.0)));
    // Anvil: stump + horned top.
    parts.push(part(frustum(0.16, 0.18, 0.34, 7), srgb(TIMBER_DARK), xyz(0.55, 0.17, 0.9)));
    parts.push(part(cuboid(0.34, 0.13, 0.18), metal, xyz(0.55, 0.4, 0.9)));
    parts.push(part(cone(0.07, 0.2, 5), metal, rot_z(0.78, 0.42, 0.9, -FRAC_PI_2)));
    pennant(&mut parts, -0.78, 1.5, 0.55, 0.7);
    merge(parts)
}

// Bazaar: a tiled awning over a trestle, two side stalls and piled goods.
fn build_market() -> Mesh {
    let wood = srgb(TIMBER);
    let cloth = srgb(TEAM_CLOTH); // main awning carries the faction colour
    let cloth2 = srgb(0xe7d39a);
    let crate_c = srgb(TIMBER_DARK);
    let fruit = srgb(0xc0532f);
    let grain = srgb(0xd9b35a);
    let pot = srgb(0x9a6a3a);

    let mut parts = vec![part(cuboid(1.9, 0.55, 0.7), wood, xyz(0.0, 0.28, -0.45))];
    for sx in [-0.88_f32, 0.88] {
        for sz in [-0.72_f32, 0.72] {
            parts.push(part(cyl(0.05, 1.5, 6), wood, xyz(sx, 0.75, sz)));
        }
    }
    // Two-slope striped awning: main faction slab + accent slab.
    parts.push(part(cuboid(2.05, 0.07, 0.95), cloth, rot_x(0.0, 1.5, -0.45, -0.2)));
    parts.push(part(cuboid(2.05, 0.07, 0.95), cloth2, rot_x(0.0, 1.5, 0.45, 0.2)));
    parts.push(part(cuboid(2.1, 0.08, 0.1), wood, xyz(0.0, 1.62, 0.0)));
    // Goods on the counter and ground: crates, fruit mound, grain sacks, a pot.
    for sx in [-0.62_f32, 0.62] {
        parts.push(part(cuboid(0.36, 0.36, 0.36), crate_c, xyz(sx, 0.18, 0.5)));
    }
    parts.push(part(cone(0.2, 0.22, 6), fruit, xyz(0.0, 0.66, -0.4)));
    parts.push(part(frustum(0.13, 0.17, 0.32, 7), grain, xyz(-0.45, 0.18, 0.62)));
    parts.push(part(frustum(0.1, 0.16, 0.34, 8), pot, xyz(0.45, 0.59, -0.4)));
    pennant(&mut parts, 0.92, 1.78, 0.72, 0.7);
    merge(parts)
}

// Granary: stepped clay drum capped by a low dome — a desert silo.
fn build_granary() -> Mesh {
    let clay = srgb(0xcdb07a);
    let clay_dark = srgb(0xb89a64);
    let dome = srgb(0xd8c590);
    let team = srgb(TEAM_CLOTH);

    let parts = vec![
        part(frustum(1.0, 1.1, 0.4, 12), clay_dark, xyz(0.0, 0.2, 0.0)),
        part(frustum(0.82, 0.95, 1.25, 12), clay, xyz(0.0, 1.0, 0.0)),
        part(cyl(0.86, 0.16, 12), team, xyz(0.0, 1.2, 0.0)),
        // Low dome cap (TS hemisphere; full sphere here, lower half hidden in the silo).
        part(sphere(0.82), dome, xyz(0.0, 1.62, 0.0)),
        part(sphere(0.1), clay_dark, xyz(0.0, 2.5, 0.0)),
        // Loading hatch + ramp.
        part(cuboid(0.42, 0.6, 0.12), clay_dark, xyz(0.0, 0.62, 0.92)),
        part(cuboid(0.5, 0.06, 0.5), srgb(TIMBER_DARK), rot_x(0.0, 0.16, 1.15, 0.35)),
    ];
    merge(parts)
}

// Fishing hut: a planked hut on stilts over the shore with a dock and drying nets.
fn build_fishing_hut() -> Mesh {
    let wood = srgb(0x9a7a52);
    let roof = srgb(0x6a4a2a);
    let net = srgb(0xb8b8a0);
    let dark = srgb(TIMBER_DARK);

    let mut parts = Vec::new();
    for sx in [-0.32_f32, 0.32] {
        for sz in [-0.32_f32, 0.32] {
            parts.push(part(cyl(0.055, 0.6, 5), dark, xyz(sx, 0.3, sz)));
        }
    }
    parts.push(part(cuboid(1.0, 0.08, 1.0), wood, xyz(0.0, 0.6, 0.0)));
    parts.push(part(cuboid(0.85, 0.55, 0.85), wood, xyz(0.0, 0.92, 0.0)));
    // Lean-to single-pitch roof.
    parts.push(part(cuboid(1.0, 0.1, 1.0), roof, rot_x(0.0, 1.28, 0.0, 0.28)));
    // Plank dock reaching toward the water with two pilings.
    parts.push(part(cuboid(0.5, 0.07, 1.0), wood, xyz(0.0, 0.5, 0.95)));
    for dz in [0.7_f32, 1.2] {
        parts.push(part(cyl(0.045, 0.5, 5), dark, xyz(0.18, 0.28, dz)));
    }
    // Drying-net frame off the side.
    parts.push(part(cyl(0.035, 1.1, 5), wood, xyz(0.72, 0.95, -0.1)));
    parts.push(part(cuboid(0.55, 0.55, 0.02), net, rot_y(0.78, 0.78, 0.1, -0.5)));
    // A couple of floats / fish on the deck.
    parts.push(part(sphere(0.08), roof, xyz(-0.3, 0.7, 0.3)));
    pennant(&mut parts, -0.45, 1.45, -0.35, 0.7);
    merge(parts)
}

// Siege workshop: an open timber frame, sawn lumber, and a half-built engine
// (a mangonel-like throwing arm on a wheeled bed).
fn build_siege_workshop() -> Mesh {
    let wood = srgb(0x7a5a32);
    let wood_light = srgb(0xa07c46);
    let roof = srgb(TIMBER_DARK);
    let metal = srgb(0x8a8a8a);

    // Open frame: four corner posts + back wall only, leaving the front open.
    let mut parts = Vec::new();
    for sx in [-0.92_f32, 0.92] {
        for sz in [-0.7_f32, 0.7] {
            parts.push(part(cuboid(0.16, 1.3, 0.16), wood, xyz(sx, 0.65, sz)));
        }
    }
    parts.push(part(cuboid(2.0, 1.1, 0.16), wood, xyz(0.0, 0.6, -0.78)));
    // Pitched plank roof.
    for s in [-1.0_f32, 1.0] {
        parts.push(part(cuboid(2.15, 0.1, 0.95), roof, rot_x(0.0, 1.42, s * 0.42, s * 0.3)));
    }
    // Cross-brace on the back wall.
    parts.push(part(cuboid(2.0, 0.1, 0.1), wood_light, rot_z(0.0, 0.9, -0.7, 0.18)));
    // Stacked sawn lumber.
    for i in 0..3 {
        parts.push(part(
            cyl(0.07, 1.0, 6),
            wood_light,
            rot_x(-0.7, 0.12 + i as f32 * 0.15, -0.4 + (i % 2) as f32 * 0.05, FRAC_PI_2),
        ));
    }
    // Half-built engine out front: bed, two wheels, a raised throwing arm.
    parts.push(part(cuboid(0.9, 0.16, 0.5), wood, xyz(0.15, 0.34, 0.78)));
    for wz in [0.55_f32, 1.0] {
        parts.push(part(cyl(0.22, 0.1, 10), metal, rot_x(0.15, 0.22, wz, FRAC_PI_2)));
    }
    parts.push(part(cuboid(0.08, 0.8, 0.08), wood_light, rot_x(0.15, 0.7, 0.78, -0.7)));
    parts.push(part(cuboid(0.16, 0.1, 0.16), metal, xyz(0.15, 1.02, 0.45)));
    pennant(&mut parts, -0.92, 1.7, -0.7, 0.7);
    merge(parts)
}

// Keep: a fortified donjon on a battered plinth, four corner drum towers with
// conical caps, crenellated curtain walls, a tall central tower with a banner
// and an arched main gate.
fn build_keep() -> Mesh {
    let stone = srgb(STONE);
    let dark = srgb(STONE_DARK);
    let team = srgb(TEAM_CLOTH);

    let s = 3.2_f32;
    let wall_h = 1.3_f32;
    let wall_t = 0.34_f32;
    let half = s / 2.0 - wall_t / 2.0;

    // Battered plinth (wider at the foot).
    let mut parts = vec![
        part(cuboid(s + 0.3, 0.3, s + 0.3), dark, xyz(0.0, 0.15, 0.0)),
        part(cuboid(s, 0.3, s), dark, xyz(0.0, 0.4, 0.0)),
    ];

    let mk_wall = |parts: &mut Vec<Mesh>, x: f32, z: f32, rot: f32| {
        parts.push(part(
            cuboid(s - 0.2, wall_h, wall_t),
            stone,
            rot_y(x, 0.55 + wall_h / 2.0, z, rot),
        ));
        let n = 6;
        for i in 0..n {
            let off = (i as f32 / (n - 1) as f32 - 0.5) * (s - 0.7);
            let (cx, cz) =
                if rot == 0.0 { (x + off, z) } else { (x, z + off) };
            parts.push(part(cuboid(0.3, 0.32, wall_t), stone, rot_y(cx, 0.55 + wall_h + 0.16, cz, rot)));
        }
        // Arrow slit centred on each curtain face.
        arrow_slit(parts, x, 0.55 + wall_h * 0.55, z, rot);
    };
    mk_wall(&mut parts, 0.0, half, 0.0);
    mk_wall(&mut parts, 0.0, -half, 0.0);
    mk_wall(&mut parts, half, 0.0, FRAC_PI_2);
    mk_wall(&mut parts, -half, 0.0, FRAC_PI_2);

    let tower_h = 2.3_f32;
    let tower_r = 0.48_f32;
    for (sx, sz) in [(1.0_f32, 1.0_f32), (1.0, -1.0), (-1.0, -1.0), (-1.0, 1.0)] {
        let tx = sx * (s / 2.0 - 0.05);
        let tz = sz * (s / 2.0 - 0.05);
        parts.push(part(
            frustum(tower_r, tower_r * 1.15, tower_h, 8),
            stone,
            xyz(tx, 0.55 + tower_h / 2.0, tz),
        ));
        // Parapet ring + merlons on each drum.
        let t_top = 0.55 + tower_h;
        parts.push(part(frustum(tower_r * 1.2, tower_r * 1.05, 0.2, 8), dark, xyz(tx, t_top + 0.1, tz)));
        for i in 0..6 {
            let a = (i as f32 / 6.0) * PI * 2.0;
            parts.push(part(
                cuboid(0.16, 0.24, 0.13),
                stone,
                rot_y(tx + a.cos() * tower_r * 1.15, t_top + 0.3, tz + a.sin() * tower_r * 1.15, -a),
            ));
        }
        parts.push(part(cone(tower_r * 1.35, 0.8, 8), team, xyz(tx, t_top + 0.6, tz)));
    }

    let keep_h = 2.9_f32;
    let keep_s = 1.4_f32;
    parts.push(part(cuboid(keep_s, keep_h, keep_s), stone, xyz(0.0, 0.55 + keep_h / 2.0, 0.0)));
    // Buttress pilasters on the central tower faces.
    for (ax, az) in [(1.0_f32, 0.0_f32), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
        parts.push(part(
            cuboid(0.16, keep_h, 0.16),
            dark,
            xyz(ax * keep_s * 0.5, 0.55 + keep_h / 2.0, az * keep_s * 0.5),
        ));
    }
    // Crown the donjon with a full merlon ring.
    square_merlons(&mut parts, stone, 0.0, 0.55 + keep_h, 0.0, keep_s + 0.1, 0.26, 0.36);
    // Upper-window arrow slits on the central tower.
    for (sx, sz, rot) in [
        (0.0, keep_s / 2.0, 0.0),
        (0.0, -keep_s / 2.0, 0.0),
        (keep_s / 2.0, 0.0, FRAC_PI_2),
        (-keep_s / 2.0, 0.0, FRAC_PI_2),
    ] {
        arrow_slit(&mut parts, sx, 0.55 + keep_h * 0.7, sz, rot);
    }

    parts.push(part(cyl(0.045, 1.1, 5), dark, xyz(0.0, 0.55 + keep_h + 0.7, 0.0)));
    pennant(&mut parts, 0.0, 0.55 + keep_h + 0.7, 0.0, 1.1);

    // Recessed arched main gate in the front curtain (TS half-cylinder arch;
    // full drum here, lower half hidden against the dark gate recess).
    parts.push(part(cuboid(0.74, 0.95, 0.24), srgb(0x2a241c), xyz(0.0, 0.55 + 0.47, half + 0.04)));
    parts.push(part(cyl(0.37, 0.24, 10), stone, rot_x(0.0, 0.55 + 0.95, half + 0.04, FRAC_PI_2)));

    merge(parts)
}
