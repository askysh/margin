//! The meeting library: plain Markdown files with YAML frontmatter in the user's folder.
//! We read them directly (no index), and edit only the `## Notes` section in place. The
//! frontmatter bytes and every other section are kept exactly (RESEARCH.md §1.5).

use crate::corecfg;
use minutes_core::markdown::split_frontmatter;
use regex::Regex;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

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
}

pub fn output_dir() -> PathBuf {
    corecfg::load().output_dir
}

pub fn list() -> Vec<MeetingSummary> {
    let root = output_dir();
    let mut files = Vec::new();
    collect(&root, &root, &mut files);
    let mut out: Vec<MeetingSummary> = files.iter().filter_map(|p| summarize_file(p)).collect();
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

fn summarize_file(path: &Path) -> Option<MeetingSummary> {
    let content = std::fs::read_to_string(path).ok()?;
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

fn turn_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\[(?:(.+?) )?(\d{1,2}:\d{2}(?::\d{2})?)\]\s?(.*)$").unwrap())
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
    let attendees = fm
        .get("attendees")
        .and_then(|v| v.as_sequence())
        .map(|s| s.iter().filter_map(|a| a.as_str().map(String::from)).collect())
        .unwrap_or_default();
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
    })
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

pub fn splice_notes(content: &str, notes: &str) -> String {
    let (_, body) = split_frontmatter(content);
    let head = &content[..content.len() - body.len()];
    // A level-2 heading typed into notes would split the section on the next read.
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

#[derive(Debug, Serialize)]
pub struct SearchHit {
    pub path: String,
    pub excerpt: String,
}

/// Case-insensitive search over whole files. Fine for thousands of meetings, and it takes
/// no lock, unlike core's per-query FTS index.
pub fn search(query: &str) -> Vec<SearchHit> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return vec![];
    }
    list()
        .into_iter()
        .filter_map(|m| {
            let content = std::fs::read_to_string(&m.path).ok()?;
            let (_, body) = split_frontmatter(&content);
            let in_title = m.title.to_lowercase().contains(&q);
            let line = body.lines().find(|l| l.to_lowercase().contains(&q));
            if !in_title && line.is_none() {
                return None;
            }
            Some(SearchHit {
                path: m.path,
                excerpt: line.map(plain_line).unwrap_or_default(),
            })
        })
        .collect()
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
    let mut items = vec![path.to_path_buf()];
    items.extend(
        minutes_core::capture::meeting_audio_artifact_paths(path)
            .into_iter()
            .filter(|p| p.exists()),
    );
    trash::delete_all(&items).map_err(|e| format!("Could not move to Trash: {e}"))
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
}
