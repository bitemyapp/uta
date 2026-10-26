use aizome_uta::{
    model::Stream,
    player::{Control, EventKind, Player},
};
use std::{
    io::Write,
    sync::mpsc,
    time::{Duration, Instant},
};

#[test]
fn mpv_plays_seeks_pauses_and_finishes_with_the_correct_generation() {
    let mut file = tempfile::Builder::new().suffix(".wav").tempfile().unwrap();
    let samples = 48000u32 * 3;
    let mut wav = Vec::new();
    wav.extend(b"RIFF");
    wav.extend((36 + samples * 2).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(48000u32.to_le_bytes());
    wav.extend(96000u32.to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend((samples * 2).to_le_bytes());
    wav.resize(wav.len() + samples as usize * 2, 0);
    file.write_all(&wav).unwrap();
    let (tx, rx) = mpsc::channel();
    let player = Player::start(
        move |event| {
            let _ = tx.send(event);
        },
        true,
    );
    player.send(Control::Load {
        generation: 42,
        stream: Stream {
            url: file.path().display().to_string(),
            ..Stream::default()
        },
        volume: 50.0,
        paused: false,
    });
    let until = Instant::now() + Duration::from_secs(12);
    let (mut started, mut paused, mut ended) = (false, false, false);
    while Instant::now() < until {
        let event = rx
            .recv_timeout(Duration::from_secs(3))
            .expect("mpv sends playback events");
        assert_eq!(event.generation, 42);
        match event.kind {
            EventKind::Started => {
                started = true;
                player.send(Control::Pause(true));
            }
            EventKind::Paused(true) => {
                paused = true;
                player.send(Control::Seek(2.0));
                player.send(Control::Pause(false));
            }
            EventKind::Ended => {
                ended = true;
                break;
            }
            EventKind::Failed(error) => panic!("mpv: {error}"),
            _ => {}
        }
    }
    assert!(
        started && paused && ended,
        "mpv completes the playback lifecycle"
    );
    player.send(Control::Stop);
}

#[test]
fn a_second_launch_hands_over_without_replacing_the_running_socket() {
    use aizome_uta::instance::{Claim, claim_in};
    use std::io::Read;
    let dir = tempfile::tempdir().unwrap();
    let Claim::First(first) = claim_in(dir.path().into(), &[]).unwrap() else {
        panic!()
    };
    assert!(matches!(
        claim_in(dir.path().into(), &["jazz".into()]).unwrap(),
        Claim::Handed
    ));
    let (mut socket, _) = first.listener.accept().unwrap();
    let mut message = String::new();
    socket.read_to_string(&mut message).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<String>>(&message).unwrap(),
        ["jazz"]
    );
    drop(first);
    assert!(matches!(
        claim_in(dir.path().into(), &[]).unwrap(),
        Claim::First(_)
    ));
}
