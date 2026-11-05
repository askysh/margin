# Survey: Recording + Live Transcription on macOS (minutes-upstream)

Repo: `~/dev/minutes-upstream` (read-only). All paths below are relative to it.
Crate under study: `minutes-core` (`crates/core`), version 0.32.1, license MIT.

---

## TL;DR

- Recording is **in-process and blocking**: `minutes_core::capture::record_to_wav_with_lifecycle(path, stop_flag, &config, ctx)` runs on the caller's thread until an `Arc<AtomicBool>` flips or a stop sentinel file appears. There is no recorder daemon. The "PID file" (`~/.minutes/recording.pid`) is a cross-process *lock / advertisement*, created by the caller (`pid::create()`), not by `record_to_wav`.
- `minutes stop` does **not** send a signal. It writes `~/.minutes/recording.stop` (sentinel), and the recorder polls for it (500 ms or inside the capture loop). `minutes record` ignores SIGTERM on purpose.
- System audio exists in **two places**:
  1. **Core (usable from our app)**: `system_audio_backend::CoreAudioTapSystemAudioBackend`, a Core Audio Process Tap (`cidre` crate, macOS 14.4+), opt-in by config: `recording.capture_backend = "core-audio-tap"` + `[recording.sources] call = "auto"`. Also a CPAL loopback path (BlackHole etc.) and PocketStation (Linux/Windows).
  2. **Tauri app only**: `tauri/src-tauri/src/system_audio_record.swift` + `system_audio_tap.swift` compiled into a separate `system_audio_record` helper binary (ScreenCaptureKit for mic + .mov, Core Audio tap preferred for the system stem, SCK as fallback). macOS 15+. Driven by `tauri/src-tauri/src/call_capture.rs` (also Tauri-only).
- Mic + system are written as **separate stems plus a mix**: `<base>.wav` (mixed), `<base>.voice.wav`, `<base>.system.wav` (16 kHz mono i16). Pipeline uses stems for speaker attribution.
- Live transcript: whisper "sidecar" thread inside the recorder writes finalized lines to **`~/.minutes/live-transcript.jsonl`** (truncated per session), a status sidecar `~/.minutes/live-transcript-status.json`, and `live.utterance.final` events to `~/.minutes/events.jsonl`. Live text is fed **mixed** audio, so `speaker` is always `null`. Partials (in-progress drafts) only go through a local-socket "capture relay" (`copilot::CaptureRelayClient`).
- Mic-only vs call is decided by **config** (`recording.sources.call` set or not), not by `RecordingIntent`. Intent only affects preflight blocking/warnings and metadata.
- Permissions: core has `microphone_status()`, `request_microphone_access()`, `screen_recording_status()` (preflight-only, no request), plus rows for input monitoring/accessibility/automation. **No API for the "System Audio Recording" TCC (kTCCServiceAudioCapture)** used by Process Taps; the tap silently delivers zeros if refused.
- Call detection: core has only `capture::detect_active_call_app(&config)` (a `ps` name match). The real detector (mic-in-use + process family attribution + browser tab AppleScript) is Tauri-only (`call_detect.rs`, plus `mic_check.swift` helper). Calendar (`calendar.rs`) is in core and reusable (EventKit helper + AppleScript fallback).

---

## 1. Start / stop a meeting recording

### 1.1 Core entry points (`crates/core/src/capture.rs`)

```rust
// capture.rs:2056
pub fn record_to_wav(output_path: &Path, stop_flag: Arc<AtomicBool>, config: &Config)
    -> Result<(), CaptureError>;

// capture.rs:2064  (what CLI and Tauri actually call)
pub fn record_to_wav_with_lifecycle(
    output_path: &Path,
    stop_flag: Arc<AtomicBool>,
    config: &Config,
    started_context: Option<RecordingStartedContext>,
) -> Result<(), CaptureError>;

// capture.rs:447
pub struct RecordingStartedContext {
    pub session_id: Option<String>,
    pub source: String,          // "capture"
    pub capabilities: Vec<String>, // e.g. ["audio.capture","live.utterance.final","mode.meeting","intent.call"]
}
```

Doc comment (capture.rs:2050): "Blocks until `stop_flag` is set to true (via signal handler) or a stop sentinel file is detected (from `minutes stop`). Writes raw PCM to a WAV file... Automatically reconnects if the audio device changes mid-recording."

What `record_to_wav_with_lifecycle` does internally (capture.rs:2064-2130):
- Refuses if a previous CoreAudio input teardown is still in flight (`CaptureError::CaptureOwnerBusy`).
- `resolve_capture_plan(config)` -> `CapturePlan::Single` (mic) or `CapturePlan::Dual` (mic + system). Dual needs the `streaming` feature.
- Creates a live-partials channel if `whisper`+`streaming` features and `config.live_transcript.recording_sidecar`.
- Starts `copilot::CaptureRelayServer` (a local Unix socket, owner-only 0600, discovery file in `~/.minutes`). If another Minutes process already owns capture -> `CaptureError::AlreadyRecording(owner_pid)`.
- Dual plan -> `record_to_wav_dual_source(...)` (capture.rs:1654). Single -> in-function loop (~capture.rs:2140+).
- Both loops: call `pid::check_and_clear_sentinel()` at start (clear stale) and every iteration (capture.rs:1753, 2242); also poll `extend.sentinel`, mic-mute sentinel, `RecordingSafetyGuard` (silence auto-stop 30 min, max 8 h, low disk 500 MB, WAV 4 GiB stem limit).
- Starts the live sidecar thread (see section 3) and optional screen-context capture.
- Returns `Err(CaptureError::EmptyRecording)` if zero samples.

