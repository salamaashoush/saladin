"""Unit rig builders, one per (kind, faction). Each unit = named part objects
(Body/LegL/LegR/ArmL/ArmR/WheelFL..) matching the client's RigGroup contract:
verts pivot-relative, glb node translation = joint pivot. Pure-white verts =
team-tintable (the client's bake_team recolors exact white). Units face -Y
(game +Z). Proportions from sim unit_def (height h to head top, radius r).

Faction kits: Ayyubid = turbans, spiked helms, round shields, robes;
Crusader = nasal/kettle/great helms, kite shields, surcoats. Siege engines
are shared (no faction suffix).
"""

import math

from common import Builder

AY = "ayyubid"
CR = "crusader"

TINT = 0xFFFFFF  # team color slot — must stay EXACTLY white
SKIN = 0xD9A878
METAL = 0x9AA0A6
STEEL = 0xC4C9CF
IRON = 0x4A4D52
WOOD = 0x6B4A2B
WOOD_DARK = 0x4A3522
LEATHER = 0x7A5230
ROPE = 0xB9A06A
GOLD = 0xD6B24A
WHITE_CLOTH = 0xF2EFE6
HIDE_BAY = 0x6A4A2A
HIDE_DARK = 0x33251A
STONE = 0x7A7A7A


def _helmet(b, style, hz, r, s=1.0):
    """Headgear vocabulary shared by foot + mounted riders."""
    if style == "turban":
        b.sphere(WHITE_CLOTH, r * 0.38 * s, (0, 0, hz + r * 0.12 * s), seg=8, scale=(1.0, 1.0, 0.72))
        b.box(TINT, (r * 0.78 * s, r * 0.78 * s, r * 0.1 * s), (0, 0, hz + r * 0.06 * s), bevel=0.0)
    elif style == "spike":
        b.cone(METAL, r * 0.36 * s, r * 0.62 * s, (0, 0, hz + r * 0.34 * s), seg=8)
        b.cyl(GOLD, r * 0.37 * s, 0.018, (0, 0, hz + r * 0.1 * s), seg=8, bevel=0.0)
    elif style == "conical":
        b.cone(METAL, r * 0.36 * s, r * 0.55 * s, (0, 0, hz + r * 0.32 * s), seg=8)
        b.box(METAL, (0.02, 0.02, r * 0.34 * s), (0, -r * 0.32 * s, hz - 0.01), bevel=0.0)
    elif style == "kettle":
        b.cyl(METAL, r * 0.48 * s, 0.02, (0, 0, hz + r * 0.1 * s), seg=9, bevel=0.0)
        b.sphere(METAL, r * 0.3 * s, (0, 0, hz + r * 0.16 * s), seg=8, hemi=True)
    elif style == "great":
        b.cyl(STEEL, r * 0.32 * s, r * 0.56 * s, (0, 0, hz + r * 0.06 * s), seg=8, bevel=0.0)
        b.box(IRON, (r * 0.55 * s, 0.012, 0.02), (0, -r * 0.3 * s, hz), bevel=0.0)
    elif style == "hat":
        b.cone(0xC9A64A, r * 0.5 * s, r * 0.28 * s, (0, 0, hz + r * 0.3 * s), seg=8)
    elif style == "hood":
        b.sphere(TINT, r * 0.37 * s, (0, 0, hz + r * 0.06 * s), seg=8, scale=(1.0, 1.05, 0.95))


def _round_shield(b, hand, r, boss=METAL, face=LEATHER):
    b.cyl(face, r * 0.55, 0.035, (hand[0] - 0.03, hand[1], hand[2] + 0.08), seg=10, rot=(0, math.pi / 2, 0), bevel=0.0)
    b.sphere(boss, r * 0.12, (hand[0] - 0.06, hand[1], hand[2] + 0.08), seg=6)


def _kite_shield(b, hand, r, h):
    b.box(TINT, (0.04, r * 0.8, h * 0.36), (hand[0] - 0.05, hand[1], hand[2] - 0.02), rot=(0.08, 0, 0), bevel=0.02)
    b.cone(TINT, r * 0.4, h * 0.14, (hand[0] - 0.05, hand[1], hand[2] - h * 0.25), seg=4, rot=(math.pi, 0, 0))


