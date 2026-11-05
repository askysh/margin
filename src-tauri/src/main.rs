#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod corecfg;
mod devcli;
mod fsutil;
mod library;
mod models;
mod paths;
mod permissions;
mod prefs;
mod processing;
mod recorder;
mod reset;
mod speakers;
mod summaries;
mod tray;

use recorder::Recorder;
use tauri::{Manager, RunEvent, WindowEvent};

fn main() {
    // Isolate minutes-core from any other Minutes install before anything reads its env.
    paths::install_env();

    // minutes-core re-executes this binary for a few sandboxed worker roles.
    if let Some(code) = minutes_core::graph_worker::maybe_run_policy_projection_worker() {
        std::process::exit(code);
    }
    if let Some(code) = minutes_core::audio_decode_worker::maybe_run_audio_decode_worker() {
        std::process::exit(code);
    }
    minutes_core::install_whisper_logging_hooks();
    init_logging();

    if std::env::args().any(|a| a == processing::WORKER_FLAG) {
        minutes_core::exit_without_cxx_teardown(processing::run_worker_mode());
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--dev") {
        minutes_core::exit_without_cxx_teardown(devcli::run(&args[1..]));
    }

    extend_path();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Recorder::default())
        .setup(|app| {
            let handle = app.handle().clone();
            if let Some(window) = app.get_webview_window("main") {
                #[cfg(target_os = "macos")]
                {
                    use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};
                    let _ = apply_vibrancy(&window, NSVisualEffectMaterial::Sidebar, None, None);
                }
                let _ = window.show();
            }
            tray::init(&handle)?;
            processing::spawn_watcher(handle);
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window keeps Margin in the menu bar (and any recording going).
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::boot,
            commands::save_settings,
            commands::folder_choices,
            commands::complete_onboarding,
            commands::models_status,
            commands::download_models,
            commands::permissions_state,
            commands::request_permission,
            commands::open_privacy_settings,
            commands::summary_backends,
            commands::start_recording,
            commands::stop_recording,
            commands::recorder_view,
            commands::set_draft_notes,
            commands::set_recording_title,
            commands::discard_orphaned_draft,
            commands::list_jobs,
            commands::retry_job,
            commands::list_meetings,
            commands::read_meeting,
            commands::save_notes,
            commands::search_meetings,
            commands::trash_meeting,
            commands::reveal_in_finder,
            commands::open_in_obsidian,
            commands::rename_speaker,
            commands::summarize_meeting,
            commands::list_voices,
            commands::forget_voice,
            commands::reset_preview,
            commands::reset_app,
            commands::mcp_snippet,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Margin");

    app.run(|app, event| match event {
        RunEvent::Reopen { .. } => tray::show_main(app),
        RunEvent::ExitRequested { .. } => {
            // Cmd-Q while recording: stop and queue the meeting rather than lose it.
            let recorder = app.state::<Recorder>();
            if recorder.is_recording() {
                let _ = recorder.stop(app);
            }
        }
        _ => {}
    });
}

fn init_logging() {
    use tracing_subscriber::EnvFilter;
    let dir = paths::state_dir().join("logs");
    let _ = std::fs::create_dir_all(&dir);
    let filter = EnvFilter::try_from_env("MARGIN_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,whisper_rs=warn,ggml=warn,ort=warn"));
    let builder = tracing_subscriber::fmt().with_env_filter(filter).with_ansi(false);
    match std::fs::OpenOptions::new().create(true).append(true).open(dir.join("margin.log")) {
        Ok(file) => builder.with_writer(std::sync::Mutex::new(file)).init(),
        Err(_) => builder.init(),
    }
}

/// Finder-launched apps get PATH=/usr/bin:/bin:/usr/sbin:/sbin. Summary agents (claude) and
/// ffmpeg usually live elsewhere.
fn extend_path() {
    let home = paths::home();
    let mut parts: Vec<String> = std::env::var("PATH").unwrap_or_default().split(':').map(String::from).collect();
    for extra in [
        "/opt/homebrew/bin".to_string(),
        "/usr/local/bin".to_string(),
        home.join(".local/bin").display().to_string(),
    ] {
        if !parts.contains(&extra) {
            parts.push(extra);
        }
    }
    std::env::set_var("PATH", parts.join(":"));
}
