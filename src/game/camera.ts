import * as THREE from 'three';
import { WORLD_SIZE } from '../../shared/index.ts';

// Position an orthographic iso camera looking at `center` from a fixed offset.
export function placeCamera(
  camera: THREE.OrthographicCamera,
  center: THREE.Vector3,
  offset: THREE.Vector3
) {
  camera.position.copy(center).add(offset);
  camera.lookAt(center);
}

// Resize the orthographic frustum to the container aspect at the given zoom.
export function applyProjection(
  camera: THREE.OrthographicCamera,
  viewSize: number,
  width: number,
  height: number
) {
  const aspect = width / Math.max(1, height);
  camera.left = -viewSize * aspect;
  camera.right = viewSize * aspect;
  camera.top = viewSize;
  camera.bottom = -viewSize;
  camera.updateProjectionMatrix();
}

// Iso-screen WASD/arrow pan. Mutates `center` in place, clamped to the world,
// then re-aims the camera. Returns true if the camera moved.
export function panCamera(
  camera: THREE.OrthographicCamera,
  center: THREE.Vector3,
  offset: THREE.Vector3,
  viewSize: number,
  keys: Set<string>,
  dt: number
): boolean {
  const sp = viewSize * 1.6 * dt;
  let dx = 0;
  let dz = 0;
  if (keys.has('w') || keys.has('arrowup')) dz -= 1;
  if (keys.has('s') || keys.has('arrowdown')) dz += 1;
  if (keys.has('a') || keys.has('arrowleft')) dx -= 1;
  if (keys.has('d') || keys.has('arrowright')) dx += 1;
  if (dx === 0 && dz === 0) return false;
  center.x += (dx + dz) * sp;
  center.z += (dz - dx) * sp;
  center.x = Math.max(-10, Math.min(WORLD_SIZE + 10, center.x));
  center.z = Math.max(-10, Math.min(WORLD_SIZE + 10, center.z));
  placeCamera(camera, center, offset);
  return true;
}
