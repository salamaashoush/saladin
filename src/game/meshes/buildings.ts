import * as THREE from 'three';
import { BuildingKind } from '../../../shared/index.ts';

export function buildByKind(kind: number, color: number): THREE.Group {
  if (kind === BuildingKind.Wall) return buildWallSlab();
  if (kind === BuildingKind.Tower) return buildTower(color);
  if (kind === BuildingKind.Barracks) return buildBarracks(color);
  if (kind === BuildingKind.Gatehouse) return buildGatehouse(color);
  if (kind === BuildingKind.House) return buildHouse(color);
  if (kind === BuildingKind.Stable) return buildStable(color);
  if (kind === BuildingKind.Blacksmith) return buildBlacksmith(color);
  if (kind === BuildingKind.Market) return buildMarket(color);
  if (kind === BuildingKind.Granary) return buildGranary(color);
  if (kind === BuildingKind.FishingHut) return buildFishingHut(color);
  if (kind === BuildingKind.SiegeWorkshop) return buildSiegeWorkshop(color);
  return buildKeep(color);
}

function teamMat(color: number): THREE.MeshStandardMaterial {
  return new THREE.MeshStandardMaterial({
    color,
    side: THREE.DoubleSide,
    flatShading: true,
  });
}

function flag(g: THREE.Group, team: THREE.Material, x: number, y: number, z: number) {
  const dark = new THREE.MeshStandardMaterial({ color: 0x5a3a22, flatShading: true });
  const pole = new THREE.Mesh(new THREE.CylinderGeometry(0.04, 0.04, 0.7, 5), dark);
  pole.position.set(x, y, z);
  g.add(pole);
  const f = new THREE.Mesh(new THREE.PlaneGeometry(0.45, 0.28), team);
  f.position.set(x + 0.22, y + 0.18, z);
  g.add(f);
}

// Open-fronted horse shed: low timber barn with a fenced paddock rail.
export function buildStable(color: number): THREE.Group {
  const g = new THREE.Group();
  const wall = new THREE.MeshStandardMaterial({ color: 0x9a7a4a, roughness: 1, flatShading: true });
  const roofMat = new THREE.MeshStandardMaterial({ color: 0x6b4a2b, roughness: 1, flatShading: true });
  const dark = new THREE.MeshStandardMaterial({ color: 0x4a3320, flatShading: true });
  const team = teamMat(color);
  g.userData.tintMat = team;
  const base = new THREE.Mesh(new THREE.BoxGeometry(2, 0.9, 1.5), wall);
  base.position.set(0, 0.45, -0.2);
  base.castShadow = true;
  base.receiveShadow = true;
  g.add(base);
  const roof = new THREE.Mesh(new THREE.BoxGeometry(2.1, 0.18, 1.7), roofMat);
  roof.position.set(0, 1.0, -0.2);
  roof.rotation.z = 0.05;
  roof.castShadow = true;
  g.add(roof);
  // Paddock rails at the front.
  for (const z of [0.7, 0.95]) {
    const rail = new THREE.Mesh(new THREE.BoxGeometry(1.9, 0.06, 0.06), dark);
    rail.position.set(0, 0.55, z);
    g.add(rail);
  }
  for (const x of [-0.9, 0, 0.9]) {
    const post = new THREE.Mesh(new THREE.BoxGeometry(0.08, 0.7, 0.08), dark);
    post.position.set(x, 0.35, 0.83);
    g.add(post);
  }
  flag(g, team, 0.85, 1.4, -0.6);
  return g;
}

// Stone smithy with a chimney venting forge smoke (a small ember-lit cube).
export function buildBlacksmith(color: number): THREE.Group {
  const g = new THREE.Group();
  const stone = new THREE.MeshStandardMaterial({ color: 0x8a847a, roughness: 1, flatShading: true });
  const roofMat = new THREE.MeshStandardMaterial({ color: 0x3a3a3a, roughness: 1, flatShading: true });
  const ember = new THREE.MeshStandardMaterial({ color: 0xd9531e, emissive: 0x6a2200, flatShading: true });
  const team = teamMat(color);
  g.userData.tintMat = team;
  const base = new THREE.Mesh(new THREE.BoxGeometry(1.9, 1.1, 1.9), stone);
  base.position.y = 0.55;
  base.castShadow = true;
  base.receiveShadow = true;
  g.add(base);
  const roof = new THREE.Mesh(new THREE.BoxGeometry(2.0, 0.16, 2.0), roofMat);
  roof.position.y = 1.18;
  roof.castShadow = true;
  g.add(roof);
  const chimney = new THREE.Mesh(new THREE.BoxGeometry(0.45, 0.9, 0.45), stone);
  chimney.position.set(0.6, 1.55, 0.6);
  chimney.castShadow = true;
  g.add(chimney);
  const forge = new THREE.Mesh(new THREE.BoxGeometry(0.5, 0.5, 0.3), ember);
  forge.position.set(0, 0.4, 0.98);
  g.add(forge);
  flag(g, team, -0.7, 1.4, -0.6);
  return g;
}

