use aizome_kit::{
    Appearance,
    prelude::*,
    testing::{Harness, assert_golden},
    theme::ThemeChoice,
};
use aizome_uta::{
    app::{Msg, Page, Playback, Uta},
    model::{Queue, Repeat, Stream},
    player::{Event, EventKind},
    service::{Reply, Request, Service, demo_tracks},
    store::{Saved, Store},
};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Fixture {
    connected: bool,
    calls: Mutex<Vec<(Request, bool)>>,
}
impl Service for Fixture {
    fn inline(&self) -> bool {
        true
    }
    fn call(&self, request: Request, auth: bool) -> Result<Reply, String> {
        self.calls.lock().unwrap().push((request.clone(), auth));
        Ok(match request {
            Request::Status => Reply {
                connected: self.connected,
                account: "Test listener".into(),
                ..Reply::default()
            },
            Request::Search { .. } | Request::Liked => Reply {
                tracks: demo_tracks(),
                ..Reply::default()
            },
            Request::Connect { .. } => Reply {
                connected: true,
                account: "Test listener".into(),
                ..Reply::default()
            },
            Request::Disconnect | Request::Rate { .. } => Reply::default(),
            Request::Resolve { .. } => Reply {
                stream: Some(Stream {
                    url: "https://example.invalid/audio".into(),
                    codec: "opus".into(),
                    bitrate: 251.0,
                    authenticated: auth,
                    ..Stream::default()
                }),
                ..Reply::default()
            },
            _ => Reply::default(),
        })
    }
}

fn app(connected: bool) -> (Harness<Uta>, Arc<Fixture>) {
    let service = Arc::new(Fixture {
        connected,
        ..Fixture::default()
    });
    (
        Harness::new(Uta::new(service.clone(), None, false)),
        service,
    )
}

fn search(h: &mut Harness<Uta>) {
    h.send(Msg::Query("Evening listening".into()));
    h.send(Msg::Search);
    assert_eq!(h.program().tracks.len(), 6);
}

#[test]
fn queue_repeat_skip_and_removal_preserve_the_playing_song() {
    let mut queue = Queue::default();
    let songs = demo_tracks();
    queue.replace(songs.clone(), 2);
    queue.remove(0);
    assert_eq!(queue.track().unwrap().id, songs[2].id);
    queue.repeat = Repeat::One;
    assert!(queue.advance(true));
    assert_eq!(queue.track().unwrap().id, songs[2].id);
    assert!(queue.advance(false));
    assert_eq!(queue.track().unwrap().id, songs[3].id);
    queue.current = Some(queue.tracks.len() - 1);
    queue.repeat = Repeat::Off;
    assert!(!queue.advance(true));
    queue.repeat = Repeat::All;
    assert!(queue.advance(true));
    assert_eq!(queue.current, Some(0));
    let current = queue.track().unwrap().id.clone();
    let mut identities: Vec<_> = queue.tracks.iter().map(|t| t.id.clone()).collect();
    identities.sort();
    queue.shuffle();
    assert_eq!(queue.track().unwrap().id, current);
    let mut after: Vec<_> = queue.tracks.iter().map(|t| t.id.clone()).collect();
    after.sort();
    assert_eq!(identities, after);
}

#[test]
fn stop_and_navigation_discard_stale_background_replies() {
    let (mut h, _) = app(false);
    search(&mut h);
    let content_generation = h.program().content_generation;
    h.send(Msg::Play(0));
    let play_generation = h.program().play_generation;
    assert_eq!(h.program().playback, Playback::Resolving);
    h.send(Msg::Stop);
    h.send(Msg::Resolved(
        play_generation,
        Ok(Reply {
            stream: Some(Stream::default()),
            ..Reply::default()
        }),
    ));
    h.send(Msg::Player(Event {
        generation: play_generation,
        kind: EventKind::Started,
    }));
    assert_eq!(h.program().playback, Playback::Stopped);
    h.send(Msg::Page(Page::Account));
    h.send(Msg::Content(
        content_generation,
        Ok(Reply {
            tracks: demo_tracks(),
            ..Reply::default()
        }),
    ));
    assert!(h.program().tracks.is_empty());
    assert_eq!(h.program().page, Page::Account);
}

#[test]
fn account_authentication_reaches_search_and_playback_and_disconnect_cancels_audio() {
    let (mut h, fixture) = app(true);
    search(&mut h);
    h.send(Msg::Play(1));
    let generation = h.program().play_generation;
    h.send(Msg::Player(Event {
        generation,
        kind: EventKind::Started,
    }));
    assert_eq!(h.program().playback, Playback::Playing);
    let calls = fixture.calls.lock().unwrap();
    assert!(
        calls
            .iter()
            .any(|(r, auth)| matches!(r, Request::Search { .. }) && *auth)
    );
    assert!(
        calls
            .iter()
            .any(|(r, auth)| matches!(r, Request::Resolve { .. }) && *auth)
    );
    drop(calls);
    h.send(Msg::Page(Page::Account));
    h.send(Msg::Disconnect);
    assert!(!h.program().connected);
    assert_eq!(h.program().playback, Playback::Stopped);
    h.send(Msg::Player(Event {
        generation,
        kind: EventKind::Ended,
    }));
    assert_eq!(h.program().playback, Playback::Stopped);
    assert_eq!(h.program().queue.current, Some(1));
}

