// Three.js render + input layer. NOT authoritative — it mirrors the module's
// tables into meshes and turns input into reducer calls. The server tick owns
// truth; the client interpolates between the position snapshots it receives.
import * as THREE from 'three';
import {
  WORLD_SIZE,
  MOVE_TICK_MS,
  UNIT_DEFS,
  BUILDING_DEFS,
  PLAYER_COLORS,
  UnitKind,
  BuildingKind,
  isPassable,
  footprintTiles,
  footprintCenter,
  canPlace,
} from '../../shared/index.ts';
import { buildTerrain, terrainHeight } from './Terrain.ts';
import { buildSky, buildOcean, HORIZON } from './Environment.ts';
import {
  BAR_W,
  BAR_H,
  buildUnit,
  buildByKind,
  buildWallSlab,
  buildTree,
  buildSelRing,
  buildHpBar,
} from './meshes/index.ts';
import { placeCamera, applyProjection, panCamera } from './camera.ts';
import { drawMinimap, type Minimap } from './minimap.ts';
import { lineTiles, formation } from './input.ts';
import { useGameStore } from '../store/gameStore';

type Arche = 'unit' | 'building' | 'tree';

interface RObj {
  group: THREE.Group;
  arche: Arche;
  body: THREE.Object3D; // bobbed pivot for units; mesh otherwise
  kind: number;
  ownerHex?: string;
  hp: number;
  maxHp: number;
  tintMat?: THREE.MeshStandardMaterial;
  selRing?: THREE.Mesh;
  hpBar?: THREE.Group;
  fromX: number;
  fromZ: number;
  toX: number;
  toZ: number;
  facing: number;
  lerp: number;
  phase: number;
  rallyX?: number;
  rallyZ?: number;
}

interface PosRow {
  entityId: bigint;
  x: number;
  y: number;
  facing: number;
}

interface GameConn {
  db: any;
  reducers: {
    moveUnit: (a: { entityId: bigint; x: number; y: number }) => void;
    gatherResource: (a: { entityId: bigint; nodeId: bigint }) => void;
    attackUnit: (a: { entityId: bigint; targetId: bigint }) => void;
    placeBuilding: (a: { kind: number; x: number; y: number }) => void;
    placeWall: (a: { tiles: Array<{ x: number; y: number }> }) => void;
    demolishBuilding: (a: { entityId: bigint }) => void;
    setRally: (a: { entityId: bigint; x: number; y: number }) => void;
    setStance: (a: { entityIds: bigint[]; stance: number }) => void;
  };
}

const INTERP_S = MOVE_TICK_MS / 1000;
const GROUND = '#c2a06a';

export class SaladinGame {
  private readonly container: HTMLElement;
  private readonly scene = new THREE.Scene();
  private readonly camera: THREE.OrthographicCamera;
  private readonly renderer: THREE.WebGLRenderer;
  private readonly raycaster = new THREE.Raycaster();
  private readonly pointer = new THREE.Vector2();
  private terrain: THREE.Object3D; // pickable ground (fallback plane, then chunks)
  private readonly sky = buildSky();
  private readonly ocean = buildOcean();
  private seed = 0;
  private readonly selBox: HTMLDivElement;

  private readonly center = new THREE.Vector3(WORLD_SIZE / 2, 0, WORLD_SIZE / 2);
  private readonly offset = new THREE.Vector3(28, 38, 28);
  private viewSize = 17;

  private readonly objs = new Map<string, RObj>();
  private readonly pos = new Map<string, PosRow>();
  private readonly playerColors = new Map<string, number>();
  private readonly keys = new Set<string>();
  private readonly selected = new Set<string>();

  private myHex = '';
  private myKeepId: string | null = null;
  private framed = false;
  private selectedBuildingId: string | null = null;
  private buildingSelRing?: THREE.Mesh;
  private rallyFlag?: THREE.Object3D;
  private conn: GameConn | null = null;
  private unsub: Array<() => void> = [];

  private mini?: Minimap;
  private miniAccum = 0;

  private dragStart: { x: number; y: number } | null = null;
  private dragging = false;

  private buildMode: number | null = null;
  private demolishMode = false;
  private demolishing = false;
  private readonly demolishedThisDrag = new Set<string>();
  private ghost?: THREE.Group;
  private buildDragStart: { tx: number; ty: number } | null = null;
  private readonly occupied = new Set<number>();
  private readonly wallByTile = new Map<number, string>();
  private readonly pendingBuildings = new Set<string>();
  private readonly arrows: Array<{
    mesh: THREE.Mesh;
    fx: number;
    fz: number;
    tx: number;
    tz: number;
    t: number;
  }> = [];
  private storeUnsub?: () => void;

  private raf = 0;
  private last = 0;
  private disposed = false;

