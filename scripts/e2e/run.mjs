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
}

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
