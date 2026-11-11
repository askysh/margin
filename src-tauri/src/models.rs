//! Model downloads with progress. minutes-core has no public downloader (UPSTREAM-PATCHES.md
//! #2), so we use the same URLs core and its CLI use and write to the directories our config
//! points at.

use crate::{corecfg, events, paths};
use minutes_core::config::Config;
use minutes_core::diarize;
use serde::Serialize;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

static DOWNLOADING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize)]
pub struct ModelFile {
    pub id: String,
    pub label: String,
    pub url: String,
    #[serde(skip)]
    pub dest: PathBuf,
    pub approx_bytes: u64,
    pub installed: bool,
    /// The live transcript model. Recording does not wait for it.
    pub live: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub id: String,
    pub downloaded: u64,
    pub total: u64,
    pub finished: bool,
    pub all_done: bool,
    pub error: Option<String>,
}

pub fn required(cfg: &Config) -> Vec<ModelFile> {
    let models = paths::models_dir();
    let model = &cfg.transcription.model;
    let (label, approx) = corecfg::MODELS
        .iter()
        .find(|(id, _, _)| id == model)
        .map(|(_, label, bytes)| (*label, *bytes))
        .unwrap_or(("Custom", 0));
    let whisper = |model: &str, label: String, approx: u64| ModelFile {
        id: format!("whisper-{model}"),
        label,
        url: format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{model}.bin"),
        dest: models.join(format!("ggml-{model}.bin")),
        approx_bytes: approx,
        installed: false,
        live: false,
    };
    let mut files = vec![whisper(model, format!("Transcription model ({label})"), approx)];
    // The live transcript uses a smaller model when the chosen one is too slow for it.
    let live = corecfg::live_model(model);
    if &live != model {
        let approx = corecfg::MODELS.iter().find(|(id, _, _)| *id == live).map(|(_, _, b)| *b).unwrap_or(0);
        files.push(ModelFile { live: true, ..whisper(&live, "Live transcript model".into(), approx) });
    }
    if cfg.transcription.vad_model == "silero-v6.2.0" {
        files.push(ModelFile {
            id: "vad".into(),
            label: "Voice activity detector".into(),
            url: "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v6.2.0.bin".into(),
            dest: models.join("ggml-silero-v6.2.0.bin"),
            approx_bytes: 885_000,
            installed: false,
            live: false,
        });
    }
    let seg = diarize::diarization_model_for_config(cfg);
    let emb = diarize::embedding_model_for_config(cfg);
    files.push(ModelFile {
        id: "diarize-segmentation".into(),
        label: "Speaker separation".into(),
        url: seg.segmentation_url.into(),
        dest: paths::diarization_models_dir().join(seg.segmentation_filename),
        approx_bytes: 5_900_000,
        installed: false,
        live: false,
    });
    files.push(ModelFile {
        id: "diarize-embedding".into(),
        label: "Voice recognition".into(),
        url: emb.url.into(),
        dest: paths::diarization_models_dir().join(emb.filename),
        approx_bytes: 28_000_000,
        installed: false,
        live: false,
    });
    for f in &mut files {
        f.installed = std::fs::metadata(&f.dest).map(|m| m.len() > 0).unwrap_or(false);
    }
    files
}

pub fn all_installed(cfg: &Config) -> bool {
    required(cfg).iter().all(|f| f.installed)
}

/// Every model that recording needs is on disk. The live transcript model is not one of
/// them: without it the live transcript is empty, and the meeting is still transcribed.
pub fn ready_to_record(cfg: &Config) -> bool {
    required(cfg).iter().all(|f| f.installed || f.live)
}

/// The live transcript model to use for this recording. A Best user who updated from 0.0.8
/// has `base` but not yet `small` (D52). Until `small` is downloaded, use `base`.
pub fn live_model_on_disk(cfg: &Config) -> String {
    let on_disk = |m: &str| paths::models_dir().join(format!("ggml-{m}.bin")).exists();
    pick_live(&cfg.live_transcript.model, on_disk)
}

fn pick_live(want: &str, on_disk: impl Fn(&str) -> bool) -> String {
    if !on_disk(want) && on_disk("base") { "base".into() } else { want.into() }
}