  constructor(container: HTMLElement) {
    this.container = container;
    const w = container.clientWidth || 800;
    const h = container.clientHeight || 600;

    this.renderer = new THREE.WebGLRenderer({ antialias: true });
    this.renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
    this.renderer.setSize(w, h);
    this.renderer.shadowMap.enabled = true;
    container.appendChild(this.renderer.domElement);

    this.scene.background = HORIZON.clone();
    this.scene.fog = new THREE.Fog(HORIZON.clone(), 260, 1100);
    this.scene.add(this.sky);
    this.scene.add(this.ocean);

    const aspect = w / h;
    this.camera = new THREE.OrthographicCamera(
      -this.viewSize * aspect,
      this.viewSize * aspect,
      this.viewSize,
      -this.viewSize,
      0.1,
      6000
    );
    this.placeCamera();

    this.scene.add(new THREE.HemisphereLight('#ffffff', '#6b5a3a', 0.9));
    const sun = new THREE.DirectionalLight('#fff3d6', 1.1);
    sun.position.set(40, 70, 20);
    sun.castShadow = true;
    sun.shadow.mapSize.set(2048, 2048);
    const sc = sun.shadow.camera as THREE.OrthographicCamera;
    sc.left = -WORLD_SIZE;
    sc.right = WORLD_SIZE;
    sc.top = WORLD_SIZE;
    sc.bottom = -WORLD_SIZE;
    sc.updateProjectionMatrix();
    this.scene.add(sun);

    // Fallback flat ground for picking until the terrain chunks stream in.
    const fallback = new THREE.Mesh(
      new THREE.PlaneGeometry(WORLD_SIZE, WORLD_SIZE),
      new THREE.MeshStandardMaterial({ color: GROUND })
    );
    fallback.name = 'ground';
    fallback.rotation.x = -Math.PI / 2;
    fallback.position.set(WORLD_SIZE / 2, 0, WORLD_SIZE / 2);
    fallback.receiveShadow = true;
    this.terrain = fallback;
    this.scene.add(fallback);

    this.selBox = document.createElement('div');
    this.selBox.style.cssText =
      'position:absolute;border:1px solid #ffec80;background:rgba(255,236,128,0.15);pointer-events:none;display:none;z-index:5;';
    container.appendChild(this.selBox);

    this.storeUnsub = useGameStore.subscribe((s) => {
      if (s.buildMode !== this.buildMode) this.setBuildMode(s.buildMode);
      if (s.demolishMode !== this.demolishMode)
        this.setDemolishMode(s.demolishMode);
    });

    this.bindEvents();
    this.last = performance.now();
    this.loop();

    if (import.meta.env?.DEV)
      (window as unknown as { __saladin: SaladinGame }).__saladin = this;
  }

  setIdentity(hex: string) {
    this.myHex = hex;
  }

  setMinimapCanvas(c: HTMLCanvasElement | null) {
    if (!c) {
      this.mini = undefined;
      return;
    }
    const ctx = c.getContext('2d');
    if (ctx) this.mini = { canvas: c, ctx };
  }

  focusWorld(x: number, y: number) {
    this.focusOn(x, y);
  }

  // ── connection wiring ───────────────────────────────────────────────────────

  attach(conn: GameConn) {
    this.detach();
    this.conn = conn;
    const db = conn.db;

    const on = (
      table: any,
      ins: (r: any) => void,
      del: (r: any) => void,
      upd?: (r: any) => void
    ) => {
      const fi = (_c: any, r: any) => ins(r);
      const fd = (_c: any, r: any) => del(r);
      const fu = (_c: any, _o: any, r: any) => upd?.(r);
      table.onInsert(fi);
      table.onDelete(fd);
      if (upd) table.onUpdate(fu);
      this.unsub.push(() => {
        table.removeOnInsert?.(fi);
        table.removeOnDelete?.(fd);
        if (upd) table.removeOnUpdate?.(fu);
      });
    };

    on(
      db.entity,
      (r) => this.onPos(r),
      (r) => this.onPosDelete(r),
      (r) => this.onPos(r)
    );
    on(
      db.unit,
      (r) =>
        this.spawnUnit(r.entityId, r.kind, r.hp, r.owner?.toHexString?.()),
      (r) => this.removeObj(r.entityId),
      (r) => this.onUnitUpdate(r)
    );
    on(
      db.building,
      (r) =>
        this.spawnBuilding(
          r.entityId,
          r.kind,
          r.hp,
          r.owner?.toHexString?.(),
          r.rallyX,
          r.rallyY
        ),
      (r) => this.removeObj(r.entityId),
      (r) => this.onBuildingUpdate(r)
    );
    on(
      db.resourceNode,
      (r) => this.spawnTree(r.entityId, r.remaining),
      (r) => this.removeObj(r.entityId),
      (r) => this.scaleTree(r.entityId, r.remaining)
    );
    on(
      db.player,
      (r) => this.onPlayer(r),
      () => {},
      (r) => this.onPlayer(r)
    );
    on(
      db.config,
      (r) => this.onConfig(r),
      () => {},
      (r) => this.onConfig(r)
    );

    const onShotInsert = (_c: any, r: any) => this.onShot(r);
    db.shot.onInsert(onShotInsert);
    this.unsub.push(() => db.shot.removeOnInsert?.(onShotInsert));
  }

  private onShot(r: any) {
    const mesh = new THREE.Mesh(
      new THREE.CylinderGeometry(0.03, 0.03, 0.55, 5),
      new THREE.MeshBasicMaterial({ color: 0x2e2114 })
    );
    this.scene.add(mesh);
    this.arrows.push({
      mesh,
      fx: r.fromX,
      fz: r.fromY,
      tx: r.toX,
      tz: r.toY,
      t: 0,
    });
  }

  private onConfig(r: any) {
    if (this.seed || !r?.seed) return;
    this.seed = r.seed;
    const t = buildTerrain(this.seed);
    this.scene.remove(this.terrain);
    this.terrain.traverse((c) => {
      const m = c as THREE.Mesh;
      m.geometry?.dispose?.();
      (m.material as THREE.Material | undefined)?.dispose?.();
    });
    this.terrain = t.group;
    this.scene.add(t.group);
  }

  private heightAt(x: number, z: number): number {
    return this.seed ? terrainHeight(this.seed, x, z) : 0;
  }

  detach() {
    this.unsub.forEach((u) => u());
    this.unsub = [];
    this.conn = null;
  }

  // ── table -> render ─────────────────────────────────────────────────────────

  private onPos(r: PosRow) {
    const id = r.entityId.toString();
    this.pos.set(id, r);
    this.tryFrameKeep(id);
    const o = this.objs.get(id);
    if (!o) return;
    if (o.arche === 'building') {
      // Buildings are static — snap, don't interpolate, so occupancy is exact.
      o.group.position.x = r.x;
      o.group.position.z = r.y;
      o.fromX = r.x;
      o.fromZ = r.y;
      o.toX = r.x;
      o.toZ = r.y;
      o.lerp = 1;
      if (this.pendingBuildings.delete(id)) this.finalizeBuildingPlacement(id);
      return;
    }
    o.fromX = o.group.position.x;
    o.fromZ = o.group.position.z;
    o.toX = r.x;
    o.toZ = r.y;
    o.facing = r.facing;
    o.lerp = 0;
  }

