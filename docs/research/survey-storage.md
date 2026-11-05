# minutes-upstream survey: Diarization, Speakers, Meeting Storage

Upstream: `~/dev/minutes-upstream` (read-only). Paths below are relative to that root.
Core crate version: 0.32.1 (see `crates/cli/Cargo.toml:50`).

---

## 1. Diarization

### 1.1 Feature flags (`crates/core/Cargo.toml:12-34`)

```toml
default = ["whisper"]
diarize = ["dep:pyannote-rs", "dep:knf-rs", "dep:ort", "dep:ndarray"]
streaming-diarize = ["streaming"]   # subprocess IPC only, no ort/parakeet deps
streaming = ["dep:crossbeam-channel"]
vad-ort = ["streaming", "dep:ort", "dep:ndarray"]
```

- `diarize` is NOT on by default in core. The CLI turns it on by default
  (`crates/cli/Cargo.toml:28` `default = ["whisper", "diarize"]`). The Tauri app
  uses `minutes-core = { path = ..., features = ["streaming", "diarize"] }`
  (`tauri/src-tauri/Cargo.toml:32`).
- Your app's git dependency must say `features = ["diarize", ...]`. Without it,
  `diarize()` returns `None` with reason `EngineUnresolved`, and
  `voice::embed_solo_clip` returns an error (`voice.rs:538-545`).
- `diarize.rs` is always compiled (`lib.rs:141`). The ML internals are behind
  `#[cfg(feature = "diarize")]`.
- `streaming_diarize` is compiled only with `streaming-diarize` (`lib.rs:227-228`).

### 1.2 Public API (`crates/core/src/diarize.rs`)

Types:
- `pub struct SpeakerSegment { pub speaker: String, pub start: f64, pub end: f64 }` (174)
- `pub struct SpeakerEmbeddingSegment { pub embedding: Vec<f32>, pub embedding_seconds: f64, pub speech_seconds: f64 }` (185, memory only)
- `pub struct DiarizationResult` (194): `segments`, `num_speakers`, `system_dominant_ratio`,
  `voice_dominant_ratio`, `degraded_capture: Option<DegradedCapture>`, `from_stems`, `source_aware`,
  `speaker_embeddings: HashMap<String, Vec<f32>>` (per-label centroid),
  `speaker_embedding_segments: HashMap<String, Vec<SpeakerEmbeddingSegment>>`. Implements `Default`.
- `pub enum Confidence { High, Medium, Low }` (297), serialized lowercase.
- `pub enum AttributionSource { Deterministic, Llm, Enrollment, Manual, MlBleedDegraded ("ml-bleed-degraded"), StemRecovery ("stem-recovery"), Unknown(String) /* untagged */ }` (306)
- `pub struct SpeakerAttribution { pub speaker_label: String, pub name: String, pub confidence: Confidence, pub source: AttributionSource }` (328). This is the `speaker_map` entry type.
- `pub enum DiarizationPurpose { PrimaryMeeting, Auxiliary }` (336)
- `pub struct DiarizationContext<'a> { pub purpose, pub transcript_windows: Option<&'a [TranscriptWindow]> }` (342)
- `pub struct TranscriptWindow { pub start_secs: f32, pub end_secs: f32 }` (348)
- `pub enum DiarizationUnavailable { Disabled, EngineUnresolved, EngineFailed(String), SystemStemOnlyFailed, WorkerBusy, WorkerTimedOut, WorkerPanicked, CapabilityRejected }` (363), with `reason_code()` and `detail()`.
- `pub enum DiarizationOutcome { Result(DiarizationResult), Skipped { reason: DegradedCapture }, NotConfigured { reason: DiarizationUnavailable } }` (432)
- `DegradedCapture`, `FailureKind`, `CaptureSource`, `ObservedSignal`, `DiagnosticConfidence` (237-286)
- `pub struct StemPaths` (987), `pub enum StemSignal` (1036)

Functions:
```rust
pub fn diarize_with_context(audio_path: &Path, config: &Config, ctx: DiarizationContext<'_>) -> DiarizationOutcome   // 2900
pub fn diarize(audio_path: &Path, config: &Config) -> Option<DiarizationResult>                                       // 3065 (Auxiliary purpose)
pub fn apply_speakers(transcript: &str, result: &DiarizationResult) -> String                                       // 3082: "[m:ss] text" -> "[SPEAKER_N m:ss] text"
pub fn apply_confirmed_names(transcript: &str, attributions: &[SpeakerAttribution]) -> String                       // 456: rewrites only High-confidence labels
pub fn diarize_from_stems(stems: &StemPaths, config: &Config) -> Option<DiarizationResult>                          // 2122
pub fn discover_stems(audio_path: &Path) -> Option<StemPaths>                                                       // 1576
pub fn models_installed(config: &Config) -> bool                                                                    // 694
pub fn diarization_model_info(name: &str) -> Option<&'static DiarizationModelInfo>                                  // 589
pub fn diarization_model_for_config(config: &Config) -> &'static DiarizationModelInfo                               // 599
pub fn embedding_model_info(name: &str) -> Option<&'static EmbeddingModelInfo>                                      // 605
pub fn embedding_model_for_config(config: &Config) -> &'static EmbeddingModelInfo                                   // 617
#[cfg(feature = "diarize")]
pub fn extract_speaker_embeddings(windows: &[Vec<i16>], config: &Config) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error>> // 645
pub fn audio_duration_secs(audio_path: &Path) -> Result<f64, String>                                                // 936
pub fn classify_stem_signal(path: &Path) -> StemSignal; pub fn stem_has_audio(path: &Path) -> bool                  // 1045, 1061
```
Constants: `DIARIZATION_MODEL_NAMES = ["legacy", "community-1"]`, `EMBEDDING_MODEL_NAMES = ["cam++", "cam++-lm"]`.

