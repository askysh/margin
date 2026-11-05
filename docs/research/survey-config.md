# minutes-upstream survey: CONFIG, MODELS/SETUP, SUMMARIES, MCP, RESET, CLI JSON

Repo: `~/dev/minutes-upstream` (read-only). All refs are relative to that root.
Leftover user state inspected (read-only): `~/.config/minutes/{config.toml,customize.toml}`, `~/.minutes/`, `~/meetings/`.

---

## 1. Config (`crates/core/src/config.rs`, 4145 lines)

### 1.1 Location, format, precedence
- Format: TOML, serde with `#[serde(default)]` on **every** struct, so a sparse or empty file is valid. The file is optional (header comment at config.rs:5-10: "Compiled defaults -> config file override -> CLI flag override").
- Path: `Config::config_path()` (config.rs:1940)
  ```rust
  pub fn config_path() -> PathBuf  // $MINUTES_CONFIG_PATH (non-empty) else $XDG_CONFIG_HOME/minutes/config.toml else ~/.config/minutes/config.toml
  ```
  The helpers are `config_path_override` (config.rs:1641) and `config_base_dir_from` (config.rs:1630). `XDG_CONFIG_HOME` is honored on macOS too.
- Sibling file: `customize.toml` (theme/accent). Its path comes from `customize::customize_path()` = `config_path().with_file_name("customize.toml")` (customize.rs:80).
- State dir: `Config::minutes_dir()` (config.rs:2386) is `$MINUTES_DATA_DIR` (must be absolute) else `$HOME/.minutes` (config.rs:1617-1628). `home_dir()` reads `$HOME` first (config.rs:1603).

### 1.2 Top-level `Config` struct (config.rs:16-50)
```rust
#[derive(Debug, Clone, Serialize, Deserialize)] #[serde(default)]
pub struct Config {
    pub output_dir: PathBuf,                 // default ~/meetings   (config.rs:1733)
    pub transcription: TranscriptionConfig,  pub diarization: DiarizationConfig,
    pub summarization: SummarizationConfig,  pub copilot: CopilotConfig,
    pub search: SearchConfig,  pub daily_notes: DailyNotesConfig,  pub security: SecurityConfig,
    pub watch: WatchConfig,  pub assistant: AssistantConfig,  pub privacy: PrivacyConfig,
    pub consent: ConsentConfig,  pub screen_context: ScreenContextConfig,
    pub desktop_context: DesktopContextConfig,  pub calendar: CalendarConfig,
    pub call_detection: CallDetectionConfig,  pub identity: IdentityConfig,
    pub vault: VaultConfig,  pub dictation: DictationConfig,  pub voice: VoiceConfig,
    pub voice_live: VoiceLiveConfig,  pub live_transcript: LiveTranscriptConfig,
    pub recording: RecordingConfig,  pub retention: RetentionConfig,  pub hooks: HooksConfig,
    pub knowledge: KnowledgeConfig,  pub palette: PaletteConfig,
    pub global_hotkey: GlobalHotkeyConfig,  pub notifications: NotificationsConfig,  pub ui: UiConfig,
}
```
There are 30 sections in total. The real file at `~/.config/minutes/config.toml` (6.5 KB, mode 0600) has every section written out, so a full `save()` has run on it at some point.

### 1.3 Relevant sections and fields

**Output dir**: `output_dir: PathBuf`. The default is `~/meetings`. Memos go in `output_dir/memos` (markdown.rs:1712, `ensure_dirs` config.rs:2364). Core does **not** expand `~`. Write absolute paths. The only tilde expansion in core is in voice_live/tools.rs:1151.

**`[transcription]` TranscriptionConfig** (config.rs:237-381, defaults config.rs:1767-1796)
| field | default | notes |
|---|---|---|
| `engine: String` | `"auto"` | "auto", "whisper", "sherpa", "parakeet", "apple-speech". Auto picks sherpa Parakeet v3 only on Apple Silicon with the engine compiled and the model installed. Otherwise it uses Whisper. |
| `model: String` | `"small"` | Whisper ggml name |
| `model_path: PathBuf` | `<minutes_dir>/models` | the root for all models |
| `min_words` | 3 | |
| `language: Option<String>` | None | |
| `vad_model` | `"silero-v6.2.0"` | resolves to `ggml-silero-v6.2.0.bin` under model_path. An empty string disables VAD. |
| `vad_engine` | `"whisper-silero"` | `"ort-silero"` is opt-in experimental |
| `noise_reduction` | true | needs the `denoise` feature |
| `compressed_decode_fallback` | true | |
| `parakeet_binary/parakeet_model/parakeet_vocab/parakeet_*` | ... | parakeet.cpp is **safety-gated off** on every platform (resolves to Whisper) |
| `sherpa_model_dir: String` | "" | empty means `<model_path>/sherpa/parakeet-tdt-0.6b-v3-int8` or `$MINUTES_SHERPA_MODEL_DIR` |
| `partial_max_secs` | 30 | |
| `name_correction` | Off | |
| `apple_speech_shadow` | false | |

