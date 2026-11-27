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
        title: str_field(&fm, "title").map_or_else(|| file_title(path), |t| if is_core_cut_title(&t) { tidy_title(&t) } else { t }),
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
    let joined = strip_inline_marks(&text.join(" "));
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

pub(crate) fn parse_transcript(body: &str) -> Vec<Turn> {
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
/// `task`, when given, is the text the user saw. If the item at `index` has another text
/// now, the list changed (a new summary) and the tick is refused, not put on another item.
pub fn set_action_item(path: &Path, index: usize, done: bool, task: Option<&str>) -> Result<(), String> {
    ensure_inside_output(path)?;
    crate::fsutil::with_file_lock(path, || {
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let (fm, body) = frontmatter(&content);
        if task.is_some_and(|t| !task_at(&fm, body, index, t)) {
            return Err("The action items changed. Check the list and tick the item again.".to_string());
        }
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

fn task_at(fm: &serde_yaml::Value, body: &str, index: usize, task: &str) -> bool {
    action_items(fm, body).get(index).is_some_and(|i| i.task == task.trim())
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
    // Section headings as `sections()` sees them: a `## ` line inside a fenced code block
    // is text, not the end of the section.
    let mut in_fence = false;
    let heading: Vec<bool> = lines
        .iter()
        .map(|l| {
            if l.trim_start().starts_with("```") {
                in_fence = !in_fence;
            }
            !in_fence && l.starts_with("## ")
        })
        .collect();
    let heading_at = |name: &str| (0..lines.len()).find(|&i| heading[i] && lines[i].trim_end() == name);
    let notes_at = heading_at("## Notes");
    let mut out = String::from(head);
    match notes_at {
        Some(start) => {
            let end = (start + 1..lines.len()).find(|&i| heading[i]).unwrap_or(lines.len());
            out.extend(lines[..start].iter().copied());
            out.push_str(&new_section);
            out.extend(lines[end..].iter().copied());
        }
        None => {
            let at = heading_at("## Transcript").unwrap_or(lines.len());
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
    let title = tidy_title(title);
    let title = title.as_str();
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

/// What a search answers: the hits, and when the query found nothing and a near-miss
/// spelling did, the query that was used instead (D93). The window shows "Showing results
/// for ...". Serialized as `{ "hits": [...], "corrected": "warranty" | null }`.
#[derive(Debug, Serialize)]
pub struct SearchResults {
    pub hits: Vec<SearchHit>,
    pub corrected: Option<String>,
}

/// Case-insensitive search over whole files, with a typo fallback when nothing matches.
pub fn search(query: &str) -> SearchResults {
    let hits = search_exact(query);
    if !hits.is_empty() {
        return SearchResults { hits, corrected: None };
    }
    let corrected = corrected_query(query).filter(|c| c != &query.trim().to_lowercase());
    let hits = corrected.as_deref().map(search_exact).unwrap_or_default();
    SearchResults { corrected: (!hits.is_empty()).then_some(corrected).flatten(), hits }
}

/// Runs `f` over the meetings on a few threads. Results keep the library's order.
fn par_filter_map<T: Send>(meetings: &[MeetingSummary], f: impl Fn(&MeetingSummary) -> Option<T> + Sync) -> Vec<T> {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(8);
    let chunk = meetings.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let parts: Vec<_> = meetings.chunks(chunk).map(|part| scope.spawn(|| part.iter().filter_map(&f).collect::<Vec<_>>())).collect();
        parts.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
    })
}

/// Substring search over whole files. Fine for thousands of meetings, and it takes no lock,
/// unlike core's per-query FTS index. Files are read on a few threads.
fn search_exact(query: &str) -> Vec<SearchHit> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return vec![];
    }
    par_filter_map(&list(), |m| search_file(m, &q))
}

/// Every word in the library with the number of times it occurs, titles and bodies.
fn corpus_words() -> HashMap<String, usize> {
    let per_meeting = par_filter_map(&list(), |m| {
        let content = std::fs::read_to_string(&m.path).ok()?;
        let (_, body) = split_frontmatter(&content);
        let mut counts: HashMap<String, usize> = HashMap::new();
        for word in format!("{} {body}", m.title).to_lowercase().split(|c: char| !c.is_alphabetic()) {
            if (3..=24).contains(&word.chars().count()) {
                *counts.entry(word.to_string()).or_default() += 1;
            }
        }
        Some(counts)
    });
    per_meeting.into_iter().fold(HashMap::new(), |mut all, counts| {
        for (word, n) in counts {
            *all.entry(word).or_default() += n;
        }
        all
    })
}

/// The query with each unknown word swapped for the closest word in the library, within two
/// edits (one for words of 3 or 4 letters). None when no word changed.
fn corrected_query(query: &str) -> Option<String> {
    corrected_query_in(query, &corpus_words())
}

fn corrected_query_in(query: &str, corpus: &HashMap<String, usize>) -> Option<String> {
    let lowered = query.trim().to_lowercase();
    let mut changed = false;
    let fixed: Vec<String> = lowered
        .split_whitespace()
        .map(|w| match closest_word(w, corpus) {
            Some(c) => {
                changed = true;
                c.to_string()
            }
            None => w.to_string(),
        })
        .collect();
    changed.then(|| fixed.join(" "))
}

/// The corpus word nearest to an unknown `word` within the edit limit. None for a known
/// word, a short one or one with a digit. Ties go to the word that occurs more, then
/// alphabetical order, so the answer is the same on every run.
fn closest_word<'a>(word: &str, corpus: &'a HashMap<String, usize>) -> Option<&'a str> {
    let chars: Vec<char> = word.chars().collect();
    if chars.len() < 3 || !chars.iter().all(|c| c.is_alphabetic()) || corpus.contains_key(word) {
        return None;
    }
    let limit = if chars.len() <= 4 { 1 } else { 2 };
    corpus
        .iter()
        .filter(|(c, _)| c.chars().count().abs_diff(chars.len()) <= limit)
        .filter_map(|(c, n)| edit_distance(&chars, &c.chars().collect::<Vec<_>>(), limit).map(|d| (d, std::cmp::Reverse(*n), c.as_str())))
        .min()
        .map(|(_, _, c)| c)
}

/// Optimal string alignment distance (a swap of two neighbours is one edit, as in
/// "waranty"/"warrnty"), or None above `limit`.
fn edit_distance(a: &[char], b: &[char], limit: usize) -> Option<usize> {
    let mut prev2: Vec<usize> = vec![0; b.len() + 1];
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                cur[j] = cur[j].min(prev2[j - 2] + 1);
            }
        }
        if cur.iter().min().is_some_and(|m| *m > limit) {
            return None;
        }
        prev2 = prev;
        prev = cur;
    }
    Some(prev[b.len()]).filter(|d| *d <= limit)
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

