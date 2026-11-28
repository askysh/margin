# Changelog

All notable changes to Margin are in this file. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Decision numbers (D1, D2 and so
on) point to [DECISIONS.md](DECISIONS.md).

## [Unreleased]

### Added

- *Start notes from my calendar* in Settings → Recording. When an event with a call link or
  two or more people starts, Margin offers Start notes, in a banner and in a notification.
  It opens the call and records it under the event's title. Margin asks to read the
  calendar when you turn it on. Meetings then get the event's title and attendees (D67).
- A warning when the microphone records nothing. Thirty seconds into a recording, Margin
  checks the microphone track and says so when it is silent, for example with a
  Bluetooth headset that a call app holds (D68).
- *Answer without thinking first* in Settings → Summaries, for Ollama. Models such as
  qwen3 now finish a summary inside the 2-minute limit. On by default (D69).
- Settings → Knowledge: keep core's knowledge wiki after each meeting, with the meeting's
  people and, when Margin writes a summary, its decisions and action items. No model
  writes the wiki. Also a command to run after each meeting, with *Use Margin Desk* (D70).
- *Speaker model* in Settings → Transcription: *Better* tells similar voices apart. In a
  test with four people it found all four and gave a third as many wrong speaker labels,
  and it takes about 7 minutes per hour of meeting instead of 2 (D72).

### Fixed

- *Summarize again* could replace a title you typed when it matched the first words
  spoken. Only the automatic first summary of a new meeting names it now (D92).
- Words in Settings → *Names and words to spell right* never reached Whisper: the measured
  "no text conditioning" setting also drops Whisper's prompt. It now applies only while
  the word list is empty (D82).
- Margin rewrote finished meetings at every launch: the silence scrub ran again on each
  meeting it had already processed. It now runs only on a meeting that just finished (D90).
- The silence scrub deleted real speech: a line that merely contained "Thank you for
  listening", several lines in Russian, or a Hindi sentence in an English meeting. A
  phrase must now be the whole line, Indic scripts are never removed, and at most one short
  stray line in another script goes. Every removed line is kept in a hidden
  `.<meeting>.scrubbed.txt` beside the meeting (D90).
- Notes typed after Settings, or after *Change microphone*, during a recording were lost.
  The notes now take the keyboard back, and a letter typed with nothing focused goes to them
  (D73).
- A refused *Run this command* stays in the box, marked invalid, instead of being cleared
  (D75).
- A speaker core could not match shows as *Unknown speaker*, not "UNKNOWN" (D76).
- Setup now offers *Start notes from my calendar* and points to Margin Desk in Settings →
  Knowledge. Setup and Settings say that ⌘⇧R works in Margin and ⌃⌥⌘R in any app (D74).
- Ask says that it always uses Claude Code, whatever the summary engine is (D77).
- Folders show `~` for the home folder wherever it is (D78).
- The sidebar says *Showing results for "…"* when search used a corrected spelling (D79).
- Lines that Whisper made up from its own prompt during room noise, such as "Preserve
  spelling exactly when heard", no longer appear in the transcript (D71).
- Lines that Whisper makes up on silence, such as "Продолжение следует..." or "Thanks for
  watching", are removed from the transcript. A recording with nothing real left is marked
  *no speech* and named "No speech recorded", instead of getting an invented title and
  summary (D90, D91).
- Meeting titles no longer end with "...": they are cut at a word. A meeting named from the
  first words spoken is renamed from its summary, also after *Summarize again*, unless it
  came from your calendar or you named it (D92).
- Search finds "warranty" for "warrnty": when nothing matches, it looks for the closest word
  in your meetings and says what it searched for (D93).
- A summary no longer opens with "Based on the transcript provided, here is..." (D94).

## [0.0.11] - 2026-11-17

### Added

- Margin Notes for Windows, a first build: record a meeting in the room, and get the
  transcript and the meeting file. It uses the graphics card for the transcript, and
  needs Windows 11. *Use the graphics card* in Settings → Transcription turns that off.
  Setup and Settings say when the meetings folder is in OneDrive. It cannot record a
  call yet (D61, D62, D63).
- `MarginNotes-setup-<version>.exe`, an installer for Windows. The copy that it installs
  updates itself, as the Mac app does. The installer is not signed, so Windows SmartScreen
  warns at the first install: select *More info*, then *Run anyway* (D65, D66).