  private onPosDelete(r: PosRow) {
    this.removeObj(r.entityId);
  }

  private tryFrameKeep(id: string) {
    if (this.framed || id !== this.myKeepId) return;
    const p = this.pos.get(id);
    if (!p) return;
    const c = WORLD_SIZE / 2;
    this.focusOn(p.x + Math.sign(c - p.x) * 5, p.y + Math.sign(c - p.y) * 5);
    this.framed = true;
  }

  private onPlayer(r: any) {
    const hex = r.identity.toHexString();
    const color = PLAYER_COLORS[r.color % PLAYER_COLORS.length];
    this.playerColors.set(hex, color);
    for (const o of this.objs.values())
      if (o.ownerHex === hex && o.tintMat) o.tintMat.color.setHex(color);
  }

  private onUnitUpdate(r: any) {
    const o = this.objs.get(r.entityId.toString());
    if (!o) return;
    o.hp = r.hp;
    this.updateHpBar(o);
  }

  private onBuildingUpdate(r: any) {
    const id = r.entityId.toString();
    const o = this.objs.get(id);
    if (!o) return;
    o.hp = r.hp;
    o.rallyX = r.rallyX;
    o.rallyZ = r.rallyY;
    this.updateHpBar(o);
    if (this.selectedBuildingId === id) this.updateBuildingHighlight();
  }

  private newGroup(id: string, p: PosRow): THREE.Group {
    const group = new THREE.Group();
    group.userData.rid = id;
    group.position.set(p.x, 0, p.y);
    return group;
  }

  private spawnUnit(
    entityId: bigint,
    kind: number,
    hp: number,
    ownerHex?: string
  ) {
    const id = entityId.toString();
    if (this.objs.has(id)) return;
    const def = UNIT_DEFS[kind as UnitKind] ?? UNIT_DEFS[UnitKind.Peasant];
    const color = ownerHex ? this.playerColors.get(ownerHex) ?? 0xdddddd : 0xdddddd;
    const p =
      this.pos.get(id) ?? { entityId, x: WORLD_SIZE / 2, y: WORLD_SIZE / 2, facing: 0 };
    const group = this.newGroup(id, p);

    const body = buildUnit(kind, color);
    group.add(body);
    const ring = buildSelRing(def.radius);
    group.add(ring);
    const hpBar = buildHpBar();
    hpBar.position.y = def.height + def.radius * 2.4 + 0.35;
    group.add(hpBar);

    this.scene.add(group);
    const o: RObj = {
      group,
      arche: 'unit',
      body,
      kind,
      ownerHex,
      hp,
      maxHp: def.maxHp,
      tintMat: body.userData.tintMat as THREE.MeshStandardMaterial,
      selRing: ring,
      hpBar,
      fromX: p.x,
      fromZ: p.y,
      toX: p.x,
      toZ: p.y,
      facing: p.facing,
      lerp: 1,
      phase: (Number(entityId % 1000n) / 1000) * Math.PI * 2,
    };
    this.objs.set(id, o);
    this.updateHpBar(o);
  }

  private spawnBuilding(
    entityId: bigint,
    kind: number,
    hp: number,
    ownerHex?: string,
    rallyX?: number,
    rallyY?: number
  ) {
    const id = entityId.toString();
    if (this.objs.has(id)) return;
    const def = BUILDING_DEFS[kind as 0] ?? BUILDING_DEFS[BuildingKind.Keep];
    const color = ownerHex ? this.playerColors.get(ownerHex) ?? 0xdddddd : 0xdddddd;
    const known = this.pos.get(id);
    const p =
      known ?? { entityId, x: WORLD_SIZE / 2, y: WORLD_SIZE / 2, facing: 0 };
    const group = this.newGroup(id, p);
    const body =
      kind === BuildingKind.Wall
        ? buildWallSlab()
        : buildByKind(kind, color);
    group.add(body);
    const hpBar = buildHpBar();
    hpBar.position.y = def.height + 0.6;
    group.add(hpBar);
    this.scene.add(group);
    const o: RObj = {
      group,
      arche: 'building',
      body,
      kind,
      ownerHex,
      hp,
      maxHp: def.maxHp,
      tintMat: body.userData.tintMat as THREE.MeshStandardMaterial,
      hpBar,
      fromX: p.x,
      fromZ: p.y,
      toX: p.x,
      toZ: p.y,
      facing: 0,
      lerp: 1,
      phase: 0,
      rallyX,
      rallyZ: rallyY,
    };
    this.objs.set(id, o);
    this.updateHpBar(o);
    // Occupancy + wall orientation need the real position, which may arrive
    // (via the entity row) after this building row. Defer if it's not here yet.
    if (known) this.finalizeBuildingPlacement(id);
    else this.pendingBuildings.add(id);
  }

  private finalizeBuildingPlacement(id: string) {
    const o = this.objs.get(id);
    if (!o || o.arche !== 'building') return;
    const x = o.group.position.x;
    const z = o.group.position.z;
    this.stampOccupancy(o.kind, x, z, true);
    if (o.kind === BuildingKind.Wall) {
      this.wallByTile.set(Math.floor(z) * WORLD_SIZE + Math.floor(x), id);
      o.group.rotation.y = this.wallAngleAt(x, z);
    }
    this.refreshWallsAround(o.kind, x, z);
    if (o.kind === BuildingKind.Keep && o.ownerHex === this.myHex) {
      this.myKeepId = id;
      this.tryFrameKeep(id);
    }
  }

  private refreshWallsAround(kind: number, x: number, y: number) {
    const f = (BUILDING_DEFS[kind as 0] ?? BUILDING_DEFS[BuildingKind.Keep])
      .footprint;
    for (const { tx, ty } of footprintTiles(f, x, y))
      this.refreshWallNeighbors(tx + 0.5, ty + 0.5);
  }

