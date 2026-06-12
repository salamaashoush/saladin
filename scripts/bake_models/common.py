"""Shared Blender helpers for Saladin model baking.

Run inside `blender --background --factory-startup --python <script>`.
Every asset is authored at world scale (1 tile = 1 unit, Blender Z-up,
front = -Y so the glTF exporter's Y-up flip lands the front on game +Z),
joined to ONE mesh with a `Col` vertex-color attribute (linear floats,
AO multiplied in), and exported as a single-mesh GLB the client parses
straight into a bevy Mesh.
"""

import math

import bpy
from mathutils import Euler, Vector

# Palette mirrored from crates/client/src/render/models/buildings.rs.
STONE = 0x9C958A
STONE_DARK = 0x7D766B
TIMBER = 0x8A6A3A
TIMBER_DARK = 0x5A3A22
PLASTER = 0xCBB487
THATCH = 0x9A7A45
TEAM_CLOTH = 0xDDDDDD  # near-white; the in-game material carries the team tint
SLIT_DARK = 0x2A2620


def srgb_to_linear(c):
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def rgba(hex_color):
    r = srgb_to_linear(((hex_color >> 16) & 0xFF) / 255.0)
    g = srgb_to_linear(((hex_color >> 8) & 0xFF) / 255.0)
    b = srgb_to_linear((hex_color & 0xFF) / 255.0)
    return (r, g, b, 1.0)


def reset_scene():
    bpy.ops.wm.read_factory_settings(use_empty=True)


def _fill_color(obj, hex_color):
    mesh = obj.data
    attr = mesh.color_attributes.new(name="Col", type="FLOAT_COLOR", domain="POINT")
    col = rgba(hex_color)
    for d in attr.data:
        d.color = col
    mesh.color_attributes.active_color_index = mesh.color_attributes.find("Col")


def _place(obj, loc, rot, scale):
    obj.location = loc
    if rot:
        obj.rotation_euler = Euler(rot, "XYZ")
    if scale:
        obj.scale = scale


