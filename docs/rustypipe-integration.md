# RustyPipe integration trial

This report describes the original anonymous trial. See
[persistent authenticated playback](premium-playback.md) for the current branch.

Implemented on `research/youtube-integration-options`, 27 October 2026. Builds,
automated tests, playback checks and measurements ran over SSH on
`wx-workstation`. The local checkout contains the implementation and evidence.

## Try it

Close any existing Uta window first: a second process activates the existing
instance. The prepared local package can be started with:

```sh
./result-rustypipe/bin/aizome-uta
```

Use it without an account connected to exercise native playback. Connected
accounts deliberately retain the established service, including their library
and audio extraction. No personal browser cookies or keyring sessions were read
for this evaluation. To compare the previous service with the same build:

```sh
UTA_SERVICE=python ./result-rustypipe/bin/aizome-uta
```

The reproducible package target is `nix build .#uta-rustypipe`; Cargo developers
can use `nix develop --command cargo run --release --features rustypipe`.
The ordinary package and builds without that feature retain Python throughout.

Try an uncached song, Songs/Videos/Albums search, a public album or playlist,
radio, seeking, Next/Previous, and reconnecting an account. Account operations
use the original implementation; this trial does not establish native Premium
playback or multi-account fidelity.

## Behavior and boundaries

| Operation | Adapter in this trial |
| --- | --- |
| Anonymous Songs, Videos, Albums search | RustyPipe, retained HTTP client; same 40-item fetch limit |
| Anonymous album/playlist tracks and radio | RustyPipe, bounded continuations; existing list limits |
| Anonymous audio | RustyPipe with an explicit VisionOS client; mpv remains the player |
| Top search | Python: RustyPipe 0.11.4 returned no playable rows for the current top-results layout |
| Combined playlist search, ordinary Up next | Python, preserving existing semantics |
| All connected-account requests, account discovery/connect/disconnect, library and likes | Python, preserving credentials and account selection |
| Native extraction failure | Retry through the existing helper |
| Native URL rejected before playback advances | Evict the URL, invalidate that player generation, retry once through yt-dlp |

Native failures never cause connected requests to retry anonymously. A failure
after playback has advanced is reported rather than restarting the song. Existing
generation checks, queue caching and prefetch remain in place. Each native HTTP
request has a 10-second timeout and the operation has a 20-second deadline before
falling back. Independent calls share a two-thread async runtime and HTTP client;
search does not serialize playback behind a worker queue.

RustyPipe has no credential access here. Its disk cache, raw-response reporter
and external BotGuard helper are explicitly disabled. Its client state stays in
memory. Third-party RustyPipe/HTTP logs are suppressed even with `RUST_LOG`, and
the adapter returns fixed errors rather than upstream response bodies or URLs.
No browser, Node, Deno or BotGuard process is needed by the successful native
path. Python/Deno remain packaged for account operations and fallback.

The unmodified 0.11.4 iOS client resolved a URL in 129 ms, but mpv failed and a
2 MiB request returned HTTP 403. TV and DesktopMusic extraction also failed.
The vendored patch adds a named VisionOS client, using the context already working
in the pinned yt-dlp. Uta explicitly selects it only for anonymous requests.
See [vendor provenance and patch](../vendor/README.md). This is a maintenance
commitment; it is not an assertion that stock RustyPipe works unchanged.

The opt-in executable includes GPL-3.0-only RustyPipe. Uta's own files keep their
MIT OR Apache-2.0 license; the Nix trial package declares the combined build's
GPL license and installs the upstream license. The default build does not link
RustyPipe.

## Latency

Same workstation/network and pinned Nix development shell, release builds,
Rust 1.98.0, Python 3.14.7, ytmusicapi 1.12.3, yt-dlp 2026.8.19,
RustyPipe 0.11.4 plus the checked-in patch. No account access.

