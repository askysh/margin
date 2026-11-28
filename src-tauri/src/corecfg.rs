//! The minutes-core config file, plus the small slice of it that Margin's settings expose.
//!
//! We always write keys explicitly instead of relying on core defaults. One reason: if
//! `summarization.engine` is missing, core silently forces it to "auto", which runs
//! whatever agent CLI is installed (docs/research.md §1.6).

use crate::paths;
use minutes_core::config::Config;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const MODELS: &[(&str, &str, u64)] = &[
    // (id, label, approximate bytes)
    ("base", "Fast", 147_951_465),
    ("small", "Balanced", 487_601_967),
    ("large-v3-turbo", "Best", 1_624_555_275),
    ("hinglish", "Hindi + English (Devanagari)", 1_081_140_203),
];

/// Models with no public download: the user puts `ggml-<id>.bin` in the models folder.
/// "hinglish" is Trelis/whisper-hinglish-preview (Apache-2.0) converted to ggml q5_0.
pub const LOCAL_ONLY_MODELS: &[&str] = &["hinglish"];

/// Smallest local-only model file that counts as installed.
pub const MIN_LOCAL_MODEL_BYTES: u64 = 500_000_000;

/// What transcribes, in order, when a local-only model's file is missing.
pub const MISSING_LOCAL_MODEL_FALLBACKS: &[&str] = &["large-v3-turbo", "small"];

/// The language a model must run with. The Hinglish model writes Devanagari only, English
/// words included, so it is for Hindi-heavy calls and always runs as Hindi (DECISIONS.md D81).
pub fn model_language(model: &str) -> Option<&'static str> {
    (model == "hinglish").then_some("hi")
}

/// Rolling transcription must use the final pass's model, or core rejects its pieces. The
/// Hinglish model is large-v3 sized and cannot keep up during a meeting, so rolling is off
/// with it (D81).
pub fn rolling_supported(model: &str) -> bool {
    model != "hinglish"
}

/// The language the user had before choosing a model with a fixed language. `save` keeps
/// it here and puts it back when they choose another model.
fn language_stash_path() -> PathBuf {
    paths::config_path().with_file_name("language-before-hinglish")
}

/// Settles the transcription language on save. `previous` is the config on disk before
/// this save and `stash` the remembered language (`Some(None)` is auto-detect). Returns
/// the stash to keep: `None` deletes it.
fn settle_language(cfg: &mut Config, previous: Option<&Config>, stash: Option<Option<String>>) -> Option<Option<String>> {
    if let Some(forced) = model_language(&cfg.transcription.model) {
        cfg.transcription.language = Some(forced.into());
        let before = previous
            .filter(|p| model_language(&p.transcription.model).is_none())
            .map(|p| p.transcription.language.clone())
            .unwrap_or(None);
        return Some(stash.unwrap_or(before));
    }
    if let Some(before) = stash {
        // Settings showed the forced language, so an unchanged "hi" is not a choice.
        if previous.is_some_and(|p| model_language(&p.transcription.model).is_some())
            && cfg.transcription.language == previous.and_then(|p| p.transcription.language.clone())
        {
            cfg.transcription.language = before;
        }
    }
    None
}

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
    /// Leave Margin's windows out of screen sharing and screenshots (screenshare.rs).
    #[serde(default)]
    pub hide_from_screen_share: Option<bool>,
    // Kept outside config.toml (see extras.rs).
    #[serde(default)]
    pub vocabulary: Option<Vec<String>>,
    #[serde(default)]
    pub summary_instructions: Option<String>,
    #[serde(default)]
    pub open_at_login: Option<bool>,
    /// Transcribe during the meeting (rolling.rs). Kept in prefs.json.
    #[serde(default)]
    pub rolling_transcription: Option<bool>,
    /// Ollama summaries answer without thinking first (ollama_proxy.rs). Kept in prefs.json.
    #[serde(default)]
    pub ollama_fast: Option<bool>,
    /// Offer to start notes before a calendar event (calendar_watch.rs, D-calendar).
    #[serde(default)]
    pub calendar_toast: Option<bool>,
    /// Core's knowledge wiki after each meeting, with engine "none" (no model).
    #[serde(default)]
    pub knowledge_enabled: Option<bool>,
    /// "obsidian" | "wiki" | "para"
    #[serde(default)]
    pub knowledge_adapter: Option<String>,
    #[serde(default)]
    pub knowledge_path: Option<String>,
    /// A shell command run after each processed meeting, with the meeting path appended
    /// (core's `hooks.post_record`). "" turns it off.
    #[serde(default)]
    pub post_record: Option<String>,
    /// Core's speaker model set: "legacy" or "community-1" (D72).
    #[serde(default)]
    pub speaker_model: Option<String>,
}

