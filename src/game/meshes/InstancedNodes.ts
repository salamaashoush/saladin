import * as THREE from "three";
import { ResourceType } from "../../../shared/index.ts";
import { buildResourceNode } from "./props.ts";
import {
  bakeGroup,
  instancedVertexColorMaterial,
  type BakedGeometry,
} from "./bake.ts";

// One InstancedMesh per resource kind (tree/rock/forage/gold-vein, keyed by
// ResourceType) so every node of a kind draws in a single call regardless of
// count. Resource nodes never move and are never recoloured, so an instance's
// matrix is rewritten only on add (placement) and on scale changes (depletion
// shrinks the node) — never per frame, keeping this off the render loop entirely.
//
// Add/remove compacts the instance buffer: a removed slot is back-filled by the
// last live instance so the dense [0..count) range stays drawable. The instanceId
// -> entityId map lets a raycast hit resolve back to the node for right-click
// gather, mirroring InstancedUnits.

interface KindBatch {
  mesh: THREE.InstancedMesh;
  geo: THREE.BufferGeometry;
  material: THREE.Material;
  capacity: number;
  count: number;
  ids: string[]; // slot -> entityId
  slotOf: Map<string, number>; // entityId -> slot
  // Per-instance transform inputs, kept so a position OR scale change can be
  // recomposed without re-deriving the other.
  px: number[];
  py: number[];
  pz: number[];
  scale: number[];
  dirty: boolean;
}

// The resource kinds whose source meshes use flat shading; the rest keep their
// smooth baked normals (matches props.ts per-mesh material flags exactly).
const FLAT_SHADED = new Set<number>([ResourceType.Stone, ResourceType.Gold]);

const _m = new THREE.Matrix4();
const _q = new THREE.Quaternion();
const _pos = new THREE.Vector3();
const _scl = new THREE.Vector3();

const INITIAL_CAP = 64;

export class InstancedNodes {
  private readonly batches = new Map<number, KindBatch>();
  private readonly scene: THREE.Scene;

  constructor(scene: THREE.Scene) {
    this.scene = scene;
  }

  private bakeKind(resType: number): BakedGeometry {
    // Nodes carry no team tint: bake with no tint material so every part keeps
    // its fixed colour as a baked vertex colour (tintable=0 throughout).
    const proto = buildResourceNode(resType);
    return bakeGroup(proto, undefined);
  }

  private ensureBatch(resType: number): KindBatch {
    let b = this.batches.get(resType);
    if (b) return b;
    const baked = this.bakeKind(resType);
    const material = instancedVertexColorMaterial({
      flatShading: FLAT_SHADED.has(resType),
      ...(resType === ResourceType.Gold
        ? { metalness: 0.6, roughness: 0.3 }
        : {}),
    });
    const cap = INITIAL_CAP;
    const mesh = new THREE.InstancedMesh(baked.geometry, material, cap);
    mesh.castShadow = true;
    mesh.receiveShadow = false;
    mesh.frustumCulled = false; // culled per-instance via the matrix range
    mesh.count = 0;
    mesh.instanceMatrix.setUsage(THREE.DynamicDrawUsage);

    b = {
      mesh,
      geo: baked.geometry,
      material,
      capacity: cap,
      count: 0,
      ids: [],
      slotOf: new Map(),
      px: [],
      py: [],
      pz: [],
      scale: [],
      dirty: false,
    };
    this.batches.set(resType, b);
    this.scene.add(mesh);
    return b;
  }

  // Grow the instanced mesh (geometry + material shared) to a larger capacity,
  // copying existing instance matrices over.
  private grow(b: KindBatch, need: number) {
    let cap = b.capacity;
    while (cap < need) cap *= 2;
    const next = new THREE.InstancedMesh(b.geo, b.material, cap);
    next.castShadow = true;
    next.receiveShadow = false;
    next.frustumCulled = false;
    next.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
    for (let i = 0; i < b.count; i++) {
      b.mesh.getMatrixAt(i, _m);
      next.setMatrixAt(i, _m);
    }
    next.count = b.count;
    this.scene.remove(b.mesh);
    b.mesh.dispose();
    this.scene.add(next);
    b.mesh = next;
    b.capacity = cap;
  }

  has(resType: number, id: string): boolean {
    return this.batches.get(resType)?.slotOf.has(id) ?? false;
  }