  private refreshWallNeighbors(x: number, y: number) {
    const tx = Math.floor(x);
    const ty = Math.floor(y);
    for (const [dx, dy] of [
      [0, -1],
      [0, 1],
      [1, 0],
      [-1, 0],
      [1, 1],
      [1, -1],
      [-1, 1],
      [-1, -1],
    ]) {
      const id = this.wallByTile.get((ty + dy) * WORLD_SIZE + (tx + dx));
      const o = id ? this.objs.get(id) : undefined;
      if (o) this.rebuildWall(o);
    }
  }

  private rebuildWall(o: RObj) {
    // Slab template is the same; only the run orientation changes.
    o.group.rotation.y = this.wallAngleAt(
      o.group.position.x,
      o.group.position.z
    );
  }

  private stampOccupancy(kind: number, x: number, y: number, add: boolean) {
    const f = (BUILDING_DEFS[kind as 0] ?? BUILDING_DEFS[BuildingKind.Keep])
      .footprint;
    for (const { tx, ty } of footprintTiles(f, x, y)) {
      const i = ty * WORLD_SIZE + tx;
      if (add) this.occupied.add(i);
      else this.occupied.delete(i);
    }
  }

  private spawnTree(entityId: bigint, remaining: number) {
    const id = entityId.toString();
    if (this.objs.has(id)) return;
    const p =
      this.pos.get(id) ?? { entityId, x: WORLD_SIZE / 2, y: WORLD_SIZE / 2, facing: 0 };
    const group = this.newGroup(id, p);
    const body = buildTree();
    group.add(body);
    this.scaleTreeGroup(group, remaining);
    this.scene.add(group);
    this.objs.set(id, {
      group,
      arche: 'tree',
      body,
      kind: -1,
      hp: 0,
      maxHp: 0,
      fromX: p.x,
      fromZ: p.y,
      toX: p.x,
      toZ: p.y,
      facing: 0,
      lerp: 1,
      phase: 0,
    });
  }

  private scaleTree(entityId: bigint, remaining: number) {
    const o = this.objs.get(entityId.toString());
    if (o) this.scaleTreeGroup(o.group, remaining);
  }

  private scaleTreeGroup(group: THREE.Group, remaining: number) {
    group.scale.setScalar(0.5 + 0.5 * Math.min(1, remaining / 120));
  }

  private removeObj(entityId: bigint) {
    const id = entityId.toString();
    const o = this.objs.get(id);
    if (!o) return;
    const bx = o.group.position.x;
    const bz = o.group.position.z;
    const isWall = o.arche === 'building' && o.kind === BuildingKind.Wall;
    this.pendingBuildings.delete(id);
    if (o.arche === 'building') this.stampOccupancy(o.kind, bx, bz, false);
    if (isWall) this.wallByTile.delete(Math.floor(bz) * WORLD_SIZE + Math.floor(bx));
    this.scene.remove(o.group);
    o.group.traverse((c) => {
      const m = c as THREE.Mesh;
      m.geometry?.dispose?.();
      const mat = m.material as THREE.Material | THREE.Material[] | undefined;
      if (Array.isArray(mat)) mat.forEach((x) => x.dispose());
      else mat?.dispose?.();
    });
    this.objs.delete(id);
    if (o.arche === 'building') this.refreshWallsAround(o.kind, bx, bz);
    if (this.selectedBuildingId === id) this.clearBuildingSel();
    if (this.selected.delete(id)) this.emitSelection();
  }

  // ── wall orientation + overlays ───────────────────────────────────────────

  // Is the neighbour tile occupied by a building (wall/keep/tower) to connect to?
  // Orientation (Y rotation) of a wall segment: the average line through its
  // neighbours (8-way, double-angle averaging) so the wall follows the run —
  // horizontal, vertical, or diagonal.
  private wallAngleAt(x: number, y: number): number {
    const tx = Math.floor(x);
    const ty = Math.floor(y);
    const has = (dx: number, dy: number) =>
      this.occupied.has((ty + dy) * WORLD_SIZE + (tx + dx));
    let ax = 0;
    let ay = 0;
    let n = 0;
    for (const [dx, dy] of [
      [1, 0],
      [-1, 0],
      [0, 1],
      [0, -1],
      [1, 1],
      [1, -1],
      [-1, 1],
      [-1, -1],
    ]) {
      if (!has(dx, dy)) continue;
      const ang = Math.atan2(dy, dx);
      ax += Math.cos(2 * ang);
      ay += Math.sin(2 * ang);
      n++;
    }
    if (n === 0) return 0;
    return -Math.atan2(ay, ax) / 2;
  }

  private updateHpBar(o: RObj) {
    if (!o.hpBar || o.maxHp <= 0) return;
    const ratio = Math.max(0, Math.min(1, o.hp / o.maxHp));
    const fg = o.hpBar.userData.fg as THREE.Sprite;
    fg.scale.x = BAR_W * ratio;
    fg.position.x = -(BAR_W * (1 - ratio)) / 2;
    (fg.material as THREE.SpriteMaterial).color.setHex(
      ratio > 0.5 ? 0x33dd44 : ratio > 0.25 ? 0xddcc33 : 0xdd3333
    );
    o.hpBar.visible = ratio < 0.999;
  }

  // ── selection + commands ────────────────────────────────────────────────────

  private emitSelection() {
    let peasants = 0;
    let spearmen = 0;
    let archers = 0;
    let knights = 0;
    let hpSum = 0;
    let n = 0;
    for (const [id, o] of this.objs) {
      const sel = this.selected.has(id);
      if (o.selRing) o.selRing.visible = sel;
      if (!sel) continue;
      if (o.kind === UnitKind.Spearman) spearmen++;
      else if (o.kind === UnitKind.Archer) archers++;
      else if (o.kind === UnitKind.Knight) knights++;
      else peasants++;
      if (o.maxHp > 0) {
        hpSum += o.hp / o.maxHp;
        n++;
      }
    }
    useGameStore.getState().setSelection({
      total: this.selected.size,
      peasants,
      spearmen,
      archers,
      knights,
      avgHp: n > 0 ? hpSum / n : 1,
    });
  }