It does NOT: create/remove the PID file, write recording metadata, queue processing, or run the pipeline. The caller does that.

Live audio level for a meter: `pub fn audio_level() -> u32` (capture.rs:21), 0-100, global atomic.

Other useful pub fns in capture.rs:
- `preflight_recording(mode: CaptureMode, requested_intent: Option<RecordingIntent>, allow_degraded: bool, config: &Config) -> Result<CapturePreflight, String>` (3186)
- `preflight_recording_with_native_call_capture(mode, requested_intent, allow_degraded, native_call_capture_available: bool, config) -> Result<CapturePreflight, String>` (3201)
- `preflight_microphone_only() -> PermissionPreflight` (839)
- `CapturePreflight { intent, inferred_call_app, input_device, system_audio_ready, allow_degraded, microphone_permission, screen_recording_permission, blocking_reason: Option<String>, blocking_kind, warnings: Vec<String> }` (464)
- `list_input_devices() -> Vec<String>` (3387), `list_input_devices_detailed() -> Vec<InputDeviceEntry>` (3352), `list_devices_categorized() -> Vec<CategorizedDevice>` (3486), `detect_loopback_device() -> Option<String>` (3531), `selected_input_device_name(&Config)` (3093), `auto_heal_missing_recording_device(&mut Config) -> bool` (3443), `canonicalize_input_device_setting(&str) -> Option<String>` (2683)
- `stem_paths_for(audio_path) -> Option<diarize::StemPaths { voice, system }>` (560)
- `write_extend_sentinel()` (426), `check_and_clear_extend_sentinel()` (415)
- `RecordingSafetyGuard`, `StemSizeGuard` (for hosts that supervise their own capture, like the Tauri native path)

`RecordingIntent` (capture.rs:433): `Memo | Room | Call` (serde kebab-case).

### 1.2 PID / sentinel / status (`crates/core/src/pid.rs`)

Files (all under `Config::minutes_dir()` = `$MINUTES_DATA_DIR` if absolute, else `~/.minutes`):

| fn | path |
|---|---|
| `pid_path()` :24 | `recording.pid` |
| `current_wav_path()` :69 | `current.wav` (the conventional output path) |
| `recording_meta_path()` :64 | `recording-meta.json` |
| `last_result_path()` :74 | `last-result.json` |
| `processing_status_path()` :79 | `processing-status.json` |
| `stop_sentinel_path()` :618 | `recording.stop` |
| `live_transcript_pid_path()` :44 | `live-transcript.pid` |
| `live_transcript_jsonl_path()` :49 | `live-transcript.jsonl` |
| `live_transcript_wav_path()` :54 | `live-transcript.wav` |
| `live_transcript_status_path()` :59 | `live-transcript-status.json` |
| `dictation_pid_path()`, `voice_pid_path()` | `dictation.pid`, `voice.pid` |

Key functions:
```rust
pub fn create() -> Result<(), PidError>                  // :526  flock + write std::process::id(); Err(AlreadyRecording(pid))
pub fn remove() -> Result<(), PidError>                  // :573
pub fn check_recording() -> Result<Option<u32>, PidError> // :504  cleans stale PID
pub fn inspect_pid_file(path: &Path) -> PidFileState      // :310
pub fn write_stop_sentinel() -> io::Result<()>           // :631  payload "stop" = stop anyone
pub fn write_stop_sentinel_for(target_pid: u32) -> io::Result<()> // :648 payload "stop-pid:<pid>"
pub fn check_and_clear_sentinel() -> bool                // :733
pub fn spawn_sentinel_watcher(stop_flag: Arc<AtomicBool>) -> JoinHandle<()> // :739 (500 ms poll)
pub fn write_recording_metadata_with_context(mode: CaptureMode, context_session_id: Option<&str>) -> io::Result<()> // :136
pub fn read_recording_metadata() -> Option<RecordingMetadata> // :153
pub fn clear_recording_metadata() -> io::Result<()>     // :164
pub fn set_processing_status(stage, mode, title, job_id, job_count) // :172
pub fn status() -> RecordingStatus                       // :779
pub struct RecordingStatus { recording, processing, processing_stage, recording_mode, processing_title, processing_job_id, processing_job_count, pid, duration_secs, wav_path } // :761
pub enum CaptureMode { Meeting, QuickThought, Dictation, LiveTranscript } // :85
```

Sentinel semantics (pid.rs:640-735): unaddressed "stop" is honored by any recorder; "stop-pid:N" only by PID N; addressed-to-dead-PID is cleared as stale.

### 1.3 CLI `record` / `stop` (`crates/cli/src/main.rs`)

`Record` args (main.rs:342-409): `--title/-t`, `--context/-c`, `--mode meeting|quick-thought`, `--intent auto|memo|room|call`, `--allow-degraded`, `--skip-audio-probe <REASON>`, `--consent <BASIS>`, `--consent-notice <TEXT>`, `--language/-l`, `--device/-D`, `--source` (repeatable: first = voice, second = call), `--call`, `--diagnose <WAV>`, `--mute-mic`, `--template <slug>`.

