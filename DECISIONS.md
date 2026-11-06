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

## Settings parity (to renumber)

The full comparison with upstream is in `docs/parity/settings.md`.

### S1. Add only the settings a meeting notepad needs
- Options: copy upstream's panel (about 80 controls in six tabs), or add the settings that
  change a meeting's result.
- Pick: **the second**. Margin adds ten:
  - keep audio
  - microphone
  - stop after silence
  - call suggestions
  - open at login
  - meeting language
  - speaker separation
  - names and words to spell right
  - summary language
  - summary instructions
- Dictation, Coach, the command palette, the terminal, Parakeet and calendar stay out.
- Each new field in `AppSettings` is optional. A caller that does not send a field
  leaves its stored value unchanged. So an onboarding screen that does not know the new
  fields cannot reset them.

### S2. Summary instructions are a project template, not a file in `~/.minutes`
- Core reads extra prompt text only from a template. It looks for templates in
  `<cwd>/.minutes/templates/`, then in `~/.minutes/templates/`, then in the bundled set.
- Options:
  - (a) Write `~/.minutes/templates/`. That shares the file with the upstream app and
    breaks D3.
  - (b) Run Margin with its state directory as the working directory, and write
    `state/.minutes/templates/meeting.md`.
- Pick: **(b)**. The app, the processing worker and the QA bridge start in the state
  directory. The dev CLI keeps your shell's directory, because its arguments are
  relative paths.
- The template uses the slug `meeting`, the same as core's bundled default. A meeting
  file records `template: meeting`. That name resolves in every Minutes install, and it
  falls back to the bundled template when the instructions are cleared.
- Reset clears the state directory, so it also clears the instructions.

### S3. "Keep audio" deletes old audio for good
- D10 says delete means "move to the Trash". Audio retention is different. Its purpose
  is to free disk space and to keep less private audio, and the Trash keeps both.
- Pick: core's `retention` plan deletes the files. Transcripts and notes are never
  touched. The choices are Forever (the default), 30 days and 7 days. Failed and
  no-speech audio follow the same limit.
- Margin runs the cleanup when you change the setting and after each meeting is ready.
- Cost: with 7 days, "Recognise this voice" does not work on a meeting older than a
  week, because the voice sample comes from the audio.

### S4. Open at login is a LaunchAgent that opens Margin by bundle ID
- Options: `SMAppService` (needs a signed bundle, and it cannot be tested with a sandbox
  `HOME`), the autostart plugin (one more dependency), or a LaunchAgent plist.
- Pick: **a LaunchAgent** that runs `open -b com.akashmishra.margin --args --at-login`.
  It keeps working when the app moves. With `--at-login`, Margin starts in the menu bar
  and does not show its window. The e2e pass checks the file in a sandbox `HOME`.
- Reset turns it off.

### S5. A missing microphone falls back to the system default
- Core fails to start a recording when the chosen device is not connected.
- Pick: Margin checks the device before each recording. When the device is missing,
  Margin uses the system default and shows a toast. Settings marks the device
  "(not connected)".
- Calls take the mic from `recording.sources.voice`, which core reads in place of
  `recording.device`. Margin sets both.

### S6. No completion notification and no global recording shortcut yet
- A macOS notification needs the notification permission and a signed app. A global
  shortcut needs a new plugin. Neither can be tested headless without a prompt.
- Margin already opens the finished meeting in its window, and the menu bar item starts
  a recording from any app.

## App parity (to renumber)

The full comparison is in `docs/parity/app.md`.

### P1. Add only upstream features that help a notepad, plus a few small extras
- Options: (a) copy every upstream surface; (b) take the upstream features that a
  Granola-like notepad needs, and skip the rest; (c) (b) plus small extras that upstream
  does not have.
- Pick: **(c)**. Margin adds action-item checkboxes, Summarize again, Copy as Markdown,
  notifications and sound cues from upstream. The extras are playback with click-to-play
  times, the people filter, a global shortcut and the menu-bar clock and recent meetings.
  Each extra uses data Margin already keeps (the `.wav`, attendees) and adds no new
  setting except the shortcut.
- Skipped: dictation, coach, palette, terminal, calendar, memo mode, sensitive meetings,
  mic mute, archive. The reason for each is in the parity doc.

### P2. Playback goes through Tauri's asset protocol with a per-file scope
- Options: (a) a custom URI scheme with our own byte-range code; (b) the asset protocol
  with a wide scope such as the meetings folder; (c) the asset protocol with an empty
  scope, and the backend adds one `.wav` when the meeting view asks for it.
- Pick: **(c)**. Tauri's asset protocol already serves byte ranges, which WebKit needs to
  seek. The page can read only recordings that the backend checked are inside the
  meetings folder.

### P3. Action items: frontmatter first, body checklist second
- Core writes structured `action_items` when its summary has them, and keeps a
  `- [ ]` checklist in the body. Some engines write only the checklist.
- Pick: show the structured items when present. Otherwise show the checklist lines under
  `## Action Items`. Ticking uses core's `set_action_item_status` for structured items
  (status and body line change together), and changes only the box for checklist items.
  The plain "Action Items" section is hidden when the block shows, so the list appears
  once.

### P4. Notifications only when the window is not in front, with no setting
- Upstream sends "processing complete" when the window is hidden or not focused, behind
  a setting that is on by default.
- Pick: the same rule, without a setting. macOS can turn off Margin's notifications in
  System Settings, so a second switch adds a choice for little gain. A click opens the
  meeting. The call notification has a **Record call** button.
- Not copied: upstream's reminder every 20 seconds and bringing the window forward. Both
  interrupt the call.

### P5. Global shortcut ⌃⌥⌘R, off by default
- Options: ⌘⇧R (already the in-window shortcut, but browsers use it for a hard reload),
  ⌘⇧M (upstream's memo key), ⌃⌥⌘R.
- Pick: **⌃⌥⌘R**, off by default. A global shortcut takes the keys from every app, so it
  must be rare and opt-in. It records a call (or in person, when that is the default
  kind) and shows the notepad; pressed again, it stops.

### P6. Sound cues are on by default
- Upstream plays the same four sounds by default, with one switch. They confirm a start
  or stop from the menu bar or the shortcut when the window is hidden.
- Pick: the same, stored in `prefs.json` as `mute_cues` so that a missing key means on.

### P7. People filter from attendee names
- Upstream has no library filter. Its related-people chips run a search.
- Pick: attendee names in the meeting view are links. A click shows a "With <name>" pill
  in the sidebar and lists only meetings with that attendee. No new control in the
  sidebar until a filter is active.

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

### P6. Release profile: fat LTO in one codegen unit
- Options: keep `lto = "thin"`, or use `lto = "fat"` with `codegen-units = 1`.
- Measured (`docs/perf.md`): the app is 11% smaller (39.3 MiB → 35.0 MiB). Command
  latency and processing time are the same within noise. A release build after a small
  change takes 175 s instead of 99 s.
- Pick: **fat LTO, one codegen unit**. Release builds are only for shipping; day-to-day
  work uses debug builds, which this does not change.
- Not picked: `panic = "abort"`. A panic in one command would then end the whole app
  instead of failing that command.

### P7. Measured and dropped
- **Preloading the notepad font.** The font was loaded about 100 ms after navigation with
  and without the preload, so the change was removed.
- **Code-splitting Settings and Onboarding** is kept, but only for size: the startup
  chunk is 8% smaller (194 → 179 KB). Time to the first `boot` call did not change
  measurably (about 55 ms either way under the same load).
