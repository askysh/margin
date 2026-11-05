//! Detects the optional summary backends: the user's own `claude` CLI, or a local Ollama.
//! Both are optional; with neither, meetings are saved with transcript and notes only.

use serde::Serialize;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Serialize)]
pub struct SummaryBackends {
    pub claude_path: Option<String>,
    pub ollama_running: bool,
    pub ollama_models: Vec<String>,
}

/// Finder-launched apps get a minimal PATH, so look in the usual install places and fall
/// back to asking a login shell.
pub fn find_claude() -> Option<String> {
    let home = crate::paths::home();
    let candidates: Vec<PathBuf> = vec![
        home.join(".local/bin/claude"),
        home.join(".claude/local/claude"),
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
    ];
    if let Some(found) = candidates.into_iter().find(|p| is_executable(p)) {
        return Some(found.display().to_string());
    }
    if let Ok(found) = which::which("claude") {
        return Some(found.display().to_string());
    }
    let out = std::process::Command::new("/bin/zsh")
        .args(["-lc", "command -v claude"])
        .output()
        .ok()?;
    let path = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!path.is_empty() && is_executable(std::path::Path::new(&path))).then_some(path)
}

fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

pub fn ollama_models(base_url: &str) -> Option<Vec<String>> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_millis(800)))
        .build()
        .new_agent();
    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    let body: serde_json::Value = agent.get(&url).call().ok()?.body_mut().read_json().ok()?;
    Some(
        body["models"]
            .as_array()
            .map(|models| {
                models
                    .iter()
                    .filter_map(|m| m["name"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
    )
}

pub fn detect(ollama_url: &str) -> SummaryBackends {
    let models = ollama_models(ollama_url);
    SummaryBackends {
        claude_path: find_claude(),
        ollama_running: models.is_some(),
        ollama_models: models.unwrap_or_default(),
    }
}