Dispatch (main.rs:2063-2141): `resolve_recording_device_overrides(&mut config, &source, device, call)` (main.rs:3214) rewrites `config.recording.sources`/`device` in memory, then `cmd_record`.

`cmd_record` (main.rs:3388-3667), in order:
1. `capture::preflight_recording(...)` -> bail on `blocking_reason`.
2. `check_meeting_system_audio_probe(...)` (main.rs:3323): for Meeting mode with a configured system source, runs a **5 s probe** (`health::probe_system_audio_capture`) and **bails if the system output is silent** unless `--skip-audio-probe "<reason>"`.
3. Refuse if `live-transcript.pid` / `voice.pid` active or a sensitive session is on.
4. Consent (non-TTY never blocks; records `unattested`).
5. `pid::create()`, `desktop_context::maybe_start_capture_session`, `pid::write_recording_metadata_with_context`, `notes::save_recording_start()`.
6. ctrlc handler sets `stop_flag`; `libc::signal(SIGTERM, SIG_IGN)`.
7. `capture::record_to_wav_with_lifecycle(&pid::current_wav_path(), stop_flag, config, Some(ctx))` (blocks).
8. `jobs::queue_live_capture_with_recording_health(...)` (moves `current.wav` + stems into `~/.minutes/jobs/...`), writes `last-result.json` `{"status":"queued","job_id",...}`, `spawn_queue_worker()` = re-exec self `minutes process-queue` detached.
9. `cleanup_live_capture_state()` = `pid::remove()` + `clear_recording_metadata()` + `notes::cleanup()`. Prints the queued JSON to stdout.

`cmd_stop` (main.rs:3698-3800): `pid::check_recording()` -> `pid::write_stop_sentinel()` (unaddressed) -> polls until `recording.pid` disappears (120 s timeout) -> prints `last-result.json`. If no recording, does the same for a live-transcript session (30 s timeout).

`cmd_status` (main.rs:3814) prints `pid::status()` as JSON (redacted when assistant policy restricts).

So: **CLI recording = the `minutes record` process itself is the recorder** (foreground, blocking). A host app that shells out runs `minutes record ...` as a child, stops it via `minutes stop` (or by writing the sentinel), and reads `minutes status` / `last-result.json`.

### 1.4 Minimal in-process call sequence for a host app

```rust
use std::sync::{Arc, atomic::AtomicBool};
use minutes_core::{capture, pid, jobs, notes, Config, CaptureMode};

let mut config = Config::load();               // or load_strict(); MINUTES_CONFIG_PATH overrides path
// for call capture via Core Audio tap (see section 2):
// config.recording.capture_backend = "core-audio-tap".into();
// config.recording.sources = Some(SourcesConfig { voice: None, call: Some("auto".into()) });

let pre = capture::preflight_recording(CaptureMode::Meeting, Some(capture::RecordingIntent::Call), false, &config)?;
if let Some(r) = pre.blocking_reason { bail!(r) }

pid::create()?;                                // cross-process lock (Err if CLI/desktop app is recording)
pid::write_recording_metadata_with_context(CaptureMode::Meeting, None).ok();
notes::save_recording_start().ok();
let started = chrono::Local::now();

let stop = Arc::new(AtomicBool::new(false));
let wav = pid::current_wav_path();
let stop2 = stop.clone();
let handle = std::thread::spawn(move || {
    capture::record_to_wav_with_lifecycle(&wav, stop2, &config, Some(capture::RecordingStartedContext {
        session_id: None, source: "capture".into(),
        capabilities: vec!["audio.capture".into(), "live.utterance.final".into()],
    }))
});
// ... UI: poll capture::audio_level(), tail live-transcript.jsonl ...
stop.store(true, Ordering::Relaxed);           // or pid::write_stop_sentinel_for(std::process::id())
handle.join()??;

let job = jobs::queue_live_capture_with_recording_health(
    CaptureMode::Meeting, title, &pid::current_wav_path(),
    notes::read_notes(), notes::read_context(),
    Some(started), Some(chrono::Local::now()), None /*ctx session*/, None /*calendar*/,
    None /*template*/, None, None /*consent*/, None /*recording_health*/)?;
pid::remove().ok(); pid::clear_recording_metadata().ok(); notes::cleanup();
// then process: jobs::process_pending_jobs(&config, |job| ...) in a SEPARATE process (Tauri re-execs itself
// with --process-queue-worker; CLI uses `minutes process-queue`). Or pipeline::process(...) directly.
```

`jobs::queue_live_capture_with_recording_health` signature (jobs.rs:189):
```rust
pub fn queue_live_capture_with_recording_health(
    mode: CaptureMode, title: Option<String>, current_wav: &Path,
    user_notes: Option<String>, pre_context: Option<String>,
    recording_started_at: Option<DateTime<Local>>, recording_finished_at: Option<DateTime<Local>>,
    context_session_id: Option<String>, calendar_event: Option<CalendarEvent>,
    template_slug: Option<String>, consent: Option<markdown::ConsentBasis>,
    consent_notice: Option<String>, recording_health: Option<markdown::RecordingHealth>,
) -> std::io::Result<ProcessingJob>;
```
Also `jobs::queue_live_capture(...)` (jobs.rs:159, first 10 args), `jobs::process_pending_jobs<F: FnMut(&ProcessingJob)>(config, on_job_update) -> Result<(), MinutesError>` (jobs.rs:1516), `jobs::worker_active()`, `jobs::active_jobs()`.
Pipeline direct: `pipeline::process(audio_path: &Path, content_type: ContentType, title: Option<&str>, config: &Config) -> Result<WriteResult, MinutesError>` (pipeline.rs:4765), plus `process_with_progress`, `process_with_template`, `transcribe_to_artifact`.