  private clearSelection() {
    this.selected.clear();
    this.emitSelection();
  }

  private setPointer(px: number, py: number) {
    const r = this.renderer.domElement.getBoundingClientRect();
    this.pointer.x = (px / r.width) * 2 - 1;
    this.pointer.y = -(py / r.height) * 2 + 1;
  }

  private pixel(e: PointerEvent): { x: number; y: number } {
    const r = this.renderer.domElement.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  }

  private pickList(): THREE.Object3D[] {
    const list: THREE.Object3D[] = [this.terrain];
    for (const o of this.objs.values()) list.push(o.group);
    return list;
  }

  private findRoot(obj: THREE.Object3D): THREE.Object3D | null {
    let cur: THREE.Object3D | null = obj;
    while (cur && !cur.userData.rid) cur = cur.parent;
    return cur;
  }

  private clickSelect(p: { x: number; y: number }, additive: boolean) {
    this.setPointer(p.x, p.y);
    this.raycaster.setFromCamera(this.pointer, this.camera);
    const hits = this.raycaster.intersectObjects(this.pickList(), true);
    for (const hit of hits) {
      const root = this.findRoot(hit.object);
      const rid = root?.userData.rid as string | undefined;
      if (!rid) continue;
      const o = this.objs.get(rid);
      if (!o) continue;
      if (o.arche === 'unit' && o.ownerHex === this.myHex) {
        this.clearBuildingSel();
        if (!additive) this.selected.clear();
        this.selected.add(rid);
        this.emitSelection();
        return;
      }
      if (o.arche === 'building' && o.ownerHex === this.myHex) {
        this.selected.clear();
        this.emitSelection();
        this.selectBuilding(rid, o.kind);
        return;
      }
      break;
    }
    if (!additive) {
      this.clearSelection();
      this.clearBuildingSel();
    }
  }

  private selectBuilding(id: string, kind: number) {
    this.selectedBuildingId = id;
    useGameStore.getState().setSelectedBuilding({ id, kind });
    this.updateBuildingHighlight();
  }

  private clearBuildingSel() {
    if (this.selectedBuildingId === null) return;
    this.selectedBuildingId = null;
    useGameStore.getState().setSelectedBuilding(null);
    this.updateBuildingHighlight();
  }

  private updateBuildingHighlight() {
    if (this.buildingSelRing) {
      this.scene.remove(this.buildingSelRing);
      this.buildingSelRing.geometry.dispose();
      (this.buildingSelRing.material as THREE.Material).dispose();
      this.buildingSelRing = undefined;
    }
    if (this.rallyFlag) {
      this.scene.remove(this.rallyFlag);
      this.rallyFlag = undefined;
    }
    const o = this.selectedBuildingId
      ? this.objs.get(this.selectedBuildingId)
      : undefined;
    if (!o) return;
    const def = BUILDING_DEFS[o.kind as 0];
    const r = def.footprint * 0.72;
    this.buildingSelRing = new THREE.Mesh(
      new THREE.RingGeometry(r, r + 0.2, 30),
      new THREE.MeshBasicMaterial({
        color: 0x9bf06b,
        side: THREE.DoubleSide,
        depthTest: false,
        transparent: true,
        opacity: 0.9,
      })
    );
    this.buildingSelRing.rotation.x = -Math.PI / 2;
    this.buildingSelRing.position.set(
      o.group.position.x,
      o.group.position.y + 0.06,
      o.group.position.z
    );
    this.buildingSelRing.renderOrder = 2;
    this.scene.add(this.buildingSelRing);

    if (
      o.rallyX !== undefined &&
      o.rallyZ !== undefined &&
      Math.hypot(o.rallyX - o.group.position.x, o.rallyZ - o.group.position.z) >
        1.0
    ) {
      const flag = new THREE.Group();
      const pole = new THREE.Mesh(
        new THREE.CylinderGeometry(0.04, 0.04, 1.0, 5),
        new THREE.MeshBasicMaterial({ color: 0x3a2a18 })
      );
      pole.position.y = 0.5;
      flag.add(pole);
      const cloth = new THREE.Mesh(
        new THREE.PlaneGeometry(0.5, 0.3),
        new THREE.MeshBasicMaterial({
          color: 0x9bf06b,
          side: THREE.DoubleSide,
        })
      );
      cloth.position.set(0.27, 0.85, 0);
      flag.add(cloth);
      flag.position.set(o.rallyX, this.heightAt(o.rallyX, o.rallyZ), o.rallyZ);
      this.rallyFlag = flag;
      this.scene.add(flag);
    }
  }

  private boxSelect(a: { x: number; y: number }, b: { x: number; y: number }) {
    const r = this.renderer.domElement.getBoundingClientRect();
    const minX = Math.min(a.x, b.x);
    const maxX = Math.max(a.x, b.x);
    const minY = Math.min(a.y, b.y);
    const maxY = Math.max(a.y, b.y);
    this.selected.clear();
    for (const [id, o] of this.objs) {
      if (o.arche !== 'unit' || o.ownerHex !== this.myHex) continue;
      const v = o.group.position.clone().project(this.camera);
      if (v.z < -1 || v.z > 1) continue;
      const sx = (v.x * 0.5 + 0.5) * r.width;
      const sy = (-v.y * 0.5 + 0.5) * r.height;
      if (sx >= minX && sx <= maxX && sy >= minY && sy <= maxY)
        this.selected.add(id);
    }
    this.emitSelection();
  }

