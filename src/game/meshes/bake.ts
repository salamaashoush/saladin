import * as THREE from "three";

// Bake a procedurally-assembled THREE.Group (the existing buildUnit / buildByKind
// output) into ONE BufferGeometry with per-vertex colour, so a whole kind can be
// drawn by a single InstancedMesh. Each source mesh's local transform is folded
// into its vertices; its flat material colour becomes a vertex colour. Meshes
// whose material is the group's team-tint material are flagged per-vertex in a
// `tintable` attribute (1 = recolour per instance, 0 = keep baked colour), which
// the instanced material's shader reads to tint only the faction parts.
//
// This keeps the exact silhouette of the per-part build: we reuse the very same
// geometry/transforms, only collapsed to one buffer.

export interface BakedGeometry {
  geometry: THREE.BufferGeometry; // position + normal + color + tintable
  height: number; // world-space top, for placing hp bars / impostors
}

const _m = new THREE.Matrix4();
const _nm = new THREE.Matrix3();
const _v = new THREE.Vector3();
const _n = new THREE.Vector3();

// Collect (geometry, worldMatrix, color, tintable) for every drawable mesh under
// `root`, expressed in root-local space.
function collectParts(
  root: THREE.Object3D,
  tintMat: THREE.Material | undefined,
): Array<{
  geo: THREE.BufferGeometry;
  mtx: THREE.Matrix4;
  color: THREE.Color;
  tintable: boolean;
}> {
  root.updateWorldMatrix(true, true);
  const inv = new THREE.Matrix4().copy(root.matrixWorld).invert();
  const parts: Array<{
    geo: THREE.BufferGeometry;
    mtx: THREE.Matrix4;
    color: THREE.Color;
    tintable: boolean;
  }> = [];
  root.traverse((o) => {
    const mesh = o as THREE.Mesh;
    if (!mesh.isMesh || !mesh.geometry) return;
    const mat = mesh.material as
      | THREE.MeshStandardMaterial
      | THREE.MeshStandardMaterial[];
    const m0 = Array.isArray(mat) ? mat[0] : mat;
    const color =
      (m0 as THREE.MeshStandardMaterial).color?.clone() ??
      new THREE.Color(0xffffff);
    const tintable =
      !!tintMat &&
      (m0 === tintMat ||
        (Array.isArray(mat) &&
          mat.includes(tintMat as THREE.MeshStandardMaterial)));
    // worldMatrix of the part relative to the root.
    const local = new THREE.Matrix4().multiplyMatrices(inv, mesh.matrixWorld);
    parts.push({ geo: mesh.geometry, mtx: local, color, tintable });
  });
  return parts;
}