Mic mute (dual-source only): `streaming::set_mic_muted_with_sentinel(bool) -> bool`, `toggle_mic_mute_with_sentinel()`, `is_mic_muted()` (streaming.rs:149-207).

---

## 2. System audio on macOS

### 2.1 In core: `crates/core/src/system_audio_backend.rs`

```rust
pub trait SystemAudioBackend {
    fn probe(&self, secs: u32) -> Result<ProbeResult, CaptureError>;
    fn start(&mut self, sink: AudioSink) -> Result<StreamHandle, CaptureError>;   // AudioSink = crossbeam Sender<streaming::AudioChunk>
    fn current_route(&self) -> RouteDescription;
    fn permission_status(&self) -> Option<PermissionStatus>;                      // tap returns Some(Unknown)
}
pub fn system_audio_backend_for_config(config: &Config, route_hint: String) -> Result<Box<dyn SystemAudioBackend>, CaptureError>; // :113
pub fn parse_capture_backend(&str) -> Result<CaptureBackendKind, String>;   // "cpal" | "core-audio-tap" (aliases core_audio_tap, coreaudio-tap, process-tap) | "pocketstation"
pub struct CpalSystemAudioBackend; pub fn new(device_override: String)          // loopback device (BlackHole, Loopback, ZoomAudio, etc.)
#[cfg(macos)] pub struct CoreAudioTapSystemAudioBackend; pub fn new()           // :244
pub const CORE_AUDIO_TAP_MIN_MACOS: MacOsVersion = 14.4                         // :41
```

Core Audio tap implementation (`core_audio_tap_start_stream`, system_audio_backend.rs:~776, `cfg(all(target_os="macos", feature="streaming"))`) uses `cidre` 0.22.1:
- `ca::TapDesc::with_stereo_global_tap_excluding_processes(&[])` (global tap, excludes nothing), name "Minutes System Audio", private, `TapMuteBehavior::Unmuted`, `create_process_tap()`.
- Private aggregate device with the tap in `tap_list`, drift compensation, tap auto-start; IOProc downmixes to mono, resamples to 16 kHz (`TapSampleClock`), and a format monitor follows sample-rate changes (e.g. Bluetooth HFP).
- `ensure_core_audio_tap_runtime()` gates on `sw_vers` >= 14.4.

How capture.rs uses it: `resolve_capture_plan_with_host` (capture.rs:669-760):
- `config.recording.sources.call` empty -> **Single** (mic only).
- `call` set and `capture_backend == "core-audio-tap"` -> requires `call` in {"auto", "", "core-audio-tap"}; otherwise error "set [recording.sources] call = \"auto\"". -> **Dual** (mic via CPAL + tap).
- `call` set and backend `cpal` -> `"auto"` resolves via `detect_loopback_device()`; else exact device name -> Dual (errors if voice and call are the same device).

Dual writer (`DualCaptureWriters`, capture.rs:1226): three hound writers, 16 kHz mono 16-bit:
- `output_path` (mixed: `mix_dual_source_slot(voice, system)`, capture.rs:1377)
- `<stem>.voice.wav`, `<stem>.system.wav` (`stem_paths_for`, capture.rs:560)
Chunks are aligned by slot (BTreeMap per source, flush up to `min(max_voice,max_system)-1`).
Mixed audio goes to the live sidecar (`AudioSource::Mixed`).

Config docs (`docs/configuration.md:12-31`, `docs/architecture/core-audio-process-tap.md`):
```toml
[recording]
capture_backend = "core-audio-tap"
[recording.sources]
voice = "default"
call = "auto"
```
"Process Tap captures the default system output and uses macOS system-audio access. It is separate from the desktop ScreenCaptureKit helper."

### 2.2 Tauri-only: native call helper (ScreenCaptureKit + tap)

