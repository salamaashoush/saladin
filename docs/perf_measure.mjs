// Browser-driven client FPS / draw-call sweep for Saladin.
//
// Usage (via ferridriver):
//   ferridriver run docs/perf_measure.mjs -- <url> <label> <viewSize> <settleMs> <sampleMs> <out.png>
//
// Drives a headless Chromium at the saladinci client, parks the iso camera over
// the stress armies at the map centre via window.__saladin.perfFocus, lets frame
// timing settle, then averages window.__perf over a sampling window. Prints a JSON
// line (PERF_RESULT ...) with fps/frameMs/draws/tris/onScreenUnits and optionally
// writes a PNG.
//
// The stress units must already be spawned in the DB (spacetime call debug_stress)
// before this runs; this script only measures what the client renders.

const url = args[0] ?? "http://127.0.0.1:5180/";
const label = args[1] ?? "run";
const viewSize = Number(args[2] ?? 26);
const settleMs = Number(args[3] ?? 4000);
const sampleMs = Number(args[4] ?? 4000);
const outPng = args[5] ?? "";

const WORLD_CENTER = 72; // WORLD_SIZE/2 (144/2)

const browser = await chromium().launch({ headless: true });
const ctx = await browser.newContext({
  viewport: { width: 1280, height: 800 },
});
const page = await ctx.newPage();

await page.goto(url, { waitUntil: "load" });

// Wait until the game object exists and the connection has streamed units in.
await page.waitForFunction(
  () => !!(window.__saladin && window.__perf && window.__perf.units > 0),
  { timeout: 30000 },
);

// Park the camera over the contested centre at the requested zoom.
await page.evaluate(
  ([cx, vs]) => window.__saladin.perfFocus(cx, cx, vs),
  [WORLD_CENTER, viewSize],
);

// Let frame timing + interpolation settle, then sample window.__perf and average.
const samples = await page.evaluate(
  ([settle, sample]) =>
    new Promise((resolve) => {
      setTimeout(() => {
        const acc = [];
        const t0 = performance.now();
        const id = setInterval(() => {
          if (window.__perf) acc.push({ ...window.__perf });
          if (performance.now() - t0 >= sample) {
            clearInterval(id);
            resolve(acc);
          }
        }, 100);
      }, settle);
    }),
  [settleMs, sampleMs],
);

if (outPng) await page.screenshot({ path: outPng });
await browser.close();

// Reduce: drop the first couple warm samples, then average / median.
const drop = samples.length > 6 ? 2 : 0;
const warm = samples.slice(drop);
const avg = (k) => warm.reduce((s, x) => s + (x[k] || 0), 0) / warm.length;
const med = (k) => {
  const v = warm.map((x) => x[k] || 0).sort((a, b) => a - b);
  return v[Math.floor(v.length / 2)];
};

const result = {
  label,
  viewSize,
  fps: Math.round(med("fps")),
  fpsAvg: +avg("fps").toFixed(1),
  frameMs: +avg("frameMs").toFixed(2),
  drawCalls: Math.round(med("drawCalls")),
  triangles: Math.round(med("triangles")),
  units: Math.round(med("units")),
  onScreenUnits: Math.round(med("onScreenUnits")),
  buildings: Math.round(med("buildings")),
  trees: Math.round(med("trees")),
  programs: Math.round(med("programs")),
  samples: warm.length,
};

console.log("PERF_RESULT " + JSON.stringify(result));
export default result;