**`[diarization]` DiarizationConfig** (config.rs:386-411, defaults config.rs:1798-1809)
- `engine` = `"auto"`. "auto" enables pyannote-rs only if the model files exist (`resolve_diarization_engine` diarize.rs:2176). The other values are "none" and "pyannote-rs".
- `model_path` = `<minutes_dir>/models/diarization`. `model` = `"legacy"` or `"community-1"`. `threshold` = 0.4. `embedding_model` = `"cam++"` or `"cam++-lm"`. `stem_correlation_threshold` = 0.85.

**`[summarization]` SummarizationConfig** (config.rs:415-440, defaults config.rs:1811-1828)
- `engine` = `"none"` (compiled default). The other fields: `agent_command` = "claude", `agent_timeout_secs` = 300, `speaker_mapping_timeout_secs` = 30, `chunk_max_tokens` = 4000, `ollama_url` = "http://localhost:11434", `ollama_model` = "llama3.2", `openai_compatible_base_url` = "http://localhost:11434/v1", `openai_compatible_model` = "llama3.2", `openai_compatible_api_key_env` = "", `mistral_model` = "mistral-large-latest", `language` = "auto".
- **GOTCHA**: if a config file exists but has **no `[summarization].engine` key**, load forces `engine = "auto"` (`inspect_raw_toml_compat` / `apply_raw_toml_compat`, config.rs:2400-2420). "auto" means Apple FM, else whatever agent CLI it finds (claude, codex, gemini, opencode, agent). A sparse config written by our app therefore silently starts shelling out to `claude -p`. Always write `summarization.engine` explicitly.
- `openai_compatible_api_key_env == "MINUTES_OPENAI_COMPATIBLE_API_KEY"` gets cleared on load. It is a desktop-only marker (config.rs:14, 2410).

**Recording sources: `[recording]` RecordingConfig** (config.rs:1076-1125, defaults config.rs:1127-1143)
- `silence_reminder_secs` = 300, `silence_threshold` = 3, `silence_auto_stop_secs` = 1800, `max_duration_secs` = 28800, `min_disk_space_mb` = 500.
- `device: Option<String>`: input device override.
- `desktop_audio_source: Option<String>`: a desktop first-use choice. Skipped when None.
- `auto_call_intent` = false, `allow_degraded_call_capture` = false.
- `capture_backend: String` = `"cpal"`. The options are cpal loopback, the macOS Core Audio tap, or PocketStation.
- `sources: Option<SourcesConfig>` with `{ voice: Option<String>, call: Option<String> }` (config.rs:1119). When set, `device` is ignored. CLI `--source` overrides it.

