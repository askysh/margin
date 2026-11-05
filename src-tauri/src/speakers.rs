//! Rename a speaker everywhere in a meeting, and optionally remember their voice
//! (DECISIONS.md D7).
//!
//! Order matters. Every file write happens first, then the overlay confirmation, because
//! upstream binds that confirmation to the file's sha256.

use crate::{corecfg, library};
use minutes_core::diarize::{AttributionSource, Confidence, SpeakerAttribution};
use minutes_core::markdown::{split_frontmatter, update_frontmatter};
use minutes_core::voice::{self, TrustClass, VoiceSampleInput};
use regex::Regex;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct RenameResult {
    pub voice_saved: bool,
    pub note: Option<String>,
}

pub fn rename(path: &Path, label: &str, new_name: &str, remember_voice: bool) -> Result<RenameResult, String> {
    library::ensure_inside_output(path)?;
    let new_name = new_name.trim();
    if new_name.is_empty() {
        return Err("Enter a name.".into());
    }
    if new_name.contains(['[', ']', '\n']) {
        return Err("Names can't contain brackets or line breaks.".into());
    }
    if new_name == label {
        return Ok(RenameResult { voice_saved: false, note: None });
    }

    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let (fm_text, _) = split_frontmatter(&content);
    let fm: serde_yaml::Value = serde_yaml::from_str(fm_text).unwrap_or(serde_yaml::Value::Null);
    // The diarization label (SPEAKER_n) that voice embeddings are keyed by.
    let raw_label = library::speaker_map(&fm)
        .into_iter()
        .find(|(raw, name)| name == label || raw == label)
        .map(|(raw, _)| raw)
        .unwrap_or_else(|| label.to_string());
    let (speech_seconds, segments) = speech_stats(&content, label);

    // 1. The body: transcript labels, plus whole-word mentions outside the user's notes.
    let updated = rename_in_body(&content, label, new_name);
    if updated != content {
        crate::fsutil::write_atomic(path, updated.as_bytes(), None).map_err(|e| e.to_string())?;
    }

    // 2. Frontmatter: speaker_map and attendees.
    update_frontmatter(path, |fm| {
        match fm
            .speaker_map
            .iter_mut()
            .find(|a| a.speaker_label == raw_label || a.name == label)
        {
            Some(entry) => {
                entry.name = new_name.to_string();
                entry.confidence = Confidence::High;
                entry.source = AttributionSource::Manual;
            }
            None => fm.speaker_map.push(SpeakerAttribution {
                speaker_label: raw_label.clone(),
                name: new_name.to_string(),
                confidence: Confidence::High,
                source: AttributionSource::Manual,
            }),
        }
        fm.attendees.retain(|a| a != label);
        if !fm.attendees.iter().any(|a| a == new_name) {
            fm.attendees.push(new_name.to_string());
        }
        // Summaries assign action items and commitments to labels too.
        let is_label = |s: &str| s.trim_start_matches('@') == label;
        for item in fm.action_items.iter_mut().filter(|i| is_label(&i.assignee)) {
            item.assignee = new_name.to_string();
        }
        for intent in fm.intents.iter_mut() {
            if intent.who.as_deref().is_some_and(is_label) {
                intent.who = Some(new_name.to_string());
            }
        }
        if fm.people.iter().any(|p| p == label) {
            fm.people.retain(|p| p != label);
            if !fm.people.iter().any(|p| p == new_name) {
                fm.people.push(new_name.to_string());
            }
        }
    })
    .map_err(|e| format!("Could not update the meeting header: {e}"))?;

    // 3. The upstream overlay row, so the CLI and MCP see the confirmation too.
    if let Err(error) = minutes_core::overlays::write_speaker_confirmation(path, &raw_label, new_name, Some(label), None) {
        tracing::warn!(%error, "overlay confirmation not written");
    }

    // 4. Voice memory.
    if !remember_voice {
        return Ok(RenameResult { voice_saved: false, note: None });
    }
    let Some(embedding) = voice::load_meeting_embeddings(path).and_then(|mut m| m.remove(&raw_label)) else {
        return Ok(RenameResult {
            voice_saved: false,
            note: Some("Renamed. This meeting has no voice data for that speaker, so the voice wasn't saved.".into()),
        });
    };
    let cfg = corecfg::load();
    let conn = voice::open_db().map_err(|e| e.to_string())?;
    voice::insert_voice_sample(
        &conn,
        &VoiceSampleInput {
            person_slug: voice::person_slug(new_name),
            name: new_name.to_string(),
            embedding,
            model_id: voice::model_version(&cfg).to_string(),
            trust_class: TrustClass::ManuallyConfirmed,
            meeting_path: Some(path.display().to_string()),
            sidecar_speaker: Some(raw_label),
            capture_source: Some("margin-rename".into()),
            speech_seconds,
            segment_count: segments,
            quality_json: None,
            similarity: None,
            top2_margin: None,
            threshold_version: None,
            sensitivity: "normal".into(),
            created_at: None,
        },
    )
    .map_err(|e| format!("Renamed, but the voice couldn't be saved: {e}"))?;
    Ok(RenameResult { voice_saved: true, note: None })
}

