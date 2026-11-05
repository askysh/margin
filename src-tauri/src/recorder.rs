//! Recording. This is the same sequence as upstream's `minutes record`, run in-process on a
//! worker thread (RESEARCH.md §1.2), plus live-transcript tailing with "You / Them"
//! guesses (RESEARCH.md §1.3, DECISIONS.md D8).
//!
//! A recording stays "active" from Start until its audio is queued for processing,
//! including the seconds spent finalizing after Stop. Quit and Reset look at that state,
//! so they can never cut a recording off half-saved.

use crate::{corecfg, events, paths, processing};
use chrono::{DateTime, Local};
use minutes_core::capture::{self, RecordingIntent, RecordingStartedContext};
use minutes_core::config::{Config, SourcesConfig};
use minutes_core::pid::{self, CaptureMode};
use minutes_core::{jobs, live_transcript, notes};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// A WAV smaller than this (header plus about a second of audio) holds nothing worth
/// processing.
const MIN_USEFUL_WAV_BYTES: u64 = 44 + 16_000 * 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// Mic + system audio (Zoom, Meet, Teams…).
    Call,
    /// Mic only.
    InPerson,
}

#[derive(Debug, Clone, Serialize)]
pub struct LiveLine {
    pub line: usize,
    pub offset_ms: u64,
    pub text: String,
    /// "you" | "them" when the call-mode heuristic is confident.
    pub speaker: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecorderView {
    pub recording: bool,
    pub stopping: bool,
    pub kind: Option<Kind>,
    pub title: Option<String>,
    pub started_at: Option<String>,
    pub elapsed_secs: u64,
    pub level: u32,
    pub lines: Vec<LiveLine>,
    pub notes: String,
}

struct Active {
    session: u64,
    kind: Kind,
    title: Option<String>,
    started_at: DateTime<Local>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<Result<(), String>>>,
    wav: PathBuf,
    lines: Vec<LiveLine>,
    notes: String,
    stopping: bool,
}

#[derive(Default)]
pub struct Recorder {
    active: Mutex<Option<Active>>,
    sessions: AtomicU64,
}

pub fn recorder() -> &'static Recorder {
    static R: OnceLock<Recorder> = OnceLock::new();
    R.get_or_init(Recorder::default)
}

#[derive(Debug, Clone, Serialize)]
pub struct Stopped {
    pub job_id: String,
    pub title: Option<String>,
}

impl Recorder {
    /// True from Start until the audio is safely queued (so also while stopping).
    pub fn is_recording(&self) -> bool {
        self.active.lock().unwrap().is_some()
    }

    pub fn view(&self) -> RecorderView {
        let guard = self.active.lock().unwrap();
        match guard.as_ref() {
            None => RecorderView {
                recording: false,
                stopping: false,
                kind: None,
                title: None,
                started_at: None,
                elapsed_secs: 0,
                level: 0,
                lines: vec![],
                notes: String::new(),
            },
            Some(a) => RecorderView {
                recording: true,
                stopping: a.stopping,
                kind: Some(a.kind),
                title: a.title.clone(),
                started_at: Some(a.started_at.to_rfc3339()),
                elapsed_secs: (Local::now() - a.started_at).num_seconds().max(0) as u64,
                level: if a.stopping { 0 } else { capture::audio_level() },
                lines: a.lines.clone(),
                notes: a.notes.clone(),
            },
        }
    }

    pub fn set_notes(&self, text: String) {
        let mut guard = self.active.lock().unwrap();
        if let Some(a) = guard.as_mut() {
            a.notes = text.clone();
            drop(guard);
            // Crash safety: keep the draft on disk while recording.
            let _ = crate::fsutil::write_atomic(&paths::drafts_dir().join("current.md"), text.as_bytes(), Some(0o600));
        }
    }

    pub fn set_title(&self, title: Option<String>) {
        let changed = match self.active.lock().unwrap().as_mut() {
            Some(a) => {
                a.title = title.filter(|t| !t.trim().is_empty());
                true
            }
            None => false,
        };
        if changed {
            // The sidebar and menu bar show the title.
            events::emit("recorder", self.view());
        }
    }

