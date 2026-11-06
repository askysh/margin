//! Ask: chat with Claude about one meeting or about all of them (DECISIONS.md, "Chat").
//!
//! Each question runs the user's own `claude` CLI headless (`claude -p`), with the meetings
//! folder as its working directory. It gets read-only tools (Read, Grep, Glob) and the
//! "dontAsk" permission mode, so it can read inside that folder and nothing else, and it
//! can never write or run commands. Answers stream to the UI as `chat-delta` events.
//! Follow-up questions resume the same Claude session.
//!
//! One conversation per meeting, plus one across all meetings, is kept as JSON under
//! `state/chat/`. Reset wipes `state/`, so it clears these too.

use crate::{corecfg, events, library, paths, summaries};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// A question that takes longer than this is stopped.
const TURN_LIMIT: Duration = Duration::from_secs(300);
/// The meeting file is put in the first prompt whole, up to this many bytes.
const MEETING_CONTEXT_BYTES: usize = 200_000;
/// The "all meetings" prompt lists at most this many meetings.
const INDEX_LIMIT: usize = 300;
const TOOLS: &str = "Read,Grep,Glob";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Source {
    pub path: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub text: String,
    #[serde(default)]
    pub sources: Vec<Source>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub stopped: bool,
    pub at: String,
}

/// One conversation. `meeting` is None for the conversation across all meetings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thread {
    pub key: String,
    pub meeting: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub messages: Vec<Message>,
    /// The rest is live state, not saved.
    #[serde(skip_deserializing)]
    pub running: bool,
    #[serde(skip_deserializing)]
    pub partial: String,
    #[serde(skip_deserializing)]
    pub activity: Option<String>,
    #[serde(skip_deserializing)]
    pub elapsed_secs: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Readiness {
    pub ready: bool,
    /// "not-installed" | "not-signed-in" | "too-old"
    pub problem: Option<&'static str>,
    pub message: Option<String>,
}

/// Payload of `chat-delta` (the answer so far) and `chat-activity` (what Claude is doing).
#[derive(Debug, Clone, Serialize)]
struct KeyedText<'a> {
    key: &'a str,
    text: &'a str,
}

// ---------------------------------------------------------------- storage

fn chat_dir() -> PathBuf {
    paths::state_dir().join("chat")
}

pub fn key_for(meeting: Option<&str>) -> String {
    match meeting {
        None => "all".into(),
        Some(path) => format!("m-{}", &minutes_core::policy_fs::content_sha256_hex(path.as_bytes())[..20]),
    }
}

fn thread_file(key: &str) -> PathBuf {
    chat_dir().join(format!("{key}.json"))
}

fn empty_thread(meeting: Option<&str>) -> Thread {
    Thread {
        key: key_for(meeting),
        meeting: meeting.map(String::from),
        session_id: None,
        messages: Vec::new(),
        running: false,
        partial: String::new(),
        activity: None,
        elapsed_secs: 0,
    }
}

fn read_saved(meeting: Option<&str>) -> Thread {
    let key = key_for(meeting);
    std::fs::read(thread_file(&key))
        .ok()
        .and_then(|b| serde_json::from_slice::<Thread>(&b).ok())
        .filter(|t| t.key == key)
        .unwrap_or_else(|| empty_thread(meeting))
}

fn save(thread: &Thread) -> Result<(), String> {
    std::fs::create_dir_all(chat_dir()).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec_pretty(thread).map_err(|e| e.to_string())?;
    crate::fsutil::write_atomic(&thread_file(&thread.key), &bytes, Some(0o600)).map_err(|e| format!("Couldn't save the chat: {e}"))
}

