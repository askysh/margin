//! The minutes-core config file, plus the small slice of it that Margin's settings expose.
//!
//! We always write keys explicitly instead of relying on core defaults. One reason: if
//! `summarization.engine` is missing, core silently forces it to "auto", which runs
//! whatever agent CLI is installed (RESEARCH.md §1.6).

use crate::paths;
use minutes_core::config::Config;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const MODELS: &[(&str, &str, u64)] = &[
    // (id, label, approximate bytes)
    ("base", "Fast", 147_951_465),
    ("small", "Balanced", 487_601_967),
    ("large-v3-turbo", "Best", 1_624_555_275),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub output_dir: String,
    pub model: String,
    /// "none" | "claude" | "ollama"
    pub summary_engine: String,
    pub claude_path: Option<String>,
    pub ollama_model: String,
    pub user_name: String,
    // The fields below are optional so that a caller which does not know them (an older
    // onboarding screen, say) leaves them unchanged. `settings()` always fills them.
    /// Whisper language code, or "auto".
    #[serde(default)]
    pub transcription_language: Option<String>,
    /// An English language name such as "French", or "auto" (follow the meeting).
    #[serde(default)]
    pub summary_language: Option<String>,
    /// An input device name, or "" for the system default.
    #[serde(default)]
    pub input_device: Option<String>,
    #[serde(default)]
    pub speaker_separation: Option<bool>,
    /// Minutes of silence before a recording stops by itself. 0 means never.
    #[serde(default)]
    pub silence_stop_minutes: Option<u64>,
    /// Days to keep meeting audio. 0 means keep it forever.
    #[serde(default)]
    pub keep_audio_days: Option<u32>,
    #[serde(default)]
    pub call_detection: Option<bool>,
    // Kept outside config.toml (see extras.rs).
    #[serde(default)]
    pub vocabulary: Option<Vec<String>>,
    #[serde(default)]
    pub summary_instructions: Option<String>,
    #[serde(default)]
    pub open_at_login: Option<bool>,
}

/// Choices offered for "Stop after silence", in minutes. 30 is core's default.
pub const SILENCE_STOP_CHOICES: &[u64] = &[0, 5, 15, 30, 60];
/// Choices offered for "Keep audio", in days. 0 means forever.
pub const KEEP_AUDIO_CHOICES: &[u32] = &[0, 30, 7];

/// The languages whisper accepts, as (code, English name).
pub const LANGUAGES: &[(&str, &str)] = &[
    ("af", "Afrikaans"), ("am", "Amharic"), ("ar", "Arabic"), ("as", "Assamese"), ("az", "Azerbaijani"),
    ("ba", "Bashkir"), ("be", "Belarusian"), ("bg", "Bulgarian"), ("bn", "Bengali"), ("bo", "Tibetan"),
    ("br", "Breton"), ("bs", "Bosnian"), ("ca", "Catalan"), ("cs", "Czech"), ("cy", "Welsh"),
    ("da", "Danish"), ("de", "German"), ("el", "Greek"), ("en", "English"), ("es", "Spanish"),
    ("et", "Estonian"), ("eu", "Basque"), ("fa", "Persian"), ("fi", "Finnish"), ("fo", "Faroese"),
    ("fr", "French"), ("gl", "Galician"), ("gu", "Gujarati"), ("ha", "Hausa"), ("haw", "Hawaiian"),
    ("he", "Hebrew"), ("hi", "Hindi"), ("hr", "Croatian"), ("ht", "Haitian Creole"), ("hu", "Hungarian"),
    ("hy", "Armenian"), ("id", "Indonesian"), ("is", "Icelandic"), ("it", "Italian"), ("ja", "Japanese"),
    ("jw", "Javanese"), ("ka", "Georgian"), ("kk", "Kazakh"), ("km", "Khmer"), ("kn", "Kannada"),
    ("ko", "Korean"), ("la", "Latin"), ("lb", "Luxembourgish"), ("ln", "Lingala"), ("lo", "Lao"),
    ("lt", "Lithuanian"), ("lv", "Latvian"), ("mg", "Malagasy"), ("mi", "Maori"), ("mk", "Macedonian"),
    ("ml", "Malayalam"), ("mn", "Mongolian"), ("mr", "Marathi"), ("ms", "Malay"), ("mt", "Maltese"),
    ("my", "Myanmar"), ("ne", "Nepali"), ("nl", "Dutch"), ("nn", "Norwegian Nynorsk"), ("no", "Norwegian"),
    ("oc", "Occitan"), ("pa", "Punjabi"), ("pl", "Polish"), ("ps", "Pashto"), ("pt", "Portuguese"),
    ("ro", "Romanian"), ("ru", "Russian"), ("sa", "Sanskrit"), ("sd", "Sindhi"), ("si", "Sinhala"),
    ("sk", "Slovak"), ("sl", "Slovenian"), ("sn", "Shona"), ("so", "Somali"), ("sq", "Albanian"),
    ("sr", "Serbian"), ("su", "Sundanese"), ("sv", "Swedish"), ("sw", "Swahili"), ("ta", "Tamil"),
    ("te", "Telugu"), ("tg", "Tajik"), ("th", "Thai"), ("tk", "Turkmen"), ("tl", "Tagalog"),
    ("tr", "Turkish"), ("tt", "Tatar"), ("uk", "Ukrainian"), ("ur", "Urdu"), ("uz", "Uzbek"),
    ("vi", "Vietnamese"), ("yi", "Yiddish"), ("yo", "Yoruba"), ("zh", "Chinese"), ("yue", "Cantonese"),
];