- `tauri/src-tauri/src/system_audio_record.swift` (~2300 lines) + `system_audio_tap.swift`, compiled by `tauri/src-tauri/build.rs:380-420` with `swiftc -parse-as-library -target <arch>-apple-macos14.0` into `tauri/src-tauri/bin/system_audio_record`, shipped as `externalBin` (`tauri.macos.conf.json`). Runtime requires macOS 15 (`SCRecordingOutput`).
- CLI: `system_audio_record <output.mov> [--microphone-name <exact>]`, `--list-microphones` (JSON `{"devices":[...]}`), self-tests. Stop = SIGTERM (handled via DispatchSource; finalizes .mov moov atom and stems).
- stdout = JSON lines: `microphone_selected|microphone_fallback`, `stems {voice_stem, system_stem}`, `health` every 150 ms `{call_audio_live, mic_live, levels, call_audio_source: core_audio_tap|screencapturekit, call_audio_tap_heard, call_audio_tap_seconds, call_audio_screencapturekit_seconds, ...}`, `system_audio_source`, `system_audio_tap_started|rebuilt|failed|rate_mismatch|stop_timeout`, `capture_interrupted`, `capture_resumed`, `finalizing`, `finalize_complete`, `audio_format(_unsupported)`, `microphone_disconnected`.
- Mic comes from ScreenCaptureKit; the system stem comes from the Core Audio tap when live, otherwise SCK (`SystemAudioSourceArbiter`). SCK alone cannot hear FaceTime (`avconferenced`) or Continuity iPhone calls (`callservicesd`); the tap can.
- Output: `~/.minutes/native-captures/YYYY-MM-DD-HHMMSS-call.mov` + `...-call.voice.wav` + `...-call.system.wav`.
- Rust driver: `tauri/src-tauri/src/call_capture.rs`:
  - `pub fn availability() -> CallCaptureAvailability` (:768) (macOS >= 15 and helper binary found beside exe or in `bin/`)
  - `pub fn start_native_call_capture(configured_microphone: Option<&str>, enable_recovery_test_signal: bool) -> Result<NativeCallCaptureSession, String>` (:807)
  - `NativeCallCaptureSession::{output_path, stem_paths, try_wait, source_health -> CallSourceHealth, take_call_audio_notice, capture_warning, stop}` (:599-760). `stop()` = SIGTERM then wait on stdout progress (30 s silence / 600 s ceiling -> SIGKILL).
- This is **not in core**. To use it in our app we'd need to vendor/compile the two Swift files (MIT) and port `call_capture.rs`.

### 2.3 Permissions for system audio

- Info.plist (`tauri/src-tauri/Info.plist`): `NSMicrophoneUsageDescription`, `NSAudioCaptureUsageDescription` (Process Tap / "System Audio Recording"), `NSScreenCaptureUsageDescription` (SCK helper and screenshots), `NSCalendarsFullAccessUsageDescription`.
- Entitlements (`entitlements.plist`): `com.apple.security.device.audio-input`, `com.apple.security.files.user-selected.read-write`. No new entitlement for Process Tap.
- **Core Audio tap -> "Screen & System Audio Recording > System Audio Recording only" TCC (kTCCServiceAudioCapture)**, not Screen Recording. Refusal is silent: the tap delivers zeros. Core has no status/request API for it (docs/architecture/core-audio-process-tap.md: "There is no reliable permission-status shortcut... the readiness probe remains the source of truth"). Reset for testing: `tccutil reset AudioCapture <bundle-id>`.
- **ScreenCaptureKit helper -> Screen Recording TCC** + Microphone.
- TCC attribution: the plan doc stresses testing "from the app (never a shell, which would attribute the permission to the terminal)". A child process launched by our .app is attributed to our app.
- Core's preflight (`evaluate_permission_preflight`, capture.rs:843) still *warns* (non-blocking) about Screen Recording when intent=Call and a system route is ready, even on the tap backend.

---

## 3. Live / streaming transcription

### 3.1 Two producers, one file

1. **Recording sidecar** (during `record_to_wav_with_lifecycle` or Tauri native capture): `capture::start_live_sidecar_with_source` (capture.rs:2590, `pub(crate)`, needs `whisper`+`streaming`) spawns thread "live-sidecar" running `live_transcript::run_tracked_sidecar_mpsc`. Gated by `config.live_transcript.recording_sidecar` (default `true`). Model: `live_transcript.model`, empty = `dictation.model` (a small fast whisper).
2. **Standalone Live** (`minutes live`, Tauri "Go Live"): `live_transcript::run*` - mic only (single device), its own PID `live-transcript.pid`, saves `live-transcript.wav` if `save_wav` (default true), promotes to a meeting on stop per `promote_on_stop` (default `process`).

Public fns (crates/core/src/live_transcript.rs, `cfg(feature="whisper")`):
```rust
pub fn run(stop_flag: Arc<AtomicBool>, config: &Config, existing_context_session_id: Option<String>)
    -> Result<(usize, f64, PathBuf), MinutesError>;                         // :981
pub fn run_with_partials(stop_flag, config, ctx_id, partial_publisher: Option<LivePartialPublisher>) -> ...;  // :994
pub fn run_with_partials_releasing_capture(..., release_capture: Box<dyn FnOnce() + Send>) -> ...;           // :1016
pub fn run_with_partials_for_background(stop_flag, config, ctx_id, partial_publisher) -> ...;               // :1037 (desktop; archives then returns)
pub fn run_sidecar_mpsc(rx: mpsc::Receiver<Vec<f32>>, stop_flag, config, partial_publisher);               // :3435 feed your own 16 kHz f32 audio
pub fn read_since_line(since_line: usize) -> Result<Vec<TranscriptLine>, MinutesError>;                   // :4073
pub fn read_since_duration(duration_ms: u64) -> Result<Vec<TranscriptLine>, MinutesError>;                // :4115
pub fn session_status() -> SessionStatus;                                                                   // :4136
pub fn clear_status_file(); pub fn mark_recording_sidecar_disabled();
```
Plus `stem_tail::spawn_live_transcription_from_stems(voice_stem: PathBuf, system_stem: Option<PathBuf>, config: &Config, stop_flag: Arc<AtomicBool>, partial_publisher: Option<LivePartialPublisher>) -> Option<JoinHandle<()>>` (stem_tail.rs:376) - tails growing WAV stems written by an external recorder (used by Tauri for the Swift helper path). Pub, reusable.

