import { describe, it, expect } from 'vitest';
import { allocSlot, spawnCorner, MAX_PLAYERS } from '../shared/index.ts';

describe('allocSlot', () => {
  it('hands out the lowest free slot', () => {
    expect(allocSlot([], MAX_PLAYERS)).toBe(0);
    expect(allocSlot([0], MAX_PLAYERS)).toBe(1);
    expect(allocSlot([0, 1, 2], MAX_PLAYERS)).toBe(3);
  });

  it('returns -1 when every slot is taken', () => {
    const all = Array.from({ length: MAX_PLAYERS }, (_, i) => i);
    expect(allocSlot(all, MAX_PLAYERS)).toBe(-1);
  });

  it('reuses a freed middle slot (leaver does not push others up)', () => {
    // player in slot 1 left; the next joiner must take 1, not MAX_PLAYERS
    expect(allocSlot([0, 2, 3], MAX_PLAYERS)).toBe(1);
  });
});

describe('spawn corners are stable per slot', () => {
  const key = (c: { x: number; y: number }) => `${c.x},${c.y}`;

  it(`assigns ${MAX_PLAYERS} distinct corners for ${MAX_PLAYERS} sequential joins`, () => {
    const used: number[] = [];
    const corners = [];
    for (let i = 0; i < MAX_PLAYERS; i++) {
      const slot = allocSlot(used, MAX_PLAYERS);
      expect(slot).toBeGreaterThanOrEqual(0);
      used.push(slot);
      corners.push(spawnCorner(slot));
    }
    const unique = new Set(corners.map(key));
    expect(unique.size).toBe(MAX_PLAYERS);
  });

  it(`spawnCorner(0..${MAX_PLAYERS - 1}) are all distinct`, () => {
    const all = Array.from({ length: MAX_PLAYERS }, (_, i) => spawnCorner(i));
    const unique = new Set(all.map(key));
    expect(unique.size).toBe(MAX_PLAYERS);
  });

  it('every anchor sits inside the spawn margin (buildable band)', () => {
    const WORLD = 144;
    const MARGIN = 24;
    for (let i = 0; i < MAX_PLAYERS; i++) {
      const c = spawnCorner(i);
      expect(c.x).toBeGreaterThanOrEqual(MARGIN);
      expect(c.x).toBeLessThanOrEqual(WORLD - MARGIN);
      expect(c.y).toBeGreaterThanOrEqual(MARGIN);
      expect(c.y).toBeLessThanOrEqual(WORLD - MARGIN);
    }
  });

  it('a leave frees a slot reused by the next joiner without collision', () => {
    // Fill every slot, then a mid player leaves; the next joiner must take the
    // freed slot and land on a corner no surviving player occupies.
    const used = Array.from({ length: MAX_PLAYERS }, (_, i) => i);
    const leaver = 3;
    const survivors = used.filter((s) => s !== leaver);

    const rejoinSlot = allocSlot(survivors, MAX_PLAYERS);
    expect(rejoinSlot).toBe(leaver);

    const rejoin = spawnCorner(rejoinSlot);
    for (const s of survivors) {
      expect(spawnCorner(s)).not.toEqual(rejoin);
    }
  });

  it('a rejoin after a leave does not collide with a survivor corner', () => {
    // slots 0 and 2 occupied, 1 free; rejoin takes 1
    const slot = allocSlot([0, 2], MAX_PLAYERS);
    expect(slot).toBe(1);
    const survivor0 = spawnCorner(0);
    const survivor2 = spawnCorner(2);
    const rejoin = spawnCorner(slot);
    expect(rejoin).not.toEqual(survivor0);
    expect(rejoin).not.toEqual(survivor2);
  });
});
