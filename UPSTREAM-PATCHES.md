# Upstream patches (candidates for PRs to silverstein/minutes)

Each entry: what, why, where. Nothing here has been applied to upstream. We work around
each one locally, except #11, which Margin carries in a fork (DECISIONS.md, "A minutes-core
fork with one patch").

## 1. `voice::db_path()` should honour `MINUTES_DATA_DIR`
- **Where:** `crates/core/src/voice.rs` `db_path()` (v0.32.1 ~line 135). It hardcodes
  `dirs::home_dir()/.minutes/voices.db`.
- **Why:** every other piece of state follows `Config::minutes_dir()` (`MINUTES_DATA_DIR`).
  `voices.db` alone leaks into `~/.minutes`, so a host that isolates its state still shares
  voice profiles with any other Minutes install.
- **Patch:** `Config::minutes_dir().join("voices.db")`, with a one-time fallback that reads
  the legacy path.
- **Workaround:** we share `~/.minutes/voices.db`, and Reset leaves it alone unless asked.

## 2. Public model-download API with progress
- **Where:** the CLI's private `download_file` and `cmd_setup` in `crates/cli/src/main.rs`.
  Upstream's Tauri app re-implements the same thing in `commands.rs`.
- **Why:** every GUI host needs "download whisper/VAD/diarization models with a progress
  bar". Today each host copies the URLs and the loop.
- **Patch:** `minutes_core::models::{required_models(&Config) -> Vec<ModelSpec>,
  download(&ModelSpec, &mut dyn FnMut(u64, u64))}` with checksums.
- **Workaround:** our own downloader in `src-tauri/src/models.rs`.

## 3. Ollama summaries: disable "thinking", make the timeout configurable
- **Where:** `crates/core/src/summarize.rs`, the Ollama `/api/generate` request (v0.32.1
  ~line 2997), and `HTTP_TIMEOUT` (~line 3015, a fixed 2 minutes for every LLM call).
- **Why:** reasoning models (qwen3, deepseek-r1, gpt-oss) spend most of their output on
  thinking. On an M-series Mac, qwen3.5:4b runs at about 17 tokens/s. A three-line prompt
  produced 2,889 characters of thinking. A real meeting never finishes inside 2 minutes,
  and the user only sees "provider_error".
- **Patch:** send `"think": false` (Ollama ≥ 0.9 ignores it for models without thinking).
  Add `summarization.ollama_timeout_secs`, defaulting to the current 120.
- **Workaround:** Margin's error message names the cause and suggests a non-thinking model
  or Claude.

## 4. Rename a speaker in the Markdown itself
- **Where:** `crates/core/src/overlays.rs` (`write_speaker_confirmation`) and the CLI's
  `confirm`.
- **Why:** a confirmation is an overlay row bound to the file's sha256.
  - Any later edit (notes, a re-summary) silently drops it for readers that check the
    hash.
  - The Markdown keeps `SPEAKER_1`, so Obsidian never sees the name.
  - The CLI also refuses labels that aren't already in `speaker_map`.
- **Patch:** `markdown::rename_speaker(path, label, name) -> Result<()>`. It would rewrite
  transcript labels and `@label` / whole-word mentions outside `## Notes`, and update
  `speaker_map`, `attendees`, `action_items[].assignee`, `intents[].who` and `people`.
  Then it writes the overlay row against the new bytes.
- **Workaround:** Margin does exactly this in `src-tauri/src/speakers.rs`.

## 5. Tag live-transcript lines with their capture source
- **Where:** `crates/core/src/live_transcript.rs`, the recording sidecar. In dual-source
  call capture, the live whisper thread hears the mixed signal and writes
  `speaker: null`.
- **Why:** core already has the separate voice and system streams. With them, a live UI
  could show "You" and "Them" from facts instead of guesses.
- **Patch:** in dual-source mode, compare per-source energy over each utterance's window
  inside the sidecar and set `speaker` to `"MICROPHONE"` or `"SYSTEM"` (the labels
  `source_transcript.rs` already uses), or `null` when unclear.
