// README screenshots: the built UI against the real backend in a sandbox (as run.mjs),
// with a small demo library of believable meetings. Writes, at the app's default window
// size (1180×760 CSS px, 2×), in light and dark:
//   docs/images/screenshot-{light,dark}.png   a meeting: notes left, named transcript right
//   docs/images/settings-{light,dark}.png     Settings → Recording
//
//   bun run build && (cd src-tauri && cargo build)
//   scripts/e2e/make-fixtures.sh /tmp/margin-fixtures
//   node scripts/e2e/screenshots.mjs --seed-models /tmp/margin-fixtures/models
//
// --seed-models is needed: setup waits for the speech models. E2E_DIR (default
// /tmp/margin-shots), E2E_BRIDGE_PORT, E2E_UI_PORT and CARGO_TARGET_DIR work as in run.mjs.
import { webkit } from "playwright";
import { spawn, execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { MEETINGS, demoFile } from "./demo-meetings.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const argv = process.argv.slice(2);
const seed = argv.includes("--seed-models") ? argv[argv.indexOf("--seed-models") + 1] : null;
if (!seed) throw new Error("--seed-models <dir> is required (see scripts/e2e/make-fixtures.sh)");
const images = path.join(root, "docs/images");

const base = process.env.E2E_DIR || "/tmp/margin-shots";
const home = path.join(base, "home");
const marginHome = path.join(base, "margin");
fs.rmSync(base, { recursive: true, force: true });
fs.mkdirSync(home, { recursive: true });
fs.cpSync(seed, path.join(marginHome, "state/models"), { recursive: true });
const env = { ...process.env, HOME: home, MARGIN_HOME: marginHome };
const bin = path.join(process.env.CARGO_TARGET_DIR || path.join(root, "src-tauri/target"), "debug/margin");
const port = { bridge: process.env.E2E_BRIDGE_PORT || "4555", ui: process.env.E2E_UI_PORT || "4184" };

const procs = [spawn(bin, ["--dev", "serve", port.bridge], { env, stdio: "ignore" }), spawn("bunx", ["vite", "preview", "--port", port.ui, "--strictPort"], { cwd: root, stdio: "ignore" })];
const browser = await webkit.launch();
try {
  await new Promise((r) => setTimeout(r, 2500));
  const light = await openPage("light");
  await onboard(light, "Maya Chen");
  seedLibrary(path.join(home, "Documents/Meetings"));
  await shoot(light, "light");
  await shoot(await openPage("dark"), "dark");
} finally {
  await browser.close();
  for (const p of procs) p.kill();
}
for (const f of ["screenshot-light", "screenshot-dark", "settings-light", "settings-dark"]) {
  console.log(`${f}.png ${Math.round(fs.statSync(path.join(images, `${f}.png`)).size / 1024)} KB`);
}

async function openPage(scheme) {
  const ctx = await browser.newContext({ viewport: { width: 1180, height: 760 }, deviceScaleFactor: 2, colorScheme: scheme });
  const page = await ctx.newPage();
  await page.addInitScript(`window.__E2E_BRIDGE__ = "http://127.0.0.1:${port.bridge}";`);
  await page.addInitScript({ path: path.join(root, "scripts/e2e/bridge-shim.js") });
  await page.goto(`http://localhost:${port.ui}`);
  return page;
}

/** Setup with its defaults, as a new user would take it: a name, then Return on each step. */
async function onboard(page, name) {
  await page.getByLabel("Your name, so transcripts can label you").fill(name);
  for (const heading of ["Where should meetings go?", "Let Margin hear the meeting.", "Want summaries too?", "You're ready."]) {
    await page.keyboard.press("Enter");
    await page.getByText(heading).waitFor({ timeout: 8000 });
  }
  await page.getByText("Speech models ready").waitFor({ timeout: 15000 });
  await page.keyboard.press("Enter");
  await page.getByText(/Your first meeting starts here|Ready when you are/).waitFor({ timeout: 8000 });
}

function seedLibrary(dir) {
  fs.mkdirSync(dir, { recursive: true });
  for (const m of MEETINGS) fs.writeFileSync(path.join(dir, m.file), demoFile(m), { mode: 0o600 });
}

async function shoot(page, scheme) {
  await page.reload();
  const hero = page.locator(".row").filter({ hasText: MEETINGS[0].title }).first();
  await hero.waitFor({ timeout: 8000 });
  await hero.click();
  await page.locator(".turn").first().waitFor();
  await calm(page);
  await page.screenshot({ path: path.join(images, `screenshot-${scheme}.png`), animations: "disabled" });
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("navigation", { name: "Settings sections" }).getByRole("button", { name: "Recording", exact: true }).click();
  await page.getByRole("heading", { level: 1, name: "Recording" }).waitFor();
  await calm(page);
  await page.screenshot({ path: path.join(images, `settings-${scheme}.png`), animations: "disabled" });
  compress(path.join(images, `screenshot-${scheme}.png`));
  compress(path.join(images, `settings-${scheme}.png`));
}

/** No toasts, banners (a call app open on this Mac), focus rings or hover states. */
async function calm(page) {
  await page.locator(".toast").waitFor({ state: "detached", timeout: 10000 }).catch(() => {});
  for (const b of await page.locator(".call-banner").getByRole("button", { name: /Dismiss|Later/ }).all()) await b.click();
  await page.mouse.move(1179, 759);
  await page.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());
  await page.waitForTimeout(400);
}

/** Smaller PNGs with pngquant when it is installed; the 2× originals are otherwise kept. */
function compress(file) {
  try {
    execFileSync("pngquant", ["--force", "--skip-if-larger", "--quality=80-95", "--output", file, file], { stdio: "ignore" });
  } catch {
    // pngquant is missing or the result would be larger.
  }
}
