# Ready after Stop: where the time goes, and how to remove the wait

Research for one problem: after Stop, a long meeting stays in "processing" for a long
time. An 80-minute call took 39.6 minutes from Stop to Complete. The goal is a finished
meeting a few seconds after Stop, with transcripts that are at least as good as now.

Measured on 2026-11-06 on the owner's Apple Silicon MacBook Air, with minutes-core
v0.32.1. Load averages for each block of runs are in the tables. Other builds ran on the
Mac at the same time, so compare figures only inside one table.

## Summary

1. **Transcription is 82% of the wait.** For the real 80-minute call it took 32.3 of the
   39.6 minutes. The rest was the summary (5.0 min, a time-out), and two speaker passes
   (2.3 min).
2. **The live transcript did not work in that meeting.** The sidecar wrote 0 lines in 80
   minutes. The live model resolves to "base", and only "small" was installed. The fix is
   being made in main.
3. **Rolling transcription removes the wait.** If Margin transcribes the audio in 2-minute
   pieces during the meeting, 12 s of work is left at Stop for a 60-minute call (24 s
   expected on real audio). The quality is the same as the batch pass.
4. **Core already writes the transcript before the speaker passes and the summary.** Margin
   waits for Complete and shows only a step list. Margin can open the meeting at that
   point and update it in place.
5. **Core cannot take a transcript from the host.** Rolling transcription in the real job
   needs one upstream change: a job input for transcript checkpoints.

## How it was measured

- All runs used a sandbox `HOME` and `MARGIN_HOME` under `/private/tmp/claude-501/margin-live/`.
  No run captured audio or opened the app.
- The only real data read was `margin.log` in the app's log folder. `margin-worker.log`
  was empty. No meeting file was opened.
- Test audio: `docs/research/live-scripts/gen_corpus.py` makes a call with macOS `say`.
  The call has four voices, sentences from this repo's docs, and a mic stem ("You") and
  a system stem (three remote voices). A reference transcript comes with it. The 60-minute
  call has 8,412 words, and the 20-minute call is its first 20 minutes.
- `gen_hard.py` adds noise, mic bleed, background voices and short replies ("Yeah.").
  The bleed makes the mic stem transcribe the remote voices too, so its WER is 35%. That
  is not a useful baseline, and the tables below use the clean call.
- `eval.py` gives the word error rate (WER) of the time-ordered transcript. `stemwer.py`
  gives the WER of each stem against its own speakers.
- Whisper on Metal is not deterministic. The same input gave 8.2% and 9.0% WER in two
  runs, so a WER difference under about 1 point is noise.
- **Synthetic speech is easier than a real meeting.** On real audio, `small` needed 24 s
  of compute for each meeting minute. On the synthetic call it needed 5.4 s. The
  projections below use the real rate, and use the synthetic runs only for ratios.

New dev commands (research only, not in the app path):

| Command | What it does |
|---|---|
| `margin --dev process <wav>` | Now also queues the `.voice.wav` / `.system.wav` stems and prints the time of each job stage. |
| `margin --dev live-replay <wav> [speed] [model]` | Feeds a WAV through core's live sidecar (`live_transcript::run_sidecar_mpsc`). |
| `margin --dev rolling <wav> [segment-secs] [out.md]` | The rolling prototype (`src-tauri/src/rolling.rs`). |

## Time breakdown after Stop

### The real 80-minute call (from `margin.log`)

The recording ran 05:06–06:26 UTC (4,832 s). It was a call with mic and system stems, the
`small` model and the Claude summary.

| Stage | Time | Details |
|---|---|---|
| Transcribe the mic stem | 1,041 s | 286 VAD chunks, 286 whisper calls |
| Transcribe the system stem | 895 s | 431 VAD chunks, 431 whisper calls |
| **Transcription in total** | **1,938 s (32.3 min)** | 717 calls on 6,532 s of chunk audio. Median 1.28 s a call. |
| Speakers, run 1 (system stem) | 75 s | 25 s loading the audio, 19 s segmentation, 30 s embeddings and clusters |
| Summary (Claude CLI) | 300 s | Timed out. No summary. |
| Speakers, run 2 (mic stem, "is this you?") | 63 s | 24 s loading the audio again |
| Save | < 1 s | |
| **Stop to Complete** | **2,377 s (39.6 min)** | |