pub fn load() -> Config {
    let path = paths::config_path();
    if !path.exists() {
        return fresh();
    }
    Config::load_strict_from(&path).unwrap_or_else(|error| {
        tracing::warn!(%error, "config unreadable; using fresh defaults");
        fresh()
    })
}

pub fn exists() -> bool {
    paths::config_path().exists()
}

/// Defaults for a first run. Nothing is written until `save`.
pub fn fresh() -> Config {
    let mut cfg = Config::default();
    cfg.output_dir = paths::default_output_dir();
    cfg.transcription.engine = "whisper".into();
    cfg.transcription.model = "small".into();
    cfg.summarization.engine = "none".into();
    cfg.diarization.engine = "auto".into();
    cfg.identity.name = full_user_name();
    // Off in v1: core would query EventKit from the background worker, which raises a
    // Calendars prompt nobody asked for (DECISIONS.md D9).
    cfg.calendar.enabled = false;
    pin_owned_keys(&mut cfg);
    cfg
}

/// Keys Margin owns, re-asserted on every save so a hand edit cannot point core at the
/// upstream app's directories.
fn pin_owned_keys(cfg: &mut Config) {
    cfg.transcription.model_path = paths::models_dir();
    cfg.diarization.model_path = paths::diarization_models_dir();
    cfg.vault.enabled = false;
    cfg.live_transcript.recording_sidecar = true;
    if cfg.summarization.engine.trim().is_empty() {
        cfg.summarization.engine = "none".into();
    }
}

pub fn save(cfg: &mut Config) -> Result<(), String> {
    pin_owned_keys(cfg);
    if let Some(parent) = paths::config_path().parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    cfg.save_to(&paths::config_path())
        .map_err(|e| format!("Could not save settings: {e}"))
}

pub fn settings(cfg: &Config) -> AppSettings {
    let engine = match cfg.summarization.engine.as_str() {
        "agent" => "claude",
        "ollama" => "ollama",
        _ => "none",
    };
    AppSettings {
        output_dir: cfg.output_dir.display().to_string(),
        model: cfg.transcription.model.clone(),
        summary_engine: engine.into(),
        claude_path: (engine == "claude").then(|| cfg.summarization.agent_command.clone()),
        ollama_model: cfg.summarization.ollama_model.clone(),
        user_name: cfg.identity.name.clone().unwrap_or_default(),
        transcription_language: Some(cfg.transcription.language.clone().unwrap_or_else(|| "auto".into())),
        summary_language: Some(cfg.summarization.language.clone()),
        input_device: Some(cfg.recording.device.clone().unwrap_or_default()),
        speaker_separation: Some(cfg.diarization.engine != "none"),
        silence_stop_minutes: Some(cfg.recording.silence_auto_stop_secs / 60),
        keep_audio_days: Some(if cfg.retention.auto_cleanup { cfg.retention.successful_audio_days } else { 0 }),
        call_detection: Some(cfg.call_detection.enabled),
        vocabulary: None,
        summary_instructions: None,
        open_at_login: None,
    }
}