// Open market stall under a striped awning.
export function buildMarket(color: number): THREE.Group {
  const g = new THREE.Group();
  const wood = new THREE.MeshStandardMaterial({ color: 0x8a6a3a, roughness: 1, flatShading: true });
  const cloth = new THREE.MeshStandardMaterial({ color: 0xd9c27a, roughness: 1, side: THREE.DoubleSide, flatShading: true });
  const crate = new THREE.MeshStandardMaterial({ color: 0x6b4a2b, flatShading: true });
  const team = teamMat(color);
  g.userData.tintMat = team;
  const counter = new THREE.Mesh(new THREE.BoxGeometry(1.8, 0.6, 0.7), wood);
  counter.position.set(0, 0.3, -0.4);
  counter.castShadow = true;
  counter.receiveShadow = true;
  g.add(counter);
  for (const sx of [-0.85, 0.85])
    for (const sz of [-0.7, 0.7]) {
      const post = new THREE.Mesh(new THREE.CylinderGeometry(0.05, 0.05, 1.4, 6), wood);
      post.position.set(sx, 0.7, sz);
      g.add(post);
    }
  const awning = new THREE.Mesh(new THREE.BoxGeometry(2.0, 0.08, 1.7), cloth);
  awning.position.set(0, 1.45, 0);
  awning.rotation.x = 0.12;
  awning.castShadow = true;
  g.add(awning);
  for (const sx of [-0.6, 0.6]) {
    const box = new THREE.Mesh(new THREE.BoxGeometry(0.4, 0.4, 0.4), crate);
    box.position.set(sx, 0.2, 0.45);
    g.add(box);
  }
  flag(g, team, 0.9, 1.6, 0.7);
  return g;
}

// Round grain silo with a conical cap.
export function buildGranary(color: number): THREE.Group {
  const g = new THREE.Group();
  const clay = new THREE.MeshStandardMaterial({ color: 0xcdb07a, roughness: 1, flatShading: true });
  const roofMat = new THREE.MeshStandardMaterial({ color: 0x8a5a2f, roughness: 1, flatShading: true });
  const team = teamMat(color);
  g.userData.tintMat = team;
  const silo = new THREE.Mesh(new THREE.CylinderGeometry(0.85, 0.95, 1.5, 12), clay);
  silo.position.y = 0.75;
  silo.castShadow = true;
  silo.receiveShadow = true;
  g.add(silo);
  const band = new THREE.Mesh(new THREE.CylinderGeometry(0.9, 0.9, 0.18, 12), team);
  band.position.y = 1.1;
  g.add(band);
  const roof = new THREE.Mesh(new THREE.ConeGeometry(1.0, 0.7, 12), roofMat);
  roof.position.y = 1.85;
  roof.castShadow = true;
  g.add(roof);
  const door = new THREE.Mesh(new THREE.BoxGeometry(0.4, 0.6, 0.1), roofMat);
  door.position.set(0, 0.3, 0.9);
  g.add(door);
  return g;
}

// Small shore hut on stilts with a drying net.
export function buildFishingHut(color: number): THREE.Group {
  const g = new THREE.Group();
  const wood = new THREE.MeshStandardMaterial({ color: 0x9a7a52, roughness: 1, flatShading: true });
  const roofMat = new THREE.MeshStandardMaterial({ color: 0x6a4a2a, roughness: 1, flatShading: true });
  const net = new THREE.MeshStandardMaterial({ color: 0xb8b8a0, roughness: 1, side: THREE.DoubleSide, flatShading: true });
  const team = teamMat(color);
  g.userData.tintMat = team;
  for (const sx of [-0.35, 0.35])
    for (const sz of [-0.35, 0.35]) {
      const stilt = new THREE.Mesh(new THREE.CylinderGeometry(0.05, 0.05, 0.5, 5), wood);
      stilt.position.set(sx, 0.25, sz);
      g.add(stilt);
    }
  const hut = new THREE.Mesh(new THREE.BoxGeometry(0.95, 0.6, 0.95), wood);
  hut.position.y = 0.8;
  hut.castShadow = true;
  g.add(hut);
  const roof = new THREE.Mesh(new THREE.ConeGeometry(0.85, 0.5, 4), roofMat);
  roof.position.y = 1.3;
  roof.rotation.y = Math.PI / 4;
  roof.castShadow = true;
  g.add(roof);
  const pole = new THREE.Mesh(new THREE.CylinderGeometry(0.03, 0.03, 1.0, 5), wood);
  pole.position.set(0.7, 0.5, 0.0);
  pole.rotation.z = 0.3;
  g.add(pole);
  const netMesh = new THREE.Mesh(new THREE.PlaneGeometry(0.5, 0.5), net);
  netMesh.position.set(0.85, 0.55, 0);
  g.add(netMesh);
  flag(g, team, -0.45, 1.4, 0.0);
  return g;
}