Two more facts from the log:

- **The slow calls carry most of the time.** The 100 slowest calls (14%) took 1,108 s
  (57%). These were 20–37 s each, on chunks of 4–43 s with few words: 6.6 s of audio and
  6 words took 37.7 s. That fits whisper's temperature fallback. Core decodes with
  `best_of 5` and `temperature_inc 0.2`, and each failed decode at temperature 0 retries
  at 0.2, 0.4 and so on. The synthetic audio never shows this tail.
- The live sidecar logged `lines=0 duration_secs="4832.0"`, and warned that
  `ggml-base.bin` was missing, on every utterance.

### Synthetic calls, `small` (sandbox, load 2.5–3.0)

| Stage | 20 min | 60 min |
|---|---|---|
| Transcription | 92 s | 325 s (234 calls, median 1.22 s) |
| Speakers, run 1 | 23 s | 64 s (18 s loading the audio) |
| Speakers, run 2 | 15 s | 45 s (18 s loading the audio) |
| Stop to Complete | 130 s | 435 s |
| WER | 8.2% | 8.5% |

### What core does after Stop

`jobs::process_pending_jobs` → `pipeline::transcribe_to_artifact` → for a call,
`transcribe_independent_stems` (pipeline.rs:2509). Each stem is transcribed by itself and
in sequence:

1. `detect_meeting_vad_chunks` (transcribe.rs:2364) cuts the stem at 800 ms of energy
   silence, into chunks of 1–45 s.
2. Each chunk is one whisper `full()` call (transcribe.rs:913). Every call makes a new
   whisper state (1658), so whisper.cpp also loads the Silero VAD model again. Every call
   pays a full 30 s encoder pass, even for a 1 s chunk.
3. `source_transcript::merge_sources` merges the stems as `[MICROPHONE m:ss]` /
   `[SYSTEM m:ss]` lines.
4. **Core writes the meeting file now** and sets the job to `TranscriptOnly`
   (jobs.rs:1710). The job's `output_path` is set.
5. `enrich_transcript_artifact`: speakers run 1 (pyannote on the system stem), the
   summary, speakers run 2 (pyannote on the mic stem, only to check the owner's voice
   profile), then the file is rewritten in place.

Whisper settings that config cannot change: `best_of 5`, temperature fallback, threads
(up to 8), `audio_ctx`, flash attention (off). Config can change `transcription.model`,
`transcription.vad_model` and `diarization.engine`.

## Options

### 1. Show the meeting at "transcript ready", and upgrade it in place

- **What:** open the meeting when the job reaches `TranscriptOnly`. A quiet line says
  "Finding speakers…" and then "Summarizing…". The open meeting reloads when the job
  completes. Until `TranscriptOnly`, the processing view shows the live transcript
  (after the live-model fix) in place of the bare step list.
- **Numbers:** the user stops waiting at the end of transcription, not at Complete. For
  the real call, that removes 6.3 of the 39.6 minutes from the wait (the summary and both
  speaker passes). With the live transcript, the user has text to read 0 s after Stop.
- **Quality:** none lost. The labels change from `MICROPHONE`/`SYSTEM` to speaker names
  when run 1 is done.
- **Effort:** small, Margin only (`App.tsx`, `ProcessingView.tsx`, `processing.rs`).
  - Notes: `processing::finish` puts the user's exact notes in the file only at a
    terminal state. Until then the file has core's cleaned bullets. The notes editor must
    not save over the file while core still has to rewrite it.
  - Speaker rename must wait for Complete.
- **Upstream:** none. Upstream's own app does the same: it writes the file at
  `TranscriptOnly` with an "Open Transcript" button.

### 2. Rolling transcription during the recording

- **What:** a background thread transcribes each stem in pieces while the meeting runs.
  A piece closes every N seconds of audio, at the quietest 100 ms of its last 15 s. Each
  piece uses core's public `transcribe::transcribe_meeting`, so the chunking and whisper
  settings are those of the final pass. At Stop, only the open piece of each stem is left.
