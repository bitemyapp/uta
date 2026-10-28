# Persistent RustyPipe playback

Branch: `research/rustypipe-premium`, based on `research/rustypipe-native`.
The earlier [anonymous latency report](rustypipe-integration.md) is historical;
its numbers are not Premium measurements.

## Install and connect

RustyPipe is enabled by default in Cargo, the Nix package and the NixOS module.
`uta-rustypipe` remains a compatibility alias; `uta-python` builds without RustyPipe.
The default executable can also select the helper with `--backend python`.

Build `nix build` on `wx-workstation`. The locally installed
command is `~/.local/bin/aizome-uta`, also available as **Uta** in the application
launcher. Installation lives under `~/.local/share/aizome/uta/builds/`, with a
`current` symlink and user GC roots for the Nix runtime dependencies. It survives
closing the terminal, rebooting, and deleting the development checkout.

Open **Account → Connect** and select your signed-in browser, or use the existing
paste-request/cookies option. The session stays in the desktop Secret Service
keyring under the existing Uta identity; restarting the app restores it. Do not
paste session material into benchmark output or a Git issue.

The UI reports the codec, bitrate and whether the request used the account.
“Account playback” means credentials were supplied; it does not assert that
YouTube granted Premium or returned a particular quality. Check the actual format.

## Routing

- Native, anonymous public Songs/Videos/Albums searches, collections and radio,
  including while connected. Unsupported searches/Up next retain their helper.
- Connected playback uses a retained cookie-authenticated `WEB_REMIX` client.
  The selected Google account index is carried through bootstrap and checked.
  RustyPipe selects the best available audio using its codec-aware bitrate ranking.
- Current signature and throttling challenges use pinned EJS 0.8.0 in bounded
  QuickJS through rquickjs 0.14.0 when the old extractor cannot handle the player. Preprocessed player
  code, a loaded solver worker and HTTP connections are reused in memory. Cold setup is measured separately
  from warm resolution; no native account or player cache is written to disk.
- Account discovery, connecting, library synchronization and likes use the existing
  helper. Empty anonymous collections retry with the account to handle private lists.
- Native resolution/initial media transport failure falls back to authenticated
  yt-dlp, preserving the selected account. Detected Premium accounts use
  `web_music`; other accounts retain yt-dlp's authenticated client selection. It never substitutes anonymous
  playback for a connected request. Mid-song failures do not restart playback.
- Native credentials expire after at most 30 minutes (earlier for expiring cookies).
  Status, connect and disconnect invalidate that in-memory client. Credentials and
  media URLs are redacted from diagnostics and never included in benchmark JSON.

Both native and helper paths depend on YouTube's unofficial interfaces. Premium
Opus 774 is now validated with the connected account; other brand-account cases
remain untested. No personal session was used for the initial pre-install verification.
Browser connection now follows the account active in YouTube Music, while an explicit
account from pasted headers takes precedence.

## Side-by-side and ABBA

```sh
# Separate anonymous window, with no keyring/browser reads or account writes:
aizome-uta --ignore-credentials
# Separate helper-only window and queue:
aizome-uta --backend python
# Combine both:
aizome-uta --backend python --ignore-credentials

# After connecting: A=native, B=yt-dlp, same saved account, ABBA repeated three times:
aizome-uta --benchmark Az4SnX521pI --rounds 3 > authenticated-abba.json
# Same comparison without consulting any persisted credentials:
aizome-uta --benchmark Az4SnX521pI --rounds 3 --ignore-credentials > anonymous-abba.json
# A=authenticated native, B=anonymous native (quality may differ):
aizome-uta --benchmark Az4SnX521pI --rounds 3 --compare-auth > auth-abba.json
```

Flags are removed before search/activation. Each GUI backend/credential combination
has its own activation socket and saved queue. Media keys belong to the first
window to acquire the desktop MPRIS name. Benchmarks are headless, do not register
MPRIS, and do not change queues, account settings or files in the keyring.

The benchmark makes strict native requests so fallback cannot masquerade as native
success. JSON includes codec, format ID, client, bitrate, authentication mode,
resolution time, total time to first positive mpv position, and failures. The first
request to each native client includes cold initialization. Subsequent requests
reuse that client but resolve fresh URLs; GUI prefetch/URL caches are bypassed.
mpv uses a fresh null sink per sample: this measures playback progress, not speaker
latency. Compare matched formats and report cold samples separately. Authentication
comparisons can have different codecs/quality and must not be pooled blindly.

## Verification

Run builds/tests on the workstation. Credentials for eventual Premium measurements
remain on the installed local machine; do not copy the keyring to the build host.

```sh
nix develop --command cargo test --features rustypipe
nix develop --command cargo test
nix develop --command python3 -m unittest discover -s service/tests -v
nix develop --command cargo clippy --all-targets --features rustypipe -- -D warnings
nix develop --command cargo test --features rustypipe --test network -- --ignored
# Supply a freshly downloaded public player.js, without cookies:
UTA_TEST_PLAYER_JS=/tmp/uta-current-player.js nix develop --command cargo test --release --features rustypipe --test solver -- --ignored
```

Authenticated latency and Premium media validation are complete. See
[the results and account-selection fix](premium-latency-results.md).

Pre-install workstation checks passed: Rust tests with and without the feature,
26 Python tests, Clippy, all three public network checks (including WEB_REMIX),
and the current public player solver check. Player `096efeec` took 4,312 ms to
preprocess, 175 ms to load, and 2.1 ms to reuse and solve a throttling challenge.
These are solver components, not authenticated playback measurements.

The final Nix package passed its embedded checks on the workstation and the
[packaged anonymous ABBA smoke run](research/rustypipe-premium-anonymous-smoke-2026-10-28.json)
played all four samples with the same Opus format 251 (127.8 kbps). Native resolution
was 595 ms cold / 88 ms warm; helper samples were 1,504 / 1,414 ms. This single block
verifies the runner, not a statistically robust latency estimate.

Initial installed executable SHA-256 (used for the latency measurements):
`30eba26d1ed0a54aa6a25c18a8aa64c4a0d0613f4bb0dd352e23cd097866c395`.
The copied executable matched the workstation output; the local launcher reports
`Uta 0.1.0 (RustyPipe: true)`. Installation registered nine runtime GC roots and the
application-menu entry. Local verification used only `--version` and `--help`.

Current persistent installation, including the active-account and expired-session
fixes, has executable SHA-256
`8b61fb5200aab350bb891c1d1aaee1c519b065c41d0cef80527056d7d3eca685`.
Its Nix package checks and all 29 Python tests passed on `wx-workstation`.
The local executable hash matches the workstation package. Reopen Uta to load it;
use Account → Reconnect if Google has rotated the saved browser session.

CI and `nix flake check` cover both the default native `uta` package and `uta-python`.
CI checks the reported backend feature for each package. Live non-Premium authenticated playback has not
been validated in this trial; its helper retains upstream client selection rather
than the Premium-only Music restriction.