def _bow(b, hand, R, x_off=-0.02):
    n = 7
    for i in range(n):
        a0 = -1.1 + 2.2 * i / n
        a1 = -1.1 + 2.2 * (i + 1) / n
        z0, y0 = math.cos(a0) * R, math.sin(a0) * R
        z1, y1 = math.cos(a1) * R, math.sin(a1) * R
        b.cyl(
            WOOD_DARK,
            0.012,
            math.dist((y0, z0), (y1, z1)) * 1.15,
            (hand[0] + x_off, hand[1] + (y0 + y1) / 2, hand[2] + (z0 + z1) / 2),
            seg=4,
            rot=(math.atan2(y1 - y0, z1 - z0), 0, 0),
            bevel=0.0,
        )
    b.cyl(ROPE, 0.005, R * 1.78, (hand[0] + x_off, hand[1], hand[2]), seg=3, bevel=0.0)


def _leg(group, h, r, sx, color=LEATHER):
    b = Builder(group)
    hip = h * 0.46
    x = sx * r * 0.32
    b.cyl(color, r * 0.16, hip * 0.82, (x, 0, hip * 0.55), seg=6, r_top=r * 0.13, bevel=0.0)
    b.box(WOOD_DARK, (r * 0.3, r * 0.42, hip * 0.16), (x, -r * 0.06, hip * 0.08), bevel=0.0)
    return b.join(subdivide=0), (x, 0, hip)


def _arm(group, h, r, sx, sleeve):
    b = Builder(group)
    sz = h * 0.92
    x = sx * r * 0.72
    ln = h * 0.34
    b.cyl(sleeve, r * 0.13, ln * 0.55, (x, 0, sz - ln * 0.28), seg=6, r_top=r * 0.11, bevel=0.0)
    b.cyl(SKIN, r * 0.09, ln * 0.45, (x, 0, sz - ln * 0.75), seg=6, bevel=0.0)
    hand = (x, 0, sz - ln)
    return b, hand, (x, 0, sz)


def _torso(b, h, r, tunic, belt=True):
    b.cyl(tunic, r * 0.62, h * 0.34, (0, 0, h * 0.62), seg=8, r_top=r * 0.52, bevel=0.0)
    if belt:
        b.cyl(LEATHER, r * 0.56, h * 0.05, (0, 0, h * 0.47), seg=8, bevel=0.0)
    b.sphere(tunic, r * 0.5, (0, 0, h * 0.8), seg=8, scale=(1.0, 0.9, 0.55))


def _head(b, h, r):
    z = h * 0.88
    b.sphere(SKIN, r * 0.34, (0, 0, z), seg=10)
    return z


# ── infantry ─────────────────────────────────────────────────────────────────


