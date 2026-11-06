//! One place where backend events leave the process. The app routes them to Tauri; the
//! `--dev serve` QA bridge queues them for a browser. Backend modules never need a Tauri
//! handle just to tell the UI something happened.

use serde::Serialize;
use std::sync::OnceLock;

type Sink = Box<dyn Fn(&str, serde_json::Value) + Send + Sync>;
static SINK: OnceLock<Sink> = OnceLock::new();

/// Pseudo-event asking the host to refresh the menu-bar item.
pub const TRAY_REFRESH: &str = "__tray-refresh";

/// Pseudo-event asking the host to register or drop the global shortcut.
pub const SHORTCUT_REFRESH: &str = "__shortcut-refresh";

/// A system notification for the host to show (the QA bridge only queues it).
pub const NOTIFY: &str = "notify";

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct Notice {
    pub title: String,
    pub body: String,
    /// Opened by a click on the notification.
    pub meeting: Option<String>,
    /// Set for "a call app is active": the notification offers Record.
    pub call_app: Option<String>,
}

pub fn notify(notice: Notice) {
    emit(NOTIFY, notice);
}

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