/// The saved conversation, plus the answer in progress when Claude is still writing.
pub fn load(meeting: Option<&str>) -> Thread {
    let mut thread = read_saved(meeting);
    if let Some(run) = running().lock().unwrap().get(&thread.key) {
        thread.running = true;
        thread.partial = run.partial.lock().unwrap().clone();
        thread.activity = run.activity.lock().unwrap().clone();
        thread.elapsed_secs = run.started.elapsed().as_secs();
    } else if thread.messages.last().is_some_and(|m| m.role == Role::User) {
        // Margin quit while Claude was answering.
        thread.messages.push(assistant_message(String::new(), vec![], Some("No answer: Margin closed before Claude finished.".into()), false));
    }
    thread
}

/// Stop any answer in progress and forget the conversation.
pub fn clear(meeting: Option<&str>) -> Result<(), String> {
    let key = key_for(meeting);
    stop_key(&key, true);
    match std::fs::remove_file(thread_file(&key)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("Couldn't clear the chat: {e}")),
        _ => Ok(()),
    }
}

/// Reset: stop every answer in progress so none is saved after state/ is wiped.
pub fn forget_all() {
    let keys: Vec<String> = running().lock().unwrap().keys().cloned().collect();
    for key in keys {
        stop_key(&key, true);
    }
    let _ = std::fs::remove_dir_all(chat_dir());
}

fn assistant_message(text: String, sources: Vec<Source>, error: Option<String>, stopped: bool) -> Message {
    Message { role: Role::Assistant, text, sources, error, stopped, at: now() }
}

fn now() -> String {
    chrono::Local::now().to_rfc3339()
}

// ---------------------------------------------------------------- claude CLI

/// The `claude` binary: the one chosen in Settings → Summaries, else the one Margin finds.
pub fn claude_bin() -> Option<String> {
    let configured = corecfg::load().summarization.agent_command;
    let configured = configured.trim();
    if Path::new(configured).is_absolute() && summaries::is_executable(Path::new(configured)) {
        return Some(configured.to_string());
    }
    summaries::find_claude()
}

/// Flags this `claude` understands, from its `--help` (cached per binary).
#[derive(Debug, Clone, Copy, Default)]
struct Features {
    confinable: bool,
    safe_mode: bool,
    no_slash: bool,
    no_chrome: bool,
}

fn features(bin: &str) -> Features {
    static CACHE: OnceLock<Mutex<HashMap<String, Features>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(f) = cache.lock().unwrap().get(bin) {
        return *f;
    }
    let help = run_quick(bin, &["--help"]).map(|o| o.stdout).unwrap_or_default();
    let f = features_from_help(&help);
    if f.confinable {
        cache.lock().unwrap().insert(bin.to_string(), f);
    }
    f
}

fn features_from_help(help: &str) -> Features {
    Features {
        confinable: help.contains("--tools") && help.contains("dontAsk") && help.contains("--resume"),
        safe_mode: help.contains("--safe-mode"),
        no_slash: help.contains("--disable-slash-commands"),
        no_chrome: help.contains("--no-chrome"),
    }
}

struct Quick {
    stdout: String,
}

/// Run a short `claude` subcommand with a 5 s limit.
fn run_quick(bin: &str, args: &[&str]) -> Option<Quick> {
    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    Some(Quick { stdout: reader.join().unwrap_or_default() })
}

/// Some(true/false) from `claude auth status --json`; None when this version can't say.
fn signed_in(bin: &str) -> Option<bool> {
    let out = run_quick(bin, &["auth", "status", "--json"])?;
    let value: serde_json::Value = serde_json::from_str(&out.stdout).ok()?;
    value.get("loggedIn")?.as_bool()
}

pub const NOT_INSTALLED: &str = "Ask needs Claude Code. Install it from claude.com/claude-code, then try again.";
pub const NOT_SIGNED_IN: &str = "Claude isn't signed in. Open Terminal, run `claude`, sign in, then try again.";
pub const TOO_OLD: &str = "Your Claude Code is too old for Ask. Run `claude update` in Terminal, then try again.";

