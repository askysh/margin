//! Nice-to-have: suggest recording when a call app (Zoom, Teams, …) starts. It uses core's
//! process-name check, which can't tell "Zoom is open" from "Zoom is in a call". So this
//! only offers a dismissible suggestion, once each time a call app appears.

use crate::{corecfg, events, prefs, recorder::recorder};
use std::time::Duration;

pub fn spawn() {
    std::thread::Builder::new()
        .name("margin-call-detect".into())
        .spawn(move || {
            let mut last: Option<String> = None;
            loop {
                std::thread::sleep(Duration::from_secs(15));
                if !prefs::load().onboarding_complete || recorder().is_recording() {
                    last = None;
                    continue;
                }
                let cfg = corecfg::load();
                // Settings → Recording → "Suggest recording when a call starts".
                if !cfg.call_detection.enabled {
                    last = None;
                    continue;
                }
                let now = minutes_core::capture::detect_active_call_app(&cfg);
                if let Some(app) = now.as_ref().filter(|_| now != last) {
                    events::emit("call-detected", Some(app));
                    // Shown only when the window isn't in front, where the banner is.
                    events::notify(events::Notice {
                        title: format!("{app} looks active"),
                        body: "Record this call in Margin?".into(),
                        meeting: None,
                        call_app: Some(app.clone()),
                    });
                }
                last = now;
            }
        })
        .expect("spawn call detector");
}