/// Download every missing model in the background. Emits `model-progress`.
pub fn download_missing() -> Result<(), String> {
    if DOWNLOADING.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let files: Vec<ModelFile> = required(&corecfg::load()).into_iter().filter(|f| !f.installed).collect();
    std::thread::Builder::new()
        .name("margin-models".into())
        .spawn(move || {
            let mut failed = None;
            for f in &files {
                if let Err(error) = fetch(f, &mut |p| events::emit("model-progress", p)) {
                    events::emit(
                        "model-progress",
                        Progress { id: f.id.clone(), downloaded: 0, total: 0, finished: false, all_done: true, error: Some(error.clone()) },
                    );
                    failed = Some(error);
                    break;
                }
            }
            if failed.is_none() {
                events::emit(
                    "model-progress",
                    Progress { id: "all".into(), downloaded: 0, total: 0, finished: true, all_done: true, error: None },
                );
            }
            DOWNLOADING.store(false, Ordering::SeqCst);
        })
        .map_err(|e| {
            DOWNLOADING.store(false, Ordering::SeqCst);
            e.to_string()
        })?;
    Ok(())
}

pub fn fetch(f: &ModelFile, progress: &mut dyn FnMut(Progress)) -> Result<(), String> {
    if let Some(dir) = f.dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let part = f.dest.with_extension("part");
    // A model is up to 1.6 GB, so there is no limit on the whole download. A connection
    // that stops sending for a minute fails, so the download does not wait for ever.
    let agent = ureq::Agent::config_builder()
        .timeout_connect(Some(std::time::Duration::from_secs(20)))
        .timeout_recv_response(Some(std::time::Duration::from_secs(60)))
        .timeout_per_read(Some(std::time::Duration::from_secs(60)))
        .build()
        .new_agent();
    let response = agent
        .get(&f.url)
        .call()
        .map_err(|e| format!("Couldn't download {}: {e}. Check your internet connection and try again.", f.label))?;
    // The size the server sends. The estimate is for the progress bar only: a file of
    // another size is not an incomplete download.
    let length = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let total = length.unwrap_or(f.approx_bytes);
    let mut reader = response.into_body().into_reader();
    let mut out = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 256 * 1024];
    let mut downloaded = 0u64;
    let mut last_emit = std::time::Instant::now();
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("Download of {} was interrupted: {e}", f.label))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|e| format!("Couldn't save {}: {e}", f.label))?;
        downloaded += n as u64;
        if last_emit.elapsed().as_millis() > 150 {
            last_emit = std::time::Instant::now();
            progress(Progress { id: f.id.clone(), downloaded, total, finished: false, all_done: false, error: None });
        }
    }
    out.sync_all().map_err(|e| e.to_string())?;
    if let Err(error) = check_size(downloaded, length) {
        let _ = std::fs::remove_file(&part);
        return Err(format!("{} {error}", f.label));
    }
    std::fs::rename(&part, &f.dest).map_err(|e| e.to_string())?;
    progress(Progress { id: f.id.clone(), downloaded, total: downloaded, finished: true, all_done: false, error: None });
    Ok(())
}

/// A download is complete when it has the size the server sent. Without a size, any
/// file that is not empty is accepted.
fn check_size(downloaded: u64, length: Option<u64>) -> Result<(), String> {
    match length {
        Some(total) if downloaded != total => Err(format!("download was incomplete ({downloaded} of {total} bytes). Try again.")),
        None if downloaded == 0 => Err("download was empty. Try again.".into()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_size_check_uses_only_the_size_the_server_sent() {
        assert!(check_size(100, Some(100)).is_ok());
        assert!(check_size(99, Some(100)).is_err());
        // No Content-Length: the estimate is not the size of the file.
        assert!(check_size(487_601_967, None).is_ok());
        assert!(check_size(0, None).is_err());
    }

    #[test]
    fn live_model_falls_back_to_base_until_small_is_downloaded() {
        assert_eq!(pick_live("small", |m| m == "small" || m == "base"), "small");
        assert_eq!(pick_live("small", |m| m == "base"), "base");
        // Without `base` too, keep the wanted model. The live transcript is then empty.
        assert_eq!(pick_live("small", |_| false), "small");
    }
}
