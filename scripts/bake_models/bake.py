"""Bake a model set to assets/models/<set>/*.glb + a contact sheet.

    blender --background --factory-startup --python scripts/bake_models/bake.py -- \
        --set props [--only food_deer,stone_a] [--sheet /tmp/props_sheet.png] [--no-ao]
"""

import importlib
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import bpy

import common

ROOT = Path(__file__).resolve().parents[2]


def main():
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    which = "buildings"
    only = None
    sheet = None
    ao = True
    i = 0
    while i < len(argv):
        if argv[i] == "--set":
            which = argv[i + 1]
            i += 2
        elif argv[i] == "--only":
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
    sheet = sheet or f"/tmp/{which}_sheet.png"
    builders = importlib.import_module(which).BUILDERS

    common.reset_scene()
    world = bpy.data.worlds.new("bake")
    bpy.context.scene.world = world
    world.light_settings.distance = 2.5

    out = ROOT / "assets" / "models" / which
    out.mkdir(parents=True, exist_ok=True)
    done = []
    for name in only or list(builders):
        obj = builders[name]()
        if ao:
            common.bake_ao(obj)
        common.export_glb(obj, out / f"{name}.glb")
        # park finished models away from the origin so the next AO bake
        # doesn't pick up their occlusion; render_sheet repositions them
        obj.location.x += 1000.0
        done.append(obj)
        print(f"baked {name}: {len(obj.data.vertices)} verts")

    common.render_sheet(done, sheet, cell=3.0 if which == "props" else 6.0)
    print(f"sheet: {sheet}")


main()
