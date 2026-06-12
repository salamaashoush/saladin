"""Resource-node + prop builders. Proportions match render/models/props.rs
(animals ~0.7 long facing -Y -> game +Z, trees ~1.6-2.4 tall, rocks ~0.9 wide).
"""

import math

from common import Builder

TRUNK = 0x6B4A2B
TRUNK_DARK = 0x553A20


# ── trees ────────────────────────────────────────────────────────────────────


def tree_broadleaf():
    b = Builder("tree_broadleaf")
    b.cyl(TRUNK, 0.16, 0.8, (0, 0, 0.4), seg=6, r_top=0.09, bevel=0.0)
    b.cyl(TRUNK, 0.05, 0.4, (0.12, 0.05, 0.75), seg=5, r_top=0.03, rot=(0.5, 0, 0), bevel=0.0)
    b.blob(0x4A7A33, 0.54, (0, 0, 1.2))
    b.blob(0x3D6628, 0.38, (0.34, 0.16, 0.95))
    b.blob(0x3D6628, 0.33, (-0.3, -0.18, 0.98))
    b.blob(0x5D9440, 0.3, (0.06, 0.05, 1.58))
    return b.join(subdivide=0)


def tree_broadleaf_tall():
    b = Builder("tree_broadleaf_tall")
    b.cyl(TRUNK_DARK, 0.14, 1.05, (0, 0, 0.52), seg=6, r_top=0.08, bevel=0.0)
    b.blob(0x456F2F, 0.44, (0, 0, 1.32))
    b.blob(0x456F2F, 0.35, (0.22, -0.14, 1.06))
    b.blob(0x558B3A, 0.3, (-0.05, 0.08, 1.7))
    return b.join(subdivide=0)


def tree_conifer():
    b = Builder("tree_conifer")
    b.cyl(TRUNK_DARK, 0.13, 0.6, (0, 0, 0.3), seg=6, r_top=0.07, bevel=0.0)
    for r, h, z, c in ((0.54, 0.8, 0.8, 0x2F5D2A), (0.42, 0.7, 1.22, 0x356B30), (0.28, 0.6, 1.62, 0x3C7A36)):
        b.cone(c, r, h, (0, 0, z + h / 2 - 0.3), seg=8)
    return b.join(subdivide=0)


def tree_olive():
    b = Builder("tree_olive")
    b.cyl(TRUNK, 0.13, 0.55, (0.05, 0, 0.28), seg=5, r_top=0.07, rot=(0, 0.18, 0), bevel=0.0)
    b.cyl(TRUNK, 0.09, 0.45, (-0.12, 0.05, 0.32), seg=5, r_top=0.06, rot=(0, -0.35, 0), bevel=0.0)
    b.blob(0x5D7340, 0.46, (0, 0, 0.8), scale=(1, 1, 0.62))
    b.blob(0x4C6133, 0.32, (0.32, 0.12, 0.7), scale=(1, 1, 0.6))
    b.blob(0x4C6133, 0.27, (-0.3, -0.12, 0.72), scale=(1, 1, 0.65))
    return b.join(subdivide=0)


def tree_palm():
    b = Builder("tree_palm")
    b.cyl(0x7A5A32, 0.12, 2.0, (0, 0, 1.0), seg=6, r_top=0.07, rot=(0, 0.06, 0), bevel=0.0)
    # ring collars up the trunk for the fibrous look
    for z in (0.5, 0.95, 1.4):
        b.cyl(0x6B4A28, 0.115, 0.07, (z * 0.06, 0, z), seg=6, bevel=0.0)
    n = 8
    for i in range(n):
        a = i / n * math.tau
        c = 0x3F8F49 if i % 2 == 0 else 0x357A3E
        # frond: flat cone blade, tilted down (rot X) then yawed outward
        # (rot Z) — local Y is the blade thickness after that order
        dx, dy = math.cos(a), math.sin(a)
        droop = 1.7 + (i % 2) * 0.25
        b.cone(
            c,
            0.22,
            1.2,
            (0.12 + dx * 0.52, dy * 0.52, 1.98 - (i % 2) * 0.08),
            seg=4,
            rot=(droop, 0, a + math.pi / 2),
            scale=(1.0, 0.28, 1.0),
        )
    b.blob(0x3F8F49, 0.16, (0.12, 0, 2.04), noise=0.15)
    b.sphere(0xB07A2A, 0.09, (0.26, 0.1, 1.88), seg=8)
    b.sphere(0x9A6A24, 0.07, (-0.02, -0.16, 1.86), seg=8)
    return b.join(subdivide=0)


