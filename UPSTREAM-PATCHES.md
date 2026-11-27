# Upstream patches (candidates for PRs to silverstein/minutes)

Each entry: what, why, where. Upstream merged #11 (transcript checkpoints). Nothing else
here has been applied to upstream. We work around #1–#24 locally. #25 onward are commits
in Margin's vendored core, `margin/vendor/minutes-core` (DECISIONS.md D80). Each one is
written to be sent upstream as a pull request, and each defaults to today's behaviour.

## 1. `voice::db_path()` should honour `MINUTES_DATA_DIR`
- **Where:** `crates/core/src/voice.rs` `db_path()` (v0.32.1 ~line 135). It hardcodes
  `dirs::home_dir()/.minutes/voices.db`.
- **Why:** every other piece of state follows `Config::minutes_dir()` (`MINUTES_DATA_DIR`).
  `voices.db` alone leaks into `~/.minutes`, so a host that isolates its state still shares
  voice profiles with any other Minutes install.
- **Patch:** `Config::minutes_dir().join("voices.db")`, with a one-time fallback that reads
  the legacy path.
- **Workaround:** we share `~/.minutes/voices.db`, and Reset leaves it alone unless asked.
- **On Windows:** `dirs::home_dir()` asks Windows for the profile folder and ignores `HOME`
  and `USERPROFILE`. Core's own `config::home_dir()` reads both, for that reason. So a
  sandbox cannot move `voices.db` at all, and `margin --dev process` in a sandbox wrote
  `C:\Users\<user>\.minutes\voices.db` (2026-11-13). The same is true for the mic-mute
  sentinel (#9). Tests on Windows link the folder into the sandbox (docs/development.md).

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
- **Workaround:** `src-tauri/src/ollama_proxy.rs` (D69). For each summary and title call,
  Margin points core's `ollama_url` at a local forwarder that adds `"think": false` to
  `/api/generate` and `/api/chat`. An Ollama Modelfile cannot set `think` (Ollama 0.32.14
  refuses `PARAMETER think`). Settings → Summaries → *Answer without thinking first* turns
  it off. Remove the forwarder when core sends `think` itself.

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
- **Merged upstream** as https://github.com/silverstein/minutes/pull/1316 (merge commit
  `209265f`, 2026-11-06). Margin pins that commit on upstream main until a tag has it
  (DECISIONS.md, "Back on upstream minutes-core").
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
- **Status:** merged upstream as https://github.com/silverstein/minutes/pull/1316. The
  patch as built differs from the sketch above:
  - Core makes the spans itself (`transcript_checkpoint::transcribe_span`), so a host
    cannot drift from the final pass's chunking or settings.
  - Each span has a source, a sample range, a SHA-256 of its samples and a settings
    fingerprint. The final pass reuses only spans that match the final file and settings,
    and transcribes every other range, so a failed piece in the middle is fine.
  - `jobs::queue_live_capture_with_transcript_checkpoint` saves them beside the job.
- **Workaround:** none now. Until 0.0.4, Margin used the same patch from the
  `askysh/minutes` fork.

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

## 16. A public cut helper for transcript checkpoint spans
- **Where:** `crates/core/src/transcribe.rs` `detect_meeting_vad_chunks` (~line 2387 at
  `209265f`) is `pub(crate)`. The doc of `transcript_checkpoint::transcribe_span` says to
  cut a span in a pause, but core gives no way to find a good cut.
- **Why:** a span gets the same chunks as the final pass only when it starts where the
  chunker resets, on its 100 ms grid from the start of the source. A span that starts
  anywhere else gets other chunks, and whisper gives other words. On a real 80-minute
  call, about 12% of the words were different. To cut in the right place, a host must
  copy the chunker's rules.
- **Patch:** make a cut helper public, for example `transcript_checkpoint::span_cut(pending:
  &[f32], target: usize) -> Option<usize>`, as `capture_checkpoint::span_cut` in
  silverstein/minutes#1319. At least, make `detect_meeting_vad_chunks` public.
