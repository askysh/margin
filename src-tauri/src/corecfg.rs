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
}

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
    Ok(())
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
