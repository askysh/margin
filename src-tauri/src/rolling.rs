//! Rolling transcription (DECISIONS.md "Rolling transcription", docs/research/live-transcription.md).
//!
//! While a meeting records, a background thread transcribes each source in pieces of
//! about two minutes: the mic and call stems for a call, the recording itself in person.
//! Each piece goes through core's `transcript_checkpoint::transcribe_span`, which uses the
//! same chunking, model and settings as the final pass. A piece ends where core's chunker
//! resets (piece_cut.rs), so its chunks, and so its words, are those of the final pass.
//! At Stop, the finished pieces go to the job as a transcript checkpoint, and core
//! transcribes only what they don't cover, usually the last minute or two. Core checks
//! every piece against the final audio and settings, and transcribes again any range whose
//! piece does not match.
//!
//! The thread only reads the growing capture files and never blocks the capture. It runs
//! at utility priority. A failed piece is dropped (the final pass covers that range). If
//! the thread falls behind, or a piece panics, it stops for the rest of the meeting.
//!
//! It runs with every model. Best (large-v3-turbo) keeps up on a MacBook Air M4 with the
//! live transcript running at the same time (DECISIONS.md D52).
//!
//! `margin --dev rolling <wav> [segment-secs] [out.md]` is the research prototype that
//! measured this design. It is kept below `Session`.

use crate::{corecfg, events, piece_cut, prefs};
use minutes_core::config::Config;
use minutes_core::transcript_checkpoint::{self as checkpoint, CheckpointSource, TranscriptCheckpoint};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Audio per piece. Shorter pieces leave less at Stop but load the model more often.
const PIECE_SECS: usize = 120;
/// How often the thread reads the growing files.
const READ_EVERY: Duration = Duration::from_secs(2);
/// More untranscribed audio than this on one source means the thread can't keep up.
const MAX_BACKLOG_PIECES: usize = 4;
/// Stop waits this long for a piece that is being transcribed. Redoing it after Stop
/// would take longer than waiting.
pub const STOP_WAIT: Duration = Duration::from_secs(20);

/// The Settings switch "Transcribe during the meeting". On by default, for every model.
pub fn pref() -> bool {
    !prefs::load().no_rolling_transcription
}

pub fn set_pref(on: bool) -> Result<(), String> {
    let mut p = prefs::load();
    p.no_rolling_transcription = !on;
    prefs::save(&p)
}

/// What happened during one meeting, for the log and the UI.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Progress {
    pub pieces: usize,
    pub failed: usize,
    pub covered_secs: f64,
    pub compute_secs: f64,
    /// Why the thread stopped early, if it did.
    pub stopped: Option<String>,
}

#[derive(Default)]
struct Shared {
    checkpoint: TranscriptCheckpoint,
    progress: Progress,
}

/// One meeting's rolling transcription.
pub struct Session {
    stop: Arc<AtomicBool>,
    title: Arc<Mutex<Option<String>>>,
    shared: Arc<Mutex<Shared>>,
    handle: Option<JoinHandle<()>>,
}

/// Start transcribing `wav` (and its stems, for a call) while it records. `None` when
/// rolling transcription is off or the model isn't installed.
pub fn start(call: bool, wav: &Path, title: Option<String>) -> Option<Session> {
    // The worker transcribes with this same config (processing.rs changes only the
    // summary engine), so the pieces match the final pass.
    let cfg = corecfg::load();
    if !pref() {
        return None;
    }
    let model = crate::paths::models_dir().join(format!("ggml-{}.bin", cfg.transcription.model));
    if !model.exists() {
        tracing::info!("rolling transcription off: the model is not installed yet");
        return None;
    }
    let sources = if call {
        let stems = minutes_core::capture::stem_paths_for(wav)?;
        vec![(CheckpointSource::Voice, stems.voice), (CheckpointSource::System, stems.system)]
    } else {
        vec![(CheckpointSource::Recording, wav.to_path_buf())]
    };
    let stop = Arc::new(AtomicBool::new(false));
    let title = Arc::new(Mutex::new(title));
    let shared: Arc<Mutex<Shared>> = Arc::default();
    let worker = Worker {
        cfg,
        piece: piece_samples(),
        sources: sources.into_iter().map(|(source, path)| Tail::new(source, path)).collect(),
        stop: stop.clone(),
        title: title.clone(),
        shared: shared.clone(),
    };
    let handle = std::thread::Builder::new()
        .name("margin-rolling".into())
        .spawn(move || worker.run())
        .map_err(|error| tracing::warn!(%error, "could not start rolling transcription"))
        .ok()?;
    Some(Session { stop, title, shared, handle: Some(handle) })
}

