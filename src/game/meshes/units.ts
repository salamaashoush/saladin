import * as THREE from 'three';
import { UNIT_DEFS, UnitKind } from '../../../shared/index.ts';

// Shared, faction-independent materials. Cloth/banner accents use the per-call
// `tunic` material so factions read at a glance; everything else is shared so
// many instances on screen stay cheap.
const SKIN = new THREE.MeshStandardMaterial({ color: 0xd9a878, flatShading: true });
const SKIN_DARK = new THREE.MeshStandardMaterial({ color: 0xb07a4a, flatShading: true });
const METAL = new THREE.MeshStandardMaterial({
  color: 0x9aa0a6,
  metalness: 0.45,
  roughness: 0.5,
  flatShading: true,
});
const STEEL = new THREE.MeshStandardMaterial({
  color: 0xc4c9cf,
  metalness: 0.6,
  roughness: 0.35,
  flatShading: true,
});
const IRON = new THREE.MeshStandardMaterial({
  color: 0x4a4d52,
  metalness: 0.5,
  roughness: 0.55,
  flatShading: true,
});
const WOOD = new THREE.MeshStandardMaterial({ color: 0x6b4a2b, flatShading: true });
const WOOD_DARK = new THREE.MeshStandardMaterial({ color: 0x4a3522, flatShading: true });
const LEATHER = new THREE.MeshStandardMaterial({ color: 0x7a5230, flatShading: true });
const ROPE = new THREE.MeshStandardMaterial({ color: 0xb9a06a, flatShading: true });
const GOLD = new THREE.MeshStandardMaterial({
  color: 0xd6b24a,
  metalness: 0.6,
  roughness: 0.35,
  flatShading: true,
});
const WHITE_CLOTH = new THREE.MeshStandardMaterial({ color: 0xf2efe6, flatShading: true });
const GREEN_SASH = new THREE.MeshStandardMaterial({ color: 0x2f7d4f, flatShading: true });
const HIDE_BAY = new THREE.MeshStandardMaterial({ color: 0x6a4a2a, flatShading: true });
const HIDE_DARK = new THREE.MeshStandardMaterial({ color: 0x33251a, flatShading: true });
const HIDE_GREY = new THREE.MeshStandardMaterial({ color: 0x5a4632, flatShading: true });

function mesh(
  geo: THREE.BufferGeometry,
  mat: THREE.Material,
  x = 0,
  y = 0,
  z = 0
): THREE.Mesh {
  const m = new THREE.Mesh(geo, mat);
  m.position.set(x, y, z);
  return m;
}

// A four-legged horse body centred on origin, top of back ~ saddleY. Forward is
// +Z (matches the rest of the unit kit and the game's facing convention).
function buildHorse(r: number, h: number, hide: THREE.Material): THREE.Group {
  const g = new THREE.Group();
  const body = mesh(new THREE.BoxGeometry(r * 0.85, h * 0.38, h * 1.05), hide, 0, h * 0.32, 0);
  body.castShadow = true;
  g.add(body);
  // Chest taper toward the front.
  const chest = mesh(new THREE.BoxGeometry(r * 0.78, h * 0.34, h * 0.3), hide, 0, h * 0.34, h * 0.5);
  g.add(chest);
  const rump = mesh(new THREE.BoxGeometry(r * 0.82, h * 0.4, h * 0.32), hide, 0, h * 0.36, -h * 0.5);
  g.add(rump);
  const neck = mesh(new THREE.BoxGeometry(r * 0.42, h * 0.5, r * 0.45), hide, 0, h * 0.55, h * 0.5);
  neck.rotation.x = -0.55;
  g.add(neck);
  const headM = mesh(new THREE.BoxGeometry(r * 0.36, r * 0.42, h * 0.42), hide, 0, h * 0.74, h * 0.72);
  headM.rotation.x = 0.2;
  g.add(headM);
  const muzzle = mesh(new THREE.BoxGeometry(r * 0.28, r * 0.3, h * 0.2), hide, 0, h * 0.66, h * 0.9);
  g.add(muzzle);
  // Ears.
  for (const sx of [-1, 1] as const)
    g.add(mesh(new THREE.ConeGeometry(r * 0.08, r * 0.18, 4), hide, sx * r * 0.12, h * 0.9, h * 0.62));
  // Mane.
  const mane = mesh(new THREE.BoxGeometry(r * 0.12, h * 0.5, r * 0.12), HIDE_DARK, 0, h * 0.62, h * 0.46);
  mane.rotation.x = -0.55;
  g.add(mane);
  // Tail.
  const tail = mesh(new THREE.ConeGeometry(r * 0.12, h * 0.45, 5), HIDE_DARK, 0, h * 0.32, -h * 0.66);
  tail.rotation.x = 0.7;
  g.add(tail);
  // Legs, slightly splayed for a planted stance.
  for (const sx of [-1, 1] as const)
    for (const sz of [-1, 1] as const) {
      const leg = mesh(
        new THREE.CylinderGeometry(r * 0.11, r * 0.09, h * 0.36, 5),
        hide,
        sx * r * 0.3,
        h * 0.13,
        sz * h * 0.4
      );
      g.add(leg);
      g.add(mesh(new THREE.BoxGeometry(r * 0.16, r * 0.1, r * 0.2), IRON, sx * r * 0.3, h * 0.03, sz * h * 0.4));
    }
  return g;
}