  // Register a node and write its (static) transform immediately. resType is the
  // node's ResourceType; scale is the depletion-driven uniform scale.
  add(
    resType: number,
    id: string,
    x: number,
    y: number,
    z: number,
    scale: number,
  ): void {
    const b = this.ensureBatch(resType);
    const existing = b.slotOf.get(id);
    if (existing !== undefined) {
      this.write(b, existing, x, y, z, scale);
      return;
    }
    if (b.count + 1 > b.capacity) this.grow(b, b.count + 1);
    const slot = b.count;
    b.ids[slot] = id;
    b.slotOf.set(id, slot);
    b.count++;
    b.mesh.count = b.count;
    this.write(b, slot, x, y, z, scale);
  }

  // Remove a node, compacting its slot with the last live instance so [0..count)
  // stays dense.
  remove(resType: number, id: string): void {
    const b = this.batches.get(resType);
    if (!b) return;
    const slot = b.slotOf.get(id);
    if (slot === undefined) return;
    const last = b.count - 1;
    if (slot !== last) {
      const movedId = b.ids[last];
      b.px[slot] = b.px[last];
      b.py[slot] = b.py[last];
      b.pz[slot] = b.pz[last];
      b.scale[slot] = b.scale[last];
      b.mesh.getMatrixAt(last, _m);
      b.mesh.setMatrixAt(slot, _m);
      b.ids[slot] = movedId;
      b.slotOf.set(movedId, slot);
    }
    b.ids.pop();
    b.px.pop();
    b.py.pop();
    b.pz.pop();
    b.scale.pop();
    b.slotOf.delete(id);
    b.count = last;
    b.mesh.count = b.count;
    b.dirty = true;
  }

  // Update only the depletion scale of an existing node (position unchanged).
  setScale(resType: number, id: string, scale: number): void {
    const b = this.batches.get(resType);
    if (!b) return;
    const slot = b.slotOf.get(id);
    if (slot === undefined) return;
    this.write(b, slot, b.px[slot], b.py[slot], b.pz[slot], scale);
  }

  // Re-seat a node at a new ground height (terrain rebuilt) without disturbing its
  // scale. No-op if the node isn't in this batch.
  setHeight(resType: number, id: string, y: number): void {
    const b = this.batches.get(resType);
    if (!b) return;
    const slot = b.slotOf.get(id);
    if (slot === undefined) return;
    this.write(b, slot, b.px[slot], y, b.pz[slot], b.scale[slot]);
  }

  private write(
    b: KindBatch,
    slot: number,
    x: number,
    y: number,
    z: number,
    scale: number,
  ) {
    b.px[slot] = x;
    b.py[slot] = y;
    b.pz[slot] = z;
    b.scale[slot] = scale;
    _pos.set(x, y, z);
    _q.identity();
    _scl.set(scale, scale, scale);
    _m.compose(_pos, _q, _scl);
    b.mesh.setMatrixAt(slot, _m);
    b.dirty = true;
  }

  // The InstancedMesh per kind, tagged so a raycast hit can be resolved back to an
  // entityId via resolveHit(). Used by the pick path (right-click to gather).
  pickMeshes(): THREE.InstancedMesh[] {
    const out: THREE.InstancedMesh[] = [];
    for (const [resType, b] of this.batches) {
      if (b.count === 0) continue;
      b.mesh.userData.instNode = resType;
      out.push(b.mesh);
    }
    return out;
  }

  // Map a raycast hit on one of the pick meshes to its node entityId, or null.
  resolveHit(
    mesh: THREE.Object3D,
    instanceId: number | undefined,
  ): string | null {
    const resType = mesh.userData.instNode as number | undefined;
    if (resType === undefined || instanceId === undefined) return null;
    const b = this.batches.get(resType);
    if (!b || instanceId >= b.count) return null;
    return b.ids[instanceId] ?? null;
  }

  // Flush dirty instance buffers. Cheap: only fires on add/remove/scale, never
  // per frame, so the bounding sphere recompute (needed for shadows) is rare.
  flush(): void {
    for (const b of this.batches.values()) {
      if (!b.dirty) continue;
      b.mesh.instanceMatrix.needsUpdate = true;
      b.mesh.computeBoundingSphere();
      b.dirty = false;
    }
  }

  dispose(): void {
    for (const b of this.batches.values()) {
      this.scene.remove(b.mesh);
      b.mesh.dispose();
      b.geo.dispose();
      b.material.dispose();
    }
    this.batches.clear();
  }
}
