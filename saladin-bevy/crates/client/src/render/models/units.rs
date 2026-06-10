//! Detailed procedural unit meshes (port of src/game/meshes/units.ts +
//! unitImpostor.ts). Each kind is baked into ONE mesh with per-vertex colors:
//! team-tintable parts (the TS `tunic` material) are vertex-colored WHITE so the
//! owner's team material tints them; fixed details (wood, metal, skin, hide)
//! keep their baked colors.

use std::f32::consts::{FRAC_PI_2, PI};

use bevy::mesh::{Mesh, MeshBuilder, Meshable};
use bevy::prelude::*;
use saladin_sim::{UnitKind, unit_def};

/// Team-tintable parts: pure white so instance/material tint multiplies in.
const TINT: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

fn srgb(hex: u32) -> [f32; 4] {
    let c = Color::srgb_u8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8).to_linear();
    [c.red, c.green, c.blue, 1.0]
}

struct Pal {
    skin: [f32; 4],
    metal: [f32; 4],
    steel: [f32; 4],
    iron: [f32; 4],
    wood: [f32; 4],
    wood_dark: [f32; 4],
    leather: [f32; 4],
    rope: [f32; 4],
    gold: [f32; 4],
    white_cloth: [f32; 4],
    green_sash: [f32; 4],
    hide_bay: [f32; 4],
    hide_dark: [f32; 4],
    hide_grey: [f32; 4],
    stone: [f32; 4],
}

