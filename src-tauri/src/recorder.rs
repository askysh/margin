//! Recording: the same sequence as upstream's `minutes record`, run in-process on a worker
//! thread (RESEARCH.md §1.2). Then live-transcript tailing with "You / Them" guesses
//! (RESEARCH.md §1.3, DECISIONS.md D8).

use crate::{corecfg, paths, processing};
use chrono::{DateTime, Local};
use minutes_core::capture::{self, RecordingIntent, RecordingStartedContext};
use minutes_core::config::SourcesConfig;
use minutes_core::pid::{self, CaptureMode};
use minutes_core::{jobs, live_transcript, notes};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

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
}

#[derive(Default)]
pub struct Recorder {
    active: Mutex<Option<Active>>,
    sessions: AtomicU64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Stopped {
    pub job_id: String,
    pub title: Option<String>,
}

impl Recorder {
    pub fn is_recording(&self) -> bool {
        self.active.lock().unwrap().is_some()
    }

    pub fn view(&self) -> RecorderView {
        let guard = self.active.lock().unwrap();
        match guard.as_ref() {
            None => RecorderView {
                recording: false,
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
                kind: Some(a.kind),
                title: a.title.clone(),
                started_at: Some(a.started_at.to_rfc3339()),
                elapsed_secs: (Local::now() - a.started_at).num_seconds().max(0) as u64,
                level: capture::audio_level(),
                lines: a.lines.clone(),
                notes: a.notes.clone(),
            },
        }
    }

    pub fn set_notes(&self, text: String) {
        if let Some(a) = self.active.lock().unwrap().as_mut() {
            // Crash safety: keep the draft on disk while recording.
            let _ = crate::fsutil::write_atomic(
                &paths::drafts_dir().join("current.md"),
                text.as_bytes(),
                Some(0o600),
            );
            a.notes = text;
        }
    }

    pub fn set_title(&self, title: Option<String>) {
        if let Some(a) = self.active.lock().unwrap().as_mut() {
            a.title = title.filter(|t| !t.trim().is_empty());
        }
    }