  // Right-click: set rally if a building is selected, else command units.
  private command(p: { x: number; y: number }) {
    if (!this.conn) return;
    this.setPointer(p.x, p.y);
    this.raycaster.setFromCamera(this.pointer, this.camera);
    const hits = this.raycaster.intersectObjects(this.pickList(), true);

    if (this.selectedBuildingId) {
      const g = hits.find((h) => h.object.name === 'ground');
      if (g) {
        this.conn.reducers.setRally({
          entityId: BigInt(this.selectedBuildingId),
          x: g.point.x,
          y: g.point.z,
        });
      }
      return;
    }

    if (this.selected.size === 0) return;
    for (const hit of hits) {
      const root = this.findRoot(hit.object);
      const rid = root?.userData.rid as string | undefined;
      if (rid) {
        const o = this.objs.get(rid);
        if (!o) continue;
        if (o.arche === 'unit' && o.ownerHex !== this.myHex) {
          this.commandAttack(BigInt(rid));
          return;
        }
        if (o.arche === 'tree') {
          this.commandGather(BigInt(rid));
          return;
        }
        if (o.arche === 'building') {
          if (o.ownerHex !== this.myHex) this.commandAttack(BigInt(rid));
          else this.commandMove(o.group.position.x, o.group.position.z);
          return;
        }
        // own unit / other: ignore, let it fall through to nothing
        return;
      }
      if (hit.object.name === 'ground') {
        this.commandMove(hit.point.x, hit.point.z);
        return;
      }
    }
  }

  // Set combat posture on the currently selected units (called from the HUD).
  setSelectedStance(stance: number) {
    if (!this.conn || this.selected.size === 0) return;
    const entityIds = [...this.selected].map((id) => BigInt(id));
    this.conn.reducers.setStance({ entityIds, stance });
  }

  private commandAttack(targetId: bigint) {
    for (const id of this.selected) {
      const o = this.objs.get(id);
      const def = o ? UNIT_DEFS[o.kind as UnitKind] : undefined;
      if (def && def.attack > 0)
        this.conn!.reducers.attackUnit({ entityId: BigInt(id), targetId });
    }
  }

  private commandGather(nodeId: bigint) {
    for (const id of this.selected) {
      const o = this.objs.get(id);
      const def = o ? UNIT_DEFS[o.kind as UnitKind] : undefined;
      if (def && def.carry > 0)
        this.conn!.reducers.gatherResource({ entityId: BigInt(id), nodeId });
    }
  }

  private commandMove(gx: number, gz: number) {
    const ids = [...this.selected];
    const offs = formation(ids.length);
    ids.forEach((id, i) => {
      this.conn!.reducers.moveUnit({
        entityId: BigInt(id),
        x: gx + offs[i].x,
        y: gz + offs[i].y,
      });
    });
  }

  // ── build mode ──────────────────────────────────────────────────────────────

  private setBuildMode(mode: number | null) {
    this.buildMode = mode;
    this.buildDragStart = null;
    this.clearGhost();
    if (mode !== null) {
      this.ghost = new THREE.Group();
      this.scene.add(this.ghost);
    }
    this.selBox.style.display = 'none';
  }

  private clearGhost() {
    if (!this.ghost) return;
    this.scene.remove(this.ghost);
    this.ghost.traverse((c) => {
      const m = c as THREE.Mesh;
      m.geometry?.dispose?.();
      (m.material as THREE.Material | undefined)?.dispose?.();
    });
    this.ghost = undefined;
  }

  private placeValid(cx: number, cy: number): boolean {
    if (this.buildMode === null) return false;
    return canPlace(
      this.buildMode as 0,
      cx,
      cy,
      (tx, ty) => isPassable(this.seed, tx, ty),
      (tx, ty) => this.occupied.has(ty * WORLD_SIZE + tx)
    );
  }

  // Placement cells under the cursor: a single footprint, or a dragged wall line.
  private buildCells(hx: number, hz: number): Array<{ cx: number; cy: number; f: number }> {
    if (this.buildMode === null) return [];
    const def = BUILDING_DEFS[this.buildMode as 0];
    if (this.buildMode === BuildingKind.Wall) {
      const hov = { tx: Math.floor(hx), ty: Math.floor(hz) };
      const tiles = this.buildDragStart ? lineTiles(this.buildDragStart, hov) : [hov];
      return tiles.map((t) => ({ cx: t.tx + 0.5, cy: t.ty + 0.5, f: 1 }));
    }
    const c = footprintCenter(def.footprint, hx, hz);
    return [{ cx: c.x, cy: c.y, f: def.footprint }];
  }

  // Wall orientation under the cursor: along the drag axis (so the preview
  // matches how it will render), or its connection angle for a single tile.
  private ghostWallAngle(hx: number, hz: number): number {
    if (this.buildDragStart) {
      const dx = Math.floor(hx) - this.buildDragStart.tx;
      const dy = Math.floor(hz) - this.buildDragStart.ty;
      if (dx === 0 && dy === 0) return this.wallAngleAt(hx, hz);
      return Math.abs(dx) >= Math.abs(dy) ? 0 : -Math.PI / 2;
    }
    return this.wallAngleAt(hx, hz);
  }

  private applyGhostMaterial(obj: THREE.Object3D, valid: boolean) {
    const mat = new THREE.MeshBasicMaterial({
      color: valid ? 0x44ee55 : 0xee4433,
      transparent: true,
      opacity: 0.5,
      depthWrite: false,
    });
    obj.traverse((c) => {
      const m = c as THREE.Mesh;
      if (m.isMesh) m.material = mat;
    });
  }

  // Ghost shows the ACTUAL model (oriented) so it's WYSIWYG.
  private updateGhost(hx: number, hz: number) {
    if (this.buildMode === null) return;
    this.clearGhost();
    this.ghost = new THREE.Group();
    const isWall = this.buildMode === BuildingKind.Wall;
    const angle = isWall ? this.ghostWallAngle(hx, hz) : 0;
    for (const { cx, cy } of this.buildCells(hx, hz)) {
      const model = isWall
        ? buildWallSlab()
        : buildByKind(this.buildMode, 0xdddddd);
      this.applyGhostMaterial(model, this.placeValid(cx, cy));
      model.position.set(cx, this.heightAt(cx, cy), cy);
      if (isWall) model.rotation.y = angle;
      this.ghost.add(model);
    }
    this.scene.add(this.ghost);
  }