class Builder:
    """Accumulates colored primitive parts, then joins/bakes/exports."""

    def __init__(self, name):
        self.name = name
        self.parts = []

    def _register(self, color, bevel=0.0):
        obj = bpy.context.object
        if bevel > 0.0:
            mod = obj.modifiers.new("bevel", "BEVEL")
            mod.width = bevel
            mod.segments = 2
            mod.angle_limit = math.radians(40)
        _fill_color(obj, color)
        self.parts.append(obj)
        return obj

    def box(self, color, size, loc, rot=None, bevel=0.02):
        bpy.ops.mesh.primitive_cube_add(size=1.0, location=loc)
        obj = bpy.context.object
        obj.scale = size
        if rot:
            obj.rotation_euler = Euler(rot, "XYZ")
        return self._register(color, bevel)

    def cyl(self, color, r, h, loc, seg=12, rot=None, r_top=None, bevel=0.02):
        if r_top is None:
            bpy.ops.mesh.primitive_cylinder_add(vertices=seg, radius=r, depth=h, location=loc)
        else:
            bpy.ops.mesh.primitive_cone_add(
                vertices=seg, radius1=r, radius2=r_top, depth=h, location=loc
            )
        obj = bpy.context.object
        if rot:
            obj.rotation_euler = Euler(rot, "XYZ")
        return self._register(color, bevel)

    def cone(self, color, r, h, loc, seg=12, rot=None, bevel=0.0, scale=None):
        bpy.ops.mesh.primitive_cone_add(vertices=seg, radius1=r, radius2=0.0, depth=h, location=loc)
        obj = bpy.context.object
        if rot:
            obj.rotation_euler = Euler(rot, "XYZ")
        if scale:
            obj.scale = scale
        return self._register(color, bevel)

    def sphere(self, color, r, loc, seg=16, scale=None, hemi=False, rot=None):
        bpy.ops.mesh.primitive_uv_sphere_add(segments=seg, ring_count=max(seg // 2, 6), radius=r, location=loc)
        obj = bpy.context.object
        if scale:
            obj.scale = scale
        if rot:
            obj.rotation_euler = Euler(rot, "XYZ")
        if hemi:
            # chop the lower half so domes don't waste verts inside walls
            import bmesh

            bm = bmesh.new()
            bm.from_mesh(obj.data)
            geom = [v for v in bm.verts if v.co.z < -1e-4]
            bmesh.ops.delete(bm, geom=geom, context="VERTS")
            bm.to_mesh(obj.data)
            bm.free()
        return self._register(color, bevel=0.0)

    def rock(self, color, r, loc, rng, squash=0.5, dark=None, yaw=None):
        """Angular faceted rock chunk: icosphere-1 with per-vertex radial
        jitter (seeded rng → reproducible), squashed low, bottom flattened
        into the ground. Verts color-graded top→crevice for free depth."""
        bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=1, radius=r, location=loc)
        obj = bpy.context.object
        for v in obj.data.vertices:
            v.co.x *= 0.7 + rng.random() * 0.6
            v.co.y *= 0.7 + rng.random() * 0.6
            v.co.z *= squash * (0.65 + rng.random() * 0.5)
            if v.co.z < 0:
                v.co.z *= 0.3
        obj.rotation_euler = Euler((0, 0, (yaw if yaw is not None else rng.random() * 6.2832)), "XYZ")
        # z-graded fill: tops bright, undersides crevice-dark
        mesh = obj.data
        attr = mesh.color_attributes.new(name="Col", type="FLOAT_COLOR", domain="POINT")
        top = rgba(color)
        low = rgba(dark if dark is not None else ((color >> 1) & 0x7F7F7F))
        zmax = max(v.co.z for v in mesh.vertices) or 1.0
        for i, v in enumerate(mesh.vertices):
            t = max(0.0, min(1.0, v.co.z / zmax))
            attr.data[i].color = tuple(low[k] + (top[k] - low[k]) * t for k in range(4))
        mesh.color_attributes.active_color_index = mesh.color_attributes.find("Col")
        self.parts.append(obj)
        return obj

    def blob(self, color, r, loc, scale=None, noise=0.25, subdiv=2):
        """Organic faceted lump: icosphere + clouds-texture displace.
        Deterministic (texture noise is a pure function of coordinates)."""
        bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=subdiv, radius=r, location=loc)
        obj = bpy.context.object
        if scale:
            obj.scale = scale
        if noise > 0.0:
            tex = bpy.data.textures.get("blobnoise")
            if tex is None:
                tex = bpy.data.textures.new("blobnoise", "CLOUDS")
                tex.noise_scale = 0.55
            mod = obj.modifiers.new("disp", "DISPLACE")
            mod.texture = tex
            mod.strength = noise * r
            mod.texture_coords = "GLOBAL"
        return self._register(color, bevel=0.0)

    def prism(self, color, w, d, h, loc, rot=None, bevel=0.02):
        """Gable-roof solid: w along X, d along Y, ridge along X at height h.
        Base rectangle sits at loc z; triangular ends included."""
        x, y, z = w / 2, d / 2, h
        verts = [
            (-x, -y, 0), (x, -y, 0), (x, y, 0), (-x, y, 0),
            (-x, 0, z), (x, 0, z),
        ]
        faces = [
            (0, 1, 2, 3),          # bottom
            (0, 1, 5, 4),          # -Y slope
            (3, 2, 5, 4),          # +Y slope
            (0, 3, 4),             # -X gable
            (1, 2, 5),             # +X gable
        ]
        mesh = bpy.data.meshes.new("prism")
        mesh.from_pydata(verts, [], faces)
        mesh.update()
        obj = bpy.data.objects.new("prism", mesh)
        bpy.context.collection.objects.link(obj)
        _place(obj, loc, rot, None)
        bpy.context.view_layer.objects.active = obj
        if bevel > 0.0:
            mod = obj.modifiers.new("bevel", "BEVEL")
            mod.width = bevel
            mod.segments = 2
            mod.angle_limit = math.radians(40)
        _fill_color(obj, color)
        self.parts.append(obj)
        return obj

    def torus(self, color, major, minor, loc, seg=16, rot=None):
        bpy.ops.mesh.primitive_torus_add(
            major_radius=major, minor_radius=minor, major_segments=seg, minor_segments=6, location=loc
        )
        obj = bpy.context.object
        if rot:
            obj.rotation_euler = Euler(rot, "XYZ")
        return self._register(color, bevel=0.0)

    # ── compound conveniences shared across buildings ──────────────────────

    def merlon_ring(self, color, cx, cy, top_z, radius, count=8, size=(0.18, 0.14, 0.28)):
        for i in range(count):
            a = i / count * math.tau
            self.box(
                color,
                size,
                (cx + math.cos(a) * radius, cy + math.sin(a) * radius, top_z + size[2] / 2),
                rot=(0, 0, a + math.pi / 2),
            )

    def merlon_row(self, color, start, end, top_z, count, size=(0.2, 0.16, 0.26)):
        sx, sy = start
        ex, ey = end
        ang = math.atan2(ey - sy, ex - sx)
        for i in range(count):
            t = i / max(count - 1, 1)
            x = sx + (ex - sx) * t
            y = sy + (ey - sy) * t
            self.box(color, size, (x, y, top_z + size[2] / 2), rot=(0, 0, ang))

    def arrow_slit(self, x, y, z, yaw=0.0):
        self.box(SLIT_DARK, (0.07, 0.06, 0.4), (x, y, z), rot=(0, 0, yaw), bevel=0.0)

    def pennant(self, x, y, z, h=0.9):
        self.cyl(TIMBER_DARK, 0.035, h, (x, y, z + h / 2), seg=6, bevel=0.0)
        self.sphere(TEAM_CLOTH, 0.05, (x, y, z + h + 0.03), seg=8)
        self.box(TEAM_CLOTH, (0.42, 0.02, 0.2), (x + 0.22, y, z + h - 0.12), bevel=0.0)

    # pointed (four-centred-ish) arch doorway: dark recess + raised surround
    def arch_door(self, x, y, z_base, w=0.5, h=0.8, yaw=0.0, frame=STONE_DARK, depth=0.14):
        rot = (0, 0, yaw)
        self.box(SLIT_DARK, (w, depth, h), (x, y, z_base + h / 2), rot=rot, bevel=0.0)
        cosw, sinw = math.cos(yaw), math.sin(yaw)
        # peak block suggesting the point of the arch
        self.box(
            SLIT_DARK,
            (w * 0.5, depth, w * 0.45),
            (x, y, z_base + h + w * 0.1),
            rot=(0, math.pi / 4, yaw),
            bevel=0.0,
        )
        # surround posts + lintel slab, slightly proud of the wall
        off = depth * 0.25
        px, py = x + sinw * off, y - cosw * off
        for s in (-1.0, 1.0):
            self.box(
                frame,
                (0.1, depth, h + w * 0.3),
                (px + cosw * s * (w / 2 + 0.05), py + sinw * s * (w / 2 + 0.05), z_base + (h + w * 0.3) / 2),
                rot=rot,
            )
        self.box(
            frame,
            (w + 0.2, depth, 0.12),
            (px, py, z_base + h + w * 0.3),
            rot=rot,
        )

    # ── finalize ────────────────────────────────────────────────────────────

    def join(self, subdivide=1):
        for o in bpy.context.selected_objects:
            o.select_set(False)
        for o in self.parts:
            o.select_set(True)
        bpy.context.view_layer.objects.active = self.parts[0]
        bpy.ops.object.convert(target="MESH")  # applies bevels
        bpy.ops.object.join()
        obj = bpy.context.object
        obj.name = self.name
        # bake the surviving object transform into the verts — the client
        # loader reads raw POSITION data and ignores node transforms
        bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
        if subdivide > 0:
            # densify so vertex-color AO has something to land on
            mod = obj.modifiers.new("sub", "SUBSURF")
            mod.subdivision_type = "SIMPLE"
            mod.levels = subdivide
            bpy.ops.object.convert(target="MESH")
        return obj


