//! Local powerset speaker tracks. Local channel numbers are never global IDs.
use std::collections::BTreeSet;
use std::ops::Range;

pub(super) const FRAME_HOP: usize = 270;
pub(super) const FRAME_START: usize = 721;
/// Clustering's input budget: two hours of ten-second windows, three local
/// speakers each.
pub(super) const MAX_OBSERVATIONS: usize = 2160;

#[derive(Debug)]
pub(super) struct LocalSpeakerTrack {
    pub window: usize,
    pub regions: Vec<Range<usize>>,
    // Only frames with exactly this speaker active may identify a voice.
    pub clean_regions: Vec<Range<usize>>,
}

#[derive(Clone)]
pub(super) struct Observation {
    pub embedding: Vec<f32>,
    pub window: usize,
    pub seconds: f64,
}

pub(super) struct Clustered {
    pub roots: Vec<usize>,
    pub embeddings: Vec<Vec<f32>>,
}

/// Bounds on the number of speakers clustering may return. `None` leaves that
/// side to the similarity threshold alone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct SpeakerBounds {
    pub min: Option<usize>,
    pub max: Option<usize>,
}

/// Clusters being agglomerated. A cluster lives at the index of its first
/// observation; `roots` maps every observation to its cluster.
struct Agglomeration {
    roots: Vec<usize>,
    active: Vec<bool>,
    revisions: Vec<u32>,
    embeddings: Vec<Vec<f32>>,
    weights: Vec<f64>,
    windows: Vec<BTreeSet<usize>>,
}

impl Agglomeration {
    fn active_count(&self) -> usize {
        self.active.iter().filter(|&&value| value).count()
    }

    /// Fold cluster `j` into `i`: the centroid is the speech-weighted mean.
    fn merge(&mut self, i: usize, j: usize) {
        let total = self.weights[i] + self.weights[j];
        for dimension in 0..self.embeddings[i].len() {
            self.embeddings[i][dimension] = ((self.embeddings[i][dimension] as f64
                * self.weights[i]
                + self.embeddings[j][dimension] as f64 * self.weights[j])
                / total) as f32;
        }
        self.weights[i] = total;
        self.active[j] = false;
        self.revisions[i] += 1;
        let other = std::mem::take(&mut self.windows[j]);
        self.windows[i].extend(other);
        for root in &mut self.roots {
            if *root == j {
                *root = i;
            }
        }
    }

    /// The most similar pair of active clusters, ignoring the same-window
    /// constraint. Only used to honour `SpeakerBounds::max` once no
    /// compatible pair is left.
    fn closest_active_pair(&self) -> Option<(usize, usize)> {
        let active: Vec<usize> = (0..self.active.len()).filter(|&k| self.active[k]).collect();
        let mut best: Option<(f32, usize, usize)> = None;
        for (position, &i) in active.iter().enumerate() {
            for &j in &active[position + 1..] {
                let similarity =
                    crate::voice::cosine_similarity(&self.embeddings[i], &self.embeddings[j]);
                if best.is_none_or(|(top, _, _)| similarity > top) {
                    best = Some((similarity, i, j));
                }
            }
        }
        best.map(|(_, i, j)| (i, j))
    }
}

#[derive(PartialEq)]
struct Pair {
    similarity: f32,
    left: usize,
    right: usize,
    left_revision: u32,
    right_revision: u32,
}
impl Eq for Pair {}
impl PartialOrd for Pair {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Pair {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.similarity
            .total_cmp(&other.similarity)
            .then(other.left.cmp(&self.left))
            .then(other.right.cmp(&self.right))
            .then(self.left_revision.cmp(&other.left_revision))
            .then(self.right_revision.cmp(&other.right_revision))
    }
}