# ── rocks (AoE4-style low rubble fields; angular slabs, dark crevices) ──────

import random

STONE_LIGHT = 0xB3AEA3
STONE_CREV = 0x57534B
GOLD_ROCK = 0x57504A
GOLD_ROCK_CREV = 0x2E2A26
GOLD_VEIN = 0xF0B428
GOLD_VEIN_HI = 0xFFD75E


def _slab_field(name, seed, color, crev, big=3, mid=3, chips=6, span=0.55):
    """Wide LOW cluster of angular slabs + debris apron — reads as a quarry
    outcrop, not a boulder pile."""
    b = Builder(name)
    rng = random.Random(seed)
    placed = []
    for i in range(big):
        a = rng.random() * 6.2832
        d = rng.random() * span * 0.5
        x, y = math.cos(a) * d, math.sin(a) * d
        r = 0.3 + rng.random() * 0.14
        b.rock(color, r, (x, y, r * 0.22), rng, squash=0.42, dark=crev)
        placed.append((x, y))
    for i in range(mid):
        a = rng.random() * 6.2832
        d = span * (0.5 + rng.random() * 0.6)
        b.rock(color, 0.16 + rng.random() * 0.1, (math.cos(a) * d, math.sin(a) * d, 0.045), rng, squash=0.45, dark=crev)
    for i in range(chips):
        a = rng.random() * 6.2832
        d = span * (0.8 + rng.random() * 0.7)
        b.rock(color, 0.05 + rng.random() * 0.05, (math.cos(a) * d, math.sin(a) * d, 0.02), rng, squash=0.6, dark=crev)
    return b, rng, placed


def stone_a():
    b, _, _ = _slab_field("stone_a", 11, STONE_LIGHT, STONE_CREV)
    return b.join(subdivide=0)


def stone_b():
    b, _, _ = _slab_field("stone_b", 23, STONE_LIGHT, STONE_CREV, big=2, mid=4)
    return b.join(subdivide=0)


def stone_c():
    b, _, _ = _slab_field("stone_c", 37, 0xA8A399, STONE_CREV, big=3, mid=2, chips=8)
    return b.join(subdivide=0)


def _gold_mine(name, seed):
    """Dark angular rock mass with bright gold veins snaking through the
    crack lines across its top (AoM/AoE read: glowing seams, not sprinkles)."""
    b, rng, placed = _slab_field(name, seed, GOLD_ROCK, GOLD_ROCK_CREV, big=3, mid=3, chips=5, span=0.5)
    # veins: jagged polylines of thin bright segments draped over the tops
    for vx, vy in placed:
        x, y = vx, vy
        ang = rng.random() * 6.2832
        z = 0.19 + rng.random() * 0.05
        for s in range(4 + rng.randrange(3)):
            step = 0.09 + rng.random() * 0.06
            ang += (rng.random() - 0.5) * 1.6
            nx, ny = x + math.cos(ang) * step, y + math.sin(ang) * step
            # keep the seam ON the rock mass — pull wandering ends back in
            dd = math.dist((nx, ny), (vx, vy))
            if dd > 0.26:
                nx = vx + (nx - vx) * 0.26 / dd
                ny = vy + (ny - vy) * 0.26 / dd
            mx, my = (x + nx) / 2, (y + ny) / 2
            c = GOLD_VEIN_HI if s % 2 == 0 else GOLD_VEIN
            b.box(c, (step * 1.15, 0.035 + rng.random() * 0.02, 0.03), (mx, my, z), rot=(0, 0, ang), bevel=0.0)
            x, y = nx, ny
            z = max(0.11, z - 0.02)
    # a few chunky nuggets seated in the crevices
    for i in range(4):
        a = rng.random() * 6.2832
        d = 0.2 + rng.random() * 0.3
        b.sphere(GOLD_VEIN, 0.05 + rng.random() * 0.03, (math.cos(a) * d, math.sin(a) * d, 0.07), seg=6)
    return b.join(subdivide=0)


