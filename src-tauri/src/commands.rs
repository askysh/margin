//! The operations the UI can ask for. Each is a plain function (`ops::*`). The Tauri
//! commands below run them on the blocking pool so the UI thread stays free, and the
//! `--dev serve` QA bridge calls the same functions over HTTP.

use crate::recorder::{recorder, Kind, RecorderView, Stopped};
use crate::{corecfg, extras, library, models, paths, permissions, prefs, processing, reset, speakers, summaries};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub type Res<T> = Result<T, String>;

pub mod ops {
    use super::*;

    #[derive(Serialize)]
    pub struct Boot {
        onboarded: bool,
        settings: corecfg::AppSettings,
        prefs: prefs::AppPrefs,
        recorder: RecorderView,
        orphaned_draft: Option<String>,
        upstream_tag: &'static str,
        version: &'static str,
    }

    pub fn boot() -> Res<Boot> {
        tracing::info!("ui connected (boot)");
        Ok(Boot {
            onboarded: prefs::load().onboarding_complete && corecfg::exists(),
            settings: current_settings(),
            prefs: prefs::load().app(),
            recorder: recorder().view(),
            orphaned_draft: crate::recorder::orphaned_draft(),
            upstream_tag: "v0.32.1",
            version: env!("CARGO_PKG_VERSION"),
        })
    }

    pub fn save_settings(settings: corecfg::AppSettings) -> Res<corecfg::AppSettings> {
        static SAVING: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _one_at_a_time = SAVING.lock().unwrap_or_else(|e| e.into_inner());
        let first_save = !corecfg::exists();
        let mut cfg = corecfg::load();
        let keep_audio_before = corecfg::settings(&cfg).keep_audio_days;
        corecfg::apply(&mut cfg, &settings)?;
        // Onboarding saves your name before you choose a folder. Don't create the default
        // folder until it has actually been chosen.
        if !(first_save && cfg.output_dir == paths::default_output_dir()) {
            std::fs::create_dir_all(&cfg.output_dir).map_err(|e| format!("Can't use that folder: {e}"))?;
        }
        corecfg::save(&mut cfg)?;
        extras::apply(&settings)?;
        if settings.keep_audio_days.is_some() && settings.keep_audio_days != keep_audio_before {
            corecfg::audio_cleanup(&cfg);
        }
        Ok(current_settings())
    }

    pub fn current_settings() -> corecfg::AppSettings {
        extras::fill(corecfg::settings(&corecfg::load()))
    }

    #[derive(Serialize)]
    pub struct SettingsOptions {
        languages: Vec<Language>,
        input_devices: Vec<String>,
        silence_stop_minutes: &'static [u64],
        keep_audio_days: &'static [u32],
    }

    #[derive(Serialize)]
    pub struct Language {
        code: &'static str,
        name: &'static str,
    }

    /// The choices Settings offers. Listing input devices reads Core Audio's device list
    /// only; it opens no device, so it raises no permission prompt.
    pub fn settings_options() -> Res<SettingsOptions> {
        let mut languages: Vec<Language> = corecfg::LANGUAGES.iter().map(|&(code, name)| Language { code, name }).collect();
        languages.sort_by_key(|l| l.name);
        let input_devices = minutes_core::capture::list_devices_categorized()
            .into_iter()
            .filter(|d| d.category == minutes_core::capture::DeviceCategory::Microphone)
            .map(|d| d.name)
            .collect();
        Ok(SettingsOptions {
            languages,
            input_devices,
            silence_stop_minutes: corecfg::SILENCE_STOP_CHOICES,
            keep_audio_days: corecfg::KEEP_AUDIO_CHOICES,
        })
    }

    pub fn save_app_prefs(prefs: prefs::AppPrefs) -> Res<prefs::AppPrefs> {
        let mut p = prefs::load();
        p.mute_cues = !prefs.sound_cues;
        p.record_shortcut = prefs.record_shortcut;
        prefs::save(&p)?;
        crate::events::emit(crate::events::SHORTCUT_REFRESH, ());
        crate::events::emit("app-prefs", p.app());
        Ok(p.app())
    }

    #[derive(Serialize)]
    pub struct FolderChoices {
        default_dir: String,
        obsidian_vaults: Vec<Vault>,
    }

    #[derive(Serialize)]
    pub struct Vault {
        name: String,
        path: String,
    }

