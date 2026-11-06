//! Reset: wipe Margin's settings and state, optionally its meetings, models and saved
//! voices, then run onboarding again. It never touches the upstream Minutes app's
//! `~/.config/minutes` or `~/.minutes` (apart from the shared voices.db, and only when
//! asked).
//!
//! Meetings are moved to the Trash, never unlinked. Only files Margin recognises as meetings
//! are moved, so a meetings folder that is also an Obsidian vault keeps its other notes.

use crate::{library, paths, recorder::Recorder, speakers};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct ResetOptions {
    pub delete_meetings: bool,
    pub delete_models: bool,
    pub forget_voices: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResetPreview {
    pub output_dir: String,
    pub meeting_count: usize,
    pub models_bytes: u64,
    pub voice_count: usize,
    pub upstream_app_installed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResetReport {
    pub meetings_trashed: usize,
    pub models_deleted: bool,
    pub voices_forgotten: usize,
}

pub fn preview() -> ResetPreview {
    ResetPreview {
        output_dir: library::output_dir().display().to_string(),
        meeting_count: library::list().len(),
        models_bytes: dir_size(&paths::models_dir()),
        voice_count: speakers::list_voices().len(),
        upstream_app_installed: PathBuf::from("/Applications/Minutes.app").exists(),
    }
}

pub fn run(recorder: &Recorder, opts: &ResetOptions) -> Result<ResetReport, String> {
    if recorder.is_recording() {
        return Err("Stop the recording before resetting.".into());
    }
    if minutes_core::jobs::worker_active() || !minutes_core::jobs::active_jobs().is_empty() {
        return Err("A meeting is still being processed. Try again when it finishes.".into());
    }
    let mut report = ResetReport { meetings_trashed: 0, models_deleted: false, voices_forgotten: 0 };
    let cfg = crate::corecfg::load();
    let root = cfg.output_dir.clone();

    // Audio of failed jobs, or a capture left by a crash, lives in state/. It is meeting
    // audio, so it follows the "keep my meetings" choice instead of being wiped with state.
    crate::recorder::recover_leftover_capture(&cfg);
    let orphaned_audio = job_audio();
    if !opts.delete_meetings && !orphaned_audio.is_empty() {
        let dir = root.join("failed-captures");
        std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't save unprocessed audio: {e}"))?;
        for src in &orphaned_audio {
            let dest = dir.join(src.file_name().unwrap_or_default());
            if std::fs::rename(src, &dest).is_err() {
                std::fs::copy(src, &dest).map_err(|e| format!("Couldn't save unprocessed audio: {e}"))?;
            }
        }
    }

    if opts.delete_meetings {
        let mut items: Vec<PathBuf> = Vec::new();
        let add_meeting = |md: PathBuf, items: &mut Vec<PathBuf>| {
            items.extend(minutes_core::capture::meeting_audio_artifact_paths(&md).into_iter().filter(|p| p.exists()));
            items.push(md);
        };
        for meeting in library::list() {
            add_meeting(PathBuf::from(&meeting.path), &mut items);
            report.meetings_trashed += 1;
        }
        // archive/ may be the user's own folder (a vault root): only take meeting files.
        for md in library::meeting_files_in(&root.join("archive")) {
            add_meeting(md, &mut items);
        }
        for f in files_with_ext(&root.join("failed-captures"), "wav") {
            items.push(f);
        }
        items.extend(orphaned_audio.iter().cloned());
        items.sort();
        items.dedup();
        if !items.is_empty() {
            trash::delete_all(&items).map_err(|e| format!("Couldn't move meetings to the Trash: {e}"))?;
        }
    }

    if opts.forget_voices {
        for v in speakers::list_voices() {
            if speakers::forget_voice(&v.slug).is_ok() {
                report.voices_forgotten += 1;
            }
        }
    }

    // Chats live in state/chat. Stop any answer in progress so it isn't saved afterwards.
    crate::chat::forget_all();

    // State: everything under state/, except models unless asked.
    let state = paths::state_dir();
    if let Ok(entries) = std::fs::read_dir(&state) {
        for entry in entries.flatten() {
            if entry.file_name() == "models" && !opts.delete_models {
                continue;
            }
            let p = entry.path();
            let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
        }
    }
    report.models_deleted = opts.delete_models;

    for file in [paths::config_path(), paths::prefs_path()] {
        let _ = std::fs::remove_file(file);
    }
    let _ = std::fs::remove_dir_all(paths::drafts_dir());
    paths::ensure_dirs();
    crate::events::refresh_tray();
    Ok(report)
}

/// Audio files under state/jobs (recursively): captures whose processing failed.
fn job_audio() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![minutes_core::jobs::jobs_dir()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "wav") {
                out.push(p);
            }
        }
    }
    out
}

fn files_with_ext(dir: &std::path::Path, ext: &str) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|it| it.flatten().map(|e| e.path()).filter(|p| p.is_file() && p.extension().is_some_and(|x| x == ext)).collect())
        .unwrap_or_default()
}

fn dir_size(dir: &std::path::Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    entries
        .flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => dir_size(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}
