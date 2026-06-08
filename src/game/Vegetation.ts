// Cosmetic vegetation + props, scattered deterministically from the seed + the
// biome catalog. ZERO database rows — this is pure client dressing recomputed
// from the same seeded worldgen the module agrees on, so it never desyncs and
// never costs a subscription. Resource trees/rocks/forage are separate DB nodes;
// these are the shrubs, palms, dune grass, rocks and boulders around them.
//
// Each Decoration kind becomes one InstancedMesh (one draw call) so thousands of
// props stay cheap on the 144² map. Placement: a per-tile hash decides whether a
// prop drops, biome density is the accept-probability, and a second hash jitters
// position/scale/rotation so the field doesn't read as a grid.
import * as THREE from 'three';
import {
  WORLD_SIZE,
  sampleTerrain,
  hash2,
  mixSeed,
  Decoration,
  biomeDecoration,
  biasOf,
  type MapBias,
} from '../../shared/index.ts';
import { terrainHeight } from './Terrain.ts';

export interface VegetationBuild {
  group: THREE.Group;
}

// A reusable mesh template per decoration kind: geometry + material, merged into
// a single child group so an InstancedMesh can carry multi-part props (e.g. palm
// trunk + fronds) by instancing each part with the same transform.
interface PropTemplate {
  parts: Array<{ geo: THREE.BufferGeometry; mat: THREE.Material }>;
  baseScale: number; // nominal world height, jittered per instance
}

function mat(color: number, opts: THREE.MeshStandardMaterialParameters = {}) {
  return new THREE.MeshStandardMaterial({ color, flatShading: true, roughness: 0.95, ...opts });
}

function shrubTemplate(): PropTemplate {
  return {
    parts: [{ geo: new THREE.IcosahedronGeometry(0.32, 0), mat: mat(0x6e7d3a) }],
    baseScale: 1,
  };
}

function duneGrassTemplate(): PropTemplate {
  // A low flat tuft — a squashed cone reads as a clump of grass.
  return {
    parts: [{ geo: new THREE.ConeGeometry(0.22, 0.5, 4), mat: mat(0xb7a55c) }],
    baseScale: 1,
  };
}

function rockTemplate(): PropTemplate {
  return {
    parts: [{ geo: new THREE.DodecahedronGeometry(0.3, 0), mat: mat(0x8a8175) }],
    baseScale: 1,
  };
}

function boulderTemplate(): PropTemplate {
  return {
    parts: [{ geo: new THREE.DodecahedronGeometry(0.55, 0), mat: mat(0x9aa0a6) }],
    baseScale: 1,
  };
}

function reedsTemplate(): PropTemplate {
  return {
    parts: [{ geo: new THREE.CylinderGeometry(0.04, 0.06, 0.9, 4), mat: mat(0x7d8a4a) }],
    baseScale: 1,
  };
}

// A palm: a slim leaning trunk under a flat green crown.
function palmTemplate(): PropTemplate {
  const trunk = new THREE.CylinderGeometry(0.07, 0.1, 1.5, 5);
  trunk.translate(0, 0.75, 0);
  const crown = new THREE.ConeGeometry(0.75, 0.45, 6);
  crown.translate(0, 1.6, 0);
  return {
    parts: [
      { geo: trunk, mat: mat(0x7a5a32) },
      { geo: crown, mat: mat(0x3f8f49) },
    ],
    baseScale: 1,
  };
}

// A small cosmetic conifer to thicken the forest floor between resource trees.
function pineTemplate(): PropTemplate {
  const trunk = new THREE.CylinderGeometry(0.08, 0.11, 0.5, 5);
  trunk.translate(0, 0.25, 0);
  const foliage = new THREE.ConeGeometry(0.5, 1.3, 6);
  foliage.translate(0, 1.05, 0);
  return {
    parts: [
      { geo: trunk, mat: mat(0x5b4127) },
      { geo: foliage, mat: mat(0x2f6b30) },
    ],
    baseScale: 1,
  };
}

