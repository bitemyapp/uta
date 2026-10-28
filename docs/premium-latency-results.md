# Authenticated and anonymous playback latency — 28 October 2026

RustyPipe successfully played Premium **Opus format 774, 255–276 kbps**, and
anonymous **Opus format 251, 128–143 kbps**. All **108** ABBA samples started
playback. A separate authenticated track played through normal EOF at 172.73
seconds; its 2 MiB media-range check returned HTTP 206 with all requested bytes.

## Results

Medians on the locally installed desktop, using the same three public tracks and
matched formats within each backend comparison:

| Playback mode | Native resolution | yt-dlp resolution | Native start | yt-dlp start |
| --- | ---: | ---: | ---: | ---: |
| Premium Opus 774 | 168 ms | 3,860 ms | 400 ms | 4,084 ms |
| Anonymous Opus 251 | 100 ms | 1,536 ms | 309 ms | 1,841 ms |

Native values exclude the first resolution in each fresh process: 15 retained-client
samples per mode. The helper starts a new Python/yt-dlp process for each request,
as it does in the app, so its baseline includes all 18 samples per mode. This is
about 10.2× faster Premium startup and 6.0× faster anonymous startup during continued
use, under these test conditions.

**Cold startup is slower for Premium native playback.** Its first start had a
6.35-second median (5.87–7.26 seconds), including account bootstrap and player
JavaScript preprocessing. Anonymous native cold startup was 1.11 seconds
(0.95–1.75 seconds). The current solver cache is in memory and is lost when the
process closes. These cold samples are retained in the evidence, not hidden in
the warm summary. Native Premium warm startup also had one 1.36-second outlier.

A separate direct native ABBA comparison measured Premium at **165 ms resolution /
398 ms start**, versus anonymous at **72 ms / 317 ms**, again 15 warm samples each.
That comparison changes both client and quality (`WEB_REMIX` Premium versus
`VISIONOS` anonymous); it does not isolate authentication overhead alone.

## Method and evidence

All authenticated work ran on the local desktop using the persisted Secret Service
session. Credentials were never copied to `wx-workstation`, written into reports,
or printed. Builds and synthetic tests ran on the workstation.

- Public IDs: `Az4SnX521pI`, `WDe5PzVNSr8`, `jy5x7bDYd4o`.
- Three ABBA blocks per track for Premium native/yt-dlp, anonymous native/yt-dlp,
  and native authenticated/anonymous: nine processes and 108 total samples.
- Native calls were strict: no fallback could be reported as native success.
- Every sample resolved a fresh URL and started a fresh mpv with a null audio sink.
  GUI prefetch and signed-URL caches were bypassed. Start time ends at the first
  positive mpv position, not sound reaching a physical speaker.
- Within each backend comparison, the two adapters returned the exact same format
  ID and reported bitrate for each track. The Premium and anonymous modes have
  intentionally different quality.
- These are small-sample measurements on one desktop/network. They establish the
  observed medians, not a broad tail-latency or reliability guarantee.

[All samples and computed summaries](research/rustypipe-authenticated-latency-2026-10-28.json)
include the measured executable hash and commit (`739d816`).
[Full Premium track validation](research/rustypipe-premium-full-track-2026-10-28.json)
used the workstation-built probe with authenticated native resolution. That check
played without seeking; the `seek_ok` field is false because seeking was not attempted.

## Account-selection fix

The first attempt exposed a connection bug: browser import selected Google's first
signed-in account, while YouTube Music was using another account with Premium.
The initial native attempt returned standard-quality format 251 and failed to
start; the helper rejected conflicting account selections. Those failed diagnostic
attempts are excluded from the successful ABBA experiment above.

The saved connection was corrected to the active Premium account and validated by
YouTube's Premium indicator and returned high-quality audio formats. Browser
connection/reconnection now detects the active account from Music's signed-in
configuration before saving it. Explicit account selection in pasted request
headers still takes precedence. A browser-style User-Agent is needed for that
page; the minimal User-Agent can return an unsupported-browser response.

YouTube stopped accepting the saved browser cookies after the measurements.
The helper now verifies the logged-in bootstrap response and fails with a reconnect
message rather than treating expired cookies as authenticated playback. Reopen Uta
and use **Account → Reconnect** to import the browser's current session.

The follow-up changes affect connection selection and expired-session handling;
the native resolver measured above is unchanged. Regression tests cover active
account selection, explicit pasted selection, malformed/signed-out configuration,
and revoked-cookie rejection. The rebuilt persistent installation retains the
same commands and isolated anonymous comparison mode.
