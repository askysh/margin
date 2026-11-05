//! Tauri commands. Anything that blocks runs on the blocking pool, so the UI thread stays
//! free.

use crate::recorder::{Kind, RecorderView, Recorder, Stopped};
use crate::{corecfg, library, models, paths, permissions, prefs, processing, reset, speakers, summaries};
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};

type Res<T> = Result<T, String>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

#[derive(Serialize)]
pub struct Boot {
    onboarded: bool,
    settings: corecfg::AppSettings,
    recorder: RecorderView,
    orphaned_draft: Option<String>,
    upstream_tag: &'static str,
    version: &'static str,
}

#[tauri::command]
pub fn boot(recorder: State<Recorder>) -> Boot {
    Boot {
        onboarded: prefs::load().onboarding_complete && corecfg::exists(),
        settings: corecfg::settings(&corecfg::load()),
        recorder: recorder.view(),
        orphaned_draft: crate::recorder::orphaned_draft(),
        upstream_tag: "v0.32.1",
        version: env!("CARGO_PKG_VERSION"),
    }
}

// ---- settings & onboarding -------------------------------------------------------------

#[tauri::command]
pub async fn save_settings(settings: corecfg::AppSettings) -> Res<corecfg::AppSettings> {
    blocking(move || {
        let mut cfg = corecfg::load();
        corecfg::apply(&mut cfg, &settings)?;
        std::fs::create_dir_all(&cfg.output_dir)
            .map_err(|e| format!("Can't use that folder: {e}"))?;
        corecfg::save(&mut cfg)?;
        Ok(corecfg::settings(&corecfg::load()))
    })
    .await
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

#[tauri::command]
pub fn folder_choices() -> FolderChoices {
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
    FolderChoices { default_dir: paths::default_output_dir().display().to_string(), obsidian_vaults: vaults }
}

#[tauri::command]
pub fn complete_onboarding(app: AppHandle) -> Res<()> {
    let mut p = prefs::load();
    p.onboarding_complete = true;
    prefs::save(&p)?;
    crate::tray::refresh(&app);
    Ok(())
}

#[tauri::command]
pub fn models_status() -> Vec<models::ModelFile> {
    models::required(&corecfg::load())
}

#[tauri::command]
pub fn download_models(app: AppHandle) -> Res<()> {
    models::download_missing(app)
}

#[tauri::command]
pub fn permissions_state() -> permissions::PermissionState {
    permissions::state()
}

#[tauri::command]
pub async fn request_permission(which: String) -> Res<&'static str> {
    blocking(move || {
        Ok(match which.as_str() {
            "microphone" => permissions::request_microphone(),
            _ => permissions::request_system_audio(),
        })
    })
    .await
}

#[tauri::command]
pub fn open_privacy_settings(pane: String) {
    permissions::open_settings(&pane);
}

#[tauri::command]
pub async fn summary_backends() -> Res<summaries::SummaryBackends> {
    blocking(|| Ok(summaries::detect(&corecfg::load().summarization.ollama_url))).await
}

// ---- recording -------------------------------------------------------------------------

#[tauri::command]
pub async fn start_recording(app: AppHandle, kind: Kind, title: Option<String>) -> Res<RecorderView> {
    if !models::all_installed(&corecfg::load()) {
        return Err("The transcription models aren't downloaded yet. Open Settings → Transcription.".into());
    }
    blocking(move || app.state::<Recorder>().start(&app, kind, title)).await
}

#[tauri::command]
pub async fn stop_recording(app: AppHandle) -> Res<Stopped> {
    blocking(move || app.state::<Recorder>().stop(&app)).await
}

#[tauri::command]
pub fn recorder_view(recorder: State<Recorder>) -> RecorderView {
    recorder.view()
}

#[tauri::command]
pub fn set_draft_notes(recorder: State<Recorder>, text: String) {
    recorder.set_notes(text);
}

#[tauri::command]
pub fn set_recording_title(recorder: State<Recorder>, title: Option<String>) {
    recorder.set_title(title);
}

