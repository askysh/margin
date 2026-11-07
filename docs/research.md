# RESEARCH — building a Granola-style shell on `minutes-core`

Upstream: `silverstein/minutes` at tag **v0.32.1** (commit `b92ef348`, 2026-11-05).
Surveyed from the reference clone at `~/dev/minutes-upstream`. The detailed notes, with
file:line references and signatures, are in [research/](research/):

| File | Covers |
|---|---|
| [survey-recording.md](research/survey-recording.md) | Start/stop, system audio, live transcript, permissions, call detection |
| [survey-storage.md](research/survey-storage.md) | Diarization, speaker rename, voice profiles, Markdown format, search, notes, vault |
| [survey-config.md](research/survey-config.md) | Config, state-path inventory, model downloads, summaries, MCP, CLI JSON |
| [survey-build.md](research/survey-build.md) | Native deps, build flags, the Tauri app's packaging, release workflow, cask |

## TL;DR

1. **The core is a library we can call in-process.** Recording, live transcription,
   post-processing, diarization, search and config are all `pub` and take a `&Config`.
   Upstream's own Tauri app calls the same functions. We do not need to bundle the CLI.
2. **It builds as a git dependency, with one trap.** Downstream crates do not see
   upstream's `Cargo.lock`, so cargo resolves `ort` 2.0.0-rc.15 and `pyannote-rs` does not
   compile. Fix: pin `ort = "=2.0.0-rc.10"` and seed our `Cargo.lock` from upstream's lock
   at the same tag. With that, a cold debug build of `minutes-core`
   (`whisper,streaming,diarize,metal`) takes about 95 s on this M-series, 10-core Mac.
3. **Upstream Minutes.app is installed and running on this Mac.** It owns `~/.minutes` and
   `~/.config/minutes`. We isolate with `MINUTES_DATA_DIR` and `MINUTES_CONFIG_PATH`.
   That makes Reset safe: it can never break the other app. One leak remains: `voices.db`
   is hardcoded to `~/.minutes`. This is upstream patch #1.
4. **System audio is in core** as a Core Audio process tap (`cidre`, macOS 14.4+). It needs
   the *System Audio Recording* TCC permission, not Screen Recording. Core cannot check or
   request that permission. If it is denied, the tap silently records silence.
5. **Live transcript has no speaker labels upstream.** Live lines always have
   `speaker: null`. Speakers appear only after post-processing. In v1 we get
   "You / Them" live labels in call mode with a host-side heuristic. See §1.3.
6. **Speaker renames in upstream are fragile.** They are overlay rows bound to the file's
   sha256, and the Markdown is never rewritten. We rewrite the Markdown ourselves (so
   Obsidian sees names) and also save a voice sample, so the voice is recognised next time.
7. **No reset exists anywhere upstream.** We build it, against a complete inventory of
   paths (§1.8).

---

## 1. Survey of `minutes-core` / `minutes-cli`

### 1.1 Crates, features, native deps

- `minutes-core` is on crates.io (0.32.1). The crates.io tarball leaves out
  `scripts/calendar-events.swift`, so the calendar helper is disabled in that build.
  → Use a **git dependency pinned to a tag**. Workspace inheritance and the in-repo path
  dependency (`whisper-guard`) resolve fine from git.
- Features: `default = ["whisper"]`. We need `whisper` + `streaming` (live transcript and
  the system-audio tap) + `diarize` (pyannote-rs + ort) + `metal`.
  - `parakeet` is dormant: it resolves to whisper.
  - `engine-sherpa` needs a separately built, dlopened plugin with three hand-signed
    dylibs. Skip it in v1.
  - `streaming-diarize` has no production callers.
- Native build requirements:
  - **Xcode 26 SDK + swiftc.** `crates/core/build.rs` always compiles a static Swift
    bridge, and the calendar helper uses macOS 26 Speech APIs.
  - **cmake.** whisper.cpp and knf-rs-sys need it. It was not installed here; I installed
    it with `brew install cmake`.
  - **libclang**, for bindgen. It comes with Xcode.
  - **ort-sys downloads ONNX Runtime 1.22 at build time** from `cdn.pyke.io` and links it
    statically, so there is no dylib to bundle.
  - Required environment:
    - `CXXFLAGS=-I$(xcrun --show-sdk-path)/usr/include/c++/v1` (macOS 26+)
    - `MACOSX_DEPLOYMENT_TARGET=14.0`
  - Our `build.rs` must add `-Wl,-rpath,/usr/lib/swift` to our binary. Core's link-arg
    does not reach downstream binaries.
- `main()` should first call `graph_worker::maybe_run_policy_projection_worker()` and
  `audio_decode_worker::maybe_run_audio_decode_worker()`. Core re-executes the current
  binary for these worker roles.
