"""Building builders, one per (kind, faction). Same world-scale footprints as
render/models/buildings.rs (1 tile = 1 unit; Blender Z-up, front authored at
-Y -> game +Z after export).

Factions diverge on silhouette + palette: Ayyubid = sandstone/plaster, domes,
pointed arches, crescent finials; Crusader = cool grey stone, square towers,
steep timber/slate gables, banner poles.
"""

import math

from common import (
    PLASTER,
    SLIT_DARK,
    STONE,
    STONE_DARK,
    TEAM_CLOTH,
    THATCH,
    TIMBER,
    TIMBER_DARK,
    Builder,
)

AY = "ayyubid"
CR = "crusader"

# per-faction masonry palettes
WALL_A = 0xC4AD80  # warm sandstone
WALL_A_DK = 0xA08A60
WALL_C = 0x968F84  # cool field stone
WALL_C_DK = 0x756E63
ROOF_C = 0x5A4A38  # crusader timber/slate roof
DOME_A = 0xD8C590


def _stone(f):
    return (WALL_A, WALL_A_DK) if f == AY else (WALL_C, WALL_C_DK)


def _finial(b, f, x, y, z):
    """Crescent on a short post (Ayyubid) / cross (Crusader)."""
    if f == AY:
        b.cyl(0xD6B24A, 0.02, 0.18, (x, y, z + 0.09), seg=5, bevel=0.0)
        b.torus(0xD6B24A, 0.07, 0.018, (x, y, z + 0.26), seg=10, rot=(math.pi / 2, 0, 0))
    else:
        b.box(0xD6B24A, (0.04, 0.04, 0.3), (x, y, z + 0.15), bevel=0.0)
        b.box(0xD6B24A, (0.16, 0.04, 0.04), (x, y, z + 0.22), bevel=0.0)


