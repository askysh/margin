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
    if minutes_core::jobs::worker_active() {
        return Err("A meeting is still being processed. Try again when it finishes.".into());
    }
    let mut report = ResetReport { meetings_trashed: 0, models_deleted: false, voices_forgotten: 0 };

    // Meetings first: this needs the current config to know where they are.
    if opts.delete_meetings {
        let root = library::output_dir();
        let mut items: Vec<PathBuf> = Vec::new();
        for meeting in library::list() {
            let md = PathBuf::from(&meeting.path);
            items.extend(minutes_core::capture::meeting_audio_artifact_paths(&md).into_iter().filter(|p| p.exists()));
            items.push(md);
            report.meetings_trashed += 1;
        }
        for dir in ["archive", "failed-captures"] {
            let p = root.join(dir);
            if p.exists() {
                items.push(p);
            }
        }
        if !items.is_empty() {
            trash::delete_all(&items).map_err(|e| format!("Could not move meetings to the Trash: {e}"))?;
        }
    }

    if opts.forget_voices {
        for v in speakers::list_voices() {
            if speakers::forget_voice(&v.slug).is_ok() {
                report.voices_forgotten += 1;
            }
        }
    }

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
    paths::install_env();
    Ok(report)
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
