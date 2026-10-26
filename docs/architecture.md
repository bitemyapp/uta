# Uta's architecture

The interface is a Rust `aizome-kit::Program`: messages update state and a
view describes the native widget tree. Network and keyring operations run
off the UI thread. Visible song rows are built through the kit's virtual
list. The window uses `org.aizome.Uta`, so Aizome recognizes its native command
key and exports its menus to the shell's menu bar and command palette.

## Service boundary

`service/uta_service.py` accepts a bounded JSON request on stdin and returns
one JSON response on stdout. Rust embeds this source into the executable.
The Nix wrapper chooses the Python interpreter and dependencies. The helper
uses `ytmusicapi` for search and libraries and `yt-dlp` for stream extraction;
Python does not draw any of the interface.

Each operation runs in its own process with a two-minute outer timeout and
shorter HTTP timeouts. Responses are capped at 16 MiB. Diagnostics from the
libraries are suppressed; only deliberately written errors cross into the
UI. Request generations discard stale content and playback replies after
navigation, skipping, stopping, or account changes.

The official Data API does not provide this music playback path. The adapters
use YouTube's unofficial web interfaces, with their attendant maintenance
requirements. Relevant upstream documentation:

- [ytmusicapi browser authentication](https://ytmusicapi.readthedocs.io/en/stable/setup/browser.html)
- [yt-dlp embedding](https://github.com/yt-dlp/yt-dlp#embedding-yt-dlp)
- [MPRIS](https://specifications.freedesktop.org/mpris-spec/latest/)

## Account session

The service reads browser cookie databases itself instead of using yt-dlp's
importer. That importer copies a database without its write-ahead log, so it
misses a sign-in that a running Firefox hasn't yet checkpointed. It also
chooses Chromium's keyring from the desktop's name, and on an unknown desktop
such as Aizome it picks the fixed "basic text" key. That fails for the
keyring-sealed cookies of Chrome and Chromium, which Sekka starts with
`--password-store=gnome-libsecret`.

- **Discovery** (`discover`) walks the known profile folders of the Firefox,
  Chromium, and WebKit families, including Flatpak and Snap ones. Profile
  names come from `profiles.ini` and `Local State`. For each database it reads
  only the host, name, expiry, and last-access columns of youtube.com cookies.
  Cookie values are never decrypted here, the keyring is never opened, and
  nothing prompts. A profile counts as signed in when unexpired
  `__Secure-3PAPISID` and `__Secure-3PSID` cookies are present.
- **Snapshots.** Every read copies the database with its `-wal` and
  `-journal` files into a private temporary folder in `$XDG_RUNTIME_DIR`
  (memory, not disk). It opens the copy and removes it straight away, so the
  browser's lock is never contended.
- **Partitions.** Firefox containers are separate sign-ins. Cookies
  partitioned under another site (YouTube embedded in its page) and those
  from private windows are excluded. Cookies partitioned under youtube.com
  itself are kept, because the browser sends them to music.youtube.com.
- **Decryption.** Chromium's `v10` values use the fixed key. Its `v11` values
  are tried against every "… Safe Storage" secret in the unlocked keyrings,
  and against KWallet on Plasma. From Chrome 130, the SHA-256 prefix of the
  host proves which key is right.
- **Other sources.** A chosen folder is matched to a known profile where
  possible. Pasted text is parsed as cookies.txt, a cURL command, or request
  headers; a pasted `x-goog-authuser` selects among signed-in accounts.

Connecting is always explicit, apart from one case: after **Sign in with your
browser**, Uta looks every four seconds, for about five minutes, and connects
the first sign-in that wasn't there before. Cookies are filtered by YouTube's
domain boundary and expiry, and a YouTube account request must succeed before
replacing the previous secret. The secret is stored using Secret Service,
matched by `application=org.aizome.Uta` and `service=youtube-music`. It
includes the account display name, the YouTube cookies, the account index,
and the source's identity and label for **Reconnect**. Secret Service prompts
are handled by the desktop keyring. Pasted text travels to the helper only
on its standard input, and the Rust `Secret` type prints only its length.

Anonymous operations do not read the keyring or browser. Connected operations
fail if the keyring cannot be read or the required session cookie has expired;
they do not deliberately retry anonymously. The same cookie jar reaches
yt-dlp's music player requests, and Uta lets the extractor select the best
audio stream the service offers. The displayed codec and bitrate describe
the resolved format, not a verified Premium entitlement.

Connecting or disconnecting invalidates in-flight library and player results
and stops current audio. Failed reconnection preserves the previous secret.
Disconnect removes the keyring secret but retains the local queue and volume.

## Audio and desktop integration

mpv streams the audio as it arrives: it is heard about a third of a second
after mpv has the stream's URL. Most of a song's start is asking YouTube for
that URL, which takes one to three seconds. So while a song plays, Uta looks
up the next one in the queue. It starts three seconds in, so the lookup's
burst of work doesn't compete with the song's own start, and again whenever
the queue changes what comes next. Uta also keeps the streams of the last
few songs, so Next, Previous, and Repeat One start without asking YouTube.
A kept stream is used only if its signed URL lasts until the song would end.
Skipping to a song still being looked up waits for that lookup rather than
starting a second one. Kept streams live only in memory, and connecting or
disconnecting discards them.

An actor supervises mpv and controls it over a Unix socket inside a private
temporary directory. A new load replaces the previous mpv process, giving
events an unambiguous playback generation. Stream URLs travel over the
socket. Only a small allowlist of transport headers reaches mpv; session
cookies remain in the service helper. mpv loads no user configuration and
does not run its own YouTube extraction.

MPRIS exports track metadata and controls on `org.mpris.MediaPlayer2.uta`.
Neither cookies nor signed stream URLs are exported. Its D-Bus connection
and property notifications run on a separate thread. A missing session bus
does not prevent ordinary playback.

A file lock guards the single-instance socket at
`$XDG_RUNTIME_DIR/aizome-app.uta.sock`. Later launches activate the existing
window and pass their search text. The lock prevents racing launches from
unlinking a live socket.

Queue metadata and volume live in
`$XDG_STATE_HOME/aizome/uta/session.json` (normally
`~/.local/state/aizome/uta/session.json`). Writes are serialized, coalesced,
and atomically replaced. Restoring a queue never starts playback.

## Validation boundaries

The automated suite uses synthetic account sessions for authentication tests.
The opt-in network check searches public songs, resolves one, and verifies
that mpv advances through real audio using a null sink. Validating an actual
Premium entitlement requires a listener to connect their own account in the
app; those tests do not import the developer's personal browser cookies.
