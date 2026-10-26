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

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
pub struct Stream {
    pub url: String,
    pub headers: std::collections::BTreeMap<String, String>,
    pub codec: String,
    pub bitrate: f64,
    pub authenticated: bool,
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

pub fn valid_video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