fn pal() -> Pal {
    Pal {
        skin: srgb(0xd9a878),
        metal: srgb(0x9aa0a6),
        steel: srgb(0xc4c9cf),
        iron: srgb(0x4a4d52),
        wood: srgb(0x6b4a2b),
        wood_dark: srgb(0x4a3522),
        leather: srgb(0x7a5230),
        rope: srgb(0xb9a06a),
        gold: srgb(0xd6b24a),
        white_cloth: srgb(0xf2efe6),
        green_sash: srgb(0x2f7d4f),
        hide_bay: srgb(0x6a4a2a),
        hide_dark: srgb(0x33251a),
        hide_grey: srgb(0x5a4632),
        stone: srgb(0x7a7a7a),
    }
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

fn at(x: f32, y: f32, z: f32, q: Quat) -> Transform {
    Transform::from_xyz(x, y, z).with_rotation(q)
}

fn boxm(w: f32, h: f32, d: f32) -> Mesh {
    Mesh::from(Cuboid::new(w, h, d))
}

/// THREE CylinderGeometry(rTop, rBottom, h, seg): tapered → conical frustum.
fn cyl(r_top: f32, r_bot: f32, h: f32, seg: u32) -> Mesh {
    if (r_top - r_bot).abs() < 1e-5 {
        Cylinder::new(r_top, h).mesh().resolution(seg).build()
    } else {
        ConicalFrustum { radius_top: r_top, radius_bottom: r_bot, height: h }
            .mesh()
            .resolution(seg)
            .build()
    }
}

fn cone(r: f32, h: f32, seg: u32) -> Mesh {
    Cone { radius: r, height: h }.mesh().resolution(seg).build()
}

fn sphere(r: f32, sectors: u32, stacks: u32) -> Mesh {
    Sphere::new(r).mesh().uv(sectors, stacks)
}

/// Bevy torus lies in the XZ plane (axis Y); THREE's lies in XY (axis Z).
fn torus(major: f32, minor: f32, minor_res: usize, major_res: usize, arc: f32) -> Mesh {
    Torus { minor_radius: minor, major_radius: major }
        .mesh()
        .minor_resolution(minor_res)
        .major_resolution(major_res)
        .angle_range(0.0..=arc)
        .build()
}

/// Rotation that maps a bevy torus into THREE's XY-plane orientation, with the
/// THREE rotation `q` applied on top.
fn three_torus_rot(q: Quat) -> Quat {
    q * Quat::from_rotation_x(FRAC_PI_2)
}

fn is_mounted(k: UnitKind) -> bool {
    matches!(k, UnitKind::Knight | UnitKind::HorseArcher | UnitKind::Mamluk)
}

/// A four-legged horse centred on origin, forward = +Z.
fn push_horse(parts: &mut Vec<Mesh>, r: f32, h: f32, hide: [f32; 4], p: &Pal) {
    parts.push(part(boxm(r * 0.85, h * 0.38, h * 1.05), hide, xyz(0.0, h * 0.32, 0.0)));
    parts.push(part(boxm(r * 0.78, h * 0.34, h * 0.3), hide, xyz(0.0, h * 0.34, h * 0.5)));
    parts.push(part(boxm(r * 0.82, h * 0.4, h * 0.32), hide, xyz(0.0, h * 0.36, -h * 0.5)));
    parts.push(part(
        boxm(r * 0.42, h * 0.5, r * 0.45),
        hide,
        at(0.0, h * 0.55, h * 0.5, Quat::from_rotation_x(-0.55)),
    ));
    parts.push(part(
        boxm(r * 0.36, r * 0.42, h * 0.42),
        hide,
        at(0.0, h * 0.74, h * 0.72, Quat::from_rotation_x(0.2)),
    ));
    parts.push(part(boxm(r * 0.28, r * 0.3, h * 0.2), hide, xyz(0.0, h * 0.66, h * 0.9)));
    // Ears.
    for sx in [-1.0f32, 1.0] {
        parts.push(part(cone(r * 0.08, r * 0.18, 4), hide, xyz(sx * r * 0.12, h * 0.9, h * 0.62)));
    }
    // Mane.
    parts.push(part(
        boxm(r * 0.12, h * 0.5, r * 0.12),
        p.hide_dark,
        at(0.0, h * 0.62, h * 0.46, Quat::from_rotation_x(-0.55)),
    ));
    // Tail.
    parts.push(part(
        cone(r * 0.12, h * 0.45, 5),
        p.hide_dark,
        at(0.0, h * 0.32, -h * 0.66, Quat::from_rotation_x(0.7)),
    ));
    // Legs + hooves, slightly splayed for a planted stance.
    for sx in [-1.0f32, 1.0] {
        for sz in [-1.0f32, 1.0] {
            parts.push(part(
                cyl(r * 0.11, r * 0.09, h * 0.36, 5),
                hide,
                xyz(sx * r * 0.3, h * 0.13, sz * h * 0.4),
            ));
            parts.push(part(
                boxm(r * 0.16, r * 0.1, r * 0.2),
                p.iron,
                xyz(sx * r * 0.3, h * 0.03, sz * h * 0.4),
            ));
        }
    }
}

/// A small saddle blanket in the team colour, draped over the horse's back.
fn push_caparison(parts: &mut Vec<Mesh>, r: f32, h: f32) {
    parts.push(part(boxm(r * 1.0, h * 0.06, h * 0.7), TINT, xyz(0.0, h * 0.5, -h * 0.02)));
    for sx in [-1.0f32, 1.0] {
        parts.push(part(boxm(0.04, h * 0.22, h * 0.6), TINT, xyz(sx * r * 0.5, h * 0.4, -h * 0.02)));
    }
}

/// Full-detail unit mesh, team parts vertex-colored white for tinting.
pub fn unit_mesh(kind: UnitKind) -> Mesh {
    let d = unit_def(kind);
    let h = d.height.to_num::<f32>();
    let r = d.radius.to_num::<f32>();
    let p = pal();
    let mut parts: Vec<Mesh> = Vec::new();

    let mounted = is_mounted(kind);
    let siege = matches!(kind, UnitKind::Ram | UnitKind::Mangonel);

    // ---- Infantry / rider body (skipped for siege engines) ----
    if !siege {
        let base_y = if mounted { h * 0.55 } else { 0.0 };

        if mounted {
            // Splayed riding legs straddling the horse.
            for sx in [-1.0f32, 1.0] {
                parts.push(part(
                    boxm(r * 0.28, h * 0.34, r * 0.32),
                    TINT,
                    at(sx * r * 0.42, base_y + h * 0.06, 0.0, Quat::from_rotation_z(sx * 0.35)),
                ));
                parts.push(part(
                    cyl(r * 0.12, r * 0.1, h * 0.36, 5),
                    p.leather,
                    xyz(sx * r * 0.55, base_y - h * 0.18, 0.0),
                ));
            }
        } else {
            for sx in [-1.0f32, 1.0] {
                parts.push(part(
                    cyl(r * 0.24, r * 0.2, h * 0.46, 6),
                    p.leather,
                    xyz(sx * r * 0.32, h * 0.23, 0.0),
                ));
                // Boots.
                parts.push(part(
                    boxm(r * 0.28, r * 0.18, r * 0.42),
                    p.wood_dark,
                    xyz(sx * r * 0.32, h * 0.04, r * 0.08),
                ));
            }
        }

        // Torso: tapered tunic, plus belt.
        parts.push(part(cyl(r * 0.7, r * 0.92, h * 0.6, 8), TINT, xyz(0.0, base_y + h * 0.72, 0.0)));
        parts.push(part(
            cyl(r * 0.74, r * 0.74, h * 0.07, 8),
            p.leather,
            xyz(0.0, base_y + h * 0.46, 0.0),
        ));

        // Shoulders + forward-set arms (omitted for the cloaked Imam).
        if kind != UnitKind::Imam {
            for sx in [-1.0f32, 1.0] {
                parts.push(part(sphere(r * 0.26, 6, 5), TINT, xyz(sx * r * 0.72, base_y + h * 0.92, 0.0)));
                parts.push(part(
                    cyl(r * 0.16, r * 0.18, h * 0.42, 5),
                    TINT,
                    at(sx * r * 0.78, base_y + h * 0.7, r * 0.08, Quat::from_rotation_x(-0.25)),
                ));
            }
        }

        // Head + neck.
        parts.push(part(cyl(r * 0.26, r * 0.3, h * 0.1, 6), p.skin, xyz(0.0, base_y + h * 1.0, 0.0)));
        parts.push(part(
            sphere(r * 0.62, 9, 7),
            p.skin,
            Transform {
                translation: Vec3::new(0.0, base_y + h * 1.12, 0.0),
                rotation: Quat::IDENTITY,
                scale: Vec3::new(0.9, 1.05, 0.95),
            },
        ));

        // ---- Per-unit kit ----
        match kind {
            UnitKind::Peasant => {
                // Wide-brimmed straw hat + jerkin band + hoe held across the body.
                parts.push(part(cone(r * 0.85, r * 0.45, 8), p.rope, xyz(0.0, base_y + h * 1.26, 0.0)));
                parts.push(part(
                    cyl(r * 0.85, r * 0.85, 0.03, 8),
                    p.rope,
                    xyz(0.0, base_y + h * 1.18, 0.0),
                ));
                parts.push(part(
                    cyl(r * 0.72, r * 0.72, h * 0.18, 8),
                    p.leather,
                    xyz(0.0, base_y + h * 0.86, 0.0),
                ));
                parts.push(part(
                    cyl(0.025, 0.025, h * 1.6, 5),
                    p.wood,
                    at(r * 0.85, base_y + h * 0.8, 0.0, Quat::from_rotation_z(0.12)),
                ));
                parts.push(part(
                    boxm(0.06, r * 0.5, 0.16),
                    p.iron,
                    at(r * 1.02, base_y + h * 1.5, 0.0, Quat::from_rotation_z(0.9)),
                ));
            }
            UnitKind::Spearman => {
                // Conical nasal helm + round shield + long spear.
                parts.push(part(cone(r * 0.66, r * 0.7, 8), p.steel, xyz(0.0, base_y + h * 1.34, 0.0)));
                parts.push(part(sphere(0.04, 5, 4), p.steel, xyz(0.0, base_y + h * 1.56, 0.0)));
                parts.push(part(boxm(0.05, r * 0.3, 0.04), p.steel, xyz(0.0, base_y + h * 1.06, r * 0.55)));
                parts.push(part(
                    cyl(r * 0.7, r * 0.7, 0.07, 14),
                    TINT,
                    at(
                        -r * 1.0,
                        base_y + h * 0.74,
                        r * 0.12,
                        Quat::from_euler(EulerRot::XYZ, FRAC_PI_2, 0.0, 0.15),
                    ),
                ));
                parts.push(part(sphere(r * 0.16, 7, 5), p.steel, xyz(-r * 1.04, base_y + h * 0.74, r * 0.18)));
                parts.push(part(cyl(0.028, 0.032, h * 2.6, 5), p.wood, xyz(r * 0.92, base_y + h * 1.0, 0.0)));
                parts.push(part(cone(0.06, 0.28, 6), p.steel, xyz(r * 0.92, base_y + h * 2.3, 0.0)));
                parts.push(part(boxm(0.02, 0.1, 0.12), p.steel, xyz(r * 0.92, base_y + h * 2.1, 0.0)));
            }
            UnitKind::Archer => {
                // Pointed hood (team colour), shouldered bow and a back quiver.
                parts.push(part(cone(r * 0.72, r * 0.85, 7), TINT, xyz(0.0, base_y + h * 1.28, -r * 0.04)));
                // Cowl drape onto the shoulders (TS hemisphere -> full sphere; the
                // lower half hides inside the tunic torso).
                parts.push(part(sphere(r * 0.6, 8, 6), TINT, xyz(0.0, base_y + h * 1.02, -r * 0.06)));
                parts.push(part(
                    torus(r * 0.95, 0.035, 5, 12, PI * 1.25),
                    p.wood,
                    at(
                        -r * 1.0,
                        base_y + h * 0.85,
                        r * 0.05,
                        three_torus_rot(Quat::from_rotation_z(FRAC_PI_2 - 0.65)),
                    ),
                ));
                parts.push(part(cyl(0.006, 0.006, r * 1.7, 3), p.rope, xyz(-r * 0.86, base_y + h * 0.85, r * 0.05)));
                parts.push(part(
                    cyl(r * 0.2, r * 0.24, h * 0.55, 6),
                    p.leather,
                    at(r * 0.5, base_y + h * 0.95, -r * 0.4, Quat::from_rotation_x(0.3)),
                ));
                for dx in [-0.06f32, 0.0, 0.06] {
                    parts.push(part(
                        cyl(0.012, 0.012, h * 0.4, 4),
                        p.wood,
                        xyz(r * 0.5 + dx, base_y + h * 1.32, -r * 0.5),
                    ));
                    parts.push(part(cone(0.04, 0.08, 4), TINT, xyz(r * 0.5 + dx, base_y + h * 1.5, -r * 0.5)));
                }
            }
            UnitKind::Knight => {
                // Heavy mounted lancer: great helm, mail drape, kite shield, lance.
                push_horse(&mut parts, r, h, p.hide_grey, &p);
                push_caparison(&mut parts, r, h);
                // Surcoat skirt over the saddle.
                parts.push(part(cone(r * 0.85, h * 0.45, 8), TINT, xyz(0.0, base_y + h * 0.42, 0.0)));
                // Great helm + dome (TS hemisphere -> full sphere) + visor slit.
                parts.push(part(cyl(r * 0.5, r * 0.52, h * 0.42, 8), p.steel, xyz(0.0, base_y + h * 1.18, 0.0)));
                parts.push(part(sphere(r * 0.5, 8, 5), p.steel, xyz(0.0, base_y + h * 1.38, 0.0)));
                parts.push(part(boxm(r * 0.6, 0.04, 0.04), p.iron, xyz(0.0, base_y + h * 1.2, r * 0.5)));
                // Crest in team colour.
                parts.push(part(boxm(0.04, r * 0.4, r * 0.45), TINT, xyz(0.0, base_y + h * 1.6, 0.0)));
                // Mail drape over the shoulders.
                parts.push(part(
                    cyl(r * 0.82, r * 0.86, h * 0.16, 8),
                    p.metal,
                    xyz(0.0, base_y + h * 0.9, 0.0),
                ));
                // Kite shield: 3-sided cone squashed flat, point down.
                parts.push(part(
                    cone(r * 0.5, h * 0.85, 3),
                    TINT,
                    Transform {
                        translation: Vec3::new(-r * 0.95, base_y + h * 0.6, r * 0.2),
                        rotation: Quat::from_euler(EulerRot::XYZ, PI, 0.2, 0.0),
                        scale: Vec3::new(1.0, 1.0, 0.18),
                    },
                ));
                // Couched lance angled forward (+Z), tip + pennon.
                parts.push(part(
                    cyl(0.03, 0.045, h * 2.9, 6),
                    p.wood,
                    at(r * 0.85, base_y + h * 0.78, h * 0.2, Quat::from_rotation_x(FRAC_PI_2 - 0.12)),
                ));
                parts.push(part(cone(0.07, 0.32, 6), p.steel, xyz(r * 0.85, base_y + h * 0.95, h * 1.65)));
                parts.push(part(boxm(0.02, r * 0.5, r * 0.7), TINT, xyz(r * 0.85, base_y + h * 1.05, h * 0.95)));
            }
            UnitKind::HorseArcher => {
                // Light steppe cavalry: turban, recurve bow, minimal armour.
                push_horse(&mut parts, r, h, p.hide_bay, &p);
                parts.push(part(boxm(r * 0.9, h * 0.05, h * 0.55), TINT, xyz(0.0, base_y * 0.92, 0.0)));
                // Wrapped turban + team-colour band (horizontal ring around the head).
                parts.push(part(
                    sphere(r * 0.5, 8, 6),
                    p.white_cloth,
                    Transform {
                        translation: Vec3::new(0.0, base_y + h * 1.18, 0.0),
                        rotation: Quat::IDENTITY,
                        scale: Vec3::new(1.0, 0.78, 1.0),
                    },
                ));
                parts.push(part(torus(r * 0.5, r * 0.12, 5, 10, PI * 2.0), TINT, xyz(0.0, base_y + h * 1.1, 0.0)));
                // Sash across the chest.
                parts.push(part(
                    boxm(r * 1.4, r * 0.22, 0.04),
                    TINT,
                    at(0.0, base_y + h * 0.75, r * 0.32, Quat::from_rotation_z(0.5)),
                ));
                // Recurve bow held out to the left.
                parts.push(part(
                    torus(r * 0.85, 0.03, 5, 14, PI * 1.15),
                    p.wood,
                    at(
                        -r * 1.0,
                        base_y + h * 0.92,
                        r * 0.1,
                        three_torus_rot(Quat::from_euler(EulerRot::XYZ, 0.0, 0.3, FRAC_PI_2 - 0.5)),
                    ),
                ));
                parts.push(part(cyl(0.005, 0.005, r * 1.5, 3), p.rope, xyz(-r * 0.88, base_y + h * 0.92, r * 0.1)));
                // Quiver at the hip.
                parts.push(part(
                    cyl(r * 0.16, r * 0.2, h * 0.4, 6),
                    p.leather,
                    at(r * 0.6, base_y + h * 0.5, -r * 0.2, Quat::from_rotation_x(0.25)),
                ));
            }
            UnitKind::Mamluk => {
                // Ornate elite cavalry: lamellar coat, plumed helm, raised sabre.
                push_horse(&mut parts, r, h, p.hide_dark, &p);
                push_caparison(&mut parts, r, h);
                parts.push(part(cyl(r * 0.78, r * 0.86, h * 0.5, 8), p.metal, xyz(0.0, base_y + h * 0.72, 0.0)));
                parts.push(part(boxm(r * 0.5, h * 0.3, 0.05), p.gold, xyz(0.0, base_y + h * 0.82, r * 0.42)));
                // Pointed helm + gold tip + mail aventail.
                parts.push(part(cone(r * 0.52, h * 0.5, 8), p.steel, xyz(0.0, base_y + h * 1.28, 0.0)));
                parts.push(part(sphere(0.045, 5, 4), p.gold, xyz(0.0, base_y + h * 1.55, 0.0)));
                parts.push(part(cyl(r * 0.46, r * 0.5, h * 0.14, 8), p.metal, xyz(0.0, base_y + h * 1.02, 0.0)));
                // Plume in team colour.
                parts.push(part(
                    cone(r * 0.12, h * 0.55, 5),
                    TINT,
                    at(0.0, base_y + h * 1.75, -r * 0.05, Quat::from_rotation_x(-0.3)),
                ));
                // Raised curved sabre in the right hand + gold hilt.
                parts.push(part(
                    torus(r * 0.55, 0.03, 5, 10, PI * 0.85),
                    p.steel,
                    at(
                        r * 1.0,
                        base_y + h * 1.2,
                        0.0,
                        three_torus_rot(Quat::from_euler(EulerRot::XYZ, 0.2, 0.0, -0.5)),
                    ),
                ));
                parts.push(part(boxm(0.05, r * 0.22, 0.05), p.gold, xyz(r * 0.95, base_y + h * 0.95, 0.0)));
                // Small round shield on the off side.
                parts.push(part(
                    cyl(r * 0.45, r * 0.45, 0.06, 12),
                    TINT,
                    at(-r * 0.95, base_y + h * 0.8, r * 0.1, Quat::from_rotation_x(FRAC_PI_2)),
                ));
                parts.push(part(sphere(r * 0.12, 6, 5), p.gold, xyz(-r * 0.99, base_y + h * 0.8, r * 0.14)));
            }
            UnitKind::Crossbowman => {
                // Kettle helm (TS hemisphere -> squashed sphere) + brim.
                parts.push(part(
                    sphere(r * 0.6, 8, 6),
                    p.steel,
                    Transform {
                        translation: Vec3::new(0.0, base_y + h * 1.18, 0.0),
                        rotation: Quat::IDENTITY,
                        scale: Vec3::new(1.0, 0.7, 1.0),
                    },
                ));
                parts.push(part(cyl(r * 0.75, r * 0.75, 0.05, 12), p.steel, xyz(0.0, base_y + h * 1.12, 0.0)));
                // Crossbow: stock + prod levelled forward, bolt loaded.
                parts.push(part(boxm(0.07, 0.08, h * 0.95), p.wood, xyz(-r * 0.7, base_y + h * 0.92, r * 0.3)));
                parts.push(part(
                    boxm(r * 1.5, 0.05, 0.07),
                    p.steel,
                    at(-r * 0.7, base_y + h * 0.92, r * 0.7, Quat::from_rotation_y(0.15)),
                ));
                parts.push(part(cyl(0.012, 0.012, h * 0.5, 4), p.iron, xyz(-r * 0.7, base_y + h * 0.94, r * 0.6)));
                // Pavise: tall body shield planted beside the soldier, rib + spike.
                parts.push(part(boxm(r * 1.1, h * 1.2, 0.08), TINT, xyz(r * 1.15, base_y + h * 0.35, r * 0.15)));
                parts.push(part(boxm(0.06, h * 1.1, 0.1), p.wood_dark, xyz(r * 1.15, base_y + h * 0.35, r * 0.2)));
                parts.push(part(cone(0.05, h * 0.2, 5), p.iron, xyz(r * 1.15, base_y - h * 0.32, r * 0.15)));
            }
            UnitKind::Imam => {
                // Robed support figure: flowing robe, white turban, prayer staff.
                parts.push(part(cone(r * 1.05, h * 1.15, 10), TINT, xyz(0.0, base_y + h * 0.5, 0.0)));
                parts.push(part(cone(r * 0.78, h * 0.7, 8), p.white_cloth, xyz(0.0, base_y + h * 0.78, r * 0.04)));
                // Wide green sash.
                parts.push(part(
                    cyl(r * 0.7, r * 0.78, h * 0.12, 10),
                    p.green_sash,
                    xyz(0.0, base_y + h * 0.74, 0.0),
                ));
                // Layered turban + wrap + team-colour tail.
                parts.push(part(
                    sphere(r * 0.6, 10, 8),
                    p.white_cloth,
                    Transform {
                        translation: Vec3::new(0.0, base_y + h * 1.18, 0.0),
                        rotation: Quat::IDENTITY,
                        scale: Vec3::new(1.0, 0.7, 1.0),
                    },
                ));
                parts.push(part(
                    torus(r * 0.58, r * 0.14, 6, 10, PI * 2.0),
                    p.white_cloth,
                    xyz(0.0, base_y + h * 1.1, 0.0),
                ));
                parts.push(part(boxm(0.04, h * 0.3, r * 0.3), TINT, xyz(0.0, base_y + h * 1.0, -r * 0.5)));
                // Tall staff with a gilded knob + ring.
                parts.push(part(cyl(0.022, 0.026, h * 1.7, 5), p.wood, xyz(r * 0.92, base_y + h * 0.78, 0.0)));
                parts.push(part(sphere(0.07, 8, 6), p.gold, xyz(r * 0.92, base_y + h * 1.65, 0.0)));
                parts.push(part(torus(0.06, 0.018, 5, 8, PI * 2.0), p.gold, xyz(r * 0.92, base_y + h * 1.55, 0.0)));
            }
            _ => {}
        }
    }

    // ---- Siege engines ----
    match kind {
        UnitKind::Ram => {
            // Timber-roofed wheeled battering ram with an iron-capped head.
            parts.push(part(boxm(h * 1.3, h * 0.12, h * 0.7), p.wood_dark, xyz(0.0, r * 0.5, 0.0)));
            // A-frame uprights supporting the roof.
            for sz in [-1.0f32, 1.0] {
                for sx in [-1.0f32, 1.0] {
                    parts.push(part(
                        cyl(r * 0.1, r * 0.12, h * 0.85, 5),
                        p.wood,
                        at(sx * h * 0.5, h * 0.9, sz * h * 0.28, Quat::from_rotation_z(-sx * 0.12)),
                    ));
                }
            }
            // Pitched plank roof in two slabs + ridge beam.
            for sz in [-1.0f32, 1.0] {
                parts.push(part(
                    boxm(h * 1.45, 0.1, h * 0.55),
                    p.wood,
                    at(0.0, h * 1.18, sz * h * 0.2, Quat::from_rotation_x(sz * 0.5)),
                ));
            }
            parts.push(part(boxm(h * 1.5, 0.08, 0.1), p.wood_dark, xyz(0.0, h * 1.32, 0.0)));
            // The ram beam slung under the roof, with iron rings + sling ropes.
            parts.push(part(
                cyl(r * 0.28, r * 0.3, h * 1.5, 8),
                p.wood,
                at(0.0, h * 0.78, 0.0, Quat::from_rotation_z(FRAC_PI_2)),
            ));
            for sx in [-1.0f32, 1.0] {
                parts.push(part(
                    torus(r * 0.32, 0.03, 5, 10, PI * 2.0),
                    p.iron,
                    at(sx * h * 0.3, h * 0.78, 0.0, Quat::from_rotation_z(FRAC_PI_2)),
                ));
                parts.push(part(cyl(0.02, 0.02, h * 0.36, 4), p.rope, xyz(sx * h * 0.3, h * 0.98, 0.0)));
            }
            // Iron ram head pointing out the front (+X) + steel collar.
            parts.push(part(
                cone(r * 0.42, r * 0.8, 8),
                p.iron,
                at(h * 0.82, h * 0.78, 0.0, Quat::from_rotation_z(-FRAC_PI_2)),
            ));
            parts.push(part(
                cyl(r * 0.32, r * 0.32, r * 0.2, 8),
                p.steel,
                at(h * 0.7, h * 0.78, 0.0, Quat::from_rotation_z(FRAC_PI_2)),
            ));
            // Four wheels with iron rims.
            for sx in [-1.0f32, 1.0] {
                for sz in [-1.0f32, 1.0] {
                    parts.push(part(
                        cyl(r * 0.42, r * 0.42, 0.12, 10),
                        p.wood,
                        at(sx * h * 0.45, r * 0.42, sz * h * 0.32, Quat::from_rotation_x(FRAC_PI_2)),
                    ));
                    parts.push(part(
                        torus(r * 0.42, 0.03, 5, 10, PI * 2.0),
                        p.iron,
                        at(sx * h * 0.45, r * 0.42, sz * h * 0.32, Quat::from_rotation_x(FRAC_PI_2)),
                    ));
                }
            }
        }
        UnitKind::Mangonel => {
            // Wheeled catapult: arm cocked back, sling loaded, counterweight box.
            parts.push(part(boxm(h * 0.95, 0.18, h * 1.15), p.wood_dark, xyz(0.0, r * 0.55, 0.0)));
            // Side rails.
            for sx in [-1.0f32, 1.0] {
                parts.push(part(boxm(0.1, 0.12, h * 1.1), p.wood, xyz(sx * h * 0.42, r * 0.7, 0.0)));
            }
            // A-frame the arm pivots on.
            for sx in [-1.0f32, 1.0] {
                parts.push(part(
                    boxm(0.08, h * 0.85, 0.08),
                    p.wood,
                    at(sx * r * 0.55, h * 0.62, h * 0.18, Quat::from_rotation_x(0.3)),
                ));
                parts.push(part(
                    boxm(0.08, h * 0.85, 0.08),
                    p.wood,
                    at(sx * r * 0.55, h * 0.62, -h * 0.18, Quat::from_rotation_x(-0.3)),
                ));
            }
            // Pivot axle.
            parts.push(part(
                cyl(0.05, 0.05, h * 0.9, 6),
                p.iron,
                at(0.0, h * 0.95, 0.0, Quat::from_rotation_z(FRAC_PI_2)),
            ));
            // Throwing arm, cocked back over the rear.
            parts.push(part(
                cyl(0.05, 0.06, h * 1.5, 6),
                p.wood,
                at(0.0, h * 0.78, -h * 0.18, Quat::from_rotation_x(-0.85)),
            ));
            // Counterweight box + lid at the short (rear, low) end.
            parts.push(part(boxm(r * 0.7, r * 0.7, r * 0.7), p.iron, xyz(0.0, h * 0.35, -h * 0.55)));
            parts.push(part(boxm(r * 0.74, 0.06, r * 0.74), p.wood_dark, xyz(0.0, h * 0.7, -h * 0.5)));
            // Sling bucket loaded with a stone at the long (front, high) end.
            parts.push(part(sphere(r * 0.4, 8, 6), p.leather, xyz(0.0, h * 1.18, h * 0.55)));
            parts.push(part(sphere(r * 0.3, 7, 6), p.stone, xyz(0.0, h * 1.22, h * 0.55)));
            // Faction banner on a pole at the rear.
            parts.push(part(cyl(0.02, 0.02, h * 0.9, 4), p.wood, xyz(-h * 0.4, h * 1.0, -h * 0.5)));
            parts.push(part(boxm(0.02, r * 0.5, r * 0.6), TINT, xyz(-h * 0.4, h * 1.25, -h * 0.65)));
            // Four wheels with iron rims.
            for sx in [-1.0f32, 1.0] {
                for sz in [-1.0f32, 1.0] {
                    parts.push(part(
                        cyl(r * 0.4, r * 0.4, 0.1, 10),
                        p.wood,
                        at(sx * h * 0.42, r * 0.4, sz * h * 0.42, Quat::from_rotation_x(FRAC_PI_2)),
                    ));
                    parts.push(part(
                        torus(r * 0.4, 0.028, 5, 10, PI * 2.0),
                        p.iron,
                        at(sx * h * 0.42, r * 0.4, sz * h * 0.42, Quat::from_rotation_x(FRAC_PI_2)),
                    ));
                }
            }
        }
        _ => {}
    }

    merge(parts)
}

/// Low-poly far-zoom impostor: gross shape (foot vs mounted vs siege), team
/// tint on the torso/body, rough height — a fraction of the triangles.
pub fn unit_impostor_mesh(kind: UnitKind) -> Mesh {
    let d = unit_def(kind);
    let h = d.height.to_num::<f32>();
    let r = d.radius.to_num::<f32>();
    let skin = srgb(0xd9a878);
    let hide = srgb(0x5a4632);
    let wood = srgb(0x6b4a2b);
    let mut parts: Vec<Mesh> = Vec::new();

    let mounted = is_mounted(kind);
    if matches!(kind, UnitKind::Ram | UnitKind::Mangonel) {
        // A single tinted block roughly the size of the engine, plus a wood base.
        parts.push(part(boxm(h * 1.1, h * 0.5, h * 0.7), wood, xyz(0.0, r * 0.5, 0.0)));
        parts.push(part(boxm(h * 1.0, h * 0.5, h * 0.6), TINT, xyz(0.0, h * 0.9, 0.0)));
        return merge(parts);
    }

    let base_y = if mounted { h * 0.55 } else { 0.0 };
    if mounted {
        // Coarse horse body block + neck/head block + four stubby legs.
        parts.push(part(boxm(r * 0.8, h * 0.4, h * 1.1), hide, xyz(0.0, h * 0.32, 0.0)));
        parts.push(part(boxm(r * 0.4, h * 0.4, r * 0.4), hide, xyz(0.0, h * 0.62, h * 0.55)));
        for sx in [-1.0f32, 1.0] {
            for sz in [-1.0f32, 1.0] {
                parts.push(part(
                    boxm(r * 0.18, h * 0.34, r * 0.18),
                    hide,
                    xyz(sx * r * 0.3, h * 0.14, sz * h * 0.4),
                ));
            }
        }
    } else {
        // Two stubby legs.
        for sx in [-1.0f32, 1.0] {
            parts.push(part(boxm(r * 0.5, h * 0.45, r * 0.5), hide, xyz(sx * r * 0.28, h * 0.22, 0.0)));
        }
    }

    // Tinted torso + skin head: the parts a player tracks at a glance.
    parts.push(part(cyl(r * 0.7, r * 0.92, h * 0.62, 5), TINT, xyz(0.0, base_y + h * 0.72, 0.0)));
    parts.push(part(sphere(r * 0.6, 5, 4), skin, xyz(0.0, base_y + h * 1.12, 0.0)));
    merge(parts)
}