- **Workaround:** `src-tauri/src/piece_cut.rs` has Margin's copy of the chunker's rules
  (DECISIONS.md D49). A test compiles core's own chunker from the pinned source and
  compares the two.

## 17. No Screen Recording warning for a process-tap call capture
- **Where:** `crates/core/src/capture.rs` `evaluate_permission_preflight` (~line 843 at
  `209265f`), which adds `SCREEN_PERMISSION_DENIED_WARNING` (~line 808) for every call
  whose system audio is ready but whose Screen Recording permission is not granted.
- **Why:** a Core Audio process tap (`recording.capture_backend = "core-audio-tap"`) needs
  the System Audio Recording permission, not Screen Recording. Margin records calls that
  way (DECISIONS.md D5). So each Start in Margin logged "Minutes does not have Screen
  Recording permission, so native call capture may not include system audio", and the
  call audio was there. The warning also names "Minutes", not the host app.
- **Patch:** warn about Screen Recording only for the ScreenCaptureKit route. For the
  process tap, check the System Audio Recording permission (TCC
  `kTCCServiceAudioCapture`) and warn about that one. Give each warning a kind (an enum
  next to the text), so a host can filter or reword it without matching the text.
- **Workaround:** `recorder.rs` `is_screen_recording_warning` leaves out warnings that name
  Screen Recording when Margin's own check found System Audio Recording granted.

## 18. Return the summary's title from resummarize
- **Where:** `crates/core/src/resummarize.rs` `resummarize_meeting` (~line 685 at
  `209265f`). The summary call already writes a title (`summarize::Summary.title`), but
  `ResummarizeReport` does not carry it, and `sanitize_llm_title_candidate` and
  `llm_title_passes_quality` in `pipeline.rs` are private.
- **Why:** a host that writes the summary after processing (Margin, DECISIONS.md D40) has
  no summary during processing, so core's title step keeps its fallback title. To name
  the meeting, Margin makes a second model call with `summarize::refine_title` and checks
  the answer with its own rules.
- **Patch:** add `title: Option<String>` to `ResummarizeReport`, cleaned and checked with
  the pipeline's title rules. Or make those two functions public.
- **Workaround:** `summaries.rs` `title_from_summary` (DECISIONS.md D59).

## 19. Windows: say "no microphone" when there is no input device
- **Where:** `crates/core/src/error.rs` `CaptureError::DeviceNotFound`, and the end of the
  input device selection in `capture.rs` (~line 2895 at `209265f`,
  `host.default_input_device().ok_or(CaptureError::DeviceNotFound)`).
- **Why:** the text is "audio device not found — is VB-CABLE installed?". A Windows PC
  with no microphone connected gets this for a plain microphone recording. VB-CABLE is a
  loopback driver, and it has nothing to do with a microphone. `preflight_recording` gives
  no blocking reason for the same PC.
- **Patch:** a "no input device" error with its own text, and a blocking reason in
  `preflight_recording` when the host has no input device.
- **Workaround:** on Windows, `recorder.rs` `ensure_permissions` stops the start with
  Margin's own message when `list_devices_categorized()` is empty.

## 20. Windows: no console window for child programs
- **Where:** `crates/core/src/bounded_child.rs` `configure_process_tree`, the Windows
  version (~line 1335 at `209265f`). It sets `command.creation_flags(CREATE_SUSPENDED)`
  and nothing else. Every `BoundedCommand` goes through it: the summary agent
  (`summarize.rs`), ffmpeg (`ffmpeg.rs`, `pipeline.rs`, `transcribe.rs`, `diarize.rs`) and
  the hooks (`hooks.rs`). `engine_process.rs` `command()` makes a plain `Command` with no
  flags too.
- **Why:** a release build of a Windows app has no console. When it starts a console
  program, such as the `claude` CLI for a summary, Windows opens a console window for
  that program. Core sets no `CREATE_NO_WINDOW` flag anywhere.
