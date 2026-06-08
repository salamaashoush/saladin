// Biome heightmap, built as a grid of chunk meshes (not one giant mesh) so the
// renderer frustum-culls off-screen chunks and we can swap per-chunk LOD later.
// Same seed the module used → what we draw matches where the server placed land.
import * as THREE from 'three';
import {
  WORLD_SIZE,
  sampleTerrain,
  renderHeight,
  BIOME_COLOR,
} from '../../shared/index.ts';

export interface TerrainBuild {
  group: THREE.Group; // contains all chunk meshes (named 'ground')
}

const CHUNK = 24; // tiles per chunk side

export function terrainHeight(seed: number, x: number, z: number): number {
  return renderHeight(sampleTerrain(seed, x, z).height);
}

function buildChunk(
  seed: number,
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
      const idx = (j * V + i) * 3;
      positions[idx] = x;
      positions[idx + 1] = renderHeight(s.height);
      positions[idx + 2] = z;
      c.setHex(BIOME_COLOR[s.biome]);
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
  return mesh;
}

export function buildTerrain(seed: number): TerrainBuild {
  // One shared material — flat shading gives the stylized low-poly facet look.
  const material = new THREE.MeshStandardMaterial({
    vertexColors: true,
    roughness: 0.96,
    flatShading: true,
  });

  const group = new THREE.Group();
  for (let cz = 0; cz < WORLD_SIZE; cz += CHUNK) {
    for (let cx = 0; cx < WORLD_SIZE; cx += CHUNK) {
      const size = Math.min(CHUNK, WORLD_SIZE - cx, WORLD_SIZE - cz);
      group.add(buildChunk(seed, cx, cz, size, material));
    }
  }
  return { group };
}
