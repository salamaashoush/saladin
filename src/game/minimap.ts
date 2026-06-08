import { WORLD_SIZE, BIOME_COLOR, sampleTerrain } from '../../shared/index.ts';

export interface Minimap {
  canvas: HTMLCanvasElement;
  ctx: CanvasRenderingContext2D;
  bg?: ImageData;
}

export interface MinimapBlip {
  x: number;
  z: number;
  arche: 'unit' | 'building' | 'tree';
  color: number;
}

// Render the terrain backdrop (cached after first paint), object blips, and the
// camera viewport rectangle onto the minimap canvas.
export function drawMinimap(
  mini: Minimap,
  seed: number,
  blips: Iterable<MinimapBlip>,
  centerX: number,
  centerZ: number,
  viewSize: number
) {
  if (!seed) return;
  const { canvas, ctx } = mini;
  const S = canvas.width;
  if (!mini.bg) {
    const img = ctx.createImageData(S, S);
    for (let py = 0; py < S; py++) {
      for (let px = 0; px < S; px++) {
        const col =
          BIOME_COLOR[
            sampleTerrain(seed, (px / S) * WORLD_SIZE, (py / S) * WORLD_SIZE)
              .biome
          ];
        const i = (py * S + px) * 4;
        img.data[i] = (col >> 16) & 255;
        img.data[i + 1] = (col >> 8) & 255;
        img.data[i + 2] = col & 255;
        img.data[i + 3] = 255;
      }
    }
    mini.bg = img;
  }
  ctx.putImageData(mini.bg, 0, 0);

  for (const b of blips) {
    const px = (b.x / WORLD_SIZE) * S;
    const py = (b.z / WORLD_SIZE) * S;
    if (b.arche === 'tree') {
      ctx.fillStyle = '#2f5a25';
      ctx.fillRect(px, py, 1.4, 1.4);
      continue;
    }
    ctx.fillStyle = '#' + b.color.toString(16).padStart(6, '0');
    const sz = b.arche === 'building' ? 5 : 3;
    ctx.fillRect(px - sz / 2, py - sz / 2, sz, sz);
  }

  const half = viewSize;
  ctx.strokeStyle = 'rgba(255,255,255,0.85)';
  ctx.lineWidth = 1;
  ctx.strokeRect(
    ((centerX - half) / WORLD_SIZE) * S,
    ((centerZ - half) / WORLD_SIZE) * S,
    ((half * 2) / WORLD_SIZE) * S,
    ((half * 2) / WORLD_SIZE) * S
  );
}