pub fn readiness() -> Readiness {
    let problem = |p, m: &str| Readiness { ready: false, problem: Some(p), message: Some(m.into()) };
    let Some(bin) = claude_bin() else { return problem("not-installed", NOT_INSTALLED) };
    if !features(&bin).confinable {
        return problem("too-old", TOO_OLD);
    }
    match signed_in(&bin) {
        Some(false) => problem("not-signed-in", NOT_SIGNED_IN),
        _ => Readiness { ready: true, problem: None, message: None },
    }
}

/// The command line for one turn. Only read-only tools exist in the session, and
/// "dontAsk" denies anything that would need a permission prompt, which includes reading
/// outside the working directory.
fn claude_args(f: Features, resume: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = ["-p", "--output-format", "stream-json", "--verbose", "--include-partial-messages"]
        .map(String::from)
        .to_vec();
    if f.safe_mode {
        args.push("--safe-mode".into());
    }
    args.extend(["--setting-sources", "", "--strict-mcp-config", "--mcp-config", r#"{"mcpServers":{}}"#].map(String::from));
    // No --allowedTools: a bare "Read" rule would also approve reads outside the folder.
    args.extend(["--tools", TOOLS, "--permission-mode", "dontAsk"].map(String::from));
    args.extend(["--disallowedTools", "Bash,Edit,Write,NotebookEdit,WebFetch,WebSearch,Task"].map(String::from));
    if f.no_slash {
        args.push("--disable-slash-commands".into());
    }
    if f.no_chrome {
        args.push("--no-chrome".into());
    }
    args.extend(["--system-prompt".to_string(), system_prompt()]);
    if let Some(id) = resume {
        args.extend(["--resume".to_string(), id.to_string()]);
    }
    args
}

fn system_prompt() -> String {
    let name = corecfg::load().identity.name.unwrap_or_else(|| "the user".into());
    let today = chrono::Local::now().format("%A %-d %B %Y");
    format!(
        "You are Ask, the assistant inside Margin, a meeting notepad. You talk with {name}. \
Today is {today}.\n\
The working folder holds {name}'s meetings as Markdown files. Each has YAML frontmatter (title, date, attendees), \
then sections such as `## Summary`, `## Notes` ({name}'s own notes, typed during the meeting) and `## Transcript` \
(lines like `[Speaker 1:23] text`). In call recordings, lines labelled You or MICROPHONE are {name}; Them or SYSTEM \
are the other people.\n\
You can only read: use Grep, Glob and Read in this folder. Never ask to run commands or edit files.\n\
Answer from the meetings only. If they don't contain the answer, say so in one sentence. Be brief and concrete: \
short paragraphs or bullet lists, no preamble, no closing offers. Quote people only when it helps. \
When you write a draft (an email, a message), make it ready to paste.\n\
When you use a meeting, cite it once as a Markdown link to its file name, for example \
[Launch sync](2026-11-05-launch-sync.md). Don't cite line numbers."
    )
}

// ---------------------------------------------------------------- prompts

fn relative(path: &str) -> String {
    let root = library::output_dir();
    Path::new(path).strip_prefix(&root).map(|p| p.display().to_string()).unwrap_or_else(|_| path.to_string())
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn meeting_prompt(path: &str, question: &str) -> Result<String, String> {
    let content = std::fs::read_to_string(path).map_err(|e| format!("Couldn't open the meeting: {e}"))?;
    let body = truncate(&content, MEETING_CONTEXT_BYTES);
    let more = if body.len() < content.len() { "\n(The file is longer than this. Read it for the rest.)" } else { "" };
    let file = relative(path);
    Ok(format!(
        "{name} is looking at one meeting, `{file}`. Its whole file is below, so you don't need to read it. \
Read other meetings only if the question needs them.\n\n<meeting file=\"{file}\">\n{body}\n</meeting>{more}\n\nQuestion: {question}",
        name = user_name(),
    ))
}

fn all_prompt(question: &str) -> String {
    let meetings = library::list();
    let index: Vec<String> = meetings
        .iter()
        .take(INDEX_LIMIT)
        .map(|m| format!("- {} · {} · {}", m.date.as_deref().map(|d| truncate(d, 10)).unwrap_or("no date"), m.title, relative(&m.path)))
        .collect();
    let more = if meetings.len() > INDEX_LIMIT { format!("\n(and {} older meetings)", meetings.len() - INDEX_LIMIT) } else { String::new() };
    format!(
        "{name} is asking about all their meetings. These are the meetings in this folder, newest first \
(date · title · file):\n{}{more}\n\nUse Grep to find what you need, then Read the relevant files.\n\nQuestion: {question}",
        index.join("\n"),
        name = user_name(),
    )
}

fn user_name() -> String {
    corecfg::load().identity.name.unwrap_or_else(|| "The user".into())
}

/// Used when the Claude session can't be resumed: start a new one with the recent turns.
fn replay_prompt(thread: &Thread, first: String) -> String {
    let recent: Vec<String> = thread
        .messages
        .iter()
        .rev()
        .skip(1)
        .take(8)
        .rev()
        .filter(|m| !m.text.trim().is_empty())
        .map(|m| format!("{}: {}", if m.role == Role::User { "User" } else { "Assistant" }, truncate(&m.text, 4000)))
        .collect();
    if recent.is_empty() {
        return first;
    }
    format!("[Earlier in this conversation]\n{}\n\n[Now]\n{first}", recent.join("\n"))
}

// ---------------------------------------------------------------- stream-json

/// What a line of `claude --output-format stream-json` means for the UI.
#[derive(Debug, PartialEq)]
enum Update {
    Text,
    Tool { name: String, path: Option<String> },
    Thinking,
}

/// Turn state built up from stream-json lines. Pure, so it can be tested with captured
/// output.
#[derive(Debug, Default)]
struct Stream {
    session_id: Option<String>,
    /// Text of the assistant message being written. A message that ends in a tool call is
    /// preamble ("Let me look"), so a new message replaces it.
    text: String,
    read_paths: Vec<String>,
    /// Set by the final `result` line: Ok(answer) or Err(reason).
    result: Option<Result<String, String>>,
    auth_failed: bool,
    resume_failed: bool,
}

impl Stream {
    fn feed(&mut self, line: &str) -> Vec<Update> {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { return vec![] };
        if let Some(id) = v["session_id"].as_str() {
            self.session_id.get_or_insert_with(|| id.to_string());
        }
        match v["type"].as_str() {
            Some("stream_event") => self.event(&v["event"]),
            Some("assistant") => self.assistant(&v),
            Some("result") => {
                self.result(&v);
                vec![]
            }
            _ => vec![],
        }
    }

    fn event(&mut self, e: &serde_json::Value) -> Vec<Update> {
        match e["type"].as_str() {
            Some("message_start") if !self.text.is_empty() => {
                self.text.clear();
                vec![Update::Text]
            }
            Some("content_block_start") if e["content_block"]["type"] == "thinking" => vec![Update::Thinking],
            Some("content_block_delta") => match e["delta"]["text"].as_str() {
                Some(t) if e["delta"]["type"] == "text_delta" => {
                    self.text.push_str(t);
                    vec![Update::Text]
                }
                _ => vec![],
            },
            _ => vec![],
        }
    }

    fn assistant(&mut self, v: &serde_json::Value) -> Vec<Update> {
        if v["error"] == "authentication_failed" {
            self.auth_failed = true;
        }
        let mut out = Vec::new();
        for block in v["message"]["content"].as_array().into_iter().flatten() {
            match block["type"].as_str() {
                Some("tool_use") => {
                    let name = block["name"].as_str().unwrap_or_default().to_string();
                    let path = block["input"]["file_path"].as_str().map(String::from);
                    if name == "Read" {
                        if let Some(p) = &path {
                            self.read_paths.push(p.clone());
                        }
                    }
                    out.push(Update::Tool { name, path });
                }
                // Without partial messages, text arrives only here.
                Some("text") if self.text.is_empty() => {
                    self.text = block["text"].as_str().unwrap_or_default().to_string();
                    out.push(Update::Text);
                }
                _ => {}
            }
        }
        out
    }

    fn result(&mut self, v: &serde_json::Value) {
        let text = v["result"].as_str().unwrap_or_default().to_string();
        if v["is_error"].as_bool() == Some(false) {
            self.result = Some(Ok(if text.trim().is_empty() { self.text.clone() } else { text }));
            return;
        }
        if v["subtype"] == "error_during_execution" && v["num_turns"].as_u64() == Some(0) {
            self.resume_failed = true;
        }
        if text.contains("Not logged in") || text.contains("/login") {
            self.auth_failed = true;
        }
        let reason = if text.trim().is_empty() { v["subtype"].as_str().unwrap_or("error").replace('_', " ") } else { text };
        self.result = Some(Err(reason));
    }
}

// ---------------------------------------------------------------- running turns

struct Running {
    pgid: Mutex<Option<i32>>,
    cancelled: AtomicBool,
    /// Set by Clear and Reset: don't save this turn when it ends.
    discard: AtomicBool,
    partial: Mutex<String>,
    activity: Mutex<Option<String>>,
    started: Instant,
}

fn running() -> &'static Mutex<HashMap<String, Arc<Running>>> {
    static R: OnceLock<Mutex<HashMap<String, Arc<Running>>>> = OnceLock::new();
    R.get_or_init(Default::default)
}

pub fn stop(meeting: Option<&str>) {
    stop_key(&key_for(meeting), false);
}

fn stop_key(key: &str, discard: bool) {
    let Some(run) = running().lock().unwrap().get(key).cloned() else { return };
    if discard {
        run.discard.store(true, Ordering::SeqCst);
    }
    run.cancelled.store(true, Ordering::SeqCst);
    kill_group(&run);
}

/// SIGTERM the process group, then SIGKILL it if it is still there a moment later.
fn kill_group(run: &Running) {
    let Some(pgid) = *run.pgid.lock().unwrap() else { return };
    unsafe { libc::killpg(pgid, libc::SIGTERM) };
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(800));
        unsafe { libc::killpg(pgid, libc::SIGKILL) };
    });
}

