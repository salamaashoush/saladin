import * as THREE from "three";
import { UNIT_DEFS, UnitKind } from "../../../shared/index.ts";
import { buildUnit } from "./units.ts";
import {
  bakeGroup,
  instancedTintMaterial,
  type BakedGeometry,
} from "./bake.ts";
import { buildUnitImpostor } from "./unitImpostor.ts";

// One InstancedMesh per unit kind (plus a low-detail impostor variant) so every
// unit of a kind draws in a single call, with the faction tint carried per
// instance via instanceColor. The per-instance transform (ground position +
// facing yaw + idle bob) is rewritten each frame from the interpolated unit pool.
//
// Add/remove compacts the instance buffer: a removed slot is back-filled by the
// last live instance so the dense [0..count) range stays drawable. Selection
// rings + hp bars are NOT instanced here — they live as cheap per-unit overlays
// on the owning RObj group, unchanged from before; only the body is instanced.

const SENTINEL = 0xdddddd; // neutral tint baked into the kind template

interface KindBatch {
  full: THREE.InstancedMesh;
  lod: THREE.InstancedMesh; // low-detail impostor, same instance slots
  height: number;
  capacity: number;
  count: number;
  ids: string[]; // slot -> entityId
  slotOf: Map<string, number>; // entityId -> slot
  fullGeo: THREE.BufferGeometry;
  lodGeo: THREE.BufferGeometry;
  material: THREE.Material;
  dirtyMatrix: boolean;
  dirtyColor: boolean;
}

const _m = new THREE.Matrix4();
const _q = new THREE.Quaternion();
const _pos = new THREE.Vector3();
const _scl = new THREE.Vector3(1, 1, 1);
const _up = new THREE.Vector3(0, 1, 0);
const _color = new THREE.Color();

const INITIAL_CAP = 64;

export class InstancedUnits {
  private readonly batches = new Map<number, KindBatch>();
  private readonly scene: THREE.Scene;
  // Far-zoom LOD switch: above this orthographic half-height the impostor draws.
  private lodViewSize = 46;
  private lodActive = false;

  constructor(scene: THREE.Scene) {
    this.scene = scene;
  }

  private bakeKind(kind: number): { full: BakedGeometry; lod: BakedGeometry } {
    const proto = buildUnit(kind, SENTINEL);
    const tintMat = proto.userData.tintMat as THREE.Material | undefined;
    const full = bakeGroup(proto, tintMat);
    const impostor = buildUnitImpostor(kind, SENTINEL);
    const lod = bakeGroup(
      impostor,
      impostor.userData.tintMat as THREE.Material | undefined,
    );
    return { full, lod };
  }

  private ensureBatch(kind: number): KindBatch {
    let b = this.batches.get(kind);
    if (b) return b;
    const { full, lod } = this.bakeKind(kind);
    const material = instancedTintMaterial();
    const cap = INITIAL_CAP;

    const fullMesh = new THREE.InstancedMesh(full.geometry, material, cap);
    fullMesh.castShadow = true;
    fullMesh.receiveShadow = false;
    fullMesh.frustumCulled = false; // we cull per-instance via the matrix range
    fullMesh.count = 0;
    fullMesh.instanceMatrix.setUsage(THREE.DynamicDrawUsage);

    const lodMesh = new THREE.InstancedMesh(lod.geometry, material, cap);
    lodMesh.castShadow = false;
    lodMesh.receiveShadow = false;
    lodMesh.frustumCulled = false;
    lodMesh.count = 0;
    lodMesh.visible = false;
    lodMesh.instanceMatrix.setUsage(THREE.DynamicDrawUsage);

    b = {
      full: fullMesh,
      lod: lodMesh,
      height: full.height,
      capacity: cap,
      count: 0,
      ids: [],
      slotOf: new Map(),
      fullGeo: full.geometry,
      lodGeo: lod.geometry,
      material,
      dirtyMatrix: true,
      dirtyColor: true,
    };
    this.batches.set(kind, b);
    this.scene.add(fullMesh, lodMesh);
    return b;
  }

  // Grow both instanced meshes (geometry + material shared) to a larger capacity,
  // copying existing instance matrices + colours over.
  private grow(b: KindBatch, need: number) {
    let cap = b.capacity;
    while (cap < need) cap *= 2;
    const newFull = new THREE.InstancedMesh(b.fullGeo, b.material, cap);
    const newLod = new THREE.InstancedMesh(b.lodGeo, b.material, cap);
    for (const [src, dst] of [
      [b.full, newFull],
      [b.lod, newLod],
    ] as const) {
      dst.castShadow = src.castShadow;
      dst.receiveShadow = false;
      dst.frustumCulled = false;
      dst.visible = src.visible;
      dst.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
      // Copy existing per-instance matrices + colours.
      for (let i = 0; i < b.count; i++) {
        src.getMatrixAt(i, _m);
        dst.setMatrixAt(i, _m);
      }
      if (src.instanceColor) {
        for (let i = 0; i < b.count; i++) {
          src.getColorAt(i, _color);
          dst.setColorAt(i, _color);
        }
      }
      dst.count = b.count;
    }
    this.scene.remove(b.full, b.lod);
    b.full.dispose();
    b.lod.dispose();
    this.scene.add(newFull, newLod);
    newFull.visible = !this.lodActive;
    newLod.visible = this.lodActive;
    b.full = newFull;
    b.lod = newLod;
    b.capacity = cap;
  }

  has(kind: number, id: string): boolean {
    return this.batches.get(kind)?.slotOf.has(id) ?? false;
  }