**`[live_transcript]` LiveTranscriptConfig** (config.rs:1425-1462, defaults config.rs:1465-1478)
- `coverage_shadow` = false.
- `backend` = `"inherit"`. The values are inherit, whisper, parakeet, apple-speech. parakeet and apple-speech currently resolve to whisper.
- `model` = "". Empty means **`dictation.model`** (default "base"), not `transcription.model` (#756).
- `max_utterance_secs` = 30, `save_wav` = true, `recording_sidecar` = true.
- `promote_on_stop` = `Process`. The values are process, preserve, off.
- `shortcut_enabled`, `shortcut`.
- Helpers: `Config::effective_live_transcript_backend(&self) -> &str` (config.rs:1908), `standalone_live_backend_setting` (config.rs:1930).

**`[vault]` VaultConfig** (config.rs:1022-1030, defaults config.rs:1063-1072): `enabled` = false, `path` = "", `meetings_subdir` = "areas/meetings", `strategy` = "auto". The strategy values are auto, symlink, copy, direct.

Other sections a GUI may care about:
- `[identity]` (name, emails, aliases).
- `[consent] mode` = "remind".
- `[call_detection]`.
- `[calendar]`.
- `[voice]` (speaker-ID enrollment, `match_threshold` 0.65).
- `[retention]` (audio retention days, `auto_cleanup` = false).
- `[hooks] post_record`.
- `[dictation] model` = "base".
- `[watch] paths` = ["~/.minutes/inbox"].

### 1.4 Load and save APIs (`impl Config`, config.rs:1900-2389)
```rust
pub fn config_path() -> PathBuf
pub fn load() -> Self                                   // lenient: missing or invalid file gives Default (warns)
pub fn load_strict() -> Result<Self, String>            // missing file gives Default; unreadable or malformed gives Err
pub fn load_strict_from(path: &Path) -> Result<Self, String>
pub fn load_with_migrations() -> Self                   // what the Tauri app uses at startup (main.rs:1946); also retires legacy caches
pub fn load_with_migrations_from(path: &Path) -> Self   // may WRITE the file back (apple-speech, palette, dictation migrations)
pub fn load_from(path: &Path) -> Self
pub fn save(&self) -> std::io::Result<()>               // to config_path()
pub fn save_to(&self, path: &Path) -> std::io::Result<()>
pub fn upsert_string_setting_at(path: &Path, section: &str, key: &str, value: &str) -> std::io::Result<()>
pub fn ensure_dirs(&self) -> std::io::Result<()>        // creates output_dir, memos/, ~/.minutes/{inbox/{processed,failed},logs}, .metadata_never_index
pub fn minutes_dir() -> PathBuf
pub fn effective_live_transcript_backend(&self) -> &str
```
`Default` is implemented for `Config`, so `Config::default()` works.

How `save_to` works (config.rs:2217-2305):
- It serializes, then **merges into the existing document with toml_edit**. Unknown keys and comments survive, and keys you cleared are removed.
- It writes atomically through a tempfile and persist.
- It refuses a non-file path, or a malformed existing file (InvalidData).
- It fails with `ErrorKind::WouldBlock` ("config changed during save; reload before retrying") if another writer touched the file meanwhile.
- It logs changed key names to the JSON log.

`upsert_string_setting_at` edits a single string key in place. `minutes setup --model` uses it.

### 1.5 Env overrides (complete list found)
| var | effect | ref |
|---|---|---|
| `MINUTES_CONFIG_PATH` | full config path override | config.rs:1942 |
| `XDG_CONFIG_HOME` | config base dir | config.rs:1638 |
| `MINUTES_DATA_DIR` | `minutes_dir()` (absolute only) | config.rs:1618 |
| `HOME` | everything | config.rs:1607 |
| `MINUTES_HOME` | overlays.db and vocabulary.toml root ("correction_state_dir"), also MCP's state root | overlays.rs:129, vocabulary.rs:14; mcp index.ts:4794 |
| `MINUTES_LANG` | ui.language at runtime | config.rs:62 |
| `MINUTES_SHERPA_MODEL_DIR`, `MINUTES_SHERPA_STREAMING_MODEL_DIR` | sherpa model dirs | sherpa_engine.rs:45-70 |
| `MINUTES_OPENAI_COMPATIBLE_API_KEY` | desktop Keychain-hydrated key for openai-compatible summaries | config.rs:14, summarize.rs:2842 |
| `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `MISTRAL_API_KEY` | API summarizers | summarize.rs:2504/2638/2708 |
| `GEMINI_API_KEY` (configurable `voice_live.api_key_env`) | voice live | |
| `MINUTES_CLI_RESTRICTED_POLICY=deny` | CLI goes strict-config and assistant-safe (some cmds refuse) | cli main.rs:4429-4445 |
| `MINUTES_EMBED_WORKER` | semantic worker path | |

### 1.6 Can a host app supply config programmatically?
- **Yes, for most pipeline APIs.** Core functions take `&Config`: `pipeline::process_with_template(path, ct, title, config, ...)`, `summarize::summarize_with_template_report(..., config, ...)`, `resummarize::resummarize_meeting(path, config, opts)`, `health::check_all(&Config)`, `sherpa_engine::install_pinned_model(&Config, ...)`, `search::search(q, config, filters)`. Build a `Config` value and pass it.
- **But not fully isolated.** Some internals re-read the global file through `Config::load()`:
  - calendar.rs:154 and 440 (calendar filter and enabled flag)
  - desktop_context.rs:144
  - dictation.rs:1363 and 1492
  - diarize.rs:4319 (`compressed_decode_fallback`)
  - knowledge.rs:5386 (`load_strict`)
  
  These follow `MINUTES_CONFIG_PATH`.
- **State paths ignore `Config` entirely.** They come from env: `MINUTES_DATA_DIR`/`HOME` for `minutes_dir()`, and `MINUTES_HOME` for overlays and vocabulary.
- **`voice::db_path()` (voice.rs:135) is hardcoded to `dirs::home_dir()/.minutes/voices.db`.** It ignores `MINUTES_DATA_DIR`. `template.rs:333` (`~/.minutes/templates`) likewise uses only `HOME`.
- Recommended approach:
  - Share the standard global file (`~/.config/minutes/config.toml`) so the CLI and MCP see the same settings.
  - Use `Config::load_strict()` and `save()`/`upsert_string_setting_at`.
  - For a sandbox, set `MINUTES_CONFIG_PATH` + `MINUTES_DATA_DIR` + `MINUTES_HOME` together, process-wide, before calling core. voices.db still escapes this.

### 1.7 Full inventory of files and dirs Minutes writes (for "Reset")

**A. Config (wipe on Reset)**
- `~/.config/minutes/config.toml`
- `~/.config/minutes/customize.toml`

**B. State root `~/.minutes/` (`Config::minutes_dir()`)**. Observed on this Mac: `.metadata_never_index`, `activation-state.json`, `bin/apple-speech-helper`, `chat/`, `desktop-control/`, `inbox/`, `jobs/`, `lib/apple-speech-helper.swift`, `logs/`, `models/ggml-base.bin` (148 MB), `palette_first_run_shown`, `recall-workspace.json`, `whats-new.json`.

Paths referenced in code:

| Category | Paths |
|---|---|
| Models (big; offer "keep models") | `models/ggml-{tiny,base,small,medium,large-v3}.bin`; `models/ggml-silero-v6.2.0.bin`; `models/silero-vad-v6.2.0.onnx` (vad-ort builds); `models/diarization/{segmentation-3.0.onnx, segmentation-community-1.onnx, wespeaker_en_voxceleb_CAM++.onnx, voxceleb_CAM++_LM.onnx, embedding-community-1.onnx}`; `models/sherpa/parakeet-tdt-0.6b-v3-int8/{encoder,decoder,joiner}.int8.onnx + tokens.txt`; `models/sherpa/orukeet-v0.1.0-int8/`; `models/sherpa/streaming-zipformer-en-20M-2023-02-17/`; `models/embedding/embeddinggemma-2-q8_0/embeddinggemma-2-Q8_0.gguf` (semantic, ~310 MB); `models/parakeet/...` (legacy); lock files `models/.minutes-model-setup.lock`, `models/sherpa/.parakeet-v3-install.lock`; `*.partial` / `*.bin.download` temp files |
| User data, not config (decide per Reset policy) | `voices.db` (voice profiles, voice.rs:135); `overlays.db` (speaker corrections, overlays.rs:185); `vocabulary.toml` (vocabulary.rs:14); `context.db` (desktop context, context_store.rs:396); `dictation-history.json`; `dictation-recovery/`; `dictation-failed/`; `templates/` (user summary templates); `voice-sessions/`; `assistant/` (Terminal workspace with `.claude/skills`); `preps/`; `briefs/`; `automations/`; `automation-runs/`; `decision-boards/`; `reading-lists/`; `prototypes/`; `prototype-tests/`; `work-capsules/`; `artifact-snapshots/`; `preserved-knowledge/`; `music/`; `research/apple-speech/`; `demo/` (demo corpus); `semantic/` (semantic index, semantic_index.rs:42) |
| Raw audio and recovery (may contain unprocessed meetings!) | `native-captures/` (incl. `call.voice.wav`); `current.wav`; `live-transcript.wav`; `live-transcript.jsonl`; `inbox/{,processed,failed}`; `screens/` |
| Jobs and queue | `jobs/` and `jobs/archive/` (jobs.rs:273-290); `processing-status.json`; `processing-worker.pid` |
| PID, sentinels, live state (only valid while running) | `recording.pid`; `recording.stop`; `recording-start.txt`; `recording-meta.json`; `last-result.json`; `current-notes.md`; `current-context.txt`; `current-consent.json`; `extend.sentinel`; `dictation.pid`; `voice.pid`; `live-transcript.pid`; `live-transcript-status.json`; `live-remote-audio.json`; `copilot.{pid,stop,pause,feedback.json}`; `copilot-status.json`; `dashboard.pid`; `watch.lock`; sensitive-session file and lock (sensitive.rs:104-110); mic-mute sentinel (streaming.rs:162); `desktop-control/{requests,responses,...}` |
| Event bus | `events.jsonl`; `events.0001.jsonl`; `events.0002.jsonl`; `events.seq`; `events.seq.tmp`; `events.lock`; `agents.allow` (agent allowlist) |
| Logs | `logs/minutes.log` (logging.rs:17-24) and other diagnostics; `logs/mcp-crash.log` (MCP); `dictation-startup-debug.jsonl`; `dictation-focus-debug.jsonl` |
| App UI state | `activation-state.json`; `whats-new.json`; `palette.json`; `palette_first_run_shown`; `recent-artifacts.json`; `recall-workspace.json`; `parakeet-invite.json`; `tag-nudge-shown`; `chat/` |
| Helpers and binaries | `bin/apple-speech-helper`; `bin/apple-fm-helper`; `lib/` (Swift sources); `tmp/` |
| Legacy caches (retired at startup by `policy_fs::retire_legacy_policy_caches`) | `search.db`; `graph.db`; QMD mirror dir (`knowledge::qmd_policy_mirror_path`) |

**C. Meetings dir `output_dir` (default `~/meetings`). Keep on "Reset but keep meetings"**
- `*.md` meetings
- `*.wav` audio beside meetings (observed)
- `memos/`
- `archive/` (target of `minutes delete`)
- `failed-captures/` (retention.rs:228)
- `daily/` (daily_notes default path `~/meetings/daily`)
- `.metadata_never_index`
- hidden resummarize backups `.<name>.pre-resummarize.<ts>.bak` (resummarize.rs:956)

The vault (if enabled) may contain symlinks or copies at `vault.path/meetings_subdir`.

**D. Outside both (Desktop app / CLI side effects)**
- LaunchAgents from `minutes service install`: `~/Library/LaunchAgents/dev.getminutes.{watcher,weekly-summary,proactive-context}.plist`, plus legacy `dev.getminutes.weekly-lint.plist` (cli main.rs:9215-9500). `minutes service uninstall` removes them.
- Tauri app (`com.useminutes.desktop`):
  - `app_config_dir()/cli_setup_state.json` (cli_setup.rs:476)
  - `~/Library/Caches/com.useminutes.desktop`
  - `~/Library/Preferences/com.useminutes.desktop.plist` (both observed on this Mac)
  - The app's "Set up CLI" symlinks `~/.local/bin/minutes`.
- Keychain items (tauri secret_store.rs:6-13): service `"Minutes OpenAI-compatible Summaries"` / account `"default"`, and service `"Minutes Voice (Gemini)"` / account `"default"`.
- Claude Desktop MCPB extension config (managed by Claude Desktop, not Minutes).

**Reset recipe implications:** stop the recorder, live, dictation, copilot and processing workers, and the watcher LaunchAgent first. The pid files and sentinels are the coordination protocol. Then delete A and the chosen parts of B. Leave C untouched for "keep meetings". Optionally keep `models/` to avoid re-downloading GBs. Note that `voices.db`, `overlays.db` and `vocabulary.toml` are user-curated data, not config.

---

## 2. Existing reset / uninstall functionality
Grep for `reset|uninstall|wipe|factory` across crates/cli, crates/core, tauri/src-tauri, tauri/src finds **no factory-reset or "wipe config" feature anywhere**. The closest partial tools:
- `minutes service uninstall`: removes LaunchAgents (cli main.rs:1092-1098 clap, impl main.rs:9427-9443; Linux main.rs:9584).
- `minutes voices --delete` / `minutes voice delete-all`: voice profiles (cli/voice.rs `VoiceAction::DeleteAll`).
- `minutes demo --clean`: removes demo meetings.
- `minutes cleanup [--apply] [--older-than 14d] [--json]`: raw-audio retention cleanup (core `retention::preview_audio_retention` / `apply_audio_retention`).
- `minutes delete <slug> [--with-audio] [--force]`: per-meeting.
- `minutes qmd cleanup`: legacy QMD state.
- `transcription.parakeet_fp16_blacklist_reset`: unrelated.

We must implement Reset ourselves from the inventory above.

---

## 3. Model setup / download

### 3.1 `minutes setup` (CLI clap at main.rs:661-703, dispatch main.rs:2507-2570)
Flags:
- `-m/--model <tiny|base|small|medium|large-v3>`, or with `--diarization`: `legacy|community-1`
- `--download-only` (requires --model; doesn't change config)
- `--vad`
- `--list` (human text only)
- `--diarization`
- `--parakeet` (no-op compat, gated)
- `--parakeet-model`
- `--sherpa`
- `--sherpa-model orukeet`
- `--demo`

With no flags it plans through `setup_plan` (main.rs:7946). It installs the configured Whisper model, plus the dictation and live models if they differ, plus sherpa Parakeet v3 on Apple Silicon builds with the sherpa plugin.

| model | URL | dest | integrity |
|---|---|---|---|
| Whisper ggml (`cmd_setup` main.rs:8003) | `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{model}.bin` (unpinned `main`) | `<model_path>/ggml-{model}.bin` | **size floor only** via `transcribe::expected_whisper_model_size_bytes` (transcribe.rs:2576). No SHA. |
| Silero VAD (`download_vad_models` main.rs:8195, also auto-run after any whisper setup) | `https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v6.2.0.bin` (~885 KB) | `<model_path>/ggml-silero-v6.2.0.bin` | none. A failure is only a warning. |
| Silero ONNX (only `vad-ort` builds) | `https://github.com/snakers4/silero-vad/raw/v6.0/src/silero_vad/data/silero_vad.onnx` | `<model_path>/silero-vad-v6.2.0.onnx` | none |
| Diarization (`cmd_setup_diarization` main.rs:8285; URLs in diarize.rs:505-560) | legacy seg: `github.com/thewh1teagle/pyannote-rs/releases/download/v0.1.0/segmentation-3.0.onnx`; community-1 seg: `huggingface.co/altunenes/speaker-diarization-community-1-onnx/resolve/main/segmentation-community-1.onnx`; emb cam++: `.../pyannote-rs/releases/download/v0.1.0/wespeaker_en_voxceleb_CAM++.onnx`; cam++-lm: `huggingface.co/Wespeaker/wespeaker-voxceleb-campplus-LM/resolve/main/voxceleb_CAM%2B%2B_LM.onnx`; community-1 emb: `.../embedding_model.onnx`, saved as `embedding-community-1.onnx` | `diarization.model_path` | **none (existence only)**. It does NOT write config; it prints instructions. engine=auto picks the models up automatically. |
| Sherpa Parakeet v3 int8 (CLI `--sherpa`, main.rs:8594) | `huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/main/{encoder,decoder,joiner}.int8.onnx, tokens.txt` (~670 MB) | `<model_path>/sherpa/parakeet-tdt-0.6b-v3-int8/` | CLI path: size floors (`sherpa_engine::MODEL_FILES`). Refuses on builds without `engine-sherpa`. |
| Sherpa Parakeet v3 **pinned** (desktop path) | `.../resolve/2bda32ec70b097a55adaa07d9a7173915b43cc78/<file>` | same default dir | **exact bytes + SHA-256** (`PINNED_MODEL_FILES` sherpa_engine.rs:86-108) |
| Orukeet (`--sherpa-model orukeet`) | `huggingface.co/oruk/orukeet/resolve/eac739d.../onnx/sherpa-v0.1.0-int8` | `<model_path>/sherpa/orukeet-v0.1.0-int8/` | manifest SHA (orukeet.rs:10-13) |
| Semantic EmbeddingGemma 2 (`minutes semantic ...`) | `huggingface.co/unsloth/embeddinggemma-2-GGUF/resolve/031f0d4.../embeddinggemma-2-Q8_0.gguf` (~310 MB) | `<model_path>/embedding/embeddinggemma-2-q8_0/` | SHA (semantic.rs:20, 69) |
| parakeet.cpp | — | — | gated off, installs nothing |

CLI downloader `download_file(url, dest)` (main.rs:8714) is **private to the CLI binary**:
- ureq client
- writes `dest.partial` then renames it
- 8 GiB / Content-Length cap
- progress printed to **stderr** as `\r  X / Y MB (Z%)` every 500 ms
- a cross-process lock `models/.minutes-model-setup.lock` serializes it (main.rs:8164)

### 3.2 Reusable pub functions with progress callbacks (minutes-core)
- `pub fn sherpa_engine::install_pinned_model(config: &Config, progress: &mut dyn FnMut(u64, u64)) -> Result<PathBuf, String>` (sherpa_engine.rs:128). Progress is `(bytes done, total)`. It is SHA-verified, resumable per file, and the module is always compiled (lib.rs:216-218). **This is the only core download fn with progress.** It refuses if a custom `sherpa_model_dir` is set.
- `pub fn sherpa_engine::pinned_model_bytes() -> u64`, `model_dir(&Config)`, `default_model_dir(&Config)`, `model_files_present(&Path)`.
- `pub fn orukeet::install(model_root: &Path) -> Result<PathBuf, String>`: no progress.
- `pub fn semantic::install_model(model_root: &Path) -> Result<PathBuf, String>` and `model_installed(&Path)`: no progress.
- `pub fn diarize::diarization_model_info(name) -> Option<&'static DiarizationModelInfo>`, `embedding_model_for_config(&Config) -> &'static EmbeddingModelInfo`, `models_installed(&Config) -> bool`. These give filenames and URLs only, with no downloader.
- `pub fn transcribe::expected_whisper_model_size_bytes(&str) -> Option<u64>`, `transcription_engine_resolution(&Config)`, `parakeet_install_offered(&Config)`, `sherpa_selectable(&Config)`, `resolve_model_path_by_name(...)`.
- `pub fn health::check_all(&Config) -> Vec<HealthItem{label,state,detail,optional}>` (health.rs:26-40) covers model, VAD, ffmpeg, and more. It is good for an onboarding checklist.
- **No core fn downloads Whisper, VAD or diarization models.** Copy the Tauri reqwest streaming loop, or shell out to `minutes setup` and parse stderr `\r` progress.

### 3.3 How the Tauri app shows progress
- Whisper: `#[tauri::command] pub async fn cmd_download_model(app, state, model: String) -> Result<String,String>` (commands.rs:10587).
  - It validates against the allowlist tiny, base, small, medium, large-v3 (commands.rs:3032).
  - It has its own reqwest `bytes_stream()` loop (commands.rs:10487-10580) writing `ggml-{m}.bin.download`, then renames.
  - It emits the event **`"download-model"`** with payload `ModelDownloadProgress { model, status: "downloading"|"complete"|"failed", downloaded_bytes, total_bytes: Option<u64>, percent: Option<u8>, message: Option<String> }` (commands.rs:10443, 10462).
  - A global `MODEL_DOWNLOAD_ACTIVE` AtomicBool stops concurrent downloads. The size floor is checked after download.
  - The frontend `listen('download-model', ...)` in tauri/src/index.html (~14898).
  - It does **not** download VAD.
- Parakeet v3: `parakeet_install_commands.rs`.
  - `cmd_install_parakeet()` spawns a thread that calls `sherpa_engine::install_pinned_model` with a closure updating a static `Mutex<InstallProgress{phase,done,total,error}>`. The UI **polls** `cmd_parakeet_install_status()` -> `ParakeetInstallStatus{offered, invite, switches_from_whisper, download_bytes, phase, done, total, error}`.
  - After install it moves `engine="whisper"` to `"auto"` and verifies `transcription_engine_resolution(..).engine == "sherpa"`.
- The desktop app has **no diarization download**. It points users to the CLI.

---

## 4. Summaries (`crates/core/src/summarize.rs`, `ollama.rs`, `resummarize.rs`)

### 4.1 Backends (`summarization.engine`, dispatch summarize.rs:208-275)
| engine | how invoked | config / keys |
|---|---|---|
| `"none"` (compiled default) | skip and return `summary: None` (summarize.rs:183) | — |
| `"auto"` | Apple Foundation Models if `apple_fm::is_available()`, else `detect_agent_cli()` (claude > codex > gemini > opencode > agent, summarize.rs:984), else skip | — |
| `"agent"` | subprocess of `agent_command` (default `claude`). For claude it runs `claude -p ... -` with the prompt on stdin, `--allowedTools Read --add-dir` for screenshots, and lean mode for speaker mapping. codex uses `exec`, gemini `-p`, opencode `run`, pi `-p` (summarize.rs:1662+). Map-reduce chunking with a coverage report. | `agent_command`, `agent_timeout_secs`, `speaker_mapping_timeout_secs`. Path resolution searches ~/.cargo/bin, ~/.local/bin, /opt/homebrew/bin, and more, because GUI apps get a minimal PATH (summarize.rs:1029) |
| `"apple"` / `"apple-fm"` | on-device Apple FM (macOS 26+, via `~/.minutes/bin/apple-fm-helper`) | — |
| `"claude"` | Anthropic Messages API via ureq, model `claude-sonnet-4-20250514` hardcoded (summarize.rs:666) | `ANTHROPIC_API_KEY` env |
| `"openai"` | OpenAI chat API, `gpt-4o-mini` (`gpt-4o` with screens) | `OPENAI_API_KEY` env |
| `"mistral"` | Mistral API | `MISTRAL_API_KEY`, `mistral_model` |
| `"ollama"` | `POST {ollama_url}/api/generate`, non-streaming, per chunk (summarize.rs:2985) | `ollama_url`, `ollama_model` |
| `"openai-compatible"` / `"openai_compatible"` | `POST {base}/chat/completions`. The key comes from the env var named in `openai_compatible_api_key_env`. If that is empty: no key for local URLs, else `MINUTES_OPENAI_COMPATIBLE_API_KEY` (summarize.rs:2831) | `openai_compatible_base_url/model/api_key_env` |
| other | warn and skip | |

- The HTTP timeout for API engines is 120 s.
- `ollama.rs` is a separate streaming NDJSON adapter (`OllamaAdapter`, `CancelToken`, `OllamaHealth`). Recall chat and copilot use it, not batch summaries.
- **Summary is optional.** Every failure path returns `SummaryReport{summary: None}` and the pipeline continues. The meeting markdown is still written, with transcript only. Agent timeouts add `processing_warnings` and status `degraded`.
- There is also LLM speaker mapping (`map_speakers*`) and title refinement on the same engine.

Pub API:
```rust
pub fn summarize(transcript: &str, config: &Config) -> Option<Summary>
pub fn summarize_with_screens(transcript: &str, screen_files: &[PathBuf], config: &Config, log_file: Option<&str>) -> Option<Summary>
pub fn summarize_with_template(transcript: &str, user_notes: Option<&str>, screen_files: &[PathBuf], config: &Config, template: Option<&Template>, log_file: Option<&str>) -> Option<Summary>
pub fn summarize_with_template_report(... same ...) -> SummaryReport { summary: Option<Summary>, coverage: Option<SummaryCoverage> }
pub fn format_summary(summary: &Summary) -> String
pub fn detect_agent_cli() -> Option<String>
pub struct Summary { title: Option<String>, text, decisions, action_items, open_questions, commitments, key_points, participants: Vec<String> }
```

### 4.2 Re-summarize an existing meeting (`resummarize.rs`)
```rust
pub fn resummarize_meeting(path: &Path, config: &Config, opts: &ResummarizeOptions) -> Result<ResummarizeReport, ResummarizeError>  // resummarize.rs:685
pub struct ResummarizeOptions { pub apply: bool, pub template_override: Option<String>, pub refresh: crate::derived::RefreshOptions }  // Default
pub struct ResummarizeReport { path, applied, backup: Option<PathBuf>, engine, model, template: Option<String>, new_ai_body, sections_replaced, merge_notes: Vec<MergeNote>, action_items, decisions, duration_ms, refresh: Option<RefreshReport> }
pub enum ResummarizeError { Unsupported(String), Frontmatter(String), TemplateUnavailable{..}, SummarizeFailed{..}, ConcurrentEdit, Markdown(..), Io(..) }  // error.rs:191
```
- To override the engine, clone the config and set `summarization.engine`.
- `apply=false` is a preview, but **it still calls the model**.
- On apply it writes a hidden backup `.<file>.pre-resummarize.<ts>.bak` beside the meeting (`prune_resummarize_backups`).
- It replaces only the `## Summary/Decisions/Action Items/Open Questions/Commitments` sections and merges action-item and decision status.
- engine "none" is a hard error, not a write.
- CLI: `minutes resummarize <path|search-term> [--apply] [--engine X] [--template slug] [--ingest] [--json]`.

---

## 5. MCP (`crates/mcp`, TypeScript, npm `minutes-mcp` v0.32.1)

What it does:
- A stdio MCP server (plus an HTTP transport) for Claude Desktop, Claude Code, Cursor and others. It also ships an MCP-Apps dashboard UI.
- It has about 34 tools, including: start/stop_recording, get_status, list/search/get_meeting, process_audio, add_note, start/stop_dictation, start_live_transcript, read_live_transcript, list_processing_jobs, resummarize_meeting, ingest_meeting, list_voices, confirm_speaker, copilot tools, context tools (activity_summary, search_context, get_moment, get_screen_context), consistency_report, get_person_profile, track_commitments, relationship_map, research_topic, knowledge_status, add_agent_annotation.

How it works:
- **Reads** use `minutes-sdk` (crates/sdk) to parse markdown and YAML frontmatter **directly from the meetings dir**.
- **Actions** shell out with `execFile` to the `minutes` CLI: `status`, `stop`, `get`, `transcript`, `voices`, `copilot`, `health --json`, `capabilities --json`, `setup --model X --download-only`, `meetings-root --json`, `policy-audit`.
- Binary discovery `findMinutesBinary()` (index.ts:2185) checks these in order:
  1. repo `target/{release,debug}`
  2. `~/.cargo/bin`
  3. `~/.local/bin`
  4. `/opt/homebrew/bin`
  5. `/usr/local/bin`
  6. `/Applications/Minutes.app/Contents/MacOS/minutes`, `~/Applications/Minutes.app/...`, `Minutes Dev.app`
  7. PATH
  
  If none is found, it can auto-install by downloading a release, or via brew or cargo (index.ts:2350-2465).
- Meetings dir: `getEffectiveMeetingsDir()` (index.ts:4830) uses the `MEETINGS_DIR` env if set. Else it runs `minutes meetings-root --json` (a hidden CLI command returning `{schema_version:1, output_dir}`, which reads `output_dir` from the same config.toml). If there is no CLI, it falls back to `~/meetings`.
- State dir: `MINUTES_HOME` env, else `~/.minutes`.
- On boot, unless `MINUTES_MCP_AUTO_SETUP=0`, it runs `minutes health --json` and can call `minutes setup --model <configured> --download-only`.

Install and run:
- `npx minutes-mcp`, or a `claude_desktop_config.json` entry (crates/mcp/claude-desktop-config.json).
- Or the MCPB extension (`manifest.mcpb.json`: `node ${__dirname}/crates/mcp/dist/index.js` with env `MINUTES_HOME=${user_config.minutes_home}` default `${HOME}/.minutes` and `MEETINGS_DIR=${user_config.meetings_dir}` default `${HOME}/meetings`).
- The CLI and Tauri app do not write MCP client configs.

To keep MCP working, our app must:
1. Write meetings in the same markdown and frontmatter format into config `output_dir`. Keep the default `~/meetings`, because the **MCPB extension pins `MEETINGS_DIR` to `~/meetings` by default** and overrides config.
2. Keep config in the standard path and state in `~/.minutes`.
3. Keep pid files, `last-result.json` and the jobs dir semantics (using minutes-core capture/pid APIs does this), so MCP `get_status` / `stop_recording` (which run `minutes status` / `minutes stop`) see recordings our app started.
4. Ensure a `minutes` CLI is discoverable at one of the paths above, for example by symlinking `~/.local/bin/minutes` like the official app does. Otherwise MCP tries to auto-install one.

---

## 6. CLI machine-readable output (crates/cli/src/main.rs, `enum Commands` main.rs:329-1385)

Convention: human text goes to **stderr** and JSON to **stdout**. Many commands print JSON on stdout even without a flag. The `--json` envelope (`json_envelope`, main.rs:46-153) is `{ ok: true, command: "minutes X", data: T, meta: { schemaVersion: 1, generatedAt } }`. The global flag is only `-v/--verbose`.

**Always JSON on stdout (no flag):**
- `minutes status`: `RecordingStatus{recording, processing, processing_stage, recording_mode, processing_title, processing_job_id, processing_job_count, pid, duration_secs, wav_path}` (pid.rs:761). Under `MINUTES_CLI_RESTRICTED_POLICY=deny` it returns a reduced set.
- `minutes list [-l N] [-t meeting|memo] [--sync|--no-sync] [--include-restricted]`: pretty JSON array on stdout. When empty it prints `[]`. It is the same as search with an empty query (main.rs:4657).
- `minutes search <q> ... ` in default text mode prints a pretty JSON array on stdout. `--format json` gives **JSONL** (one object per line, empty output when no results). Flags: `-t`, `-s/--since`, `-l`, `--intents-only`, `--intent-kind`, `--owner`, `--sync/--no-sync`, `--include-restricted`.
- `minutes actions [-a name] [--include-restricted]`: JSON array.
- `minutes devices`: JSON array of names. `minutes sources`: grouped JSON.
- `minutes stop`: blocks up to 120 s and prints `last-result.json` contents or `{status:"queued", job_id, title, mode}`.
- `minutes record ...`: foreground and blocking. At the end it prints the queued-result JSON.
- `minutes process <path> [-t meeting|memo] [-n note] [--title] [-l lang] [--template]`: `{status:"done", file, title, words}`.
- `minutes schema`: JSON Schema of the frontmatter.

**With `--json`:**
- `health --json` (envelope + top-level engine, effective_engine, reason)
- `capabilities --json`
- `paths --json` (`{config_path, minutes_dir, output_dir}`)
- `storage --json`
- `cleanup [--apply] [--older-than] --json`
- `jobs [--all] [-l N] --json`
- `get <slug|path> --json [--compact-json] [--include-restricted]`
- `voices --json [--delete]`
- `transcribe <audio> --json [-l] [--diarize]`
- `resummarize <m> [--apply] [--engine] [--template] [--ingest] --json`
- `redo-speaker-mapping <m> [--apply] [--engine] --json`
- `people [--rebuild] [-l] --json`, `people merge ... --json`
- `commitments [-p] [-l] --json`
- `automate <weekly-summary|proactive-context> --json`
- `vocabulary {add,list,remove,rebuild,suggest} --json`
- `context {status,search,activity-summary,get-moment,screen} --json`
- `copilot {status,eval} --json`
- `semantic search --json`
- `apple-speech capabilities --json`
- `talk --json` (NDJSON)
- Hidden: `preflight-record --json`, `meetings-root --json`, `knowledge-status --json`, `agent-readiness --json`

**Format flag:**
- `transcript [--since N|5m] [--status] [--format json|text, default json] [--include-current]`: live transcript reads.
- `events [-l] [--event-type] [--since] [--follow] [--since-seq N]`: NDJSON stream with `--follow`.

**No JSON:**
- `setup` (all variants; progress and messages on stderr only)
- `service`, `logs`, `export` (CSV)
- `note`, `extend`, `mic-toggle`
- `live` (foreground)
- `dictate` (`--stdout` gives text)
- `template list`, `vault`, `enroll`, `confirm`, `delete`, `clean`, `ingest`, `research`, `person`, `consistency`, `insights`
