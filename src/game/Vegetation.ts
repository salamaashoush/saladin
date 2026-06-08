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

// Merge several geometries that share a material into one buffer so a multi-lump
// prop (a two-blob shrub, a tiered pine) still instances as a single draw call.
function merge(...geos: THREE.BufferGeometry[]): THREE.BufferGeometry {
  const out = geos[0].clone();
  const positions: THREE.BufferAttribute[] = [];
  let total = 0;
  for (const g of geos) {
    const ng = g.index ? g.toNonIndexed() : g;
    positions.push(ng.getAttribute('position') as THREE.BufferAttribute);
    total += positions[positions.length - 1].count;
  }
  const arr = new Float32Array(total * 3);
  let o = 0;
  for (const p of positions) {
    arr.set(p.array as Float32Array, o);
    o += p.count * 3;
  }
  out.deleteAttribute('normal');
  out.deleteAttribute('uv');
  out.setIndex(null);
  out.setAttribute('position', new THREE.BufferAttribute(arr, 3));
  out.computeVertexNormals();
  return out;
}

// A two-lobe brush clump — a big blob with a smaller offset companion reads as a
// dry desert/steppe shrub rather than a lone ball.
function shrubTemplate(): PropTemplate {
  const a = new THREE.IcosahedronGeometry(0.3, 0);
  a.translate(0, 0.26, 0);
  const b = new THREE.IcosahedronGeometry(0.19, 0);
  b.translate(0.22, 0.16, 0.1);
  return { parts: [{ geo: merge(a, b), mat: mat(0x6e7d3a) }], baseScale: 1 };
}

// A fan of a few thin blades rather than one cone — a sparse tuft of dune grass.
function duneGrassTemplate(): PropTemplate {
  const blades: THREE.BufferGeometry[] = [];
  const n = 4;
  for (let i = 0; i < n; i++) {
    const blade = new THREE.ConeGeometry(0.05, 0.5 + (i % 2) * 0.18, 3);
    blade.translate(0, 0.28, 0);
    const ang = (i / n) * Math.PI * 2;
    blade.rotateZ(0.28);
    blade.rotateY(ang);
    blade.translate(Math.cos(ang) * 0.07, 0, Math.sin(ang) * 0.07);
    blades.push(blade);
  }
  return { parts: [{ geo: merge(...blades), mat: mat(0xc2b06a) }], baseScale: 1 };
}

// A faceted loose stone — squashed a touch so it sits like a rock, not a ball.
function rockTemplate(): PropTemplate {
  const g = new THREE.DodecahedronGeometry(0.3, 0);
  g.scale(1, 0.7, 1);
  g.translate(0, 0.16, 0);
  return { parts: [{ geo: g, mat: mat(0x8a8175) }], baseScale: 1 };
}

// A big two-mass boulder for the high, cold biomes.
function boulderTemplate(): PropTemplate {
  const a = new THREE.DodecahedronGeometry(0.55, 0);
  a.scale(1, 0.8, 1);
  a.translate(0, 0.4, 0);
  const b = new THREE.IcosahedronGeometry(0.3, 0);
  b.translate(0.45, 0.18, 0.12);
  return { parts: [{ geo: merge(a, b), mat: mat(0x9aa0a6) }], baseScale: 1 };
}

// A small clump of marsh reeds at the shallows' edge — a few stalks of varied
// height capped with a darker seed head.
function reedsTemplate(): PropTemplate {
  const stalks: THREE.BufferGeometry[] = [];
  const heads: THREE.BufferGeometry[] = [];
  const n = 5;
  for (let i = 0; i < n; i++) {
    const h = 0.7 + (i % 3) * 0.22;
    const stalk = new THREE.CylinderGeometry(0.025, 0.04, h, 4);
    const dx = (i - (n - 1) / 2) * 0.09;
    stalk.translate(dx, h / 2, (i % 2) * 0.06);
    stalks.push(stalk);
    const head = new THREE.CylinderGeometry(0.05, 0.05, 0.16, 4);
    head.translate(dx, h, (i % 2) * 0.06);
    heads.push(head);
  }
  return {
    parts: [
      { geo: merge(...stalks), mat: mat(0x8a9a52) },
      { geo: merge(...heads), mat: mat(0x6b5a2e) },
    ],
    baseScale: 1,
  };
}

