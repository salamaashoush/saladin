// Pure deterministic PRNG + spatial hash. No imports, no global state, no
// Math.random. The module (authority: where land/resources land) and the client
// (render) both recompute worldgen from these — same seed in, same bytes out, on
// every platform. Determinism is the contract: do not introduce floats that vary
// by engine here.

// Classic mulberry32: a 32-bit counter PRNG. Returns a stateful generator that
// yields successive floats in [0, 1). Seed is coerced to u32.
export function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// Stateless spatial hash: a stable float in [0, 1) for an (x, y) integer lattice
// point under `seed`. Used as white noise for value-noise interpolation and as a
// per-tile accept roll in node placement. Coordinates are floored to ints so the
// same tile always hashes identically.
export function hash2(x: number, y: number, seed: number): number {
  const ix = Math.floor(x) | 0;
  const iy = Math.floor(y) | 0;
  let h = (ix * 374761393 + iy * 668265263 + (seed | 0) * 2246822519) | 0;
  h = (h ^ (h >>> 13)) | 0;
  h = Math.imul(h, 1274126177) | 0;
  return ((h ^ (h >>> 16)) >>> 0) / 4294967296;
}

// Fold an arbitrary integer into a fresh seed. Used to derive an independent
// stream (e.g. per resource kind) from one world seed without correlation.
export function mixSeed(seed: number, salt: number): number {
  let h = ((seed | 0) ^ Math.imul(salt | 0, 0x9e3779b1)) | 0;
  h = Math.imul(h ^ (h >>> 16), 0x85ebca6b) | 0;
  h = Math.imul(h ^ (h >>> 13), 0xc2b2ae35) | 0;
  return (h ^ (h >>> 16)) >>> 0;
}