- **Patch:** on Windows, `CREATE_SUSPENDED | CREATE_NO_WINDOW` in `configure_process_tree`,
  and `CREATE_NO_WINDOW` in `engine_process::command()`.
- **Workaround:** none for the programs that core starts. Margin sets the flag for the
  programs that it starts itself (`win.rs`): the processing worker, Ask and the `claude`
  checks.
- **Checked on the release build (2026-11-13):** no console window opens when the app
  starts, and none when it records and processes a meeting with summaries off. That path
  starts no console program: the workers are `margin.exe` itself, which has no console.
  A summary with `claude` goes through `BoundedCommand`, so it will open one. Not run:
  the test PC has the owner's real `claude`.

## 21. Windows: a release build compiles whisper.cpp with no optimization
- **Where:** `whisper-rs-sys` 0.15.0 `build.rs`, which minutes-core uses through
  `whisper-rs` 0.16. It sets `CMAKE_BUILD_TYPE=Release` for a release build. The `cmake`
  crate then sets `CMAKE_C_FLAGS_RELEASE` and `CMAKE_CXX_FLAGS_RELEASE` to the flags of
  the `cc` crate, with the optimization flags taken out (`cmake` 0.1.57 `lib.rs`, "The
  visual studio generator apparently doesn't respect `CMAKE_C_FLAGS`"). With the Visual
  Studio generator this replaces CMake's own `/O2 /Ob2 /DNDEBUG`.
- **Why:** every release build of minutes-core on Windows with MSVC gets a whisper.cpp
  with no optimization. On the test laptop the `small` model took 190 s for 46 s of
  audio. With the flags back it takes 13 s. A debug build is fast by accident: the build
  script names `RelWithDebInfo` there, so the `cmake` crate replaces the flags of a
  configuration that the build does not use.
- **Patch:** in `whisper-rs-sys`, do not set `CMAKE_BUILD_TYPE` for a multi-config
  generator, or set the two `_RELEASE` variables to CMake's defaults. Until then, upstream
  can say in its Windows build notes to set the two variables. The build script passes
  each `CMAKE_` variable of the environment to CMake.
- **Workaround:** `scripts/build-windows.sh` sets `CMAKE_C_FLAGS_RELEASE` and
  `CMAKE_CXX_FLAGS_RELEASE` to `-MD -O2 -Ob2 -DNDEBUG`. It also sets `GGML_NATIVE=OFF`,
  so the build does not depend on the CPU of the build PC. A plain
  `cargo build --release` needs the same variables.

## 22. Windows: the `vulkan` feature needs a short build path and `vulkan-1.dll`
- **Where:** `crates/core/Cargo.toml` `vulkan = ["whisper-rs/vulkan"]`, and the text
  that says to prefer Vulkan on Windows. The causes are in `whisper-rs-sys` 0.15.0 and
  whisper.cpp 1.8.3 (`ggml/src/ggml-vulkan/CMakeLists.txt`, the `vulkan-shaders-gen`
  external project).
- **Why:** two things stop a first build or a first run.
  - The shader tool is a CMake project inside the build folder of `whisper-rs-sys`. With
    the folder names of cargo, its paths go past the 260 characters that MSBuild allows,
    also from a short target folder such as `C:\Users\<user>\.margin-vk`. The build stops
    with "The C compiler is not able to compile a simple test program".
  - `whisper-rs-sys` links `vulkan-1` as a normal import. An app with the feature does
    not start on a PC that has no `vulkan-1.dll`. With the DLL and no usable driver, the
    feature works well: whisper.cpp says "no GPU found" and uses the CPU.
- **Patch:** say both in the Windows build notes: build from a target folder with a very
  short path (a `subst` drive works), and ship `vulkan-1.dll` beside the app. In
  `whisper-rs-sys`, a delay-load of `vulkan-1.dll` with a check before the first call
  would remove the second point.
- **Workaround:** `scripts/build-windows.sh` maps the target folder to a drive
  letter while it builds, and puts `vulkan-1.dll` from the Vulkan runtime beside the exe.
- **Measured (2026-11-13, RTX 3070 Ti Laptop GPU):** `large-v3-turbo` runs 20 times
  faster than real time with Vulkan, and 1.05 times on 8 CPU cores
  (docs/development.md, "Whisper on the GPU").

## 23. A setting to keep Whisper off the GPU
- **Where:** `crates/core/src/transcribe.rs` `whisper_context_params()` (~line 1415 at
  `209265f`). It sets `params.use_gpu` to true when a GPU feature is compiled in. The
  config has no field for it.
- **Why:** a build with `vulkan` or `cuda` runs on many graphics drivers. A user with a
  driver that gives a wrong transcript, or that stops the app, has no way back to the
  CPU. On macOS the question does not come up, because Metal is one driver.
- **Patch:** a config field, `transcription.use_gpu`, true by default, that
  `whisper_context_params()` reads.
- **Workaround:** on Windows, Settings has "Use the graphics card". When it is off,
  Margin sets `VK_LOADER_DRIVERS_DISABLE=*` in its own process before anything else runs
  (`win.rs` `whisper_on_cpu`). The Vulkan loader then hides every driver, whisper.cpp
  finds no device and uses the CPU. A change applies at the next start of the app.

## 24. Drop transcript segments that reproduce the Whisper prompt
- **Where:** `crates/core/src/transcribe.rs`, `whisper_initial_prompt` and the
  anti-hallucination layers after decoding.
- **Why:** on noise, Whisper outputs its initial prompt ("Names and terms that may appear
  in this audio: … Preserve spelling exactly when heard."). Margin measured 8 such lines in
  225 seconds of room tone (D71).
- **Patch:** after decoding, drop a segment whose normalized text is contained in the
  normalized prompt, or that contains the prompt's fixed sentence.
- **Workaround:** `library::scrub_prompt_echo` removes the lines when a meeting is ready.

## 25. VAD and decoding settings for the batch whisper pass
- **Where:** `crates/core/src/config.rs` `TranscriptionConfig`; `transcribe.rs`
  `transcribe_with_whisper` (new `apply_whisper_tuning`, `configured_vad_params`).
- **Why:** `whisper_guard::default_whisper_params` always uses whisper.cpp's Silero
  defaults (threshold 0.5, 250 ms shortest speech, 30 ms padding) and whisper.cpp's text
  conditioning. On AMI, 77% of Margin's word errors are deletions, mostly short
  backchannels that the VAD drops (docs/benchmark/2026-11-27-improvement-plan.md §1.3).
  Hosts cannot tune any of it.
- **Patch:** optional `transcription.vad_threshold`, `vad_min_speech_ms`,
  `vad_min_silence_ms`, `vad_speech_pad_ms` and `n_max_text_ctx`, plus
  `suppress_nst` (default false). A missing key keeps whisper.cpp's value, so existing
  configs decode exactly as before. The fields are part of `TranscriptionConfig`'s
  `Debug` output, so transcript checkpoints made with other values are not reused.
- **Caveat for the PR:** whisper.cpp passes the initial prompt through the same buffer as
  the earlier text. `n_max_text_ctx = 0` therefore also drops the vocabulary prompt. The
  doc comment says so.
- **Tests:** `transcribe::tests::vad_tuning_is_absent_by_default_and_follows_the_config`.

## 26. An opt-in bare-term vocabulary prompt (measured worse; off by default)
- **Where:** `crates/core/src/transcribe.rs` `DecodeHints::whisper_terms_prompt` and
  `transcribe_with_whisper`; `config.rs` `transcription.prompt_terms_only`.
- **Why:** Whisper's prompt is earlier transcript text that conditions spelling and
  style. It does not follow the instruction in "Names and terms that may appear in this
  audio: … Preserve spelling exactly when heard.", and reads that sentence back on noise
  (Margin D71, #24).
- **Measured (AMI, large-v3-turbo; Margin D82):** the bare list `"AMI ES2004a final,
  <user name>"` raised WER from 26.6% to 30.3% on ES2004a (insertions 33 → 89) and from
  26.2% to 26.7% on TS3003a. An unpunctuated list conditions unpunctuated, fragmented
  output and repeated phrases. So the sentence stays the default, and
  `prompt_terms_only = true` is only for experiments.
- **Tests:** `transcribe::tests::decode_hints_build_whisper_prompt` checks both prompts.

## 27. A speaker-count range for diarization, and a cap from the calendar
- **Where:** `crates/core/src/config.rs` `DiarizationConfig`; `diarize_tracks.rs`
  `cluster_embeddings` (`SpeakerBounds`); `diarize_legacy.rs` (`cap_template_clusters`);
  `pipeline.rs` (`config_with_attendee_speaker_cap`, at both diarization call sites).
- **Why:** clustering stops at a fixed cosine threshold and knows nothing about how many
  people were there. On AMI ES2004a, community-1 found 5 speakers for 4 people. The
  matched calendar event already tells core who was invited.
- **Patch:** `diarization.max_speakers` and `min_speakers` (both optional) and
  `max_speakers_from_attendees` (default false).
  - community-1: the agglomeration stops above the threshold once at the minimum. Above
    the maximum after the threshold pass, it queues the remaining clusters' compatible
    pairs (k²/2 for k clusters, never one per observation pair) and keeps merging. When
    only same-window pairs are left, the closest clusters merge anyway, because the count
    was asked for.
  - legacy: the closest template clusters merge down to the maximum.
  - With `max_speakers_from_attendees`, the cap is the attendees plus one, or
    `max_speakers` if that is smaller. The count comes from the job's calendar event when
    it has one, else from the attendee list saved with the transcript, with combined
    entries ("Gert and Liam", "Bo & Cy") split.
- **Tests:** `diarize_tracks::tests::a_maximum_*`, `a_minimum_stops_merging_above_the_threshold`;
  `diarize_legacy::tests::template_clusters_merge_closest_first_down_to_the_cap`;
  `pipeline::tests::the_attendee_cap_is_attendees_plus_one_and_never_raises_max_speakers`.

## 28. Give short community-1 tracks a speaker instead of `UNKNOWN`
- **Where:** `crates/core/src/diarize.rs` `diarize_with_pyannote_rs`; `diarize_tracks.rs`
  `attach_short_tracks`; `config.rs` `diarization.attach_unknown`.
- **Why:** a local track with under 1.5 s of clean audio is not clustered and is labelled
  `UNKNOWN`. That covers most backchannels: 64 `UNKNOWN` lines in Margin's ES2004a
  transcript.
- **Patch:** with `attach_unknown` (default false), such a track takes:
  1. the speaker of the nearest clustered speech on both sides, when it is the same
     speaker and each side is within 1 s;
  2. else the nearest cluster to an embedding of its own clean audio (when it has at least
     0.5 s), if the similarity is above the clustering threshold;
  3. else stays `UNKNOWN`.

  It never takes a speaker that another clustered track of the same window has, since
  the segmentation model separated them. An attached track is recorded as speech of its
  label with no embedded audio. When a label's attached speech is more than 10% of its
  speech, it is reported as uncovered, so voice matching's coverage guard
  (`diarize::segment_covers_its_speech`) does not auto-name that label. Up to 10%, naming
  stays possible. The default path keeps upstream's strict guard. A short track whose
  embedding comes back invalid is skipped, not an error.
- **Tests:** `diarize_tracks::tests::a_short_track_*` (3 tests).

## 29. Write the raw diarization as RTTM
- **Where:** `crates/core/src/diarize.rs` (`segments_to_rttm`, `write_meeting_rttm`);
  `pipeline.rs`, next to the meeting embeddings sidecar, after any title rename;
  `config.rs` `diarization.write_rttm`.
- **Why:** transcript lines are a poor stand-in for speaker turns: no overlap, and a line
  runs until the next one. Scoring tools (pyannote.metrics, dscore, md-eval) read RTTM.
- **Patch:** with `write_rttm` (default false), `<meeting>.diarization.rttm` sits beside
  the meeting `.md`, one `SPEAKER <file> 1 <start> <dur> <NA> <NA> <spk> <NA> <NA>` line
  per segment. The first line is the comment `;; written by Margin`. Core replaces only a
  regular file (not a symlink) that starts with that line. It writes a 0600 temp file
  and renames it into place. The temp name is unique per write
  (`<target>.<pid>-<n>.tmp`), and only files of exactly that shape left more than a minute
  ago are cleaned up. The marker check reads 64 bytes. A foreign file is
  logged and left alone. The name is core's own, so a user's `<meeting>.rttm` is never
  touched. RTTMs from Margin's first round of builds have no marker and count as foreign.
  Those builds never shipped.
- **Tests:** `diarize::tests::rttm_has_one_speaker_line_per_segment_and_skips_empty_ones`,
  `the_rttm_has_its_own_private_name_beside_the_meeting`.

## 30. A fuller community-1 pipeline: overlapping windows and frame aggregation
- **Where:** `crates/core/src/diarize.rs` (`segmentation_classes`, `diarize_community_full`);
  `diarize_tracks.rs` (`decode_window_slots`, `aggregate_windows`); `config.rs`
  `diarization.speaker_pipeline`.
- **Why:** core's community-1 path uses non-overlapping 10 s windows and labels each local
  track on its own. pyannote's pipeline slides the window, clusters every window's local
  speakers, and decides each frame from all the windows that cover it.
- **Patch:** `speaker_pipeline = "full"` (default `"simplified"`):
  - a 10 s window every 2.5 s, longer steps when needed to stay within clustering's
    observation budget;
  - an embedding per local speaker with at least 1 s of clean audio;
  - the existing centroid agglomeration, with the configured threshold and bounds;
  - per frame, the speaker count is the rounded mean count of active local speakers
    over the covering windows, and the frame goes to that many speakers with the most
    votes.

  No VBx: it needs PLDA parameters that the ONNX model set does not include. The value
  is read without regard to case. An unknown value is logged and means "simplified".
  Each observation reports its whole track as speech. Aggregated speech that no observed
  track covers, compared on the same frame grid, is reported as uncovered, so the
  voice-matching coverage guard holds and a fully embedded speaker can still be named.
- **Tests:** `diarize_tracks::tests::decoding_keeps_each_tracks_local_slot`,
  `aggregation_votes_across_overlapping_windows_and_keeps_overlap`.

## 31. The summary prompt forbids a lead-in and a closing offer
- **Where:** `crates/core/src/summarize.rs` `build_base_system_prompt`.
- **Why:** small local models (qwen3.5:4b) open with "Based on the transcript provided,
  here is a structured summary…" and close with "Let me know if…". Those lines land in
  the meeting note.
- **Patch:** `summarization.no_lead_in` (default false: upstream's prompt). When it is
  on, one paragraph goes before the format block: start with the TITLE line, end with the
  last section, nothing before or after. Margin turns it on (`corecfg.rs`) and also
  strips such lines afterwards.
- **Tests:** `summarize::tests::the_system_prompt_forbids_a_lead_in_and_a_closing_offer`.

## 32. A model that is not one of ggerganov's files
- **Where:** `crates/core/src/health.rs` (~line 353) and `transcribe.rs` resolve
  `model_path/ggml-{model}.bin`. The CLI's setup downloads only from
  `huggingface.co/ggerganov/whisper.cpp`.
- **Why:** fine-tuned whisper models, such as Trelis/whisper-hinglish-preview converted
  to ggml, load in stock whisper.cpp. A host can offer one only by naming the file
  `ggml-<id>.bin` and handling the download itself.
- **Patch:** an optional `transcription.model_file` (a path), used instead of the derived
  name when set, and a `ModelSpec` with a URL and checksum in the download API from #2.
- **Workaround:** Margin's "hinglish" model id (DECISIONS.md D81) expects
  `ggml-hinglish.bin` in the models folder and offers no download.

## 33. Fallback models for a missing model file
- **Where:** `crates/core/src/transcribe.rs` `load_whisper_context`, `ModelFallback`,
  `FilterStats::model_fallback`; `config.rs` `transcription.missing_model_fallbacks`,
  `missing_model_fallback_language`, `TranscriptionConfig::language_for`,
  `Config::live_language`; `pipeline.rs` (both writers of `processing_warnings`);
  `live_transcript.rs`, `apple_speech_shadow.rs`; `process_trace.rs` `update_language`;
  `dictation.rs`; `source_transcript.rs` (merges the record of both tracks); `summarize.rs`
  `config_for_transcript`; `resummarize.rs`.
- **Why:** a host that offers a model it cannot download (#32) needs the meeting
  transcribed anyway when the file goes missing, without rewriting the user's choice.
- **Patch:** `missing_model_fallbacks: Vec<String>` (default empty, which keeps upstream's
  error). When `model`'s file is missing, empty or fails to load, the next entry that
  loads is used, with a warning.
  - **Language.** With fallbacks set, `language` belongs to `model` alone. Every other
    model runs with `missing_model_fallback_language` (default `None`: detect): a
    fallback, the live transcript's model, whatever the live backend, and dictation
    (`Config::dictation_language`). The process
    trace records the fallback's language. With no fallbacks, every reader gets
    `language`, as upstream.
  - **Record.** The meeting file gets a processing warning
    `{step: transcribe, reason: model_fallback, message: "used <model> because <model>
    missing|empty|failed to load"}`, and `status: degraded`, as for `engine_fallback`.
    It travels in `FilterStats::model_fallback` from the model load to both pipeline
    paths. `ModelFallback::from_warning` / `recorded_in` read it back, so a host need not
    parse the message.
  - **Summary.** An `"auto"` summary language follows `transcription.language`
    (`summarize::get_effective_summary_language`). After a fallback the transcript is in
    the fallback's language, so `summarize::config_for_transcript` gives the summary
    `language_for(<model that ran>)`: the summary and the LLM title in the pipeline
    (both paths; `title_generation_decision` takes the fallback), and
    `resummarize_meeting`, which reads the meeting's `model_fallback` record. Without a
    fallback the config is passed through unchanged.
- **Tests:** `transcribe::tests::missing_empty_or_broken_models_fall_back_with_the_fallback_language`,
  `the_process_trace_records_the_fallback_language`, `a_model_fallback_warning_reads_back`,
  `chunk_stats_keep_the_model_fallback`;
  `pipeline::tests::the_meeting_file_records_which_fallback_model_transcribed`;
  `summarize::tests::an_auto_summary_follows_the_language_of_the_model_that_transcribed`;
  `resummarize::tests::after_a_model_fallback_the_summary_language_follows_the_fallback`;
  `pipeline::tests::the_llm_title_follows_the_language_of_the_model_that_transcribed`;
  `config::tests::dictation_gets_the_language_only_when_it_runs_the_configured_model`;
  `config::tests::without_fallbacks_every_model_and_the_live_transcript_keep_the_language`,
  `with_fallbacks_only_the_configured_model_gets_the_language`;
  `live_transcript::tests::the_live_transcript_beside_a_hinglish_model_uses_the_fallback_language`.
  The tests that load a model need `MINUTES_TEST_WHISPER_MODEL`, the path of a real ggml
  model, and return early without it.

