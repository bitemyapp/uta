use crate::{
    instance::Instance,
    model::{Collection, Link, Queue, Source, Stream, Track},
    player::{Control, Event, EventKind, Player},
    service::{REQUEST_LIMIT, Reply, Request, Secret, Service},
    store::{Saved, Store},
};
use aizome_kit::prelude::*;
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const APP_ID: &str = "org.aizome.Uta";
pub const QUEUE_LIMIT: usize = 5000;
/// Search's kinds, as the segmented control lists them.
pub const SEARCH_FILTERS: [&str; 5] = ["top", "songs", "videos", "albums", "playlists"];
/// While waiting for a sign-in, look this often, for about five minutes.
const POLL: Duration = Duration::from_secs(4);
const POLLS: u32 = 75;
/// Streams kept for songs just played or looked up ahead.
const STREAMS: usize = 4;
/// How long a song plays before the next is looked up: the lookup's burst
/// of work would otherwise compete with the song's own start.
const LOOKAHEAD: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Search,
    Liked,
    Playlists,
    Albums,
    History,
    Collection,
    Queue,
    Account,
}

impl Page {
    /// Pages of the listener's own library, which need a connected account.
    pub fn library(self) -> bool {
        matches!(
            self,
            Page::Liked | Page::Playlists | Page::Albums | Page::History
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Playback {
    Stopped,
    Resolving,
    Playing,
    Paused,
}

#[derive(Clone)]
pub enum Msg {
    Page(Page),
    Query(String),
    Filter(usize),
    Search,
    Select(Option<usize>),
    OpenCollection(usize),
    Play(usize),
    PlaySelected,
    PlayNext(usize),
    PlaySelectedNext,
    Enqueue(usize),
    EnqueueSelected,
    Content(u64, Result<Reply, String>),
    Toggle,
    Next,
    Previous,
    Stop,
    Seek(f64),
    Volume(f64),
    Repeat,
    Shuffle,
    Remove(usize),
    Move(usize, usize),
    ClearQueue,
    /// A radio from a song in the list, or from the one playing.
    StartRadio(Option<usize>),
    Radio(u64, String, Result<Reply, String>),
    Resolved(u64, Result<Reply, String>),
    /// Look up the next song's stream, if this playback is still current.
    Prefetch(u64),
    /// A stream looked up ahead, for the account generation it was asked under.
    Prefetched(u64, String, Result<Reply, String>),
    Player(Event),
    Artwork(u64, Vec<u8>),
    Discover,
    Discovered(u64, Result<Reply, String>),
    Poll(u64),
    /// Open YouTube Music in the browser and connect once it's signed in.
    SignIn,
    StopWaiting,
    Connect(String),
    ConnectClipboard,
    ChooseFolder,
    Folder(Option<Vec<PathBuf>>),
    Disconnect,
    Auth(u64, Result<Reply, String>),
    Like(String, bool),
    Rated(u64, String, bool, Result<Reply, String>),
    CopyLink(String),
    OpenSource,
    FocusSearch,
    Activate(Vec<String>),
    Refresh,
    Save(u64),
    Saved(Result<(), String>),
    DismissError,
    Quit,
    Media(crate::mpris::Action),
    SetRepeat(crate::model::Repeat),
}

pub struct Uta {
    pub page: Page,
    pub query: String,
    pub result_query: String,
    pub filter: usize,
    pub title: String,
    pub tracks: Vec<Track>,
    pub collections: Vec<Collection>,
    pub collection: Option<Collection>,
    pub selected: Option<usize>,
    pub busy: bool,
    pub searched: bool,
    pub error: Option<String>,
    pub connected: bool,
    pub account: String,
    pub account_note: String,
    pub account_busy: bool,
    /// The browser the session came from, and its identity to reconnect with.
    pub source: String,
    pub source_id: String,
    /// The browser profiles last found, and the service's clock then.
    pub sources: Vec<Source>,
    pub sources_now: f64,
    pub sources_checked: bool,
    pub discovering: bool,
    /// Waiting for a browser to save a new sign-in, after `SignIn`.
    pub signing_in: bool,
    pub radio_busy: bool,
    pub queue: Queue,
    pub volume: f64,
    pub playback: Playback,
    pause_on_start: bool,
    native_stream: bool,
    seek_generation: u64,
    pub position: f64,
    pub duration: f64,
    pub stream_label: String,
    pub artwork: Option<Image>,
    pub rating: HashSet<String>,
    pub demo: bool,
    pub window: WindowKey,
    pub instance: Option<Instance>,
    pub content_generation: u64,
    pub play_generation: u64,
    pub auth_generation: u64,
    /// The song whose stream is being looked up ahead of time.
    pub prefetching: Option<String>,
    // The song being resolved to play.
    resolving: Option<String>,
    // Streams of songs just played or looked up ahead, newest last: one
    // found here starts without asking YouTube. Only in memory, and checked
    // against its signed URL's expiry before it's used.
    streams: Vec<(String, Stream)>,
    discover_generation: u64,
    radio_generation: u64,
    polls_left: u32,
    // Sign-ins already there when waiting began: only a new one connects by itself.
    known_sign_ins: HashSet<String>,
    // A song link opened: play it when its list arrives.
    autoplay: Option<String>,
    services: Arc<dyn Service>,
    player: Option<Player>,
    mpris: Option<crate::mpris::Mpris>,
    store: Option<Store>,
    save_generation: u64,
    // Serialize background writes, so old snapshots cannot overwrite new ones.
    save_sender: Option<std::sync::mpsc::Sender<Saved>>,
    save_thread: Option<std::thread::JoinHandle<()>>,
}

impl Uta {
    pub fn new(services: Arc<dyn Service>, store: Option<Store>, demo: bool) -> Self {
        let saved = store.as_ref().map(Store::load).unwrap_or_default();
        Self {
            page: Page::Search,
            query: String::new(),
            result_query: String::new(),
            filter: 0,
            title: String::new(),
            tracks: Vec::new(),
            collections: Vec::new(),
            collection: None,
            selected: None,
            busy: false,
            searched: false,
            error: None,
            connected: false,
            account: String::new(),
            account_note: String::new(),
            account_busy: true,
            source: String::new(),
            source_id: String::new(),
            sources: Vec::new(),
            sources_now: 0.0,
            sources_checked: false,
            discovering: false,
            signing_in: false,
            radio_busy: false,
            queue: saved.queue,
            volume: saved.volume,
            playback: Playback::Stopped,
            pause_on_start: false,
            native_stream: false,
            seek_generation: 0,
            position: 0.0,
            duration: 0.0,
            stream_label: String::new(),
            artwork: None,
            rating: HashSet::new(),
            demo,
            window: WindowKey::NONE,
            instance: None,
            content_generation: 0,
            play_generation: 0,
            auth_generation: 0,
            prefetching: None,
            resolving: None,
            streams: Vec::new(),
            discover_generation: 0,
            radio_generation: 0,
            polls_left: 0,
            known_sign_ins: HashSet::new(),
            autoplay: None,
            services,
            player: None,
            mpris: None,
            store,
            save_generation: 0,
            save_sender: None,
            save_thread: None,
        }
    }

    pub fn visible_tracks(&self) -> &[Track] {
        if self.page == Page::Queue {
            &self.queue.tracks
        } else {
            &self.tracks
        }
    }

    /// The browser to offer first: the one most recently on YouTube.
    pub fn best_source(&self) -> Option<&Source> {
        self.sources.iter().find(|s| s.signed_in)
    }

    fn control(&self, command: Control) {
        if let Some(player) = &self.player {
            player.send(command);
        }
    }

    fn request(
        &self,
        request: Request,
        cx: &mut Cx<Msg>,
        message: impl FnOnce(Result<Reply, String>) -> Msg + Send + 'static,
    ) {
        let services = self.services.clone();
        let authenticated = self.connected;
        if services.inline() {
            cx.send(message(services.call(request, authenticated)));
        } else {
            cx.spawn(move || message(services.call(request, authenticated)));
        }
    }

    fn load(&mut self, request: Request, cx: &mut Cx<Msg>) {
        if self.account_busy {
            return;
        }
        self.content_generation += 1;
        if let Request::Search { query, .. } = &request {
            self.result_query = query.clone();
        }
        let generation = self.content_generation;
        self.busy = true;
        self.searched = true;
        self.error = None;
        self.selected = None;
        self.tracks.clear();
        self.collections.clear();
        self.request(request, cx, move |reply| Msg::Content(generation, reply));
    }

    fn refresh(&mut self, cx: &mut Cx<Msg>) {
        match self.page {
            Page::Search if !self.query.trim().is_empty() => {
                if let Some(link) = crate::model::link(&self.query) {
                    return self.open_link(link, cx);
                }
                self.load(
                    Request::Search {
                        query: self.query.trim().into(),
                        filter: SEARCH_FILTERS[self.filter].into(),
                    },
                    cx,
                )
            }
            Page::Liked if self.connected => self.load(Request::Liked, cx),
            Page::Playlists if self.connected => self.load(Request::Playlists, cx),
            Page::Albums if self.connected => self.load(Request::Albums, cx),
            Page::History if self.connected => self.load(Request::History, cx),
            Page::Collection => {
                if let Some(collection) = &self.collection {
                    let request = if collection.kind == "watch" {
                        Request::Watch {
                            id: collection.id.clone(),
                            radio: false,
                        }
                    } else {
                        Request::Collection {
                            id: collection.id.clone(),
                            kind: collection.kind.clone(),
                        }
                    };
                    self.load(request, cx);
                }
            }
            _ => {}
        }
    }

    fn navigate(&mut self, page: Page, cx: &mut Cx<Msg>) {
        self.content_generation += 1;
        self.page = page;
        self.busy = false;
        self.searched = false;
        self.selected = None;
        self.autoplay = None;
        self.tracks.clear();
        self.collections.clear();
        self.error = None;
        self.title = match page {
            Page::Search => "Search".into(),
            Page::Liked => "Liked songs".into(),
            Page::Playlists => "Your playlists".into(),
            Page::Albums => "Your albums".into(),
            Page::History => "History".into(),
            Page::Queue => "Up next".into(),
            Page::Account => "Your account".into(),
            Page::Collection => self
                .collection
                .as_ref()
                .map(|c| c.title.clone())
                .unwrap_or_default(),
        };
        // The account page and a library's invitation to connect show the
        // browsers that are signed in.
        if page == Page::Account || (page.library() && !self.connected && !self.sources_checked) {
            self.discover(cx);
        }
        self.refresh(cx);
    }

    fn open_link(&mut self, link: Link, cx: &mut Cx<Msg>) {
        let (id, kind) = match &link {
            Link::Song(id) => (id.clone(), "watch"),
            Link::Playlist(id) => (id.clone(), "playlist"),
            Link::Album(id) => (id.clone(), "album"),
        };
        self.collection = Some(Collection {
            id: id.clone(),
            title: if kind == "watch" {
                "Up next"
            } else {
                "Opening link…"
            }
            .into(),
            subtitle: String::new(),
            kind: kind.into(),
        });
        self.navigate(Page::Collection, cx);
        if matches!(link, Link::Song(_)) {
            self.autoplay = Some(id);
        }
    }

    fn changed(&mut self, cx: &mut Cx<Msg>) {
        self.save_generation += 1;
        cx.after(Duration::from_millis(400), Msg::Save(self.save_generation));
    }

    fn save(&self) {
        if let Some(sender) = &self.save_sender {
            let _ = sender.send(Saved {
                queue: self.queue.clone(),
                volume: self.volume,
            });
        }
    }

    fn stop(&mut self) {
        self.play_generation += 1;
        self.control(Control::Stop);
        self.playback = Playback::Stopped;
        self.native_stream = false;
        self.position = 0.0;
        self.stream_label.clear();
        if self.queue.track().is_none() {
            self.duration = 0.0;
            self.artwork = None;
        }
    }

    fn play_current(&mut self, cx: &mut Cx<Msg>) {
        if self.account_busy {
            return;
        }
        self.stop();
        let Some(track) = self.queue.track().cloned() else {
            return;
        };
        self.duration = track.duration;
        self.artwork = None;
        self.error = None;
        self.playback = Playback::Resolving;
        self.pause_on_start = false;
        self.changed(cx);
        let generation = self.play_generation;
        self.resolving = Some(track.id.clone());
        if let Some(stream) = self.ready_stream(&track) {
            // Looked up ahead, or just played: no need to ask YouTube.
            let reply = Reply {
                stream: Some(stream),
                ..Reply::default()
            };
            cx.send(Msg::Resolved(generation, Ok(reply)));
        } else if self.prefetching.as_deref() != Some(track.id.as_str()) {
            let id = track.id.clone();
            self.request(Request::Resolve { id }, cx, move |reply| {
                Msg::Resolved(generation, reply)
            });
        }
        // Otherwise the lookup already under way answers (`Prefetched`).
        cx.set_title(self.window, format!("{} — Uta", track.title));
        if !self.services.inline() && safe_artwork_url(&track.thumbnail) {
            cx.spawn(move || {
                let bytes = ureq::Agent::config_builder()
                    .timeout_global(Some(Duration::from_secs(10)))
                    .build()
                    .new_agent()
                    .get(&track.thumbnail)
                    .call()
                    .ok()
                    .and_then(|mut reply| {
                        reply
                            .body_mut()
                            .with_config()
                            .limit(4 * 1024 * 1024)
                            .read_to_vec()
                            .ok()
                    })
                    .unwrap_or_default();
                Msg::Artwork(generation, bytes)
            });
        }
    }

    fn play(&mut self, index: usize, cx: &mut Cx<Msg>) {
        if self.account_busy || index >= self.visible_tracks().len() {
            return;
        }
        // Choosing a song supersedes a radio still being fetched.
        self.radio_generation += 1;
        self.radio_busy = false;
        if self.page == Page::Queue {
            self.queue.current = Some(index);
        } else {
            let mut tracks = self.tracks.clone();
            tracks.truncate(QUEUE_LIMIT);
            self.queue.replace(tracks, index);
        }
        self.play_current(cx);
    }

    /// A stream kept for this song that will last until it has played.
    fn ready_stream(&self, track: &Track) -> Option<Stream> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_secs_f64();
        let (_, stream) = self.streams.iter().rev().find(|(id, _)| *id == track.id)?;
        crate::model::stream_expiry(&stream.url)
            .is_some_and(|expiry| expiry > now + track.duration.max(0.0) + 120.0)
            .then(|| stream.clone())
    }

    fn remember(&mut self, id: String, stream: Stream) {
        self.streams.retain(|(kept, _)| *kept != id);
        self.streams.push((id, stream));
        if self.streams.len() > STREAMS {
            self.streams.remove(0);
        }
    }

    /// While a song plays, look up the next one, so it starts at once.
    fn prefetch_next(&mut self, cx: &mut Cx<Msg>) {
        if self.account_busy || !matches!(self.playback, Playback::Playing | Playback::Paused) {
            return;
        }
        let Some(next) = self.queue.peek_next().cloned() else {
            return;
        };
        if self.prefetching.as_deref() == Some(next.id.as_str())
            || self.ready_stream(&next).is_some()
        {
            return;
        }
        self.prefetching = Some(next.id.clone());
        let generation = self.auth_generation;
        let id = next.id.clone();
        self.request(Request::Resolve { id: next.id }, cx, move |reply| {
            Msg::Prefetched(generation, id, reply)
        });
    }

    fn discover(&mut self, cx: &mut Cx<Msg>) {
        if self.discovering {
            return;
        }
        self.discovering = true;
        self.discover_generation += 1;
        let generation = self.discover_generation;
        self.request(Request::Discover, cx, move |reply| {
            Msg::Discovered(generation, reply)
        });
    }

    fn auth(&mut self, request: Request, cx: &mut Cx<Msg>) {
        if self.account_busy || self.demo {
            return;
        }
        self.stop();
        self.signing_in = false;
        // Streams were asked for under the old account (or none).
        self.streams.clear();
        self.prefetching = None;
        self.auth_generation += 1;
        self.content_generation += 1;
        self.radio_generation += 1;
        self.radio_busy = false;
        self.tracks.clear();
        self.collections.clear();
        self.rating.clear();
        self.busy = false;
        self.account_busy = true;
        self.error = None;
        self.account_note.clear();
        let generation = self.auth_generation;
        self.request(request, cx, move |reply| Msg::Auth(generation, reply));
    }

    fn song_link(id: &str) -> String {
        format!("https://music.youtube.com/watch?v={id}")
    }
}

impl Program for Uta {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new(APP_ID, "Uta", "YouTube Music").icon(Icon::EighthNotes)
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.window = cx.open_window(
            WindowSpec::new("Uta — YouTube Music")
                .size(1040.0, 740.0)
                .min_size(800.0, 560.0),
        );
        if !self.services.inline() {
            let tx = cx.stream();
            self.player = Some(Player::start(
                move |event| {
                    tx.send(Msg::Player(event));
                },
                false,
            ));
            let tx = cx.stream();
            self.mpris = Some(crate::mpris::Mpris::start(move |action| {
                tx.send(Msg::Media(action));
            }));
            if let Some(mpris) = &self.mpris {
                mpris.publish(crate::mpris::State {
                    track: self.queue.track().cloned(),
                    volume: self.volume / 100.0,
                    can_previous: self.queue.track().is_some(),
                    repeat: self.queue.repeat.clone(),
                    ..crate::mpris::State::default()
                });
            }
        }
        if let Some(instance) = &self.instance
            && let Ok(listener) = instance.listener.try_clone()
        {
            let tx = cx.stream();
            crate::instance::serve(listener, move |args| tx.send(Msg::Activate(args)));
        }
        if let Some(store) = self.store.clone() {
            let (sender, receiver) = std::sync::mpsc::channel();
            let tx = cx.stream();
            self.save_sender = Some(sender);
            self.save_thread = Some(std::thread::spawn(move || {
                while let Ok(mut saved) = receiver.recv() {
                    while let Ok(newer) = receiver.try_recv() {
                        saved = newer;
                    }
                    let result = store.save(&saved);
                    if result.is_err() {
                        tx.send(Msg::Saved(result));
                    }
                }
            }));
        }
        self.request(Request::Status, cx, |reply| Msg::Auth(0, reply));
        cx.focus("search");
    }

    fn update(&mut self, message: Msg, cx: &mut Cx<Msg>) {
        match message {
            Msg::Page(page) => self.navigate(page, cx),
            Msg::Query(query) => {
                self.query = query;
            }
            Msg::Filter(filter) => {
                self.filter = filter.min(SEARCH_FILTERS.len() - 1);
                if self.page == Page::Search && self.searched {
                    self.refresh(cx);
                }
            }
            Msg::Search => {
                if !self.query.trim().is_empty() {
                    self.navigate(Page::Search, cx);
                }
            }
            Msg::Refresh => {
                if self.page == Page::Account {
                    self.discover(cx);
                }
                self.refresh(cx)
            }
            Msg::Select(index) => self.selected = index,
            Msg::OpenCollection(index) => {
                if let Some(collection) = self.collections.get(index).cloned() {
                    self.collection = Some(collection);
                    self.navigate(Page::Collection, cx);
                }
            }
            Msg::Content(generation, result) if generation == self.content_generation => {
                self.busy = false;
                match result {
                    Ok(reply) => {
                        self.tracks = reply.tracks;
                        self.collections = reply.collections;
                        if !reply.title.is_empty() {
                            self.title = reply.title;
                        }
                        if let Some(id) = self.autoplay.take()
                            && let Some(index) = self.tracks.iter().position(|t| t.id == id)
                        {
                            self.play(index, cx);
                        }
                    }
                    Err(error) => self.error = Some(error),
                }
            }
            Msg::Play(index) => self.play(index, cx),
            Msg::PlaySelected => {
                if let Some(index) = self.selected {
                    self.play(index, cx);
                }
            }
            Msg::PlayNext(index) => {
                if self.page == Page::Queue {
                    // Right after the song playing, wherever this row was.
                    let to = self.queue.current.map_or(0, |i| i + usize::from(index > i));
                    if self.queue.move_track(index, to) {
                        self.selected = None;
                        self.changed(cx);
                        self.prefetch_next(cx);
                    }
                } else if self.queue.tracks.len() < QUEUE_LIMIT
                    && let Some(track) = self.tracks.get(index).cloned()
                {
                    self.queue.insert_next(track);
                    self.changed(cx);
                    self.prefetch_next(cx);
                }
            }
            Msg::PlaySelectedNext => {
                if let Some(index) = self.selected {
                    cx.send(Msg::PlayNext(index));
                }
            }
            Msg::Enqueue(index) => {
                if self.queue.tracks.len() < QUEUE_LIMIT
                    && let Some(track) = self.visible_tracks().get(index).cloned()
                {
                    self.queue.tracks.push(track);
                    self.changed(cx);
                    self.prefetch_next(cx);
                }
            }
            Msg::EnqueueSelected => {
                if let Some(index) = self.selected {
                    cx.send(Msg::Enqueue(index));
                }
            }
            Msg::Toggle => match self.playback {
                Playback::Playing => {
                    self.control(Control::Pause(true));
                    self.playback = Playback::Paused;
                }
                Playback::Paused => {
                    self.control(Control::Pause(false));
                    self.playback = Playback::Playing;
                }
                Playback::Resolving => self.stop(),
                Playback::Stopped => {
                    if self.queue.track().is_none() && !self.queue.tracks.is_empty() {
                        self.queue.current = Some(0);
                    }
                    if self.queue.track().is_some() {
                        self.play_current(cx);
                    } else if !self.tracks.is_empty() {
                        self.play(0, cx);
                    }
                }
            },
            Msg::Stop => self.stop(),
            Msg::Next => {
                let previous = self.playback;
                if self.queue.advance(false) {
                    if previous == Playback::Stopped {
                        self.position = 0.0;
                        self.duration = self.queue.track().map_or(0.0, |t| t.duration);
                        self.artwork = None;
                        self.changed(cx);
                    } else {
                        self.play_current(cx);
                        self.pause_on_start = previous == Playback::Paused;
                    }
                } else {
                    self.stop();
                }
            }
            Msg::Previous => {
                let previous = self.playback;
                if self.position > 3.0 {
                    self.control(Control::Seek(0.0));
                    self.position = 0.0;
                    self.seek_generation += 1;
                } else if self.queue.previous() {
                    if previous == Playback::Stopped {
                        self.duration = self.queue.track().map_or(0.0, |t| t.duration);
                        self.artwork = None;
                        self.changed(cx);
                    } else {
                        self.play_current(cx);
                        self.pause_on_start = previous == Playback::Paused;
                    }
                }
            }
            Msg::Seek(position) => {
                if position.is_finite()
                    && matches!(self.playback, Playback::Playing | Playback::Paused)
                {
                    self.position = position.clamp(0.0, self.duration.max(0.0));
                    self.control(Control::Seek(self.position));
                    self.seek_generation += 1;
                }
            }
            Msg::Volume(volume) => {
                if volume.is_finite() {
                    self.volume = volume.clamp(0.0, 100.0);
                    self.control(Control::Volume(self.volume));
                    self.changed(cx);
                }
            }
            Msg::Repeat => {
                self.queue.repeat = self.queue.repeat.cycle();
                self.changed(cx);
                self.prefetch_next(cx);
            }
            Msg::SetRepeat(repeat) => {
                self.queue.repeat = repeat;
                self.changed(cx);
                self.prefetch_next(cx);
            }
            Msg::Shuffle => {
                self.queue.shuffle();
                self.changed(cx);
                self.prefetch_next(cx);
            }
            Msg::Remove(index) => {
                let was_playing = self.playback != Playback::Stopped;
                let was_paused = self.playback == Playback::Paused;
                if self.queue.remove(index) {
                    self.stop();
                    if was_playing {
                        self.play_current(cx);
                        self.pause_on_start = was_paused;
                    }
                }
                self.selected = None;
                self.changed(cx);
                self.prefetch_next(cx);
            }
            Msg::Move(from, to) => {
                if self.queue.move_track(from, to) {
                    if self.selected == Some(from) {
                        self.selected = Some(to);
                    }
                    self.changed(cx);
                    self.prefetch_next(cx);
                }
            }
            Msg::ClearQueue => {
                self.stop();
                self.radio_generation += 1;
                self.radio_busy = false;
                self.queue.tracks.clear();
                self.queue.current = None;
                self.duration = 0.0;
                self.artwork = None;
                self.selected = None;
                cx.set_title(self.window, "Uta — YouTube Music");
                self.changed(cx);
            }
            Msg::StartRadio(index) => {
                let seed = match index {
                    Some(i) => self.visible_tracks().get(i).cloned(),
                    None => self.queue.track().cloned(),
                };
                if let Some(seed) = seed.filter(|_| !self.account_busy) {
                    self.radio_generation += 1;
                    self.radio_busy = true;
                    let generation = self.radio_generation;
                    let id = seed.id.clone();
                    self.request(Request::Watch { id, radio: true }, cx, move |reply| {
                        Msg::Radio(generation, seed.id, reply)
                    });
                }
            }
            Msg::Radio(generation, seed, result) if generation == self.radio_generation => {
                self.radio_busy = false;
                match result {
                    Ok(reply) => {
                        let mut tracks = reply.tracks;
                        let playing = self.playback != Playback::Stopped
                            && self.queue.track().is_some_and(|t| t.id == seed);
                        if playing {
                            // Keep the song playing; the radio follows it.
                            let current = self.queue.track().cloned().expect("playing");
                            tracks.retain(|t| t.id != seed);
                            tracks.insert(0, current);
                            tracks.truncate(QUEUE_LIMIT);
                            self.queue.tracks = tracks;
                            self.queue.current = Some(0);
                        } else if !tracks.is_empty() {
                            tracks.truncate(QUEUE_LIMIT);
                            let start = tracks.iter().position(|t| t.id == seed).unwrap_or(0);
                            self.queue.replace(tracks, start);
                            self.play_current(cx);
                        }
                        if self.page == Page::Queue {
                            self.selected = None;
                        }
                        self.changed(cx);
                        self.prefetch_next(cx);
                    }
                    Err(error) => self.error = Some(error),
                }
            }
            Msg::Resolved(generation, result) if generation == self.play_generation => match result
                .and_then(|reply| {
                    reply
                        .stream
                        .ok_or("YouTube returned no audio stream".into())
                }) {
                Ok(stream) => {
                    self.native_stream = stream.native;
                    self.stream_label = stream_description(&stream);
                    // Taken, so a second answer for this song can't load it twice.
                    if let Some(id) = self.resolving.take() {
                        self.remember(id, stream.clone());
                    }
                    self.control(Control::Load {
                        generation,
                        stream,
                        volume: self.volume,
                        paused: self.pause_on_start,
                    });
                }
                Err(error) => {
                    self.playback = Playback::Stopped;
                    self.error = Some(error);
                }
            },
            Msg::Prefetch(generation) if generation == self.play_generation => {
                self.prefetch_next(cx);
            }
            Msg::Prefetched(generation, id, result) if generation == self.auth_generation => {
                if self.prefetching.as_deref() == Some(id.as_str()) {
                    self.prefetching = None;
                }
                if self.playback == Playback::Resolving
                    && self.resolving.as_deref() == Some(id.as_str())
                {
                    // The listener skipped to this song while it was looked up.
                    cx.send(Msg::Resolved(self.play_generation, result));
                } else if let Ok(Reply {
                    stream: Some(stream),
                    ..
                }) = result
                {
                    self.remember(id, stream);
                }
            }
            Msg::Player(event) if event.generation == self.play_generation => match event.kind {
                EventKind::Started => {
                    self.playback = if self.pause_on_start {
                        Playback::Paused
                    } else {
                        Playback::Playing
                    };
                    cx.after(LOOKAHEAD, Msg::Prefetch(self.play_generation));
                }
                EventKind::Position(position) => {
                    self.position = position.max(0.0);
                    if self.position > 0.0 {
                        self.native_stream = false;
                    }
                }
                EventKind::Duration(duration) => self.duration = duration.max(0.0),
                EventKind::Paused(paused)
                    if matches!(self.playback, Playback::Playing | Playback::Paused) =>
                {
                    self.playback = if paused {
                        Playback::Paused
                    } else {
                        Playback::Playing
                    };
                }
                EventKind::Ended => {
                    let was_paused = self.playback == Playback::Paused;
                    if self.queue.advance(true) {
                        self.play_current(cx);
                        self.pause_on_start = was_paused;
                    } else {
                        self.stop();
                    }
                }
                EventKind::Failed(error) => {
                    if self.native_stream
                        && self.position == 0.0
                        && let Some(id) = self.queue.track().map(|track| track.id.clone())
                    {
                        // Invalidate the failed URL and its player generation before
                        // retrying. A failed Python stream cannot enter this branch.
                        self.streams.retain(|(cached, _)| cached != &id);
                        self.stop();
                        self.playback = Playback::Resolving;
                        self.resolving = Some(id.clone());
                        let generation = self.play_generation;
                        self.request(Request::ResolveFallback { id }, cx, move |reply| {
                            Msg::Resolved(generation, reply)
                        });
                    } else {
                        self.stop();
                        self.error = Some(error);
                    }
                }
                _ => {}
            },
            Msg::Artwork(generation, bytes) if generation == self.play_generation => {
                self.artwork = Image::decode(bytes);
            }
            Msg::Discover => self.discover(cx),
            Msg::Discovered(generation, result) if generation == self.discover_generation => {
                self.discovering = false;
                match result {
                    Ok(reply) => {
                        self.sources = reply.sources;
                        self.sources_now = reply.now;
                        self.sources_checked = true;
                    }
                    Err(error) if self.page == Page::Account => self.account_note = error,
                    Err(_) => {}
                }
                if self.signing_in && !self.connected {
                    let new = self
                        .sources
                        .iter()
                        .find(|s| s.signed_in && !self.known_sign_ins.contains(&s.id));
                    // While another account request runs, keep looking.
                    if let Some(source) = new.filter(|_| !self.account_busy) {
                        let id = source.id.clone();
                        self.auth(Request::Connect { source: id }, cx);
                    } else if self.polls_left > 0 {
                        self.polls_left -= 1;
                        cx.after(POLL, Msg::Poll(self.discover_generation));
                    } else {
                        self.signing_in = false;
                    }
                }
            }
            Msg::Poll(generation) if generation == self.discover_generation && self.signing_in => {
                self.discover(cx);
            }
            Msg::SignIn => {
                // Tests and the demo answer inline: they never open a browser.
                if !self.services.inline() {
                    cx.open_url("https://music.youtube.com/");
                }
                if !self.demo && !self.connected {
                    self.signing_in = true;
                    self.polls_left = POLLS;
                    self.known_sign_ins = self
                        .sources
                        .iter()
                        .filter(|s| s.signed_in)
                        .map(|s| s.id.clone())
                        .collect();
                    self.discover(cx);
                }
            }
            Msg::StopWaiting => self.signing_in = false,
            Msg::Connect(source) => self.auth(Request::Connect { source }, cx),
            Msg::ConnectClipboard => match cx.clipboard_text() {
                Some(text) if text.len() > REQUEST_LIMIT - 4096 => {
                    self.error = Some("The clipboard holds too much. Copy a single request, or a cookies.txt export of youtube.com only.".into());
                }
                Some(text) if !text.trim().is_empty() => {
                    self.auth(Request::ConnectText { text: Secret(text) }, cx)
                }
                _ => {
                    self.error = Some("The clipboard is empty. Copy a request to music.youtube.com from your browser's developer tools, then try again.".into());
                }
            },
            Msg::ChooseFolder => cx.open_panel(
                self.window,
                FileDialog::new()
                    .title("Choose a browser profile folder")
                    .directories(),
                Msg::Folder,
            ),
            Msg::Folder(Some(paths)) => {
                if let Some(path) = paths.first() {
                    match path.to_str() {
                        Some(path) => self.auth(
                            Request::Connect {
                                source: format!("folder:{path}"),
                            },
                            cx,
                        ),
                        None => {
                            self.error =
                                Some("Uta can't read a folder whose name isn't UTF-8.".into());
                        }
                    }
                }
            }
            Msg::Disconnect => self.auth(Request::Disconnect, cx),
            Msg::Auth(generation, result) if generation == self.auth_generation => {
                self.account_busy = false;
                match result {
                    Ok(reply) => {
                        self.connected = reply.connected;
                        self.account = reply.account;
                        self.account_note = reply.message;
                        self.source = reply.source;
                        self.source_id = reply.source_id;
                        self.content_generation += 1;
                        if self.demo {
                            self.query = "Evening listening".into();
                        }
                        self.refresh(cx);
                    }
                    Err(error) => self.error = Some(error),
                }
                if self.page == Page::Account {
                    self.discover(cx);
                }
            }
            Msg::Like(id, like)
                if self.connected && !self.account_busy && !self.rating.contains(&id) =>
            {
                self.rating.insert(id.clone());
                let generation = self.auth_generation;
                self.request(
                    Request::Rate {
                        id: id.clone(),
                        like,
                    },
                    cx,
                    move |reply| Msg::Rated(generation, id, like, reply),
                );
            }
            Msg::Rated(generation, id, liked, result) if generation == self.auth_generation => {
                self.rating.remove(&id);
                match result {
                    Ok(_) => {
                        for track in self.tracks.iter_mut().chain(self.queue.tracks.iter_mut()) {
                            if track.id == id {
                                track.liked = liked;
                            }
                        }
                        self.changed(cx);
                    }
                    Err(error) => self.error = Some(error),
                }
            }
            Msg::CopyLink(id) => cx.set_clipboard_text(&Self::song_link(&id)),
            Msg::OpenSource => cx.open_url("https://github.com/bitemyapp/uta"),
            Msg::FocusSearch => {
                if self.page != Page::Search {
                    self.navigate(Page::Search, cx);
                }
                cx.focus("search");
            }
            Msg::Activate(args) => {
                cx.activate(self.window);
                if !args.is_empty() {
                    self.query = args.join(" ");
                    cx.send(Msg::Search);
                }
            }
            Msg::Save(generation) if generation == self.save_generation => self.save(),
            Msg::Saved(Err(error)) => self.error = Some(error),
            Msg::DismissError => self.error = None,
            Msg::Quit => cx.quit(),
            Msg::Media(action) => {
                use crate::mpris::Action;
                let message = match action {
                    Action::Play
                        if matches!(self.playback, Playback::Stopped | Playback::Paused) =>
                    {
                        Some(Msg::Toggle)
                    }
                    Action::Pause if self.playback == Playback::Playing => Some(Msg::Toggle),
                    Action::Toggle => Some(Msg::Toggle),
                    Action::Stop => Some(Msg::Stop),
                    Action::Next => Some(Msg::Next),
                    Action::Previous => Some(Msg::Previous),
                    Action::Seek(position) => Some(Msg::Seek(position)),
                    Action::Volume(volume) => Some(Msg::Volume(volume)),
                    Action::Repeat(repeat) => Some(Msg::SetRepeat(repeat)),
                    Action::Raise => Some(Msg::Activate(vec![])),
                    Action::Quit => Some(Msg::Quit),
                    _ => None,
                };
                if let Some(message) = message {
                    cx.send(message);
                }
            }
            _ => {}
        }
        if let Some(mpris) = &self.mpris {
            mpris.publish(crate::mpris::State {
                track: self.queue.track().cloned(),
                status: match self.playback {
                    Playback::Playing => "Playing",
                    Playback::Paused => "Paused",
                    _ => "Stopped",
                }
                .into(),
                position: self.position,
                duration: self.duration,
                volume: self.volume / 100.0,
                can_next: self.queue.current.is_some_and(|i| {
                    i + 1 < self.queue.tracks.len()
                        || self.queue.repeat == crate::model::Repeat::All
                }),
                can_previous: self.queue.track().is_some(),
                repeat: self.queue.repeat.clone(),
                seek_generation: self.seek_generation,
            });
        }
    }

    fn view(&self, _window: WindowKey) -> Element<Msg> {
        crate::ui::view(self)
    }

    fn menus(&self) -> MenuBar<Msg> {
        let current = self.queue.track();
        let selected = self.selected.is_some();
        let app = Menu::new("Uta")
            .item(MenuItem::role_item("About Uta", MenuRole::About))
            .item(MenuItem::new("Account…", Msg::Page(Page::Account)).shortcut(Shortcut::cmd(',')))
            .separator()
            .item(MenuItem::role_item("Hide Uta", MenuRole::Hide).shortcut(Shortcut::cmd('h')))
            .item(MenuItem::new("Quit Uta", Msg::Quit).shortcut(Shortcut::cmd('q')));
        let mut playback = Menu::new("Playback")
            .item(MenuItem::new("Play / Pause", Msg::Toggle).shortcut(Shortcut::cmd('p')))
            .item(
                MenuItem::new("Next Song", Msg::Next)
                    .shortcut(Shortcut::cmd_key(ShortcutKey::Right)),
            )
            .item(
                MenuItem::new("Previous Song", Msg::Previous)
                    .shortcut(Shortcut::cmd_key(ShortcutKey::Left)),
            )
            .item(MenuItem::new("Stop", Msg::Stop).shortcut(Shortcut::cmd('.')))
            .separator()
            .item(MenuItem::new("Play Selected", Msg::PlaySelected).enabled(selected))
            .item(MenuItem::new("Play Selected Next", Msg::PlaySelectedNext).enabled(selected))
            .item(MenuItem::new("Add Selected to Queue", Msg::EnqueueSelected).enabled(selected))
            .item(
                MenuItem::new("Start Radio from This Song", Msg::StartRadio(None))
                    .enabled(current.is_some() && !self.radio_busy),
            )
            .separator()
            .item(MenuItem::new("Shuffle Queue", Msg::Shuffle).enabled(self.queue.tracks.len() > 1))
            .item(MenuItem::new(self.queue.repeat.label(), Msg::Repeat));
        if let Some(track) = current {
            playback = playback
                .separator()
                .item(
                    MenuItem::new(
                        if track.liked {
                            "Unlike Song"
                        } else {
                            "Like Song"
                        },
                        Msg::Like(track.id.clone(), !track.liked),
                    )
                    .enabled(self.connected && !self.rating.contains(&track.id)),
                )
                .item(MenuItem::new(
                    "Copy Song Link",
                    Msg::CopyLink(track.id.clone()),
                ));
        }
        let view = Menu::new("View")
            .item(MenuItem::new("Search", Msg::FocusSearch).shortcut(Shortcut::cmd('f')))
            .item(MenuItem::new("Liked Songs", Msg::Page(Page::Liked)).shortcut(Shortcut::cmd('1')))
            .item(
                MenuItem::new("Playlists", Msg::Page(Page::Playlists)).shortcut(Shortcut::cmd('2')),
            )
            .item(MenuItem::new("Queue", Msg::Page(Page::Queue)).shortcut(Shortcut::cmd('3')))
            .item(MenuItem::new("Albums", Msg::Page(Page::Albums)))
            .item(MenuItem::new("History", Msg::Page(Page::History)))
            .separator()
            .item(MenuItem::new("Refresh", Msg::Refresh).shortcut(Shortcut::cmd('r')));
        MenuBar::standard(&self.info())
            .replace("Uta", app)
            .insert(1, playback)
            .with_menu(view)
            .replace(
                aizome_kit::menu::HELP,
                Menu::new("Help").item(MenuItem::new("Uta on GitHub", Msg::OpenSource)),
            )
    }
}

impl Drop for Uta {
    fn drop(&mut self) {
        self.control(Control::Quit);
        self.save();
        self.save_sender.take();
        if let Some(thread) = self.save_thread.take() {
            let _ = thread.join();
        }
    }
}

fn safe_artwork_url(url: &str) -> bool {
    let Some(host) = url
        .strip_prefix("https://")
        .and_then(|u| u.split('/').next())
    else {
        return false;
    };
    ["ytimg.com", "googleusercontent.com", "ggpht.com"]
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
}

fn stream_description(stream: &Stream) -> String {
    let rate = if stream.bitrate > 0.0 {
        format!(" · {:.0} kbps", stream.bitrate)
    } else {
        String::new()
    };
    format!(
        "{}{rate}{}",
        stream.codec.to_uppercase(),
        if stream.authenticated {
            " · Account playback"
        } else {
            ""
        }
    )
}