/// Speaker model sets core supports.
pub const SPEAKER_MODELS: &[&str] = &["legacy", "community-1"];

/// Adapters core's knowledge wiki accepts.
pub const KNOWLEDGE_ADAPTERS: &[&str] = &["obsidian", "wiki", "para"];

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
    let mut cfg = Config::load_strict_from(&path).unwrap_or_else(|error| {
        tracing::warn!(%error, "config unreadable; using fresh defaults");
        fresh()
    });
    // Files written by an older Margin lack newer pinned keys (the live model, for one).
    pin_owned_keys(&mut cfg);
    cfg
}

/// The model for the live transcript while recording. Core's default is the dictation
/// model ("base"), which Margin never downloads, so the live transcript stayed empty.
/// Use the chosen model, except the large one. Large lags and loses lines live, and rolling
/// transcription (D52) needs the GPU too. Balanced is the next best model: 9.3% WER live,
/// against 11.7% for Fast (docs/research/live-transcription.md).
pub fn live_model(model: &str) -> String {
    // The Hinglish model is large-v3 sized: too slow live, as large is.
    if model.starts_with("large") || model == "hinglish" { "small".into() } else { model.into() }
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

/// Keys Margin owns, re-asserted on every load and save so a hand edit cannot point core
/// at the upstream app's directories.
fn pin_owned_keys(cfg: &mut Config) {
    pin_with(cfg, &crate::prefs::load());
    cfg.transcription.missing_model_fallback_language = fallback_language(&cfg.transcription.model, read_language_stash);
}

/// The language a fallback model runs with: never the forced language of the model it
/// stands in for, but the user's own from before (`stash`), auto-detect by default.
fn fallback_language(model: &str, stash: impl FnOnce() -> Option<Option<String>>) -> Option<String> {
    model_language(model).and_then(|_| stash().flatten())
}

/// The remembered language (`Some(None)` is auto-detect), or `None` without one.
fn read_language_stash() -> Option<Option<String>> {
    std::fs::read_to_string(language_stash_path())
        .ok()
        .map(|code| Some(code.trim().to_string()).filter(|c| !c.is_empty() && c != "auto"))
}

fn pin_with(cfg: &mut Config, prefs: &crate::prefs::Prefs) {
    // Margin's switch, not core's default (D54). Every config that Margin wrote before
    // says `true`, so the file cannot tell a choice from a default.
    cfg.privacy.hide_from_screen_share = prefs.hide_from_screen_share;
    cfg.transcription.model_path = paths::models_dir();
    // A local-only model's file can go missing; core then transcribes with the first of
    // these that is installed (UPSTREAM-PATCHES #33), and Start says so (commands.rs).
    cfg.transcription.missing_model_fallbacks = if LOCAL_ONLY_MODELS.contains(&cfg.transcription.model.as_str()) {
        MISSING_LOCAL_MODEL_FALLBACKS.iter().map(|m| m.to_string()).collect()
    } else {
        Vec::new()
    };
    cfg.diarization.model_path = paths::diarization_models_dir();
    pin_measured_transcription_defaults(&mut cfg.transcription, !crate::extras::vocabulary_terms().is_empty());
    cfg.vault.enabled = false;
    cfg.live_transcript.recording_sidecar = true;
    cfg.live_transcript.model = live_model(&cfg.transcription.model);
    // Summaries run after the meeting is shown (D40), so a long one can take its time:
    // core's 5-minute default cut off Claude on an 80-minute meeting.
    cfg.summarization.agent_timeout_secs = cfg.summarization.agent_timeout_secs.max(900);
    // Core's patch (UPSTREAM-PATCHES #31): no "Here is a summary" lead-in. Margin also
    // strips such lines afterwards.
    cfg.summarization.no_lead_in = true;
    if cfg.summarization.engine.trim().is_empty() {
        cfg.summarization.engine = "none".into();
    }
}

// Measured on AMI ES2004a and TS3003a (D82): core's meeting chunker dropped 6–16% of the
// reference speech before Whisper, and conditioning on earlier text added errors. Together
// these settings cut WER from 26.6/26.2% to 21.8/20.5%, with diarization unchanged. Only
// core's own defaults are replaced, so a value someone set in config.toml stays.
const MEETING_CHUNK_NOISE_MULTIPLIER: f32 = 2.5;
const MEETING_CHUNK_PAD_MS: u32 = 300;

/// `has_vocabulary`: the user's Settings word list is not empty. whisper.cpp passes the
/// initial prompt through the conditioning buffer, so `n_max_text_ctx = 0` would drop the
/// words and the name and title hints (UPSTREAM-PATCHES #25). Then the context stays
/// upstream's. Earlier builds saved Margin's 0 into config.toml, where it cannot be told from
/// a choice, so a 0 counts as Margin's own value; any other number is the user's and stays.
fn pin_measured_transcription_defaults(t: &mut minutes_core::config::TranscriptionConfig, has_vocabulary: bool) {
    let core = minutes_core::config::TranscriptionConfig::default();
    if t.meeting_chunk_noise_multiplier == core.meeting_chunk_noise_multiplier {
        t.meeting_chunk_noise_multiplier = MEETING_CHUNK_NOISE_MULTIPLIER;
    }
    if t.meeting_chunk_pad_ms == core.meeting_chunk_pad_ms {
        t.meeting_chunk_pad_ms = MEETING_CHUNK_PAD_MS;
    }
    if matches!(t.n_max_text_ctx, None | Some(0)) {
        t.n_max_text_ctx = (!has_vocabulary).then_some(0);
    }
}

pub fn save(cfg: &mut Config) -> Result<(), String> {
    pin_owned_keys(cfg);
    if let Some(parent) = paths::config_path().parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let previous = Config::load_strict_from(&paths::config_path()).ok();
    let stash_path = language_stash_path();
    let stash = read_language_stash();
    match settle_language(cfg, previous.as_ref(), stash) {
        Some(language) => std::fs::write(&stash_path, language.unwrap_or_else(|| "auto".into()))
            .map_err(|e| format!("Could not save settings: {e}"))?,
        None => {
            let _ = std::fs::remove_file(&stash_path);
        }
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
        hide_from_screen_share: Some(cfg.privacy.hide_from_screen_share),
        vocabulary: None,
        summary_instructions: None,
        open_at_login: None,
        rolling_transcription: None,
        ollama_fast: None,
        calendar_toast: Some(cfg.calendar.enabled),
        knowledge_enabled: Some(cfg.knowledge.enabled),
        knowledge_adapter: Some(cfg.knowledge.adapter.clone()),
        knowledge_path: Some(cfg.knowledge.path.display().to_string()),
        post_record: Some(cfg.hooks.post_record.clone().unwrap_or_default()),
        speaker_model: Some(cfg.diarization.model.clone()),
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
    if LOCAL_ONLY_MODELS.contains(&s.model.as_str()) && s.model != cfg.transcription.model {
        let file = cfg.transcription.model_path.join(format!("ggml-{}.bin", s.model));
        if !crate::models::local_model_installed(&file) {
            return Err(format!("Copy ggml-{}.bin to {} first. This model has no download.", s.model, file.parent().unwrap_or(&file).display()));
        }
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
    if let Some(language) = model_language(&cfg.transcription.model) {
        // `save` remembers the language this replaces (settle_language).
        cfg.transcription.language = Some(language.into());
    }
    if let Some(on) = s.speaker_separation {
        cfg.diarization.engine = if on { "auto" } else { "none" }.into();
    }
    if let Some(model) = s.speaker_model.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
        if !SPEAKER_MODELS.contains(&model) {
            return Err(format!("Unknown speaker model {model}"));
        }
        cfg.diarization.model = model.into();
    }
    if let Some(days) = s.keep_audio_days {
        apply_keep_audio(cfg, days)?;
    }
    if let Some(on) = s.call_detection {
        cfg.call_detection.enabled = on;
    }
    if let Some(on) = s.hide_from_screen_share {
        cfg.privacy.hide_from_screen_share = on;
    }
    if let Some(on) = s.calendar_toast {
        apply_calendar(cfg, on);
    }
    apply_knowledge(cfg, s)?;
    if let Some(command) = &s.post_record {
        let command = command.trim();
        if !command.is_empty() && !command_runs(command) {
            let program = command_program(command).unwrap_or_else(|| command.to_string());
            return Err(format!("Margin can't find the program “{program}”. Use its full path, or click Use Margin Desk."));
        }
        cfg.hooks.post_record = (!command.is_empty()).then(|| command.to_string());
    }
    Ok(())
}

/// True when the command's program can run from Margin: a full path to an executable file,
/// or a name on Margin's PATH. Core runs the hook with `sh -c` in the processing worker, so a
/// name that only an interactive shell knows (an alias, a profile PATH) would fail there,
/// silently, after every meeting.
fn command_runs(command: &str) -> bool {
    let Some(program) = command_program(command) else { return false };
    if program.contains(['/', '\\']) {
        let path = std::path::Path::new(&program);
        return path.is_file() && crate::summaries::is_executable(path);
    }
    which::which(&program).is_ok()
}

/// The first word of a command line as the hook's shell reads it, quotes removed: `sh -c`
/// (a backslash escapes the next character outside single quotes) or, on Windows, `cmd.exe`
/// (a backslash is a path separator). None for an unclosed quote.
fn command_program(command: &str) -> Option<String> {
    let mut program = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for c in command.trim_start().chars() {
        match (quote, c) {
            _ if escaped => {
                program.push(c);
                escaped = false;
            }
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') | (None, '\\') if !cfg!(windows) => escaped = true,
            (Some(_), c) => program.push(c),
            (None, '\'' | '"') => quote = Some(c),
            (None, c) if c.is_whitespace() => break,
            (None, c) => program.push(c),
        }
    }
    (!program.is_empty() && quote.is_none()).then_some(program)
}

/// `path` as one word of a hook command line, which core runs with `sh -c`, or with
/// `cmd.exe` on Windows, where "C:\Program Files\nodejs\node.exe" would split at the space.
pub fn shell_word(path: &std::path::Path) -> String {
    let text = path.display().to_string();
    if cfg!(windows) {
        // A Windows path cannot hold a double quote, so nothing inside needs escaping.
        return format!("\"{text}\"");
    }
    if !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+@:,=".contains(c)) {
        return text;
    }
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// The calendar toast and core's event match share one switch. The match gives a meeting
/// the event's title and attendees. Events with no attendees and no call link (focus
/// blocks, reminders) are left out, so they neither title a meeting nor raise a toast.
fn apply_calendar(cfg: &mut Config, on: bool) {
    cfg.calendar.enabled = on;
    if on {
        cfg.calendar.use_event_title_for_meeting_title = true;
        cfg.calendar.require_attendees_or_url = true;
    }
}

fn apply_knowledge(cfg: &mut Config, s: &AppSettings) -> Result<(), String> {
    if let Some(adapter) = s.knowledge_adapter.as_deref().map(str::trim).filter(|a| !a.is_empty()) {
        if !KNOWLEDGE_ADAPTERS.contains(&adapter) {
            return Err(format!("Unknown knowledge format {adapter}"));
        }
        cfg.knowledge.adapter = adapter.into();
    }
    if let Some(path) = s.knowledge_path.as_deref().map(str::trim) {
        let path = PathBuf::from(expand_tilde(path));
        if !path.as_os_str().is_empty() && !path.is_absolute() {
            return Err("Choose a knowledge folder with a full path.".into());
        }
        cfg.knowledge.path = path;
    }
    if let Some(on) = s.knowledge_enabled {
        if on && cfg.knowledge.path.as_os_str().is_empty() {
            return Err("Choose a folder for the knowledge wiki first.".into());
        }
        cfg.knowledge.enabled = on;
        // Facts from the meeting's frontmatter only. A model would write facts nobody can
        // check against the transcript, so Margin never sets another engine here.
        cfg.knowledge.engine = "none".into();
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

/// The Windows account's display name, used as a default for "your name". A local account
/// often has none. Setup then leaves the field empty.
#[cfg(windows)]
fn full_user_name() -> Option<String> {
    crate::win::display_name()
}

/// The macOS account's full name, used as a default for "your name".
#[cfg(not(windows))]
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
        // A full path on each platform. The tests write nothing there.
        cfg.output_dir = PathBuf::from(if cfg!(windows) { r"C:\margin-test-meetings" } else { "/tmp/margin-test-meetings" });
        cfg.transcription.model = "small".into();
        cfg.transcription.model_path = PathBuf::from(if cfg!(windows) { r"C:\margin-test-models" } else { "/tmp/margin-test-models" });
        cfg.summarization.engine = "none".into();
        let s = settings(&cfg);
        (cfg, s)
    }

    #[test]
    fn calendar_knowledge_and_hook_settings_round_trip() {
        let (mut cfg, mut s) = base();
        s.calendar_toast = Some(true);
        s.knowledge_adapter = Some("obsidian".into());
        s.knowledge_path = Some("/tmp/margin-test-wiki".into());
        s.knowledge_enabled = Some(true);
        s.post_record = Some("  /bin/echo /opt/desk.mjs run  ".into());
        apply(&mut cfg, &s).unwrap();
        assert!(cfg.calendar.enabled && cfg.calendar.use_event_title_for_meeting_title && cfg.calendar.require_attendees_or_url);
        assert!(cfg.knowledge.enabled);
        assert_eq!(cfg.knowledge.engine, "none", "Margin never asks a model for wiki facts");
        assert_eq!(cfg.knowledge.adapter, "obsidian");
        assert_eq!(cfg.hooks.post_record.as_deref(), Some("/bin/echo /opt/desk.mjs run"));
        let back = settings(&cfg);
        assert_eq!(back.calendar_toast, Some(true));
        assert_eq!(back.knowledge_enabled, Some(true));
        assert_eq!(back.knowledge_path.as_deref(), Some("/tmp/margin-test-wiki"));
        assert_eq!(back.post_record.as_deref(), Some("/bin/echo /opt/desk.mjs run"));

        let mut off = back.clone();
        off.calendar_toast = Some(false);
        off.post_record = Some("".into());
        apply(&mut cfg, &off).unwrap();
        assert!(!cfg.calendar.enabled);
        assert_eq!(cfg.hooks.post_record, None, "an empty command turns the hook off");
    }

    #[test]
    fn the_speaker_model_round_trips_and_is_checked() {
        let (mut cfg, mut s) = base();
        s.speaker_model = Some("community-1".into());
        apply(&mut cfg, &s).unwrap();
        assert_eq!(cfg.diarization.model, "community-1");
        assert_eq!(settings(&cfg).speaker_model.as_deref(), Some("community-1"));
        s.speaker_model = Some("pyannote-cloud".into());
        assert!(apply(&mut cfg, &s).is_err());
    }

    #[test]
    fn a_post_record_command_must_name_a_program_that_runs() {
        let (mut cfg, mut s) = base();
        s.post_record = Some("desk run".into());
        let err = apply(&mut cfg, &s).unwrap_err();
        assert!(err.contains("can't find the program “desk”"), "{err}");
        s.post_record = Some("/no/such/node /x.mjs run".into());
        assert!(apply(&mut cfg, &s).is_err());
        s.post_record = Some("sh -c true".into());
        assert!(apply(&mut cfg, &s).is_ok(), "a name on PATH runs");
        s.post_record = Some("".into());
        assert!(apply(&mut cfg, &s).is_ok(), "empty turns the hook off");
    }

    #[test]
    fn knowledge_settings_are_checked() {
        let (mut cfg, mut s) = base();
        s.knowledge_adapter = Some("notion".into());
        assert!(apply(&mut cfg, &s).is_err());
        let (mut cfg, mut s) = base();
        s.knowledge_path = Some("relative/wiki".into());
        assert!(apply(&mut cfg, &s).is_err());
        let (mut cfg, mut s) = base();
        s.knowledge_path = Some("".into());
        s.knowledge_enabled = Some(true);
        assert!(apply(&mut cfg, &s).unwrap_err().contains("folder"), "on with no folder is refused");
    }

    #[test]
    fn live_model_is_balanced_for_best() {
        assert_eq!(live_model("base"), "base");
        assert_eq!(live_model("small"), "small");
        assert_eq!(live_model("large-v3-turbo"), "small");
        assert_eq!(live_model("hinglish"), "small");
    }

    #[test]
    fn the_hinglish_model_needs_its_file_and_always_transcribes_hindi() {
        let (mut cfg, mut s) = base();
        s.model = "hinglish".into();
        let err = apply(&mut cfg, &s).unwrap_err();
        assert!(err.contains("ggml-hinglish.bin") && err.contains("no download"), "{err}");
        assert_eq!(cfg.transcription.model, "small", "a refused choice changes nothing");
        // Already selected (the file was there when it was chosen): kept, and Hindi.
        cfg.transcription.model = "hinglish".into();
        s.transcription_language = Some("en".into());
        apply(&mut cfg, &s).unwrap();
        assert_eq!(cfg.transcription.language.as_deref(), Some("hi"));
    }

    #[test]
    fn leaving_hinglish_gives_back_the_language_from_before() {
        let with = |model: &str, language: Option<&str>| {
            let mut cfg = Config::default();
            cfg.transcription.model = model.into();
            cfg.transcription.language = language.map(String::from);
            cfg
        };
        // small (German) -> hinglish: Hindi, and German is remembered.
        let mut cfg = with("hinglish", Some("de"));
        let stash = settle_language(&mut cfg, Some(&with("small", Some("de"))), None);
        assert_eq!(cfg.transcription.language.as_deref(), Some("hi"));
        assert_eq!(stash, Some(Some("de".into())));
        // Saved again while on hinglish: the stash stays.
        let mut again = with("hinglish", Some("hi"));
        assert_eq!(settle_language(&mut again, Some(&cfg), stash.clone()), stash);
        // hinglish -> small, Settings still showing "hi": German comes back.
        let mut back = with("small", Some("hi"));
        assert_eq!(settle_language(&mut back, Some(&cfg), stash), None);
        assert_eq!(back.transcription.language.as_deref(), Some("de"));
        // From auto-detect, the default: auto comes back.
        let mut cfg = with("hinglish", None);
        let stash = settle_language(&mut cfg, Some(&with("small", None)), None);
        let mut back = with("small", Some("hi"));
        settle_language(&mut back, Some(&cfg), stash);
        assert_eq!(back.transcription.language, None);
        // A language picked while switching away is the user's choice.
        let mut picked = with("small", Some("fr"));
        settle_language(&mut picked, Some(&cfg), Some(None));
        assert_eq!(picked.transcription.language.as_deref(), Some("fr"));
    }

    #[test]
    fn an_empty_hinglish_file_is_not_installed_and_only_hinglish_gets_fallbacks() {
        let dir = std::env::temp_dir().join(format!("margin-hinglish-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ggml-hinglish.bin"), b"").unwrap();
        let (mut cfg, mut s) = base();
        cfg.transcription.model_path = dir.clone();
        s.model = "hinglish".into();
        assert!(apply(&mut cfg, &s).is_err(), "an empty file is not installed, as in models.rs");
        std::fs::write(dir.join("ggml-hinglish.bin"), b"ggml").unwrap();
        assert!(apply(&mut cfg, &s).is_err(), "a half-copied file is not installed");
        // Sparse: the size rule reads the length, not the blocks.
        std::fs::File::options().write(true).open(dir.join("ggml-hinglish.bin")).unwrap().set_len(MIN_LOCAL_MODEL_BYTES).unwrap();
        apply(&mut cfg, &s).unwrap();
        pin_with(&mut cfg, &crate::prefs::Prefs::default());
        assert_eq!(cfg.transcription.missing_model_fallbacks, vec!["large-v3-turbo", "small"]);
        cfg.transcription.model = "small".into();
        pin_with(&mut cfg, &crate::prefs::Prefs::default());
        assert!(cfg.transcription.missing_model_fallbacks.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_fallback_model_gets_the_users_language_not_hindi() {
        assert_eq!(fallback_language("hinglish", || Some(Some("de".into()))).as_deref(), Some("de"));
        assert_eq!(fallback_language("hinglish", || Some(None)), None, "auto-detect");
        assert_eq!(fallback_language("hinglish", || None), None);
        assert_eq!(fallback_language("small", || Some(Some("de".into()))), None);
    }

    #[test]
    fn rolling_is_off_only_for_the_hinglish_model() {
        assert!(rolling_supported("large-v3-turbo"));
        assert!(!rolling_supported("hinglish"));
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
        s.hide_from_screen_share = Some(false);
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
        assert!(!cfg.privacy.hide_from_screen_share);
        assert_eq!(back.hide_from_screen_share, Some(false));
    }

    /// Core's default hides the windows, and old configs say so. Margin's switch is off
    /// until you turn it on, whatever the file says.
    #[test]
    fn screen_share_hiding_follows_margin_not_the_file() {
        let mut cfg = Config::default();
        assert!(cfg.privacy.hide_from_screen_share, "core's default changed");
        pin_with(&mut cfg, &crate::prefs::Prefs::default());
        assert!(!cfg.privacy.hide_from_screen_share);
        assert_eq!(settings(&cfg).hide_from_screen_share, Some(false));
        let on = crate::prefs::Prefs { hide_from_screen_share: true, ..Default::default() };
        pin_with(&mut cfg, &on);
        assert!(cfg.privacy.hide_from_screen_share);
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

    #[test]
    fn measured_transcription_defaults_replace_only_cores_defaults() {
        let mut t = minutes_core::config::TranscriptionConfig::default();
        pin_measured_transcription_defaults(&mut t, false);
        assert_eq!(t.meeting_chunk_noise_multiplier, MEETING_CHUNK_NOISE_MULTIPLIER);
        assert_eq!(t.meeting_chunk_pad_ms, MEETING_CHUNK_PAD_MS);
        assert_eq!(t.n_max_text_ctx, Some(0));

        for has_vocabulary in [false, true] {
            let mut chosen = minutes_core::config::TranscriptionConfig::default();
            chosen.meeting_chunk_noise_multiplier = 3.0;
            chosen.meeting_chunk_pad_ms = 150;
            chosen.n_max_text_ctx = Some(64);
            pin_measured_transcription_defaults(&mut chosen, has_vocabulary);
            assert_eq!((chosen.meeting_chunk_noise_multiplier, chosen.meeting_chunk_pad_ms, chosen.n_max_text_ctx), (3.0, 150, Some(64)));
        }
    }

    #[test]
    fn a_hook_command_with_spaces_in_its_paths_runs() {
        let dir = std::env::temp_dir().join(format!("margin-hook-{}", std::process::id())).join("Program Files");
        std::fs::create_dir_all(&dir).unwrap();
        let node = dir.join("node js");
        std::fs::write(&node, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&node, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let desk = dir.join("Akash's desk.mjs");
        let command = format!("{} {} run", shell_word(&node), shell_word(&desk));
        assert_eq!(command_program(&command).as_deref(), Some(node.to_str().unwrap()));
        assert!(command_runs(&command), "{command}");
        assert!(!command_runs(&format!("{} run", node.display())), "unquoted, the space splits the program");
        assert_eq!(shell_word(std::path::Path::new("/opt/homebrew/bin/node")), "/opt/homebrew/bin/node");
        assert_eq!(command_program("'/a b/c"), None, "an unclosed quote");
        #[cfg(unix)]
        {
            // The words that sh itself reads.
            let words = std::process::Command::new("sh").arg("-c").arg(format!("printf '%s|' {command}")).output().unwrap();
            assert_eq!(String::from_utf8(words.stdout).unwrap(), format!("{}|{}|run|", node.display(), desk.display()));
        }
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn vocabulary_words_keep_whispers_text_context() {
        let mut t = minutes_core::config::TranscriptionConfig::default();
        pin_measured_transcription_defaults(&mut t, true);
        assert_eq!(t.n_max_text_ctx, None, "a 0 would drop the words from Whisper's prompt");
        assert_eq!(t.meeting_chunk_pad_ms, MEETING_CHUNK_PAD_MS, "the chunk settings do not depend on it");
        // A config.toml that an earlier build saved with Margin's 0.
        let mut saved = minutes_core::config::TranscriptionConfig { n_max_text_ctx: Some(0), ..Default::default() };
        pin_measured_transcription_defaults(&mut saved, true);
        assert_eq!(saved.n_max_text_ctx, None);
        pin_measured_transcription_defaults(&mut saved, false);
        assert_eq!(saved.n_max_text_ctx, Some(0), "an emptied word list brings the measured setting back");
    }
}
