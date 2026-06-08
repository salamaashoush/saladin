// Visual check: park over the stress army, hide the React menu overlay so the
// instanced units are visible, and screenshot. Also returns __perf for the HUD.
//   ferridriver run docs/perf_shot.mjs -- <url> <viewSize> <out.png>
const url = args[0] ?? "http://127.0.0.1:5180/";
const viewSize = Number(args[1] ?? 22);
const outPng =
  args[2] ??
  "/home/sashoush/Workspace/saladin/.ferridriver/artifacts/fps-after.png";
const WORLD_CENTER = 72;

const browser = await chromium().launch({ headless: true });
const ctx = await browser.newContext({
  viewport: { width: 1280, height: 800 },
});
const page = await ctx.newPage();
await page.goto(url, { waitUntil: "load" });
await page.waitForFunction(
  () => !!(window.__saladin && window.__perf && window.__perf.units > 0),
  { timeout: 30000 },
);
await page.evaluate(
  ([cx, vs]) => window.__saladin.perfFocus(cx, cx, vs),
  [WORLD_CENTER, viewSize],
);
// Hide every React overlay (menu, status) so only the WebGL canvas + perf HUD show.
await page.evaluate(() => {
  const root = document.querySelector(".game-root");
  if (root)
    root.querySelectorAll(":scope > *:not(.viewport)").forEach((el) => {
      el.style.display = "none";
    });
});
await page.evaluate(() => new Promise((r) => setTimeout(r, 2500)));
await page.screenshot({ path: outPng });
const perf = await page.evaluate(() => window.__perf);
await browser.close();
console.log("SHOT_PERF " + JSON.stringify(perf));
export default perf;
