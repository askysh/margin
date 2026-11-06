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

## Settings parity

The full comparison with upstream is in `docs/parity/settings.md`.

### D13. Add only the settings a meeting notepad needs
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

### D14. Summary instructions are a project template, not a file in `~/.minutes`
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

### D15. "Keep audio" deletes old audio for good
- D10 says delete means "move to the Trash". Audio retention is different. Its purpose
  is to free disk space and to keep less private audio, and the Trash keeps both.
- Pick: core's `retention` plan deletes the files. Transcripts and notes are never
  touched. The choices are Forever (the default), 30 days and 7 days. Failed and
  no-speech audio follow the same limit.
- Margin runs the cleanup when you change the setting and after each meeting is ready.
- Cost: with 7 days, "Recognise this voice" does not work on a meeting older than a
  week, because the voice sample comes from the audio.

### D16. Open at login is a LaunchAgent that opens Margin by bundle ID
- Options: `SMAppService` (needs a signed bundle, and it cannot be tested with a sandbox
  `HOME`), the autostart plugin (one more dependency), or a LaunchAgent plist.
- Pick: **a LaunchAgent** that runs `open -b com.akashmishra.margin --args --at-login`.
  It keeps working when the app moves. With `--at-login`, Margin starts in the menu bar
  and does not show its window. The e2e pass checks the file in a sandbox `HOME`.
- Reset turns it off.

### D17. A missing microphone falls back to the system default
- Core fails to start a recording when the chosen device is not connected.
- Pick: Margin checks the device before each recording. When the device is missing,
  Margin uses the system default and shows a toast. Settings marks the device
  "(not connected)".
- Calls take the mic from `recording.sources.voice`, which core reads in place of
  `recording.device`. Margin sets both.

### D18. No completion notification and no global recording shortcut yet (superseded)
- Superseded by D22 (notifications) and D23 (global shortcut), which the app-parity
  work added later the same day. The original reasoning:
- A macOS notification needs the notification permission and a signed app. A global
  shortcut needs a new plugin. Neither can be tested headless without a prompt.
- Margin already opens the finished meeting in its window, and the menu bar item starts
  a recording from any app.

## App parity

The full comparison is in `docs/parity/app.md`.

### D19. Add only upstream features that help a notepad, plus a few small extras
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

### D20. Playback goes through Tauri's asset protocol with a per-file scope
- Options: (a) a custom URI scheme with our own byte-range code; (b) the asset protocol
  with a wide scope such as the meetings folder; (c) the asset protocol with an empty
  scope, and the backend adds one `.wav` when the meeting view asks for it.
- Pick: **(c)**. Tauri's asset protocol already serves byte ranges, which WebKit needs to
  seek. The page can read only recordings that the backend checked are inside the
  meetings folder.

### D21. Action items: frontmatter first, body checklist second
- Core writes structured `action_items` when its summary has them, and keeps a
  `- [ ]` checklist in the body. Some engines write only the checklist.
- Pick: show the structured items when present. Otherwise show the checklist lines under
  `## Action Items`. Ticking uses core's `set_action_item_status` for structured items
  (status and body line change together), and changes only the box for checklist items.
  The plain "Action Items" section is hidden when the block shows, so the list appears
  once.

### D22. Notifications only when the window is not in front, with no setting
- Upstream sends "processing complete" when the window is hidden or not focused, behind
  a setting that is on by default.
- Pick: the same rule, without a setting. macOS can turn off Margin's notifications in
  System Settings, so a second switch adds a choice for little gain. A click opens the
  meeting. The call notification has a **Record call** button.
- Not copied: upstream's reminder every 20 seconds and bringing the window forward. Both
  interrupt the call.