def keep(f):
    b = Builder(f"keep_{f}")
    stone, stone_dk = _stone(f)
    s = 3.2
    wall_h = 1.3
    wall_t = 0.34
    half = s / 2 - wall_t / 2

    b.box(stone_dk, (s + 0.5, s + 0.5, 0.22), (0, 0, 0.11), bevel=0.04)
    b.box(stone_dk, (s + 0.16, s + 0.16, 0.26), (0, 0, 0.35), bevel=0.04)

    top = 0.48 + wall_h
    for axis, sign in ((1, 1), (1, -1), (0, 1), (0, -1)):
        yaw = 0.0 if axis == 1 else math.pi / 2
        loc = (0, sign * half, 0.48 + wall_h / 2) if axis == 1 else (sign * half, 0, 0.48 + wall_h / 2)
        b.box(stone, (s - 0.3, wall_t, wall_h), loc, rot=(0, 0, yaw), bevel=0.03)
        cap = (0, sign * half, top + 0.04) if axis == 1 else (sign * half, 0, top + 0.04)
        b.box(stone_dk, (s - 0.2, wall_t + 0.12, 0.08), cap, rot=(0, 0, yaw))
        n = 6
        for i in range(n):
            off = (i / (n - 1) - 0.5) * (s - 0.8)
            mpos = (off, sign * half, top + 0.08) if axis == 1 else (sign * half, off, top + 0.08)
            if f == AY:
                # stepped Islamic merlon: slab + smaller cap
                b.box(stone, (0.26, wall_t, 0.18), (mpos[0], mpos[1], mpos[2] + 0.09), rot=(0, 0, yaw))
                b.box(stone, (0.14, wall_t, 0.12), (mpos[0], mpos[1], mpos[2] + 0.24), rot=(0, 0, yaw))
            else:
                b.box(stone, (0.26, wall_t, 0.3), (mpos[0], mpos[1], mpos[2] + 0.15), rot=(0, 0, yaw))
        slit = (0, sign * (half + 0.02), 0.48 + wall_h * 0.55) if axis == 1 else (
            sign * (half + 0.02), 0, 0.48 + wall_h * 0.55)
        b.arrow_slit(slit[0], slit[1], slit[2], yaw=yaw + math.pi / 2)

    tower_h = 2.3
    tr = 0.5
    for sx in (-1, 1):
        for sy in (-1, 1):
            tx = sx * (s / 2 - 0.05)
            ty = sy * (s / 2 - 0.05)
            t_top = 0.5 + tower_h
            if f == AY:
                # round tapered drums with onion-ish domes
                b.cyl(stone, tr * 1.18, 0.5, (tx, ty, 0.25), seg=10, r_top=tr * 1.08, bevel=0.0)
                b.cyl(stone, tr * 1.08, tower_h, (tx, ty, 0.5 + tower_h / 2), seg=10, r_top=tr, bevel=0.0)
                b.cyl(stone_dk, tr * 1.04, 0.1, (tx, ty, 0.5 + tower_h * 0.55), seg=10, bevel=0.0)
                b.cyl(stone_dk, tr * 1.3, 0.18, (tx, ty, t_top + 0.13), seg=10, r_top=tr * 1.22, bevel=0.0)
                b.merlon_ring(stone, tx, ty, t_top + 0.22, tr * 1.2, count=7, size=(0.16, 0.12, 0.22))
                b.sphere(TEAM_CLOTH, tr * 1.05, (tx, ty, t_top + 0.42), seg=12, hemi=True)
                b.sphere(stone_dk, 0.06, (tx, ty, t_top + 1.42 - 0.32), seg=8)
            else:
                # square crusader turrets with steep cone caps
                b.box(stone, (tr * 2.2, tr * 2.2, 0.5), (tx, ty, 0.25), bevel=0.03)
                b.box(stone, (tr * 2.0, tr * 2.0, tower_h), (tx, ty, 0.5 + tower_h / 2), bevel=0.03)
                b.box(stone_dk, (tr * 2.3, tr * 2.3, 0.16), (tx, ty, t_top + 0.08), bevel=0.02)
                b.merlon_ring(stone, tx, ty, t_top + 0.16, tr * 1.1, count=8, size=(0.16, 0.12, 0.22))
                b.cone(TEAM_CLOTH, tr * 1.35, 1.0, (tx, ty, t_top + 0.66), seg=4, rot=(0, 0, math.pi / 4))
            b.arrow_slit(tx, ty - tr * 1.06, 0.5 + tower_h * 0.45, yaw=0.0)

    kz = 0.48
    b.box(stone, (1.7, 1.7, 1.7), (0, 0, kz + 0.85), bevel=0.04)
    b.box(stone_dk, (1.78, 1.78, 0.1), (0, 0, kz + 1.72), bevel=0.0)
    b.box(stone, (1.45, 1.45, 1.25), (0, 0, kz + 1.77 + 0.62), bevel=0.04)
    for ax, ay in ((1, 0), (-1, 0), (0, 1), (0, -1)):
        b.box(stone_dk, (0.18, 0.18, 1.65), (ax * 0.85, ay * 0.85, kz + 0.82), bevel=0.0)
    crown = kz + 1.77 + 1.25
    if f == AY:
        # great dome over the donjon
        b.sphere(DOME_A, 0.78, (0, 0, crown + 0.05), seg=16, hemi=True)
        _finial(b, f, 0, 0, crown + 0.82)
    else:
        b.box(stone_dk, (1.6, 1.6, 0.1), (0, 0, crown + 0.05), bevel=0.0)
        n = 4
        for i in range(n):
            off = (i / (n - 1) - 0.5) * 1.3
            for sgn in (-1, 1):
                b.box(stone, (0.24, 0.2, 0.3), (off, sgn * 0.72, crown + 0.23))
                b.box(stone, (0.2, 0.24, 0.3), (sgn * 0.72, off, crown + 0.23))
        b.cyl(TIMBER_DARK, 0.045, 1.1, (0, 0, crown + 0.6), seg=5, bevel=0.0)
        b.pennant(0, 0, crown + 0.55, h=0.9)
    for yaw in (0.0, math.pi / 2):
        for sgn in (-1, 1):
            x = 0 if yaw == 0.0 else sgn * 0.74
            y = sgn * 0.74 if yaw == 0.0 else 0
            b.arrow_slit(x, y, kz + 2.6, yaw=yaw + math.pi / 2)

    b.arch_door(0, -half, 0.48, w=0.7, h=0.95, yaw=0.0, frame=stone_dk, depth=wall_t + 0.1)
    return b.join(subdivide=1)


