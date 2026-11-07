# Changelog

All notable changes to Margin are in this file. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Decision numbers (D1, D2 and so
on) point to [DECISIONS.md](DECISIONS.md).

## [0.0.6] - Unreleased

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
