//! Settings that live outside config.toml:
//!
//! - **Vocabulary**: names and words to spell right. minutes-core reads them from its
//!   `vocabulary.toml` (in Margin's state directory) and offers them to whisper as hints.
//! - **Summary instructions**: written as a project template that overrides core's bundled
//!   "meeting" template. Core looks for project templates in `<cwd>/.minutes/templates/`,
//!   and Margin runs with the state directory as its working directory (`use_state_cwd`),
//!   so nothing is written to the shared `~/.minutes` (DECISIONS.md, settings parity).
//! - **Open at login**: a LaunchAgent in `~/Library/LaunchAgents`. Not on Windows (D61):
//!   a Store package needs a startup task in its manifest, which Margin does not have yet.
//!   There the setting reads as off, a save leaves it alone, and Settings does not show it.
//! - **Transcribe during the meeting**: Margin's prefs.json (rolling.rs).

use crate::{corecfg::AppSettings, paths};
use minutes_core::template::TemplateFrontmatter;
use minutes_core::vocabulary::{self, VocabularyEntry, VocabularyKind, VocabularyPriority, VocabularySource, VocabularyStore};
use std::path::PathBuf;

/// The bundled template Margin's instructions are layered on. Using the same slug means a
/// meeting's `template: meeting` still resolves in any other Minutes install.
pub const TEMPLATE_SLUG: &str = "meeting";
const LOGIN_LABEL: &str = "com.akashmishra.margin.login";
pub const LOGIN_ARG: &str = "--at-login";
const MAX_INSTRUCTIONS: usize = 2000;
const MAX_TERMS: usize = 100;

/// Fill in the fields of `s` that are stored here.
pub fn fill(mut s: AppSettings) -> AppSettings {
    s.vocabulary = Some(vocabulary_terms());
    s.summary_instructions = Some(summary_instructions());
    s.open_at_login = Some(open_at_login());
    s.rolling_transcription = Some(crate::rolling::pref());
    s
}

/// Save the fields of `s` that are stored here. A field that is `None` is left alone.
pub fn apply(s: &AppSettings) -> Result<(), String> {
    if let Some(terms) = &s.vocabulary {
        set_vocabulary(terms)?;
    }
    if let Some(text) = &s.summary_instructions {
        set_summary_instructions(text)?;
    }
    if let Some(on) = s.open_at_login {
        set_open_at_login(on)?;
    }
    if let Some(on) = s.rolling_transcription {
        crate::rolling::set_pref(on)?;
    }
    Ok(())
}

/// Margin runs from the state directory so core finds the summary template there.
pub fn use_state_cwd() {
    let dir = paths::state_dir();
    if let Err(error) = std::env::set_current_dir(&dir) {
        tracing::warn!(%error, dir = %dir.display(), "could not change the working directory");
    }
}

// ---------------------------------------------------------------- vocabulary

/// The terms you typed in Settings. Entries that core or the upstream app added for other
/// reasons (a speaker confirmation, an import) are not shown and are never removed.
pub fn vocabulary_terms() -> Vec<String> {
    vocabulary::load()
        .map(|store| store.entries.into_iter().filter(is_ours).map(|e| e.canonical).collect())
        .unwrap_or_default()
}

pub fn set_vocabulary(terms: &[String]) -> Result<(), String> {
    let mut store = vocabulary::load().unwrap_or_else(|error| {
        tracing::warn!(%error, "vocabulary unreadable; starting a new one");
        VocabularyStore::empty()
    });
    store.entries.retain(|e| !is_ours(e));
    store.entries.extend(clean_terms(terms).into_iter().map(|term| {
        let mut entry = VocabularyEntry::new(VocabularyKind::Term, term);
        entry.priority = VocabularyPriority::High;
        entry.source = VocabularySource::Manual;
        entry
    }));
    vocabulary::save_at(&vocabulary::default_path(), &store).map_err(|e| format!("Couldn't save the word list: {e}"))
}

fn is_ours(entry: &VocabularyEntry) -> bool {
    entry.source == VocabularySource::Manual
}

/// One term per line or comma, trimmed, without duplicates (ignoring case).
fn clean_terms(terms: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for term in terms.iter().flat_map(|t| t.split([',', '\n'])) {
        let term = term.trim();
        if !term.is_empty() && !out.iter().any(|t| t.eq_ignore_ascii_case(term)) {
            out.push(term.chars().take(80).collect());
        }
    }
    out.truncate(MAX_TERMS);
    out
}

// ---------------------------------------------------------------- summary instructions

fn template_path() -> PathBuf {
    paths::state_dir().join(".minutes/templates").join(format!("{TEMPLATE_SLUG}.md"))
}

