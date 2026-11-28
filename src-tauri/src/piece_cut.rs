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
//! Core then pads each chunk by `transcription.meeting_chunk_pad_ms` on both sides, clamped
//! to the audio it was given, and merges chunks that overlap (UPSTREAM-PATCHES.md #34). A
//! piece's chunks are padded inside the piece, so padding can never reach into the next
//! piece, but a chunk next to a cut would be padded across the cut in the whole stem. So a
//! reset point is a cut only when padding can't reach it from either side: the chunk before
//! ends at least `pad` before it, and the chunk after starts at least `pad` after it. The
//! padded chunks then don't touch the cut, nothing merges across it, and padding the
//! piece's chunks gives the stem's padded chunks. A chunk that closed at 45 s ends at its
//! reset point, so with padding that point is never a cut.
//!
//! Core keeps its chunker private (`transcribe::detect_meeting_vad_chunks`,
//! UPSTREAM-PATCHES.md #16, #34). `chunks` (tests only) follows the same rules, with the noise
//! multiplier and padding from the effective config ([`Rules::of`]). A test compiles core's
//! own chunker from the pinned source and checks that the two give the same chunks. If core
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
/// How fast the noise floor follows a silent window that is louder than it.
const FLOOR_RISE: f32 = 0.02;

/// Silence after a chunk before the chunker resets.
const PAUSE: usize = CLOSE_AFTER_SILENT as usize * WINDOW;
/// A piece prefers a cut no earlier than this before its target length.
const SEARCH_BEFORE_TARGET: usize = 15 * RATE;
/// The quietest-window fallback looks this far back.
const QUIET_LOOK_BACK: usize = 15 * RATE;

/// The two chunker settings that come from the config, as core's `MeetingChunking::from_config`
/// reads them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rules {
    /// A window is loud when its RMS is above this many times the noise floor.
    loud: f32,
    /// Samples of padding on each side of a chunk.
    pad: usize,
}

impl Rules {
    pub fn of(t: &minutes_core::config::TranscriptionConfig) -> Self {
        Self::new(t.meeting_chunk_noise_multiplier, t.meeting_chunk_pad_ms)
    }

    fn new(noise_multiplier: f32, pad_ms: u32) -> Self {
        let loud = if noise_multiplier.is_finite() && noise_multiplier > 0.0 { noise_multiplier } else { 4.0 };
        Self { loud, pad: pad_ms as usize * (RATE / 1000) }
    }
}

/// The chunks that core's chunker makes of `samples`, as `(start, end)` sample ranges. Cuts
/// need only the unpadded chunks; the tests compare these with core's.
#[cfg(test)]
fn chunks(samples: &[f32], rules: Rules) -> Vec<(usize, usize)> {
    pad_chunks(energy_chunks(samples, rules.loud), rules.pad, samples.len())
}

/// The chunks before padding.
fn energy_chunks(samples: &[f32], loud: f32) -> Vec<(usize, usize)> {
    let mut chunker = Chunker::new(loud);
    let mut out: Vec<_> = samples.chunks(WINDOW).enumerate().filter_map(|(i, w)| chunker.step(i * WINDOW, w)).collect();
    out.extend(chunker.finish(samples.len()));
    out
}

/// Core's `pad_meeting_chunks`: each chunk widened by `pad` within `0..len`. Chunks that
/// then overlap merge, unless the merge would pass 45 s: then they meet halfway between the
/// unpadded chunks.
#[cfg(test)]
fn pad_chunks(chunks: Vec<(usize, usize)>, pad: usize, len: usize) -> Vec<(usize, usize)> {
    if pad == 0 {
        return chunks;
    }
    let mut padded: Vec<(usize, usize)> = Vec::with_capacity(chunks.len());
    let mut previous_end = 0;
    for (start, end) in chunks {
        let (padded_start, padded_end) = (start.saturating_sub(pad), end.saturating_add(pad).min(len));
        match padded.last_mut() {
            Some(last) if padded_start < last.1 => {
                if padded_end - last.0 <= MAX_CHUNK {
                    last.1 = padded_end;
                } else {
                    let meet = previous_end + (start - previous_end) / 2;
                    last.1 = meet;
                    padded.push((meet, padded_end));
                }
            }
            _ => padded.push((padded_start, padded_end)),
        }
        previous_end = end;
    }
    padded
}

