use aizome_uta::{
    mpris::{Action, Mpris, State},
    service::demo_tracks,
};
use std::{
    collections::HashMap,
    sync::mpsc,
    time::{Duration, Instant},
};
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::OwnedValue,
};

#[test]
fn mpris_controls() {
    let _ = env_logger::builder().is_test(true).try_init();
    // Every run gets a private session bus; never register on, or control, the
    // developer's real desktop bus. The child only runs this one test.
    if std::env::var_os("UTA_MPRIS_TEST_PRIVATE_BUS").is_none() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("session.conf");
        std::fs::write(
            &config,
            r#"<busconfig>
          <type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth>
          <policy context="default">
            <allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/>
          </policy>
        </busconfig>"#,
        )
        .unwrap();
        let status = std::process::Command::new("dbus-run-session")
            .arg(format!("--config-file={}", config.display()))
            .arg("--")
            .arg(std::env::current_exe().unwrap())
            .args(["--exact", "mpris_controls", "--nocapture"])
            .env("UTA_MPRIS_TEST_PRIVATE_BUS", "1")
            .status()
            .expect("dbus-run-session is installed");
        assert!(status.success());
        return;
    }
    let (tx, rx) = mpsc::channel();
    let media = Mpris::start(move |action| {
        tx.send(action).unwrap();
    });
    media.publish(State {
        track: Some(demo_tracks()[0].clone()),
        status: "Playing".into(),
        position: 5.0,
        duration: 248.0,
        volume: 0.7,
        can_next: true,
        can_previous: true,
        ..State::default()
    });
    let connection = Connection::session().unwrap();
    let player = Proxy::new(
        &connection,
        "org.mpris.MediaPlayer2.uta",
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2.Player",
    )
    .unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let status = player.get_property::<String>("PlaybackStatus");
        if status.as_deref().ok() == Some("Playing") {
            break;
        }
        assert!(
            Instant::now() < until,
            "MPRIS becomes available: {status:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let metadata: HashMap<String, OwnedValue> = player.get_property("Metadata").unwrap();
    assert_eq!(
        String::try_from(metadata["xesam:title"].try_clone().unwrap()).unwrap(),
        "Soft light"
    );
    assert!(!metadata.contains_key("cookies"));
    assert_eq!(player.get_property::<i64>("Position").unwrap(), 5_000_000);
    player.call::<_, _, ()>("Pause", &()).unwrap();
    assert!(matches!(
        rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        Action::Pause
    ));
    player.call::<_, _, ()>("Seek", &(2_000_000i64,)).unwrap();
    assert!(
        matches!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), Action::Seek(v) if v == 7.0)
    );
    player.set_property("Volume", 0.35f64).unwrap();
    assert!(
        matches!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), Action::Volume(v) if v == 35.0)
    );
    player.call::<_, _, ()>("Next", &()).unwrap();
    assert!(matches!(
        rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        Action::Next
    ));
    player.set_property("LoopStatus", "Track").unwrap();
    assert!(matches!(
        rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        Action::Repeat(aizome_uta::model::Repeat::One)
    ));
    media.publish(State {
        track: Some(demo_tracks()[0].clone()),
        status: "Paused".into(),
        position: 7.0,
        duration: 248.0,
        ..State::default()
    });
    let until = Instant::now() + Duration::from_secs(3);
    while player.get_property::<String>("PlaybackStatus").unwrap() != "Paused" {
        assert!(
            Instant::now() < until,
            "property-change signals update the client's cache"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