- Upstream's XPC services (graph worker, Apple Speech) are tied to upstream's Apple team
  ID. Without them core falls back to in-process work, so we don't ship them.

### 1.2 Recording (mic + system audio, no bot)

- `capture::record_to_wav_with_lifecycle(path, stop: Arc<AtomicBool>, &Config, ctx)` records
  **in the caller's process and blocks**. We run it on a `std::thread` and set the flag to
  stop.
- `pid::create()` / `pid::remove()` hold a cross-process lock at
  `<data>/recording.pid`. `minutes stop` writes a stop-sentinel file; it sends no signal.
- Before recording, call `capture::preflight_recording(mode, intent, allow_degraded, &cfg)`.
- After recording:
  1. `jobs::queue_live_capture_with_recording_health(...)` queues the job.
  2. `jobs::process_pending_jobs(&cfg, cb)` processes it: transcribe, diarize, summarize,
     write the Markdown.
  Both upstream frontends run step 2 in a **child process**. Our app will re-exec itself
  with `--process-queue-worker`.
- **Mic vs call** depends on config, not on a flag:
  - Call: `recording.capture_backend = "core-audio-tap"` and
    `recording.sources = { call = "auto" }`.
  - Mic only: no `sources`.
  `RecordingIntent::{Memo,Room,Call}` changes only preflight warnings.
- In dual-source mode core writes `<base>.wav` (mixed) plus `.voice.wav` and `.system.wav`
  (16 kHz mono). The pipeline uses the split files for speaker attribution.
- Permissions (`macos_permissions`): `microphone_status()`, `request_microphone_access()`,
  `screen_recording_status()` (check only). There is nothing for System Audio Recording.
  We trigger that prompt with a short tap probe and deep-link to System Settings.
- The ScreenCaptureKit system-audio helper exists only in upstream's Tauri crate (Swift,
  macOS 15). We don't need it.

### 1.3 Live transcript

- A whisper thread inside the recorder appends finished utterances to
  `<data>/live-transcript.jsonl`. The file is truncated per session, and this is on by
  default (`live_transcript.recording_sidecar`).
- Line shape: `{line, ts, offset_ms, duration_ms, text, speaker:null}`.
- To read it: `live_transcript::read_since_line(n)` and `session_status()`. We poll every
  ~700 ms and emit Tauri events.
- In-progress drafts (partials) go only to a private relay socket. v1 shows final lines.
- **Speaker turns live:** none upstream. The live thread hears the mixed signal.
  - **v1 heuristic:** in call mode, use `offset_ms`/`duration_ms` to read the matching
    window from `.voice.wav` and `.system.wav`, which are still growing during the
    recording, and compare RMS. Louder mic → "You"; louder system → "Them".
  - In-person, mic-only meetings get no live labels. Proper speaker labels appear after
    processing.

### 1.4 Diarization, speakers, voice memory

- `diarize::diarize_with_context` runs inside `process_pending_jobs`. It needs two ONNX
  files in `<data>/models/diarization`: `segmentation-3.0.onnx` and
  `wespeaker_en_voxceleb_CAM++.onnx`. Their URLs are public constants in `diarize.rs`.
- Transcript lines look like `[SPEAKER_0 0:00] text`, or `[Alice 0:22]` when the pipeline
  was confident. `speaker_map` in the frontmatter records any mapping.
- Upstream rename: `overlays::write_speaker_confirmation(meeting, label, name, ..)` writes a
  row in `overlays.db`, bound to the file's sha256.
  - It is lost on any later edit, and the Markdown never changes.
  - The CLI refuses labels that aren't already in `speaker_map`.
- **Our rename plan:**
  1. Rewrite the transcript labels and `speaker_map` in the file, with an atomic write.
  2. Then write the overlay confirmation, so its hash matches the new bytes.
  3. Then save a voice sample:
     - Take the centroid for that label from `voice::load_meeting_embeddings(meeting)`.
     - Save it with `voice::insert_voice_sample(.., TrustClass::ManuallyConfirmed,
       model_id = voice::model_version(&cfg))`.
     Later meetings match against all stored profiles, so the person is recognised.
- `voices.db` path is hardcoded to `~/.minutes/voices.db` (shared with upstream app).

### 1.5 Storage format

- `output_dir/` holds `YYYY-MM-DD-<slug>.md` plus sibling `.wav` files. It also has
  `memos/`, `archive/`, `failed-captures/` and `daily/`, and dot-dirs are ignored. Files
  are mode 0600.
- The frontmatter is YAML. The fields we use are title, type, date, duration, attendees,
  speaker_map, tags and status. The full table is in [survey-storage.md](research/survey-storage.md).
  - **Unknown keys are dropped** whenever core re-writes the YAML.
  - `schema/meeting.schema.json` is out of date; `minutes schema` gives the current one.
