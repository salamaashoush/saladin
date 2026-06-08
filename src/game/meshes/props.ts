import * as THREE from 'three';
import { RESOURCE_DEFS, ResourceType } from '../../../shared/index.ts';

export const BAR_W = 0.8;
export const BAR_H = 0.12;

// Wood — a conifer: green cone over a brown trunk.
function buildTree(): THREE.Group {
  const g = new THREE.Group();
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
  g.add(foliage, trunk);
  return g;
}

// Stone — a low, faceted grey boulder.
function buildRock(): THREE.Group {
  const g = new THREE.Group();
  const rock = new THREE.Mesh(
    new THREE.DodecahedronGeometry(0.55, 0),
    new THREE.MeshStandardMaterial({
      color: RESOURCE_DEFS[ResourceType.Stone].color,
      flatShading: true,
    })
  );
  rock.position.y = 0.4;
  rock.scale.y = 0.7;
  rock.castShadow = true;
  g.add(rock);
  return g;
}

// Food — a golden grain/forage tuft: a squat amber cylinder.
function buildForage(): THREE.Group {
  const g = new THREE.Group();
  const tuft = new THREE.Mesh(
    new THREE.CylinderGeometry(0.45, 0.5, 0.5, 8),
    new THREE.MeshStandardMaterial({
      color: RESOURCE_DEFS[ResourceType.Food].color,
    })
  );
  tuft.position.y = 0.32;
  tuft.castShadow = true;
  g.add(tuft);
  return g;
}

// Gold — a glinting octahedral vein.
function buildGoldVein(): THREE.Group {
  const g = new THREE.Group();
  const vein = new THREE.Mesh(
    new THREE.OctahedronGeometry(0.45, 0),
    new THREE.MeshStandardMaterial({
      color: RESOURCE_DEFS[ResourceType.Gold].color,
      metalness: 0.6,
      roughness: 0.3,
      flatShading: true,
    })
  );
  vein.position.y = 0.45;
  vein.castShadow = true;
  g.add(vein);
  return g;
}

// Build the mesh for a resource node by its resType. Distinct shape + color per
// resource so a player reads the map at a glance.
export function buildResourceNode(resType: number): THREE.Group {
  switch (resType) {
    case ResourceType.Stone:
      return buildRock();
    case ResourceType.Food:
      return buildForage();
    case ResourceType.Gold:
      return buildGoldVein();
    default:
      return buildTree();
  }
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
