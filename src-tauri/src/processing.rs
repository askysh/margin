//! Post-recording processing (transcribe, diarize, summarize, write the Markdown). It runs in
//! a child process, as in both upstream frontends: whisper and ONNX get their own address
//! space, and processing keeps going if the window is closed.

use crate::{corecfg, events, library, paths};
use minutes_core::jobs::{self, JobState};
use serde::Serialize;
use std::collections::HashMap;
use std::time::Duration;

pub const WORKER_FLAG: &str = "--process-queue-worker";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct JobView {
    pub id: String,
    pub title: Option<String>,
    pub state: JobState,
    pub stage: Option<String>,
    pub output_path: Option<String>,
    pub created_at: String,
    pub error: Option<String>,
    /// Notes typed during the recording, until they're added to the meeting file.
    pub draft: Option<String>,
}

pub fn run_worker_mode() -> i32 {
    let cfg = corecfg::load();
    let code = match jobs::process_pending_jobs(&cfg, |job| {
        tracing::info!(id = %job.id, state = ?job.state, "job update");
    }) {
        Ok(()) => 0,
        Err(error) => {
            tracing::error!(%error, "processing worker failed");
            1
        }
    };
    minutes_core::hooks::drain(minutes_core::hooks::HOOK_DEADLINE);
    code
}

pub fn spawn_worker() {
    if jobs::worker_active() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let log_dir = paths::state_dir().join("logs");
    let _ = std::fs::create_dir_all(&log_dir);
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("margin-worker.log"));
    let mut cmd = std::process::Command::new(exe);
    // The state directory, where core finds the summary-instructions template.
    cmd.arg(WORKER_FLAG).current_dir(paths::state_dir()).stdin(std::process::Stdio::null());
    if let Ok(log) = log {
        if let Ok(clone) = log.try_clone() {
            cmd.stdout(clone);
        }
        cmd.stderr(log);
    }
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
    match minutes_core::spawn_reaped(&mut cmd) {
        Ok(_) => tracing::info!("processing worker started"),
        Err(error) => tracing::error!(%error, "could not start processing worker"),
    }
}

pub fn save_draft(job_id: &str, notes: &str) {
    if notes.trim().is_empty() {
        return;
    }
    let _ = crate::fsutil::write_atomic(
        &paths::drafts_dir().join(format!("{job_id}.md")),
        notes.as_bytes(),
        Some(0o600),
    );
}

pub fn job_views() -> Vec<JobView> {
    jobs::display_jobs(Some(30), true)
        .into_iter()
        .map(|j| JobView {
            draft: std::fs::read_to_string(paths::drafts_dir().join(format!("{}.md", j.id))).ok(),
            id: j.id,
            title: j.title,
            state: j.state,
            stage: j.stage,
            output_path: j.output_path,
            created_at: j.created_at.to_rfc3339(),
            error: j.error,
        })
        .collect()
}

/// Polls job files, pushes changes to the UI, and puts the user's exact notes into the
/// finished meeting file.
pub fn spawn_watcher() {
    // Resume anything left from a previous run (crash, quit mid-processing).
    if !jobs::active_jobs().is_empty() {
        spawn_worker();
    }
    std::thread::Builder::new()
        .name("margin-jobs".into())
        .spawn(move || {
            let mut last: HashMap<String, JobView> = HashMap::new();
            let mut first = true;
            let mut idle_polls = 0u32;
            loop {
                let views = job_views();
                let mut changed = false;
                for view in &views {
                    if last.get(&view.id) != Some(view) {
                        changed = true;
                        if view.state.is_terminal() {
                            finish(view, !first);
                        }
                    }
                }
                first = false;
                // A job queued just as the worker finished its loop would wait forever.
                let pending = views.iter().any(|v| v.state == JobState::Queued);
                idle_polls = if pending && !jobs::worker_active() { idle_polls + 1 } else { 0 };
                if idle_polls >= 5 {
                    idle_polls = 0;
                    spawn_worker();
                }
                if changed || views.len() != last.len() {
                    events::emit("jobs", &views);
                    last = views.into_iter().map(|v| (v.id.clone(), v)).collect();
                }
                std::thread::sleep(Duration::from_millis(1000));
            }
        })
        .expect("spawn job watcher");
}

/// `announce` is false on the first poll, so old jobs from the archive don't each fire
/// "meeting-ready". Their leftover drafts are still restored.
fn finish(job: &JobView, announce: bool) {
    if announce && job.state == JobState::Failed {
        events::notify(events::Notice {
            title: "A meeting couldn't be processed".into(),
            body: "Open Margin to see why and try again.".into(),
            meeting: None,
            call_app: None,
        });
    }
    let draft = paths::drafts_dir().join(format!("{}.md", job.id));
    let Some(output) = job.output_path.as_deref() else {
        return;
    };
    // A failed job may be retried, and a retry rewrites the file. Keep the draft until a
    // run succeeds.
    if !matches!(job.state, JobState::Complete | JobState::NeedsReview) {
        return;
    }
    if let Ok(notes) = std::fs::read_to_string(&draft) {
        let out = std::path::Path::new(output);
        // Core wrote the notes as cleaned bullets. If the section differs from that, someone
        // (Obsidian, say, while Margin was closed) edited it since: theirs wins.
        let written = library::notes_section(out).unwrap_or_default();
        let expected = crate::recorder::notes_for_summarizer(&notes)
            .map(|n| n.lines().map(|l| format!("- {l}")).collect::<Vec<_>>().join("\n"))
            .unwrap_or_default();
        if written.trim() != expected.trim() && !written.trim().is_empty() {
            tracing::info!(output, "notes changed since processing; keeping them");
            let _ = std::fs::remove_file(&draft);
        } else {
        match library::replace_notes(out, &notes) {
            Ok(()) => {
                let _ = std::fs::remove_file(&draft);
            }
            Err(error) => tracing::warn!(%error, output, "could not restore notes"),
        }
        }
    }
    if announce {
        events::emit("meeting-ready", serde_json::json!({ "job_id": job.id, "path": output }));
        // Settings → "Keep audio": a good moment to drop audio that is now too old.
        std::thread::spawn(|| corecfg::audio_cleanup(&corecfg::load()));
        events::notify(events::Notice {
            title: "Meeting ready".into(),
            body: format!("“{}” is transcribed and saved.", job.title.as_deref().unwrap_or("Your meeting")),
            meeting: Some(output.to_string()),
            call_app: None,
        });
        // A new meeting for the menu bar's recent list.
        events::refresh_tray();
    }
}