pub fn summary_instructions() -> String {
    minutes_core::template::Template::load_file(&template_path(), minutes_core::template::TemplateSource::Project)
        .map(|t| t.frontmatter.additional_instructions)
        .unwrap_or_default()
}

/// The template slug to queue a meeting with, or `None` when there are no instructions.
pub fn summary_template_slug() -> Option<String> {
    (!summary_instructions().trim().is_empty()).then(|| TEMPLATE_SLUG.to_string())
}

pub fn set_summary_instructions(text: &str) -> Result<(), String> {
    let text = text.trim();
    let path = template_path();
    if text.is_empty() {
        // Meetings that recorded `template: meeting` fall back to core's bundled one.
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        };
    }
    if text.chars().count() > MAX_INSTRUCTIONS {
        return Err(format!("Keep summary instructions under {MAX_INSTRUCTIONS} characters."));
    }
    std::fs::create_dir_all(path.parent().expect("template dir")).map_err(|e| e.to_string())?;
    crate::fsutil::write_atomic(&path, template_text(text)?.as_bytes(), Some(0o600))
        .map_err(|e| format!("Couldn't save the summary instructions: {e}"))
}

fn template_text(instructions: &str) -> Result<String, String> {
    let fm = TemplateFrontmatter {
        name: "Your instructions".into(),
        slug: TEMPLATE_SLUG.into(),
        version: "1.0.0".into(),
        description: "Summary instructions from Margin's Settings.".into(),
        keywords: vec![],
        extends_base: true,
        additional_instructions: instructions.into(),
        language: None,
    };
    let yaml = serde_yaml::to_string(&fm).map_err(|e| e.to_string())?;
    Ok(format!("---\n{yaml}---\n\nWritten by Margin. Edit it in Settings → Summaries.\n"))
}

// ---------------------------------------------------------------- open at login

fn login_plist_path() -> PathBuf {
    paths::home().join("Library/LaunchAgents").join(format!("{LOGIN_LABEL}.plist"))
}

pub fn open_at_login() -> bool {
    if !cfg!(target_os = "macos") {
        return false;
    }
    login_plist_path().exists()
}

pub fn set_open_at_login(on: bool) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Ok(());
    }
    let path = login_plist_path();
    if !on {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("Couldn't turn off open at login: {e}")),
            _ => Ok(()),
        };
    }
    std::fs::create_dir_all(path.parent().expect("LaunchAgents dir")).map_err(|e| e.to_string())?;
    crate::fsutil::write_atomic(&path, login_plist().as_bytes(), Some(0o644))
        .map_err(|e| format!("Couldn't turn on open at login: {e}"))
}

/// Opens Margin by bundle ID, so the item keeps working if the app is moved. The extra
/// argument tells Margin to start in the menu bar without showing its window.
fn login_plist() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LOGIN_LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>/usr/bin/open</string>
    <string>-b</string>
    <string>com.akashmishra.margin</string>
    <string>--args</string>
    <string>{LOGIN_ARG}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
</dict>
</plist>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terms_are_split_trimmed_and_deduplicated() {
        let got = clean_terms(&["Priya, Kubernetes\n  priya ".into(), "".into(), "SOC 2".into()]);
        assert_eq!(got, vec!["Priya", "Kubernetes", "SOC 2"]);
    }

    #[test]
    fn our_terms_become_decoder_hints() {
        let store = VocabularyStore {
            entries: clean_terms(&["Margin, Akash".into()])
                .into_iter()
                .map(|t| {
                    let mut e = VocabularyEntry::new(VocabularyKind::Term, t);
                    e.priority = VocabularyPriority::High;
                    e
                })
                .collect(),
        }
        .normalized()
        .unwrap();
        assert_eq!(store.decode_phrases(8), vec!["Akash", "Margin"]);
    }

    #[test]
    fn template_round_trips_through_core() {
        let text = template_text("Put action items first.\nUse \"quotes\": fine.").unwrap();
        let t = minutes_core::template::Template::from_str(&text, minutes_core::template::TemplateSource::Project, None, "test").unwrap();
        assert_eq!(t.frontmatter.slug, "meeting");
        assert_eq!(t.frontmatter.additional_instructions, "Put action items first.\nUse \"quotes\": fine.");
        let prompt = minutes_core::template::compose_additional_instructions("BASE", Some(&t));
        assert!(prompt.contains("Put action items first."), "{prompt}");
    }

    #[test]
    fn login_plist_is_well_formed() {
        let plist = login_plist();
        assert!(plist.contains("<string>com.akashmishra.margin.login</string>"));
        assert!(plist.contains("<string>--at-login</string>"));
        assert_eq!(plist.matches("<dict>").count(), plist.matches("</dict>").count());
    }
}