def peasant(f):
    """The workhorse unit — richest body of the infantry set. Bare hands:
    tools (axe/pick/sickle) are separate meshes the client parents onto the
    right hand and swaps per activity. Arm pivots sit AT the shoulder balls
    and the sleeves overlap them, so the joints stay visually connected
    through every swing."""
    h, r = 0.7, 0.22
    parts = []
    b = Builder("Body")
    # knee-length work tunic: gentle flare, hem above the knee so legs read
    b.cyl(TINT, r * 0.6, h * 0.26, (0, 0, h * 0.43), seg=9, r_top=r * 0.5, bevel=0.0)
    b.cyl(ROPE, r * 0.52, h * 0.04, (0, 0, h * 0.555), seg=9, bevel=0.0)
    # torso
    b.cyl(TINT, r * 0.5, h * 0.24, (0, 0, h * 0.68), seg=9, r_top=r * 0.44, bevel=0.0)
    b.sphere(TINT, r * 0.46, (0, 0, h * 0.79), seg=9, scale=(1.05, 0.85, 0.5))
    # belt kit: pouch + water gourd
    b.sphere(LEATHER, r * 0.15, (r * 0.46, -r * 0.28, h * 0.55), seg=7, scale=(1, 0.7, 1.1))
    b.sphere(0xC9A64A, r * 0.12, (-r * 0.48, r * 0.24, h * 0.54), seg=7, scale=(1, 1, 1.3))
    # shoulder balls the arm pivots anchor into
    for sx in (-1, 1):
        b.sphere(TINT, r * 0.24, (sx * r * 0.56, 0, h * 0.84), seg=8)
    # visible neck, head clear of the chest, faction headgear
    b.cyl(SKIN, r * 0.13, h * 0.1, (0, 0, h * 0.88), seg=6, bevel=0.0)
    hz = h * 0.99
    b.sphere(SKIN, r * 0.31, (0, 0, hz), seg=10)
    if f == CR:
        _helmet(b, "hat", hz, r * 0.92)
    else:
        _helmet(b, "turban", hz, r * 0.92)
    parts.append((b.join(subdivide=0), (0, 0, 0)))
    for grp, sx in (("LegL", -1), ("LegR", 1)):
        obj, piv = _leg(grp, h, r, sx)
        parts.append((obj, piv))
    # arms pivot at the shoulder balls; sleeve tops overlap them
    for grp, sx in (("ArmR", 1), ("ArmL", -1)):
        b = Builder(grp)
        px, pz = sx * r * 0.62, h * 0.85
        ln = h * 0.38
        b.cyl(TINT, r * 0.16, ln * 0.5, (px, 0, pz - ln * 0.2), seg=7, r_top=r * 0.13, bevel=0.0)
        b.cyl(SKIN, r * 0.1, ln * 0.5, (px, 0, pz - ln * 0.72), seg=6, bevel=0.0)
        b.sphere(SKIN, r * 0.12, (px, 0, pz - ln), seg=6)
        parts.append((b.join(subdivide=0), (px, 0, pz)))
    return parts


def spearman(f):
    h, r = 0.85, 0.26
    parts = []
    b = Builder("Body")
    _torso(b, h, r, TINT)
    hz = _head(b, h, r)
    _helmet(b, "conical" if f == CR else "spike", hz, r)
    parts.append((b.join(subdivide=0), (0, 0, 0)))
    for grp, sx in (("LegL", -1), ("LegR", 1)):
        obj, piv = _leg(grp, h, r, sx)
        parts.append((obj, piv))
    b, hand, piv = _arm("ArmR", h, r, 1, TINT)
    b.cyl(WOOD, 0.016, h * 1.15, (hand[0], hand[1], hand[2] + h * 0.18), seg=5, bevel=0.0)
    b.cone(STEEL, 0.035, 0.12, (hand[0], hand[1], hand[2] + h * 0.78), seg=5)
    parts.append((b.join(subdivide=0), piv))
    b, hand, piv = _arm("ArmL", h, r, -1, TINT)
    if f == CR:
        _kite_shield(b, hand, r, h)
    else:
        _round_shield(b, hand, r, boss=GOLD)
    parts.append((b.join(subdivide=0), piv))
    return parts


def archer(f):
    h, r = 0.8, 0.24
    parts = []
    b = Builder("Body")
    _torso(b, h, r, TINT)
    hz = _head(b, h, r)
    _helmet(b, "hood" if f == CR else "turban", hz, r)
    b.cyl(LEATHER, 0.045, 0.3, (0.08, r * 0.5, h * 0.62), seg=6, rot=(0.4, 0.3, 0), bevel=0.0)
    for i in range(3):
        b.cyl(WOOD_DARK, 0.008, 0.1, (0.05 + i * 0.025, r * 0.52 - i * 0.01, h * 0.78), seg=4, rot=(0.4, 0.3, 0), bevel=0.0)
    parts.append((b.join(subdivide=0), (0, 0, 0)))
    for grp, sx in (("LegL", -1), ("LegR", 1)):
        obj, piv = _leg(grp, h, r, sx)
        parts.append((obj, piv))
    b, hand, piv = _arm("ArmL", h, r, -1, TINT)
    _bow(b, hand, h * 0.32)
    parts.append((b.join(subdivide=0), piv))
    b, hand, piv = _arm("ArmR", h, r, 1, TINT)
    parts.append((b.join(subdivide=0), piv))
    return parts


