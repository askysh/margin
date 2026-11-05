//! One place where backend events leave the process. The app routes them to Tauri; the
//! `--dev serve` QA bridge queues them for a browser. Backend modules never need a Tauri
//! handle just to tell the UI something happened.

use serde::Serialize;
use std::sync::OnceLock;

type Sink = Box<dyn Fn(&str, serde_json::Value) + Send + Sync>;
static SINK: OnceLock<Sink> = OnceLock::new();

/// Pseudo-event asking the host to refresh the menu-bar item.
pub const TRAY_REFRESH: &str = "__tray-refresh";

pub fn set_sink(sink: Sink) {
    let _ = SINK.set(sink);
}

pub fn emit<T: Serialize>(name: &str, payload: T) {
    if let (Some(sink), Ok(value)) = (SINK.get(), serde_json::to_value(payload)) {
        sink(name, value);
    }
}

pub fn refresh_tray() {
    emit(TRAY_REFRESH, ());
}
