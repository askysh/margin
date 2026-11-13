#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod applespeech;
mod audio;
mod calldetect;
mod chat;
#[cfg(target_os = "macos")]
mod chip;
// No recording chip on Windows yet (D61): the same calls, and each one does nothing.
#[cfg(not(target_os = "macos"))]
#[path = "chip_off.rs"]
mod chip;
mod commands;
mod corecfg;
mod devcli;
mod devserve;
mod events;
mod extras;
mod fsutil;
mod library;
mod models;
#[cfg(target_os = "macos")]
mod notepad;
// The notepad view with Tauri's own window calls, in place of AppKit.
#[cfg(not(target_os = "macos"))]
#[path = "notepad_plain.rs"]
mod notepad;
mod notify;
mod paths;
mod permissions;
mod piece_cut;
mod prefs;
mod processing;
mod recorder;
mod reset;
mod rolling;
mod screenshare;
mod shortcut;
mod speakers;
mod summaries;
mod tray;
mod updater;
#[cfg(windows)]
mod win;

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
    let worker = std::env::args().any(|a| a == processing::WORKER_FLAG);
    let dev = std::env::args().nth(1).as_deref() == Some("--dev");
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        pid = std::process::id(),
        role = if worker { "worker" } else if dev { "dev" } else { "app" },
        "Margin started"
    );

    if worker {
        extras::use_state_cwd();
        minutes_core::exit_without_cxx_teardown(processing::run_worker_mode());
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--dev") {
        minutes_core::exit_without_cxx_teardown(devcli::run(&args[1..]));
    }
    extras::use_state_cwd();
    // Opened by "Open at login": start in the menu bar only.
    let at_login = args.iter().any(|a| a == extras::LOGIN_ARG);

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(shortcut::plugin())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            let handle = app.handle().clone();
            let sink = handle.clone();
            events::set_sink(Box::new(move |name, payload| route(&sink, name, payload)));
            if let Some(window) = app.get_webview_window("main") {
                #[cfg(target_os = "macos")]
                {
                    use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};
                    let _ = apply_vibrancy(&window, NSVisualEffectMaterial::Sidebar, None, None);
                }
                let _ = window.set_content_protected(screenshare::hidden());
                let _ = if at_login { window.hide() } else { window.show() };
                // The notepad view follows the width (D57). Log the first size, so a window
                // that opens narrow can be explained.
                if let (Ok(size), Ok(scale)) = (window.inner_size(), window.scale_factor()) {
                    tracing::info!(width = size.width as f64 / scale, height = size.height as f64 / scale, "main window opened");
                }
            }
            notepad::install_menu(&handle)?;
            tray::init(&handle)?;
            notify::init(&handle);
            shortcut::refresh(&handle);
            summaries::recover_notes_drafts();
            processing::spawn_watcher();
            // A model that this version needs and an earlier one did not, such as the live
            // transcript model for Best (D52), downloads in the background.
            if prefs::load().onboarding_complete && !models::all_installed(&corecfg::load()) {
                if let Err(error) = models::download_missing() {
                    tracing::warn!(%error, "could not start the model download");
                }
            }
            // Windows cannot record a call yet (D61), so it does not suggest one.
            if cfg!(target_os = "macos") {
                calldetect::spawn();
            }
            updater::spawn(&handle);
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            match event {
                // Closing the window keeps Margin in the menu bar (and any recording going).
                WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    let _ = window.hide();
                    chip::sync(window.app_handle());
                }
                // Minimize, restore and focus changes can show or hide the window. The
                // recorder's tick also checks, for changes with no event, such as ⌘H.
                WindowEvent::Focused(_) => chip::sync(window.app_handle()),
                WindowEvent::Resized(_) => {
                    chip::sync(window.app_handle());
                    notepad::sync_menu(window.app_handle());
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::tauri_cmds::boot,
            commands::tauri_cmds::save_settings,
            commands::tauri_cmds::settings_options,
            commands::tauri_cmds::save_app_prefs,
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
            commands::tauri_cmds::shrink_to_chip,
            commands::tauri_cmds::toggle_notepad,
            commands::tauri_cmds::chip_open_notes,
            commands::tauri_cmds::chip_stop,
            chip::chip_hello,
            chip::chip_width,
            chip::chip_drag_start,
            chip::chip_drag,
            chip::chip_drag_end,
            chip::chip_nudge,
            chip::chip_release_focus,
            commands::tauri_cmds::set_draft_notes,
            commands::tauri_cmds::set_recording_title,
            commands::tauri_cmds::discard_orphaned_draft,
            commands::tauri_cmds::list_jobs,
            commands::tauri_cmds::live_draft,
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
            commands::tauri_cmds::set_action_item,
            audio::meeting_audio,
            commands::tauri_cmds::list_voices,
            commands::tauri_cmds::forget_voice,
            commands::tauri_cmds::reset_preview,
            commands::tauri_cmds::reset_app,
            commands::tauri_cmds::mcp_snippet,
            commands::tauri_cmds::chat_status,
            commands::tauri_cmds::chat_thread,
            commands::tauri_cmds::chat_send,
            commands::tauri_cmds::chat_stop,
            commands::tauri_cmds::chat_clear,
            update_status,
            check_updates,
            install_update,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Margin");

    app.run(|app, event| match event {
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => tray::show_main(app),
        RunEvent::ExitRequested { api, code, .. } => {
            // Cmd-Q while recording or processing: save the meeting and ask first, off the
            // main thread (tray::quit). `code` is set when tray::quit itself exits.
            if code.is_none() && (recorder::recorder().is_recording() || processing::busy()) {
                api.prevent_exit();
                let app = app.clone();
                std::thread::spawn(move || tray::quit(&app));
            }
        }
        // Quitting with an update downloaded installs it (not during a recording).
        RunEvent::Exit => updater::install_on_quit(),
        _ => {}
    });
}