/// Ask a question. Returns at once with the question added; the answer streams as
/// `chat-delta` / `chat-activity` events and ends with `chat-done` (the saved thread).
pub fn send(meeting: Option<&str>, question: &str) -> Result<Thread, String> {
    let question = question.trim();
    if question.is_empty() {
        return Err("Type a question first.".into());
    }
    if let Some(path) = meeting {
        library::ensure_inside_output(Path::new(path))?;
    }
    let ready = readiness();
    if !ready.ready {
        return Err(ready.message.unwrap_or_default());
    }
    let bin = claude_bin().ok_or(NOT_INSTALLED)?;
    let mut thread = read_saved(meeting);
    let run = Arc::new(Running {
        pgid: Mutex::new(None),
        cancelled: AtomicBool::new(false),
        discard: AtomicBool::new(false),
        partial: Mutex::new(String::new()),
        activity: Mutex::new(Some("Thinking…".into())),
        started: Instant::now(),
    });
    {
        let mut map = running().lock().unwrap();
        if map.contains_key(&thread.key) {
            return Err("Claude is still answering. Stop it first.".into());
        }
        map.insert(thread.key.clone(), run.clone());
    }
    thread.messages.push(Message { role: Role::User, text: question.into(), sources: vec![], error: None, stopped: false, at: now() });
    if let Err(e) = save(&thread) {
        running().lock().unwrap().remove(&thread.key);
        return Err(e);
    }
    let first = match meeting {
        Some(path) => meeting_prompt(path, question),
        None => Ok(all_prompt(question)),
    };
    let reply = thread.clone();
    let question = question.to_string();
    std::thread::spawn(move || {
        let outcome = first.and_then(|first| answer(&bin, &thread, &question, first, &run));
        finish(thread, outcome, &run);
    });
    Ok(Thread { running: true, activity: Some("Thinking…".into()), ..reply })
}

