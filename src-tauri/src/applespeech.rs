//! The Apple Speech live transcript (DECISIONS.md D56).
//!
//! minutes-core's live transcript runs Whisper on the GPU. While Margin transcribes an
//! earlier meeting, the two compete for the GPU, and the live transcript drops lines. Core
//! has Apple Speech code, but at 209265f it never uses it during a recording. So Margin
//! runs its own helper, `margin-speech` (src-tauri/speech), which uses the macOS 26
//! SpeechAnalyzer on the Neural Engine.
//!
//! One helper process runs for each audio source: the microphone and the call audio in a
//! call, the microphone in person. A feeder thread reads the growing WAV file and sends
//! its samples to the helper. A reader thread writes each final result to the live
//! transcript file in core's line format, with the source as the speaker. The recorder
//! reads that file as before. With Apple Speech on, core's live transcript is off.
//!
//! Apple Speech exists on macOS only. On Windows (D61) `chosen_locale` gives `None`, so
//! the live transcript is always core's Whisper one, and Settings does not offer a choice.

use minutes_core::pid;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The setting value that means core's Whisper live transcript.
pub const WHISPER: &str = "whisper";

/// The locales that Settings offers for Apple Speech.
pub const LOCALES: [&str; 3] = ["hi-IN", "en-IN", "en-US"];

const POLL: Duration = Duration::from_millis(250);

/// The helper next to Margin's own binary. Tauri puts it there (`bundle.externalBin`).
fn helper() -> Option<PathBuf> {
    let path = std::env::current_exe().ok()?.parent()?.join("margin-speech");
    path.is_file().then_some(path)
}

/// The Apple Speech locale chosen in Settings, or `None` for Whisper.
pub fn chosen_locale() -> Option<String> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    crate::prefs::load().live_transcript.filter(|l| LOCALES.contains(&l.as_str()))
}

fn run(command: &str, locale: &str, timeout: Duration) -> Result<(), String> {
    let helper = helper().ok_or("The Apple Speech helper is missing from Margin.")?;
    let mut child = Command::new(helper)
        .args([command, locale])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            if status.success() {
                return Ok(());
            }
            let mut message = String::new();
            if let Some(mut err) = child.stderr.take() {
                let _ = err.read_to_string(&mut message);
            }
            return Err(message.trim().to_string());
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("Apple Speech did not answer within {} s.", timeout.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Download the speech assets for `locale`, if macOS does not have them yet. Settings
/// calls it when you choose Apple Speech, so a recording never waits for a download.
pub fn install(locale: &str) -> Result<(), String> {
    run("install", locale, Duration::from_secs(600))
}

/// Apple Speech can transcribe `locale` now. macOS keeps the speech assets for each app,
/// so assets that another copy of the helper downloaded show as "not installed" until this
/// helper asks for them. That takes about 0.1 s when the files are on the Mac. A real
/// download keeps going in the background, and this recording uses Whisper.
pub fn ready(locale: &str) -> bool {
    if run("check", locale, Duration::from_secs(5)).is_ok() {
        return true;
    }
    let (done, wait) = std::sync::mpsc::channel();
    let owned = locale.to_string();
    std::thread::spawn(move || {
        let _ = done.send(install(&owned));
    });
    match wait.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(())) if run("check", locale, Duration::from_secs(5)).is_ok() => true,
        outcome => {
            tracing::warn!(%locale, ?outcome, "Apple Speech is not ready; the live transcript uses Whisper");
            false
        }
    }
}

/// The live transcript file, shared by the sources of one recording. Line numbers must
/// grow in file order, so a line is numbered and written under one lock.
struct Lines {
    next: usize,
    started: chrono::DateTime<chrono::Local>,
}

/// One running Apple Speech live transcript.
pub struct Session {
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    /// Each helper, with its source name for the log and the count of lines it wrote.
    children: Vec<(Arc<Mutex<Child>>, &'static str, Arc<AtomicUsize>)>,
}

/// Start a helper for each source. `sources` pairs a WAV that the recorder is writing with
/// the speaker label for its lines ("you", "them", or `None`).
pub fn start(locale: &str, sources: Vec<(PathBuf, Option<&'static str>)>) -> Option<Session> {
    let helper = helper()?;
    let stop = Arc::new(AtomicBool::new(false));
    let lines = Arc::new(Mutex::new(Lines { next: 0, started: chrono::Local::now() }));
    let mut session = Session { stop: stop.clone(), threads: Vec::new(), children: Vec::new() };
    for (wav, speaker) in sources {
        let source = speaker.unwrap_or("room");
        let mut child = match Command::new(&helper)
            .args(["stream", locale])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                tracing::warn!(%error, "could not start the Apple Speech helper");
                session.finish(Duration::ZERO);
                return None;
            }
        };
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        // The helper writes only errors to stderr, never transcript text. Keep them in the
        // log, so a live transcript with no lines can be explained.
        if let Some(stderr) = child.stderr.take() {
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    tracing::warn!(source, message = %line, "Apple Speech helper");
                }
            });
        }
        let written = Arc::new(AtomicUsize::new(0));
        session.children.push((Arc::new(Mutex::new(child)), source, written.clone()));
        let stop = stop.clone();
        session.threads.push(std::thread::spawn(move || feed(&wav, stdin, &stop)));
        let lines = lines.clone();
        session.threads.push(std::thread::spawn(move || write_lines(stdout, speaker, &lines, &written)));
    }
    tracing::info!(%locale, sources = session.children.len(), "Apple Speech live transcript started");
    Some(session)
}

