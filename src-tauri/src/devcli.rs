//! `margin --dev <cmd>`: headless checks of the core integration without the UI. Use
//! with `MARGIN_HOME=/some/tmp/dir` to stay away from your real data.
//!
//!   margin --dev models            download missing models, printing progress
//!   margin --dev process <wav>     queue a WAV as a meeting and process it in-process
//!   margin --dev list              list meetings as JSON
//!   margin --dev rename <md> <label> <name> [--no-voice]
//!   margin --dev permissions       print permission state

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
    if !corecfg::exists() {
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
        Some("permissions") => {
            println!("{}", serde_json::to_string_pretty(&permissions::state()).unwrap());
            Ok(())
        }
        _ => Err("commands: models | process <wav> [title] | list | read <md> | rename <md> <label> <name> | permissions".into()),
    }
}