impl Session {
    /// The meeting title, which goes into the transcriber's hints.
    pub fn set_title(&self, title: Option<String>) {
        *self.title.lock().unwrap() = title;
    }

    /// Stop after the piece in progress, waiting at most `wait`, and return the pieces
    /// done. A piece still running after `wait` finishes in the background and is
    /// dropped; the final pass transcribes that range.
    pub fn finish(mut self, wait: Duration) -> Option<TranscriptCheckpoint> {
        self.stop.store(true, Ordering::SeqCst);
        let deadline = Instant::now() + wait;
        if let Some(handle) = self.handle.take() {
            while !handle.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
            if handle.is_finished() {
                let _ = handle.join();
            }
        }
        let shared = self.shared.lock().unwrap();
        tracing::info!(progress = ?shared.progress, "rolling transcription finished");
        (!shared.checkpoint.is_empty()).then(|| shared.checkpoint.clone())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Debug builds can use short pieces (scripts/e2e plays a 40 s call).
fn piece_samples() -> usize {
    let secs = cfg!(debug_assertions)
        .then(|| std::env::var("MARGIN_ROLLING_PIECE_SECS").ok()?.parse::<usize>().ok())
        .flatten()
        .filter(|s| *s >= 4)
        .unwrap_or(PIECE_SECS);
    secs * SR
}

struct Worker {
    cfg: Config,
    piece: usize,
    sources: Vec<Tail>,
    stop: Arc<AtomicBool>,
    title: Arc<Mutex<Option<String>>>,
    shared: Arc<Mutex<Shared>>,
}

impl Worker {
    fn run(mut self) {
        lower_priority();
        while !self.stop.load(Ordering::SeqCst) {
            for tail in &mut self.sources {
                tail.read_new();
            }
            if let Some(reason) = self.falling_behind() {
                self.give_up(reason);
                return;
            }
            // The earliest piece that can end now.
            let ready = self
                .sources
                .iter()
                .enumerate()
                .filter_map(|(i, t)| Some((t.next_start, i, piece_cut::cut(&t.pending, self.piece)?)))
                .min();
            let Some((_, i, cut)) = ready else {
                self.sleep(READ_EVERY);
                continue;
            };
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.transcribe_next(i, cut)));
            if outcome.is_err() {
                self.give_up("a piece crashed the transcriber".into());
                return;
            }
        }
    }

    /// Transcribes the first `cut` pending samples of source `i`.
    fn transcribe_next(&mut self, i: usize, cut: usize) {
        let tail = &mut self.sources[i];
        let samples: Vec<f32> = tail.pending.drain(..cut).collect();
        let start = tail.next_start;
        tail.next_start += cut as u64;
        let source = tail.source;
        let title = self.title.lock().unwrap().clone();
        let hints = checkpoint::decode_hints_for_queued_meeting(title.as_deref(), None, &self.cfg);
        let t = Instant::now();
        let result = checkpoint::transcribe_span(&samples, start, source, &self.cfg, &hints);
        let compute = t.elapsed().as_secs_f64();
        let secs = samples.len() as f64 / SR as f64;
        let mut shared = self.shared.lock().unwrap();
        shared.progress.compute_secs += compute;
        match result {
            Ok(span) => {
                tracing::info!(?source, start_secs = start as f64 / SR as f64, secs, compute, lines = span.lines.len(), "rolling piece done");
                shared.checkpoint.spans.push(span);
                shared.progress.pieces += 1;
                shared.progress.covered_secs += secs;
            }
            Err(error) => {
                tracing::warn!(?source, start_secs = start as f64 / SR as f64, %error, "rolling piece failed; the final pass covers it");
                shared.progress.failed += 1;
            }
        }
        events::emit("rolling", &shared.progress);
    }