Runtime limits: one process-global diarization worker (`WorkerBusy` if one is
already running), 15-minute deadline, maximum 2 hours of audio (`diarize.rs:9-11`).
The worker uses ffmpeg (if present) to convert the audio to 16 kHz mono s16.

Engines (doc comment at `diarize.rs:2890-2899`): `"auto"` (default; pyannote-rs
if the models are on disk, else skip), `"pyannote-rs"`, `"pyannote"` (legacy Python), `"none"`.

### 1.3 Models

Config: `[diarization]` in `~/.config/minutes/config.toml` (`config.rs:386-411`, defaults `1798-1808`):
```rust
pub struct DiarizationConfig {
    pub engine: String,            // "auto"
    pub model_path: PathBuf,       // ~/.minutes/models/diarization   (minutes_dir() honours MINUTES_DATA_DIR, config.rs:1617)
    pub model: String,             // "legacy" | "community-1"
    pub threshold: f32,            // 0.4
    pub embedding_model: String,   // "cam++" | "cam++-lm"
    pub stem_correlation_threshold: f32, // 0.85
}
```

| Set | File | URL |
|---|---|---|
| legacy segmentation | `segmentation-3.0.onnx` | `https://github.com/thewh1teagle/pyannote-rs/releases/download/v0.1.0/segmentation-3.0.onnx` |
| legacy embedding `cam++` (default) | `wespeaker_en_voxceleb_CAM++.onnx` | `https://github.com/thewh1teagle/pyannote-rs/releases/download/v0.1.0/wespeaker_en_voxceleb_CAM++.onnx` |
| embedding `cam++-lm` | `voxceleb_CAM++_LM.onnx` | `https://huggingface.co/Wespeaker/wespeaker-voxceleb-campplus-LM/resolve/main/voxceleb_CAM%2B%2B_LM.onnx` |
| community-1 segmentation | `segmentation-community-1.onnx` | `https://huggingface.co/altunenes/speaker-diarization-community-1-onnx/resolve/main/segmentation-community-1.onnx` |
| community-1 embedding (forced with community-1) | `embedding-community-1.onnx` | `https://huggingface.co/altunenes/speaker-diarization-community-1-onnx/resolve/main/embedding_model.onnx` |

(`diarize.rs:504-623`.) Model version tags such as `wespeaker_en_voxceleb_CAM++_v0.3`
are stored with voice profiles. Profiles only match the same model.

Download: `minutes setup --diarization [--model legacy|community-1]` ->
`cmd_setup_diarization` (`crates/cli/src/main.rs:8285-8353`). It downloads the two
files into `config.diarization.model_path`. It does NOT write config. It only
prints instructions to set `engine = "pyannote-rs"`. With `engine = "auto"` the
models are used automatically when present. Total size is about 34 MB.
Your app can call `diarize::diarization_model_for_config` and
`embedding_model_for_config` and download the `*_url` fields itself.

### 1.4 `sidecars/diarize-streaming`

- Rust binary `minutes-diarize-sidecar` (`sidecars/diarize-streaming/Cargo.toml`). It runs NVIDIA
  Sortformer (4-speaker streaming) through `parakeet-rs 0.3.6` (feature `sortformer`) and ONNX `ort` rc.12. No Python.
- It has its OWN cargo workspace (`[workspace]` in its Cargo.toml) because ort rc.12/ndarray 0.17
  conflicts with pyannote-rs's ort rc.10/ndarray 0.16. Build: `cargo build --release` in that directory.
- Model: manual download, about 492 MB:
  `https://huggingface.co/altunenes/parakeet-rs/resolve/main/diar_streaming_sortformer_4spk-v2.1.onnx`. Latency is about 10 s, and 4 speakers is the maximum.
- Protocol v1 (README): args `--model <onnx> [--config callhome|dihard3]`. stdin carries frames of u32 LE length plus f32 LE PCM at 16 kHz mono. stdout is NDJSON: `{"event":"ready",...}`, then `{"start_ms","end_ms","speaker"}` per segment, then `{"event":"flush_done"}`. You must drain stdout on a separate thread.
- Consumer in core: `crates/core/src/streaming_diarize.rs` (feature `streaming-diarize`):
  - `pub struct DiarSegment { pub start_ms: u64, pub end_ms: u64, pub speaker: u32 }` (26)
  - `impl StreamingDiarizeSidecar { pub fn spawn(bin: &Path, model: &Path) -> io::Result<Self>; pub fn feed_audio(&mut self, samples: &[f32]); pub fn segments(&self) -> Vec<DiarSegment>; pub fn finish(self) -> Vec<DiarSegment> }` (37-130)
  - `pub fn assign_speakers(utterances: &[(u64 /*line*/, u64, u64)], segments: &[DiarSegment]) -> Vec<(u64, u32)>` (146)
- **Status: dormant.** No production call sites. It is not bundled in Tauri (`tauri.conf.json`
  resources contain only the assistant skill bundle), and it is not built in CI. `docs/investigations/diarization-history-and-live-latency-2026-10-18.md:60-67`
  confirms this: live transcript lines are written with `speaker: None`.
- **It is not required for live speaker turns, because nothing produces live speaker turns.**
  Speaker labels appear only after post-processing (offline pyannote-rs in `pipeline.rs`).
  To show live speakers, you must spawn and wire the sidecar yourself (and build and sign it).

---

## 2. Speaker renaming and voice memory

### 2.1 How labels flow

1. The transcriber writes `[m:ss] text` lines (`transcribe.rs:212` etc.).
2. `diarize::apply_speakers` prefixes the labels: `[SPEAKER_0 0:00] text` (`diarize.rs:3201`; labels `SPEAKER_{n}`, `diarize.rs:2287`).
3. Attribution runs in this order: Level 2 voice enrollment, then Level 0 deterministic (1-on-1), then Level 1 LLM (`pipeline.rs:6446`, `3320-3470`).
   It builds `speaker_map: Vec<SpeakerAttribution>` **containing only the labels that something attributed**.
