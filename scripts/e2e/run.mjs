// End-to-end QA: the real UI (built dist/) in headless WebKit, driven like a user, against
// the real backend (`margin --dev serve`) in a throwaway sandbox. Nothing touches your
// real settings, meetings or voices, and no macOS permission prompt can appear.
//
//   bun run build && (cd src-tauri && cargo build)
//   scripts/e2e/make-fixtures.sh /tmp/margin-fixtures
//   node scripts/e2e/run.mjs --audio /tmp/margin-fixtures/meeting.wav \
//     --seed-models /tmp/margin-fixtures/models --summaries fake-ollama \
//     --simulate /tmp/margin-fixtures/call.wav [--dark] [--download]
//
// make-fixtures.sh makes the audio this pass expects with the macOS voices: meeting.wav
// (two voices, says "legal review" for the search check) and call.wav with its stems and
// live transcript (long enough that the test, not the file, stops the call). It also links
// the small models from your Margin install into models/.
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
// With --simulate, rolling transcription cuts 8 s pieces, so the short call has several.
const env = { ...process.env, HOME: home, MARGIN_HOME: marginHome, ...(simulate ? { MARGIN_SIMULATE_CAPTURE: path.resolve(simulate), MARGIN_ROLLING_PIECE_SECS: "8" } : {}) };
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
  await page.screenshot({ path: path.join(out, `${id}-${name.replace(/[^a-z0-9]+/gi, "-").toLowerCase()}.png`), animations: "disabled" }).catch(() => {});
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
// Settings is a page with a section nav, not a dialog.
const settingsPage = () => page.getByRole("region", { name: "Settings" });
const settingsNav = () => page.getByRole("navigation", { name: "Settings sections" });
const goSection = async (label) => {
  await settingsNav().getByRole("button", { name: label, exact: true }).click();
  await settingsPage().getByRole("heading", { level: 1, name: label === "About" ? "Margin" : label, exact: true }).waitFor();
};
const openSettings = async (section = "General") => {
  if (!(await settingsPage().isVisible())) await page.getByRole("button", { name: "Settings", exact: true }).click();
  await settingsPage().waitFor();
  await goSection(section);
};
const closeSettings = async () => {
  await page.getByRole("button", { name: "Back", exact: true }).click();
  await settingsPage().waitFor({ state: "detached" });
};

