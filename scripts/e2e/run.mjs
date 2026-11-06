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
// --dark runs the whole pass in dark mode (for screenshots).
// --download also tests a real model switch + download (Fast model, ~148 MB).
// Ask (chat) runs against a stand-in claude (scripts/e2e/fake-claude.py), never the real one.
// Screenshots and report.json go to $E2E_OUT (default /tmp/margin-e2e/out).
// E2E_BRIDGE_PORT, E2E_UI_PORT, E2E_OLLAMA_PORT and CARGO_TARGET_DIR move a run off the defaults.
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
// --simulate <wav>: debug builds play this file instead of the mic (MARGIN_SIMULATE_CAPTURE),
// with <wav>.voice.wav / .system.wav stems and <wav>.live.jsonl as the live transcript.
const simulate = opt("--simulate");
if (!audio) throw new Error("--audio <wav> is required");

const base = process.env.E2E_DIR || "/tmp/margin-e2e";
const out = process.env.E2E_OUT || path.join(base, "out");
const home = path.join(base, "home");
const marginHome = path.join(base, "margin");
fs.rmSync(home, { recursive: true, force: true });
fs.rmSync(marginHome, { recursive: true, force: true });
fs.rmSync(out, { recursive: true, force: true });
fs.mkdirSync(out, { recursive: true });
fs.mkdirSync(home, { recursive: true });
if (seed) {
  const dest = path.join(marginHome, "state/models");
  fs.cpSync(seed, dest, { recursive: true });
}
const meetingsDir = path.join(home, "Documents/Meetings");
const env = { ...process.env, HOME: home, MARGIN_HOME: marginHome, ...(simulate ? { MARGIN_SIMULATE_CAPTURE: path.resolve(simulate) } : {}) };
const bin = path.join(process.env.CARGO_TARGET_DIR || path.join(root, "src-tauri/target"), "debug/margin");
// Ports can be moved so several runs (one per checkout) don't collide.
const port = { bridge: process.env.E2E_BRIDGE_PORT || "4545", ui: process.env.E2E_UI_PORT || "4174", ollama: process.env.E2E_OLLAMA_PORT || "11435" };

const procs = [];
const start = (cmd, args, extra = {}) => {
  const p = spawn(cmd, args, { env, cwd: root, stdio: ["ignore", "pipe", "pipe"], ...extra });
  const log = fs.createWriteStream(path.join(out, `${path.basename(cmd)}.log`));
  p.stdout.pipe(log);
  p.stderr.pipe(log);
  procs.push(p);
  return p;
};
start(bin, ["--dev", "serve", port.bridge]);
if (summaries === "fake-ollama") start("/usr/bin/python3", ["-I", path.join(root, "scripts/e2e/fake-ollama.py"), port.ollama, path.join(out, "ollama-requests.log")]);
start("bunx", ["vite", "preview", "--port", port.ui, "--strictPort"]);
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
const ctx = await browser.newContext({ viewport: { width: 1180, height: 760 }, deviceScaleFactor: 2, colorScheme: flag("--dark") ? "dark" : "light" });
const page = await ctx.newPage();
const consoleErrors = [];
page.on("pageerror", (e) => consoleErrors.push(e.message));
page.on("console", (m) => m.type() === "error" && consoleErrors.push(m.text()));
await page.addInitScript(`window.__E2E_BRIDGE__ = "http://127.0.0.1:${port.bridge}";`);
await page.addInitScript({ path: path.join(root, "scripts/e2e/bridge-shim.js") });
// Record what the page copies and which sounds it plays, without needing a clipboard or speakers.
await page.addInitScript(() => {
  window.__copied = [];
  window.__plays = [];
  Object.defineProperty(navigator, "clipboard", { value: { writeText: async (t) => void window.__copied.push(t) }, configurable: true });
  const play = HTMLMediaElement.prototype.play;
  HTMLMediaElement.prototype.play = function () {
    window.__plays.push(this.currentSrc || this.src);
    return play.call(this);
  };
});
const copied = () => page.evaluate(() => window.__copied.at(-1) ?? "");
const plays = () => page.evaluate(() => window.__plays.map((s) => s.split("/").pop()));
// Every event the backend sent, including system notifications the app would show.
const bridgeEvents = async () => (await (await fetch(`http://127.0.0.1:${port.bridge}/events?since=0`)).json());
await page.goto(`http://localhost:${port.ui}`);
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
  assert(!fs.existsSync(meetingsDir), "default folder created before it was chosen");
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

