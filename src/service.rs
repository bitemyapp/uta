use crate::model::{Collection, Source, Stream, Track};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

/// Text that may hold a session's cookies: sent to the helper, never printed.
#[derive(Clone, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Secret(pub String);

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Secret({} bytes)", self.0.len())
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Status,
    /// Internal, pipe-only handoff from the keyring helper to the native client.
    NativeSession,
    /// Look through the browsers' profiles for YouTube sign-ins.
    Discover,
    /// Connect with a profile from `Discover`, or `folder:<path>`.
    Connect {
        source: String,
    },
    /// Connect with a request or cookies.txt copied from a browser.
    ConnectText {
        text: Secret,
    },
    Disconnect,
    Search {
        query: String,
        filter: String,
    },
    Liked,
    Playlists,
    Albums,
    History,
    Collection {
        id: String,
        kind: String,
    },
    /// A song's "up next" list, or a radio from it.
    Watch {
        id: String,
        radio: bool,
    },
    Resolve {
        id: String,
    },
    /// Retry a native stream rejected by the audio transport, once via yt-dlp.
    ResolveFallback {
        id: String,
    },
    Rate {
        id: String,
        like: bool,
    },
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Reply {
    /// Which adapter answered, for diagnostics and latency measurements.
    pub backend: String,
    pub connected: bool,
    pub account: String,
    pub message: String,
    /// Where the saved session came from, and its identity to reconnect with.
    pub source: String,
    pub source_id: String,
    pub sources: Vec<Source>,
    /// The service's clock when it answered, for "used 5 minutes ago".
    pub now: f64,
    pub title: String,
    pub tracks: Vec<Track>,
    pub collections: Vec<Collection>,
    pub stream: Option<Stream>,
    pub credentials: Option<NativeCredentials>,
}

/// Deliberately redacted when a Reply is logged. Never persisted by RustyPipe.
#[derive(Clone, Debug, Deserialize)]
pub struct NativeCredentials {
    pub cookie: Secret,
    pub authuser: String,
    pub expires: u64,
}

/// This boundary prevents *all* account I/O in the comparison profile,
/// including account discovery, connecting and disconnecting.
pub struct IgnoreCredentials(pub Arc<dyn Service>);
impl Service for IgnoreCredentials {
    fn call(&self, request: Request, _authenticated: bool) -> Result<Reply, String> {
        match request {
            Request::Status | Request::Discover => Ok(Reply {
                message: "Saved credentials are ignored in this window.".into(),
                ..Reply::default()
            }),
            Request::Connect { .. }
            | Request::ConnectText { .. }
            | Request::Disconnect
            | Request::NativeSession
            | Request::Liked
            | Request::Playlists
            | Request::Albums
            | Request::History
            | Request::Rate { .. } => {
                Err("Account actions are disabled by --ignore-credentials.".into())
            }
            request => self.0.call(request, false),
        }
    }
}

pub trait Service: Send + Sync {
    fn call(&self, request: Request, authenticated: bool) -> Result<Reply, String>;
    fn inline(&self) -> bool {
        false
    }
}

/// The most songs the helper returns for a library list or collection.
pub const SONG_LIMIT: usize = 1000;

/// The largest request the helper reads; pasted cookies are the large ones.
pub const REQUEST_LIMIT: usize = 1024 * 1024;

pub struct Live;

impl Service for Live {
    fn call(&self, request: Request, authenticated: bool) -> Result<Reply, String> {
        let request = match request {
            Request::ResolveFallback { id } => Request::Resolve { id },
            request => request,
        };
        let python = std::env::var_os("UTA_PYTHON").unwrap_or_else(|| "python3".into());
        let mut command = Command::new(python);
        crate::process::kill_with_parent(&mut command);
        let mut request = serde_json::to_value(&request).map_err(|_| "Could not encode request")?;
        request["authenticated"] = authenticated.into();
        let input = serde_json::to_vec(&request).map_err(|_| "Could not encode request")?;
        if input.len() > REQUEST_LIMIT {
            return Err("That is too much to send to the music service.".into());
        }
        let mut child = command
            .args(["-c", include_str!("../service/uta_service.py")])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| {
                "Could not start the music service. Run Uta through its Nix package or development shell."
            })?;
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
            Ok(Reply {
                backend: "python".into(),
                ..response.data
            })
        }
    }
}

/// Explicitly selected demo data; it never contacts YouTube, the keyring or
/// a browser.
pub struct Demo;

impl Service for Demo {
    fn inline(&self) -> bool {
        true
    }
    fn call(&self, request: Request, _authenticated: bool) -> Result<Reply, String> {
        match request {
            Request::Status => Ok(Reply::default()),
            Request::Discover => Ok(Reply {
                sources: demo_sources(),
                now: DEMO_NOW,
                ..Reply::default()
            }),
            Request::Search { .. }
            | Request::Liked
            | Request::History
            | Request::Collection { .. }
            | Request::Watch { .. } => Ok(Reply {
                title: "Evening listening".into(),
                tracks: demo_tracks(),
                ..Reply::default()
            }),
            Request::Playlists | Request::Albums => Ok(Reply {
                collections: vec![Collection {
                    id: "demo".into(),
                    title: "Evening listening".into(),
                    subtitle: "A quiet place to begin".into(),
                    kind: "playlist".into(),
                }],
                ..Reply::default()
            }),
            Request::Resolve { .. } | Request::ResolveFallback { .. } => {
                Err("Demo mode shows the interface without playing audio.".into())
            }
            _ => Err("Account actions are unavailable in demo mode.".into()),
        }
    }
}

/// A fixed clock for the demo's fictional browsers, so its pictures don't age.
pub const DEMO_NOW: f64 = 1_793_000_000.0;

pub fn demo_sources() -> Vec<Source> {
    vec![
        Source {
            id: "chromium:demo".into(),
            browser: "Chromium".into(),
            profile: "Person 1".into(),
            signed_in: true,
            used: DEMO_NOW - 240.0,
        },
        Source {
            id: "gecko:demo".into(),
            browser: "Firefox".into(),
            profile: "default-release".into(),
            signed_in: true,
            used: DEMO_NOW - 86400.0 * 3.0,
        },
        Source {
            id: "gecko:demo-work".into(),
            browser: "Firefox".into(),
            profile: "work".into(),
            signed_in: false,
            used: 0.0,
        },
    ]
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
    configured(
        std::env::var("UTA_SERVICE").as_deref() == Ok("python"),
        false,
    )
}

pub fn configured(python: bool, ignore_credentials: bool) -> Arc<dyn Service> {
    let service = backend(python);
    if ignore_credentials {
        Arc::new(IgnoreCredentials(service))
    } else {
        service
    }
}

fn backend(_python: bool) -> Arc<dyn Service> {
    #[cfg(feature = "rustypipe")]
    if !_python {
        match crate::native_service::Native::new() {
            Ok(native) => return Arc::new(native),
            Err(_) => log::warn!("Native music service unavailable; using Python"),
        }
    }
    Arc::new(Live)
}
