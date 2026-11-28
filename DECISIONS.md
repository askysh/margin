# Decisions

Product and technical decisions made without asking. Each one lists the options and the pick.

## D1. Product name: "Margin"
- Options: reuse "Minutes" (it confuses the two apps, and both are installed side by side),
  "Margin", "Notch", "Scribe".
- Pick: **Margin**, for notes in the margin of a meeting. Bundle ID
  `com.akashmishra.margin`, following benchbar's `com.akashmishra.benchbar`. It is easy to
  rename before the first release.

## D2. Shell: Tauri v2 + React + TypeScript (Vite)
- See docs/research.md §2.

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
- Replaced by D50: Ask is now a bar at the bottom.

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
  file's lock during the rewrite. Notes typed meanwhile go into a draft, as in D43, and
  into the file when the summary ends (since 0.0.10: before, the saves waited behind the
  lock, and a Quit lost them). A draft that a Quit left goes into the file at the next
  launch.
- Claude gets 15 minutes for a summary instead of 5, because nothing waits for it now.
- Cost: core's LLM speaker naming (it guesses names from the notes during the summary
  step) no longer runs during processing. Renaming a speaker once, with voice memory,
  still names them in later meetings.

### D41. The live transcript uses the chosen model
- **The Best part is superseded by D52.** With Best, the live transcript uses Balanced
  (`small`), not `base`.
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

## 0.0.4: transcribe during the meeting

### D46. A minutes-core fork with one patch
- **Superseded by D48 (0.0.5).** Upstream merged the patch, and Margin no longer uses the
  fork.
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
  tag once the patch is rebased in the fork (docs/development.md, "Updating minutes-core").
- When upstream ships the patch, `scripts/bump-upstream.sh <tag>` goes back to upstream by
  tag. If upstream declines it, Margin keeps rebasing one commit per core release.
- What the patch adds (core API):
  - `transcript_checkpoint::transcribe_span` transcribes a span of one source with the
    final pass's chunking and settings.
  - `jobs::queue_live_capture_with_transcript_checkpoint` queues a capture with those
    spans.
  - The final pass reuses a span only if its audio digest and settings fingerprint match
    the final file and settings. Otherwise it transcribes that range again.

### D47. Rolling transcription
- **"Fast and Balanced only" is superseded by D52.** Rolling transcription now works with
  Best too. The 4× estimate below was not a measurement.
- While a meeting records, `rolling.rs` transcribes each source (the mic and call stems,
  or the in-person recording) in pieces of about 2 minutes. At first each piece ended at
  the quietest 100 ms of its last 15 s. D49 replaces that cut. At Stop, the pieces go to
  the job, and core transcribes only the rest.
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
  change no longer match, and core transcribes their range again after Stop. **Changed by
  D60:** rolling now does those pieces again during the meeting.
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

## 0.0.5: no fork