  // Register a unit; returns its instance slot. The transform/colour are written
  // on the next sync() once the live position is known.
  add(kind: number, id: string): number {
    const b = this.ensureBatch(kind);
    const existing = b.slotOf.get(id);
    if (existing !== undefined) return existing;
    if (b.count + 1 > b.capacity) this.grow(b, b.count + 1);
    const slot = b.count;
    b.ids[slot] = id;
    b.slotOf.set(id, slot);
    b.count++;
    b.full.count = b.count;
    b.lod.count = b.count;
    b.dirtyMatrix = true;
    b.dirtyColor = true;
    return slot;
  }

  // Remove a unit, compacting its slot with the last live instance so [0..count)
  // stays dense.
  remove(kind: number, id: string) {
    const b = this.batches.get(kind);
    if (!b) return;
    const slot = b.slotOf.get(id);
    if (slot === undefined) return;
    const last = b.count - 1;
    if (slot !== last) {
      const movedId = b.ids[last];
      b.full.getMatrixAt(last, _m);
      b.full.setMatrixAt(slot, _m);
      b.lod.getMatrixAt(last, _m);
      b.lod.setMatrixAt(slot, _m);
      if (b.full.instanceColor) {
        b.full.getColorAt(last, _color);
        b.full.setColorAt(slot, _color);
        b.lod.setColorAt(slot, _color);
      }
      b.ids[slot] = movedId;
      b.slotOf.set(movedId, slot);
    }
    b.ids.pop();
    b.slotOf.delete(id);
    b.count = last;
    b.full.count = b.count;
    b.lod.count = b.count;
    b.dirtyMatrix = true;
    b.dirtyColor = true;
  }

  setColor(kind: number, id: string, color: number) {
    const b = this.batches.get(kind);
    if (!b) return;
    const slot = b.slotOf.get(id);
    if (slot === undefined) return;
    _color.setHex(color);
    b.full.setColorAt(slot, _color);
    b.lod.setColorAt(slot, _color);
    b.dirtyColor = true;
  }

  // Write one instance's transform: ground position, facing yaw, idle bob height.
  // `visible` units render; hidden ones (garrisoned) collapse to a zero-scale
  // matrix so they cost no pixels without disturbing the dense slot range.
  setTransform(
    kind: number,
    id: string,
    x: number,
    y: number,
    z: number,
    facing: number,
    bobY: number,
    visible: boolean,
  ) {
    const b = this.batches.get(kind);
    if (!b) return;
    const slot = b.slotOf.get(id);
    if (slot === undefined) return;
    if (visible) {
      _q.setFromAxisAngle(_up, -facing);
      _pos.set(x, y + bobY, z);
      _scl.set(1, 1, 1);
    } else {
      _q.identity();
      _pos.set(x, y, z);
      _scl.set(0, 0, 0);
    }
    _m.compose(_pos, _q, _scl);
    b.full.setMatrixAt(slot, _m);
    b.lod.setMatrixAt(slot, _m);
    b.dirtyMatrix = true;
  }

  heightOf(kind: number): number {
    return (
      this.batches.get(kind)?.height ?? UNIT_DEFS[kind as UnitKind]?.height ?? 1
    );
  }

  // The currently-visible InstancedMesh per kind, tagged so a raycast hit can be
  // resolved back to an entityId via resolveHit(). Used by the pick path.
  pickMeshes(): THREE.InstancedMesh[] {
    const out: THREE.InstancedMesh[] = [];
    for (const [kind, b] of this.batches) {
      const m = this.lodActive ? b.lod : b.full;
      m.userData.instKind = kind;
      out.push(m);
    }
    return out;
  }

  // Map a raycast hit on one of the pick meshes to its entityId, or null.
  resolveHit(
    mesh: THREE.Object3D,
    instanceId: number | undefined,
  ): string | null {
    const kind = mesh.userData.instKind as number | undefined;
    if (kind === undefined || instanceId === undefined) return null;
    const b = this.batches.get(kind);
    if (!b || instanceId >= b.count) return null;
    return b.ids[instanceId] ?? null;
  }

  // Choose full vs impostor by orthographic zoom (the only LOD lever for an ortho
  // iso camera — distance is uniform across the frame). One toggle per kind.
  setViewSize(viewSize: number) {
    const wantLod = viewSize >= this.lodViewSize;
    if (wantLod === this.lodActive) return;
    this.lodActive = wantLod;
    for (const b of this.batches.values()) {
      b.full.visible = !wantLod;
      b.lod.visible = wantLod;
    }
  }

  // Flush dirty instance buffers once per frame (after all setTransform/setColor).
  flush() {
    for (const b of this.batches.values()) {
      if (b.dirtyMatrix) {
        b.full.instanceMatrix.needsUpdate = true;
        b.lod.instanceMatrix.needsUpdate = true;
        b.full.computeBoundingSphere();
        b.dirtyMatrix = false;
      }
      if (b.dirtyColor) {
        if (b.full.instanceColor) b.full.instanceColor.needsUpdate = true;
        if (b.lod.instanceColor) b.lod.instanceColor.needsUpdate = true;
        b.dirtyColor = false;
      }
    }
  }

  dispose() {
    for (const b of this.batches.values()) {
      this.scene.remove(b.full, b.lod);
      b.full.dispose();
      b.lod.dispose();
      b.fullGeo.dispose();
      b.lodGeo.dispose();
      b.material.dispose();
    }
    this.batches.clear();
  }
}
