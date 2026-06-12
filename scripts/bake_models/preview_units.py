"""Studio close-up renders of assembled unit rigs (no export — inspection
only). One large PNG per unit per angle, flat light + cavity so joint gaps
and part connections are obvious.

    blender --background --factory-startup --python scripts/bake_models/preview_units.py -- \
        --only peasant_ayyubid,peasant_crusader [--out /tmp/unit_preview]
"""

import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import bpy

import common
from units import BUILDERS


def main():
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    only = None
    out = "/tmp/unit_preview"
    i = 0
    while i < len(argv):
        if argv[i] == "--only":
            only = argv[i + 1].split(",")
            i += 2
        elif argv[i] == "--out":
            out = argv[i + 1]
            i += 2
        else:
            raise SystemExit(f"unknown arg {argv[i]}")

    for name in only or list(BUILDERS):
        common.reset_scene()
        parts = BUILDERS[name]()  # assembled world pose at origin
        objs = [o for o, _ in parts]
        zmax = max(max((o.matrix_world @ v.co).z for v in o.data.vertices) for o in objs)
        center = zmax * 0.48

        scene = bpy.context.scene
        scene.view_settings.view_transform = "Standard"
        scene.render.engine = "BLENDER_WORKBENCH"
        scene.display.shading.light = "FLAT"
        scene.display.shading.color_type = "VERTEX"
        scene.display.shading.show_shadows = False
        scene.display.shading.show_cavity = True
        scene.display.shading.cavity_type = "BOTH"
        scene.render.resolution_x = 700
        scene.render.resolution_y = 800

        for label, yaw in (("front", math.radians(210)), ("back", math.radians(35))):
            d = 4.0
            cam_x = math.sin(yaw) * d
            cam_y = math.cos(yaw) * d
            bpy.ops.object.camera_add(
                location=(cam_x, cam_y, center + d * 0.45),
            )
            cam = bpy.context.object
            cam.data.type = "ORTHO"
            cam.data.ortho_scale = zmax * 1.35
            # aim at the model center
            direction = bpy.context.object.location - bpy.data.objects[objs[0].name].matrix_world.translation
            from mathutils import Vector

            look = Vector((0.0, 0.0, center)) - cam.location
            cam.rotation_euler = look.to_track_quat("-Z", "Y").to_euler()
            scene.camera = cam
            scene.render.filepath = f"{out}_{name}_{label}.png"
            bpy.ops.render.render(write_still=True)
            print(f"wrote {out}_{name}_{label}.png")


main()
