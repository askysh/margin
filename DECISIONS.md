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
