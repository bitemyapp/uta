//! Standard Linux media controls. D-Bus runs off the UI thread and sees only
//! track metadata; authenticated stream URLs and cookies never reach MPRIS.
use crate::model::Track;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, mpsc},
};
use zbus::{
    blocking::Connection,
    zvariant::{ObjectPath, OwnedValue, Str, Value},
};

const PATH: &str = "/org/mpris/MediaPlayer2";
const INTERFACE: &str = "org.mpris.MediaPlayer2.Player";

#[derive(Clone, Debug)]
pub enum Action {
    Play,
    Pause,
    Toggle,
    Stop,
    Next,
    Previous,
    Seek(f64),
    Volume(f64),
    Repeat(crate::model::Repeat),
    Raise,
    Quit,
}

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub track: Option<Track>,
    pub status: String,
    pub position: f64,
    pub duration: f64,
    pub volume: f64,
    pub can_next: bool,
    pub can_previous: bool,
    pub repeat: crate::model::Repeat,
    pub seek_generation: u64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            track: None,
            status: "Stopped".into(),
            position: 0.0,
            duration: 0.0,
            volume: 0.7,
            can_next: false,
            can_previous: false,
            repeat: crate::model::Repeat::Off,
            seek_generation: 0,
        }
    }
}

type Notify = Arc<dyn Fn(Action) + Send + Sync>;
struct Root(Notify);

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {
        (self.0)(Action::Raise);
    }
    fn quit(&self) {
        (self.0)(Action::Quit);
    }
    #[zbus(property)]
    fn can_quit(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_raise(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn identity(&self) -> &str {
        "Uta"
    }
    #[zbus(property)]
    fn desktop_entry(&self) -> &str {
        "org.aizome.Uta"
    }
    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        vec![]
    }
    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        vec![]
    }
}

struct Transport {
    state: Arc<Mutex<State>>,
    notify: Notify,
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Transport {
    fn next(&self) {
        if self.state.lock().unwrap().can_next {
            (self.notify)(Action::Next);
        }
    }
    fn previous(&self) {
        if self.state.lock().unwrap().can_previous {
            (self.notify)(Action::Previous);
        }
    }
    fn pause(&self) {
        (self.notify)(Action::Pause);
    }
    fn play_pause(&self) {
        (self.notify)(Action::Toggle);
    }
    fn stop(&self) {
        (self.notify)(Action::Stop);
    }
    fn play(&self) {
        if self.state.lock().unwrap().track.is_some() {
            (self.notify)(Action::Play);
        }
    }
    fn seek(&self, offset: i64) {
        let state = self.state.lock().unwrap();
        if state.status == "Stopped" {
            return;
        }
        let position = (state.position + offset as f64 / 1_000_000.0).max(0.0);
        if position > state.duration {
            if state.can_next {
                (self.notify)(Action::Next);
            }
        } else {
            (self.notify)(Action::Seek(position));
        }
    }
    fn set_position(&self, track_id: ObjectPath<'_>, position: i64) {
        let state = self.state.lock().unwrap();
        if state
            .track
            .as_ref()
            .is_some_and(|t| track_path(t) == track_id.as_str())
            && position >= 0
            && position as f64 / 1_000_000.0 <= state.duration
            && state.status != "Stopped"
        {
            (self.notify)(Action::Seek(position as f64 / 1_000_000.0));
        }
    }
    fn open_uri(&self, _uri: &str) -> zbus::fdo::Result<()> {
        Err(zbus::fdo::Error::NotSupported(
            "Use Uta's music search to open a song".into(),
        ))
    }
    #[zbus(property)]
    fn playback_status(&self) -> String {
        self.state.lock().unwrap().status.clone()
    }
    #[zbus(property)]
    fn loop_status(&self) -> &str {
        repeat_name(&self.state.lock().unwrap().repeat)
    }
    #[zbus(property)]
    fn set_loop_status(&self, value: &str) -> zbus::fdo::Result<()> {
        use crate::model::Repeat;
        let repeat = match value {
            "None" => Repeat::Off,
            "Track" => Repeat::One,
            "Playlist" => Repeat::All,
            _ => return Err(zbus::fdo::Error::InvalidArgs("Unknown repeat mode".into())),
        };
        (self.notify)(Action::Repeat(repeat));
        Ok(())
    }
    #[zbus(signal)]
    async fn seeked(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        position: i64,
    ) -> zbus::Result<()>;
    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn set_rate(&self, rate: f64) -> zbus::fdo::Result<()> {
        if rate == 0.0 {
            (self.notify)(Action::Pause);
            Ok(())
        } else if rate == 1.0 {
            Ok(())
        } else {
            Err(zbus::fdo::Error::NotSupported(
                "Uta plays at normal speed".into(),
            ))
        }
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        metadata(&self.state.lock().unwrap())
    }
    #[zbus(property)]
    fn volume(&self) -> f64 {
        self.state.lock().unwrap().volume
    }
    #[zbus(property)]
    fn set_volume(&self, volume: f64) {
        if volume.is_finite() {
            (self.notify)(Action::Volume(volume.clamp(0.0, 1.0) * 100.0));
        }
    }
    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        (self.state.lock().unwrap().position * 1_000_000.0) as i64
    }
    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        self.state.lock().unwrap().can_next
    }
    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        self.state.lock().unwrap().can_previous
    }
    #[zbus(property)]
    fn can_play(&self) -> bool {
        self.state.lock().unwrap().track.is_some()
    }
    #[zbus(property)]
    fn can_pause(&self) -> bool {
        self.state.lock().unwrap().status != "Stopped"
    }
    #[zbus(property)]
    fn can_seek(&self) -> bool {
        self.state.lock().unwrap().status != "Stopped"
    }
    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

