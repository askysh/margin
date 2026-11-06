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
