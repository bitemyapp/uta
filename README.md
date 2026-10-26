# Uta 歌 — YouTube Music

Uta is a native YouTube Music client for
[Aizome](https://github.com/bitemyapp/aizome),
[Sekka](https://github.com/bitemyapp/sekka)'s desktop environment.
*Uta* means “song.” Its Rust interface uses Aizome's widget kit and
[Sidestep](https://github.com/bitemyapp/sidestep), with native Wayland windows.

![Uta in Aizome's Night theme, with demonstration song metadata](tests/snapshots/search-night.png)

This is an early, working client. Search songs, albums, and playlists; play
audio; build a queue; and connect your account to browse your library.

- Native Day and Night themes, keyboard navigation, global menus, and Aizome's command palette.
- Play/pause, next/previous, seeking, volume, shuffle, and repeat.
- A saved queue that restores without automatically playing.
- MPRIS integration for desktop media controls and media keys.
- Liked songs, library playlists, and liking/unliking the current song.
- Account linking through a signed-in browser, without creating API credentials.

## Run

On Linux with Nix flakes:

```sh
nix run github:bitemyapp/uta
```

The package includes the music service dependencies, mpv, Deno, the desktop
entry, and the app icon. It works in Aizome and other Wayland desktops.
For a preview with fictional song metadata and no network or account access:

```sh
nix run github:bitemyapp/uta -- --demo
```

For a NixOS configuration, add Uta as a flake input, import
`uta.nixosModules.default`, and set `programs.uta.enable = true;`.
The executable is `aizome-uta`; the desktop app ID is `org.aizome.Uta`.

## Connect your account

1. Open **Connect account** in Uta's sidebar.
2. Sign in at **music.youtube.com** in your browser.
3. Select that browser in Uta and click **Connect browser**. If needed, enter
   the browser profile directory; otherwise its default profile is used.

Firefox, Chromium, Chrome, Brave, Edge, Vivaldi, and Opera are supported through
yt-dlp's browser-cookie importer. A running Secret Service desktop keyring,
such as GNOME Keyring, is required to save the session. Browser sandboxing or
cookie encryption can require choosing the profile directory or closing the
browser before importing. The first Google account in that browser session is
used; a dedicated browser profile is useful when you have several accounts.

Uta retains only YouTube cookies, stored in the desktop keyring. Both
library requests and audio extraction use the saved session. Cookies are not
written to the queue file, command-line arguments, or application logs.
**Disconnect** stops playback and removes the saved session. Reconnect here
if YouTube expires it; Uta does not continuously read your browser profile.

With a Premium account connected, Uta requests the best available audio using
that session. The player displays the selected codec and bitrate; a connected
account does not guarantee that every song exposes a Premium format. YouTube's
unofficial interfaces and playback requirements change, so the service
dependencies occasionally need updating. Premium offline downloads and
gapless playback are not implemented.

## Keyboard

| Shortcut | Action |
| --- | --- |
| Super+F | Search |
| Enter in search | Run search |
| Enter on a selected song | Play |
| Super+P | Play/pause |
| Super+Left / Right | Previous / next |
| Super+. | Stop |
| Super+1 / 2 / 3 | Liked songs / playlists / queue |
| Super+, | Account |
| Super+R | Refresh |
| Super+Q | Quit |

Search fetches up to 40 matches, library playlists up to 100, and song
collections up to 200 tracks. Pagination, playlist editing, and uploads are
not yet implemented. The queue can hold up to 5,000 songs.

## Develop and test

`nix build` assembles the pinned Aizome and Sidestep sources automatically.
For Cargo development, the repositories sit beside one another:

```text
work/
  uta/
  aizome/                 # 729cf1c140b349dc6f8a9d7ba1772c4b9bb4fe18
  sidestep-aizome/        # 16a1e376c0bd85d52ef020e114bbd1707fb9b14b
```

Aizome's `sidestep` link points to `../sidestep-aizome`. These revisions are
also recorded in `flake.nix` and `flake.lock`. Existing checkouts can be used
for toolkit development; the package always builds against the locked inputs.

```sh
nix develop
cargo run -- --demo
cargo test
python3 -m unittest discover -s service/tests -v
cargo clippy --all-targets -- -D warnings
cargo fmt --check -p aizome-uta
nix build
```

The tests exercise native widgets, screenshots in both themes, playback races,
queue persistence, cookie filtering, authenticated extraction, actual mpv
playback through a silent output, and MPRIS on a private D-Bus session.
They do not read your browser session or contact YouTube. Golden tests require
Aizome's fonts, which `nix develop` supplies. To deliberately update them:

```sh
AIZOME_UPDATE_GOLDENS=1 cargo test --test app native_views_in_both_themes
```

Optional checks against public YouTube content, without account access:

```sh
python3 scripts/smoke-service
cargo test --test network -- --ignored --nocapture
```

See [architecture](docs/architecture.md) for the service boundary and
[performance](docs/performance.md) for the frame-budget measurement.

## License

MIT OR Apache-2.0, matching Aizome. See [LICENSE-MIT](LICENSE-MIT) and
[LICENSE-APACHE](LICENSE-APACHE).