4. `apply_confirmed_names` rewrites the transcript lines **only for High-confidence entries**
   (`pipeline.rs:3439`). So on disk a line can read `[SPEAKER_1 0:29]` or `[Mat 0:22]`.
5. `pipeline.rs:5949-5951` and `6739-6741` write `.<stem>.embeddings` next to the .md file:
   JSON `{"SPEAKER_0":[f32...], ...}`, mode 0600 (`voice.rs:1561-1599`).

### 2.2 Renaming (current design: overlay DB, markdown is NOT rewritten)

Core API: `crates/core/src/overlays.rs`
```rust
pub fn write_speaker_confirmation(meeting_path: &Path, speaker_label: &str, name: &str,
    reversible_to: Option<&str>, note: Option<&str>) -> Result<SpeakerConfirmation, OverlayError>   // 953
pub fn write_speaker_confirmation_at(db_path: &Path, ...same...)                                  // 970
pub fn load_speaker_confirmations_for_meeting_at(db_path: &Path, meeting_path: &Path) -> Result<Vec<SpeakerConfirmation>, OverlayError> // 1245 (path-keyed only)
pub fn load_speaker_confirmations_for_source_at(db_path: &Path, meeting_path: &Path, source_content: &[u8]) -> Result<Vec<SpeakerConfirmation>, OverlayError> // 1257 (path AND sha256 of bytes)
pub fn apply_speaker_confirmations(speaker_map: &mut Vec<SpeakerAttribution>, confirmations: &[SpeakerConfirmation]) // 1273 (upserts as High/Manual; pushes a new entry if the label is absent)
pub fn speaker_overlay_source_sha256(source_content: &[u8]) -> String                             // 1269
pub fn default_db_path() -> PathBuf  // $MINUTES_HOME/overlays.db or ~/.minutes/overlays.db       // 185
pub struct SpeakerConfirmation { meeting_key, speaker_label, name, confidence, source, reversible_to, note, created_at, source_sha256: Option<String> } // 26
```
SQLite table `overlays(id, entity_key UNIQUE with overlay_type, overlay_type='speaker', value, confidence, source, reversible_to, note, created_at, source_sha256)` (`overlays.rs:810-824`).
`entity_key = "meeting:<canonical path>#speaker:<label>"` (847). On write it stores the
**sha256 of the meeting file bytes at confirmation time** (`stable_source_sha256`).

CLI: `minutes confirm --meeting <path.md> [--speaker SPEAKER_1 --name Alice] [--save-voice]`
(`main.rs:1367-1384`; `cmd_confirm` `main.rs:18971-19109`).
- Without `--speaker/--name` it runs an interactive Y/n/name walk over `speaker_map`.
- It **errors if the label is not already in `frontmatter.speaker_map`** (19025-19037). If `speaker_map` is empty, it prints "No speaker attributions found" and exits.
- `--save-voice` **always bails**: "cannot trust legacy meeting embedding sidecars" (18983-18987).
- It ends with "Meeting markdown was not rewritten."
- It is also blocked under a restrictive assistant policy (`assistant_policy_allows_restricted_content`).

Tauri: `cmd_confirm_speaker(meeting_path: String, speaker_label: String, name: String) -> Result<String,String>`
(`tauri/src-tauri/src/commands.rs:10242-10283`). It has the same "must exist in speaker_map" check and
calls `overlays::write_speaker_confirmation(..., Some(previous_name), Some("desktop confirm"))`.
The palette path is the same (`palette_dispatch.rs:316`).
`cmd_list_voices` (10235) -> `voice::list_profiles`.

Readers that apply overlays:
- Desktop `meeting_detail` (`commands.rs:9838-9864`) uses `load_speaker_confirmations_for_meeting_at`, which is path-keyed and IGNORES the sha.
- `minutes get <slug> --json` (`main.rs:14117-14160`) uses `..._for_source_at`, which applies **only when the current bytes hash equals the hash at confirmation time**. It outputs
  `{path, frontmatter(with overlaid speaker_map), body (raw), overlay_applied, overlay_source_sha256, raw_markdown?}`.
- Graph (`graph.rs:2922`) is also sha-bound.

**Does renaming rewrite the markdown everywhere? No.** Neither frontmatter nor body changes.
Consumers must map `[SPEAKER_1 …]` to the name at render time. Also
`minutes redo-speaker-mapping <meeting> --apply` (`main.rs:859`, `6073`) only rewrites `speaker_map` in the
frontmatter (via `update_frontmatter`). It never touches the transcript lines.
`markdown::update_frontmatter` is the core primitive if you want to persist names into the YAML yourself.

### 2.3 Voice persistence (`crates/core/src/voice.rs`)

DB: `~/.minutes/voices.db` (hardcoded `dirs::home_dir()/.minutes`. It ignores MINUTES_DATA_DIR and MINUTES_HOME. `voice.rs:135-141`). Mode 0600, WAL.
Tables (`voice.rs:147-226`):
- `voice_profiles`: legacy, one row per slug (`save_profile`/`save_profile_blended`, used by old `minutes enroll`, `main.rs:18890`). It is migrated by `migrate_legacy_profiles`.
- `voice_samples`: immutable (triggers forbid DELETE and content UPDATE). It holds an embedding BLOB, `model_id`, `trust_class` (`manual|manually_confirmed|source_candidate|voicematch_candidate`), `meeting_path`, `sidecar_speaker`, quality, `revoked_at`.
- `voice_active_profiles (person_slug, model_id) PK`: a robust mean of the non-revoked samples. **The pipeline matches against this table.**