def house(f):
    b = Builder(f"house_{f}")
    if f == AY:
        b.box(PLASTER, (1.6, 1.7, 1.05), (-0.1, 0, 0.525), bevel=0.03)
        b.box(0xBFA377, (1.68, 1.78, 0.12), (-0.1, 0, 1.11), bevel=0.02)
        for w, d, x, y in ((1.68, 0.1, -0.1, 0.86), (1.68, 0.1, -0.1, -0.86), (0.1, 1.78, 0.71, 0), (0.1, 1.78, -0.91, 0)):
            b.box(0xBFA377, (w, d, 0.2), (x, y, 1.27), bevel=0.0)
        b.box(0xBFA377, (0.7, 0.7, 0.7), (0.95, 0.4, 0.35), bevel=0.03)
        b.sphere(DOME_A, 0.39, (0.95, 0.4, 0.7), seg=14, hemi=True)
        for i in range(5):
            b.box(0xBFA377, (0.3, 0.24, 0.16), (-0.95, -0.55 + i * 0.24, 0.08 + i * 0.2), bevel=0.0)
        b.arch_door(-0.1, -0.85, 0.0, w=0.42, h=0.62, yaw=0.0, frame=TIMBER_DARK, depth=0.12)
        b.box(TIMBER_DARK, (0.6, 0.34, 0.05), (-0.1, -1.0, 0.78), rot=(0.32, 0, 0), bevel=0.0)
        b.box(TIMBER_DARK, (0.3, 0.06, 0.3), (-0.55, -0.86, 0.7), bevel=0.0)
        b.box(STONE_DARK, (0.4, 0.07, 0.07), (-0.55, -0.86, 0.9), bevel=0.0)
        b.cyl(0x9A6A3A, 0.12, 0.26, (0.25, 0.45, 1.3), seg=8, r_top=0.07, bevel=0.0)
        b.cyl(0x9A6A3A, 0.1, 0.22, (0.02, 0.55, 1.28), seg=8, r_top=0.06, bevel=0.0)
        b.pennant(0.5, 0.5, 1.32, h=0.6)
    else:
        # timber-framed cottage with a steep shingle gable
        wall = 0xC9B894
        b.box(wall, (1.5, 1.7, 0.95), (0, 0, 0.475), bevel=0.03)
        for sx in (-1, 1):
            for sy in (-1, 1):
                b.box(TIMBER_DARK, (0.12, 0.12, 0.98), (sx * 0.7, sy * 0.8, 0.49), bevel=0.0)
        for sy in (-1, 1):
            b.box(TIMBER_DARK, (1.5, 0.08, 0.1), (0, sy * 0.83, 0.62), bevel=0.0)
            b.box(TIMBER_DARK, (0.66, 0.08, 0.09), (-0.35, sy * 0.84, 0.3), rot=(0, 0.5, 0), bevel=0.0)
        b.prism(ROOF_C, 1.7, 2.0, 0.75, (0, 0, 0.93), bevel=0.03)
        b.box(TIMBER_DARK, (1.76, 0.14, 0.1), (0, 0, 1.7), bevel=0.0)
        # stone chimney out the roof side
        b.box(WALL_C_DK, (0.3, 0.3, 1.0), (0.5, 0.45, 1.3), bevel=0.02)
        b.arch_door(0, -0.85, 0.0, w=0.4, h=0.6, yaw=0.0, frame=TIMBER_DARK, depth=0.12)
        b.box(TIMBER_DARK, (0.3, 0.06, 0.3), (-0.45, -0.86, 0.55), bevel=0.0)
        b.pennant(-0.6, 0.6, 1.4, h=0.6)
    return b.join(subdivide=1)


def granary(f):
    b = Builder(f"granary_{f}")
    if f == AY:
        b.cyl(0xB89A64, 1.1, 0.4, (0, 0, 0.2), seg=14, r_top=1.0, bevel=0.0)
        b.cyl(0xCDB07A, 0.95, 1.25, (0, 0, 1.0), seg=14, r_top=0.82, bevel=0.0)
        for i in range(6):
            a = i / 6 * math.tau + 0.3
            b.box(0xB89A64, (0.14, 0.2, 1.1), (math.cos(a) * 0.92, math.sin(a) * 0.92, 0.75), rot=(0, 0.12, a), bevel=0.0)
        b.cyl(TEAM_CLOTH, 0.88, 0.16, (0, 0, 1.68), seg=14, bevel=0.0)
        b.sphere(DOME_A, 0.8, (0, 0, 1.72), seg=16, hemi=True)
        _finial(b, f, 0, 0, 2.5)
        for i in range(5):
            a = i / 5 * math.tau
            b.box(SLIT_DARK, (0.1, 0.06, 0.14), (math.cos(a) * 0.84, math.sin(a) * 0.84, 1.45), rot=(0, 0, a + math.pi / 2), bevel=0.0)
        b.arch_door(0, -0.93, 0.2, w=0.4, h=0.55, yaw=0.0, frame=0xB89A64, depth=0.16)
        b.box(TIMBER_DARK, (0.5, 0.8, 0.06), (0, -1.25, 0.18), rot=(0.32, 0, 0), bevel=0.0)
        b.cyl(0xD9B35A, 0.16, 0.3, (-0.55, -1.05, 0.15), seg=8, r_top=0.12, bevel=0.0)
        b.cyl(0xD9B35A, 0.14, 0.26, (-0.78, -0.85, 0.13), seg=8, r_top=0.1, bevel=0.0)
    else:
        # long tithe barn on a stone footing
        b.box(WALL_C_DK, (1.9, 1.5, 0.3), (0, 0, 0.15), bevel=0.02)
        b.box(0xA08A62, (1.8, 1.4, 0.85), (0, 0, 0.72), bevel=0.03)
        for sx in (-1, 1):
            b.box(TIMBER_DARK, (0.12, 1.42, 0.1), (sx * 0.6, 0, 0.72), bevel=0.0)
        b.prism(ROOF_C, 2.05, 1.7, 0.8, (0, 0, 1.13), bevel=0.03)
        b.box(TIMBER_DARK, (2.1, 0.16, 0.1), (0, 0, 1.95), bevel=0.0)
        b.arch_door(0, -0.72, 0.3, w=0.55, h=0.7, yaw=0.0, frame=TIMBER_DARK, depth=0.14)
        # grain sacks + cart wheel against the wall
        b.cyl(0xD9B35A, 0.16, 0.3, (-0.7, -0.85, 0.15), seg=8, r_top=0.12, bevel=0.0)
        b.cyl(0xD9B35A, 0.13, 0.24, (-0.45, -0.95, 0.12), seg=8, r_top=0.1, bevel=0.0)
        b.cyl(TIMBER_DARK, 0.28, 0.06, (0.8, -0.78, 0.3), seg=10, rot=(0, math.pi / 2, 0.3), bevel=0.0)
        b.pennant(0.85, 0.6, 1.55, h=0.65)
    return b.join(subdivide=1)


