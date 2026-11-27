//! Live remote-silence detection for call recordings (#1266).
//!
//! A call recording that captures the local microphone but never receives the
//! remote side (for example a FaceTime Audio call, or an iPhone cellular call
//! answered on the Mac through Continuity, whose audio ScreenCaptureKit does
//! not deliver) used to produce no signal until post-processing, long
//! after the call was over and the remote side was unrecoverable. This module
//! is the pure, time-injected state machine that lets a capture loop notice
//! the problem within about a minute.
//!
//! The detector is fed periodic per-source level samples (0..=100, the same
//! scale the native call helper reports). It does not depend on call
//! detection having identified an app: the caller only runs it when the
//! recording actually has a system/call-audio stem.
//!
//! ```text
//!            silent system + mic speech for `never_heard_window`
//!   Armed ──────────────────────────────────────────────────────▶ Alerted
//!     ▲     (or `dropout_window` once remote audio was heard)        │
//!     │                                                              │
//!     └──────── remote audio audible again for `rearm_audible` ──────┘
//!                     (emits Recovered; next alert waits for
//!                      `renotify_cooldown` after the previous one)
//! ```
//!
//! Design notes:
//! - Time only accumulates across *observed* samples. A gap in telemetry (a
//!   stale helper, a suspended laptop) neither counts as silence nor resets
//!   the window, and each step is clamped to `max_step`.
//! - Short system-audio blips (a notification sound) do not count as hearing
//!   the remote side; that takes `remote_heard_audible` of audible system
//!   audio inside one window.
//! - Mic speech is required so a recording that is waiting for a call to
//!   connect, or a quiet room, never alarms.

use crate::config::Config;
use crate::markdown::RemoteAudioStatus;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Tunables for [`RemoteSilenceDetector`]. [`Default`] holds the shipped values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteSilenceThresholds {
    /// System level at or above which call audio counts as audible. The native
    /// helper reports `min(100, rms * 2000)`, so 2 is roughly -60 dBFS;
    /// digital silence reads 0.
    pub system_audible_level: u32,
    /// Mic level at or above which the local side counts as speaking (8 is
    /// roughly -48 dBFS, above typical room noise).
    pub mic_speech_level: u32,
    /// Silent-system window before alerting when remote audio has never been
    /// heard in this recording.
    pub never_heard_window: Duration,
    /// Silent-system window before alerting after remote audio was heard at
    /// least once. Longer, because a remote listener can be quiet while the
    /// local person presents.
    pub dropout_window: Duration,
    /// Minimum mic speech inside the silent window.
    pub min_mic_speech: Duration,
    /// Audible system audio inside one window that counts as hearing the
    /// remote side (resets the window).
    pub remote_heard_audible: Duration,
    /// Audible system audio after an alert that re-arms the detector.
    pub rearm_audible: Duration,
    /// Minimum time between two alerts in one recording.
    pub renotify_cooldown: Duration,
    /// Largest step credited for one observation.
    pub max_step: Duration,
}

impl Default for RemoteSilenceThresholds {
    fn default() -> Self {
        Self {
            system_audible_level: 2,
            mic_speech_level: 8,
            never_heard_window: Duration::from_secs(60),
            dropout_window: Duration::from_secs(120),
            min_mic_speech: Duration::from_secs(15),
            remote_heard_audible: Duration::from_secs(3),
            rearm_audible: Duration::from_secs(5),
            renotify_cooldown: Duration::from_secs(10 * 60),
            max_step: Duration::from_secs(1),
        }
    }
}

/// One live level observation from a recording with a system stem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelSample {
    /// Local microphone level, 0..=100.
    pub mic_level: u32,
    /// System/call-audio level, 0..=100. Use 0 when the system source is
    /// attached but delivering no buffers.
    pub system_level: u32,
}

/// Why an alert fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteSilenceAlertKind {
    /// Remote audio has not been heard at all in this recording.
    NeverHeard,
    /// Remote audio was heard earlier, then went silent.
    Lost,
}