def gold_a():
    return _gold_mine("gold_a", 41)


def gold_b():
    return _gold_mine("gold_b", 53)


# ── food animals (face -Y -> game +Z; sim anchor at origin) ─────────────────

HIDE_DEER = 0x8A6A42
HIDE_DEER_DK = 0x6E5232
BELLY = 0xA8895E
HIDE_BOAR = 0x4F3A28
HIDE_BOAR_DK = 0x3A2B1D


def _deer_body(b, head_z, head_pitch):
    # body: chest deeper than rump
    b.sphere(HIDE_DEER, 0.26, (0, 0.05, 0.42), seg=12, scale=(0.75, 1.5, 0.85))
    b.sphere(BELLY, 0.2, (0, 0.05, 0.34), seg=10, scale=(0.7, 1.2, 0.7))
    # neck + head
    b.cyl(HIDE_DEER, 0.09, 0.34, (0, -0.32, 0.58), seg=7, r_top=0.07, rot=(0.7, 0, 0), bevel=0.0)
    b.sphere(HIDE_DEER, 0.11, (0, -0.44, head_z), seg=10, scale=(0.8, 1.3, 0.9))
    b.cyl(HIDE_DEER_DK, 0.045, 0.12, (0, -0.55, head_z - 0.02), seg=6, r_top=0.035, rot=(math.pi / 2 - head_pitch, 0, 0), bevel=0.0)
    # ears + antlers
    for sx in (-1, 1):
        b.cone(HIDE_DEER_DK, 0.035, 0.1, (sx * 0.07, -0.4, head_z + 0.1), seg=4, rot=(0, sx * 0.5, 0))
        b.cyl(0xD8CCB2, 0.018, 0.22, (sx * 0.06, -0.38, head_z + 0.18), seg=4, r_top=0.012, rot=(-0.4, sx * 0.5, 0), bevel=0.0)
        b.cyl(0xD8CCB2, 0.013, 0.13, (sx * 0.1, -0.36, head_z + 0.24), seg=4, r_top=0.009, rot=(-0.1, sx * 1.1, 0), bevel=0.0)
    # legs
    for sx in (-1, 1):
        for sy in (-1, 1):
            b.cyl(HIDE_DEER_DK, 0.038, 0.42, (sx * 0.12, 0.05 + sy * 0.26, 0.21), seg=5, r_top=0.028, bevel=0.0)
    # tail
    b.sphere(0xE8E0CC, 0.05, (0, 0.43, 0.5), seg=6)


def food_deer():
    b = Builder("food_deer")
    _deer_body(b, 0.78, 0.25)
    return b.join(subdivide=0)


def food_deer_grazing():
    b = Builder("food_deer_grazing")
    # body identical, neck/head swept down to the grass
    b.sphere(HIDE_DEER, 0.26, (0, 0.05, 0.42), seg=12, scale=(0.75, 1.5, 0.85))
    b.sphere(BELLY, 0.2, (0, 0.05, 0.34), seg=10, scale=(0.7, 1.2, 0.7))
    b.cyl(HIDE_DEER, 0.09, 0.4, (0, -0.32, 0.36), seg=7, r_top=0.06, rot=(1.9, 0, 0), bevel=0.0)
    b.sphere(HIDE_DEER, 0.11, (0, -0.5, 0.16), seg=10, scale=(0.8, 1.3, 0.9))
    b.cyl(HIDE_DEER_DK, 0.045, 0.1, (0, -0.56, 0.08), seg=6, r_top=0.03, rot=(2.6, 0, 0), bevel=0.0)
    for sx in (-1, 1):
        b.cone(HIDE_DEER_DK, 0.035, 0.1, (sx * 0.08, -0.48, 0.26), seg=4, rot=(1.2, sx * 0.5, 0))
        b.cyl(0xD8CCB2, 0.018, 0.2, (sx * 0.06, -0.46, 0.3), seg=4, r_top=0.012, rot=(0.6, sx * 0.5, 0), bevel=0.0)
    for sx in (-1, 1):
        for sy in (-1, 1):
            b.cyl(HIDE_DEER_DK, 0.038, 0.42, (sx * 0.12, 0.05 + sy * 0.26, 0.21), seg=5, r_top=0.028, bevel=0.0)
    b.sphere(0xE8E0CC, 0.05, (0, 0.43, 0.5), seg=6)
    return b.join(subdivide=0)