def crossbowman(f):
    h, r = 0.82, 0.25
    parts = []
    b = Builder("Body")
    _torso(b, h, r, TINT)
    hz = _head(b, h, r)
    _helmet(b, "kettle" if f == CR else "turban", hz, r)
    parts.append((b.join(subdivide=0), (0, 0, 0)))
    for grp, sx in (("LegL", -1), ("LegR", 1)):
        obj, piv = _leg(grp, h, r, sx)
        parts.append((obj, piv))
    b, hand, piv = _arm("ArmL", h, r, -1, TINT)
    b.box(WOOD, (0.04, 0.3, 0.04), (hand[0], hand[1] - 0.1, hand[2] + 0.04), bevel=0.0)
    b.box(STEEL, (0.3, 0.03, 0.025), (hand[0], hand[1] - 0.22, hand[2] + 0.05), bevel=0.0)
    b.cyl(ROPE, 0.004, 0.3, (hand[0], hand[1] - 0.16, hand[2] + 0.05), seg=3, rot=(0, math.pi / 2, 0), bevel=0.0)
    parts.append((b.join(subdivide=0), piv))
    b, hand, piv = _arm("ArmR", h, r, 1, TINT)
    parts.append((b.join(subdivide=0), piv))
    return parts


def imam(f):
    """Ayyubid imam / Crusader chaplain — same battlefield role."""
    h, r = 0.85, 0.24
    robe = WHITE_CLOTH if f == AY else 0x6E5640  # brown cassock
    parts = []
    b = Builder("Body")
    b.cyl(robe, r * 0.66, h * 0.62, (0, 0, h * 0.34), seg=9, r_top=r * 0.5, bevel=0.0)
    b.sphere(robe, r * 0.5, (0, 0, h * 0.68), seg=8, scale=(1.0, 0.9, 0.6))
    b.box(TINT, (r * 0.24, r * 1.1, h * 0.4), (0, 0, h * 0.6), rot=(0, 0.5, 0), bevel=0.0)
    hz = _head(b, h, r)
    if f == AY:
        b.cyl(TINT, r * 0.36, h * 0.1, (0, 0, hz + r * 0.3), seg=8, bevel=0.0)
        b.sphere(WHITE_CLOTH, r * 0.37, (0, 0, hz + r * 0.42), seg=8, scale=(1, 1, 0.5))
    else:
        b.sphere(robe, r * 0.37, (0, 0, hz + r * 0.06), seg=8, scale=(1.0, 1.05, 0.9))
    parts.append((b.join(subdivide=0), (0, 0, 0)))
    for grp, sx in (("LegL", -1), ("LegR", 1)):
        obj, piv = _leg(grp, h, r, sx, color=robe)
        parts.append((obj, piv))
    b, hand, piv = _arm("ArmR", h, r, 1, robe)
    b.cyl(WOOD_DARK, 0.015, h * 1.0, (hand[0], hand[1], hand[2] + h * 0.12), seg=5, bevel=0.0)
    if f == AY:
        b.torus(GOLD, 0.05, 0.012, (hand[0], hand[1], hand[2] + h * 0.66), seg=10, rot=(0, math.pi / 2, 0))
    else:
        b.box(GOLD, (0.025, 0.025, 0.16), (hand[0], hand[1], hand[2] + h * 0.62), bevel=0.0)
        b.box(GOLD, (0.1, 0.025, 0.025), (hand[0], hand[1], hand[2] + h * 0.65), bevel=0.0)
    parts.append((b.join(subdivide=0), piv))
    b, hand, piv = _arm("ArmL", h, r, -1, robe)
    parts.append((b.join(subdivide=0), piv))
    return parts


# ── mounted ──────────────────────────────────────────────────────────────────

RIDER_SCALE = 0.78
SADDLE_Y = 0.55