def bake_ao(obj, samples=24, floor=0.45, protect_white=False):
    """Cycles AO baked into a temp color attribute, multiplied into Col.

    protect_white: leave pure-white verts untouched — the client's
    `bake_team` recolors exact (1,1,1,1) verts to the owner's color, so AO
    on those would break team tinting.
    """
    scene = bpy.context.scene
    scene.render.engine = "CYCLES"
    scene.cycles.device = "CPU"
    scene.cycles.samples = samples
    mesh = obj.data
    ao = mesh.color_attributes.new(name="AO", type="FLOAT_COLOR", domain="POINT")
    mesh.color_attributes.active_color_index = mesh.color_attributes.find("AO")
    for o in bpy.context.selected_objects:
        o.select_set(False)
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.bake(type="AO", target="VERTEX_COLORS")
    col = mesh.color_attributes["Col"]
    ao = mesh.color_attributes["AO"]
    for i, d in enumerate(col.data):
        r, g, b, a = d.color
        if protect_white and r == 1.0 and g == 1.0 and b == 1.0:
            continue
        k = floor + (1.0 - floor) * ao.data[i].color[0]
        d.color = (r * k, g * k, b * k, a)
    mesh.color_attributes.active_color_index = mesh.color_attributes.find("AO")
    mesh.color_attributes.remove(mesh.color_attributes["AO"])
    mesh.color_attributes.active_color_index = mesh.color_attributes.find("Col")