/// Offline centroid agglomeration: merge the strongest compatible pair first,
/// rather than committing each arriving observation to an early template.
/// Maximum observations follows the two-hour, ten-second, three-local-slot
/// input contract. The pair queue is bounded, fallibly allocated and checked
/// for cancellation throughout construction and merging.
///
/// `bounds.min` stops merging at that many clusters even above the threshold.
/// `bounds.max` keeps merging below the threshold until at most that many are
/// left; when only same-window pairs remain, the closest clusters merge anyway,
/// because the caller asked for that count.
pub(super) fn cluster_embeddings(
    observations: &[Observation],
    threshold: f32,
    max_clusters: usize,
    bounds: SpeakerBounds,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Clustered, &'static str> {
    let count = observations.len();
    if count > MAX_OBSERVATIONS {
        return Err("diarization observation budget exceeded");
    }
    if !threshold.is_finite()
        || observations.iter().any(|o| {
            o.embedding.is_empty()
                || !o.seconds.is_finite()
                || o.seconds <= 0.0
                || o.embedding.iter().any(|v| !v.is_finite())
                || o.embedding.len() != observations[0].embedding.len()
        })
    {
        return Err("invalid diarization clustering evidence");
    }
    let mut state = Agglomeration {
        roots: (0..count).collect(),
        active: vec![true; count],
        revisions: vec![0_u32; count],
        embeddings: observations.iter().map(|o| o.embedding.clone()).collect(),
        weights: observations.iter().map(|o| o.seconds).collect(),
        windows: observations
            .iter()
            .map(|o| BTreeSet::from([o.window]))
            .collect(),
    };
    let min_clusters = bounds.min.unwrap_or(1).max(1);
    let max_wanted = bounds.max.map(|max| max.max(min_clusters));
    let mut pairs = std::collections::BinaryHeap::new();
    for i in 0..count {
        if cancelled() {
            return Err("diarization cancelled");
        }
        for j in i + 1..count {
            if observations[i].window != observations[j].window {
                queue_pair(&mut pairs, &state, i, j, Some(threshold))?;
            }
        }
    }
    let mut clusters = count;
    merge_queued(&mut state, &mut pairs, &mut clusters, min_clusters, Some(threshold), &mut cancelled)?;
    if let Some(max) = max_wanted.filter(|&max| clusters > max) {
        // Below the threshold now: only the clusters left, so at most k^2/2
        // pairs for k clusters, not one per observation pair.
        let active: Vec<usize> = (0..count).filter(|&k| state.active[k]).collect();
        for (position, &i) in active.iter().enumerate() {
            for &j in &active[position + 1..] {
                if state.windows[i].is_disjoint(&state.windows[j]) {
                    queue_pair(&mut pairs, &state, i, j, None)?;
                }
            }
        }
        merge_queued(&mut state, &mut pairs, &mut clusters, max, None, &mut cancelled)?;
        while clusters > max {
            if cancelled() {
                return Err("diarization cancelled");
            }
            let Some((i, j)) = state.closest_active_pair() else {
                break;
            };
            state.merge(i, j);
            clusters -= 1;
        }
    }
    if state.active_count() > max_clusters {
        return Err("diarization speaker-template budget exceeded");
    }
    Ok(Clustered {
        roots: state.roots,
        embeddings: state.embeddings,
    })
}

/// Queue the pair `(i, j)` at the clusters' current revisions, unless its
/// similarity is at or below `floor`. The queue is bounded and fallibly grown.
fn queue_pair(
    pairs: &mut std::collections::BinaryHeap<Pair>,
    state: &Agglomeration,
    i: usize,
    j: usize,
    floor: Option<f32>,
) -> Result<(), &'static str> {
    const MAX_PAIRS: usize = MAX_OBSERVATIONS * MAX_OBSERVATIONS;
    let (left, right) = (i.min(j), i.max(j));
    let similarity = crate::voice::cosine_similarity(&state.embeddings[left], &state.embeddings[right]);
    if floor.is_some_and(|floor| similarity <= floor) {
        return Ok(());
    }
    if pairs.len() >= MAX_PAIRS {
        return Err("diarization pair budget exceeded");
    }
    if pairs.len() == pairs.capacity() {
        pairs
            .try_reserve(1024.min(MAX_PAIRS - pairs.len()))
            .map_err(|_| "diarization pair allocation refused")?;
    }
    pairs.push(Pair {
        similarity,
        left,
        right,
        left_revision: state.revisions[left],
        right_revision: state.revisions[right],
    });
    Ok(())
}

