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

The account screen explicitly initiates browser import. Cookies are filtered
by YouTube's domain boundary and expiry, and a YouTube account request must
succeed before replacing the previous secret. The secret is stored using
Secret Service, matched by `application=org.aizome.Uta` and
`service=youtube-music`. It includes the account display name and YouTube
cookies. Secret Service prompts are handled by the desktop keyring.

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