def rebase_origin(obj, pivot):
    """Move the object's origin to `pivot` (world): verts become
    pivot-relative, object location becomes the pivot — exactly the client's
    RigPart contract (child entity at pivot, mesh verts pivot-relative)."""
    px, py, pz = pivot
    for v in obj.data.vertices:
        v.co.x -= px
        v.co.y -= py
        v.co.z -= pz
    obj.location = (px, py, pz)


def export_glb_parts(objs, path):
    """Multi-node GLB: one node per part object (name + origin preserved)."""
    for o in bpy.context.selected_objects:
        o.select_set(False)
    for o in objs:
        o.select_set(True)
    bpy.context.view_layer.objects.active = objs[0]
    bpy.ops.export_scene.gltf(
        filepath=str(path),
        export_format="GLB",
        use_selection=True,
        export_materials="NONE",
        export_vertex_color="ACTIVE",
        export_yup=True,
    )


def export_glb(obj, path):
    for o in bpy.context.selected_objects:
        o.select_set(False)
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.export_scene.gltf(
        filepath=str(path),
        export_format="GLB",
        use_selection=True,
        export_materials="NONE",
        export_vertex_color="ACTIVE",
        export_yup=True,
    )


def render_sheet(objs, path, cell=6.0, res=1600, park_x=1000.0):
    """Grid the finished objects and render one iso contact sheet.

    Entries may be lists (multi-part rigs) — those move as one group.
    Every entry is expected to have been parked at +park_x after its bake;
    the shift back is additive so part pivots stay intact."""
    import bpy

    cols = max(1, math.ceil(math.sqrt(len(objs))))
    rows = math.ceil(len(objs) / cols)
    for i, entry in enumerate(objs):
        group = entry if isinstance(entry, list) else [entry]
        dx = (i % cols) * cell - park_x
        dy = -(i // cols) * cell
        for obj in group:
            obj.location.x += dx
            obj.location.y += dy

    cx = (cols - 1) * cell / 2
    cy = -(rows - 1) * cell / 2
    span = max(cols, rows) * cell

    bpy.ops.object.camera_add(
        location=(cx + span, cy - span, span * 0.9),
        rotation=(math.radians(60), 0, math.radians(45)),
    )
    cam = bpy.context.object
    cam.data.type = "ORTHO"
    cam.data.ortho_scale = span * 1.25
    bpy.context.scene.camera = cam
    bpy.ops.object.light_add(type="SUN", rotation=(math.radians(40), math.radians(20), 0))
    bpy.context.object.data.energy = 3.0

    scene = bpy.context.scene
    scene.render.engine = "BLENDER_WORKBENCH"
    scene.view_settings.view_transform = "Standard"
    scene.display.shading.light = "FLAT"
    scene.display.shading.color_type = "VERTEX"
    scene.display.shading.show_shadows = True
    scene.display.shading.show_cavity = True
    scene.display.shading.cavity_type = "BOTH"
    scene.render.resolution_x = res
    scene.render.resolution_y = int(res * rows / cols)
    scene.render.filepath = str(path)
    bpy.ops.render.render(write_still=True)
