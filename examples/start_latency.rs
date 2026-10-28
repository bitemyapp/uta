//! How long a song takes to be heard, against public YouTube (no account):
//! one chosen from a list, which is looked up then, and the next in the
//! queue, looked up while the first played. Plays at volume 0.
use aizome_kit::testing::Harness;
use aizome_uta::{
    app::{Msg, Playback, Uta},
    service,
};
use std::time::{Duration, Instant};

fn until(h: &mut Harness<Uta>, what: &str, done: impl Fn(&Uta) -> bool) -> Duration {
    let start = Instant::now();
    while !done(h.program()) {
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "{what} timed out"
        );
        assert!(h.program().error.is_none(), "{:?}", h.program().error);
        std::thread::sleep(Duration::from_millis(5));
        h.advance(Duration::from_millis(5));
    }
    start.elapsed()
}

fn heard(index: usize) -> impl Fn(&Uta) -> bool {
    move |app| {
        app.queue.current == Some(index) && app.playback == Playback::Playing && app.position > 0.0
    }
}

fn main() {
    // Media controls stay off the desktop's bus while this runs.
    // SAFETY: no other thread exists yet.
    unsafe { std::env::remove_var("DBUS_SESSION_BUS_ADDRESS") };
    let mut h = Harness::new(Uta::new(service::live(), None, false));
    until(&mut h, "account status", |app| !app.account_busy);
    h.send(Msg::Volume(0.0));
    h.send(Msg::Query("Bill Evans Waltz for Debby".into()));
    h.send(Msg::Search);
    until(&mut h, "search", |app| !app.busy && app.tracks.len() > 3);
    for round in 0..3 {
        let first = round * 2;
        h.send(Msg::Play(first));
        let chosen = until(&mut h, "the chosen song", heard(first));
        // Listen a while: the next song is looked up meanwhile.
        let listened = Instant::now();
        until(&mut h, "listening", |_| {
            listened.elapsed() > Duration::from_secs(6)
        });
        until(&mut h, "the look-ahead", |app| app.prefetching.is_none());
        h.send(Msg::Next);
        let started = until(&mut h, "the next song to load", |app| {
            app.queue.current == Some(first + 1) && app.playback == Playback::Playing
        });
        let next = started + until(&mut h, "the next song", heard(first + 1));
        println!(
            "chosen from the list: heard after {:.2}s; next in the queue: {:.2}s (loaded at {:.2}s)",
            chosen.as_secs_f64(),
            next.as_secs_f64(),
            started.as_secs_f64()
        );
    }
    h.send(Msg::Stop);
}