def food_deer_carcass():
    b = Builder("food_deer_carcass")
    # on its side, legs out — same masses laid flat
    b.sphere(HIDE_DEER, 0.26, (0, 0.02, 0.2), seg=12, scale=(1.5, 0.85, 0.75), rot=(0, 0, 0.2))
    b.sphere(HIDE_DEER, 0.11, (-0.5, -0.1, 0.12), seg=10, scale=(1.3, 0.9, 0.8))
    b.cyl(0xD8CCB2, 0.016, 0.2, (-0.55, -0.05, 0.2), seg=4, r_top=0.01, rot=(0, 1.2, 0.4), bevel=0.0)
    for i, dy in enumerate((-0.18, -0.05, 0.1, 0.22)):
        b.cyl(HIDE_DEER_DK, 0.035, 0.4, (0.1 + (i % 2) * 0.08, dy, 0.16), seg=5, r_top=0.026, rot=(0.2 + (i % 2) * 0.25, 1.45, 0), bevel=0.0)
    return b.join(subdivide=0)


def food_boar():
    b = Builder("food_boar")
    b.sphere(HIDE_BOAR, 0.3, (0, 0.04, 0.37), seg=12, scale=(0.95, 1.6, 0.9))
    b.box(HIDE_BOAR_DK, (0.1, 0.6, 0.1), (0, 0.06, 0.62), bevel=0.02)
    b.sphere(HIDE_BOAR, 0.17, (0, -0.42, 0.38), seg=10, scale=(0.9, 1.1, 0.9))
    b.cyl(0x8A6A55, 0.07, 0.12, (0, -0.58, 0.34), seg=7, rot=(math.pi / 2, 0, 0), bevel=0.0)
    for sx in (-1, 1):
        b.cone(HIDE_BOAR_DK, 0.035, 0.08, (sx * 0.1, -0.4, 0.52), seg=4)
        b.cone(0xE8E0CC, 0.018, 0.09, (sx * 0.08, -0.56, 0.3), seg=4, rot=(-0.5, 0, sx * 0.6))
        for sy in (-1, 1):
            b.cyl(HIDE_BOAR_DK, 0.045, 0.3, (sx * 0.14, 0.04 + sy * 0.3, 0.15), seg=5, r_top=0.035, bevel=0.0)
    return b.join(subdivide=0)


def food_boar_carcass():
    b = Builder("food_boar_carcass")
    b.sphere(HIDE_BOAR, 0.3, (0, 0.0, 0.24), seg=12, scale=(1.6, 0.9, 0.85), rot=(0, 0, 0.15))
    b.sphere(HIDE_BOAR, 0.17, (-0.5, -0.06, 0.18), seg=10, scale=(1.1, 0.9, 0.9))
    b.cone(0xE8E0CC, 0.018, 0.09, (-0.6, 0.0, 0.22), seg=4, rot=(0, 1.3, 0.5))
    for i, dy in enumerate((-0.2, -0.06, 0.1, 0.24)):
        b.cyl(HIDE_BOAR_DK, 0.04, 0.28, (0.12, dy, 0.14), seg=5, r_top=0.03, rot=(0.2, 1.5, 0), bevel=0.0)
    return b.join(subdivide=0)


def food_berry():
    b = Builder("food_berry")
    b.blob(0x3E6B2E, 0.42, (0, 0, 0.3), scale=(1.15, 1.05, 0.7), noise=0.35)
    b.blob(0x355927, 0.3, (0.3, 0.2, 0.22), scale=(1, 1, 0.65), noise=0.35)
    for i in range(9):
        a = i * 2.39996  # golden angle scatter
        r = 0.18 + (i % 3) * 0.12
        b.sphere(0xB0303A, 0.045, (math.cos(a) * r, math.sin(a) * r * 0.8, 0.5 + (i % 2) * 0.08), seg=6)
    return b.join(subdivide=0)