def barracks(f):
    b = Builder(f"barracks_{f}")
    if f == CR:
        wall = 0xB89A6A
        b.box(wall, (2.0, 2.0, 1.2), (0, 0, 0.6), bevel=0.03)
        for sx in (-1, 1):
            for sy in (-1, 1):
                b.box(TIMBER_DARK, (0.16, 0.16, 1.25), (sx * 0.94, sy * 0.94, 0.62), bevel=0.0)
        for sy in (-1, 1):
            b.box(TIMBER_DARK, (2.0, 0.1, 0.12), (0, sy * 0.97, 0.78), bevel=0.0)
            for sx in (-1, 1):
                b.box(TIMBER_DARK, (0.8, 0.08, 0.1), (sx * 0.45, sy * 0.99, 0.45), rot=(0, sx * 0.5, 0), bevel=0.0)
        b.prism(THATCH, 2.3, 2.4, 0.78, (0, 0, 1.18), bevel=0.04)
        b.box(TIMBER_DARK, (2.36, 0.2, 0.14), (0, 0, 1.98), bevel=0.0)
        b.arch_door(0, -1.0, 0.0, w=0.6, h=0.85, yaw=0.0, frame=TIMBER_DARK, depth=0.14)
        b.box(TIMBER_DARK, (0.7, 0.08, 0.5), (0.65, -1.04, 0.55), bevel=0.0)
        for i, x in enumerate((0.5, 0.66, 0.82)):
            b.cyl(TIMBER, 0.022, 0.95, (x, -1.08, 0.48), seg=5, rot=(0.12, (i - 1) * 0.15, 0), bevel=0.0)
        b.cyl(0x8A4A2A, 0.16, 0.05, (-0.6, -1.02, 0.7), seg=10, rot=(math.pi / 2, 0, 0), bevel=0.0)
        b.pennant(0, 0, 2.0, h=0.85)
    else:
        # plastered drill hall: flat roof, corner dome, shaded awning
        stone, stone_dk = _stone(AY)
        b.box(stone, (2.0, 2.0, 1.15), (0, 0, 0.575), bevel=0.03)
        b.box(stone_dk, (2.08, 2.08, 0.12), (0, 0, 1.2), bevel=0.02)
        for w, d, x, y in ((2.08, 0.1, 0, 1.0), (2.08, 0.1, 0, -1.0), (0.1, 2.08, 1.0, 0), (0.1, 2.08, -1.0, 0)):
            b.box(stone_dk, (w, d, 0.18), (x, y, 1.35), bevel=0.0)
        b.sphere(DOME_A, 0.42, (0.6, 0.6, 1.26), seg=14, hemi=True)
        _finial(b, AY, 0.6, 0.6, 1.66)
        # striped awning over the door on timber poles
        b.box(TEAM_CLOTH, (1.0, 0.55, 0.05), (-0.3, -1.18, 0.95), rot=(0.18, 0, 0), bevel=0.0)
        for sx in (-0.75, 0.15):
            b.cyl(TIMBER_DARK, 0.035, 0.9, (sx, -1.4, 0.45), seg=5, bevel=0.0)
        b.arch_door(-0.3, -1.0, 0.0, w=0.6, h=0.8, yaw=0.0, frame=stone_dk, depth=0.14)
        # weapon rack + shields on the wall
        b.box(TIMBER_DARK, (0.6, 0.08, 0.5), (0.7, -1.04, 0.5), bevel=0.0)
        for i, x in enumerate((0.55, 0.7, 0.85)):
            b.cyl(TIMBER, 0.022, 0.9, (x, -1.08, 0.45), seg=5, rot=(0.12, (i - 1) * 0.15, 0), bevel=0.0)
        b.cyl(0xD6B24A, 0.15, 0.04, (-0.85, -1.02, 0.75), seg=10, rot=(math.pi / 2, 0, 0), bevel=0.0)
    return b.join(subdivide=1)