impl RemoteSilenceAlertKind {
    /// Stable label for logs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NeverHeard => "never-heard",
            Self::Lost => "lost",
        }
    }
}

/// Details of a fired alert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteSilenceAlert {
    pub kind: RemoteSilenceAlertKind,
    /// Observed silent-system time in the window that fired.
    pub silent_for: Duration,
    /// Mic speech inside that window.
    pub mic_speech: Duration,
}

/// State transition reported by [`RemoteSilenceDetector::observe`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteSilenceEvent {
    /// Show the user a warning now.
    Alert(RemoteSilenceAlert),
    /// Remote audio is reaching the recording again after an alert.
    Recovered,
}

/// User-facing title for the alert. No em dashes (user-facing copy rule).
pub const ALERT_TITLE: &str = "Minutes isn't hearing the other side of this call";

/// User-facing alert body for `kind`.
pub fn alert_body(kind: RemoteSilenceAlertKind) -> &'static str {
    match kind {
        RemoteSilenceAlertKind::NeverHeard => {
            "No call audio has reached Minutes for over a minute while you were speaking, so only your microphone is being recorded. FaceTime and phone calls answered on this Mac may not be capturable. Check your output device, or switch to Zoom, Meet, or Teams."
        }
        RemoteSilenceAlertKind::Lost => {
            "Call audio stopped reaching Minutes for two minutes while you were speaking. If the other person is talking, their side is not being recorded. Check your output device."
        }
    }
}

/// How the native call helper is capturing the other side (#1266). The
/// right advice for a silent call depends on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallAudioCapture {
    /// The Core Audio tap, which hears FaceTime and phone calls answered on
    /// the Mac. `heard_audio` is false when it has delivered only digital
    /// silence, the signature of a refused System Audio Recording permission.
    Tap {
        heard_audio: bool,
    },
    /// ScreenCaptureKit, which cannot hear FaceTime or phone calls.
    ScreenCaptureKit,
    Unknown,
}

/// User-facing alert body for `kind`, given how call audio is captured.
pub fn alert_body_for(kind: RemoteSilenceAlertKind, capture: CallAudioCapture) -> &'static str {
    match (kind, capture) {
        (RemoteSilenceAlertKind::NeverHeard, CallAudioCapture::Tap { heard_audio: false }) => {
            "No call audio has reached Minutes for over a minute while you were speaking, so only your microphone is being recorded. Check that Minutes is allowed under System Settings > Privacy & Security > Screen & System Audio Recording, and that the call is playing on this Mac."
        }
        (RemoteSilenceAlertKind::NeverHeard, CallAudioCapture::Tap { heard_audio: true }) => {
            "No call audio has reached Minutes for over a minute while you were speaking, so only your microphone is being recorded. Check that the call is playing on this Mac and not on your phone or another device."
        }
        _ => alert_body(kind),
    }
}

/// User-facing title when remote audio comes back after an alert.
pub const RECOVERED_TITLE: &str = "Minutes is hearing the other side again";

/// User-facing body when remote audio comes back after an alert.
pub const RECOVERED_BODY: &str =
    "Call audio is reaching Minutes again. The silent stretch may be missing from the transcript.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Armed,
    Alerted,
}

/// Silence-window state machine. See the module docs.
#[derive(Debug, Clone)]
pub struct RemoteSilenceDetector {
    thresholds: RemoteSilenceThresholds,
    phase: Phase,
    last_observed_at: Option<Instant>,
    window_observed: Duration,
    window_mic_speech: Duration,
    window_system_audible: Duration,
    rearm_system_audible: Duration,
    remote_heard: bool,
    alerts_fired: u32,
    last_alert_at: Option<Instant>,
}

impl Default for RemoteSilenceDetector {
    fn default() -> Self {
        Self::new(RemoteSilenceThresholds::default())
    }
}

