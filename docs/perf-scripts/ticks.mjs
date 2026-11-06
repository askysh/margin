// node ticks.mjs <margin-bin> <dist> <bport> <uport>: UI main-thread cost of one live tick, with the
// 600-meeting library in the sidebar and a 700-line live transcript. HOME/MARGIN_HOME from env.
import { webkit } from "playwright";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";
const [bin, dist, bport, uport] = process.argv.slice(2);
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const vite = spawn("bunx", ["vite", "preview", "--outDir", dist, "--port", uport, "--strictPort"], { cwd: root, stdio: "ignore" });
const bridge = spawn(bin, ["--dev", "serve", bport], { stdio: "ignore" });
await new Promise((r) => setTimeout(r, 2500));
const browser = await webkit.launch();
const page = await (await browser.newContext({ viewport: { width: 1180, height: 760 } })).newPage();
await page.addInitScript(`window.__E2E_BRIDGE__ = "http://127.0.0.1:${bport}";`);
await page.addInitScript({ path: `${root}/scripts/e2e/bridge-shim.js` });
await page.goto(`http://localhost:${uport}`);
await page.locator(".row").nth(500).waitFor({ timeout: 20000 });
const result = await page.evaluate(async () => {
  const e = window.__E2E__;
  const now = new Date().toISOString();
  const lines = Array.from({ length: 700 }, (_, i) => ({ line: i + 1, offset_ms: i * 5000, text: `Line ${i} about the launch plan and pricing review for next quarter.`, speaker: i % 3 ? "them" : "you" }));
  e.emit("recorder", { recording: true, stopping: false, kind: "call", title: "Perf", started_at: now, elapsed_secs: 0, level: 0, lines: [], notes: "" });
  await new Promise((r) => setTimeout(r, 300));
  e.emit("live-lines", lines);
  await new Promise((r) => setTimeout(r, 500));
  const ch = new MessageChannel(); let wake = null; ch.port1.onmessage = () => wake(); const hop = () => new Promise((r) => { wake = r; ch.port2.postMessage(0); }); const flush = async () => { await hop(); await hop(); };
  const run = async (event, n, payload) => { const t = performance.now(); for (let i = 0; i < n; i++) { e.emit(event, payload(i)); await flush(); } return (performance.now() - t) / n; };
  const base = await run("no-such-event", 200, () => null);
  const tick = await run("recorder-tick", 200, (i) => ({ elapsed_secs: i, level: (i * 7) % 60 }));
  const line = await run("live-lines", 50, (i) => [{ line: 1000 + i, offset_ms: 3_600_000 + i, text: "A new line.", speaker: "you" }]);
  return { liveLines: document.querySelectorAll(".live-line").length, sidebarRows: document.querySelectorAll(".row").length, perTickMs: +(tick - base).toFixed(2), perNewLineMs: +(line - base).toFixed(2), baseMs: +base.toFixed(2) };
});
console.log(JSON.stringify(result));
await browser.close(); bridge.kill(); vite.kill();