// Timbered workshop with a partly-built siege frame out front.
export function buildSiegeWorkshop(color: number): THREE.Group {
  const g = new THREE.Group();
  const wood = new THREE.MeshStandardMaterial({ color: 0x7a5a32, roughness: 1, flatShading: true });
  const roofMat = new THREE.MeshStandardMaterial({ color: 0x4a3320, roughness: 1, flatShading: true });
  const metal = new THREE.MeshStandardMaterial({ color: 0x8a8a8a, metalness: 0.3, roughness: 0.7, flatShading: true });
  const team = teamMat(color);
  g.userData.tintMat = team;
  const base = new THREE.Mesh(new THREE.BoxGeometry(2, 1.0, 1.6), wood);
  base.position.set(0, 0.5, -0.2);
  base.castShadow = true;
  base.receiveShadow = true;
  g.add(base);
  const roof = new THREE.Mesh(new THREE.BoxGeometry(2.1, 0.16, 1.8), roofMat);
  roof.position.set(0, 1.08, -0.2);
  roof.castShadow = true;
  g.add(roof);
  // Half-built siege frame.
  const frame = new THREE.Mesh(new THREE.BoxGeometry(0.9, 0.7, 0.12), wood);
  frame.position.set(0, 0.45, 0.75);
  frame.rotation.x = 0.25;
  g.add(frame);
  const wheel = new THREE.Mesh(new THREE.CylinderGeometry(0.3, 0.3, 0.12, 10), metal);
  wheel.rotation.x = Math.PI / 2;
  wheel.position.set(0.5, 0.3, 0.85);
  g.add(wheel);
  flag(g, team, -0.85, 1.4, -0.6);
  return g;
}

export function buildGatehouse(color: number): THREE.Group {
  const g = new THREE.Group();
  const stone = new THREE.MeshStandardMaterial({
    color: 0x9a948a,
    roughness: 1,
    flatShading: true,
  });
  const team = new THREE.MeshStandardMaterial({ color, flatShading: true });
  g.userData.tintMat = team;
  for (const sx of [-0.36, 0.36]) {
    const pillar = new THREE.Mesh(new THREE.BoxGeometry(0.28, 1.3, 0.55), stone);
    pillar.position.set(sx, 0.65, 0);
    pillar.castShadow = true;
    g.add(pillar);
  }
  const lintel = new THREE.Mesh(new THREE.BoxGeometry(1.0, 0.28, 0.55), stone);
  lintel.position.y = 1.44;
  lintel.castShadow = true;
  g.add(lintel);
  const band = new THREE.Mesh(new THREE.BoxGeometry(0.5, 0.16, 0.57), team);
  band.position.y = 1.44;
  g.add(band);
  for (const sx of [-0.32, 0.32]) {
    const m = new THREE.Mesh(new THREE.BoxGeometry(0.22, 0.24, 0.55), stone);
    m.position.set(sx, 1.7, 0);
    g.add(m);
  }
  return g;
}