def _tower(f, name, tall):
    b = Builder(name)
    stone, stone_dk = _stone(f)
    body_h = 3.3 if tall else 2.4
    r = 0.52 if tall else 0.5
    top = 0.42 + body_h
    if f == AY:
        b.cyl(stone_dk, r * 1.45, 0.42, (0, 0, 0.21), seg=10, r_top=r * 1.26, bevel=0.0)
        b.cyl(stone, r * 1.2, body_h, (0, 0, 0.42 + body_h / 2), seg=10, r_top=r, bevel=0.0)
        if tall:
            b.cyl(stone_dk, r * 1.1, 0.14, (0, 0, 0.42 + body_h * 0.55), seg=10, bevel=0.0)
    else:
        b.box(stone_dk, (r * 2.6, r * 2.6, 0.42), (0, 0, 0.21), bevel=0.03)
        b.box(stone, (r * 2.15, r * 2.15, body_h), (0, 0, 0.42 + body_h / 2), bevel=0.03)
        if tall:
            b.box(stone_dk, (r * 2.25, r * 2.25, 0.14), (0, 0, 0.42 + body_h * 0.55), bevel=0.0)
    levels = (0.45, 0.72) if tall else (0.55,)
    for ly in levels:
        for i in range(4):
            a = i / 4 * math.tau + (0.4 if f == AY else math.pi / 4)
            z = 0.42 + body_h * ly
            b.arrow_slit(math.cos(a) * r * 1.06, math.sin(a) * r * 1.06, z, yaw=a + math.pi / 2)
    for i in range(8):
        a = i / 8 * math.tau
        b.box(stone_dk, (0.12, 0.16, 0.16), (math.cos(a) * r * 1.12, math.sin(a) * r * 1.12, top - 0.06), rot=(0, 0, a), bevel=0.0)
    if f == AY:
        b.cyl(stone_dk, r * 1.34, 0.26, (0, 0, top + 0.13), seg=10, r_top=r * 1.24, bevel=0.0)
        b.merlon_ring(stone, 0, 0, top + 0.26, r * 1.24, count=8, size=(0.18, 0.14, 0.3))
        cap_z = top + (0.62 if tall else 0.5)
        b.sphere(TEAM_CLOTH, r * 1.1, (0, 0, cap_z - 0.05), seg=12, hemi=True)
        _finial(b, f, 0, 0, cap_z + r * 1.02)
    else:
        b.box(stone_dk, (r * 2.7, r * 2.7, 0.22), (0, 0, top + 0.11), bevel=0.02)
        b.merlon_ring(stone, 0, 0, top + 0.22, r * 1.22, count=8, size=(0.18, 0.14, 0.3))
        cap_z = top + (0.62 if tall else 0.5)
        b.cone(TEAM_CLOTH, r * 1.35, 0.95 if tall else 0.75, (0, 0, cap_z + (0.95 if tall else 0.75) / 2 - 0.12), seg=4, rot=(0, 0, math.pi / 4))
        b.sphere(stone_dk, 0.07, (0, 0, cap_z + (0.95 if tall else 0.75) + 0.02), seg=8)
    b.arch_door(0, -r * 1.16, 0.0, w=0.36, h=0.6, yaw=0.0, frame=stone_dk, depth=0.18)
    if tall:
        b.pennant(r * 1.2, 0, top + 0.4, h=0.85)
    return b.join(subdivide=1)


def tower(f):
    return _tower(f, f"tower_{f}", False)


def watchtower(f):
    return _tower(f, f"watchtower_{f}", True)


def wall(f):
    b = Builder(f"wall_{f}")
    stone, stone_dk = _stone(f)
    s = 0.56
    h = 0.74
    b.box(stone_dk, (s + 0.16, s + 0.16, 0.18), (0, 0, 0.09), bevel=0.03)
    b.box(stone, (s, s, h), (0, 0, h / 2), bevel=0.03)
    b.box(stone_dk, (s + 0.12, s + 0.12, 0.1), (0, 0, h + 0.05), bevel=0.02)
    if f == AY:
        b.box(stone, (0.3, 0.3, 0.16), (0, 0, h + 0.18), bevel=0.02)
        b.box(stone, (0.16, 0.16, 0.12), (0, 0, h + 0.3), bevel=0.0)
    else:
        b.box(stone, (0.3, 0.3, 0.26), (0, 0, h + 0.23), bevel=0.02)
    return b.join(subdivide=1)