Key API:
```rust
pub fn open_db() -> Result<Connection, VoiceError>; pub fn open_db_at(path: &Path) -> ...      // 143,147
#[cfg(feature="diarize")] pub fn embed_solo_clip(wav_path: &Path, config: &Config) -> Result<SoloEmbedding, VoiceError> // 512 (quality-gated: >=5s speech, SNR>=8dB, consistency>=0.70)
pub fn insert_voice_sample(conn: &Connection, sample: &VoiceSampleInput) -> Result<i64, VoiceError> // 887 (rebuilds the active profile atomically)
pub fn revoke_voice_sample(conn, id: i64); pub fn revoke_voice_person(conn, slug: &str) -> Result<usize,_> // 931,1149
pub fn list_active_profiles(conn, model_id: &str) -> Result<Vec<ActiveProfile>,_>             // 995
pub fn list_voice_enrollments(...) ; pub fn list_profiles(conn) -> Result<Vec<VoiceProfile>,_> // 1012,1482
pub fn match_active_profiles(embedding: &[f32], model_id: &str, profiles: &[ActiveProfile], threshold: f32) -> VoiceMatchEvidence // 1103
pub fn load_meeting_embeddings(meeting_path: &Path) -> Option<HashMap<String, Vec<f32>>>      // 1584
pub fn meeting_embeddings_sidecar_path(meeting_path: &Path) -> PathBuf  // dir/.<stem>.embeddings // 1592
pub fn model_version(config: &Config) -> &'static str                                         // 43
pub fn person_slug(text: &str) -> String                                                      // 1612
pub fn delete_all_voice_data(config: &Config) -> Result<DeleteVoiceDataReport,_>              // 1308
pub struct VoiceSampleInput { person_slug, name, embedding, model_id, trust_class: TrustClass, meeting_path, sidecar_speaker, capture_source, speech_seconds, segment_count, quality_json, similarity, top2_margin, threshold_version, sensitivity: String, created_at: Option<String> } // 651
```
CLI: `minutes voice enroll|status|list|test|remove <slug>|delete-all` (`crates/cli/src/voice.rs:16-33`).
`enroll` records the mic and works **only for yourself**: it requires `[identity] name` and inserts `TrustClass::Manual`.
`minutes enroll [--file wav]` is the legacy self-only command.

`[voice]` config (`config.rs:188-233`): `enabled=true`, `match_threshold=0.65`,
`store_meeting_embeddings`, `passive_candidate_capture`, `retain_non_self_embeddings`, etc.
The last few flags are **not read anywhere outside config.rs**. They are dormant.

The pipeline matches every active profile for the current model, not only self
(`pipeline.rs:962-990`, `735-950`). It requires a unique winner above the threshold, every reliable
segment agreeing, and one-to-one assignment. It then emits `AttributionSource::Enrollment` with `Confidence::High`.
So a non-self profile inserted for "Alice" **would** be recognized in future meetings.

`person_identity.rs` is entirely `pub(crate)`: name canonicalization (role-suffix
stripping, `strip_speaker_label`, plausibility). It is not a public API and has nothing to do with voice.

### 2.4 Exact sequence: "rename SPEAKER_X in meeting M to Alice and remember her voice"

There is no single upstream command for this. The supported surfaces cover only the rename.
Using core directly (feature `diarize` required for step 3b):

1. Read M. Parse it with `markdown::split_frontmatter` + `serde_yaml::from_str::<Frontmatter>`.
2. Rename (overlay):
   `overlays::write_speaker_confirmation(&M, "SPEAKER_1", "Alice", Some(prev_name_or_label), Some("my-app"))`.
   - Core does not require the label to be in `speaker_map` (only the CLI and Tauri wrappers check).
     `apply_speaker_confirmations` adds it if it is absent.
   - Optional for durability: also `markdown::update_frontmatter(&M, |f| { upsert speaker_map entry {label, "Alice", High, Manual} })`.
     Note that this changes the bytes, so the sha-bound overlay just written no longer applies.
     Do the frontmatter write first, then write the overlay. Better: do only one of the two.
3. Remember the voice (no supported CLI or Tauri path for non-self people):
   a. Quick but "untrusted" by upstream's standard: `voice::load_meeting_embeddings(&M)?["SPEAKER_1"]`.
      This is the cluster centroid, keyed by the ORIGINAL diarizer label.
   b. Upstream-blessed quality path: get a single-speaker WAV of Alice (for example, cut her segments out of
      `<slug>.wav` yourself. Segment times are not persisted, but you can derive them from `[SPEAKER_1 m:ss]` transcript lines),
      then `voice::embed_solo_clip(&wav, &config)?`.
   c. `voice::insert_voice_sample(&voice::open_db()?, &VoiceSampleInput { person_slug: voice::person_slug("Alice"), name: "Alice".into(), embedding, model_id: voice::model_version(&config).into(), trust_class: TrustClass::ManuallyConfirmed, meeting_path: Some(M), sidecar_speaker: Some("SPEAKER_1".into()), sensitivity: "normal".into(), ... })`.
      The `model_id` must equal `model_version(&config)`, or the profile is never matched.
4. Optional: add "Alice" to vocabulary (`vocabulary::load/save_at`; see `cmd_remember_vocabulary_person`, `commands.rs:10286`) so the name is spelled correctly.

---

## 3. Meeting storage format (`crates/core/src/markdown.rs`)

### 3.1 Frontmatter (`pub struct Frontmatter`, `markdown.rs:1255-1350`)

`#[derive(Serialize, Deserialize, JsonSchema)]`. There is NO `deny_unknown_fields` and NO `#[serde(flatten)]` catch-all,
**so unknown keys are silently dropped on any serde round trip** (update_frontmatter, rewrite).