export function buildHouse(color: number): THREE.Group {
  const g = new THREE.Group();
  const wall = new THREE.MeshStandardMaterial({
    color: 0xcbb487,
    roughness: 1,
    flatShading: true,
  });
  const roofMat = new THREE.MeshStandardMaterial({
    color: 0x8a4b2f,
    roughness: 1,
    flatShading: true,
  });
  const dark = new THREE.MeshStandardMaterial({
    color: 0x5a3a22,
    flatShading: true,
  });
  const team = new THREE.MeshStandardMaterial({
    color,
    side: THREE.DoubleSide,
    flatShading: true,
  });
  g.userData.tintMat = team;
  const base = new THREE.Mesh(new THREE.BoxGeometry(1.7, 1.0, 1.7), wall);
  base.position.y = 0.5;
  base.castShadow = true;
  base.receiveShadow = true;
  g.add(base);
  const roof = new THREE.Mesh(new THREE.ConeGeometry(1.45, 0.85, 4), roofMat);
  roof.position.y = 1.42;
  roof.rotation.y = Math.PI / 4;
  roof.castShadow = true;
  g.add(roof);
  const door = new THREE.Mesh(new THREE.BoxGeometry(0.4, 0.6, 0.12), dark);
  door.position.set(0, 0.3, 0.86);
  g.add(door);
  const pole = new THREE.Mesh(
    new THREE.CylinderGeometry(0.03, 0.03, 0.5, 5),
    dark
  );
  pole.position.set(0.62, 1.9, 0.62);
  g.add(pole);
  const flag = new THREE.Mesh(new THREE.PlaneGeometry(0.42, 0.26), team);
  flag.position.set(0.84, 2.0, 0.62);
  g.add(flag);
  return g;
}

// A straight crenellated wall slab along local X. The group is rotated to the
// run direction; the slab is long enough (1.5) to bridge orthogonal AND
// diagonal neighbours, so a line reads as one continuous wall.
export function buildWallSlab(): THREE.Group {
  const g = new THREE.Group();
  const stone = new THREE.MeshStandardMaterial({
    color: 0x9a948a,
    roughness: 1,
    flatShading: true,
  });
  const cap = new THREE.MeshStandardMaterial({
    color: 0x847c71,
    roughness: 1,
    flatShading: true,
  });
  const H = 0.6;
  const TK = 0.42;
  const L = 1.5;
  const body = new THREE.Mesh(new THREE.BoxGeometry(L, H, TK), stone);
  body.position.y = H / 2;
  body.castShadow = true;
  body.receiveShadow = true;
  g.add(body);
  const c = new THREE.Mesh(new THREE.BoxGeometry(L, 0.1, TK + 0.06), cap);
  c.position.y = H + 0.05;
  g.add(c);
  const top = H + 0.1 + 0.12;
  for (const ox of [-0.52, -0.17, 0.18, 0.53]) {
    const m = new THREE.Mesh(new THREE.BoxGeometry(0.24, 0.24, TK), stone);
    m.position.set(ox, top, 0);
    g.add(m);
  }
  return g;
}

export function buildTower(color: number): THREE.Group {
  const g = new THREE.Group();
  const stone = new THREE.MeshStandardMaterial({
    color: 0x9c958a,
    roughness: 1,
    flatShading: true,
  });
  const team = new THREE.MeshStandardMaterial({ color, flatShading: true });
  g.userData.tintMat = team;
  const body = new THREE.Mesh(
    new THREE.CylinderGeometry(0.5, 0.62, 2.4, 8),
    stone
  );
  body.position.y = 1.2;
  body.castShadow = true;
  g.add(body);
  for (let i = 0; i < 6; i++) {
    const a = (i / 6) * Math.PI * 2;
    const cren = new THREE.Mesh(new THREE.BoxGeometry(0.2, 0.32, 0.2), stone);
    cren.position.set(Math.cos(a) * 0.5, 2.45, Math.sin(a) * 0.5);
    g.add(cren);
  }
  const roof = new THREE.Mesh(new THREE.ConeGeometry(0.64, 0.7, 8), team);
  roof.position.y = 2.95;
  roof.castShadow = true;
  g.add(roof);
  return g;
}

export function buildBarracks(color: number): THREE.Group {
  const g = new THREE.Group();
  const wall = new THREE.MeshStandardMaterial({
    color: 0xb89a6a,
    roughness: 1,
    flatShading: true,
  });
  const dark = new THREE.MeshStandardMaterial({
    color: 0x6b4a2b,
    flatShading: true,
  });
  const team = new THREE.MeshStandardMaterial({
    color,
    side: THREE.DoubleSide,
    flatShading: true,
  });
  g.userData.tintMat = team;
  const base = new THREE.Mesh(new THREE.BoxGeometry(2, 1.2, 2), wall);
  base.position.y = 0.6;
  base.castShadow = true;
  base.receiveShadow = true;
  g.add(base);
  const roof = new THREE.Mesh(new THREE.ConeGeometry(1.65, 0.95, 4), team);
  roof.position.y = 1.68;
  roof.rotation.y = Math.PI / 4;
  roof.castShadow = true;
  g.add(roof);
  const door = new THREE.Mesh(new THREE.BoxGeometry(0.5, 0.8, 0.12), dark);
  door.position.set(0, 0.4, 1.0);
  g.add(door);
  const pole = new THREE.Mesh(
    new THREE.CylinderGeometry(0.04, 0.04, 0.8, 5),
    dark
  );
  pole.position.set(0.78, 2.05, 0.78);
  g.add(pole);
  const flag = new THREE.Mesh(new THREE.PlaneGeometry(0.5, 0.3), team);
  flag.position.set(1.02, 2.25, 0.78);
  g.add(flag);
  return g;
}