// ---------------------------------------------------------------- onboarding
await check("onboarding shows on first run", async () => {
  await page.getByText("Notes in your words.").waitFor({ timeout: 8000 });
});
await check("name step saves your name (Return continues)", async () => {
  await page.getByLabel("Your name, so transcripts can label you").fill("Akash QA");
  await page.keyboard.press("Enter");
  await page.getByText("Where should meetings go?").waitFor();
  const cfg = fs.readFileSync(path.join(marginHome, "config.toml"), "utf8");
  assert(cfg.includes('name = "Akash QA"'), "identity.name not written");
  assert(/\[summarization\][^[]*engine = "none"/s.test(cfg), "summarization.engine not explicit none");
  assert(/\[calendar\][^[]*enabled = false/s.test(cfg), "calendar not disabled");
  assert(!fs.existsSync(meetingsDir), "default folder created before it was chosen");
});
await check("setup step: Documents and Balanced preselected, saved, download starts", async () => {
  await page.getByRole("radio", { name: /Documents/ }).waitFor();
  assert((await page.getByRole("radio", { name: /Documents/ }).getAttribute("aria-checked")) === "true", "Documents not preselected");
  assert((await page.getByRole("radio", { name: /Balanced/ }).getAttribute("aria-checked")) === "true", "Balanced not preselected");
  assert(!fs.existsSync(meetingsDir), "default folder created before Continue");
  await button("Continue").click();
  await page.getByText("Let Margin hear the meeting.").waitFor();
  assert(fs.existsSync(meetingsDir), "meetings folder not created");
  const cfg = fs.readFileSync(path.join(marginHome, "config.toml"), "utf8");
  assert(cfg.includes(`output_dir = "${meetingsDir}"`), "output_dir not saved");
  assert(/\[transcription\][^[]*model = "small"/s.test(cfg), "model not saved as small");
  await page.getByText(/Speech models ready|Downloading speech models/).waitFor({ timeout: 10000 });
  assert((await page.evaluate(() => window.__E2E__.calls)).includes("download_models"), "download not started on setup");
});
await check("permissions step explains and never prompts", async () => {
  await page.getByText("System audio", { exact: true }).waitFor();
  await page.getByText(/Margin never records your screen/).waitFor();
  await page.keyboard.press("Enter");
  await page.getByText("Want summaries too?").waitFor();
});
await check("summaries step: none preselected and saved", async () => {
  assert((await page.getByRole("radio", { name: /No summaries/ }).getAttribute("aria-checked")) === "true", "No summaries not preselected");
  await button("Continue").click();
  await page.getByText("You're ready.").waitFor();
  const cfg = fs.readFileSync(path.join(marginHome, "config.toml"), "utf8");
  assert(/\[summarization\][^[]*engine = "none"/s.test(cfg), "summaries choice not saved");
});
await check("ready step shows how to record, then setup finishes", async () => {
  await page.getByText("From the menu bar.").waitFor();
  await page.getByText(/⌘⇧R/).first().waitFor();
  await page.getByText("Speech models ready").waitFor({ timeout: seed ? 15000 : 600000 });
  await button("Start using Margin").click();
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
  // While this file exists, the debug worker waits at "separating speakers", so the checks
  // below can look at a meeting that is open while core still processes it.
  const hold = path.join(marginHome, "e2e-hold-speakers");
  const drafts = path.join(marginHome, "drafts");
  const draftFiles = () => (fs.existsSync(drafts) ? fs.readdirSync(drafts) : []);
  let stoppedAt = 0;
  await check("typing right up to Stop keeps every word", async () => {
    fs.writeFileSync(hold, "");
    await page.keyboard.press("Enter");
    await page.keyboard.type("last words before stop");
    await page.getByRole("button", { name: "Stop" }).click();
    stoppedAt = Date.now();
    await page.getByText(/Transcribing|Queued|Telling speakers apart|Saving/).first().waitFor({ timeout: 15000 });
  });
  await check("right after Stop, the live transcript shows as a draft", async () => {
    await page.locator(".draft-transcript .live-line").nth(3).waitFor({ timeout: 3000 });
    const waited = Date.now() - stoppedAt;
    assert(await page.locator(".steps").isVisible(), "processing steps not shown next to the draft");
    await page.getByText("Draft from the live transcript. The final transcript replaces it.").waitFor();
    const first = await page.locator(".draft-transcript .live-line").first().innerText();
    assert(first.includes("launch plan"), `first draft line: ${first}`);
    const who = await page.locator(".draft-transcript .live-speaker").allInnerTexts();
    assert(who.includes("You") && who.includes("Them"), `draft speakers: ${who.join(",")}`);
    assert(draftFiles().some((f) => f.endsWith(".live.json")), `live draft not kept with the job: ${draftFiles().join(", ")}`);
    console.log(`     draft shown ${waited} ms after Stop`);
  });
  let liveFile = null;
  await check("the meeting opens while speakers are still being separated", async () => {
    await page.getByText("Finishing: separating speakers…").waitFor({ timeout: 180000 });
    assert((await page.getByLabel("Meeting title").inputValue()) === "QA live call", "did not land on the processed meeting");
    await page.locator(".turn").first().waitFor();
    const jobs = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke("list_jobs"));
    const job = jobs.find((j) => j.title === "QA live call");
    assert(job && job.state === "diarizing", `job is ${job?.state}, not still separating speakers`);
    assert(await page.getByLabel("Meeting title").isDisabled(), "title editable while core still writes the file");
    assert((await page.locator(".chip").count()) === 0, "speaker rename offered while core still writes the file");
    assert((await page.getByLabel("Your notes").getAttribute("readonly")) === null, "notes locked while finishing");
    const f = meetingFiles().find((x) => x.includes("qa-live-call"));
    assert(f, "no qa-live-call file yet");
    liveFile = path.join(meetingsDir, f);
    // The page and the transcript column must survive the update (checked below).
    await page.evaluate(() => {
      document.querySelector(".page-meeting").dataset.e2eSame = "1";
      const col = document.querySelector(".transcript-col");
      col.scrollTop = 40;
      window.__e2eScroll = col.scrollTop;
    });
  });
  await check("notes typed while it finishes go to the draft, not the file", async () => {
    const before = fs.readFileSync(liveFile, "utf8");
    await page.getByLabel("Your notes").click();
    await page.keyboard.press("Meta+ArrowDown");
    await page.keyboard.press("Enter"); // continues the list with "- "
    await page.keyboard.type("typed while finishing");
    await page.getByText("Saved", { exact: true }).waitFor({ timeout: 5000 });
    assert(fs.readFileSync(liveFile, "utf8") === before, "the meeting file was written while core still works on it");
    const draft = draftFiles().find((x) => x.endsWith(".md") && x !== "current.md");
    assert(draft && fs.readFileSync(path.join(drafts, draft), "utf8").endsWith("- typed while finishing"), `draft: ${draft}`);
    // Typed just before the job completes.
    await page.keyboard.type(" (just typed)");
  });
  let liveMd = null;
  await check("when the job completes, the open meeting updates in place", async () => {
    fs.rmSync(hold, { force: true });
    await page.locator(".finishing").waitFor({ state: "detached", timeout: 120000 });
    await page.locator(".chip").first().waitFor({ timeout: 5000 });
    assert(await page.evaluate(() => document.querySelector(".page-meeting")?.dataset.e2eSame === "1"), "the meeting view was rebuilt, not updated");
    const scroll = await page.evaluate(() => [document.querySelector(".transcript-col").scrollTop, window.__e2eScroll]);
    assert(scroll[0] === scroll[1], `transcript scroll moved: ${scroll}`);
    assert(!(await page.getByLabel("Meeting title").isDisabled()), "title still locked");
    assert((await page.getByLabel("Your notes").inputValue()).endsWith("- typed while finishing (just typed)"), "the latest edit was lost");
    await page.getByText("Saved", { exact: true }).waitFor({ timeout: 5000 });
    await page.waitForTimeout(300);
    liveMd = fs.readFileSync(liveFile, "utf8");
    const notes = liveMd.slice(liveMd.indexOf("## Notes"), liveMd.indexOf("## Transcript"));
    assert(notes === "## Notes\n\n- risk: download size\n- last words before stop\n- typed while finishing (just typed)\n\n", "notes not exact:\n" + notes);
    assert(draftFiles().every((f) => f === "current.md"), `drafts left: ${draftFiles().join(", ")}`);
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
  await check("rolling: pieces are transcribed during the recording and reused", async () => {
    const events = await bridgeEvents();
    const stopped = events.findIndex((e) => e.event === "recording-stopped");
    const before = events.slice(0, stopped).filter((e) => e.event === "rolling").map((e) => e.payload);
    const last = before.at(-1);
    assert(last && last.pieces >= 2 && !last.stopped, `progress before Stop: ${JSON.stringify(last)}`);
    // Core checks each piece against the final audio and settings before it reuses it.
    const log = fs.readFileSync(path.join(marginHome, "state/logs/margin.log"), "utf8");
    const used = [...log.matchAll(/checkpoint spans offered=(\d+) accepted=(\d+)/g)].map((m) => [+m[1], +m[2]]);
    const offered = used.reduce((n, [o]) => n + o, 0);
    const accepted = used.reduce((n, [, a]) => n + a, 0);
    assert(used.length === 2 && accepted === offered && accepted === last.pieces, `offered ${offered}, accepted ${accepted}, pieces ${last.pieces}`);
  });
  await check("rolling: the meeting matches a normal run", async () => {
    // The audio this meeting recorded (kept beside it, with its stems), processed after
    // Stop only, in a separate sandbox.
    const recorded = path.join(meetingsDir, meetingFiles().find((x) => x.includes("qa-live-call")).replace(/\.md$/, ".wav"));
    assert(fs.existsSync(recorded.replace(/\.wav$/, ".voice.wav")), "no recorded stems beside the meeting");
    const sb = path.join(base, "batch");
    fs.rmSync(sb, { recursive: true, force: true });
    fs.mkdirSync(path.join(sb, "home"), { recursive: true });
    fs.cpSync(seed, path.join(sb, "margin/state/models"), { recursive: true });
    const benv = { ...process.env, HOME: path.join(sb, "home"), MARGIN_HOME: path.join(sb, "margin") };
    execFileSync(bin, ["--dev", "process", recorded, "QA live call"], { env: benv, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
    const dir = path.join(sb, "home/Documents/Meetings");
    const batch = fs.readFileSync(path.join(dir, fs.readdirSync(dir).find((f) => f.endsWith(".md"))), "utf8");
    const words = (md) => md.slice(md.indexOf("## Transcript")).replace(/^\[[^\]]*\]/gm, "").toLowerCase().replace(/[^a-z0-9' ]+/g, " ").split(/\s+/).filter(Boolean);
    const [a, b] = [words(batch), words(liveMd)];
    let prev = [...Array(b.length + 1).keys()];
    for (let i = 1; i <= a.length; i++) {
      const cur = [i];
      for (let j = 1; j <= b.length; j++) cur[j] = Math.min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1));
      prev = cur;
    }
    const wer = prev[b.length] / Math.max(a.length, 1);
    fs.writeFileSync(path.join(out, "batch-call.md"), batch);
    console.log(`     rolling vs batch: ${b.length} / ${a.length} words, WER ${(wer * 100).toFixed(1)}%`);
    assert(a.length > 20 && wer <= 0.05, `WER ${(wer * 100).toFixed(1)}% over ${a.length} words`);
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
  await openSettings("Voices");
  await page.getByRole("list", { name: "Voices Margin recognises" }).getByText("Priya", { exact: true }).waitFor();
  await page.getByRole("button", { name: "Forget Priya" }).waitFor();
  await closeSettings();
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
    "## Summary", "", "- We planned the launch.", "",
    "> Agent input coverage: complete. 1/1 source chunks; 29 transcript bytes and 0 user-note bytes. All source chunks were processed in 1 model calls.", "",
    "## Action Items", "", "- [ ] @Priya: send the deck", "- [ ] book the room", "",
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
  // Core's delivery report after a summary is a debug record: hidden in the view, kept in the file.
  assert(!(await page.locator(".page-meeting").innerText()).includes("Agent input coverage"), "the coverage report shows");
  await page.getByRole("checkbox", { name: "send the deck" }).click();
  await page.waitForFunction(() => document.querySelector('[aria-label="send the deck"]')?.getAttribute("aria-checked") === "true");
  await page.waitForTimeout(600);
  const md = fs.readFileSync(fixture, "utf8");
  assert(/task: send the deck\n  due: Friday\n  status: done/.test(md), "frontmatter status not done");
  assert(md.includes("- [x] @Priya: send the deck") && md.includes("- [ ] book the room"), "body checkbox not ticked");
  assert(md.includes("> Agent input coverage: complete."), "the coverage report was dropped from the file");
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
    await openSettings("Summaries");
    const label = /Ollama, on this Mac/;
    await page.getByRole("radio", { name: label }).click();
    await page.waitForTimeout(500);
    await closeSettings();
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
  await check("a new meeting opens first, then gets its summary by itself", async () => {
    // The stand-in Ollama answers slowly now, so the order is visible: the meeting is ready
    // without a summary, shows that one is being written, then gets it (D40).
    const slow = path.join(out, "ollama-requests.log.slow");
    fs.writeFileSync(slow, "");
    try {
      dev("queue", audio, "QA auto summary");
      await page.locator(".row").filter({ hasText: "QA auto summary" }).first().waitFor({ timeout: 120000 });
      await page.locator(".row-job").waitFor({ state: "detached", timeout: 120000 });
      await page.locator(".row").filter({ hasText: "QA auto summary" }).first().click();
      const file = path.join(meetingsDir, meetingFiles().find((x) => x.includes("qa-auto-summary")));
      assert(!fs.readFileSync(file, "utf8").includes("## Summary"), "the summary was written before the meeting was ready");
      await page.locator(".writing-summary").waitFor({ timeout: 5000 });
      await page.screenshot({ path: path.join(out, "writing-summary.png") });
    } finally {
      fs.rmSync(slow, { force: true });
    }
    await page.locator(".eyebrow", { hasText: "Summary" }).waitFor({ timeout: 120000 });
    await page.locator(".writing-summary").waitFor({ state: "detached", timeout: 5000 });
    assert(fs.readFileSync(path.join(meetingsDir, meetingFiles().find((x) => x.includes("qa-auto-summary"))), "utf8").includes("## Summary"), "no summary in the file");
  });
}

// ---------------------------------------------------------------- settings parity
const cfgText = () => fs.readFileSync(path.join(marginHome, "config.toml"), "utf8");
const inSection = (section, line) => new RegExp(`^\\[${section}\\]\\n(?:[^\\[].*\\n)*?${line}$`, "m").test(cfgText());
const loginPlist = path.join(home, "Library/LaunchAgents/com.akashmishra.margin.login.plist");
const settled = () => page.waitForTimeout(700); // saves are queued; let the last one land
const toggle = (name) => page.getByRole("switch", { name });
const SECTIONS = ["General", "Recording", "Transcription", "Summaries", "Voices", "Permissions", "Integrations", "Updates", "About", "Start over"];
// One screenshot per section, plus one scrolled to the end when the section is taller than the window.
const shootSections = async (name) => {
  for (const label of SECTIONS) {
    await goSection(label);
    const slug = label.toLowerCase().replace(/ /g, "-");
    await page.screenshot({ path: path.join(out, `${name}-${slug}.png`), animations: "disabled" });
    const [h, vh] = await settingsPage().evaluate((el) => [el.scrollHeight, el.clientHeight]);
    if (h > vh + 4) {
      await settingsPage().evaluate((el) => el.scrollTo(0, el.scrollHeight));
      await page.screenshot({ path: path.join(out, `${name}-${slug}-end.png`), animations: "disabled" });
    }
  }
};
const current = async () => (await settingsNav().locator('[aria-current="page"]').textContent()).trim();
const focusedText = () => page.evaluate(() => document.activeElement?.textContent?.trim());
let openTitle = "";

await check("settings: a page with a section nav, opened with ⌘,", async () => {
  await page.locator(".row").filter({ hasText: "QA launch sync" }).first().click();
  await page.getByLabel("Meeting title").waitFor();
  openTitle = await page.getByLabel("Meeting title").inputValue();
  await page.keyboard.press("Meta+Comma");
  await settingsPage().waitFor();
  // It reopens the section used last (Summaries, if the summary checks ran), as System Settings does.
  assert((await current()) === (summaries === "none" ? "General" : "Summaries"), `current section: ${await current()}`);
  assert(!(await page.getByRole("navigation", { name: "Meetings" }).isVisible()), "meeting list still showing next to Settings");
  assert((await page.getByRole("button", { name: "Settings", exact: true }).getAttribute("aria-pressed")) === "true", "gear not shown as pressed");
  await goSection("Recording");
  assert((await current()) === "Recording", "nav did not follow the section");
  assert((await settingsNav().locator('[aria-current="page"]').count()) === 1, "more than one current section");
});
await check("settings: arrow keys move through the sections, Return opens one", async () => {
  await settingsNav().getByRole("button", { name: "Recording", exact: true }).focus();
  await page.keyboard.press("ArrowDown");
  assert((await focusedText()) === "Transcription", `ArrowDown moved focus to ${await focusedText()}`);
  assert((await current()) === "Recording", "ArrowDown opened a section");
  await page.keyboard.press("Enter");
  await settingsPage().getByRole("heading", { level: 1, name: "Transcription" }).waitFor();
  await page.keyboard.press("End");
  assert((await focusedText()) === "Start over", `End moved focus to ${await focusedText()}`);
});
await check("settings: Back returns to the meeting you had open", async () => {
  await closeSettings();
  assert((await page.getByLabel("Meeting title").inputValue()) === openTitle, "the meeting is not open after Back");
  assert(await page.locator(".row.selected").filter({ hasText: openTitle }).isVisible(), "sidebar selection lost");
  // ⌘, again opens the section you were on.
  await page.keyboard.press("Meta+Comma");
  await settingsPage().getByRole("heading", { level: 1, name: "Transcription" }).waitFor();
  await closeSettings();
});
await check("settings: Esc leaves, and a name typed just before is saved", async () => {
  await openSettings("General");
  await page.getByLabel("Name in transcripts").fill("Akash Escape");
  await page.keyboard.press("Escape");
  await settingsPage().waitFor({ state: "detached" });
  await settled();
  assert(fs.readFileSync(path.join(marginHome, "config.toml"), "utf8").includes('name = "Akash Escape"'), "name typed before Esc not saved");
  await openSettings("General");
  await page.getByLabel("Name in transcripts").fill("Akash QA");
  await page.keyboard.press("Enter");
  await settled();
  assert(fs.readFileSync(path.join(marginHome, "config.toml"), "utf8").includes('name = "Akash QA"'), "Return did not save the name");
  assert(await settingsPage().isVisible(), "Return left Settings");
  await closeSettings();
});
await check("a permission error links to Settings → Permissions", async () => {
  await page.evaluate(() => window.__E2E__.emit("error-toast", "Margin can't use the microphone. Allow it in System Settings → Privacy & Security → Microphone, then try again."));
  await page.locator(".toast").getByRole("button", { name: "Open Permissions" }).click();
  await settingsPage().getByRole("heading", { level: 1, name: "Permissions" }).waitFor();
  assert((await current()) === "Permissions", "nav not on Permissions");
  await settingsPage().getByText("Microphone", { exact: true }).waitFor();
  await settingsPage().getByText("System audio", { exact: true }).waitFor();
  await closeSettings();
});
await check("the menu bar's open-settings event opens Settings", async () => {
  await page.evaluate(() => window.__E2E__.emit("open-settings", null));
  await settingsPage().waitFor();
  assert((await current()) === "Permissions", `opened on ${await current()}, not the last section`);
  await closeSettings();
});
await check("settings: About shows the app and minutes-core versions", async () => {
  await openSettings("About");
  const text = await settingsPage().innerText();
  assert(/Version\s+\d+\.\d+\.\d+/.test(text), `no app version: ${text}`);
  assert(/minutes-core[\s\S]*\bv?\d+\.\d+\.\d+/.test(text), `no minutes-core version: ${text.replace(/\n/g, " | ")}`);
  await closeSettings();
});
await check("settings: new choices start at calm defaults", async () => {
  const value = (label) => page.getByLabel(label, { exact: true }).inputValue();
  await openSettings("General");
  assert((await value("Keep audio")) === "0", "audio is not kept forever");
  assert(!(await toggle(/Open Margin when you log in/).isChecked()), "open at login on");
  await goSection("Recording");
  assert((await value("Microphone")) === "", "microphone is not the system default");
  assert((await value("Stop after silence")) === "30", "silence limit is not 30 minutes");
  assert(await toggle(/Suggest recording when a call starts/).isChecked(), "call suggestions off");
  await goSection("Transcription");
  assert((await value("Language spoken in meetings")) === "auto", "language is not auto-detect");
  assert(await toggle(/Tell speakers apart/).isChecked(), "speaker separation off");
  assert(await toggle(/Transcribe during the meeting/).isChecked(), "rolling transcription off for Balanced");
  await shootSections("settings");
  await goSection("Transcription");
});
await check("settings: transcribing during the meeting can be turned off", async () => {
  const prefsFile = path.join(marginHome, "prefs.json");
  const rollingOff = () => JSON.parse(fs.readFileSync(prefsFile, "utf8")).no_rolling_transcription === true;
  await toggle(/Transcribe during the meeting/).uncheck();
  await settled();
  assert(rollingOff(), "switch off not saved");
  await closeSettings();
  await openSettings("Transcription");
  assert(!(await toggle(/Transcribe during the meeting/).isChecked()), "switch off not kept");
  await toggle(/Transcribe during the meeting/).check();
  await settled();
  assert(!rollingOff(), "switch on not saved");
});
await check("settings: language, microphone and silence limit are saved for core", async () => {
  await page.getByLabel("Language spoken in meetings").selectOption("en");
  await goSection("Recording");
  const mic = (await page.getByLabel("Microphone", { exact: true }).locator("option").allInnerTexts()).find((m) => m !== "System default");
  if (mic) await page.getByLabel("Microphone", { exact: true }).selectOption({ label: mic });
  await page.getByLabel("Stop after silence").selectOption("15");
  await settled();
  assert(inSection("transcription", 'language = "en"'), "transcription.language not written");
  assert(inSection("recording", "silence_auto_stop_secs = 900"), "silence limit not written");
  if (mic) assert(inSection("recording", `device = "${mic}"`), "microphone not written");
  await closeSettings();
  await openSettings("Transcription");
  assert((await page.getByLabel("Language spoken in meetings").inputValue()) === "en", "language not kept");
  await goSection("Recording");
  assert((await page.getByLabel("Stop after silence").inputValue()) === "15", "silence limit not kept");
  // Back to the default so nothing below depends on this Mac's hardware.
  await page.getByLabel("Microphone", { exact: true }).selectOption("");
  await settled();
  assert(!/^device = /m.test(cfgText()), "system default not restored");
});
await check("settings: names to spell right go to core's vocabulary", async () => {
  await goSection("Transcription");
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
  await goSection("General");
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
await check("settings: hiding from screen sharing is off, and the switch reaches the windows", async () => {
  // The bridge has no windows. It queues the request that the app sends to its windows.
  const asked = async () => (await bridgeEvents()).filter((e) => e.event === "__screen-share-refresh").map((e) => e.payload);
  await goSection("General");
  const sw = toggle(/Hide Margin from screen sharing/);
  assert(!(await sw.isChecked()), "switch is on by default");
  assert(inSection("privacy", "hide_from_screen_share = false"), "privacy.hide_from_screen_share not false by default");
  const before = (await asked()).length;
  await sw.check();
  await settled();
  assert(inSection("privacy", "hide_from_screen_share = true"), "privacy.hide_from_screen_share not true");
  await closeSettings();
  await openSettings("General");
  assert(await toggle(/Hide Margin from screen sharing/).isChecked(), "switch on not kept");
  await toggle(/Hide Margin from screen sharing/).uncheck();
  await settled();
  assert(inSection("privacy", "hide_from_screen_share = false"), "privacy.hide_from_screen_share not false again");
  const sent = (await asked()).slice(before);
  assert(JSON.stringify(sent) === "[true,false]", `windows not told: ${JSON.stringify(sent)}`);
});
await check("settings: call suggestions follow the switch", async () => {
  // A stand-in process named like Zoom's, which core's call detection looks for. (A
  // copy of /bin/sleep is killed by macOS; a symlink keeps its own name in `ps`.)
  const zoom = path.join(base, "bin/zoom.us");
  fs.mkdirSync(path.dirname(zoom), { recursive: true });
  fs.rmSync(zoom, { force: true });
  fs.symlinkSync("/bin/sleep", zoom);
  await goSection("Recording");
  await toggle(/Suggest recording when a call starts/).uncheck();
  await settled();
  assert(inSection("call_detection", "enabled = false"), "call_detection.enabled not false");
  await closeSettings();
  const fake = spawn(zoom, ["300"], { stdio: "ignore" });
  procs.push(fake);
  await page.waitForTimeout(17000); // the detector polls every 15 s
  assert((await page.locator(".call-banner").count()) === 0, "banner shown while switched off");
  await openSettings("Recording");
  await toggle(/Suggest recording when a call starts/).check();
  await settled();
  await closeSettings();
  await page.locator(".call-banner", { hasText: "Zoom looks active" }).waitFor({ timeout: 20000 });
  fake.kill();
  await page.locator(".call-banner").getByRole("button", { name: "Dismiss" }).click();
});
if (summaries === "fake-ollama") {
  await check("settings: summary language and instructions reach the summarizer", async () => {
    await openSettings("Summaries");
    await page.getByLabel("Summary language").selectOption("French");
    const box = page.getByLabel("Instructions for summaries");
    await box.fill("Start with decisions. Keep each point short.");
    await box.blur();
    await settled();
    assert(inSection("summarization", 'language = "French"'), "summary language not written");
    await closeSettings();
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
  await openSettings("Transcription");
  await toggle(/Tell speakers apart/).uncheck();
  await settled();
  assert(inSection("diarization", 'engine = "none"'), "diarization.engine not none");
  await closeSettings();
  dev("queue", audio, "QA no speakers");
  await page.locator(".row").filter({ hasText: "QA no speakers" }).first().waitFor({ timeout: 120000 });
  await page.locator(".row-job").waitFor({ state: "detached", timeout: 120000 });
  const md = fs.readFileSync(path.join(meetingsDir, meetingFiles().find((x) => x.includes("qa-no-speakers"))), "utf8");
  fs.writeFileSync(path.join(out, "no-speakers.md"), md);
  assert(md.includes("## Transcript"), "no transcript");
  assert(!/^\[(SPEAKER_\d+|Priya) \d+:\d\d\]/m.test(md), "transcript still has speaker labels");
  await openSettings("Transcription");
  await toggle(/Tell speakers apart/).check();
  await settled();
  assert(inSection("diarization", 'engine = "auto"'), "diarization not back on");
  await closeSettings();
});
await check("settings: keep audio for 7 days deletes only older audio", async () => {
  const recent = meetingFiles().find((x) => x.includes("qa-launch-sync"));
  const recentWav = path.join(meetingsDir, recent.replace(/\.md$/, ".wav"));
  const oldMd = path.join(meetingsDir, "2026-09-01-qa-old-meeting.md");
  const oldWav = oldMd.replace(/\.md$/, ".wav");
  const longAgo = new Date(Date.now() - 40 * 86400e3).toISOString();
  fs.writeFileSync(oldMd, fs.readFileSync(path.join(meetingsDir, recent), "utf8").replace(/^date: .*$/m, `date: ${longAgo}`).replace(/^title: .*$/m, "title: QA old meeting"));
  fs.copyFileSync(recentWav, oldWav);
  await openSettings("General");
  await page.getByLabel("Keep audio").selectOption("7");
  await settled();
  assert(inSection("retention", "auto_cleanup = true") && inSection("retention", "successful_audio_days = 7"), "retention not written");
  assert(!fs.existsSync(oldWav), "old audio kept");
  assert(fs.existsSync(oldMd), "old meeting file removed");
  assert(fs.existsSync(recentWav), "recent audio removed");
  await page.getByLabel("Keep audio").selectOption("0");
  await settled();
  assert(inSection("retention", "auto_cleanup = false"), "forever not restored");
  await closeSettings();
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
  // Closed, Ask is the bar at the bottom of the meeting; a click opens it.
  await page.locator(".ask-bar", { hasText: "Ask about this meeting" }).click();
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

// ---------------------------------------------------------------- narrow window (D51)
await check("a narrow window shows one column and the sidebar as a drawer", async () => {
  await page.setViewportSize({ width: 520, height: 760 });
  try {
    await page.locator(".sidebar").first().waitFor({ state: "hidden" });
    await page.getByRole("button", { name: "Show sidebar" }).click();
    await page.locator(".row").filter({ hasText: "QA follow-up" }).first().click();
    await page.locator(".drawer-scrim").waitFor({ state: "detached" });
    await page.getByLabel("Meeting title").waitFor();
    assert((await page.locator(".transcript-col").count()) === 0, "a transcript column in a narrow window");
    await page.getByRole("radio", { name: "Transcript" }).click();
    await page.locator(".turn").first().waitFor();
    assert(!(await page.getByLabel("Meeting title").isVisible()), "the notes still show with Transcript picked");
    await page.screenshot({ path: path.join(out, "narrow-transcript.png") });
    await page.getByRole("radio", { name: "Notes" }).click();
    await page.getByLabel("Meeting title").waitFor();
  } finally {
    await page.setViewportSize({ width: 1180, height: 760 });
  }
  // ⌘\ hides and shows the docked sidebar.
  await page.keyboard.press("Meta+\\");
  await page.locator(".sidebar").first().waitFor({ state: "hidden" });
  await page.keyboard.press("Meta+\\");
  await page.locator(".sidebar").first().waitFor();
});

// ---------------------------------------------------------------- settings: model switch
if (flag("--download")) {
  await check("switching to Fast downloads it with progress", async () => {
    await openSettings("Transcription");
    await page.getByRole("radio", { name: /Fast/ }).click();
    await page.getByText(/Downloading speech models/).waitFor({ timeout: 15000 });
    await page.screenshot({ path: path.join(out, "download-progress.png") });
    await page.getByText("Speech models ready").waitFor({ timeout: 600000 });
    assert(fs.existsSync(path.join(marginHome, "state/models/ggml-base.bin")), "base model missing");
    await closeSettings();
  });
}

// ---------------------------------------------------------------- sounds and shortcut prefs
await check("settings: sounds and the global shortcut are saved", async () => {
  await openSettings("Recording");
  await page.getByLabel(/Play a sound/).uncheck();
  await page.getByLabel(/from any app/).check();
  await page.waitForTimeout(400);
  const prefs = JSON.parse(fs.readFileSync(path.join(marginHome, "prefs.json"), "utf8"));
  assert(prefs.mute_cues === true && prefs.record_shortcut === true, JSON.stringify(prefs));
  await closeSettings();
  await openSettings("Recording");
  assert(!(await page.getByLabel(/Play a sound/).isChecked()) && (await page.getByLabel(/from any app/).isChecked()), "prefs not shown after reopening");
  await page.getByLabel(/Play a sound/).check();
  await page.getByLabel(/from any app/).uncheck();
  await page.waitForTimeout(400);
  await closeSettings();
});
await check("settings: automatic updates switch is saved", async () => {
  await openSettings("Updates");
  const sw = toggle(/Update automatically/);
  const was = await sw.isChecked();
  await sw.setChecked(!was);
  await page.waitForTimeout(400);
  const prefs = () => JSON.parse(fs.readFileSync(path.join(marginHome, "prefs.json"), "utf8"));
  assert(prefs().no_auto_update === was, `switch not saved: ${JSON.stringify(prefs())}`);
  await sw.setChecked(was);
  await page.waitForTimeout(400);
  await settingsPage().getByRole("button", { name: "Check now" }).waitFor();
  await closeSettings();
});
// ---------------------------------------------------------------- the recording chip (D53)
// The chip is its own page (chip.html) in its own window. Here it runs in a second page with
// the same bridge. The window itself (level, Spaces, size, place) needs the real app.
const openChip = async () => {
  const p = await ctx.newPage();
  p.on("pageerror", (e) => consoleErrors.push(`chip: ${e.message}`));
  p.on("console", (m) => m.type() === "error" && consoleErrors.push(`chip: ${m.text()}`));
  await p.addInitScript(`window.__E2E_BRIDGE__ = "http://127.0.0.1:${port.bridge}";`);
  await p.addInitScript({ path: path.join(root, "scripts/e2e/bridge-shim.js") });
  await p.setViewportSize({ width: 260, height: 52 });
  await p.goto(`http://localhost:${port.ui}/chip.html`);
  // The window is transparent. A backdrop makes the screenshots readable.
  await p.addStyleTag({ content: `html { background: ${flag("--dark") ? "#05070a" : "#c7d9e8"} }` });
  await p.locator(".chip").waitFor({ state: "attached" });
  return p;
};
const chipCalls = (p) => p.evaluate(() => window.__E2E__.calls);
const shootChip = (p, name) => p.screenshot({ path: path.join(out, `chip-${name}.png`) }).catch(() => {});
const latestSeq = async () => Number(await (await fetch(`http://127.0.0.1:${port.bridge}/latest`)).text());
const eventSince = async (name, since, timeout = 10000) => {
  const deadline = Date.now() + timeout;
  for (;;) {
    const found = (await bridgeEvents()).find((e) => e.seq > since && e.event === name);
    if (found) return found;
    assert(Date.now() < deadline, `no "${name}" event`);
    await page.waitForTimeout(200);
  }
};

await check("settings: the recording chip is on by default and is saved", async () => {
  await openSettings("Recording");
  const sw = toggle(/Show the recording chip/);
  assert(await sw.isChecked(), "the recording chip is not on by default");
  const prefs = () => JSON.parse(fs.readFileSync(path.join(marginHome, "prefs.json"), "utf8"));
  await sw.setChecked(false);
  await page.waitForTimeout(400);
  assert(prefs().no_recording_chip === true, `switch off not saved: ${JSON.stringify(prefs())}`);
  await sw.setChecked(true);
  await page.waitForTimeout(400);
  assert(prefs().no_recording_chip === false, `switch on not saved: ${JSON.stringify(prefs())}`);
  await closeSettings();
});
await check("chip: draws the time, the level and silence from the recorder events", async () => {
  const c = await openChip();
  assert(await c.locator(".chip.off").count(), "the chip shows with no recording");
  const emit = (event, payload) => c.evaluate(([e, p]) => window.__E2E__.emit(e, p), [event, payload]);
  await emit("recorder", { recording: true, stopping: false });
  await emit("chip-visible", true);
  await emit("recorder-tick", { elapsed_secs: 754, level: 30 });
  await c.locator(".chip:not(.off)").waitFor();
  assert((await c.locator(".time").innerText()) === "12:34", `time: ${await c.locator(".time").innerText()}`);
  await c.getByRole("button", { name: /^Margin is recording, 12 minutes\. Open notes$/ }).waitFor();
  const face = c.locator(".face");
  assert(Number(await c.locator(".chip").evaluate((el) => el.style.getPropertyValue("--lv"))) > 0.5, "no level halo");
  await c.waitForTimeout(500);
  await shootChip(c, "rest");
  // 21 s with no sound: the chip says so once, and the dot stays red.
  for (let t = 760; t <= 781; t += 3) await emit("recorder-tick", { elapsed_secs: t, level: 0 });
  await c.getByText("No sound comes in").waitFor();
  assert(await c.locator(".chip.silent .warn").isVisible(), "no mic mark");
  assert((await c.locator(".dot").evaluate((el) => getComputedStyle(el).backgroundColor)) !== "rgba(0, 0, 0, 0)", "the dot lost its red");
  await c.getByRole("status").getByText(/hears no sound/).waitFor();
  await c.waitForTimeout(300);
  await shootChip(c, "silent");
  await emit("recorder-tick", { elapsed_secs: 790, level: 0 });
  await c.locator(".chip.silent:not(.has-msg)").waitFor();
  await emit("recorder-tick", { elapsed_secs: 791, level: 12 });
  await c.locator(".chip:not(.silent)").waitFor();
  // VoiceOver hears the time once a minute, not each second.
  await c.getByRole("button", { name: /^Margin is recording, 13 minutes\. Open notes$/ }).waitFor();
  // Hover: Stop and Notes slide out. The window grows first.
  await face.hover();
  await c.locator(".chip.open").waitFor();
  await c.getByRole("button", { name: "Stop recording", exact: true }).waitFor();
  await c.getByRole("button", { name: "Open notes", exact: true }).waitFor();
  assert((await chipCalls(c)).includes("chip_width"), "the window was not resized for the controls");
  await c.waitForTimeout(300);
  await shootChip(c, "open");
  // Stop needs two clicks. The first arms it, and it disarms after 3 s.
  await c.getByRole("button", { name: "Stop recording", exact: true }).click();
  await c.getByRole("button", { name: "Click again to stop recording" }).waitFor();
  assert((await c.locator(".cbtn.stop").innerText()) === "Stop?", "the armed Stop does not say Stop?");
  await c.getByRole("status").getByText(/Click Stop again/).waitFor();
  await shootChip(c, "armed");
  await c.waitForTimeout(3300);
  await c.getByRole("button", { name: "Stop recording", exact: true }).waitFor({ timeout: 1000 });
  assert(!(await chipCalls(c)).includes("chip_stop"), "one click stopped the recording");
  // Leaving closes the controls.
  await c.mouse.move(2, 2);
  await c.locator(".chip:not(.open)").waitFor();
  // Keyboard: Tab opens the controls, arrows move the chip, Escape gives the focus back.
  await c.keyboard.press("Tab");
  await c.locator(".chip.open").waitFor();
  await c.keyboard.press("ArrowDown");
  await c.keyboard.press("Shift+ArrowUp");
  await c.keyboard.press("Escape");
  await c.locator(".chip:not(.open)").waitFor();
  const keys = await chipCalls(c);
  assert(keys.filter((x) => x === "chip_nudge").length === 2 && keys.includes("chip_release_focus"), `calls: ${keys.join(", ")}`);
  // A drag moves the window and saves the place. It is not a click.
  const box = await face.boundingBox();
  await c.mouse.move(box.x + 20, box.y + 10);
  await c.mouse.down();
  await c.mouse.move(box.x + 20, box.y + 30, { steps: 5 });
  assert(await c.locator(".chip.dragging").count(), "no drag state");
  await shootChip(c, "drag");
  await c.mouse.up();
  await c.waitForTimeout(200);
  const drag = await chipCalls(c);
  for (const cmd of ["chip_drag_start", "chip_drag", "chip_drag_end"]) assert(drag.includes(cmd), `${cmd} not called`);
  assert(!drag.includes("chip_open_notes"), "a drag opened the notes");
  // After Stop: red goes, and the chip says Saving until it slides out.
  await emit("recorder", { recording: true, stopping: true });
  await c.locator(".chip.saving").getByText("Saving", { exact: true }).waitFor();
  await c.getByRole("button", { name: "Saving the recording" }).waitFor();
  await c.waitForTimeout(300);
  await shootChip(c, "saving");
  await emit("chip-visible", false);
  await c.locator(".chip.off").waitFor();
  await c.close();
});
// The chip follows a real (simulated) recording.
if (simulate) {
await check("chip: Start hides the window, and the live view can shrink to the chip", async () => {
  const since = await latestSeq();
  await page.keyboard.press("Meta+Shift+I");
  await page.locator(".page-live").waitFor({ timeout: 10000 });
  await eventSince("shrink-to-chip", since);
  const again = await latestSeq();
  await page.getByRole("button", { name: "Shrink to the recording chip" }).click();
  await eventSince("shrink-to-chip", again);
});
let chip = null;
await check("chip: follows the recording, and Notes brings back the live notes", async () => {
  chip = await openChip();
  await chip.locator(".chip:not(.off)").waitFor({ timeout: 5000 });
  const t0 = await chip.locator(".time").innerText();
  await chip.waitForFunction((t) => document.querySelector(".time").textContent !== t, t0, { timeout: 5000 });
  await page.getByLabel("Meeting title").fill("QA chip");
  await page.getByLabel("Your notes").click();
  await page.keyboard.type("- typed before the chip");
  // Away from the live notes, as if the window had been used for something else.
  await page.locator(".row").filter({ hasText: "QA follow-up" }).first().click();
  await page.locator(".page-live").waitFor({ state: "detached" });
  const since = await latestSeq();
  await chip.locator(".face").hover();
  await chip.getByRole("button", { name: "Open notes", exact: true }).click();
  await eventSince("open-live", since);
  await page.locator(".page-live").waitFor({ timeout: 5000 });
  await page.waitForFunction(() => document.activeElement?.getAttribute("aria-label") === "Your notes", null, { timeout: 3000 });
  assert(await page.evaluate(() => document.activeElement.selectionStart === document.activeElement.value.length), "the caret is not at the end of the notes");
  await page.keyboard.type("\nthe last line"); // Return continues the list with "- "
  await page.waitForTimeout(800); // the live view saves the draft after 350 ms
});
await check("chip: Stop needs two clicks, says Saving, and keeps the notes", async () => {
  const stopBtn = () => chip.getByRole("button", { name: /stop recording/i });
  await chip.locator(".face").hover();
  await stopBtn().click();
  await chip.getByRole("button", { name: "Click again to stop recording" }).waitFor();
  const view = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke("recorder_view"));
  assert(view.recording && !view.stopping, "the first click stopped the recording");
  await stopBtn().click();
  await chip.locator(".chip.saving").waitFor({ timeout: 3000 });
  await page.getByText(/Transcribing|Queued|Telling speakers apart|Saving/).first().waitFor({ timeout: 15000 });
  const deadline = Date.now() + 180000;
  while (!meetingFiles().some((f) => f.includes("qa-chip")) || (await page.locator(".row-job").count()) > 0) {
    assert(Date.now() < deadline, "the meeting stopped from the chip did not finish");
    await page.waitForTimeout(500);
  }
  const md = fs.readFileSync(path.join(meetingsDir, meetingFiles().find((f) => f.includes("qa-chip"))), "utf8");
  assert(md.includes("- typed before the chip\n- the last line"), "notes typed before the chip Stop are missing");
  await chip.close();
});
}

if (simulate) {
  await check("a recording started from Settings takes over the window", async () => {
    await openSettings("Recording");
    await page.keyboard.press("Meta+Shift+I");
    await page.locator(".page-live").waitFor({ timeout: 10000 });
    await settingsPage().waitFor({ state: "detached" });
    await page.getByLabel("Meeting title").fill("QA from settings");
    await page.waitForTimeout(3000);
    await page.getByRole("button", { name: "Stop" }).click();
    await page.locator(".row").filter({ hasText: "QA from settings" }).first().waitFor({ timeout: 20000 });
    // Wait for the file and the end of the job. The job row can go before the file is
    // written, and a file written later changes the meeting count that reset checks.
    const deadline = Date.now() + 180000;
    while (!meetingFiles().some((f) => f.includes("qa-from-settings")) || (await page.locator(".row-job").count()) > 0) {
      assert(Date.now() < deadline, "the meeting recorded from Settings did not finish");
      await page.waitForTimeout(500);
    }
  });
}

// ---------------------------------------------------------------- reset
await check("a long meeting's transcript and notes scroll", async () => {
  // An 80-minute meeting: hundreds of turns. The columns must scroll, not grow past the window.
  const turns = Array.from({ length: 400 }, (_, i) => `[SPEAKER_${i % 3} ${Math.floor(i / 5)}:${String((i * 12) % 60).padStart(2, "0")}] Line ${i} of a long meeting about the bill of materials.`);
  const notes = Array.from({ length: 80 }, (_, i) => `- note ${i}`).join("\n");
  fs.writeFileSync(path.join(meetingsDir, "2026-11-05-qa-long-meeting.md"),
    `---\ntitle: QA long meeting\ntype: meeting\ndate: 2026-11-05T10:36:00+05:30\nduration: 80m 32s\n---\n\n## Notes\n\n${notes}\n\n## Transcript\n\n${turns.join("\n")}\n`);
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await page.locator(".row").filter({ hasText: "QA long meeting" }).first().click();
  await page.locator(".turn").nth(399).waitFor({ state: "attached", timeout: 8000 });
  for (const col of [".transcript-col", ".notes-col"]) {
    const [sh, ch, bottom, vh] = await page.locator(col).evaluate((el) => [el.scrollHeight, el.clientHeight, el.getBoundingClientRect().bottom, window.innerHeight]);
    assert(bottom <= vh + 0.5, `${col} ends at ${bottom}px, below the window (${vh}px)`);
    assert(sh > ch, `${col} does not overflow (${sh} vs ${ch})`);
    await page.locator(col).hover();
    await page.mouse.wheel(0, 2000);
    await page.waitForTimeout(300);
    assert((await page.locator(col).evaluate((el) => el.scrollTop)) > 0, `${col} did not scroll`);
  }
});

await check("reset keeps meetings and models, then runs setup again", async () => {
  const before = meetingFiles().length;
  await openSettings("Start over");
  await button("Reset Margin…").click();
  // Esc closes the dialog and keeps Settings open; focus goes back to the button.
  await page.keyboard.press("Escape");
  await page.getByRole("alertdialog").waitFor({ state: "detached" });
  assert(await settingsPage().isVisible(), "Esc in the Reset dialog left Settings");
  assert((await focusedText()) === "Reset Margin…", `focus on ${await focusedText()}, not Reset Margin…`);
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
  // Keyboard only: Return takes each step with its default.
  for (const heading of ["Where should meetings go?", "Let Margin hear the meeting.", "Want summaries too?", "You're ready."]) {
    await page.keyboard.press("Enter");
    await page.getByText(heading).waitFor({ timeout: 5000 });
  }
  await page.keyboard.press("Enter");
  await page.locator(".row").filter({ hasText: "QA launch sync" }).first().waitFor({ timeout: 10000 });
});
await check("deleting meetings requires typing delete", async () => {
  await openSettings("Start over");
  await button("Reset Margin…").click();
  await page.getByText("Keep my meetings").click();
  const go = page.getByRole("alertdialog").getByRole("button", { name: "Reset Margin" });
  assert(await go.isDisabled(), "reset enabled without typing delete");
  await page.getByPlaceholder("delete").fill("delete");
  assert(!(await go.isDisabled()), "reset still disabled after typing delete");
  await page.getByRole("alertdialog").getByRole("button", { name: "Cancel" }).click();
  await closeSettings();
});

await check("no UI errors or failed commands", async () => {
  const errs = await page.evaluate(() => window.__E2E__.errors);
  const real = errs.filter((e) => !e.startsWith("request_permission"));
  assert(real.length === 0 && consoleErrors.length === 0, [...real, ...consoleErrors].join(" | "));
});

// How often each command ran: a check on redundant refreshes (docs/perf.md).
const calls = await page.evaluate(() => window.__E2E__.calls).catch(() => []);
const tally = Object.fromEntries(Object.entries(calls.reduce((n, c) => ({ ...n, [c]: (n[c] || 0) + 1 }), {})).sort((a, b) => b[1] - a[1]));
console.log(`commands: ${calls.length} (${Object.entries(tally).slice(0, 6).map(([k, v]) => `${k} ${v}`).join(", ")})`);
fs.writeFileSync(path.join(out, "report.json"), JSON.stringify({ results, calls: tally }, null, 2));
await browser.close();
for (const p of procs) p.kill();
const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length}/${results.length} passed. Screenshots in ${out}`);
process.exit(failed.length ? 1 : 0);