def wall_arm(f):
    b = Builder(f"wall_arm_{f}")
    stone, stone_dk = _stone(f)
    h = 0.62
    tk = 0.42
    b.box(stone_dk, (0.56, tk + 0.1, 0.18), (0.25, 0, 0.09), bevel=0.02)
    b.box(stone, (0.56, tk, h), (0.25, 0, h / 2), bevel=0.02)
    b.box(stone_dk, (0.56, tk + 0.08, 0.1), (0.25, 0, h + 0.05), bevel=0.02)
    if f == AY:
        b.box(stone, (0.2, tk, 0.14), (0.37, 0, h + 0.17), bevel=0.02)
        b.box(stone, (0.1, tk, 0.1), (0.37, 0, h + 0.29), bevel=0.0)
    else:
        b.box(stone, (0.2, tk, 0.24), (0.37, 0, h + 0.22), bevel=0.02)
    b.arrow_slit(0.3, tk / 2 + 0.02, h * 0.6, yaw=math.pi / 2)
    return b.join(subdivide=1)


def gatehouse(f):
    b = Builder(f"gatehouse_{f}")
    stone, stone_dk = _stone(f)
    for sx in (-1, 1):
        b.box(stone, (0.32, 0.62, 1.45), (sx * 0.38, 0, 0.725), bevel=0.03)
        b.box(stone_dk, (0.4, 0.7, 0.1), (sx * 0.38, 0, 1.5), bevel=0.0)
        b.merlon_row(stone, (sx * 0.38 - 0.18, -0.26), (sx * 0.38 + 0.18, -0.26), 1.55, 2, size=(0.16, 0.14, 0.22))
        b.merlon_row(stone, (sx * 0.38 - 0.18, 0.26), (sx * 0.38 + 0.18, 0.26), 1.55, 2, size=(0.16, 0.14, 0.22))
        b.arrow_slit(sx * 0.38, -0.33, 1.05, yaw=0.0)
    b.box(SLIT_DARK, (0.5, 0.64, 1.0), (0, 0, 0.5), bevel=0.0)
    b.box(stone_dk, (0.8, 0.6, 0.28), (0, 0, 1.3), bevel=0.03)
    b.cyl(stone_dk, 0.33, 0.6, (0, 0, 1.12), seg=12, rot=(math.pi / 2, 0, 0), bevel=0.0)
    for x in (-0.16, 0.0, 0.16):
        b.box(TIMBER_DARK, (0.05, 0.04, 0.9), (x, -0.3, 0.55), bevel=0.0)
    b.box(stone_dk, (1.06, 0.62, 0.14), (0, 0, 1.62), bevel=0.02)
    b.merlon_row(stone, (-0.45, -0.28), (0.45, -0.28), 1.69, 4, size=(0.18, 0.14, 0.2))
    b.merlon_row(stone, (-0.45, 0.28), (0.45, 0.28), 1.69, 4, size=(0.18, 0.14, 0.2))
    b.box(TEAM_CLOTH, (0.4, 0.03, 0.36), (0, -0.32, 1.16), bevel=0.0)
    _finial(b, f, 0, 0.2, 1.76)
    return b.join(subdivide=1)


def stable(f):
    b = Builder(f"stable_{f}")
    wall = 0x9A7A4A if f == CR else WALL_A
    roof = 0x6B4A2B if f == CR else WALL_A_DK
    b.box(wall, (2.0, 0.18, 0.95), (0, 0.78, 0.55), bevel=0.02)
    for x in (-1.0, 0.0, 1.0):
        b.box(wall, (0.16, 1.4, 0.85), (x, 0.1, 0.5), bevel=0.02)
    b.box(roof, (2.2, 1.9, 0.12), (0, 0.05, 1.22), rot=(-0.22, 0, 0), bevel=0.03)
    b.box(TIMBER_DARK, (2.24, 0.14, 0.12), (0, 0.95, 1.42), bevel=0.0)
    b.box(0xC9A64A, (0.55, 0.4, 0.36), (-0.7, -0.62, 0.18), bevel=0.03)
    b.cone(0xC9A64A, 0.32, 0.34, (0.85, -0.55, 0.17), seg=7)
    b.box(TIMBER_DARK, (0.6, 0.24, 0.18), (0.0, -0.55, 0.09), bevel=0.0)
    b.box(0x5B4632, (0.3, 0.72, 0.34), (0.5, 0.1, 0.62), bevel=0.04)
    b.box(0x5B4632, (0.16, 0.18, 0.32), (0.5, -0.3, 0.85), rot=(0.35, 0, 0), bevel=0.02)
    b.box(0x5B4632, (0.15, 0.3, 0.16), (0.5, -0.46, 1.0), bevel=0.02)
    for z in (0.32, 0.52):
        b.box(TIMBER_DARK, (1.9, 0.06, 0.06), (0, -0.9, z), bevel=0.0)
    for x in (-0.92, 0.0, 0.92):
        b.box(TIMBER_DARK, (0.08, 0.08, 0.6), (x, -0.9, 0.3), bevel=0.0)
    b.pennant(0.92, 0.7, 1.5, h=0.7)
    return b.join(subdivide=1)