- **Workaround:** Margin reads the growing `.voice.wav` / `.system.wav` stems and compares
  loudness for each line (`recorder.rs::guess_speaker`). It works, but has to infer
  timing from the line's write time.

## 6. User templates should follow `MINUTES_DATA_DIR`
- **Where:** `crates/core/src/template.rs` `default_user_templates_dir()` (v0.32.1 ~line
  333). It hardcodes `$HOME/.minutes/templates`.
- **Why:** this is the same leak as #1. A host that isolates its state cannot keep its
  own templates without sharing them with every other Minutes install.
- **Patch:** `Config::minutes_dir().join("templates")`, with a fallback that reads the
  legacy path.
- **Workaround:** Margin writes a project template in `<state>/.minutes/templates/` and
  runs with the state directory as its working directory (DECISIONS.md D14).

## 7. Summary instructions in the config
- **Where:** `crates/core/src/config.rs` `SummarizationConfig`.
- **Why:** a host that wants a "custom instructions" box must write a template file and
  pass its slug to every queued job and every re-summary.
- **Patch:** add `summarization.additional_instructions: String`, applied in
  `build_system_prompt` after the template's own instructions.
- **Workaround:** the project template from #6.

## 8. Vocabulary hints are capped at 8 and sorted by name
- **Where:** `crates/core/src/pipeline.rs` `build_decode_hints` (`decode_phrases(8)`), and
  `transcribe.rs` `whisper_initial_prompt` (12 phrases in total).
- **Why:** when a user lists more than 8 terms of the same priority, the decoder gets the
  first 8 in alphabetical order, not the 8 that matter.
- **Patch:** keep the order the user gave (or a per-entry rank), and make the cap a
  config value.
- **Workaround:** Settings says that the transcriber gets up to 8 of the terms.

## 9. The mic-mute sentinel should honour `MINUTES_DATA_DIR`
- **Where:** `crates/core/src/streaming.rs` `mic_mute_sentinel_path()` (v0.32.1 ~line
  162). It hardcodes `~/.minutes/mic_mute`.