- [PRIVACY.md](PRIVACY.md), the privacy policy.

## [0.0.10] - 2026-11-11

### Added

- View → "Enter Notepad View" (⌘⇧N), a button in the title row and "Open as Notepad" in
  the menu bar make the window narrow, beside your call. The same menu item or button
  puts the window back at its earlier size and place (D57).
- A meeting shows when the recording started and stopped, for example "11:31 AM –
  12:24 PM", and when it was ready, for example "Ready at 3:00 PM • 2h 36m after Stop".
  An older meeting without these times shows the start time as before (D58).
- While a meeting is processing, its page shows when you pressed Stop and how long ago
  that was (D58).

### Fixed

- The traffic lights, the sidebar button, Record call and Settings now sit on one line
  in the title row (D57).
- The meeting header uses one kind of separator, and a wrapped line no longer ends in a
  separator.
- The sidebar shows "9:00 AM · 32m" with a space on both sides of the dot.
- Notes on the processing page no longer show a rule above the first line.
- A meeting longer than 99 minutes shows every transcript line after 99:59 as its own
  line. Before, those lines joined the line before them.
- Move to Trash and Reset now also move the hidden backups that "Summarize again" keeps
  beside a meeting. Each one is a full copy of the meeting (D10).
- A failed Summarize no longer leaves "Summarizing…" on the meeting.
- Notes no longer change under the caret after a save. Before, the meeting view could
  replace them with the saved copy, without the spaces at the end, and move the caret.
- Action items cannot be ticked while Margin writes the summary. Before, a tick at that
  moment could land on another item of the new summary.
- Notes typed in a meeting whose processing failed after the transcript was saved now
  go into the meeting file too. Before, they stayed in a draft that Reset deleted.
- Notes typed while Margin writes a summary are kept when you quit. They wait in a
  draft and go into the meeting when the summary ends, or at the next launch (D40).
- "Restart to update" asks first while a meeting is still transcribing, as Quit does
  (D55).
- After Settings refuses a change, later changes save again. Before, each later save
  sent the refused change too and failed.
- Notes with a code block that holds a `## ` line no longer repeat their text at each
  save.
- The audio player no longer says the recording can't be played after two quick clicks
  on transcript times.
- The notes box grows when the window gets narrower, so the end of the notes is no
  longer cut off.
- A long meeting title in the sidebar ends in "…" in place of a hard cut.
- A summary heading with a Unicode line separator no longer freezes the window.
- Copy, More and the Record menu close each other, so only one is open at a time. Esc
  closes the Record menu too.
- Ask no longer shows the previous meeting's conversation after a fast switch between
  meetings.
- A model download that stops receiving data fails after a minute with a message, in
  place of waiting for ever. A server that does not send the file size no longer makes
  a good download fail.
- A failed save, for example on a full disk, no longer leaves a hidden `.margin-tmp`
  file beside the meeting.
- When Stop cannot queue a recording, the message names the folder that has the audio.
- Settings → Voices → Forget asks first, and says so when it fails.
- Tab stays inside the Reset dialog.
- The log no longer warns about the Screen Recording permission at each call Start.
  Margin records the call audio with System Audio Recording, which it checks itself.
- A meeting with no title gets one from its summary, in place of the first words of the
  transcript. The file keeps its name (D59).
- Settings in the notepad view: the traffic lights no longer cross the border of the icon
  list.
- A title typed during a recording, or a Settings change, no longer makes the wait after
  Stop longer. Margin transcribes the earlier pieces again during the meeting (D60).

## [0.0.9] - 2026-11-10

### Added

- Settings → General → Privacy → "Hide Margin from screen sharing". When it is on, people
  on a call do not see Margin when you share your screen, and screenshots leave Margin
  out. It is off by default (D54).

### Changed

- Settings → Recording → "Live transcript" can use Apple Speech in place of Whisper:
  Hindi and English, English (India) or English (US). Apple Speech runs on the Neural
  Engine, so the live transcript no longer drops lines while Margin transcribes an earlier
  meeting. Hindi and English writes the Hindi in Latin letters. It needs macOS 26.
  Whisper stays the default, and the final transcript does not change (D56).