- **Measured** (`margin --dev rolling`, 60-minute call, 120 s pieces, load 2.5–3.0):

  | | Batch after Stop | Rolling |
  |---|---|---|
  | Compute in total | 325 s | 399 s (+23%, a model load per piece) |
  | RTF during the meeting | — | 0.11 |
  | Transcription left after Stop | 325 s | **12 s** |
  | WER | 8.5% | 9.1% |

- **Quality:** the same, inside the run-to-run noise. A word diff against the batch
  transcript shows 141 scattered one-word changes in both directions ("diarization" /
  "diorization"). None is at a cut point.
- **Real audio:** the real rate is 24 s of compute for each meeting minute (RTF 0.40). So
  the thread keeps up, but the GPU is busy for 40% of the meeting, on top of the live
  sidecar. That costs battery, and heat on a fanless MacBook Air. The work left at Stop
  is the open piece:
  - 120 s pieces: 1 minute on average, which is about 24 s (48 s at most).
  - 60 s pieces: about 12 s (24 s at most), with more model loads.
- **Best model:** large-v3-turbo needs 4.2× the compute of `small`. On real audio that is
  an RTF of about 1.7, so the thread falls behind. Rolling works with Fast and Balanced
  only, unless the core patch in option 5 and the fallback cap make turbo fast enough.
- **Effort:** medium. The prototype is about 150 lines. The app needs:
  - the thread to tail the growing stems (`recorder.rs` already reads them for the live
    "You / Them" labels),
  - a checkpoint file next to the job,
  - and a hand-off to core.
- **Upstream:** needed for the hand-off. Core's job always re-transcribes the WAV
  (config.rs:1452: "The final transcript never comes from the sidecar"). The public
  `pipeline::write_transcript_artifact` + `enrich_transcript_artifact` accept host text,
  but that route drops the stem-aware speaker labels and bypasses the job state that
  Margin's UI follows. See patch 11 in UPSTREAM-PATCHES.md.

### 3. Reuse the live transcript as the final transcript

- **Measured** (`margin --dev live-replay`, 20-minute call fed at 3× real time):

  | Live model | WER | Lines | Peak memory | User CPU for 20 min of audio | Lag at 3× (median, p90) |
  |---|---|---|---|---|---|
  | `small` | 9.3% | 113 | 795 MB | 152 s | 2.6 s, 3.3 s |
  | `base` | 11.7% | 113 | 403 MB | 146 s | 1.3 s, 1.8 s |
  | `large-v3-turbo` | 21.8% | 81 | 1,927 MB | 275 s | 5.5 s, 11.6 s |

  The final pass on the same audio gave 8.2–9.0% (`small`), 11.3% (`base`) and 6.2%
  (turbo).
- **Quality:** on clean speech the live `small` text is close to the final `small` pass.
  But the live sidecar hears only the mixed signal. So its lines have no
  `MICROPHONE`/`SYSTEM` source, and the speaker passes cannot use the stems. Its offsets
  come from the wall clock, not from the audio. Turbo fell behind at 3× and lost lines.
- **Upstream:** core has no way to finish a job from the live JSONL.
- **Verdict:** use the live transcript as the draft that the user reads after Stop
  (option 1), not as the final text. Rolling transcription (option 2) gives the final text
  just as fast, per stem and with the final settings.

### 4. Use the stems in place of speaker separation on calls

- On calls, core already gets "You" against "Them" from the stems. On the synthetic call
  this was 99% right (`SPEAKER_0`/`MICROPHONE` lines against the rest).
- Pyannote on the system stem did not separate the three remote voices. It put them in
  one or two labels, and most system lines kept the plain `SYSTEM` label.
- Run 2 only checks whether the mic speaker matches the owner's voice profile. It does not
  change a label. It costs 45 s (60 min) to 63 s (80 min), and 18–25 s of that is loading
  the audio, which run 1 already did.
- **Options:**
  - `diarization.engine = "none"` for calls removes both runs: 109 s (60 min) or 138 s
    (80 min) saved. But the remote speakers then all get one label, and no voice is
    remembered.
  - Better: keep run 1, and let core skip run 2 when the owner has no voice profile, or
    reuse the audio from run 1 (patch 14).
