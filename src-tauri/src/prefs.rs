//! Margin's own small preferences file. Kept separate from minutes-core's config.toml.

use crate::paths;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub onboarding_complete: bool,
    /// "call" | "in-person": what the big Record button does.
    pub default_kind: Option<String>,
    /// Sounds on start, stop, ready and failure. Stored inverted so a missing key is on.
    pub mute_cues: bool,
    /// The global start/stop shortcut (`RECORD_SHORTCUT`). Off unless you turn it on.
    pub record_shortcut: bool,
    /// Check for updates and download them. Stored inverted so a missing key is on.
    pub no_auto_update: bool,
    /// Transcribe during the meeting (rolling.rs). Stored inverted so a missing key is on.
    pub no_rolling_transcription: bool,
    /// The recording chip (chip.rs). Stored inverted so a missing key is on.
    pub no_recording_chip: bool,
    /// Where the chip sits on the right edge: the middle of the chip as a fraction of the
    /// visible screen height, from the top. None means the default place.
    pub chip_place: Option<f64>,
}

/// The preferences the UI can change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppPrefs {
    pub sound_cues: bool,
    pub record_shortcut: bool,
    #[serde(default = "yes")]
    pub auto_update: bool,
    /// Hide the window and show the recording chip while Margin records.
    #[serde(default = "yes")]
    pub recording_chip: bool,
}

fn yes() -> bool {
    true
}

impl Prefs {
    pub fn recording_chip(&self) -> bool {
        !self.no_recording_chip
    }

    pub fn app(&self) -> AppPrefs {
        AppPrefs {
            sound_cues: !self.mute_cues,
            record_shortcut: self.record_shortcut,
            auto_update: !self.no_auto_update,
            recording_chip: !self.no_recording_chip,
        }
    }
}

pub fn load() -> Prefs {
    std::fs::read(paths::prefs_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save(prefs: &Prefs) -> Result<(), String> {
    let path = paths::prefs_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_vec_pretty(prefs).map_err(|e| e.to_string())?;
    crate::fsutil::write_atomic(&path, &json, None).map_err(|e| e.to_string())
}
