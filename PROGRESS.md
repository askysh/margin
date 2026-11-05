# Progress

## Start here (morning of 2026-11-06)

What I could not do overnight is the one thing that needs you: real microphone and
system-audio capture, because macOS asks a person for permission. Everything else was
tested end to end (31/31). Suggested first run, about 10 minutes:

1. `scripts/build-app.sh && open src-tauri/target/release/bundle/macos/Margin.app`
2. Onboarding:
   - Your name.
   - Pick the **Obsidian: Workspace 42** option, or Documents.
   - **Balanced** model: a 523 MB download.
   - **Allow** both permissions.
   - Summaries: **Claude** (it found `~/.local/bin/claude`).
3. **Record in person** (⌘⇧I) for 30 s while talking. Type a couple of notes, then Stop.
   You should see:
   - live lines on the right
   - the red rule moving with your voice
   - processing, then the finished meeting with your notes.
4. **Record call** (⌘⇧R) with a YouTube video playing. Live lines should alternate
   You / Them.
5. Rename a speaker. Edit notes, then open the file in Obsidian. Settings → **Reset
   Margin…**.

If capture fails, the error toast says why, and `~/Library/Application
Support/Margin/state/logs/margin.log` has the details. Margin doesn't touch the upstream
Minutes app's settings, and the two can be installed side by side. Quit Minutes.app while
testing, so the two don't record at once.

## 2026-11-06

### M0: Phase 1 research (done)
- RESEARCH.md, DECISIONS.md and UPSTREAM-PATCHES.md written. The detailed surveys are in
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