### 3.2 JSONL format (`~/.minutes/live-transcript.jsonl`, mode 0600, truncated at session start)

`TranscriptLine` (live_transcript.rs:58):
```json
{"line":1,"ts":"2026-11-06T10:01:02.345-08:00","offset_ms":1234,"duration_ms":2500,"text":"hello everyone","speaker":null}
```
One line per **finalized** utterance (VAD-segmented, force-finalized at `max_utterance_secs` = 30). `speaker` is always `null` today (writer sets `None`, live_transcript.rs:833); mixed audio is fed.

Status sidecar `~/.minutes/live-transcript-status.json` = `LiveStatus { start_time, updated_at, state: starting|healthy|degraded|failed|stopped|disabled, line_count, last_offset_ms, last_duration_ms, last_final_at?, session_id?, diagnostic?, pending_utterances?, dropped_utterances? }` (live_transcript.rs:499).

`session_status()` returns `SessionStatus { active, pid, line_count, duration_secs, latest_final_age_secs, session_id, jsonl_path, source: standalone|recording-sidecar, diagnostic, disabled, remote_audio }` (live_transcript.rs:387).

### 3.3 Events (`crates/core/src/events.rs`)

Append-only `~/.minutes/events.jsonl` (rotates at 10 MB, seq in `events.seq`). Envelope `{v, seq, timestamp, event_type, ...}`:
- `live.utterance.final {session_id, source, transcript_path, line, text, speaker, offset_ms, duration_ms}` (events.rs:343) - emitted per JSONL line.
- `recording.started {session_id, source, capabilities}`, `recording.completed {path,title,word_count,content_type,duration}`, `live.remote_audio {remote_audio: missing|restored, kind, message}`, `MicMuted/MicUnmuted`, `DeviceChanged`, `screen_context.unavailable`, ...
Readers: `read_events_since_seq(since_seq: u64, limit: Option<usize>) -> Vec<EventEnvelope>` (:719), `read_events(since, limit)` (:708), `latest_event_seq()` (:730).

### 3.4 Partials (in-progress drafts)

`live_partials::channel_with_source(epoch, capacity, source) -> (LivePartialPublisher, LivePartialSubscriber)` (live_partials.rs:334); `LivePartialSubscriber::try_recv() -> Option<LivePartialEvent>`; `LivePartial { session_epoch, utterance_sequence, revision, is_final, source, text, speaker, offset_ms, ... }`.
**Gotcha:** `record_to_wav_with_lifecycle` creates this channel itself and hands the subscriber to `CaptureRelayServer`; the caller never gets it. The only way to see recording partials is a socket client: `copilot::CaptureRelayClient::connect(RelayCursor::default())?.try_recv() -> Result<Option<RelayFrame>, _>` (copilot/relay.rs:631-725), frames `Attached|Transcript{seq, update: Utterance{utterance, ...}|Superseded}|Heartbeat|Gap|Shutdown|...`; `copilot::reduce_relay_draft(&frames, mode, now)` gives the single current draft. For standalone `live_transcript::run_with_partials` you own the publisher, so you can keep the subscriber.

### 3.5 How a UI tails it

- Simplest: poll `live_transcript::read_since_line(last_line)` (re-reads the whole file each time; fine for meeting-scale files) or tail the JSONL with a file watcher; use `session_status()` for health.
- CLI: `minutes transcript --since <N|5m|30s> --format json|text [--include-current] [--status]` (main.rs:1323, handler ~19370).
- Existing desktop app main window does **not** render line-by-line live text. `cmd_live_transcript_status` (commands.rs:23439) returns only `{active, line_count, duration_secs, latestFinalAgeSecs, audioLevel, source, diagnostic, copilotWindowContract}`, polled by the frontend. Events emitted: `live-transcript:started|stopped|error|warning`. Text reaches agents via the JSONL and the Recall panel; the copilot HUD uses `copilot:state`/`copilot:nudge`. The palette reads `read_since_line(0)` (palette_dispatch.rs:495).

---

## 4. Mic-only (room) vs call recording

- **What actually selects the capture path:** `config.recording.sources.call` (Option). None/empty -> single mic (`config.recording.device` or system default). Set -> dual source, backend by `config.recording.capture_backend` (default `"cpal"`).
- `RecordingIntent` (`Memo|Room|Call`) does not change the capture path in core. It drives preflight: Call with no system route -> `blocking_kind = CallRoute` unless `allow_degraded`/`recording.allow_degraded_call_capture`; Room/Memo fine. Inferred as Room unless `recording.auto_call_intent = true` (default false) and a call app is running (capture.rs:3104).
- CLI: `--intent room|call`, `--call` (= intent call + `detect_loopback_device()` for CPAL), `--source A --source B`, `--device`.
- Desktop: `recording.desktop_audio_source = "microphone" | "microphone-and-system"` (config.rs:1098) chosen once by the user; `recording_audio::prepare` (tauri recording_audio.rs:96) maps it to Room/Call and, for non-Call, forces `sources = None`, `capture_backend = "cpal"`. Call in desktop -> native Swift helper when available and `native_route_allowed` (true when backend is cpal or no explicit call source); if the user configured `core-audio-tap` + a call source, the desktop uses the in-process core path instead (recording_audio.rs:57).

