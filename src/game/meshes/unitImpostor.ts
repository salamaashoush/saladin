import * as THREE from "three";
import { UNIT_DEFS, UnitKind } from "../../../shared/index.ts";

// A drastically simplified, low-triangle silhouette per unit kind for far-zoom
// LOD. It keeps the gross shape (foot vs mounted vs siege), the team tint (the
// torso/body uses the tint material so faction colour still reads), and the
// rough height — so a distant army still reads correctly while costing a fraction
// of the triangles. Baked into one InstancedMesh per kind by InstancedUnits.

const SKIN = new THREE.MeshStandardMaterial({
  color: 0xd9a878,
  flatShading: true,
});
const HIDE = new THREE.MeshStandardMaterial({
  color: 0x5a4632,
  flatShading: true,
});
const WOOD = new THREE.MeshStandardMaterial({
  color: 0x6b4a2b,
  flatShading: true,
});

function mesh(
  geo: THREE.BufferGeometry,
  mat: THREE.Material,
  x = 0,
  y = 0,
  z = 0,
): THREE.Mesh {
  const m = new THREE.Mesh(geo, mat);
  m.position.set(x, y, z);
  return m;
}

export function buildUnitImpostor(kind: number, color: number): THREE.Group {
  const def = UNIT_DEFS[kind as UnitKind] ?? UNIT_DEFS[UnitKind.Peasant];
  const g = new THREE.Group();
  const h = def.height;
  const r = def.radius;
  const tunic = new THREE.MeshStandardMaterial({ color, flatShading: true });
  g.userData.tintMat = tunic;

  const isMounted =
    kind === UnitKind.Knight ||
    kind === UnitKind.HorseArcher ||
    kind === UnitKind.Mamluk;
  const isSiege = kind === UnitKind.Ram || kind === UnitKind.Mangonel;

  if (isSiege) {
    // A single tinted block roughly the size of the engine, plus a wood base.
    g.add(
      mesh(
        new THREE.BoxGeometry(h * 1.1, h * 0.5, h * 0.7),
        WOOD,
        0,
        r * 0.5,
        0,
      ),
    );
    g.add(
      mesh(
        new THREE.BoxGeometry(h * 1.0, h * 0.5, h * 0.6),
        tunic,
        0,
        h * 0.9,
        0,
      ),
    );
    return g;
  }

  const baseY = isMounted ? h * 0.55 : 0;
  if (isMounted) {
    // Coarse horse body block + four stubby legs.
    g.add(
      mesh(
        new THREE.BoxGeometry(r * 0.8, h * 0.4, h * 1.1),
        HIDE,
        0,
        h * 0.32,
        0,
      ),
    );
    g.add(
      mesh(
        new THREE.BoxGeometry(r * 0.4, h * 0.4, r * 0.4),
        HIDE,
        0,
        h * 0.62,
        h * 0.55,
      ),
    );
    for (const sx of [-1, 1] as const)
      for (const sz of [-1, 1] as const)
        g.add(
          mesh(
            new THREE.BoxGeometry(r * 0.18, h * 0.34, r * 0.18),
            HIDE,
            sx * r * 0.3,
            h * 0.14,
            sz * h * 0.4,
          ),
        );
  } else {
    // Two stubby legs.
    g.add(
      mesh(
        new THREE.BoxGeometry(r * 0.5, h * 0.45, r * 0.5),
        HIDE,
        -r * 0.28,
        h * 0.22,
        0,
      ),
    );
    g.add(
      mesh(
        new THREE.BoxGeometry(r * 0.5, h * 0.45, r * 0.5),
        HIDE,
        r * 0.28,
        h * 0.22,
        0,
      ),
    );
  }

  // Tinted torso (faction colour) + a skin head — the parts a player tracks at a
  // glance. A 5-sided cylinder + low-seg sphere keep the silhouette but cut tris.
  const torso = mesh(
    new THREE.CylinderGeometry(r * 0.7, r * 0.92, h * 0.62, 5),
    tunic,
    0,
    baseY + h * 0.72,
    0,
  );
  g.add(torso);
  g.add(
    mesh(new THREE.SphereGeometry(r * 0.6, 5, 4), SKIN, 0, baseY + h * 1.12, 0),
  );
  return g;
}
