// node startup.mjs <margin-bin> <dist-dir> <bridge-port> <ui-port> [runs]
// HOME/MARGIN_HOME from env. Measures: process start -> bridge ready; page navigation -> first `boot`
// invoke (as logged by the backend: "window page loaded (boot)"); navigation -> library rows rendered.
import { webkit } from "playwright";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";
import fs from "node:fs";
const [bin, dist, bport, uport, runsArg] = process.argv.slice(2);
const runs = Number(runsArg || 7);
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const vite = spawn("bunx", ["vite", "preview", "--outDir", dist, "--port", uport, "--strictPort"], { cwd: root, stdio: "ignore" });
await new Promise((r) => setTimeout(r, 2000));
const browser = await webkit.launch();
const toFont = [], ready = [], toBoot = [], toRows = [], bootLog = [];
const logFile = `${process.env.MARGIN_HOME}/state/logs/margin.log`;
for (let i = 0; i < runs; i++) {
  const t0 = Date.now();
  const p = spawn(bin, ["--dev", "serve", bport], { stdio: "ignore" });
  while (true) { try { await fetch(`http://127.0.0.1:${bport}/latest`); break; } catch { await new Promise((r) => setTimeout(r, 5)); } }
  ready.push(Date.now() - t0);
  const ctx = await browser.newContext({ viewport: { width: 1180, height: 760 } });
  const page = await ctx.newPage();
  await page.addInitScript(`window.__E2E_BRIDGE__ = "http://127.0.0.1:${bport}";`);
  await page.addInitScript({ path: `${root}/scripts/e2e/bridge-shim.js` });
  await page.addInitScript(() => {
    const orig = Object.getOwnPropertyDescriptor(window, "__TAURI_INTERNALS__");
    window.__T_BOOT__ = null;
    const wrap = () => { const inv = window.__TAURI_INTERNALS__.invoke; window.__TAURI_INTERNALS__.invoke = (c, a) => { if (c === "boot" && window.__T_BOOT__ === null) window.__T_BOOT__ = performance.now(); return inv(c, a); }; };
    queueMicrotask(wrap);
    // When the notepad font has loaded, timed from page start.
    window.__T_FONT__ = null;
    const poll = setInterval(() => {
      if ([...document.fonts].some((f) => f.family.includes("Recursive") && f.status === "loaded")) { window.__T_FONT__ = performance.now(); clearInterval(poll); }
    }, 1);
  });
  const before = fs.existsSync(logFile) ? fs.statSync(logFile).size : 0;
  const n0 = Date.now();
  await page.goto(`http://localhost:${uport}`);
  await page.locator(".row, .ob-title, .ob-step").first().waitFor({ timeout: 20000 });
  const nRows = Date.now() - n0;
  const tb = await page.evaluate(() => window.__T_BOOT__);
  const tf = await page.evaluate(async () => { while (window.__T_FONT__ === null) await new Promise((r) => setTimeout(r, 5)); return window.__T_FONT__; });
  toFont.push(tf);
  toBoot.push(tb); toRows.push(nRows);
  const tail = fs.readFileSync(logFile).subarray(before).toString("utf8");
  const m = tail.match(/^(\S+)\s+INFO.*window page loaded \(boot\)/m);
  if (m) bootLog.push(new Date(m[1]).getTime() - n0);
  await ctx.close(); p.kill(); await new Promise((r) => setTimeout(r, 300));
}
const med = (xs) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];
console.log(JSON.stringify({ runs, bridgeReadyMs: med(ready), navToBootInvokeMs: +med(toBoot).toFixed(1), navToBootLoggedMs: bootLog.length ? med(bootLog) : null, navToRowsMs: med(toRows), navToFontMs: +med(toFont).toFixed(1), raw: { ready, toBoot: toBoot.map((x) => +x.toFixed(1)), bootLog, toRows } }, null, 1));
await browser.close(); vite.kill();
