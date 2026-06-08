// Biome heightmap, built as a grid of chunk meshes (not one giant mesh) so the
// renderer frustum-culls off-screen chunks and we can swap per-chunk LOD later.
// Same seed the module used → what we draw matches where the server placed land.
//
// Render only: per-biome base color is blended toward a darker `shade` by how
// high the tile sits in its band, giving low-poly hills/mountains visible depth.
// Elevation is amplified via renderHeight (biome emphasis + preset elevGain) so
// the relief reads at a glance on the 144² map.
import * as THREE from 'three';
import {
  WORLD_SIZE,
  sampleTerrain,
  renderHeight,
  BIOME_COLOR,
  BIOME_SHADE,
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

// Blend the biome's base color toward its shade by `t` in [0,1]. t rises with how
// far the tile sits above its band's floor, so peaks darken and plains stay light.
const _base = new THREE.Color();
const _shade = new THREE.Color();
function facetColor(out: THREE.Color, biome: number, t: number): void {
  _base.setHex(BIOME_COLOR[biome as 0]);
  _shade.setHex(BIOME_SHADE[biome as 0]);
  out.copy(_base).lerp(_shade, Math.max(0, Math.min(1, t)));
}

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
      // Darken with render height: higher facets read as steeper/shadowed. Scale
      // is gentle so plains keep their hue and only relief picks up depth.
      facetColor(c, s.biome, y * 0.05);
      colors[idx] = c.r;
      colors[idx + 1] = c.g;
      colors[idx + 2] = c.b;
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