def fish_school():
    b = Builder("fish_school")
    b.torus(0xC8E8EE, 0.42, 0.025, (0, 0, 0.05), seg=18)
    b.torus(0xC8E8EE, 0.65, 0.02, (0, 0, 0.04), seg=18)
    for i, (dx, dy, yaw, s) in enumerate(((0, 0, 0.4, 1.0), (0.35, -0.2, 2.6, 0.8), (-0.3, -0.25, 4.4, 0.85), (-0.1, 0.35, 1.6, 0.7))):
        c = 0xA8C4CC if i % 2 == 0 else 0x5E7E8C
        b.sphere(c, 0.2 * s, (dx, dy, 0.1 * s), seg=8, scale=(0.35, 1.05, 0.55), rot=(0, 0, yaw))
        tx, ty = dx - math.sin(yaw) * 0.26 * s, dy - math.cos(yaw) * 0.26 * s
        b.cone(c, 0.1 * s, 0.18 * s, (tx, ty, 0.12 * s), seg=4, rot=(0.9, 0, yaw))
    return b.join(subdivide=0)


# ── cosmetic terrain decorations (vegetation.rs prop_meshes order) ──────────


def prop_shrub():
    b = Builder("prop_shrub")
    b.blob(0x6E7D3A, 0.3, (0, 0, 0.2), scale=(1, 1, 0.8), noise=0.4)
    b.blob(0x5C6A30, 0.19, (0.24, 0.1, 0.12), scale=(1, 1, 0.85), noise=0.4)
    b.blob(0x5C6A30, 0.14, (-0.2, -0.12, 0.1), scale=(1, 1, 0.8), noise=0.4)
    return b.join(subdivide=0)


def prop_dune_grass():
    b = Builder("prop_dune_grass")
    for i in range(5):
        a = i / 5 * math.tau
        h = 0.5 + (i % 2) * 0.18
        lean = 0.3 + (i % 3) * 0.08
        b.cone(0xC2B06A, 0.045, h, (math.cos(a) * 0.08, math.sin(a) * 0.08, h * 0.45), seg=3, rot=(math.sin(a) * lean, -math.cos(a) * lean, 0))
    return b.join(subdivide=0)


def prop_rock():
    b = Builder("prop_rock")
    rng = random.Random(7)
    b.rock(0x8C8880, 0.24, (0, 0, 0.06), rng, squash=0.5, dark=0x4E4B45)
    b.rock(0x807C74, 0.11, (0.24, 0.13, 0.03), rng, squash=0.55, dark=0x4E4B45)
    return b.join(subdivide=0)


def prop_boulder():
    b = Builder("prop_boulder")
    rng = random.Random(19)
    b.rock(0x9A968C, 0.42, (0, 0, 0.16), rng, squash=0.62, dark=0x55514A)
    b.rock(0x8C887F, 0.2, (0.4, 0.1, 0.06), rng, squash=0.5, dark=0x55514A)
    b.rock(0x9A968C, 0.1, (-0.32, -0.22, 0.03), rng, squash=0.6, dark=0x55514A)
    return b.join(subdivide=0)


def prop_reeds():
    b = Builder("prop_reeds")
    for i in range(5):
        h = 0.7 + (i % 3) * 0.22
        dx = (i - 2) * 0.09
        dy = (i % 2) * 0.06
        b.cyl(0x8A9A52, 0.035, h, (dx, dy, h / 2), seg=4, r_top=0.022, bevel=0.0)
        b.cyl(0x6B5A2E, 0.05, 0.16, (dx, dy, h), seg=4, bevel=0.0)
    return b.join(subdivide=0)


def prop_palm():
    b = Builder("prop_palm")
    b.cyl(0x7A5A32, 0.1, 1.6, (0, 0, 0.8), seg=5, r_top=0.06, bevel=0.0)
    for i in range(6):
        a = i / 6 * math.tau
        dx, dy = math.cos(a), math.sin(a)
        b.cone(0x3F8F49, 0.18, 0.95, (dx * 0.4, dy * 0.4, 1.56), seg=4, rot=(1.75, 0, a + math.pi / 2), scale=(1, 0.3, 1))
    b.blob(0x3F8F49, 0.13, (0, 0, 1.62), noise=0.15)
    return b.join(subdivide=0)


def prop_pine():
    b = Builder("prop_pine")
    b.cyl(0x553A20, 0.1, 0.5, (0, 0, 0.25), seg=5, r_top=0.07, bevel=0.0)
    for r, h, z in ((0.42, 0.6, 0.62), (0.32, 0.55, 0.95), (0.2, 0.5, 1.28)):
        b.cone(0x2F5D2A, r, h, (0, 0, z), seg=7)
    return b.join(subdivide=0)


