//! `margin --dev <cmd>`: headless checks of the core integration without the UI. Use
//! with `MARGIN_HOME=/some/tmp/dir` to stay away from your real data.
//!
//!   margin --dev models            download missing models, printing progress
//!   margin --dev process <wav>     queue a WAV as a meeting and process it in-process
//!   margin --dev list              list meetings as JSON
//!   margin --dev rename <md> <label> <name> [--no-voice]
//!   margin --dev permissions       print permission state
//!   margin --dev ask [--meeting <md>] <question> [<follow-up> ...]   chat with Claude

use crate::{corecfg, library, models, permissions, speakers};
use std::path::PathBuf;

pub fn run(args: &[String]) -> i32 {
    match run_inner(args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn run_inner(args: &[String]) -> Result<(), String> {
    let mut cfg = corecfg::load();
    // `serve` must look like a first launch to the UI, so only the other commands get a
    // config written up front.
    if !corecfg::exists() && args.first().map(String::as_str) != Some("serve") {
        corecfg::save(&mut cfg)?;
    }
    match args.first().map(String::as_str) {
        Some("models") => {
            for f in models::required(&cfg).into_iter().filter(|f| !f.installed) {
                eprintln!("downloading {} from {}", f.label, f.url);
                models::fetch(&f, &mut |p| {
                    if p.finished {
                        eprintln!("  done: {} bytes", p.downloaded);
                    }
                })?;
            }
            println!("{}", serde_json::to_string_pretty(&models::required(&cfg)).unwrap());
            Ok(())
        }
        Some("process") => {
            let wav = PathBuf::from(args.get(1).ok_or("usage: process <wav>")?);
            cfg.ensure_dirs().map_err(|e| e.to_string())?;
            // Queue a copy, as the recorder does, then drain the queue in this process.
            let copy = minutes_core::config::Config::minutes_dir().join("dev-input.wav");
            std::fs::copy(&wav, &copy).map_err(|e| e.to_string())?;
            let job = minutes_core::jobs::queue_live_capture_with_recording_health(
                minutes_core::pid::CaptureMode::Meeting,
                args.get(2).cloned(),
                &copy,
                Some("dev harness note".into()),
                None,
                Some(chrono::Local::now()),
                Some(chrono::Local::now()),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .map_err(|e| e.to_string())?;
            eprintln!("queued job {}", job.id);
            let t = std::time::Instant::now();
            minutes_core::jobs::process_pending_jobs(&cfg, |j| eprintln!("  {:?} {:?}", j.state, j.stage))
                .map_err(|e| e.to_string())?;
            let done = minutes_core::jobs::load_job(&job.id);
            eprintln!("finished in {:.1}s", t.elapsed().as_secs_f32());
            println!("{}", serde_json::to_string_pretty(&done.map(|j| (j.state, j.output_path, j.error))).unwrap());
            Ok(())
        }
        Some("queue") => {
            // What Stop does after capture: queue the audio with the notes draft and start
            // the worker child. A running app (or QA bridge) then finishes the job.
            let wav = PathBuf::from(args.get(1).ok_or("usage: queue <wav> [title] [notes-file]")?);
            cfg.ensure_dirs().map_err(|e| e.to_string())?;
            let current = minutes_core::pid::current_wav_path();
            std::fs::copy(&wav, &current).map_err(|e| e.to_string())?;
            let notes = match args.get(3) {
                Some(f) => std::fs::read_to_string(f).map_err(|e| e.to_string())?,
                None => String::new(),
            };
            let summary_notes: Vec<&str> =
                notes.lines().map(|l| l.trim().trim_start_matches(['-', '*']).trim()).filter(|l| !l.is_empty()).collect();
            let job = minutes_core::jobs::queue_live_capture_with_recording_health(
                minutes_core::pid::CaptureMode::Meeting,
                args.get(2).cloned(),
                &current,
                (!summary_notes.is_empty()).then(|| summary_notes.join("\n")),
                None,
                Some(chrono::Local::now() - chrono::Duration::seconds(40)),
                Some(chrono::Local::now()),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .map_err(|e| e.to_string())?;
            crate::processing::save_draft(&job.id, &notes);
            crate::processing::spawn_worker();
            println!("{}", job.id);
            Ok(())
        }
        Some("summarize") => {
            let md = args.get(1).ok_or("usage: summarize <md> [claude|ollama]")?.clone();
            if let Some(engine) = args.get(2) {
                let mut s = corecfg::settings(&cfg);
                s.summary_engine = engine.clone();
                corecfg::apply(&mut cfg, &s)?;
                corecfg::save(&mut cfg)?;
            }
            let t = std::time::Instant::now();
            crate::commands::ops::summarize_meeting(md)?;
            eprintln!("summarized in {:.1}s", t.elapsed().as_secs_f32());
            Ok(())
        }
        Some("list") => {
            println!("{}", serde_json::to_string_pretty(&library::list()).unwrap());
            Ok(())
        }
        Some("read") => {
            let md = PathBuf::from(args.get(1).ok_or("usage: read <md>")?);
            println!("{}", serde_json::to_string_pretty(&library::read(&md)?).unwrap());
            Ok(())
        }
        Some("rename") => {
            let [_, md, label, name, ..] = args else { return Err("usage: rename <md> <label> <name>".into()) };
            let remember = !args.iter().any(|a| a == "--no-voice");
            let r = speakers::rename(&PathBuf::from(md), label, name, remember)?;
            println!("{}", serde_json::to_string_pretty(&r).unwrap());
            Ok(())
        }
        Some("reset") => {
            println!("{}", serde_json::to_string_pretty(&crate::reset::preview()).unwrap());
            let opts = crate::reset::ResetOptions {
                delete_meetings: args.iter().any(|a| a == "--meetings"),
                delete_models: args.iter().any(|a| a == "--models"),
                forget_voices: args.iter().any(|a| a == "--voices"),
            };
            let report = crate::reset::run(crate::recorder::recorder(), &opts)?;
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
            Ok(())
        }
        Some("serve") => {
            let port = args.get(1).and_then(|p| p.parse().ok()).unwrap_or(4545);
            crate::devserve::serve(port)
        }
        Some("preflight") => {
            // The device and capture-plan checks Start runs, without recording.
            let kind = if args.get(1).map(String::as_str) == Some("call") {
                crate::recorder::Kind::Call
            } else {
                crate::recorder::Kind::InPerson
            };
            let cfg = crate::recorder::session_config(kind);
            let intent = match kind {
                crate::recorder::Kind::Call => minutes_core::capture::RecordingIntent::Call,
                crate::recorder::Kind::InPerson => minutes_core::capture::RecordingIntent::Room,
            };
            let p = minutes_core::capture::preflight_recording(minutes_core::pid::CaptureMode::Meeting, Some(intent), false, &cfg)?;
            println!("{}", serde_json::to_string_pretty(&p).unwrap());
            Ok(())
        }
        Some("ask") => ask(&args[1..]),
        Some("permissions") => {
            println!("{}", serde_json::to_string_pretty(&permissions::state()).unwrap());
            Ok(())
        }
        _ => Err("commands: models | process <wav> [title] | list | read <md> | rename <md> <label> <name> | permissions".into()),
    }
}

/// `ask [--meeting <md>] <question> [<follow-up> ...]`: what the Ask panel does, with the
/// answer streamed to stderr and timings printed. Each extra argument is a follow-up.
fn ask(args: &[String]) -> Result<(), String> {
    let (meeting, questions) = match args {
        [flag, md, rest @ ..] if flag == "--meeting" => (Some(md.clone()), rest),
        rest => (None, rest),
    };
    if questions.is_empty() {
        return Err("usage: ask [--meeting <md>] <question> [<follow-up> ...]".into());
    }
    // (characters printed, time of first text, number of text updates)
    let shown = std::sync::Arc::new(std::sync::Mutex::new((0usize, None::<std::time::Instant>, 0usize)));
    let sink = shown.clone();
    crate::events::set_sink(Box::new(move |name, payload| {
        let text = payload["text"].as_str().unwrap_or_default();
        let mut s = sink.lock().unwrap();
        match name {
            "chat-delta" => {
                s.2 += 1;
                if text.len() < s.0 {
                    eprintln!("\n[preamble cleared]");
                    s.0 = 0;
                }
                eprint!("{}", &text[s.0..]);
                s.0 = text.len();
                if !text.is_empty() {
                    s.1.get_or_insert_with(std::time::Instant::now);
                }
            }
            "chat-activity" => eprintln!("[{text}]"),
            _ => {}
        }
    }));
    eprintln!("{}", serde_json::to_string(&crate::chat::readiness()).unwrap());
    for q in questions {
        *shown.lock().unwrap() = (0, None, 0);
        eprintln!("\n> {q}");
        let start = std::time::Instant::now();
        crate::chat::send(meeting.as_deref(), q)?;
        while crate::chat::load(meeting.as_deref()).running {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let thread = crate::chat::load(meeting.as_deref());
        let last = thread.messages.last().ok_or("no answer")?;
        let (_, first, updates) = *shown.lock().unwrap();
        let first_text = first.map(|t| (t - start).as_secs_f32());
        eprintln!("\n[first text after {:.1}s, {updates} text updates, done after {:.1}s]", first_text.unwrap_or(-1.0), start.elapsed().as_secs_f32());
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "question": q, "answer": last.text, "sources": last.sources, "error": last.error, "session": thread.session_id,
        })).unwrap());
    }
    Ok(())
}
