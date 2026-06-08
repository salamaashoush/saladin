import { Identity } from 'spacetimedb';
import { t } from 'spacetimedb/server';
import { WORLD_SIZE, MAX_PLAYERS } from '../../../shared/constants.ts';
import { enemyFaction, aiName, spawnCorner, AI_PROFILES } from '../../../shared/defs.ts';
import { Faction, UnitKind } from '../../../shared/enums.ts';
import { AiPhase } from '../../../shared/ai.ts';
import { spacetimedb } from '../schema/db.ts';
import { foundPlayer, spawnUnitEntity } from '../world/spawn.ts';
import { createMatch, scatterMatchNodes } from '../world/match.ts';
import { clearMatchRows } from '../world/scope.ts';
import { clampWorld } from '../world/util.ts';

// DEV-ONLY stress harness. Founds `matches` independent matches, each with two
// opposing synthetic players whose armies are spawned overlapping near the map
// centre so combat actually engages. Total live fighting units ≈ matches ×
// unitsPerMatch. Deterministic: positions come from index arithmetic, identities
// from fixed bit patterns (disjoint from real players and from spawnAi's bots),
// so a given (matches, unitsPerMatch) always produces the same world.
//
// Unit mix per side: ~1/4 Peasant gatherers (drive movement + economy) and the
// rest Spearmen (attack 12, aggroRange 6) given an enemy of the opposing owner in
// the same match, so the O(N^2) combat acquisition path runs at scale.

// Deterministic, collision-free identity for stress player `n`. spawnAi uses the
// top bit (1n<<255n); we use the next bit down so the two namespaces never collide.
function stressIdentity(n: bigint): any {
  return new Identity((1n << 254n) | n);
}

// Lay `count` units of `kind` in a square block around (cx,cy), one tile apart, so
// an army occupies a compact region near the contested centre.
function spawnArmy(
  ctx: any,
  owner: any,
  kind: number,
  count: number,
  cx: number,
  cy: number,
  matchId: bigint
): bigint[] {
  const ids: bigint[] = [];
  const side = Math.max(1, Math.ceil(Math.sqrt(count)));
  for (let i = 0; i < count; i++) {
    const row = Math.floor(i / side);
    const col = i % side;
    const x = clampWorld(cx + (col - side / 2));
    const y = clampWorld(cy + (row - side / 2));
    ids.push(spawnUnitEntity(ctx, owner, kind, x, y, matchId));
  }
  return ids;
}

export const debugStress = spacetimedb.reducer(
  { matches: t.u32(), unitsPerMatch: t.u32() },
  (ctx, { matches, unitsPerMatch }) => {
    const perSide = Math.max(1, Math.floor(unitsPerMatch / 2));
    const gatherersPerSide = Math.floor(perSide / 4);
    const soldiersPerSide = Math.max(0, perSide - gatherersPerSide);

    let idCounter = 1n;
    for (let m = 0; m < matches; m++) {
      // Two opposing owners. foundPlayer drops each a keep at its own corner (gives
      // gatherers a dropoff + a defeat keep); armies are then placed at the centre.
      const idA = stressIdentity(idCounter++);
      const idB = stressIdentity(idCounter++);
      const matchId = createMatch(ctx, idA, 'stress', 1, 'continental');
      scatterMatchNodes(ctx, matchId);

      foundPlayer(ctx, idA, 'StressA', Faction.Ayyubid, matchId);
      foundPlayer(ctx, idB, 'StressB', enemyFaction(Faction.Ayyubid), matchId);

      const cx = WORLD_SIZE / 2;
      const cy = WORLD_SIZE / 2;
      // Two armies a few tiles apart so they are inside Spearman aggroRange (6).
      spawnArmy(ctx, idA, UnitKind.Spearman, soldiersPerSide, cx - 4, cy, matchId);
      spawnArmy(ctx, idB, UnitKind.Spearman, soldiersPerSide, cx + 4, cy, matchId);
      // Gatherers idle near each keep corner; they exercise the movement loop once
      // sent to work but here simply add to the live unit population.
      spawnArmy(ctx, idA, UnitKind.Peasant, gatherersPerSide, cx - 12, cy - 12, matchId);
      spawnArmy(ctx, idB, UnitKind.Peasant, gatherersPerSide, cx + 12, cy + 12, matchId);
    }
  }
);

// DEV-ONLY: one big match with `bots` REAL ai-controlled players (each gets an ai
// row, so aiBrain plans + commands them) and `unitsPerBot` pre-placed units each,
// spread to their own corners (not blobbed — realistic spatial spread). Used to
// FEEL the full runtime at scale, including AI brain cost. Bots are mutual enemies
// (alternating factions), so combat engages across the map.
export const debugBigMatch = spacetimedb.reducer(
  { bots: t.u32(), unitsPerBot: t.u32() },
  (ctx, { bots, unitsPerBot }) => {
    const n = Math.max(2, Math.min(bots, MAX_PLAYERS));
    const ids: any[] = [];
    for (let i = 0; i < n; i++) ids.push(new Identity((1n << 253n) | BigInt(i + 1)));

    const matchId = createMatch(ctx, ids[0], 'bigmatch', 1, 'continental');
    scatterMatchNodes(ctx, matchId);

    const soldiers = Math.max(0, Math.floor(unitsPerBot * 0.8));
    const gatherers = Math.max(0, unitsPerBot - soldiers);

    for (let i = 0; i < n; i++) {
      const faction = i % 2 === 0 ? Faction.Ayyubid : Faction.Crusader;
      foundPlayer(ctx, ids[i], aiName(faction, i), faction, matchId);
      // Real AI bot: aiBrain iterates the ai table and drives this identity.
      ctx.db.ai.insert({
        matchId,
        identity: ids[i],
        host: ids[0],
        difficulty: 1,
        decisionCd: (i % 5) * 0.2,
        waveTimer: AI_PROFILES[1].firstWaveDelay,
        phase: AiPhase.Boot,
        scoutId: 0n,
        threatTimer: 0,
      });
      // Army massed near this bot's own corner so the eight forces start spread
      // across the map and converge — not all stacked on one tile.
      const c = spawnCorner(i);
      spawnArmy(ctx, ids[i], UnitKind.Spearman, soldiers, c.x, c.y, matchId);
      spawnArmy(ctx, ids[i], UnitKind.Peasant, gatherers, c.x + 8, c.y + 8, matchId);
    }
  }
);

// Tear down every stress-spawned match (and its rows) so a measurement run leaves a
// clean DB. Identifies stress matches by name; harmless if none exist.
export const debugStressClear = spacetimedb.reducer((ctx) => {
  const stressMatchIds = [...ctx.db.match.iter()]
    .filter((m: any) => m.name === 'stress' || m.name === 'bigmatch')
    .map((m: any) => m.matchId);
  for (const matchId of stressMatchIds) clearMatchRows(ctx, matchId);
});