struct Chunker {
    loud: f32,
    floor: f32,
    voiced: bool,
    hangover: u32,
    silent: u32,
    /// The start of the open chunk.
    open: Option<usize>,
    voice_end: usize,
}

impl Chunker {
    fn new(loud: f32) -> Self {
        Self { loud, floor: FLOOR_START, voiced: false, hangover: 0, silent: 0, open: None, voice_end: 0 }
    }

    /// Reads the window that starts at sample `at`, and returns a chunk if it closes one.
    fn step(&mut self, at: usize, window: &[f32]) -> Option<(usize, usize)> {
        let end = at + window.len();
        // Same arithmetic as core, so the floor has the same f32 value at each window.
        let rms = (window.iter().map(|&s| f64::from(s) * f64::from(s)).sum::<f64>() / window.len() as f64).sqrt() as f32;
        if rms > self.floor * self.loud {
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
            *self = Self::new(self.loud);
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

/// The reset points that unpadded `chunks` of `len` samples show, where `pad` padding
/// reaches neither of them (module doc). A chunk of 1 s to 45 s, with at least 800 ms of
/// silence after it, closed when that silence was done. A 45 s chunk closed at its end.
fn resets(chunks: &[(usize, usize)], len: usize, pad: usize) -> Vec<Reset> {
    let next_starts = chunks.iter().skip(1).map(|c| c.0).chain([len]);
    chunks
        .iter()
        .zip(next_starts)
        .filter_map(|(&(start, end), next)| {
            if end - start >= MAX_CHUNK {
                (pad == 0).then_some(Reset { at: end, pause: None })
            } else if end - start >= MIN_CHUNK && pad <= PAUSE && next >= end + PAUSE + pad {
                Some(Reset { at: end + PAUSE, pause: Some(next - end) })
            } else {
                None
            }
        })
        .collect()
}

/// Where to end a piece of `pending`, or `None` to wait for more audio. `pending` must start
/// at a reset point of the stem: sample 0, or an earlier cut from this function, made with
/// the same `rules`.
///
/// 1. The longest pause that ends at most 15 s before `target`, or later.
/// 2. Else the first 45 s split in that range (only without padding).
/// 3. Else wait, until there is 1.5 × `target`. Then take the latest reset point.
/// 4. With no reset point at all, the quietest 100 ms in the last 15 s. This cut is not a
///    reset point, so the chunks near it can differ from the whole-file pass. Core still
///    reuses the piece: its text is right for its own audio, but a chunk near the cut can
///    be split or padded differently than the whole-file pass would.
///
/// With padding, a pause is a reset point only if it leaves `pad` clear on both sides: with
/// 300 ms, at least 1.1 s from a chunk's voiced end to the next chunk, against 800 ms
/// without. Padding over 800 ms leaves no reset point, so every cut is step 4.
///
/// With no speech at all there is no reset point. The cut is at `target`. The next piece
/// starts with the first noise floor, and the whole stem has the floor that the silence
/// gave it. In silence the two floors come close within seconds, and in digital silence
/// they become equal. So this cut is close, not exact.
pub fn cut(pending: &[f32], target: usize, rules: Rules) -> Option<usize> {
    if pending.len() < target {
        return None;
    }
    let longest = target + target / 2;
    // Whole windows only, as the stem's pass reads them.
    let view = &pending[..pending.len().min(longest) / WINDOW * WINDOW];
    let chunks = energy_chunks(view, rules.loud);
    if chunks.is_empty() {
        return Some(target / WINDOW * WINDOW);
    }
    let resets = resets(&chunks, view.len(), rules.pad);
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
pub fn pieces(samples: &[f32], target: usize, step: usize, rules: Rules) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut len = target;
    while start + len <= samples.len() {
        match cut(&samples[start..start + len], target, rules) {
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

    /// Core's defaults, and Margin's measured defaults (corecfg.rs, D82), as
    /// `(noise multiplier, pad ms)`.
    pub(super) const SETTINGS: [(f32, u32); 2] = [(4.0, 0), (2.5, 300)];

    fn upstream() -> Rules {
        Rules::new(4.0, 0)
    }

    fn padded() -> Rules {
        Rules::new(2.5, 300)
    }

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
    fn rules_read_the_config_as_core_does() {
        let mut t = minutes_core::config::TranscriptionConfig::default();
        assert_eq!(Rules::of(&t), upstream());
        t.meeting_chunk_noise_multiplier = 2.5;
        t.meeting_chunk_pad_ms = 300;
        assert_eq!(Rules::of(&t), Rules { loud: 2.5, pad: 4_800 });
        for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert_eq!(Rules::new(bad, 0), upstream(), "{bad}");
        }
    }

    #[test]
    fn a_piece_ends_after_the_longest_pause_near_its_target() {
        // The hangover keeps 500 ms of a pause voiced, and the reset comes 800 ms later.
        let samples = speech(125, &[(60.0, 64.0), (107.0, 108.5), (111.0, 113.5)]);
        assert_eq!(cut(&samples, TARGET, upstream()), Some(secs(111.5) + PAUSE));
        assert_eq!(cut(&samples, TARGET, padded()), Some(secs(111.5) + PAUSE));
        // One pause in the range is enough.
        let samples = speech(125, &[(107.0, 108.5)]);
        assert_eq!(cut(&samples, TARGET, upstream()), Some(secs(107.5) + PAUSE));
    }

    #[test]
    fn a_pause_that_padding_reaches_is_not_a_cut() {
        // 1 s from the end of the voiced chunk to the next: a reset point, but 300 ms of
        // padding on the next chunk would start it before the cut.
        let samples = speech(125, &[(107.0, 108.5)]);
        assert_eq!(cut(&samples, TARGET, padded()), None);
        // 1.1 s leaves 300 ms clear on each side.
        let samples = speech(125, &[(107.0, 108.6)]);
        assert_eq!(cut(&samples, TARGET, padded()), Some(secs(107.5) + PAUSE));
        // Padding over 800 ms reaches every reset point.
        let samples = speech(125, &[(107.0, 113.0)]);
        assert_eq!(cut(&samples, TARGET, Rules::new(4.0, 900)), None);
    }

    #[test]
    fn a_short_gap_is_not_a_pause() {
        // 1 s of silence is 500 ms after the hangover: the chunker does not close.
        let samples = speech(120, &[(110.0, 111.0)]);
        assert_eq!(cut(&samples, TARGET, upstream()), None);
    }

    #[test]
    fn speech_with_no_pause_is_cut_at_a_45_s_split() {
        // Splits at 45 s and 90 s are too early. Wait.
        assert_eq!(cut(&speech(120, &[]), TARGET, upstream()), None);
        // The split at 135 s is in range.
        assert_eq!(cut(&speech(150, &[]), TARGET, upstream()), Some(135 * RATE));
        // A pause in range comes before a split in range.
        let samples = speech(150, &[(130.0, 132.0)]);
        assert_eq!(cut(&samples, TARGET, upstream()), Some(secs(130.5) + PAUSE));
    }

    #[test]
    fn with_padding_a_45_s_split_is_not_a_cut() {
        // The chunk before a split ends at the split, so its padding crosses it.
        assert_eq!(cut(&speech(150, &[]), TARGET, padded()), None);
        let samples = speech(150, &[(130.0, 132.0)]);
        assert_eq!(cut(&samples, TARGET, padded()), Some(secs(130.5) + PAUSE));
    }

    #[test]
    fn at_one_and_a_half_targets_the_latest_reset_point_is_used() {
        // A pause at 60 s, then silence with one short sound. The short sound is an open
        // chunk, so the pause is the only reset point.
        let mut samples = speech(185, &[(60.0, 185.0)]);
        samples[160 * RATE..160 * RATE + RATE * 3 / 10].copy_from_slice(&speech(1, &[])[..RATE * 3 / 10]);
        for rules in [upstream(), padded()] {
            assert_eq!(cut(&samples[..179 * RATE], TARGET, rules), None);
            assert_eq!(cut(&samples, TARGET, rules), Some(secs(60.5) + PAUSE));
        }
    }

    #[test]
    fn with_no_reset_point_the_quietest_window_is_used() {
        // A 0.3 s sound is shorter than 1 s, so its chunk never closes.
        let mut samples = vec![0.003f32; 185 * RATE];
        samples[RATE..RATE + RATE * 3 / 10].copy_from_slice(&speech(1, &[])[..RATE * 3 / 10]);
        samples[170 * RATE..170 * RATE + WINDOW].fill(0.0);
        assert_eq!(cut(&samples, TARGET, upstream()), Some(170 * RATE));
    }

    #[test]
    fn silence_is_cut_at_the_target() {
        assert_eq!(cut(&vec![0.0; TARGET + RATE], TARGET, upstream()), Some(TARGET));
        assert_eq!(cut(&vec![0.0; TARGET - 1], TARGET, upstream()), None);
    }

    fn xorshift() -> impl FnMut() -> f32 {
        let mut seed = 0x2545_f491_u32;
        move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as f32 / u32::MAX as f32 - 0.5
        }
    }

    fn quantize(samples: &[f32]) -> Vec<f32> {
        samples.iter().map(|&s| (s * 32768.0).round().clamp(-32768.0, 32767.0) / 32768.0).collect()
    }

    /// Speech of changing loudness over room noise, with pauses of many lengths at odd
    /// offsets. The noise moves the floor, so a piece that starts in the wrong state gets
    /// other chunks.
    pub(super) fn meeting(secs: usize) -> Vec<f32> {
        let mut noise = xorshift();
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
        quantize(&samples)
    }

    /// Loud talk that fades in and out through quiet speech, between 2.5 and 4 times the
    /// room's noise floor and below it, around pauses of 0.9 s to 3.1 s. The pauses sit
    /// near the boundary between a reset point that padding reaches and one it doesn't, so
    /// cuts land next to quiet speech that only the lower multiplier, or the padding, keeps.
    pub(super) fn quiet_edges(secs: usize) -> Vec<f32> {
        let mut noise = xorshift();
        let room = 0.001;
        // The noise's RMS: the floor the chunker settles on in a pause.
        let floor = room / 12f32.sqrt();
        let mut samples = Vec::with_capacity(secs * RATE);
        let mut k = 0;
        while samples.len() < secs * RATE {
            let talk = [7.3, 3.1, 12.9, 5.5, 21.7][k % 5];
            let gap = [0.9, 1.25, 1.35, 1.55, 1.65, 2.0, 3.1, 1.45][k % 8];
            // Quiet levels as multiples of the floor, as a sine's amplitude for that RMS.
            let quiet = [3.0, 1.8, 3.6, 2.2][k % 4] * floor * 2f32.sqrt();
            let fade = [0.25, 0.4, 0.15][k % 3];
            let mut tone = |amp: f32, len: f32, samples: &mut Vec<f32>| {
                for n in 0..(len * RATE as f32) as usize {
                    samples.push(amp * (n as f32 * 0.043).sin() + room * noise());
                }
            };
            tone(quiet, fade, &mut samples);
            tone(0.1, talk, &mut samples);
            tone(quiet, fade, &mut samples);
            tone(0.0, gap, &mut samples);
            k += 1;
        }
        samples.truncate(secs * RATE);
        quantize(&samples)
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

    fn chunks_of_pieces(samples: &[f32], pieces: &[(usize, usize)], rules: Rules) -> Vec<(usize, usize)> {
        piece_chunks(pieces, pieces.iter().map(|&(a, b)| chunks(&samples[a..b], rules)).collect())
    }

    #[test]
    fn pieces_give_the_same_chunks_as_the_whole_stem() {
        for (name, samples, target) in [("meeting", meeting(900), TARGET), ("quiet edges", quiet_edges(600), 30 * RATE)] {
            for (multiplier, pad_ms) in SETTINGS {
                let rules = Rules::new(multiplier, pad_ms);
                let pieces = pieces(&samples, target, 2 * RATE, rules);
                assert!(pieces.len() >= 6, "{name} {multiplier}/{pad_ms}: {} pieces", pieces.len());
                assert!(pieces.iter().all(|&(a, _)| a % WINDOW == 0));
                if let Some(diff) = first_difference(&chunks(&samples, rules), &chunks_of_pieces(&samples, &pieces, rules)) {
                    panic!("{name} {multiplier}/{pad_ms}: {diff}");
                }
            }
        }
    }

    #[test]
    fn a_cut_that_padding_reaches_breaks_the_chunks() {
        // What cutting at every reset point would do with padding. This shows that the test
        // above can fail on padding.
        let samples = speech(22, &[(10.0, 11.4)]);
        let at = secs(10.5) + PAUSE;
        let pieces = [(0, at), (at, samples.len())];
        assert!(first_difference(&chunks(&samples, upstream()), &chunks_of_pieces(&samples, &pieces, upstream())).is_none());
        assert!(first_difference(&chunks(&samples, padded()), &chunks_of_pieces(&samples, &pieces, padded())).is_some());
        assert_eq!(resets(&energy_chunks(&samples, 2.5), samples.len(), padded().pad), vec![]);
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
        assert!(first_difference(&chunks(&samples, upstream()), &chunks_of_pieces(&samples, &pieces, upstream())).is_some());
    }
}

/// Core's own chunker, compiled from the pinned minutes-core source, for the tests. It is
/// never copied into this repository: the test reads it from Cargo's checkout.
#[cfg(test)]
mod core_chunker {
    use super::tests::{first_difference, meeting, piece_chunks, quiet_edges, SETTINGS};
    use super::*;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::OnceLock;

    /// The items of core's transcribe.rs that make up its meeting chunker, each found by the
    /// start of its declaration.
    const CORE_ITEMS: [&str; 7] = [
        "const MEETING_CHUNK_MAX_SAMPLES",
        "struct MeetingChunking",
        "impl Default for MeetingChunking",
        "impl MeetingChunking",
        "fn detect_meeting_vad_chunks(",
        "fn pad_meeting_chunks(",
        "fn detect_energy_chunks(",
    ];

    /// Reads a 16-bit mono WAV, a noise multiplier and a pad in ms, which it gives core's
    /// `MeetingChunking::from_config` as the config. Then reads one `start end` range per
    /// stdin line, and prints the chunks of each range, relative to its start.
    const MAIN: &str = r#"
struct Config {
    transcription: TranscriptionConfig,
}

struct TranscriptionConfig {
    meeting_chunk_noise_multiplier: f32,
    meeting_chunk_pad_ms: u32,
}

fn main() {
    use std::io::BufRead;
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("wav path");
    let config = Config {
        transcription: TranscriptionConfig {
            meeting_chunk_noise_multiplier: args.next().expect("multiplier").parse().unwrap(),
            meeting_chunk_pad_ms: args.next().expect("pad ms").parse().unwrap(),
        },
    };
    let chunking = MeetingChunking::from_config(&config);
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
        let out: Vec<String> = detect_meeting_vad_chunks(&samples[a..b], chunking).iter().map(|(s, e)| format!("{s} {e}")).collect();
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

    /// The item of `source` declared by `declaration`, with its doc comment and attributes.
    fn core_item(source: &str, declaration: &str) -> String {
        let at = source
            .match_indices(declaration)
            .map(|(at, _)| at)
            .find(|&at| {
                let line_start = source[..at].rfind('\n').map_or(0, |n| n + 1);
                matches!(source[line_start..at].trim(), "" | "pub(crate)" | "pub")
            })
            .unwrap_or_else(|| panic!("core has no `{declaration}`. Its chunker changed: check piece_cut.rs against it."));
        let mut begin = source[..at].rfind('\n').map_or(0, |n| n + 1);
        while let Some(previous) = source[..begin.saturating_sub(1)].rfind('\n').map(|n| n + 1) {
            let line = source[previous..begin].trim_start();
            if !(line.starts_with("///") || line.starts_with("#[")) {
                break;
            }
            begin = previous;
        }
        let terminator = if declaration.starts_with("const ") { ";\n" } else { "\n}\n" };
        let end = at + source[at..].find(terminator).unwrap_or_else(|| panic!("end of `{declaration}`")) + terminator.len();
        source[begin..end].replace("pub(crate) ", "")
    }

    /// Builds the helper once per test run, in this crate's build folder.
    fn helper() -> &'static Path {
        static HELPER: OnceLock<PathBuf> = OnceLock::new();
        HELPER.get_or_init(|| {
            let source = std::fs::read_to_string(core_transcribe_rs()).expect("read core's transcribe.rs");
            let items: Vec<String> = CORE_ITEMS.iter().map(|d| core_item(&source, d)).collect();
            let dir = Path::new(env!("OUT_DIR")).join("core-chunker");
            std::fs::create_dir_all(&dir).unwrap();
            let main = dir.join("main.rs");
            std::fs::write(&main, format!("#![allow(dead_code)]\n{}\n{MAIN}", items.join("\n"))).unwrap();
            let bin = dir.join("core-chunker");
            let out = Command::new(rustc()).args(["--edition", "2021", "-O", "-o"]).arg(&bin).arg(&main).output().expect("run rustc");
            assert!(out.status.success(), "rustc: {}", String::from_utf8_lossy(&out.stderr));
            bin
        })
    }

    /// Core's chunks of each `(start, end)` range of the WAV at `wav`, with a config of
    /// `multiplier` and `pad_ms`.
    fn core_chunks(wav: &Path, (multiplier, pad_ms): (f32, u32), ranges: &[(usize, usize)]) -> Vec<Vec<(usize, usize)>> {
        let mut child = Command::new(helper())
            .arg(wav)
            .arg(multiplier.to_string())
            .arg(pad_ms.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
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
            ("quiet edges", quiet_edges(300)),
            ("demo", read_wav(&demo_wav())),
            ("silence", vec![0.0; 30 * RATE + 77]),
            ("short sounds", q(short_sounds)),
            ("no pause", q((0..200 * RATE).map(|n| 0.2 * (n as f32 * 0.031).sin()).collect())),
            ("edge", q(edge)),
        ]
    }

    /// Core's demo recording: real speech, 16 kHz mono.
    fn demo_wav() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../vendor/minutes-core/crates/assets/demo.wav")
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = Path::new(env!("OUT_DIR")).join(format!("{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn margin_chunker_is_cores_chunker() {
        let dir = temp_dir("chunker");
        // Margin's settings, plus values that core's config reading replaces, and padding
        // that merges chunks and meets the 45 s limit.
        let settings = SETTINGS.into_iter().chain([(f32::NAN, 0), (0.0, 1_200), (3.0, 5_000)]);
        let signals = signals();
        for (multiplier, pad_ms) in settings {
            let rules = Rules::new(multiplier, pad_ms);
            for (name, samples) in &signals {
                let wav = dir.join("signal.wav");
                write_wav(&wav, samples);
                let samples = read_wav(&wav);
                // The whole signal, and ranges that do not start on the grid or at a reset.
                let ranges = [(0, samples.len()), (777, samples.len()), (0, samples.len() / 3), (12_345, samples.len() / 2)];
                for (core, &(a, b)) in core_chunks(&wav, (multiplier, pad_ms), &ranges).iter().zip(&ranges) {
                    assert_eq!(&chunks(&samples[a..b], rules), core, "{name}, {multiplier}/{pad_ms} ms, samples {a}..{b}");
                }
            }
        }
        std::fs::remove_dir_all(dir).ok();
    }

    /// Cuts `wav` with [`pieces`], and checks with core's chunker that the pieces, and the
    /// audio after the last cut, give the same chunks as the whole file.
    fn assert_pieces_chunk_like_the_whole_stem(wav: &Path, target: usize, settings: (f32, u32)) -> (usize, usize) {
        let rules = Rules::new(settings.0, settings.1);
        let samples = read_wav(wav);
        let pieces = pieces(&samples, target, 2 * RATE, rules);
        assert!(pieces.len() >= 2, "too few pieces to compare");
        let whole = core_chunks(wav, settings, &[(0, samples.len())]).remove(0);
        let per_piece = piece_chunks(&pieces, core_chunks(wav, settings, &pieces));
        if let Some(diff) = first_difference(&whole, &per_piece) {
            panic!("{settings:?}: {diff}\npieces {pieces:?}\nenergy {:?}\nwhole {whole:?}", energy_chunks(&samples, rules.loud));
        }
        // Margin's chunker on the whole stem as well.
        assert!(chunks(&samples, rules) == whole, "Margin's chunker differs from core's on this stem");
        (pieces.len(), whole.len())
    }

    #[test]
    fn pieces_give_cores_chunks_of_the_whole_stem() {
        let dir = temp_dir("pieces");
        for (name, samples, target) in [("meeting", meeting(900), 120 * RATE), ("quiet-edges", quiet_edges(600), 30 * RATE)] {
            let wav = dir.join(format!("{name}.wav"));
            write_wav(&wav, &samples);
            for settings in SETTINGS {
                assert_pieces_chunk_like_the_whole_stem(&wav, target, settings);
            }
        }
        // Real speech: core's 10.6 s demo recording six times over, after 1.3 s to 2.5 s of
        // silence, in 20 s pieces. The demo itself has no pause, so shorter pieces would end
        // at the quietest window, which is not exact.
        let wav = dir.join("demo-x6.wav");
        let demo = read_wav(&demo_wav());
        let speech: Vec<f32> = [1.3, 2.5, 1.4, 2.2, 1.5, 2.4]
            .iter()
            .flat_map(|&gap: &f32| std::iter::repeat_n(0.0, (gap * RATE as f32) as usize).chain(demo.iter().copied()))
            .collect();
        write_wav(&wav, &speech);
        for settings in SETTINGS {
            assert_pieces_chunk_like_the_whole_stem(&wav, 20 * RATE, settings);
        }
        std::fs::remove_dir_all(dir).ok();
    }

    /// A real stem: `MARGIN_REAL_STEM=/path/x.voice.wav cargo test real_stem -- --ignored
    /// --nocapture`. It runs no model, so it takes seconds.
    #[test]
    #[ignore]
    fn real_stem_pieces_give_cores_chunks_of_the_whole_stem() {
        let path = PathBuf::from(std::env::var("MARGIN_REAL_STEM").expect("MARGIN_REAL_STEM"));
        for settings in SETTINGS {
            let (pieces, chunks) = assert_pieces_chunk_like_the_whole_stem(&path, 120 * RATE, settings);
            println!("{}, {settings:?}: {pieces} pieces, {chunks} chunks, all the same as the whole stem", path.display());
        }
    }
}
