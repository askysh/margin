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
            let result = summarize_or_ingest(
                || crate::commands::ops::summarize_meeting_with(path.clone(), true, fallback),
                || is_running(&path),
                || ingest_without_summary(std::path::Path::new(&path)),
            );
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

pub const ALREADY_WRITING: &str = "Margin is already writing this meeting's summary.";

/// The first summary of a new meeting also puts it in the knowledge wiki. When it fails, the
/// meeting goes in without a summary: the worker left the wiki to this step (D70). When a
/// Summarize click got there first, that run does not touch the wiki, so this waits for it
/// and then ingests the meeting with whatever summary it wrote.
fn summarize_or_ingest(
    summarize: impl FnOnce() -> Result<(), String>,
    still_writing: impl Fn() -> bool,
    ingest: impl FnOnce(),
) -> Result<(), String> {
    let result = summarize();
    match &result {
        Err(error) if error == ALREADY_WRITING => {
            let waited_until = std::time::Instant::now() + std::time::Duration::from_secs(30 * 60);
            while still_writing() && std::time::Instant::now() < waited_until {
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            ingest();
            Ok(())
        }
        Err(_) => {
            ingest();
            result
        }
        Ok(()) => result,
    }
}

/// With a summary engine set, the worker leaves the knowledge wiki to the app's summary step,
/// so a meeting gets one wiki log entry, not two (D70).
pub fn wiki_waits_for_summary(cfg: &minutes_core::config::Config) -> bool {
    cfg.summarization.engine != "none"
}

/// Puts a meeting whose first summary was not written by this step (it failed, a Summarize
/// click wrote it, or the meeting has no speech) in the knowledge wiki from the file as it is
/// now, with or without a summary, as the worker does when summaries are off. Does nothing
/// when the worker already ingested it.
pub fn ingest_without_summary(path: &std::path::Path) {
    let cfg = crate::corecfg::load();
    if !wiki_waits_for_summary(&cfg) || !cfg.knowledge.enabled {
        return;
    }
    let report = minutes_core::derived::refresh_derived_views(path, &cfg, &minutes_core::derived::RefreshOptions { ingest_knowledge: true });
    for warning in report.warnings {
        tracing::warn!(%warning, "knowledge wiki update without a summary");
    }
}

fn file_title(path: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let (fm, _) = minutes_core::markdown::split_frontmatter(&content);
    serde_yaml::from_str::<minutes_core::markdown::Frontmatter>(fm).ok().map(|f| f.title)
}

/// The words of a text, lowercase, without punctuation.
fn title_words(text: &str) -> Vec<String> {
    text.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(String::from).collect()
}

/// Only the automatic first summary of a newly finished meeting renames it, and only when
/// its title is the machine's (D92).
fn wants_new_title(after_processing: bool, fm: &minutes_core::markdown::Frontmatter, body: &str, fallback: Option<&str>) -> bool {
    after_processing && is_auto_title(fm, body, fallback)
}

/// The title came from the machine and the user has not touched it (D92). Margin does not
/// record renames, so this reads the file:
/// - the title is the fallback that Stop left (`fallback`, set for a meeting started with no
///   title);
/// - it is core's cut title that ends with "...", which no one types;
/// - it is two or more words that are the opening words of the transcript, as core titles an
///   untitled meeting. A calendar meeting keeps its event title, and a one-word title is
///   never taken for the machine's, since "Standup" can be what the user typed.
fn is_auto_title(fm: &minutes_core::markdown::Frontmatter, body: &str, fallback: Option<&str>) -> bool {
    if fallback == Some(fm.title.as_str()) {
        return true;
    }
    if fm.calendar_event.is_some() {
        return false;
    }
    if crate::library::is_core_cut_title(&fm.title) {
        return true;
    }
    let title = title_words(&fm.title);
    if title.len() < 2 {
        return false;
    }
    let opening: Vec<String> = crate::library::parse_transcript(body).iter().take(3).flat_map(|t| title_words(&t.text)).take(40).collect();
    (0..=3).any(|skip| opening.get(skip..).is_some_and(|rest| rest.starts_with(&title)))
}

/// Names a meeting whose title is the machine's from its new summary, with core's title
/// prompt (D59, D92). The worker runs with summaries off (D40), so core's own title step had
/// no summary and kept its fallback, the first words of the transcript. Only the frontmatter
/// title changes. The file keeps its name, so links and the job record still find it, as with
/// a title typed in the meeting view. A title changed while the model ran is kept.
fn title_from_summary(path: &std::path::Path, after_processing: bool, fallback: Option<&str>) {
    let refined = (|| -> Result<Option<(String, String)>, String> {
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let (fm_text, body) = minutes_core::markdown::split_frontmatter(&content);
        let fm: minutes_core::markdown::Frontmatter = serde_yaml::from_str(fm_text).map_err(|e| e.to_string())?;
        if !wants_new_title(after_processing, &fm, body, fallback) {
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
        let mut cfg = crate::corecfg::load();
        let _forwarder = crate::ollama_proxy::fast_ollama(&mut cfg);
        // The title is in the language the transcript was written in (D83).
        let fallback = minutes_core::transcribe::ModelFallback::recorded_in(&fm.processing_warnings);
        let cfg = minutes_core::summarize::config_for_transcript(&cfg, fallback.as_ref());
        let answer = minutes_core::summarize::refine_title(&text, &summary, &fm.entities, &cfg).map_err(|e| e.to_string())?;
        Ok(clean_title(&answer.title).map(|title| (fm.title, title)))
    })();
    match refined {
        Ok(Some((before, title))) => {
            // The user may have renamed it while the model ran: check again.
            if file_title(path).as_deref() != Some(before.as_str()) {
                return;
            }
            match crate::library::set_title(path, &title) {
                Ok(()) => tracing::info!("named a meeting from its summary"),
                Err(error) => tracing::warn!(%error, "could not set the title from the summary"),
            }
        }
        Ok(None) => tracing::info!("no new title from the summary; the meeting keeps its title"),
        Err(error) => tracing::warn!(%error, "title from the summary failed"),
    }
}

/// Retitle and clean a summary core just wrote: the model's preamble goes (D94), then, after
/// the first summary of a newly finished meeting, an auto title is replaced (D92). Called
/// with the summary still marked as running, so the view reads the file once, when the run
/// ends. Failures are logged; the summary stays.
pub fn finish_summary(path: &std::path::Path, after_processing: bool, fallback: Option<&str>) {
    match clean_summary_in_file(path) {
        Ok(true) => tracing::info!("removed a preamble from the summary"),
        Ok(false) => {}
        Err(error) => tracing::warn!(%error, "could not clean the summary"),
    }
    title_from_summary(path, after_processing, fallback);
}

const PREAMBLE_OPENERS: &[&str] = &[
    "based on the transcript", "based on the meeting", "based on the provided", "based on the conversation", "based on the audio",
    "here is", "here's", "here are", "below is", "below are", "the following is", "the following summary",
    "sure", "certainly", "of course", "absolutely", "okay", "i've summarized", "i have summarized", "this is a summary",
];
const PREAMBLE_TOPICS: &[&str] = &["summar", "overview", "takeaway", "notes", "recap", "breakdown"];
const POSTAMBLE_OPENERS: &[&str] = &["let me know", "i hope this", "feel free to", "if you'd like", "if you would like", "would you like me", "i can also"];

fn plain_lower(line: &str) -> String {
    line.trim().trim_start_matches(['*', '#', '>', '_', ' ']).to_lowercase()
}

/// A line that talks about the summary instead of being part of it. Short, begins like an
/// assistant's reply, and names what it introduces.
fn is_preamble(line: &str) -> bool {
    let l = plain_lower(line);
    l.chars().count() <= 400 && PREAMBLE_OPENERS.iter().any(|o| l.starts_with(o)) && PREAMBLE_TOPICS.iter().any(|t| l.contains(t))
}

fn is_postamble(line: &str) -> bool {
    let l = plain_lower(line);
    l.chars().count() <= 300 && POSTAMBLE_OPENERS.iter().any(|o| l.starts_with(o))
}

/// The meeting file with the model's lead-in line and closing offer removed from `## Summary`
/// (D94: "Based on the transcript provided, here is a structured summary of..."). None when
/// there was nothing to remove. Only the first and last non-empty lines of the section are
/// looked at, and only when they read as chatter.
pub fn strip_summary_chatter(content: &str) -> Option<String> {
    let lines: Vec<&str> = content.split_inclusive('\n').collect();
    let heading = lines.iter().position(|l| l.trim_end() == "## Summary")?;
    let end = lines[heading + 1..].iter().position(|l| l.starts_with("## ")).map_or(lines.len(), |i| heading + 1 + i);
    let mut keep: Vec<bool> = vec![true; lines.len()];
    let first = (heading + 1..end).find(|&i| !lines[i].trim().is_empty())?;
    let mut changed = false;
    if is_preamble(lines[first]) {
        keep[first] = false;
        changed = true;
        for i in first + 1..end {
            if !lines[i].trim().is_empty() {
                break;
            }
            keep[i] = false;
        }
    }
    if let Some(last) = (heading + 1..end).rev().find(|&i| keep[i] && !lines[i].trim().is_empty()) {
        if last > first && is_postamble(lines[last]) {
            keep[last] = false;
            changed = true;
            for i in (first + 1..last).rev().take_while(|&i| lines[i].trim().is_empty()) {
                keep[i] = false;
            }
        }
    }
    changed.then(|| lines.iter().zip(&keep).filter(|(_, k)| **k).map(|(l, _)| *l).collect())
}

fn clean_summary_in_file(path: &std::path::Path) -> Result<bool, String> {
    crate::fsutil::with_file_lock(path, || {
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let Some(clean) = strip_summary_chatter(&content) else { return Ok(false) };
        crate::fsutil::write_atomic(path, clean.as_bytes(), None).map_err(|e| e.to_string())?;
        Ok(true)
    })
}

/// The model's answer as a title, or None when it is not one: one line, no label or
/// quotes, 2 to 12 words, at most 80 characters, and not a sentence about the task.
fn clean_title(answer: &str) -> Option<String> {
    let line = answer.lines().map(str::trim).find(|l| !l.is_empty())?;
    let line = ["Title:", "title:", "Meeting title:"].iter().fold(line, |l, p| l.strip_prefix(p).unwrap_or(l)).trim();
    let wrap = |c: char| matches!(c, '"' | '\'' | '`' | '*' | '#' | '-') || c.is_whitespace();
    let title = line.trim_start_matches(wrap).trim_end_matches(|c: char| wrap(c) || matches!(c, '.' | '…' | ':' | ';'));
    let words = title.split_whitespace().count();
    let lower = title.to_lowercase();
    let chatty = ["here", "i ", "i'", "sure", "certainly", "based on", "the title"].iter().any(|p| lower.starts_with(p));
    ((2..=12).contains(&words) && title.chars().count() <= 80 && !chatty).then(|| title.to_string())
}

/// Node.js for Margin Desk. An app opened from Finder gets a short PATH, and Homebrew's
/// versioned `node@22` or nvm is often added only in `~/.zshrc`, which a login shell does
/// not read. So look on PATH, then in the usual install places, newest version first, then
/// ask an interactive login shell as the last resort.
pub fn find_node(home: &std::path::Path) -> Option<PathBuf> {
    if let Ok(found) = which::which("node") {
        return Some(found);
    }
    let mut candidates: Vec<PathBuf> = vec![PathBuf::from("/opt/homebrew/bin/node"), PathBuf::from("/usr/local/bin/node")];
    for root in [PathBuf::from("/opt/homebrew/opt"), PathBuf::from("/usr/local/opt"), home.join(".nvm/versions/node")] {
        let mut versions: Vec<PathBuf> = std::fs::read_dir(&root).into_iter().flatten().flatten()
            .map(|e| e.path())
            .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("node") || n.starts_with('v')))
            .collect();
        versions.sort_by_key(|p| std::cmp::Reverse(node_version(p)));
        candidates.extend(versions.into_iter().map(|p| p.join("bin/node")));
    }
    if let Some(found) = candidates.into_iter().find(|p| is_executable(p)) {
        return Some(found);
    }
    let out = std::process::Command::new("/bin/zsh").args(["-lic", "command -v node"]).stdin(std::process::Stdio::null()).output().ok()?;
    let path = String::from_utf8(out.stdout).ok()?.lines().map(str::trim).rfind(|l| l.starts_with('/'))?.to_string();
    let path = PathBuf::from(path);
    is_executable(&path).then_some(path)
}