/// Merge the most similar compatible pairs from the queue while more than
/// `stop_at` clusters remain. Pairs created by a merge are queued under the
/// same `floor`. The heap pops in falling similarity, so the queue running dry
/// is the only other stop.
fn merge_queued(
    state: &mut Agglomeration,
    pairs: &mut std::collections::BinaryHeap<Pair>,
    clusters: &mut usize,
    stop_at: usize,
    floor: Option<f32>,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<(), &'static str> {
    while *clusters > stop_at {
        let Some(pair) = pairs.pop() else {
            break;
        };
        if cancelled() {
            return Err("diarization cancelled");
        }
        let (i, j) = (pair.left, pair.right);
        if !state.active[i]
            || !state.active[j]
            || pair.left_revision != state.revisions[i]
            || pair.right_revision != state.revisions[j]
            || !state.windows[i].is_disjoint(&state.windows[j])
        {
            continue;
        }
        state.merge(i, j);
        *clusters -= 1;
        for k in 0..state.active.len() {
            if k != i && state.active[k] && state.windows[i].is_disjoint(&state.windows[k]) {
                queue_pair(pairs, state, i, k, floor)?;
            }
        }
    }
    pairs.clear();
    Ok(())
}

/// Speakers for tracks that clustering left out (`assignments[i] == None`).
///
/// A track never takes a speaker that another clustered track of its own
/// window has: the segmentation model already said they are different people.
/// Among the other speakers, a track takes:
/// 1. the speaker of the nearest clustered speech before and after it, when
///    both are the same speaker and each is within `max_gap` samples;
/// 2. else the cluster nearest to its own embedding, when it has one and the
///    similarity is above `min_similarity`;
/// 3. else nothing, and stays unknown.
///
/// Only the clustering's own assignments are evidence, so the result does not
/// depend on the order in which tracks are attached.
pub(super) fn attach_short_tracks(
    tracks: &[LocalSpeakerTrack],
    assignments: &mut [Option<usize>],
    short_embeddings: &[Option<Vec<f32>>],
    centroids: &[Vec<f32>],
    min_similarity: f32,
    max_gap: usize,
) {
    let clustered = assignments.to_vec();
    let mut speakers: Vec<usize> = clustered.iter().flatten().copied().collect();
    speakers.sort_unstable();
    speakers.dedup();
    for index in 0..tracks.len() {
        if clustered[index].is_some() {
            continue;
        }
        let track = &tracks[index];
        let (Some(first), Some(last)) = (track.regions.first(), track.regions.last()) else {
            continue;
        };
        let (start, end) = (first.start, last.end);
        let excluded: BTreeSet<usize> = tracks
            .iter()
            .zip(&clustered)
            .filter(|(other, _)| other.window == track.window)
            .filter_map(|(_, root)| *root)
            .collect();
        let mut before: Option<(usize, usize)> = None;
        let mut after: Option<(usize, usize)> = None;
        for (other, root) in tracks.iter().zip(&clustered) {
            let Some(root) = *root else { continue };
            if excluded.contains(&root) {
                continue;
            }
            for region in &other.regions {
                if region.end <= start && before.is_none_or(|(at, _)| region.end > at) {
                    before = Some((region.end, root));
                }
                if region.start >= end && after.is_none_or(|(at, _)| region.start < at) {
                    after = Some((region.start, root));
                }
            }
        }
        if let (Some((before_end, left)), Some((after_start, right))) = (before, after) {
            if left == right && start - before_end <= max_gap && after_start - end <= max_gap {
                assignments[index] = Some(left);
                continue;
            }
        }
        let Some(embedding) = &short_embeddings[index] else {
            continue;
        };
        assignments[index] = speakers
            .iter()
            .filter(|root| !excluded.contains(root))
            .map(|&root| (crate::voice::cosine_similarity(embedding, &centroids[root]), root))
            .filter(|(similarity, _)| *similarity > min_similarity)
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, root)| root);
    }
}

