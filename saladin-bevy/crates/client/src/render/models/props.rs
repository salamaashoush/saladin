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

/// Resource-node mesh by type: conifer / boulder / forage tuft / gold vein.
pub fn resource_node_mesh(res: ResourceType) -> Mesh {
    let c = lin(resource_def(res).color);
    match res {
        ResourceType::Stone => part(
            dodecahedron(0.55),
            c,
            Transform::from_xyz(0.0, 0.4, 0.0).with_scale(Vec3::new(1.0, 0.7, 1.0)),
        ),
        ResourceType::Food => part(frustum(0.45, 0.5, 0.5, 8), c, xyz(0.0, 0.32, 0.0)),
        ResourceType::Gold => part(octahedron(0.45), c, xyz(0.0, 0.45, 0.0)),
        _ => merge(vec![
            part(cone(0.6, 1.5, 7), c, xyz(0.0, 1.2, 0.0)),
            part(frustum(0.12, 0.16, 0.6, 6), lin(0x6b4a2b), xyz(0.0, 0.3, 0.0)),
        ]),
    }
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
    merge(vec![
        part(icosahedron(0.3), c, xyz(0.0, 0.26, 0.0)),
        part(icosahedron(0.19), c, xyz(0.22, 0.16, 0.1)),
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

// A faceted loose stone, squashed so it sits like a rock.
fn rock() -> Mesh {
    part(
        dodecahedron(0.3),
        lin(0x8a8175),
        Transform::from_xyz(0.0, 0.16, 0.0).with_scale(Vec3::new(1.0, 0.7, 1.0)),
    )
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

// A small cosmetic conifer: stacked cone tiers over a stub trunk.
fn pine() -> Mesh {
    let mut parts = vec![part(frustum(0.08, 0.11, 0.5, 5), lin(0x5b4127), xyz(0.0, 0.25, 0.0))];
    let tiers = lin(0x2f6b30);
    for (r, h, y) in [(0.55, 0.8, 0.6), (0.42, 0.7, 1.05), (0.28, 0.6, 1.45)] {
        parts.push(part(cone(r, h, 6), tiers, xyz(0.0, y, 0.0)));
    }
    merge(parts)
}