    pub fn start(&self, app: &AppHandle, kind: Kind, title: Option<String>) -> Result<RecorderView, String> {
        let mut guard = self.active.lock().unwrap();
        if guard.is_some() {
            return Err("Already recording.".into());
        }
        let mut cfg = corecfg::load();
        match kind {
            Kind::Call => {
                cfg.recording.capture_backend = "core-audio-tap".into();
                cfg.recording.sources = Some(SourcesConfig {
                    voice: None,
                    call: Some("auto".into()),
                });
            }
            Kind::InPerson => cfg.recording.sources = None,
        }
        cfg.ensure_dirs().map_err(|e| format!("Could not create folders: {e}"))?;

        let intent = match kind {
            Kind::Call => RecordingIntent::Call,
            Kind::InPerson => RecordingIntent::Room,
        };
        let preflight = capture::preflight_recording(CaptureMode::Meeting, Some(intent), false, &cfg)?;
        if let Some(reason) = preflight.blocking_reason {
            return Err(reason);
        }
        for warning in &preflight.warnings {
            tracing::info!(%warning, "recording preflight");
        }
        if pid::inspect_pid_file(&pid::live_transcript_pid_path()).is_active() {
            return Err("A live transcript session is already running.".into());
        }

        pid::create().map_err(|e| format!("Could not start recording: {e}"))?;
        let started_at = Local::now();
        let _ = pid::write_recording_metadata_with_context(CaptureMode::Meeting, None);
        let _ = notes::save_recording_start();
        notes::clear_consent();
        let _ = std::fs::remove_file(paths::drafts_dir().join("current.md"));

        let stop = Arc::new(AtomicBool::new(false));
        let wav = pid::current_wav_path();
        let handle = {
            let (stop, wav, cfg) = (stop.clone(), wav.clone(), cfg.clone());
            std::thread::Builder::new()
                .name("margin-capture".into())
                .spawn(move || {
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
        });
        drop(guard);
        spawn_ticker(app.clone(), session);
        crate::tray::refresh(app);
        Ok(self.view())
    }

    /// Stop, queue the processing job, and start the worker. Safe to call twice.
    pub fn stop(&self, app: &AppHandle) -> Result<Stopped, String> {
        let Some(mut active) = self.active.lock().unwrap().take() else {
            return Err("Not recording.".into());
        };
        active.stop.store(true, Ordering::SeqCst);
        let capture_result = active
            .handle
            .take()
            .map(|h| h.join().unwrap_or_else(|_| Err("Recorder thread crashed.".into())))
            .unwrap_or(Ok(()));
        let finished_at = Local::now();
        let result = (|| {
            if let Err(error) = &capture_result {
                // The capture may still have produced audio worth processing.
                tracing::warn!(%error, "capture ended with an error");
                if !active.wav.exists() {
                    return Err(format!("Recording failed: {error}"));
                }
            }
            let summary_notes = notes_for_summarizer(&active.notes);
            let job = jobs::queue_live_capture_with_recording_health(
                CaptureMode::Meeting,
                active.title.clone(),
                &active.wav,
                summary_notes,
                None,
                Some(active.started_at),
                Some(finished_at),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .map_err(|e| format!("Could not queue the recording for processing: {e}"))?;
            processing::save_draft(&job.id, &active.notes);
            let _ = pid::set_processing_status(
                job.stage.as_deref(),
                Some(CaptureMode::Meeting),
                job.title.as_deref(),
                Some(&job.id),
                jobs::active_job_count(),
            );
            Ok(Stopped {
                job_id: job.id,
                title: job.title,
            })
        })();
        let _ = pid::remove();
        let _ = pid::clear_recording_metadata();
        notes::cleanup();
        if result.is_ok() {
            let _ = std::fs::remove_file(paths::drafts_dir().join("current.md"));
            processing::spawn_worker();
        }
        let _ = app.emit("recorder", self.view());
        crate::tray::refresh(app);
        result
    }
}

/// The pipeline turns each notes line into a bullet, so strip the user's own list markers
/// to avoid "- - item". The exact text is spliced back after processing.
fn notes_for_summarizer(notes: &str) -> Option<String> {
    let cleaned: Vec<String> = notes
        .lines()
        .map(|l| {
            let t = l.trim();
            t.trim_start_matches(['-', '*', '•']).trim_start().to_string()
        })
        .filter(|l| !l.is_empty())
        .collect();
    (!cleaned.is_empty()).then(|| cleaned.join("\n"))
}

fn spawn_ticker(app: AppHandle, session: u64) {
    std::thread::Builder::new()
        .name("margin-recorder-tick".into())
        .spawn(move || {
            let mut since = 0usize;
            loop {
                std::thread::sleep(Duration::from_millis(500));
                let recorder = app.state::<Recorder>();
                let (kind, wav, ended) = {
                    let guard = recorder.active.lock().unwrap();
                    match guard.as_ref() {
                        Some(a) if a.session == session => (
                            a.kind,
                            a.wav.clone(),
                            a.handle.as_ref().is_some_and(|h| h.is_finished()),
                        ),
                        _ => return,
                    }
                };
                if ended {
                    // Silence auto-stop, max duration, disk space, or a capture error.
                    let _ = recorder.stop(&app);
                    let _ = app.emit("recording-ended", "The recording stopped on its own.");
                    return;
                }
                let new_lines: Vec<LiveLine> = live_transcript::read_since_line(since)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|l| l.line > since)
                    .map(|l| LiveLine {
                        line: l.line,
                        offset_ms: l.offset_ms,
                        speaker: match kind {
                            Kind::Call => guess_speaker(&wav, l.offset_ms, l.duration_ms),
                            Kind::InPerson => None,
                        },
                        text: l.text,
                    })
                    .collect();
                if let Some(last) = new_lines.last() {
                    since = last.line;
                    if let Some(a) = recorder.active.lock().unwrap().as_mut() {
                        a.lines.extend(new_lines.iter().cloned());
                    }
                    let _ = app.emit("live-lines", &new_lines);
                }
                let view = recorder.view();
                let _ = app.emit(
                    "recorder-tick",
                    serde_json::json!({ "elapsed_secs": view.elapsed_secs, "level": view.level }),
                );
            }
        })
        .expect("spawn ticker");
}

/// Compare loudness of the mic stem and the system stem over the line's time window.
fn guess_speaker(wav: &Path, offset_ms: u64, duration_ms: u64) -> Option<&'static str> {
    let stems = capture::stem_paths_for(wav)?;
    let voice = window_rms(&stems.voice, offset_ms, duration_ms)?;
    let system = window_rms(&stems.system, offset_ms, duration_ms)?;
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

/// RMS of a window of a 16 kHz, 16-bit mono WAV that may still be growing (its header
/// sizes are not final yet, so read raw samples after the `data` chunk id).
fn window_rms(path: &Path, offset_ms: u64, duration_ms: u64) -> Option<f32> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let mut header = [0u8; 256];
    let n = file.read(&mut header).ok()?;
    let data_at = header[..n].windows(4).position(|w| w == b"data")? + 8;
    let start = data_at as u64 + offset_ms * 16 * 2;
    let len = (duration_ms.clamp(200, 30_000) * 16 * 2) as usize;
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = vec![0u8; len];
    let read = file.read(&mut buf).ok()?;
    if read < 3200 {
        return None;
    }
    let samples = buf[..read - read % 2]
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0);
    let (sum, count) = samples.fold((0f64, 0usize), |(s, c), x| (s + (x * x) as f64, c + 1));
    Some((sum / count.max(1) as f64).sqrt() as f32)
}

/// Recover a draft left by a crash mid-recording (shown once in the UI).
pub fn orphaned_draft() -> Option<String> {
    std::fs::read_to_string(paths::drafts_dir().join("current.md"))
        .ok()
        .filter(|s| !s.trim().is_empty())
}
