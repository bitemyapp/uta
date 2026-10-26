//! A supervised mpv process controlled over a private Unix socket. Every load
//! has its own process and generation, so a superseded stream cannot start late.
use crate::model::Stream;
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub enum Control {
    Load {
        generation: u64,
        stream: Stream,
        volume: f64,
        paused: bool,
    },
    Pause(bool),
    Seek(f64),
    Volume(f64),
    Stop,
    Quit,
}

#[derive(Clone, Debug)]
pub enum EventKind {
    Started,
    Position(f64),
    Duration(f64),
    Paused(bool),
    Ended,
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct Event {
    pub generation: u64,
    pub kind: EventKind,
}

pub struct Player {
    sender: Sender<Control>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Player {
    pub fn start(notify: impl Fn(Event) + Send + 'static, null_audio: bool) -> Self {
        let (sender, receiver) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("uta-player".into())
            .spawn(move || worker(receiver, notify, null_audio))
            .expect("start audio worker");
        Self {
            sender,
            thread: Some(thread),
        }
    }

    pub fn send(&self, command: Control) {
        let _ = self.sender.send(command);
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.sender.send(Control::Quit);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Session {
    child: Child,
    socket: UnixStream,
    _directory: tempfile::TempDir,
    buffer: Vec<u8>,
    generation: u64,
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Session {
    fn launch(
        generation: u64,
        stream: Stream,
        volume: f64,
        paused: bool,
        null_audio: bool,
    ) -> Result<Self, String> {
        let mut builder = tempfile::Builder::new();
        builder.prefix("uta-player-");
        let directory = std::env::var_os("XDG_RUNTIME_DIR")
            .and_then(|dir| builder.tempdir_in(dir).ok())
            .or_else(|| builder.tempdir().ok())
            .ok_or("Could not create the player's private socket directory")?;
        let path = directory.path().join("mpv.sock");
        let mut command = Command::new(std::env::var_os("UTA_MPV").unwrap_or_else(|| "mpv".into()));
        crate::process::kill_with_parent(&mut command);
        command
            .args([
                "--no-config",
                "--idle=yes",
                "--no-terminal",
                "--really-quiet",
                "--no-video",
                "--audio-display=no",
                "--ytdl=no",
                "--audio-client-name=Uta",
            ])
            .arg(format!("--input-ipc-server={}", path.display()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if null_audio {
            command.arg("--ao=null");
        }
        let mut child = command
            .spawn()
            .map_err(|_| "Could not start mpv. Install mpv or use Uta's Nix package.")?;
        let started = Instant::now();
        let socket = loop {
            if let Ok(socket) = UnixStream::connect(&path) {
                break socket;
            }
            if started.elapsed() > Duration::from_secs(4)
                || child.try_wait().ok().flatten().is_some()
            {
                let _ = child.kill();
                let _ = child.wait();
                return Err(
                    "The audio player did not start. Check mpv and your audio service.".into(),
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let mut session = Self {
            child,
            socket,
            _directory: directory,
            buffer: Vec::new(),
            generation,
        };
        session
            .socket
            .set_read_timeout(Some(Duration::from_millis(20)))
            .map_err(|_| "Could not configure audio IPC")?;
        session
            .socket
            .set_write_timeout(Some(Duration::from_millis(500)))
            .map_err(|_| "Could not configure audio IPC")?;
        session.command(json!(["set_property", "volume", volume.clamp(0.0, 100.0)]))?;
        // Use mpv's native list property instead of escaping headers into a
        // command-line string. Session cookies never leave the service helper.
        let headers: Vec<String> = stream
            .headers
            .iter()
            .filter(|(key, value)| {
                !key.contains(['\r', '\n', ':']) && !value.contains(['\r', '\n'])
            })
            .filter(|(key, _)| {
                ["user-agent", "referer", "origin", "accept"]
                    .contains(&key.to_ascii_lowercase().as_str())
            })
            .map(|(key, value)| format!("{key}: {value}"))
            .collect();
        session.command(json!(["set_property", "http-header-fields", headers]))?;
        for (id, property) in [(1, "time-pos"), (2, "duration"), (3, "pause")] {
            session.command(json!(["observe_property", id, property]))?;
        }
        session.command(json!(["set_property", "pause", paused]))?;
        session.command(json!(["loadfile", stream.url, "replace"]))?;
        Ok(session)
    }

    fn command(&mut self, command: Value) -> Result<(), String> {
        // Only JSON on the private socket contains the expiring stream URL.
        let mut line = serde_json::to_vec(&json!({"command": command}))
            .map_err(|_| "Invalid player command")?;
        line.push(b'\n');
        self.socket
            .write_all(&line)
            .map_err(|_| "The audio player stopped responding".into())
    }

    fn poll(&mut self) -> Result<Vec<EventKind>, String> {
        let mut bytes = [0; 8192];
        match self.socket.read(&mut bytes) {
            Ok(0) => return Err("The audio player exited unexpectedly".into()),
            Ok(n) => self.buffer.extend_from_slice(&bytes[..n]),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return Err("Lost contact with the audio player".into()),
        }
        if self.buffer.len() > 1024 * 1024 {
            return Err("Invalid response from the audio player".into());
        }
        let mut events = Vec::new();
        while let Some(end) = self.buffer.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=end).collect();
            let Ok(value) = serde_json::from_slice::<Value>(&line) else {
                continue;
            };
            match value["event"].as_str() {
                Some("file-loaded") => events.push(EventKind::Started),
                Some("property-change") => match value["name"].as_str() {
                    Some("time-pos") => if let Some(v) = value["data"].as_f64() { events.push(EventKind::Position(v)); },
                    Some("duration") => if let Some(v) = value["data"].as_f64() { events.push(EventKind::Duration(v)); },
                    Some("pause") => if let Some(v) = value["data"].as_bool() { events.push(EventKind::Paused(v)); },
                    _ => {},
                },
                Some("end-file") => match value["reason"].as_str() {
                    Some("eof") => events.push(EventKind::Ended),
                    Some("error") => events.push(EventKind::Failed("This stream could not be played. Try playing the song again to request a fresh stream.".into())),
                    _ => {},
                },
                _ => if let Some(error) = value["error"].as_str() && error != "success" {
                    return Err("The audio player could not apply a playback command".into());
                },
            }
        }
        Ok(events)
    }
}

fn worker(receiver: Receiver<Control>, notify: impl Fn(Event), null_audio: bool) {
    let mut session: Option<Session> = None;
    loop {
        let command = if session.is_none() {
            match receiver.recv() {
                Ok(c) => Some(c),
                Err(_) => break,
            }
        } else {
            match receiver.try_recv() {
                Ok(c) => Some(c),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        };
        match command {
            Some(Control::Quit) => break,
            Some(Control::Stop) => {
                session = None;
            }
            Some(Control::Load {
                generation,
                stream,
                volume,
                paused,
            }) => {
                session = None;
                match Session::launch(generation, stream, volume, paused, null_audio) {
                    Ok(player) => session = Some(player),
                    Err(error) => notify(Event {
                        generation,
                        kind: EventKind::Failed(error),
                    }),
                }
            }
            Some(command) => {
                if let Some(player) = session.as_mut() {
                    let json = match command {
                        Control::Pause(paused) => json!(["set_property", "pause", paused]),
                        Control::Seek(seconds) => json!(["seek", seconds.max(0.0), "absolute"]),
                        Control::Volume(volume) => {
                            json!(["set_property", "volume", volume.clamp(0.0, 100.0)])
                        }
                        _ => unreachable!(),
                    };
                    if let Err(error) = player.command(json) {
                        notify(Event {
                            generation: player.generation,
                            kind: EventKind::Failed(error),
                        });
                        session = None;
                    }
                }
            }
            None => {}
        }
        if let Some(player) = session.as_mut() {
            let generation = player.generation;
            match player.poll() {
                Ok(events) => {
                    let terminal = events
                        .iter()
                        .any(|e| matches!(e, EventKind::Ended | EventKind::Failed(_)));
                    for kind in events {
                        notify(Event { generation, kind });
                    }
                    if terminal {
                        session = None;
                    }
                }
                Err(error) => {
                    notify(Event {
                        generation,
                        kind: EventKind::Failed(error),
                    });
                    session = None;
                }
            }
        }
    }
}
