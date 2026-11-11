//! The meeting library: plain Markdown files with YAML frontmatter in the user's folder.
//! We read them directly (no index), and edit only the `## Notes` section in place. The
//! frontmatter bytes and every other section are kept exactly (docs/research.md §1.5).

use crate::corecfg;
use minutes_core::markdown::split_frontmatter;
use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

const SKIP_DIRS: &[&str] = &["archive", "failed-captures", "processed", "failed", "daily", "dictations"];

#[derive(Debug, Clone, Serialize)]
pub struct MeetingSummary {
    pub path: String,
    pub title: String,
    pub date: Option<String>,
    pub duration: Option<String>,
    pub kind: String,
    pub status: Option<String>,
    pub snippet: String,
    /// Attendees from the frontmatter, for the library's people filter.
    pub people: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Section {
    pub heading: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Turn {
    pub speaker: Option<String>,
    pub ts: String,
    pub text: String,
    /// "MICROPHONE" | "SYSTEM" when core tagged a diarized line with its capture source.
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Speaker {
    pub label: String,
    /// Name from `speaker_map`, when the label is still a raw `SPEAKER_n`.
    pub name: Option<String>,
    pub turns: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct MeetingDetail {
    #[serde(flatten)]
    pub summary: MeetingSummary,
    pub attendees: Vec<String>,
    pub sections: Vec<Section>,
    pub notes: String,
    pub transcript: Vec<Turn>,
    pub speakers: Vec<Speaker>,
    pub in_obsidian_vault: bool,
    pub has_audio: bool,
    pub action_items: Vec<ActionItem>,
    /// Margin is writing the summary now (set by the command, not read from the file).
    pub summarizing: bool,
    /// Start, Stop and ready times from core's job record (set by the command, D58).
    pub times: Option<crate::processing::JobTimes>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ActionItem {
    pub task: String,
    pub assignee: Option<String>,
    pub due: Option<String>,
    pub done: bool,
}

pub fn output_dir() -> PathBuf {
    corecfg::load().output_dir
}

pub fn list() -> Vec<MeetingSummary> {
    let root = output_dir();
    let mut files = Vec::new();
    collect(&root, &root, &mut files);
    let mut out = cached_summaries(files);
    // Dates carry different UTC offsets, so compare instants, not strings.
    let instant = |m: &MeetingSummary| {
        m.date
            .as_deref()
            .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
            .map(|d| d.timestamp_millis())
            .unwrap_or(0)
    };
    out.sort_by(|a, b| instant(b).cmp(&instant(a)).then_with(|| b.path.cmp(&a.path)));
    out
}

/// List entries by file, kept while the file's size and modification time stay the same.
/// Files without meeting frontmatter are cached too (as `None`), so they aren't re-read.
type SummaryCache = HashMap<PathBuf, (Option<SystemTime>, u64, Option<MeetingSummary>)>;

fn cached_summaries(files: Vec<PathBuf>) -> Vec<MeetingSummary> {
    static CACHE: OnceLock<Mutex<SummaryCache>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    let mut fresh = SummaryCache::with_capacity(files.len());
    for path in files {
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        let (modified, len) = (meta.modified().ok(), meta.len());
        let summary = match cache.remove(&path) {
            Some((m, l, summary)) if m == modified && l == len && modified.is_some() => summary,
            _ => summarize_file(&path),
        };
        fresh.insert(path, (modified, len, summary));
    }
    let out = fresh.values().filter_map(|(_, _, s)| s.clone()).collect();
    // Files that are gone drop out here.
    *cache = fresh;
    out
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            // One level deep (memos/), never the archive or failure folders.
            if dir == root && !SKIP_DIRS.contains(&name.as_str()) {
                collect(root, &path, out);
            }
        } else if name.ends_with(".md") {
            out.push(path);
        }
    }
}

/// `.md` files directly in `dir` that carry meeting frontmatter (`type:` present).
pub fn meeting_files_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return vec![] };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .filter(|p| {
            std::fs::read_to_string(p)
                .map(|c| frontmatter(&c).0.get("type").is_some())
                .unwrap_or(false)
        })
        .collect()
}

fn frontmatter(content: &str) -> (serde_yaml::Value, &str) {
    let (fm, body) = split_frontmatter(content);
    let value = serde_yaml::from_str(fm).unwrap_or(serde_yaml::Value::Null);
    (value, body)
}

fn str_field(fm: &serde_yaml::Value, key: &str) -> Option<String> {
    match fm.get(key)? {
        serde_yaml::Value::String(s) => Some(s.clone()),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Enough of a meeting file for the library list: frontmatter plus the start of the body.
/// Summary and notes come before the transcript, which is usually most of the file.
fn read_head(path: &Path) -> Option<String> {
    use std::io::Read;
    const HEAD: usize = 16 * 1024;
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; HEAD];
    let mut n = 0;
    while n < HEAD {
        match file.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(_) => return None,
        }
    }
    buf.truncate(n);
    let text = match String::from_utf8(buf) {
        Ok(t) => t,
        // Cut inside a multi-byte character: drop the partial tail.
        Err(e) => {
            let valid = e.utf8_error().valid_up_to();
            let mut b = e.into_bytes();
            b.truncate(valid);
            String::from_utf8(b).ok()?
        }
    };
    // Frontmatter longer than the head (unusual): read it all.
    if n == HEAD && text.starts_with("---") && !text[3..].contains("\n---") {
        return std::fs::read_to_string(path).ok();
    }
    Some(text)
}

fn summarize_file(path: &Path) -> Option<MeetingSummary> {
    let content = read_head(path)?;
    let (fm, body) = frontmatter(&content);
    if fm.is_null() {
        return None;
    }
    let sections = sections(body);
    let snippet = sections
        .iter()
        .find(|s| s.heading == "Summary")
        .or_else(|| sections.iter().find(|s| s.heading == "Notes"))
        .or_else(|| sections.iter().find(|s| s.heading == "Transcript"))
        .map(|s| plain_snippet(&s.body))
        .unwrap_or_default();
    Some(MeetingSummary {
        path: path.display().to_string(),
        title: str_field(&fm, "title").unwrap_or_else(|| file_title(path)),
        date: str_field(&fm, "date"),
        duration: str_field(&fm, "duration"),
        kind: str_field(&fm, "type").unwrap_or_else(|| "meeting".into()),
        status: str_field(&fm, "status"),
        snippet,
        people: string_list(&fm, "attendees"),
    })
}

fn file_title(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

fn plain_snippet(md: &str) -> String {
    let re = turn_re();
    let text: Vec<String> = md
        .lines()
        .map(|l| {
            let l = l.trim();
            match re.captures(l) {
                Some(c) => c.get(3).map_or("", |m| m.as_str()).to_string(),
                None => l.trim_start_matches(['-', '*', '#', '>', ' ']).to_string(),
            }
        })
        .filter(|l| !l.is_empty())
        .take(3)
        .collect();
    let joined = text.join(" ");
    match joined.char_indices().nth(160) {
        Some((i, _)) => format!("{}…", &joined[..i]),
        None => joined,
    }
}

/// Split a body into `## ` sections. Text before the first heading is dropped (core writes
/// none). Headings inside fenced code blocks are not treated as section breaks.
pub fn sections(body: &str) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    let mut in_fence = false;
    for line in body.split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        }
        if !in_fence {
            if let Some(h) = line.strip_prefix("## ") {
                out.push(Section { heading: h.trim().to_string(), body: String::new() });
                continue;
            }
        }
        if let Some(last) = out.last_mut() {
            last.body.push_str(line);
        }
    }
    for s in &mut out {
        s.body = s.body.trim_matches('\n').to_string();
    }
    out
}

