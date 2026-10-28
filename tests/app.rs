use aizome_kit::{
    Appearance,
    prelude::*,
    testing::{Harness, assert_golden},
    theme::ThemeChoice,
};
use aizome_uta::{
    app::{Msg, Page, Playback, Uta},
    model::{Link, Queue, Repeat, Source, Stream, Track, ago, link},
    player::{Event, EventKind},
    service::{Reply, Request, Secret, Service, demo_tracks},
    store::{Saved, Store},
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

const NOW: f64 = 1_793_000_000.0;

#[derive(Default)]
struct Fixture {
    connected: bool,
    calls: Mutex<Vec<(Request, bool)>>,
    sources: Mutex<Vec<Source>>,
    /// Streams whose signed URLs expire within a minute.
    short_lived: bool,
    native_resolution: bool,
}

fn source(id: &str, browser: &str, profile: &str, signed_in: bool, used: f64) -> Source {
    Source {
        id: id.into(),
        browser: browser.into(),
        profile: profile.into(),
        signed_in,
        used,
    }
}

fn browsers() -> Vec<Source> {
    vec![
        source(
            "chromium:/c/Default",
            "Chromium",
            "Person 1",
            true,
            NOW - 300.0,
        ),
        source(
            "gecko:/f/x.default",
            "Firefox",
            "default-release",
            false,
            0.0,
        ),
    ]
}

impl Fixture {
    fn with_browsers(connected: bool) -> Self {
        Self {
            connected,
            sources: Mutex::new(browsers()),
            ..Self::default()
        }
    }

    fn requests(&self) -> Vec<Request> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(|(r, _)| r.clone())
            .collect()
    }

    /// How often a song's stream was asked for, and with which account state.
    fn resolves(&self, song: &str) -> Vec<bool> {
        let calls = self.calls.lock().unwrap();
        calls
            .iter()
            .filter(|(r, _)| matches!(r, Request::Resolve { id } if id == song))
            .map(|(_, auth)| *auth)
            .collect()
    }
}

fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

impl Service for Fixture {
    fn inline(&self) -> bool {
        true
    }
    fn call(&self, request: Request, auth: bool) -> Result<Reply, String> {
        self.calls.lock().unwrap().push((request.clone(), auth));
        let native = self.native_resolution && matches!(request, Request::Resolve { .. });
        let connected = |source: &str| Reply {
            connected: true,
            account: "Test listener".into(),
            source: source.into(),
            source_id: if source.contains("Chromium") {
                "chromium:/c/Default".into()
            } else {
                String::new()
            },
            ..Reply::default()
        };
        Ok(match request {
            Request::Status if self.connected => connected("Chromium · Person 1"),
            Request::Status => Reply::default(),
            Request::Discover => Reply {
                sources: self.sources.lock().unwrap().clone(),
                now: NOW,
                ..Reply::default()
            },
            Request::Search { .. } | Request::Liked | Request::History => Reply {
                tracks: demo_tracks(),
                ..Reply::default()
            },
            Request::Watch { id, radio } => {
                // The song first, then what YouTube would play after it.
                let mut tracks = demo_tracks();
                let first = tracks.iter().position(|t| t.id == id).unwrap_or(0);
                tracks.rotate_left(first);
                if radio {
                    tracks.push(Track {
                        id: "radio000001".into(),
                        title: "Something similar".into(),
                        ..Track::default()
                    });
                }
                Reply {
                    title: if radio { "Radio" } else { "Up next" }.into(),
                    tracks,
                    ..Reply::default()
                }
            }
            Request::Connect { source } => {
                let found = self
                    .sources
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|s| s.id == source)
                    .cloned();
                match found {
                    Some(s) if s.signed_in => connected(&s.label()),
                    _ => return Err("That browser profile is no longer available.".into()),
                }
            }
            Request::ConnectText { .. } => connected("Copied from your browser"),
            Request::Disconnect | Request::Rate { .. } => Reply::default(),
            Request::Resolve { id } | Request::ResolveFallback { id } => Reply {
                stream: Some(Stream {
                    // Signed like YouTube's: good for six hours, or a minute.
                    url: format!(
                        "https://example.invalid/videoplayback?id={id}&expire={:.0}",
                        unix_now() + if self.short_lived { 60.0 } else { 21600.0 }
                    ),
                    codec: "opus".into(),
                    bitrate: 251.0,
                    authenticated: auth,
                    native,
                    ..Stream::default()
                }),
                ..Reply::default()
            },
            _ => Reply::default(),
        })
    }
}