def _horse(b, h, r, hide, caparison=None):
    back = h * SADDLE_Y
    b.sphere(hide, h * 0.23, (0, 0, back - h * 0.1), seg=10, scale=(0.85, 2.1, 0.95))
    if caparison:
        b.sphere(caparison, h * 0.245, (0, 0, back - h * 0.1), seg=10, scale=(0.8, 1.7, 0.8))
    b.sphere(hide, h * 0.15, (0, -h * 0.38, back - h * 0.08), seg=8)
    b.sphere(hide, h * 0.15, (0, h * 0.38, back - h * 0.06), seg=8)
    b.cyl(hide, h * 0.09, h * 0.32, (0, -h * 0.46, back + h * 0.12), seg=7, r_top=h * 0.07, rot=(0.7, 0, 0), bevel=0.0)
    b.sphere(hide, h * 0.085, (0, -h * 0.58, back + h * 0.26), seg=8, scale=(0.8, 1.5, 0.9))
    for sx in (-1, 1):
        b.cone(HIDE_DARK, 0.025, 0.06, (sx * 0.045, -h * 0.52, back + h * 0.33), seg=4)
    b.box(HIDE_DARK, (0.03, h * 0.3, h * 0.08), (0, -h * 0.42, back + h * 0.16), rot=(0.7, 0, 0), bevel=0.0)
    b.cyl(HIDE_DARK, 0.03, h * 0.3, (0, h * 0.52, back - h * 0.16), seg=5, r_top=0.015, rot=(-0.5, 0, 0), bevel=0.0)
    return back


def _horse_leg(group, h, r, sx, sy, hide):
    b = Builder(group)
    hip = h * SADDLE_Y - h * 0.12
    x = sx * r * 0.3
    y = sy * h * 0.42
    b.cyl(hide, h * 0.045, hip * 0.95, (x, y, hip * 0.5), seg=5, r_top=h * 0.035, bevel=0.0)
    b.cyl(HIDE_DARK, h * 0.045, h * 0.05, (x, y, h * 0.03), seg=5, bevel=0.0)
    return b.join(subdivide=0), (x, y, hip)


def _rider(b, h, r, tunic, helmet):
    s = RIDER_SCALE
    base = h * SADDLE_Y
    b.cyl(tunic, r * 0.5 * s, h * 0.3 * s, (0, 0, base + h * 0.16 * s), seg=8, r_top=r * 0.42 * s, bevel=0.0)
    hz = base + h * 0.36 * s
    b.sphere(SKIN, r * 0.27 * s, (0, 0, hz), seg=8)
    _helmet(b, helmet, hz, r, s=s)
    return base, hz


def _rider_arm(group, h, r, sx, sleeve):
    s = RIDER_SCALE
    base = h * SADDLE_Y
    x = sx * r * 0.72 * s
    sz = base + h * 0.34 * s
    b = Builder(group)
    ln = h * 0.3 * s
    b.cyl(sleeve, r * 0.12 * s, ln * 0.55, (x, 0, sz - ln * 0.28), seg=5, r_top=r * 0.1 * s, bevel=0.0)
    b.cyl(SKIN, r * 0.08 * s, ln * 0.45, (x, 0, sz - ln * 0.75), seg=5, bevel=0.0)
    return b, (x, 0, sz - ln), (x, 0, sz)


def _mounted(h, r, hide, tunic, helmet, arm_r=None, arm_l=None, caparison=None):
    parts = []
    b = Builder("Body")
    _horse(b, h, r, hide, caparison)
    _rider(b, h, r, tunic, helmet)
    b.box(tunic if caparison is None else caparison, (r * 0.7, h * 0.3, 0.03), (0, 0, h * SADDLE_Y + 0.01), bevel=0.0)
    parts.append((b.join(subdivide=0), (0, 0, 0)))
    for grp, sx, sy in (("WheelFL", -1, -1), ("WheelFR", 1, -1), ("WheelBL", -1, 1), ("WheelBR", 1, 1)):
        obj, piv = _horse_leg(grp, h, r, sx, sy, hide)
        parts.append((obj, piv))
    for grp, sx, fn in (("ArmR", 1, arm_r), ("ArmL", -1, arm_l)):
        b, hand, piv = _rider_arm(grp, h, r, sx, tunic)
        if fn:
            fn(b, hand)
        parts.append((b.join(subdivide=0), piv))
    return parts