function templateFor(kind: number): PropTemplate | null {
  switch (kind) {
    case Decoration.Shrub:
      return shrubTemplate();
    case Decoration.DuneGrass:
      return duneGrassTemplate();
    case Decoration.Rock:
      return rockTemplate();
    case Decoration.Boulder:
      return boulderTemplate();
    case Decoration.Reeds:
      return reedsTemplate();
    case Decoration.Palm:
      return palmTemplate();
    case Decoration.PineCluster:
      return pineTemplate();
    default:
      return null;
  }
}

const KINDS = [
  Decoration.Shrub,
  Decoration.DuneGrass,
  Decoration.Rock,
  Decoration.Boulder,
  Decoration.Reeds,
  Decoration.Palm,
  Decoration.PineCluster,
];

// Collect jittered transforms per decoration kind by walking the tile grid. A
// tile may host at most one prop; biome density is the accept threshold.
function collectTransforms(
  seed: number,
  bias: MapBias
): Map<number, THREE.Matrix4[]> {
  const byKind = new Map<number, THREE.Matrix4[]>();
  for (const k of KINDS) byKind.set(k, []);

  const m = new THREE.Matrix4();
  const q = new THREE.Quaternion();
  const pos = new THREE.Vector3();
  const scl = new THREE.Vector3();
  const up = new THREE.Vector3(0, 1, 0);

  for (let ty = 1; ty < WORLD_SIZE - 1; ty++) {
    for (let tx = 1; tx < WORLD_SIZE - 1; tx++) {
      const s = sampleTerrain(seed, tx + 0.5, ty + 0.5);
      const dec = biomeDecoration(s.biome);
      if (dec.kind === Decoration.None || dec.density <= 0) continue;
      // Independent hash stream per decoration so kinds don't correlate.
      const roll = hash2(tx, ty, mixSeed(seed, 7000 + dec.kind));
      if (roll >= dec.density) continue;

      const list = byKind.get(dec.kind);
      if (!list) continue;

      // Jitter inside the tile + random yaw + scale variation, all from hashes.
      const jx = hash2(tx, ty, mixSeed(seed, 8101 + dec.kind));
      const jy = hash2(tx, ty, mixSeed(seed, 8203 + dec.kind));
      const jr = hash2(tx, ty, mixSeed(seed, 8307 + dec.kind));
      const js = hash2(tx, ty, mixSeed(seed, 8419 + dec.kind));
      const wx = tx + 0.2 + jx * 0.6;
      const wz = ty + 0.2 + jy * 0.6;
      const wy = terrainHeight(seed, wx, wz, bias);
      const scale = 0.75 + js * 0.6;

      pos.set(wx, wy, wz);
      q.setFromAxisAngle(up, jr * Math.PI * 2);
      scl.set(scale, scale, scale);
      m.compose(pos, q, scl);
      list.push(m.clone());
    }
  }
  return byKind;
}

export function buildVegetation(
  seed: number,
  presetId = 'continental'
): VegetationBuild {
  const group = new THREE.Group();
  group.name = 'vegetation';
  if (!seed) return { group };

  const bias = biasOf(presetId);
  const byKind = collectTransforms(seed, bias);

  for (const [kind, transforms] of byKind) {
    if (transforms.length === 0) continue;
    const tpl = templateFor(kind);
    if (!tpl) continue;
    // One InstancedMesh per part keeps the multi-material props (palm, pine) as
    // few draw calls as their part count — still one call per part for the whole
    // map, regardless of how many props there are.
    for (const part of tpl.parts) {
      const inst = new THREE.InstancedMesh(part.geo, part.mat, transforms.length);
      inst.castShadow = true;
      inst.receiveShadow = false;
      transforms.forEach((mtx, i) => inst.setMatrixAt(i, mtx));
      inst.instanceMatrix.needsUpdate = true;
      inst.frustumCulled = false; // props span the whole map; matrices are baked
      group.add(inst);
    }
  }
  return { group };
}