- With option 1 these passes run after the meeting is open, so they matter less.

### 5. Faster whisper settings

**Prototype core patch** (`docs/research/patches/minutes-core-whisper-chunks.prototype.diff`,
built against a sandbox copy of the tagged core). Each change has an env toggle. 20-minute
call, `small`, one block, load 2.7–3.5:

| Variant | Transcription | Change | WER, time-ordered | WER, mic / system stem |
|---|---|---|---|---|
| Off (control) | 131.5 s | — | 8.4% | 5.1% / 9.5% |
| Pack chunks into ≤30 s windows | 109.1 s | −17% | 11.7% | 5.2% / 9.3% |
| Reuse one whisper state (and its VAD) | 114.5 s | −13% | 8.3% | 5.4% / 9.3% |
| Flash attention | 116.9 s | −11% | 8.1% | 4.8% / 9.3% |
| All three | 93.5 s | −29% | 11.7% | 4.7% / 8.9% |

- State reuse and flash attention are free wins.
- Packing keeps the words: the WER of each stem is the same. But one packed window gives
  one long line with one timestamp. In the merged call transcript, the mic and system
  turns then interleave in the wrong order, which raises the time-ordered WER. A
  packing patch must split the output at whisper's segment times, or pack only short gaps
  inside one turn.

**Models and VAD** (20-minute call, one block, load 1.9–2.6):

| Setting | Transcription | WER | WER, mic / system | Peak memory | Fixed cost a call |
|---|---|---|---|---|---|
| Balanced, `small` (control) | 95.7 s | 9.0% | 6.6% / 9.8% | 1.01 GB | 0.8 s |
| `small`, Silero VAD off (`vad_model = ""`) | 114.8 s | 8.5% | 5.5% / 9.6% | 1.09 GB | — |
| Fast, `base` | 50.1 s (−48%) | 11.3% | 7.6% / 12.5% | 0.60 GB | 0.03 s |
| Best, `large-v3-turbo` | 402.3 s (4.2×) | 6.2% | 4.4% / 6.8% | 2.06 GB | 3.9 s |

- Silero VAD inside each call saves time, because it skips silence inside a chunk. Keep it.
- The fixed cost of a call grows with the model's encoder: 0.03 s (`base`), 0.8 s
  (`small`), 3.9 s (turbo). Fewer calls (packing) help the large models most.
- `base` halves the time but adds about 2.5 points of WER. That is a quality loss, so it
  is not a fix for the default.

**Temperature fallback** (not measured: the synthetic audio never triggers it). On the
real call, the 100 slowest calls took 1,108 s. If a cap (no fallback past 0.4, or
`best_of 2`) brought them to the median of 1.3 s, transcription would fall by about 980 s,
which is half. The quality risk is real: fallback exists to break repetition loops. This
needs a test on real recordings with the owner's consent (patch 13).

**Parakeet.** Upstream's desktop app uses Parakeet v3 through the sherpa plugin
(`engine-sherpa`, a dlopened, separately signed plugin and a 670 MB model). Upstream's
numbers: 151 ms against 290 ms (Whisper `small`) a clip on an M4 Max, and 3.46% against
5.81% WER (`site/app/writing/whisper-cpp-vs-parakeet-cpp`). It covers 25 European languages
and has no vocabulary hints. The `parakeet` feature that Margin could turn on resolves to
whisper at this tag (`parakeet_private_audio_transport_supported()` returns false). This
is the largest change and it is not measured here.

### 6. What upstream Minutes does

Upstream does not transcribe during the recording. Its live transcript is for display
only. After Stop it re-transcribes the WAV in a worker child, writes the file at
`TranscriptOnly` with "Open Transcript", and enriches it in place. Its speed lever is the
engine: Parakeet through sherpa when installed, otherwise Whisper `small`. The three
commits after v0.32.1 do not change processing.

## The live model: `small` or `base`

The coordinator's fix sets `live_transcript.model` to the user's chosen model.

