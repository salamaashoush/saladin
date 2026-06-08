# Terrain Architecture — scaling to support every system

Goal: terrain that scales from the current 96² island to large multiplayer maps,
and that **feeds every gameplay system** (pathfinding, building placement, fog of
war, resource placement, biome effects) — not just a pretty mesh.

## Principle: seed + sparse overrides, never per-tile rows

The authoritative world is a **single seed** in `config` plus a small **override
layer** for player changes (chopped trees, building footprints, scorched land).
Both the module (authority) and client recompute terrain from the seed in
`shared/terrain.ts`. This keeps replication tiny — we never stream a 96×96 (or
512×512) tile table; clients derive identical terrain locally.

```
config.seed ──┬─► module: where is land / where do resources go / passability
              └─► client: render mesh, minimap, build previews
override tables (sparse): resource_node (trees), building (footprints), ...
```

## Three layers

| Layer | Owner | Now | Scales via |
| --- | --- | --- | --- |
| **Data** (biome/height/passable/buildable/occupied) | module + shared | `sampleTerrain`, `isLand`, `findLandNear` | a chunked `TileGrid` cache; sparse override map for occupancy |
| **Sim** (pathfinding, placement, fog) | module | straight-line move, land-aware spawn | flow-field + A*, footprint occupancy, per-player visibility grid |
| **Render** | client | chunked vertex-colored heightmap, flat-shaded; camera-follow ocean + sky dome | quadtree LOD, instanced vegetation, splat textures |

## Rendering (done + next)

Done this pass:
- **Chunked terrain** (`Terrain.ts`): grid of `CHUNK`-sized meshes, not one giant
  mesh → the renderer frustum-culls off-screen chunks; per-chunk LOD swaps later.
- **Flat shading** for the stylized low-poly facet look.
- **Endless sea**: opaque ocean + gradient **sky dome** both **ride the camera**,
  so no finite-plane edge is ever reachable; fog blends the far ocean to horizon.
  (A transparent ocean exposed the bright sky past the finite terrain — read as a
  hard square; opaque sea hides that seam.)

Next, in priority order:
1. **Quadtree LOD** per chunk — select detail by screen-space error (distance ×
   geometric error). Far chunks drop to coarse geometry. ([three.js #507])
2. **Instanced vegetation** — trees/rocks as `InstancedMesh` per chunk, distance-
   culled. Thousands of props at a handful of draw calls.
3. **Texture splatting + normal maps** — blend biome textures by slope/height
   instead of flat vertex colors, for higher fidelity when zoomed in. ([interverse LOD])
4. **Geometry-clipmap ocean** with scrolling normals for real wave shading.

## Pathfinding (the big one for "support all systems")

Units currently walk straight lines (can cross water/trees). Target, research-
backed ([jdxdev flowfields], [moonjump]):

- **Tile passability grid** derived from terrain + occupancy (water, mountain,
  buildings = blocked). Lives in the module; rebuilt incrementally when buildings
  change.
- **Flow fields** for group/shared-destination movement — one Dijkstra flood per
  destination, then every unit reads its direction in O(1). Ideal for RTS crowds.
- **A\*/JPS** for individual unique-destination moves.
- **Hierarchical** (chunk-level portals) once maps are large, so flood costs stay
  bounded.
- Combine: A* for singletons, flow fields for armies toward a rally point.

## Building placement

Footprint check against the **buildable** tile layer (`isLand` + not occupied +
flat enough). On place, stamp the footprint into the occupancy override → instantly
feeds passability + future placement. Client shows a green/red ghost via the same
shared predicate.

## Fog of war

Per-player **visibility grid** (unit/building sight radius stamped each tick).
SpacetimeDB **row-level subscription filters** replicate only visible entities to
each client — fog becomes a data-access feature, not a render hack.

## Authority

The module owns the data + sim layers (seed, occupancy, passability, visibility);
the client only renders and previews. Same `shared/` code on both sides keeps them
in lockstep deterministically.

## Sources
- [Chunked LOD terrain — three.js #507](https://github.com/mrdoob/three.js/issues/507)
- [@interverse/three-terrain-lod](https://www.npmjs.com/package/@interverse/three-terrain-lod)
- [RTS Pathfinding — Flowfields (jdxdev)](https://www.jdxdev.com/blog/2020/05/03/flowfields/)
- [Flow Fields — How It Works (moonjump)](https://moonjump.com/game-dev-mechanics-flow-fields-how-it-works/)
- [Fluffy grass / instanced vegetation (Codrops)](https://tympanus.net/codrops/2025/02/04/how-to-make-the-fluffiest-grass-with-three-js/)
