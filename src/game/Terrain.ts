// Biome heightmap, built as a grid of chunk meshes (not one giant mesh) so the
// renderer frustum-culls off-screen chunks and we can swap per-chunk LOD later.
// Same seed the module used → what we draw matches where the server placed land.
//
// RENDER DRESSING ONLY. The vertex positions come straight from renderHeight()
// (shared with the module's view of where land is) — we never move a vertex. All
// the polish below is vertex COLOUR: a biome base blended toward its shade by how
// high the tile sits, a directional slope tint so sun-facing faces lighten, a
// crisp foam line at the waterline, and a faint deterministic hash dither so the
// low-poly facets don't read as flat colour fields. None of it touches the
// heightmap the client/module share.
import * as THREE from 'three';
import {
  WORLD_SIZE,
  sampleTerrain,
  renderHeight,
  BIOME_COLOR,
  BIOME_SHADE,
  Biome,
  hash2,
  biasOf,
  NEUTRAL_BIAS,
  type MapBias,
} from '../../shared/index.ts';

export interface TerrainBuild {
  group: THREE.Group; // contains all chunk meshes (named 'ground')
}

const CHUNK = 24; // tiles per chunk side

export function terrainHeight(
  seed: number,
  x: number,
  z: number,
  bias: MapBias = NEUTRAL_BIAS
): number {
  const s = sampleTerrain(seed, x, z);
  return renderHeight(s.height, s.biome, bias);
}

// Sun azimuth used only for the cheap per-vertex slope tint below (matches the
// DirectionalLight in SaladinGame so lit faces agree with the real shading).
const SUN = new THREE.Vector3(40, 70, 20).normalize();

// Pale sand the coastline fades to right at the waterline, so the shore reads as
// a crisp bright strip instead of a muddy band.
const FOAM = new THREE.Color('#efe4bf');
const SNOW_TINT = new THREE.Color('#f4f8fb');

const _base = new THREE.Color();
const _shade = new THREE.Color();

// Blend the biome's base toward its shade by how far this vertex sits above its
// band's floor, then layer the shore/snow accents. `hNorm` is the raw 0..1 field
// height; `relY` is the world render height used for the elevation darkening.
function biomeColor(out: THREE.Color, biome: number, hNorm: number, relY: number): void {
  _base.setHex(BIOME_COLOR[biome as 0]);
  _shade.setHex(BIOME_SHADE[biome as 0]);
  // Elevation darkening: gentle on plains, stronger as relief climbs.
  const elev = Math.max(0, Math.min(1, relY * 0.045));
  out.copy(_base).lerp(_shade, elev);

  // Snow caps brighten toward fresh-snow white as they climb.
  if (biome === Biome.Snow) {
    out.lerp(SNOW_TINT, Math.max(0, Math.min(1, (hNorm - 0.82) * 4)));
  }
  // A bright foam strip on the first sliver of beach above the waterline.
  if (biome === Biome.Sand) {
    const beach = 1 - Math.min(1, Math.abs(hNorm - 0.4) * 18);
    if (beach > 0) out.lerp(FOAM, beach * 0.5);
  }
}

const _n = new THREE.Vector3();
function buildChunk(
  seed: number,
  bias: MapBias,
  ox: number,
  oz: number,
  size: number,
  material: THREE.Material
): THREE.Mesh {
  const V = size + 1;
  const positions = new Float32Array(V * V * 3);
  const colors = new Float32Array(V * V * 3);
  const c = new THREE.Color();

  for (let j = 0; j < V; j++) {
    for (let i = 0; i < V; i++) {
      const x = ox + i;
      const z = oz + j;
      const s = sampleTerrain(seed, x, z);
      const y = renderHeight(s.height, s.biome, bias);
      const idx = (j * V + i) * 3;
      positions[idx] = x;
      positions[idx + 1] = y;
      positions[idx + 2] = z;

      biomeColor(c, s.biome, s.height, y);

      // Directional slope tint: sample neighbour render-heights to get a coarse
      // surface normal, then lighten faces tilted toward the sun and shade those
      // facing away. Cheap finite differences — peaks pop, valleys sink.
      const sx = sampleTerrain(seed, x + 1, z);
      const sz = sampleTerrain(seed, x, z + 1);
      const hx = renderHeight(sx.height, sx.biome, bias);
      const hz = renderHeight(sz.height, sz.biome, bias);
      _n.set(y - hx, 1, y - hz).normalize();
      const lit = _n.dot(SUN); // -1..1
      const shadeMul = 0.86 + Math.max(-1, Math.min(1, lit)) * 0.16;

      // Deterministic per-vertex dither so flat facets get a touch of grain.
      const dither = (hash2(x, z, seed ^ 0x5eed) - 0.5) * 0.05;

      const m = Math.max(0.55, shadeMul + dither);
      colors[idx] = c.r * m;
      colors[idx + 1] = c.g * m;
      colors[idx + 2] = c.b * m;
    }
  }

  const indices: number[] = [];
  for (let j = 0; j < size; j++) {
    for (let i = 0; i < size; i++) {
      const a = j * V + i;
      const b = a + 1;
      const d = a + V;
      const e = d + 1;
      indices.push(a, d, b, b, d, e);
    }
  }

  const geo = new THREE.BufferGeometry();
  geo.setAttribute('position', new THREE.BufferAttribute(positions, 3));
  geo.setAttribute('color', new THREE.BufferAttribute(colors, 3));
  geo.setIndex(indices);
  geo.computeVertexNormals();

  const mesh = new THREE.Mesh(geo, material);
  mesh.name = 'ground';
  mesh.receiveShadow = true;
  mesh.castShadow = true;
  return mesh;
}

export function buildTerrain(seed: number, presetId = 'continental'): TerrainBuild {
  const bias = biasOf(presetId);
  // One shared material — flat shading gives the stylized low-poly facet look.
  const material = new THREE.MeshStandardMaterial({
    vertexColors: true,
    roughness: 0.97,
    flatShading: true,
  });

  const group = new THREE.Group();
  for (let cz = 0; cz < WORLD_SIZE; cz += CHUNK) {
    for (let cx = 0; cx < WORLD_SIZE; cx += CHUNK) {
      const size = Math.min(CHUNK, WORLD_SIZE - cx, WORLD_SIZE - cz);
      group.add(buildChunk(seed, bias, cx, cz, size, material));
    }
  }
  return { group };
}