/// Bold, italic and code marks read as noise in a one-line excerpt ("Price Target:** €25").
fn strip_inline_marks(text: &str) -> String {
    text.replace("**", "").replace("__", "").replace('`', "")
}

fn plain_line(line: &str) -> String {
    let l = line.trim();
    let text = strip_inline_marks(&match turn_re().captures(l) {
        Some(c) => c[3].to_string(),
        None => l.trim_start_matches(['-', '*', '#', '>', ' ']).to_string(),
    });
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

/// Whisper gets a prompt from core: "Names and terms that may appear in this audio: <title,
/// names>. Preserve spelling exactly when heard." On room noise Whisper often reads it back,
/// and the transcript gets lines nobody said (benchmark 26 Nov 2026: 8 echoes in 225 s of
/// room tone). These lines are removed from `## Transcript`:
/// - a line that contains the prompt's fixed words ("preserve spelling exactly",
///   "names and terms that may appear");
/// - a line that is only the meeting title, when it occurs at least twice. One title-only
///   line can be real speech.
/// - silence hallucinations (D90), see `drop_silence_hallucinations`.
/// Every other byte stays. Returns the cleaned content and the number of lines removed.
pub fn scrub_prompt_echo_text(content: &str, title: &str) -> (String, usize) {
    let (without_echoes, echoes) = drop_prompt_echoes(content, title);
    let (clean, hallucinations) = drop_silence_hallucinations(&without_echoes);
    (clean, echoes + hallucinations)
}

fn drop_prompt_echoes(content: &str, title: &str) -> (String, usize) {
    let norm = |s: &str| s.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
    let spoken = |line: &str| -> Option<String> {
        let rest = line.strip_prefix('[')?;
        let close = rest.find(']')?;
        Some(rest[close + 1..].trim().to_string())
    };
    let title_n = norm(title);
    let Some(start) = content.find("\n## Transcript") else { return (content.to_string(), 0) };
    let (head, tail) = content.split_at(start);
    let title_lines = tail.lines().filter(|l| spoken(l).is_some_and(|t| !title_n.is_empty() && norm(&t) == title_n)).count();
    let mut removed = 0;
    let mut last_was_echo = false;
    let kept: Vec<&str> = tail
        .split_inclusive('\n')
        .filter(|l| {
            // Core writes "[...] [repeated audio removed - N identical segments collapsed]"
            // after a line it repeated. When that line was an echo, the marker goes with it.
            if l.trim_start().starts_with("[...] [repeated audio removed") {
                if last_was_echo { removed += 1; return false; }
                return true;
            }
            let Some(text) = spoken(l.trim_end()) else { last_was_echo = false; return true };
            let t = norm(&text);
            let echo = t.contains("preserve spelling exactly") || t.contains("names and terms that may appear")
                || (title_lines >= 2 && !title_n.is_empty() && t == title_n);
            if echo { removed += 1; }
            last_was_echo = echo;
            !echo
        })
        .collect();
    (format!("{head}{}", kept.concat()), removed)
}

/// Lines Whisper writes on silence, hum and noise because its training data held them as
/// subtitles. Compared after `normalise`. A line that contains one of these is never speech
/// in a meeting. Multilingual on purpose: the Russian "to be continued" came from a quiet
/// microphone in an English meeting (26 Nov 2026).
const SILENCE_PHRASES: &[&str] = &[
    "thanks for watching", "thank you for watching", "thanks for listening", "thank you for listening",
    "please subscribe", "like and subscribe", "don t forget to subscribe", "subscribe to my channel",
    "see you in the next video", "amara org",
    "продолжение следует", "спасибо за просмотр", "подписывайтесь на канал", "субтитры сделал", "субтитры создавал",
    "редактор субтитров",
    "untertitel der amara org", "untertitel im auftrag des zdf", "vielen dank fürs zuschauen",
    "sous titres", "sous titrage", "merci d avoir regardé", "merci d avoir regarde",
    "subtítulos realizados por la comunidad", "subtitulos realizados por la comunidad", "gracias por ver el video", "suscríbete", "suscribete",
    "legendas pela comunidade", "obrigado por assistir",
    "sottotitoli creati dalla comunità", "sottotitoli creati dalla comunita", "grazie per la visione",
    "ondertitels ingediend door de amara", "bedankt voor het kijken",
    "ご視聴ありがとうございました", "チャンネル登録", "字幕由", "请不要忘记订阅", "谢谢观看", "感谢观看", "謝謝觀看", "시청해 주셔서 감사합니다",
    "اشتركوا في القناة", "ترجمة نانسي قنقر",
];

/// Short lines that Whisper also writes on silence but that a person can say. They go only
/// when nothing else real is in the transcript.
const WEAK_PHRASES: &[&str] = &[
    "thank you", "thanks", "thank you very much", "thank you so much", "bye", "bye bye", "goodbye", "you", "okay", "ok",
    "yeah", "yes", "no", "so", "hmm", "mm", "uh", "um", "oh", "music", "applause", "спасибо", "музыка",
];

/// Core tags a call line's text with its capture source, `[SYSTEM] text`. That tag is real
/// (a call's remote side), so a line is judged by what follows it.
fn strip_source_tag(text: &str) -> &str {
    let text = text.trim();
    ["[MICROPHONE]", "[SYSTEM]"].iter().find_map(|tag| text.strip_prefix(tag)).unwrap_or(text).trim()
}

fn normalise(s: &str) -> String {
    s.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `[BLANK_AUDIO]`, `(music)`, `♪`: markers and no words.
fn is_marker_only(text: &str) -> bool {
    let mut rest = text.trim();
    while let Some(first) = rest.chars().next() {
        let close = match first {
            '[' => ']',
            '(' => ')',
            '♪' | '♫' | '*' | '.' | '…' => {
                rest = rest[first.len_utf8()..].trim_start();
                continue;
            }
            _ => return false,
        };
        match rest.find(close) {
            Some(i) => rest = rest[i + 1..].trim_start(),
            None => return false,
        }
    }
    true
}

fn is_silence_phrase(text: &str) -> bool {
    let n = normalise(text);
    !n.is_empty() && SILENCE_PHRASES.iter().any(|p| n.contains(&normalise(p)))
}

fn is_weak_phrase(text: &str) -> bool {
    WEAK_PHRASES.contains(&normalise(text).as_str())
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Script {
    Latin,
    Cyrillic,
    Greek,
    Hebrew,
    Arabic,
    Thai,
    Hangul,
    Cjk,
    Indic,
}

fn script_of(c: char) -> Option<Script> {
    if !c.is_alphabetic() {
        return None;
    }
    Some(match c as u32 {
        0..=0x024F | 0x1E00..=0x1EFF => Script::Latin,
        0x0370..=0x03FF => Script::Greek,
        0x0400..=0x052F => Script::Cyrillic,
        0x0590..=0x05FF => Script::Hebrew,
        0x0600..=0x06FF => Script::Arabic,
        // Devanagari, Bengali, Gurmukhi, Gujarati, Odia, Tamil, Telugu, Kannada, Malayalam.
        0x0900..=0x0D7F => Script::Indic,
        0x0E00..=0x0E7F => Script::Thai,
        0x1100..=0x11FF | 0xAC00..=0xD7AF => Script::Hangul,
        0x3040..=0x30FF | 0x4E00..=0x9FFF => Script::Cjk,
        _ => return None,
    })
}

fn script_letters(text: &str) -> HashMap<Script, usize> {
    let mut counts = HashMap::new();
    for script in text.chars().filter_map(script_of) {
        *counts.entry(script).or_default() += 1;
    }
    counts
}

/// The script that holds at least 60% of a text's letters.
fn line_script(text: &str) -> Option<Script> {
    let counts = script_letters(text);
    let total: usize = counts.values().sum();
    let (script, n) = counts.into_iter().max_by_key(|&(_, n)| n)?;
    (n * 10 >= total * 6).then_some(script)
}

/// Indexes of lines in a script that the rest of the transcript does not use. A hallucination
/// is one stray line (a lone Cyrillic or Chinese line in an English meeting). People do mix
/// Indic scripts (Devanagari, Kannada, Tamil, Telugu, ...) into English speech, so a line in
/// one goes only when it is the single stray line of a transcript of 20 lines or more. A
/// script that more than a tenth of the lines use is a real share and stays. Latin is never
/// foreign: English words are everywhere in speech in other languages.
fn foreign_script_lines(texts: &[(usize, String)]) -> Vec<usize> {
    let mut letters: HashMap<Script, usize> = HashMap::new();
    for (_, text) in texts {
        for (script, n) in script_letters(text) {
            *letters.entry(script).or_default() += n;
        }
    }
    let Some((&dominant, _)) = letters.iter().max_by_key(|&(script, n)| (*n, *script == Script::Latin)) else { return vec![] };
    let mut by_script: HashMap<Script, Vec<usize>> = HashMap::new();
    for (i, text) in texts {
        if let Some(script) = line_script(text).filter(|s| *s != dominant && *s != Script::Latin) {
            by_script.entry(script).or_default().push(*i);
        }
    }
    let n = texts.len();
    by_script
        .into_iter()
        .filter(|(script, lines)| {
            let allowed = match (*script == Script::Indic, n) {
                (true, n) => usize::from(n >= 20),
                (false, n) if n >= 3 => n.div_ceil(10),
                _ => 0,
            };
            lines.len() <= allowed
        })
        .flat_map(|(_, lines)| lines)
        .collect()
}

fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

/// What core writes in place of the trailing noise it trimmed. On a silent recording all of
/// the transcript is noise, so this line is all that is left, and it is not speech.
const CORE_TRAILING_NOISE_MARKER: &str = "[Recording ended - ";

/// Whisper writes lines nobody said when the microphone hears silence or noise. This removes
/// them from `## Transcript` (D90). A transcript line goes when:
/// - it is only markers (`[BLANK_AUDIO]`, `(music)`, a bare `[SYSTEM]`);
/// - it holds a known silence phrase ("Thanks for watching", "Продолжение следует", the
///   Amara subtitle credit, in many languages);
/// - it is a stray line in a script the rest of the transcript does not use;
/// - it is a dangling fragment that starts with "..." in a transcript of at most 12 words
///   in which something else was just removed (a silent recording yields a subtitle credit
///   and a half sentence together);
/// - it is core's trailing-noise marker and no speech is left;
/// - it is a "thank you" or one-word line, and nothing real is left (silence often becomes
///   a lone "Thank you").
/// Every other byte stays. Returns the cleaned content and the number of lines removed.
fn drop_silence_hallucinations(content: &str) -> (String, usize) {
    let Some(start) = content.find("\n## Transcript") else { return (content.to_string(), 0) };
    let (head, tail) = content.split_at(start);
    let lines: Vec<&str> = tail.split_inclusive('\n').collect();
    let spoken: Vec<Option<String>> = lines
        .iter()
        .map(|l| turn_re().captures(l.trim()).map(|c| strip_source_tag(&c[3]).to_string()))
        .collect();
    let mut dropped: Vec<bool> = spoken.iter().map(|t| t.as_deref().is_some_and(|t| is_marker_only(t) || is_silence_phrase(t))).collect();
    let live = |dropped: &[bool]| -> Vec<(usize, String)> {
        spoken.iter().enumerate().filter(|(i, _)| !dropped[*i]).filter_map(|(i, t)| t.clone().map(|t| (i, t))).collect()
    };
    for i in foreign_script_lines(&live(&dropped)) {
        dropped[i] = true;
    }
    let remaining = live(&dropped);
    if dropped.iter().any(|d| *d) && remaining.iter().map(|(_, t)| word_count(t)).sum::<usize>() <= 12 {
        for (i, t) in &remaining {
            if t.starts_with("...") || t.starts_with('…') {
                dropped[*i] = true;
            }
        }
    }
    let remaining = live(&dropped);
    if remaining.iter().all(|(_, t)| is_weak_phrase(t)) {
        for (i, _) in &remaining {
            dropped[*i] = true;
        }
    }
    let nothing_spoken = live(&dropped).is_empty();
    let mut removed = 0;
    let mut last_dropped = false;
    let kept: Vec<&str> = lines
        .iter()
        .enumerate()
        .filter(|(i, l)| {
            if nothing_spoken && l.trim_start().starts_with(CORE_TRAILING_NOISE_MARKER) {
                removed += 1;
                return false;
            }
            let marker = l.trim_start().starts_with("[...] [repeated audio removed");
            let drop = if marker { last_dropped } else { dropped[*i] };
            if marker || spoken[*i].is_some() {
                last_dropped = drop;
            }
            removed += usize::from(drop);
            !drop
        })
        .map(|(_, l)| *l)
        .collect();
    (format!("{head}{}", kept.concat()), removed)
}

/// Words left in `## Transcript`, not counting source tags.
fn transcript_words(content: &str) -> usize {
    let (_, body) = split_frontmatter(content);
    let Some(at) = body.find("## Transcript") else { return 0 };
    parse_transcript(&body[at..]).iter().map(|t| word_count(&t.text)).sum()
}

const NO_SPEECH_TITLE: &str = "No speech recorded";

/// After the scrub (D91): when the transcript has no words left, say so in the file. The
/// status becomes `no-speech` (the library already shows that), a `processing_warnings`
/// entry says why, and an untitled meeting gets a plain title and loses the entities that
/// were guessed from invented words. `removed` is how many lines the scrub took out: with
/// none, an empty transcript is not this function's to judge. Returns true when the meeting
/// has no speech, whether core already knew or this call found out. A title the user chose
/// stays.
pub fn mark_no_speech(path: &Path, untitled: bool, removed: usize) -> Result<bool, String> {
    let marked = crate::fsutil::with_file_lock(path, || write_no_speech(path, untitled, removed))?;
    if marked {
        let _ = minutes_core::derived::refresh_derived_views(path, &corecfg::load(), &Default::default());
    }
    Ok(marked)
}

fn write_no_speech(path: &Path, untitled: bool, removed: usize) -> Result<bool, String> {
    use minutes_core::markdown::{Frontmatter, OutputStatus, ProcessingWarning};
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let (fm_text, _) = split_frontmatter(&content);
    let fm: Frontmatter = serde_yaml::from_str(fm_text).map_err(|e| e.to_string())?;
    if fm.status == Some(OutputStatus::NoSpeech) {
        return Ok(true);
    }
    if removed == 0 || transcript_words(&content) > 0 {
        return Ok(false);
    }
    minutes_core::markdown::update_frontmatter(path, |fm| {
        fm.status = Some(OutputStatus::NoSpeech);
        if untitled {
            fm.title = NO_SPEECH_TITLE.to_string();
            fm.entities = Default::default();
        }
        fm.processing_warnings.push(ProcessingWarning {
            step: "transcribe".into(),
            reason: "no_speech".into(),
            timeout_secs: None,
            message: Some("Only lines that Whisper makes up on silence were heard, so Margin removed them.".into()),
        });
    })
    .map_err(|e| format!("Couldn't mark the meeting as having no speech: {e}"))?;
    Ok(true)
}

/// Core cuts a title at 57 characters and adds "..." (60 in all), in the middle of a word.
const CORE_CUT_TITLE_CHARS: usize = 60;

const TITLE_TAIL_WORDS: &[&str] = &["a", "an", "and", "as", "at", "by", "for", "from", "in", "of", "on", "or", "the", "to", "with", "i", "you", "is", "am", "but", "so"];

pub fn is_core_cut_title(title: &str) -> bool {
    title.chars().count() == CORE_CUT_TITLE_CHARS && title.ends_with("...")
}

/// A title never ends with "..." (D92). Core's own cut (exactly 60 characters ending in
/// "...") leaves half a word, so that word goes, and then dangling "in", "a", "the", so
/// "...Giving You a Split in Ca..." reads "...Giving You a Split". Any other ellipsis is
/// only removed.
pub fn tidy_title(title: &str) -> String {
    let title = title.trim();
    let Some(body) = title.strip_suffix("...").or_else(|| title.strip_suffix('…')) else { return title.to_string() };
    if !is_core_cut_title(title) {
        return body.trim().trim_end_matches([',', ';', ':', '-', '.']).to_string();
    }
    let mut words: Vec<&str> = body.split_whitespace().collect();
    if !body.ends_with(char::is_whitespace) {
        words.pop();
    }
    let dangling = |w: &&str| {
        let bare = w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
        bare.is_empty() || TITLE_TAIL_WORDS.contains(&bare.as_str())
    };
    while words.last().is_some_and(dangling) {
        words.pop();
    }
    let tidy = words.join(" ");
    match tidy.trim_end_matches([',', ';', ':', '-', ' ']) {
        "" => body.trim().to_string(),
        cut => cut.to_string(),
    }
}

/// A title that core cut and ended with "..." is rewritten at a word boundary (D92).
pub fn tidy_title_in_file(path: &Path) -> Result<bool, String> {
    crate::fsutil::with_file_lock(path, || {
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let (fm_text, _) = split_frontmatter(&content);
        let fm: minutes_core::markdown::Frontmatter = serde_yaml::from_str(fm_text).map_err(|e| e.to_string())?;
        let tidy = tidy_title(&fm.title);
        if tidy == fm.title || tidy.is_empty() {
            return Ok(false);
        }
        minutes_core::markdown::update_frontmatter(path, |fm| fm.title = tidy).map_err(|e| e.to_string())?;
        Ok(true)
    })
}

/// `scrub_prompt_echo_text` on a meeting file, under its lock. Writes only when a line goes.
pub fn scrub_prompt_echo(path: &Path) -> Result<usize, String> {
    crate::fsutil::with_file_lock(path, || {
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let (fm, _) = split_frontmatter(&content);
        let title = serde_yaml::from_str::<minutes_core::markdown::Frontmatter>(fm).map(|f| f.title).unwrap_or_default();
        let (cleaned, removed) = scrub_prompt_echo_text(&content, &title);
        if removed > 0 {
            crate::fsutil::write_atomic(path, cleaned.as_bytes(), None).map_err(|e| e.to_string())?;
        }
        Ok(removed)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_echoes_leave_the_transcript_and_real_speech_stays() {
        let md = "---\ntitle: Silence test\n---\n\n## Notes\n\nPreserve spelling exactly when heard is in my notes.\n\n## Transcript\n\n[SPEAKER_1 0:00] Hello everyone.\n[UNKNOWN 1:15] Silence test. Preserve spelling exactly when hearing.\n[UNKNOWN 1:20] Names and terms that may appear in this audio: Mohan.\n[UNKNOWN 4:45] Silence test.\n[SPEAKER_1 5:00] Thanks, bye.\n";
        let (out, n) = scrub_prompt_echo_text(md, "Silence test");
        assert_eq!(n, 2);
        assert!(out.contains("[SPEAKER_1 0:00] Hello everyone.") && out.contains("[SPEAKER_1 5:00] Thanks, bye."));
        assert!(out.contains("[UNKNOWN 4:45] Silence test."), "one title-only line may be real speech");
        assert!(out.contains("Preserve spelling exactly when heard is in my notes."), "notes are never touched");
        let twice = "---\ntitle: Standup\n---\n## Transcript\n[A 0:01] Standup.\n[A 0:30] Standup.\n[B 0:40] Real words.\n";
        let (out, n) = scrub_prompt_echo_text(twice, "Standup");
        assert_eq!(n, 2);
        assert!(out.ends_with("[B 0:40] Real words.\n"));
        assert_eq!(scrub_prompt_echo_text("no transcript", "x"), ("no transcript".to_string(), 0));
        let collapsed = "---\ntitle: T\n---\n## Transcript\n[U 1:15] T. Preserve spelling exactly when hearing.\n[...] [repeated audio removed - 7 identical segments collapsed]\n[A 2:00] Real.\n[...] [repeated audio removed - 2 identical segments collapsed]\n";
        let (out, n) = scrub_prompt_echo_text(collapsed, "T");
        assert_eq!(n, 2, "the echo and its marker");
        assert!(out.contains("[A 2:00] Real.\n[...] [repeated audio removed - 2"), "a marker after real speech stays");
    }

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
    fn splice_skips_headings_in_code_blocks() {
        let file = "---\ntitle: T\n---\n\n## Notes\n\n```\n## not a heading\n```\nold tail\n\n## Transcript\n\n[0:00] hi\n";
        let out = splice_notes(file, "new");
        assert_eq!(out, "---\ntitle: T\n---\n\n## Notes\n\nnew\n\n## Transcript\n\n[0:00] hi\n");
        // Saving the same notes twice changes nothing.
        let (_, body) = split_frontmatter(file);
        let notes = sections(body).into_iter().find(|s| s.heading == "Notes").unwrap().body;
        let once = splice_notes(file, &notes);
        assert_eq!(splice_notes(&once, &notes), once);
        assert_eq!(once.matches("old tail").count(), 1);
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
    fn action_item_tick_checks_the_task_under_the_lock() {
        let body = "## Action Items\n- [ ] send the deck\n- [ ] book the room\n";
        let none = serde_yaml::Value::Null;
        assert!(task_at(&none, body, 1, "book the room"));
        // A new summary put another item at index 1.
        let after = "## Action Items\n- [ ] book the room\n- [ ] call Priya\n";
        assert!(!task_at(&none, after, 1, "book the room"));
        assert!(!task_at(&none, after, 5, "book the room"));
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

    const HALLUCINATION_MEETING: &str = "---\ntitle: Royal Descents Global Zoos Update\ntype: meeting\ndate: 2026-11-26T20:57:34+05:30\nduration: 3s\nstatus: complete\nentities:\n  projects:\n  - slug: royal-descent-zoos-across\n    label: Royal Descent Zoos Across\n    aliases:\n    - royal descent zoos across\n---\n\n## Notes\n\nmy note\n\n## Transcript\n\n[MICROPHONE 0:00] ...of royal descent in zoos across the world.\n[SPEAKER_1 0:00] [SYSTEM] Продолжение следует...\n";

    fn spoken_lines(content: &str) -> Vec<String> {
        let (_, body) = split_frontmatter(content);
        parse_transcript(body).into_iter().map(|t| t.text).collect()
    }

    fn temp_meeting(name: &str, content: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("margin-halluc-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("m.md");
        std::fs::write(&file, content).unwrap();
        file
    }

    #[test]
    fn the_silence_hallucination_meeting_loses_both_lines() {
        let (out, n) = scrub_prompt_echo_text(HALLUCINATION_MEETING, "Royal Descents Global Zoos Update");
        assert_eq!(n, 2);
        assert!(spoken_lines(&out).is_empty());
        assert!(out.contains("## Notes\n\nmy note"));
    }

    #[test]
    fn a_meeting_with_nothing_real_left_is_marked_no_speech_with_a_plain_title() {
        let file = temp_meeting("mark", HALLUCINATION_MEETING);
        let removed = scrub_prompt_echo(&file).unwrap();
        assert_eq!(removed, 2);
        assert!(write_no_speech(&file, true, removed).unwrap());
        let content = std::fs::read_to_string(&file).unwrap();
        let fm: minutes_core::markdown::Frontmatter = serde_yaml::from_str(split_frontmatter(&content).0).unwrap();
        assert_eq!(fm.title, "No speech recorded");
        assert_eq!(fm.status, Some(minutes_core::markdown::OutputStatus::NoSpeech));
        assert_eq!(fm.processing_warnings.len(), 1);
        assert_eq!(fm.processing_warnings[0].reason, "no_speech");
        assert!(fm.entities.is_empty());
        assert!(content.contains("## Notes\n\nmy note"));
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    #[test]
    fn core_trailing_noise_marker_alone_is_a_silent_recording() {
        let silent = "---\ntitle: QA silent\ntype: meeting\ndate: 2026-11-27\nduration: 6s\nstatus: transcript-only\n---\n\n## Transcript\n\n[Recording ended - 1 lines of trailing noise removed]\n";
        let file = temp_meeting("noise-marker", silent);
        let removed = scrub_prompt_echo(&file).unwrap();
        assert_eq!(removed, 1);
        assert!(write_no_speech(&file, false, removed).unwrap());
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
        let real = "---\ntitle: T\n---\n\n## Transcript\n\n[A 0:00] We ship on Friday.\n[Recording ended - 2 lines of trailing noise removed]\n";
        assert_eq!(scrub_prompt_echo_text(real, "T"), (real.to_string(), 0));
    }

    #[test]
    fn a_user_title_survives_the_no_speech_mark() {
        let file = temp_meeting("kept", &HALLUCINATION_MEETING.replace("Royal Descents Global Zoos Update", "Standup"));
        let removed = scrub_prompt_echo(&file).unwrap();
        assert!(write_no_speech(&file, false, removed).unwrap());
        let content = std::fs::read_to_string(&file).unwrap();
        let fm: minutes_core::markdown::Frontmatter = serde_yaml::from_str(split_frontmatter(&content).0).unwrap();
        assert_eq!(fm.title, "Standup");
        assert_eq!(fm.status, Some(minutes_core::markdown::OutputStatus::NoSpeech));
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    #[test]
    fn a_meeting_with_real_speech_is_not_marked() {
        let md = "---\ntitle: Planning\ntype: meeting\ndate: 2026-11-26\nduration: 10m\n---\n\n## Transcript\n\n[SPEAKER_1 0:00] We ship on Friday.\n[SPEAKER_1 0:05] Продолжение следует...\n";
        let file = temp_meeting("real", md);
        let removed = scrub_prompt_echo(&file).unwrap();
        assert_eq!(removed, 1);
        assert!(!write_no_speech(&file, true, removed).unwrap());
        assert!(std::fs::read_to_string(&file).unwrap().contains("title: Planning"));
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    #[test]
    fn an_empty_transcript_that_the_scrub_did_not_touch_is_left_alone() {
        let md = "---\ntitle: Planning\ntype: meeting\ndate: 2026-11-26\nduration: 10m\n---\n\n## Transcript\n\n";
        let file = temp_meeting("empty", md);
        assert!(!write_no_speech(&file, true, 0).unwrap());
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    #[test]
    fn a_lone_thank_you_is_silence_but_a_thank_you_in_a_real_meeting_stays() {
        let silent = "---\ntitle: T\n---\n\n## Transcript\n\n[SPEAKER_1 0:00] Thank you.\n";
        let (out, n) = scrub_prompt_echo_text(silent, "T");
        assert_eq!((n, spoken_lines(&out).len()), (1, 0));
        let real = "---\ntitle: T\n---\n\n## Transcript\n\n[SPEAKER_1 0:00] We are done with the budget.\n[SPEAKER_2 0:09] Thank you.\n";
        let (out, n) = scrub_prompt_echo_text(real, "T");
        assert_eq!((n, spoken_lines(&out).len()), (0, 2));
    }

    #[test]
    fn known_phrases_and_markers_go_in_any_language() {
        let md = "---\ntitle: T\n---\n\n## Transcript\n\n[A 0:00] Let us start with the roadmap.\n[A 0:10] Thanks for watching!\n[A 0:12] Субтитры сделал DimaTorzok\n[A 0:14] [BLANK_AUDIO]\n[A 0:16] (music)\n[A 0:18] 字幕由 Amara.org 社区提供\n[A 0:20] [SYSTEM] [BLANK_AUDIO]\n[A 0:22] Then we agree on Friday.\n";
        let (out, n) = scrub_prompt_echo_text(md, "T");
        assert_eq!(n, 6);
        assert_eq!(spoken_lines(&out), ["Let us start with the roadmap.", "Then we agree on Friday."]);
    }

    #[test]
    fn a_source_tag_is_not_a_hallucination() {
        let md = "---\ntitle: T\n---\n\n## Transcript\n\n[SPEAKER_0 0:00] [MICROPHONE] Can you hear me?\n[SPEAKER_1 0:03] [SYSTEM] Yes, loud and clear.\n";
        assert_eq!(scrub_prompt_echo_text(md, "T").1, 0);
    }

    #[test]
    fn a_stray_line_in_a_foreign_script_goes_and_a_real_share_stays() {
        let english: String = (0..9).map(|i| format!("[A 0:{i:02}] This is a sentence number {i} about the launch.\n")).collect();
        let stray = format!("---\ntitle: T\n---\n\n## Transcript\n\n{english}[A 0:30] Это совсем другой язык.\n");
        let (out, n) = scrub_prompt_echo_text(&stray, "T");
        assert_eq!((n, spoken_lines(&out).len()), (1, 9));

        let mixed = format!("---\ntitle: T\n---\n\n## Transcript\n\n{english}[A 0:30] यह पूरी बात ठीक है।\n[A 0:31] ಇದು ಸರಿಯಾಗಿದೆ.\n[A 0:32] இது சரி.\n");
        assert_eq!(scrub_prompt_echo_text(&mixed, "T").1, 0);

        let one_hindi_line = format!("---\ntitle: T\n---\n\n## Transcript\n\n{english}[A 0:30] धन्यवाद सब लोग।\n");
        assert_eq!(scrub_prompt_echo_text(&one_hindi_line, "T").1, 0, "ten lines is too few to call one Devanagari line stray");

        let mostly_telugu: String = (0..6).map(|i| format!("[A 1:{i:02}] ఇది సమావేశం భాగం {i}.\n")).collect();
        let md = format!("---\ntitle: T\n---\n\n## Transcript\n\n{mostly_telugu}[A 2:00] Okay.\n");
        assert_eq!(scrub_prompt_echo_text(&md, "T").1, 0);
    }

    #[test]
    fn titles_never_end_with_dots() {
        let cut = "I Have Kept People in Love, I Am Giving You a Split in Ca...";
        assert!(is_core_cut_title(cut));
        assert_eq!(tidy_title(cut), "I Have Kept People in Love, I Am Giving You a Split");
        assert_eq!(tidy_title("Wait for it..."), "Wait for it");
        assert_eq!(tidy_title("Warranty flow…"), "Warranty flow");
        assert_eq!(tidy_title("Q3 Pricing Review"), "Q3 Pricing Review");
        assert_eq!(tidy_title("..."), "");
        assert!(!tidy_title(cut).ends_with('.'));
    }

    #[test]
    fn a_core_cut_title_reads_whole_in_the_library_and_in_the_file() {
        let cut = "I Have Kept People in Love, I Am Giving You a Split in Ca...";
        let md = format!("---\ntitle: {cut}\ntype: meeting\ndate: 2026-11-26\nduration: 1m\n---\n\n## Transcript\n\n[A 0:00] hello there\n");
        let file = temp_meeting("cut", &md);
        assert_eq!(summarize_file(&file).unwrap().title, "I Have Kept People in Love, I Am Giving You a Split");
        assert!(tidy_title_in_file(&file).unwrap());
        assert!(std::fs::read_to_string(&file).unwrap().contains("Giving You a Split\n"));
        assert!(!tidy_title_in_file(&file).unwrap());
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    fn corpus(words: &[(&str, usize)]) -> HashMap<String, usize> {
        words.iter().map(|(w, n)| (w.to_string(), *n)).collect()
    }

    #[test]
    fn a_misspelled_word_finds_the_closest_library_word() {
        let c = corpus(&[("warranty", 5), ("warrant", 1), ("pricing", 3), ("launch", 2), ("wiring", 1)]);
        assert_eq!(corrected_query_in("warrnty", &c).as_deref(), Some("warranty"));
        assert_eq!(corrected_query_in("waranty flow", &c).as_deref(), Some("warranty flow"));
        assert_eq!(corrected_query_in("prcing", &c).as_deref(), Some("pricing"));
        assert_eq!(corrected_query_in("WARRNTY", &c).as_deref(), Some("warranty"));
    }

    #[test]
    fn a_word_too_far_off_or_too_short_or_known_is_not_corrected() {
        let c = corpus(&[("warranty", 5), ("pricing", 3), ("the", 9)]);
        assert_eq!(corrected_query_in("zebra", &c), None);
        assert_eq!(corrected_query_in("warrxxxy", &c), None);
        assert_eq!(corrected_query_in("pricing", &c), None);
        assert_eq!(corrected_query_in("tha", &c).as_deref(), Some("the"));
        assert_eq!(corrected_query_in("th", &c), None);
        assert_eq!(corrected_query_in("q3", &c), None);
    }

    #[test]
    fn edit_distance_counts_swaps_once_and_stops_at_the_limit() {
        let d = |a: &str, b: &str, l| edit_distance(&a.chars().collect::<Vec<_>>(), &b.chars().collect::<Vec<_>>(), l);
        assert_eq!(d("warranty", "warranty", 2), Some(0));
        assert_eq!(d("warrnty", "warranty", 2), Some(1));
        assert_eq!(d("wrarnty", "warranty", 2), Some(2));
        assert_eq!(d("abcdef", "uvwxyz", 2), None);
        assert_eq!(d("pricnig", "pricing", 1), Some(1));
    }

    #[test]
    fn search_results_serialize_with_a_corrected_field() {
        let none = serde_json::to_value(SearchResults { hits: vec![], corrected: None }).unwrap();
        assert_eq!(none, serde_json::json!({ "hits": [], "corrected": null }));
        let some = serde_json::to_value(SearchResults { hits: vec![SearchHit { path: "/a.md".into(), excerpt: "x".into() }], corrected: Some("warranty".into()) }).unwrap();
        assert_eq!(some["corrected"], "warranty");
        assert_eq!(some["hits"][0]["path"], "/a.md");
    }
}