| field | type | notes |
|---|---|---|
| `title` | `String` | required |
| `type` | `ContentType` = `meeting\|memo\|dictation\|note` | required |
| `date` | `DateTime<Local>` | required. Accepts RFC3339, local `YYYY-MM-DDTHH:MM:SS`, or `YYYY-MM-DD` |
| `duration` | `String` | default `"0s"`; pipeline format `"71m 54s"` / `"42s"` (`pipeline.rs:6820`) |
| `source` | `Option<String>` | |
| `status` | `Option<OutputStatus>` = `complete\|no-speech\|transcript-only\|degraded` | |
| `processing_warnings` | `Vec<ProcessingWarning{step, reason, timeout_secs?, message?}>` | |
| `tags` | `Vec<String>` | |
| `attendees` | `Vec<String>` | |
| `attendees_raw` | `Option<String>` | legacy Granola-style |
| `calendar_event` | `Option<String>` | |
| `people` | `Vec<String>` | slugs |
| `entities` | `EntityLinks{people: Vec<EntityRef>, projects: Vec<EntityRef>}`; `EntityRef{slug, label, aliases: Vec<String>}` | |
| `device` | `Option<String>` | |
| `captured_at` | `Option<DateTime<Local>>` | |
| `context` | `Option<String>` | |
| `action_items` | `Vec<ActionItem{assignee: String, task: String, due: Option<String>, status: String /*open\|done*/}>` | |
| `decisions` | `Vec<Decision{id?, effective_date?, text, topic?, authority?, supersedes?}>` | |
| `intents` | `Vec<Intent{kind: action-item\|decision\|open-question\|commitment, what, who?, status, by_date?}>` | |
| `recorded_by` | `Option<String>` | also adds a filename suffix |
| `capture` | `Option<CapturePolicy>` = `none` | no-capture artifacts |
| `sensitivity` | `Option<Sensitivity>` = `normal\|restricted` | explicit `null` is a parse ERROR (`deserialize_present_sensitivity`) |
| `debrief` | `Option<DebriefStatus>` = `pending\|complete\|not-applicable` | |
| `consent` | `Option<ConsentBasis>` = `verbal_all_parties\|notice_in_invite\|recorded_disclosed\|na\|unattested` | |
| `consent_notice` | `Option<String>` | |
| `visibility` | `Option<Visibility>` = `private\|team` | team means file mode 0640 |
| `speaker_map` | `Vec<SpeakerAttribution{speaker_label, name, confidence: high\|medium\|low, source}>` | |
| `name_corrections` | `Vec<NameCorrection{raw, corrected}>` | |
| `spelling_corrections` | `Vec<SpellingCorrection{raw, corrected, count: usize}>` | |
| `recording_health` | `Option<RecordingHealth{voice_stem_active_ratio?, system_stem_active_ratio?, system_dominant_ratio?, capture_warnings: Vec<CaptureWarning{kind, source, message, diagnostic_confidence}>, diarization_path?: stem-energy\|ml\|ml-bleed-degraded\|none, remote_audio?: missing\|partial, capture_backend?, call_audio_source?}>` | |
| `speaker_mapping` | `Option<SpeakerMappingHealth{status, model, diarized_speakers, mapped_speakers, attendees, duration_ms?, reason?, last_run?}>` | |
| `summarization` | `Option<SummarizationHealth{status, model, template?, duration_ms?, reason?, last_run?}>` | |
| `template` | `Option<String>` | |
| `filter_diagnosis` | `#[serde(skip)]` | never on disk |

Keys that other code reads RAW but that are NOT in the struct (they are lost on a serde round trip):
`audio_retention: pinned` (`retention.rs:285`). Fixtures also use `minutes_demo` and `promised_to`.

The reader crate's `Frontmatter` (`crates/reader/src/types.rs:73-121`) is a smaller subset. It has no consent,
name/spelling corrections, captured_at, device, speaker_mapping, summarization, or template. Its `date` has no lenient parser.

### 3.2 JSON schema

`schema/meeting.schema.json` exists but is **stale** (last changed 2026-05-13). It lists only 18 properties
(no speaker_map, sensitivity, consent, recording_health, ...); required = title, type, date, duration.
Generate the live schema with `minutes schema` (`main.rs:14075`: `schemars::schema_for!(Frontmatter)`).

### 3.3 Body layout (`render_markdown`, `markdown.rs:1596-1647`; `summarize::format_summary`, `summarize.rs:349-389`)

```
---\n<serde_yaml>---\n\n
## Summary\n\n- key point...\n          (or free text)
\n## Decisions\n\n- [x] ...\n           (inside the summary string)
\n## Action Items\n\n- [ ] ...\n
\n## Open Questions\n\n- ...\n
\n## Commitments\n\n- ...\n\n
*No speech detected...*  (status no-speech only, + **Diagnosis** / **Retry audio**)
## Notes\n\n- [4:23] user note\n\n      (only when user notes exist; each non-empty line is prefixed "- ")
## Transcript\n\n[SPEAKER_0 0:00] text\n[Alice 0:22] text\n...
```
- AI-owned sections: `resummarize::AI_SECTIONS` = Summary, Decisions, Action Items, Open Questions, Commitments (`resummarize.rs:39`).
- Transcript timestamp is `[m:ss]` with minutes unbounded (for example `[75:03]`). The speaker form is `[LABEL m:ss] text`.
  The label is `SPEAKER_n`, or a person name if it was High-confidence at processing time. Non-lexical events look like `[SPEAKER_0 1:02] [laughter]`.
  Unlabeled transcripts are plain `[m:ss] text`.
- Section helpers: `pub fn split_frontmatter(&str) -> (&str, &str)` (2527), `find_sections` (2634),
  `find_unique_section(body, name) -> Result<Option<SectionRange>, MarkdownError>` (2656, fence-aware, fails on duplicates),
  `section_text` (2673), `first_section_text` (2687), `extract_field` (2547).