- Body order: `## Summary`, Decisions / Action Items / Open Questions, `## Notes`, then
  `## Transcript` last.
- Obsidian reads these files directly. Nothing proprietary is needed.
- **There is no persistent SQLite index any more.** `search::search(q, &cfg, &filters)`
  builds a per-query in-memory FTS5 index. It can fail under a concurrent search, so retry.
  For the library list we parse frontmatter ourselves (fast, and nothing gets locked).
- Notes:
  - During recording: `notes::add_note(text)` (timestamped bullets).
  - After recording: `notes::annotate_meeting(path, text)` (append only). There is no
    "replace notes" API.
  - **Our editor** replaces the `## Notes` body itself. It keeps the frontmatter bytes,
    keeps exactly one of each heading and keeps `## Transcript` last. It writes atomically
    and keeps the file mode. Afterwards it calls `derived::refresh_derived_views`.
- Vault: `[vault] enabled, path, meetings_subdir, strategy = auto|symlink|copy|direct`.
  This is what made upstream's folder setup painful. **We sidestep it:** the user picks
  the output folder directly. It can be inside an Obsidian vault or iCloud Drive, which is
  `strategy = direct` in effect, with no symlinks or copies.

### 1.6 Config

- TOML at `$MINUTES_CONFIG_PATH`, else `~/.config/minutes/config.toml`. Every section is
  `#[serde(default)]`.
- Loading and saving:
  - `load_strict_from(&Path)` returns an error instead of hiding one.
  - `save_to(&Path)` merges into the existing file, keeps comments and is atomic.
- Most APIs take a `&Config` value. A few modules re-read the global file (calendar,
  diarize, knowledge), which is fine once `MINUTES_CONFIG_PATH` points at our file.
- Gotchas:
  - If `summarization.engine` is missing, load forces it to `auto`, which silently runs
    `claude -p` when that is installed. **Always write `engine` explicitly.**
  - `output_dir` is not tilde-expanded. Write absolute paths.

### 1.7 Models, summaries, MCP

- Models:
  - Whisper ggml files come from `huggingface.co/ggerganov/whisper.cpp`, checked by size
    only. `base` is 148 MB and `small` is 488 MB.
  - Silero VAD is required by the default `vad_model`.
  - The two diarization ONNX files.
  - **No core function downloads these with progress**; the CLI's downloader is private.
    We write a small downloader (ureq streaming, progress events, temp-then-rename) using
    the URLs core exposes. Upstream patch #2 candidate.
- Summaries (`summarize.rs`): engines `none | agent (claude/codex CLI on stdin) | ollama |
  claude (API key) | openai | …`.
  - A summary is always optional; a failure yields `summary: None` and the meeting is
    still written.
  - To summarize later: `resummarize::resummarize_meeting(path, &cfg, &opts)`.
  - We expose three choices: **None (default) / Claude CLI / Ollama**.
- MCP (`crates/mcp`, TypeScript) reads Markdown from `MEETINGS_DIR`. It keeps working
  against our folder if the user sets `MEETINGS_DIR` to our output folder. Settings shows
  a copy-paste snippet.
- CLI JSON: `status`, `list`, `search`, `get --json`, `health --json`, `transcript`. Useful
  for debugging, but we call core directly.

### 1.8 State inventory (what Reset must handle)

| Path (our isolated layout) | What | Reset |
|---|---|---|
| `~/Library/Application Support/Margin/config.toml` | core config + our app prefs | always delete |
| `~/Library/Application Support/Margin/state/` (`MINUTES_DATA_DIR`) | pid/sentinel files, live transcript, jobs, logs, overlays.db, native captures | delete (after stopping any recording or worker) |
| `…/state/models/` | whisper, VAD and diarization models (≈200–700 MB) | keep by default; checkbox to delete |
| `<output folder>/*.md, *.wav, memos/…` | the user's meetings | keep by default; delete only with a typed confirmation |
| `~/.minutes/voices.db` | voice profiles (shared with upstream app; hardcoded) | keep by default; checkbox "forget voices" |
| `~/.config/minutes`, `~/.minutes` (upstream app) | not ours | **never touched** |

---

## 2. Shell decision: Tauri v2 + React (TypeScript, Vite)

**Pick: Tauri v2 with a React frontend.** The core is a large Rust library with no C ABI.
It uses blocking threads, callbacks and many structs. A SwiftUI shell would need either a
uniffi/cbindgen bridge crate covering every call we use, or shelling out to the CLI.
- The bridge crate is basically the same Rust layer we'd write as Tauri commands anyway,
  plus Swift bindings to maintain on every upstream bump.
