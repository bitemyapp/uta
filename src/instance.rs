//! The lock is held for the lifetime of the app, including while its socket is
//! replaced after a crash. A competing launch never unlinks a live socket.
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{OpenOptionsExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::PathBuf,
    time::Duration,
};

pub struct Instance {
    pub listener: UnixListener,
    _lock: File,
    path: PathBuf,
}
pub enum Claim {
    First(Instance),
    Handed,
}

impl Drop for Instance {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn claim(args: &[String]) -> Result<Claim, String> {
    claim_profile(args, "default")
}

pub fn claim_profile(args: &[String], profile: &str) -> Result<Claim, String> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or("XDG_RUNTIME_DIR is missing. Launch Uta inside your desktop session.")?;
    claim_profile_in(dir, args, profile)
}

pub fn claim_in(dir: PathBuf, args: &[String]) -> Result<Claim, String> {
    claim_profile_in(dir, args, "default")
}

pub fn claim_profile_in(dir: PathBuf, args: &[String], profile: &str) -> Result<Claim, String> {
    if !matches!(
        profile,
        "default" | "anonymous" | "python" | "python-anonymous"
    ) {
        return Err("Invalid instance profile".into());
    }
    let name = if profile == "default" {
        "aizome-app.uta".into()
    } else {
        format!("aizome-app.uta-{profile}")
    };
    let path = dir.join(format!("{name}.sock"));
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(dir.join(format!("{name}.lock")))
        .map_err(|_| "Could not open Uta's instance lock")?;
    // SAFETY: this is a live descriptor owned by `lock`; flock does not retain
    // pointers and the lock is released when the File drops.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        for _ in 0..50 {
            if let Ok(mut socket) = UnixStream::connect(&path) {
                socket.set_write_timeout(Some(Duration::from_secs(1))).ok();
                let mut message = serde_json::to_vec(args).map_err(|_| "Invalid launch request")?;
                message.push(b'\n');
                socket
                    .write_all(&message)
                    .map_err(|_| "Could not activate the running Uta")?;
                return Ok(Claim::Handed);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        return Err("Uta is already running but could not be reached.".into());
    }
    if path.exists() {
        std::fs::remove_file(&path).map_err(|_| "Could not replace Uta's stale socket")?;
    }
    let listener =
        UnixListener::bind(&path).map_err(|_| "Could not create Uta's activation socket")?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .map_err(|_| "Could not protect Uta's socket")?;
    Ok(Claim::First(Instance {
        listener,
        _lock: lock,
        path,
    }))
}

pub fn serve(listener: UnixListener, notify: impl Fn(Vec<String>) -> bool + Send + 'static) {
    std::thread::spawn(move || {
        for socket in listener.incoming() {
            let Ok(socket) = socket else { break };
            socket
                .set_read_timeout(Some(Duration::from_millis(500)))
                .ok();
            let mut buffer = Vec::new();
            if socket.take(8192).read_to_end(&mut buffer).is_ok()
                && let Ok(args) = serde_json::from_slice::<Vec<String>>(&buffer)
                && !notify(args)
            {
                break;
            }
        }
    });
}