// A palm: a slim leaning trunk under a ring of drooping fronds.
function palmTemplate(): PropTemplate {
  const trunk = new THREE.CylinderGeometry(0.06, 0.1, 1.6, 5);
  trunk.translate(0, 0.8, 0);
  const fronds: THREE.BufferGeometry[] = [];
  const n = 6;
  for (let i = 0; i < n; i++) {
    const frond = new THREE.ConeGeometry(0.16, 0.9, 3);
    // Lay the frond outward and droop it down from the crown.
    frond.rotateX(Math.PI / 2);
    frond.translate(0, 0, 0.45);
    frond.rotateX(-0.5);
    const ang = (i / n) * Math.PI * 2;
    frond.rotateY(ang);
    frond.translate(0, 1.6, 0);
    fronds.push(frond);
  }
  const crown = new THREE.IcosahedronGeometry(0.13, 0);
  crown.translate(0, 1.62, 0);
  return {
    parts: [
      { geo: trunk, mat: mat(0x7a5a32) },
      { geo: merge(...fronds, crown), mat: mat(0x3f8f49) },
    ],
    baseScale: 1,
  };
}

// A small cosmetic conifer (stacked tiers) thickening the forest between trees.
function pineTemplate(): PropTemplate {
  const trunk = new THREE.CylinderGeometry(0.08, 0.11, 0.5, 5);
  trunk.translate(0, 0.25, 0);
  const tiers: THREE.BufferGeometry[] = [];
  const levels = [
    { r: 0.55, h: 0.8, y: 0.6 },
    { r: 0.42, h: 0.7, y: 1.05 },
    { r: 0.28, h: 0.6, y: 1.45 },
  ];
  for (const l of levels) {
    const cone = new THREE.ConeGeometry(l.r, l.h, 6);
    cone.translate(0, l.y, 0);
    tiers.push(cone);
  }
  return {
    parts: [
      { geo: trunk, mat: mat(0x5b4127) },
      { geo: merge(...tiers), mat: mat(0x2f6b30) },
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
  const qLean = new THREE.Quaternion();
  const pos = new THREE.Vector3();
  const scl = new THREE.Vector3();
  const up = new THREE.Vector3(0, 1, 0);
  const lean = new THREE.Vector3();

  // Rocks/boulders may tumble to any orientation; organic props only lean a few
  // degrees off vertical so they still read as growing up from the ground.
  const isRubble = (k: number) => k === Decoration.Rock || k === Decoration.Boulder;

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
      const jl = hash2(tx, ty, mixSeed(seed, 8527 + dec.kind));
      const jla = hash2(tx, ty, mixSeed(seed, 8629 + dec.kind));
      const jsx = hash2(tx, ty, mixSeed(seed, 8731 + dec.kind));
      const wx = tx + 0.2 + jx * 0.6;
      const wz = ty + 0.2 + jy * 0.6;
      const wy = terrainHeight(seed, wx, wz, bias);
      const scale = 0.75 + js * 0.6;

      pos.set(wx, wy, wz);
      q.setFromAxisAngle(up, jr * Math.PI * 2);
      // Lean: rubble tumbles fully, plants tip only slightly off vertical.
      const maxLean = isRubble(dec.kind) ? Math.PI * 0.5 : 0.22;
      lean
        .set(Math.cos(jla * Math.PI * 2), 0, Math.sin(jla * Math.PI * 2))
        .normalize();
      qLean.setFromAxisAngle(lean, jl * maxLean);
      q.multiply(qLean);
      // Slight non-uniform scale so no two instances are identical silhouettes.
      scl.set(scale * (0.9 + jsx * 0.2), scale * (0.92 + js * 0.16), scale * (0.9 + jx * 0.2));
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
