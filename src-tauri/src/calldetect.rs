//! Nice-to-have: suggest recording when a call app (Zoom, Teams, …) starts. It uses core's
//! process-name check, which can't tell "Zoom is open" from "Zoom is in a call". So this
//! only offers a dismissible suggestion, once each time a call app appears.

use crate::{corecfg, prefs, recorder::Recorder};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("margin-call-detect".into())
        .spawn(move || {
            let mut last: Option<String> = None;
            loop {
                std::thread::sleep(Duration::from_secs(15));
                if !prefs::load().onboarding_complete || app.state::<Recorder>().is_recording() {
                    last = None;
                    continue;
                }
                let now = minutes_core::capture::detect_active_call_app(&corecfg::load());
                if now.is_some() && now != last {
                    let _ = app.emit("call-detected", now.clone());
                }
                last = now;
            }
        })
        .expect("spawn call detector");
}