- During a recording, the Margin window hides and a small recording chip shows the time at
  the right edge of the screen. Point at it for Stop and Notes. Stop needs two clicks.
  Click the chip to go back to your notes, and drag it up or down to move it. Settings →
  Recording → "Show the recording chip" turns it off (D53).
- With the recording chip on, a recording started from the menu bar, the global shortcut
  or a notification does not bring the window forward (D53).
- "Transcribe during the meeting" works with the Best model too. An 80-minute meeting with
  Best is ready about 3 minutes after Stop, not about an hour. On a MacBook Air M4 it uses
  about the same battery as Balanced (D52).
- With Best, the live transcript uses Balanced in place of Fast, which is more accurate.
  Margin downloads Balanced (488 MB) in the background after the update. Until it is
  there, the live transcript uses Fast, and recording does not wait for it (D52).
- Settings → Transcription says what to do for meetings that mix two languages, such as
  Hindi and English: keep Auto-detect and use the Best model.
- When the speech models are ready, Settings shows their size on this Mac without a full
  progress bar.

### Fixed

- Quit asks first while a meeting is still being transcribed, and names the meeting. Keep
  Running is the default. Before, a Quit could stop the transcription, and Margin started
  it again from the beginning (D55).
- A summary by Claude no longer shows the line "Agent input coverage: …" at its end. The
  meeting file keeps it. When part of the meeting did not reach the summarizer, the
  meeting shows a short note instead.
- Text that scrolls under the Ask bar fades out, so no words show between the bar and the
  bottom of the window.
- The red recording dot sits next to the timer.
- A finished transcript no longer fades in each time you open the meeting. Only new live
  lines do.
- A long folder path wraps as a whole and does not break in the middle of a word.

## [0.0.8] - 2026-11-09

### Changed

- Ask is a bar at the bottom of the window. Click it or press ⌘J, and it opens upward. An
  answer keeps going when you close it (D50).
- The window can be as narrow as 440 px, to sit beside a call or a browser. A narrow
  window shows the sidebar as a drawer, and a meeting shows Notes or Transcript, picked
  with a switch (D51).
- ⌘\ hides and shows the sidebar.

### Fixed

- In a window under 900 px wide, the home page text no longer gets squeezed into a narrow
  column.

### Development

- TypeScript 5.9 checks the UI.
- The e2e pass opens Ask from its bar, and checks a narrow window and the sidebar toggle.

## [0.0.7] - 2026-11-08

### Fixed

- Transcribe during the meeting gives the same transcript as transcription after Stop.
  Each piece now ends where core's chunker starts a new chunk. Before, about 12% of the
  words on a long call were different, and words at the cuts were lost (D49).

### Development

- Every change goes into the `next-release` branch, Dependabot's updates too. Merging
  `next-release` into `main` tags the version and drafts the release.
- `scripts/bump-version.sh` runs on Linux too, and dates the section of the version that
  is released.

## [0.0.6] - 2026-11-07

### Development

- Vite 8 and @vitejs/plugin-react 6 build the UI. Playwright 1.64 runs the e2e pass.
- A merge into `main` releases its version: the release workflow tags the merge commit,
  builds and notarizes the app, and drafts the release with the version's CHANGELOG
  section as its notes. Work for a release goes into a `release/X.Y.Z` branch.
- `scripts/bump-version.sh X.Y.Z` sets the version in every file that holds it.
- CI checks that the versions agree, and that a release pull request has a new version
  and release notes.
- Dependabot groups its updates, and opens no major update for React or TypeScript.
- The workflows use actions/checkout v7 and actions/cache v6.

## [0.0.5] - 2026-11-07

### Added

- A menu bar item, **Settings…**.
- A permission error has an **Open Permissions** button that opens Settings → Permissions.
- Settings → About shows the Margin version and the minutes-core version.
- `scripts/make-icons.swift` (`bun run icons`) draws every icon from one source.

### Changed

- Settings is a page in the main window, in place of a sheet. A list of sections is on the
  left: General, Recording, Transcription, Summaries, Voices, Permissions, Integrations,
  Updates, About and Start over. *Back* or Esc returns to the meeting, search and scroll
  position you left, and ⌘, opens the section you used last.
