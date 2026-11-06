# Decisions

Product and technical decisions made without asking. Each one lists the options and the pick.

## D1. Product name: "Margin"
- Options: reuse "Minutes" (it confuses the two apps, and both are installed side by side),
  "Margin", "Notch", "Scribe".
- Pick: **Margin**, for notes in the margin of a meeting. Bundle ID
  `com.akashmishra.margin`, following benchbar's `com.akashmishra.benchbar`. It is easy to
  rename before the first release.

## D2. Shell: Tauri v2 + React + TypeScript (Vite)
- See RESEARCH.md §2.

## D3. Isolate our state from the upstream Minutes.app
- Context: upstream Minutes.app is installed and running on this Mac. It owns `~/.minutes`
  and `~/.config/minutes`.
- Options:
  - (a) Share them. Both apps then fight over `recording.pid`, a `minutes stop` from either
    one stops both, and our Reset would wipe its config.
  - (b) Isolate with `MINUTES_CONFIG_PATH` and `MINUTES_DATA_DIR`.
- Pick: **(b)**. Config is `~/Library/Application Support/Margin/config.toml`, and state
  goes in `~/Library/Application Support/Margin/state/`.
- Exception: `voices.db` is hardcoded to `~/.minutes`, so it is shared.
  See UPSTREAM-PATCHES.md #1.
- Cost: MCP and the `minutes` CLI do not see our state by default. Settings shows the
  `MEETINGS_DIR` snippet that makes MCP read our meetings.

## D4. Output folder is written to directly (no vault symlink/copy)
- Default `~/Documents/Meetings`. The user can pick any folder, such as a subfolder of an
  Obsidian vault or of iCloud Drive.
- We leave `[vault]` disabled. That removes upstream's symlink/copy setup, which is the
  file-management pain point.

## D5. System audio via Core Audio process tap (not ScreenCaptureKit)
- The core tap needs macOS **14.4+** and the "System Audio Recording" permission, not
  Screen Recording.
- The brief says to request screen recording, but the tap does not need it. We ask only
  for what we use, and say why.

## D6. Summaries default to None
- We always write `summarization.engine` explicitly, because a missing key means `auto`,
  which silently runs `claude -p`.
- The choices are None, Claude CLI (`agent` engine with the claude command) and Ollama.

## D7. Speaker rename rewrites the Markdown
- Upstream stores renames only as overlay rows bound to the file's hash, which any later
  edit invalidates.
- We rewrite the transcript labels and `speaker_map` in the file so Obsidian shows names.
  Then we write the overlay row against the new bytes, and save a voice sample so the
  person is recognised next time.

## D8. Live "You / Them" labels are a host-side heuristic
- Upstream live lines have no speaker.
- In call mode we compare the RMS of the `.voice.wav` and `.system.wav` windows for each
  line. The UI shows these labels as approximate. Proper diarized labels replace them
  after processing.

## D9. Calendar lookup is off in v1
- minutes-core matches a recording to the calendar event at the same time, through an
  EventKit helper. It runs inside the background processing worker, so it would raise a
  Calendars permission prompt in the middle of processing, with no explanation in the UI.
- Pick: `calendar.enabled = false` in new configs. Info.plist still carries
  `NSCalendarsFullAccessUsageDescription`, so a later opt-in (a Settings toggle with its
  own permission step) only needs UI.
- Call-app detection (a "Zoom looks active" banner) ships, since it needs no permission.

## D10. Delete means "move to the Trash"
- Deleting a meeting and resetting with "delete meetings" both move files to the Trash.
  Reset moves only files Margin recognises as meetings (plus `archive/` and
  `failed-captures/`), never the whole folder, because the folder may be inside an
  Obsidian vault.

## D11. QA runs the real UI against the real backend, never against your data
- Tauri has no WebDriver on macOS. So `margin --dev serve` exposes the same command
  functions over a localhost HTTP bridge, and `scripts/e2e/run.mjs` drives the built UI
  in headless WebKit through a small IPC shim.
- The bridge refuses permission prompts. Tests run with a sandbox `HOME` and
  `MARGIN_HOME`, so test voices never reach `~/.minutes/voices.db`.

## D12. Capture simulation exists only in debug builds
- `MARGIN_SIMULATE_CAPTURE=<wav>` replaces the microphone with a file, plus optional
  stems and a live-transcript script. That lets record → live transcript → stop →
  process be tested without TCC.
- It is compiled into debug builds only (`cfg!(debug_assertions)`). Release builds
  ignore the variable.

## Perf and onboarding (to renumber)

### P1. Measure first, keep only what the numbers support
- The numbers are in `docs/perf.md`, with the scripts that produced them.
- Kept: the jobs-watcher change, the library cache, faster search, lazy views and
  fewer re-renders. Each one has a before and after figure.

### P2. The jobs watcher reads the archive only when something changed
- Context: minutes-core never prunes `jobs/archive/`, so it has one file per processed
  meeting. The watcher read and parsed all of them every second. With 600 meetings the
  idle app used 2.1% CPU.
- Options:
  - (a) Poll less often. The UI then notices a finished meeting later.
  - (b) Prune the archive. That changes core's data, which the CLI also reads.
  - (c) Each second, read only the active job files and the timestamps of the archive and
    drafts folders. Do the full listing only when one of them changes.
- Pick: **(c)**. Core writes job files by rename, so every change moves a folder
  timestamp. `list_jobs` still runs every second, so a job whose worker died is still
  re-queued.

### P3. The library list is cached by file size and modification time
- The cache lives in memory and is rebuilt on each list. Entries for files that are gone
  drop out. A change that keeps both the size and the modification time is missed. That
  needs a same-size write in the same nanosecond, and Margin and core write by rename.

### P4. Onboarding: five short steps, every one with a default
- Steps: You → Setup (folder and model together) → Permissions → Summaries → Ready.
- Return continues on every step. A user can finish with Return alone.
- Folder and model share a page. Both have good defaults (Documents, Balanced), and the
  download starts one step earlier.
- The download runs in the background. Nothing waits for it. The Ready step and Home show
  its progress, and recording starts working when it finishes. Before, the last step
  blocked until the download finished.
- The Ready step shows the three ways to start a recording: the window button, the menu
  bar and the shortcuts.
- Summaries still default to None (D6).

### P5. No microphone level check during onboarding
- A level meter must open the microphone. That shows the macOS prompt if access is not
  decided yet. If access is already granted, it records real audio in a test run, which
  the QA rules forbid.
- Pick: **no meter**. The live view's rule already shows the level during the first
  recording. Add a meter later, shown only after `microphone == granted`.

### P6. Release profile unchanged
- See `docs/perf.md`. The size and speed gain of `codegen-units = 1` and fat LTO is small
  next to the cost in build time, because most of the binary is whisper.cpp and ONNX
  Runtime, which these settings do not touch.