    pub fn start(&self, kind: Kind, title: Option<String>) -> Result<RecorderView, String> {
        if self.is_recording() {
            return Err("Already recording.".into());
        }
        // Slow checks (device listing, process scan) run without holding the lock, so the
        // UI can keep asking for the recorder state meanwhile.
        let simulate = simulated_source();
        if simulate.is_none() {
            ensure_permissions(kind)?;
        }
        let cfg = session_config(kind);
        cfg.ensure_dirs().map_err(|e| format!("Couldn't create the meetings folder: {e}"))?;
        let intent = match kind {
            Kind::Call => RecordingIntent::Call,
            Kind::InPerson => RecordingIntent::Room,
        };
        if simulate.is_none() {
            let preflight = capture::preflight_recording(CaptureMode::Meeting, Some(intent), false, &cfg)?;
            if let Some(reason) = preflight.blocking_reason {
                return Err(reason);
            }
            for warning in &preflight.warnings {
                tracing::info!(%warning, "recording preflight");
            }
        }
        if pid::inspect_pid_file(&pid::live_transcript_pid_path()).is_active() {
            return Err("A live transcript session is already running.".into());
        }
        if pid::inspect_pid_file(&pid::voice_pid_path()).is_active() {
            return Err("A voice session is open. Close it before recording.".into());
        }
        minutes_core::sensitive::ensure_inactive_for_recording().map_err(|e| e.to_string())?;

        let mut guard = self.active.lock().unwrap();
        if guard.is_some() {
            return Err("Already recording.".into());
        }
        if let Ok(Some(owner)) = pid::check_recording() {
            return Err(format!("Another recording is in progress (process {owner})."));
        }
        // Audio left by a crash would block the new capture, and would be processed under
        // this meeting's title. Keep it, but out of the way.
        if let Some(moved) = recover_leftover_capture(&cfg) {
            tracing::warn!(path = %moved.display(), "moved an unfinished earlier capture aside");
            events::emit("error-toast", format!("Audio from an interrupted recording was saved to {}.", moved.display()));
        }
        // Stale lines from the previous meeting would otherwise show until core truncates
        // the file, which happens a moment into capture.
        let _ = std::fs::write(pid::live_transcript_jsonl_path(), b"");

        pid::create().map_err(|e| format!("Couldn't start recording: {e}"))?;
        let started_at = Local::now();
        let _ = pid::write_recording_metadata_with_context(CaptureMode::Meeting, None);
        let _ = notes::save_recording_start();
        notes::clear_consent();
        let _ = std::fs::remove_file(paths::drafts_dir().join("current.md"));

        let stop = Arc::new(AtomicBool::new(false));
        let wav = pid::current_wav_path();
        let handle = {
            let (stop, wav) = (stop.clone(), wav.clone());
            std::thread::Builder::new()
                .name("margin-capture".into())
                .spawn(move || {
                    if let Some(source) = simulate {
                        return simulate_capture(&source, &wav, &stop);
                    }
                    capture::record_to_wav_with_lifecycle(
                        &wav,
                        stop,
                        &cfg,
                        Some(RecordingStartedContext {
                            session_id: None,
                            source: "capture".into(),
                            capabilities: vec![
                                "audio.capture".into(),
                                "live.utterance.final".into(),
                                format!("intent.{}", intent.as_str()),
                            ],
                        }),
                    )
                    .map_err(|e| e.to_string())
                })
                .map_err(|e| {
                    let _ = pid::remove();
                    e.to_string()
                })?
        };

        let session = self.sessions.fetch_add(1, Ordering::SeqCst) + 1;
        *guard = Some(Active {
            session,
            kind,
            title: title.filter(|t| !t.trim().is_empty()),
            started_at,
            stop,
            handle: Some(handle),
            wav,
            lines: vec![],
            notes: String::new(),
            stopping: false,
        });
        drop(guard);
        spawn_ticker(session);
        let view = self.view();
        events::emit("recorder", &view);
        events::refresh_tray();
        Ok(view)
    }