// A small saddle blanket in the team colour, draped over the horse's back.
function addCaparison(g: THREE.Group, r: number, h: number, tunic: THREE.Material): void {
  const blanket = mesh(new THREE.BoxGeometry(r * 1.0, h * 0.06, h * 0.7), tunic, 0, h * 0.5, -h * 0.02);
  g.add(blanket);
  for (const sx of [-1, 1] as const) {
    const skirt = mesh(new THREE.BoxGeometry(0.04, h * 0.22, h * 0.6), tunic, sx * r * 0.5, h * 0.4, -h * 0.02);
    g.add(skirt);
  }
}

export function buildUnit(kind: number, color: number): THREE.Group {
  const def = UNIT_DEFS[kind as UnitKind] ?? UNIT_DEFS[UnitKind.Peasant];
  const pivot = new THREE.Group();
  const h = def.height;
  const r = def.radius;
  const tunic = new THREE.MeshStandardMaterial({ color, flatShading: true });
  pivot.userData.tintMat = tunic;

  const isMounted =
    kind === UnitKind.Knight ||
    kind === UnitKind.HorseArcher ||
    kind === UnitKind.Mamluk;
  const isSiege = kind === UnitKind.Ram || kind === UnitKind.Mangonel;

  // ---- Infantry / rider body (skipped for siege engines) ----
  if (!isSiege) {
    // Mounted riders sit higher; their torso origin shifts up onto the saddle.
    const baseY = isMounted ? h * 0.55 : 0;

    if (isMounted) {
      // Splayed riding legs straddling the horse.
      for (const sx of [-1, 1] as const) {
        const thigh = mesh(
          new THREE.BoxGeometry(r * 0.28, h * 0.34, r * 0.32),
          tunic,
          sx * r * 0.42,
          baseY + h * 0.06,
          0
        );
        thigh.rotation.z = sx * 0.35;
        pivot.add(thigh);
        const shin = mesh(
          new THREE.CylinderGeometry(r * 0.12, r * 0.1, h * 0.36, 5),
          LEATHER,
          sx * r * 0.55,
          baseY - h * 0.18,
          0
        );
        pivot.add(shin);
      }
    } else {
      const legGeo = new THREE.CylinderGeometry(r * 0.24, r * 0.2, h * 0.46, 6);
      const legL = mesh(legGeo, LEATHER, -r * 0.32, h * 0.23, 0);
      const legR = mesh(legGeo, LEATHER, r * 0.32, h * 0.23, 0);
      pivot.add(legL, legR);
      // Boots.
      for (const sx of [-1, 1] as const)
        pivot.add(mesh(new THREE.BoxGeometry(r * 0.28, r * 0.18, r * 0.42), WOOD_DARK, sx * r * 0.32, h * 0.04, r * 0.08));
    }

    // Torso: tapered tunic with a faint shoulder line.
    const torso = mesh(
      new THREE.CylinderGeometry(r * 0.7, r * 0.92, h * 0.6, 8),
      tunic,
      0,
      baseY + h * 0.72,
      0
    );
    torso.castShadow = true;
    pivot.add(torso);
    // Belt.
    pivot.add(
      mesh(new THREE.CylinderGeometry(r * 0.74, r * 0.74, h * 0.07, 8), LEATHER, 0, baseY + h * 0.46, 0)
    );

    // Shoulders + slightly forward-set arms for an alert idle pose. Arms are
    // omitted for the heavily-cloaked Imam (handled in its own branch).
    if (kind !== UnitKind.Imam) {
      for (const sx of [-1, 1] as const) {
        const shoulder = mesh(
          new THREE.SphereGeometry(r * 0.26, 6, 5),
          tunic,
          sx * r * 0.72,
          baseY + h * 0.92,
          0
        );
        pivot.add(shoulder);
        const arm = mesh(
          new THREE.CylinderGeometry(r * 0.16, r * 0.18, h * 0.42, 5),
          tunic,
          sx * r * 0.78,
          baseY + h * 0.7,
          r * 0.08
        );
        arm.rotation.x = -0.25;
        pivot.add(arm);
      }
    }

    // Head + neck.
    pivot.add(mesh(new THREE.CylinderGeometry(r * 0.26, r * 0.3, h * 0.1, 6), SKIN, 0, baseY + h * 1.0, 0));
    const head = mesh(new THREE.SphereGeometry(r * 0.62, 9, 7), SKIN, 0, baseY + h * 1.12, 0);
    head.scale.set(0.9, 1.05, 0.95);
    head.castShadow = true;
    pivot.add(head);

    // ---- Per-unit kit ----
    if (kind === UnitKind.Peasant) {
      // Bare-headed labourer with a wide-brimmed straw hat and a hoe/tool.
      const hat = mesh(new THREE.ConeGeometry(r * 0.85, r * 0.45, 8), ROPE, 0, baseY + h * 1.26, 0);
      pivot.add(hat);
      pivot.add(mesh(new THREE.CylinderGeometry(r * 0.85, r * 0.85, 0.03, 8), ROPE, 0, baseY + h * 1.18, 0));
      // Simple sleeveless jerkin band.
      pivot.add(
        mesh(new THREE.CylinderGeometry(r * 0.72, r * 0.72, h * 0.18, 8), LEATHER, 0, baseY + h * 0.86, 0)
      );
      // Tool: a long haft with an angled iron hoe head, held across the body.
      const haft = mesh(new THREE.CylinderGeometry(0.025, 0.025, h * 1.6, 5), WOOD, r * 0.85, baseY + h * 0.8, 0);
      haft.rotation.z = 0.12;
      pivot.add(haft);
      const hoe = mesh(new THREE.BoxGeometry(0.06, r * 0.5, 0.16), IRON, r * 1.02, baseY + h * 1.5, 0);
      hoe.rotation.z = 0.9;
      pivot.add(hoe);
    } else if (kind === UnitKind.Spearman) {
      // Conical nasal helm + round shield + long spear.
      const helm = mesh(new THREE.ConeGeometry(r * 0.66, r * 0.7, 8), STEEL, 0, baseY + h * 1.34, 0);
      pivot.add(helm);
      pivot.add(mesh(new THREE.SphereGeometry(0.04, 5, 4), STEEL, 0, baseY + h * 1.56, 0));
      // Nasal guard.
      pivot.add(mesh(new THREE.BoxGeometry(0.05, r * 0.3, 0.04), STEEL, 0, baseY + h * 1.06, r * 0.55));
      // Round shield on the left arm: faced in team colour with a metal boss.
      const shield = mesh(new THREE.CylinderGeometry(r * 0.7, r * 0.7, 0.07, 14), tunic, -r * 1.0, baseY + h * 0.74, r * 0.12);
      shield.rotation.x = Math.PI / 2;
      shield.rotation.z = 0.15;
      pivot.add(shield);
      pivot.add(mesh(new THREE.SphereGeometry(r * 0.16, 7, 5), STEEL, -r * 1.04, baseY + h * 0.74, r * 0.18));
      // Spear, planted upright in the right hand.
      const spear = mesh(new THREE.CylinderGeometry(0.028, 0.032, h * 2.6, 5), WOOD, r * 0.92, baseY + h * 1.0, 0);
      pivot.add(spear);
      pivot.add(mesh(new THREE.ConeGeometry(0.06, 0.28, 6), STEEL, r * 0.92, baseY + h * 2.3, 0));
      // Leaf-blade collar.
      pivot.add(mesh(new THREE.BoxGeometry(0.02, 0.1, 0.12), STEEL, r * 0.92, baseY + h * 2.1, 0));
    } else if (kind === UnitKind.Archer) {
      // Pointed hood (team colour), shouldered bow and a back quiver.
      const hood = mesh(new THREE.ConeGeometry(r * 0.72, r * 0.85, 7), tunic, 0, baseY + h * 1.28, -r * 0.04);
      pivot.add(hood);
      // Cowl drape onto the shoulders.
      pivot.add(mesh(new THREE.SphereGeometry(r * 0.6, 8, 6, 0, Math.PI * 2, 0, Math.PI / 2), tunic, 0, baseY + h * 1.02, -r * 0.06));
      // Curved bow strung vertically in the left hand.
      const bow = mesh(new THREE.TorusGeometry(r * 0.95, 0.035, 5, 12, Math.PI * 1.25), WOOD, -r * 1.0, baseY + h * 0.85, r * 0.05);
      bow.rotation.z = Math.PI / 2 - 0.65;
      pivot.add(bow);
      // Bowstring.
      pivot.add(mesh(new THREE.CylinderGeometry(0.006, 0.006, r * 1.7, 3), ROPE, -r * 0.86, baseY + h * 0.85, r * 0.05));
      // Quiver of arrows slung on the back.
      const quiver = mesh(new THREE.CylinderGeometry(r * 0.2, r * 0.24, h * 0.55, 6), LEATHER, r * 0.5, baseY + h * 0.95, -r * 0.4);
      quiver.rotation.x = 0.3;
      pivot.add(quiver);
      for (const dx of [-0.06, 0, 0.06]) {
        pivot.add(mesh(new THREE.CylinderGeometry(0.012, 0.012, h * 0.4, 4), WOOD, r * 0.5 + dx, baseY + h * 1.32, -r * 0.5));
        pivot.add(mesh(new THREE.ConeGeometry(0.04, 0.08, 4), tunic, r * 0.5 + dx, baseY + h * 1.5, -r * 0.5));
      }
    } else if (kind === UnitKind.Knight) {
      // Heavy mounted lancer: great helm, mail drape (surcoat in team colour),
      // kite shield and a couched lance.
      const horse = buildHorse(r, h, HIDE_GREY);
      addCaparison(horse, r, h, tunic);
      pivot.add(horse);
      // Surcoat skirt over the saddle.
      pivot.add(mesh(new THREE.ConeGeometry(r * 0.85, h * 0.45, 8, 1, true), tunic, 0, baseY + h * 0.42, 0));
      // Great helm.
      const helm = mesh(new THREE.CylinderGeometry(r * 0.5, r * 0.52, h * 0.42, 8), STEEL, 0, baseY + h * 1.18, 0);
      pivot.add(helm);
      pivot.add(mesh(new THREE.SphereGeometry(r * 0.5, 8, 5, 0, Math.PI * 2, 0, Math.PI / 2), STEEL, 0, baseY + h * 1.38, 0));
      // Visor slit.
      pivot.add(mesh(new THREE.BoxGeometry(r * 0.6, 0.04, 0.04), IRON, 0, baseY + h * 1.2, r * 0.5));
      // Crest in team colour.
      pivot.add(mesh(new THREE.BoxGeometry(0.04, r * 0.4, r * 0.45), tunic, 0, baseY + h * 1.6, 0));
      // Mail drape over the shoulders.
      pivot.add(mesh(new THREE.CylinderGeometry(r * 0.82, r * 0.86, h * 0.16, 8), METAL, 0, baseY + h * 0.9, 0));
      // Kite shield on the left, faced in team colour.
      const shield = new THREE.Mesh(new THREE.BufferGeometry(), tunic);
      const kite = new THREE.Mesh(new THREE.ConeGeometry(r * 0.5, h * 0.85, 3), tunic);
      kite.position.set(-r * 0.95, baseY + h * 0.6, r * 0.2);
      kite.rotation.set(Math.PI, 0.2, 0);
      kite.scale.set(1, 1, 0.18);
      pivot.add(shield, kite);
      // Couched lance angled forward (+Z).
      const lance = mesh(new THREE.CylinderGeometry(0.03, 0.045, h * 2.9, 6), WOOD, r * 0.85, baseY + h * 0.78, h * 0.2);
      lance.rotation.x = Math.PI / 2 - 0.12;
      pivot.add(lance);
      pivot.add(mesh(new THREE.ConeGeometry(0.07, 0.32, 6), STEEL, r * 0.85, baseY + h * 0.95, h * 1.65));
      // Pennon on the lance.
      const pennon = mesh(new THREE.BoxGeometry(0.02, r * 0.5, r * 0.7), tunic, r * 0.85, baseY + h * 1.05, h * 0.95);
      pivot.add(pennon);
    } else if (kind === UnitKind.HorseArcher) {
      // Light, fast steppe-style cavalry: turban, recurve bow, minimal armour.
      const horse = buildHorse(r, h, HIDE_BAY);
      pivot.add(horse);
      // Light saddle blanket fringe in team colour.
      pivot.add(mesh(new THREE.BoxGeometry(r * 0.9, h * 0.05, h * 0.55), tunic, 0, baseY * 0.92, 0));
      // Wrapped turban.
      const turban = mesh(new THREE.SphereGeometry(r * 0.5, 8, 6), WHITE_CLOTH, 0, baseY + h * 1.18, 0);
      turban.scale.y = 0.78;
      pivot.add(turban);
      pivot.add(mesh(new THREE.TorusGeometry(r * 0.5, r * 0.12, 5, 10), tunic, 0, baseY + h * 1.1, 0));
      // Sash across the chest.
      const sash = mesh(new THREE.BoxGeometry(r * 1.4, r * 0.22, 0.04), tunic, 0, baseY + h * 0.75, r * 0.32);
      sash.rotation.z = 0.5;
      pivot.add(sash);
      // Recurve bow drawn, held out to the left.
      const bow = mesh(new THREE.TorusGeometry(r * 0.85, 0.03, 5, 14, Math.PI * 1.15), WOOD, -r * 1.0, baseY + h * 0.92, r * 0.1);
      bow.rotation.set(0, 0.3, Math.PI / 2 - 0.5);
      pivot.add(bow);
      pivot.add(mesh(new THREE.CylinderGeometry(0.005, 0.005, r * 1.5, 3), ROPE, -r * 0.88, baseY + h * 0.92, r * 0.1));
      // Quiver at the hip.
      const quiver = mesh(new THREE.CylinderGeometry(r * 0.16, r * 0.2, h * 0.4, 6), LEATHER, r * 0.6, baseY + h * 0.5, -r * 0.2);
      quiver.rotation.x = 0.25;
      pivot.add(quiver);
    } else if (kind === UnitKind.Mamluk) {
      // Ornate elite cavalry: lamellar coat, plumed helm, raised sabre.
      const horse = buildHorse(r, h, HIDE_DARK);
      addCaparison(horse, r, h, tunic);
      pivot.add(horse);
      // Lamellar / scale coat.
      pivot.add(mesh(new THREE.CylinderGeometry(r * 0.78, r * 0.86, h * 0.5, 8), METAL, 0, baseY + h * 0.72, 0));
      // Gilded chest plate.
      pivot.add(mesh(new THREE.BoxGeometry(r * 0.5, h * 0.3, 0.05), GOLD, 0, baseY + h * 0.82, r * 0.42));
      // Pointed helm with mail aventail and a tall plume.
      const helm = mesh(new THREE.ConeGeometry(r * 0.52, h * 0.5, 8), STEEL, 0, baseY + h * 1.28, 0);
      pivot.add(helm);
      pivot.add(mesh(new THREE.SphereGeometry(0.045, 5, 4), GOLD, 0, baseY + h * 1.55, 0));
      pivot.add(mesh(new THREE.CylinderGeometry(r * 0.46, r * 0.5, h * 0.14, 8), METAL, 0, baseY + h * 1.02, 0));
      // Plume in team colour.
      const plume = mesh(new THREE.ConeGeometry(r * 0.12, h * 0.55, 5), tunic, 0, baseY + h * 1.75, -r * 0.05);
      plume.rotation.x = -0.3;
      pivot.add(plume);
      // Raised curved sabre in the right hand.
      const sabre = mesh(new THREE.TorusGeometry(r * 0.55, 0.03, 5, 10, Math.PI * 0.85), STEEL, r * 1.0, baseY + h * 1.2, 0);
      sabre.rotation.set(0.2, 0, -0.5);
      pivot.add(sabre);
      pivot.add(mesh(new THREE.BoxGeometry(0.05, r * 0.22, 0.05), GOLD, r * 0.95, baseY + h * 0.95, 0));
      // Small round shield on the off side.
      const shield = mesh(new THREE.CylinderGeometry(r * 0.45, r * 0.45, 0.06, 12), tunic, -r * 0.95, baseY + h * 0.8, r * 0.1);
      shield.rotation.x = Math.PI / 2;
      pivot.add(shield);
      pivot.add(mesh(new THREE.SphereGeometry(r * 0.12, 6, 5), GOLD, -r * 0.99, baseY + h * 0.8, r * 0.14));
    } else if (kind === UnitKind.Crossbowman) {
      // Kettle helm, levelled crossbow, large pavise shield planted in front.
      const helm = mesh(new THREE.SphereGeometry(r * 0.6, 8, 6, 0, Math.PI * 2, 0, Math.PI / 2), STEEL, 0, baseY + h * 1.18, 0);
      helm.scale.y = 0.7;
      pivot.add(helm);
      pivot.add(mesh(new THREE.CylinderGeometry(r * 0.75, r * 0.75, 0.05, 12), STEEL, 0, baseY + h * 1.12, 0)); // brim
      // Crossbow: stock + prod (limbs) levelled forward.
      const stock = mesh(new THREE.BoxGeometry(0.07, 0.08, h * 0.95), WOOD, -r * 0.7, baseY + h * 0.92, r * 0.3);
      pivot.add(stock);
      const prod = mesh(new THREE.BoxGeometry(r * 1.5, 0.05, 0.07), STEEL, -r * 0.7, baseY + h * 0.92, r * 0.7);
      prod.rotation.y = 0.15;
      pivot.add(prod);
      // Bolt loaded.
      pivot.add(mesh(new THREE.CylinderGeometry(0.012, 0.012, h * 0.5, 4), IRON, -r * 0.7, baseY + h * 0.94, r * 0.6));
      // Pavise: a tall body shield planted on the ground beside the soldier.
      const pavise = mesh(new THREE.BoxGeometry(r * 1.1, h * 1.2, 0.08), tunic, r * 1.15, baseY + h * 0.35, r * 0.15);
      pavise.castShadow = true;
      pivot.add(pavise);
      // Pavise rib + spike.
      pivot.add(mesh(new THREE.BoxGeometry(0.06, h * 1.1, 0.1), WOOD_DARK, r * 1.15, baseY + h * 0.35, r * 0.2));
      pivot.add(mesh(new THREE.ConeGeometry(0.05, h * 0.2, 5), IRON, r * 1.15, baseY - h * 0.32, r * 0.15));
    } else if (kind === UnitKind.Imam) {
      // Robed support figure: a flowing robe, white turban, prayer staff — no
      // weapon. The tint marks the owner on the robe and sash.
      // (Arms intentionally omitted above; the robe envelops them.)
      const robe = mesh(new THREE.ConeGeometry(r * 1.05, h * 1.15, 10), tunic, 0, baseY + h * 0.5, 0);
      robe.castShadow = true;
      pivot.add(robe);
      // Robe overlay panel for layered cloth.
      pivot.add(mesh(new THREE.ConeGeometry(r * 0.78, h * 0.7, 8), WHITE_CLOTH, 0, baseY + h * 0.78, r * 0.04));
      // Wide green sash.
      pivot.add(mesh(new THREE.CylinderGeometry(r * 0.7, r * 0.78, h * 0.12, 10), GREEN_SASH, 0, baseY + h * 0.74, 0));
      // Layered turban.
      const turban = mesh(new THREE.SphereGeometry(r * 0.6, 10, 8), WHITE_CLOTH, 0, baseY + h * 1.18, 0);
      turban.scale.y = 0.7;
      pivot.add(turban);
      pivot.add(mesh(new THREE.TorusGeometry(r * 0.58, r * 0.14, 6, 10), WHITE_CLOTH, 0, baseY + h * 1.1, 0));
      // Turban tail in team colour.
      pivot.add(mesh(new THREE.BoxGeometry(0.04, h * 0.3, r * 0.3), tunic, 0, baseY + h * 1.0, -r * 0.5));
      // Tall staff with a gilded knob, held upright.
      pivot.add(mesh(new THREE.CylinderGeometry(0.022, 0.026, h * 1.7, 5), WOOD, r * 0.92, baseY + h * 0.78, 0));
      pivot.add(mesh(new THREE.SphereGeometry(0.07, 8, 6), GOLD, r * 0.92, baseY + h * 1.65, 0));
      pivot.add(mesh(new THREE.TorusGeometry(0.06, 0.018, 5, 8), GOLD, r * 0.92, baseY + h * 1.55, 0));
    }
  }

  // ---- Siege engines ----
  if (kind === UnitKind.Ram) {
    // Timber-roofed wheeled battering ram with an iron-capped head.
    const chassis = mesh(new THREE.BoxGeometry(h * 1.3, h * 0.12, h * 0.7), WOOD_DARK, 0, r * 0.5, 0);
    chassis.castShadow = true;
    pivot.add(chassis);
    // A-frame uprights supporting the roof.
    for (const sz of [-1, 1] as const)
      for (const sx of [-1, 1] as const) {
        const post = mesh(new THREE.CylinderGeometry(r * 0.1, r * 0.12, h * 0.85, 5), WOOD, sx * h * 0.5, h * 0.9, sz * h * 0.28);
        post.rotation.z = -sx * 0.12;
        pivot.add(post);
      }
    // Pitched plank roof in two slabs.
    for (const sz of [-1, 1] as const) {
      const slab = mesh(new THREE.BoxGeometry(h * 1.45, 0.1, h * 0.55), WOOD, 0, h * 1.18, sz * h * 0.2);
      slab.rotation.x = sz * 0.5;
      slab.castShadow = true;
      pivot.add(slab);
    }
    // Ridge beam.
    pivot.add(mesh(new THREE.BoxGeometry(h * 1.5, 0.08, 0.1), WOOD_DARK, 0, h * 1.32, 0));
    // The ram beam, slung under the roof on ropes.
    const beam = mesh(new THREE.CylinderGeometry(r * 0.28, r * 0.3, h * 1.5, 8), WOOD, 0, h * 0.78, 0);
    beam.rotation.z = Math.PI / 2;
    beam.castShadow = true;
    pivot.add(beam);
    // Iron rings + sling ropes.
    for (const sx of [-1, 1] as const) {
      pivot.add(mesh(new THREE.TorusGeometry(r * 0.32, 0.03, 5, 10), IRON, sx * h * 0.3, h * 0.78, 0));
      pivot.add(mesh(new THREE.CylinderGeometry(0.02, 0.02, h * 0.36, 4), ROPE, sx * h * 0.3, h * 0.98, 0));
    }
    // Iron ram head (animal-headed cap), pointing forward (+Z) and out the front (+X).
    const head = mesh(new THREE.ConeGeometry(r * 0.42, r * 0.8, 8), IRON, h * 0.82, h * 0.78, 0);
    head.rotation.z = -Math.PI / 2;
    pivot.add(head);
    pivot.add(mesh(new THREE.CylinderGeometry(r * 0.32, r * 0.32, r * 0.2, 8), STEEL, h * 0.7, h * 0.78, 0));
    // Four wheels.
    for (const sx of [-1, 1] as const)
      for (const sz of [-1, 1] as const) {
        const wheel = mesh(new THREE.CylinderGeometry(r * 0.42, r * 0.42, 0.12, 10), WOOD, sx * h * 0.45, r * 0.42, sz * h * 0.32);
        wheel.rotation.x = Math.PI / 2;
        wheel.castShadow = true;
        pivot.add(wheel);
        pivot.add(mesh(new THREE.TorusGeometry(r * 0.42, 0.03, 5, 10), IRON, sx * h * 0.45, r * 0.42, sz * h * 0.32).rotateX(Math.PI / 2));
      }
  } else if (kind === UnitKind.Mangonel) {
    // Wheeled traction/counterweight catapult: throwing arm cocked back, sling
    // bucket loaded, counterweight box at the short end.
    const base = mesh(new THREE.BoxGeometry(h * 0.95, 0.18, h * 1.15), WOOD_DARK, 0, r * 0.55, 0);
    base.castShadow = true;
    pivot.add(base);
    // Side rails.
    for (const sx of [-1, 1] as const)
      pivot.add(mesh(new THREE.BoxGeometry(0.1, 0.12, h * 1.1), WOOD, sx * h * 0.42, r * 0.7, 0));
    // A-frame that the arm pivots on.
    for (const sx of [-1, 1] as const) {
      const strutF = mesh(new THREE.BoxGeometry(0.08, h * 0.85, 0.08), WOOD, sx * r * 0.55, h * 0.62, h * 0.18);
      strutF.rotation.x = 0.3;
      pivot.add(strutF);
      const strutB = mesh(new THREE.BoxGeometry(0.08, h * 0.85, 0.08), WOOD, sx * r * 0.55, h * 0.62, -h * 0.18);
      strutB.rotation.x = -0.3;
      pivot.add(strutB);
    }
    // Pivot axle.
    pivot.add(mesh(new THREE.CylinderGeometry(0.05, 0.05, h * 0.9, 6), IRON, 0, h * 0.95, 0).rotateZ(Math.PI / 2));
    // Throwing arm, cocked back over the rear.
    const arm = mesh(new THREE.CylinderGeometry(0.05, 0.06, h * 1.5, 6), WOOD, 0, h * 0.78, -h * 0.18);
    arm.rotation.x = -0.85;
    arm.castShadow = true;
    pivot.add(arm);
    // Counterweight box at the short (rear, low) end.
    const cw = mesh(new THREE.BoxGeometry(r * 0.7, r * 0.7, r * 0.7), IRON, 0, h * 0.35, -h * 0.55);
    cw.castShadow = true;
    pivot.add(cw);
    pivot.add(mesh(new THREE.BoxGeometry(r * 0.74, 0.06, r * 0.74), WOOD_DARK, 0, h * 0.7, -h * 0.5));
    // Sling bucket loaded with a stone at the long (front, high) end.
    const bucket = mesh(new THREE.SphereGeometry(r * 0.4, 8, 6, 0, Math.PI * 2, 0, Math.PI / 2), LEATHER, 0, h * 1.18, h * 0.55);
    pivot.add(bucket);
    pivot.add(mesh(new THREE.SphereGeometry(r * 0.3, 7, 6), new THREE.MeshStandardMaterial({ color: 0x7a7a7a, flatShading: true }), 0, h * 1.22, h * 0.55));
    // Faction banner on a pole at the rear.
    pivot.add(mesh(new THREE.CylinderGeometry(0.02, 0.02, h * 0.9, 4), WOOD, -h * 0.4, h * 1.0, -h * 0.5));
    pivot.add(mesh(new THREE.BoxGeometry(0.02, r * 0.5, r * 0.6), tunic, -h * 0.4, h * 1.25, -h * 0.65));
    // Four wheels.
    for (const sx of [-1, 1] as const)
      for (const sz of [-1, 1] as const) {
        const wheel = mesh(new THREE.CylinderGeometry(r * 0.4, r * 0.4, 0.1, 10), WOOD, sx * h * 0.42, r * 0.4, sz * h * 0.42);
        wheel.rotation.x = Math.PI / 2;
        wheel.castShadow = true;
        pivot.add(wheel);
        pivot.add(mesh(new THREE.TorusGeometry(r * 0.4, 0.028, 5, 10), IRON, sx * h * 0.42, r * 0.4, sz * h * 0.42).rotateX(Math.PI / 2));
      }
  }

  return pivot;
}