fn rename_in_body(content: &str, label: &str, new_name: &str) -> String {
    let (_, body) = split_frontmatter(content);
    let head = &content[..content.len() - body.len()];
    let tag = format!("[{label} ");
    let word = Regex::new(&format!(r"\b{}\b", regex::escape(label))).unwrap();
    let mut out = String::from(head);
    let mut section = String::new();
    for line in body.split_inclusive('\n') {
        if let Some(h) = line.strip_prefix("## ") {
            section = h.trim().to_string();
            out.push_str(line);
            continue;
        }
        match section.as_str() {
            "Transcript" => match line.strip_prefix(&tag) {
                Some(rest) => {
                    out.push('[');
                    out.push_str(new_name);
                    out.push(' ');
                    out.push_str(rest);
                }
                None => out.push_str(line),
            },
            // The user's own words stay as typed.
            "Notes" => out.push_str(line),
            _ => out.push_str(&word.replace_all(line, regex::NoExpand(new_name))),
        }
    }
    out
}

/// Rough seconds of speech for a label: time from each of its turns to the next turn.
fn speech_stats(content: &str, label: &str) -> (f64, u32) {
    let re = Regex::new(r"^\[(?:(.+?) )?(\d{1,2}:\d{2}(?::\d{2})?)\]").unwrap();
    let turns: Vec<(Option<String>, f64)> = content
        .lines()
        .filter_map(|l| {
            let c = re.captures(l)?;
            Some((c.get(1).map(|m| m.as_str().to_string()), parse_ts(&c[2])))
        })
        .collect();
    let mut seconds = 0.0;
    let mut count = 0;
    for (i, (speaker, ts)) in turns.iter().enumerate() {
        if speaker.as_deref() == Some(label) {
            count += 1;
            let next = turns.get(i + 1).map(|t| t.1).unwrap_or(ts + 5.0);
            seconds += (next - ts).clamp(0.5, 30.0);
        }
    }
    (seconds, count)
}

fn parse_ts(ts: &str) -> f64 {
    ts.split(':').fold(0.0, |acc, part| acc * 60.0 + part.parse::<f64>().unwrap_or(0.0))
}

#[derive(Debug, Serialize)]
pub struct VoiceProfileView {
    pub slug: String,
    pub name: String,
    pub samples: u32,
}

pub fn list_voices() -> Vec<VoiceProfileView> {
    let Ok(conn) = voice::open_db() else { return vec![] };
    voice::list_profiles_for_display(&conn)
        .unwrap_or_default()
        .into_iter()
        .map(|p| VoiceProfileView { slug: p.person_slug, name: p.name, samples: p.sample_count })
        .collect()
}

pub fn forget_voice(slug: &str) -> Result<(), String> {
    let conn = voice::open_db().map_err(|e| e.to_string())?;
    voice::delete_profile(&conn, slug).map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renames_everywhere_but_notes() {
        let file = "---\ntitle: T\n---\n\n## Summary\n\nSPEAKER_1 owns pricing.\n\n## Notes\n\n- ask SPEAKER_1\n\n## Transcript\n\n[SPEAKER_1 0:00] hi\n[SPEAKER_10 0:05] no\n";
        let out = rename_in_body(file, "SPEAKER_1", "Priya");
        assert!(out.contains("Priya owns pricing."));
        assert!(out.contains("- ask SPEAKER_1"));
        assert!(out.contains("[Priya 0:00] hi"));
        assert!(out.contains("[SPEAKER_10 0:05] no"));
    }

    #[test]
    fn speech_stats_counts_turns() {
        let file = "[A 0:00] x\n[B 0:10] y\n[A 0:20] z\n";
        let (secs, n) = speech_stats(file, "A");
        assert_eq!(n, 2);
        assert!((secs - 15.0).abs() < 0.01);
    }
}
