import * as THREE from "three";
import { BAR_W, BAR_H } from "./props.ts";

// Floating HP bars for every damaged unit, drawn as TWO InstancedMeshes (a dark
// backing + a coloured fill) instead of two THREE.Sprites per unit. That collapses
// up to 2×N sprite draws into a constant 2 draws, keeping the bars off the
// draw-call curve while preserving the exact look: a thin bar above the unit,
// green→yellow→red by ratio, hidden at full health.
//
// The bars are camera-facing quads. The iso camera's orientation is fixed, so the
// quad billboard can be oriented once from the camera basis each frame (cheap) and
// the same orientation reused for every instance.

interface BarSlot {
  id: string;
  x: number;
  y: number; // bar centre world Y (already offset above the unit)
  z: number;
  ratio: number; // 0..1, <1 to be visible
}

const _m = new THREE.Matrix4();
const _pos = new THREE.Vector3();
const _scl = new THREE.Vector3();
const _q = new THREE.Quaternion();
const _color = new THREE.Color();
const _basis = new THREE.Matrix4();

const GREEN = 0x33dd44;
const YELLOW = 0xddcc33;
const RED = 0xdd3333;

export class InstancedHpBars {
  private readonly scene: THREE.Scene;
  private bg: THREE.InstancedMesh;
  private fg: THREE.InstancedMesh;
  private capacity = 64;
  private readonly slots = new Map<string, BarSlot>();
  private readonly quad: THREE.PlaneGeometry;
  private dirty = true;

  constructor(scene: THREE.Scene) {
    this.scene = scene;
    this.quad = new THREE.PlaneGeometry(1, 1);
    this.bg = this.makeMesh(0x2a0000, this.capacity, 3);
    this.fg = this.makeMesh(0xffffff, this.capacity, 4); // tinted per instance
    this.scene.add(this.bg, this.fg);
  }

  private makeMesh(
    color: number,
    cap: number,
    renderOrder: number,
  ): THREE.InstancedMesh {
    const mat = new THREE.MeshBasicMaterial({
      color,
      depthTest: false,
      transparent: true,
    });
    const m = new THREE.InstancedMesh(this.quad, mat, cap);
    m.frustumCulled = false;
    m.renderOrder = renderOrder;
    m.count = 0;
    m.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
    return m;
  }

  private grow(need: number) {
    let cap = this.capacity;
    while (cap < need) cap *= 2;
    this.scene.remove(this.bg, this.fg);
    this.bg.dispose();
    this.fg.dispose();
    this.bg = this.makeMesh(0x2a0000, cap, 3);
    this.fg = this.makeMesh(0xffffff, cap, 4);
    this.scene.add(this.bg, this.fg);
    this.capacity = cap;
    this.dirty = true;
  }

  // Add or update a unit's HP bar. ratio>=1 (full health) removes it.
  set(id: string, x: number, y: number, z: number, ratio: number) {
    if (ratio >= 0.999) {
      if (this.slots.delete(id)) this.dirty = true;
      return;
    }
    const s = this.slots.get(id);
    if (s) {
      s.x = x;
      s.y = y;
      s.z = z;
      s.ratio = ratio;
    } else {
      this.slots.set(id, { id, x, y, z, ratio });
    }
    this.dirty = true;
  }

  remove(id: string) {
    if (this.slots.delete(id)) this.dirty = true;
  }

  // Rebuild the instance buffers from the live slots, billboarded to the camera.
  // Called each frame; cheap when nothing changed (early-out on !dirty unless the
  // camera basis must be refreshed — we always refresh because units move).
  update(camera: THREE.Camera) {
    const n = this.slots.size;
    if (n > this.capacity) this.grow(n);

    // Shared billboard orientation from the camera basis (right + up vectors).
    _basis.copy(camera.matrixWorld);
    _q.setFromRotationMatrix(_basis);

    let i = 0;
    for (const s of this.slots.values()) {
      // Backing bar: full width.
      _pos.set(s.x, s.y, s.z);
      _scl.set(BAR_W, BAR_H, 1);
      _m.compose(_pos, _q, _scl);
      this.bg.setMatrixAt(i, _m);

      // Fill bar: width scaled by ratio, left-anchored like the sprite version.
      const fillW = BAR_W * s.ratio;
      // Shift left by half the missing width, along the camera-right axis.
      _pos
        .set(s.x, s.y, s.z)
        .add(
          new THREE.Vector3(1, 0, 0)
            .applyQuaternion(_q)
            .multiplyScalar(-(BAR_W * (1 - s.ratio)) / 2),
        );
      // Nudge toward the camera so the fill sits in front of the backing.
      _pos.add(
        new THREE.Vector3(0, 0, 1).applyQuaternion(_q).multiplyScalar(0.002),
      );
      _scl.set(fillW, BAR_H, 1);
      _m.compose(_pos, _q, _scl);
      this.fg.setMatrixAt(i, _m);
      _color.setHex(s.ratio > 0.5 ? GREEN : s.ratio > 0.25 ? YELLOW : RED);
      this.fg.setColorAt(i, _color);
      i++;
    }
    this.bg.count = n;
    this.fg.count = n;
    this.bg.instanceMatrix.needsUpdate = true;
    this.fg.instanceMatrix.needsUpdate = true;
    if (this.fg.instanceColor) this.fg.instanceColor.needsUpdate = true;
    this.dirty = false;
  }

  dispose() {
    this.scene.remove(this.bg, this.fg);
    this.bg.dispose();
    this.fg.dispose();
    this.quad.dispose();
    this.slots.clear();
  }
}
