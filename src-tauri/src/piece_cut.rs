//! Where a rolling piece ends (DECISIONS.md D49).
//!
//! Core transcribes each piece with its VAD chunker, and at Stop it runs the same chunker
//! on the audio after the last piece. Whisper gives the same words for the same audio, so a
//! piece matches the whole-file pass only when the chunker cuts it into the same chunks.
//!
//! The chunker reads 100 ms windows, counted from the first sample it gets, and keeps a
//! noise floor that changes with the audio. It goes back to its first state each time it
//! closes a chunk. A piece that starts at such a reset point, on the stem's 100 ms grid,
//! gets the same chunks as the whole stem. Pieces start at sample 0, and every cut is a
//! reset point, so every piece starts at one.
//!
//! Core keeps its chunker private (`transcribe::detect_meeting_vad_chunks`,
//! UPSTREAM-PATCHES.md #16). [`chunks`] follows the same rules. A test compiles core's own
//! chunker from the pinned source and checks that the two give the same chunks. If core
//! changes its chunker, that test fails.

/// Samples per second of the canonical audio.
const RATE: usize = 16_000;
/// The chunker's window: 100 ms. Cuts are on this grid.
pub const WINDOW: usize = RATE / 10;

// The chunker's rules, as core's transcribe.rs has them at the pinned commit.
/// Windows of silence that close a chunk (800 ms).
const CLOSE_AFTER_SILENT: u32 = 8;
/// Windows that stay voiced after the last loud window (500 ms).
const HANGOVER: u32 = 5;
/// A chunk shorter than this does not close on silence.
const MIN_CHUNK: usize = RATE;
/// A chunk this long closes, with or without silence.
const MAX_CHUNK: usize = 45 * RATE;
const FLOOR_START: f32 = 0.001;
const FLOOR_MIN: f32 = 0.0001;
const FLOOR_MAX: f32 = 0.02;
/// A window is loud when its RMS is above this many times the noise floor.
const LOUD: f32 = 4.0;
/// How fast the noise floor follows a silent window that is louder than it.
const FLOOR_RISE: f32 = 0.02;

/// Silence after a chunk before the chunker resets.
const PAUSE: usize = CLOSE_AFTER_SILENT as usize * WINDOW;
/// A piece prefers a cut no earlier than this before its target length.
const SEARCH_BEFORE_TARGET: usize = 15 * RATE;
/// The quietest-window fallback looks this far back.
const QUIET_LOOK_BACK: usize = 15 * RATE;

/// The chunks that core's chunker makes of `samples`, as `(start, end)` sample ranges.
pub fn chunks(samples: &[f32]) -> Vec<(usize, usize)> {
    let mut chunker = Chunker::default();
    let mut out: Vec<_> = samples.chunks(WINDOW).enumerate().filter_map(|(i, w)| chunker.step(i * WINDOW, w)).collect();
    out.extend(chunker.finish(samples.len()));
    out
}

struct Chunker {
    floor: f32,
    voiced: bool,
    hangover: u32,
    silent: u32,
    /// The start of the open chunk.
    open: Option<usize>,
    voice_end: usize,
}

impl Default for Chunker {
    fn default() -> Self {
        Self { floor: FLOOR_START, voiced: false, hangover: 0, silent: 0, open: None, voice_end: 0 }
    }
}

impl Chunker {
    /// Reads the window that starts at sample `at`, and returns a chunk if it closes one.
    fn step(&mut self, at: usize, window: &[f32]) -> Option<(usize, usize)> {
        let end = at + window.len();
        // Same arithmetic as core, so the floor has the same f32 value at each window.
        let rms = (window.iter().map(|&s| f64::from(s) * f64::from(s)).sum::<f64>() / window.len() as f64).sqrt() as f32;
        if rms > self.floor * LOUD {
            self.voiced = true;
            self.hangover = HANGOVER;
            self.silent = 0;
        } else if self.hangover > 0 {
            self.hangover -= 1;
            self.silent = 0;
        } else {
            self.voiced = false;
            self.silent += 1;
            let rate = if rms > self.floor { FLOOR_RISE } else { FLOOR_RISE * 3.0 };
            self.floor += (rms - self.floor) * rate;
            self.floor = self.floor.clamp(FLOOR_MIN, FLOOR_MAX);
        }
        if self.voiced {
            self.open.get_or_insert(at);
            self.voice_end = end;
        }
        let start = self.open?;
        let len = self.voice_end.saturating_sub(start);
        let closed = if len >= MAX_CHUNK {
            Some((start, self.voice_end.max(end)))
        } else if !self.voiced && self.silent >= CLOSE_AFTER_SILENT && len >= MIN_CHUNK {
            Some((start, self.voice_end))
        } else {
            None
        };
        if closed.is_some() {
            *self = Self::default();
        }
        closed
    }