pub fn apply(cfg: &mut Config, s: &AppSettings) -> Result<(), String> {
    let out = PathBuf::from(expand_tilde(&s.output_dir));
    if !out.is_absolute() {
        return Err("Choose a folder with a full path.".into());
    }
    cfg.output_dir = out;
    if !MODELS.iter().any(|(id, _, _)| *id == s.model) {
        return Err(format!("Unknown model {}", s.model));
    }
    cfg.transcription.model = s.model.clone();
    match s.summary_engine.as_str() {
        "claude" => {
            let path = s
                .claude_path
                .clone()
                .or_else(crate::summaries::find_claude)
                .ok_or("Claude CLI not found. Install it from claude.com/claude-code, then try again.")?;
            cfg.summarization.engine = "agent".into();
            cfg.summarization.agent_command = path;
        }
        "ollama" => {
            cfg.summarization.engine = "ollama".into();
            if !s.ollama_model.trim().is_empty() {
                cfg.summarization.ollama_model = s.ollama_model.trim().into();
            }
        }
        _ => cfg.summarization.engine = "none".into(),
    }
    let name = s.user_name.trim();
    cfg.identity.name = (!name.is_empty()).then(|| name.to_string());
    apply_recording(cfg, s)?;
    apply_languages(cfg, s)?;
    if let Some(on) = s.speaker_separation {
        cfg.diarization.engine = if on { "auto" } else { "none" }.into();
    }
    if let Some(days) = s.keep_audio_days {
        apply_keep_audio(cfg, days)?;
    }
    if let Some(on) = s.call_detection {
        cfg.call_detection.enabled = on;
    }
    Ok(())
}

fn apply_recording(cfg: &mut Config, s: &AppSettings) -> Result<(), String> {
    if let Some(device) = &s.input_device {
        cfg.recording.device = minutes_core::capture::canonicalize_input_device_setting(device);
    }
    if let Some(minutes) = s.silence_stop_minutes {
        if !SILENCE_STOP_CHOICES.contains(&minutes) {
            return Err(format!("Unknown silence limit: {minutes} minutes"));
        }
        cfg.recording.silence_auto_stop_secs = minutes * 60;
    }
    Ok(())
}

fn apply_languages(cfg: &mut Config, s: &AppSettings) -> Result<(), String> {
    if let Some(code) = s.transcription_language.as_deref().map(str::trim) {
        cfg.transcription.language = match code {
            "" | "auto" => None,
            c if LANGUAGES.iter().any(|(id, _)| *id == c) => Some(c.to_string()),
            c => return Err(format!("Unknown language {c}")),
        };
    }
    if let Some(name) = s.summary_language.as_deref().map(str::trim) {
        cfg.summarization.language = match name {
            "" | "auto" => "auto".into(),
            n if LANGUAGES.iter().any(|(_, label)| *label == n) => n.to_string(),
            n => return Err(format!("Unknown summary language {n}")),
        };
    }
    Ok(())
}

/// 0 keeps audio forever. Otherwise core's retention deletes meeting audio older than
/// `days` (see `audio_cleanup`). Restricted meetings keep their own, shorter window.
fn apply_keep_audio(cfg: &mut Config, days: u32) -> Result<(), String> {
    if !KEEP_AUDIO_CHOICES.contains(&days) {
        return Err(format!("Unknown audio retention: {days} days"));
    }
    let r = &mut cfg.retention;
    r.auto_cleanup = days > 0;
    if days > 0 {
        r.successful_audio_days = days;
        r.failed_audio_days = days;
        r.restricted_audio_days = r.restricted_audio_days.min(days);
    }
    Ok(())
}

