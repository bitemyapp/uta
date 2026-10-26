//! Exercise actual layout and rasterization without a display or network.
use aizome_kit::testing::Harness;
use aizome_uta::{
    app::{Msg, Playback, Uta},
    player::{Event, EventKind},
    service::{Demo, demo_tracks},
};
use std::{sync::Arc, time::Instant};

fn main() {
    // The song count, 200 by default; library lists hold up to 1000.
    let count: usize = std::env::args()
        .nth(1)
        .and_then(|n| n.parse().ok())
        .unwrap_or(200);
    let mut h = Harness::new(Uta::new(Arc::new(Demo), None, true));
    let w = h.window();
    let songs = demo_tracks();
    h.program_mut().tracks = (0..count)
        .map(|i| {
            let mut track = songs[i % songs.len()].clone();
            track.id = format!("demo{i:07}");
            track
        })
        .collect();
    h.send(Msg::Play(0));
    h.program_mut().error = None;
    h.program_mut().playback = Playback::Playing;
    let generation = h.program().play_generation;
    let mut samples = Vec::new();
    for i in 0..140 {
        let start = Instant::now();
        h.send(Msg::Player(Event {
            generation,
            kind: EventKind::Position(i as f64 / 4.0),
        }));
        h.render_frame(w, 1.0);
        if i >= 20 {
            samples.push(start.elapsed().as_micros());
        }
    }
    samples.sort();
    println!(
        "{count}-song view; update, layout, damage rasterization: p50={}µs p99={}µs",
        samples[60], samples[118]
    );
}