- **Fast or Balanced chosen: use the chosen model.**
  - `small` live has 2.4 points less WER than `base` live (9.3% against 11.7%).
  - Both use the same CPU (about 12–13% of one core at real time). Whisper runs on the
    GPU.
  - `small` keeps about 390 MB more memory during the recording.
  - The extra lag is small. At 3× the lag was 2.6 s against 1.3 s. Lag at 3× is the VAD
    wait plus three times the whisper time. If `small` takes about 3× as long as `base`,
    the lag at real time is about 1.3 s (`small`) and 0.9 s (`base`). That is an estimate.
  - Both kept up at 3×, so at real time each has at least 3× headroom. The sidecar gets
    audio over a channel from the capture thread and cannot block it.
- **Best chosen: do not use large-v3-turbo live.** It fell behind at 3× (22 s to catch up,
  30% of the lines lost), and used 1.9 GB. At real time it may keep up, with about 5–10 s
  of lag. Download `small` (488 MB) as the live model for these users, or `base` (148 MB)
  if download size matters more than live quality.
- If rolling transcription ships (Phase 2), the live sidecar shares the GPU with it. The
  measurements above did not include that. Measure both together before Phase 2 ships.

## Recommended plan

Times are for a call with the `small` model, at the real-audio rate. "Readable" means the
user has the final transcript open.

| | Today | Phase 1 | Phase 2 | Phase 3 |
|---|---|---|---|---|
| 60 min: first text after Stop | 31 min | 0 s (live draft) | 0 s | 0 s |
| 60 min: final transcript readable | 31 min | 24 min | **15–50 s** | 10–35 s |
| 60 min: speakers in place | 31 min | 26 min | 2–3 min | 1–2 min |
| 80 min: first text after Stop | 39.6 min | 0 s (live draft) | 0 s | 0 s |
| 80 min: final transcript readable | 39.6 min | 32 min | **15–50 s** | 10–35 s |
| 80 min: speakers in place | 39.6 min | 34.5 min | 2.5–3.5 min | 1.5–2.5 min |

The summary runs after these and no longer blocks anything. Its time-out is handled in
main. In Phases 2 and 3 the times do not grow with the length of the meeting, except for
the speaker passes.

### Phase 1: next version, Margin only

1. Live model fix (in main).
2. After Stop, show the live transcript in the processing view.
3. Open the meeting at `TranscriptOnly`, and reload it in place at each later stage.
   Block notes saves and speaker rename until Complete.
4. Summary off the critical path (in main).

### Phase 2: rolling transcription (needs upstream patch 11)

1. Send patch 11 upstream: a job input for per-stem transcript checkpoints.
2. A recorder thread that transcribes 120 s pieces of each stem while recording. It
   writes the checkpoint next to the job. Fast and Balanced only. Best uses the batch
   pass.
3. At Stop, queue the job with the checkpoint. Core transcribes only the tail.
4. Measure the GPU and battery cost during a real call, with the live sidecar running.
   If it is too high, use larger pieces, or start rolling only after 10 minutes.

If upstream does not take patch 11, Margin can carry it as a `[patch]` on the tagged core
(as for the `ort` pin). The rest of the work stays the same.

### Phase 3: later, upstream

1. Patch 12: reuse the whisper state, turn on flash attention, pack with segment
   timestamps (−29% on synthetic audio).
2. Patch 13: a cap on temperature fallback, after tests on real audio (up to −50% on the
   real call, an estimate).
3. Patch 14: skip or slim speakers run 2 (−45 to −63 s).
4. Evaluate Parakeet through sherpa as a fourth model choice.

## Files

- `src-tauri/src/rolling.rs`: the rolling prototype.
- `src-tauri/src/devcli.rs`: `live-replay`, `rolling`, and stems plus stage times in
  `process`.
- `docs/research/patches/minutes-core-whisper-chunks.prototype.diff`: the prototype core
  patch. Build it with
  `cargo build --release --config 'patch."https://github.com/silverstein/minutes".minutes-core.path="<copy>/crates/core"'`
  in a separate `CARGO_TARGET_DIR`, then restore `Cargo.lock`.
- `docs/research/live-scripts/`: the corpus, evaluation and run scripts.
