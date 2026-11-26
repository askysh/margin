//! A local forwarder that asks Ollama to answer without thinking first.
//!
//! Models that think first, such as qwen3, spend most of core's 2-minute summary limit on
//! hidden reasoning, so the summary fails. Ollama turns this off with `"think": false` in
//! the request body, but core's Ollama calls do not send it, and an Ollama Modelfile cannot
//! set it. So for the length of one summary call Margin points core's `ollama_url` at this
//! forwarder. It adds `"think": false` to the JSON body of `/api/generate` and `/api/chat`
//! and passes every other request through unchanged. UPSTREAM-PATCHES.md asks core for an
//! `ollama_think` key; this module goes away when core has one.

use std::sync::Arc;
use std::time::Duration;

/// Paths whose JSON body gets `"think": false`.
const THINKING_PATHS: &[&str] = &["/api/generate", "/api/chat"];

/// Core times out its own call after 2 minutes. The forwarder waits longer, so that core's
/// limit is the one that applies and its error message stays the same.
const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(600);

/// A running forwarder. It stops when dropped.
pub struct Forwarder {
    url: String,
    server: Arc<tiny_http::Server>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Forwarder {
    /// The base URL to give core in place of the real Ollama URL.
    pub fn url(&self) -> &str {
        &self.url
    }
}

impl Drop for Forwarder {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Point `cfg` at a forwarder when it summarizes with Ollama and "Answer without thinking
/// first" is on. Keep the returned guard alive for the whole call; the forwarder stops when
/// it drops. With the switch off, another engine, or no free port, `cfg` stays as it is.
pub fn fast_ollama(cfg: &mut minutes_core::config::Config) -> Option<Forwarder> {
    if cfg.summarization.engine != "ollama" || crate::prefs::load().ollama_thinking {
        return None;
    }
    match start(&cfg.summarization.ollama_url) {
        Ok(forwarder) => {
            cfg.summarization.ollama_url = forwarder.url().to_string();
            Some(forwarder)
        }
        Err(error) => {
            tracing::warn!(%error, "Ollama forwarder not started; the model may think first");
            None
        }
    }
}

/// The request body with `"think": false` added, for a thinking path and a JSON object
/// body that does not set `think` itself. Any other body comes back unchanged.
pub fn rewrite(path: &str, body: &[u8]) -> Vec<u8> {
    let path = path.split('?').next().unwrap_or(path);
    if !THINKING_PATHS.contains(&path) {
        return body.to_vec();
    }
    match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(serde_json::Value::Object(mut map)) if !map.contains_key("think") => {
            map.insert("think".into(), serde_json::Value::Bool(false));
            serde_json::to_vec(&map).unwrap_or_else(|_| body.to_vec())
        }
        _ => body.to_vec(),
    }
}

/// Start a forwarder to `upstream` (an Ollama base URL such as `http://localhost:11434`) on
/// a free local port.
pub fn start(upstream: &str) -> Result<Forwarder, String> {
    let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").map_err(|e| format!("Could not start the Ollama forwarder: {e}"))?);
    let port = server.server_addr().to_ip().map(|a| a.port()).ok_or("The Ollama forwarder has no port")?;
    let upstream = upstream.trim_end_matches('/').to_string();
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(UPSTREAM_TIMEOUT))
        .http_status_as_error(false)
        .build()
        .new_agent();
    let listener = Arc::clone(&server);
    let thread = std::thread::Builder::new()
        .name("margin-ollama-forwarder".into())
        .spawn(move || {
            for request in listener.incoming_requests() {
                let agent = agent.clone();
                let upstream = upstream.clone();
                // One thread per request: core sends one chunk at a time, and a slow answer
                // must not block a second caller.
                std::thread::spawn(move || forward(&agent, &upstream, request));
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(Forwarder { url: format!("http://127.0.0.1:{port}"), server, thread: Some(thread) })
}

fn forward(agent: &ureq::Agent, upstream: &str, mut request: tiny_http::Request) {
    let path = request.url().to_string();
    let mut body = Vec::new();
    if let Err(error) = request.as_reader().read_to_end(&mut body) {
        let _ = request.respond(tiny_http::Response::from_string(error.to_string()).with_status_code(400));
        return;
    }
    let url = format!("{upstream}{path}");
    let result = match request.method() {
        tiny_http::Method::Get => agent.get(&url).call(),
        _ => agent.post(&url).header("Content-Type", "application/json").send(&rewrite(&path, &body)[..]),
    };
    let response = match result {
        Ok(mut answer) => {
            let status = answer.status().as_u16();
            let bytes = answer.body_mut().with_config().limit(256 * 1024 * 1024).read_to_vec().unwrap_or_default();
            tiny_http::Response::from_data(bytes).with_status_code(status)
        }
        Err(error) => tiny_http::Response::from_string(format!("{{\"error\":{}}}", serde_json::Value::String(error.to_string()))).with_status_code(502),
    };
    let header = tiny_http::Header::from_bytes("Content-Type", "application/json").expect("static header");
    let _ = request.respond(response.with_header(header));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrite_adds_think_false_only_where_it_belongs() {
        let generate = rewrite("/api/generate", br#"{"model":"qwen3.5:4b","prompt":"hi","stream":false}"#);
        let v: serde_json::Value = serde_json::from_slice(&generate).unwrap();
        assert_eq!(v["think"], false);
        assert_eq!(v["model"], "qwen3.5:4b");
        let chat: serde_json::Value = serde_json::from_slice(&rewrite("/api/chat?x=1", br#"{"model":"m","messages":[]}"#)).unwrap();
        assert_eq!(chat["think"], false);
        assert_eq!(rewrite("/api/tags", b"{}"), b"{}".to_vec());
        assert_eq!(rewrite("/api/embed", br#"{"model":"e"}"#), br#"{"model":"e"}"#.to_vec());
        let kept: serde_json::Value = serde_json::from_slice(&rewrite("/api/generate", br#"{"think":true}"#)).unwrap();
        assert_eq!(kept["think"], true, "a caller's own choice stays");
        assert_eq!(rewrite("/api/generate", b"not json"), b"not json".to_vec());
        assert_eq!(rewrite("/api/generate", b"[1,2]"), b"[1,2]".to_vec());
    }

    /// A stand-in for Ollama that answers with the body it received.
    fn echo_server() -> (String, std::thread::JoinHandle<()>, Arc<tiny_http::Server>) {
        let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
        let port = server.server_addr().to_ip().unwrap().port();
        let s = Arc::clone(&server);
        let handle = std::thread::spawn(move || {
            for mut request in s.incoming_requests() {
                let mut body = String::new();
                request.as_reader().read_to_string(&mut body).unwrap();
                let reply = if request.url() == "/api/tags" { r#"{"models":[{"name":"qwen3.5:4b"}]}"#.to_string() } else { body };
                let status = if request.url() == "/missing" { 404 } else { 200 };
                request.respond(tiny_http::Response::from_string(reply).with_status_code(status)).unwrap();
            }
        });
        (format!("http://127.0.0.1:{port}"), handle, server)
    }

    #[test]
    fn forwarder_injects_think_and_passes_the_rest_through() {
        let (upstream, handle, server) = echo_server();
        let fwd = start(&upstream).unwrap();
        let agent = ureq::Agent::config_builder().http_status_as_error(false).build().new_agent();
        let echoed: serde_json::Value = agent
            .post(&format!("{}/api/generate", fwd.url()))
            .send(&br#"{"model":"qwen3.5:4b","prompt":"hi"}"#[..])
            .unwrap()
            .body_mut()
            .read_json()
            .unwrap();
        assert_eq!(echoed["think"], false);
        assert_eq!(echoed["prompt"], "hi");
        let tags: serde_json::Value = agent.get(&format!("{}/api/tags", fwd.url())).call().unwrap().body_mut().read_json().unwrap();
        assert_eq!(tags["models"][0]["name"], "qwen3.5:4b");
        let missing = agent.post(&format!("{}/missing", fwd.url())).send(&b"{}"[..]).unwrap();
        assert_eq!(missing.status().as_u16(), 404, "the upstream status reaches core");
        drop(fwd);
        server.unblock();
        handle.join().unwrap();
    }

    #[test]
    fn an_unreachable_upstream_is_a_502_not_a_hang() {
        let closed = {
            let s = tiny_http::Server::http("127.0.0.1:0").unwrap();
            format!("http://127.0.0.1:{}", s.server_addr().to_ip().unwrap().port())
        };
        let fwd = start(&closed).unwrap();
        let agent = ureq::Agent::config_builder().http_status_as_error(false).build().new_agent();
        let answer = agent.post(&format!("{}/api/generate", fwd.url())).send(&b"{}"[..]).unwrap();
        assert_eq!(answer.status().as_u16(), 502);
    }
}