### 3.4 Filename and directories

- `generate_slug` (`markdown.rs:2127-2167`): `YYYY-MM-DD-<title lowercased, non-alnum to '-', collapsed, max 60 chars>[-<first word of recorded_by, alnum, max 10>].md`.
  An empty title gives `YYYY-MM-DD-untitled.md`. On collision the name becomes `-2`, `-3`, ... (`collision_filename`, 2402). It is published with a no-replace rename from a hidden staging file.
- `config.output_dir` default `~/meetings` (`config.rs:1733`):
  - meeting and note go to `~/meetings/`
  - memo goes to `~/meetings/memos/`
  - dictation goes to `~/meetings/dictations/`
  - daily notes go to `~/meetings/daily` (config.rs:1868)
  - inactive dirs (excluded from the corpus): `archive`, `processed`, `failed`, `failed-captures`, and any dot-dir or dotfile (`markdown.rs:29-38`). `minutes delete` moves files to `archive/` by default.
- Siblings next to `<slug>.md`: `<slug>.wav`, `<slug>.voice.wav`, `<slug>.system.wav` (stems), `.<slug>.embeddings` (0600 JSON),
  `.<slug>.md.pre-resummarize.<ts>.bak` (resummarize backups), and the corpus lease dir `~/meetings/.minutes-private-projection/`.
- State in `~/.minutes/`: `overlays.db`, `voices.db`, `current-notes.md`, `current-context.txt`, `current-consent.json`, `recording-start.txt`, `models/`.
  Config is in `~/.config/minutes/config.toml`.
- Permissions: .md files are 0600 (or 0640 with `visibility: team`). Rewrites are atomic and preserve the mode. The embeddings sidecar and voices.db are 0600.

### 3.5 Writers

```rust
pub fn write(fm: &Frontmatter, transcript: &str, summary: Option<&str>, user_notes: Option<&str>, config: &Config) -> Result<WriteResult, MarkdownError> // 1650
pub fn rewrite(path: &Path, fm: &Frontmatter, transcript: &str, summary: Option<&str>, user_notes: Option<&str>) -> Result<WriteResult, MarkdownError> // 1760 (full re-render: drops custom sections!)
pub fn rewrite_with_retry_path_guarded(..., expected_current: &str) -> ...                     // 1811
pub fn update_frontmatter<F: FnOnce(&mut Frontmatter)>(path: &Path, update: F) -> Result<(), MarkdownError> // 2203 (body bytes preserved; guarded)
pub fn set_action_item_status(path: &Path, index: usize, done: bool) -> Result<(), MarkdownError> // 2240
pub fn rename_meeting(path: &Path, new_title: &str) -> Result<PathBuf, MarkdownError>          // 1923 (may rename the file)
pub fn atomic_rewrite_preserving_mode_guarded(path: &Path, new_content: &str, expected_current: Option<&str>, backup_to: Option<&Path>) -> Result<(), MarkdownError> // 2316
pub struct WriteResult { pub path: PathBuf, pub title: String, pub word_count: usize, pub content_type: ContentType }
```

### 3.6 Realistic example (`~/meetings/2026-11-05-northwind-sso-follow-up.md`, mode 0600)

```markdown
---
title: Northwind SSO Follow-up
type: meeting
date: 2026-11-05T15:30:12.418-07:00
duration: 31m 4s
source: recording
status: complete
tags:
- customer
- northwind
attendees:
- Mat S.
- Riley B.
people:
- mat
- riley
entities:
  people:
  - slug: riley
    label: Riley B.
    aliases:
    - Riley
action_items:
- assignee: mat
  task: Fix SSO nested-groups onboarding for Northwind
  due: 2026-11-30
  status: open
decisions:
- text: Ship nested-groups fix before end of month
  topic: sso
  authority: high
intents:
- kind: commitment
  what: Ship nested-groups fix
  who: mat
  status: open
  by_date: 2026-11-30
recorded_by: Mat S.
speaker_map:
- speaker_label: SPEAKER_0
  name: Mat S.
  confidence: high
  source: enrollment
- speaker_label: SPEAKER_1
  name: Riley B.
  confidence: medium
  source: llm
recording_health:
  voice_stem_active_ratio: 0.41
  system_stem_active_ratio: 0.52
  diarization_path: ml
speaker_mapping:
  status: ok
  model: agent:claude
  diarized_speakers: 2
  mapped_speakers: 2
  attendees: 2
  duration_ms: 2140
  last_run: 2026-11-05T16:02:01-07:00
---

## Summary

- Riley's team keeps failing SSO with Okta nested groups.
- Mat committed to a fix before end of month.

## Decisions

- [x] Ship nested-groups fix before end of month

## Action Items

- [ ] @mat: Fix SSO nested-groups onboarding (by 2026-11-30)

## Notes

- [4:23] Riley said "churn" twice
- [Nov 05, post-meeting] Confirmed via email

## Transcript

[SPEAKER_1 0:00] Half of our eighteen people bounced at SSO this month.
[Mat S. 0:22] What's the failure point?
[SPEAKER_1 0:29] Okta nested groups.
```
(The filename gets the suffix `-mat` because `recorded_by` is set: `2026-11-05-northwind-sso-follow-up-mat.md`.
SPEAKER_0 was rewritten to the name because it was High confidence. SPEAKER_1, at Medium, was not.)

---

## 4. Reading back, listing, and search