fn app(connected: bool) -> (Harness<Uta>, Arc<Fixture>) {
    let service = Arc::new(Fixture::with_browsers(connected));
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
            Uta::new(Arc::new(Fixture::with_browsers(true)), None, false),
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
        h.send(Msg::Disconnect);
        assert_golden(
            &mut h,
            w,
            format!("tests/snapshots/connect-{name}.png"),
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

#[test]
fn a_signed_in_browser_connects_with_one_click_and_names_itself() {
    let (mut h, fixture) = app(false);
    let w = h.window();
    h.send(Msg::Page(Page::Account));
    assert!(h.texts(w).iter().any(|t| t == "Chromium · Person 1"));
    assert!(
        h.texts(w)
            .iter()
            .any(|t| t == "Also checked, not signed in: Firefox · default-release.")
    );
    let connect = h.find_id(w, "connect-0").expect("a connect button");
    h.click(w, connect.center());
    let app = h.program();
    assert!(app.connected);
    assert_eq!(app.source, "Chromium · Person 1");
    assert!(
        fixture
            .requests()
            .iter()
            .any(|r| matches!(r, Request::Connect { source } if source == "chromium:/c/Default"))
    );
    // Reconnecting later goes back to the same browser.
    assert!(h.find_id(w, "reconnect").is_some());
}

#[test]
fn signing_in_with_a_browser_connects_once_its_sign_in_appears() {
    let (mut h, fixture) = app(false);
    fixture.sources.lock().unwrap()[0].signed_in = false;
    h.send(Msg::Page(Page::Account));
    h.send(Msg::SignIn);
    assert!(h.program().signing_in);
    h.advance(Duration::from_secs(5));
    assert!(!h.program().connected);
    // The browser saves the sign-in; the next look connects by itself.
    fixture.sources.lock().unwrap()[1].signed_in = true;
    h.advance(Duration::from_secs(5));
    let app = h.program();
    assert!(app.connected);
    assert!(!app.signing_in);
    assert_eq!(app.source, "Firefox · default-release");
}

#[test]
fn a_sign_in_already_there_is_offered_but_not_connected_by_itself() {
    let (mut h, _) = app(false);
    h.send(Msg::Page(Page::Account));
    h.send(Msg::SignIn);
    for _ in 0..3 {
        h.advance(Duration::from_secs(5));
    }
    assert!(!h.program().connected);
    assert!(h.program().signing_in);
    h.send(Msg::StopWaiting);
    h.advance(Duration::from_secs(5));
    assert!(!h.program().signing_in);
}

#[test]
fn the_library_invites_a_one_click_connection() {
    let (mut h, fixture) = app(false);
    let w = h.window();
    h.send(Msg::Page(Page::Liked));
    let connect = h.find_id(w, "connect-best").expect("one-click connect");
    assert!(
        h.texts(w)
            .iter()
            .any(|t| t == "Connect Chromium · Person 1")
    );
    h.click(w, connect.center());
    assert!(h.program().connected);
    assert_eq!(h.program().tracks.len(), 6);
    assert!(
        fixture
            .requests()
            .iter()
            .any(|r| matches!(r, Request::Liked))
    );
}

#[test]
fn a_request_copied_from_any_browser_connects_without_printing_its_cookies() {
    let (mut h, fixture) = app(false);
    let w = h.window();
    h.send(Msg::Page(Page::Account));
    h.platform.clipboard =
        Some("curl 'https://music.youtube.com/' -b '__Secure-3PAPISID=private'".into());
    let paste = h.find_id(w, "connect-clipboard").expect("clipboard button");
    h.click(w, paste.center());
    assert!(h.program().connected);
    let requests = fixture.requests();
    let text = requests
        .iter()
        .find(|r| matches!(r, Request::ConnectText { .. }))
        .expect("sent");
    assert!(!format!("{text:?}").contains("private"));
    assert!(serde_json::to_string(text).unwrap().contains("private"));
    assert!(!format!("{:?}", Secret("private".into())).contains("private"));
}

#[test]
fn an_empty_clipboard_explains_what_to_copy() {
    let (mut h, fixture) = app(false);
    h.send(Msg::Page(Page::Account));
    h.platform.clipboard = None;
    h.send(Msg::ConnectClipboard);
    assert!(
        h.program()
            .error
            .as_deref()
            .unwrap()
            .contains("developer tools")
    );
    assert!(
        !fixture
            .requests()
            .iter()
            .any(|r| matches!(r, Request::ConnectText { .. }))
    );
}

#[test]
fn song_links_play_and_playlist_links_open() {
    let (mut h, fixture) = app(false);
    let songs = demo_tracks();
    h.send(Msg::Query(format!(
        "https://music.youtube.com/watch?v={}&si=x",
        songs[2].id
    )));
    h.send(Msg::Search);
    let app = h.program();
    assert_eq!(app.page, Page::Collection);
    assert_eq!(app.title, "Up next");
    assert_eq!(app.queue.track().unwrap().id, songs[2].id);
    assert_eq!(app.playback, Playback::Resolving);
    h.send(Msg::Query(
        "music.youtube.com/playlist?list=PLabc_123".into(),
    ));
    h.send(Msg::Search);
    assert!(fixture.requests().iter().any(|r| matches!(r,
        Request::Collection { id, kind } if id == "PLabc_123" && kind == "playlist")));
}

#[test]
fn radio_follows_the_song_playing_without_restarting_it() {
    let (mut h, fixture) = app(false);
    search(&mut h);
    h.send(Msg::Play(1));
    let generation = h.program().play_generation;
    h.send(Msg::Player(Event {
        generation,
        kind: EventKind::Started,
    }));
    h.send(Msg::StartRadio(None));
    let app = h.program();
    assert_eq!(app.play_generation, generation);
    assert_eq!(app.playback, Playback::Playing);
    assert_eq!(app.queue.current, Some(0));
    assert_eq!(app.queue.tracks[0].id, demo_tracks()[1].id);
    assert_eq!(app.queue.tracks.last().unwrap().id, "radio000001");
    assert_eq!(
        app.queue
            .tracks
            .iter()
            .filter(|t| t.id == demo_tracks()[1].id)
            .count(),
        1
    );
    assert!(
        fixture
            .requests()
            .iter()
            .any(|r| matches!(r, Request::Watch { radio: true, .. }))
    );
}

#[test]
fn play_next_and_moving_rows_keep_the_song_playing() {
    let (mut h, _) = app(false);
    search(&mut h);
    let songs = demo_tracks();
    h.send(Msg::Play(0));
    h.send(Msg::Page(Page::Queue));
    h.send(Msg::PlayNext(4));
    let order: Vec<_> = h
        .program()
        .queue
        .tracks
        .iter()
        .map(|t| t.title.clone())
        .collect();
    assert_eq!(order[1], songs[4].title);
    h.send(Msg::Move(0, 2));
    assert_eq!(h.program().queue.current, Some(2));
    assert_eq!(h.program().queue.track().unwrap().id, songs[0].id);
    h.send(Msg::Page(Page::Search));
    search(&mut h);
    h.send(Msg::PlayNext(5));
    assert_eq!(h.program().queue.tracks[3].id, songs[5].id);
    assert_eq!(h.program().queue.tracks.len(), 7);
}

#[test]
fn links_relative_times_and_queue_moves() {
    assert_eq!(
        link("https://music.youtube.com/watch?v=dQw4w9WgXcQ&list=RDAMVM"),
        Some(Link::Song("dQw4w9WgXcQ".into()))
    );
    assert_eq!(
        link("youtu.be/dQw4w9WgXcQ"),
        Some(Link::Song("dQw4w9WgXcQ".into()))
    );
    assert_eq!(
        link("https://www.youtube.com/playlist?list=PL_x-1"),
        Some(Link::Playlist("PL_x-1".into()))
    );
    assert_eq!(
        link("https://music.youtube.com/browse/VLPL_x-1"),
        Some(Link::Playlist("PL_x-1".into()))
    );
    assert_eq!(
        link("https://music.youtube.com/browse/MPREb_abc"),
        Some(Link::Album("MPREb_abc".into()))
    );
    assert_eq!(link("https://music.youtube.com/watch?v=short"), None);
    assert_eq!(link("https://evil.example/watch?v=dQw4w9WgXcQ"), None);
    assert_eq!(link("Bill Evans Waltz for Debby"), None);
    assert_eq!(ago(NOW - 30.0, NOW), "just now");
    assert_eq!(ago(NOW - 300.0, NOW), "5 minutes ago");
    assert_eq!(ago(NOW - 3600.0, NOW), "1 hour ago");
    assert_eq!(ago(NOW - 90000.0, NOW), "yesterday");
    assert_eq!(ago(NOW - 86400.0 * 9.0, NOW), "9 days ago");
    let mut queue = Queue::default();
    queue.replace(demo_tracks(), 3);
    assert!(queue.move_track(5, 0));
    assert_eq!(queue.current, Some(4));
    assert!(queue.move_track(4, 5));
    assert_eq!(queue.current, Some(5));
    assert!(!queue.move_track(0, 6));
    queue.insert_next(demo_tracks()[0].clone());
    assert_eq!(queue.tracks.len(), 7);
    assert_eq!(queue.current, Some(5));
}

/// The song loads and plays for a few seconds, long enough to look ahead.
fn start(h: &mut Harness<Uta>) {
    let generation = h.program().play_generation;
    h.send(Msg::Player(Event {
        generation,
        kind: EventKind::Started,
    }));
    h.advance(Duration::from_secs(4));
}

#[test]
fn the_next_song_is_looked_up_while_one_plays_and_starts_without_asking_again() {
    let (mut h, fixture) = app(false);
    let songs = demo_tracks();
    search(&mut h);
    h.send(Msg::Play(0));
    let generation = h.program().play_generation;
    h.send(Msg::Player(Event {
        generation,
        kind: EventKind::Started,
    }));
    // Not while the song is starting: its own start comes first.
    h.advance(Duration::from_secs(1));
    assert!(fixture.resolves(&songs[1].id).is_empty());
    h.advance(Duration::from_secs(3));
    assert_eq!(fixture.resolves(&songs[1].id).len(), 1);
    h.send(Msg::Next);
    // Its stream is already here: it loads at once, without a second lookup.
    assert_eq!(fixture.resolves(&songs[1].id).len(), 1);
    assert_eq!(h.program().stream_label, "OPUS · 251 kbps");
    start(&mut h);
    assert_eq!(fixture.resolves(&songs[2].id).len(), 1);
    // Back to a song just played: its stream is kept too.
    h.send(Msg::Previous);
    assert_eq!(h.program().queue.current, Some(0));
    assert_eq!(fixture.resolves(&songs[0].id).len(), 1);
    // Reordering the queue looks up the song now next.
    start(&mut h);
    h.send(Msg::Page(Page::Queue));
    h.send(Msg::Move(4, 1));
    assert_eq!(fixture.resolves(&songs[4].id).len(), 1);
}

#[test]
fn a_stream_that_would_expire_before_the_song_ends_is_looked_up_again() {
    let fixture = Arc::new(Fixture {
        short_lived: true,
        ..Fixture::with_browsers(false)
    });
    let mut h = Harness::new(Uta::new(fixture.clone(), None, false));
    let songs = demo_tracks();
    search(&mut h);
    h.send(Msg::Play(0));
    start(&mut h);
    h.send(Msg::Next);
    assert_eq!(fixture.resolves(&songs[1].id).len(), 2);
}

#[test]
fn skipping_to_a_song_still_being_looked_up_waits_for_that_lookup() {
    let fixture = Arc::new(Fixture {
        short_lived: true,
        ..Fixture::with_browsers(false)
    });
    let mut h = Harness::new(Uta::new(fixture.clone(), None, false));
    let songs = demo_tracks();
    search(&mut h);
    h.send(Msg::Play(0));
    start(&mut h);
    // As if the look-ahead for the next song hadn't answered yet.
    h.program_mut().prefetching = Some(songs[1].id.clone());
    h.send(Msg::Next);
    assert_eq!(fixture.resolves(&songs[1].id).len(), 1);
    assert_eq!(h.program().playback, Playback::Resolving);
    assert!(h.program().stream_label.is_empty());
    let generation = h.program().auth_generation;
    let stream = Stream {
        url: "https://example.invalid/videoplayback?expire=1".into(),
        codec: "opus".into(),
        bitrate: 160.0,
        ..Stream::default()
    };
    h.send(Msg::Prefetched(
        generation,
        songs[1].id.clone(),
        Ok(Reply {
            stream: Some(stream),
            ..Reply::default()
        }),
    ));
    assert_eq!(h.program().stream_label, "OPUS · 160 kbps");
    assert!(h.program().prefetching.is_none());
}

#[test]
fn connecting_forgets_streams_looked_up_without_the_account() {
    let (mut h, fixture) = app(false);
    let songs = demo_tracks();
    search(&mut h);
    h.send(Msg::Play(0));
    start(&mut h);
    assert_eq!(fixture.resolves(&songs[1].id), [false]);
    h.send(Msg::Connect("chromium:/c/Default".into()));
    assert!(h.program().connected);
    search(&mut h);
    h.send(Msg::Play(1));
    assert_eq!(fixture.resolves(&songs[1].id), [false, true]);
}

#[test]
fn signed_urls_say_when_they_expire_and_the_queue_says_what_is_next() {
    use aizome_uta::model::stream_expiry;
    assert_eq!(
        stream_expiry("https://rr1.googlevideo.com/videoplayback?expire=1793021000&ei=x"),
        Some(1793021000.0)
    );
    assert_eq!(
        stream_expiry(
            "https://manifest.googlevideo.com/api/manifest/hls_playlist/expire/1793021000/ei/x/file/index.m3u8"
        ),
        Some(1793021000.0)
    );
    assert_eq!(stream_expiry("https://example.invalid/audio"), None);
    let mut queue = Queue::default();
    queue.replace(demo_tracks(), 5);
    assert!(queue.peek_next().is_none());
    queue.repeat = Repeat::All;
    assert_eq!(queue.peek_next().unwrap().id, demo_tracks()[0].id);
    queue.current = Some(2);
    assert_eq!(queue.peek_next().unwrap().id, demo_tracks()[3].id);
}

fn native_app() -> (Harness<Uta>, Arc<Fixture>) {
    let fixture = Arc::new(Fixture {
        native_resolution: true,
        ..Fixture::default()
    });
    let mut h = Harness::new(Uta::new(fixture.clone(), None, false));
    search(&mut h);
    h.send(Msg::Play(0));
    (h, fixture)
}

fn fallback_calls(fixture: &Fixture) -> usize {
    fixture
        .requests()
        .iter()
        .filter(|r| matches!(r, Request::ResolveFallback { .. }))
        .count()
}

#[test]
fn native_transport_failure_retries_once_and_replaces_the_failed_cached_url() {
    let (mut h, fixture) = native_app();
    let first = h.program().play_generation;
    h.send(Msg::Player(Event {
        generation: first,
        kind: EventKind::Failed("403".into()),
    }));
    assert_eq!(fallback_calls(&fixture), 1);
    assert!(h.program().error.is_none());
    assert_eq!(h.program().playback, Playback::Resolving);
    let retry = h.program().play_generation;
    assert_ne!(retry, first);
    // A late event from the failed player must not stop the retry.
    h.send(Msg::Player(Event {
        generation: first,
        kind: EventKind::Failed("late".into()),
    }));
    assert_eq!(h.program().playback, Playback::Resolving);
    // A failure from the fallback is reported, without an extraction loop.
    h.send(Msg::Player(Event {
        generation: retry,
        kind: EventKind::Failed("fallback failed".into()),
    }));
    assert_eq!(fallback_calls(&fixture), 1);
    assert_eq!(h.program().error.as_deref(), Some("fallback failed"));
    h.send(Msg::Play(0));
    let generation = h.program().play_generation;
    h.send(Msg::Player(Event {
        generation,
        kind: EventKind::Failed("cached fallback failed".into()),
    }));
    assert_eq!(
        fallback_calls(&fixture),
        1,
        "cache now holds the fallback, not the rejected native URL"
    );
}

#[test]
fn native_transport_failure_after_playback_does_not_restart_the_song() {
    let (mut h, fixture) = native_app();
    let generation = h.program().play_generation;
    h.send(Msg::Player(Event {
        generation,
        kind: EventKind::Started,
    }));
    h.send(Msg::Player(Event {
        generation,
        kind: EventKind::Position(4.0),
    }));
    h.send(Msg::Player(Event {
        generation,
        kind: EventKind::Failed("network lost".into()),
    }));
    assert_eq!(fallback_calls(&fixture), 0);
    assert_eq!(h.program().error.as_deref(), Some("network lost"));
}

#[test]
fn skipped_native_stream_cannot_trigger_fallback_for_the_next_track() {
    let (mut h, fixture) = native_app();
    let generation = h.program().play_generation;
    h.send(Msg::Next);
    h.send(Msg::Player(Event {
        generation,
        kind: EventKind::Failed("obsolete".into()),
    }));
    assert_eq!(fallback_calls(&fixture), 0);
    assert_eq!(h.program().queue.current, Some(1));
    assert!(h.program().error.is_none());
}