| Measurement | Before | After | Samples per adapter |
| --- | ---: | ---: | ---: |
| Filtered song search, 40-item fetch limit | 1,955 ms | 1,342 ms | 5, alternating order |
| Audio URL resolution after search | 1,294 ms | 106 ms | 5, same IDs, alternating order |
| Resolution + first positive mpv position, null sink | 1,459 ms | 220 ms | 5, same IDs, alternating order |
| Full app: chosen track to positive playback position | 2.19 s | 1.11 s | 3 before, then 3 after |
| Full app: prefetched Next to positive position | 0.41 s | 0.41 s | 3 before, then 3 after |

These are medians, not p95 estimates. The native resolver is warm after search
in the interleaved benchmark. Its first filtered search took 1,581 ms, including
native bootstrap. Search fetched up to 40 rows in both adapters; native ID
deduplication produced 36–40 displayed rows versus 40 in Python. The same five
audio IDs selected Opus with identical reported bitrates (128–144 kbps).
All ten streams passed first playback, seeking past 30 seconds, and separate
2 MiB HTTP 206 reads. Those validation reads occur after the start measurement.
The separate native full-track check played `jy5x7bDYd4o` through normal EOF,
with a final observed position of 172.70 seconds, without seeking or fallback.

The full app includes dispatch, UI/harness work and mpv startup, at volume zero;
it measures playback progress, not sound reaching a physical speaker. Chosen
track samples were **1.96, 2.72, 2.19 s before** and **1.27, 0.57, 1.11 s after**.
It uses the same Top search and queue selection flow on both builds. This is a
small sequential sample with different cold/warm and CDN states, so it is less
controlled than the paired component benchmark. Prefetched playback was already
fast and did not materially improve.

The initial, broader baseline separately measured helper resolution at 1,613 ms
median. Use the interleaved 1,294 ms baseline for the comparison above; timing
varies across runs. Memory, idle CPU, browser comparisons and authenticated
performance have not been measured and are not claimed as improvements.

Evidence:

- [Initial workstation baseline](research/rustypipe-before-2026-10-27.json)
- [Interleaved final comparison](research/rustypipe-comparison-2026-10-27.json)
- [Application startup samples](research/rustypipe-app-start-2026-10-27.json)
- [Full-track playback](research/rustypipe-full-track-2026-10-27.json)

## Reproduce and validate

```sh
nix develop --command cargo test --features rustypipe
nix develop --command python3 -m unittest discover -s service/tests -q
nix develop --command cargo clippy --all-targets --features rustypipe -- -D warnings
nix develop --command cargo fmt --check -p aizome-uta
nix develop --command cargo test --features rustypipe --test network -- --ignored --nocapture
nix develop --command cargo run --release --features rustypipe --example youtube_latency > /tmp/comparison.json
nix develop --command cargo run --release --features rustypipe --example youtube_latency -- --full-track
nix develop --command env UTA_SERVICE=python cargo run --release --features rustypipe --example start_latency
nix develop --command cargo run --release --features rustypipe --example start_latency
```

The network tests require native success for the supported metadata and playback
operations; they cannot pass by silently using Python. The unit tests cover
account routing, extraction fallback, track conversion, rejected-stream recovery,
stale player events, and avoiding retries after playback has advanced. Existing
UI snapshots, queue, playback, MPRIS and synthetic authentication checks remain
part of the suite. The optional full-track probe uses a silent sink and requires
normal end-of-file after more than a minute of playback.

Validation completed: 38 Rust tests with the feature, 33 without it, 24 Python
tests, both opt-in network tests, Clippy, formatting, and the Nix package's
release checks passed. The package was built on the workstation at
`/nix/store/m2f0mqp3dc4bahim6i5wngs0jhzv694p-aizome-uta-0.1.0`.
The local Nix daemon would not import that unsigned store output, so its contents
were copied into `target/rustypipe-package` and only the launcher's executable
path was relocated. `result-rustypipe` points there. All referenced runtime
dependencies were already present locally. The compiled executable's SHA-256
matches the remote package:
`e3aaaa887b8c0d07c2fc4621b1d2440f89c7f22b7176ac143f2795585ed22691`.
The local launcher's `--version` check passed; no local compilation was needed.
