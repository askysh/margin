//! Where Margin keeps its files, and the environment that isolates minutes-core from any
//! other Minutes install on the same Mac (see DECISIONS.md D3).

use std::path::PathBuf;

/// `~/Library/Application Support/Margin`, or `$MARGIN_HOME` when set (used by tests and
/// for trying the app against a throwaway directory).
pub fn app_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("MARGIN_HOME").map(PathBuf::from) {
        if dir.is_absolute() {
            return dir;
        }
    }
    dirs::data_dir()
        .unwrap_or_else(|| home().join("Library/Application Support"))
        .join("Margin")
}

pub fn home() -> PathBuf {
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
    home().join("Documents").join("Meetings")
}

/// Point minutes-core at our directories. Must run before any other thread starts and
/// before minutes-core is touched; child processes inherit it.
pub fn install_env() {
    let state = state_dir();
    let _ = std::fs::create_dir_all(&state);
    let _ = std::fs::create_dir_all(drafts_dir());
    std::env::set_var("MINUTES_CONFIG_PATH", config_path());
    std::env::set_var("MINUTES_DATA_DIR", &state);
    std::env::set_var("MINUTES_HOME", &state);
}