struct Outcome {
    stream: Stream,
    timed_out: bool,
    stderr: String,
}

/// Run the turn: resume the session when there is one, and start over with the recent
/// turns if Claude no longer has it.
fn answer(bin: &str, thread: &Thread, question: &str, first: String, run: &Running) -> Result<Outcome, String> {
    let f = features(bin);
    if let Some(id) = &thread.session_id {
        let out = run_claude(bin, &claude_args(f, Some(id)), question, &thread.key, run)?;
        if !(out.stream.resume_failed && out.stream.text.is_empty()) || run.cancelled.load(Ordering::SeqCst) {
            return Ok(out);
        }
        tracing::info!("chat: session {id} not found; starting a new one");
    }
    let prompt = if thread.session_id.is_some() { replay_prompt(thread, first) } else { first };
    run_claude(bin, &claude_args(f, None), &prompt, &thread.key, run)
}

fn run_claude(bin: &str, args: &[String], prompt: &str, key: &str, run: &Running) -> Result<Outcome, String> {
    use std::os::unix::process::CommandExt;
    let mut child = Command::new(bin)
        .args(args)
        .current_dir(library::output_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|e| format!("Couldn't start Claude: {e}"))?;
    *run.pgid.lock().unwrap() = Some(child.id() as i32);
    if run.cancelled.load(Ordering::SeqCst) {
        kill_group(run);
    }
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(prompt.as_bytes());
    }
    let stderr = child.stderr.take().map(|mut e| {
        std::thread::spawn(move || {
            let mut s = String::new();
            let _ = e.read_to_string(&mut s);
            s
        })
    });
    let timed_out = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    watchdog(*run.pgid.lock().unwrap(), done.clone(), timed_out.clone());
    let stream = read_stream(child.stdout.take(), key, run);
    done.store(true, Ordering::SeqCst);
    let _ = child.wait();
    let stderr = stderr.and_then(|h| h.join().ok()).unwrap_or_default();
    Ok(Outcome { stream, timed_out: timed_out.load(Ordering::SeqCst), stderr })
}