export function buildKeep(color: number): THREE.Group {
  const g = new THREE.Group();
  const stone = new THREE.MeshStandardMaterial({
    color: 0x9c958a,
    roughness: 1,
    flatShading: true,
  });
  const dark = new THREE.MeshStandardMaterial({
    color: 0x7d766b,
    roughness: 1,
    flatShading: true,
  });
  const team = new THREE.MeshStandardMaterial({
    color,
    roughness: 0.8,
    side: THREE.DoubleSide,
    flatShading: true,
  });
  g.userData.tintMat = team;

  const S = 3.2;
  const wallH = 1.3;
  const wallT = 0.34;
  const half = S / 2 - wallT / 2;

  const base = new THREE.Mesh(new THREE.BoxGeometry(S, 0.5, S), dark);
  base.position.y = 0.25;
  base.castShadow = true;
  base.receiveShadow = true;
  g.add(base);

  const mkWall = (x: number, z: number, rot: number) => {
    const w = new THREE.Mesh(new THREE.BoxGeometry(S - 0.2, wallH, wallT), stone);
    w.position.set(x, 0.5 + wallH / 2, z);
    w.rotation.y = rot;
    w.castShadow = true;
    g.add(w);
    const n = 5;
    for (let i = 0; i < n; i++) {
      const cren = new THREE.Mesh(new THREE.BoxGeometry(0.3, 0.3, wallT), stone);
      const off = (i / (n - 1) - 0.5) * (S - 0.7);
      cren.position.set(
        x + (rot === 0 ? off : 0),
        0.5 + wallH + 0.15,
        z + (rot === 0 ? 0 : off)
      );
      cren.rotation.y = rot;
      g.add(cren);
    }
  };
  mkWall(0, half, 0);
  mkWall(0, -half, 0);
  mkWall(half, 0, Math.PI / 2);
  mkWall(-half, 0, Math.PI / 2);

  const towerH = 2.1;
  const towerR = 0.46;
  for (const [sx, sz] of [
    [1, 1],
    [1, -1],
    [-1, -1],
    [-1, 1],
  ]) {
    const tx = sx * (S / 2 - 0.1);
    const tz = sz * (S / 2 - 0.1);
    const tower = new THREE.Mesh(
      new THREE.CylinderGeometry(towerR, towerR * 1.1, towerH, 8),
      stone
    );
    tower.position.set(tx, 0.5 + towerH / 2, tz);
    tower.castShadow = true;
    g.add(tower);
    const roof = new THREE.Mesh(new THREE.ConeGeometry(towerR * 1.3, 0.75, 8), team);
    roof.position.set(tx, 0.5 + towerH + 0.37, tz);
    roof.castShadow = true;
    g.add(roof);
  }

  const keepH = 2.7;
  const keepS = 1.35;
  const keep = new THREE.Mesh(new THREE.BoxGeometry(keepS, keepH, keepS), stone);
  keep.position.y = 0.5 + keepH / 2;
  keep.castShadow = true;
  g.add(keep);
  for (const [sx, sz] of [
    [1, 1],
    [1, -1],
    [-1, -1],
    [-1, 1],
  ]) {
    const cren = new THREE.Mesh(new THREE.BoxGeometry(0.3, 0.36, 0.3), stone);
    cren.position.set(
      sx * (keepS / 2 - 0.15),
      0.5 + keepH + 0.18,
      sz * (keepS / 2 - 0.15)
    );
    g.add(cren);
  }

  const pole = new THREE.Mesh(
    new THREE.CylinderGeometry(0.04, 0.04, 1.0, 5),
    dark
  );
  pole.position.set(0, 0.5 + keepH + 0.5, 0);
  g.add(pole);
  const flag = new THREE.Mesh(new THREE.PlaneGeometry(0.62, 0.36), team);
  flag.position.set(0.33, 0.5 + keepH + 0.78, 0);
  g.add(flag);

  const gate = new THREE.Mesh(new THREE.BoxGeometry(0.72, 0.92, 0.22), dark);
  gate.position.set(0, 0.5 + 0.46, half + 0.04);
  g.add(gate);

  return g;
}
