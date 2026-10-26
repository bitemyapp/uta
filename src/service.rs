use crate::model::{Collection, Stream, Track};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Status,
    Connect { browser: String, profile: String },
    Disconnect,
    Search { query: String, filter: String },
    Liked,
    Playlists,
    Collection { id: String, kind: String },
    Resolve { id: String },
    Rate { id: String, like: bool },
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Reply {
    pub connected: bool,
    pub account: String,
    pub message: String,
    pub title: String,
    pub tracks: Vec<Track>,
    pub collections: Vec<Collection>,
    pub stream: Option<Stream>,
}

pub trait Service: Send + Sync {
    fn call(&self, request: Request, authenticated: bool) -> Result<Reply, String>;
    fn inline(&self) -> bool {
        false
    }
}

pub struct Live;

impl Service for Live {
    fn call(&self, request: Request, authenticated: bool) -> Result<Reply, String> {
        let python = std::env::var_os("UTA_PYTHON").unwrap_or_else(|| "python3".into());
        let mut command = Command::new(python);
        crate::process::kill_with_parent(&mut command);
        let mut child = command
            .args(["-c", include_str!("../service/uta_service.py")])
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
            .spawn().map_err(|_| "Could not start the music service. Run Uta through its Nix package or development shell.")?;
        let mut request = serde_json::to_value(&request).map_err(|_| "Could not encode request")?;
        request["authenticated"] = authenticated.into();
        let input = serde_json::to_vec(&request).map_err(|_| "Could not encode request")?;
        let sent = child
            .stdin
            .take()
            .ok_or("Music service input is unavailable")?
            .write_all(&input);
        if sent.is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Could not send the music request".into());
        }
        let stdout = child
            .stdout
            .take()
            .ok_or("Music service output is unavailable")?;
        let reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            stdout
                .take(16 * 1024 * 1024)
                .read_to_end(&mut bytes)
                .map(|_| bytes)
        });
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Err(_) => break Err("Music service stopped unexpectedly"),
                _ if started.elapsed() > Duration::from_secs(120) => {
                    break Err("The request timed out. Check your connection and try again.");
                }
                _ => std::thread::sleep(Duration::from_millis(30)),
            }
        };
        if status.is_err() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let bytes = reader
            .join()
            .map_err(|_| "Music service output failed")?
            .map_err(|_| "Could not read the music response")?;
        status?;
        #[derive(Deserialize)]
        struct Envelope {
            #[serde(default)]
            data: Reply,
            #[serde(default)]
            error: Option<String>,
        }
        let response: Envelope = serde_json::from_slice(&bytes)
            .map_err(|_| "The music service returned an invalid response. Check that its dependencies are installed.")?;
        if let Some(error) = response.error {
            Err(error)
        } else {
            Ok(response.data)
        }
    }
}

/// Explicitly selected demo data; it never contacts YouTube or the keyring.
pub struct Demo;

impl Service for Demo {
    fn inline(&self) -> bool {
        true
    }
    fn call(&self, request: Request, _authenticated: bool) -> Result<Reply, String> {
        match request {
            Request::Status => Ok(Reply::default()),
            Request::Search { .. } | Request::Liked | Request::Collection { .. } => Ok(Reply {
                title: "Evening listening".into(),
                tracks: demo_tracks(),
                ..Reply::default()
            }),
            Request::Playlists => Ok(Reply {
                collections: vec![Collection {
                    id: "demo".into(),
                    title: "Evening listening".into(),
                    subtitle: "A quiet place to begin".into(),
                    kind: "playlist".into(),
                }],
                ..Reply::default()
            }),
            Request::Resolve { .. } => {
                Err("Demo mode shows the interface without playing audio.".into())
            }
            _ => Err("Account actions are unavailable in demo mode.".into()),
        }
    }
}

pub fn demo_tracks() -> Vec<Track> {
    [
        ("Soft light", "The Indigo Quartet", "After the rain", 248.0),
        (
            "A window left open",
            "Mika & the Pines",
            "Small hours",
            192.0,
        ),
        ("Blue hour", "Jun Arai", "Between stations", 315.0),
        (
            "Paper lanterns",
            "The Indigo Quartet",
            "After the rain",
            226.0,
        ),
        ("Passing trains", "Nami", "Somewhere nearby", 284.0),
        (
            "The long way home",
            "Mika & the Pines",
            "Small hours",
            201.0,
        ),
    ]
    .iter()
    .enumerate()
    .map(|(i, (title, artists, album, duration))| Track {
        id: format!("demo{:07}", i),
        title: (*title).into(),
        artists: (*artists).into(),
        album: (*album).into(),
        duration: *duration,
        ..Track::default()
    })
    .collect()
}

pub fn live() -> Arc<dyn Service> {
    Arc::new(Live)
}