// ---------------------------------------------------------------- record a call (simulated mic)
if (simulate) {
  await check("record a call: live notepad opens", async () => {
    await page.getByRole("button", { name: "Record call", exact: true }).click();
    await page.getByLabel("Meeting title").waitFor({ timeout: 10000 });
    await page.locator(".rule-live").waitFor();
    await page.locator(".row-live").waitFor();
  });
  await check("live transcript streams with You / Them", async () => {
    await page.getByLabel("Meeting title").fill("QA live call");
    await page.locator(".row-live", { hasText: "QA live call" }).waitFor({ timeout: 3000 });
    await page.getByLabel("Your notes").click();
    await page.keyboard.type("- risk: download size");
    await page.locator(".live-line").nth(3).waitFor({ timeout: 45000 });
    const who = await page.locator(".live-speaker").allInnerTexts();
    assert(who.includes("You") && who.includes("Them"), `speakers seen: ${who.join(",")}`);
    const first = await page.locator(".live-line").first().innerText();
    assert(first.includes("launch plan"), `first line: ${first}`);
  });
  await check("typing right up to Stop keeps every word", async () => {
    await page.keyboard.press("Enter");
    await page.keyboard.type("last words before stop");
    await page.getByRole("button", { name: "Stop" }).click();
    await page.getByText(/Transcribing|Queued|Telling speakers apart|Saving/).first().waitFor({ timeout: 15000 });
  });
  let liveMd = null;
  await check("after processing, the meeting opens with the notes", async () => {
    await page.locator(".turn").first().waitFor({ timeout: 180000 });
    assert((await page.getByLabel("Meeting title").inputValue()) === "QA live call", "did not land on the processed meeting");
    const f = meetingFiles().find((x) => x.includes("qa-live-call"));
    assert(f, "no qa-live-call file");
    liveMd = fs.readFileSync(path.join(meetingsDir, f), "utf8");
    assert(liveMd.includes("## Notes\n\n- risk: download size\n- last words before stop\n"), "notes not exact:\n" + liveMd.slice(liveMd.indexOf("## Notes"), liveMd.indexOf("## Transcript")));
  });
  await check("sound cues play on start, stop and ready", async () => {
    const heard = await plays();
    for (const cue of ["cue-start.wav", "cue-stop.wav", "cue-complete.wav"]) assert(heard.includes(cue), `${cue} not played (${heard.join(", ")})`);
    const r = await page.evaluate(() => fetch("audio/cue-start.wav").then((x) => [x.status, x.headers.get("content-type")]));
    assert(r[0] === 200, `cue file not served: ${r}`);
  });
  await check("transcript rows use the same names as the speaker chips", async () => {
    const chips = new Set(await page.locator(".chip").allInnerTexts());
    const rows = new Set(await page.locator(".turn-speaker").allInnerTexts());
    for (const r of rows) assert(chips.has(r), `row label "${r}" has no chip (${[...chips].join(", ")})`);
  });
  await check("call stems give speaker separation", async () => {
    const labels = [...new Set([...liveMd.matchAll(/^\[([^\]]+?) \d+:\d\d\]/gm)].map((m) => m[1]))];
    assert(labels.length >= 2, `labels: ${labels.join(", ")}`);
    fs.writeFileSync(path.join(out, "live-call.md"), liveMd);
  });
}

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
await check("a 'meeting ready' notification is sent for it", async () => {
  const ready = (await bridgeEvents()).filter((e) => e.event === "notify" && e.payload.title === "Meeting ready");
  const n = ready.find((e) => e.payload.body.includes("QA launch sync"));
  assert(n, `no notification: ${JSON.stringify(ready.map((e) => e.payload.body))}`);
  assert(n.payload.meeting?.includes("qa-launch-sync"), `notification opens ${n.payload.meeting}`);
});
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
  await page.locator(".turn").first().waitFor();
  assert((await page.getByLabel("Meeting title").inputValue()) === "QA launch sync", "wrong meeting opened");
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