/// The major version in a folder name such as `node@22` or `v20.11.0`; 0 when there is none.
fn node_version(dir: &std::path::Path) -> u32 {
    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name.trim_start_matches("node").trim_start_matches(['@', 'v']).split('.').next().and_then(|n| n.parse().ok()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::clean_title;

    #[test]
    fn node_versions_sort_newest_first() {
        assert_eq!(super::node_version(std::path::Path::new("/opt/homebrew/opt/node@22")), 22);
        assert_eq!(super::node_version(std::path::Path::new("/x/.nvm/versions/node/v20.11.0")), 20);
        assert_eq!(super::node_version(std::path::Path::new("/opt/homebrew/opt/node")), 0);
    }

    const AMI: &str = "---\ntitle: AMI TS3003a\n---\n\n## Summary\n\nBased on the transcript provided, here is a structured summary of the project discussion and key takeaways from the meeting about designing a new remote control product.\n\n### **Project Overview**\n*   **Product:** A multi-function remote control.\n\n## Transcript\n\n[A 0:00] hi\n";

    #[test]
    fn a_model_preamble_leaves_the_summary() {
        let out = super::strip_summary_chatter(AMI).unwrap();
        assert!(out.contains("## Summary\n\n### **Project Overview**"), "{out}");
        assert!(!out.contains("Based on the transcript"));
        assert!(out.contains("## Transcript\n\n[A 0:00] hi"));
    }

    #[test]
    fn other_lead_ins_and_closing_offers_go() {
        for lead in ["Here is a summary of the meeting:", "Sure! Here's the meeting summary.", "**Here is a structured summary**", "Certainly. Below is an overview of the call."] {
            let md = format!("---\ntitle: T\n---\n\n## Summary\n\n{lead}\n\n- Point one\n\nLet me know if you want more detail.\n\n## Action Items\n\n- x\n");
            let out = super::strip_summary_chatter(&md).unwrap_or_else(|| panic!("{lead}"));
            assert_eq!(out, "---\ntitle: T\n---\n\n## Summary\n\n- Point one\n\n## Action Items\n\n- x\n", "{lead}");
        }
    }

    #[test]
    fn a_real_summary_is_not_touched() {
        let md = "---\ntitle: T\n---\n\n## Summary\n\n- Pricing is settled.\n- Here is what we decided: ship Friday.\n\n## Transcript\n\n[A 0:00] hi\n";
        assert_eq!(super::strip_summary_chatter(md), None);
        let prose = "---\ntitle: T\n---\n\n## Summary\n\nThe team reviewed the Q3 summary slides and agreed on the launch.\n";
        assert_eq!(super::strip_summary_chatter(prose), None);
        assert_eq!(super::strip_summary_chatter("---\ntitle: T\n---\n\nno summary section\n"), None);
    }

    fn fm(title: &str, calendar: bool) -> minutes_core::markdown::Frontmatter {
        let cal = if calendar { "calendar_event: Standup 10:00\n" } else { "" };
        serde_yaml::from_str(&format!("title: \"{title}\"\ntype: meeting\ndate: 2026-11-26\nduration: 1m\n{cal}")).unwrap()
    }

    const BODY: &str = "\n## Transcript\n\n[SPEAKER_1 0:00] I have kept people in love, I am giving you a split in cash.\n[SPEAKER_1 0:09] And then we go.\n";

    #[test]
    fn a_title_made_from_the_opening_words_is_the_machines() {
        assert!(super::is_auto_title(&fm("I Have Kept People in Love", false), BODY, None));
        assert!(super::is_auto_title(&fm("Have Kept People", false), BODY, None));
        assert!(super::is_auto_title(&fm("Stop Here", false), BODY, Some("Stop Here")));
        assert!(super::is_auto_title(&fm("I Have Kept People in Love, I Am Giving You a Split in Ca...", false), "", None));
    }

    #[test]
    fn a_title_the_user_may_have_typed_is_not_replaced() {
        assert!(!super::is_auto_title(&fm("Q3 Pricing Review", false), BODY, None));
        assert!(!super::is_auto_title(&fm("I", false), BODY, None));
        assert!(!super::is_auto_title(&fm("I Have Kept People in Love", true), BODY, None));
        assert!(!super::is_auto_title(&fm("Wait for it...", false), BODY, None));
    }

    #[test]
    fn a_summarize_click_never_renames_the_meeting() {
        let typed = fm("I Have Kept People in Love", false);
        assert!(!super::wants_new_title(false, &typed, BODY, None), "a title typed as the opening words stays");
        assert!(!super::wants_new_title(false, &fm("Stop Here", false), BODY, Some("Stop Here")));
        assert!(super::wants_new_title(true, &typed, BODY, None), "the first summary after processing names it");
    }

    #[test]
    fn a_failed_first_summary_still_reaches_the_wiki() {
        let ingested = std::cell::Cell::new(0);
        let failed = super::summarize_or_ingest(|| Err("Ollama isn't running.".into()), || false, || ingested.set(ingested.get() + 1));
        assert!(failed.is_err());
        assert_eq!(ingested.get(), 1, "the meeting goes in without a summary");
        assert!(super::summarize_or_ingest(|| Ok(()), || false, || ingested.set(ingested.get() + 1)).is_ok());
        assert_eq!(ingested.get(), 1, "a summary that ran did the ingest itself");
        let polls = std::cell::Cell::new(0);
        let busy = super::summarize_or_ingest(
            || Err(super::ALREADY_WRITING.into()),
            || { polls.set(polls.get() + 1); polls.get() < 3 },
            || ingested.set(ingested.get() + 1),
        );
        assert!(busy.is_ok(), "a Summarize click in progress is not a failure");
        assert_eq!(polls.get(), 3, "waits for the click's summary to finish");
        assert_eq!(ingested.get(), 2, "then ingests once, with that summary");

        let mut cfg = minutes_core::config::Config::default();
        cfg.summarization.engine = "ollama".into();
        assert!(super::wiki_waits_for_summary(&cfg));
        cfg.summarization.engine = "none".into();
        assert!(!super::wiki_waits_for_summary(&cfg), "the worker ingests when summaries are off");
    }

    #[test]
    fn a_title_answer_never_ends_in_dots() {
        assert_eq!(clean_title("Warranty Flow for Pump Returns...").as_deref(), Some("Warranty Flow for Pump Returns"));
        assert_eq!(clean_title("Warranty Flow for Pump Returns…").as_deref(), Some("Warranty Flow for Pump Returns"));
    }

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