/// Core writes `[m:ss]` with no limit on the minutes, so a long meeting has `[100:00]`.
fn turn_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\[(?:(.+?) )?(\d+:\d{2}(?::\d{2})?)\]\s?(.*)$").unwrap())
}

fn parse_transcript(body: &str) -> Vec<Turn> {
    let re = turn_re();
    let mut turns: Vec<Turn> = Vec::new();
    for line in body.lines() {
        if let Some(c) = re.captures(line.trim()) {
            // Call recordings: `[SPEAKER_0 0:22] [MICROPHONE] text` carries the source too.
            let (source, text) = match c[3].strip_prefix("[MICROPHONE] ") {
                Some(t) => (Some("MICROPHONE"), t),
                None => match c[3].strip_prefix("[SYSTEM] ") {
                    Some(t) => (Some("SYSTEM"), t),
                    None => (None, &c[3]),
                },
            };
            turns.push(Turn {
                speaker: c.get(1).map(|m| m.as_str().to_string()),
                ts: c[2].to_string(),
                text: text.to_string(),
                source: source.map(String::from),
            });
        } else if let Some(last) = turns.last_mut() {
            if !line.trim().is_empty() {
                last.text.push(' ');
                last.text.push_str(line.trim());
            }
        }
    }
    turns
}