impl RemoteSilenceDetector {
    pub fn new(thresholds: RemoteSilenceThresholds) -> Self {
        Self {
            thresholds,
            phase: Phase::Armed,
            last_observed_at: None,
            window_observed: Duration::ZERO,
            window_mic_speech: Duration::ZERO,
            window_system_audible: Duration::ZERO,
            rearm_system_audible: Duration::ZERO,
            remote_heard: false,
            alerts_fired: 0,
            last_alert_at: None,
        }
    }

    /// Feed one observation. `sample` is `None` when per-source telemetry is
    /// unavailable or stale; that time is not credited to any window.
    pub fn observe(
        &mut self,
        now: Instant,
        sample: Option<LevelSample>,
    ) -> Option<RemoteSilenceEvent> {
        let previous = self.last_observed_at.replace(now);
        let sample = sample?;
        let Some(previous) = previous else {
            // The first sample only establishes the clock.
            return None;
        };
        let step = now
            .saturating_duration_since(previous)
            .min(self.thresholds.max_step);
        if step.is_zero() {
            return None;
        }

        let system_audible = sample.system_level >= self.thresholds.system_audible_level;
        let mic_speaking = sample.mic_level >= self.thresholds.mic_speech_level;

        if system_audible {
            self.window_system_audible += step;
            if self.phase == Phase::Alerted {
                self.rearm_system_audible += step;
            }
            if self.window_system_audible >= self.thresholds.remote_heard_audible {
                self.remote_heard = true;
                self.reset_window();
            }
            if self.phase == Phase::Alerted
                && self.rearm_system_audible >= self.thresholds.rearm_audible
            {
                self.phase = Phase::Armed;
                self.rearm_system_audible = Duration::ZERO;
                self.reset_window();
                return Some(RemoteSilenceEvent::Recovered);
            }
            return None;
        }

        self.window_observed += step;
        if mic_speaking {
            self.window_mic_speech += step;
        }

        if self.phase != Phase::Armed {
            return None;
        }
        let window = if self.remote_heard {
            self.thresholds.dropout_window
        } else {
            self.thresholds.never_heard_window
        };
        if self.window_observed < window || self.window_mic_speech < self.thresholds.min_mic_speech
        {
            return None;
        }
        if self
            .last_alert_at
            .is_some_and(|at| now.saturating_duration_since(at) < self.thresholds.renotify_cooldown)
        {
            return None;
        }

        let alert = RemoteSilenceAlert {
            kind: if self.remote_heard {
                RemoteSilenceAlertKind::Lost
            } else {
                RemoteSilenceAlertKind::NeverHeard
            },
            silent_for: self.window_observed,
            mic_speech: self.window_mic_speech,
        };
        self.phase = Phase::Alerted;
        self.rearm_system_audible = Duration::ZERO;
        self.alerts_fired += 1;
        self.last_alert_at = Some(now);
        Some(RemoteSilenceEvent::Alert(alert))
    }

    /// True while an alert is outstanding (fired and not yet recovered).
    pub fn is_alerting(&self) -> bool {
        self.phase == Phase::Alerted
    }

    /// Number of alerts fired in this recording.
    pub fn alerts_fired(&self) -> u32 {
        self.alerts_fired
    }

    /// Whether sustained remote audio was heard at any point.
    pub fn remote_heard(&self) -> bool {
        self.remote_heard
    }

    /// Durable outcome for frontmatter once the recording ends. `None` when no
    /// alert fired (no positive evidence of loss).
    pub fn outcome(&self) -> Option<RemoteAudioStatus> {
        if self.alerts_fired == 0 {
            None
        } else if self.remote_heard {
            Some(RemoteAudioStatus::Partial)
        } else {
            Some(RemoteAudioStatus::Missing)
        }
    }

    fn reset_window(&mut self) {
        self.window_observed = Duration::ZERO;
        self.window_mic_speech = Duration::ZERO;
        self.window_system_audible = Duration::ZERO;
    }
}

// ── Live surface (#1266) ─────────────────────────────────────────────────
//
// The detector runs inside the capturing process. Agents read the live
// transcript through other processes (`minutes transcript --status`, MCP), so
// the outstanding alert is published as a small state file bound to the
// recording PID, plus a `live.remote_audio` event in `events.jsonl`.

