use crate::{
    instance::Instance,
    model::{Collection, Queue, Stream, Track},
    player::{Control, Event, EventKind, Player},
    service::{Reply, Request, Service},
    store::{Saved, Store},
};
use aizome_kit::prelude::*;
use std::{collections::HashSet, sync::Arc, time::Duration};

pub const APP_ID: &str = "org.aizome.Uta";
pub const BROWSERS: [&str; 7] = [
    "firefox", "chromium", "chrome", "brave", "edge", "vivaldi", "opera",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Search,
    Liked,
    Playlists,
    Collection,
    Queue,
    Account,
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
    ClearQueue,
    Resolved(u64, Result<Reply, String>),
    Player(Event),
    Artwork(u64, Vec<u8>),
    Browser(usize),
    Profile(String),
    Connect,
    Disconnect,
    Auth(u64, Result<Reply, String>),
    Like(String, bool),
    Rated(u64, String, bool, Result<Reply, String>),
    OpenLogin,
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
    pub browser: usize,
    pub profile: String,
    pub queue: Queue,
    pub volume: f64,
    pub playback: Playback,
    pause_on_start: bool,
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
            browser: 0,
            profile: String::new(),
            queue: saved.queue,
            volume: saved.volume,
            playback: Playback::Stopped,
            pause_on_start: false,
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
            Page::Search if !self.query.trim().is_empty() => self.load(
                Request::Search {
                    query: self.query.trim().into(),
                    filter: ["songs", "albums", "playlists"][self.filter].into(),
                },
                cx,
            ),
            Page::Liked if self.connected => self.load(Request::Liked, cx),
            Page::Playlists if self.connected => self.load(Request::Playlists, cx),
            Page::Collection => {
                if let Some(collection) = &self.collection {
                    self.load(
                        Request::Collection {
                            id: collection.id.clone(),
                            kind: collection.kind.clone(),
                        },
                        cx,
                    );
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
        self.tracks.clear();
        self.collections.clear();
        self.error = None;
        self.title = match page {
            Page::Search => "Search".into(),
            Page::Liked => "Liked songs".into(),
            Page::Playlists => "Your playlists".into(),
            Page::Queue => "Up next".into(),
            Page::Account => "Your account".into(),
            Page::Collection => self
                .collection
                .as_ref()
                .map(|c| c.title.clone())
                .unwrap_or_default(),
        };
        self.refresh(cx);
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
        self.request(Request::Resolve { id: track.id }, cx, move |reply| {
            Msg::Resolved(generation, reply)
        });
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
        if self.page == Page::Queue {
            self.queue.current = Some(index);
        } else {
            self.queue.replace(self.tracks.clone(), index);
        }
        self.play_current(cx);
    }

    fn auth(&mut self, request: Request, cx: &mut Cx<Msg>) {
        if self.account_busy {
            return;
        }
        self.stop();
        self.auth_generation += 1;
        self.content_generation += 1;
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
}

impl Program for Uta {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new(APP_ID, "Uta", "YouTube Music").icon(Icon::Headphones)
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
                self.filter = filter.min(2);
                if self.page == Page::Search && self.searched {
                    self.refresh(cx);
                }
            }
            Msg::Search => {
                if !self.query.trim().is_empty() {
                    self.navigate(Page::Search, cx);
                }
            }
            Msg::Refresh => self.refresh(cx),
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
            Msg::Enqueue(index) => {
                if self.queue.tracks.len() < 5000
                    && let Some(track) = self.visible_tracks().get(index).cloned()
                {
                    self.queue.tracks.push(track);
                    self.changed(cx);
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
            }
            Msg::SetRepeat(repeat) => {
                self.queue.repeat = repeat;
                self.changed(cx);
            }
            Msg::Shuffle => {
                self.queue.shuffle();
                self.changed(cx);
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
            }
            Msg::ClearQueue => {
                self.stop();
                self.queue.tracks.clear();
                self.queue.current = None;
                self.duration = 0.0;
                self.artwork = None;
                self.selected = None;
                self.changed(cx);
            }
            Msg::Resolved(generation, result) if generation == self.play_generation => match result
                .and_then(|reply| {
                    reply
                        .stream
                        .ok_or("YouTube returned no audio stream".into())
                }) {
                Ok(stream) => {
                    self.stream_label = stream_description(&stream);
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
            Msg::Player(event) if event.generation == self.play_generation => match event.kind {
                EventKind::Started => {
                    self.playback = if self.pause_on_start {
                        Playback::Paused
                    } else {
                        Playback::Playing
                    }
                }
                EventKind::Position(position) => self.position = position.max(0.0),
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
                    self.stop();
                    self.error = Some(error);
                }
                _ => {}
            },
            Msg::Artwork(generation, bytes) if generation == self.play_generation => {
                self.artwork = Image::decode(bytes);
            }
            Msg::Browser(browser) => self.browser = browser.min(BROWSERS.len() - 1),
            Msg::Profile(profile) => self.profile = profile,
            Msg::Connect => self.auth(
                Request::Connect {
                    browser: BROWSERS[self.browser].into(),
                    profile: self.profile.trim().into(),
                },
                cx,
            ),
            Msg::Disconnect => self.auth(Request::Disconnect, cx),
            Msg::Auth(generation, result) if generation == self.auth_generation => {
                self.account_busy = false;
                match result {
                    Ok(reply) => {
                        self.connected = reply.connected;
                        self.account = reply.account;
                        self.account_note = reply.message;
                        self.content_generation += 1;
                        if self.demo {
                            self.query = "Evening listening".into();
                        }
                        self.refresh(cx);
                    }
                    Err(error) => self.error = Some(error),
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
            Msg::OpenLogin => cx.open_url("https://music.youtube.com/"),
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
        let app = Menu::new("Uta")
            .item(MenuItem::role_item("About Uta", MenuRole::About))
            .item(MenuItem::new("Account…", Msg::Page(Page::Account)).shortcut(Shortcut::cmd(',')))
            .separator()
            .item(MenuItem::role_item("Hide Uta", MenuRole::Hide).shortcut(Shortcut::cmd('h')))
            .item(MenuItem::new("Quit Uta", Msg::Quit).shortcut(Shortcut::cmd('q')));
        let playback = Menu::new("Playback")
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
            .item(
                MenuItem::new("Play Selected", Msg::PlaySelected).enabled(self.selected.is_some()),
            )
            .item(
                MenuItem::new("Add Selected to Queue", Msg::EnqueueSelected)
                    .enabled(self.selected.is_some()),
            )
            .item(MenuItem::new("Shuffle Queue", Msg::Shuffle).enabled(self.queue.tracks.len() > 1))
            .item(MenuItem::new(self.queue.repeat.label(), Msg::Repeat));
        let view = Menu::new("View")
            .item(MenuItem::new("Search", Msg::FocusSearch).shortcut(Shortcut::cmd('f')))
            .item(MenuItem::new("Liked Songs", Msg::Page(Page::Liked)).shortcut(Shortcut::cmd('1')))
            .item(
                MenuItem::new("Playlists", Msg::Page(Page::Playlists)).shortcut(Shortcut::cmd('2')),
            )
            .item(MenuItem::new("Queue", Msg::Page(Page::Queue)).shortcut(Shortcut::cmd('3')))
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