  private commitBuild(hx: number, hz: number) {
    if (this.buildMode === null || !this.conn) return;
    const valid = this.buildCells(hx, hz).filter((c) =>
      this.placeValid(c.cx, c.cy)
    );
    if (this.buildMode === BuildingKind.Wall) {
      // One batched call for the whole dragged line — no per-tile reducer flood.
      if (valid.length > 0)
        this.conn.reducers.placeWall({
          tiles: valid.map((c) => ({ x: c.cx, y: c.cy })),
        });
      return;
    }
    for (const { cx, cy } of valid)
      this.conn.reducers.placeBuilding({ kind: this.buildMode, x: cx, y: cy });
  }

  private groundTile(e: PointerEvent): { hx: number; hz: number } | null {
    const p = this.pixel(e);
    this.setPointer(p.x, p.y);
    this.raycaster.setFromCamera(this.pointer, this.camera);
    const hits = this.raycaster.intersectObject(this.terrain, true);
    return hits[0] ? { hx: hits[0].point.x, hz: hits[0].point.z } : null;
  }

  // ── demolish mode ───────────────────────────────────────────────────────────

  private setDemolishMode(on: boolean) {
    this.demolishMode = on;
    this.buildDragStart = null;
    this.clearGhost();
    if (on) {
      this.ghost = new THREE.Group();
      this.scene.add(this.ghost);
    }
    this.selBox.style.display = 'none';
  }

  private ownBuildingUnder(e: PointerEvent): { id: string; o: RObj } | null {
    const p = this.pixel(e);
    this.setPointer(p.x, p.y);
    this.raycaster.setFromCamera(this.pointer, this.camera);
    const groups = [...this.objs.values()].map((o) => o.group);
    for (const hit of this.raycaster.intersectObjects(groups, true)) {
      const root = this.findRoot(hit.object);
      const rid = root?.userData.rid as string | undefined;
      if (!rid) continue;
      const o = this.objs.get(rid);
      if (o && o.arche === 'building' && o.ownerHex === this.myHex)
        return { id: rid, o };
      return null; // topmost object isn't a demolishable building
    }
    return null;
  }

  private updateDemolishGhost(e: PointerEvent) {
    if (!this.demolishMode) return;
    this.clearGhost();
    this.ghost = new THREE.Group();
    const tgt = this.ownBuildingUnder(e);
    if (tgt) {
      const def = BUILDING_DEFS[tgt.o.kind as 0];
      const h = def.height + 0.4;
      const m = new THREE.Mesh(
        new THREE.BoxGeometry(def.footprint * 1.05, h, def.footprint * 1.05),
        new THREE.MeshBasicMaterial({
          color: 0xff4030,
          transparent: true,
          opacity: 0.4,
          depthWrite: false,
        })
      );
      const px = tgt.o.group.position.x;
      const pz = tgt.o.group.position.z;
      m.position.set(px, this.heightAt(px, pz) + h / 2, pz);
      this.ghost.add(m);
    }
    this.scene.add(this.ghost);
  }

  // Demolish the building directly under the cursor (works regardless of the
  // wall's elevation — uses the picked building, not a terrain tile). Drag to
  // paint-demolish a row of walls.
  private demolishUnder(e: PointerEvent) {
    if (!this.conn) return;
    const tgt = this.ownBuildingUnder(e);
    if (tgt && !this.demolishedThisDrag.has(tgt.id)) {
      this.demolishedThisDrag.add(tgt.id);
      this.conn.reducers.demolishBuilding({ entityId: BigInt(tgt.id) });
    }
  }

  // ── input ───────────────────────────────────────────────────────────────────

  private bindEvents() {
    const el = this.renderer.domElement;
    el.addEventListener('pointerdown', this.onPointerDown);
    el.addEventListener('contextmenu', (e) => e.preventDefault());
    el.addEventListener('wheel', this.onWheel, { passive: false });
    window.addEventListener('pointermove', this.onPointerMove);
    window.addEventListener('pointerup', this.onPointerUp);
    window.addEventListener('resize', this.onResize);
    window.addEventListener('keydown', this.onKey);
    window.addEventListener('keyup', this.onKey);
  }

  private onPointerDown = (e: PointerEvent) => {
    if (this.demolishMode) {
      if (e.button === 2) {
        useGameStore.getState().setDemolishMode(false);
        return;
      }
      if (e.button !== 0) return;
      this.demolishedThisDrag.clear();
      this.demolishing = true;
      this.demolishUnder(e);
      return;
    }
    if (this.buildMode !== null) {
      if (e.button === 2) {
        useGameStore.getState().setBuildMode(null);
        return;
      }
      if (e.button !== 0) return;
      const t = this.groundTile(e);
      if (!t) return;
      if (this.buildMode === BuildingKind.Wall) {
        this.buildDragStart = { tx: Math.floor(t.hx), ty: Math.floor(t.hz) };
        this.updateGhost(t.hx, t.hz);
      } else {
        this.commitBuild(t.hx, t.hz);
      }
      return;
    }
    if (e.button === 2) {
      this.command(this.pixel(e));
      return;
    }
    if (e.button !== 0) return;
    this.dragStart = this.pixel(e);
    this.dragging = false;
  };

  private onPointerMove = (e: PointerEvent) => {
    if (this.demolishMode) {
      this.updateDemolishGhost(e);
      if (this.demolishing) this.demolishUnder(e);
      return;
    }
    if (this.buildMode !== null) {
      const t = this.groundTile(e);
      if (t) this.updateGhost(t.hx, t.hz);
      return;
    }
    if (!this.dragStart) return;
    const p = this.pixel(e);
    if (!this.dragging && Math.hypot(p.x - this.dragStart.x, p.y - this.dragStart.y) > 4)
      this.dragging = true;
    if (this.dragging) {
      const a = this.dragStart;
      this.selBox.style.display = 'block';
      this.selBox.style.left = `${Math.min(a.x, p.x)}px`;
      this.selBox.style.top = `${Math.min(a.y, p.y)}px`;
      this.selBox.style.width = `${Math.abs(p.x - a.x)}px`;
      this.selBox.style.height = `${Math.abs(p.y - a.y)}px`;
    }
  };