// ---------------------------------------------------------------- playback and copy
await check("play the recording and jump from a transcript time", async () => {
  await page.locator(".player").waitFor({ timeout: 5000 });
  await page.waitForFunction(() => (document.querySelector(".player audio")?.duration ?? 0) > 1, null, { timeout: 10000 });
  const turn = page.locator(".turn").nth(2);
  const ts = await turn.locator(".ts-btn").innerText();
  const [m, sec] = ts.split(":").map(Number);
  await turn.locator(".ts-btn").click();
  await page.waitForFunction(() => !document.querySelector(".player audio").paused, null, { timeout: 5000 });
  const at = await page.evaluate(() => document.querySelector(".player audio").currentTime);
  assert(Math.abs(at - (m * 60 + sec)) < 1.5, `jumped to ${at}, wanted ${ts}`);
  await page.locator(".turn.playing").first().waitFor({ timeout: 3000 });
  await page.getByRole("button", { name: "Pause recording" }).click();
  assert(await page.evaluate(() => document.querySelector(".player audio").paused), "pause did nothing");
  // The bridge, like the asset protocol, serves byte ranges and nothing outside the folder.
  const wav = await page.evaluate(() => document.querySelector(".player audio").src);
  const r = await fetch(wav, { headers: { Range: "bytes=0-99" } });
  assert(r.status === 206 && (await r.arrayBuffer()).byteLength === 100, `range: ${r.status}`);
  const outside = await fetch(`http://127.0.0.1:${port.bridge}/asset?path=${encodeURIComponent(path.join(base, "silence.wav"))}`);
  assert(outside.status === 404, `file outside the folder served: ${outside.status}`);
});
await check("copy the transcript and the whole meeting as Markdown", async () => {
  await page.locator(".actions").getByRole("button", { name: "Copy", exact: true }).click();
  await page.getByRole("menuitem", { name: /Transcript/ }).click();
  await page.locator(".toast", { hasText: "Transcript copied." }).waitFor();
  const t = await copied();
  assert(/^\[Speaker 1 0:00\] /.test(t) && !t.includes("SPEAKER_"), `transcript copy: ${t.slice(0, 80)}`);
  await page.locator(".actions").getByRole("button", { name: "Copy", exact: true }).click();
  await page.getByRole("menuitem", { name: /Whole meeting/ }).click();
  const all = await copied();
  assert(all.startsWith("# QA launch sync\n") && !all.includes("\n---\n"), `meeting copy starts: ${all.slice(0, 60)}`);
  assert(all.includes("## Notes\n\n- download size is the #1 risk") && all.includes("## Transcript\n\n[Speaker 1 0:00]"), "meeting copy is missing a part");
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

await check("click a person to see only their meetings", async () => {
  await page.locator(".row").filter({ hasText: "QA launch sync" }).first().click();
  await page.locator(".person-btn", { hasText: "Priya" }).first().click();
  await page.locator(".filter-pill", { hasText: "Priya" }).waitFor();
  const rows = await page.locator(".list .row-title").allInnerTexts();
  assert(rows.some((r) => r.includes("QA follow-up")) && rows.some((r) => r.includes("QA launch sync")), `rows: ${rows.join(" | ")}`);
  assert(!rows.some((r) => r.includes("QA live call")), `a meeting without Priya is listed: ${rows.join(" | ")}`);
  await page.getByRole("button", { name: "Show all meetings" }).click();
  await page.locator(".filter-pill").waitFor({ state: "detached" });
});
await check("action items from the frontmatter can be ticked and copied", async () => {
  const fixture = path.join(meetingsDir, "2026-11-01-qa-actions.md");
  fs.writeFileSync(fixture, [
    "---", "title: QA actions", "type: meeting", "date: 2026-11-01T10:00:00+05:30", "duration: 30m",
    "attendees:", "- Priya", "action_items:",
    "- assignee: Priya", "  task: send the deck", "  due: Friday", "  status: open",
    "- assignee: unassigned", "  task: book the room", "  status: open", "---", "",
    "## Summary", "", "- We planned the launch.", "", "## Action Items", "", "- [ ] @Priya: send the deck", "- [ ] book the room", "",
    "## Transcript", "", "[Priya 0:00] Let's plan the launch.", "",
  ].join("\n"), { mode: 0o600 });
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await page.locator(".row").filter({ hasText: "QA actions" }).first().click();
  const items = page.locator(".action-items li");
  await items.first().waitFor({ timeout: 5000 });
  assert((await items.count()) === 2, `items: ${await items.count()}`);
  assert((await items.first().innerText()).includes("Priya · due Friday"), await items.first().innerText());
  const plain = await page.locator(".summary-block > .eyebrow").allInnerTexts();
  assert(!plain.some((h) => /^action items$/i.test(h.trim())), `checklist shown twice: ${plain.join(", ")}`);
  await page.getByRole("checkbox", { name: "send the deck" }).click();
  await page.waitForFunction(() => document.querySelector('[aria-label="send the deck"]')?.getAttribute("aria-checked") === "true");
  await page.waitForTimeout(600);
  const md = fs.readFileSync(fixture, "utf8");
  assert(/task: send the deck\n  due: Friday\n  status: done/.test(md), "frontmatter status not done");
  assert(md.includes("- [x] @Priya: send the deck") && md.includes("- [ ] book the room"), "body checkbox not ticked");
  await page.locator(".action-items").getByRole("button", { name: "Copy" }).click();
  assert((await copied()) === "- [x] @Priya: send the deck (due Friday)\n- [ ] book the room", `copied: ${await copied()}`);
});
await check("rename the meeting from its title", async () => {
  await page.locator(".row").filter({ hasText: "QA launch sync" }).first().click();
  const title = page.getByLabel("Meeting title");
  await title.click();
  await page.keyboard.press("Meta+A");
  await page.keyboard.type("QA launch sync, renamed");
  await page.keyboard.press("Enter");
  await page.locator(".row-title", { hasText: "QA launch sync, renamed" }).waitFor({ timeout: 5000 });
  assert(/^title: QA launch sync, renamed$/m.test(fs.readFileSync(mdPath, "utf8")), "frontmatter title not updated");
});
await check("a silent recording explains itself", async () => {
  const silent = path.join(base, "silence.wav");
  const wav = Buffer.alloc(44 + 16000 * 2 * 6);
  wav.write("RIFF", 0); wav.writeUInt32LE(36 + 16000 * 12, 4); wav.write("WAVEfmt ", 8); wav.writeUInt32LE(16, 16);
  wav.writeUInt16LE(1, 20); wav.writeUInt16LE(1, 22); wav.writeUInt32LE(16000, 24); wav.writeUInt32LE(32000, 28);
  wav.writeUInt16LE(2, 32); wav.writeUInt16LE(16, 34); wav.write("data", 36); wav.writeUInt32LE(16000 * 12, 40);
  fs.writeFileSync(silent, wav);
  dev("queue", silent, "QA silent");
  await page.locator(".row").filter({ hasText: "QA silent" }).first().waitFor({ timeout: 60000 });
  await page.locator(".row-job").waitFor({ state: "detached", timeout: 60000 });
  await page.locator(".row").filter({ hasText: "QA silent" }).first().click();
  await page.getByText("No speech was detected in this recording.").waitFor({ timeout: 5000 });
});

await check("switching meetings right after typing still saves the notes", async () => {
  await page.locator(".row").filter({ hasText: "QA follow-up" }).first().click();
  await page.getByLabel("Your notes").click();
  await page.keyboard.press("Meta+ArrowDown");
  await page.keyboard.type("\nquick switch note");
  await page.locator(".row").filter({ hasText: "QA launch sync" }).first().click();
  await page.waitForTimeout(1500);
  const f = meetingFiles().find((x) => x.includes("qa-follow-up"));
  assert(fs.readFileSync(path.join(meetingsDir, f), "utf8").includes("quick switch note"), "edit lost on quick switch");
});
if (simulate) {
  await check("leaving and returning to a recording keeps its notes", async () => {
    await page.getByRole("button", { name: "More recording options" }).click();
    await page.getByRole("menuitem", { name: /In person/ }).click();
    await page.locator(".page-live").waitFor({ timeout: 10000 });
    await page.getByLabel("Your notes").click();
    await page.keyboard.type("- kept across navigation");
    await page.locator(".row").filter({ hasText: "QA follow-up" }).first().click();
    await page.waitForTimeout(800);
    await page.locator(".row-live").click();
    assert((await page.getByLabel("Your notes").inputValue()).includes("kept across navigation"), "live notes wiped on return");
    assert((await page.locator(".live-speaker").count()) === 0, "in-person recording shows You/Them");
    await page.getByLabel("Meeting title").fill("QA in person");
    await page.waitForTimeout(8000); // a first sentence of speech
    await page.getByRole("button", { name: "Stop" }).click();
    await page.locator(".turn").first().waitFor({ timeout: 180000 });
    const f = meetingFiles().find((x) => x.includes("qa-in-person"));
    assert(f && fs.readFileSync(path.join(meetingsDir, f), "utf8").includes("- kept across navigation"), "notes missing in saved file");
  });
}

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
      fs.writeFileSync(cfgPath, fs.readFileSync(cfgPath, "utf8").replace(/^ollama_url = .*$/m, `ollama_url = "http://127.0.0.1:${port.ollama}"`));
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
  await check("a summary's own checklist can be ticked", async () => {
    await page.getByRole("checkbox", { name: /Build the settings screen/ }).click();
    await page.waitForTimeout(800);
    assert(fs.readFileSync(mdPath, "utf8").includes("- [x] Build the settings screen"), "checklist box not ticked");
  });
  await check("summarize again replaces the summary and keeps notes", async () => {
    await page.getByRole("button", { name: "Summarize again" }).click();
    await page.locator(".toast").filter({ hasText: /Summary updated|failed|nothing/ }).waitFor({ timeout: 300000 });
    assert((await page.locator(".toast").innerText()).includes("Summary updated"), await page.locator(".toast").innerText());
    const md = fs.readFileSync(mdPath, "utf8");
    assert((md.match(/^## Summary$/gm) || []).length === 1, "summary duplicated");
    assert(md.includes("- decided: ship Fast by default") && /\[Priya \d+:\d\d\]/.test(md), "summarizing again lost notes or names");
    const asked = fs.readFileSync(path.join(out, "ollama-requests.log"), "utf8").trim().split("\n").length;
    assert(asked >= 2, `ollama asked ${asked} times`);
  });
}

// ---------------------------------------------------------------- settings parity
const cfgText = () => fs.readFileSync(path.join(marginHome, "config.toml"), "utf8");
const inSection = (section, line) => new RegExp(`^\\[${section}\\]\\n(?:[^\\[].*\\n)*?${line}$`, "m").test(cfgText());
const loginPlist = path.join(home, "Library/LaunchAgents/com.akashmishra.margin.login.plist");
const openSettings = async () => {
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("dialog", { name: "Settings" }).waitFor();
};
const settled = () => page.waitForTimeout(700); // saves are queued; let the last one land
const toggle = (name) => page.getByRole("switch", { name });
// The sheet scrolls, so take one screenshot per screenful.
const shootSheet = async (name) => {
  const body = page.locator(".sheet-body");
  const [h, vh] = await body.evaluate((el) => [el.scrollHeight, el.clientHeight]);
  for (let y = 0, i = 1; y < h; y += vh - 40, i++) {
    await body.evaluate((el, top) => el.scrollTo(0, top), y);
    await page.screenshot({ path: path.join(out, `${name}-${i}.png`) });
  }
  await body.evaluate((el) => el.scrollTo(0, 0));
};

await check("settings: new choices start at calm defaults", async () => {
  await openSettings();
  await page.getByRole("heading", { name: "Recording", exact: true }).waitFor();
  const value = (label) => page.getByLabel(label, { exact: true }).inputValue();
  assert((await value("Microphone")) === "", "microphone is not the system default");
  assert((await value("Keep audio")) === "0", "audio is not kept forever");
  assert((await value("Language spoken in meetings")) === "auto", "language is not auto-detect");
  assert((await value("Stop after silence")) === "30", "silence limit is not 30 minutes");
  assert(await toggle(/Tell speakers apart/).isChecked(), "speaker separation off");
  assert(await toggle(/Suggest recording when a call starts/).isChecked(), "call suggestions off");
  assert(!(await toggle(/Open Margin when you log in/).isChecked()), "open at login on");
  await shootSheet("settings-sheet");
});
await check("settings: language, microphone and silence limit are saved for core", async () => {
  await page.getByLabel("Language spoken in meetings").selectOption("en");
  const mic = (await page.getByLabel("Microphone", { exact: true }).locator("option").allInnerTexts()).find((m) => m !== "System default");
  if (mic) await page.getByLabel("Microphone", { exact: true }).selectOption({ label: mic });
  await page.getByLabel("Stop after silence").selectOption("15");
  await settled();
  assert(inSection("transcription", 'language = "en"'), "transcription.language not written");
  assert(inSection("recording", "silence_auto_stop_secs = 900"), "silence limit not written");
  if (mic) assert(inSection("recording", `device = "${mic}"`), "microphone not written");
  await button("Done").click();
  await openSettings();
  assert((await page.getByLabel("Language spoken in meetings").inputValue()) === "en", "language not kept");
  assert((await page.getByLabel("Stop after silence").inputValue()) === "15", "silence limit not kept");
  // Back to the default so nothing below depends on this Mac's hardware.
  await page.getByLabel("Microphone", { exact: true }).selectOption("");
  await settled();
  assert(!/^device = /m.test(cfgText()), "system default not restored");
});
await check("settings: names to spell right go to core's vocabulary", async () => {
  const box = page.getByLabel("Names and words to spell right");
  await box.fill("Priya Raman\nKubernetes\npriya raman");
  await box.blur();
  await settled();
  const voc = fs.readFileSync(path.join(marginHome, "state/vocabulary.toml"), "utf8");
  assert(voc.includes('canonical = "Priya Raman"') && voc.includes('canonical = "Kubernetes"'), "terms missing:\n" + voc);
  assert(voc.match(/Priya Raman/gi).length === 1, "duplicate term kept");
  assert(voc.includes('source = "manual"'), "terms not marked as yours");
});
await check("settings: open at login adds and removes a login item", async () => {
  const sw = toggle(/Open Margin when you log in/);
  await sw.check();
  await settled();
  assert(fs.existsSync(loginPlist), "no LaunchAgent written");
  execFileSync("/usr/bin/plutil", ["-lint", loginPlist]);
  assert(fs.readFileSync(loginPlist, "utf8").includes("<string>--at-login</string>"), "LaunchAgent lacks --at-login");
  await sw.uncheck();
  await settled();
  assert(!fs.existsSync(loginPlist), "LaunchAgent not removed");
  await sw.check(); // left on: Reset must turn it off
  await settled();
});
await check("settings: call suggestions follow the switch", async () => {
  // A stand-in process named like Zoom's, which core's call detection looks for. (A
  // copy of /bin/sleep is killed by macOS; a symlink keeps its own name in `ps`.)
  const zoom = path.join(base, "bin/zoom.us");
  fs.mkdirSync(path.dirname(zoom), { recursive: true });
  fs.rmSync(zoom, { force: true });
  fs.symlinkSync("/bin/sleep", zoom);
  await toggle(/Suggest recording when a call starts/).uncheck();
  await settled();
  assert(inSection("call_detection", "enabled = false"), "call_detection.enabled not false");
  await button("Done").click();
  const fake = spawn(zoom, ["300"], { stdio: "ignore" });
  procs.push(fake);
  await page.waitForTimeout(17000); // the detector polls every 15 s
  assert((await page.locator(".call-banner").count()) === 0, "banner shown while switched off");
  await openSettings();
  await toggle(/Suggest recording when a call starts/).check();
  await settled();
  await button("Done").click();
  await page.locator(".call-banner", { hasText: "Zoom looks active" }).waitFor({ timeout: 20000 });
  fake.kill();
  await page.locator(".call-banner").getByRole("button", { name: "Dismiss" }).click();
});
if (summaries === "fake-ollama") {
  await check("settings: summary language and instructions reach the summarizer", async () => {
    await openSettings();
    await page.getByLabel("Summary language").selectOption("French");
    const box = page.getByLabel("Instructions for summaries");
    await box.fill("Start with decisions. Keep each point short.");
    await box.blur();
    await settled();
    assert(inSection("summarization", 'language = "French"'), "summary language not written");
    await button("Done").click();
    const log = path.join(out, "ollama-requests.log");
    const seen = fs.readFileSync(log, "utf8").split("\n").filter(Boolean).length;
    await page.locator(".row").filter({ hasText: "QA follow-up" }).first().click();
    await button("Summarize").click();
    await page.locator(".toast").filter({ hasText: /Summary added|failed|returned nothing/ }).waitFor({ timeout: 120000 });
    assert((await page.locator(".toast").innerText()).includes("Summary added"), await page.locator(".toast").innerText());
    const sent = fs.readFileSync(log, "utf8").split("\n").filter(Boolean).slice(seen).map((l) => JSON.parse(l));
    const text = sent.map((r) => `${r.system}\n${r.prompt}`).join("\n");
    assert(text.includes("Start with decisions. Keep each point short."), "instructions not sent");
    assert(text.includes("Always respond in French"), "summary language not sent");
    const md = fs.readFileSync(path.join(meetingsDir, meetingFiles().find((x) => x.includes("qa-follow-up"))), "utf8");
    assert(/^template: meeting$/m.test(md), "meeting not tagged with the template");
  });
}
await check("settings: with speaker separation off, a meeting has no speaker labels", async () => {
  await openSettings();
  await toggle(/Tell speakers apart/).uncheck();
  await settled();
  assert(inSection("diarization", 'engine = "none"'), "diarization.engine not none");
  await button("Done").click();
  dev("queue", audio, "QA no speakers");
  await page.locator(".row").filter({ hasText: "QA no speakers" }).first().waitFor({ timeout: 120000 });
  await page.locator(".row-job").waitFor({ state: "detached", timeout: 120000 });
  const md = fs.readFileSync(path.join(meetingsDir, meetingFiles().find((x) => x.includes("qa-no-speakers"))), "utf8");
  fs.writeFileSync(path.join(out, "no-speakers.md"), md);
  assert(md.includes("## Transcript"), "no transcript");
  assert(!/^\[(SPEAKER_\d+|Priya) \d+:\d\d\]/m.test(md), "transcript still has speaker labels");
  await openSettings();
  await toggle(/Tell speakers apart/).check();
  await settled();
  assert(inSection("diarization", 'engine = "auto"'), "diarization not back on");
  await button("Done").click();
});
await check("settings: keep audio for 7 days deletes only older audio", async () => {
  const recent = meetingFiles().find((x) => x.includes("qa-launch-sync"));
  const recentWav = path.join(meetingsDir, recent.replace(/\.md$/, ".wav"));
  const oldMd = path.join(meetingsDir, "2026-09-01-qa-old-meeting.md");
  const oldWav = oldMd.replace(/\.md$/, ".wav");
  const longAgo = new Date(Date.now() - 40 * 86400e3).toISOString();
  fs.writeFileSync(oldMd, fs.readFileSync(path.join(meetingsDir, recent), "utf8").replace(/^date: .*$/m, `date: ${longAgo}`).replace(/^title: .*$/m, "title: QA old meeting"));
  fs.copyFileSync(recentWav, oldWav);
  await openSettings();
  await page.getByLabel("Keep audio").selectOption("7");
  await settled();
  assert(inSection("retention", "auto_cleanup = true") && inSection("retention", "successful_audio_days = 7"), "retention not written");
  assert(!fs.existsSync(oldWav), "old audio kept");
  assert(fs.existsSync(oldMd), "old meeting file removed");
  assert(fs.existsSync(recentWav), "recent audio removed");
  await page.getByLabel("Keep audio").selectOption("0");
  await settled();
  assert(inSection("retention", "auto_cleanup = false"), "forever not restored");
  await button("Done").click();
});

// ---------------------------------------------------------------- ask (stand-in claude)
const claudeDir = path.join(base, "fake-claude");
const claudeBin = path.join(claudeDir, "claude");
const claudeCalls = () => {
  const f = path.join(claudeDir, "calls.jsonl");
  return fs.existsSync(f) ? fs.readFileSync(f, "utf8").trim().split("\n").map((l) => JSON.parse(l)) : [];
};
const argAfter = (call, flag) => call.args[call.args.indexOf(flag) + 1];
const chatFile = (name) => path.join(marginHome, "state/chat", name);
const useFakeClaude = () => {
  const cfgPath = path.join(marginHome, "config.toml");
  const cfg = fs.readFileSync(cfgPath, "utf8").replace(/(\[summarization\][^[]*?)agent_command = "[^"]*"/s, `$1agent_command = "${claudeBin}"`);
  assert(cfg.includes(claudeBin), "could not point agent_command at the stand-in claude");
  fs.writeFileSync(cfgPath, cfg);
};
const answered = () => page.locator(".ask-a:not(.ask-pending)").last();

await check("ask: not signed in says how to fix it", async () => {
  fs.rmSync(claudeDir, { recursive: true, force: true });
  fs.mkdirSync(claudeDir, { recursive: true });
  fs.copyFileSync(path.join(root, "scripts/e2e/fake-claude.py"), claudeBin);
  fs.chmodSync(claudeBin, 0o755);
  fs.writeFileSync(path.join(claudeDir, "logged-out"), "");
  useFakeClaude();
  await page.locator(".row").filter({ hasText: "QA follow-up" }).first().click();
  await page.getByLabel("Meeting title").waitFor();
  await page.keyboard.press("Meta+j");
  await page.locator(".ask").waitFor({ timeout: 5000 });
  try {
    await page.locator(".ask-setup", { hasText: /isn't signed in.*run claude/ }).waitFor({ timeout: 8000 });
    assert(await page.locator(".ask-compose").getByRole("button", { name: "Ask" }).isDisabled(), "Ask enabled while not signed in");
    await page.screenshot({ path: path.join(out, "ask-not-signed-in.png") });
  } finally {
    fs.rmSync(path.join(claudeDir, "logged-out"), { force: true });
  }
  await button("Check again").click();
  await page.locator(".ask-setup").waitFor({ state: "detached", timeout: 8000 });
});
await check("ask across all meetings streams an answer with meeting links", async () => {
  await page.locator(".ask").getByRole("radio", { name: "All meetings" }).click();
  await page.getByText("Across all your meetings").waitFor();
  await page.locator(".ask-chip", { hasText: "What did I commit to this week?" }).click();
  // The answer shows while it streams, before it is done.
  await page.waitForFunction(() => (document.querySelector(".ask-pending .md")?.textContent ?? "").includes("From"), null, { timeout: 10000 });
  await page.screenshot({ path: path.join(out, "ask-streaming.png") });
  await answered().locator(".ask-sources").waitFor({ timeout: 15000 });
  const text = await answered().innerText();
  assert(text.includes("build the settings screen"), `answer: ${text}`);
  assert(await answered().locator(".ask-sources .link-btn", { hasText: "QA launch sync" }).count(), "no QA launch sync source");
  const call = claudeCalls().at(-1);
  assert(fs.realpathSync(call.cwd) === fs.realpathSync(meetingsDir), `cwd ${call.cwd}`);
  assert(argAfter(call, "--tools") === "Read,Grep,Glob", `tools ${argAfter(call, "--tools")}`);
  assert(argAfter(call, "--permission-mode") === "dontAsk", "not dontAsk");
  assert(!call.args.includes("--resume") && !call.args.includes("--allowedTools"), "unexpected flags");
  assert(call.prompt.includes("QA launch sync") && call.prompt.includes("Question: What did I commit to this week?"), "prompt lacks the meeting list");
});
await check("ask: a follow-up continues the same conversation", async () => {
  await page.getByLabel("Ask a question").fill("And who owns it?");
  await page.keyboard.press("Enter");
  await page.locator(".ask-a:not(.ask-pending)", { hasText: "Following up on “What did I commit to this week?”" }).waitFor({ timeout: 15000 });
  const call = claudeCalls().at(-1);
  const saved = JSON.parse(fs.readFileSync(chatFile("all.json"), "utf8"));
  assert(argAfter(call, "--resume") === saved.session_id, `resume ${argAfter(call, "--resume")} vs ${saved.session_id}`);
  assert(call.prompt === "And who owns it?", `prompt: ${call.prompt.slice(0, 80)}`);
});
await check("ask: Stop ends a long answer and its process", async () => {
  await page.getByLabel("Ask a question").fill("slow: walk me through everything");
  await page.keyboard.press("Enter");
  await page.locator(".ask-pending .md", { hasText: "Still going through it" }).waitFor({ timeout: 10000 });
  const pid = Number(fs.readFileSync(path.join(claudeDir, "claude.pid"), "utf8"));
  await page.locator(".ask-compose").getByRole("button", { name: "Stop", exact: true }).click();
  await answered().locator(".quiet-note", { hasText: "Stopped." }).waitFor({ timeout: 5000 });
  await page.waitForTimeout(1200);
  let alive = true;
  try { process.kill(pid, 0); } catch { alive = false; }
  assert(!alive, `claude ${pid} still running`);
});
await check("ask: a meeting link in an answer opens that meeting", async () => {
  assert((await page.getByLabel("Meeting title").inputValue()) === "QA follow-up", "expected to start on QA follow-up");
  await page.locator(".ask-a .md-link-btn", { hasText: "QA launch sync" }).first().click();
  await page.waitForFunction(() => document.querySelector('[aria-label="Meeting title"]')?.value?.startsWith("QA launch sync"), null, { timeout: 5000 });
  assert(await page.locator(".ask").isVisible(), "Ask closed on navigation");
});
await check("ask about one meeting puts that meeting in context", async () => {
  await page.locator(".ask").getByRole("radio", { name: "This meeting" }).click();
  await page.getByText(/About “QA launch sync/).waitFor();
  await page.locator(".ask-chip", { hasText: "Draft a follow-up email" }).waitFor();
  await page.locator(".ask-chip", { hasText: "What decisions were made?" }).click();
  await answered().locator(".ask-sources").waitFor({ timeout: 15000 });
  const call = claudeCalls().at(-1);
  assert(/<meeting file="2026-[^"]*qa-launch-sync[^"]*\.md">/.test(call.prompt) && call.prompt.includes("## Transcript"), "meeting file not in the prompt");
  assert(!call.args.includes("--resume"), "a new meeting chat resumed a session");
});
await check("ask: history survives a reload", async () => {
  await page.reload();
  await page.locator(".row").filter({ hasText: "QA launch sync" }).first().click();
  await button("Ask about this meeting").click();
  await page.locator(".ask-q", { hasText: "What decisions were made?" }).waitFor({ timeout: 8000 });
  await page.locator(".ask").getByRole("radio", { name: "All meetings" }).click();
  await page.locator(".ask-q", { hasText: "And who owns it?" }).waitFor();
  await page.locator(".ask .quiet-note", { hasText: "Stopped." }).waitFor();
});
await check("ask: Clear removes a conversation", async () => {
  await page.locator(".ask-head").getByRole("button", { name: "Clear" }).click();
  await page.locator(".ask-chip").first().waitFor();
  assert(!fs.existsSync(chatFile("all.json")), "all.json still there");
  assert(fs.readdirSync(path.join(marginHome, "state/chat")).length === 1, "the meeting chat should remain");
  await page.getByLabel("Ask a question").press("Escape");
  await page.locator(".ask").waitFor({ state: "detached" });
});

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

// ---------------------------------------------------------------- sounds and shortcut prefs
await check("settings: sounds and the global shortcut are saved", async () => {
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByLabel(/Play a sound/).uncheck();
  await page.getByLabel(/from any app/).check();
  await page.waitForTimeout(400);
  const prefs = JSON.parse(fs.readFileSync(path.join(marginHome, "prefs.json"), "utf8"));
  assert(prefs.mute_cues === true && prefs.record_shortcut === true, JSON.stringify(prefs));
  await button("Done").click();
  await page.getByRole("button", { name: "Settings" }).click();
  assert(!(await page.getByLabel(/Play a sound/).isChecked()) && (await page.getByLabel(/from any app/).isChecked()), "prefs not shown after reopening");
  await page.getByLabel(/Play a sound/).check();
  await page.getByLabel(/from any app/).uncheck();
  await page.waitForTimeout(400);
  await button("Done").click();
});

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
  assert(!fs.existsSync(loginPlist), "open at login not turned off");
  assert(meetingFiles().length === before, `meetings changed: ${before} -> ${meetingFiles().length}`);
});
await check("reset also clears Ask history", async () => {
  assert(!fs.existsSync(path.join(marginHome, "state/chat")), "state/chat survived reset");
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