    fn falling_behind(&self) -> Option<String> {
        self.sources
            .iter()
            .find(|t| t.pending.len() > MAX_BACKLOG_PIECES * self.piece)
            .map(|t| format!("fell behind on {:?}", t.source))
    }

    fn give_up(&self, reason: String) {
        tracing::warn!(%reason, "rolling transcription stopped for this meeting");
        let mut shared = self.shared.lock().unwrap();
        shared.progress.stopped = Some(reason);
        events::emit("rolling", &shared.progress);
    }

    fn sleep(&self, total: Duration) {
        let until = Instant::now() + total;
        while Instant::now() < until && !self.stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// Utility priority: the capture and the UI always come first.
fn lower_priority() {
    // SAFETY: sets the calling thread's own QoS class; no pointers are involved.
    let rc = unsafe { libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0) };
    if rc != 0 {
        tracing::debug!(rc, "could not lower the rolling thread's priority");
    }
}

/// Reads the samples a growing 16 kHz mono 16-bit WAV has gained since the last read.
/// The header's sizes aren't final while recording, so it reads the raw data after the
/// `data` chunk id, as recorder.rs does for the live labels. `pending` holds every sample
/// from the stem's first one on, as `s / 32768`, the canonical samples that core reads
/// from the final file. So sample counts are on the stem's clock and grid.
struct Tail {
    source: CheckpointSource,
    path: PathBuf,
    data_at: Option<u64>,
    /// Bytes of sample data read so far.
    read: u64,
    /// The source sample at which `pending` starts.
    next_start: u64,
    pending: Vec<f32>,
    unusable: bool,
}

impl Tail {
    fn new(source: CheckpointSource, path: PathBuf) -> Self {
        Self { source, path, data_at: None, read: 0, next_start: 0, pending: Vec::new(), unusable: false }
    }

    fn read_new(&mut self) {
        use std::io::{Read, Seek, SeekFrom};
        if self.unusable {
            return;
        }
        let Ok(mut file) = std::fs::File::open(&self.path) else { return };
        if self.data_at.is_none() {
            match wav_data_offset(&mut file) {
                Ok(Some(at)) => self.data_at = Some(at),
                Ok(None) => return, // header not written yet
                Err(reason) => {
                    tracing::info!(source = ?self.source, %reason, "rolling transcription skips this source");
                    self.unusable = true;
                    return;
                }
            }
        }
        let data_at = self.data_at.unwrap_or(0);
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        let available = len.saturating_sub(data_at + self.read) & !1;
        if available == 0 || file.seek(SeekFrom::Start(data_at + self.read)).is_err() {
            return;
        }
        let mut buf = vec![0u8; available as usize];
        let Ok(n) = file.read(&mut buf) else { return };
        let n = n & !1;
        self.pending.extend(buf[..n].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0));
        self.read += n as u64;
    }
}

