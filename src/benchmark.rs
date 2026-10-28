//! Headless ABBA measurements. No signed URLs, cookies or response bodies leave
//! this module; no GUI state, session files or account settings are changed.
use crate::{
    native_service::Native,
    options::Options,
    player::{Control, EventKind, Player},
    service::{Live, Request, Service},
};
use serde_json::{Value, json};
use std::{
    sync::mpsc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub fn run(options: &Options) -> Result<(), String> {
    let id = options.benchmark.as_ref().ok_or("Missing benchmark ID")?;
    let native = Native::new()?;
    if !options.ignore_credentials && !Live.call(Request::Status, false)?.connected {
        return Err("Connect your account in Uta first, or use --ignore-credentials.".into());
    }
    let mut samples = Vec::new();
    for round in 0..options.rounds {
        for (slot, arm) in ["A", "B", "B", "A"].into_iter().enumerate() {
            let python = !options.compare_auth && arm == "B";
            let authenticated =
                !options.ignore_credentials && (!options.compare_auth || arm == "A");
            let backend = if python { "python" } else { "rustypipe" };
            let started = Instant::now();
            let request = Request::Resolve { id: id.clone() };
            let result = if python {
                Live.call(request, authenticated)
            } else {
                native.call_native_mode(&request, authenticated)
            };
            let resolve_ms = started.elapsed().as_secs_f64() * 1000.0;
            let mut row = json!({"round":round, "slot":slot, "arm":arm, "backend":backend,
                "authenticated":authenticated, "resolve_ms":resolve_ms, "ok":false});
            if let Ok(reply) = result
                && let Some(stream) = reply.stream
            {
                row["codec"] = json!(stream.codec);
                row["bitrate_kbps"] = json!(stream.bitrate);
                row["format_id"] = json!(stream.format_id);
                row["client"] = json!(stream.client);
                let (tx, rx) = mpsc::channel();
                let player = Player::start(
                    move |event| {
                        let _ = tx.send(event);
                    },
                    true,
                );
                player.send(Control::Load {
                    generation: 1,
                    stream,
                    volume: 0.0,
                    paused: false,
                });
                let deadline = Instant::now() + Duration::from_secs(20);
                while Instant::now() < deadline {
                    if let Ok(event) = rx.recv_timeout(Duration::from_millis(500)) {
                        match event.kind {
                            EventKind::Position(p) if p > 0.0 => {
                                row["start_ms"] = json!(started.elapsed().as_secs_f64() * 1000.0);
                                row["ok"] = json!(true);
                                break;
                            }
                            EventKind::Failed(_) | EventKind::Ended => break,
                            _ => {}
                        }
                    }
                }
                player.send(Control::Stop);
            }
            eprintln!(
                "Round {} {arm}: {backend}, authenticated={authenticated}, {resolve_ms:.0} ms resolve, played={}",
                round + 1,
                row["ok"]
            );
            samples.push(row);
        }
    }
    let passed = samples.iter().all(|s| s["ok"] == Value::Bool(true));
    println!("{}", serde_json::to_string_pretty(&json!({
        "schema":1, "order":"ABBA", "video_id":id,
        "utc_unix":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
        "comparison":if options.compare_auth { "native authenticated vs native anonymous" } else { "native vs python, same authentication" },
        "note":"Strict native calls, no fallback. Retained clients; first samples include cold initialization. Fresh mpv per sample with null audio output. start_ms measures resolve to first positive mpv position, not audible hardware output. Authentication comparisons can return different quality; group results by codec/format.",
        "samples":samples
    })).map_err(|_| "Could not encode benchmark results")?);
    if passed {
        Ok(())
    } else {
        Err("Some samples failed; failures are retained in the JSON report.".into())
    }
}
