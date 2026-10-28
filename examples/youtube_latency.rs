//! Interleaved anonymous service/playback benchmark; stdout contains only
//! sanitized JSON. Run in nix develop, with --features rustypipe.
use aizome_uta::{
    model::Stream,
    native_service::Native,
    player::{Control, EventKind, Player},
    service::{Live, Request, Service},
};
use serde_json::{Value, json};
use std::{
    io::Read,
    sync::mpsc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

fn playback(stream: &Stream, full_track: bool) -> Value {
    let (tx, rx) = mpsc::channel();
    let start = Instant::now();
    let player = Player::start(
        move |event| {
            let _ = tx.send(event);
        },
        true,
    );
    player.send(Control::Load {
        generation: 1,
        stream: stream.clone(),
        volume: 0.0,
        paused: false,
    });
    let mut first = None;
    let mut seek_ok = false;
    let mut ended = false;
    let mut last_position = 0.0;
    while start.elapsed() < Duration::from_secs(if full_track { 600 } else { 15 }) {
        let Ok(event) = rx.recv_timeout(Duration::from_secs(2)) else {
            continue;
        };
        match event.kind {
            EventKind::Position(position) if position > 0.0 => {
                last_position = position;
                if first.is_none() {
                    first = Some(start.elapsed().as_secs_f64() * 1000.0);
                    if !full_track {
                        player.send(Control::Seek(30.0));
                    }
                } else if !full_track && position > 30.5 {
                    seek_ok = true;
                    break;
                }
            }
            EventKind::Ended => {
                ended = true;
                break;
            }
            EventKind::Failed(_) => break,
            _ => {}
        }
    }
    player.send(Control::Stop);
    json!({"playback_ms": first, "seek_ok": seek_ok, "ended": ended, "last_position": last_position})
}

fn media(stream: &Stream) -> Value {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .new_agent();
    let mut request = agent.get(&stream.url).header("Range", "bytes=0-2097151");
    for (key, value) in &stream.headers {
        request = request.header(key, value);
    }
    match request.call() {
        Ok(mut response) => {
            let status = response.status().as_u16();
            let mut body = Vec::new();
            let bytes = response
                .body_mut()
                .as_reader()
                .take(2 * 1024 * 1024)
                .read_to_end(&mut body)
                .unwrap_or(0);
            json!({"status": status, "bytes": bytes, "ok": status == 206 && bytes == 2 * 1024 * 1024})
        }
        Err(_) => json!({"ok": false}),
    }
}

fn main() {
    let queries = [
        "Bill Evans Waltz for Debby",
        "Bach cello suite Yo Yo Ma",
        "Miles Davis So What",
        "Debussy Clair de Lune",
        "Ella Fitzgerald Misty",
    ];
    let ids = [
        "Az4SnX521pI",
        "WDe5PzVNSr8",
        "KJEzFvXx3Xw",
        "WKU8DJzipW4",
        "jy5x7bDYd4o",
    ];
    let native = Native::new().expect("native service");
    if std::env::args().any(|arg| arg == "--full-track") {
        let id = "jy5x7bDYd4o";
        let stream = native
            .call_native(&Request::Resolve { id: id.into() })
            .unwrap()
            .stream
            .unwrap();
        let result = playback(&stream, true);
        println!(
            "{}",
            json!({"id": id, "backend": "rustypipe", "playback": result})
        );
        assert_eq!(result["ended"], true, "track must finish normally");
        assert!(result["last_position"].as_f64().unwrap() > 60.0);
        return;
    }
    let mut samples = Vec::new();
    for (round, (query, id)) in queries.iter().zip(ids).enumerate() {
        for backend in if round % 2 == 0 {
            ["python", "rustypipe"]
        } else {
            ["rustypipe", "python"]
        } {
            for (operation, request) in [
                (
                    "search",
                    Request::Search {
                        query: (*query).into(),
                        filter: "songs".into(),
                    },
                ),
                ("resolve", Request::Resolve { id: id.into() }),
            ] {
                let start = Instant::now();
                let result = if backend == "python" {
                    Live.call(request, false)
                } else {
                    native.call_native(&request)
                };
                let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                let mut row = json!({"backend": backend, "operation": operation, "round": round, "ms": elapsed, "ok": result.is_ok()});
                if let Ok(reply) = result {
                    row["rows"] = json!(reply.tracks.len());
                    if let Some(stream) = reply.stream {
                        row["id"] = json!(id);
                        row["codec"] = json!(stream.codec);
                        row["bitrate"] = json!(stream.bitrate);
                        row["playback"] = playback(&stream, false);
                        row["media"] = media(&stream);
                        if let Some(playback_ms) = row["playback"]["playback_ms"].as_f64() {
                            row["start_ms"] = json!(elapsed + playback_ms);
                        }
                    }
                }
                eprintln!(
                    "{backend} {operation} round {round}: {elapsed:.0} ms, success {}",
                    row["ok"]
                );
                samples.push(row);
            }
        }
    }
    println!("{}", serde_json::to_string_pretty(&json!({
        "utc_unix": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
        "host": "wx-workstation", "rustypipe": "0.11.4 + VisionOs patch", "samples": samples,
        "note": "Retained native client; fresh Python helper per request. Same 40-row limit, queries and video IDs; alternating order. Null audio sink, first positive position, seek to 30 seconds and separate 2 MiB range validation. No credentials."
    })).unwrap());
}
