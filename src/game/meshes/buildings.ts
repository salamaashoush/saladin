import * as THREE from 'three';
import { BuildingKind } from '../../../shared/index.ts';

export function buildByKind(kind: number, color: number): THREE.Group {
  if (kind === BuildingKind.Wall) return buildWallSlab();
  if (kind === BuildingKind.Tower) return buildTower(color);
  if (kind === BuildingKind.Watchtower) return buildWatchtower(color);
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

// Shared palette so buildings read as one settlement, not a parts bin.
const STONE = 0x9c958a;
const STONE_DARK = 0x7d766b;
const TIMBER = 0x8a6a3a;
const TIMBER_DARK = 0x5a3a22;
const PLASTER = 0xcbb487;
const THATCH = 0x9a7a45;

function stoneMat() {
  return new THREE.MeshStandardMaterial({ color: STONE, roughness: 1, flatShading: true });
}
function stoneDarkMat() {
  return new THREE.MeshStandardMaterial({ color: STONE_DARK, roughness: 1, flatShading: true });
}
function timberMat() {
  return new THREE.MeshStandardMaterial({ color: TIMBER, roughness: 1, flatShading: true });
}
function timberDarkMat() {
  return new THREE.MeshStandardMaterial({ color: TIMBER_DARK, roughness: 1, flatShading: true });
}

// A pennant on a leaning pole — reads as a triangular war banner rather than a
// flat plane. `team` is the faction-tinted cloth so ownership is obvious.
function pennant(
  g: THREE.Group,
  team: THREE.Material,
  x: number,
  y: number,
  z: number,
  h = 0.7
) {
  const pole = new THREE.Mesh(new THREE.CylinderGeometry(0.035, 0.045, h, 5), timberDarkMat());
  pole.position.set(x, y, z);
  pole.castShadow = true;
  g.add(pole);
  const finial = new THREE.Mesh(new THREE.SphereGeometry(0.06, 6, 4), team);
  finial.position.set(x, y + h / 2 + 0.04, z);
  g.add(finial);
  // Swallow-tail banner built from two skewed triangles.
  const cloth = new THREE.Shape();
  cloth.moveTo(0, 0);
  cloth.lineTo(0.46, 0.05);
  cloth.lineTo(0.34, 0.105);
  cloth.lineTo(0.46, 0.16);
  cloth.lineTo(0, 0.21);
  cloth.closePath();
  const f = new THREE.Mesh(new THREE.ShapeGeometry(cloth), team);
  f.position.set(x + 0.02, y + h / 2 - 0.24, z);
  g.add(f);
}

// Crenellated parapet ring stamped around a square top edge.
function squareMerlons(
  g: THREE.Group,
  mat: THREE.Material,
  cx: number,
  topY: number,
  cz: number,
  span: number,
  size = 0.26,
  height = 0.3
) {
  const half = span / 2 - size / 2;
  const steps = Math.max(2, Math.round(span / (size * 1.7)));
  for (let i = 0; i <= steps; i++) {
    const t = i / steps - 0.5;
    for (const [ox, oz] of [
      [t * span, -half],
      [t * span, half],
      [-half, t * span],
      [half, t * span],
    ]) {
      // Skip the inner duplicates on corners by only placing perimeter band.
      if (Math.abs(ox) > half + 0.001 || Math.abs(oz) > half + 0.001) continue;
      const m = new THREE.Mesh(new THREE.BoxGeometry(size, height, size), mat);
      m.position.set(cx + ox, topY + height / 2, cz + oz);
      g.add(m);
    }
  }
}

// Tall narrow recess that reads as an arrow loop / window when slightly inset.
function arrowSlit(g: THREE.Group, x: number, y: number, z: number, ry = 0) {
  const slit = new THREE.Mesh(
    new THREE.BoxGeometry(0.08, 0.4, 0.06),
    new THREE.MeshStandardMaterial({ color: 0x2a2620, roughness: 1, flatShading: true })
  );
  slit.position.set(x, y, z);
  slit.rotation.y = ry;
  g.add(slit);
}

// Open-fronted horse shed: timber barn, hay, a paddock rail and a horse hint.
export function buildStable(color: number): THREE.Group {
  const g = new THREE.Group();
  const wall = new THREE.MeshStandardMaterial({ color: 0x9a7a4a, roughness: 1, flatShading: true });
  const roofMat = new THREE.MeshStandardMaterial({ color: 0x6b4a2b, roughness: 1, flatShading: true });
  const dark = timberDarkMat();
  const hayMat = new THREE.MeshStandardMaterial({ color: 0xc9a64a, roughness: 1, flatShading: true });
  const horseMat = new THREE.MeshStandardMaterial({ color: 0x5b4632, roughness: 1, flatShading: true });
  const team = teamMat(color);
  g.userData.tintMat = team;

  // Back wall + two stall partitions, leaving the front open.
  const back = new THREE.Mesh(new THREE.BoxGeometry(2, 0.95, 0.18), wall);
  back.position.set(0, 0.55, -0.78);
  back.castShadow = true;
  back.receiveShadow = true;
  g.add(back);
  for (const x of [-1, 0, 1]) {
    const part = new THREE.Mesh(new THREE.BoxGeometry(0.16, 0.85, 1.4), wall);
    part.position.set(x, 0.5, -0.1);
    part.castShadow = true;
    g.add(part);
  }
  // Pitched plank roof (two slabs) over the stalls.
  for (const s of [-1, 1]) {
    const slope = new THREE.Mesh(new THREE.BoxGeometry(2.15, 0.12, 1.0), roofMat);
    slope.position.set(0, 1.12, s * 0.42);
    slope.rotation.x = s * 0.32;
    slope.castShadow = true;
    g.add(slope);
  }
  const ridge = new THREE.Mesh(new THREE.BoxGeometry(2.2, 0.12, 0.16), dark);
  ridge.position.set(0, 1.32, 0);
  g.add(ridge);
  // Hay bale + loose pile.
  const bale = new THREE.Mesh(new THREE.BoxGeometry(0.55, 0.36, 0.4), hayMat);
  bale.position.set(-0.7, 0.18, 0.62);
  bale.castShadow = true;
  g.add(bale);
  const pile = new THREE.Mesh(new THREE.ConeGeometry(0.32, 0.32, 6), hayMat);
  pile.position.set(0.85, 0.16, 0.55);
  g.add(pile);
  // Horse hint: a low body block + neck + head poking from a stall.
  const horseBody = new THREE.Mesh(new THREE.BoxGeometry(0.7, 0.32, 0.26), horseMat);
  horseBody.position.set(0.35, 0.46, 0.1);
  g.add(horseBody);
  const neck = new THREE.Mesh(new THREE.BoxGeometry(0.16, 0.3, 0.16), horseMat);
  neck.position.set(0.68, 0.66, 0.1);
  neck.rotation.z = -0.4;
  g.add(neck);
  const head = new THREE.Mesh(new THREE.BoxGeometry(0.26, 0.16, 0.14), horseMat);
  head.position.set(0.82, 0.78, 0.1);
  g.add(head);
  // Paddock rail at the front.
  for (const z of [0.78, 0.96]) {
    const rail = new THREE.Mesh(new THREE.BoxGeometry(1.9, 0.06, 0.06), dark);
    rail.position.set(0, 0.52 + (z - 0.78) * 1.1, z);
    g.add(rail);
  }
  for (const x of [-0.92, 0, 0.92]) {
    const post = new THREE.Mesh(new THREE.BoxGeometry(0.08, 0.72, 0.08), dark);
    post.position.set(x, 0.36, 0.9);
    g.add(post);
  }
  pennant(g, team, 0.92, 1.55, -0.7);
  return g;
}

// Stone smithy: pitched roof, a tall venting chimney with ember glow, an anvil
// out front lit by the forge.
export function buildBlacksmith(color: number): THREE.Group {
  const g = new THREE.Group();
  const stone = stoneMat();
  const roofMat = new THREE.MeshStandardMaterial({ color: 0x3a3a3a, roughness: 1, flatShading: true });
  const ember = new THREE.MeshStandardMaterial({ color: 0xd9531e, emissive: 0x8a2a00, emissiveIntensity: 1.4, flatShading: true });
  const metal = new THREE.MeshStandardMaterial({ color: 0x3c3c40, metalness: 0.5, roughness: 0.6, flatShading: true });
  const team = teamMat(color);
  g.userData.tintMat = team;

  const base = new THREE.Mesh(new THREE.BoxGeometry(1.9, 1.05, 1.9), stone);
  base.position.y = 0.525;
  base.castShadow = true;
  base.receiveShadow = true;
  g.add(base);
  // Stone-trim quoins at the corners read as masonry.
  for (const sx of [-1, 1])
    for (const sz of [-1, 1]) {
      const quoin = new THREE.Mesh(new THREE.BoxGeometry(0.2, 1.05, 0.2), stoneDarkMat());
      quoin.position.set(sx * 0.85, 0.525, sz * 0.85);
      g.add(quoin);
    }
  for (const s of [-1, 1]) {
    const slope = new THREE.Mesh(new THREE.BoxGeometry(2.05, 0.12, 1.05), roofMat);
    slope.position.set(0, 1.18, s * 0.45);
    slope.rotation.x = s * 0.34;
    slope.castShadow = true;
    g.add(slope);
  }
  const chimney = new THREE.Mesh(new THREE.BoxGeometry(0.4, 1.15, 0.4), stoneDarkMat());
  chimney.position.set(0.62, 1.55, -0.55);
  chimney.castShadow = true;
  g.add(chimney);
  const chimTop = new THREE.Mesh(new THREE.BoxGeometry(0.5, 0.14, 0.5), stoneDarkMat());
  chimTop.position.set(0.62, 2.16, -0.55);
  g.add(chimTop);
  const glow = new THREE.Mesh(new THREE.BoxGeometry(0.26, 0.12, 0.26), ember);
  glow.position.set(0.62, 2.24, -0.55);
  g.add(glow);
  // Forge mouth glowing under the roof.
  const forge = new THREE.Mesh(new THREE.BoxGeometry(0.5, 0.42, 0.22), ember);
  forge.position.set(-0.3, 0.42, 1.0);
  g.add(forge);
  // Anvil: stump + horned top.
  const stump = new THREE.Mesh(new THREE.CylinderGeometry(0.16, 0.18, 0.34, 7), timberDarkMat());
  stump.position.set(0.55, 0.17, 0.9);
  g.add(stump);
  const anvilBody = new THREE.Mesh(new THREE.BoxGeometry(0.34, 0.13, 0.18), metal);
  anvilBody.position.set(0.55, 0.4, 0.9);
  g.add(anvilBody);
  const horn = new THREE.Mesh(new THREE.ConeGeometry(0.07, 0.2, 5), metal);
  horn.rotation.z = -Math.PI / 2;
  horn.position.set(0.78, 0.42, 0.9);
  g.add(horn);
  pennant(g, team, -0.78, 1.5, 0.55);
  return g;
}

// Bazaar: a tiled awning over a trestle, two side stalls and piled goods.
export function buildMarket(color: number): THREE.Group {
  const g = new THREE.Group();
  const wood = timberMat();
  const cloth = teamMat(color); // main awning carries the faction colour
  const cloth2 = new THREE.MeshStandardMaterial({ color: 0xe7d39a, roughness: 1, side: THREE.DoubleSide, flatShading: true });
  const crate = timberDarkMat();
  const fruit = new THREE.MeshStandardMaterial({ color: 0xc0532f, roughness: 1, flatShading: true });
  const grain = new THREE.MeshStandardMaterial({ color: 0xd9b35a, roughness: 1, flatShading: true });
  const pot = new THREE.MeshStandardMaterial({ color: 0x9a6a3a, roughness: 1, flatShading: true });
  const team = cloth;
  g.userData.tintMat = team;

  const counter = new THREE.Mesh(new THREE.BoxGeometry(1.9, 0.55, 0.7), wood);
  counter.position.set(0, 0.28, -0.45);
  counter.castShadow = true;
  counter.receiveShadow = true;
  g.add(counter);
  for (const sx of [-0.88, 0.88])
    for (const sz of [-0.72, 0.72]) {
      const post = new THREE.Mesh(new THREE.CylinderGeometry(0.05, 0.05, 1.5, 6), wood);
      post.position.set(sx, 0.75, sz);
      post.castShadow = true;
      g.add(post);
    }
  // Two-slope striped awning: main faction slab + accent slab.
  const awn1 = new THREE.Mesh(new THREE.BoxGeometry(2.05, 0.07, 0.95), cloth);
  awn1.position.set(0, 1.5, -0.45);
  awn1.rotation.x = -0.2;
  awn1.castShadow = true;
  g.add(awn1);
  const awn2 = new THREE.Mesh(new THREE.BoxGeometry(2.05, 0.07, 0.95), cloth2);
  awn2.position.set(0, 1.5, 0.45);
  awn2.rotation.x = 0.2;
  awn2.castShadow = true;
  g.add(awn2);
  const ridge = new THREE.Mesh(new THREE.BoxGeometry(2.1, 0.08, 0.1), wood);
  ridge.position.set(0, 1.62, 0);
  g.add(ridge);
  // Goods on the counter and ground: crates, fruit mound, grain sacks, a pot.
  for (const sx of [-0.62, 0.62]) {
    const box = new THREE.Mesh(new THREE.BoxGeometry(0.36, 0.36, 0.36), crate);
    box.position.set(sx, 0.18, 0.5);
    box.castShadow = true;
    g.add(box);
  }
  const fruitMound = new THREE.Mesh(new THREE.ConeGeometry(0.2, 0.22, 6), fruit);
  fruitMound.position.set(0, 0.66, -0.4);
  g.add(fruitMound);
  const sack = new THREE.Mesh(new THREE.CylinderGeometry(0.13, 0.17, 0.32, 7), grain);
  sack.position.set(-0.45, 0.18, 0.62);
  g.add(sack);
  const jug = new THREE.Mesh(new THREE.CylinderGeometry(0.1, 0.16, 0.34, 8), pot);
  jug.position.set(0.45, 0.59, -0.4);
  g.add(jug);
  pennant(g, team, 0.92, 1.78, 0.72);
  return g;
}

// Granary: stepped clay drum capped by a low dome — a desert silo.
export function buildGranary(color: number): THREE.Group {
  const g = new THREE.Group();
  const clay = new THREE.MeshStandardMaterial({ color: 0xcdb07a, roughness: 1, flatShading: true });
  const clayDark = new THREE.MeshStandardMaterial({ color: 0xb89a64, roughness: 1, flatShading: true });
  const dome = new THREE.MeshStandardMaterial({ color: 0xd8c590, roughness: 1, flatShading: true });
  const team = teamMat(color);
  g.userData.tintMat = team;

  const skirt = new THREE.Mesh(new THREE.CylinderGeometry(1.0, 1.1, 0.4, 12), clayDark);
  skirt.position.y = 0.2;
  skirt.castShadow = true;
  skirt.receiveShadow = true;
  g.add(skirt);
  const silo = new THREE.Mesh(new THREE.CylinderGeometry(0.82, 0.95, 1.25, 12), clay);
  silo.position.y = 1.0;
  silo.castShadow = true;
  g.add(silo);
  const band = new THREE.Mesh(new THREE.CylinderGeometry(0.86, 0.86, 0.16, 12), team);
  band.position.y = 1.2;
  g.add(band);
  // Low hemispherical dome cap.
  const cap = new THREE.Mesh(new THREE.SphereGeometry(0.82, 12, 6, 0, Math.PI * 2, 0, Math.PI / 2), dome);
  cap.position.y = 1.62;
  cap.castShadow = true;
  g.add(cap);
  const knob = new THREE.Mesh(new THREE.SphereGeometry(0.1, 6, 5), clayDark);
  knob.position.y = 2.5;
  g.add(knob);
  // Loading hatch + ramp.
  const door = new THREE.Mesh(new THREE.BoxGeometry(0.42, 0.6, 0.12), clayDark);
  door.position.set(0, 0.62, 0.92);
  g.add(door);
  const ramp = new THREE.Mesh(new THREE.BoxGeometry(0.5, 0.06, 0.5), timberDarkMat());
  ramp.position.set(0, 0.16, 1.15);
  ramp.rotation.x = 0.35;
  g.add(ramp);
  return g;
}

// Fishing hut: a planked hut on stilts over the shore with a dock and drying nets.
export function buildFishingHut(color: number): THREE.Group {
  const g = new THREE.Group();
  const wood = new THREE.MeshStandardMaterial({ color: 0x9a7a52, roughness: 1, flatShading: true });
  const roofMat = new THREE.MeshStandardMaterial({ color: 0x6a4a2a, roughness: 1, flatShading: true });
  const net = new THREE.MeshStandardMaterial({ color: 0xb8b8a0, roughness: 1, side: THREE.DoubleSide, flatShading: true });
  const team = teamMat(color);
  g.userData.tintMat = team;

  for (const sx of [-0.32, 0.32])
    for (const sz of [-0.32, 0.32]) {
      const stilt = new THREE.Mesh(new THREE.CylinderGeometry(0.055, 0.055, 0.6, 5), timberDarkMat());
      stilt.position.set(sx, 0.3, sz);
      stilt.castShadow = true;
      g.add(stilt);
    }
  const deck = new THREE.Mesh(new THREE.BoxGeometry(1.0, 0.08, 1.0), wood);
  deck.position.y = 0.6;
  deck.castShadow = true;
  g.add(deck);
  const hut = new THREE.Mesh(new THREE.BoxGeometry(0.85, 0.55, 0.85), wood);
  hut.position.y = 0.92;
  hut.castShadow = true;
  g.add(hut);
  // Lean-to single-pitch roof.
  const roof = new THREE.Mesh(new THREE.BoxGeometry(1.0, 0.1, 1.0), roofMat);
  roof.position.set(0, 1.28, 0);
  roof.rotation.x = 0.28;
  roof.castShadow = true;
  g.add(roof);
  // Plank dock reaching toward the water with two pilings.
  const dock = new THREE.Mesh(new THREE.BoxGeometry(0.5, 0.07, 1.0), wood);
  dock.position.set(0, 0.5, 0.95);
  g.add(dock);
  for (const dz of [0.7, 1.2]) {
    const pile = new THREE.Mesh(new THREE.CylinderGeometry(0.045, 0.045, 0.5, 5), timberDarkMat());
    pile.position.set(0.18, 0.28, dz);
    g.add(pile);
  }
  // Drying-net frame off the side.
  const pole = new THREE.Mesh(new THREE.CylinderGeometry(0.035, 0.035, 1.1, 5), wood);
  pole.position.set(0.72, 0.95, -0.1);
  g.add(pole);
  const netMesh = new THREE.Mesh(new THREE.PlaneGeometry(0.55, 0.55), net);
  netMesh.position.set(0.78, 0.78, 0.1);
  netMesh.rotation.y = -0.5;
  g.add(netMesh);
  // A couple of floats / fish on the deck.
  const float = new THREE.Mesh(new THREE.SphereGeometry(0.08, 6, 5), roofMat);
  float.position.set(-0.3, 0.7, 0.3);
  g.add(float);
  pennant(g, team, -0.45, 1.45, -0.35);
  return g;
}

// Siege workshop: an open timber frame, sawn lumber, and a half-built engine
// (a mangonel-like throwing arm on a wheeled bed).
export function buildSiegeWorkshop(color: number): THREE.Group {
  const g = new THREE.Group();
  const wood = new THREE.MeshStandardMaterial({ color: 0x7a5a32, roughness: 1, flatShading: true });
  const woodLight = new THREE.MeshStandardMaterial({ color: 0xa07c46, roughness: 1, flatShading: true });
  const roofMat = timberDarkMat();
  const metal = new THREE.MeshStandardMaterial({ color: 0x8a8a8a, metalness: 0.3, roughness: 0.7, flatShading: true });
  const team = teamMat(color);
  g.userData.tintMat = team;

  // Open frame: four corner posts + back wall only, leaving the front open.
  for (const sx of [-0.92, 0.92])
    for (const sz of [-0.7, 0.7]) {
      const post = new THREE.Mesh(new THREE.BoxGeometry(0.16, 1.3, 0.16), wood);
      post.position.set(sx, 0.65, sz);
      post.castShadow = true;
      g.add(post);
    }
  const back = new THREE.Mesh(new THREE.BoxGeometry(2.0, 1.1, 0.16), wood);
  back.position.set(0, 0.6, -0.78);
  back.castShadow = true;
  back.receiveShadow = true;
  g.add(back);
  // Pitched plank roof.
  for (const s of [-1, 1]) {
    const slope = new THREE.Mesh(new THREE.BoxGeometry(2.15, 0.1, 0.95), roofMat);
    slope.position.set(0, 1.42, s * 0.42);
    slope.rotation.x = s * 0.3;
    slope.castShadow = true;
    g.add(slope);
  }
  // Cross-brace on the back wall.
  const brace = new THREE.Mesh(new THREE.BoxGeometry(2.0, 0.1, 0.1), woodLight);
  brace.position.set(0, 0.9, -0.7);
  brace.rotation.z = 0.18;
  g.add(brace);
  // Stacked sawn lumber.
  for (let i = 0; i < 3; i++) {
    const log = new THREE.Mesh(new THREE.CylinderGeometry(0.07, 0.07, 1.0, 6), woodLight);
    log.rotation.x = Math.PI / 2;
    log.position.set(-0.7, 0.12 + i * 0.15, -0.4 + (i % 2) * 0.05);
    g.add(log);
  }
  // Half-built engine out front: bed, two wheels, a raised throwing arm.
  const bed = new THREE.Mesh(new THREE.BoxGeometry(0.9, 0.16, 0.5), wood);
  bed.position.set(0.15, 0.34, 0.78);
  bed.castShadow = true;
  g.add(bed);
  for (const wz of [0.55, 1.0]) {
    const wheel = new THREE.Mesh(new THREE.CylinderGeometry(0.22, 0.22, 0.1, 10), metal);
    wheel.rotation.x = Math.PI / 2;
    wheel.position.set(0.15, 0.22, wz);
    g.add(wheel);
  }
  const arm = new THREE.Mesh(new THREE.BoxGeometry(0.08, 0.8, 0.08), woodLight);
  arm.position.set(0.15, 0.7, 0.78);
  arm.rotation.x = -0.7;
  arm.castShadow = true;
  g.add(arm);
  const bucket = new THREE.Mesh(new THREE.BoxGeometry(0.16, 0.1, 0.16), metal);
  bucket.position.set(0.15, 1.02, 0.45);
  g.add(bucket);
  pennant(g, team, -0.92, 1.7, -0.7);
  return g;
}

// Gatehouse: two flanking stone towers, a recessed arched gateway, machicolated
// top with merlons and a faction banner over the arch.
export function buildGatehouse(color: number): THREE.Group {
  const g = new THREE.Group();
  const stone = stoneMat();
  const stoneDk = stoneDarkMat();
  const team = teamMat(color);
  g.userData.tintMat = team;

  for (const sx of [-0.38, 0.38]) {
    const pillar = new THREE.Mesh(new THREE.BoxGeometry(0.3, 1.45, 0.6), stone);
    pillar.position.set(sx, 0.725, 0);
    pillar.castShadow = true;
    pillar.receiveShadow = true;
    g.add(pillar);
    squareMerlons(g, stone, sx, 1.45, 0, 0.55, 0.16, 0.22);
    arrowSlit(g, sx, 1.05, 0.31);
  }
  // Arched lintel above the passage (curved top via a half-cylinder).
  const lintel = new THREE.Mesh(new THREE.BoxGeometry(0.78, 0.26, 0.58), stoneDk);
  lintel.position.y = 1.28;
  lintel.castShadow = true;
  g.add(lintel);
  const arch = new THREE.Mesh(
    new THREE.CylinderGeometry(0.32, 0.32, 0.58, 12, 1, false, 0, Math.PI),
    stoneDk
  );
  arch.rotation.z = Math.PI;
  arch.rotation.x = Math.PI / 2;
  arch.position.set(0, 1.1, 0);
  g.add(arch);
  // Dark gateway recess so the passage reads as open.
  const passage = new THREE.Mesh(
    new THREE.BoxGeometry(0.5, 1.0, 0.62),
    new THREE.MeshStandardMaterial({ color: 0x20201c, roughness: 1, flatShading: true })
  );
  passage.position.set(0, 0.5, 0);
  g.add(passage);
  // Faction banner draped over the arch.
  const banner = new THREE.Mesh(new THREE.PlaneGeometry(0.4, 0.34), team);
  banner.position.set(0, 1.18, 0.3);
  g.add(banner);
  // Battlement walkway joining the two towers.
  const walk = new THREE.Mesh(new THREE.BoxGeometry(1.0, 0.14, 0.6), stoneDk);
  walk.position.y = 1.55;
  g.add(walk);
  squareMerlons(g, stone, 0, 1.62, 0, 0.9, 0.18, 0.2);
  return g;
}

// Levantine dwelling: flat-roofed plastered cube with a parapet, a low domed
// adjacent room, shuttered window and an awning over the door.
export function buildHouse(color: number): THREE.Group {
  const g = new THREE.Group();
  const wall = new THREE.MeshStandardMaterial({ color: PLASTER, roughness: 1, flatShading: true });
  const wallWarm = new THREE.MeshStandardMaterial({ color: 0xbfa377, roughness: 1, flatShading: true });
  const dark = timberDarkMat();
  const dome = new THREE.MeshStandardMaterial({ color: 0xd2bd8e, roughness: 1, flatShading: true });
  const team = teamMat(color);
  g.userData.tintMat = team;

  // Main block, slightly off-square for a hand-built look.
  const base = new THREE.Mesh(new THREE.BoxGeometry(1.6, 1.05, 1.7), wall);
  base.position.set(-0.1, 0.525, 0);
  base.castShadow = true;
  base.receiveShadow = true;
  g.add(base);
  // Flat roof slab + low parapet so it reads as a usable rooftop.
  const roof = new THREE.Mesh(new THREE.BoxGeometry(1.66, 0.12, 1.76), wallWarm);
  roof.position.set(-0.1, 1.11, 0);
  roof.castShadow = true;
  g.add(roof);
  for (const [w, d, x, z] of [
    [1.66, 0.1, -0.1, 0.86],
    [1.66, 0.1, -0.1, -0.86],
    [0.1, 1.76, 0.71, 0],
    [0.1, 1.76, -0.91, 0],
  ] as const) {
    const par = new THREE.Mesh(new THREE.BoxGeometry(w, 0.18, d), wallWarm);
    par.position.set(x, 1.26, z);
    g.add(par);
  }
  // A small lower annex with a domed cap (a second room / oven).
  const annex = new THREE.Mesh(new THREE.BoxGeometry(0.7, 0.7, 0.7), wallWarm);
  annex.position.set(0.95, 0.35, 0.4);
  annex.castShadow = true;
  g.add(annex);
  const annexDome = new THREE.Mesh(
    new THREE.SphereGeometry(0.38, 10, 5, 0, Math.PI * 2, 0, Math.PI / 2),
    dome
  );
  annexDome.position.set(0.95, 0.7, 0.4);
  annexDome.castShadow = true;
  g.add(annexDome);
  // Door with a small awning, plus a shuttered window.
  const door = new THREE.Mesh(new THREE.BoxGeometry(0.4, 0.62, 0.12), dark);
  door.position.set(-0.1, 0.31, 0.86);
  g.add(door);
  const awning = new THREE.Mesh(new THREE.BoxGeometry(0.55, 0.05, 0.3), dark);
  awning.position.set(-0.1, 0.68, 0.98);
  awning.rotation.x = 0.3;
  g.add(awning);
  const window = new THREE.Mesh(new THREE.BoxGeometry(0.3, 0.3, 0.06), dark);
  window.position.set(-0.55, 0.7, 0.86);
  g.add(window);
  // Faction cloth hung on the rooftop pole.
  const pole = new THREE.Mesh(new THREE.CylinderGeometry(0.03, 0.03, 0.55, 5), dark);
  pole.position.set(0.5, 1.6, 0.5);
  g.add(pole);
  const flag = new THREE.Mesh(new THREE.PlaneGeometry(0.4, 0.26), team);
  flag.position.set(0.71, 1.66, 0.5);
  g.add(flag);
  return g;
}

// A straight crenellated wall slab along local X. The group is rotated to the
// run direction; the slab is long enough (1.5) to bridge orthogonal AND
// diagonal neighbours, so a line reads as one continuous wall.
export function buildWallSlab(): THREE.Group {
  const g = new THREE.Group();
  const stone = stoneMat();
  const cap = new THREE.MeshStandardMaterial({ color: 0x847c71, roughness: 1, flatShading: true });
  const H = 0.62;
  const TK = 0.42;
  const L = 1.5;

  // Slightly battered base (wider at the foot) for a heavier silhouette.
  const foot = new THREE.Mesh(new THREE.BoxGeometry(L, 0.18, TK + 0.1), stoneDarkMat());
  foot.position.y = 0.09;
  foot.receiveShadow = true;
  g.add(foot);
  const body = new THREE.Mesh(new THREE.BoxGeometry(L, H, TK), stone);
  body.position.y = H / 2;
  body.castShadow = true;
  body.receiveShadow = true;
  g.add(body);
  // Walkway cap with a slight overhang.
  const c = new THREE.Mesh(new THREE.BoxGeometry(L, 0.1, TK + 0.08), cap);
  c.position.y = H + 0.05;
  c.castShadow = true;
  g.add(c);
  const top = H + 0.1 + 0.12;
  for (const ox of [-0.52, -0.17, 0.18, 0.53]) {
    const m = new THREE.Mesh(new THREE.BoxGeometry(0.24, 0.26, TK), stone);
    m.position.set(ox, top, 0);
    g.add(m);
  }
  // Arrow slits between merlons on the outward face.
  for (const ox of [-0.35, 0.0, 0.35]) arrowSlit(g, ox, H * 0.6, TK / 2 + 0.01);
  return g;
}

// Round watch tower: a tapered stone drum, arrow slits, a corbelled parapet with
// merlons and a conical tinted cap. `tall` makes the Watchtower variant rise
// higher with an extra fighting tier and a banner.
function towerCore(color: number, tall: boolean): THREE.Group {
  const g = new THREE.Group();
  const stone = stoneMat();
  const stoneDk = stoneDarkMat();
  const team = new THREE.MeshStandardMaterial({ color, flatShading: true });
  g.userData.tintMat = team;

  const bodyH = tall ? 3.3 : 2.4;
  const r = tall ? 0.52 : 0.5;
  const base = new THREE.Mesh(new THREE.CylinderGeometry(r * 1.25, r * 1.4, 0.4, 8), stoneDk);
  base.position.y = 0.2;
  base.castShadow = true;
  base.receiveShadow = true;
  g.add(base);
  const body = new THREE.Mesh(new THREE.CylinderGeometry(r, r * 1.18, bodyH, 8), stone);
  body.position.y = 0.4 + bodyH / 2;
  body.castShadow = true;
  g.add(body);
  // String course separating tiers.
  if (tall) {
    const course = new THREE.Mesh(new THREE.CylinderGeometry(r * 1.06, r * 1.06, 0.14, 8), stoneDk);
    course.position.y = 0.4 + bodyH * 0.55;
    g.add(course);
  }
  // Arrow slits around the shaft on two levels.
  const levels = tall ? [0.45, 0.72] : [0.55];
  for (const ly of levels)
    for (let i = 0; i < 4; i++) {
      const a = (i / 4) * Math.PI * 2 + 0.4;
      const y = 0.4 + bodyH * ly;
      arrowSlit(g, Math.cos(a) * r * 1.02, y, Math.sin(a) * r * 1.02, -a);
    }
  // Corbelled parapet ring (a slightly wider drum) + merlons.
  const topY = 0.4 + bodyH;
  const parapet = new THREE.Mesh(new THREE.CylinderGeometry(r * 1.3, r * 1.15, 0.26, 8), stoneDk);
  parapet.position.y = topY + 0.13;
  parapet.castShadow = true;
  g.add(parapet);
  const mCount = 8;
  for (let i = 0; i < mCount; i++) {
    const a = (i / mCount) * Math.PI * 2;
    const cren = new THREE.Mesh(new THREE.BoxGeometry(0.2, 0.32, 0.16), stone);
    cren.position.set(Math.cos(a) * r * 1.25, topY + 0.42, Math.sin(a) * r * 1.25);
    cren.rotation.y = -a;
    g.add(cren);
  }
  const roof = new THREE.Mesh(new THREE.ConeGeometry(r * 1.35, tall ? 0.95 : 0.75, 8), team);
  roof.position.y = topY + (tall ? 1.0 : 0.85);
  roof.castShadow = true;
  g.add(roof);
  const finial = new THREE.Mesh(new THREE.SphereGeometry(0.08, 6, 5), stoneDk);
  finial.position.y = topY + (tall ? 1.55 : 1.3);
  g.add(finial);
  if (tall) pennant(g, team, r * 1.25, topY + 0.6, 0, 0.8);
  return g;
}

export function buildTower(color: number): THREE.Group {
  return towerCore(color, false);
}

export function buildWatchtower(color: number): THREE.Group {
  return towerCore(color, true);
}

// Barracks: a long timber-framed hall with exposed posts, a thatched pitched
// roof, a banner over a wide door and a training-yard weapon rack hint.
export function buildBarracks(color: number): THREE.Group {
  const g = new THREE.Group();
  const wall = new THREE.MeshStandardMaterial({ color: 0xb89a6a, roughness: 1, flatShading: true });
  const beam = timberDarkMat();
  const thatch = new THREE.MeshStandardMaterial({ color: THATCH, roughness: 1, flatShading: true });
  const team = teamMat(color);
  g.userData.tintMat = team;

  const base = new THREE.Mesh(new THREE.BoxGeometry(2, 1.2, 2), wall);
  base.position.y = 0.6;
  base.castShadow = true;
  base.receiveShadow = true;
  g.add(base);
  // Exposed timber framing: corner posts + a mid rail.
  for (const sx of [-1, 1])
    for (const sz of [-1, 1]) {
      const post = new THREE.Mesh(new THREE.BoxGeometry(0.16, 1.2, 0.16), beam);
      post.position.set(sx * 0.92, 0.6, sz * 0.92);
      g.add(post);
    }
  for (const sz of [-1, 1]) {
    const rail = new THREE.Mesh(new THREE.BoxGeometry(2.0, 0.12, 0.1), beam);
    rail.position.set(0, 0.75, sz * 0.95);
    g.add(rail);
  }
  // Thatched gable roof from two slabs + ridge.
  for (const s of [-1, 1]) {
    const slope = new THREE.Mesh(new THREE.BoxGeometry(2.2, 0.14, 1.25), thatch);
    slope.position.set(0, 1.55, s * 0.55);
    slope.rotation.x = s * 0.5;
    slope.castShadow = true;
    g.add(slope);
  }
  const ridge = new THREE.Mesh(new THREE.BoxGeometry(2.25, 0.16, 0.18), beam);
  ridge.position.set(0, 1.92, 0);
  g.add(ridge);
  // Gable ends filling the triangle.
  for (const sz of [-1, 1]) {
    const gable = new THREE.Mesh(new THREE.ConeGeometry(0.9, 0.85, 3), wall);
    gable.rotation.x = sz < 0 ? Math.PI / 2 : -Math.PI / 2;
    gable.rotation.z = Math.PI;
    gable.position.set(0, 1.55, sz * 1.0);
    g.add(gable);
  }
  const door = new THREE.Mesh(new THREE.BoxGeometry(0.55, 0.85, 0.12), beam);
  door.position.set(0, 0.42, 1.01);
  g.add(door);
  // Weapon rack: two upright spears against the wall.
  for (const sx of [0.6, 0.78]) {
    const spear = new THREE.Mesh(new THREE.CylinderGeometry(0.02, 0.02, 0.9, 4), beam);
    spear.position.set(sx, 0.45, 1.02);
    spear.rotation.z = (sx - 0.69) * 1.5;
    g.add(spear);
  }
  // Banner on a roof pole.
  const pole = new THREE.Mesh(new THREE.CylinderGeometry(0.04, 0.04, 0.85, 5), beam);
  pole.position.set(0, 2.4, 0);
  pole.castShadow = true;
  g.add(pole);
  const flag = new THREE.Mesh(new THREE.PlaneGeometry(0.55, 0.34), team);
  flag.position.set(0.28, 2.5, 0);
  g.add(flag);
  return g;
}

// Keep: a fortified donjon on a battered plinth, four corner drum towers with
// conical caps, crenellated curtain walls, a tall central tower with a banner
// and an arched main gate.
export function buildKeep(color: number): THREE.Group {
  const g = new THREE.Group();
  const stone = stoneMat();
  const dark = stoneDarkMat();
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

  // Battered plinth (wider at the foot).
  const plinth = new THREE.Mesh(new THREE.BoxGeometry(S + 0.3, 0.3, S + 0.3), dark);
  plinth.position.y = 0.15;
  plinth.receiveShadow = true;
  g.add(plinth);
  const base = new THREE.Mesh(new THREE.BoxGeometry(S, 0.3, S), dark);
  base.position.y = 0.4;
  base.castShadow = true;
  base.receiveShadow = true;
  g.add(base);

  const mkWall = (x: number, z: number, rot: number) => {
    const w = new THREE.Mesh(new THREE.BoxGeometry(S - 0.2, wallH, wallT), stone);
    w.position.set(x, 0.55 + wallH / 2, z);
    w.rotation.y = rot;
    w.castShadow = true;
    g.add(w);
    const n = 6;
    for (let i = 0; i < n; i++) {
      const cren = new THREE.Mesh(new THREE.BoxGeometry(0.3, 0.32, wallT), stone);
      const off = (i / (n - 1) - 0.5) * (S - 0.7);
      cren.position.set(
        x + (rot === 0 ? off : 0),
        0.55 + wallH + 0.16,
        z + (rot === 0 ? 0 : off)
      );
      cren.rotation.y = rot;
      g.add(cren);
    }
    // Arrow slit centred on each curtain face.
    arrowSlit(
      g,
      x + (rot === 0 ? 0 : (z > 0 ? 0 : 0)),
      0.55 + wallH * 0.55,
      z,
      rot
    );
  };
  mkWall(0, half, 0);
  mkWall(0, -half, 0);
  mkWall(half, 0, Math.PI / 2);
  mkWall(-half, 0, Math.PI / 2);

  const towerH = 2.3;
  const towerR = 0.48;
  for (const [sx, sz] of [
    [1, 1],
    [1, -1],
    [-1, -1],
    [-1, 1],
  ]) {
    const tx = sx * (S / 2 - 0.05);
    const tz = sz * (S / 2 - 0.05);
    const tower = new THREE.Mesh(
      new THREE.CylinderGeometry(towerR, towerR * 1.15, towerH, 8),
      stone
    );
    tower.position.set(tx, 0.55 + towerH / 2, tz);
    tower.castShadow = true;
    g.add(tower);
    // Parapet ring + merlons on each drum.
    const tTop = 0.55 + towerH;
    const ring = new THREE.Mesh(new THREE.CylinderGeometry(towerR * 1.2, towerR * 1.05, 0.2, 8), dark);
    ring.position.set(tx, tTop + 0.1, tz);
    g.add(ring);
    for (let i = 0; i < 6; i++) {
      const a = (i / 6) * Math.PI * 2;
      const cren = new THREE.Mesh(new THREE.BoxGeometry(0.16, 0.24, 0.13), stone);
      cren.position.set(tx + Math.cos(a) * towerR * 1.15, tTop + 0.3, tz + Math.sin(a) * towerR * 1.15);
      cren.rotation.y = -a;
      g.add(cren);
    }
    const roof = new THREE.Mesh(new THREE.ConeGeometry(towerR * 1.35, 0.8, 8), team);
    roof.position.set(tx, tTop + 0.6, tz);
    roof.castShadow = true;
    g.add(roof);
  }

  const keepH = 2.9;
  const keepS = 1.4;
  const keep = new THREE.Mesh(new THREE.BoxGeometry(keepS, keepH, keepS), stone);
  keep.position.y = 0.55 + keepH / 2;
  keep.castShadow = true;
  g.add(keep);
  // Buttress pilasters on the central tower faces.
  for (const [ax, az] of [
    [1, 0],
    [-1, 0],
    [0, 1],
    [0, -1],
  ]) {
    const but = new THREE.Mesh(new THREE.BoxGeometry(0.16, keepH, 0.16), dark);
    but.position.set(ax * keepS * 0.5, 0.55 + keepH / 2, az * keepS * 0.5);
    g.add(but);
  }
  // Crown the donjon with a full merlon ring.
  squareMerlons(g, stone, 0, 0.55 + keepH, 0, keepS + 0.1, 0.26, 0.36);
  // Upper-window arrow slits on the central tower.
  for (const [sx, sz, rot] of [
    [0, keepS / 2, 0],
    [0, -keepS / 2, 0],
    [keepS / 2, 0, Math.PI / 2],
    [-keepS / 2, 0, Math.PI / 2],
  ] as const) {
    arrowSlit(g, sx, 0.55 + keepH * 0.7, sz, rot);
  }

  const pole = new THREE.Mesh(new THREE.CylinderGeometry(0.045, 0.045, 1.1, 5), dark);
  pole.position.set(0, 0.55 + keepH + 0.7, 0);
  pole.castShadow = true;
  g.add(pole);
  pennant(g, team, 0, 0.55 + keepH + 0.7, 0, 1.1);

  // Recessed arched main gate in the front curtain.
  const gate = new THREE.Mesh(
    new THREE.BoxGeometry(0.74, 0.95, 0.24),
    new THREE.MeshStandardMaterial({ color: 0x2a241c, roughness: 1, flatShading: true })
  );
  gate.position.set(0, 0.55 + 0.47, half + 0.04);
  g.add(gate);
  const gateArch = new THREE.Mesh(
    new THREE.CylinderGeometry(0.37, 0.37, 0.24, 10, 1, false, 0, Math.PI),
    stone
  );
  gateArch.rotation.x = Math.PI / 2;
  gateArch.position.set(0, 0.55 + 0.95, half + 0.04);
  g.add(gateArch);

  return g;
}