    /// The chunk still open at the end of the audio.
    fn finish(self, total: usize) -> Option<(usize, usize)> {
        let start = self.open?;
        let end = if self.voice_end > start { self.voice_end } else { total };
        (end > start).then_some((start, end))
    }
}

/// A sample at which the chunker is back in its first state.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Reset {
    at: usize,
    /// The silence before the next chunk, for a chunk that closed on silence. `None` for a
    /// chunk that closed at 45 s.
    pause: Option<usize>,
}

/// The reset points that `chunks` of `len` samples show. A chunk of 1 s to 45 s, with at
/// least 800 ms of silence after it, closed when that silence was done. A 45 s chunk closed
/// at its end.
fn resets(chunks: &[(usize, usize)], len: usize) -> Vec<Reset> {
    let next_starts = chunks.iter().skip(1).map(|c| c.0).chain([len]);
    chunks
        .iter()
        .zip(next_starts)
        .filter_map(|(&(start, end), next)| {
            if end - start >= MAX_CHUNK {
                Some(Reset { at: end, pause: None })
            } else if end - start >= MIN_CHUNK && next >= end + PAUSE {
                Some(Reset { at: end + PAUSE, pause: Some(next - end) })
            } else {
                None
            }
        })
        .collect()
}

/// Where to end a piece of `pending`, or `None` to wait for more audio. `pending` must start
/// at a reset point of the stem: sample 0, or an earlier cut from this function.
///
/// 1. The longest pause that ends at most 15 s before `target`, or later.
/// 2. Else the first 45 s split in that range.
/// 3. Else wait, until there is 1.5 × `target`. Then take the latest reset point.
/// 4. With no reset point at all, the quietest 100 ms in the last 15 s. This cut is not a
///    reset point, so the chunks near it can differ from the whole-file pass.
///
/// With no speech at all there is no reset point. The cut is at `target`. The next piece
/// starts with the first noise floor, and the whole stem has the floor that the silence
/// gave it. In silence the two floors come close within seconds, and in digital silence
/// they become equal. So this cut is close, not exact.
pub fn cut(pending: &[f32], target: usize) -> Option<usize> {
    if pending.len() < target {
        return None;
    }
    let longest = target + target / 2;
    // Whole windows only, as the stem's pass reads them.
    let view = &pending[..pending.len().min(longest) / WINDOW * WINDOW];
    let chunks = chunks(view);
    if chunks.is_empty() {
        return Some(target / WINDOW * WINDOW);
    }
    let resets = resets(&chunks, view.len());
    let earliest = target.saturating_sub(SEARCH_BEFORE_TARGET);
    let late = || resets.iter().filter(|r| r.at >= earliest);
    let pause = late().filter_map(|r| Some((r.pause?, std::cmp::Reverse(r.at)))).max().map(|(_, at)| at.0);
    let split = late().find(|r| r.pause.is_none()).map(|r| r.at);
    if let Some(at) = pause.or(split) {
        return Some(at);
    }
    if view.len() < longest / WINDOW * WINDOW {
        return None;
    }
    Some(resets.last().map_or_else(|| quietest_window(view), |r| r.at))
}

/// The start of the quietest 100 ms grid window in the last 15 s of `samples`.
fn quietest_window(samples: &[f32]) -> usize {
    let energy = |at: usize| samples[at..at + WINDOW].iter().map(|&s| f64::from(s).powi(2)).sum::<f64>();
    let begin = samples.len().saturating_sub(QUIET_LOOK_BACK).max(WINDOW) / WINDOW * WINDOW;
    (begin..=samples.len() - WINDOW).step_by(WINDOW).min_by(|&a, &b| energy(a).total_cmp(&energy(b))).unwrap_or(begin)
}

