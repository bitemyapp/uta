//! Opt-in integration check against public YouTube content. Never authenticates.
use aizome_uta::{
    player::{Control, EventKind, Player},
    service::{self, Request},
};
use std::{sync::mpsc, time::Duration};

#[test]
#[ignore = "requires network access to YouTube; run in nix develop"]
fn public_song_search_resolution_and_real_audio_transport() {
    let service = service::live();
    let result = service
        .call(
            Request::Search {
                query: "Bill Evans Waltz for Debby".into(),
                filter: "songs".into(),
            },
            false,
        )
        .unwrap();
    #[cfg(feature = "rustypipe")]
    assert_eq!(
        result.backend, "rustypipe",
        "search must exercise the native adapter"
    );
    assert!(!result.tracks.is_empty());
    let track = result
        .tracks
        .iter()
        .find(|t| t.artists == "Bill Evans")
        .unwrap_or(&result.tracks[0]);
    let reply = service
        .call(
            Request::Resolve {
                id: track.id.clone(),
            },
            false,
        )
        .unwrap();
    #[cfg(feature = "rustypipe")]
    assert_eq!(
        reply.backend, "rustypipe",
        "resolution must not hide a fallback"
    );
    let stream = reply.stream.unwrap();
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

#[cfg(feature = "rustypipe")]
#[test]
#[ignore = "requires anonymous YouTube Music requests; run in nix develop"]
fn native_public_metadata_coverage() {
    use aizome_uta::native_service::Native;
    let service = Native::new().unwrap();
    for filter in ["songs", "videos", "albums"] {
        let found = service
            .call_native(&Request::Search {
                query: "Bill Evans Waltz for Debby".into(),
                filter: filter.into(),
            })
            .unwrap();
        if filter == "albums" {
            assert!(!found.collections.is_empty());
            let album = service
                .call_native(&Request::Collection {
                    id: found.collections[0].id.clone(),
                    kind: "album".into(),
                })
                .unwrap();
            assert!(!album.tracks.is_empty());
        } else {
            assert!(!found.tracks.is_empty(), "{filter}");
        }
    }
    let radio = service
        .call_native(&Request::Watch {
            id: "Az4SnX521pI".into(),
            radio: true,
        })
        .unwrap();
    assert!(!radio.tracks.is_empty());
    // Combined playlist search deliberately remains on the existing adapter.
    use aizome_uta::service::{Live, Service};
    let top = service
        .call(
            Request::Search {
                query: "Bill Evans Waltz for Debby".into(),
                filter: "top".into(),
            },
            false,
        )
        .unwrap();
    assert_eq!(top.backend, "python");
    assert!(!top.tracks.is_empty());
    let playlists = Live
        .call(
            Request::Search {
                query: "Bill Evans".into(),
                filter: "playlists".into(),
            },
            false,
        )
        .unwrap();
    let playlist = service
        .call_native(&Request::Collection {
            id: playlists.collections[0].id.clone(),
            kind: "playlist".into(),
        })
        .unwrap();
    assert!(!playlist.tracks.is_empty());
}