#[test]
fn eof_advances_once_and_an_old_eof_cannot_skip_a_second_song() {
    let (mut h, _) = app(false);
    search(&mut h);
    h.send(Msg::Play(0));
    let generation = h.program().play_generation;
    h.send(Msg::Player(Event {
        generation,
        kind: EventKind::Ended,
    }));
    assert_eq!(h.program().queue.current, Some(1));
    h.send(Msg::Player(Event {
        generation,
        kind: EventKind::Ended,
    }));
    assert_eq!(h.program().queue.current, Some(1));
}

#[test]
fn skipping_preserves_pause_and_stop_instead_of_starting_audio() {
    let (mut h, _) = app(false);
    search(&mut h);
    h.send(Msg::Play(0));
    let generation = h.program().play_generation;
    h.send(Msg::Player(Event {
        generation,
        kind: EventKind::Started,
    }));
    h.send(Msg::Toggle);
    assert_eq!(h.program().playback, Playback::Paused);
    h.send(Msg::Next);
    let generation = h.program().play_generation;
    h.send(Msg::Player(Event {
        generation,
        kind: EventKind::Started,
    }));
    assert_eq!(h.program().queue.current, Some(1));
    assert_eq!(h.program().playback, Playback::Paused);
    h.send(Msg::Stop);
    h.send(Msg::Next);
    assert_eq!(h.program().queue.current, Some(2));
    assert_eq!(h.program().playback, Playback::Stopped);
}

#[test]
fn liking_a_song_updates_the_visible_list_and_queue_after_success() {
    let (mut h, fixture) = app(true);
    search(&mut h);
    h.send(Msg::Play(0));
    let id = h.program().tracks[0].id.clone();
    h.send(Msg::Like(id.clone(), true));
    assert!(h.program().tracks[0].liked);
    assert!(h.program().queue.track().unwrap().liked);
    assert!(h.program().rating.is_empty());
    assert!(fixture.calls.lock().unwrap().iter().any(
        |(r, auth)| matches!(r, Request::Rate { id: found, like: true } if *found == id) && *auth
    ));
}

#[test]
fn keyboard_search_and_row_actions_work_through_real_widgets() {
    let (mut h, _) = app(false);
    let w = h.window();
    h.focus_window(w, true);
    h.command(w, 'f');
    h.type_text(w, "Evening listening");
    h.named(
        w,
        aizome_kit::sidestep_ui::NamedKey::Enter,
        aizome_kit::sidestep_ui::Modifiers::NONE,
    );
    assert_eq!(h.program().tracks.len(), 6);
    let play = h.find_id(w, "play-1").expect("visible play action");
    h.click(w, play.center());
    assert_eq!(h.program().queue.current, Some(1));
    h.command(w, '3');
    assert_eq!(h.program().page, Page::Queue);
    assert!(h.texts(w).iter().any(|text| text == "Up next"));
}

#[test]
fn persisted_queue_restores_without_autoplay_and_rejects_invalid_state() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store(dir.path().join("session.json"));
    let mut queue = Queue::default();
    queue.replace(demo_tracks(), 3);
    store
        .save(&Saved {
            queue,
            volume: 42.0,
        })
        .unwrap();
    let app = Uta::new(Arc::new(Fixture::default()), Some(store.clone()), false);
    assert_eq!(app.queue.current, Some(3));
    assert_eq!(app.volume, 42.0);
    assert_eq!(app.playback, Playback::Stopped);
    std::fs::write(&store.0, "{broken").unwrap();
    assert!(store.load().queue.tracks.is_empty());
}

#[test]
fn native_views_in_both_themes() {
    for (theme, name) in [(ThemeChoice::Night, "night"), (ThemeChoice::Day, "day")] {
        let appearance = Appearance {
            theme,
            reduced_motion: true,
            ..Appearance::default()
        };
        let mut h = Harness::with(
            Uta::new(
                Arc::new(Fixture {
                    connected: true,
                    ..Fixture::default()
                }),
                None,
                false,
            ),
            appearance,
            true,
        );
        assert!(
            h.theme().has_ui_font(),
            "Run this test with AIZOME_FONT_DIR or inside nix develop"
        );
        let w = h.window();
        search(&mut h);
        h.send(Msg::Play(1));
        let generation = h.program().play_generation;
        h.send(Msg::Player(Event {
            generation,
            kind: EventKind::Started,
        }));
        h.send(Msg::Player(Event {
            generation,
            kind: EventKind::Position(72.0),
        }));
        assert_golden(&mut h, w, format!("tests/snapshots/search-{name}.png"), 1.0);
        h.send(Msg::Page(Page::Account));
        assert_golden(
            &mut h,
            w,
            format!("tests/snapshots/account-{name}.png"),
            1.0,
        );
        h.resize(w, Size::new(800.0, 560.0));
        h.send(Msg::Page(Page::Queue));
        assert_golden(
            &mut h,
            w,
            format!("tests/snapshots/queue-small-{name}.png"),
            1.0,
        );
    }
}