    /// Stop, queue the processing job, and start the worker. `notes` and `title` are the
    /// UI's latest values, which may be newer than the last debounced update.
    pub fn stop(&self, notes: Option<String>, title: Option<String>) -> Result<Stopped, String> {
        self.stop_session(None, notes, title)
    }

    fn stop_session(&self, session: Option<u64>, notes: Option<String>, title: Option<String>) -> Result<Stopped, String> {
        let handle = {
            let mut guard = self.active.lock().unwrap();
            let a = guard.as_mut().ok_or("Not recording.")?;
            if session.is_some_and(|s| s != a.session) {
                return Err("That recording already ended.".into());
            }
            if a.stopping {
                return Err("Already stopping.".into());
            }
            a.stopping = true;
            if let Some(n) = notes {
                a.notes = n;
            }
            if let Some(t) = title {
                a.title = Some(t).filter(|t| !t.trim().is_empty());
            }
            a.stop.store(true, Ordering::SeqCst);
            a.handle.take()
        };
        events::emit("recorder", self.view());
        let capture_result = handle
            .map(|h| h.join().unwrap_or_else(|_| Err("The recorder crashed.".into())))
            .unwrap_or(Ok(()));
        let finished_at = Local::now();
        let (wav, title, started_at, draft) = {
            let guard = self.active.lock().unwrap();
            let a = guard.as_ref().expect("active while stopping");
            (a.wav.clone(), a.title.clone(), a.started_at, a.notes.clone())
        };

        let result = (|| {
            let size = std::fs::metadata(&wav).map(|m| m.len()).unwrap_or(0);
            if let Err(error) = &capture_result {
                tracing::warn!(%error, size, "capture ended with an error");
                if size < MIN_USEFUL_WAV_BYTES {
                    recover_leftover_capture(&corecfg::load());
                    return Err(format!("Recording failed: {error}"));
                }
            } else if size < MIN_USEFUL_WAV_BYTES {
                recover_leftover_capture(&corecfg::load());
                return Err("The recording was too short to save.".into());
            }
            let job = jobs::queue_live_capture_with_recording_health(
                CaptureMode::Meeting,
                title,
                &wav,
                notes_for_summarizer(&draft),
                None,
                Some(started_at),
                Some(finished_at),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .map_err(|e| {
                recover_leftover_capture(&corecfg::load());
                format!("Couldn't queue the recording for processing: {e}. The audio is in your meetings folder under failed-captures.")
            })?;
            processing::save_draft(&job.id, &draft);
            let _ = pid::set_processing_status(
                job.stage.as_deref(),
                Some(CaptureMode::Meeting),
                job.title.as_deref(),
                Some(&job.id),
                jobs::active_job_count(),
            );
            Ok(Stopped { job_id: job.id, title: job.title })
        })();
        let _ = pid::remove();
        let _ = pid::clear_recording_metadata();
        notes::cleanup();
        if result.is_ok() {
            let _ = std::fs::remove_file(paths::drafts_dir().join("current.md"));
            processing::spawn_worker();
        }
        *self.active.lock().unwrap() = None;
        events::emit("recorder", self.view());
        events::refresh_tray();
        result
    }

    /// For Quit: stop a running recording, or wait for a stop already in progress.
    pub fn finish_before_exit(&self, timeout: Duration) {
        if self.is_recording() {
            let _ = self.stop(None, None);
        }
        let deadline = Instant::now() + timeout;
        while self.is_recording() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// Ask macOS before opening any device. Opening an input while the decision is still
/// pending can block forever if the prompt never shows (see core's macos_permissions), and
/// a denied system-audio permission makes the tap record silence.
fn ensure_permissions(kind: Kind) -> Result<(), String> {
    let state = crate::permissions::state();
    let mic = match state.microphone {
        "not-determined" => crate::permissions::request_microphone(),
        other => other,
    };
    if mic == "denied" {
        return Err("Margin can't use the microphone. Allow it in System Settings → Privacy & Security → Microphone, then try again.".into());
    }
    if mic != "granted" {
        return Err("macOS didn't answer the microphone request. Check for a permission prompt, or allow Margin in System Settings → Privacy & Security → Microphone.".into());
    }
    if kind == Kind::Call {
        let sys = match state.system_audio {
            "not-determined" => crate::permissions::request_system_audio(),
            other => other,
        };
        if sys == "denied" {
            return Err("Margin can't hear the other people on the call. Allow it in System Settings → Privacy & Security → Screen & System Audio Recording (under System Audio Recording Only), or record in person instead.".into());
        }
    }
    Ok(())
}

pub fn session_config(kind: Kind) -> Config {
    let mut cfg = corecfg::load();
    match kind {
        Kind::Call => {
            cfg.recording.capture_backend = "core-audio-tap".into();
            cfg.recording.sources = Some(SourcesConfig { voice: None, call: Some("auto".into()) });
        }
        Kind::InPerson => cfg.recording.sources = None,
    }
    cfg
}

/// Move `current.wav` and its stems to `<meetings>/failed-captures/`, if any exist.
pub fn recover_leftover_capture(cfg: &Config) -> Option<PathBuf> {
    let wav = pid::current_wav_path();
    let stems = capture::stem_paths_for(&wav)?;
    let found: Vec<(PathBuf, &str)> = [(wav, ".wav"), (stems.voice, ".voice.wav"), (stems.system, ".system.wav")]
        .into_iter()
        .filter(|(p, _)| p.exists())
        .collect();
    if found.is_empty() {
        return None;
    }
    let dir = cfg.output_dir.join("failed-captures");
    std::fs::create_dir_all(&dir).ok()?;
    let stamp = Local::now().format("%Y-%m-%d-%H%M%S");
    let mut first = None;
    for (src, suffix) in found {
        let dest = dir.join(format!("{stamp}-interrupted{suffix}"));
        if std::fs::rename(&src, &dest).is_err() {
            if std::fs::copy(&src, &dest).is_ok() {
                let _ = std::fs::remove_file(&src);
            } else {
                continue;
            }
        }
        first.get_or_insert(dest);
    }
    first
}

/// The pipeline turns each notes line into a bullet, so strip the user's own list markers
/// to avoid "- - item". The exact text is spliced back after processing.
fn notes_for_summarizer(notes: &str) -> Option<String> {
    let cleaned: Vec<String> = notes
        .lines()
        .map(|l| l.trim().trim_start_matches(['-', '*', '•']).trim_start().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    (!cleaned.is_empty()).then(|| cleaned.join("\n"))
}

fn spawn_ticker(session: u64) {
    std::thread::Builder::new()
        .name("margin-recorder-tick".into())
        .spawn(move || {
            let r = recorder();
            let mut since = 0usize;
            loop {
                std::thread::sleep(Duration::from_millis(500));
                let (kind, wav, ended) = {
                    let guard = r.active.lock().unwrap();
                    match guard.as_ref() {
                        Some(a) if a.session == session && !a.stopping => {
                            (a.kind, a.wav.clone(), a.handle.as_ref().is_some_and(|h| h.is_finished()))
                        }
                        _ => return,
                    }
                };
                if ended {
                    // Silence auto-stop, max duration, low disk space, or a capture error.
                    match r.stop_session(Some(session), None, None) {
                        Ok(_) => events::emit("recording-ended", "The recording stopped on its own and is being processed."),
                        Err(e) => events::emit("error-toast", e),
                    }
                    return;
                }
                let read = live_transcript::read_since_line(since).unwrap_or_default();
                let new_lines: Vec<LiveLine> = read
                    .into_iter()
                    .filter(|l| l.line > since)
                    .map(|l| LiveLine {
                        line: l.line,
                        offset_ms: l.offset_ms.saturating_sub(l.duration_ms),
                        speaker: match kind {
                            Kind::Call => guess_speaker(&wav, l.ts, l.duration_ms),
                            Kind::InPerson => None,
                        },
                        text: l.text,
                    })
                    .collect();
                if let Some(last) = new_lines.last() {
                    since = last.line;
                    if let Some(a) = r.active.lock().unwrap().as_mut() {
                        a.lines.extend(new_lines.iter().cloned());
                    }
                    events::emit("live-lines", &new_lines);
                }
                let view_level = capture::audio_level();
                let elapsed = {
                    let guard = r.active.lock().unwrap();
                    guard.as_ref().map(|a| (Local::now() - a.started_at).num_seconds().max(0)).unwrap_or(0)
                };
                events::emit("recorder-tick", serde_json::json!({ "elapsed_secs": elapsed, "level": view_level }));
            }
        })
        .expect("spawn ticker");
}

/// Compare the loudness of the mic stem and the system stem over the line's audio.
///
/// Core stamps a live line when it is written, after the utterance ended and was
/// transcribed. So we measure from the end of each stem file: the utterance ended about
/// `now - ts` ago and lasted `duration_ms`.
fn guess_speaker(wav: &Path, ts: DateTime<Local>, duration_ms: u64) -> Option<&'static str> {
    let stems = capture::stem_paths_for(wav)?;
    let lag_ms = (Local::now() - ts).num_milliseconds().max(0) as u64;
    let voice = window_rms_from_end(&stems.voice, lag_ms, duration_ms)?;
    let system = window_rms_from_end(&stems.system, lag_ms, duration_ms)?;
    if voice < 0.003 && system < 0.003 {
        return None;
    }
    if voice > system * 1.5 {
        Some("you")
    } else if system > voice * 1.5 {
        Some("them")
    } else {
        None
    }
}

/// RMS of a 16 kHz, 16-bit mono WAV over `[end - duration, end]`, where `end` is `lag_ms`
/// before the end of the data written so far. The file may still be growing, so its
/// header sizes aren't final; we read the raw samples after the `data` chunk id.
fn window_rms_from_end(path: &Path, lag_ms: u64, duration_ms: u64) -> Option<f32> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut header = [0u8; 256];
    let n = file.read(&mut header).ok()?;
    let data_at = header[..n].windows(4).position(|w| w == b"data")? as u64 + 8;
    let total_ms = len.saturating_sub(data_at) / 32;
    let end_ms = total_ms.saturating_sub(lag_ms);
    let start_ms = end_ms.saturating_sub(duration_ms.clamp(200, 30_000));
    let bytes = ((end_ms - start_ms) * 32) as usize;
    if bytes < 3200 {
        return None;
    }
    file.seek(SeekFrom::Start(data_at + start_ms * 32)).ok()?;
    let mut buf = vec![0u8; bytes];
    let read = file.read(&mut buf).ok()?;
    let samples = buf[..read - read % 2].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0);
    let (sum, count) = samples.fold((0f64, 0usize), |(s, c), x| (s + (x * x) as f64, c + 1));
    (count > 0).then(|| (sum / count as f64).sqrt() as f32)
}

/// Debug builds only: `MARGIN_SIMULATE_CAPTURE=<wav>` replaces the microphone with a file,
/// so the whole record → live transcript → stop → process flow can be tested without
/// permissions (scripts/e2e). Release builds ignore it.
fn simulated_source() -> Option<PathBuf> {
    if !cfg!(debug_assertions) {
        return None;
    }
    std::env::var_os("MARGIN_SIMULATE_CAPTURE").map(PathBuf::from).filter(|p| p.exists())
}

/// Plays `source` (16 kHz mono 16-bit) into `dest` in real time, copying its
/// `.voice.wav` and `.system.wav` stems alongside when present. Lines from
/// `<source>.live.jsonl` (`{"offset_ms","duration_ms","text"}`) are written to the live
/// transcript once their time has passed, as core's sidecar would write them.
fn simulate_capture(source: &Path, dest: &Path, stop: &AtomicBool) -> Result<(), String> {
    let spec = hound::WavSpec { channels: 1, sample_rate: 16_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let read = |p: &Path| -> Option<Vec<i16>> { hound::WavReader::open(p).ok()?.samples::<i16>().collect::<Result<_, _>>().ok() };
    let main = read(source).ok_or("simulated source unreadable")?;
    let src_stems = capture::stem_paths_for(source);
    let dst_stems = capture::stem_paths_for(dest).ok_or("bad dest")?;
    let mut tracks: Vec<(Vec<i16>, hound::WavWriter<std::io::BufWriter<std::fs::File>>)> =
        vec![(main, hound::WavWriter::create(dest, spec).map_err(|e| e.to_string())?)];
    if let Some(stems) = src_stems {
        for (src, dst) in [(stems.voice, dst_stems.voice), (stems.system, dst_stems.system)] {
            if let Some(samples) = read(&src) {
                tracks.push((samples, hound::WavWriter::create(&dst, spec).map_err(|e| e.to_string())?));
            }
        }
    }
    let lines: Vec<serde_json::Value> = std::fs::read_to_string(source.with_extension("live.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let started = Instant::now();
    let start_wall = Local::now();
    let mut written = 0usize;
    let mut next_line = 0usize;
    let total = tracks[0].0.len();
    while written < total && !stop.load(Ordering::SeqCst) {
        let chunk = 1600.min(total - written); // 100 ms
        for (samples, writer) in tracks.iter_mut() {
            for &s in samples.iter().skip(written).take(chunk) {
                writer.write_sample(s).map_err(|e| e.to_string())?;
            }
            writer.flush().map_err(|e| e.to_string())?;
        }
        written += chunk;
        let elapsed_ms = started.elapsed().as_millis() as u64;
        while let Some(l) = lines.get(next_line) {
            let end = l["offset_ms"].as_u64().unwrap_or(0) + l["duration_ms"].as_u64().unwrap_or(0);
            if end + 1200 > elapsed_ms {
                break;
            }
            next_line += 1;
            let line = serde_json::json!({
                "line": next_line,
                "ts": (start_wall + chrono::Duration::milliseconds(elapsed_ms as i64)).to_rfc3339(),
                "offset_ms": end,
                "duration_ms": l["duration_ms"],
                "text": l["text"],
                "speaker": null,
            });
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(pid::live_transcript_jsonl_path()).map_err(|e| e.to_string())?;
            writeln!(f, "{line}").map_err(|e| e.to_string())?;
        }
        let target = Duration::from_millis((written as u64) / 16);
        if let Some(wait) = target.checked_sub(started.elapsed()) {
            std::thread::sleep(wait);
        }
    }
    for (_, writer) in tracks {
        writer.finalize().map_err(|e| e.to_string())?;
    }
    // Like core's silence auto-stop: the source ran out before Stop was pressed.
    Ok(())
}

/// A draft left by a crash mid-recording (shown once in the UI).
pub fn orphaned_draft() -> Option<String> {
    std::fs::read_to_string(paths::drafts_dir().join("current.md")).ok().filter(|s| !s.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav_with(samples: &[i16], path: &Path) {
        let spec = hound::WavSpec { channels: 1, sample_rate: 16_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for s in samples {
            w.write_sample(*s).unwrap();
        }
        w.finalize().unwrap();
    }

    #[test]
    fn rms_window_is_measured_from_the_end() {
        let dir = std::env::temp_dir().join(format!("margin-rms-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // 2 s of silence, then 1 s of loud signal.
        let mut s = vec![0i16; 32_000];
        s.extend(std::iter::repeat_n(8_000i16, 16_000));
        let p = dir.join("x.wav");
        wav_with(&s, &p);
        let loud = window_rms_from_end(&p, 0, 1000).unwrap();
        let quiet = window_rms_from_end(&p, 1000, 1000).unwrap();
        assert!(loud > 0.2, "loud {loud}");
        assert!(quiet < 0.01, "quiet {quiet}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn summarizer_notes_drop_markers() {
        assert_eq!(notes_for_summarizer("- a\n\n* b\n• c\n").as_deref(), Some("a\nb\nc"));
        assert_eq!(notes_for_summarizer("  \n"), None);
    }
}