- The CLI has no JSON progress for `setup`, no live-transcript push, and owns the recording
  process lifecycle.

Tauri lets the Rust side call `minutes-core` in-process, exactly as upstream's app proves
works. All polish work happens in a web UI we can iterate on quickly. The cost is a less
native feel: no real NSTextView, and WebKit scrolling. We mitigate with vibrancy, the macOS
overlay title bar, SF system fonts and careful typography.

SwiftUI would win on native polish, and is worth revisiting for v2 if the Rust surface we
use settles into a small, stable API. React was picked over Svelte only for ecosystem
depth: editor and virtualized lists.

---

## 3. v1 scope and UI sketch (Granola-modelled)

### Scope

**In:**
- Onboarding: welcome → folder → models (progress) → permissions (mic, system audio) →
  done.
- Record: a "Call" (mic + system audio) or "In person" (mic only) meeting. Start and stop
  from the main window and from the menu-bar tray. A timer and a level meter show while
  recording.
- A notepad-first meeting view:
  - During the meeting you type notes. They save live to the `## Notes` section of the
    meeting file once it exists. Before that, they are buffered and passed to the job.
  - A live transcript side panel shows "You / Them" in calls.
  - After processing: summary, speaker-labelled transcript, and click-to-rename on
    speakers (rename everywhere and remember the voice).
- Library: a sidebar grouped by Today / Yesterday / This week / Earlier, with search
  across title, notes and transcript. Clicking opens the meeting. "Show in Finder" and
  "Open in Obsidian" are available.
- Settings: output folder, model (base / small), summary engine (None / Claude CLI /
  Ollama), permissions status, and a **Reset…** button with a clear dialog. The dialog has
  "Keep my meetings" (default on), "Keep downloaded models" (default on) and "Forget saved
  voices" (default off). Reset then re-runs onboarding.

**Out (v1):** cloud sync, calendar UI, templates, the dictation, copilot and voice-live
features, Windows/Linux, signing, auto-update, live partials.

**Nice-to-have if cheap:** a "Zoom/Meet/Teams call detected — record?" prompt using
`capture::detect_active_call_app`.

### Main window

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ ● ● ●                                                   [● Record ▾] [⚙]     │
├────────────────────┬─────────────────────────────────────────┬───────────────┤
│ 🔍 Search          │  Weekly sync with Priya                 │ LIVE          │
│                    │  Thu 6 Nov · 10:02 · 00:14:32 ● REC     │ TRANSCRIPT    │
│ TODAY              │  ───────────────────────────────────    │               │
│ ▸ Weekly sync  ●   │  - ship onboarding by Fri               │ You  0:12     │
│   Design review    │  - Priya owns pricing page              │ so the plan…  │
│ YESTERDAY          │  - risk: model download size            │ Them 0:20     │
│   1:1 Akash/Sam    │  |                                      │ right, and…   │
│ THIS WEEK          │                                         │               │
│   Customer call    │                                         │  ▁▃▅▂ level   │
│                    │                                         │               │
│                    │                                         │ [■ Stop]      │
└────────────────────┴─────────────────────────────────────────┴───────────────┘
```

After processing, the meeting page has these parts:
- **Summary**, rendered from the file.
- **Your notes**, editable.
- **Transcript**, which you can collapse.

Speaker chips such as `[SPEAKER_1 ▾]` open a "Rename…" popover with a "Remember this
voice" box, checked by default.

### Menu bar

```
 ◉  ▾
 ├ Start call recording        ⌘⇧R
 ├ Start in-person recording
 ├ ─────────
 ├ Stop recording (00:14:32)
 ├ Open Margin
 └ Quit
```

### Settings

```
 General   Output folder  ~/Documents/Meetings   [Change…] [Show in Finder]
           Transcription  ( ) Fast – base  (•) Accurate – small
 Summaries (•) None  ( ) Claude CLI (found at /opt/homebrew/bin/claude)  ( ) Ollama
 Permissions  Microphone ✓   System audio ⚠ [Open System Settings]
 ─────────────────────────────────────────────
 Reset Margin…   Wipes settings and app state, then restarts setup.
                 Your meetings are kept unless you choose otherwise.
```

## 4. Risks noted for Phase 2

- **The System Audio Recording permission cannot be queried.** We must explain it and
  verify it with a probe: a short tap recording, then check its RMS while something is
  playing.
- **Build size and time.** Release builds with LTO will take several minutes. The .app
  will be large, mostly from statically linked ORT.
- **Upstream moves daily.** We pin the tag and keep a `scripts/bump-upstream.sh` that
  re-seeds `Cargo.lock` from the new tag.
- **The heuristic live "You / Them" labels can be wrong** with echo or speakerphone, so
  they are labelled as approximate in the UI.
