// End-to-end QA: the real UI (built dist/) in headless WebKit, driven like a user, against
// the real backend (`margin --dev serve`) in a throwaway sandbox. Nothing touches your
// real settings, meetings or voices, and no macOS permission prompt can appear.
//
//   bun run build && (cd src-tauri && cargo build)
//   node scripts/e2e/run.mjs --audio meeting.wav [--seed-models DIR] [--summaries claude|ollama|none] [--download]
//
// --seed-models copies model files into the sandbox (skips a ~520 MB download).
// --summaries fake-ollama runs a stand-in Ollama (scripts/e2e/fake-ollama.py) so the whole
// summary flow is tested without a real model. "claude" needs your real HOME, so it is not
// offered here; use `margin --dev summarize` for that.
// --download also tests a real model switch + download (Fast model, ~148 MB).
// Screenshots and report.json go to $E2E_OUT (default /tmp/margin-e2e/out).
import { webkit } from "playwright";
import { spawn, execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const argv = process.argv.slice(2);
const opt = (k, d) => (argv.includes(k) ? argv[argv.indexOf(k) + 1] : d);
const flag = (k) => argv.includes(k);
const audio = opt("--audio");
const seed = opt("--seed-models");
const summaries = opt("--summaries", "none");
if (!audio) throw new Error("--audio <wav> is required");

const base = process.env.E2E_DIR || "/tmp/margin-e2e";
const out = process.env.E2E_OUT || path.join(base, "out");
const home = path.join(base, "home");
const marginHome = path.join(base, "margin");
fs.rmSync(home, { recursive: true, force: true });
fs.rmSync(marginHome, { recursive: true, force: true });
fs.mkdirSync(out, { recursive: true });
fs.mkdirSync(home, { recursive: true });
if (seed) {
  const dest = path.join(marginHome, "state/models");
  fs.cpSync(seed, dest, { recursive: true });
}
const meetingsDir = path.join(home, "Documents/Meetings");
const env = { ...process.env, HOME: home, MARGIN_HOME: marginHome };
const bin = path.join(root, "src-tauri/target/debug/margin");

const procs = [];
const start = (cmd, args, extra = {}) => {
  const p = spawn(cmd, args, { env, cwd: root, stdio: ["ignore", "pipe", "pipe"], ...extra });
  const log = fs.createWriteStream(path.join(out, `${path.basename(cmd)}.log`));
  p.stdout.pipe(log);
  p.stderr.pipe(log);
  procs.push(p);
  return p;
};
start(bin, ["--dev", "serve", "4545"]);
if (summaries === "fake-ollama") start("/usr/bin/python3", ["-I", path.join(root, "scripts/e2e/fake-ollama.py"), "11435", path.join(out, "ollama-requests.log")]);
start("bunx", ["vite", "preview", "--port", "4174", "--strictPort"]);
await new Promise((r) => setTimeout(r, 2500));

const results = [];
let step = 0;
async function check(name, fn) {
  step++;
  const id = String(step).padStart(2, "0");
  try {
    await fn();
    results.push({ id, name, ok: true });
    console.log(`ok   ${id} ${name}`);
  } catch (e) {
    results.push({ id, name, ok: false, error: String(e?.message ?? e).split("\n")[0] });
    console.log(`FAIL ${id} ${name}: ${String(e?.message ?? e).split("\n")[0]}`);
  }
  await page.screenshot({ path: path.join(out, `${id}-${name.replace(/[^a-z0-9]+/gi, "-").toLowerCase()}.png`) }).catch(() => {});
}
const assert = (cond, msg) => {
  if (!cond) throw new Error(msg);
};
const dev = (...a) => execFileSync(bin, ["--dev", ...a], { env, encoding: "utf8" });
const meetingFiles = () => (fs.existsSync(meetingsDir) ? fs.readdirSync(meetingsDir).filter((f) => f.endsWith(".md")) : []);

const browser = await webkit.launch();
const ctx = await browser.newContext({ viewport: { width: 1180, height: 760 }, deviceScaleFactor: 2 });
const page = await ctx.newPage();
const consoleErrors = [];
page.on("pageerror", (e) => consoleErrors.push(e.message));
page.on("console", (m) => m.type() === "error" && consoleErrors.push(m.text()));
await page.addInitScript({ path: path.join(root, "scripts/e2e/bridge-shim.js") });
await page.goto("http://localhost:4174");
const ui = (sel) => page.locator(sel);
const button = (name) => page.getByRole("button", { name, exact: false });

// ---------------------------------------------------------------- onboarding
await check("onboarding shows on first run", async () => {
  await page.getByText("Notes in your words.").waitFor({ timeout: 8000 });
});
await check("name step saves your name", async () => {
  await page.getByLabel("Your name, so transcripts can label you").fill("Akash QA");
  await button("Continue").click();
  await page.getByText("Where should meetings go?").waitFor();
  const cfg = fs.readFileSync(path.join(marginHome, "config.toml"), "utf8");
  assert(cfg.includes('name = "Akash QA"'), "identity.name not written");
  assert(/\[summarization\][^[]*engine = "none"/s.test(cfg), "summarization.engine not explicit none");
  assert(/\[calendar\][^[]*enabled = false/s.test(cfg), "calendar not disabled");
});
await check("folder step defaults to Documents/Meetings", async () => {
  await page.getByRole("radio", { name: /Documents/ }).click();
  await button("Use this folder").click();
  await page.getByText("How accurate should transcripts be?").waitFor();
  assert(fs.existsSync(meetingsDir), "meetings folder not created");
});
await check("model step: Balanced, continue", async () => {
  await page.getByRole("radio", { name: /Balanced/ }).click();
  await button("Download and continue").click();
  await page.getByText("Two permissions, both explained.").waitFor();
});
await check("permissions step explains and never prompts", async () => {
  await page.getByText("System audio", { exact: true }).waitFor();
  await page.getByText(/Margin never records your screen/).waitFor();
  await page.getByText(/Speech models ready|Downloading speech models/).waitFor({ timeout: 10000 });
  await button("Continue").click();
  await page.getByText("Want summaries too?").waitFor();
});
await check("summaries step: finish setup", async () => {
  const done = button("Start using Margin");
  await done.waitFor({ timeout: seed ? 15000 : 600000 });
  await done.click();
  await page.getByText(/Your first meeting starts here/).waitFor({ timeout: 8000 });
  assert(JSON.parse(fs.readFileSync(path.join(marginHome, "prefs.json"), "utf8")).onboarding_complete, "prefs not saved");
});

// ---------------------------------------------------------------- a meeting arrives
const notesFile = path.join(base, "notes.md");
fs.writeFileSync(notesFile, "- download size is the #1 risk\n- offer Fast model, upgrade later\n## Follow-ups\n- [ ] ask legal about pricing\n");
await check("stopped recording is processed while the app is open", async () => {
  dev("queue", audio, "QA launch sync", notesFile);
  await page.locator(".row-job").first().waitFor({ timeout: 10000 });
  await page.locator(".row").filter({ hasText: "QA launch sync" }).first().waitFor({ timeout: 120000 });
  await page.locator(".row-job").waitFor({ state: "detached", timeout: 120000 });
});
let mdPath = null;
await check("meeting file has the exact notes and a transcript", async () => {
  const f = meetingFiles().find((x) => x.includes("qa-launch-sync"));
  assert(f, `meeting not found in ${meetingFiles().join(", ")}`);
  mdPath = path.join(meetingsDir, f);
  const md = fs.readFileSync(mdPath, "utf8");
  assert(md.includes("## Notes\n\n- download size is the #1 risk\n- offer Fast model, upgrade later\n### Follow-ups\n- [ ] ask legal about pricing\n"), "notes not spliced verbatim");
  assert(md.includes("## Transcript"), "no transcript");
  assert(/\[SPEAKER_\d+ \d+:\d\d\]/.test(md), "no speaker labels");
});
await check("open the meeting from the sidebar", async () => {
  await page.locator(".row").filter({ hasText: "QA launch sync" }).first().click();
  await page.locator("h1.title", { hasText: "QA launch sync" }).waitFor();
  await page.locator(".turn").first().waitFor();
  assert((await page.locator(".chip").count()) >= 2, "expected at least 2 speaker chips");
});

// ---------------------------------------------------------------- notes editing
await check("editing notes saves into the file and keeps the frontmatter", async () => {
  const before = fs.readFileSync(mdPath, "utf8");
  const fmBefore = before.slice(0, before.indexOf("\n---", 3) + 4);
  const editor = page.getByLabel("Your notes");
  await editor.click();
  await page.keyboard.press("Meta+ArrowDown");
  // The last line is a task item: Enter continues it, a second Enter ends the list.
  await page.keyboard.press("Enter");
  await page.keyboard.press("Enter");
  await page.keyboard.type("- decided: ship Fast by default");
  await page.getByText("Saved", { exact: true }).waitFor({ timeout: 5000 });
  const after = fs.readFileSync(mdPath, "utf8");
  assert(after.startsWith(fmBefore), "frontmatter changed");
  assert(after.includes("- decided: ship Fast by default"), "typed note not in file");
  assert(after.indexOf("## Notes") < after.indexOf("## Transcript"), "section order broken");
});
await check("list continuation: Enter after a bullet starts a new bullet", async () => {
  await page.keyboard.press("Enter");
  await page.keyboard.type("auto bullet");
  await page.getByText("Saved", { exact: true }).waitFor({ timeout: 5000 });
  await page.waitForTimeout(800);
  assert(fs.readFileSync(mdPath, "utf8").includes("- auto bullet"), "Enter did not continue the list");
});

// ---------------------------------------------------------------- rename
await check("rename a speaker everywhere and remember the voice", async () => {
  await page.locator(".chip.unnamed").first().click();
  await page.locator(".popover input:not([type])").fill("Priya");
  await button("Rename everywhere").click();
  await page.locator(".toast").filter({ hasText: /Renamed to Priya/ }).waitFor({ timeout: 8000 });
  await page.locator(".chip", { hasText: "Priya" }).waitFor();
  const md = fs.readFileSync(mdPath, "utf8");
  assert(/\[Priya \d+:\d\d\]/.test(md), "transcript label not renamed");
  assert(/speaker_map:\n- speaker_label: SPEAKER_\d+\n  name: Priya/.test(md), "speaker_map not written");
  assert(md.includes("- Priya"), "attendees not updated");
  assert(md.includes("- decided: ship Fast by default"), "rename lost notes");
});
await check("the saved voice shows in Settings", async () => {
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByText("Voices Margin recognises").waitFor();
  await page.locator(".voices li", { hasText: "Priya" }).waitFor();
  await button("Done").click();
});
await check("a second meeting with the same voice is labelled automatically", async () => {
  dev("queue", audio, "QA follow-up");
  await page.locator(".row").filter({ hasText: "QA follow-up" }).first().waitFor({ timeout: 120000 });
  await page.locator(".row-job").waitFor({ state: "detached", timeout: 120000 });
  const f = meetingFiles().find((x) => x.includes("qa-follow-up"));
  const md = fs.readFileSync(path.join(meetingsDir, f), "utf8");
  assert(/\[Priya \d+:\d\d\]/.test(md), "Priya not recognised in the second meeting");
  await page.locator(".row").filter({ hasText: "QA follow-up" }).first().click();
  await page.locator(".chip", { hasText: "Priya" }).waitFor();
});

// ---------------------------------------------------------------- search
await check("search finds a meeting by transcript text", async () => {
  await page.getByLabel("Search meetings").fill("legal review");
  await page.locator(".row-hit").first().waitFor({ timeout: 5000 });
  assert((await page.locator(".row-hit").first().innerText()).toLowerCase().includes("legal review"), "hit excerpt wrong");
});
await check("search with no match says so", async () => {
  await page.getByLabel("Search meetings").fill("zebra unicorn");
  await page.getByText(/No meetings mention/).waitFor();
  await page.getByLabel("Search meetings").press("Escape");
  await page.locator(".row").filter({ hasText: "QA launch sync" }).first().waitFor();
});

// ---------------------------------------------------------------- summaries
if (summaries !== "none") {
  await check(`summaries via ${summaries}`, async () => {
    if (summaries === "fake-ollama") {
      const cfgPath = path.join(marginHome, "config.toml");
      fs.writeFileSync(cfgPath, fs.readFileSync(cfgPath, "utf8").replace(/^ollama_url = .*$/m, 'ollama_url = "http://127.0.0.1:11435"'));
    }
    await page.getByRole("button", { name: "Settings" }).click();
    const label = /Ollama, on this Mac/;
    await page.getByRole("radio", { name: label }).click();
    await page.waitForTimeout(500);
    await button("Done").click();
    await page.locator(".row").filter({ hasText: "QA launch sync" }).first().click();
    await button("Summarize").click();
    await page.locator(".toast").filter({ hasText: /Summary added|Summary failed|returned nothing/ }).waitFor({ timeout: 300000 });
    const toast = await page.locator(".toast").innerText();
    assert(toast.includes("Summary added"), toast);
    await page.locator(".eyebrow", { hasText: "Summary" }).waitFor();
    const md = fs.readFileSync(mdPath, "utf8");
    assert(md.includes("## Summary"), "no summary in file");
    assert(md.includes("- decided: ship Fast by default"), "summarizing lost the user's notes");
    assert(/\[Priya \d+:\d\d\]/.test(md), "summarizing lost the rename");
  });
}

// ---------------------------------------------------------------- settings: model switch
if (flag("--download")) {
  await check("switching to Fast downloads it with progress", async () => {
    await page.getByRole("button", { name: "Settings" }).click();
    await page.getByRole("radio", { name: /Fast/ }).click();
    await page.getByText(/Downloading speech models/).waitFor({ timeout: 15000 });
    await page.screenshot({ path: path.join(out, "download-progress.png") });
    await page.getByText("Speech models ready").waitFor({ timeout: 600000 });
    assert(fs.existsSync(path.join(marginHome, "state/models/ggml-base.bin")), "base model missing");
    await button("Done").click();
  });
}

// ---------------------------------------------------------------- reset
await check("reset keeps meetings and models, then runs setup again", async () => {
  const before = meetingFiles().length;
  await page.getByRole("button", { name: "Settings" }).click();
  await button("Reset Margin…").click();
  await page.getByText("Reset Margin?").waitFor();
  await page.getByText(/Keep my meetings \(\d+ in/).waitFor();
  await page.getByRole("alertdialog").getByRole("button", { name: "Reset Margin" }).click();
  await page.getByText("Notes in your words.").waitFor({ timeout: 10000 });
  assert(!fs.existsSync(path.join(marginHome, "config.toml")), "config not removed");
  assert(!fs.existsSync(path.join(marginHome, "prefs.json")), "prefs not removed");
  assert(fs.existsSync(path.join(marginHome, "state/models")), "models removed");
  assert(meetingFiles().length === before, `meetings changed: ${before} -> ${meetingFiles().length}`);
});
await check("after re-onboarding the kept meetings are back", async () => {
  await button("Continue").click();
  await page.getByRole("radio", { name: /Documents/ }).click();
  await button("Use this folder").click();
  await button("Download and continue").click();
  await button("Continue").click();
  await button("Start using Margin").click();
  await page.locator(".row").filter({ hasText: "QA launch sync" }).first().waitFor({ timeout: 10000 });
});
await check("deleting meetings requires typing delete", async () => {
  await page.getByRole("button", { name: "Settings" }).click();
  await button("Reset Margin…").click();
  await page.getByText("Keep my meetings").click();
  const go = page.getByRole("alertdialog").getByRole("button", { name: "Reset Margin" });
  assert(await go.isDisabled(), "reset enabled without typing delete");
  await page.getByPlaceholder("delete").fill("delete");
  assert(!(await go.isDisabled()), "reset still disabled after typing delete");
  await page.getByRole("alertdialog").getByRole("button", { name: "Cancel" }).click();
  await button("Done").click();
});

await check("no UI errors or failed commands", async () => {
  const errs = await page.evaluate(() => window.__E2E__.errors);
  const real = errs.filter((e) => !e.startsWith("request_permission"));
  assert(real.length === 0 && consoleErrors.length === 0, [...real, ...consoleErrors].join(" | "));
});

fs.writeFileSync(path.join(out, "report.json"), JSON.stringify(results, null, 2));
await browser.close();
for (const p of procs) p.kill();
const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length}/${results.length} passed. Screenshots in ${out}`);
process.exit(failed.length ? 1 : 0);