  private onPointerUp = (e: PointerEvent) => {
    if (this.demolishMode) {
      this.demolishing = false;
      return;
    }
    if (this.buildMode !== null) {
      if (e.button === 0 && this.buildDragStart) {
        const t = this.groundTile(e);
        if (t) this.commitBuild(t.hx, t.hz);
        this.buildDragStart = null;
        if (t) this.updateGhost(t.hx, t.hz);
      }
      return;
    }
    if (e.button !== 0 || !this.dragStart) return;
    const p = this.pixel(e);
    if (this.dragging) this.boxSelect(this.dragStart, p);
    else this.clickSelect(p, e.shiftKey);
    this.dragStart = null;
    this.dragging = false;
    this.selBox.style.display = 'none';
  };

  private onWheel = (e: WheelEvent) => {
    e.preventDefault();
    this.viewSize = Math.max(
      9,
      Math.min(55, this.viewSize + Math.sign(e.deltaY) * 2)
    );
    this.updateProjection();
  };

  private onKey = (e: KeyboardEvent) => {
    if (e.type === 'keydown') this.keys.add(e.key.toLowerCase());
    else this.keys.delete(e.key.toLowerCase());
  };

  private onResize = () => {
    this.renderer.setSize(this.container.clientWidth, this.container.clientHeight);
    this.updateProjection();
  };

  private updateProjection() {
    applyProjection(
      this.camera,
      this.viewSize,
      this.container.clientWidth,
      this.container.clientHeight
    );
  }

  private placeCamera() {
    placeCamera(this.camera, this.center, this.offset);
  }

  private focusOn(x: number, y: number) {
    this.center.x = x;
    this.center.z = y;
    this.center.y = this.heightAt(x, y);
    this.placeCamera();
  }

  private *minimapBlips() {
    for (const o of this.objs.values())
      yield {
        x: o.group.position.x,
        z: o.group.position.z,
        arche: o.arche,
        color: o.ownerHex
          ? this.playerColors.get(o.ownerHex) ?? 0xffffff
          : 0xffffff,
      };
  }

  private drawMinimap() {
    if (!this.mini) return;
    drawMinimap(
      this.mini,
      this.seed,
      this.minimapBlips(),
      this.center.x,
      this.center.z,
      this.viewSize
    );
  }

  // ── loop ────────────────────────────────────────────────────────────────────

  private loop = () => {
    if (this.disposed) return;
    this.raf = requestAnimationFrame(this.loop);
    const now = performance.now();
    const dt = Math.min(0.1, (now - this.last) / 1000);
    this.last = now;

    this.panCamera(dt);

    // Sky + ocean ride with the camera so their edges are never reachable.
    this.sky.position.set(this.center.x, 0, this.center.z);
    this.ocean.position.set(this.center.x, -0.05, this.center.z);

    const bob = now * 0.005;
    for (const o of this.objs.values()) {
      if (o.lerp < 1) {
        o.lerp = Math.min(1, o.lerp + dt / INTERP_S);
        o.group.position.x = o.fromX + (o.toX - o.fromX) * o.lerp;
        o.group.position.z = o.fromZ + (o.toZ - o.fromZ) * o.lerp;
      }
      o.group.position.y = this.heightAt(o.group.position.x, o.group.position.z);
      if (o.arche === 'unit') {
        o.group.rotation.y = -o.facing;
        o.body.position.y = Math.abs(Math.sin(bob + o.phase)) * 0.07;
      }
    }

    for (let i = this.arrows.length - 1; i >= 0; i--) {
      const a = this.arrows[i];
      a.t += dt / 0.22;
      if (a.t >= 1) {
        this.scene.remove(a.mesh);
        a.mesh.geometry.dispose();
        (a.mesh.material as THREE.Material).dispose();
        this.arrows.splice(i, 1);
        continue;
      }
      const x = a.fx + (a.tx - a.fx) * a.t;
      const z = a.fz + (a.tz - a.fz) * a.t;
      const arc = Math.sin(a.t * Math.PI) * 1.1;
      a.mesh.position.set(x, this.heightAt(x, z) + 0.6 + arc, z);
      a.mesh.lookAt(a.tx, this.heightAt(a.tx, a.tz) + 0.6, a.tz);
      a.mesh.rotateX(Math.PI / 2);
    }

    this.miniAccum += dt;
    if (this.miniAccum > 0.12) {
      this.miniAccum = 0;
      this.drawMinimap();
    }

    this.renderer.render(this.scene, this.camera);
  };

  private panCamera(dt: number) {
    panCamera(
      this.camera,
      this.center,
      this.offset,
      this.viewSize,
      this.keys,
      dt
    );
  }

  dispose() {
    this.disposed = true;
    cancelAnimationFrame(this.raf);
    this.storeUnsub?.();
    this.detach();
    window.removeEventListener('pointermove', this.onPointerMove);
    window.removeEventListener('pointerup', this.onPointerUp);
    window.removeEventListener('resize', this.onResize);
    window.removeEventListener('keydown', this.onKey);
    window.removeEventListener('keyup', this.onKey);
    this.renderer.domElement.removeEventListener('pointerdown', this.onPointerDown);
    this.renderer.domElement.removeEventListener('wheel', this.onWheel);
    this.selBox.remove();
    this.renderer.dispose();
    if (this.renderer.domElement.parentElement === this.container)
      this.container.removeChild(this.renderer.domElement);
  }

  // exposed for HUD / debug
  debugInfo() {
    const byArche: Record<string, number> = {};
    for (const o of this.objs.values())
      byArche[o.arche] = (byArche[o.arche] ?? 0) + 1;
    return {
      myHex: this.myHex,
      objs: this.objs.size,
      byArche,
      selected: this.selected.size,
      framed: this.framed,
      attached: !!this.conn,
    };
  }
}
