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
    /// No download exists: the user copies the file to `dest` (corecfg::LOCAL_ONLY_MODELS).
    pub local_only: bool,
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
    let whisper = |model: &str, label: String, approx: u64| {
        let local_only = corecfg::LOCAL_ONLY_MODELS.contains(&model);
        ModelFile {
            id: format!("whisper-{model}"),
            label,
            url: if local_only {
                String::new()
            } else {
                format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{model}.bin")
            },
            dest: models.join(format!("ggml-{model}.bin")),
            approx_bytes: approx,
            installed: false,
            live: false,
            local_only,
        }
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
            local_only: false,
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
        local_only: false,
    });
    files.push(ModelFile {
        id: "diarize-embedding".into(),
        label: "Voice recognition".into(),
        url: emb.url.into(),
        dest: paths::diarization_models_dir().join(emb.filename),
        approx_bytes: 28_000_000,
        installed: false,
        live: false,
        local_only: false,
    });
    for f in &mut files {
        f.installed = if f.local_only { local_model_installed(&f.dest) } else { file_installed(&f.dest) };
    }
    files
}

/// A model file counts as installed when it is there and not empty.
pub fn file_installed(path: &std::path::Path) -> bool {
    std::fs::metadata(path).map(|m| m.len() > 0).unwrap_or(false)
}

/// A local-only model is copied by hand, so a half-copied file is likely: it counts as
/// installed only from `corecfg::MIN_LOCAL_MODEL_BYTES` (the Hinglish q5_0 is 1.08 GB).
pub fn local_model_installed(path: &std::path::Path) -> bool {
    std::fs::metadata(path).map(|m| m.len() >= corecfg::MIN_LOCAL_MODEL_BYTES).unwrap_or(false)
}

pub fn all_installed(cfg: &Config) -> bool {
    required(cfg).iter().all(|f| f.installed)
}

/// Every model that recording needs is on disk. The live transcript model is not one of
/// them: without it the live transcript is empty, and the meeting is still transcribed.
pub fn ready_to_record(cfg: &Config) -> bool {
    let fallback = local_model_fallback(cfg).is_some();
    required(cfg).iter().all(|f| f.installed || f.live || (f.local_only && fallback))
}

/// The first installed model of `corecfg::MISSING_LOCAL_MODEL_FALLBACKS`, while the chosen
/// local-only model's file is missing: something can transcribe. Core may still skip it when
/// it fails to load, so never name it as the model that will run.
pub fn local_model_fallback(cfg: &Config) -> Option<String> {
    missing_local_model(cfg)?;
    pick_fallback(|m| file_installed(&paths::models_dir().join(format!("ggml-{m}.bin"))))
}

/// Said once when a recording starts while `file` (the chosen local-only model) is missing.
/// It names no model: which one runs is core's choice (`ModelFallback` in the meeting file).
pub fn local_model_fallback_notice(cfg: &Config, file: &ModelFile) -> events::Notice {
    let language = match cfg.transcription.missing_model_fallback_language.as_deref() {
        None => "auto-detect",
        Some(code) => corecfg::LANGUAGES.iter().find(|(id, _)| *id == code).map_or(code, |(_, name)| name),
    };
    events::Notice {
        title: "Hindi + English model file is missing or unreadable".into(),
        body: format!(
            "This meeting uses another installed model, language {language}. Copy ggml-hinglish.bin to {} to use it again.",
            file.dest.parent().unwrap_or(&file.dest).display()
        ),
        meeting: None,
        call_app: None,
        calendar: None,
    }
}

fn pick_fallback(installed: impl Fn(&str) -> bool) -> Option<String> {
    corecfg::MISSING_LOCAL_MODEL_FALLBACKS.iter().find(|m| installed(m)).map(|m| m.to_string())
}

/// The chosen model when it is local-only and its file is not in the models folder. Margin
/// cannot download it, so recording says where the file goes instead of "still getting".
pub fn missing_local_model(cfg: &Config) -> Option<ModelFile> {
    required(cfg).into_iter().find(|f| f.local_only && !f.installed)
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
    // A missing local-only model cannot be downloaded: leave it to the user and get the rest.
    let files: Vec<ModelFile> = required(&corecfg::load()).into_iter().filter(|f| !f.installed && !f.local_only).collect();
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
    if f.local_only {
        return Err(format!(
            "{} has no download. Copy its file to {} and try again.",
            f.label,
            f.dest.display()
        ));
    }
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
    fn a_local_only_model_is_never_downloaded() {
        let mut cfg = Config::default();
        cfg.transcription.model = "hinglish".into();
        let files = required(&cfg);
        let main = files.iter().find(|f| f.id == "whisper-hinglish").unwrap();
        assert!(main.local_only && main.url.is_empty());
        assert!(main.dest.ends_with("ggml-hinglish.bin"));
        let err = fetch(main, &mut |_| {}).unwrap_err();
        assert!(err.contains("no download"), "{err}");
        assert_eq!(missing_local_model(&cfg).map(|f| f.id), Some("whisper-hinglish".into()));
        cfg.transcription.model = "small".into();
        assert!(missing_local_model(&cfg).is_none());
        assert!(files.iter().all(|f| f.id == "whisper-hinglish" || !f.local_only));
    }

    #[test]
    fn the_fallback_notice_names_the_language_but_no_model() {
        let mut cfg = Config::default();
        cfg.transcription.model = "hinglish".into();
        cfg.transcription.missing_model_fallback_language = Some("de".into());
        let file = required(&cfg).into_iter().find(|f| f.local_only).unwrap();
        let notice = local_model_fallback_notice(&cfg, &file);
        let text = format!("{} {}", notice.title, notice.body);
        assert!(text.contains("missing or unreadable") && text.contains("another installed model, language German"), "{text}");
        for model in corecfg::MISSING_LOCAL_MODEL_FALLBACKS {
            assert!(!text.contains(model), "core may skip {model}: {text}");
        }
        cfg.transcription.missing_model_fallback_language = None;
        assert!(local_model_fallback_notice(&cfg, &file).body.contains("language auto-detect"));
    }

    #[test]
    fn a_half_copied_local_model_is_not_installed() {
        let dir = std::env::temp_dir().join(format!("margin-local-model-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("ggml-hinglish.bin");
        std::fs::write(&file, b"ggml").unwrap();
        assert!(file_installed(&file) && !local_model_installed(&file));
        std::fs::File::options().write(true).open(&file).unwrap().set_len(corecfg::MIN_LOCAL_MODEL_BYTES).unwrap();
        assert!(local_model_installed(&file));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_local_model_falls_back_to_turbo_then_small() {
        assert_eq!(pick_fallback(|m| m == "small" || m == "large-v3-turbo").as_deref(), Some("large-v3-turbo"));
        assert_eq!(pick_fallback(|m| m == "small").as_deref(), Some("small"));
        assert_eq!(pick_fallback(|_| false), None);
    }

    #[test]
    fn live_model_falls_back_to_base_until_small_is_downloaded() {
        assert_eq!(pick_live("small", |m| m == "small" || m == "base"), "small");
        assert_eq!(pick_live("small", |m| m == "base"), "base");
        // Without `base` too, keep the wanted model. The live transcript is then empty.
        assert_eq!(pick_live("small", |_| false), "small");
    }
}
