//! Margin's own small preferences file. Kept separate from minutes-core's config.toml.

use crate::paths;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub onboarding_complete: bool,
    /// "call" | "in-person": what the big Record button does.
    pub default_kind: Option<String>,
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