def blacksmith(f):
    b = Builder(f"blacksmith_{f}")
    stone, stone_dk = _stone(f)
    b.box(stone, (1.9, 1.9, 1.05), (0, 0, 0.525), bevel=0.03)
    for sx in (-1, 1):
        for sy in (-1, 1):
            b.box(stone_dk, (0.2, 0.2, 1.08), (sx * 0.85, sy * 0.85, 0.54), bevel=0.0)
    if f == AY:
        b.box(stone_dk, (2.0, 2.0, 0.12), (0, 0, 1.14), bevel=0.02)
        b.sphere(DOME_A, 0.5, (-0.4, 0.3, 1.18), seg=14, hemi=True)
    else:
        b.prism(0x3A3A3A, 2.1, 2.1, 0.62, (0, 0, 1.04), bevel=0.03)
    b.box(stone_dk, (0.4, 0.4, 1.3), (0.62, 0.55, 1.6), bevel=0.02)
    b.box(stone_dk, (0.52, 0.52, 0.14), (0.62, 0.55, 2.3), bevel=0.0)
    b.box(0xD9531E, (0.26, 0.26, 0.1), (0.62, 0.55, 2.38), bevel=0.0)
    b.box(0xD9531E, (0.5, 0.2, 0.42), (-0.3, -0.97, 0.42), bevel=0.0)
    b.cyl(TIMBER_DARK, 0.17, 0.34, (0.55, -0.9, 0.17), seg=8, r_top=0.15, bevel=0.0)
    b.box(0x3C3C40, (0.34, 0.18, 0.13), (0.55, -0.9, 0.4), bevel=0.02)
    b.cone(0x3C3C40, 0.07, 0.2, (0.78, -0.9, 0.42), seg=5, rot=(0, math.pi / 2, 0))
    b.cyl(0x4A4A50, 0.16, 0.3, (0.1, -0.95, 0.15), seg=9, bevel=0.0)
    b.pennant(-0.78, 0.55, 1.45, h=0.7)
    return b.join(subdivide=1)


def market(f):
    b = Builder(f"market_{f}")
    b.box(TIMBER, (1.9, 0.7, 0.55), (0, 0.45, 0.28), bevel=0.03)
    for sx in (-0.88, 0.88):
        for sy in (-0.72, 0.72):
            b.cyl(TIMBER, 0.05, 1.5, (sx, sy, 0.75), seg=6, bevel=0.0)
    ang = 0.42
    accent = 0xE7D39A if f == AY else 0x8A4A2A
    b.box(TEAM_CLOTH, (2.05, 1.0, 0.06), (0, -0.46, 1.42), rot=(ang, 0, 0), bevel=0.0)
    b.box(accent, (2.05, 1.0, 0.06), (0, 0.46, 1.42), rot=(-ang, 0, 0), bevel=0.0)
    b.box(TIMBER, (2.1, 0.1, 0.08), (0, 0, 1.62), bevel=0.0)
    for sx in (-0.62, 0.62):
        b.box(TIMBER_DARK, (0.36, 0.36, 0.36), (sx, -0.5, 0.18), bevel=0.02)
    b.cone(0xC0532F, 0.2, 0.24, (0.0, 0.4, 0.68), seg=8)
    b.cone(0x7A9A3A, 0.16, 0.2, (-0.45, 0.42, 0.66), seg=8)
    b.cyl(0xD9B35A, 0.16, 0.32, (-0.45, -0.62, 0.16), seg=8, r_top=0.12, bevel=0.0)
    b.cyl(0x9A6A3A, 0.15, 0.34, (0.45, 0.4, 0.73), seg=8, r_top=0.1, bevel=0.0)
    b.cyl(0x8A3A2A, 0.09, 0.7, (0.78, -0.35, 0.1), seg=8, rot=(0, math.pi / 2, 0.4), bevel=0.0)
    b.pennant(0.92, -0.72, 1.55, h=0.7)
    return b.join(subdivide=1)