/// The pieces that the rolling thread cuts `samples` into when all of it is there. It reads
/// `step` more samples each time it waits, as the thread reads the growing file. The last
/// piece is the audio after the last cut, which the final pass transcribes.
pub fn pieces(samples: &[f32], target: usize, step: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut len = target;
    while start + len <= samples.len() {
        match cut(&samples[start..start + len], target) {
            Some(at) => {
                out.push((start, start + at));
                start += at;
                len = target;
            }
            None => len += step,
        }
    }
    if start < samples.len() {
        out.push((start, samples.len()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGET: usize = 120 * RATE;

    /// A tone, with silence over each `(from, to)` range in seconds.
    fn speech(secs: usize, pauses: &[(f32, f32)]) -> Vec<f32> {
        let mut samples: Vec<f32> = (0..secs * RATE).map(|n| 0.2 * (n as f32 * 0.031).sin()).collect();
        for &(from, to) in pauses {
            samples[(from * RATE as f32) as usize..(to * RATE as f32) as usize].fill(0.0);
        }
        samples
    }

    fn secs(s: f32) -> usize {
        (s * RATE as f32) as usize
    }

    #[test]
    fn a_piece_ends_after_the_longest_pause_near_its_target() {
        // The hangover keeps 500 ms of a pause voiced, and the reset comes 800 ms later.
        let samples = speech(125, &[(60.0, 64.0), (107.0, 108.5), (111.0, 113.5)]);
        assert_eq!(cut(&samples, TARGET), Some(secs(111.5) + PAUSE));
        // One pause in the range is enough.
        let samples = speech(125, &[(107.0, 108.5)]);
        assert_eq!(cut(&samples, TARGET), Some(secs(107.5) + PAUSE));
    }

    #[test]
    fn a_short_gap_is_not_a_pause() {
        // 1 s of silence is 500 ms after the hangover: the chunker does not close.
        let samples = speech(120, &[(110.0, 111.0)]);
        assert_eq!(cut(&samples, TARGET), None);
    }

    #[test]
    fn speech_with_no_pause_is_cut_at_a_45_s_split() {
        // Splits at 45 s and 90 s are too early. Wait.
        assert_eq!(cut(&speech(120, &[]), TARGET), None);
        // The split at 135 s is in range.
        assert_eq!(cut(&speech(150, &[]), TARGET), Some(135 * RATE));
        // A pause in range comes before a split in range.
        let samples = speech(150, &[(130.0, 132.0)]);
        assert_eq!(cut(&samples, TARGET), Some(secs(130.5) + PAUSE));
    }

    #[test]
    fn at_one_and_a_half_targets_the_latest_reset_point_is_used() {
        // A pause at 60 s, then silence with one short sound. The short sound is an open
        // chunk, so the pause is the only reset point.
        let mut samples = speech(185, &[(60.0, 185.0)]);
        samples[160 * RATE..160 * RATE + RATE * 3 / 10].copy_from_slice(&speech(1, &[])[..RATE * 3 / 10]);
        assert_eq!(cut(&samples[..179 * RATE], TARGET), None);
        assert_eq!(cut(&samples, TARGET), Some(secs(60.5) + PAUSE));
    }

    #[test]
    fn with_no_reset_point_the_quietest_window_is_used() {
        // A 0.3 s sound is shorter than 1 s, so its chunk never closes.
        let mut samples = vec![0.003f32; 185 * RATE];
        samples[RATE..RATE + RATE * 3 / 10].copy_from_slice(&speech(1, &[])[..RATE * 3 / 10]);
        samples[170 * RATE..170 * RATE + WINDOW].fill(0.0);
        assert_eq!(cut(&samples, TARGET), Some(170 * RATE));
    }

    #[test]
    fn silence_is_cut_at_the_target() {
        assert_eq!(cut(&vec![0.0; TARGET + RATE], TARGET), Some(TARGET));
        assert_eq!(cut(&vec![0.0; TARGET - 1], TARGET), None);
    }

    /// Speech of changing loudness over room noise, with pauses of many lengths at odd
    /// offsets. The noise moves the floor, so a piece that starts in the wrong state gets
    /// other chunks.
    pub(super) fn meeting(secs: usize) -> Vec<f32> {
        let mut seed = 0x2545_f491_u32;
        let mut noise = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as f32 / u32::MAX as f32 - 0.5
        };
        let mut samples = Vec::with_capacity(secs * RATE);
        let mut t = 0.0f32;
        let mut k = 0;
        while samples.len() < secs * RATE {
            let talk = [6.3, 11.7, 2.9, 31.1, 0.7, 52.0, 8.8, 3.3][k % 8];
            let gap = [0.31, 1.27, 0.58, 2.13, 0.94, 1.61, 4.4][k % 7];
            let loud = [0.2, 0.006, 0.05, 0.12, 0.004, 0.02, 0.3][k % 7 * 3 % 7];
            let room = [0.002, 0.004, 0.0008][k % 3];
            for n in 0..(talk * RATE as f32) as usize {
                samples.push(loud * (n as f32 * 0.031).sin() * (1.0 + 0.5 * (t * 3.1).sin()) + room * noise());
                t += 1.0 / RATE as f32;
            }
            for _ in 0..(gap * RATE as f32) as usize {
                samples.push(room * noise());
                t += 1.0 / RATE as f32;
            }
            k += 1;
        }
        samples.truncate(secs * RATE);
        // Quantize as a 16-bit stem does.
        samples.iter().map(|&s| (s * 32768.0).round().clamp(-32768.0, 32767.0) / 32768.0).collect()
    }

    /// Each piece's chunks, moved onto the stem's clock.
    pub(super) fn piece_chunks(pieces: &[(usize, usize)], per_piece: Vec<Vec<(usize, usize)>>) -> Vec<(usize, usize)> {
        per_piece.into_iter().zip(pieces).flat_map(|(c, &(a, _))| c.into_iter().map(move |(s, e)| (a + s, a + e))).collect()
    }

    pub(super) fn first_difference(whole: &[(usize, usize)], pieces: &[(usize, usize)]) -> Option<String> {
        let i = (0..whole.len().max(pieces.len())).find(|&i| whole.get(i) != pieces.get(i))?;
        let s = |c: Option<&(usize, usize)>| c.map(|&(a, b)| (a as f64 / RATE as f64, b as f64 / RATE as f64));
        Some(format!("chunk {i} of {}: whole {:?}, pieces {:?}", whole.len(), s(whole.get(i)), s(pieces.get(i))))
    }

    #[test]
    fn pieces_give_the_same_chunks_as_the_whole_stem() {
        let samples = meeting(900);
        let pieces = pieces(&samples, TARGET, 2 * RATE);
        assert!(pieces.len() >= 6, "{} pieces", pieces.len());
        assert!(pieces.iter().all(|&(a, _)| a % WINDOW == 0));
        let per_piece = piece_chunks(&pieces, pieces.iter().map(|&(a, b)| chunks(&samples[a..b])).collect());
        if let Some(diff) = first_difference(&chunks(&samples), &per_piece) {
            panic!("{diff}");
        }
    }

    #[test]
    fn quietest_window_cuts_break_the_chunks() {
        // What the old cut did. This shows that the test above can fail.
        let samples = meeting(900);
        let mut cuts = vec![0];
        while cuts.last().unwrap() + TARGET < samples.len() {
            let start = *cuts.last().unwrap();
            cuts.push(start + quietest_window(&samples[start..start + TARGET]) + WINDOW / 2);
        }
        cuts.push(samples.len());
        let pieces: Vec<_> = cuts.windows(2).map(|w| (w[0], w[1])).collect();
        let per_piece = piece_chunks(&pieces, pieces.iter().map(|&(a, b)| chunks(&samples[a..b])).collect());
        assert!(first_difference(&chunks(&samples), &per_piece).is_some());
    }
}

/// Core's own chunker, compiled from the pinned minutes-core source, for the tests. It is
/// never copied into this repository: the test reads it from Cargo's checkout.
#[cfg(test)]
mod core_chunker {
    use super::tests::{first_difference, meeting, piece_chunks};
    use super::*;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::OnceLock;

    /// Reads a 16-bit mono WAV, then one `start end` range per stdin line. Prints the
    /// chunks of each range, relative to its start.
    const MAIN: &str = r#"
fn main() {
    use std::io::BufRead;
    let path = std::env::args().nth(1).expect("wav path");
    let bytes = std::fs::read(path).expect("read wav");
    let mut at = 12;
    let data = loop {
        let id = &bytes[at..at + 4];
        let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        if id == b"data" {
            break &bytes[at + 8..(at + 8 + len).min(bytes.len())];
        }
        at += 8 + len + len % 2;
    };
    let samples: Vec<f32> = data.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap();
        let mut it = line.split_whitespace().map(|n| n.parse::<usize>().unwrap());
        let (a, b) = (it.next().unwrap(), it.next().unwrap());
        let out: Vec<String> = detect_meeting_vad_chunks(&samples[a..b]).iter().map(|(s, e)| format!("{s} {e}")).collect();
        println!("{}", out.join(" "));
    }
}
"#;

    /// The toolchain's rustc, next to the cargo that builds this crate.
    fn rustc() -> PathBuf {
        let rustc = Path::new(env!("CARGO")).with_file_name("rustc");
        if rustc.exists() { rustc } else { PathBuf::from("rustc") }
    }

    /// The pinned minutes-core's `src/transcribe.rs`, from `cargo metadata`.
    fn core_transcribe_rs() -> PathBuf {
        // Only the host's dependencies: the others are not downloaded.
        let version = Command::new(rustc()).arg("-vV").output().expect("run rustc -vV");
        let version = String::from_utf8_lossy(&version.stdout);
        let host = version.lines().find_map(|l| l.strip_prefix("host: ")).expect("rustc host");
        let out = Command::new(env!("CARGO"))
            .args(["metadata", "--format-version", "1", "--offline", "--filter-platform", host, "--manifest-path"])
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
            .output()
            .expect("run cargo metadata");
        assert!(out.status.success(), "cargo metadata: {}", String::from_utf8_lossy(&out.stderr));
        let meta: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let manifest = meta["packages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "minutes-core")
            .and_then(|p| p["manifest_path"].as_str())
            .expect("minutes-core in cargo metadata");
        Path::new(manifest).with_file_name("src").join("transcribe.rs")
    }

    /// Builds the helper once per test run, in this crate's build folder.
    fn helper() -> &'static Path {
        static HELPER: OnceLock<PathBuf> = OnceLock::new();
        HELPER.get_or_init(|| {
            let source = std::fs::read_to_string(core_transcribe_rs()).expect("read core's transcribe.rs");
            let at = source.find("fn detect_meeting_vad_chunks(").expect(
                "core has no detect_meeting_vad_chunks. Its chunker changed: check piece_cut.rs against it.",
            );
            let begin = source[..at].rfind('\n').map_or(0, |n| n + 1);
            let end = at + source[at..].find("\n}\n").expect("end of detect_meeting_vad_chunks") + 3;
            let function = source[begin..end].replacen("pub(crate) fn", "fn", 1);
            let dir = Path::new(env!("OUT_DIR")).join("core-chunker");
            std::fs::create_dir_all(&dir).unwrap();
            let main = dir.join("main.rs");
            std::fs::write(&main, format!("{function}\n{MAIN}")).unwrap();
            let bin = dir.join("core-chunker");
            let out = Command::new(rustc()).args(["--edition", "2021", "-O", "-o"]).arg(&bin).arg(&main).output().expect("run rustc");
            assert!(out.status.success(), "rustc: {}", String::from_utf8_lossy(&out.stderr));
            bin
        })
    }

    /// Core's chunks of each `(start, end)` range of the WAV at `wav`.
    fn core_chunks(wav: &Path, ranges: &[(usize, usize)]) -> Vec<Vec<(usize, usize)>> {
        let mut child = Command::new(helper()).arg(wav).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let input: String = ranges.iter().map(|(a, b)| format!("{a} {b}\n")).collect();
        child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success());
        String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(|line| {
                let n: Vec<usize> = line.split_whitespace().map(|n| n.parse().unwrap()).collect();
                n.chunks(2).map(|p| (p[0], p[1])).collect()
            })
            .collect()
    }

    fn write_wav(path: &Path, samples: &[f32]) {
        let spec = hound::WavSpec { channels: 1, sample_rate: RATE as u32, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for &s in samples {
            w.write_sample((s * 32768.0) as i16).unwrap();
        }
        w.finalize().unwrap();
    }

    fn read_wav(path: &Path) -> Vec<f32> {
        hound::WavReader::open(path).unwrap().samples::<i16>().map(|s| s.unwrap() as f32 / 32768.0).collect()
    }

    /// The signals that the comparison uses, each quantized to 16 bits.
    fn signals() -> Vec<(&'static str, Vec<f32>)> {
        let q = |v: Vec<f32>| v.iter().map(|&s| (s * 32768.0).round().clamp(-32768.0, 32767.0) / 32768.0).collect::<Vec<_>>();
        let mut short_sounds = vec![0.0f32; 60 * RATE];
        for k in 0..20 {
            let at = k * 3 * RATE + k * 37;
            for n in 0..(k + 1) * WINDOW / 2 {
                short_sounds[at + n] = 0.1 * (n as f32 * 0.05).sin();
            }
        }
        // Loudness near the threshold, so the floor decides each window.
        let edge: Vec<f32> = (0..120 * RATE).map(|n| 0.004 * (1.0 + (n as f32 / 9000.0).sin()) * (n as f32 * 0.07).sin()).collect();
        vec![
            ("meeting", meeting(600)),
            ("silence", vec![0.0; 30 * RATE + 77]),
            ("short sounds", q(short_sounds)),
            ("no pause", q((0..200 * RATE).map(|n| 0.2 * (n as f32 * 0.031).sin()).collect())),
            ("edge", q(edge)),
        ]
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = Path::new(env!("OUT_DIR")).join(format!("{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn margin_chunker_is_cores_chunker() {
        let dir = temp_dir("chunker");
        for (name, samples) in signals() {
            let wav = dir.join("signal.wav");
            write_wav(&wav, &samples);
            let samples = read_wav(&wav);
            // The whole signal, and ranges that do not start on the grid or at a reset.
            let ranges = [(0, samples.len()), (777, samples.len()), (0, samples.len() / 3), (12_345, samples.len() / 2)];
            for (core, &(a, b)) in core_chunks(&wav, &ranges).iter().zip(&ranges) {
                assert_eq!(&chunks(&samples[a..b]), core, "{name}, samples {a}..{b}");
            }
        }
        std::fs::remove_dir_all(dir).ok();
    }

    /// Cuts `wav` with [`pieces`], and checks with core's chunker that the pieces, and the
    /// audio after the last cut, give the same chunks as the whole file.
    fn assert_pieces_chunk_like_the_whole_stem(wav: &Path, target: usize) -> (usize, usize) {
        let samples = read_wav(wav);
        let pieces = pieces(&samples, target, 2 * RATE);
        assert!(pieces.len() >= 2, "too few pieces to compare");
        let whole = core_chunks(wav, &[(0, samples.len())]).remove(0);
        let per_piece = piece_chunks(&pieces, core_chunks(wav, &pieces));
        if let Some(diff) = first_difference(&whole, &per_piece) {
            panic!("{diff}");
        }
        // Margin's chunker on the whole stem as well.
        assert!(chunks(&samples) == whole, "Margin's chunker differs from core's on this stem");
        (pieces.len(), whole.len())
    }

    #[test]
    fn pieces_give_cores_chunks_of_the_whole_stem() {
        let dir = temp_dir("pieces");
        let wav = dir.join("meeting.wav");
        write_wav(&wav, &meeting(900));
        assert_pieces_chunk_like_the_whole_stem(&wav, 120 * RATE);
        std::fs::remove_dir_all(dir).ok();
    }

    /// A real stem: `MARGIN_REAL_STEM=/path/x.voice.wav cargo test real_stem -- --ignored
    /// --nocapture`. It runs no model, so it takes seconds.
    #[test]
    #[ignore]
    fn real_stem_pieces_give_cores_chunks_of_the_whole_stem() {
        let path = PathBuf::from(std::env::var("MARGIN_REAL_STEM").expect("MARGIN_REAL_STEM"));
        let (pieces, chunks) = assert_pieces_chunk_like_the_whole_stem(&path, 120 * RATE);
        println!("{}: {pieces} pieces, {chunks} chunks, all the same as the whole stem", path.display());
    }
}