### D48. Back on upstream minutes-core
- Upstream merged the transcript checkpoint patch (UPSTREAM-PATCHES.md #11) as
  silverstein/minutes PR #1316, merge commit `209265f` on main. No tag has it yet. The
  latest tag is v0.32.1.
- Options: (a) stay on the fork until upstream ships a tag; (b) pin upstream main by
  commit.
- Pick: **(b)**. `minutes-core` comes from `silverstein/minutes`, pinned by
  `rev = "209265fac94763d45171acb284c258eff6718690"`: v0.32.1 plus 6 commits. The core
  code at that commit is the same as the fork commit that 0.0.4 used, so rolling
  transcription needs no change.
- `Cargo.lock` is seeded from upstream's lock at that commit. It still has ort
  2.0.0-rc.10, so the `ort` pin stays.
- `scripts/bump-upstream.sh <ref>` takes a tag or a full commit SHA. For a commit, it
  makes sure that the commit is on upstream main and after the latest tag.
- Settings shows the core as "v0.32.1 + 209265f", not as a tag that does not have the
  patch.
- When a tag with the patch ships, run `scripts/bump-upstream.sh <tag>` to go back to a
  tag pin. The askysh/minutes fork is no longer used.

## 0.0.7: pieces that match the final pass

### D49. A rolling piece ends where core's chunker resets
- New fact: on the owner's real 80-minute call, the transcript from rolling pieces was
  different from the whole-file pass in about 12% of the words. Whisper on this Mac gives
  the same words for the same audio: two whole-file passes gave the same transcript. So
  the cut caused the difference.
  - Core's chunker reads 100 ms windows, counted from the first sample of the audio it
    gets. Its noise floor changes with the audio.
  - A piece that starts at any sample moves that grid and starts the noise floor again.
    The chunks move by up to 0.1 s, and whisper gets other audio. Words at the cuts were
    lost, and words changed in every piece.
- Options: (a) keep the quietest-100 ms cut; (b) cut only where the chunker resets, with
  Margin's own copy of the chunker's rules; (c) wait until core makes its chunker or a cut
  helper public.
- Pick: **(b)** now, and **(c)** when upstream ships it (UPSTREAM-PATCHES.md #16).
  `piece_cut.rs` cuts on the stem's 100 ms grid, at a point where the chunker is back in
  its first state:
  - after a chunk of 1 s to 45 s, when 800 ms of silence closes it: the chunk's end plus
    12,800 samples;
  - at the end of a 45 s chunk, which the chunker splits without a pause.
- Which reset point:
  1. the longest pause that ends at most 15 s before the target length (2 minutes), or
     later;
  2. else the first 45 s split in that range;
  3. else wait for more audio, up to 1.5 × the target, and then take the latest reset
     point in the piece;
  4. with no reset point at all, the quietest 100 ms of the last 15 s, on the grid.
  With no speech at all, the cut is at the target.
- Pieces start at sample 0, and every cut is a reset point. So each piece, and the audio
  after the last piece, gets the same chunks as the whole stem.
- Measured:
  - Core's own capture checkpoint with the same rule (silverstein/minutes#1319 plus this
    fix), on the real call: the same transcript as the whole-file pass, word for word
    (5,106 of 5,106 and 4,402 of 4,402 words). The transcript was ready 9 s after Stop,
    against 29 minutes.
  - Margin's rule on the same two stems, with core's chunker: 40 and 42 pieces. Every
    chunk is the same as in the whole-stem pass (286 and 431 chunks).
- Risk: if core changes its chunker, Margin's copy cuts in the wrong place. Then pieces
  are still accepted, but their words differ from the final pass. The test
  `margin_chunker_is_cores_chunker` compiles core's `detect_meeting_vad_chunks` from the
  pinned source and compares it with Margin's copy. It fails when the two differ.
  `scripts/bump-upstream.sh` runs the tests, so a new core with another chunker fails
  there. The test reads core's code from Cargo's checkout. That code is not copied into
  this repository.
- Limits: two cuts are not reset points. In a long silence, the next piece starts with
  the first noise floor, and the whole stem has the floor that the silence gave it. The
  two come close within seconds of silence, and in digital silence they become equal.
  The quietest-100 ms fallback can move the chunks near it. Both are rare in a call.

## Window layout

### D50. Ask is a bar at the bottom that opens upward
- New fact: the owner uses Margin in a narrow window beside the browser. There, the panel
  of D29 covered most of the notes, and the owner asked for a bar at the bottom that
  opens when you click it.
- Options: (a) keep the docked panel; (b) a page of its own; (c) a bar at the bottom.
- Pick: **(c)**. Closed, Ask is a bar at the bottom of the meeting and the home page. A
  click or ⌘J opens it upward into a sheet, with the question field where the bar was.
  Esc, the chevron or a click outside the sheet closes it.
- With a transcript column, the bar and the sheet sit over the notes column, so the
  transcript stays in view. Source links still open a meeting and keep the sheet open,
  as in D29.
- The sheet stays mounted when it is closed. An answer keeps streaming, the bar shows
  that Claude is working, and a question you started to type waits in the bar.
- The bar is not shown while you record or while a meeting is processed, so nothing
  covers your notes. ⌘J still opens Ask there.
- The sidebar row "Ask about your meetings" and the meeting's "Ask about this meeting"
  link are removed. The bar replaces both.
- Ask checks the `claude` CLI only when you first open it, not at startup.

### D51. A narrow window is a notepad
- New fact: the same use, beside the browser. The window could not go below 820 px, and
  under 900 px a page rule gave the home page three columns. The home page text then got
  about 250 px.
- Options: (a) keep 820 px and fix the rule; (b) a smaller window with a layout for it,
  as other meeting notepads do.
- Pick: **(b)**. The window can go down to 440 px.
  - ⌘\ or the button right of the traffic lights hides and shows the sidebar. Margin
    remembers the choice.
  - Under 760 px, the sidebar is a drawer over the notes. A meeting picked in the drawer
    closes it.
  - When the page is under 820 px, a meeting, a recording and a processing view show one
    column. A switch above it picks Notes or Transcript. The notes column is hidden, not
    removed, so the caret, the scroll and the audio player stay.
  - Settings in a narrow window shows its sections as icons only.
- The page rule that gave the home page three columns is fixed.

## Rolling transcription with Best

### D52. Rolling transcription works with Best, and Best's live transcript uses Balanced
- New fact: D47 kept Best out because of an RTF of about 1.7. That number came from "4.2×
  the compute of `small`" on synthetic audio. It was not measured on real audio. On
  2026-11-09 the owner measured it on a MacBook Air M4 (16 GB, debug build) with
  `margin --dev rolling <wav> 120`. The input was 11 minutes of a real call, both stems,
  with Hindi and English mixed:

  | Model | RTF | Work left after Stop | Lines | Garbled non-Latin lines |
  |---|---|---|---|---|
  | `small` | 0.463 | 50.8 s | 148 | 9 |
  | `large-v3-turbo` | 0.729 | 58.5 s | 224 | 0 |
  | `large-v3-turbo-q5_0` | 0.735 | 59.2 s | — | — |

  Turbo keeps up. `small` writes garbled Devanagari on this call and drops content.
  The q5_0 model is not faster, so Margin does not add it.
- Options:
  - Rolling with Best: (a) keep it off, (b) turn it on.
  - The live model for Best: (a) keep `base`, (b) `small`, (c) turbo.
- Pick: **rolling on with Best**, and **`small` (Balanced) live**.
  - Rolling with Best was checked at real time with the live transcript running at the
    same time (below). It kept up, and it did not change the live transcript.
  - `small` live has 2.4 points less WER than `base` live (9.3% against 11.7%), and it
    costs 390 MB more memory. On the Hindi and English call, `small` still garbles some
    Hindi lines. `base` was not measured on that call. Turbo live lagged and lost lines at 3× (docs/research/live-transcription.md),
    and it would share the GPU with rolling turbo.
- Measured at 1× real time, the same 11 minutes (41:00 to 52:00 of the call), debug
  build, on AC power. `margin --dev rolling-job` with Best and `margin --dev live-replay`
  with `small` ran at the same time, in two sandboxes:

  | | Best, rolling off | Best, rolling on |
  |---|---|---|
  | Stop to transcript ready | 428.4 s | **50.6 s** |
  | Stop to Complete | 457.4 s | 77.6 s |
  | Pieces made / failed / accepted by core | — | 10 / 0 / 10 |
  | Stop waited for a piece | — | 0.2 s |
  | Rolling compute during the meeting | — | 298 s for 660 s |
  | Rolling stopped itself (8 minutes behind) | — | no |
  | Transcript against rolling off | — | the same, word for word (1,629 words) |

  | Live transcript (`small`) | Alone | With rolling Best |
  |---|---|---|
  | Lines | 54 | 54 |
  | Text | — | the same, word for word (1,575 words) |
  | Extra time before a line shows (median, p90, max) | — | 0 s, 6.6 s, 8.1 s |
  | Peak memory | 798 MB | 806 MB |
  | GPU busy (median of 5 s samples, 27% with nothing running) | 12% | 95% |

  The rolling process used 4% CPU on average and up to 2.0 GB of memory during the
  meeting. No thermal warning was recorded.
- Measured on battery on 2026-11-09, the same Mac, debug build, screen on, no other work.
  The input was the whole 80-minute call (4,832 s, both stems) at 1× real time, with
  rolling and the live transcript (`small`) at the same time. Balanced ran on 15 minutes
  of the same call (41:00 to 56:00) after an 18-minute rest. The "matched" column is Best
  on the same 15 minutes. A sampler read `ioreg` (battery charge in mAh, system power,
  GPU) and `ProcessInfo.thermalState` every 10 s:

  | | Best, 80 min | Best, matched 15 min | Balanced, 15 min |
  |---|---|---|---|
  | Battery per hour (of 4,061 mAh) | 26% | 25% | 24% |
  | System power (mean; 3.8 W with nothing running) | 13.4 W | 13.0 W | 15.0 W |
  | GPU busy (median) | 98% | 99% | 67% |
  | Thermal state | "fair" for 82% of the samples, from minute 4 | "fair" | nominal |
  | Peak memory of the processes | 2.6 GB | 2.6 GB | 1.7 GB |
  | Pieces made / failed | 76 / 0 | — | 14 / 0 |
  | Stop to transcript ready | 156 s | — | 21 s |
  | Stop to Complete | 351 s | — | 58 s |

  - Best uses about the same charge as Balanced. The GPU runs at full power with both
    models. Best keeps it busy for longer, at lower clocks.
  - The thermal state never went past "fair", and rolling did not fall behind.
  - The final transcript of the whole call had 11,784 words and no garbled lines. The
    saved Balanced transcript of the same call had 9,667 words and 9 garbled lines.
  - The live transcript (`small`) stayed 3 to 7 s behind the audio for the whole call.
    It garbled 35 of its 456 lines on the Hindi parts.
- Upgrade: a Best user from 0.0.8 has `base` but not `small`. Margin downloads `small` at
  launch in the background. Recording does not wait for the live model
  (`models::ready_to_record`). Until `small` is there, the live transcript uses `base`.
- Costs and risks:
  - The GPU is busy for most of the meeting. That costs battery, and on a fanless MacBook
    Air it adds heat. Measured on battery (below): Best uses about the same charge per
    hour as Balanced, and the Mac stays at the "fair" thermal state.
  - In the app, both models are in one process: about 2.7 GB at peak, plus the app.
  - A slower Mac than the M4 can fall behind. Then rolling stops itself (D47), and Best
    transcribes the rest after Stop, as before.
  - The switch "Transcribe during the meeting" can turn it off.

## The recording chip

### D53. A recording chip takes the place of the window during a recording
- New fact: after a real meeting, a user said the full Margin window during a recording
  is "too invasive". They asked for a small recording sign at the edge of the screen,
  with the notes one click away.
- Options:
  - What shows during a recording: (a) the window, as before; (b) a small chip, and the
    window only when you ask for it.
  - The design: three prototypes. A, "Quiet pill": a dot and the time, with Stop and
    Notes on hover. B, "Edge tab": a narrow tab with a level rule. C, "Proof of life":
    two level lanes, the latest line and a quick-note field.
  - The window at Stop: (a) it comes back on the processing view; (b) it stays away.
- Pick: **(b), design A**, with two parts from C.
  - When a recording starts, the window hides and the chip slides in at the right edge
    of the screen, about a third of the way down. Settings → Recording → "Show the
    recording chip" turns it off. It is on by default. Off keeps the window as before.
  - The chip shows while a recording runs and the main window is not visible: hidden,
    minimized, or closed to the menu bar. When the window shows, the chip goes. A
    click on the chip brings the window back on the live notes, with the caret at the
    end. The live view has a control that shrinks the window to the chip.
  - From C: Stop needs two clicks. The first click arms it ("Stop?") for 3 s. The dot
    stays red while Margin records, also when no sound comes in.
  - Left out: pause (Margin has no pause), the you and them levels (the recorder sends
    one mixed level) and the quick note (a click opens the notes).
  - A Start from the menu bar, the global shortcut or a notification no longer brings
    the window forward when the chip is on. If the Start fails, the window comes
    forward with the error.
- Pick for Stop: **the window stays away**. At Stop the chip says "Saving", then slides
  out. The window goes to the processing view and then the meeting, as before, but it
  does not come forward. The call can still be on when you press Stop, and a window that
  jumps forward takes the keyboard from it. The "meeting ready" notification opens the
  meeting, and so does the menu bar. If the Stop fails, a notification says why.
- The window:
  - A second webview window, `chip`, from its own page `chip.html` and its own
    capability (`core:event:default` only). It draws its states from the recorder's
    events, as the main window does.
  - Transparent, with no frame and no shadow (the page draws the shadow). The level is
    the status bar level, so it stays over other windows. It joins all Spaces (but not
    another app's full-screen Space, see the real-app check below), stays
    out of ⌘` and Mission Control, and does not hide when Margin hides.
  - It is shown without becoming the key window, so the call app keeps the keyboard
    when the chip appears. At Start, Margin hides itself if it is the active app, so
    macOS gives the keyboard back to the app you were in.
  - It is a plain window, not a non-activating panel. tao's window class has its own
    state, so it cannot become an NSPanel after it is made, and `tauri-nspanel` is a new
    crate. So a click on the chip makes Margin the active app. The chip gives the
    keyboard back when the pointer leaves it, after a click, and on Escape.
  - The window follows the size of the pill, so no empty part of it takes clicks meant
    for the app behind. WebKit can send no hover events to a window of an app that is
    not active, so Margin also checks where the pointer is, 10 times a second, while the
    chip shows.
  - A drag moves the chip up and down only, inside the visible part of the screen. The
    arrow keys move it too. Margin keeps the place as a fraction of the screen height, so
    it suits a different display.
- Silence: the recorder does not report a silent mic. The chip says "No sound comes in"
  for 8 s when the level stays at zero for 20 s of recording, then keeps a mic mark.
  Quiet room noise is above zero, so a quiet moment does not set it off.
- Costs and risks:
  - A click on the chip activates Margin for a moment, which a non-activating panel
    would not do.
  - The design's green track along the screen edge during a drag is left out. The
    window is only as tall as the pill, so it has no room for the track.
  - A display that goes away while the chip shows keeps the chip where macOS puts it.
    The next time the chip shows, it uses the saved place.
- Real-app check on 2026-11-10 (debug build in a sandbox, a MacBook Air M4, TextEdit as
  the call app, clicks and keys sent with CGEvent):
  - Passed: Start from the window and from ⌃⌥⌘R hides the window and gives the keyboard
    back to TextEdit. Hover opens the chip while TextEdit is active. The window shrinks
    back after a hover. Two-click Stop arms and resets. A click opens the notes. ⌘W, ⌘M
    and ⌘H bring the chip back. A drag moves y only and stops at the menu bar and the
    Dock. The place survives a restart. Stop from the chip saves, keeps the window away
    and keeps the notes. "No sound comes in" shows. With the chip off, Start keeps the
    window.
  - Fixed: a click on the chip left Margin as the active app with no window. The chip
    asked for the focus back only when its page had focus, which a borderless window
    never has. A right-click opened the web view's menu.
  - **Open: the chip does not show over a full-screen app.** The collection behaviour
    `FullScreenAuxiliary` lets a window join the full-screen Space of its own app only.
    A window of a regular app shows over another app's full-screen Space only as a
    non-activating panel, or when the app is an accessory app. The options are
    `tauri-nspanel` (a new crate), or the accessory activation policy while the chip
    shows (no Dock icon during a recording). This needs a decision.
  - Not checked: VoiceOver, reduced motion, a second display, a real call app.

## minutes-core options at 209265f

### D54. Show "Hide Margin from screen sharing", and leave the other core options out
- New facts:
  - minutes-core's `privacy.hide_from_screen_share` defaults to `true`, and Margin writes
    that value into every `config.toml`. No Margin window applied it, so the file said
    that Margin was hidden from screen sharing, and it was not. Core only stores the key.
    The app that owns the windows must apply it.
  - docs/parity/settings.md skipped the setting because Margin shows no live coaching.
    But Margin shows your notes and the live transcript during a call, and they are
    private too.
  - Reports say that macOS 15 ScreenCaptureKit ignores `NSWindow.sharingType = none`.
    A check on this Mac (macOS 27.2) says that it works here (see below).
- Options for the screen sharing key:
  - (a) Keep it unused.
  - (b) Apply it, with a switch that is off by default.
  - (c) Apply it, with a switch that is on by default, as core and upstream do.
- Pick: **(b)**. Settings → General → Privacy → "Hide Margin from screen sharing". It is
  off by default. When it is on, people on a call do not see your notes when you share
  your screen.
  - It uses Tauri's content protection on the main window and on the recording chip.
    A change applies at once to the open windows (`screenshare.rs`).
  - Why off: screenshots leave Margin out too, and so does a screen share that is meant
    to show Margin. The owner takes screenshots of Margin to report problems. A setting
    that hides the app from them must be a choice, not a surprise after an update.
  - Margin keeps the switch in its own prefs (`hide_from_screen_share`) and copies it
    into `privacy.hide_from_screen_share` on every load and save. Every config that Margin
    wrote before says `true`, so the file cannot tell a choice from a default.
- Real-app check on 2026-11-10 (debug build in a sandbox, macOS 27.2): a ScreenCaptureKit
  capture of the display, with and without the Margin window. With the key on,
  0 of 361 sampled pixels in the window changed, so the capture had already left the
  window out. With the switch off in `config.toml`, 361 of 361 changed. Not checked: the
  recording chip, macOS 14 and 15, a real call app. The e2e pass checks that the switch
  writes the key and asks the windows to change.
- Left out, with the reason:
  - `transcription.engine = "parakeet"`: core marks it a "dormant subprocess contract".
    At 209265f every platform resolves it to Whisper.
  - `transcription.engine = "sherpa"` (Parakeet v3): it needs the `engine-sherpa`
    feature, a separate sherpa plugin library from upstream built and signed with
    Margin, and a model of about 665 MB. Parakeet v3 covers 25 European languages, not
    Hindi and not most of the languages in Margin's language list. Without all of these,
    core falls back to Whisper.
  - `transcription.engine = "apple-speech"`: live transcript only. Batch transcription
    falls back to Whisper.
  - `transcription.noise_reduction`: it needs the `denoise` feature, which Margin does
    not compile. It does nothing.
  - `transcription.vad_engine = "ort-silero"`: it needs the `vad-ort` feature and an
    ONNX model. Upstream found regressions on real meetings and does not use it by
    default.
  - `transcription.name_correction = "conservative"`: it works, but upstream keeps it off
    "until a real-audio evaluation", and it can change a name to a wrong one. Not tried on
    mixed-language meetings.
  - `transcription.apple_speech_shadow`: measurement only. It never changes a transcript.
  - `diarization.model = "community-1"` and `diarization.embedding_model = "cam++-lm"`:
    opt-in in core, not in upstream's own Settings, and not measured on Margin's audio.
    A new embedding model changes voice match scores, so saved voices can stop matching.
  - `diarization.stem_correlation_threshold`: a number to tune, not a choice. If the
    laptop-mic case needs a change, Margin should change the default after a measurement.
  - `call_detection.stop_when_call_ends`: core does not read it. Upstream's app
    implements it. Core's check finds a running call app, not a call, so Margin cannot
    tell when a call ends.
  - `call_detection.auto_record`, `consent.*`, `calendar.*`, the API-key summary engines,
    `summarization.ollama_url`: no new fact since D9 and D13.
  - `recording.max_duration_secs`, `retention.keep_pinned_audio`, `identity.aliases`,
    `transcription.partial_max_secs`, `live_transcript.backend`: core defaults are good,
    Margin has no pins or calendar, or the option is for developers.

### D55. Ask before Quit while a meeting is processing
- New fact: on 2026-11-10 the owner quit Margin from the menu bar while a 62-minute call
  was 82% through transcription with Best. The worker runs in its own process group and
  should outlive Margin, and it did outlive an earlier Quit that day. This time it
  stopped 10 s after the Quit, with no crash report and no log line. The next worker
  found the job "crashed while transcribing" and transcribed both stems again from the
  start. About 100 minutes of work were lost, and the meeting was ready 2 h 47 min after
  Stop. The cause of the stop is not known.
- Options:
  - (a) Keep Quit as it is, and fix the cause when it is found.
  - (b) Ask before Quit while a job is queued or processing.
  - (c) Make a retry resume where it stopped. That is a change in minutes-core, sent
    upstream as an issue first.
- Pick: **(b)** now, and (c) upstream. Quit from the menu bar or with ⌘Q asks "Margin is
  still transcribing" and names the meeting. **Keep Running** is the default button.
  **Quit Anyway** quits as before. A recording is saved before the question, as before.
  `processing::confirm_quit`, `tray::quit`.
- The question says that transcription "can" start again from the beginning, because a
  Quit does not always stop the worker.

### D56. An Apple Speech live transcript, in Margin's own code
- New facts (2026-11-10):
  - During a call, Margin was still transcribing the previous call with Best on the GPU.
    Core's Whisper live transcript shares the GPU, and it dropped 103 utterances.
  - The owner's meetings mix Hindi and English. On the 11-09 meeting, the Whisper live
    transcript (Balanced) wrote garbled Devanagari for the Hindi parts. Apple Speech with
    `hi-IN` wrote them as Hinglish in Latin letters ("Granola ka jaise nahin hai ab side
    mein ek chota sa icon bas dikhta hai") and kept the English. `en-IN` dropped most of
    the Hindi. Both took under 2 s for 91 s of audio.
  - Streamed at real time, an Apple Speech line arrived 1 to 4 s after its utterance
    ended. Apple Speech closes a line at a pause, so a line can cover up to 30 s.
  - minutes-core has Apple Speech code, but at 209265f a recording's live transcript
    always uses Whisper ("Apple Speech runs only in standalone live transcript"), and it
    needs a signed XPC service that Margin does not ship.
- Options:
  - (a) Wait for upstream.
  - (b) Margin's own helper, and core's live transcript off while it runs.
- Pick: **(b)**. Settings → Recording → "Live transcript": Whisper (the default), or
  Apple Speech in Hindi and English (`hi-IN`), English (India) or English (US).
  - `src-tauri/speech/margin-speech.swift` uses SpeechAnalyzer with SpeechTranscriber
    (macOS 26). `build.rs` compiles it, and Tauri bundles and signs it
    (`bundle.externalBin`). It reads float32 samples on stdin and writes final results as
    JSON lines.
  - `applespeech.rs` runs one helper for each source: the microphone and the call audio
    in a call, the microphone in person. It reads the growing WAV files and writes core's
    live transcript lines. The source gives "You" and "Them" exactly, in place of the
    loudness guess.
  - At Start, Margin checks that the locale is ready. If not, the live transcript uses
    Whisper and a message says so. Choosing a locale in Settings downloads its assets.
  - The final transcript does not change. It still uses the speech model.
- Cost: Margin maintains a Swift helper, and the build needs Xcode 26 or later, as
  minutes-core already does. The upstream route stays open: when core allows Apple Speech
  during a recording, Margin can drop the helper.

## The notepad view and meeting times

### D57. A direct way into the notepad view, and back
- New fact: the owner uses the notepad view (D51) every day, beside a call. The only way
  into it was to drag the window narrower by hand, and the only way back was to drag it
  wider again. The owner asked for a direct way.
- Options:
  - (a) A separate notepad window.
  - (b) A mode flag that changes the layout, apart from the width.
  - (c) The width stays the only switch. A command resizes the window, and leaving puts
    the window back where it was.
- Pick: **(c)**. The layout still follows the width (under 760 px is the notepad), so a
  resize by hand still works, and there is one state, not two.
  - Ways in and out: View → "Enter Notepad View" or "Leave Notepad View" (⌘⇧N), and a
    button right of the sidebar button in the title row. The menu bar item has "Open as
    Notepad". `notepad.rs`.
  - Entering keeps the window's top, bottom and right edge, at 480 px wide. Leaving
    restores the last wide frame. Margin also remembers the last notepad frame, so a
    notepad that you moved or resized comes back where you left it. A frame that no
    longer fits the screen is moved and shrunk onto it. The frames are kept for the run
    only, because the window opens wide at each launch.
  - AppKit animates the resize (`setFrame:display:animate:`).
  - The recording chip gets no new control. A click on it brings the window back in the
    view it had, and D53 keeps the chip to the time, Stop and Notes.
  - The traffic lights move down 6 pt (`trafficLightPosition` y 22 → 28). At 22 their
    centre was 20 pt from the top, and the title row's controls are centred at 26 pt in
    the 52 pt row. Now all of them share one centre line, in both views.

### D58. Start, Stop and ready times come from core's job record
- New fact: the owner asked to see the recording's start and Stop times, and when the
  notes were ready, as numbers in the app. Long waits after Stop are the problem the
  owner works on upstream, so the time from Stop to ready matters most.
- Options for the data:
  - (a) Add the times to the meeting file. That changes the meeting file format, which
    Obsidian, the minutes CLI and MCP read. Not without the owner's OK.
  - (b) Work out Stop as the date plus the duration. The duration is the audio's length,
    not the clock's, and there is no ready time in the file.
  - (c) Core's job record. Core writes `recording_started_at`, `recording_finished_at`
    and `finished_at` for each job, and keeps finished jobs in `<state>/jobs/archive/`.
    It does not prune that folder.
- Pick: **(c)**. `processing::times_for` finds the newest job whose `output_path` is the
  meeting file. It reads the active jobs, then the archive. The archive listing is kept
  until the folder changes, as in D32.
  - The header shows "Tue, 10 Nov • 11:31 AM – 12:24 PM • 53m 3s • people". Under it, a
    quiet line shows "Ready at 3:00 PM • 2h 36m after Stop".
  - "Ready" is the job's `finished_at`: the time core finished the transcript and the
    speakers, which is when the "Meeting ready" notification goes out. It is shown only
    for a job that produced a meeting. The summary comes later (D40) and is not in it.
  - The processing view shows "Stopped at 12:24 PM • 4m 12s ago". It counts each second.
  - A value that the record does not have is left out. An older meeting, a meeting from
    the minutes CLI, or a file renamed since it was written shows the date and the start
    time as before. A Stop before the start, or a ready time before Stop, shows nothing.
  - A recording inside one minute shows one time, not "10:37 AM – 10:37 AM".
  - One separator for the whole header: the `·` in the date and the `•` dot between
    parts were mixed. A dot that would start a line is clipped, so a wrap leaves no dot
    at the end of a line.

## Real recordings, 2026-11-10

### D59. An untitled meeting gets its title from the summary, in the frontmatter only
- New fact: the owner's 53-minute call had no title at Stop. It is titled "Akash", from
  the first words of the transcript. The worker runs with summaries off (D40), so core's
  title step had no summary text, logged `missing-summary-text`, and kept that fallback.
  The summary that Margin writes later never set a title.
- Options:
  - (a) Keep the fallback title.
  - (b) Turn summaries on in the worker. That brings back the wait that D40 removed.
  - (c) After the summary, ask the summary engine for a title, with core's public
    `summarize::refine_title`, and change the title in the frontmatter only.
  - (d) As (c), and rename the file with core's `markdown::rename_meeting`, as core does
    when it makes a title.
- Pick: **(c)**.
  - Only for a meeting that had no title at Stop. Stop leaves a marker,
    `drafts/<job>.untitled`. A title typed in the meeting view before the summary ends
    is kept.
  - The file keeps its name, as with a title typed in the meeting view (`set_title`).
    Links from Obsidian notes, the job record that gives the meeting's times (D58) and
    Ask's chat key keep working.
  - Margin checks the model's answer: one line, 2 to 12 words, at most 80 characters,
    and not a sentence about the task. Else the meeting keeps its title. Core's own check
    is private.
- Cost: one more call to the summary engine for each untitled meeting. The file name
  still has the fallback words.
- Upstream: core's resummarize could return the title that the summary call already
  writes (`Summary.title`), so no second call is needed.

### D60. Rolling pieces are done again when the title or the settings change
- New fact: on the owner's real call on 2026-11-10, a title typed after Start changed the
  decode hints. Core rejected the first microphone piece (114 s of audio) at Stop with
  "different transcription settings" and transcribed it again. That took 55 s of the wait
  after Stop. D47 knew about the rejection, but not its cost.
- Related risk: rolling loads the config at Start, and the worker loads it again at Stop.
  A Settings change during a meeting made every piece fail the check.
- Options:
  - (a) Keep D47: core does the range again after Stop.
  - (b) Hold the title until Stop. The title also goes into the hints of every later
    piece, so this changes nothing.
  - (c) When the inputs change, do the finished pieces again from the stem file during
    the meeting, when the thread has no new piece to do.
- Pick: **(c)**. `rolling.rs` keeps the inputs of each piece: the transcription settings
  and the decode hints. Each loop reads the title, and the config file when its
  modification time changed. When the thread is idle, it does again the first piece
  whose inputs are not the current ones. New audio always comes first. A piece that fails
  the second time is dropped, and the final pass covers its range.
- A title typed in the last minute before Stop can still leave pieces that do not match.
  Stop waits for at most one piece (20 s), as before.
- Not measured on a real call yet. The log line "rolling transcription finished" now
  counts `redone`.

### D61. Margin for Windows ships on the Microsoft Store, as an MSIX named Margin Notes
- New fact: docs/brief.md put Windows out of scope for v1. The owner now has a Microsoft
  Store developer account and a Windows laptop for tests, and wants Margin on the Store.
- Where the code lives:
  - (a) A second repository for Windows.
  - (b) This repository. The UI and most of the Rust are shared, and the Windows parts
    sit behind `cfg` gates.
  - Pick: **(b)**. The work is on the `windows-port` branch until the Windows build
    records and transcribes a meeting. It then merges into `next-release` as one change.
- The package:
  - (a) An EXE installer from Tauri, linked from the Store. The publisher must sign it
    with a certificate from a trusted authority, host it, and update it. Microsoft's own
    signing service takes individual developers from the United States and Canada only.
  - (b) An MSIX. The Store signs it, hosts it and updates it, at no cost.
  - Pick: **(b)**. Tauri does not make an MSIX, so Microsoft's `winapp` CLI packs the
    built `margin.exe` with `windows/msix/AppxManifest.xml`.
- Costs of an MSIX:
  - It needs the restricted capability `runFullTrust`. Each submission gives the reason.
  - It cannot install WebView2. It uses the copy that Windows has.
  - The Store build must not update itself, so the updater (D38) is off in it.
- The name: "Margin" was taken in the Store. The product is **Margin Notes**. The package
  display name must be a reserved name, so the Start menu and the window say "Margin
  Notes" on Windows. The Mac app stays "Margin".
- The version: the Store does not take a version that starts with 0, and it keeps the
  fourth part. The package version is `(major + 1).minor.patch.0`, so Margin 0.0.10 is
  package 1.0.10.0 and Margin 1.0.0 is package 2.0.0.0. The order of versions stays the
  same for every release. About shows Margin's own version.
- Not known yet: how Windows records the other people on a call. Core's
  `core-audio-tap` backend is for macOS only. This gets its own decision.
- The identity values, the listing text and the answers for Partner Center are in
  docs/windows-store.md.

### D62. The first Windows build: where data goes, and what is off
- New fact: on 2026-11-13 Margin built and ran on Windows 11 (the owner's laptop, Visual
  Studio 2026, Rust 1.100). A 31 s recording from a test WAV became a meeting file 7 s
  after Stop, on the CPU. The Mac build passed its tests and all 88 end-to-end checks
  with the same code.
- macOS code stays as it was. Each Windows path sits beside it behind `cfg`. The Windows
  calls are in one module, `win.rs`.
- Data: Margin's settings, models and state go to `%LOCALAPPDATA%\Margin`. The roaming
  folder is wrong for them, because the models are large and a roaming profile copies
  its folder to each PC. In a Store package, Windows redirects this folder to the
  package's own store and removes it on uninstall.
- Meetings: the default folder is `Meetings` in the Documents folder that Windows gives.
  With OneDrive backup on, that folder is in OneDrive, so the audio syncs. Options: a
  local folder as the default, or the Documents default with a note. Pick: **keep the
  default, and say it**. Setup and Settings show a note when the chosen folder is in
  OneDrive, with a way to choose another folder. Documents is where a user looks for
  the files, and some users want the backup.
- Voices: minutes-core keeps `voices.db` in `%USERPROFILE%\.minutes`. It ignores `HOME`
  on Windows, so a sandbox does not isolate it. The end-to-end pass links that folder
  into its sandbox and stops when a real one exists (UPSTREAM-PATCHES.md #1).
- The global shortcut is Ctrl+Alt+Shift+R. Windows keeps the shortcuts with the Windows
  key for itself.
- Off on Windows, and hidden in the UI:
  - Record call. Core's call capture is for macOS only. Upstream tracks capture with
    WASAPI loopback in silverstein/minutes issue 253. Margin offered to contribute it
    there on 2026-11-17.
  - The recording chip. It needs a window above full-screen apps on every desktop.
    Start does not hide the window on Windows.
  - The Apple Speech live transcript (D56). The live transcript is Whisper.
  - The updater (D38) and open at login (D16). The Store updates the app. Open at login
    needs a startup task in the package.
  - The call suggestion.
- The build: `ort-sys` rc.10 finds the output folder by counting folders above
  `OUT_DIR`, and cargo 1.100 changed that layout. Options: pin an older Rust for the
  whole repository, or prepare the folders in a script. Pick: **the script**,
  `scripts/build-windows.sh`. It leaves the Mac build and the `ort` pin alone. It also
  copies `DirectML.dll` and the four Visual C++ runtime DLLs beside `margin.exe`, because
  Windows does not include them.
- Not tested yet: a real microphone (the laptop had none active), a release build, a PC
  that never had the build tools, Ask and summaries, and the tray by hand.

### D63. The Windows build uses the graphics card, and needs Windows 11
- New fact: on 2026-11-13 the release build was timed on the owner's laptop (Ryzen 7
  6800H, RTX 3070 Ti Laptop GPU). Transcription of 231 s of English speech:

  | Model | CPU | Vulkan |
  |---|---|---|
  | `base` | 14.3 s | 4.5 to 5.4 s |
  | `small` | 45.0 s | 7.5 to 8.6 s |
  | `large-v3-turbo` | 219.7 s | 10.9 to 11.3 s |

  The integrated Radeon GPU was also about 4 times faster than the CPU with
  `large-v3-turbo`. Speaker separation adds about 17 s, on the CPU in each build.
- Options: CPU only, Vulkan, or CUDA. CUDA is for NVIDIA cards only and needs its
  runtime on the PC. Upstream prefers Vulkan on Windows.
- Pick: **Vulkan, by default**. `large-v3-turbo` on the CPU runs at the speed of the
  recording. The owner's Hindi-English meetings need that model.
- One package serves each PC. With no usable Vulkan device, whisper.cpp logs "no GPU
  found" and uses the CPU. This was tested with the drivers turned off.
- The package ships the Vulkan loader, `vulkan-1.dll`, beside `margin.exe`. The exe
  imports it, and only a graphics driver installs it. Without it, the app does not start
  on a PC with no such driver.
- Cost: the package is 44 MB, not 28 MB. The build PC needs the Vulkan SDK.
- CPU: the build sets `GGML_NATIVE=OFF`, so whisper.cpp uses AVX2 on each build PC. A CPU
  with no AVX2 cannot run Margin Notes. Those are older than about 2013.
- Windows: the minimum is Windows 11. An MSIX cannot install WebView2. Windows 11
  includes it, and Windows 10 does not always have it.
- The release build of whisper.cpp had no optimization at first, and took 194 s for 46 s
  of audio. `scripts/build-windows.sh` sets the compiler flags back
  (UPSTREAM-PATCHES.md #21).
- A real microphone works. The laptop's Windows build shows its own microphone prompt for
  Margin Notes at the first recording.
- The loader is version 1.4.363.0, from the Vulkan runtime files of LunarG. Its license
  is Apache 2.0 and MIT, and the license file ships with it.
- "Use the graphics card" in Settings → Transcription turns the GPU off, for a PC with a
  bad driver. minutes-core has no setting for this (UPSTREAM-PATCHES.md #23). So Margin
  sets `VK_LOADER_DRIVERS_DISABLE=*` for itself at start, and the Vulkan loader then
  finds no device. A change applies at the next start.
- The package as an installed app, tested on 2026-11-17 with Developer Mode:
  - Windows shows a package no microphone before the user answers its prompt. Margin
    asks for that prompt at the first Record and from the Allow button.
  - Settings and models go to the store of the package, and removal deletes them. The
    meetings folder and `%USERPROFILE%\.minutes` stay.
  - The app writes nothing into its install folder.
  - To register the package folder again reset the microphone answer. Not known: if an
    update from the Store does the same.
- A real meeting, 2026-11-17: 15 minutes of one of the owner's calls, two people, mostly
  English with short parts in another language.

  | Run | Total | Transcription | Words |
  |---|---|---|---|
  | `small`, GPU | 252 s | 209 s | 1123 |
  | `large-v3-turbo`, GPU | 129 s | 83 s | 2116 |
  | `large-v3-turbo`, CPU | 2252 s | 2214 s | 2052 |
  | `large-v3-turbo`, GPU, language Hindi | 281 s | 244 s | 309 |

  - On the CPU, `large-v3-turbo` took 37 minutes for 15 minutes of audio. On the GPU it
    is about 11 times faster than the recording, and it used about 2.5 GB of GPU memory.
  - `small` was slower than `large-v3-turbo` on the GPU, and wrote half the words.
  - `large-v3-turbo` wrote English for each line, also where the speech was not English.
    Some of those lines are a guess. The Mac transcript of the same 15 minutes is the
    same: English only, 1815 words. So this is Whisper, not the Windows build. `small`
    wrote those parts in scripts that nobody can read. Language set to Hindi lost the
    English.
  - Speaker separation found three speakers and a large unknown group for two people,
    from the mixed file. A call on the Mac has one file for each side. Windows gets that
    with call capture (D62).
- Not measured: a meeting that is mostly Hindi, the live transcript on the GPU, a second
  PC, and a PC with no `vulkan-1.dll` in Windows.

## Upstream updates

### D64. A workflow of Margin's own opens the pull request for a new minutes-core
- Number: D61 to D63 are in use on the `windows-port` branch. This entry takes D64, so
  the two branches merge without two entries with one number.
- Need: upstream `silverstein/minutes` tags releases often. Each bump was a local build,
  a test run and the end-to-end pass, done by hand. Upstream cannot notify Margin, so
  Margin must ask.
- Options:
  - (a) Dependabot. dependabot-core can move a cargo git dependency that is pinned to a
    tag that looks like a version. It cannot do this bump:
    - It updates the lock with `cargo update -p minutes-core`. That resolves
      minutes-core's crates fresh. The bump must seed `Cargo.lock` from upstream's lock
      (D48), else `ort` and `pyannote-rs` move to releases that break the build.
    - It does not move a pin by `rev`, the pin that D48 uses now.
    - It does not change the label in `src-tauri/src/commands.rs` or the `# Now:` line.
    - It runs no build or test on macOS, and `ci.yml` does not build the Rust side.
    - `.github/dependabot.yml` turns off cargo version updates on purpose.
  - (b) Renovate. A custom manager can find the tag. The hosted Renovate app does not
    run commands after the update, so it cannot seed the lock or run
    `scripts/bump-upstream.sh`. A self-hosted Renovate can, but it is a second system to
    run and to trust.
  - (c) A GitHub Actions workflow in this repository that runs
    `scripts/bump-upstream.sh`.
  - (d) Keep the bump by hand.
- Pick: **(c)**, `.github/workflows/upstream-bump.yml`. It uses the script, so a bump in
  CI and a bump at a Mac are the same edit. It runs each Monday, and by hand with an
  optional tag or commit SHA.
  - The script gets `--no-build`. Without the flag it works as before.
  - Three jobs. `plan` and `check` have a read-only token. `check` compiles upstream code
    on `macos-26` with no credentials on disk, and sends the change as a patch artifact
    before the build starts. `pr` can write. It runs no upstream code, and it refuses a
    patch that changes a file other than `Cargo.toml`, `Cargo.lock` and `commands.rs`.
  - The checks run in the workflow: the UI type check and build, `cargo test`, and the
    end-to-end pass. A failed check makes the pull request a draft.
  - No cache is saved. A cache that upstream code wrote could reach the release build,
    which has the signing keys.
- Limits:
  - The workflow token opens the pull request, so `ci.yml` waits for a person to select
    "Approve workflows to run". A GitHub App token or a personal access token in a secret
    would start it at once. That is a new secret, so it is left to the owner.
  - The runner has no microphone, no system audio and no Apple Speech. A real call on a
    Mac stays a manual step.
  - It is not known yet that the end-to-end pass works on a GitHub runner (speech models
    on a virtual Mac, the macOS voices that the fixtures use). Its result shows in the
    pull request. When it does not pass, the checklist asks for a run on a Mac.

### D65. A normal installer for Windows, for tests outside the Store
- New fact: on 2026-11-17 the owner put the Store submission on hold, and asked for a way
  to install Margin Notes as a normal user does.
- `scripts/build-windows.sh --installer` makes `MarginNotes-setup-<version>.exe` with
  Tauri's NSIS bundle. It holds the same exe and DLLs as the Store package.
- It installs for one user, into `%LOCALAPPDATA%\Margin Notes`, with no administrator
  rights. It has no updater.
- It is not signed, so Windows SmartScreen warns. A certificate is a later decision, and
  only if this installer becomes a way to ship. Today it is for tests.
- WebView2: the installer downloads Microsoft's bootstrapper only when WebView2 is
  missing. Windows 11 includes it.
- The uninstaller removes the app. It leaves the settings and models in
  `%LOCALAPPDATA%\Margin`, the meetings folder and `%USERPROFILE%\.minutes`. A Store
  package differs: Windows removes its settings and models (D63).

### D66. The Windows copy from the installer updates itself, and the release builds it
- New fact: D61 left Windows updates to the Store. The installer of D65 installs a copy
  that the Store does not know, so that copy never got an update.
- Options: no update outside the Store, a second exe with the updater built in, or one
  exe that decides at start.
- Pick: **one exe that decides at start**. The updater is on when the process has no
  package identity, the exe is the one that the NSIS bundle made, and `uninstall.exe` is
  beside it. So the Store package and a build folder keep it off, with no build flag.
- The update is the setup exe, signed with the same updater key as the Mac update. The
  app reads the same `latest.json`. Its Windows entry has the key `windows-x86_64`.
- The setup runs with no questions and no administrator prompt. "Restart to update"
  starts the new version. Quit installs and does not start it. Nothing installs during a
  recording.
- Tested on 2026-11-17 with a throwaway key: 0.0.11 to 0.0.12, settings, model and a
  meeting kept byte for byte, and a wrong signature refused.
- The release: `release.yml` got two jobs, `windows-build` and `windows-publish`. They
  add the Windows files to the draft that the macOS job makes. A failed Windows build
  leaves the macOS release as it is. `ci.yml` builds the installer on the release pull
  request. The owner asked for this change to the release workflow.
- Cost: a release with no Windows entry in `latest.json` shows an error in Settings →
  Updates on Windows. Check the draft before you publish.
- The installer has no code signing certificate. SmartScreen warns at the first install,
  not at an update.
- Not tested: an update from GitHub over HTTPS, and the Windows jobs on a runner.

## Meeting flow and knowledge (26 Nov 2026)

### D67. Calendar lookup comes back as an opt-in, with a toast that starts notes
- New fact for D9: the owner asked for it. Google Meet runs in a browser, so the call
  banner (process names) never sees it, and meetings were titled with the first words
  spoken. A calendar event has the title, the people and the call link.
- Options: keep calendar off, turn it on for everyone, or a switch with its own
  permission step.
- Pick: **a switch**, *Start notes from my calendar* in Settings → Recording, off by
  default. Turning it on asks for Calendars access before anything is saved, in the
  foreground. That answers D9's reason: the prompt follows the click that asked for it.
  Margin runs core's helper directly for this, with up to 2 minutes for an answer, since
  core's own calls stop the helper after 3 seconds. Refused, write-only or unanswered
  access leaves the switch off with a message that says which.
- Margin now bundles core's `calendar-events` helper (`build.rs` compiles it from the
  pinned core checkout, `externalBin` signs it). Core looks for it beside the binary
  first. Without it, only the Mac that built Margin could read the calendar.
- When the switch is on, `calendar_watch.rs` checks every 30 seconds. An event that starts
  within a minute, or started up to five minutes ago, and has a call link or two or more
  attendees, gets one offer: a banner, and a notification with Start notes when the
  window is not in front. Start notes opens the `https://` call link and records a call
  under the event's title. Nothing records without that click.
- The same switch sets core's `calendar.use_event_title_for_meeting_title` and
  `calendar.require_attendees_or_url`, so the worker titles the meeting from the event and
  ignores focus blocks.
- Cost: core reads the calendar from the worker too, as D9 said. With access granted in
  the foreground first, that read raises no prompt.
- Not tested: the toast against a real calendar (it needs the permission). Core's helper
  reads the call link from the event's location. A Google Meet link that sits only in the
  event's notes or conference data is not seen, so such an event qualifies by its
  attendees, and Start notes records without opening the call.

### D68. A silent microphone gets a warning thirty seconds in
- New fact: a 75-minute call on a Bluetooth headset kept only the other side. The
  microphone stem peaked at 0.002 of full scale for the whole call, and nothing said so.
- Pick: once per recording, 30 seconds in, Margin reads the last 20 seconds of the
  microphone track (the voice stem on a call, the meeting file in person). A peak under
  0.004 shows a warning in the window and a notification. The recording goes on.
- Why the peak, not the loudness: a quiet room has a low average, but a working
  microphone passes 0.01 within seconds (typing, breathing). A dead one does not.
- Cost: a working microphone in a silent room for 20 seconds could warn once. The warning
  only says to check the microphone.

### D69. Ollama summaries ask the model not to think first, through a local forwarder
- New fact: core's Ollama request has no `think` field and a fixed 2-minute limit
  (UPSTREAM-PATCHES #3). qwen3.5:4b with thinking off extracts a 13-minute meeting in
  42 to 50 seconds. With thinking on, Margin's summaries failed on both test meetings.
- Options: tell people to pick another model (the old message), an Ollama Modelfile (it
  cannot set `think`), copy core's summary code (Margin copies no core code), or a
  forwarder.
- Pick: **a forwarder** (`ollama_proxy.rs`). For one summary or title call, Margin starts
  it on a free local port and points an in-memory copy of the config at it. It adds
  `"think": false` to `/api/generate` and `/api/chat` and passes everything else through.
  The config file does not change.
- Settings → Summaries → *Answer without thinking first*, on by default, kept in
  prefs.json. Remove the forwarder when core sends `think`.

### D70. Settings → Knowledge: the wiki with no model, and a command after each meeting
- Pick: core's knowledge wiki with `engine = "none"` only. Facts come from the meeting's
  frontmatter. A model would write facts nobody can check against the transcript.
- Core takes decisions and action items from the summary, and Margin's worker runs with
  summaries off (D40). So with a summary engine on, the worker leaves the wiki alone and
  the first summary after processing updates it, with one log entry per meeting. With
  summaries off, the worker updates it with the people only. *Summarize again* does not
  update the wiki, because core appends a log entry on every update.
- *After each meeting, run this command* sets core's `hooks.post_record`. *Use Margin Desk*
  fills it when `~/margin-desk/bin/desk.mjs` and Node are found.
- Semantic search stays out of Settings. Core's `embed-worker` is not in the app bundle,
  so the switch would do nothing. Margin Desk has its own search.

### D71. Whisper's prompt echoes are removed from the transcript
- New fact (benchmark, 26 Nov 2026): on room tone, keyboard noise and hum, Whisper wrote
  "Silence test. Preserve spelling exactly when hearing." 8 times in 225 seconds. Core
  gives Whisper the prompt "Names and terms that may appear in this audio: <title, names>.
  Preserve spelling exactly when heard." and Whisper reads it back on noise. Digital
  silence gave no text.
- Pick: when a meeting is ready, before the summary, Margin removes transcript lines that
  hold the prompt's fixed words, and title-only lines that occur two or more times. One
  title-only line stays, since it can be real speech. Notes and frontmatter do not change.
- UPSTREAM-PATCHES asks core to drop segments that reproduce the prompt.

### D72. A choice of speaker model in Settings → Transcription
- New fact (benchmark, AMI ES2004a, four speakers): the legacy model found 2 speakers and
  gave the wrong speaker for 37.9% of speech. Raising the threshold split voices into 8.
  core's `community-1` set found 4 and gave the wrong speaker for 14.0%. It took 118
  seconds for 17.5 minutes of audio, against 26.
- Pick: *Speaker model*, Standard (legacy) or Better (community-1). The default stays
  Standard, so no one gets a 32 MB download or slower processing without asking. Margin
  downloads the chosen set through the existing model download.

### D73. The notes get the keyboard back after Settings, and a stray key goes to them
- New fact (UX round, 26 Nov 2026): during a recording, opening Settings (or the microphone
  banner's *Change microphone*) and coming back left the focus on the page. Typing went
  nowhere, and the saved notes held only the text from before Settings. Settings hides the
  live view with `visibility: hidden`, and a hidden field loses its focus.
- Pick: the live view knows when Settings covers it. When it shows again, the notes take
  focus (the caret stays where it was), unless a field is already focused. Dismissing the
  microphone banner does the same. While nothing editable has focus, a printable key without
  ⌘, Ctrl or Alt moves focus to the notes, and the browser types the key there. Space does
  so only on a bare page, because on a button it presses the button.
- Cost: a key pressed on a bare page is no longer ignored. Shortcuts carry ⌘ or Ctrl, so
  they are untouched.

### D74. Setup mentions the calendar and Margin Desk, and the two shortcuts say where they work
- New fact: setup never mentioned the calendar banner (D67), Knowledge or Margin Desk, and the
  calendar switch is off on a new install, so nobody found them. Setup said ⌘⇧R and Settings
  said ⌃⌥⌘R, with no word on why.
- Pick: the last setup step has a *Start notes from my calendar* switch, off, with the line
  that macOS asks to read the calendar when it is switched on, and one line that Margin Desk is
  in Settings → Knowledge. The switch saves at once through `save_settings`, which asks
  macOS and refuses without access; the step then shows the refusal and the switch stays off.
  The shortcut line now says *In Margin*, and that a shortcut for any app is in Settings.
  Settings says *From any app, ⌃⌥⌘R …* and *Inside Margin, ⌘⇧R and ⌘⇧I work* without it.
- The switch is not offered on Windows, where calls cannot be recorded (D61).

### D75. A command that Margin refuses stays in the box
- New fact: a refused *Run this command* was cleared from the box. The toast said why, but
  the text was gone. The refusal puts the old value back in the settings, and the box followed.
- Pick: `save` answers with the refusal text (or null when saved). A text box that gets a
  refusal keeps the text, is marked invalid (`aria-invalid`, a red border, the reason below
  the box), and does not follow the settings back. Editing the text clears the mark. Nothing
  is saved until the backend accepts the text. This holds for every text box in Settings.

### D76. UNKNOWN is shown as "Unknown speaker"
- New fact: a meeting whose speakers core could not match has the label `UNKNOWN`, and the
  chip showed it in capitals.
- Pick: `UNKNOWN` is a raw label like `SPEAKER_n`: it shows as *Unknown speaker* with the
  dashed chip of an unnamed speaker. The other speakers keep their numbers (Speaker 1,
  Unknown speaker, Speaker 3). The file keeps `UNKNOWN`.

### D77. Ask says that it is Claude only
- New fact: Ask runs the user's `claude` CLI (D26), read-only. The summary engine in Settings
  does not matter to it. With Ollama, or summaries off, "Claude isn't signed in" read as if
  Ask could use something else.
- Options: Ask through Ollama (it cannot be limited to read-only files the way `claude
  --tools Read,Grep,Glob` is), or say it plainly.
- Pick: say it. When Ask is not ready, it adds that Ask always uses Claude Code and ignores the
  summary engine.

### D78. Paths show `~` for any home folder
- New fact: `tildify` knew only `/Users/<name>`. A home elsewhere (a sandbox, Linux, Windows)
  showed the full path on the home page, in setup and in Settings.
- Pick: it also knows `/home/<name>` and `C:\Users\<name>`. For a home with no such shape,
  the app learns it at boot from the default meetings folder, which is always
  `<home>/Documents/Meetings` on macOS (`paths::default_output_dir`). A recognisable shape wins,
  because Windows can move Documents into OneDrive. A longer folder name that only starts with
  the home (`/Users/samuel` for `/Users/sam`) is not shortened.

### D79. Search shows the spelling it used
- New fact: the backend may correct a typo in a meeting search ("warrnty").
- Pick: `search_meetings` answers with `{ hits: [{ path, excerpt }], corrected: string | null }`.
  When `corrected` is a string, the sidebar shows *Showing results for "<corrected>"* under the
  search box. The UI also accepts the bare list of an older backend, and treats null as absent.


## Silence, titles, search and summaries (26 Nov 2026, Grug round)

### D80. A vendored minutes-core with patches, again
- New fact (benchmark plan, docs/benchmark/2026-11-27-improvement-plan.md §1.3): the
  next transcription and speaker gains need changes inside core: VAD and decoding
  settings, the vocabulary prompt, the speaker count, short speaker tracks, RTTM output
  and the speaker pipeline. Core has no settings for any of them.
- Options: (a) wait for upstream; (b) a fork repository, as in D46; (c) the pristine
  crates at the pinned commit, vendored into this repository, with a Cargo `[patch]`
  pointing at them, and one commit per patch.
- Pick: **(c)**. It needs no second repository, every patch is reviewable in this
  repository's history, and the pin stays the same.
  - `margin/vendor/minutes-core/` holds `crates/core` and `crates/whisper-guard` from
    silverstein/minutes at `209265f`, unchanged in its first commit. It also holds the
    files that core's build script and tests read from outside those crates: the calendar
    helper sources, two test fixtures, `assets/demo.wav` and `mcp/src/index.ts`.
  - Its root `Cargo.toml` is upstream's `[workspace.package]` and
    `[workspace.dependencies]`, with only the two crates as members. Its `Cargo.lock` is
    upstream's lock, pruned to those crates, so core's own tests build with upstream's
    versions.
  - `src-tauri/Cargo.toml` keeps both `minutes-core` lines on the upstream `rev` and adds
    `[patch."https://github.com/silverstein/minutes"] minutes-core = { path = ... }`.
    `Cargo.lock` then lists minutes-core and whisper-guard without a source.
  - Every patch defaults to upstream's behaviour. Margin turns patches on in
    `corecfg.rs`. Each patch has an entry in UPSTREAM-PATCHES.md (#25 onward), written
    to be sent upstream as a pull request.
- How to rebase onto a new upstream ref (docs/development.md, "Updating minutes-core"):
  1. List Margin's patch commits: `git log --oneline -- margin/vendor/minutes-core`.
  2. Run `scripts/bump-upstream.sh --no-build <ref>`. It moves the pin and re-seeds
     `Cargo.lock`.
  3. Replace `crates/core`, `crates/whisper-guard` and the extra files with the new
     ref's copies, and commit that alone ("vendor pristine minutes-core <ref>").
  4. Cherry-pick each patch commit in order, and fix any conflicts.
  5. Run `cargo build` and `cargo test`. Cargo drops the source lines for the two
     patched crates from the lock again.
  6. A patch that upstream has merged is dropped. When none are left, delete
     `margin/vendor/` and the `[patch]` table, and Margin is back on plain upstream.

### D81. A Hindi + English model, from a file the user adds
- New fact (Hinglish trial, 26 Nov 2026): Trelis/whisper-hinglish-preview (Apache-2.0),
  converted to ggml, loads as a normal large-v3 model in stock whisper.cpp. On FLEURS
  Hindi (25 clips) its WER was 12.1, against 20.8 for large-v3-turbo with language `hi`.
  It writes Devanagari only, English words included.
- Pick: a fourth transcription model, `hinglish`, "Hindi + English (Devanagari)". It is
  for Hindi-heavy calls and is never the default.
  - No public URL hosts the converted file. Margin offers no download: the model is ready
    when `ggml-hinglish.bin` (q5_0, 1.08 GB) is in the models folder. A download attempt
    says where to put the file (`models::fetch`, `local_only`).
  - Margin refuses to select it until `ggml-hinglish.bin` is in the models folder and at
    least 500 MB, so a half-copied file does not count. If the file goes missing, or
    breaks, later, core transcribes with large-v3-turbo if installed, else small
    (UPSTREAM-PATCHES #33). The fallback uses the user's language from before Hinglish
    (auto-detect by default), not Hindi. A started recording says so once, and the
    finished meeting says which model ran (D83). A missing file never blocks the other
    downloads.
  - While it is selected, the transcription language is `hi`. Auto-detect would let it
    decide per window, and its output is Devanagari anyway. `save` remembers the
    language from before (`language-before-hinglish` next to config.toml), and gives it
    back when another model is chosen.
  - Rolling transcription is off with it (`corecfg::rolling_supported`). Rolling pieces
    must come from the final pass's model, and this one cannot keep up live.
  - The live transcript uses Balanced (`small`), as with Best: the model is large-v3
    sized, much slower than turbo. It runs with the user's language, not `hi` (D83).
- Core needs no patch: it loads `models_dir/ggml-<model>.bin` for any name
  (UPSTREAM-PATCHES #32 asks for a model file path and download spec).

### D82. Meeting chunks: speech threshold 2.5, 300 ms padding, no text conditioning
- New fact (AMI ES2004a and TS3003a, 27 Nov 2026): core's meeting chunker, which decides
  which audio Whisper sees, dropped 5.9% / 15.8% of the reference speech. Quiet speech
  stayed under 4 × the noise floor, and word onsets fell outside the chunks. With 2.5 and
  300 ms of padding it drops 0.2% / 4.3% (a Node port of the chunker, scratchpad
  chunkprobe). Core got both as settings, defaulting to upstream (UPSTREAM-PATCHES #34).
- Measured, ES2004a / TS3003a:

  | variant | ES2004a WER | TS3003a WER | line DER strict ES/TS | RTTM strict ES/TS |
  |---|---|---|---|---|
  | core defaults | 26.6% | 26.2% | 61.4/60.5 | 32.2/33.3 |
  | chunk 2.5/300 | 25.1% | 21.8% | 64.3/52.6 | same |
  | chunk 3/300 | 25.6% | 22.6% | 65.2/56.6 | same |
  | ctx0 | 23.4% | 25.5% | 52.7/53.7 | same |
  | chunk 2.5/300 + ctx0 | 21.8% | 20.5% | 52.6/52.7 | same |

  "chunk m/p" is `meeting_chunk_noise_multiplier` m and `meeting_chunk_pad_ms` p; "ctx0"
  is `n_max_text_ctx = 0`. The ctx0 rows ran on the build before the round-4 fixes, the
  chunk rows on 1e32585. RTTM DER comes from diarization alone, which these settings
  don't touch.
- Pick: **2.5 / 300 ms and `n_max_text_ctx = 0`**, the best row on both meetings. 3/300
  keeps less quiet speech for no gain. Conditioning off stops one bad window from
  repeating into the next ones.
  - `corecfg::pin_measured_transcription_defaults` replaces only core's own defaults, so
    a value set in config.toml stays.
  - Revised after the final review (28 Nov 2026): whisper.cpp feeds the initial prompt
    through the same buffer as the earlier text, so `n_max_text_ctx = 0` also drops the
    prompt (UPSTREAM-PATCHES #25): the Settings vocabulary and the name and title hints
    never reached the batch or rolling passes. The AMI gain was measured without user
    vocabulary. So ctx 0 applies only while the user's own Vocabulary list is empty; with
    words in it, `n_max_text_ctx` stays unset (upstream) and the words reach Whisper. The
    check runs on every config load, so adding or clearing words takes effect at the next
    meeting. Earlier builds saved the 0 into config.toml, so a 0 counts as Margin's value;
    any other number is the user's and stays. Settings says so under the word list.
- Rolling pieces (D49) must keep the whole-file chunks. Core pads each piece's chunks
  inside the piece, so padding never pulls audio from the next piece, and at Stop core
  reuses a piece whose audio digest and settings match, without comparing chunks. A cut
  that a padded chunk would cross in the whole file therefore gives a transcript that
  differs from the non-rolling one near the cut: the text fits the piece's audio, but up
  to 300 ms of a word onset is missing. So `piece_cut` reads the multiplier and padding
  from the effective config, and cuts only at a reset point that padding reaches from
  neither side: at least 1.1 s from one chunk's voiced end to the next chunk, against
  800 ms before. A 45 s split is never a cut with padding. Where there is no such point,
  the existing quietest-window fallback applies, which was never exact. The parity tests
  compile core's chunker and padding from the vendored source and check both 4.0/0 and
  2.5/300.

### D83. Hinglish: which language each model runs with, and what the notices say
- New fact (review of D81, 27 Nov 2026): core read `transcription.language` (`hi`) where
  the Hinglish model never runs: the live transcript, which uses `small`, and the process
  trace of a meeting that fell back. The notice at Start named the fallback by Margin's
  rule (the first non-empty file), but core skips a fallback that fails to load. And a
  broken Hinglish file of 500 MB or more passes Margin's size check, so core fell back
  with no word to the user.
- Live transcript with Hinglish selected. Options: (a) `hi`, as the batch pass; (b) the
  user's language from before Hinglish (auto-detect by default); (c) always auto-detect.
  Pick **(b)**. `small` forced to `hi` writes poor Devanagari, and Margin's live view is
  a draft that the batch pass replaces. (c) would detect Hindi windows and do the same.
  The cost: Hindi speech shows as rough text in the live view until the final
  transcript is in.
- Rule in core (UPSTREAM-PATCHES #33): while `missing_model_fallbacks` is set, which
  Margin does only with Hinglish, `language` belongs to `transcription.model` alone. Every
  other whisper model, a fallback or the live model, runs with
  `missing_model_fallback_language`. So the batch pass keeps `hi` with Hinglish, and a
  fallback, the live transcript, dictation and the process trace get the user's language. With
  every other model the fallback list is empty and core behaves as upstream.
- Notices:
  - At Start, with the file missing or under 500 MB: "Hindi + English model file is
    missing or unreadable", "This meeting uses another installed model, language
    <name>." It names no model, since only core knows which one loads.
  - After processing: core writes `{step: transcribe, reason: model_fallback, message:
    "used <model> because hinglish missing|empty|failed to load"}` into the meeting file.
    With "Meeting ready", Margin shows "Hindi + English model failed to load" (or "file
    was missing", "file was empty") and "This meeting used <model>." That covers a large
    broken file. Like "Meeting ready", it comes only when the job enters a finished state,
    not again when a finished job's view changes (a retitle).
- Summary and title: with summary language "auto" they follow the transcription
  language, so a Hinglish meeting is summarized in Hindi. After a fallback they follow
  the language the fallback ran with, read from the meeting's `model_fallback` record
  (core's pipeline summary and title, core's resummarize, which Margin's summaries use,
  and Margin's title step in `summaries.rs`).

### D90. Lines Whisper makes up on silence are removed with the prompt echoes
- New fact (Grug round H2): a 3-second near-silent recording came back as `[SPEAKER_1 0:00]
  [SYSTEM] Продолжение следует...` ("To be continued") and `...of royal descent in zoos across
  the world.`. The meeting got an invented title, a project page and a summary.
- Options: a phrase list only (misses what nobody lists), a model that judges each line (slow,
  and it would run on speech too), or the scrub that D71 already runs, with a few rules.
- Pick: the D71 scrub also drops a line that is only markers (`[BLANK_AUDIO]`, `(music)`), holds
  a known silence phrase (about 50, in 12 languages: "Thanks for watching", the Amara subtitle
  credit, "Продолжение следует", ...), is a stray line in a script the rest of the transcript
  does not use, or is a `...` fragment left in a transcript of 12 words or fewer after something
  else went. "Thank you", "bye" and one-word lines go only when nothing else real is left.
- The `[SYSTEM]` and `[MICROPHONE]` tags are core's capture-source tags on call lines, so a
  line is judged by what follows the tag; tagged lines are not removed as a class.
- Indic scripts (Devanagari, Kannada, Tamil, Telugu, ...) are mixed into English speech by real
  speakers, so they are never foreign.
- Revised after the final review (28 Nov 2026): the first rules deleted real speech. A phrase
  matched anywhere in a line, so "Thank you for listening, any questions?" went, and up to one
  line in ten in another script went, so five Russian lines in a 50-line meeting, or one Hindi
  sentence in 20 lines, were lost. Now:
  - a phrase must be the whole line (after the `[speaker time]` prefix, punctuation, case,
    numbers and spaces are set aside), so the list holds whole subtitle credits;
  - a line in another script (not Indic, not Latin) goes only when it is the meeting's single
    such line, has at most 6 words, and the meeting has at least 10 other lines;
  - every removed line is appended, under a timestamp, to `.<meeting file>.scrubbed.txt`
    beside the meeting (0600; Delete takes it too, D10), and a `processing_warnings` entry
    (`reason: lines_removed`) gives the count. A wrong guess is recoverable.
  - The scrub runs once, when a job has just finished. It never runs on meetings that were
    already done, such as the archived jobs that the first poll after launch sees.
- Cost: a real "Thank you" as the only thing said is dropped (kept in the sidecar). Whisper
  hallucinations that are plain English sentences in a longer recording (a YouTube-style
  sentence in a 40-minute call) are not seen, nor are credits that are not on the list.

### D91. A meeting with no speech says so, and gets no title or summary from nothing
- When the scrub removed lines and no words are left, Margin writes `status: no-speech` (the
  library already shows that), a `processing_warnings` entry (`step: transcribe`, `reason:
  no_speech`), and, for a meeting that had no title at Stop, the title "No speech recorded" with
  its entities cleared. A title that the user chose stays. No summary or title model runs.
- The scrub's count gates this, so a transcript that was already empty for another reason is
  not touched here. A meeting core already marked `no-speech` also gets no summary.

### D92. A title is the event's, the user's, or the summary's, and never ends in "..."
- Verified by reading: Start notes (D67) records under the calendar event's title, so the
  job has a title at Stop and keeps it. Core titles an untitled meeting with the first words
  spoken and cuts it at 57 characters plus "..." (`finalize_title`).
- Renames are not tracked anywhere: `set_title` writes the frontmatter. D59's guard was "the
  title still equals what Stop left", and it covered only the first summary after processing.
- Pick: a title is replaced from the summary after the first summary (and, until the
  revision below, after *Summarize again*), when it is the Stop fallback, core's 60-character cut, or two or more words that are
  the opening words of the transcript. A calendar meeting is never replaced, and the title is
  checked again after the model runs. Margin cuts a title that core ended with "..." at a word
  boundary (also when the summary engine is off), and also when the library shows it.
- Revised after the final review (28 Nov 2026): only the automatic first summary of a newly
  finished meeting replaces a title. *Summarize* and *Summarize again* never do, since the
  user may have typed a title that matches the opening words since.
- Cost: a user title that was typed as the opening words of the speech before the first
  summary is taken for the machine's. Prompt change not made: the summary prompt is core's (`build_base_system_prompt`)
  and core already asks for a TITLE line, which `resummarize` keeps out of the file; the title
  call (D59) stays the way to get one.

### D93. Search tries the closest library word when nothing matches
- New fact (Grug round L3): "warrnty" found nothing. Margin's search is its own substring scan,
  not core's index, so the fallback is Margin's.
- Pick: with no hits, each query word of 3 letters or more that is not a word in the library is
  replaced by the closest library word within 2 edits (1 for words of 3 or 4 letters; a swap of
  two letters is one edit; ties go to the more frequent word). The search runs again with that
  query. The answer is `{ "hits": [...], "corrected": "warranty" | null }`.
- This changes `search_meetings` from an array to an object. The window reads `hits`, and shows
  "Showing results for “corrected”" when `corrected` is set. The extra pass reads every file
  once more, and runs only on a search with no hits.

### D94. A model's lead-in line is removed from the summary
- New fact (Grug round L6): a summary began "Based on the transcript provided, here is a
  structured summary of ...". The model answered in free text, and core stores that as
  `## Summary`. The prompt is core's, so Margin does not edit it.
- Pick: after each summary Margin removes the first line of `## Summary` when it reads as an
  assistant's lead-in ("Based on the transcript...", "Here is...", "Sure..." and names a summary
  or overview), and a last line that offers more ("Let me know if..."). Nothing else in the
  summary changes.


## minutes-core with Margin's patches (27 Nov 2026)