def fishing_hut(f):
    b = Builder(f"fishing_hut_{f}")
    wood = 0x9A7A52 if f == CR else 0xAE8C5E
    roof = 0x6A4A2A if f == CR else WALL_A_DK
    for sx in (-0.32, 0.32):
        for sy in (-0.32, 0.32):
            b.cyl(TIMBER_DARK, 0.055, 0.6, (sx, sy, 0.3), seg=6, bevel=0.0)
    b.box(wood, (1.05, 1.05, 0.08), (0, 0, 0.62), bevel=0.02)
    b.box(wood, (0.85, 0.85, 0.55), (0, 0.05, 0.95), bevel=0.03)
    b.box(roof, (1.05, 1.1, 0.1), (0, 0.05, 1.3), rot=(0.26, 0, 0), bevel=0.03)
    b.box(wood, (0.5, 1.1, 0.07), (0, -0.95, 0.52), bevel=0.02)
    for dy in (-0.7, -1.25):
        b.cyl(TIMBER_DARK, 0.045, 0.52, (0.18, dy, 0.26), seg=6, bevel=0.0)
    b.cyl(wood, 0.035, 1.1, (0.72, 0.1, 0.95), seg=6, bevel=0.0)
    b.box(0xB8B8A0, (0.02, 0.6, 0.55), (0.78, -0.1, 0.85), rot=(0, 0, -0.4), bevel=0.0)
    b.box(TIMBER_DARK, (0.04, 0.5, 0.04), (-0.5, -0.3, 1.3), rot=(0, 0, 0.3), bevel=0.0)
    for i in range(3):
        b.box(0xA8B8C0, (0.05, 0.08, 0.16), (-0.5, -0.42 + i * 0.16, 1.18), bevel=0.0)
    b.sphere(0x6A4A2A, 0.08, (-0.3, 0.3, 0.72), seg=8)
    b.cyl(0xC9A64A, 0.12, 0.16, (0.25, 0.35, 0.74), seg=8, r_top=0.15, bevel=0.0)
    b.pennant(-0.45, 0.35, 1.4, h=0.65)
    return b.join(subdivide=1)


def siege_workshop(f):
    b = Builder(f"siege_workshop_{f}")
    wood = 0x7A5A32
    wood_light = 0xA07C46
    for sx in (-0.92, 0.92):
        for sy in (-0.7, 0.7):
            b.box(wood, (0.16, 0.16, 1.3), (sx, sy, 0.65), bevel=0.0)
    b.box(wood, (2.0, 0.16, 1.1), (0, 0.78, 0.6), bevel=0.02)
    b.box(wood_light, (2.0, 0.1, 0.1), (0, 0.72, 0.9), rot=(0, 0.18, 0), bevel=0.0)
    roof = TIMBER_DARK if f == CR else WALL_A_DK
    b.prism(roof, 2.2, 1.9, 0.55, (0, 0.05, 1.32), bevel=0.03)
    for i in range(3):
        b.cyl(wood_light, 0.07, 1.0, (-0.7, -0.35 + (i % 2) * 0.07, 0.12 + i * 0.14), seg=7, rot=(0, math.pi / 2, 0), bevel=0.0)
    b.box(wood_light, (0.08, 0.5, 0.08), (0.1, -0.2, 0.3), rot=(0.5, 0, 0), bevel=0.0)
    b.box(wood, (0.9, 0.5, 0.16), (0.15, -0.78, 0.34), bevel=0.02)
    for wy in (-0.55, -1.0):
        b.cyl(0x8A8A8A, 0.22, 0.1, (0.15, wy, 0.22), seg=10, rot=(0, math.pi / 2, 0), bevel=0.0)
    b.box(wood_light, (0.08, 0.08, 0.8), (0.15, -0.78, 0.72), rot=(-0.7, 0, 0), bevel=0.0)
    b.box(0x8A8A8A, (0.16, 0.16, 0.1), (0.15, -0.45, 1.0), bevel=0.0)
    b.pennant(-0.92, 0.7, 1.6, h=0.7)
    return b.join(subdivide=1)


def _variants(fn):
    return {f"{fn.__name__}_{f}": (lambda f=f, fn=fn: fn(f)) for f in (AY, CR)}


BUILDERS = {}
for _fn in (keep, barracks, tower, wall, wall_arm, gatehouse, house, stable, blacksmith, market, granary, fishing_hut, siege_workshop, watchtower):
    BUILDERS.update(_variants(_fn))