/// Outstanding live remote-silence state, persisted for other processes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveRemoteAudioState {
    /// Always `missing` while an alert is outstanding: the other side is not
    /// reaching the recording right now.
    pub remote_audio: RemoteAudioStatus,
    /// `never-heard` or `lost`.
    pub kind: String,
    /// PID of the process that owns the recording. A state file whose PID is
    /// not the active recording is ignored, so a crash cannot leave a stale
    /// warning on the next recording.
    pub recording_pid: u32,
    pub since: chrono::DateTime<chrono::Local>,
}

/// `~/.minutes/live-remote-audio.json`.
pub fn live_state_path() -> PathBuf {
    Config::minutes_dir().join("live-remote-audio.json")
}

/// Publish (`Some`) or clear (`None`) the live remote-audio state. Best effort:
/// a failure here must never affect capture.
pub fn write_live_state(state: Option<&LiveRemoteAudioState>) {
    let path = live_state_path();
    let Some(state) = state else {
        std::fs::remove_file(&path).ok();
        return;
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let tmp = path.with_extension("json.tmp");
    if let Ok(json) = serde_json::to_string(state) {
        if std::fs::write(&tmp, json).is_ok() {
            std::fs::rename(&tmp, &path).ok();
        }
    }
}

/// Read the published state if it belongs to `recording_pid`.
pub fn read_live_state(recording_pid: Option<u32>) -> Option<LiveRemoteAudioState> {
    let recording_pid = recording_pid?;
    let content = std::fs::read_to_string(live_state_path()).ok()?;
    let state: LiveRemoteAudioState = serde_json::from_str(&content).ok()?;
    (state.recording_pid == recording_pid).then_some(state)
}

/// Build the event that mirrors a detector transition into `events.jsonl`.
pub fn live_event(event: &RemoteSilenceEvent) -> crate::events::MinutesEvent {
    live_event_for(event, CallAudioCapture::Unknown)
}

/// [`live_event`] with the advice matched to how call audio is captured.
pub fn live_event_for(
    event: &RemoteSilenceEvent,
    capture: CallAudioCapture,
) -> crate::events::MinutesEvent {
    match event {
        RemoteSilenceEvent::Alert(alert) => crate::events::MinutesEvent::LiveRemoteAudio {
            remote_audio: "missing".into(),
            kind: Some(alert.kind.as_str().into()),
            message: alert_body_for(alert.kind, capture).into(),
        },
        RemoteSilenceEvent::Recovered => crate::events::MinutesEvent::LiveRemoteAudio {
            remote_audio: "restored".into(),
            kind: None,
            message: RECOVERED_BODY.into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TICK: Duration = Duration::from_millis(100);
    const SPEAKING: u32 = 40;
    const REMOTE: u32 = 35;

    /// Drives a detector at 10 Hz and collects every event with its offset.
    struct Harness {
        detector: RemoteSilenceDetector,
        start: Instant,
        elapsed: Duration,
        events: Vec<(Duration, RemoteSilenceEvent)>,
    }

    impl Harness {
        fn new() -> Self {
            let start = Instant::now();
            let mut detector = RemoteSilenceDetector::default();
            assert_eq!(detector.observe(start, Some(sample(0, 0))), None);
            Self {
                detector,
                start,
                elapsed: Duration::ZERO,
                events: Vec::new(),
            }
        }

        /// Run for `duration` with a per-tick sample chosen by `f(tick_index)`.
        fn run(&mut self, duration: Duration, mut f: impl FnMut(u64) -> Option<LevelSample>) {
            let ticks = duration.as_millis() / TICK.as_millis();
            for i in 0..ticks as u64 {
                self.elapsed += TICK;
                if let Some(event) = self.detector.observe(self.start + self.elapsed, f(i)) {
                    self.events.push((self.elapsed, event));
                }
            }
        }

        fn alerts(&self) -> Vec<RemoteSilenceAlert> {
            self.events
                .iter()
                .filter_map(|(_, event)| match event {
                    RemoteSilenceEvent::Alert(alert) => Some(*alert),
                    RemoteSilenceEvent::Recovered => None,
                })
                .collect()
        }
    }

    fn sample(mic_level: u32, system_level: u32) -> LevelSample {
        LevelSample {
            mic_level,
            system_level,
        }
    }

    fn secs(value: u64) -> Duration {
        Duration::from_secs(value)
    }

    /// Mic speaking in 4 s bursts with 2 s pauses (about two thirds talk time).
    fn talking(i: u64) -> u32 {
        if i % 60 < 40 {
            SPEAKING
        } else {
            0
        }
    }

    /// Remote speaking in alternation with the mic (a normal conversation).
    fn conversation(i: u64) -> LevelSample {
        if i % 80 < 40 {
            sample(SPEAKING, 0)
        } else {
            sample(0, REMOTE)
        }
    }

    #[test]
    fn silent_system_with_active_mic_for_sixty_seconds_fires_once() {
        let mut harness = Harness::new();
        harness.run(secs(59), |i| Some(sample(talking(i), 0)));
        assert!(harness.alerts().is_empty(), "must not fire before 60 s");

        harness.run(secs(2), |i| Some(sample(talking(i), 0)));
        let alerts = harness.alerts();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, RemoteSilenceAlertKind::NeverHeard);
        assert!(alerts[0].silent_for >= secs(60));
        assert!(alerts[0].mic_speech >= secs(15));
        assert!(harness.detector.is_alerting());

        // Keeps silent for the rest of a long call: still exactly one alert.
        harness.run(secs(60 * 90), |i| Some(sample(talking(i), 0)));
        assert_eq!(harness.alerts().len(), 1);
        assert_eq!(harness.detector.outcome(), Some(RemoteAudioStatus::Missing));
    }

    #[test]
    fn faint_noise_below_floor_still_counts_as_silent() {
        let mut harness = Harness::new();
        harness.run(secs(61), |i| Some(sample(talking(i), 1)));
        assert_eq!(harness.alerts().len(), 1);
    }

    #[test]
    fn system_attached_but_delivering_no_buffers_counts_as_silent() {
        // The caller reports level 0 when the system source is not live but
        // the mic is; that is still "not hearing the other side".
        let mut harness = Harness::new();
        harness.run(secs(61), |i| Some(sample(talking(i), 0)));
        assert_eq!(harness.alerts().len(), 1);
    }

    #[test]
    fn normal_two_sided_call_never_fires() {
        let mut harness = Harness::new();
        harness.run(secs(60 * 60), |i| Some(conversation(i)));
        assert!(harness.events.is_empty());
        assert!(harness.detector.remote_heard());
        assert_eq!(harness.detector.outcome(), None);
    }

    #[test]
    fn long_monologue_after_remote_was_heard_needs_the_longer_dropout_window() {
        let mut harness = Harness::new();
        harness.run(secs(30), |i| Some(conversation(i)));
        // A 90 s presentation while the remote listens silently.
        harness.run(secs(90), |i| Some(sample(talking(i), 0)));
        harness.run(secs(30), |i| Some(conversation(i)));
        assert!(harness.alerts().is_empty(), "{:?}", harness.events);
    }

    #[test]
    fn remote_dropout_mid_call_fires_lost_after_dropout_window() {
        let mut harness = Harness::new();
        harness.run(secs(120), |i| Some(conversation(i)));
        harness.run(secs(119), |i| Some(sample(talking(i), 0)));
        assert!(harness.alerts().is_empty());
        harness.run(secs(2), |i| Some(sample(talking(i), 0)));
        let alerts = harness.alerts();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, RemoteSilenceAlertKind::Lost);
        assert_eq!(harness.detector.outcome(), Some(RemoteAudioStatus::Partial));
    }

    #[test]
    fn no_system_stem_means_no_samples_and_never_fires() {
        // Mic-only and in-person recordings never construct samples (there is
        // no system stem), so the detector only ever sees `None`.
        let mut harness = Harness::new();
        harness.run(secs(60 * 30), |_| None);
        assert!(harness.events.is_empty());
        assert_eq!(harness.detector.outcome(), None);
    }

    #[test]
    fn brief_system_silence_does_not_fire() {
        let mut harness = Harness::new();
        // 40 s silent stretches separated by 10 s of remote speech.
        for _ in 0..20 {
            harness.run(secs(40), |i| Some(sample(talking(i), 0)));
            harness.run(secs(10), |i| {
                Some(sample(0, if i % 4 == 0 { 0 } else { REMOTE }))
            });
        }
        assert!(harness.events.is_empty(), "{:?}", harness.events);
    }

    #[test]
    fn quiet_room_waiting_for_a_call_does_not_fire() {
        let mut harness = Harness::new();
        harness.run(secs(5 * 60), |_| Some(sample(3, 0)));
        assert!(harness.events.is_empty());
    }

    #[test]
    fn needs_sustained_mic_speech_inside_the_window() {
        let mut harness = Harness::new();
        // 10 s of speech then silence: 60 s passes with too little speech.
        harness.run(secs(10), |_| Some(sample(SPEAKING, 0)));
        harness.run(secs(60), |_| Some(sample(0, 0)));
        assert!(harness.alerts().is_empty());
        // Speech resumes; the window keeps growing and fires once 15 s is met.
        harness.run(secs(6), |_| Some(sample(SPEAKING, 0)));
        assert_eq!(harness.alerts().len(), 1);
    }

    #[test]
    fn notification_blips_do_not_count_as_hearing_the_remote() {
        let mut harness = Harness::new();
        // A 0.5 s system sound every 20 s while the user talks.
        harness.run(secs(64), |i| {
            let system = if i % 200 < 5 { REMOTE } else { 0 };
            Some(sample(talking(i), system))
        });
        let alerts = harness.alerts();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, RemoteSilenceAlertKind::NeverHeard);
        assert!(!harness.detector.remote_heard());
    }

    #[test]
    fn stale_telemetry_gaps_are_not_credited_as_silence() {
        let mut harness = Harness::new();
        harness.run(secs(30), |i| Some(sample(talking(i), 0)));
        // Ten minutes of missing telemetry.
        harness.run(secs(600), |_| None);
        assert!(harness.alerts().is_empty());
        harness.run(secs(31), |i| Some(sample(talking(i), 0)));
        assert_eq!(harness.alerts().len(), 1);
    }

    #[test]
    fn large_clock_jumps_are_clamped() {
        let start = Instant::now();
        let mut detector = RemoteSilenceDetector::default();
        detector.observe(start, Some(sample(SPEAKING, 0)));
        // One observation after a 10 minute sleep credits at most `max_step`.
        assert_eq!(
            detector.observe(start + secs(600), Some(sample(SPEAKING, 0))),
            None
        );
        assert!(!detector.is_alerting());
    }

    #[test]
    fn recovers_when_remote_audio_returns_and_rearms_with_cooldown() {
        let mut harness = Harness::new();
        harness.run(secs(61), |i| Some(sample(talking(i), 0)));
        assert_eq!(harness.alerts().len(), 1);

        // Two seconds of remote audio is not enough to re-arm.
        harness.run(secs(2), |_| Some(sample(0, REMOTE)));
        assert!(harness.detector.is_alerting());

        // Sustained remote audio re-arms.
        harness.run(secs(4), |_| Some(sample(0, REMOTE)));
        assert!(!harness.detector.is_alerting());
        assert!(matches!(
            harness.events.last(),
            Some((_, RemoteSilenceEvent::Recovered))
        ));

        // Drops again right away: the dropout window passes but the cooldown
        // since the first alert has not, so no second notification yet.
        harness.run(secs(150), |i| Some(sample(talking(i), 0)));
        assert_eq!(harness.alerts().len(), 1);

        // After the cooldown it can fire again.
        harness.run(secs(10 * 60), |i| Some(sample(talking(i), 0)));
        let alerts = harness.alerts();
        assert_eq!(alerts.len(), 2);
        assert_eq!(alerts[1].kind, RemoteSilenceAlertKind::Lost);
        assert_eq!(harness.detector.outcome(), Some(RemoteAudioStatus::Partial));
    }

    #[test]
    fn live_state_is_bound_to_the_recording_pid() {
        let _guard = crate::test_support::home_env_lock();
        let dir = tempfile::TempDir::new().unwrap();
        let original_home = std::env::var_os("HOME");
        std::env::set_var("HOME", dir.path());

        assert_eq!(read_live_state(Some(42)), None);
        let state = LiveRemoteAudioState {
            remote_audio: RemoteAudioStatus::Missing,
            kind: "never-heard".into(),
            recording_pid: 42,
            since: chrono::Local::now(),
        };
        write_live_state(Some(&state));
        assert_eq!(read_live_state(Some(42)), Some(state.clone()));
        assert_eq!(read_live_state(Some(43)), None, "stale owner ignored");
        assert_eq!(read_live_state(None), None, "no recording, no state");
        write_live_state(None);
        assert_eq!(read_live_state(Some(42)), None);

        match original_home {
            Some(home) => std::env::set_var("HOME", home),
            None => std::env::remove_var("HOME"),
        }
    }

    #[test]
    fn live_event_serializes_with_a_stable_event_type() {
        let alert = RemoteSilenceEvent::Alert(RemoteSilenceAlert {
            kind: RemoteSilenceAlertKind::NeverHeard,
            silent_for: secs(60),
            mic_speech: secs(20),
        });
        let value = serde_json::to_value(live_event(&alert)).unwrap();
        assert_eq!(value["event_type"], "live.remote_audio");
        assert_eq!(value["remote_audio"], "missing");
        assert_eq!(value["kind"], "never-heard");

        let value = serde_json::to_value(live_event(&RemoteSilenceEvent::Recovered)).unwrap();
        assert_eq!(value["remote_audio"], "restored");
        assert!(value.get("kind").is_none());
    }

    #[test]
    fn user_facing_copy_has_no_em_dashes() {
        for text in [
            ALERT_TITLE,
            alert_body(RemoteSilenceAlertKind::NeverHeard),
            alert_body(RemoteSilenceAlertKind::Lost),
            alert_body_for(
                RemoteSilenceAlertKind::NeverHeard,
                CallAudioCapture::Tap { heard_audio: false },
            ),
            alert_body_for(
                RemoteSilenceAlertKind::NeverHeard,
                CallAudioCapture::Tap { heard_audio: true },
            ),
            RECOVERED_TITLE,
            RECOVERED_BODY,
        ] {
            assert!(!text.contains('\u{2014}'), "{text}");
            assert!(!text.contains('\u{2013}'), "{text}");
        }
    }

    #[test]
    fn alert_advice_follows_the_capture_path() {
        // ScreenCaptureKit cannot hear FaceTime: say so.
        let sck = alert_body_for(
            RemoteSilenceAlertKind::NeverHeard,
            CallAudioCapture::ScreenCaptureKit,
        );
        assert!(sck.contains("FaceTime"));
        // The tap can, so blaming FaceTime would be wrong. A tap that never
        // heard anything points at the permission instead.
        let refused = alert_body_for(
            RemoteSilenceAlertKind::NeverHeard,
            CallAudioCapture::Tap { heard_audio: false },
        );
        assert!(refused.contains("Screen & System Audio Recording"));
        assert!(!refused.contains("FaceTime"));
        let heard = alert_body_for(
            RemoteSilenceAlertKind::NeverHeard,
            CallAudioCapture::Tap { heard_audio: true },
        );
        assert!(!heard.contains("FaceTime") && !heard.contains("Privacy"));
        assert_eq!(
            alert_body_for(
                RemoteSilenceAlertKind::Lost,
                CallAudioCapture::Tap { heard_audio: true }
            ),
            alert_body(RemoteSilenceAlertKind::Lost)
        );
    }
}