### D23. Global shortcut ⌃⌥⌘R, off by default
- Options: ⌘⇧R (already the in-window shortcut, but browsers use it for a hard reload),
  ⌘⇧M (upstream's memo key), ⌃⌥⌘R.
- Pick: **⌃⌥⌘R**, off by default. A global shortcut takes the keys from every app, so it
  must be rare and opt-in. It records a call (or in person, when that is the default
  kind) and shows the notepad; pressed again, it stops.

### D24. Sound cues are on by default
- Upstream plays the same four sounds by default, with one switch. They confirm a start
  or stop from the menu bar or the shortcut when the window is hidden.
- Pick: the same, stored in `prefs.json` as `mute_cues` so that a missing key means on.

### D25. People filter from attendee names
- Upstream has no library filter. Its related-people chips run a search.
- Pick: attendee names in the meeting view are links. A click shows a "With <name>" pill
  in the sidebar and lists only meetings with that attendee. No new control in the
  sidebar until a filter is active.

## Ask (chat with Claude)

### D26. Ask uses the user's own `claude` CLI, headless
- Options: (a) the Anthropic API with a key in Settings, (b) the `claude` CLI that
  Margin already uses for summaries, (c) Ollama.
- Pick: **(b)**. No key to store, and the user's own login and plan apply. Margin
  uses the binary chosen in Settings → Summaries, or else the one it finds.
- Ollama is not in v1 (docs/parity/chat.md).

### D27. Claude can read the meetings folder, and nothing else
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

### D28. One conversation per meeting, plus one across all meetings
- A meeting chat puts the whole meeting file in its first prompt. The chat across all
  meetings puts a list of meetings (date, title, file) in its first prompt, and Claude
  uses Grep and Read.
- Follow-ups use `--resume` with the saved session ID. If Claude no longer has the
  session, Margin starts a new one and replays the last turns.
- History is saved in `state/chat/`, so it survives a restart. **Clear** deletes one
  conversation. Reset wipes `state/` and stops any answer in progress first. Move to
  Trash deletes that meeting's chat.

### D29. The panel docks on the right, over the transcript
- Options: a page of its own, a bar at the bottom of the meeting, a docked panel.
- Pick: **a docked panel**, opened with ⌘J, from the sidebar ("Ask about your
  meetings") or from a meeting ("Ask about this meeting"). A switch in the panel picks
  "This meeting" or "All meetings". Source links open a meeting and keep the panel open,
  so you can read the answer and the meeting together.

### D30. Calm states
- Not installed, not signed in and too old each show one sentence that says how to fix
  it, plus **Check again**.
- After 10 s the panel shows the time. After 60 s it suggests a narrower question.
  After 5 minutes Margin stops the turn.
- Stop keeps the partial answer, marked "Stopped.".

## Performance and onboarding

### D31. Measure first, keep only what the numbers support
- The numbers are in `docs/perf.md`, with the scripts that produced them.
- Kept: the jobs-watcher change, the library cache, faster search, lazy views and
  fewer re-renders. Each one has a before and after figure.

### D32. The jobs watcher reads the archive only when something changed
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

### D33. The library list is cached by file size and modification time
- The cache lives in memory and is rebuilt on each list. Entries for files that are gone
  drop out. A change that keeps both the size and the modification time is missed. That
  needs a same-size write in the same nanosecond, and Margin and core write by rename.

### D34. Onboarding: five short steps, every one with a default
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

### D35. No microphone level check during onboarding
- A level meter must open the microphone. That shows the macOS prompt if access is not
  decided yet. If access is already granted, it records real audio in a test run, which
  the QA rules forbid.
- Pick: **no meter**. The live view's rule already shows the level during the first
  recording. Add a meter later, shown only after `microphone == granted`.

### D36. Release profile: fat LTO in one codegen unit
- Options: keep `lto = "thin"`, or use `lto = "fat"` with `codegen-units = 1`.
- Measured (`docs/perf.md`): the app is 11% smaller (39.3 MiB → 35.0 MiB). Command
  latency and processing time are the same within noise. A release build after a small
  change takes 175 s instead of 99 s.
- Pick: **fat LTO, one codegen unit**. Release builds are only for shipping; day-to-day
  work uses debug builds, which this does not change.
- Not picked: `panic = "abort"`. A panic in one command would then end the whole app
  instead of failing that command.

### D37. Measured and dropped
- **Preloading the notepad font.** The font was loaded about 100 ms after navigation with
  and without the preload, so the change was removed.
- **Code-splitting Settings and Onboarding** is kept, but only for size: the startup
  chunk is 8% smaller (194 → 179 KB). Time to the first `boot` call did not change
  measurably (about 55 ms either way under the same load).

## Updates and distribution

### D38. Auto-update with Tauri's updater, installed on restart or quit
- Options: (a) Sparkle, as benchbar uses; (b) Tauri's updater plugin; (c) only
  `brew upgrade`.
- Pick: **(b)**. It is built for Tauri apps and needs no extra framework in the bundle. It
  checks one `latest.json` on the latest GitHub release and verifies a minisign signature
  against the public key in `tauri.conf.json` before it installs anything. Sparkle's EdDSA
  keys stay benchbar's.
- Behaviour: the first check is 20 s after launch, then one every six hours. A newer
  version downloads in the background. Margin then shows "Margin X is ready" with
  **Restart to update** in the toolbar, in the menu bar and in Settings → Updates. If you
  do nothing, it installs when you quit. It never installs or restarts during a recording.
  Settings has one switch to turn checking off.
- The update archive is packed after the app is notarized and stapled, so an update is
  the same notarized app that the DMG has.
- Only a published release is "latest", so a draft never reaches anyone's app.

### D39. Homebrew cask in askysh/homebrew-tap, updated on publish
- `brew install --cask askysh/tap/margin`. The cask has `auto_updates true`, so brew
  leaves updates to the app (D38).
- `.github/workflows/homebrew-tap.yml` runs when a release is published. It checks the
  DMG against `SHA256SUMS`, renders `packaging/homebrew/margin.rb`, runs brew style, audit,
  install and uninstall, and opens a pull request on the tap that merges itself. This is
  the same flow as benchbar's.
- `uninstall` unloads the login item and quits the app. `zap` removes Margin's own
  folders. It never removes the meetings folder.

### D40. The meeting opens when the transcript is ready; the summary follows
- Before: core's pipeline wrote the summary before the job finished, so a long meeting
  waited for it. On an 80-minute meeting, Claude ran for 300 s and then hit core's limit,
  so the meeting waited five minutes and got no summary.
- Now: the processing worker runs with summaries off. When the meeting is ready, Margin
  opens it and writes the summary in the background with the engine from Settings. The
  meeting view says "Writing the summary" and fills it in when it lands. Margin holds the
  file's lock during the rewrite, so notes typed meanwhile are saved after it.
- Claude gets 15 minutes for a summary instead of 5, because nothing waits for it now.
- Cost: core's LLM speaker naming (it guesses names from the notes during the summary
  step) no longer runs during processing. Renaming a speaker once, with voice memory,
  still names them in later meetings.

### D41. The live transcript uses the chosen model
- Core's live sidecar defaults to the dictation model ("base"), which Margin never
  downloads. With "small" chosen, the live sidecar found no model and wrote no lines: an
  80-minute meeting had an empty live transcript.
- Now `live_transcript.model` is the chosen model. For "Best" (large), which is too slow
  to keep up live, it is "base", and Margin downloads it too.

## 0.0.3: the meeting is usable right after Stop

### D42. The meeting opens when core writes the file, before speakers are separated
- Context: an 80-minute call waited 39.6 minutes after Stop. Core writes the meeting file
  when transcription ends and sets the job to `transcript-only` (jobs.rs:1710). Then it
  separates speakers and rewrites the file once, at the end. Margin waited for Complete.
- Options: (a) wait for Complete, as before; (b) an "Open transcript" button at
  `transcript-only`, as upstream's app does; (c) go to the meeting by itself at that point.
- Pick: **(c)**. The processing view becomes the meeting view when the job has an output
  file and is at `transcript-only` or later. A status line says "Finishing: separating
  speakers…". The view reads the file again at each job stage, in place, so the scroll
  position and notes being typed stay. The sidebar shows the meeting once, as its job row,
  until it is done.
- Speaker rename, title, action items, Summarize and Move to Trash stay locked until the
  job is done. The backend refuses them too (`processing::ensure_not_processing`), because
  core's last rewrite fails if the file changed after it was written.

### D43. Notes typed during processing go into the job's notes draft
- Before: the meeting view locked the notes while processing, because a write to the file
  made core's last rewrite fail.
- Now: `save_notes` writes the job's notes draft (`drafts/<job>.md`) while a job can still
  write the file, or while the draft is not in the file yet. `processing::finish` puts the
  draft in the file when the job completes. A lock makes sure that an edit can never land in
  a draft that was already moved. While the draft exists, the meeting view shows it, not
  core's copy of the notes.
- The Obsidian guard in `finish` now compares the file with the notes that were queued with
  the job (`user_notes`), not with the draft. Before, an edit during processing would have
  looked like an outside edit, and been dropped.
- An empty draft is kept. It means that the notes were cleared.

### D44. The live transcript is the draft after Stop
- Stop saves the live lines, with their You / Them guesses, to `drafts/<job>.live.json`.
  Stop reads the live transcript one more time first, so the last utterance is kept.
- The processing view shows them at once as "Draft transcript", with the note "Draft from
  the live transcript; the final transcript replaces it." The file survives a restart, and
  `finish` removes it with the notes draft. A failed job keeps both until a retry succeeds.
- The draft is for reading only. It is not the final transcript (see
  docs/research/live-transcription.md, option 3).

### D45. A debug-only hold for the e2e run
- In debug builds, the worker waits at "separating speakers" while
  `$MARGIN_HOME/e2e-hold-speakers` exists (two minutes at most). The e2e run uses it to look
  at a meeting that is open while core still processes it. Release builds ignore the file.

## 0.0.4 (to renumber)

### A minutes-core fork with one patch
- Rolling transcription needs core to take transcript work from the host. Core v0.32.1
  always transcribes the whole recording after Stop (UPSTREAM-PATCHES.md #11).
- Options: (a) wait for upstream; (b) a `[patch]` on the tagged core; (c) a fork with the
  patch, pinned by commit, and the same patch sent upstream.
- Pick: **(c)**. `minutes-core` comes from `askysh/minutes`, branch
  `margin/v0.32.1-rolling`: the v0.32.1 tag plus one commit, pinned by its SHA in
  `src-tauri/Cargo.toml`. The same patch is proposed upstream as a pull request on
  `silverstein/minutes`.
- The fork changes no dependency, so `Cargo.lock` is still seeded from upstream's lock at
  the tag, and the `ort` pin stays. `scripts/bump-upstream.sh <tag> <commit>` moves to a new
  tag once the patch is rebased in the fork (README, "Updating minutes-core").
- When upstream ships the patch, `scripts/bump-upstream.sh <tag>` goes back to upstream by
  tag. If upstream declines it, Margin keeps rebasing one commit per core release.
- What the patch adds (core API):
  - `transcript_checkpoint::transcribe_span` transcribes a span of one source with the
    final pass's chunking and settings.
  - `jobs::queue_live_capture_with_transcript_checkpoint` queues a capture with those
    spans.
  - The final pass reuses a span only if its audio digest and settings fingerprint match
    the final file and settings. Otherwise it transcribes that range again.

### Rolling transcription
- While a meeting records, `rolling.rs` transcribes each source (the mic and call stems,
  or the in-person recording) in pieces of about 2 minutes. Each piece ends at the
  quietest 100 ms of its last 15 s. At Stop, the pieces go to the job, and core
  transcribes only the rest.
- Fast and Balanced only. Best (large-v3-turbo) needs about 4× the compute of Balanced,
  and cannot keep up on a MacBook Air (docs/research/live-transcription.md).
- Settings → Recording → "Transcribe during the meeting", on by default. With Best it is
  off and cannot be changed.
- It must never risk the capture:
  - The thread only reads the growing files. It runs at utility priority.
  - A failed piece is dropped, and the final pass transcribes that range.
  - If it falls more than 8 minutes behind, or a piece panics, it stops for the rest of
    the meeting.
- Stop waits up to 20 s for a piece that is being transcribed, because the final pass
  would take longer to redo it.
- A title typed during the meeting changes the transcriber's hints. Pieces made before the
  change no longer match, and core transcribes their range again after Stop.
- Measured with `margin --dev rolling-job` on the synthetic calls, Balanced (details in
  docs/research/live-transcription.md, "Rolling transcription in the app"):
  - 20-minute call: the transcript is ready 6.9 s after Stop, against 78 s without
    rolling. Complete: 42 s against 114 s.
  - 60-minute call (played at 3× real time): ready 6.5 s after Stop, against 236 s.
    Complete: 109 s against 341 s. The speaker passes are now most of the wait.
  - WER is the same: 8.4% against 8.3% (20 min), 8.3% against 8.3% (60 min).
  - Core accepted every piece (20 of 20, 63 of 63).
- Not measured yet: GPU, battery and heat during a real call, with the live transcript
  running at the same time.