def prop_flowers():
    b = Builder("prop_flowers")
    heads = (0xE8E2D2, 0xC24A3A, 0xD9A83A, 0xE8E2D2, 0xB46AC0)
    for i, (dx, dy) in enumerate(((0, 0), (0.16, 0.08), (-0.14, 0.12), (0.05, -0.16), (-0.1, -0.1))):
        h = 0.16 + (i % 3) * 0.05
        b.cyl(0x5D7A3A, 0.01, h, (dx, dy, h / 2), seg=4, bevel=0.0)
        b.sphere(heads[i % 5], 0.035, (dx, dy, h + 0.02), seg=6)
    return b.join(subdivide=0)


# ── peasant hand tools (parented onto the right hand, swapped per task) ─────
# Origin = grip point; haft runs up local +Z, business end faces -Y (game +Z,
# the unit's facing) so the swing arc reads correctly.

WOOD_HAFT = 0x6B4A2B
TOOL_STEEL = 0xB8BDC4


def tool_axe():
    b = Builder("tool_axe")
    b.cyl(WOOD_HAFT, 0.014, 0.3, (0, 0, 0.1), seg=5, bevel=0.0)
    b.box(TOOL_STEEL, (0.018, 0.07, 0.06), (0, -0.045, 0.22), bevel=0.004)
    b.box(TOOL_STEEL, (0.02, 0.025, 0.07), (0, -0.085, 0.22), bevel=0.004)
    return b.join(subdivide=0)


def tool_pick():
    b = Builder("tool_pick")
    b.cyl(WOOD_HAFT, 0.015, 0.32, (0, 0, 0.11), seg=5, bevel=0.0)
    # curved double-point head: two tapered spikes
    for sy in (-1, 1):
        b.cone(TOOL_STEEL, 0.022, 0.14, (0, sy * 0.08, 0.245 - abs(sy) * 0.0), seg=5, rot=(sy * 1.35, 0, 0))
    b.box(TOOL_STEEL, (0.024, 0.05, 0.035), (0, 0, 0.245), bevel=0.004)
    return b.join(subdivide=0)


def tool_sickle():
    b = Builder("tool_sickle")
    b.cyl(WOOD_HAFT, 0.013, 0.12, (0, 0, 0.04), seg=5, bevel=0.0)
    # crescent blade: short angled segments sweeping forward
    ang = 0.0
    x, y, z = 0.0, -0.02, 0.11
    for i in range(4):
        seg_l = 0.055
        ang += 0.55
        ny, nz = y - math.cos(ang) * seg_l, z + math.sin(ang) * seg_l * 0.4
        b.box(TOOL_STEEL, (0.012, seg_l * 1.2, 0.018), (x, (y + ny) / 2, (z + nz) / 2), rot=(ang * 0.5, 0, 0), bevel=0.0)
        y, z = ny, nz
    return b.join(subdivide=0)


# ── ruin landmarks (rare explorable set-pieces; render-only) ─────────────────

RUIN_STONE = 0xB9B2A4
RUIN_STONE_DK = 0x6E685C


def ruin_columns():
    """Broken colonnade: column stumps of varied height on a cracked plinth."""
    b = Builder("ruin_columns")
    rng = random.Random(61)
    b.box(RUIN_STONE_DK, (2.4, 1.5, 0.18), (0, 0, 0.09), bevel=0.03)
    b.box(RUIN_STONE, (2.1, 1.2, 0.14), (0, 0, 0.25), bevel=0.02)
    for i, sx in enumerate((-0.85, -0.28, 0.28, 0.85)):
        for sy in (-0.42, 0.42):
            h = (0.5, 1.3, 0.9, 0.35)[(i + (sy > 0)) % 4] + rng.random() * 0.2
            b.cyl(RUIN_STONE, 0.12, h, (sx, sy, 0.32 + h / 2), seg=8, bevel=0.0)
            b.cyl(RUIN_STONE_DK, 0.15, 0.07, (sx, sy, 0.36), seg=8, bevel=0.0)
            if h > 1.0:
                b.cyl(RUIN_STONE_DK, 0.15, 0.08, (sx, sy, 0.32 + h), seg=8, bevel=0.0)
    # one surviving architrave span + a fallen drum
    b.box(RUIN_STONE, (0.75, 0.2, 0.16), (-0.56, -0.42, 1.75), bevel=0.02)
    b.cyl(RUIN_STONE, 0.12, 0.6, (0.5, 0.05, 0.4), seg=8, rot=(0.3, math.pi / 2, 0), bevel=0.0)
    for i in range(5):
        a = rng.random() * 6.2832
        d = 0.9 + rng.random() * 0.6
        b.rock(RUIN_STONE, 0.07 + rng.random() * 0.05, (math.cos(a) * d, math.sin(a) * d, 0.03), rng, squash=0.6, dark=RUIN_STONE_DK)
    return b.join(subdivide=0)