/// The offset of the sample data, `Ok(None)` while the header is incomplete, or an error
/// when the file is not 16 kHz mono 16-bit PCM.
fn wav_data_offset(file: &mut std::fs::File) -> Result<Option<u64>, String> {
    use std::io::Read;
    let mut header = [0u8; 512];
    let n = file.read(&mut header).map_err(|e| e.to_string())?;
    let header = &header[..n];
    let Some(fmt) = header.windows(4).position(|w| w == b"fmt ") else { return Ok(None) };
    let Some(data) = header.windows(4).position(|w| w == b"data") else { return Ok(None) };
    let field = |at: usize, len: usize| header.get(fmt + 8 + at..fmt + 8 + at + len);
    let (Some(format), Some(channels), Some(rate), Some(bits)) = (field(0, 2), field(2, 2), field(4, 4), field(14, 2)) else {
        return Ok(None);
    };
    let format = u16::from_le_bytes([format[0], format[1]]);
    let channels = u16::from_le_bytes([channels[0], channels[1]]);
    let rate = u32::from_le_bytes([rate[0], rate[1], rate[2], rate[3]]);
    let bits = u16::from_le_bytes([bits[0], bits[1]]);
    if (format, channels, rate, bits) != (1, 1, SR as u32, 16) {
        return Err(format!("not 16 kHz mono 16-bit PCM ({format}, {channels} ch, {rate} Hz, {bits} bit)"));
    }
    Ok(Some(data as u64 + 8))
}

// ---------------------------------------------------------------- research prototype

const SR: usize = 16_000;

struct Segment {
    stem: &'static str,
    end: usize,
    secs: f64,
    lines: Vec<(f64, String)>,
}

pub fn bench(cfg: &Config, args: &[String]) -> Result<(), String> {
    let wav = PathBuf::from(args.first().ok_or("usage: rolling <wav> [segment-secs] [out.md]")?);
    let segment_secs: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(120.0);
    let out = args.get(2).map(PathBuf::from);
    let stems = minutes_core::capture::stem_paths_for(&wav).ok_or("no stem paths")?;
    let mut cfg = cfg.clone();
    // The pipeline sets min_words to 0 for stems (pipeline.rs:2472); do the same.
    cfg.transcription.min_words = 0;
    let tmp = Config::minutes_dir().join("rolling");
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;

    let mut segments = Vec::new();
    for (stem, path) in [("MICROPHONE", &stems.voice), ("SYSTEM", &stems.system)] {
        let samples = read_i16(path)?;
        for (start, end) in cut_points(&samples, (segment_secs * SR as f64) as usize) {
            let piece = tmp.join(format!("{}-{start}.wav", stem.to_lowercase()));
            write_i16(&piece, &samples[start..end])?;
            let t = Instant::now();
            let lines = match minutes_core::transcribe::transcribe_meeting(&piece, &cfg) {
                Ok(r) => r.text.lines().filter_map(|l| parse_line(l, start as f64 / SR as f64)).collect(),
                Err(e) => {
                    eprintln!("  {stem} {start}: {e}");
                    vec![]
                }
            };
            let secs = t.elapsed().as_secs_f64();
            eprintln!(
                "  {stem:<10} {:>7.1}s–{:>7.1}s  {:>5.1}s  {} lines",
                start as f64 / SR as f64,
                end as f64 / SR as f64,
                secs,
                lines.len()
            );
            let _ = std::fs::remove_file(&piece);
            segments.push(Segment { stem, end, secs, lines });
        }
    }

    // Replay: one worker (the GPU), segments in the order they become available.
    let audio_end = segments.iter().map(|s| s.end).max().unwrap_or(0) as f64 / SR as f64;
    let mut order: Vec<&Segment> = segments.iter().collect();
    order.sort_by_key(|s| (s.end, s.stem));
    let mut free_at = 0.0f64;
    for s in &order {
        free_at = free_at.max(s.end as f64 / SR as f64) + s.secs;
    }
    let compute: f64 = segments.iter().map(|s| s.secs).sum();
    eprintln!(
        "audio {audio_end:.0}s, {} segments, compute {compute:.1}s (RTF {:.3}); \
         work left after Stop: {:.1}s",
        segments.len(),
        compute / audio_end,
        (free_at - audio_end).max(0.0),
    );

    if let Some(out) = out {
        let mut lines: Vec<(f64, &str, &str)> =
            segments.iter().flat_map(|s| s.lines.iter().map(move |(t, l)| (*t, s.stem, l.as_str()))).collect();
        lines.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut md = String::from("## Transcript\n\n");
        for (t, stem, text) in lines {
            md.push_str(&format!("[{stem} {}:{:02}] {text}\n", (t / 60.0) as u64, (t % 60.0) as u64));
        }
        std::fs::write(&out, md).map_err(|e| e.to_string())?;
        eprintln!("transcript: {}", out.display());
    }
    Ok(())
}