impl Session {
    /// Send the rest of the audio, let the helpers finish their last lines, and stop them.
    /// Call it after the capture has stopped, so the WAV files are complete.
    pub fn finish(mut self, wait: Duration) {
        self.stop.store(true, Ordering::SeqCst);
        let deadline = Instant::now() + wait;
        for thread in self.threads.drain(..) {
            while !thread.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
            if thread.is_finished() {
                let _ = thread.join();
            }
        }
        for (child, source, written) in &self.children {
            let mut child = child.lock().unwrap_or_else(|e| e.into_inner());
            let ended = matches!(child.try_wait(), Ok(Some(_)));
            if !ended {
                let _ = child.kill();
            }
            let status = child.wait().map(|s| s.to_string()).unwrap_or_else(|e| e.to_string());
            tracing::info!(source, lines = written.load(Ordering::SeqCst), %status, stopped_by_margin = !ended, "Apple Speech helper finished");
        }
    }
}

/// Sends the samples of a growing 16 kHz, 16-bit mono WAV to the helper as float32, until
/// `stop` and the end of the file. Closing stdin tells the helper the input ended.
fn feed(wav: &Path, mut stdin: ChildStdin, stop: &AtomicBool) {
    let mut file = None;
    let mut at = 0u64;
    loop {
        let stopping = stop.load(Ordering::SeqCst);
        if file.is_none() {
            file = std::fs::File::open(wav).ok();
            if let Some(f) = file.as_mut() {
                match data_offset(f) {
                    Some(offset) => at = offset,
                    None => file = None,
                }
            }
        }
        if let Some(f) = file.as_mut() {
            let len = f.metadata().map(|m| m.len()).unwrap_or(at);
            let available = len.saturating_sub(at) & !1;
            if available > 0 && f.seek(SeekFrom::Start(at)).is_ok() {
                let mut bytes = vec![0u8; available as usize];
                if f.read_exact(&mut bytes).is_ok() {
                    at += available;
                    if stdin.write_all(&to_f32(&bytes)).is_err() {
                        return;
                    }
                }
            }
        }
        if stopping {
            return;
        }
        std::thread::sleep(POLL);
    }
}

/// The byte where the samples start: after the `data` chunk id and its size. The recorder
/// writes the header first, with sizes that are not final yet.
fn data_offset(file: &mut std::fs::File) -> Option<u64> {
    let mut header = [0u8; 256];
    let n = file.read(&mut header).ok()?;
    header[..n].windows(4).position(|w| w == b"data").map(|p| p as u64 + 8)
}

fn to_f32(bytes: &[u8]) -> Vec<u8> {
    bytes
        .chunks_exact(2)
        .flat_map(|b| (i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).to_le_bytes())
        .collect()
}

#[derive(serde::Deserialize)]
struct Final {
    start: f64,
    end: f64,
    text: String,
}

fn write_lines(stdout: std::process::ChildStdout, speaker: Option<&'static str>, lines: &Mutex<Lines>, written: &AtomicUsize) {
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { return };
        let Ok(result) = serde_json::from_str::<Final>(&line) else { continue };
        written.fetch_add(1, Ordering::SeqCst);
        let mut lines = lines.lock().unwrap_or_else(|e| e.into_inner());
        lines.next += 1;
        let json = live_line(lines.next, lines.started, &result, speaker);
        let written = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(pid::live_transcript_jsonl_path())
            .and_then(|mut f| writeln!(f, "{json}"));
        if let Err(error) = written {
            tracing::warn!(%error, "could not write an Apple Speech live line");
        }
    }
}

/// A line in core's live transcript format: `offset_ms` is where the utterance ends.
fn live_line(n: usize, started: chrono::DateTime<chrono::Local>, r: &Final, speaker: Option<&str>) -> serde_json::Value {
    let end_ms = (r.end.max(0.0) * 1000.0) as u64;
    let start_ms = (r.start.max(0.0) * 1000.0) as u64;
    serde_json::json!({
        "line": n,
        "ts": (started + chrono::Duration::milliseconds(end_ms as i64)).to_rfc3339(),
        "offset_ms": end_ms,
        "duration_ms": end_ms.saturating_sub(start_ms),
        "text": r.text,
        "speaker": speaker,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_become_float32() {
        let bytes: Vec<u8> = [0i16, 16384, -32768].iter().flat_map(|s| s.to_le_bytes()).collect();
        let out = to_f32(&bytes);
        let floats: Vec<f32> = out.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        assert_eq!(floats, vec![0.0, 0.5, -1.0]);
    }

    #[test]
    fn a_result_becomes_a_core_live_line() {
        let started = chrono::Local::now();
        let r = Final { start: 2.5, end: 6.0, text: "haan, ho raha hai".into() };
        let line = live_line(3, started, &r, Some("them"));
        assert_eq!(line["line"], 3);
        assert_eq!(line["offset_ms"], 6000);
        assert_eq!(line["duration_ms"], 3500);
        assert_eq!(line["speaker"], "them");
        let parsed: minutes_core::live_transcript::TranscriptLine = serde_json::from_value(line).unwrap();
        assert_eq!(parsed.text, "haan, ho raha hai");
    }

    #[test]
    fn the_data_chunk_is_found_in_a_growing_wav() {
        let dir = std::env::temp_dir().join(format!("margin-applespeech-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.wav");
        let spec = hound::WavSpec { channels: 1, sample_rate: 16_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        writer.write_sample(1i16).unwrap();
        writer.flush().unwrap();
        let mut file = std::fs::File::open(&path).unwrap();
        assert_eq!(data_offset(&mut file), Some(44));
        drop(writer);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