#[tauri::command]
fn update_status() -> updater::UpdateStatus {
    updater::status()
}

#[tauri::command]
async fn check_updates(app: tauri::AppHandle) -> updater::UpdateStatus {
    tauri::async_runtime::spawn_blocking(move || updater::check(&app)).await.unwrap_or_else(|_| updater::status())
}

#[tauri::command]
async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || updater::install_and_restart(&app)).await.map_err(|e| e.to_string())?
}

/// Where a backend event goes in the app: host actions (menu bar, shortcut, system
/// notifications) are handled here; everything else goes to the window.
fn route(app: &tauri::AppHandle, name: &str, payload: serde_json::Value) {
    match name {
        events::TRAY_REFRESH => tray::refresh(app),
        events::SHORTCUT_REFRESH => shortcut::refresh(app),
        events::SCREEN_SHARE_REFRESH => screenshare::apply(app),
        events::NOTIFY => {
            if let Ok(notice) = serde_json::from_value(payload) {
                notify::show(app, notice);
            }
        }
        _ => {
            match name {
                events::SHRINK_TO_CHIP => chip::shrink(app),
                events::NOTEPAD_TOGGLE => notepad::toggle(app),
                events::OPEN_LIVE => tray::show_main(app),
                "recorder" => chip::sync(app),
                "recorder-tick" => {
                    tray::tick(app, payload["elapsed_secs"].as_u64().unwrap_or(0));
                    chip::sync(app);
                }
                _ => {}
            }
            let _ = app.emit(name, payload);
        }
    }
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

/// Windows: a program started from the Start menu has the full PATH of the account. Add
/// the two places where `claude` installs, for a PATH that an installer has not changed
/// yet. Windows separates PATH entries with a semicolon, so the standard library splits it.
#[cfg(windows)]
fn extend_path() {
    let mut parts: Vec<std::path::PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    let npm = dirs::data_dir().map(|d| d.join("npm"));
    for extra in [Some(paths::home().join(".local").join("bin")), npm].into_iter().flatten() {
        if !parts.contains(&extra) {
            parts.push(extra);
        }
    }
    if let Ok(path) = std::env::join_paths(parts) {
        std::env::set_var("PATH", path);
    }
}

/// Finder-launched apps get PATH=/usr/bin:/bin:/usr/sbin:/sbin. Summary agents (claude) and
/// ffmpeg usually live elsewhere.
#[cfg(not(windows))]
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