fn watchdog(pgid: Option<i32>, done: Arc<AtomicBool>, timed_out: Arc<AtomicBool>) {
    let Some(pgid) = pgid else { return };
    std::thread::spawn(move || {
        let start = Instant::now();
        while !done.load(Ordering::SeqCst) {
            if start.elapsed() > TURN_LIMIT {
                timed_out.store(true, Ordering::SeqCst);
                unsafe { libc::killpg(pgid, libc::SIGKILL) };
                return;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    });
}

fn read_stream(stdout: Option<std::process::ChildStdout>, key: &str, run: &Running) -> Stream {
    let mut stream = Stream::default();
    let Some(stdout) = stdout else { return stream };
    let titles = Titles::default();
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        for update in stream.feed(&line) {
            match update {
                Update::Text => {
                    *run.partial.lock().unwrap() = stream.text.clone();
                    if last_emit.elapsed() >= Duration::from_millis(50) || stream.text.is_empty() {
                        events::emit("chat-delta", KeyedText { key, text: &stream.text });
                        last_emit = Instant::now();
                    }
                }
                Update::Thinking => set_activity(key, run, "Thinking…".into()),
                Update::Tool { name, path } => set_activity(key, run, tool_activity(&name, path.as_deref(), &titles)),
            }
        }
    }
    stream
}

fn set_activity(key: &str, run: &Running, text: String) {
    events::emit("chat-activity", KeyedText { key, text: &text });
    *run.activity.lock().unwrap() = Some(text);
}

fn tool_activity(name: &str, path: Option<&str>, titles: &Titles) -> String {
    match (name, path) {
        ("Read", Some(p)) => match titles.title(p) {
            Some(t) => format!("Reading “{t}”…"),
            None => "Reading a meeting…".into(),
        },
        _ => "Searching your meetings…".into(),
    }
}

/// Meeting titles by path, read once per turn when first needed.
#[derive(Default)]
struct Titles(OnceLock<HashMap<PathBuf, String>>);

impl Titles {
    fn title(&self, path: &str) -> Option<String> {
        let map = self.0.get_or_init(|| {
            library::list().into_iter().map(|m| (canonical(Path::new(&m.path)), m.title)).collect()
        });
        map.get(&canonical(&resolve(path))).cloned()
    }
}

fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

fn resolve(path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        library::output_dir().join(p)
    }
}