fn append_range(ranges: &mut Vec<Range<usize>>, range: Range<usize>) {
    if range.is_empty() {
        return;
    }
    if let Some(last) = ranges.last_mut() {
        if last.end == range.start {
            last.end = range.end;
            return;
        }
    }
    ranges.push(range);
}

/// Powerset class -> bit mask of the three local speakers active in a frame.
const MASKS: [u8; 7] = [0, 1, 2, 4, 3, 5, 6];

/// The seven classes encode silence, three singletons, then three pairs.
/// Retain both speakers during overlap, but never feed mixed frames to an
/// embedding. The clock resets to the real start of each model input window.
pub(super) fn decode_window(
    window: usize,
    total_samples: usize,
    classes: &[usize],
    remaining_regions: usize,
) -> Result<Vec<LocalSpeakerTrack>, &'static str> {
    Ok(decode_window_slots(window, total_samples, classes, remaining_regions)?
        .into_iter()
        .map(|(_, track)| track)
        .collect())
}

/// `decode_window`, with each track's local slot (0..3) kept.
pub(super) fn decode_window_slots(
    window: usize,
    total_samples: usize,
    classes: &[usize],
    remaining_regions: usize,
) -> Result<Vec<(usize, LocalSpeakerTrack)>, &'static str> {
    let mut tracks: Vec<_> = (0..3)
        .map(|_| LocalSpeakerTrack {
            window,
            regions: Vec::new(),
            clean_regions: Vec::new(),
        })
        .collect();
    let mut regions = 0;
    for (index, &class) in classes.iter().enumerate() {
        let mask = *MASKS
            .get(class)
            .ok_or("invalid diarization powerset class")?;
        let start = window
            .saturating_add(FRAME_START)
            .saturating_add(index.saturating_mul(FRAME_HOP))
            .min(total_samples);
        let end = start.saturating_add(FRAME_HOP).min(total_samples);
        for (speaker, track) in tracks.iter_mut().enumerate() {
            if mask & (1 << speaker) == 0 || start == end {
                continue;
            }
            let before = track.regions.len() + track.clean_regions.len();
            append_range(&mut track.regions, start..end);
            if mask.count_ones() == 1 {
                append_range(&mut track.clean_regions, start..end);
            }
            regions += track.regions.len() + track.clean_regions.len() - before;
            if regions > remaining_regions {
                return Err("diarization segment budget exceeded");
            }
        }
    }
    Ok(tracks
        .into_iter()
        .enumerate()
        .filter(|(_, track)| !track.regions.is_empty())
        .collect())
}

/// `ranges` widened to whole frames of `aggregate_windows`' global grid
/// (multiples of `FRAME_HOP`). Track regions start at `window + FRAME_START +
/// k·FRAME_HOP`, off that grid, so without this the first samples of almost
/// every aggregated range would look uncovered.
pub(super) fn snap_to_frame_grid(ranges: &[Range<usize>]) -> Vec<Range<usize>> {
    ranges
        .iter()
        .map(|range| (range.start / FRAME_HOP) * FRAME_HOP..range.end.div_ceil(FRAME_HOP) * FRAME_HOP)
        .collect()
}

/// Samples of `speech` that no range in `covered` overlaps. Sorts `covered`.
pub(super) fn uncovered_samples(speech: &[Range<usize>], covered: &mut [Range<usize>]) -> usize {
    covered.sort_by_key(|range| range.start);
    let mut uncovered = 0;
    for range in speech {
        let mut cursor = range.start;
        for cover in covered.iter() {
            if cover.end <= cursor {
                continue;
            }
            if cover.start >= range.end {
                break;
            }
            uncovered += cover.start.saturating_sub(cursor);
            cursor = cursor.max(cover.end);
            if cursor >= range.end {
                break;
            }
        }
        uncovered += range.end.saturating_sub(cursor);
    }
    uncovered
}

