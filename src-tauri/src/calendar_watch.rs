//! Offers to start notes when a calendar event begins, like the call banner but from the
//! calendar. Core's `calendar::upcoming_events` reads Calendar.app through EventKit, so a
//! Google calendar added in System Settings → Internet Accounts works too. The offer is an
//! in-app banner and, when the window is not in front, a notification with Start notes.
//! Start notes opens the event's call link and records the call under the event's title.
//! Nothing records without that click. Off unless Settings → Recording turns it on (D67).

use crate::{corecfg, events, prefs, recorder::recorder};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::Duration;

/// An event worth offering, as the UI and the notification see it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Offer {
    pub title: String,
    pub start: String,
    pub url: Option<String>,
    pub attendees: Vec<String>,
}

const CHECK_EVERY: Duration = Duration::from_secs(30);
/// An event is offered from this many minutes before its start...
const LEAD_MINUTES: i64 = 1;
/// ...until this many minutes after it, for a call joined a little late.
const LATE_MINUTES: i64 = 5;

/// True for an event that starts soon or just started and looks like a meeting: it has a
/// call link or at least two attendees. A lone reminder or focus block is not offered.
pub fn qualifies(e: &minutes_core::calendar::CalendarEvent) -> bool {
    let timely = (-LATE_MINUTES..=LEAD_MINUTES).contains(&e.minutes_until);
    let meeting_like = e.url.as_deref().is_some_and(|u| !u.trim().is_empty()) || e.attendees.len() >= 2;
    timely && meeting_like
}

/// The key that keeps an event from being offered twice.
fn key(e: &minutes_core::calendar::CalendarEvent) -> String {
    format!("{}|{}", e.start, e.title)
}

/// The events to offer now, each at most once across calls.
pub fn new_offers(events: &[minutes_core::calendar::CalendarEvent], offered: &mut HashSet<String>) -> Vec<Offer> {
    events
        .iter()
        .filter(|e| qualifies(e))
        .filter(|e| offered.insert(key(e)))
        .map(|e| Offer { title: e.title.clone(), start: e.start.clone(), url: e.url.clone(), attendees: e.attendees.clone() })
        .collect()
}

pub fn spawn() {
    std::thread::Builder::new()
        .name("margin-calendar-watch".into())
        .spawn(|| {
            let mut offered: HashSet<String> = HashSet::new();
            loop {
                std::thread::sleep(CHECK_EVERY);
                if !prefs::load().onboarding_complete || recorder().is_recording() || !corecfg::load().calendar.enabled {
                    continue;
                }
                // Look a little ahead; `qualifies` picks the window.
                let events = minutes_core::calendar::upcoming_events(LEAD_MINUTES as u32 + 1);
                for offer in new_offers(&events, &mut offered) {
                    events::emit("calendar-upcoming", &offer);
                    events::notify(events::Notice {
                        title: offer.title.clone(),
                        body: "Starting now. Start notes in Margin?".into(),
                        meeting: None,
                        call_app: None,
                        calendar: Some(offer),
                    });
                }
                if offered.len() > 500 {
                    offered.clear();
                }
            }
        })
        .expect("spawn calendar watch");
}

/// Start notes for an offer: open its call link, then record the call under its title.
pub fn start_notes(offer: &Offer) -> Result<(), String> {
    if let Some(url) = offer.url.as_deref().filter(|u| u.starts_with("https://")) {
        if let Err(error) = open_url(url) {
            tracing::warn!(%error, "could not open the event's call link");
        }
    }
    crate::commands::ops::start_recording(crate::recorder::Kind::Call, Some(offer.title.clone())).map(|_| ())
}

#[cfg(target_os = "macos")]
fn open_url(url: &str) -> std::io::Result<()> {
    std::process::Command::new("/usr/bin/open").arg(url).status().map(|_| ())
}

#[cfg(not(target_os = "macos"))]
fn open_url(url: &str) -> std::io::Result<()> {
    std::process::Command::new("cmd").args(["/C", "start", "", url]).status().map(|_| ())
}

/// Ask for Calendars access now, in the foreground, right after the switch is turned on, so
/// the prompt has a reason the user just gave (D9 kept calendar off because the prompt came
/// in the middle of background processing). Returns false when access is refused.
pub fn request_access() -> bool {
    // The QA bridge never raises a macOS prompt (D11), so there it counts as refused.
    if crate::devserve::RUNNING.load(std::sync::atomic::Ordering::SeqCst) {
        return false;
    }
    let _ = minutes_core::calendar::upcoming_events(60);
    // Write-only access reads nothing, so it counts as refused.
    matches!(minutes_core::calendar::calendar_access_status(), minutes_core::calendar::CalendarAccess::FullAccess)
}

#[cfg(test)]
mod tests {
    use super::*;
    use minutes_core::calendar::CalendarEvent;

    fn event(title: &str, minutes_until: i64, url: Option<&str>, attendees: usize) -> CalendarEvent {
        serde_json::from_value(serde_json::json!({
            "title": title,
            "start": "2026-11-27 10:00",
            "minutes_until": minutes_until,
            "attendees": (0..attendees).map(|i| format!("p{i}@example.com")).collect::<Vec<_>>(),
            "url": url,
        }))
        .unwrap()
    }

    #[test]
    fn only_timely_meeting_like_events_qualify() {
        assert!(qualifies(&event("Standup", 1, Some("https://meet.google.com/abc-defg-hij"), 0)));
        assert!(qualifies(&event("Standup", 0, None, 2)));
        assert!(qualifies(&event("Late join", -5, Some("https://meet.google.com/x"), 0)));
        assert!(!qualifies(&event("Too early", 2, Some("https://meet.google.com/x"), 3)));
        assert!(!qualifies(&event("Too late", -6, Some("https://meet.google.com/x"), 3)));
        assert!(!qualifies(&event("Focus block", 0, None, 1)));
        assert!(!qualifies(&event("Blank link", 0, Some("  "), 0)));
    }

    #[test]
    fn an_event_is_offered_once() {
        let mut offered = HashSet::new();
        let events = vec![event("Standup", 1, Some("https://meet.google.com/abc"), 3), event("Focus", 0, None, 0)];
        let first = new_offers(&events, &mut offered);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].title, "Standup");
        assert_eq!(first[0].url.as_deref(), Some("https://meet.google.com/abc"));
        assert!(new_offers(&events, &mut offered).is_empty(), "the next check offers nothing new");
    }
}
