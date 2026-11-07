# Progress

## Start here (afternoon of 2026-11-06)

New today: settings and app parity with Minutes, Ask (chat with Claude), faster list,
search and idle use, a shorter onboarding, and a signing pipeline for v0.0.1. The repo
is private at https://github.com/askysh/margin.

Real microphone and system-audio capture still need you (macOS asks a person). Try this,
about 10 minutes. Quit the Margin.app that is open now first, and quit Minutes.app so
the two don't record at once.

1. Open the new build (see "Release" below for the signed one), or
   `scripts/build-app.sh && open src-tauri/target/release/bundle/macos/Margin.app`.
2. Onboarding is five steps, and Return accepts each default. On **Setup** pick the
   Obsidian vault or Documents. The model download keeps going while you finish.
   **Allow** both permissions. Pick **Claude** for summaries.
3. **Record in person** (⌘⇧I) for 30 s. Type notes, Stop. Expect live lines, the red
   rule moving with your voice, then the finished meeting with your notes.
4. **Record call** (⌘⇧R) with a video playing. Live lines should alternate You / Them.
5. In the meeting: press play, click a transcript time, tick an action item, use
   **Copy**, rename a speaker. Press **⌘J** and ask "What did I commit to?".
6. Settings: choose a microphone, add names to spell right, try **Reset Margin…**.
7. Optional: turn on the ⌃⌥⌘R shortcut and "Open Margin when you log in", then check
   that both work.

If capture fails, the toast says why, and `~/Library/Application
Support/Margin/state/logs/margin.log` has the details.

## 2026-11-06

### M0: Phase 1 research (done)
- RESEARCH.md (now [docs/research.md](research.md)), DECISIONS.md and UPSTREAM-PATCHES.md written. The detailed surveys are in
  `docs/research/`.

### M1: Rust backend on minutes-core v0.32.1, verified headless (done)
- The Tauri v2 crate in `src-tauri/` builds against `minutes-core` as a git dependency.
  Two things make that work: `Cargo.lock` is seeded from upstream's lock, and `ort` is
  pinned to `=2.0.0-rc.10`.
- State is isolated with `MINUTES_CONFIG_PATH` / `MINUTES_DATA_DIR` / `MINUTES_HOME`.
  Everything lives under `~/Library/Application Support/Margin`.
- I checked each step with `margin --dev …` in a sandbox `MARGIN_HOME` and `HOME`:
  - Model download: whisper small, Silero VAD and the two diarization ONNX files. That is
    523 MB in about 85 s.
  - Processing a 39 s, two-voice WAV through `jobs::process_pending_jobs` (transcribe,
    diarize, save) took 10 s.
  - Library read: frontmatter, sections, transcript turns and speakers.
  - Rename `SPEAKER_2` → Priya updated the transcript, `speaker_map`, `attendees`, the
    overlay row and a voice sample.
  - Re-processing the same audio auto-labelled Priya (`source: enrollment`).
- Unit tests pass (6): notes splice, transcript parsing, rename and speech stats.
- Not yet run live: mic and system-audio recording, which needs the app bundle for TCC
  prompts. Also not yet run: the UI.

### M2: UI and real-backend QA (done, overnight 2026-11-06)
- The steno-pad UI covers onboarding, the live notepad, library and search, the meeting
  view with editable notes and title, speaker rename, settings, Reset, and the call-app
  banner.
- An independent review of the untested paths found 9 problems; all 9 are fixed:
  - You/Them window anchoring
  - stop/quit and reset races
  - stale live lines
  - leftover captures
  - main-thread stalls
  - draft handling on retry
  - temp-file collisions