/// One model window of the full pipeline: its start sample, its frame
/// classes, and the global speaker (if any) each local slot was clustered to.
pub(super) struct ClusteredWindow {
    pub start: usize,
    pub classes: Vec<usize>,
    pub speakers: [Option<usize>; 3],
}

/// pyannote's aggregation over overlapping windows, on a global grid of
/// `FRAME_HOP` samples. For each grid frame:
/// - the speaker count is the mean, over the windows covering it, of the
///   number of active local speakers, rounded;
/// - a global speaker's score is the share of covering windows in which a
///   local slot clustered to it is active;
/// - the frame goes to the `count` best-scoring speakers with a score above 0.
///
/// Returns, per global speaker in `speakers`, its active sample ranges.
pub(super) fn aggregate_windows(
    windows: &[ClusteredWindow],
    total_samples: usize,
    speakers: usize,
) -> Vec<Vec<Range<usize>>> {
    let frames = total_samples.div_ceil(FRAME_HOP);
    let mut coverage = vec![0_u16; frames];
    let mut count_sum = vec![0_u16; frames];
    let mut scores = vec![vec![0_u16; frames]; speakers];
    for window in windows {
        for (index, &class) in window.classes.iter().enumerate() {
            let sample = window.start + FRAME_START + index * FRAME_HOP;
            if sample >= total_samples {
                break;
            }
            let frame = sample / FRAME_HOP;
            let mask = MASKS.get(class).copied().unwrap_or(0);
            coverage[frame] = coverage[frame].saturating_add(1);
            count_sum[frame] = count_sum[frame].saturating_add(mask.count_ones() as u16);
            let mut seen = [false; 64];
            for slot in 0..3 {
                if mask & (1 << slot) == 0 {
                    continue;
                }
                if let Some(speaker) = window.speakers[slot].filter(|&s| s < speakers) {
                    // Two slots forced onto one speaker count once.
                    if speaker < seen.len() && !std::mem::replace(&mut seen[speaker], true) {
                        scores[speaker][frame] = scores[speaker][frame].saturating_add(1);
                    }
                }
            }
        }
    }
    let mut ranges = vec![Vec::new(); speakers];
    let mut ranked: Vec<usize> = (0..speakers).collect();
    for frame in 0..frames {
        if coverage[frame] == 0 {
            continue;
        }
        let count = (count_sum[frame] as f32 / coverage[frame] as f32).round() as usize;
        if count == 0 {
            continue;
        }
        ranked.sort_by(|&a, &b| scores[b][frame].cmp(&scores[a][frame]).then(a.cmp(&b)));
        for &speaker in ranked.iter().take(count) {
            if scores[speaker][frame] == 0 {
                break;
            }
            let start = frame * FRAME_HOP;
            append_range(&mut ranges[speaker], start..(start + FRAME_HOP).min(total_samples));
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(embedding: &[f32], window: usize) -> Observation {
        Observation {
            embedding: embedding.to_vec(),
            window,
            seconds: 3.0,
        }
    }

    #[test]
    fn local_speakers_cannot_collapse_even_with_identical_embeddings() {
        let result = cluster_embeddings(
            &[observation(&[1., 0.], 0), observation(&[1., 0.], 0)],
            0.4,
            64,
            SpeakerBounds::default(),
            || false,
        )
        .unwrap();
        assert_ne!(result.roots[0], result.roots[1]);
        assert_eq!(result.embeddings[0], vec![1., 0.]);
    }

    #[test]
    fn local_channel_permutation_does_not_permute_global_identity() {
        let result = cluster_embeddings(
            &[
                observation(&[1., 0.], 0),
                observation(&[0., 1.], 0),
                observation(&[0., 1.], 160_000),
                observation(&[1., 0.], 160_000),
            ],
            0.4,
            64,
            SpeakerBounds::default(),
            || false,
        )
        .unwrap();
        assert_eq!(result.roots[0], result.roots[3]);
        assert_eq!(result.roots[1], result.roots[2]);
        assert_ne!(result.roots[0], result.roots[1]);
    }

    #[test]
    fn transitive_merge_retains_all_cannot_link_windows() {
        let result = cluster_embeddings(
            &[
                observation(&[1., 0.], 0),
                observation(&[1., 0.], 1),
                observation(&[1., 0.], 1),
            ],
            0.3,
            64,
            SpeakerBounds::default(),
            || false,
        )
        .unwrap();
        assert_ne!(result.roots[1], result.roots[2]);
    }

    #[test]
    fn global_best_pair_prevents_an_early_ambiguous_merge() {
        let input = [
            observation(&[0.8, 0.6], 0),
            observation(&[1., 0.], 1),
            observation(&[0.8, 0.6], 1),
        ];
        let result = cluster_embeddings(&input, 0.4, 64, SpeakerBounds::default(), || false).unwrap();
        assert_eq!(result.roots[0], result.roots[2]);
        assert_ne!(result.roots[0], result.roots[1]);
    }

    #[test]
    fn clustering_is_cancelled_bounded_and_rejects_nonfinite_evidence() {
        let input = [observation(&[1., 0.], 0), observation(&[0., 1.], 0)];
        let none = SpeakerBounds::default();
        assert!(cluster_embeddings(&input, 0.4, 1, none, || false).is_err());
        assert!(cluster_embeddings(&input, 0.4, 64, none, || true).is_err());
        assert!(cluster_embeddings(&[observation(&[f32::NAN], 0)], 0.4, 64, none, || false).is_err());
        assert!(
            cluster_embeddings(&vec![observation(&[1.], 0); 2161], 0.4, 64, none, || false).is_err()
        );
        assert!(cluster_embeddings(&[], 0.4, 64, none, || false)
            .unwrap()
            .roots
            .is_empty());
    }

    fn distinct_roots(result: &Clustered) -> usize {
        result.roots.iter().collect::<BTreeSet<_>>().len()
    }

    #[test]
    fn a_maximum_merges_the_closest_clusters_below_the_threshold() {
        let input = [
            observation(&[1., 0., 0.], 0),
            observation(&[0.9, 0.44, 0.], 1),
            observation(&[0., 1., 0.], 2),
            observation(&[0., 0., 1.], 3),
        ];
        let free = cluster_embeddings(&input, 0.95, 64, SpeakerBounds::default(), || false).unwrap();
        assert_eq!(distinct_roots(&free), 4);
        let bounds = SpeakerBounds { min: None, max: Some(3) };
        let capped = cluster_embeddings(&input, 0.95, 64, bounds, || false).unwrap();
        assert_eq!(distinct_roots(&capped), 3);
        assert_eq!(capped.roots[0], capped.roots[1], "the closest pair merges first");
    }

    #[test]
    fn a_maximum_overrides_the_same_window_constraint_last() {
        let input = [observation(&[1., 0.], 0), observation(&[0.8, 0.6], 0)];
        let bounds = SpeakerBounds { min: None, max: Some(1) };
        let result = cluster_embeddings(&input, 0.4, 64, bounds, || false).unwrap();
        assert_eq!(result.roots[0], result.roots[1]);
    }

    #[test]
    fn a_minimum_stops_merging_above_the_threshold() {
        let input = [
            observation(&[1., 0.], 0),
            observation(&[0.99, 0.14], 1),
            observation(&[0.98, 0.2], 2),
        ];
        let free = cluster_embeddings(&input, 0.4, 64, SpeakerBounds::default(), || false).unwrap();
        assert_eq!(distinct_roots(&free), 1);
        let bounds = SpeakerBounds { min: Some(2), max: None };
        let result = cluster_embeddings(&input, 0.4, 64, bounds, || false).unwrap();
        assert_eq!(distinct_roots(&result), 2);
    }

    fn track(window: usize, regions: &[Range<usize>]) -> LocalSpeakerTrack {
        LocalSpeakerTrack {
            window,
            regions: regions.to_vec(),
            clean_regions: Vec::new(),
        }
    }

    #[test]
    fn a_short_track_between_one_speaker_in_other_windows_takes_that_speaker() {
        let tracks = [
            track(0, &[100..900]),
            track(1, &[1000..1100]),
            track(2, &[1200..2000]),
        ];
        let mut assignments = [Some(0), None, Some(0)];
        attach_short_tracks(&tracks, &mut assignments, &[None, None, None], &[vec![1.0]], 0.4, 200);
        assert_eq!(assignments, [Some(0), Some(0), Some(0)]);

        let mut far = [Some(0), None, Some(0)];
        attach_short_tracks(&tracks, &mut far, &[None, None, None], &[vec![1.0]], 0.4, 50);
        assert_eq!(far[1], None, "neighbours beyond the gap are not evidence");
    }

    #[test]
    fn a_short_track_never_takes_a_speaker_of_its_own_window() {
        // A backchannel inside one speaker's window: a different local speaker.
        let tracks = [track(0, &[0..400, 600..1000]), track(0, &[450..550])];
        let mut assignments = [Some(0), None];
        let centroids = [vec![1.0, 0.0]];
        let short = [None, Some(vec![1.0, 0.0])];
        attach_short_tracks(&tracks, &mut assignments, &short, &centroids, 0.4, 1000);
        assert_eq!(assignments[1], None);
    }

    #[test]
    fn a_short_track_falls_back_to_the_nearest_cluster_above_the_threshold() {
        let tracks = [
            track(0, &[0..400]),
            track(1, &[1000..1400]),
            track(2, &[5000..5100]),
            track(3, &[9000..9100]),
        ];
        let centroids = [vec![1.0, 0.0], vec![0.0, 1.0]];
        let short = [None, None, Some(vec![0.2, 0.9]), Some(vec![0.7, -0.7])];
        let mut assignments = [Some(0), Some(1), None, None];
        attach_short_tracks(&tracks, &mut assignments, &short, &centroids, 0.4, 100);
        assert_eq!(assignments[2], Some(1));
        assert_eq!(assignments[3], Some(0));
        let mut strict = [Some(0), Some(1), None, None];
        attach_short_tracks(&tracks, &mut strict, &short, &centroids, 0.8, 100);
        assert_eq!(strict[2], Some(1));
        assert_eq!(strict[3], None, "0.71 is under the 0.8 floor");
    }

    #[test]
    fn uncovered_samples_counts_speech_outside_every_cover() {
        let mut covers = vec![50..70, 0..20, 15..30];
        assert_eq!(uncovered_samples(&[0..100], &mut covers), 50);
        assert_eq!(uncovered_samples(&[10..25], &mut covers), 0);
        assert_eq!(uncovered_samples(&[200..210], &mut covers), 10);
    }

    #[test]
    fn a_fully_embedded_speaker_in_an_off_grid_window_is_fully_covered() {
        // A window starting off the global grid, one speaker active throughout.
        let start = 40_000;
        let total = start + 160_000;
        let classes = vec![1; 589];
        let tracks = decode_window_slots(start, total, &classes, 100).unwrap();
        let regions = tracks[0].1.regions.clone();
        let windows = [ClusteredWindow { start, classes, speakers: [Some(0), None, None] }];
        let ranges = aggregate_windows(&windows, total, 1);
        assert!(uncovered_samples(&ranges[0], &mut regions.clone()) > 0, "the grids differ");
        assert_eq!(uncovered_samples(&ranges[0], &mut snap_to_frame_grid(&regions)), 0);
    }

    #[test]
    fn decoding_keeps_each_tracks_local_slot() {
        let slots: Vec<usize> = decode_window_slots(0, 160_000, &[3, 0, 3], 100)
            .unwrap()
            .into_iter()
            .map(|(slot, _)| slot)
            .collect();
        assert_eq!(slots, vec![2]);
    }

    #[test]
    fn aggregation_votes_across_overlapping_windows_and_keeps_overlap() {
        let frames = 4;
        let total = FRAME_START + frames * FRAME_HOP;
        // Window A says slot 0 for all frames; window B (same start, another
        // clustering of the same audio) says slot 1 -> speaker 1 on the last
        // two frames, overlapping speaker 0 there.
        let windows = [
            ClusteredWindow { start: 0, classes: vec![1, 1, 4, 4], speakers: [Some(0), Some(1), None] },
            ClusteredWindow { start: 0, classes: vec![1, 1, 4, 4], speakers: [Some(0), Some(1), None] },
        ];
        let ranges = aggregate_windows(&windows, total, 2);
        let first = FRAME_START / FRAME_HOP;
        assert_eq!(ranges[0], vec![first * FRAME_HOP..(first + 4) * FRAME_HOP]);
        assert_eq!(ranges[1], vec![(first + 2) * FRAME_HOP..(first + 4) * FRAME_HOP]);

        // Disagreeing windows: the speaker count rounds to one, and the
        // speaker more windows saw active takes the frame.
        let windows = [
            ClusteredWindow { start: 0, classes: vec![1], speakers: [Some(0), None, None] },
            ClusteredWindow { start: 0, classes: vec![1], speakers: [Some(0), None, None] },
            ClusteredWindow { start: 0, classes: vec![2], speakers: [None, Some(1), None] },
        ];
        let ranges = aggregate_windows(&windows, total, 2);
        assert_eq!(ranges[0].len(), 1);
        assert!(ranges[1].is_empty());
    }

    #[test]
    fn adjacent_speaker_change_survives_without_silence() {
        let tracks = decode_window(0, 160_000, &[1, 1, 2, 2], 100).unwrap();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].regions, vec![721..1261]);
        assert_eq!(tracks[1].regions, vec![1261..1801]);
    }

    #[test]
    fn all_overlap_pairs_keep_both_tracks_and_no_identity_audio() {
        for class in 4..7 {
            let tracks = decode_window(0, 160_000, &[class], 100).unwrap();
            assert_eq!(tracks.len(), 2);
            assert!(tracks.iter().all(|track| track.regions.len() == 1
                && track.regions[0] == (721..991)
                && track.clean_regions.is_empty()));
        }
    }

    #[test]
    fn overlap_is_removed_only_from_embedding_regions() {
        let tracks = decode_window(0, 160_000, &[1, 4, 1, 2], 100).unwrap();
        assert_eq!(tracks[0].regions, vec![721..1531]);
        assert_eq!(tracks[0].clean_regions, vec![721..991, 1261..1531]);
        assert_eq!(tracks[1].regions, vec![991..1261, 1531..1801]);
        assert_eq!(tracks[1].clean_regions, vec![1531..1801]);
    }

    #[test]
    fn ten_minute_clock_and_padded_tail_use_audio_coordinates() {
        for window in (0..10_240_000).step_by(160_000) {
            let tracks = decode_window(window, 10_240_000, &[1; 589], 10).unwrap();
            assert_eq!(tracks[0].window, window);
            assert_eq!(tracks[0].regions[0].start, window + FRAME_START);
            assert!(tracks[0].regions[0].end <= window + 160_000);
        }
        let tracks = decode_window(160_000, 161_000, &[1; 589], 10).unwrap();
        assert_eq!(tracks[0].regions, vec![160_721..161_000]);
        assert!(decode_window(160_000, 160_100, &[1; 589], 10)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn rejects_bad_classes_and_fragment_budget() {
        assert!(decode_window(0, 160_000, &[7], 100).is_err());
        assert!(decode_window(0, 160_000, &[1, 0, 1], 3).is_err());
    }
}