#[tauri::command]
pub fn discard_orphaned_draft() {
    let _ = std::fs::remove_file(paths::drafts_dir().join("current.md"));
}

#[tauri::command]
pub fn list_jobs() -> Vec<processing::JobView> {
    processing::job_views()
}

#[tauri::command]
pub fn retry_job(id: String) -> Res<()> {
    minutes_core::jobs::requeue_job(&id).map_err(|e| e.to_string())?;
    processing::spawn_worker();
    Ok(())
}

// ---- library ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_meetings() -> Res<Vec<library::MeetingSummary>> {
    blocking(|| Ok(library::list())).await
}

#[tauri::command]
pub async fn read_meeting(path: String) -> Res<library::MeetingDetail> {
    blocking(move || library::read(&PathBuf::from(path))).await
}

#[tauri::command]
pub async fn save_notes(path: String, notes: String) -> Res<()> {
    blocking(move || {
        let p = PathBuf::from(path);
        library::ensure_inside_output(&p)?;
        library::replace_notes(&p, &notes)
    })
    .await
}

#[tauri::command]
pub async fn search_meetings(query: String) -> Res<Vec<library::SearchHit>> {
    blocking(move || Ok(library::search(&query))).await
}

#[tauri::command]
pub async fn trash_meeting(path: String) -> Res<()> {
    blocking(move || library::trash(&PathBuf::from(path))).await
}

#[tauri::command]
pub fn reveal_in_finder(path: String) {
    let _ = std::process::Command::new("/usr/bin/open").arg("-R").arg(path).spawn();
}

#[tauri::command]
pub fn open_in_obsidian(path: String) -> Res<()> {
    let p = PathBuf::from(&path);
    library::obsidian_vault_for(&p).ok_or("This meeting isn't inside an Obsidian vault.")?;
    let url = format!("obsidian://open?path={}", percent_encode(&path));
    std::process::Command::new("/usr/bin/open").arg(url).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

fn percent_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[tauri::command]
pub async fn rename_speaker(path: String, label: String, name: String, remember_voice: bool) -> Res<speakers::RenameResult> {
    blocking(move || speakers::rename(&PathBuf::from(path), &label, &name, remember_voice)).await
}

#[tauri::command]
pub async fn summarize_meeting(path: String) -> Res<()> {
    blocking(move || {
        let p = PathBuf::from(path);
        library::ensure_inside_output(&p)?;
        let cfg = corecfg::load();
        if cfg.summarization.engine == "none" {
            return Err("Summaries are off. Turn on Claude or Ollama in Settings → Summaries.".into());
        }
        minutes_core::resummarize::resummarize_meeting(
            &p,
            &cfg,
            &minutes_core::resummarize::ResummarizeOptions { apply: true, template_override: None, refresh: Default::default() },
        )
        .map(|_| ())
        .map_err(|e| format!("Summary failed: {e}"))
    })
    .await
}

#[tauri::command]
pub fn list_voices() -> Vec<speakers::VoiceProfileView> {
    speakers::list_voices()
}

#[tauri::command]
pub fn forget_voice(slug: String) -> Res<()> {
    speakers::forget_voice(&slug)
}

// ---- reset -----------------------------------------------------------------------------

#[tauri::command]
pub async fn reset_preview() -> Res<reset::ResetPreview> {
    blocking(|| Ok(reset::preview())).await
}

#[tauri::command]
pub async fn reset_app(app: AppHandle, options: reset::ResetOptions) -> Res<reset::ResetReport> {
    let report = blocking({
        let app = app.clone();
        move || reset::run(&app.state::<Recorder>(), &options)
    })
    .await?;
    crate::tray::refresh(&app);
    Ok(report)
}

#[tauri::command]
pub fn mcp_snippet() -> String {
    let dir = corecfg::load().output_dir.display().to_string();
    serde_json::to_string_pretty(&serde_json::json!({
        "mcpServers": { "minutes": { "command": "npx", "args": ["-y", "minutes-mcp"], "env": { "MEETINGS_DIR": dir } } }
    }))
    .unwrap_or_default()
}