- ⌘⇧R and ⌘⇧I start a recording from Settings, and ⌘J and ⌘F work there too.
- A new app icon: a sound line in the margin crosses the rule and becomes handwriting. The
  menu bar icons match it, and the logo shows in setup, on Home and in the sidebar.
- minutes-core comes from upstream `silverstein/minutes` again. Upstream merged Margin's
  transcript checkpoint patch
  ([silverstein/minutes#1316](https://github.com/silverstein/minutes/pull/1316)), so
  Margin no longer uses the `askysh/minutes` fork. minutes-core is pinned by commit
  `209265f` until upstream tags a release that contains the patch (D48).
- `scripts/bump-upstream.sh` takes an upstream tag or commit.
- The repository docs: a new README, CONTRIBUTING.md, CODE_OF_CONDUCT.md, AGENTS.md,
  this changelog, issue and pull request templates, and a docs map in
  [docs/README.md](docs/README.md).

### Fixed

- A name or word list typed just before you leave Settings is saved.
- In the Reset dialog, Esc closes only the dialog, and focus returns to *Reset Margin…*.

## [0.0.4] - 2026-11-06

### Added

- Rolling transcription. With Fast or Balanced, Margin transcribes the meeting in pieces
  of about 2 minutes while it records. After Stop, only the last piece or two is left.
  On a 60-minute test call, the transcript was ready 6.5 s after Stop, against 236 s
  before (D47).
- Settings → Recording → "Transcribe during the meeting", on by default. With Best it is
  off.

### Changed

- minutes-core comes from the `askysh/minutes` fork: v0.32.1 plus one patch for transcript
  checkpoints, pinned by commit. The same patch went upstream as a pull request (D46).

## [0.0.3] - 2026-11-06

### Changed

- After Stop, the processing view shows the live transcript at once, as a draft (D44).
- The meeting opens as soon as minutes-core writes its file, before speakers are
  separated. It fills in when the job completes (D42).
- Notes stay editable the whole time. During processing they go into the job's notes
  draft (D43).
- The Homebrew tap workflow closes the open tap pull requests of older Margin versions.

## [0.0.2] - 2026-11-06

### Changed

- A meeting opens before its summary. Margin writes the summary after the meeting is
  ready and shows that it is writing it. Claude gets 15 minutes for a summary, up from 5
  (D40).
- The live transcript uses the chosen model, so it is no longer empty when the `base`
  model is not downloaded (D41).

### Fixed

- The Homebrew tap workflow trusts the tap before it loads the cask.
- Quitting no longer warns about an update when nothing is downloaded.

## [0.0.1] - 2026-11-06

The first release.

### Added

- A notepad-first meeting app on minutes-core v0.32.1: record a call (microphone and
  system audio) or an in-person meeting, with a live transcript beside your notes and
  You / Them labels in calls.
- Processing in a background worker: transcription, speaker separation, an optional
  summary, and a Markdown file in the folder you choose.
- Setup in five steps, each with a default.
- The meeting view: editable notes and title, speaker rename with voice recognition,
  action items, Copy as Markdown, Summarize again, and playback from any transcript time.
- The library, grouped by date, with search over titles, notes and transcripts, and a
  people filter.
- Ask: questions about one meeting or all of them, through your own `claude` CLI, with
  read-only access to the meetings folder.
- Settings: microphone, stop after silence, call suggestions, open at login, meeting
  language, speaker separation, names to spell right, summary language and instructions,
  and how long to keep audio.
- Reset, which keeps meetings, models and voices by default.
- The menu bar with the recording time and recent meetings, notifications, sound cues,
  and an optional global shortcut, ⌃⌥⌘R.
- A release signed with a Developer ID and notarized by Apple, a DMG, a Homebrew cask
  (`brew install --cask askysh/tap/margin`) and in-app updates (D38, D39).
- A security policy.

[0.0.5]: https://github.com/askysh/margin/compare/v0.0.4...HEAD
[0.0.4]: https://github.com/askysh/margin/compare/v0.0.3...v0.0.4
[0.0.3]: https://github.com/askysh/margin/compare/v0.0.2...v0.0.3
[0.0.2]: https://github.com/askysh/margin/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/askysh/margin/releases/tag/v0.0.1
