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
  } else if (kind === UnitKind.HorseArcher || kind === UnitKind.Mamluk) {
    // Mounted figures share the horse body; the rider's kit differs.
    const horse = new THREE.MeshStandardMaterial({
      color: kind === UnitKind.Mamluk ? 0x3a2a18 : 0x6a4a2a,
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
    if (kind === UnitKind.HorseArcher) {
      const turban = new THREE.Mesh(new THREE.SphereGeometry(r * 0.55, 8, 6), tunic);
      turban.position.y = h * 1.2;
      turban.scale.y = 0.7;
      pivot.add(turban);
      const bow = new THREE.Mesh(
        new THREE.TorusGeometry(r * 0.85, 0.04, 6, 14, Math.PI * 1.3),
        wood
      );
      bow.position.set(-r * 1.0, h * 0.9, 0);
      bow.rotation.z = Math.PI / 2;
      pivot.add(bow);
    } else {
      const helm = new THREE.Mesh(new THREE.ConeGeometry(r * 0.62, r * 0.9, 8), metal);
      helm.position.y = h * 1.3;
      pivot.add(helm);
      const sabre = new THREE.Mesh(
        new THREE.TorusGeometry(r * 0.5, 0.035, 5, 10, Math.PI * 0.9),
        metal
      );
      sabre.position.set(r * 1.05, h * 1.0, 0);
      sabre.rotation.set(0, 0, -0.4);
      pivot.add(sabre);
    }
  } else if (kind === UnitKind.Crossbowman) {
    const cap = new THREE.Mesh(new THREE.CylinderGeometry(r * 0.7, r * 0.7, r * 0.4, 8), metal);
    cap.position.y = h * 1.22;
    pivot.add(cap);
    const stock = new THREE.Mesh(
      new THREE.BoxGeometry(0.08, 0.08, h * 0.9),
      wood
    );
    stock.position.set(-r * 0.9, h * 0.92, r * 0.1);
    pivot.add(stock);
    const bow = new THREE.Mesh(new THREE.BoxGeometry(r * 1.4, 0.06, 0.06), wood);
    bow.position.set(-r * 0.9, h * 0.92, r * 0.5);
    pivot.add(bow);
  } else if (kind === UnitKind.Ram) {
    const frameMat = new THREE.MeshStandardMaterial({ color: 0x5a3f23, flatShading: true });
    const roof = new THREE.Mesh(new THREE.BoxGeometry(h * 1.3, 0.18, h * 0.9), wood);
    roof.position.y = h * 1.0;
    roof.castShadow = true;
    pivot.add(roof);
    const beam = new THREE.Mesh(
      new THREE.CylinderGeometry(r * 0.32, r * 0.32, h * 1.5, 8),
      frameMat
    );
    beam.rotation.z = Math.PI / 2;
    beam.position.y = h * 0.55;
    beam.castShadow = true;
    pivot.add(beam);
    const headRam = new THREE.Mesh(new THREE.ConeGeometry(r * 0.4, r * 0.7, 8), metal);
    headRam.rotation.z = -Math.PI / 2;
    headRam.position.set(h * 0.85, h * 0.55, 0);
    pivot.add(headRam);
    for (const sx of [-1, 1] as const)
      for (const sz of [-1, 1] as const) {
        const wheel = new THREE.Mesh(
          new THREE.CylinderGeometry(r * 0.4, r * 0.4, 0.12, 10),
          frameMat
        );
        wheel.rotation.x = Math.PI / 2;
        wheel.position.set(sx * h * 0.45, r * 0.4, sz * h * 0.36);
        pivot.add(wheel);
      }
  } else if (kind === UnitKind.Mangonel) {
    const frameMat = new THREE.MeshStandardMaterial({ color: 0x4a3522, flatShading: true });
    const base = new THREE.Mesh(new THREE.BoxGeometry(h * 0.9, 0.2, h * 1.1), frameMat);
    base.position.y = r * 0.5;
    base.castShadow = true;
    pivot.add(base);
    const arm = new THREE.Mesh(
      new THREE.CylinderGeometry(0.05, 0.05, h * 1.2, 6),
      wood
    );
    arm.position.set(0, h * 0.6, -h * 0.1);
    arm.rotation.x = -0.7;
    arm.castShadow = true;
    pivot.add(arm);
    const bucket = new THREE.Mesh(new THREE.SphereGeometry(r * 0.42, 8, 6, 0, Math.PI * 2, 0, Math.PI / 2), metal);
    bucket.position.set(0, h * 1.05, -h * 0.5);
    pivot.add(bucket);
    for (const sx of [-1, 1] as const) {
      const strut = new THREE.Mesh(new THREE.BoxGeometry(0.06, h * 0.7, 0.06), wood);
      strut.position.set(sx * r * 0.5, h * 0.45, 0);
      pivot.add(strut);
    }
    for (const sx of [-1, 1] as const)
      for (const sz of [-1, 1] as const) {
        const wheel = new THREE.Mesh(
          new THREE.CylinderGeometry(r * 0.38, r * 0.38, 0.1, 10),
          frameMat
        );
        wheel.rotation.x = Math.PI / 2;
        wheel.position.set(sx * h * 0.4, r * 0.38, sz * h * 0.42);
        pivot.add(wheel);
      }
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