export function bakeGroup(
  root: THREE.Object3D,
  tintMat: THREE.Material | undefined,
): BakedGeometry {
  const parts = collectParts(root, tintMat);

  let total = 0;
  const nonIndexed: Array<{
    geo: THREE.BufferGeometry;
    mtx: THREE.Matrix4;
    color: THREE.Color;
    tintable: boolean;
  }> = [];
  for (const p of parts) {
    const g = p.geo.index ? p.geo.toNonIndexed() : p.geo;
    nonIndexed.push({ ...p, geo: g });
    total += g.getAttribute("position").count;
  }

  const positions = new Float32Array(total * 3);
  const normals = new Float32Array(total * 3);
  const colors = new Float32Array(total * 3);
  const tint = new Float32Array(total);

  let o = 0;
  let maxY = 0;
  for (const p of nonIndexed) {
    const pos = p.geo.getAttribute("position") as THREE.BufferAttribute;
    const nrm = p.geo.getAttribute("normal") as
      | THREE.BufferAttribute
      | undefined;
    _m.copy(p.mtx);
    _nm.getNormalMatrix(_m);
    const count = pos.count;
    for (let i = 0; i < count; i++) {
      _v.fromBufferAttribute(pos, i).applyMatrix4(_m);
      positions[o * 3] = _v.x;
      positions[o * 3 + 1] = _v.y;
      positions[o * 3 + 2] = _v.z;
      if (_v.y > maxY) maxY = _v.y;
      if (nrm) {
        _n.fromBufferAttribute(nrm, i).applyMatrix3(_nm).normalize();
        normals[o * 3] = _n.x;
        normals[o * 3 + 1] = _n.y;
        normals[o * 3 + 2] = _n.z;
      }
      colors[o * 3] = p.color.r;
      colors[o * 3 + 1] = p.color.g;
      colors[o * 3 + 2] = p.color.b;
      tint[o] = p.tintable ? 1 : 0;
      o++;
    }
  }

  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(positions, 3));
  geometry.setAttribute("normal", new THREE.BufferAttribute(normals, 3));
  geometry.setAttribute("color", new THREE.BufferAttribute(colors, 3));
  geometry.setAttribute("tintable", new THREE.BufferAttribute(tint, 1));
  geometry.computeBoundingSphere();
  geometry.computeBoundingBox();

  // The source parts can be shared geometries (buildUnit clones per call but reuses
  // some), so only dispose the toNonIndexed temporaries we created.
  for (let i = 0; i < nonIndexed.length; i++)
    if (nonIndexed[i].geo !== parts[i].geo) nonIndexed[i].geo.dispose();

  return { geometry, height: maxY };
}

// A plain vertex-colour MeshStandardMaterial for instanced meshes whose parts are
// never recoloured per instance (e.g. resource nodes — every tree/rock keeps its
// baked colours). Same flat-shaded look as the per-part build, one draw per kind.
// No instanceColor / tintable shader patch: the baked vertex colours render as-is.
export function instancedVertexColorMaterial(
  opts: THREE.MeshStandardMaterialParameters = {},
): THREE.MeshStandardMaterial {
  return new THREE.MeshStandardMaterial({
    vertexColors: true,
    flatShading: true,
    ...opts,
  });
}

// A vertex-colour-aware MeshStandardMaterial whose tintable vertices (tintable=1)
// are multiplied by the per-instance instanceColor, while fixed parts keep their
// baked vertex colour. One material per draw, shared across all instances of a
// kind, so the faction tint stays a cheap per-instance colour.
export function instancedTintMaterial(
  opts: THREE.MeshStandardMaterialParameters = {},
): THREE.MeshStandardMaterial {
  const mat = new THREE.MeshStandardMaterial({
    vertexColors: true,
    flatShading: true,
    ...opts,
  });
  // In three r184 the stock color_vertex chunk folds the vertex `color` AND the
  // per-instance `instanceColor` into a single `vColor` varying (multiplied). We
  // replace that chunk so the instance tint multiplies ONLY tintable vertices:
  // fixed parts keep their baked colour, faction parts (baked white) become the
  // pure team colour. instanceColor is set via InstancedMesh.setColorAt.
  mat.onBeforeCompile = (shader) => {
    shader.vertexShader = shader.vertexShader.replace(
      "#include <common>",
      "#include <common>\nattribute float tintable;",
    );
    shader.vertexShader = shader.vertexShader.replace(
      "#include <color_vertex>",
      [
        "#if defined( USE_COLOR ) || defined( USE_COLOR_ALPHA ) || defined( USE_INSTANCING_COLOR )",
        "  vColor = vec4( 1.0 );",
        "#endif",
        "#ifdef USE_COLOR",
        "  vColor.rgb *= color;",
        "#endif",
        "#ifdef USE_INSTANCING_COLOR",
        "  vColor.rgb = mix( vColor.rgb, vColor.rgb * instanceColor.rgb, tintable );",
        "#endif",
      ].join("\n"),
    );
  };
  // Force a unique program key so this variant compiles separately from the stock
  // standard material (avoids cache collisions on the shared shader).
  mat.customProgramCacheKey = () => "saladin-instanced-tint";
  return mat;
}
