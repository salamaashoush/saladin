"""Bake building models to assets/models/buildings/*.glb + a contact sheet.

    blender --background --factory-startup --python scripts/bake_models/bake_buildings.py -- \
        [--only keep,house] [--sheet /tmp/buildings_sheet.png] [--no-ao]
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import bpy

import common
from buildings import BUILDERS

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "assets" / "models" / "buildings"


def main():
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    only = None
    sheet = "/tmp/buildings_sheet.png"
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
    # world with a short AO distance so grid neighbours never shade each other
    world = bpy.data.worlds.new("bake")
    bpy.context.scene.world = world
    world.light_settings.distance = 2.5

    OUT.mkdir(parents=True, exist_ok=True)
    names = only or list(BUILDERS)
    done = []
    for name in names:
        obj = BUILDERS[name]()
        if ao:
            common.bake_ao(obj)
        common.export_glb(obj, OUT / f"{name}.glb")
        # park finished models away from the origin so the next AO bake
        # doesn't pick up their occlusion; render_sheet repositions them
        obj.location.x += 1000.0
        done.append(obj)
        print(f"baked {name}: {len(obj.data.vertices)} verts")

    common.render_sheet(done, sheet)
    print(f"sheet: {sheet}")


main()