Config struct (config.rs:1076): `RecordingConfig { silence_reminder_secs, silence_threshold, silence_auto_stop_secs, max_duration_secs, min_disk_space_mb, device, desktop_audio_source, auto_call_intent, allow_degraded_call_capture, capture_backend, sources: Option<SourcesConfig { voice, call }> }`.

---

## 5. Permissions (`crates/core/src/macos_permissions.rs`)

```rust
pub enum MacPermissionKind { Microphone, ScreenRecording, InputMonitoring, Accessibility, Automation }
pub enum MacPermissionStatus { Granted, Denied, NotDetermined, NotNeeded, Unsupported, StaleOrRestartNeeded, Unknown } // is_granted()
pub fn microphone_status() -> MacPermissionStatus;                              // :166  AVCaptureDevice.authorizationStatus
pub fn request_microphone_access(cancel: &dyn Fn() -> bool, timeout: Duration) -> MicrophoneRequestOutcome; // :189  Granted|Denied|TimedOut|Cancelled
pub fn screen_recording_status() -> MacPermissionStatus;                        // :196  CGPreflightScreenCaptureAccess -> Granted|Denied only
pub fn permission_rows() -> Vec<MacPermissionRow>;                              // :156
pub fn microphone_row(), screen_recording_row(), input_monitoring_row(), accessibility_row(), automation_row() -> MacPermissionRow;
pub fn accessibility_settings_url() -> Option<&'static str>;
pub struct MacPermissionRow { kind, label, status, runtime_usable, optional, required_for, detail, settings_url, can_open_settings, can_request, restart_recommended, restart_blocked_by }
```
Missing: no screen-recording *request* (no `CGRequestScreenCaptureAccess` anywhere), and no System Audio Recording (Process Tap) status/request. Screen-recording prompts in the desktop app are triggered by ScreenCaptureKit itself or by opening `settings_url`.

---

## 6. Call detection

- **Core:** `capture::detect_active_call_app(config: &Config) -> Option<String>` (capture.rs:2981): `ps -eo comm=` matched against `config.call_detection.apps`, returns a display name ("Zoom", "Teams", "FaceTime", ...). No mic-activity check. `CallDetectionConfig { enabled, poll_interval_secs, cooldown_minutes, apps, auto_record, auto_record_apps, stop_when_call_ends, call_end_stop_countdown_secs }` (config.rs:909). `remote_silence::RemoteSilenceDetector` is in core too (for "the other side is silent" alerts).
- **Tauri only:** `tauri/src-tauri/src/call_detect.rs` (3771 lines) `CallDetector` - polls processes AND mic-in-use, attributes CoreAudio input to the app's process family via the `mic_check` Swift helper (`mic_check.swift`), probes browser tabs (Google Meet, Teams web) with AppleScript (Automation permission), sticky windows, cooldowns, emits `call:detected {app_name, process_name, is_reminder}`, `call:ended {app_name, process_name, countdown_secs}`, `call:cleared`, `call:end-countdown:*`. `call_auto_record.rs` handles auto-start. Uses only config/logging from core. Not reusable as a library; would need porting.
- **Calendar (core, reusable):** `calendar::upcoming_events(lookahead_minutes: u32) -> Vec<CalendarEvent>` (:338), `schedule_events(lookback, lookahead)` (:369), `events_overlapping(at)` (:389), `events_overlapping_now()` (:415), `calendar_access_status() -> CalendarAccess` (:295, non-prompting), `extract_meeting_url(&str)`, `filter_events`, `dedup_occurrences`. `CalendarEvent { title, start, minutes_until, attendees, url, calendar, end, location, uid }`. Uses a `calendar-events` EventKit helper compiled by core's `build.rs` from `scripts/calendar-events.swift` (found beside exe, in `tauri/src-tauri/bin/`, or core's `OUT_DIR`), AppleScript fallback. Our app needs `NSCalendarsFullAccessUsageDescription`.

---

## 7. How the Tauri app starts/stops recording

Threading: **in-process on a spawned `std::thread`**, not via the CLI binary. Processing runs in a child process (the app re-execs itself with `--process-queue-worker`, main.rs:475 -> `jobs::process_pending_jobs`).

