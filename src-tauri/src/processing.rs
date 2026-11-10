//! Post-recording processing (transcribe, diarize, summarize, write the Markdown). It runs in
//! a child process, as in both upstream frontends: whisper and ONNX get their own address
//! space, and processing keeps going if the window is closed.

use crate::recorder::LiveLine;
use crate::{corecfg, events, library, paths};
use minutes_core::jobs::{self, JobState};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
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
    let mut cfg = corecfg::load();
    // The meeting opens as soon as the transcript and speakers are done; Margin writes the
    // summary afterwards, in the app (DECISIONS.md D40). A long meeting no longer waits
    // minutes for its summary before you can read it.
    cfg.summarization.engine = "none".into();
    let code = match jobs::process_pending_jobs(&cfg, |job| {
        tracing::info!(id = %job.id, state = ?job.state, "job update");
        if job.state == JobState::Diarizing {
            hold_for_tests();
        }
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

/// Debug builds only: while `$MARGIN_HOME/e2e-hold-speakers` exists, the worker waits at
/// "separating speakers" (at most two minutes). The e2e run uses it to look at a meeting
/// that is open while core still processes it. Release builds ignore it.
fn hold_for_tests() {
    if !cfg!(debug_assertions) {
        return;
    }
    let marker = paths::app_root().join("e2e-hold-speakers");
    let deadline = std::time::Instant::now() + Duration::from_secs(120);
    while marker.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn notes_draft_path(job_id: &str) -> PathBuf {
    paths::drafts_dir().join(format!("{job_id}.md"))
}

fn live_draft_path(job_id: &str) -> PathBuf {
    paths::drafts_dir().join(format!("{job_id}.live.json"))
}

/// Held while a notes draft is written, and while `finish` moves it into the meeting file,
/// so an edit can never land in a draft that was already moved.
static DRAFTS: Mutex<()> = Mutex::new(());

/// The notes sent with Stop. An empty recording leaves no draft.
pub fn save_draft(job_id: &str, notes: &str) {
    if notes.trim().is_empty() {
        return;
    }
    let _ = crate::fsutil::write_atomic(&notes_draft_path(job_id), notes.as_bytes(), Some(0o600));
}

/// The live transcript lines of the recording, kept with the job. The processing view shows
/// them as a draft transcript until the meeting file exists.
pub fn save_live_draft(job_id: &str, lines: &[LiveLine]) {
    if lines.is_empty() {
        return;
    }
    if let Ok(json) = serde_json::to_vec(lines) {
        let _ = crate::fsutil::write_atomic(&live_draft_path(job_id), &json, Some(0o600));
    }
}

pub fn live_draft(job_id: &str) -> Vec<LiveLine> {
    std::fs::read(live_draft_path(job_id))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn same_file(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
}

/// The job that core may still write `path` for, if any.
fn active_job_for(path: &Path) -> Option<jobs::ProcessingJob> {
    jobs::active_jobs()
        .into_iter()
        .find(|j| j.output_path.as_deref().is_some_and(|o| same_file(Path::new(o), path)))
}

/// Edits that rewrite the meeting file (title, speakers, action items, summary, Trash)
/// wait until core is done with it. A write in between makes core's last rewrite fail.
pub fn ensure_not_processing(path: &Path) -> Result<(), String> {
    match active_job_for(path) {
        Some(_) => Err("Margin is still finishing this meeting. Try again when it's done.".into()),
        None => Ok(()),
    }
}

/// The job whose notes draft stands in for the notes of `path`: a job that still writes the
/// file, or one whose draft is not in the file yet (it moves in when the job completes).
fn notes_job_for(path: &Path) -> Option<String> {
    if let Some(job) = active_job_for(path) {
        return Some(job.id);
    }
    let entries = std::fs::read_dir(paths::drafts_dir()).ok()?;
    entries.flatten().find_map(|entry| {
        let name = entry.file_name().into_string().ok()?;
        let id = name.strip_suffix(".md").filter(|id| *id != "current")?;
        let job = jobs::load_job(id)?;
        job.output_path.as_deref().is_some_and(|o| same_file(Path::new(o), path)).then(|| job.id)
    })
}

/// The notes to show for `path`: the job's draft while it has one, else `None` (use the
/// file's own notes).
pub fn notes_draft_for(path: &Path) -> Option<String> {
    let id = notes_job_for(path)?;
    std::fs::read_to_string(notes_draft_path(&id)).ok()
}

/// Save notes typed in the meeting view. While core may still write the file, they go into
/// the job's notes draft, and `finish` puts them in the file when the job completes.
pub fn save_notes(path: &Path, notes: &str) -> Result<(), String> {
    let drafts = DRAFTS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(id) = notes_job_for(path) {
        // An empty draft is kept too: it means "the notes were cleared".
        return crate::fsutil::write_atomic(&notes_draft_path(&id), notes.as_bytes(), Some(0o600)).map_err(|e| e.to_string());
    }
    drop(drafts);
    library::replace_notes(path, notes)
}

/// A meeting is queued or being processed.
pub fn busy() -> bool {
    !jobs::active_jobs().is_empty()
}

/// Asks before Quit while a meeting is still processing. True means quit.
///
/// The worker runs in its own process and should outlive Margin. On 2026-11-10 it stopped
/// 10 s after a Quit, cause unknown, and the retry transcribed the call again from the
/// start: 100 minutes of work lost (D55). Until a retry can resume, Quit asks first.
pub fn confirm_quit(app: &tauri::AppHandle) -> bool {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    let active = jobs::active_jobs();
    if active.is_empty() {
        return true;
    }
    let keep = app
        .dialog()
        .message(quit_message(&active.iter().map(|j| j.title.clone()).collect::<Vec<_>>()))
        .title("Margin is still transcribing")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom("Keep Running".into(), "Quit Anyway".into()))
        .blocking_show();
    !keep
}

fn quit_message(titles: &[Option<String>]) -> String {
    let first = titles.iter().flatten().next().map_or("Your meeting".to_string(), |t| format!("“{t}”"));
    let what = match titles.len() {
        0 | 1 => format!("{first} is not ready yet."),
        2 => format!("{first} and 1 more meeting are not ready yet."),
        n => format!("{first} and {} more meetings are not ready yet.", n - 1),
    };
    format!("{what} If you quit now, transcription can stop and start again from the beginning when you open Margin.")
}

pub fn job_views() -> Vec<JobView> {
    jobs::display_jobs(Some(30), true)
        .into_iter()
        .map(|j| JobView {
            draft: std::fs::read_to_string(notes_draft_path(&j.id)).ok(),
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
///
/// Each second it reads only the active job files (usually none) and the timestamps of the
/// archive and drafts folders. The full listing, which reads every archived job, runs only
/// when one of those changed (docs/perf.md).
pub fn spawn_watcher() {
    // Resume anything left from a previous run (crash, quit mid-processing).
    if !jobs::active_jobs().is_empty() {
        spawn_worker();
    }
    std::thread::Builder::new()
        .name("margin-jobs".into())
        .spawn(move || {
            let mut last: HashMap<String, JobView> = HashMap::new();
            let mut last_key = None;
            let mut first = true;
            let mut idle_polls = 0u32;
            loop {
                // list_jobs also re-queues jobs whose worker died, so it runs every poll.
                let active = jobs::list_jobs();
                let key = change_key(&active);
                if last_key.as_ref() != Some(&key) {
                    last_key = Some(key);
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
                    if changed || views.len() != last.len() {
                        events::emit("jobs", &views);
                        last = views.into_iter().map(|v| (v.id.clone(), v)).collect();
                    }
                }
                // A job queued just as the worker finished its loop would wait forever.
                let pending = active.iter().any(|j| j.state == JobState::Queued);
                idle_polls = if pending && !jobs::worker_active() { idle_polls + 1 } else { 0 };
                if idle_polls >= 5 {
                    idle_polls = 0;
                    spawn_worker();
                }
                std::thread::sleep(Duration::from_millis(1000));
            }
        })
        .expect("spawn job watcher");
}

/// Changes whenever a job view can change: an active job file, a job moved into or out of
/// the archive, or a draft written or removed. Core and Margin write all of these by
/// rename, which updates the folder's modification time.
fn change_key(active: &[jobs::ProcessingJob]) -> String {
    let stamp = |dir: std::path::PathBuf| std::fs::metadata(dir).and_then(|m| m.modified()).ok();
    format!(
        "{}|{:?}|{:?}",
        serde_json::to_string(active).unwrap_or_default(),
        stamp(jobs::archive_dir()),
        stamp(paths::drafts_dir()),
    )
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
    let Some(output) = job.output_path.as_deref() else {
        return;
    };
    // A failed job may be retried, and a retry rewrites the file. Keep the drafts until a
    // run succeeds.
    if !matches!(job.state, JobState::Complete | JobState::NeedsReview) {
        return;
    }
    restore_notes(job, Path::new(output));
    let _ = std::fs::remove_file(live_draft_path(&job.id));
    if announce {
        events::emit("meeting-ready", serde_json::json!({ "job_id": job.id, "path": output }));
        crate::summaries::summarize_in_background(output.to_string());
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

/// Put the notes draft (the exact notes, with edits made during processing) into the
/// finished file, then drop the draft.
fn restore_notes(job: &JobView, out: &Path) {
    let _drafts = DRAFTS.lock().unwrap_or_else(|e| e.into_inner());
    let draft = notes_draft_path(&job.id);
    let Ok(notes) = std::fs::read_to_string(&draft) else {
        return;
    };
    // Core wrote the notes sent with Stop, as cleaned bullets. If the section differs from
    // that, someone (Obsidian, say, while Margin was closed) edited it since: theirs wins.
    let queued = jobs::load_job(&job.id).and_then(|j| j.user_notes);
    if !core_wrote_these_notes(&library::notes_section(out).unwrap_or_default(), queued.as_deref()) {
        tracing::info!(output = %out.display(), "notes changed since processing; keeping them");
        let _ = std::fs::remove_file(&draft);
        return;
    }
    match library::replace_notes(out, &notes) {
        Ok(()) => {
            let _ = std::fs::remove_file(&draft);
        }
        Err(error) => tracing::warn!(%error, output = %out.display(), "could not restore notes"),
    }
}

/// True when `written` (the file's notes section) is what core renders for the notes it
/// was given: one bullet per line.
fn core_wrote_these_notes(written: &str, queued: Option<&str>) -> bool {
    let expected = queued
        .map(|n| n.lines().map(|l| format!("- {l}")).collect::<Vec<_>>().join("\n"))
        .unwrap_or_default();
    written.trim().is_empty() || written.trim() == expected.trim()
}

#[cfg(test)]
mod tests {
    #[test]
    fn quit_message_names_the_meeting() {
        let one = super::quit_message(&[Some("Weekly".into())]);
        assert!(one.starts_with("“Weekly” is not ready yet. If you quit now"), "{one}");
        assert!(super::quit_message(&[None]).starts_with("Your meeting is not ready yet."));
        let two = super::quit_message(&[None, Some("Weekly".into())]);
        assert!(two.starts_with("“Weekly” and 1 more meeting are not ready yet."), "{two}");
        let three = super::quit_message(&[Some("A".into()), None, None]);
        assert!(three.starts_with("“A” and 2 more meetings are not ready yet."), "{three}");
    }

    use super::*;

    #[test]
    fn the_notes_guard_accepts_only_what_core_wrote() {
        assert!(core_wrote_these_notes("- a\n- b\n", Some("a\nb")));
        assert!(core_wrote_these_notes("", Some("a")));
        assert!(core_wrote_these_notes("", None));
        assert!(!core_wrote_these_notes("- a\n- edited in Obsidian", Some("a")));
        assert!(!core_wrote_these_notes("- something", None));
    }
}
