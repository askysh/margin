# Progress

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