/// Segment boundaries, as the rolling thread cuts them (piece_cut.rs).
fn cut_points(samples: &[i16], target: usize) -> Vec<(usize, usize)> {
    let samples: Vec<f32> = samples.iter().map(|&s| s as f32 / 32768.0).collect();
    piece_cut::pieces(&samples, target, READ_EVERY.as_secs() as usize * SR)
}

/// `[m:ss] text` → (seconds + offset, text).
fn parse_line(line: &str, offset: f64) -> Option<(f64, String)> {
    let rest = line.trim().strip_prefix('[')?;
    let close = rest.find(']')?;
    let (m, s) = rest[..close].split_once(':')?;
    let t = m.parse::<f64>().ok()? * 60.0 + s.parse::<f64>().ok()? + offset;
    Some((t, rest[close + 1..].trim().to_string()))
}

fn read_i16(path: &Path) -> Result<Vec<i16>, String> {
    hound::WavReader::open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .samples::<i16>()
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())
}

fn write_i16(path: &Path, samples: &[i16]) -> Result<(), String> {
    let spec = hound::WavSpec { channels: 1, sample_rate: SR as u32, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
    for &s in samples {
        w.write_sample(s).map_err(|e| e.to_string())?;
    }
    w.finalize().map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- measurement

/// `margin --dev rolling-job <wav> [speed] [--off]`: what a recording of `<wav>` (and its
/// call stems) does from Start to Complete, without a microphone.
///
/// The files grow at `speed` × real time at the path the recorder uses, while the real
/// rolling `Session` transcribes them. At the end it stops the session as Stop does, queues
/// the job with the checkpoint and processes it in this process. It prints the time after
/// Stop to transcript-ready and to Complete. With `--off` there is no rolling session and
/// the files are written at once.
pub fn measure_job(cfg: &Config, args: &[String]) -> Result<(), String> {
    use minutes_core::{jobs, pid};
    let wav = PathBuf::from(args.first().ok_or("usage: rolling-job <wav> [speed] [--off]")?);
    let speed: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let off = args.iter().any(|a| a == "--off");
    cfg.ensure_dirs().map_err(|e| e.to_string())?;
    let dest = pid::current_wav_path();
    let mut tracks = vec![(read_i16(&wav)?, dest.clone())];
    let src_stems = minutes_core::capture::stem_paths_for(&wav).filter(|s| s.voice.exists() && s.system.exists());
    let call = src_stems.is_some();
    if let (Some(src), Some(dst)) = (src_stems, minutes_core::capture::stem_paths_for(&dest)) {
        tracks.push((read_i16(&src.voice)?, dst.voice));
        tracks.push((read_i16(&src.system)?, dst.system));
    }
    let spec = hound::WavSpec { channels: 1, sample_rate: SR as u32, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut writers = Vec::new();
    for (_, path) in &tracks {
        writers.push(hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?);
    }
    let title = Some("Rolling measurement".to_string());
    let session = if off { None } else { start(call, &dest, title.clone()) };
    eprintln!("rolling {}", if session.is_some() { "on" } else { "off" });
    let total = tracks[0].0.len();
    let started = Instant::now();
    let mut written = 0;
    while written < total {
        let n = (SR / 10).min(total - written);
        for ((samples, _), w) in tracks.iter().zip(writers.iter_mut()) {
            for &s in samples.iter().skip(written).take(n) {
                w.write_sample(s).map_err(|e| e.to_string())?;
            }
            w.flush().map_err(|e| e.to_string())?;
        }
        written += n;
        if session.is_some() {
            let due = Duration::from_secs_f64(written as f64 / SR as f64 / speed);
            if let Some(wait) = due.checked_sub(started.elapsed()) {
                std::thread::sleep(wait);
            }
        }
    }
    for w in writers {
        w.finalize().map_err(|e| e.to_string())?;
    }
    let stop = Instant::now();
    eprintln!("recording of {:.0}s played in {:.0}s", total as f64 / SR as f64, started.elapsed().as_secs_f64());
    let progress = session.as_ref().map(|s| s.shared.lock().unwrap().progress.clone());
    let checkpoint = session.and_then(|s| s.finish(STOP_WAIT));
    let waited = stop.elapsed().as_secs_f64();
    eprintln!("stop waited {waited:.1}s for the piece in progress; before Stop: {progress:?}");
    let job = jobs::queue_live_capture_with_transcript_checkpoint(
        pid::CaptureMode::Meeting,
        title,
        &dest,
        None,
        None,
        Some(chrono::Local::now()),
        Some(chrono::Local::now()),
        None,
        None,
        None,
        None,
        None,
        None,
        checkpoint.as_ref(),
    )
    .map_err(|e| e.to_string())?;
    let mut worker_cfg = cfg.clone();
    worker_cfg.summarization.engine = "none".into();
    let mut transcript_ready = None;
    jobs::process_pending_jobs(&worker_cfg, |j| {
        eprintln!("  {:>7.1}s {:?}", stop.elapsed().as_secs_f32(), j.state);
        if j.state == jobs::JobState::TranscriptOnly && transcript_ready.is_none() {
            transcript_ready = Some(stop.elapsed().as_secs_f64());
        }
    })
    .map_err(|e| e.to_string())?;
    let done = jobs::load_job(&job.id).ok_or("job vanished")?;
    println!(
        "{}",
        serde_json::json!({
            "rolling": !off,
            "speed": speed,
            "audio_secs": total as f64 / SR as f64,
            "spans": checkpoint.as_ref().map_or(0, |c| c.spans.len()),
            "stop_wait_secs": waited,
            "after_stop_transcript_ready_secs": transcript_ready,
            "after_stop_complete_secs": stop.elapsed().as_secs_f64(),
            "state": done.state,
            "output": done.output_path,
            "error": done.error,
        })
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_growing(path: &Path, samples: &[i16]) {
        // A header with placeholder sizes, as while recording.
        let spec = hound::WavSpec { channels: 1, sample_rate: 16_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for &s in samples {
            w.write_sample(s).unwrap();
        }
        w.flush().unwrap();
        std::mem::forget(w); // never finalized
    }

    #[test]
    fn tail_reads_only_new_samples_of_a_growing_wav() {
        let dir = std::env::temp_dir().join(format!("margin-rolling-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("grow.wav");
        let spec = hound::WavSpec { channels: 1, sample_rate: 16_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        let mut tail = Tail::new(CheckpointSource::Recording, path.clone());
        for s in [100i16, -200, 300] {
            w.write_sample(s).unwrap();
        }
        w.flush().unwrap();
        tail.read_new();
        assert_eq!(tail.pending.len(), 3);
        w.write_sample(16_384i16).unwrap();
        w.flush().unwrap();
        tail.read_new();
        assert_eq!(tail.pending, vec![100.0 / 32768.0, -200.0 / 32768.0, 300.0 / 32768.0, 0.5]);
        w.finalize().unwrap();
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn tail_skips_audio_that_is_not_16k_mono() {
        let dir = std::env::temp_dir().join(format!("margin-rolling-fmt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("stereo.wav");
        let spec = hound::WavSpec { channels: 2, sample_rate: 48_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        for _ in 0..100 {
            w.write_sample(1i16).unwrap();
        }
        w.finalize().unwrap();
        let mut tail = Tail::new(CheckpointSource::Recording, path);
        tail.read_new();
        assert!(tail.unusable && tail.pending.is_empty());
        let ok = dir.join("ok.wav");
        write_growing(&ok, &[1, 2]);
        let mut tail = Tail::new(CheckpointSource::Voice, ok);
        tail.read_new();
        assert_eq!(tail.pending.len(), 2);
        std::fs::remove_dir_all(dir).ok();
    }
}
