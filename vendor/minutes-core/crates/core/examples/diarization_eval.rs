//! Small, reproducible diarization comparison over repository-owned audio.
//!
//! The fixtures contain one known speaker. They measure speech-boundary DER,
//! speaker-count accuracy, and boundary error; they do not claim to measure
//! multi-speaker confusion or reproduce pyannote's academic benchmark suite.

use minutes_core::config::Config;
use minutes_core::diarize::{self, SpeakerSegment};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

const FRAME_SECONDS: f64 = 0.01;

#[derive(Debug, Deserialize)]
struct EvalCase {
    id: String,
    audio: PathBuf,
    duration: f64,
    segments: Vec<ReferenceSegment>,
}

#[derive(Debug, Deserialize)]
struct ReferenceSegment {
    speaker: String,
    start: f64,
    end: f64,
}

#[derive(Default)]
struct Metrics {
    reference_frames: usize,
    missed_frames: usize,
    false_alarm_frames: usize,
    confusion_frames: usize,
    boundary_error_sum: f64,
    boundary_count: usize,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut model = None;
    let mut models_dir = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--model" => model = args.next(),
            "--models-dir" => models_dir = args.next().map(PathBuf::from),
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    let model = model.ok_or("usage: diarization_eval --model legacy|community-1")?;
    if !diarize::DIARIZATION_MODEL_NAMES.contains(&model.as_str()) {
        return Err(format!("unknown diarization model: {model}").into());
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("could not resolve repository root")?;
    let reference_path = root.join("tests/eval/fixtures/diarization-reference.json");
    let cases: Vec<EvalCase> = serde_json::from_str(&std::fs::read_to_string(reference_path)?)?;

    let mut config = Config::default();
    config.diarization.engine = "pyannote-rs".into();
    config.diarization.model = model.clone();
    if let Some(models_dir) = models_dir {
        config.diarization.model_path = models_dir;
    }

    println!("diarization model: {model}");
    println!(
        "frame step: {:.0} ms; collar: 0 ms; overlap: scored",
        FRAME_SECONDS * 1000.0
    );
    let mut totals = Metrics::default();
    let mut exact_counts = 0usize;

    for case in &cases {
        let audio = root.join(&case.audio);
        let result = diarize::diarize(&audio, &config).ok_or_else(|| {
            format!(
                "diarization failed for {} (install with `minutes setup --diarization --model {model}`)",
                audio.display()
            )
        })?;
        let metrics = score_case(case, &result.segments);
        let reference_speakers: HashSet<_> = case
            .segments
            .iter()
            .map(|segment| &segment.speaker)
            .collect();
        let predicted_speakers: HashSet<_> = result
            .segments
            .iter()
            .filter(|segment| segment.end > segment.start)
            .map(|segment| &segment.speaker)
            .collect();
        let count_exact = reference_speakers.len() == predicted_speakers.len();
        exact_counts += usize::from(count_exact);

        println!(
            "case={} DER={:.2}% miss={:.2}% false_alarm={:.2}% confusion={:.2}% speakers={}/{} count_exact={} boundary_MAE={:.3}s",
            case.id,
            percent(
                metrics.missed_frames + metrics.false_alarm_frames + metrics.confusion_frames,
                metrics.reference_frames
            ),
            percent(metrics.missed_frames, metrics.reference_frames),
            percent(metrics.false_alarm_frames, metrics.reference_frames),
            percent(metrics.confusion_frames, metrics.reference_frames),
            predicted_speakers.len(),
            reference_speakers.len(),
            count_exact,
            boundary_mae(&metrics)
        );
        add_metrics(&mut totals, &metrics);
    }

    println!(
        "TOTAL model={} DER={:.2}% miss={:.2}% false_alarm={:.2}% confusion={:.2}% speaker_count_accuracy={}/{} boundary_MAE={:.3}s",
        model,
        percent(
            totals.missed_frames + totals.false_alarm_frames + totals.confusion_frames,
            totals.reference_frames
        ),
        percent(totals.missed_frames, totals.reference_frames),
        percent(totals.false_alarm_frames, totals.reference_frames),
        percent(totals.confusion_frames, totals.reference_frames),
        exact_counts,
        cases.len(),
        boundary_mae(&totals)
    );
    Ok(())
}

fn score_case(case: &EvalCase, predicted: &[SpeakerSegment]) -> Metrics {
    let mut overlap: HashMap<(&str, &str), usize> = HashMap::new();
    let frame_count = (case.duration / FRAME_SECONDS).ceil() as usize;
    for frame in 0..frame_count {
        let time = (frame as f64 + 0.5) * FRAME_SECONDS;
        if let (Some(reference), Some(candidate)) = (
            reference_at(&case.segments, time),
            predicted_at(predicted, time),
        ) {
            *overlap.entry((candidate, reference)).or_default() += 1;
        }
    }

    let mut pairs: Vec<_> = overlap.into_iter().collect();
    pairs.sort_by_key(|(_, frames)| std::cmp::Reverse(*frames));
    let mut used_predicted = HashSet::new();
    let mut used_reference = HashSet::new();
    let mut mapping = HashMap::new();
    for ((candidate, reference), _) in pairs {
        if used_predicted.insert(candidate) && used_reference.insert(reference) {
            mapping.insert(candidate, reference);
        }
    }

    let mut metrics = Metrics::default();
    for frame in 0..frame_count {
        let time = (frame as f64 + 0.5) * FRAME_SECONDS;
        let reference = reference_at(&case.segments, time);
        let candidate = predicted_at(predicted, time);
        if reference.is_some() {
            metrics.reference_frames += 1;
        }
        match (reference, candidate) {
            (Some(_), None) => metrics.missed_frames += 1,
            (None, Some(_)) => metrics.false_alarm_frames += 1,
            (Some(reference), Some(candidate))
                if mapping.get(candidate).copied() != Some(reference) =>
            {
                metrics.confusion_frames += 1;
            }
            _ => {}
        }
    }

    let predicted_boundaries: Vec<f64> = predicted
        .iter()
        .flat_map(|segment| [segment.start, segment.end])
        .collect();
    for boundary in case
        .segments
        .iter()
        .flat_map(|segment| [segment.start, segment.end])
    {
        if let Some(error) = predicted_boundaries
            .iter()
            .map(|candidate| (candidate - boundary).abs())
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        {
            metrics.boundary_error_sum += error;
            metrics.boundary_count += 1;
        }
    }
    metrics
}

fn reference_at(segments: &[ReferenceSegment], time: f64) -> Option<&str> {
    segments
        .iter()
        .find(|segment| segment.start <= time && time < segment.end)
        .map(|segment| segment.speaker.as_str())
}

fn predicted_at(segments: &[SpeakerSegment], time: f64) -> Option<&str> {
    segments
        .iter()
        .find(|segment| segment.start <= time && time < segment.end)
        .map(|segment| segment.speaker.as_str())
}

fn percent(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 * 100.0 / denominator as f64
    }
}

fn boundary_mae(metrics: &Metrics) -> f64 {
    if metrics.boundary_count == 0 {
        0.0
    } else {
        metrics.boundary_error_sum / metrics.boundary_count as f64
    }
}

fn add_metrics(total: &mut Metrics, case: &Metrics) {
    total.reference_frames += case.reference_frames;
    total.missed_frames += case.missed_frames;
    total.false_alarm_frames += case.false_alarm_frames;
    total.confusion_frames += case.confusion_frames;
    total.boundary_error_sum += case.boundary_error_sum;
    total.boundary_count += case.boundary_count;
}