### 4.1 `crates/reader` (`minutes-reader`, light deps: serde, serde_yaml, chrono, walkdir, schemars)
```rust
minutes_reader::parse::split_frontmatter(content: &str) -> (&str, &str)
minutes_reader::parse::parse_meeting(path: &Path) -> Result<ParsedMeeting, String>
minutes_reader::search::list_meetings(dir: &Path, limit: usize) -> Vec<ParsedMeeting>          // date desc
minutes_reader::search::search_meetings(dir: &Path, query: &str, limit: usize) -> Vec<ParsedMeeting> // substring in title/body
minutes_reader::search::find_open_actions(dir: &Path, assignee: Option<&str>) -> Vec<(PathBuf, ActionItem)>
pub struct ParsedMeeting { pub frontmatter: Frontmatter, pub body: String, pub path: PathBuf }
```
Caveats: it walks EVERYTHING under `dir`, including `archive/`, `processed/` and dot-dirs (no inactive-dir pruning).
It has no sensitivity filtering, no overlays, and a subset Frontmatter.

### 4.2 Core search (`crates/core/src/search.rs`)
```rust
pub fn search(query: &str, config: &Config, filters: &SearchFilters) -> Result<Vec<SearchResult>, SearchError>      // 1964; empty query = list
pub fn search_with_mode(query, config, filters, mode: SyncMode) -> ...                                               // 1980
pub fn search_intents(...); pub fn find_open_actions(...); pub fn person_profile(config, person) ; pub fn consistency_report(...); pub fn cross_meeting_research(...)
pub fn resolve_slug(slug: &str, config: &Config) -> Option<PathBuf>                                                  // 1540
pub fn read_authorized_meeting(path: &Path, config: &Config, include_restricted: bool) -> Result<AuthorizedMeetingSnapshot, SearchError> // 1719
pub struct SearchFilters { content_type: Option<String>, since: Option<String>, attendee, intent_kind: Option<IntentKind>, owner, recorded_by, include_restricted: bool } // 1523 (Default)
pub struct SearchResult { path: PathBuf, title: String, date: String, content_type: String, snippet: String, matched_via_alias: Option<String>, evidence: Option<EvidenceBundle> } // 389
pub struct AuthorizedMeetingSnapshot { path, content, frontmatter } // 407
```
- Restricted meetings are excluded unless `include_restricted: true` (the desktop app sets true: `commands.rs:8592-8603`).

### 4.3 Search index (`search_index.rs`, `search_index/schema.rs`)
- **No persistent SQLite index any more.** `SearchIndex::open` uses `Connection::open_in_memory()`
  (`search_index.rs:43`, `399-423`). Every query does a full stable-corpus scan into a process-private FTS5 projection.
  The legacy `~/.minutes/search.db{,-wal,-shm}` files are actively **deleted** by `policy_fs::retire_legacy_policy_caches()`.
- Schema (in memory): `meetings(rowid, path UNIQUE, title, date, content_type, attendees_json, recorded_by, mtime_ns, size_bytes, body_hash, indexed_at)`,
  `meeting_attendees(meeting_rowid, attendee_lower)`, `meetings_fts USING fts5(title, body, tokenize='unicode61 remove_diacritics 2')`, `sync_state(key,value)` (`schema.rs:230-266`).
- "Rebuild" is implicit on every call. `SyncMode::{Auto, Force, Skip}`. CLI flags: `--sync` / `--no-sync`.
- **Cross-process lock:** there is one exclusive, NON-blocking lease per corpus at `<output_dir>/.minutes-private-projection/private-policy-projection.lock`
  (`policy_fs.rs:3229-3258`). If the CLI, MCP or desktop app is searching or building the graph at the same moment, your call fails with
  "private search projection capacity unavailable". Retry with backoff.
- The corpus must be stable during the scan. Concurrent writes can make a query fail closed and retry.

### 4.4 CLI equivalents (shell-out)
`minutes search <q> [-t meeting|memo] [--since D] [-l N] [--format json]` (JSON lines), `minutes list` (open actions),
`minutes get <slug|path> --json [--compact-json]`, `minutes schema`, `minutes note`, `minutes confirm`, `minutes resummarize`.

---

## 5. User notes (`crates/core/src/notes.rs`)

During a recording:
- `pub fn add_note(text: &str) -> Result<String, String>` (104): if a sensitive session is active it adds a marker. Otherwise it calls `add_recording_note`.
- `pub fn add_recording_note(text) -> Result<String,String>` (122): it requires an active recording PID (`pid::pid_path()`). It computes the `[m:ss]` time elapsed since
  `~/.minutes/recording-start.txt`, then O_APPENDs `[4:23] text` to `~/.minutes/current-notes.md`.
- `pub fn note_target() -> Option<&'static str>` (92): `"sensitive" | "recording" | None`.
- `save_context(text)` writes `~/.minutes/current-context.txt` (pre-meeting context). `save_consent(basis, notice)` writes `current-consent.json`.
- The pipeline reads them with `read_notes()` / `read_context()`. Notes go to the LLM as high-priority context and are rendered as `## Notes` with `- ` bullets.
  `cleanup()` deletes these files after processing.
- CLI `minutes note "text"`; Tauri `cmd_add_note(text)` (`commands.rs:7844`), `cmd_note_target` (7851).

After a meeting:
- `pub fn annotate_meeting(meeting_path: &Path, text: &str) -> Result<(), String>` (271): appends
  `- [Nov 05, post-meeting] text` to the end of `## Notes`. If there is no Notes section, it creates one before `## Transcript`/`## Decisions`/`## Action Items`, or at the end of the file.
  It uses plain `fs::write` (not atomic, not guarded) and forces mode 0600 (even for `visibility: team`). CLI: `minutes note --meeting <path> "text"` (`main.rs:3079`).
- `pub fn validate_meeting_path(meeting_path, meetings_root) -> Result<(), String>` (240): .md files inside output_dir only.
- There is NO "replace the Notes section" API in core or Tauri. Build it with `find_unique_section(body, "Notes")` + splice + `atomic_rewrite_preserving_mode_guarded(path, new, Some(&original), None)`.

