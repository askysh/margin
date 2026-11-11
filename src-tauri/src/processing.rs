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
    /// When the recording started and stopped, and when processing ended (D58).
    #[serde(flatten)]
    pub times: JobTimes,
}

/// The times in core's job record that Margin shows (D58). RFC 3339, or None when the
/// record does not have them.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct JobTimes {
    pub recording_started_at: Option<String>,
    pub recording_finished_at: Option<String>,
    /// When the job ended. Set only for a job that produced a meeting (Complete or
    /// NeedsReview), so it never shows the time a job failed.
    pub ready_at: Option<String>,
}

impl JobTimes {
    fn of(job: &jobs::ProcessingJob) -> Self {
        let produced = matches!(job.state, JobState::Complete | JobState::NeedsReview);
        JobTimes {
            recording_started_at: job.recording_started_at.map(|t| t.to_rfc3339()),
            recording_finished_at: job.recording_finished_at.map(|t| t.to_rfc3339()),
            ready_at: job.finished_at.filter(|_| produced).map(|t| t.to_rfc3339()),
        }
    }
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

/// Present while a job's meeting had no title at Stop (D59).
fn untitled_marker(job_id: &str) -> PathBuf {
    paths::drafts_dir().join(format!("{job_id}.untitled"))
}

/// The meeting of this job had no title at Stop. When it is ready, its summary names it.
pub fn mark_untitled(job_id: &str) {
    let _ = std::fs::write(untitled_marker(job_id), b"");
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
/// The flag is true for a failed job: a retry rewrites the file, so its draft stays.
fn notes_job_for(path: &Path) -> Option<(String, bool)> {
    if let Some(job) = active_job_for(path) {
        return Some((job.id, false));
    }
    let entries = std::fs::read_dir(paths::drafts_dir()).ok()?;
    entries.flatten().find_map(|entry| {
        let name = entry.file_name().into_string().ok()?;
        let id = name.strip_suffix(".md").filter(|id| *id != "current")?;
        let job = jobs::load_job(id)?;
        let failed = job.state == JobState::Failed;
        job.output_path.as_deref().is_some_and(|o| same_file(Path::new(o), path)).then(|| (job.id, failed))
    })
}

/// The notes to show for `path`: the job's draft while it has one, else `None` (use the
/// file's own notes).
pub fn notes_draft_for(path: &Path) -> Option<String> {
    let (id, _) = notes_job_for(path)?;
    std::fs::read_to_string(notes_draft_path(&id)).ok()
}

/// Save notes typed in the meeting view. While core may still write the file, they go into
/// the job's notes draft, and `finish` puts them in the file when the job completes.
///
/// A job that failed after it wrote the file can be retried, and the retry writes the
/// file again with the notes from Stop. So its draft stays, and the notes go into the
/// file too. Then the file has them even if nobody retries, and after a Reset, which
/// deletes the drafts.
pub fn save_notes(path: &Path, notes: &str) -> Result<(), String> {
    let drafts = DRAFTS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((id, failed)) = notes_job_for(path) {
        // An empty draft is kept too: it means "the notes were cleared".
        crate::fsutil::write_atomic(&notes_draft_path(&id), notes.as_bytes(), Some(0o600)).map_err(|e| e.to_string())?;
        if !failed {
            return Ok(());
        }
    }
    drop(drafts);
    crate::summaries::save_notes(&path.display().to_string(), notes, || library::replace_notes(path, notes))
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
            times: JobTimes::of(&j),
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

/// The times of the job that wrote the meeting file at `path`, or None for a meeting that
/// no job record names (an older meeting, a file renamed since, a meeting from the minutes
/// CLI). Core keeps every finished job in its archive and never prunes it (D58).
pub fn times_for(path: &Path) -> Option<JobTimes> {
    let key = path_key(&path.to_string_lossy());
    if let Some(found) = newest_for(&jobs::list_jobs(), &key) {
        return Some(found);
    }
    // The archive listing reads one file per finished job. Keep it until the archive
    // folder changes, as the jobs watcher does (D32).
    static ARCHIVE: Mutex<Option<(Option<std::time::SystemTime>, Vec<jobs::ProcessingJob>)>> = Mutex::new(None);
    let stamp = std::fs::metadata(jobs::archive_dir()).and_then(|m| m.modified()).ok();
    let mut cache = ARCHIVE.lock().unwrap_or_else(|e| e.into_inner());
    if cache.as_ref().map(|(s, _)| s) != Some(&stamp) {
        let finished = jobs::display_jobs(None, true).into_iter().filter(|j| j.state.is_terminal()).collect();
        *cache = Some((stamp, finished));
    }
    cache.as_ref().and_then(|(_, all)| newest_for(all, &key))
}

/// Of the jobs that wrote `key`, the newest. A file can have more than one job when it was
/// processed again.
fn newest_for(all: &[jobs::ProcessingJob], key: &str) -> Option<JobTimes> {
    all.iter()
        .filter(|j| j.output_path.as_deref().is_some_and(|p| path_key(p) == key))
        .max_by_key(|j| j.created_at)
        .map(JobTimes::of)
}

/// One spelling per file: /tmp and /private/tmp, or a symlinked folder, name the same file.
fn path_key(path: &str) -> String {
    std::fs::canonicalize(path).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| path.to_string())
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
    // run succeeds, and put a copy of the notes in the file it wrote, if any.
    if job.state == JobState::Failed {
        copy_notes_to_failed(job, Path::new(output));
        return;
    }
    if !matches!(job.state, JobState::Complete | JobState::NeedsReview) {
        return;
    }
    restore_notes(job, Path::new(output));
    let _ = std::fs::remove_file(live_draft_path(&job.id));
    let untitled = std::fs::remove_file(untitled_marker(&job.id)).is_ok();
    if announce {
        events::emit("meeting-ready", serde_json::json!({ "job_id": job.id, "path": output }));
        crate::summaries::summarize_in_background(output.to_string(), untitled);
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

/// A job that failed after it wrote the meeting file: copy its notes draft into the file,
/// and keep the draft for a retry. Notes edited in the file since (Obsidian) win, as in
/// `restore_notes`.
fn copy_notes_to_failed(job: &JobView, out: &Path) {
    let _drafts = DRAFTS.lock().unwrap_or_else(|e| e.into_inner());
    let Ok(notes) = std::fs::read_to_string(notes_draft_path(&job.id)) else { return };
    let Some(written) = library::notes_section(out) else { return };
    let queued = jobs::load_job(&job.id).and_then(|j| j.user_notes);
    if !core_wrote_these_notes(&written, queued.as_deref()) {
        return;
    }
    if let Err(error) = library::replace_notes(out, &notes) {
        tracing::warn!(%error, output = %out.display(), "could not copy notes into the file of a failed job");
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

    fn job(json: serde_json::Value) -> jobs::ProcessingJob {
        let mut base = serde_json::json!({
            "id": "j", "mode": "meeting", "content_type": "meeting", "title": null,
            "audio_path": "/a.wav", "state": "complete", "created_at": "2026-11-10T12:24:05+05:30"
        });
        base.as_object_mut().unwrap().extend(json.as_object().unwrap().clone());
        serde_json::from_value(base).expect("a job record as core writes it")
    }

    #[test]
    fn job_times_come_from_the_newest_job_that_wrote_the_file() {
        let all = vec![
            job(serde_json::json!({ "id": "old", "output_path": "/m/a.md", "created_at": "2026-11-10T12:24:05+05:30",
                "recording_started_at": "2026-11-10T11:31:00+05:30", "recording_finished_at": "2026-11-10T12:24:03+05:30",
                "finished_at": "2026-11-10T12:30:00+05:30" })),
            job(serde_json::json!({ "id": "new", "output_path": "/m/a.md", "created_at": "2026-11-10T15:00:00+05:30",
                "recording_started_at": "2026-11-10T11:31:00+05:30", "recording_finished_at": "2026-11-10T12:24:03+05:30",
                "finished_at": "2026-11-10T15:00:41+05:30" })),
            job(serde_json::json!({ "id": "other", "output_path": "/m/b.md", "finished_at": "2026-11-10T09:00:00+05:30" })),
        ];
        let t = newest_for(&all, "/m/a.md").unwrap();
        assert_eq!(t.recording_started_at.as_deref(), Some("2026-11-10T11:31:00+05:30"));
        assert_eq!(t.recording_finished_at.as_deref(), Some("2026-11-10T12:24:03+05:30"));
        assert_eq!(t.ready_at.as_deref(), Some("2026-11-10T15:00:41+05:30"));
        assert_eq!(newest_for(&all, "/m/c.md"), None);
    }

    #[test]
    fn job_times_leave_out_what_the_record_does_not_have() {
        // An old record with no recording times, and a failed job: no ready time.
        let old = job(serde_json::json!({ "output_path": "/m/a.md", "finished_at": "2026-11-10T12:30:00+05:30" }));
        assert_eq!(JobTimes::of(&old).recording_started_at, None);
        assert_eq!(JobTimes::of(&old).recording_finished_at, None);
        let failed = job(serde_json::json!({ "state": "failed", "finished_at": "2026-11-10T12:30:00+05:30",
            "recording_finished_at": "2026-11-10T12:24:03+05:30" }));
        assert_eq!(JobTimes::of(&failed).ready_at, None);
        assert_eq!(JobTimes::of(&failed).recording_finished_at.as_deref(), Some("2026-11-10T12:24:03+05:30"));
        let active = job(serde_json::json!({ "state": "transcribing", "started_at": "2026-11-10T12:24:10+05:30" }));
        assert_eq!(JobTimes::of(&active), JobTimes::default());
    }

    #[test]
    fn the_notes_guard_accepts_only_what_core_wrote() {
        assert!(core_wrote_these_notes("- a\n- b\n", Some("a\nb")));
        assert!(core_wrote_these_notes("", Some("a")));
        assert!(core_wrote_these_notes("", None));
        assert!(!core_wrote_these_notes("- a\n- edited in Obsidian", Some("a")));
        assert!(!core_wrote_these_notes("- something", None));
    }
}
