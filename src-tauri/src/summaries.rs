//! Detects the optional summary backends: the user's own `claude` CLI, or a local Ollama.
//! Both are optional; with neither, meetings are saved with transcript and notes only.

use serde::Serialize;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Serialize)]
pub struct SummaryBackends {
    pub claude_path: Option<String>,
    pub ollama_running: bool,
    pub ollama_models: Vec<String>,
}

/// Finder-launched apps get a minimal PATH, so look in the usual install places and fall
/// back to asking a login shell.
pub fn find_claude() -> Option<String> {
    let home = crate::paths::home();
    let candidates: Vec<PathBuf> = vec![
        home.join(".local/bin/claude"),
        home.join(".claude/local/claude"),
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
    ];
    if let Some(found) = candidates.into_iter().find(|p| is_executable(p)) {
        return Some(found.display().to_string());
    }
    if let Ok(found) = which::which("claude") {
        return Some(found.display().to_string());
    }
    let out = std::process::Command::new("/bin/zsh")
        .args(["-lc", "command -v claude"])
        .output()
        .ok()?;
    let path = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!path.is_empty() && is_executable(std::path::Path::new(&path))).then_some(path)
}

pub(crate) fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

pub fn ollama_models(base_url: &str) -> Option<Vec<String>> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_millis(800)))
        .build()
        .new_agent();
    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    let body: serde_json::Value = agent.get(&url).call().ok()?.body_mut().read_json().ok()?;
    Some(
        body["models"]
            .as_array()
            .map(|models| {
                models
                    .iter()
                    .filter_map(|m| m["name"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
    )
}

pub fn detect(ollama_url: &str) -> SummaryBackends {
    let models = ollama_models(ollama_url);
    SummaryBackends {
        claude_path: find_claude(),
        ollama_running: models.is_some(),
        ollama_models: models.unwrap_or_default(),
    }
}

/// Meetings whose summary is being written now.
static RUNNING: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

pub fn is_running(path: &str) -> bool {
    RUNNING.lock().unwrap().iter().any(|p| p == path)
}

/// Marks a meeting as summarizing for as long as the guard lives. `None` when a summary
/// for it is already being written.
pub fn start(path: &str) -> Option<RunningGuard> {
    let mut running = RUNNING.lock().unwrap();
    if running.iter().any(|p| p == path) {
        return None;
    }
    running.push(path.to_string());
    drop(running);
    crate::events::emit("summary-status", serde_json::json!({ "path": path, "running": true }));
    Some(RunningGuard(path.to_string()))
}

pub struct RunningGuard(String);

impl Drop for RunningGuard {
    /// Every run ends with `running: false`, also a run that failed or was refused, so the
    /// meeting view never keeps "Summarizing…" after it. The background run sends one more
    /// event with the error.
    fn drop(&mut self) {
        let _notes = NOTES.lock().unwrap_or_else(|e| e.into_inner());
        apply_notes_draft(&notes_draft_path(&self.0));
        RUNNING.lock().unwrap().retain(|p| p != &self.0);
        crate::events::emit("summary-status", serde_json::json!({ "path": self.0, "running": false }));
    }
}

/// Held while notes are saved and while a summary run ends, so a save never goes into a
/// draft after the draft was moved into the file.
static NOTES: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Notes typed while a summary is written. Core's rewrite refuses to replace a file that
/// changed since it read it, and the model can take minutes. So the notes wait in a draft
/// beside the job drafts (D43), not behind the file lock, and go into the file when the
/// run ends. A Quit in between keeps the draft, and the next launch puts it in the file.
#[derive(serde::Serialize, serde::Deserialize)]
struct NotesDraft {
    path: String,
    notes: String,
}

fn notes_draft_path(path: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    crate::paths::drafts_dir().join(format!("summary-{:016x}.json", h.finish()))
}

/// Saves the notes of `path`: into the draft while its summary is written, else with
/// `write`.
pub fn save_notes(path: &str, notes: &str, write: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    let _notes = NOTES.lock().unwrap_or_else(|e| e.into_inner());
    if !is_running(path) {
        return write();
    }
    let draft = NotesDraft { path: path.to_string(), notes: notes.to_string() };
    let json = serde_json::to_vec(&draft).map_err(|e| e.to_string())?;
    crate::fsutil::write_atomic(&notes_draft_path(path), &json, Some(0o600)).map_err(|e| e.to_string())
}

/// The notes typed during the summary of `path`, while they wait in the draft.
pub fn notes_draft(path: &str) -> Option<String> {
    let bytes = std::fs::read(notes_draft_path(path)).ok()?;
    serde_json::from_slice::<NotesDraft>(&bytes).ok().map(|d| d.notes)
}

fn apply_notes_draft(draft: &std::path::Path) {
    let Ok(bytes) = std::fs::read(draft) else { return };
    let Ok(d) = serde_json::from_slice::<NotesDraft>(&bytes) else {
        let _ = std::fs::remove_file(draft);
        return;
    };
    let path = std::path::Path::new(&d.path);
    if !path.exists() {
        tracing::info!("the meeting of a notes draft is gone; dropping the draft");
        let _ = std::fs::remove_file(draft);
        return;
    }
    match crate::library::replace_notes(path, &d.notes) {
        Ok(()) => {
            let _ = std::fs::remove_file(draft);
        }
        Err(error) => tracing::warn!(%error, "could not put the notes typed during a summary in the file"),
    }
}

/// At launch: notes typed during a summary that a Quit ended go into their files.
pub fn recover_notes_drafts() {
    let _notes = NOTES.lock().unwrap_or_else(|e| e.into_inner());
    let Ok(entries) = std::fs::read_dir(crate::paths::drafts_dir()) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("summary-") && name.ends_with(".json") {
            apply_notes_draft(&entry.path());
        }
    }
}

/// After a meeting is ready: write its summary, if summaries are on (D40).
pub fn summarize_in_background(path: String) {
    if crate::corecfg::load().summarization.engine == "none" {
        return;
    }
    std::thread::Builder::new()
        .name("margin-summary".into())
        .spawn(move || {
            let result = crate::commands::ops::summarize_meeting(path.clone());
            if let Err(error) = &result {
                tracing::warn!(%error, path, "summary after processing failed");
            }
            crate::events::emit(
                "summary-status",
                serde_json::json!({ "path": path, "running": false, "error": result.err() }),
            );
            crate::events::refresh_tray();
        })
        .expect("spawn summary");
}