/// Save the answer (or the error) and tell the UI the turn is over.
fn finish(mut thread: Thread, outcome: Result<Outcome, String>, run: &Running) {
    let stopped = run.cancelled.load(Ordering::SeqCst);
    let message = match outcome {
        Ok(out) => {
            if let Some(id) = &out.stream.session_id {
                thread.session_id = Some(id.clone());
            }
            reply(out, stopped, thread.meeting.as_deref())
        }
        Err(e) => assistant_message(String::new(), vec![], Some(e), false),
    };
    thread.messages.push(message);
    running().lock().unwrap().remove(&thread.key);
    if run.discard.load(Ordering::SeqCst) {
        return;
    }
    if let Err(e) = save(&thread) {
        tracing::warn!("chat not saved: {e}");
    }
    events::emit("chat-done", &thread);
}

fn reply(out: Outcome, stopped: bool, meeting: Option<&str>) -> Message {
    let Outcome { stream, timed_out, stderr } = out;
    let text = match &stream.result {
        Some(Ok(answer)) => answer.clone(),
        _ => stream.text.clone(),
    };
    let sources = sources(&stream.read_paths, &text, meeting);
    if stopped {
        return assistant_message(stream.text, sources, None, true);
    }
    let error = if timed_out {
        Some("Claude took more than 5 minutes, so Margin stopped it. Try a narrower question.".into())
    } else if stream.auth_failed {
        Some(NOT_SIGNED_IN.into())
    } else {
        match &stream.result {
            Some(Ok(_)) => None,
            Some(Err(reason)) => Some(format!("Claude couldn't answer: {reason}")),
            None => Some(format!("Claude stopped without answering.{}", last_line(&stderr))),
        }
    };
    assistant_message(text, sources, error, false)
}

fn last_line(stderr: &str) -> String {
    stderr.lines().rev().find(|l| !l.trim().is_empty()).map(|l| format!(" ({})", truncate(l.trim(), 200))).unwrap_or_default()
}

/// The meetings an answer used: files Claude read, files the answer links to, and the
/// meeting the chat is about. Only real meeting files in the folder count.
fn sources(read_paths: &[String], answer: &str, meeting: Option<&str>) -> Vec<Source> {
    let mut wanted: Vec<PathBuf> = meeting.map(PathBuf::from).into_iter().collect();
    wanted.extend(read_paths.iter().map(|p| resolve(p)));
    wanted.extend(cited_files(answer).iter().map(|p| resolve(p)));
    let library: Vec<(PathBuf, library::MeetingSummary)> =
        library::list().into_iter().map(|m| (canonical(Path::new(&m.path)), m)).collect();
    let mut out: Vec<Source> = Vec::new();
    for want in wanted {
        let want = canonical(&want);
        if let Some((_, m)) = library.iter().find(|(p, _)| *p == want) {
            if !out.iter().any(|s| s.path == m.path) {
                out.push(Source { path: m.path.clone(), title: m.title.clone() });
            }
        }
    }
    out
}

