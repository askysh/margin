//! Model downloads with progress. minutes-core has no public downloader (UPSTREAM-PATCHES.md
//! #2), so we use the same URLs core and its CLI use and write to the directories our config
//! points at.

use crate::{corecfg, paths};
use minutes_core::config::Config;
use minutes_core::diarize;
use serde::Serialize;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};

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
    let mut files = vec![ModelFile {
        id: format!("whisper-{model}"),
        label: format!("Transcription model ({label})"),
        url: format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{model}.bin"),
        dest: models.join(format!("ggml-{model}.bin")),
        approx_bytes: approx,
        installed: false,
    }];
    if cfg.transcription.vad_model == "silero-v6.2.0" {
        files.push(ModelFile {
            id: "vad".into(),
            label: "Voice activity detector".into(),
            url: "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v6.2.0.bin".into(),
            dest: models.join("ggml-silero-v6.2.0.bin"),
            approx_bytes: 885_000,
            installed: false,
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
    });
    files.push(ModelFile {
        id: "diarize-embedding".into(),
        label: "Voice recognition".into(),
        url: emb.url.into(),
        dest: paths::diarization_models_dir().join(emb.filename),
        approx_bytes: 28_000_000,
        installed: false,
    });
    for f in &mut files {
        f.installed = std::fs::metadata(&f.dest).map(|m| m.len() > 0).unwrap_or(false);
    }
    files
}

pub fn all_installed(cfg: &Config) -> bool {
    required(cfg).iter().all(|f| f.installed)
}

/// Download every missing model in the background. Emits `model-progress`.
pub fn download_missing(app: AppHandle) -> Result<(), String> {
    if DOWNLOADING.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let files: Vec<ModelFile> = required(&corecfg::load()).into_iter().filter(|f| !f.installed).collect();
    std::thread::Builder::new()
        .name("margin-models".into())
        .spawn(move || {
            let mut failed = None;
            for f in &files {
                if let Err(error) = fetch(f, &mut |p| {
                    let _ = app.emit("model-progress", p);
                }) {
                    let _ = app.emit(
                        "model-progress",
                        Progress { id: f.id.clone(), downloaded: 0, total: 0, finished: false, all_done: true, error: Some(error.clone()) },
                    );
                    failed = Some(error);
                    break;
                }
            }
            if failed.is_none() {
                let _ = app.emit(
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
    let response = ureq::get(&f.url)
        .call()
        .map_err(|e| format!("Couldn't download {}: {e}. Check your internet connection and try again.", f.label))?;
    let total = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(f.approx_bytes);
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
    if total > 0 && downloaded != total {
        let _ = std::fs::remove_file(&part);
        return Err(format!("{} download was incomplete ({downloaded} of {total} bytes). Try again.", f.label));
    }
    std::fs::rename(&part, &f.dest).map_err(|e| e.to_string())?;
    progress(Progress { id: f.id.clone(), downloaded, total, finished: true, all_done: false, error: None });
    Ok(())
}
