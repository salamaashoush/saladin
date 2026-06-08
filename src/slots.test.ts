import { describe, it, expect } from 'vitest';
import { allocSlot, spawnCorner, MAX_PLAYERS } from '../shared/index.ts';

describe('allocSlot', () => {
  it('hands out the lowest free slot', () => {
    expect(allocSlot([], 4)).toBe(0);
    expect(allocSlot([0], 4)).toBe(1);
    expect(allocSlot([0, 1, 2], 4)).toBe(3);
  });

  it('returns -1 when every slot is taken', () => {
    expect(allocSlot([0, 1, 2, 3], 4)).toBe(-1);
  });

  it('reuses a freed middle slot (leaver does not push others up)', () => {
    // player in slot 1 left; the next joiner must take 1, not 4
    expect(allocSlot([0, 2, 3], 4)).toBe(1);
  });
});

describe('spawn corners are stable per slot', () => {
  it('assigns 4 distinct corners for 4 sequential joins', () => {
    const used: number[] = [];
    const corners = [];
    for (let i = 0; i < MAX_PLAYERS; i++) {
      const slot = allocSlot(used, MAX_PLAYERS);
      used.push(slot);
      corners.push(spawnCorner(slot));
    }
    const unique = new Set(corners.map((c) => `${c.x},${c.y}`));
    expect(unique.size).toBe(MAX_PLAYERS);
  });

  it('a rejoin after a leave does not collide with a survivor corner', () => {
    // slots 0 and 2 occupied, 1 and 3 free; rejoin takes 1
    const slot = allocSlot([0, 2], MAX_PLAYERS);
    expect(slot).toBe(1);
    const survivor0 = spawnCorner(0);
    const survivor2 = spawnCorner(2);
    const rejoin = spawnCorner(slot);
    expect(rejoin).not.toEqual(survivor0);
    expect(rejoin).not.toEqual(survivor2);
  });
});
