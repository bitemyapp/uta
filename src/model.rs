use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artists: String,
    pub album: String,
    pub duration: f64,
    pub thumbnail: String,
    pub liked: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Collection {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub kind: String,
}

/// A browser profile the service found, and whether it holds a YouTube sign-in.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Source {
    pub id: String,
    pub browser: String,
    pub profile: String,
    pub signed_in: bool,
    /// When the profile last used YouTube, in Unix seconds (0 if unknown).
    pub used: f64,
}

impl Source {
    pub fn label(&self) -> String {
        if self.profile.is_empty() {
            self.browser.clone()
        } else {
            format!("{} · {}", self.browser, self.profile)
        }
    }
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
pub struct Stream {
    pub url: String,
    pub headers: std::collections::BTreeMap<String, String>,
    pub codec: String,
    pub bitrate: f64,
    pub format_id: String,
    pub client: String,
    pub authenticated: bool,
    /// A native URL can retry through the established extractor if mpv rejects it.
    pub native: bool,
}

impl std::fmt::Debug for Stream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stream")
            .field("codec", &self.codec)
            .field("bitrate", &self.bitrate)
            .field("authenticated", &self.authenticated)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

impl Repeat {
    pub fn cycle(&self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Off => "Repeat off",
            Self::All => "Repeat queue",
            Self::One => "Repeat song",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Queue {
    pub tracks: Vec<Track>,
    pub current: Option<usize>,
    pub repeat: Repeat,
}

impl Queue {
    pub fn track(&self) -> Option<&Track> {
        self.tracks.get(self.current?)
    }

    pub fn replace(&mut self, tracks: Vec<Track>, start: usize) {
        self.tracks = tracks;
        self.current = (start < self.tracks.len()).then_some(start);
    }

    pub fn advance(&mut self, finished: bool) -> bool {
        let Some(i) = self.current else { return false };
        if finished && self.repeat == Repeat::One {
            return true;
        }
        if i + 1 < self.tracks.len() {
            self.current = Some(i + 1);
            true
        } else if self.repeat == Repeat::All && !self.tracks.is_empty() {
            self.current = Some(0);
            true
        } else {
            false
        }
    }

    /// The song `advance(false)` would move to, without moving.
    pub fn peek_next(&self) -> Option<&Track> {
        let i = self.current?;
        if i + 1 < self.tracks.len() {
            self.tracks.get(i + 1)
        } else if self.repeat == Repeat::All {
            self.tracks.first()
        } else {
            None
        }
    }

    pub fn previous(&mut self) -> bool {
        match self.current {
            Some(i) if i > 0 => {
                self.current = Some(i - 1);
                true
            }
            Some(_) => true,
            None => false,
        }
    }

    /// Shuffle the upcoming songs; keep the song already playing first.
    pub fn shuffle(&mut self) {
        if let Some(i) = self.current.filter(|i| *i < self.tracks.len()) {
            self.tracks.swap(0, i);
            self.current = Some(0);
            self.tracks[1..].shuffle(&mut rand::rng());
        } else {
            self.tracks.shuffle(&mut rand::rng());
        }
    }

    /// Put a song right after the one playing (first, if none is).
    pub fn insert_next(&mut self, track: Track) {
        let at = self.current.map_or(0, |i| i + 1).min(self.tracks.len());
        self.tracks.insert(at, track);
    }

    /// Move a row, keeping the playing song current wherever it goes.
    pub fn move_track(&mut self, from: usize, to: usize) -> bool {
        if from >= self.tracks.len() || to >= self.tracks.len() || from == to {
            return false;
        }
        let track = self.tracks.remove(from);
        self.tracks.insert(to, track);
        self.current = self.current.map(|i| match i {
            i if i == from => to,
            i if from < i && i <= to => i - 1,
            i if to <= i && i < from => i + 1,
            i => i,
        });
        true
    }

    /// Returns whether removing the row also removed the playing song.
    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.tracks.len() {
            return false;
        }
        self.tracks.remove(index);
        let removed_current = self.current == Some(index);
        self.current = match self.current {
            Some(_) if self.tracks.is_empty() => None,
            Some(i) if index < i => Some(i - 1),
            Some(i) if i >= self.tracks.len() => Some(self.tracks.len() - 1),
            value => value,
        };
        removed_current
    }
}

pub fn time(seconds: f64) -> String {
    let seconds = if seconds.is_finite() {
        seconds.max(0.0) as u64
    } else {
        0
    };
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

/// When a signed stream URL stops working, in Unix seconds: YouTube writes it
/// into the query (`expire=…`) or, for manifests, the path (`/expire/…/`).
pub fn stream_expiry(url: &str) -> Option<f64> {
    let query = url.split_once('?').map_or("", |(_, q)| q);
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("expire="))
        .or_else(|| url.split("/expire/").nth(1)?.split('/').next())?
        .parse()
        .ok()
}

/// "just now", "5 minutes ago", "yesterday": how long before `now` a time was.
pub fn ago(then: f64, now: f64) -> String {
    let seconds = (now - then).max(0.0) as u64;
    let plural = |n: u64, unit: &str| format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
    match seconds {
        0..90 => "just now".into(),
        90..3600 => plural(seconds.div_ceil(60).min(59), "minute"),
        3600..86400 => plural(seconds / 3600, "hour"),
        86400..172800 => "yesterday".into(),
        _ => plural(seconds / 86400, "day"),
    }
}

pub fn valid_video_id(id: &str) -> bool {
    id.len() == 11 && id.bytes().all(id_byte)
}

fn valid_collection_id(id: &str) -> bool {
    (1..=160).contains(&id.len()) && id.bytes().all(id_byte)
}

fn id_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

/// What a YouTube or YouTube Music link points to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    Song(String),
    Playlist(String),
    Album(String),
}

/// Read a link pasted into search or passed on the command line:
/// `music.youtube.com/watch?v=…`, `…/playlist?list=…`, `…/browse/MPREb_…`
/// and `youtu.be/…`, with or without the scheme.
pub fn link(text: &str) -> Option<Link> {
    let text = text.trim();
    let rest = text
        .strip_prefix("https://")
        .or_else(|| text.strip_prefix("http://"))
        .unwrap_or(text);
    let (host, rest) = rest.split_once('/').unwrap_or((rest, ""));
    let rest = rest.split('#').next().unwrap_or_default();
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let param = |key: &str| {
        query.split('&').find_map(|pair| {
            pair.strip_prefix(key)
                .and_then(|value| value.strip_prefix('='))
                .filter(|value| !value.is_empty())
        })
    };
    let host = host.to_ascii_lowercase();
    let host = ["www.", "m."]
        .iter()
        .find_map(|p| host.strip_prefix(p))
        .unwrap_or(&host);
    let song = |id: &str| valid_video_id(id).then(|| Link::Song(id.into()));
    let playlist = |id: &str| valid_collection_id(id).then(|| Link::Playlist(id.into()));
    match host {
        "youtu.be" => song(path.trim_end_matches('/')),
        "youtube.com" | "music.youtube.com" => match path.trim_end_matches('/') {
            "watch" => param("v")
                .and_then(song)
                .or_else(|| param("list").and_then(playlist)),
            "playlist" => param("list").and_then(playlist),
            path => {
                let id = path.strip_prefix("browse/")?;
                if let Some(list) = id.strip_prefix("VL") {
                    playlist(list)
                } else {
                    (id.starts_with("MPREb_") && valid_collection_id(id))
                        .then(|| Link::Album(id.into()))
                }
            }
        },
        _ => None,
    }
}