    pub fn folder_choices() -> Res<FolderChoices> {
        let registry = paths::home().join("Library/Application Support/obsidian/obsidian.json");
        let vaults = std::fs::read(registry)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .and_then(|v| v["vaults"].as_object().cloned())
            .map(|m| {
                m.values()
                    .filter_map(|v| v["path"].as_str())
                    .filter(|p| std::path::Path::new(p).is_dir())
                    .map(|p| Vault {
                        name: std::path::Path::new(p).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                        path: p.to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(FolderChoices { default_dir: paths::default_output_dir().display().to_string(), obsidian_vaults: vaults })
    }

    pub fn complete_onboarding() -> Res<()> {
        let mut p = prefs::load();
        p.onboarding_complete = true;
        prefs::save(&p)?;
        crate::events::refresh_tray();
        crate::events::emit(crate::events::SHORTCUT_REFRESH, ());
        Ok(())
    }

    pub fn models_status() -> Res<Vec<models::ModelFile>> {
        tracing::debug!("models_status");
        Ok(models::required(&corecfg::load()))
    }

    pub fn download_models() -> Res<()> {
        models::download_missing()
    }

    pub fn permissions_state() -> Res<permissions::PermissionState> {
        Ok(permissions::state())
    }

    pub fn request_permission(which: String) -> Res<&'static str> {
        Ok(match which.as_str() {
            "microphone" => permissions::request_microphone(),
            _ => permissions::request_system_audio(),
        })
    }

    pub fn open_privacy_settings(pane: String) -> Res<()> {
        permissions::open_settings(&pane);
        Ok(())
    }

    pub fn summary_backends() -> Res<summaries::SummaryBackends> {
        Ok(summaries::detect(&corecfg::load().summarization.ollama_url))
    }

    pub fn start_recording(kind: Kind, title: Option<String>) -> Res<RecorderView> {
        if !models::all_installed(&corecfg::load()) {
            return Err("The speech models aren't downloaded yet. Open Settings → Transcription.".into());
        }
        recorder().start(kind, title)
    }

    pub fn stop_recording(notes: Option<String>, title: Option<String>) -> Res<Stopped> {
        recorder().stop(notes, title)
    }

    pub fn recorder_view() -> Res<RecorderView> {
        Ok(recorder().view())
    }

    pub fn set_draft_notes(text: String) -> Res<()> {
        recorder().set_notes(text);
        Ok(())
    }

    pub fn set_recording_title(title: Option<String>) -> Res<()> {
        recorder().set_title(title);
        Ok(())
    }

    pub fn discard_orphaned_draft() -> Res<()> {
        let _ = std::fs::remove_file(paths::drafts_dir().join("current.md"));
        Ok(())
    }

    pub fn list_jobs() -> Res<Vec<processing::JobView>> {
        Ok(processing::job_views())
    }

    pub fn retry_job(id: String) -> Res<()> {
        minutes_core::jobs::requeue_job(&id).map_err(|e| e.to_string())?;
        processing::spawn_worker();
        Ok(())
    }

    pub fn list_meetings() -> Res<Vec<library::MeetingSummary>> {
        Ok(library::list())
    }

    pub fn read_meeting(path: String) -> Res<library::MeetingDetail> {
        library::read(&PathBuf::from(path))
    }

    pub fn save_notes(path: String, notes: String) -> Res<()> {
        let p = PathBuf::from(path);
        library::ensure_inside_output(&p)?;
        library::replace_notes(&p, &notes)
    }

    pub fn set_meeting_title(path: String, title: String) -> Res<()> {
        library::set_title(&PathBuf::from(path), &title)?;
        crate::events::refresh_tray();
        Ok(())
    }

    pub fn search_meetings(query: String) -> Res<Vec<library::SearchHit>> {
        Ok(library::search(&query))
    }

    pub fn trash_meeting(path: String) -> Res<()> {
        library::trash(&PathBuf::from(path))?;
        crate::events::refresh_tray();
        Ok(())
    }

    pub fn reveal_in_finder(path: String) -> Res<()> {
        let _ = std::process::Command::new("/usr/bin/open").arg("-R").arg(path).spawn();
        Ok(())
    }

    pub fn open_in_obsidian(path: String) -> Res<()> {
        let p = PathBuf::from(&path);
        library::obsidian_vault_for(&p).ok_or("This meeting isn't inside an Obsidian vault.")?;
        let url = format!("obsidian://open?path={}", percent_encode(&path));
        std::process::Command::new("/usr/bin/open").arg(url).spawn().map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn rename_speaker(path: String, label: String, name: String, remember_voice: bool) -> Res<speakers::RenameResult> {
        speakers::rename(&PathBuf::from(path), &label, &name, remember_voice)
    }

    pub fn summarize_meeting(path: String) -> Res<()> {
        let p = PathBuf::from(path);
        library::ensure_inside_output(&p)?;
        let cfg = corecfg::load();
        if cfg.summarization.engine == "none" {
            return Err("Summaries are off. Turn on Claude or Ollama in Settings → Summaries.".into());
        }
        if cfg.summarization.engine == "ollama" {
            let model = &cfg.summarization.ollama_model;
            match summaries::ollama_models(&cfg.summarization.ollama_url) {
                None => return Err("Ollama isn't running. Start the Ollama app, then try again.".into()),
                Some(installed) if !installed.iter().any(|m| m == model || m.trim_end_matches(":latest") == model) => {
                    return Err(format!("Ollama doesn't have the model “{model}”. Pick another in Settings → Summaries."));
                }
                _ => {}
            }
        }
        let report = minutes_core::resummarize::resummarize_meeting(
            &p,
            &cfg,
            &minutes_core::resummarize::ResummarizeOptions {
                apply: true,
                template_override: extras::summary_template_slug(),
                refresh: Default::default(),
            },
        )
        .map_err(|e| {
            if cfg.summarization.engine == "ollama" {
                format!(
                    "Ollama didn't finish the summary. Summaries must finish within 2 minutes, and local models that think before answering (such as qwen3) are often slower than that. Try llama3.2 or gemma3, or Claude. ({e})"
                )
            } else {
                format!("Summary failed: {e}")
            }
        })?;
        if !report.applied {
            return Err("The summary engine returned nothing. Check that it's running, then try again.".into());
        }
        Ok(())
    }

    pub fn set_action_item(path: String, index: usize, done: bool) -> Res<()> {
        library::set_action_item(&PathBuf::from(path), index, done)
    }

    pub fn list_voices() -> Res<Vec<speakers::VoiceProfileView>> {
        Ok(speakers::list_voices())
    }

    pub fn forget_voice(slug: String) -> Res<()> {
        speakers::forget_voice(&slug)
    }

    pub fn reset_preview() -> Res<reset::ResetPreview> {
        Ok(reset::preview())
    }

    pub fn reset_app(options: reset::ResetOptions) -> Res<reset::ResetReport> {
        let report = reset::run(recorder(), &options)?;
        // Prefs are gone: the shortcut is off and the menu has no meetings until setup.
        crate::events::emit(crate::events::SHORTCUT_REFRESH, ());
        crate::events::refresh_tray();
        Ok(report)
    }

    pub fn mcp_snippet() -> Res<String> {
        let dir = corecfg::load().output_dir.display().to_string();
        Ok(serde_json::to_string_pretty(&serde_json::json!({
            "mcpServers": { "minutes": { "command": "npx", "args": ["-y", "minutes-mcp"], "env": { "MEETINGS_DIR": dir } } }
        }))
        .unwrap_or_default())
    }

    fn percent_encode(s: &str) -> String {
        s.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => (b as char).to_string(),
                _ => format!("%{b:02X}"),
            })
            .collect()
    }
}

/// Dispatch by name with JSON arguments: the `--dev serve` bridge and its UI tests use
/// this. Argument names match what the frontend sends (camelCase, as Tauri expects).
pub fn dispatch(cmd: &str, args: serde_json::Value) -> Res<serde_json::Value> {
    fn arg<T: for<'de> Deserialize<'de>>(args: &serde_json::Value, key: &str) -> Res<T> {
        serde_json::from_value(args.get(key).cloned().unwrap_or(serde_json::Value::Null))
            .map_err(|e| format!("bad argument {key}: {e}"))
    }
    fn out<T: Serialize>(r: Res<T>) -> Res<serde_json::Value> {
        r.and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string()))
    }
    let a = &args;
    match cmd {
        "boot" => out(ops::boot()),
        "save_settings" => out(ops::save_settings(arg(a, "settings")?)),
        "settings_options" => out(ops::settings_options()),
        "save_app_prefs" => out(ops::save_app_prefs(arg(a, "prefs")?)),
        "folder_choices" => out(ops::folder_choices()),
        "complete_onboarding" => out(ops::complete_onboarding()),
        "models_status" => out(ops::models_status()),
        "download_models" => out(ops::download_models()),
        "permissions_state" => out(ops::permissions_state()),
        // The QA bridge must never raise a macOS permission prompt.
        "request_permission" => Err("Permission prompts are disabled in the QA bridge.".into()),
        "open_privacy_settings" => Ok(serde_json::Value::Null),
        "summary_backends" => out(ops::summary_backends()),
        "start_recording" => out(ops::start_recording(arg(a, "kind")?, arg(a, "title")?)),
        "stop_recording" => out(ops::stop_recording(arg(a, "notes")?, arg(a, "title")?)),
        "recorder_view" => out(ops::recorder_view()),
        "set_draft_notes" => out(ops::set_draft_notes(arg(a, "text")?)),
        "set_recording_title" => out(ops::set_recording_title(arg(a, "title")?)),
        "discard_orphaned_draft" => out(ops::discard_orphaned_draft()),
        "list_jobs" => out(ops::list_jobs()),
        "retry_job" => out(ops::retry_job(arg(a, "id")?)),
        "list_meetings" => out(ops::list_meetings()),
        "read_meeting" => out(ops::read_meeting(arg(a, "path")?)),
        "save_notes" => out(ops::save_notes(arg(a, "path")?, arg(a, "notes")?)),
        "search_meetings" => out(ops::search_meetings(arg(a, "query")?)),
        "set_meeting_title" => out(ops::set_meeting_title(arg(a, "path")?, arg(a, "title")?)),
        "trash_meeting" => out(ops::trash_meeting(arg(a, "path")?)),
        "reveal_in_finder" | "open_in_obsidian" => Ok(serde_json::Value::Null),
        "rename_speaker" => out(ops::rename_speaker(arg(a, "path")?, arg(a, "label")?, arg(a, "name")?, arg(a, "rememberVoice")?)),
        "summarize_meeting" => out(ops::summarize_meeting(arg(a, "path")?)),
        "set_action_item" => out(ops::set_action_item(arg(a, "path")?, arg(a, "index")?, arg(a, "done")?)),
        // The app adds the file to the asset scope; the bridge serves it at /asset.
        "meeting_audio" => out(library::audio_path(&PathBuf::from(arg::<String>(a, "path")?)).map(|p| p.display().to_string())),
        "list_voices" => out(ops::list_voices()),
        "forget_voice" => out(ops::forget_voice(arg(a, "slug")?)),
        "reset_preview" => out(ops::reset_preview()),
        "reset_app" => out(ops::reset_app(arg(a, "options")?)),
        "mcp_snippet" => out(ops::mcp_snippet()),
        other => Err(format!("unknown command {other}")),
    }
}