Call chain:
```
#[tauri::command] cmd_start_recording(app, state, mode, intent, allow_degraded, title, language, source, consent_confirmed)
    -> Result<StartRecordingOutcome, String>                                  // commands.rs:7714
  validate_recording_launch_state; Config::load_strict; recording_audio::prepare
  desktop_recording_consent_required -> ConsentRequired { disclosure }
  prepare_cmd_recording_launch (reserve "starting")
  spawn_reserved_recording(...)                                                // commands.rs:7325
    std::thread::spawn(move || start_recording(app, rec, starting, stop, ..., mode, intent, allow_degraded, title, language))
start_recording(...)                                                          // commands.rs:6777
  recording_audio::prepare; capture::auto_heal_missing_recording_device
  arm_desktop_call_capture_route (CPAL loopback if found)                     // commands.rs:4745
  capture::preflight_recording_with_native_call_capture(mode, intent, allow_degraded, native_ok, &config)
  recording_audio::validate_route
  if intent == Call && native available:
      start_native_call_recording(...)                                        // commands.rs:3808
        pid::create(); CaptureRelayServer::start(...)
        call_capture::start_native_call_capture(mic, false)   // spawns Swift helper
        stem_tail::spawn_live_transcription_from_stems(voice, Some(system), config, live_stop, publisher)
        events::append_event(recording_started_event(...))
        loop while !stop_flag { sleep 100ms; RecordingSafetyGuard/StemSizeGuard; RemoteSilenceDetector;
                                 if pid::check_and_clear_sentinel() {...} }
        session.stop()  (SIGTERM helper) -> queue_native_call_capture_for_processing -> jobs::queue_live_capture_with_recording_health(.mov ...)
  else:
      pid::create(); pid::write_recording_metadata_with_context; notes::save_recording_start
      capture::record_to_wav_with_lifecycle(&pid::current_wav_path(), stop_flag, &config, Some(ctx))   // commands.rs:7043 (blocks this thread)
      jobs::queue_live_capture_with_recording_health(...); pid::remove(); clear_recording_metadata; notes::cleanup
      spawn_processing_worker(...)  -> child `<self exe> --process-queue-worker`                     // commands.rs:5240
#[tauri::command] cmd_stop_recording(state) -> request_stop(&state.recording, &state.stop_flag)      // commands.rs:7834
request_stop                                                                   // commands.rs:4590
  pid::check_recording(): if pid == own pid -> stop_flag.store(true)
                          else (CLI or other) -> pid::write_stop_sentinel_for(pid)  (never SIGTERM, #1271)
cmd_extend_recording -> capture::write_extend_sentinel()
```
Status: `cmd_status` -> `status_value` (commands.rs:7954) merges `pid::status_with_active_jobs`, `capture::audio_level()` / native helper levels, processing jobs.
Live: `cmd_start_live_transcript` (23327) -> `preflight_microphone_only` -> `std::thread::spawn(run_live_session)` -> `live_transcript::run_with_partials(_for_background)`; `cmd_stop_live_transcript` (23415) sets in-app flag or writes sentinel; `cmd_live_capture_plan` (23407) decides if "Go Live" should instead record both sides via native call capture.

---

## Gotchas for the new app

1. **Native SCK/tap call capture is Tauri-only Swift**, not in core. The core-only route to "other side of Zoom" is the opt-in Core Audio tap (`capture_backend = "core-audio-tap"`, `sources.call = "auto"`, macOS 14.4+, needs `streaming` feature). It taps *all* system output (global tap, excludes nothing), so notification sounds, music, VoiceOver go into the system stem.
2. **Feature flags:** core default features = `["whisper"]` only. Dual-source capture, the tap backend, and the live sidecar all need `streaming` (`cfg(all(feature="whisper", feature="streaming"))` for the sidecar). Depend with `features = ["streaming"]` (Tauri app uses `["streaming","diarize"]`; consider `metal`/`coreml` too).
3. **Build needs swiftc/Xcode**: core's `build.rs` compiles `macos_graph_xpc_authority.swift` + `macos_apple_speech_bridge.swift` into a static lib and links Swift runtime / Speech / AVFAudio, and compiles `scripts/calendar-events.swift`. Downstream binaries must add `-Wl,-rpath,/usr/lib/swift` themselves (Tauri does it in `tauri/src-tauri/build.rs:20` via `cargo:rustc-link-arg-bin=minutes-app=...`).
4. **Shared global state in `~/.minutes`**: PID file, stop sentinel, capture relay ownership, `live-transcript.jsonl`, events log, jobs dir are shared with the official Minutes app and CLI. If the real Minutes.app is recording, `pid::create()` fails and `CaptureRelayServer` returns `AlreadyOwned`. An unaddressed `minutes stop` will stop *our* recording too. Use `MINUTES_DATA_DIR` (absolute) and `MINUTES_CONFIG_PATH` env vars to isolate if desired (set before any core call; it's read from env each time).
5. `record_to_wav_with_lifecycle` does not create the PID file or queue processing; hosts must replicate the CLI/Tauri bookkeeping (pid::create/remove, metadata, notes, jobs queue) or `minutes status`/`minutes stop` won't see the recording.
6. Live text has **no speaker attribution** (mixed audio, `speaker: null`) even with separate stems; speaker labels only come from the post-stop pipeline (stems/diarization).
7. Recording partials are only reachable over the capture relay socket (`CaptureRelayClient`), not via a returned channel.
8. **System Audio Recording permission cannot be checked**; a refused tap just records silence. Need `NSAudioCaptureUsageDescription` in our Info.plist and the app must be signed (prompt attaches to the bundle ID).
9. CLI `minutes record` with a system source configured runs a **5 s silence probe and refuses to start if nothing is playing** (pass `--skip-audio-probe "<reason>"`). Also, with `core-audio-tap`, prefer `--intent call` over `--call`: `--call` overwrites `sources.call` with a detected CPAL loopback name (e.g. "BlackHole 2ch") if one exists, which the tap backend then rejects.
10. Core preflight warns about Screen Recording for Call intent even on the tap path (warning only).
11. Desktop native path requires macOS 15; core tap requires 14.4; core-tap stems live next to the output WAV (`current.voice.wav`, `current.system.wav`), native helper outputs go to `~/.minutes/native-captures/`.
12. Processing (whisper batch, diarization) is kept out of the GUI process by both CLI and Tauri; plan on a child process for `jobs::process_pending_jobs` (or shell out to `minutes process-queue`).
