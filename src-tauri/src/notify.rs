//! System notifications: "meeting ready", "processing failed" and "a call app is active".
//! They are shown only when the window isn't in front, since the window already shows the
//! same news. A click opens the meeting; the call notification offers Record call.
//! Backend modules send them as `events::NOTIFY`, so the QA bridge can see them without
//! showing anything.

use crate::events::Notice;
use crate::{events, recorder::Kind, tray};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::{Action, ActionPerformed, ActionType, NotificationExt};

const CALL_ACTIONS: &str = "margin-call";
const RECORD: &str = "record";

static NEXT_ID: AtomicI32 = AtomicI32::new(1);
static SHOWN: Mutex<Option<HashMap<i32, Notice>>> = Mutex::new(None);

pub fn init(app: &AppHandle) {
    let n = app.notification();
    let _ = n.register_action_types(vec![ActionType::builder(CALL_ACTIONS)
        .actions(vec![Action::builder(RECORD, "Record call").foreground(true).build()])
        .build()]);
    let handle = app.clone();
    let _ = n.on_action(move |performed| clicked(&handle, performed));
}

pub fn show(app: &AppHandle, notice: Notice) {
    let in_front = app
        .get_webview_window("main")
        .is_some_and(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false));
    if in_front {
        return;
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let mut builder = app.notification().builder().id(id).title(&notice.title).body(&notice.body);
    if notice.call_app.is_some() {
        builder = builder.action_type_id(CALL_ACTIONS);
    }
    SHOWN.lock().unwrap().get_or_insert_with(HashMap::new).insert(id, notice);
    if let Err(error) = builder.show() {
        tracing::warn!(%error, "notification not shown");
    }
}

fn clicked(app: &AppHandle, performed: &ActionPerformed) {
    let notice = performed
        .notification()
        .and_then(|n| SHOWN.lock().unwrap().as_mut()?.remove(&n.id()));
    match notice {
        Some(Notice { call_app: Some(_), .. }) if performed.action_id() == RECORD => tray::start_outside(app, Kind::Call),
        Some(Notice { meeting: Some(path), .. }) => {
            tray::show_main(app);
            events::emit("open-meeting", path);
        }
        _ => tray::show_main(app),
    }
}
