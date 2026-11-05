//! `margin --dev serve [port]`: a localhost-only HTTP bridge that exposes the real backend
//! to the real UI running in a browser, for end-to-end tests (scripts/e2e/). Commands go
//! through the same `commands::dispatch` the app uses. Events are queued and long-polled.
//! Permission prompts are refused, so tests can never raise a macOS dialog.
//!
//!   POST /invoke        {"cmd": "...", "args": {...}} → {"ok": true, "value": ...} | {"ok": false, "error": "..."}
//!   GET  /events?since=N                              → [{"seq": n, "event": "...", "payload": ...}]

use crate::{calldetect, commands, events, processing};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

struct Queue {
    items: Mutex<Vec<(u64, String, serde_json::Value)>>,
    ready: Condvar,
}

fn queue() -> &'static Queue {
    static Q: OnceLock<Queue> = OnceLock::new();
    Q.get_or_init(|| Queue { items: Mutex::new(Vec::new()), ready: Condvar::new() })
}

pub fn serve(port: u16) -> Result<(), String> {
    events::set_sink(Box::new(|name, payload| {
        if name == events::TRAY_REFRESH {
            return;
        }
        let q = queue();
        let mut items = q.items.lock().unwrap();
        let seq = items.last().map(|i| i.0 + 1).unwrap_or(1);
        items.push((seq, name.to_string(), payload));
        q.ready.notify_all();
    }));
    processing::spawn_watcher();
    calldetect::spawn();

    let server = tiny_http::Server::http(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    eprintln!("margin QA bridge on http://127.0.0.1:{port}");
    for request in server.incoming_requests() {
        std::thread::spawn(move || handle(request));
    }
    Ok(())
}

fn handle(mut request: tiny_http::Request) {
    let cors = [
        tiny_http::Header::from_bytes("Access-Control-Allow-Origin", "*").unwrap(),
        tiny_http::Header::from_bytes("Access-Control-Allow-Headers", "content-type").unwrap(),
        tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap(),
    ];
    let respond = |request: tiny_http::Request, body: String| {
        let mut response = tiny_http::Response::from_string(body);
        for h in &cors {
            response.add_header(h.clone());
        }
        let _ = request.respond(response);
    };
    if request.method() == &tiny_http::Method::Options {
        return respond(request, String::new());
    }
    let url = request.url().to_string();
    if url.starts_with("/invoke") {
        let mut body = String::new();
        let _ = request.as_reader().read_to_string(&mut body);
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
        let cmd = parsed["cmd"].as_str().unwrap_or_default().to_string();
        let args = parsed.get("args").cloned().unwrap_or(serde_json::json!({}));
        let result = match commands::dispatch(&cmd, args) {
            Ok(value) => serde_json::json!({ "ok": true, "value": value }),
            Err(error) => serde_json::json!({ "ok": false, "error": error }),
        };
        return respond(request, result.to_string());
    }
    if url.starts_with("/events") {
        let since: u64 = url.split("since=").nth(1).and_then(|v| v.parse().ok()).unwrap_or(0);
        let q = queue();
        let items = q.items.lock().unwrap();
        let (items, _) = q
            .ready
            .wait_timeout_while(items, Duration::from_millis(1000), |items| items.last().is_none_or(|i| i.0 <= since))
            .unwrap();
        let out: Vec<_> = items
            .iter()
            .filter(|i| i.0 > since)
            .map(|(seq, event, payload)| serde_json::json!({ "seq": seq, "event": event, "payload": payload }))
            .collect();
        drop(items);
        return respond(request, serde_json::Value::Array(out).to_string());
    }
    respond(request, "{}".into());
}
