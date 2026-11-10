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
  file's lock during the rewrite, so notes typed meanwhile are saved after it.
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
