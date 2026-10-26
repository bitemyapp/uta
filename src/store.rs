use crate::model::{Queue, valid_video_id};
use serde::{Deserialize, Serialize};
use std::{io::Write, path::PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    pub queue: Queue,
    pub volume: f64,
}

impl Default for Saved {
    fn default() -> Self {
        Self {
            queue: Queue::default(),
            volume: 70.0,
        }
    }
}

#[derive(Clone)]
pub struct Store(pub PathBuf);

impl Store {
    pub fn user() -> Option<Self> {
        let base = std::env::var_os("XDG_STATE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
        Some(Self(base.join("aizome/uta/session.json")))
    }

    pub fn load(&self) -> Saved {
        let mut saved: Saved = std::fs::read(&self.0)
            .ok()
            .filter(|bytes| bytes.len() <= 8 * 1024 * 1024)
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        // A restored queue never starts playing by itself.
        saved.queue.tracks.truncate(5000);
        if saved.queue.tracks.iter().any(|t| !valid_video_id(&t.id)) {
            saved.queue = Queue::default();
        }
        if saved
            .queue
            .current
            .is_some_and(|i| i >= saved.queue.tracks.len())
        {
            saved.queue.current = None;
        }
        saved.volume = if saved.volume.is_finite() {
            saved.volume.clamp(0.0, 100.0)
        } else {
            70.0
        };
        saved
    }

    pub fn save(&self, saved: &Saved) -> Result<(), String> {
        let dir = self.0.parent().ok_or("Invalid state path")?;
        std::fs::create_dir_all(dir).map_err(|_| "Could not create Uta's state directory")?;
        let mut file =
            tempfile::NamedTempFile::new_in(dir).map_err(|_| "Could not save the queue")?;
        serde_json::to_writer(&mut file, saved).map_err(|_| "Could not encode the queue")?;
        file.flush().map_err(|_| "Could not save the queue")?;
        file.as_file()
            .sync_all()
            .map_err(|_| "Could not save the queue")?;
        file.persist(&self.0)
            .map_err(|_| "Could not replace the saved queue")?;
        Ok(())
    }
}
