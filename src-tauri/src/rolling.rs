//! Research prototype: rolling transcription (docs/research/live-transcription.md).
//!
//! Not used by the app. `margin --dev rolling <wav> [segment-secs] [out.md]` takes a
//! finished call recording (`<wav>` plus its `.voice.wav` / `.system.wav` stems) and
//! transcribes each stem in segments, the way a background thread could during the
//! recording:
//!
//! - A segment closes every `segment-secs` of audio, at the quietest 100 ms of its last
//!   15 s, so no word is cut in two.
//! - Each segment goes through core's public `transcribe::transcribe_meeting`, which runs
//!   the same VAD chunking and whisper settings as the final pass.
//! - Lines are shifted by the segment's start and merged by time into core's
//!   `[MICROPHONE m:ss]` / `[SYSTEM m:ss]` transcript format.
//!
//! It then replays the timeline: segment k can start when the recording reaches its end
//! and the previous segment is done. What is still queued at the end of the audio is the
//! work left after Stop.

use minutes_core::config::Config;
use std::path::{Path, PathBuf};
use std::time::Instant;

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

/// Segment boundaries: every `target` samples, moved back to the quietest 100 ms window
/// in the 15 s before the boundary.
fn cut_points(samples: &[i16], target: usize) -> Vec<(usize, usize)> {
    let win = SR / 10;
    let look_back = 15 * SR;
    let mut out = Vec::new();
    let mut start = 0;
    while samples.len() - start > target + look_back {
        let nominal = start + target;
        let mut best = (f64::MAX, nominal);
        let mut w = nominal - look_back.min(target / 2);
        while w + win <= nominal {
            let e: f64 = samples[w..w + win].iter().map(|&x| (x as f64).powi(2)).sum();
            if e < best.0 {
                best = (e, w + win / 2);
            }
            w += win;
        }
        out.push((start, best.1));
        start = best.1;
    }
    out.push((start, samples.len()));
    out
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
