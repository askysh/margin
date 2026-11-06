# Upstream patches (candidates for PRs to silverstein/minutes)

Each entry: what, why, where. Nothing here has been applied to upstream. We work around
each one locally.

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
  runs with the state directory as its working directory (DECISIONS.md, S2).

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
