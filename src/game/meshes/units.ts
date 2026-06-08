import * as THREE from 'three';
import { UNIT_DEFS, UnitKind } from '../../../shared/index.ts';

export function buildUnit(kind: number, color: number): THREE.Group {
  const def = UNIT_DEFS[kind as UnitKind] ?? UNIT_DEFS[UnitKind.Peasant];
  const pivot = new THREE.Group();
  const h = def.height;
  const r = def.radius;
  const skin = new THREE.MeshStandardMaterial({ color: 0xd9a878, flatShading: true });
  const tunic = new THREE.MeshStandardMaterial({ color, flatShading: true });
  const metal = new THREE.MeshStandardMaterial({
    color: 0x9aa0a6,
    metalness: 0.3,
    roughness: 0.6,
    flatShading: true,
  });
  const wood = new THREE.MeshStandardMaterial({ color: 0x6b4a2b, flatShading: true });
  pivot.userData.tintMat = tunic;

  const legGeo = new THREE.BoxGeometry(r * 0.5, h * 0.45, r * 0.5);
  const legL = new THREE.Mesh(legGeo, tunic);
  legL.position.set(-r * 0.35, h * 0.22, 0);
  pivot.add(legL);
  const legR = new THREE.Mesh(legGeo, tunic);
  legR.position.set(r * 0.35, h * 0.22, 0);
  pivot.add(legR);

  const torso = new THREE.Mesh(
    new THREE.CylinderGeometry(r * 0.78, r * 0.95, h * 0.6, 8),
    tunic
  );
  torso.position.y = h * 0.72;
  torso.castShadow = true;
  pivot.add(torso);

  const head = new THREE.Mesh(new THREE.SphereGeometry(r * 0.7, 10, 8), skin);
  head.position.y = h * 1.08;
  head.castShadow = true;
  pivot.add(head);

  if (kind === UnitKind.Spearman) {
    const helm = new THREE.Mesh(new THREE.ConeGeometry(r * 0.78, r * 0.8, 8), metal);
    helm.position.y = h * 1.24;
    pivot.add(helm);
    const shield = new THREE.Mesh(
      new THREE.CylinderGeometry(r * 0.7, r * 0.7, 0.08, 14),
      metal
    );
    shield.rotation.x = Math.PI / 2;
    shield.position.set(-r * 1.05, h * 0.72, r * 0.15);
    pivot.add(shield);
    const spear = new THREE.Mesh(
      new THREE.CylinderGeometry(0.03, 0.03, h * 2.6, 5),
      wood
    );
    spear.position.set(r * 1.0, h * 1.1, 0);
    pivot.add(spear);
    const tip = new THREE.Mesh(new THREE.ConeGeometry(0.07, 0.26, 6), metal);
    tip.position.set(r * 1.0, h * 2.4, 0);
    pivot.add(tip);
  } else if (kind === UnitKind.Archer) {
    const hood = new THREE.Mesh(new THREE.ConeGeometry(r * 0.82, r * 0.95, 8), tunic);
    hood.position.y = h * 1.2;
    pivot.add(hood);
    const bow = new THREE.Mesh(
      new THREE.TorusGeometry(r * 0.95, 0.04, 6, 14, Math.PI * 1.3),
      wood
    );
    bow.position.set(-r * 1.05, h * 0.85, 0);
    bow.rotation.z = Math.PI / 2;
    pivot.add(bow);
    const quiver = new THREE.Mesh(
      new THREE.CylinderGeometry(r * 0.25, r * 0.25, h * 0.5, 6),
      wood
    );
    quiver.position.set(r * 0.6, h * 0.95, -r * 0.4);
    quiver.rotation.x = 0.35;
    pivot.add(quiver);
  } else if (kind === UnitKind.Knight) {
    const horse = new THREE.MeshStandardMaterial({
      color: 0x5a4632,
      flatShading: true,
    });
    const body = new THREE.Mesh(
      new THREE.BoxGeometry(r * 0.9, h * 0.4, h * 1.1),
      horse
    );
    body.position.set(0, h * 0.3, 0);
    body.castShadow = true;
    pivot.add(body);
    const neck = new THREE.Mesh(
      new THREE.BoxGeometry(r * 0.5, h * 0.45, r * 0.5),
      horse
    );
    neck.position.set(0, h * 0.52, h * 0.48);
    neck.rotation.x = -0.5;
    pivot.add(neck);
    const hhead = new THREE.Mesh(
      new THREE.BoxGeometry(r * 0.45, r * 0.5, h * 0.4),
      horse
    );
    hhead.position.set(0, h * 0.68, h * 0.66);
    pivot.add(hhead);
    for (const sx of [-1, 1] as const)
      for (const sz of [-1, 1] as const) {
        const leg = new THREE.Mesh(
          new THREE.CylinderGeometry(r * 0.12, r * 0.12, h * 0.34, 5),
          horse
        );
        leg.position.set(sx * r * 0.32, h * 0.12, sz * h * 0.42);
        pivot.add(leg);
      }
    const helm = new THREE.Mesh(
      new THREE.ConeGeometry(r * 0.7, r * 0.7, 8),
      metal
    );
    helm.position.y = h * 1.26;
    pivot.add(helm);
    const lance = new THREE.Mesh(
      new THREE.CylinderGeometry(0.035, 0.035, h * 2.8, 5),
      wood
    );
    lance.position.set(r * 1.0, h * 1.05, h * 0.2);
    lance.rotation.x = 0.18;
    pivot.add(lance);
    const ltip = new THREE.Mesh(new THREE.ConeGeometry(0.08, 0.3, 6), metal);
    ltip.position.set(r * 1.0, h * 1.05, h * 1.55);
    ltip.rotation.x = Math.PI / 2;
    pivot.add(ltip);
  } else {
    const cap = new THREE.Mesh(new THREE.ConeGeometry(r * 0.72, r * 0.55, 7), wood);
    cap.position.y = h * 1.2;
    pivot.add(cap);
    const handle = new THREE.Mesh(
      new THREE.CylinderGeometry(0.025, 0.025, h * 1.7, 5),
      wood
    );
    handle.position.set(r * 0.95, h * 0.85, 0);
    handle.rotation.z = 0.2;
    pivot.add(handle);
    const blade = new THREE.Mesh(new THREE.BoxGeometry(0.06, 0.18, 0.28), metal);
    blade.position.set(r * 1.12, h * 1.55, 0);
    pivot.add(blade);
  }
  return pivot;
}
