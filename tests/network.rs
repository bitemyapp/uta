//! Opt-in integration check against public YouTube content. Never authenticates.
use aizome_uta::{
    player::{Control, EventKind, Player},
    service::{Live, Request, Service},
};
use std::{sync::mpsc, time::Duration};

#[test]
#[ignore = "requires network access to YouTube; run in nix develop"]
fn public_song_search_resolution_and_real_audio_transport() {
    let result = Live
        .call(
            Request::Search {
                query: "Bill Evans Waltz for Debby".into(),
                filter: "songs".into(),
            },
            false,
        )
        .unwrap();
    assert!(!result.tracks.is_empty());
    let track = result
        .tracks
        .iter()
        .find(|t| t.artists == "Bill Evans")
        .unwrap_or(&result.tracks[0]);
    let stream = Live
        .call(
            Request::Resolve {
                id: track.id.clone(),
            },
            false,
        )
        .unwrap()
        .stream
        .unwrap();
    assert!(!stream.authenticated);
    println!(
        "Resolved {} — {} ({}, {:.0} kbps)",
        track.artists, track.title, stream.codec, stream.bitrate
    );
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
        volume: 50.0,
        paused: false,
    });
    let until = std::time::Instant::now() + Duration::from_secs(30);
    let mut advanced = false;
    while std::time::Instant::now() < until {
        match rx
            .recv_timeout(Duration::from_secs(10))
            .expect("audio events")
            .kind
        {
            EventKind::Position(seconds) if seconds > 0.5 => {
                advanced = true;
                break;
            }
            EventKind::Failed(error) => panic!("{error}"),
            _ => {}
        }
    }
    player.send(Control::Stop);
    assert!(
        advanced,
        "the public stream reaches mpv's audio output (null sink)"
    );
}
