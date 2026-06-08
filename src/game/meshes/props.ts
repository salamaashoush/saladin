import * as THREE from 'three';
import { RESOURCE_DEFS, ResourceType } from '../../../shared/index.ts';

export const BAR_W = 0.8;
export const BAR_H = 0.12;

export function buildTree(): THREE.Mesh {
  const foliage = new THREE.Mesh(
    new THREE.ConeGeometry(0.6, 1.5, 7),
    new THREE.MeshStandardMaterial({
      color: RESOURCE_DEFS[ResourceType.Wood].color,
    })
  );
  foliage.position.y = 1.2;
  foliage.castShadow = true;
  const trunk = new THREE.Mesh(
    new THREE.CylinderGeometry(0.12, 0.16, 0.6, 6),
    new THREE.MeshStandardMaterial({ color: '#6b4a2b' })
  );
  trunk.position.y = 0.3;
  foliage.add(trunk);
  return foliage;
}

export function buildSelRing(r: number): THREE.Mesh {
  const ring = new THREE.Mesh(
    new THREE.RingGeometry(r * 1.25, r * 1.5, 28),
    new THREE.MeshBasicMaterial({
      color: 0x9bf06b,
      side: THREE.DoubleSide,
      depthTest: false,
      transparent: true,
      opacity: 0.9,
    })
  );
  ring.rotation.x = -Math.PI / 2;
  ring.position.y = 0.05;
  ring.renderOrder = 2;
  ring.visible = false;
  return ring;
}

export function buildHpBar(): THREE.Group {
  const g = new THREE.Group();
  const bg = new THREE.Sprite(
    new THREE.SpriteMaterial({ color: 0x2a0000, depthTest: false })
  );
  bg.scale.set(BAR_W, BAR_H, 1);
  bg.renderOrder = 3;
  const fg = new THREE.Sprite(
    new THREE.SpriteMaterial({ color: 0x33dd44, depthTest: false })
  );
  fg.scale.set(BAR_W, BAR_H, 1);
  fg.position.z = 0.001;
  fg.renderOrder = 4;
  g.add(bg);
  g.add(fg);
  g.userData.fg = fg;
  g.visible = false;
  return g;
}