def knight(f):
    """Crusader knight / Ayyubid heavy ghulam lancer."""
    h, r = 1.0, 0.3

    def lance(b, hand):
        b.cyl(WOOD, 0.018, h * 1.3, (hand[0], hand[1] - h * 0.3, hand[2]), seg=5, r_top=0.008, rot=(math.pi / 2 - 0.15, 0, 0), bevel=0.0)
        b.cone(STEEL, 0.03, 0.1, (hand[0], hand[1] - h * 0.95, hand[2] + h * 0.1), seg=5, rot=(math.pi / 2 - 0.15, 0, 0))

    def cr_shield(b, hand):
        _kite_shield(b, hand, r * RIDER_SCALE, h * RIDER_SCALE)

    def ay_shield(b, hand):
        _round_shield(b, hand, r * RIDER_SCALE, boss=GOLD, face=TINT)

    if f == CR:
        return _mounted(h, r, HIDE_BAY, STEEL, "great", arm_r=lance, arm_l=cr_shield, caparison=TINT)
    return _mounted(h, r, 0x4A3A2A, STEEL, "spike", arm_r=lance, arm_l=ay_shield, caparison=TINT)


def horse_archer(f):
    """Turkic horse archer / Crusader mounted scout."""
    h, r = 0.95, 0.28

    def bow(b, hand):
        _bow(b, hand, h * 0.26, x_off=0.0)

    helmet = "kettle" if f == CR else "turban"
    hide = HIDE_BAY if f == CR else 0x8A6A42
    return _mounted(h, r, hide, TINT, helmet, arm_l=bow)


def mamluk(f):
    """Mamluk askari / Crusader sergeant cavalry."""
    h, r = 1.05, 0.31

    def scimitar(b, hand):
        b.cyl(LEATHER, 0.016, 0.1, (hand[0], hand[1], hand[2] + 0.02), seg=5, bevel=0.0)
        b.box(STEEL, (0.015, 0.05, h * 0.32), (hand[0], hand[1] - 0.03, hand[2] + h * 0.2), rot=(0.35, 0, 0), bevel=0.0)
        b.box(STEEL, (0.015, 0.05, h * 0.14), (hand[0], hand[1] - 0.085, hand[2] + h * 0.36), rot=(0.85, 0, 0), bevel=0.0)

    def sword(b, hand):
        b.cyl(LEATHER, 0.016, 0.1, (hand[0], hand[1], hand[2] + 0.02), seg=5, bevel=0.0)
        b.box(STEEL, (0.04, 0.012, 0.06), (hand[0], hand[1], hand[2] + 0.08), bevel=0.0)
        b.box(STEEL, (0.018, 0.045, h * 0.4), (hand[0], hand[1], hand[2] + h * 0.27), bevel=0.0)

    def ay_shield(b, hand):
        b.cyl(GOLD, r * 0.42 * RIDER_SCALE, 0.03, (hand[0] - 0.04, hand[1], hand[2] + 0.06), seg=9, rot=(0, math.pi / 2, 0), bevel=0.0)

    def cr_shield(b, hand):
        _kite_shield(b, hand, r * RIDER_SCALE, h * RIDER_SCALE)

    if f == CR:
        return _mounted(h, r, 0x5A4632, TINT, "great", arm_r=sword, arm_l=cr_shield)
    return _mounted(h, r, HIDE_DARK, TINT, "spike", arm_r=scimitar, arm_l=ay_shield)


# ── siege (shared between factions) ──────────────────────────────────────────