- The end-to-end QA harness (`scripts/e2e/run.mjs`) runs the real UI in headless WebKit
  against the real backend in a sandbox. **29/29 checks pass** (31/31 after M3):
  - Onboarding writes explicit config: summaries none, calendar off, your name.
  - A simulated call recording shows live You/Them lines from the real stem heuristic.
    Notes typed right up to Stop are kept. Processing runs in the worker child. The UI
    lands in the finished meeting with the notes exact.
  - A recording queued while the app is open is processed. Notes are spliced in
    verbatim, including a `## ` heading demoted to `###`.
  - Notes editing saves with the frontmatter byte-identical. List continuation works.
  - Renaming a speaker updates transcript labels, summary mentions, action-item
    assignees, commitments, attendees and `speaker_map`, and saves a voice. The next
    meeting is auto-labelled.
  - Renaming a meeting title works. Silent recordings explain themselves.
  - Search, and the no-results message.
  - The full summary flow through Settings → Summarize, using a stand-in Ollama.
  - A real model switch downloads 148 MB with progress.
  - Reset keeps meetings and models, onboarding runs again and the meetings are back.
    Delete requires typing "delete".
  - No UI errors or failed commands.
- Summaries through the real Claude CLI were checked headlessly with Akash's login: 10 s
  for a 39 s meeting.
