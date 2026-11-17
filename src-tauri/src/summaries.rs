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

/// Windows: the usual install places, then the PATH. No shell runs. The native installer
/// puts `claude.exe` in `.local\bin`, and npm puts a `claude.cmd` shim in its own folder. A
/// real program comes before a shim, because a shim starts `node` through `cmd.exe`.
#[cfg(windows)]
pub fn find_claude() -> Option<String> {
    let mut candidates: Vec<PathBuf> = vec![crate::paths::home().join(".local").join("bin").join("claude.exe")];
    for dir in [dirs::data_dir(), dirs::data_local_dir()].into_iter().flatten() {
        candidates.push(dir.join("npm").join("claude.cmd"));
    }
    if let Some(found) = candidates.into_iter().find(|p| is_executable(p)) {
        return Some(found.display().to_string());
    }
    // `which` adds the extensions in PATHEXT, so it finds claude.exe and claude.cmd.
    which::which("claude").ok().filter(|p| is_executable(p)).map(|p| p.display().to_string())
}

/// Finder-launched apps get a minimal PATH, so look in the usual install places and fall
/// back to asking a login shell.
#[cfg(not(windows))]
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

/// Windows has no executable bit. The file extension says what can run.
#[cfg(windows)]
pub(crate) fn is_executable(path: &std::path::Path) -> bool {
    let runs = path.extension().and_then(|e| e.to_str()).is_some_and(|e| ["exe", "cmd", "bat", "com"].iter().any(|x| e.eq_ignore_ascii_case(x)));
    runs && path.is_file()
}

#[cfg(unix)]
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

/// After a meeting is ready: write its summary, if summaries are on (D40). `untitled`: the
/// meeting had no title at Stop, so it has core's fallback title. Then the summary engine
/// names it too (D59).
pub fn summarize_in_background(path: String, untitled: bool) {
    if crate::corecfg::load().summarization.engine == "none" {
        return;
    }
    std::thread::Builder::new()
        .name("margin-summary".into())
        .spawn(move || {
            let fallback = untitled.then(|| file_title(std::path::Path::new(&path))).flatten();
            let result = crate::commands::ops::summarize_meeting(path.clone());
            if let Err(error) = &result {
                tracing::warn!(%error, path, "summary after processing failed");
            }
            if let (Ok(()), Some(fallback)) = (&result, fallback) {
                title_from_summary(std::path::Path::new(&path), &fallback);
            }
            crate::events::emit(
                "summary-status",
                serde_json::json!({ "path": path, "running": false, "error": result.err() }),
            );
            crate::events::refresh_tray();
        })
        .expect("spawn summary");
}

fn file_title(path: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let (fm, _) = minutes_core::markdown::split_frontmatter(&content);
    serde_yaml::from_str::<minutes_core::markdown::Frontmatter>(fm).ok().map(|f| f.title)
}

/// Names an untitled meeting from its new summary, with core's title prompt (D59). The
/// worker runs with summaries off (D40), so core's own title step had no summary and kept
/// its fallback, such as the first words of the transcript. Only the frontmatter title
/// changes. The file keeps its name, so links and the job record still find it, as with a
/// title typed in the meeting view. A title changed since Stop is kept.
fn title_from_summary(path: &std::path::Path, fallback: &str) {
    let refined = (|| -> Result<Option<String>, String> {
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let (fm_text, body) = minutes_core::markdown::split_frontmatter(&content);
        let fm: minutes_core::markdown::Frontmatter = serde_yaml::from_str(fm_text).map_err(|e| e.to_string())?;
        if fm.title != fallback {
            return Ok(None);
        }
        let Some(text) = crate::library::sections(body).into_iter().find(|s| s.heading == "Summary").map(|s| s.body) else {
            return Ok(None);
        };
        let summary = minutes_core::summarize::Summary {
            title: None,
            text: text.clone(),
            decisions: fm.decisions.iter().map(|d| d.text.clone()).collect(),
            action_items: fm.action_items.iter().map(|a| a.task.clone()).collect(),
            open_questions: vec![],
            commitments: vec![],
            key_points: vec![],
            participants: fm.attendees.clone(),
        };
        let cfg = crate::corecfg::load();
        let answer = minutes_core::summarize::refine_title(&text, &summary, &fm.entities, &cfg).map_err(|e| e.to_string())?;
        Ok(clean_title(&answer.title))
    })();
    match refined {
        Ok(Some(title)) => {
            // The user may have renamed it while the model ran: check again under the lock.
            if file_title(path).as_deref() != Some(fallback) {
                return;
            }
            match crate::library::set_title(path, &title) {
                // The run's last summary-status event, after this, makes the view read it.
                Ok(()) => tracing::info!("named an untitled meeting from its summary"),
                Err(error) => tracing::warn!(%error, "could not set the title from the summary"),
            }
        }
        Ok(None) => tracing::info!("no title from the summary; the meeting keeps its title"),
        Err(error) => tracing::warn!(%error, "title from the summary failed"),
    }
}

/// The model's answer as a title, or None when it is not one: one line, no label or
/// quotes, 2 to 12 words, at most 80 characters, and not a sentence about the task.
fn clean_title(answer: &str) -> Option<String> {
    let line = answer.lines().map(str::trim).find(|l| !l.is_empty())?;
    let line = ["Title:", "title:", "Meeting title:"].iter().fold(line, |l, p| l.strip_prefix(p).unwrap_or(l)).trim();
    let wrap = |c: char| matches!(c, '"' | '\'' | '`' | '*' | '#' | '-') || c.is_whitespace();
    let title = line.trim_start_matches(wrap).trim_end_matches(|c: char| wrap(c) || matches!(c, '.' | ':' | ';'));
    let words = title.split_whitespace().count();
    let lower = title.to_lowercase();
    let chatty = ["here", "i ", "i'", "sure", "certainly", "based on", "the title"].iter().any(|p| lower.starts_with(p));
    ((2..=12).contains(&words) && title.chars().count() <= 80 && !chatty).then(|| title.to_string())
}

#[cfg(test)]
mod tests {
    use super::clean_title;

    #[test]
    fn a_title_answer_is_cleaned_or_refused() {
        assert_eq!(clean_title("Title: \"Q3 Pricing Review\".\n").as_deref(), Some("Q3 Pricing Review"));
        assert_eq!(clean_title("**Hiring Plan for Platform Team**").as_deref(), Some("Hiring Plan for Platform Team"));
        assert_eq!(clean_title("Here is a concise title for the meeting"), None);
        assert_eq!(clean_title("- The team reviewed the launch plan.").as_deref(), Some("The team reviewed the launch plan"));
        assert_eq!(clean_title("Akash"), None);
        assert_eq!(clean_title(""), None);
        assert_eq!(clean_title(&"word ".repeat(20)), None);
    }
}
