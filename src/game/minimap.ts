import {
  WORLD_SIZE,
  BIOME_COLOR,
  BIOME_SHADE,
  sampleTerrain,
} from '../../shared/index.ts';

// Blend a biome's base color toward its shade by elevation so hills/mountains
// read darker on the minimap, matching the 3D terrain's facet shading.
function shadedBiomeColor(biome: number, height: number): number {
  const base = BIOME_COLOR[biome as 0];
  const shade = BIOME_SHADE[biome as 0];
  const t = Math.max(0, Math.min(1, (height - 0.45) * 1.6));
  const lerp = (a: number, b: number) => Math.round(a + (b - a) * t);
  const r = lerp((base >> 16) & 255, (shade >> 16) & 255);
  const g = lerp((base >> 8) & 255, (shade >> 8) & 255);
  const b = lerp(base & 255, shade & 255);
  return (r << 16) | (g << 8) | b;
}

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
        const s = sampleTerrain(
          seed,
          (px / S) * WORLD_SIZE,
          (py / S) * WORLD_SIZE
        );
        const col = shadedBiomeColor(s.biome, s.height);
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
    const hex = '#' + b.color.toString(16).padStart(6, '0');
    if (b.arche === 'tree') {
      // Resource nodes: small dot tinted by resource type (color from the blip).
      ctx.fillStyle = hex;
      ctx.fillRect(px - 0.7, py - 0.7, 1.6, 1.6);
      continue;
    }
    ctx.fillStyle = hex;
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
