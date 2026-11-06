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

## Chat (to renumber)

### C1. Ask uses the user's own `claude` CLI, headless
- Options: (a) the Anthropic API with a key in Settings, (b) the `claude` CLI that
  Margin already uses for summaries, (c) Ollama.
- Pick: **(b)**. No key to store, and the user's own login and plan apply. Margin
  uses the binary chosen in Settings → Summaries, or else the one it finds.
- Ollama is not in v1 (docs/parity/chat.md).

### C2. Claude can read the meetings folder, and nothing else
- Options: (a) no tools, with context packed into the prompt (upstream), (b) read-only
  tools in the meetings folder.
- Pick: **(b)**. The working directory is the meetings folder, and the flags are
  `--tools Read,Grep,Glob --permission-mode dontAsk`. `dontAsk` denies every action that
  needs a permission prompt, and that includes reading outside the working folder.
  `--safe-mode`, `--setting-sources ""` and an empty `--strict-mcp-config` stop the
  user's CLAUDE.md, hooks, plugins and MCP servers from loading.
- Margin does not send `--allowedTools`: a bare `Read` rule would also approve reads
  outside the folder.
- If `claude --help` does not show `--tools`, `dontAsk` and `--resume`, Ask refuses to
  run and tells the user to update Claude Code.

### C3. One conversation per meeting, plus one across all meetings
- A meeting chat puts the whole meeting file in its first prompt. The chat across all
  meetings puts a list of meetings (date, title, file) in its first prompt, and Claude
  uses Grep and Read.
- Follow-ups use `--resume` with the saved session ID. If Claude no longer has the
  session, Margin starts a new one and replays the last turns.
- History is saved in `state/chat/`, so it survives a restart. **Clear** deletes one
  conversation. Reset wipes `state/` and stops any answer in progress first. Move to
  Trash deletes that meeting's chat.

### C4. The panel docks on the right, over the transcript
- Options: a page of its own, a bar at the bottom of the meeting, a docked panel.
- Pick: **a docked panel**, opened with ⌘J, from the sidebar ("Ask about your
  meetings") or from a meeting ("Ask about this meeting"). A switch in the panel picks
  "This meeting" or "All meetings". Source links open a meeting and keep the panel open,
  so you can read the answer and the meeting together.

### C5. Calm states
- Not installed, not signed in and too old each show one sentence that says how to fix
  it, plus **Check again**.
- After 10 s the panel shows the time. After 60 s it suggests a narrower question.
  After 5 minutes Margin stops the turn.
- Stop keeps the partial answer, marked "Stopped.".