- Real Ollama (qwen3.5:4b) times out: it is a thinking model, and core has a fixed 2-minute
  limit (UPSTREAM-PATCHES.md #3). The plumbing itself works against a stand-in server.
- Recording preflight on this Mac passes for both modes. The call route is the built-in
  mic plus the Core Audio process tap, with system audio ready.
- **Still not verified:** real microphone and system-audio capture. It needs a person to
  answer the macOS permission prompts, and is the first thing to try in the morning.

### M3: second review and hardening (done, overnight 2026-11-06)
- A second independent review of the frontend state logic and the newer backend found
  3 high, 4 medium and 6 low issues. All are fixed. In short:
  - Notes can no longer be lost:
    - Live notes survive leaving and coming back to a recording.
    - Debounced saves are flushed on unmount.
    - Note saves run in order behind an edit counter.
  - Meetings that core is still enriching are read-only. Editing them made core's
    enrichment step fail.
  - Every way a recording can stop (window, menu bar, auto-stop) navigates through a
    `recording-stopped` event that carries the job ID.
  - Writes are safer:
    - A per-file lock covers notes, title and speaker rename.
    - Settings saves run one at a time.
    - Restoring a draft at launch never overwrites notes edited in Obsidian meanwhile.
  - Quit never blocks the main thread while a recording finishes saving.
  - Recordings under a second are discarded instead of leaving files in the vault.
- e2e now has 31 checks, including two that reproduce the review's scenarios: a quick
  switch right after typing, and leaving and returning to a live recording. An in-person
  recording shows no You/Them labels.
- Load test: 600 one-hour meetings (40 MB). The library list takes 0.1 s, and a full-text
  search 0.1–0.2 s (debug build).

### Settings parity with upstream (branch t3/settings-parity-upstream)
- `docs/parity/settings.md` compares every upstream setting with Margin.
- Ten settings added. Each is checked end to end in a sandbox:
  - Keep audio: a 40-day-old meeting loses its audio, and the recent audio stays.
  - Microphone: `--dev preflight` uses the chosen mic for in-person meetings and calls. An
    unplugged mic falls back to the default.
  - Stop after silence and call suggestions. A stand-in `zoom.us` process shows the
    banner only while the switch is on.
  - Open at login: the LaunchAgent is written, passes `plutil -lint`, and Reset removes
    it.
  - Meeting language and speaker separation: a meeting processed in English with speaker
    separation off has a transcript and no speaker labels.
  - Names to spell right reach core's `vocabulary.toml`.
  - Summary language and instructions reach the summarizer's prompt.
- e2e: 39/39 in light mode and in dark mode.
- Not verified: a real recording with a chosen mic (needs the permission prompt), and a
  real login.

### App parity with upstream Minutes (2026-11-06)
- `docs/parity/app.md` compares each upstream feature outside Settings with Margin.
- Added from upstream: action-item checkboxes, Summarize again, Copy as Markdown (plus
  summary, notes and transcript on their own), "meeting ready", "processing failed" and
  "call app active" notifications, and the four sound cues.
- Added for Margin only: playback with click-to-play transcript times, the people
  filter, the global shortcut ⌃⌥⌘R (off by default), and the menu-bar clock and recent
  meetings.
- e2e: 40 of 40 checks pass, in light and dark mode. The 9 new checks cover cues, the
  notification event, playback and byte ranges, copy, the people filter, both kinds of
  action items, Summarize again and the new Settings rows.
- Not verified (needs the GUI app): system notifications and their buttons, the global
  shortcut, the menu-bar clock and recent meetings, playback through the asset protocol
  in the app's WebView, and cues while the window is hidden.

### Ask: chat with Claude (done, 2026-11-06)
- Ask about one meeting or across all meetings: ⌘J, the sidebar, or the meeting view.
  The panel docks on the right. Decisions are in DECISIONS.md "Chat"; the comparison with
  upstream Recall is in docs/parity/chat.md.
- Backend: `src-tauri/src/chat.rs`. It runs `claude -p` with stream-json, read-only tools
  confined to the meetings folder (`dontAsk`), `--resume` for follow-ups, a process group
  for Stop, and history in `state/chat/`.
- e2e: 40/40 in light and dark mode. The 9 new checks use a stand-in claude
  (`scripts/e2e/fake-claude.py`): not signed in, a streamed answer with source links, a
  follow-up that resumes the session, Stop kills the process, a link opens the meeting,
  meeting context in the prompt, history after a reload, Clear, and Reset.
- Real smoke test (`margin --dev ask`, real login, sandbox folder of 5 test meetings):
  - Across all meetings, "What did I commit to, and what decisions were made?": first text
    after 2.4 s, done after 14.4 s. Claude read all 5 files. The answer was correct and
    linked 4 meetings.
  - A follow-up ("Draft a short follow-up email to Priya") resumed the session: done after
    6.2 s, with a ready-to-paste email.
  - One meeting, "What decisions were made?": first text after 2.2 s, 52 streamed updates,
    done after 5.7 s.
  - A request to read a file outside the folder was refused, and its content did not
    appear.
- The smoke test found a race (a finished answer was briefly missing from the history)
  and dropped trailing stream updates. Both are fixed.
- **Not verified:** Ask in the real app window (only the headless WebKit e2e and the dev
  CLI ran). The "not installed" state has no test; the "too old" flag check has a unit
  test only.

### M4: parity, Ask, performance, onboarding, release (done, 2026-11-06)
- Four agents worked in parallel worktrees: settings parity, app parity (meeting view,
  library, menu bar), Ask, and performance plus onboarding. All four are merged into
  `main`. The decisions are D13 to D37 in DECISIONS.md.
- e2e after the merge: 57/57 checks pass.
- A bug found only after the merge: the Settings sheet waited for the microphone list,
  and Core Audio can stall while it looks for devices (an iPhone's Continuity mic, for
  one). The lookup now runs on its own thread, and after 2 s the last list is used.
- The rarely used meeting actions (Show in Finder, Open in Obsidian, Move to Trash) are
  now under **More**, so the action line fits on one row.
- Release: version 0.0.1, `scripts/release.sh` (Developer ID, hardened runtime,
  notarization, stapling, zip and DMG), `.github/workflows/release.yml` (draft release
  on a `v*` tag) and `ci.yml` (UI type check and build on each push).


### M5: v0.0.1 released (2026-11-06)
- https://github.com/askysh/margin/releases/tag/v0.0.1: signed with the Developer ID,
  notarized and stapled (app and DMG). Install with
  `brew install --cask askysh/tap/margin` (askysh/homebrew-tap #8).
- Auto-update (D38): tested on this Mac from 0.0.1 to a local test 0.0.2: download,
  signature check, bundle swap, restart. The first update over HTTPS from GitHub runs
  with 0.0.2.
- On publish, `homebrew-tap.yml` installed the cask from the published DMG on a GitHub
  runner, checked `codesign` and Gatekeeper (`spctl`), and opened the tap PR, which
  merged after the tap's tests.
- Fixed: long transcripts and notes did not scroll, and pages ended 52px below the window.
- Next: faster processing after Stop for long meetings (research running).

### 0.0.3: ready right after Stop (Phase 1 of docs/research/live-transcription.md)
- After Stop, the processing view shows the live transcript as a draft at once.
- The meeting opens when core writes the file (`transcript-only`), before speakers are
  separated. It fills in by itself when the job completes.
- Notes stay editable the whole time. During processing they go into the job's draft.
- DECISIONS.md D42–D45. The e2e run has 62 checks.