def ruin_arch():
    """Lone standing arch with a collapsed wall stub — desert-monument read."""
    b = Builder("ruin_arch")
    rng = random.Random(67)
    for sx in (-0.5, 0.5):
        b.box(RUIN_STONE, (0.3, 0.28, 1.5), (sx, 0, 0.75), bevel=0.02)
    b.box(RUIN_STONE, (1.4, 0.3, 0.3), (0, 0, 1.62), bevel=0.02)
    b.box(0x2A2620, (0.72, 0.1, 0.95), (0, 0, 0.62), bevel=0.0)
    # collapsed wall running off one side
    b.box(RUIN_STONE, (0.9, 0.24, 0.5), (1.1, 0.02, 0.25), rot=(0, 0, 0.15), bevel=0.02)
    b.box(RUIN_STONE_DK, (0.5, 0.22, 0.3), (1.7, 0.08, 0.15), rot=(0, 0, 0.4), bevel=0.02)
    for i in range(6):
        a = rng.random() * 6.2832
        d = 0.7 + rng.random() * 0.9
        b.rock(RUIN_STONE, 0.06 + rng.random() * 0.06, (math.cos(a) * d, math.sin(a) * d, 0.03), rng, squash=0.6, dark=RUIN_STONE_DK)
    return b.join(subdivide=0)


def ruin_circle():
    """Ancient stone circle half-sunk in the ground."""
    b = Builder("ruin_circle")
    rng = random.Random(71)
    for i in range(7):
        a = i / 7 * 6.2832
        h = 0.5 + rng.random() * 0.5
        lean = (rng.random() - 0.5) * 0.35
        b.box(0x8E887C, (0.22, 0.34, h), (math.cos(a) * 0.95, math.sin(a) * 0.95, h * 0.4), rot=(lean, 0, a), bevel=0.02)
    b.cyl(RUIN_STONE_DK, 0.42, 0.12, (0, 0, 0.06), seg=10, bevel=0.0)
    for i in range(4):
        a = rng.random() * 6.2832
        b.rock(0x8E887C, 0.07, (math.cos(a) * 1.4, math.sin(a) * 1.4, 0.03), rng, squash=0.6, dark=RUIN_STONE_DK)
    return b.join(subdivide=0)


BUILDERS = {
    "tool_axe": tool_axe,
    "tool_pick": tool_pick,
    "tool_sickle": tool_sickle,
    "ruin_columns": ruin_columns,
    "ruin_arch": ruin_arch,
    "ruin_circle": ruin_circle,
    "prop_shrub": prop_shrub,
    "prop_dune_grass": prop_dune_grass,
    "prop_rock": prop_rock,
    "prop_boulder": prop_boulder,
    "prop_reeds": prop_reeds,
    "prop_palm": prop_palm,
    "prop_pine": prop_pine,
    "prop_flowers": prop_flowers,
    "tree_broadleaf": tree_broadleaf,
    "tree_conifer": tree_conifer,
    "tree_broadleaf_tall": tree_broadleaf_tall,
    "tree_olive": tree_olive,
    "tree_palm": tree_palm,
    "stone_a": stone_a,
    "stone_b": stone_b,
    "stone_c": stone_c,
    "gold_a": gold_a,
    "gold_b": gold_b,
    "food_deer": food_deer,
    "food_boar": food_boar,
    "food_berry": food_berry,
    "food_deer_grazing": food_deer_grazing,
    "food_deer_carcass": food_deer_carcass,
    "food_boar_carcass": food_boar_carcass,
    "fish_school": fish_school,
}