/// Tauri wrappers: same names and arguments as `ops`, run off the main thread.
pub mod tauri_cmds {
    use super::*;

    async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
        tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
    }

    macro_rules! cmd {
        ($name:ident ( $($arg:ident : $ty:ty),* ) -> $ret:ty) => {
            #[tauri::command]
            pub async fn $name($($arg: $ty),*) -> Res<$ret> {
                blocking(move || ops::$name($($arg),*)).await
            }
        };
    }

    cmd!(boot() -> ops::Boot);
    cmd!(save_settings(settings: corecfg::AppSettings) -> corecfg::AppSettings);
    cmd!(settings_options() -> ops::SettingsOptions);
    cmd!(save_app_prefs(prefs: prefs::AppPrefs) -> prefs::AppPrefs);
    cmd!(folder_choices() -> ops::FolderChoices);
    cmd!(complete_onboarding() -> ());
    cmd!(models_status() -> Vec<models::ModelFile>);
    cmd!(download_models() -> ());
    cmd!(permissions_state() -> permissions::PermissionState);
    cmd!(request_permission(which: String) -> &'static str);
    cmd!(open_privacy_settings(pane: String) -> ());
    cmd!(summary_backends() -> summaries::SummaryBackends);
    cmd!(start_recording(kind: Kind, title: Option<String>) -> RecorderView);
    cmd!(stop_recording(notes: Option<String>, title: Option<String>) -> Stopped);
    cmd!(recorder_view() -> RecorderView);
    cmd!(set_draft_notes(text: String) -> ());
    cmd!(set_recording_title(title: Option<String>) -> ());
    cmd!(discard_orphaned_draft() -> ());
    cmd!(list_jobs() -> Vec<processing::JobView>);
    cmd!(retry_job(id: String) -> ());
    cmd!(list_meetings() -> Vec<library::MeetingSummary>);
    cmd!(read_meeting(path: String) -> library::MeetingDetail);
    cmd!(save_notes(path: String, notes: String) -> ());
    cmd!(search_meetings(query: String) -> Vec<library::SearchHit>);
    cmd!(set_meeting_title(path: String, title: String) -> ());
    cmd!(trash_meeting(path: String) -> ());
    cmd!(reveal_in_finder(path: String) -> ());
    cmd!(open_in_obsidian(path: String) -> ());
    cmd!(rename_speaker(path: String, label: String, name: String, remember_voice: bool) -> speakers::RenameResult);
    cmd!(summarize_meeting(path: String) -> ());
    cmd!(set_action_item(path: String, index: usize, done: bool) -> ());
    cmd!(list_voices() -> Vec<speakers::VoiceProfileView>);
    cmd!(forget_voice(slug: String) -> ());
    cmd!(reset_preview() -> reset::ResetPreview);
    cmd!(reset_app(options: reset::ResetOptions) -> reset::ResetReport);
    cmd!(mcp_snippet() -> String);
}