pub fn speaker_map(fm: &serde_yaml::Value) -> Vec<(String, String)> {
    fm.get("speaker_map")
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|e| {
                    Some((
                        e.get("speaker_label")?.as_str()?.to_string(),
                        e.get("name")?.as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn read(path: &Path) -> Result<MeetingDetail, String> {
    ensure_inside_output(path)?;
    let content = std::fs::read_to_string(path).map_err(|e| format!("Could not open meeting: {e}"))?;
    let summary = summarize_file(path).ok_or("This file has no meeting frontmatter.")?;
    let (fm, body) = frontmatter(&content);
    let all = sections(body);
    let notes = all.iter().find(|s| s.heading == "Notes").map(|s| s.body.clone()).unwrap_or_default();
    let transcript = all
        .iter()
        .find(|s| s.heading == "Transcript")
        .map(|s| parse_transcript(&s.body))
        .unwrap_or_default();
    let map = speaker_map(&fm);
    let mut speakers: Vec<Speaker> = Vec::new();
    for turn in &transcript {
        let Some(label) = &turn.speaker else { continue };
        match speakers.iter_mut().find(|s| &s.label == label) {
            Some(s) => s.turns += 1,
            None => speakers.push(Speaker {
                label: label.clone(),
                name: map.iter().find(|(l, n)| l == label && n != label).map(|(_, n)| n.clone()),
                turns: 1,
            }),
        }
    }
    let attendees = string_list(&fm, "attendees");
    let action_items = action_items(&fm, body);
    Ok(MeetingDetail {
        summary,
        attendees,
        sections: all
            .into_iter()
            .filter(|s| s.heading != "Notes" && s.heading != "Transcript")
            .collect(),
        notes,
        transcript,
        speakers,
        in_obsidian_vault: obsidian_vault_for(path).is_some(),
        has_audio: path.with_extension("wav").exists(),
        action_items,
        summarizing: false,
        times: None,
    })
}

fn string_list(fm: &serde_yaml::Value, key: &str) -> Vec<String> {
    fm.get(key)
        .and_then(|v| v.as_sequence())
        .map(|s| s.iter().filter_map(|a| a.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

/// The meeting's action items. Core's structured `action_items` frontmatter wins; a
/// summary without it (some engines write only the body) falls back to the checklist
/// under `## Action Items`. The order is the one `set_action_item` uses.
pub fn action_items(fm: &serde_yaml::Value, body: &str) -> Vec<ActionItem> {
    let structured: Vec<ActionItem> = fm
        .get("action_items")
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|e| {
                    Some(ActionItem {
                        task: e.get("task")?.as_str()?.trim().to_string(),
                        assignee: e
                            .get("assignee")
                            .and_then(|a| a.as_str())
                            .map(|a| a.trim().to_string())
                            .filter(|a| !a.is_empty() && !a.eq_ignore_ascii_case("unassigned")),
                        due: e.get("due").and_then(|d| d.as_str()).map(String::from).filter(|d| !d.trim().is_empty()),
                        done: e.get("status").and_then(|s| s.as_str()) == Some("done"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    if !structured.is_empty() {
        return structured;
    }
    checklist_lines(body).into_iter().map(|(_, line)| checklist_item(&line)).collect()
}

/// `(byte offset of the box character, line)` for each `- [ ]` / `- [x]` line under
/// `## Action Items`.
fn checklist_lines(body: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut in_section = false;
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        let t = line.trim_end();
        if let Some(h) = t.strip_prefix("## ") {
            in_section = h.trim() == "Action Items";
        } else if in_section && ["- [ ] ", "- [x] ", "- [X] "].iter().any(|p| t.starts_with(p)) {
            out.push((offset + 3, t.to_string()));
        }
        offset += line.len();
    }
    out
}

/// `- [ ] @Priya: send the deck` → task "send the deck", assignee "Priya".
fn checklist_item(line: &str) -> ActionItem {
    let done = !line.starts_with("- [ ]");
    let rest = line[6..].trim();
    let (assignee, task) = match rest.strip_prefix('@').and_then(|r| r.split_once(':')) {
        Some((who, task)) => (Some(who.trim().to_string()), task.trim()),
        None => (None, rest),
    };
    ActionItem { task: task.to_string(), assignee, due: None, done }
}

/// Check or uncheck one action item, by its position in `action_items`. Structured
/// items go through core (frontmatter status plus the matching body line); body-only
/// items change their checklist box.
pub fn set_action_item(path: &Path, index: usize, done: bool) -> Result<(), String> {
    ensure_inside_output(path)?;
    crate::fsutil::with_file_lock(path, || {
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let (fm, body) = frontmatter(&content);
        let structured = fm.get("action_items").and_then(|v| v.as_sequence()).is_some_and(|s| !s.is_empty());
        if structured {
            return minutes_core::markdown::set_action_item_status(path, index, done).map_err(|e| e.to_string());
        }
        let (at, _) = checklist_lines(body).into_iter().nth(index).ok_or("That action item is gone. Reopen the meeting.")?;
        let at = content.len() - body.len() + at;
        let mut updated = content.clone();
        updated.replace_range(at..at + 1, if done { "x" } else { " " });
        crate::fsutil::write_atomic(path, updated.as_bytes(), None).map_err(|e| e.to_string())
    })
    .map_err(|e| format!("Couldn't update the action item: {e}"))?;
    let _ = minutes_core::derived::refresh_derived_views(path, &corecfg::load(), &Default::default());
    Ok(())
}

/// The recording next to a meeting file, if there is one.
pub fn audio_path(path: &Path) -> Result<PathBuf, String> {
    ensure_inside_output(path)?;
    let wav = path.with_extension("wav");
    if wav.is_file() {
        Ok(wav)
    } else {
        Err("This meeting has no recording.".into())
    }
}

/// Only touch files under the configured output folder.
pub fn ensure_inside_output(path: &Path) -> Result<(), String> {
    let root = output_dir().canonicalize().map_err(|_| "Meetings folder not found.")?;
    let target = path.canonicalize().map_err(|_| "Meeting file not found.")?;
    if target.starts_with(&root) && target.extension().is_some_and(|e| e == "md") {
        Ok(())
    } else {
        Err("That file is outside your meetings folder.".into())
    }
}

/// Replace the body of `## Notes` (or insert the section before `## Transcript`). Every
/// other byte of the file is kept, including the frontmatter.
pub fn replace_notes(path: &Path, notes: &str) -> Result<(), String> {
    crate::fsutil::with_file_lock(path, || replace_notes_locked(path, notes))
}

fn replace_notes_locked(path: &Path, notes: &str) -> Result<(), String> {
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let updated = splice_notes(&content, notes);
    if updated == content {
        return Ok(());
    }
    crate::fsutil::write_atomic(path, updated.as_bytes(), None).map_err(|e| e.to_string())?;
    let cfg = corecfg::load();
    let _ = minutes_core::derived::refresh_derived_views(path, &cfg, &Default::default());
    Ok(())
}

/// The body of the `## Notes` section as it is on disk now.
pub fn notes_section(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let (_, body) = split_frontmatter(&content);
    Some(sections(body).into_iter().find(|s| s.heading == "Notes").map(|s| s.body).unwrap_or_default())
}

pub fn splice_notes(content: &str, notes: &str) -> String {
    let (_, body) = split_frontmatter(content);
    let head = &content[..content.len() - body.len()];
    // A level-2 heading typed into notes would split the section on the next read.
    // src/notes.ts `storedNotes` repeats these rules: keep the two in step.
    let notes: String = notes
        .trim()
        .lines()
        .map(|l| match l.strip_prefix("## ") {
            Some(rest) => format!("### {rest}"),
            None => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    let new_section = if notes.is_empty() { String::new() } else { format!("## Notes\n\n{notes}\n\n") };

    let lines: Vec<&str> = body.split_inclusive('\n').collect();
    let is_heading = |l: &str| l.starts_with("## ");
    let notes_at = lines.iter().position(|l| l.trim_end() == "## Notes");
    let mut out = String::from(head);
    match notes_at {
        Some(start) => {
            let end = lines[start + 1..]
                .iter()
                .position(|l| is_heading(l))
                .map(|i| start + 1 + i)
                .unwrap_or(lines.len());
            out.extend(lines[..start].iter().copied());
            out.push_str(&new_section);
            out.extend(lines[end..].iter().copied());
        }
        None => {
            let at = lines.iter().position(|l| l.trim_end() == "## Transcript").unwrap_or(lines.len());
            out.extend(lines[..at].iter().copied());
            if at == lines.len() && !out.ends_with("\n\n") && !new_section.is_empty() {
                out.push('\n');
            }
            out.push_str(&new_section);
            out.extend(lines[at..].iter().copied());
        }
    }
    out
}

/// Change the title in the frontmatter. The file name stays, so links from Obsidian notes
/// keep working.
pub fn set_title(path: &Path, title: &str) -> Result<(), String> {
    ensure_inside_output(path)?;
    let title = title.trim();
    if title.is_empty() {
        return Err("A meeting needs a title.".into());
    }
    crate::fsutil::with_file_lock(path, || minutes_core::markdown::update_frontmatter(path, |fm| fm.title = title.to_string()))
        .map_err(|e| format!("Couldn't rename the meeting: {e}"))?;
    let cfg = corecfg::load();
    let _ = minutes_core::derived::refresh_derived_views(path, &cfg, &Default::default());
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct SearchHit {
    pub path: String,
    pub excerpt: String,
}

/// Case-insensitive search over whole files. Fine for thousands of meetings, and it takes
/// no lock, unlike core's per-query FTS index. Files are read on a few threads.
pub fn search(query: &str) -> Vec<SearchHit> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return vec![];
    }
    let meetings = list();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(8);
    let chunk = meetings.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let parts: Vec<_> = meetings
            .chunks(chunk)
            .map(|part| scope.spawn(|| part.iter().filter_map(|m| search_file(m, &q)).collect::<Vec<_>>()))
            .collect();
        // Chunks are joined in order, so hits keep the library's order.
        parts.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
    })
}

fn search_file(m: &MeetingSummary, q: &str) -> Option<SearchHit> {
    let content = std::fs::read_to_string(&m.path).ok()?;
    let (_, body) = split_frontmatter(&content);
    let in_title = m.title.to_lowercase().contains(q);
    let line = find_line(body, q);
    if !in_title && line.is_none() {
        return None;
    }
    Some(SearchHit { path: m.path.clone(), excerpt: line.map(plain_line).unwrap_or_default() })
}

/// The first line of `body` that contains the lowercase `q`, ignoring case.
fn find_line<'a>(body: &'a str, q: &str) -> Option<&'a str> {
    if !q.is_ascii() {
        return body.lines().find(|l| l.to_lowercase().contains(q));
    }
    // ASCII lowercasing keeps every byte offset, so a match maps straight back to `body`.
    let at = body.to_ascii_lowercase().find(q)?;
    let start = body[..at].rfind('\n').map_or(0, |i| i + 1);
    let end = body[at..].find('\n').map_or(body.len(), |i| at + i);
    Some(body[start..end].trim_end_matches('\r'))
}

fn plain_line(line: &str) -> String {
    let l = line.trim();
    let text = match turn_re().captures(l) {
        Some(c) => c[3].to_string(),
        None => l.trim_start_matches(['-', '*', '#', '>', ' ']).to_string(),
    };
    match text.char_indices().nth(140) {
        Some((i, _)) => format!("{}…", &text[..i]),
        None => text,
    }
}

pub fn obsidian_vault_for(path: &Path) -> Option<PathBuf> {
    path.ancestors().skip(1).find(|dir| dir.join(".obsidian").is_dir()).map(Path::to_path_buf)
}

/// Move a meeting and its audio and sidecars to the Trash.
pub fn trash(path: &Path) -> Result<(), String> {
    ensure_inside_output(path)?;
    trash::delete_all(meeting_files(path)).map_err(|e| format!("Could not move to Trash: {e}"))
}

/// A meeting file and every file beside it that belongs to it and exists: the audio, the
/// stems, the voice embeddings, and the hidden backups that core's "summarize again" keeps
/// (`.<name>.md.pre-resummarize.<time>.bak`). Each backup is a full copy of the meeting,
/// so a delete must take them too (D10).
pub fn meeting_files(path: &Path) -> Vec<PathBuf> {
    let mut items = vec![path.to_path_buf()];
    items.extend(minutes_core::capture::meeting_audio_artifact_paths(path).into_iter().filter(|p| p.exists()));
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let prefix = format!(".{name}.pre-resummarize.");
    if let (Some(dir), false) = (path.parent(), name.is_empty()) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            let mut backups: Vec<PathBuf> = entries
                .flatten()
                .filter(|e| {
                    let file = e.file_name();
                    let file = file.to_string_lossy();
                    file.strip_prefix(&prefix)
                        .and_then(|rest| rest.strip_suffix(".bak"))
                        .is_some_and(|t| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()))
                })
                .map(|e| e.path())
                .collect();
            backups.sort();
            items.extend(backups);
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "---\ntitle: T\ntype: meeting\n---\n\n## Summary\n\nS\n\n## Notes\n\n- a\n\n## Transcript\n\n[SPEAKER_0 0:00] hi\n[Alice Smith 1:02] yo\n";

    #[test]
    fn splice_replaces_notes_only() {
        let out = splice_notes(FILE, "- new\n## Big");
        assert!(out.starts_with("---\ntitle: T\ntype: meeting\n---\n\n## Summary\n\nS\n\n## Notes\n\n- new\n### Big\n\n## Transcript"));
        assert!(out.ends_with("[Alice Smith 1:02] yo\n"));
    }

    #[test]
    fn splice_inserts_before_transcript() {
        let file = "---\ntitle: T\n---\n\n## Transcript\n\n[0:00] hi\n";
        let out = splice_notes(file, "x");
        assert_eq!(out, "---\ntitle: T\n---\n\n## Notes\n\nx\n\n## Transcript\n\n[0:00] hi\n");
    }

    #[test]
    fn splice_empty_removes_section() {
        let out = splice_notes(FILE, "  ");
        assert!(!out.contains("## Notes"));
        assert!(out.contains("## Summary\n\nS\n\n## Transcript"));
    }

    #[test]
    fn action_items_fall_back_to_the_checklist() {
        let body = "## Summary\n\n- [ ] not an action\n\n## Action Items\n- [ ] @Priya: send the deck\n- [x] book the room\n\n## Notes\n\n- [ ] mine\n";
        let items = action_items(&serde_yaml::Value::Null, body);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0], ActionItem { task: "send the deck".into(), assignee: Some("Priya".into()), due: None, done: false });
        assert!(items[1].done);
        assert_eq!(&body[checklist_lines(body)[1].0..][..1], "x");
    }

    #[test]
    fn structured_action_items_win() {
        let fm: serde_yaml::Value = serde_yaml::from_str("action_items:\n- assignee: unassigned\n  task: ship it\n  due: Friday\n  status: done\n").unwrap();
        let items = action_items(&fm, "## Action Items\n- [ ] other\n");
        assert_eq!(items, vec![ActionItem { task: "ship it".into(), assignee: None, due: Some("Friday".into()), done: true }]);
    }

    #[test]
    fn list_cache_follows_file_changes() {
        let dir = std::env::temp_dir().join(format!("margin-cache-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.md");
        let titles = |files: Vec<PathBuf>| cached_summaries(files).into_iter().map(|m| m.title).collect::<Vec<_>>();
        std::fs::write(&file, "---\ntitle: One\ntype: meeting\n---\n").unwrap();
        assert_eq!(titles(vec![file.clone()]), ["One"]);
        std::fs::write(&file, "---\ntitle: Second\ntype: meeting\n---\n").unwrap();
        assert_eq!(titles(vec![file.clone()]), ["Second"]);
        std::fs::remove_file(&file).unwrap();
        assert!(titles(vec![file]).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn finds_the_matching_line_ignoring_case() {
        let body = "## Notes\n\n- Legal Review on Friday\n\n## Transcript\n\n[0:01] héllo LEGAL\n";
        assert_eq!(find_line(body, "legal review"), Some("- Legal Review on Friday"));
        assert_eq!(find_line(body, "héllo"), Some("[0:01] héllo LEGAL"));
        assert_eq!(find_line(body, "zebra"), None);
        assert_eq!(find_line("one\r\ntwo LEGAL\r\n", "legal"), Some("two LEGAL"));
    }

    #[test]
    fn parses_turns() {
        let (_, body) = split_frontmatter(FILE);
        let t = sections(body).into_iter().find(|s| s.heading == "Transcript").unwrap();
        let turns = parse_transcript(&t.body);
        assert_eq!(turns[0].speaker.as_deref(), Some("SPEAKER_0"));
        assert_eq!(turns[1].speaker.as_deref(), Some("Alice Smith"));
        assert_eq!(turns[1].ts, "1:02");
        let call = parse_transcript("[MICROPHONE 0:00] hi\n[SPEAKER_0 0:22] [SYSTEM] yo");
        assert_eq!(call[0].speaker.as_deref(), Some("MICROPHONE"));
        assert_eq!(call[1].text, "yo");
        assert_eq!(call[1].source.as_deref(), Some("SYSTEM"));
        let bare = parse_transcript("[0:09] So let's add that.");
        assert_eq!(bare[0].speaker, None);
        assert_eq!(bare[0].text, "So let's add that.");
    }

    #[test]
    fn meeting_files_include_the_resummarize_backups() {
        let dir = std::env::temp_dir().join(format!("margin-meeting-files-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let md = dir.join("2026-11-10-call.md");
        let mine = [
            "2026-11-10-call.md",
            "2026-11-10-call.wav",
            ".2026-11-10-call.md.pre-resummarize.1784000001.bak",
            ".2026-11-10-call.md.pre-resummarize.1784000002000000000.bak",
        ];
        let others = [
            ".2026-11-10-call-2.md.pre-resummarize.1784000001.bak",
            ".2026-11-10-call.md.pre-resummarize.x.bak",
            "2026-11-10-call-2.md",
        ];
        for f in mine.iter().chain(&others) {
            std::fs::write(dir.join(f), "x").unwrap();
        }
        let mut got: Vec<String> =
            meeting_files(&md).iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect();
        got.sort();
        let mut want: Vec<String> = mine.iter().map(|s| s.to_string()).collect();
        want.sort();
        assert_eq!(got, want);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_turns_after_99_minutes() {
        let turns = parse_transcript("[SPEAKER_0 99:59] one\n[SPEAKER_1 100:00] two\n[1000:05] three");
        assert_eq!(turns.len(), 3);
        assert_eq!(turns[1].speaker.as_deref(), Some("SPEAKER_1"));
        assert_eq!(turns[1].ts, "100:00");
        assert_eq!(turns[1].text, "two");
        assert_eq!(turns[2].ts, "1000:05");
    }
}
