"""Bake unit rigs to assets/models/units/*.glb (multi-node: one node per
RigGroup part) + a contact sheet.

    blender --background --factory-startup --python scripts/bake_models/bake_units.py -- \
        [--only peasant,knight] [--sheet /tmp/units_sheet.png] [--no-ao]
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import bpy

import common
from units import BUILDERS

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "assets" / "models" / "units"


def main():
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    only = None
    sheet = "/tmp/units_sheet.png"
    ao = True
    i = 0
    while i < len(argv):
        if argv[i] == "--only":
            only = argv[i + 1].split(",")
            i += 2
        elif argv[i] == "--sheet":
            sheet = argv[i + 1]
            i += 2
        elif argv[i] == "--no-ao":
            ao = False
            i += 1
        else:
            raise SystemExit(f"unknown arg {argv[i]}")

    common.reset_scene()
    world = bpy.data.worlds.new("bake")
    bpy.context.scene.world = world
    world.light_settings.distance = 2.5

    OUT.mkdir(parents=True, exist_ok=True)
    done = []
    for name in only or list(BUILDERS):
        parts = BUILDERS[name]()  # [(obj, pivot)] in assembled world pose
        if ao:
            # bake with the whole unit assembled so parts shade each other
            for obj, _ in parts:
                common.bake_ao(obj, protect_white=True)
        objs = []
        for obj, pivot in parts:
            common.rebase_origin(obj, pivot)
            objs.append(obj)
        common.export_glb_parts(objs, OUT / f"{name}.glb")
        for obj in objs:
            obj.location.x += 1000.0
        done.append(objs)
        total = sum(len(o.data.vertices) for o in objs)
        print(f"baked {name}: {len(objs)} parts, {total} verts")

    common.render_sheet(done, sheet, cell=2.5)
    print(f"sheet: {sheet}")


main()