def ram():
    h, r = 1.1, 0.5
    parts = []
    b = Builder("Body")
    for sx in (-1, 1):
        for sy in (-1, 1):
            b.box(WOOD_DARK, (0.08, 0.08, h * 0.6), (sx * r * 0.62, sy * h * 0.4, h * 0.36), bevel=0.0)
    b.prism(LEATHER, r * 1.6, h * 1.05, h * 0.3, (0, 0, h * 0.62), bevel=0.02)
    b.box(WOOD, (r * 1.7, 0.1, 0.06), (0, 0, h * 0.94), bevel=0.0)
    for sx in (-1, 1):
        b.box(WOOD, (0.05, h * 0.95, h * 0.3), (sx * r * 0.68, 0, h * 0.48), bevel=0.0)
    parts.append((b.join(subdivide=0), (0, 0, 0)))
    b = Builder("ArmR")
    b.cyl(WOOD_DARK, h * 0.07, h * 1.2, (0, 0.05, h * 0.55), seg=7, rot=(math.pi / 2, 0, 0), bevel=0.0)
    b.cyl(IRON, h * 0.085, h * 0.12, (0, -h * 0.58, h * 0.55), seg=7, rot=(math.pi / 2, 0, 0), bevel=0.0)
    for sy in (-0.3, 0.3):
        b.cyl(ROPE, 0.012, h * 0.32, (0, sy, h * 0.74), seg=4, bevel=0.0)
    parts.append((b.join(subdivide=0), (0, 0, h * 0.78)))
    for grp, sx, sy in (("WheelFL", -1, -1), ("WheelFR", 1, -1), ("WheelBL", -1, 1), ("WheelBR", 1, 1)):
        b = Builder(grp)
        x, y = sx * r * 0.78, sy * h * 0.38
        b.cyl(WOOD_DARK, h * 0.17, 0.07, (x, y, h * 0.17), seg=9, rot=(0, math.pi / 2, 0), bevel=0.0)
        b.cyl(WOOD, h * 0.05, 0.09, (x, y, h * 0.17), seg=6, rot=(0, math.pi / 2, 0), bevel=0.0)
        parts.append((b.join(subdivide=0), (x, y, h * 0.17)))
    return parts


def mangonel():
    h, r = 1.0, 0.45
    parts = []
    b = Builder("Body")
    b.box(WOOD, (r * 1.1, h * 1.1, 0.1), (0, 0, h * 0.3), bevel=0.02)
    for sy in (-0.4, 0.4):
        b.box(WOOD_DARK, (r * 1.3, 0.09, 0.09), (0, sy * h, h * 0.3), bevel=0.0)
    for sx in (-1, 1):
        b.box(WOOD_DARK, (0.08, 0.1, h * 0.5), (sx * r * 0.5, 0, h * 0.55), rot=(0.25, 0, 0), bevel=0.0)
    b.cyl(WOOD_DARK, 0.04, r * 1.1, (0, h * 0.05, h * 0.78), seg=6, rot=(0, math.pi / 2, 0), bevel=0.0)
    b.cyl(WOOD, 0.06, r * 0.9, (0, h * 0.42, h * 0.36), seg=6, rot=(0, math.pi / 2, 0), bevel=0.0)
    parts.append((b.join(subdivide=0), (0, 0, 0)))
    b = Builder("ArmR")
    b.box(WOOD, (0.07, 0.09, h * 0.85), (0, h * 0.18, h * 0.45), rot=(-0.5, 0, 0), bevel=0.0)
    b.cyl(LEATHER, h * 0.1, h * 0.06, (0, h * 0.38, h * 0.86), seg=7, r_top=h * 0.08, bevel=0.0)
    b.sphere(STONE, h * 0.07, (0, h * 0.38, h * 0.9), seg=6)
    parts.append((b.join(subdivide=0), (0, h * 0.05, h * 0.78)))
    for grp, sx, sy in (("WheelFL", -1, -1), ("WheelFR", 1, -1), ("WheelBL", -1, 1), ("WheelBR", 1, 1)):
        b = Builder(grp)
        x, y = sx * r * 0.72, sy * h * 0.42
        b.cyl(WOOD_DARK, h * 0.16, 0.06, (x, y, h * 0.16), seg=9, rot=(0, math.pi / 2, 0), bevel=0.0)
        parts.append((b.join(subdivide=0), (x, y, h * 0.16)))
    return parts


BUILDERS = {"ram": ram, "mangonel": mangonel}
for _fn in (peasant, spearman, archer, knight, horse_archer, mamluk, crossbowman, imam):
    for _f in (AY, CR):
        BUILDERS[f"{_fn.__name__}_{_f}"] = (lambda f=_f, fn=_fn: fn(f))