/// Delete meeting audio that is older than the "Keep audio" setting allows. Returns the
/// number of files removed. Does nothing when audio is kept forever.
pub fn audio_cleanup(cfg: &Config) -> usize {
    if !cfg.retention.auto_cleanup {
        return 0;
    }
    let plan = minutes_core::retention::preview_audio_retention(cfg, chrono::Local::now());
    let removal = minutes_core::retention::apply_audio_retention(&plan);
    for e in &removal.errors {
        tracing::warn!(path = %e.path.display(), error = %e.error, "audio cleanup");
    }
    if !removal.removed.is_empty() {
        tracing::info!(count = removal.removed.len(), "deleted old meeting audio");
    }
    removal.removed.len()
}

pub fn expand_tilde(path: &str) -> String {
    match path.strip_prefix("~/") {
        Some(rest) => paths::home().join(rest).display().to_string(),
        None if path == "~" => paths::home().display().to_string(),
        None => path.to_string(),
    }
}

/// The macOS account's full name, used as a default for "your name".
fn full_user_name() -> Option<String> {
    let out = std::process::Command::new("/usr/bin/id").arg("-F").output().ok()?;
    let name = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> (Config, AppSettings) {
        let mut cfg = Config::default();
        cfg.output_dir = PathBuf::from("/tmp/margin-test-meetings");
        cfg.transcription.model = "small".into();
        cfg.summarization.engine = "none".into();
        let s = settings(&cfg);
        (cfg, s)
    }

    #[test]
    fn new_settings_round_trip() {
        let (mut cfg, mut s) = base();
        s.transcription_language = Some("de".into());
        s.summary_language = Some("French".into());
        s.input_device = Some("USB Mic (48000Hz, 1 ch)".into());
        s.speaker_separation = Some(false);
        s.silence_stop_minutes = Some(0);
        s.keep_audio_days = Some(7);
        s.call_detection = Some(false);
        apply(&mut cfg, &s).unwrap();
        assert_eq!(cfg.transcription.language.as_deref(), Some("de"));
        assert_eq!(cfg.recording.device.as_deref(), Some("USB Mic"));
        assert_eq!(cfg.diarization.engine, "none");
        assert_eq!(cfg.recording.silence_auto_stop_secs, 0);
        assert!(cfg.retention.auto_cleanup && cfg.retention.successful_audio_days == 7);
        let back = settings(&cfg);
        assert_eq!(back.summary_language.as_deref(), Some("French"));
        assert_eq!(back.keep_audio_days, Some(7));
        assert_eq!(back.call_detection, Some(false));
        assert_eq!(back.speaker_separation, Some(false));
    }

    #[test]
    fn missing_fields_leave_config_alone() {
        let (mut cfg, mut s) = base();
        cfg.transcription.language = Some("ja".into());
        cfg.diarization.engine = "none".into();
        s.transcription_language = None;
        s.speaker_separation = None;
        s.keep_audio_days = None;
        apply(&mut cfg, &s).unwrap();
        assert_eq!(cfg.transcription.language.as_deref(), Some("ja"));
        assert_eq!(cfg.diarization.engine, "none");
        assert!(!cfg.retention.auto_cleanup);
    }

    #[test]
    fn auto_and_unknown_values() {
        let (mut cfg, mut s) = base();
        s.transcription_language = Some("auto".into());
        s.keep_audio_days = Some(0);
        apply(&mut cfg, &s).unwrap();
        assert_eq!(cfg.transcription.language, None);
        assert!(!cfg.retention.auto_cleanup);
        s.transcription_language = Some("klingon".into());
        assert!(apply(&mut cfg, &s).is_err());
        s.transcription_language = None;
        s.silence_stop_minutes = Some(7);
        assert!(apply(&mut cfg, &s).is_err());
    }
}