- **Why:** like `voices.db` (#1), a host that isolates its state still shares this file.
  If Margin muted the mic through it, a recording in the upstream app would be muted too.
- **Patch:** `Config::minutes_dir().join("mic_mute")`.
- **Workaround:** Margin has no mute control.

## 10. Pause and resume for a recording
- **Where:** `crates/core/src/capture.rs` `record_to_wav_with_lifecycle`.
- **Why:** a meeting often has a break or an off-the-record part. Today the only choices
  are to stop (and get two meetings) or to keep recording.
- **Patch:** a `pause: Arc<AtomicBool>` beside the stop flag. While it is set, drop
  captured frames (or write silence, to keep stem timing aligned) and pause the live
  transcript. Record the paused spans in the job, so durations stay right.
- **Workaround:** none. Margin has no pause.

## 11. Let a meeting job start from transcript checkpoints (rolling transcription)
- **Where:** `crates/core/src/jobs.rs` (`ProcessingJob`, `queue_live_capture_with_recording_health`)
  and `crates/core/src/pipeline.rs` `transcribe_independent_stems` (v0.32.1 ~line 2509).
- **Why:** the job always transcribes the whole WAV after Stop. For a long call that is
  most of the wait: 32 of 39.6 minutes for an 80-minute call. A host that transcribes
  during the recording has no way to hand its work to the job. `write_transcript_artifact`
  accepts text, but it drops the stem-aware speaker labels and the job states.
- **Patch:**
  - A `transcript_checkpoint: Option<PathBuf>` on the job. It points at a JSON file with,
    for each stem, `covered_until_samples` and the `[m:ss] text` lines found so far.
  - In `transcribe_independent_stems`, take those lines, run chunk detection only on the
    samples after `covered_until_samples`, and offset the new lines.
  - Check the checkpoint against the stem's length and model id, and ignore it when it
    does not match.
  - The rest of the pipeline is unchanged. Chunks are independent (`no_context` is on),
    so the result equals the batch pass.
- **Evidence:** docs/research/live-transcription.md, option 2. With 120 s pieces, 12 s of
  transcription is left at Stop for a 60-minute call, at the same WER.
- **Status:** sent upstream as https://github.com/silverstein/minutes/pull/1316. The patch
  as built differs from the sketch above:
  - Core makes the spans itself (`transcript_checkpoint::transcribe_span`), so a host
    cannot drift from the final pass's chunking or settings.
  - Each span has a source, a sample range, a SHA-256 of its samples and a settings
    fingerprint. The final pass reuses only spans that match the final file and settings,
    and transcribes every other range, so a failed piece in the middle is fine.
  - `jobs::queue_live_capture_with_transcript_checkpoint` saves them beside the job.
- **Workaround:** Margin uses the same patch from `askysh/minutes` (branch
  `margin/v0.32.1-rolling`), pinned by commit, until upstream ships it.

## 12. Whisper meeting chunks: reuse the state, flash attention, fewer calls
- **Where:** `crates/core/src/transcribe.rs` `transcribe_whisper_chunk_ranges` (~line 893),
  `transcribe_with_whisper` (~line 1642), `whisper_context_params` (~line 1392).
- **Why:** each VAD chunk is one `full()` call on a new state. So each call reloads the
  Silero VAD model and pays a full 30 s encoder pass. The real 80-minute call made 717
  calls, and a 2 s chunk cost about as much as a 15 s one.
- **Patch:**
  - Make one `WhisperState` per stem and reuse it for every chunk.
  - Set `flash_attn = true` when DTW is not used.
  - Pack neighbouring chunks into one window of up to 30 s, but keep whisper's segment
    timestamps. Do not give a packed window one timestamp: in a call, the mic and system
    lines then interleave in the wrong order.
- **Evidence:** a prototype is in `docs/research/patches/minutes-core-whisper-chunks.prototype.diff`.
  On a 20-minute call: −13% (state reuse), −11% (flash attention), −29% (all three). The
  WER of each stem was unchanged.
- **Workaround:** none.

## 13. A cap on whisper's temperature fallback for meeting chunks
- **Where:** `crates/whisper-guard/src/params.rs` (`best_of 5`, `temperature_inc 0.2`).
- **Why:** on the real 80-minute call, the 100 slowest calls (14%) took 57% of the
  transcription time, at 20–37 s each for short chunks with few words. That is the fallback
  re-decoding at higher temperatures, five candidates each time.
- **Patch:** `transcription.max_fallback_temperature` (for example 0.4) and
  `transcription.fallback_best_of` (for example 2), with today's values as defaults until
  a test on real meetings shows the quality cost.
- **Workaround:** none.

## 14. Skip or slim the second speaker pass
- **Where:** `crates/core/src/pipeline.rs` `single_stem_speaker_self_attribution`
  (~line 3171), which calls `diarize::diarize(&stems.voice, ..)`.
- **Why:** after the summary, core runs pyannote again on the whole mic stem, only to check
  whether the mic speaker matches the owner's voice profile. It changes no label. It took
  63 s on the 80-minute call, and 18–25 s of that loads and prepares audio that run 1 just
  loaded.
- **Patch:** skip it when no voice profile exists for `identity.name`. Otherwise reuse the
  decoded audio from run 1, and embed only a sample of the mic segments.
- **Workaround:** none.

## 15. The live model should fall back to an installed model
- **Where:** `crates/core/src/live_transcript.rs` `resolve_live_model_path` (~line 2855).
- **Why:** with `live_transcript.model` empty, the sidecar uses the dictation model, "base".
  If only `transcription.model` is installed, every utterance fails. A real 80-minute call
  got 0 live lines, with one warning per utterance and no visible error.
- **Patch:** if the resolved model is missing, use `transcription.model` when it is
  `base` or `small`. Report the failure through the live status once, not per utterance.
- **Workaround:** Margin sets `live_transcript.model` itself.