### What to preserve when your app edits and saves a meeting file
1. **Frontmatter byte-for-byte** (do not re-serialize it unless you must). serde round trips drop unknown keys (`audio_retention: pinned`, custom keys),
   and they reformat YAML. Splice only the body. If you must change YAML, use `markdown::update_frontmatter` and accept the key loss.
2. Frontmatter must still parse as `Frontmatter`: required `title`, `type`, `date`; valid enum spellings; no `sensitivity: null`.
3. Section headings exactly `## Summary`, `## Notes`, `## Transcript` (H2 at column 0). Exactly ONE of each: `find_unique_section` fails on duplicates, which breaks `resummarize`.
   Keep `## Transcript` non-empty and last. Resummarize requires it, and it re-splices only the AI sections while keeping Notes and custom sections (`resummarize.rs:593-640`).
   Do NOT call `markdown::rewrite` for edits: it re-renders from scratch and drops custom sections.
4. Transcript line grammar `[LABEL m:ss] text`. Speaker overlays and the UI parse it.
5. **Checksums:** no checksum is stored in the file. BUT speaker overlays in `~/.minutes/overlays.db` are bound to the sha256 of the file bytes at confirmation time.
   Any edit (even to the notes) silently voids those confirmations for `minutes get --json` and the graph (`load_..._for_source_at`).
   The desktop app's path-keyed loader still shows them. If you edit after confirmations, re-issue `write_speaker_confirmation` for each label after saving,
   or persist the names into `speaker_map` via `update_frontmatter`. The overlay is also keyed by canonical PATH, so renaming or moving the file orphans it.
6. Keep the filename (slug) and siblings (`.wav`, `.voice.wav`, `.system.wav`, `.<stem>.embeddings`). Delete and archive operate on these groups.
7. Preserve the mode (0600/0640). Write atomically to a hidden temp sibling (dotfiles are excluded from the corpus), then rename. Use the guarded rewrite to detect concurrent writers.
8. After saving, call `derived::refresh_derived_views(&path, &config, &RefreshOptions::default())` (`derived.rs:101`). This re-copies to the vault (copy strategy) and runs `qmd update` if configured.
   Search and graph need nothing, because they rebuild on read.

---

## 6. Vault / Obsidian (`crates/core/src/vault.rs`, `config.rs:1022-1072`)

```toml
[vault]
enabled = false
path = ""                         # vault root, for example ~/Documents/life
meetings_subdir = "areas/meetings"
strategy = "auto"                 # auto | symlink | copy | direct
```
- `auto`: `recommend_strategy` picks **copy** if the path is cloud-synced (iCloud/Dropbox/OneDrive/GDrive heuristics) or TCC-protected (~/Documents, ~/Desktop, ...). Otherwise it picks **symlink**.
- `symlink`: setup creates `<vault>/<subdir>` -> `config.output_dir`. Nothing is written per meeting.
- `copy`: `sync_file(source, config)` does `fs::copy` of the .md into `<vault>/<subdir>/` (memos go into `<subdir>/memos/`; dictations go into the subdir root). It overwrites. Only the .md is copied (no audio or sidecars).
  It is called after the pipeline writes (`pipeline.rs:5977`, `6779`) and from `derived::refresh_derived_views`. `annotate_meeting`, overlay confirmations and plain edits do NOT trigger it.
  `sync_all` copies EVERY .md under output_dir, following links and including `archive/`.
- `direct`: the user points `output_dir` at the vault. Nothing extra is done.
- API: `detect_vaults() -> Vec<DetectedVault{path, kind: obsidian|logseq|foam, cloud, tcc_protected}>` (194, scans ~/Documents, ~/Obsidian, ~/notes, ~/vault(s), and home children 1 level deep for `.obsidian/.logseq/.foam`),
  `recommend_strategy(&Path) -> VaultStrategy`, `create_symlink(link, target) -> Result<(), VaultError>`, `vault_meetings_dir(config) -> PathBuf`,
  `sync_file(&Path, &Config) -> Result<Option<PathBuf>, VaultError>`, `sync_all(&Config) -> Result<Vec<PathBuf>, VaultError>`, `check_health(&Config) -> VaultStatus`.
- CLI: `minutes vault setup [--path P] [--strategy symlink|copy|direct] [--subdir S]`, `vault status`, `vault unlink`, `vault sync` (`main.rs:1503-1524`).
  Tauri also creates the symlink in `commands.rs:6348`.
- Separate: a knowledge base (wiki/PARA/Obsidian) via `knowledge::ingest_file`. It is append-only and opt-in.

---

## Gotchas summary
- No live speaker labels exist anywhere in production. The streaming sidecar is dormant, unbundled, about 10 s latency, and supports at most 4 speakers.
- Enable the `diarize` feature on your core git dependency (it pulls in pyannote-rs/ort rc.10). Do not also link parakeet-rs/ort rc.12 in the same binary.
- Speaker rename is an overlay in `~/.minutes/overlays.db`. It is sha256- and path-bound, and the markdown is never rewritten. Your edits void it for the CLI and graph.
- The CLI and Tauri confirm commands refuse labels that are not in `speaker_map` (common when the LLM mapped nobody). Core `write_speaker_confirmation` does not refuse them.
- No supported path enrolls a non-self voice. `--save-voice` is hard-disabled. Use `insert_voice_sample` directly with a matching `model_id`.
- The voices.db path ignores MINUTES_HOME and MINUTES_DATA_DIR, but overlays.db honours MINUTES_HOME and models honour MINUTES_DATA_DIR.
- `schema/meeting.schema.json` is stale. Use `minutes schema`.
- Unknown frontmatter keys are dropped by any serde rewrite.
- Search is in-memory per call, and a non-blocking cross-process lease can make concurrent searches fail.
- The reader crate does not prune `archive/` or dot-dirs.
