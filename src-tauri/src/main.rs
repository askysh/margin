#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod calldetect;
mod commands;
mod corecfg;
mod devcli;
mod devserve;
mod events;
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

use tauri::{Emitter, Manager, RunEvent, WindowEvent};

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
    extend_path();

    if std::env::args().any(|a| a == processing::WORKER_FLAG) {
        minutes_core::exit_without_cxx_teardown(processing::run_worker_mode());
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--dev") {
        minutes_core::exit_without_cxx_teardown(devcli::run(&args[1..]));
    }

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let sink = handle.clone();
            events::set_sink(Box::new(move |name, payload| {
                if name == events::TRAY_REFRESH {
                    tray::refresh(&sink);
                } else {
                    let _ = sink.emit(name, payload);
                }
            }));
            if let Some(window) = app.get_webview_window("main") {
                #[cfg(target_os = "macos")]
                {
                    use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};
                    let _ = apply_vibrancy(&window, NSVisualEffectMaterial::Sidebar, None, None);
                }
                let _ = window.show();
            }
            tray::init(&handle)?;
            processing::spawn_watcher();
            calldetect::spawn();
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
            commands::tauri_cmds::boot,
            commands::tauri_cmds::save_settings,
            commands::tauri_cmds::folder_choices,
            commands::tauri_cmds::complete_onboarding,
            commands::tauri_cmds::models_status,
            commands::tauri_cmds::download_models,
            commands::tauri_cmds::permissions_state,
            commands::tauri_cmds::request_permission,
            commands::tauri_cmds::open_privacy_settings,
            commands::tauri_cmds::summary_backends,
            commands::tauri_cmds::start_recording,
            commands::tauri_cmds::stop_recording,
            commands::tauri_cmds::recorder_view,
            commands::tauri_cmds::set_draft_notes,
            commands::tauri_cmds::set_recording_title,
            commands::tauri_cmds::discard_orphaned_draft,
            commands::tauri_cmds::list_jobs,
            commands::tauri_cmds::retry_job,
            commands::tauri_cmds::list_meetings,
            commands::tauri_cmds::read_meeting,
            commands::tauri_cmds::save_notes,
            commands::tauri_cmds::search_meetings,
            commands::tauri_cmds::set_meeting_title,
            commands::tauri_cmds::trash_meeting,
            commands::tauri_cmds::reveal_in_finder,
            commands::tauri_cmds::open_in_obsidian,
            commands::tauri_cmds::rename_speaker,
            commands::tauri_cmds::summarize_meeting,
            commands::tauri_cmds::list_voices,
            commands::tauri_cmds::forget_voice,
            commands::tauri_cmds::reset_preview,
            commands::tauri_cmds::reset_app,
            commands::tauri_cmds::mcp_snippet,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Margin");

    app.run(|app, event| match event {
        RunEvent::Reopen { .. } => tray::show_main(app),
        RunEvent::ExitRequested { api, .. } => {
            // Cmd-Q while recording (or while a stop is finishing): save the meeting first,
            // off the main thread, then quit for real.
            if recorder::recorder().is_recording() {
                api.prevent_exit();
                let app = app.clone();
                std::thread::spawn(move || {
                    recorder::recorder().finish_before_exit(std::time::Duration::from_secs(120));
                    app.exit(0);
                });
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