/// `.md` file names an answer mentions, as link targets or in backticks.
fn cited_files(answer: &str) -> Vec<String> {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"(?:\]\(|`)([^)`\s]+?\.md)(?::\d+)?[)`]").unwrap());
    re.captures_iter(answer).map(|c| c[1].replace("%20", " ")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL: &str = r#"{"type":"system","subtype":"init","cwd":"/m","session_id":"s-1","tools":["Glob","Grep","Read"]}
{"type":"stream_event","event":{"type":"message_start","message":{"role":"assistant"}}}
{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}}
{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Let me look."}}}
{"type":"assistant","message":{"content":[{"type":"text","text":"Let me look."},{"type":"tool_use","name":"Read","input":{"file_path":"/m/2026-11-05-launch.md"}}]}}
{"type":"user","message":{"content":[{"type":"tool_result","content":"..."}]}}
{"type":"stream_event","event":{"type":"message_start","message":{"role":"assistant"}}}
{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}}
{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"You'll build "}}}
{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"the settings screen."}}}
{"type":"result","subtype":"success","is_error":false,"num_turns":2,"result":"You'll build the settings screen.","session_id":"s-1"}"#;

    fn feed_all(s: &str) -> (Stream, Vec<Update>) {
        let mut stream = Stream::default();
        let updates = s.lines().flat_map(|l| stream.feed(l)).collect();
        (stream, updates)
    }

    #[test]
    fn stream_keeps_only_the_final_message_text() {
        let (s, updates) = feed_all(REAL);
        assert_eq!(s.session_id.as_deref(), Some("s-1"));
        assert_eq!(s.text, "You'll build the settings screen.");
        assert_eq!(s.result, Some(Ok("You'll build the settings screen.".into())));
        assert_eq!(s.read_paths, vec!["/m/2026-11-05-launch.md"]);
        assert!(updates.contains(&Update::Thinking));
        assert!(updates.contains(&Update::Tool { name: "Read".into(), path: Some("/m/2026-11-05-launch.md".into()) }));
    }

    #[test]
    fn stream_detects_sign_in_and_lost_sessions() {
        let (s, _) = feed_all(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Not logged in · Please run /login"}]},"error":"authentication_failed"}
{"type":"result","subtype":"success","is_error":true,"result":"Not logged in · Please run /login"}"#,
        );
        assert!(s.auth_failed);
        assert!(matches!(s.result, Some(Err(_))));
        let (s, _) = feed_all(r#"{"type":"result","subtype":"error_during_execution","is_error":true,"num_turns":0,"session_id":"x"}"#);
        assert!(s.resume_failed);
        assert_eq!(s.result, Some(Err("error during execution".into())));
    }

    #[test]
    fn args_are_read_only_and_confined() {
        let f = features_from_help("--tools  --resume  dontAsk --safe-mode --disable-slash-commands");
        assert!(f.confinable && f.safe_mode && f.no_slash && !f.no_chrome);
        let args = claude_args(f, Some("abc"));
        let after = |flag: &str| args[args.iter().position(|a| a == flag).unwrap() + 1].clone();
        assert_eq!(after("--tools"), "Read,Grep,Glob");
        assert_eq!(after("--permission-mode"), "dontAsk");
        assert_eq!(after("--setting-sources"), "");
        assert_eq!(after("--resume"), "abc");
        assert!(args.contains(&"--safe-mode".to_string()) && args.contains(&"--strict-mcp-config".to_string()));
        assert!(!features_from_help("--resume --allowedTools").confinable);
    }

    #[test]
    fn finds_cited_files() {
        let a = "See [Launch](2026-11-05-launch.md) and `memos/idea.md:12`, not foo.md.";
        assert_eq!(cited_files(a), vec!["2026-11-05-launch.md", "memos/idea.md"]);
    }

    #[test]
    fn keys_are_stable_and_distinct() {
        assert_eq!(key_for(None), "all");
        assert_eq!(key_for(Some("/a.md")), key_for(Some("/a.md")));
        assert_ne!(key_for(Some("/a.md")), key_for(Some("/b.md")));
        assert!(key_for(Some("/a.md")).starts_with("m-"));
    }
}