fn track_path(track: &Track) -> String {
    let encoded: String = track.id.bytes().map(|byte| format!("{byte:02x}")).collect();
    format!("/org/aizome/Uta/track/v{encoded}")
}

fn repeat_name(repeat: &crate::model::Repeat) -> &'static str {
    use crate::model::Repeat;
    match repeat {
        Repeat::Off => "None",
        Repeat::All => "Playlist",
        Repeat::One => "Track",
    }
}

fn metadata(state: &State) -> HashMap<String, OwnedValue> {
    let Some(track) = &state.track else {
        return HashMap::new();
    };
    let mut values = HashMap::from([
        (
            "mpris:trackid".into(),
            ObjectPath::try_from(track_path(track)).unwrap().into(),
        ),
        (
            "mpris:length".into(),
            ((state.duration * 1_000_000.0) as i64).into(),
        ),
        ("xesam:title".into(), Str::from(track.title.clone()).into()),
        ("xesam:album".into(), Str::from(track.album.clone()).into()),
        (
            "xesam:artist".into(),
            Value::from(vec![track.artists.clone()]).try_into().unwrap(),
        ),
        (
            "xesam:url".into(),
            Str::from(format!("https://music.youtube.com/watch?v={}", track.id)).into(),
        ),
    ]);
    if track.thumbnail.starts_with("https://") {
        values.insert(
            "mpris:artUrl".into(),
            Str::from(track.thumbnail.clone()).into(),
        );
    }
    values
}

pub struct Mpris {
    updates: mpsc::Sender<State>,
}

impl Mpris {
    pub fn start(notify: impl Fn(Action) + Send + Sync + 'static) -> Self {
        let (updates, receiver) = mpsc::channel::<State>();
        let notify: Notify = Arc::new(notify);
        std::thread::spawn(move || {
            // Publish a complete initial state before acquiring the bus name.
            // Clients may cache their very first GetAll response immediately.
            let Ok(initial) = receiver.recv() else { return };
            let state = Arc::new(Mutex::new(initial));
            let connection = match connect(state.clone(), notify) {
                Ok(connection) => connection,
                Err(error) => {
                    log::warn!("Desktop media controls are unavailable: {error}");
                    return;
                }
            };
            while let Ok(mut newest) = receiver.recv() {
                while let Ok(newer) = receiver.try_recv() {
                    newest = newer;
                }
                let previous = std::mem::replace(&mut *state.lock().unwrap(), newest.clone());
                let mut changed: HashMap<&str, OwnedValue> = HashMap::new();
                if previous.status != newest.status {
                    changed.insert("PlaybackStatus", Str::from(newest.status.clone()).into());
                    changed.insert("CanPause", (newest.status != "Stopped").into());
                    changed.insert("CanSeek", (newest.status != "Stopped").into());
                }
                if previous.volume != newest.volume {
                    changed.insert("Volume", newest.volume.into());
                }
                if previous.track != newest.track || previous.duration != newest.duration {
                    changed.insert(
                        "Metadata",
                        Value::from(metadata(&newest)).try_into().unwrap(),
                    );
                    changed.insert("CanPlay", newest.track.is_some().into());
                }
                if previous.can_next != newest.can_next {
                    changed.insert("CanGoNext", newest.can_next.into());
                }
                if previous.can_previous != newest.can_previous {
                    changed.insert("CanGoPrevious", newest.can_previous.into());
                }
                if !changed.is_empty() {
                    let _ = connection.emit_signal(
                        None::<&str>,
                        PATH,
                        "org.freedesktop.DBus.Properties",
                        "PropertiesChanged",
                        &(INTERFACE, changed, Vec::<String>::new()),
                    );
                }
                if previous.repeat != newest.repeat {
                    let changed = HashMap::from([(
                        "LoopStatus",
                        OwnedValue::from(Str::from(repeat_name(&newest.repeat))),
                    )]);
                    let _ = connection.emit_signal(
                        None::<&str>,
                        PATH,
                        "org.freedesktop.DBus.Properties",
                        "PropertiesChanged",
                        &(INTERFACE, changed, Vec::<String>::new()),
                    );
                }
                if previous.seek_generation != newest.seek_generation {
                    let _ = connection.emit_signal(
                        None::<&str>,
                        PATH,
                        INTERFACE,
                        "Seeked",
                        &((newest.position * 1_000_000.0) as i64,),
                    );
                }
            }
        });
        Self { updates }
    }

    pub fn publish(&self, state: State) {
        let _ = self.updates.send(state);
    }
}

fn connect(state: Arc<Mutex<State>>, notify: Notify) -> zbus::Result<Connection> {
    zbus::blocking::connection::Builder::session()?
        .name("org.mpris.MediaPlayer2.uta")?
        .serve_at(PATH, Root(notify.clone()))?
        .serve_at(PATH, Transport { state, notify })?
        .build()
}
