//! Where Margin keeps its files, and the environment that isolates minutes-core from any
//! other Minutes install on the same Mac (see DECISIONS.md D3).
//!
//! On Windows (D61) the app root is `%LOCALAPPDATA%Margin`, and the default meetings
//! folder is `Meetings` in the Documents folder that Windows gives. Nothing is written
//! next to `margin.exe`: a Store package folder is read-only.

use std::path::PathBuf;

/// `~/Library/Application Support/Margin`, or `$MARGIN_HOME` when set (used by tests and
/// for trying the app against a throwaway directory).
pub fn app_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("MARGIN_HOME").map(PathBuf::from) {
        if dir.is_absolute() {
            return dir;
        }
    }
    // The local folder, not the roaming one: the models are large, and a roaming profile
    // copies its folder to each PC of the account.
    #[cfg(windows)]
    if let Some(dir) = dirs::data_local_dir() {
        return dir.join("Margin");
    }
    dirs::data_dir()
        .unwrap_or_else(|| home().join("Library/Application Support"))
        .join("Margin")
}

pub fn home() -> PathBuf {
    // `dirs` asks Windows for the profile folder, and Windows ignores the environment. A
    // sandbox sets HOME and USERPROFILE (AGENTS.md), so read them first, in the order that
    // minutes-core's config uses. A normal launch has the real profile in USERPROFILE.
    #[cfg(windows)]
    for name in ["HOME", "USERPROFILE"] {
        if let Some(dir) = std::env::var_os(name).map(PathBuf::from).filter(|d| d.is_absolute()) {
            return dir;
        }
    }
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// The minutes-core config file (`MINUTES_CONFIG_PATH`).
pub fn config_path() -> PathBuf {
    app_root().join("config.toml")
}

/// The minutes-core state directory (`MINUTES_DATA_DIR` and `MINUTES_HOME`).
pub fn state_dir() -> PathBuf {
    app_root().join("state")
}

pub fn models_dir() -> PathBuf {
    state_dir().join("models")
}

pub fn diarization_models_dir() -> PathBuf {
    models_dir().join("diarization")
}

/// Margin's own preferences (onboarding state). Kept out of config.toml on purpose.
pub fn prefs_path() -> PathBuf {
    app_root().join("prefs.json")
}

/// Notes typed during a recording, kept until the meeting file exists.
pub fn drafts_dir() -> PathBuf {
    app_root().join("drafts")
}

pub fn default_output_dir() -> PathBuf {
    // Windows can keep Documents in another place, such as OneDrive. Ask Windows where it
    // is. A sandbox (MARGIN_HOME) never uses the real Documents folder.
    #[cfg(windows)]
    if std::env::var_os("MARGIN_HOME").is_none() {
        if let Some(documents) = dirs::document_dir() {
            return documents.join("Meetings");
        }
    }
    home().join("Documents").join("Meetings")
}

/// Point minutes-core at our directories. Must run before any other thread starts and
/// before minutes-core is touched; child processes inherit it.
pub fn install_env() {
    let state = state_dir();
    ensure_dirs();
    std::env::set_var("MINUTES_CONFIG_PATH", config_path());
    std::env::set_var("MINUTES_DATA_DIR", &state);
    std::env::set_var("MINUTES_HOME", &state);
}

pub fn ensure_dirs() {
    let _ = std::fs::create_dir_all(state_dir());
    let _ = std::fs::create_dir_all(drafts_dir());
}
