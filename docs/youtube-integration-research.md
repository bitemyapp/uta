# YouTube and YouTube Music integration research

Investigated 27 October 2026 on `research/youtube-integration-options`.
Experiments use public content anonymously. No browser sessions, keyring entries,
Premium accounts, or account mutations were used. Production code is unchanged.

## Recommendation

**Prioritize native operation with no browser running alongside Uta.** The user
clarified that reducing browser use and resource overhead is the primary goal;
privacy is a secondary consideration. Do not make a Google partnership or
customer-created API credentials prerequisites. Browser-backed playback is
outside the recommended direction, although it remains documented below for
completeness.

The user will pursue **RustyPipe on another computer**. The next evaluation should
therefore establish whether it can replace Uta's metadata and playback-resolution
paths with acceptable feature coverage and total resource use. See the
[RustyPipe handoff](#rustypipe-handoff) for concrete checks. No RustyPipe integration
or benchmark has been performed on this research branch.

The preferred architecture is a Rust service layer, a small restricted JavaScript
engine where required, and native audio playback, with yt-dlp available as a
fallback. A narrow custom implementation and an existing Rust library are two
ways to reach that architecture. Retaining the current Python clients and
returning first pages immediately remains a useful lower-risk improvement or
transition path; it does not need to precede the separate RustyPipe experiment.

The strongest measured opportunity is avoiding unnecessary requests. The current
helper resolved a song in a median 1.60 s; a retained yt-dlp instance took 1.01 s.
A different resolver, with its unrelated metadata request experimentally omitted,
took 0.13–0.27 s and produced playable audio. This demonstrates feasibility, not a
validated Premium implementation or a production latency guarantee.

Uta already operates without a running browser after importing a session. Retain
that property and the existing connection flow initially. An occasional browser
visit for sign-in/reconnection is the working authentication approach; continuous
browser activity must not be a playback dependency. Defer extension and embedded
login work until observed connection failures justify it.

I found no public, documented Google API that supplies the combination of YouTube
Music library/radio, Premium audio streams, and native background playback that
Uta wants. Official desktop OAuth can have an ordinary sign-in experience with
developer-owned credentials, but does not unlock that missing playback API.
The public [Data API resource catalog](https://developers.google.com/youtube/data/reference/rest)
and [IFrame player API](https://developers.google.com/youtube/iframe_api_reference)
describe a different set of capabilities. A supported commercial integration with
the full desired behavior would require an agreement beyond these public APIs;
availability of such a partnership has not been established.

“Paying customers” could mean Aizome customers, YouTube Premium subscribers, or
both. The recommendation works as an investigation plan for either audience,
but the fastest anonymous playback experiments do **not** prove Premium account
fidelity. Free-account and Premium-account behavior need separate acceptance tests.

## Options for lower resource use

| Option | Role | Resource implications | Remaining work |
| --- | --- | --- | --- |
| Retained ytmusicapi / yt-dlp clients | Immediate improvement or transition | Avoids repeated imports/bootstrap; resident Python trades idle memory for response latency. An idle timeout is possible. | Measure total memory and idle CPU; keep library requests from blocking playback. |
| Rust HTTP + small JS solver + native player | Preferred architectural destination | No browser renderer, DOM or layout engine; instantiate solver work only when needed and cache reusable artifacts. Actual footprint remains unmeasured. | Maintain the required internal endpoints, response parsers, account selection and playback compatibility. |
| RustyPipe | User's next experiment; broad existing Rust candidate | Reuses native API/extraction code, but some paths require a separate BotGuard helper with a simulated browser environment. | Check licensing fit, complete dependency/process footprint, feature coverage and authenticated playback. |
| ytmapi-rs plus a dedicated resolver | Permissively licensed component option | Moves Music metadata into Rust; requires a separate playback implementation. | Fill Uta feature gaps and compare maintenance cost with a narrower custom adapter. |
| YouTube.js / DisTube service | Secondary browser-free candidates | A JS service runtime can run without a browser. Token generation can add substantial memory use; the DisTube mweb sample reached about 593 MiB RSS. | Prove authenticated media delivery and measure the complete runtime rather than the simplest anonymous path. |
| Genuine website page / Electron / persistent WebView | Documented alternative, outside the preferred direction | Retains browser machinery during use. | Does not address the stated resource and browser-dependence objective. |

For the custom native path, the proposed components are Rust requests for
search/library/radio/player data, reusable HTTP connections, cached session and
player state, a restricted QuickJS worker for required transformations, and mpv
initially. A retained mpv instance or libmpv can be evaluated separately. Launch
yt-dlp only when the native resolver cannot handle an item; do not require Python,
Node and a browser to remain resident together.

[yt-dlp EJS supports QuickJS and QuickJS-ng](https://github.com/yt-dlp/ejs), so a
small JS engine is a practical building block. QuickJS supplies JavaScript
execution without a browser UI engine. This does not establish that every
BotGuard/token requirement can be satisfied by the same small helper; measure
those paths independently. The
[ytm-rs solver implementation](https://github.com/grapedjuice/ytm-rs/blob/main/src/jsc.rs)
is an architectural reference for cached QuickJS solving, subject to the licensing
distinction described below.

The experiments so far establish latency opportunities and a few process RSS
snapshots. They do **not** establish end-to-end memory savings against a browser
tab or a complete native implementation. Compare total process-tree proportional
set size (PSS), peak memory, idle CPU/wakeups, cold start, first-result latency and
time to playable audio. Include cold and warm solver/token paths, fallback usage,
and resource release after stopping playback. Faster request completion and lower
steady memory are separate measurements.

## What Uta currently does

| Area | Implementation | Consequence |
| --- | --- | --- |
| Native UI | Rust, Aizome widgets, Sidestep/Wayland | The UI is already native; a different service runtime need not change it. |
| Service boundary | [`src/service.rs`](../src/service.rs), `Live::call`: new `python3 -c` process for every request | Imports, client initialization and connection setup repeat. Responses wait for process completion; 30 ms polling and a 120 s outer deadline. |
| Music metadata | [`music()` and `handle()`](../service/uta_service.py): new `YTMusic` each time | Visitor/bootstrap and HTTP session state are discarded. Filtered search asks for 40 results; the library can fetch continuations before returning. Track lists can accumulate 1,000 items before display. |
| Authentication | Browser discovery, SQLite/WAL snapshots, cookie decryption, Secret Service storage | Already a usable one-click experience, but much of the Python helper is browser compatibility code. Saved cookies are a snapshot; Reconnect imports current cookies. |
| Resolution | `resolve()`: new `YoutubeDL`, `bestaudio/best`, Deno available, `cachedir=False` | General downloader work occurs on a latency-sensitive path. A URL response is treated as resolution success before media is consumed. |
| Playback | [`src/player.rs`](../src/player.rs): new mpv process per track, `--ytdl=no`, private socket | Good isolation and explicit ownership of events; process startup is a smaller measured cost than extraction. |
| Prefetch | [`src/app.rs`](../src/app.rs): signed-URL memory cache, next-track lookahead after 3 seconds, pending request coalescing | Already handles sequential playback well. The largest visible gap is an unprefetched click. |

The implementation already keeps cookies out of mpv, command-line arguments and
application logs. Preserve these properties in any rewrite. Keep the existing
account-generation invalidation and playback race protections too.

Two account details deserve specific attention. `music()` forwards the saved
`authuser`; `resolve()` forwards the cookie jar but not that account selector.
Multiple Google accounts or brand channels could therefore behave differently
between browsing and playback; this is a source-level concern, not a reproduced
account failure. Also, the returned `authenticated` flag means a saved session
was supplied, not that the selected stream was verified as Premium.

The service reads history but contains no explicit playback/watchtime reporting
path. We should not promise that native listening updates history,
recommendations, or service-side listening accounting exactly like the website.
Stale UI replies are discarded, but obsolete helper work is not cancelled when
the user navigates away.

## Measurements

Same development machine and network, sequential experiments, small samples of
public music results. Python 3.14.7, ytmusicapi 1.12.3, pinned yt-dlp 2026.8.19,
Deno 2.9.7; Node 24.21.0 for DisTube. Requests were interleaved where practical.
These are medians, **not p95 estimates**. Server/CDN caches, query differences,
load and time of day remain confounders. Raw records include timestamps and load
where available.

| Experiment | Samples | Time | Result / interpretation |
| --- | ---: | ---: | --- |
| Current subprocess helper, filtered search, 40 rows | 5 | 1,899 ms median | Includes Python/service setup and network work. |
| Fresh in-process YTMusic, 40 rows | 5 | 1,702 ms | Removing process/import work alone helps modestly. |
| Retained YTMusic, 40 rows | 5 | 1,363 ms | Reuses session/bootstrap state. |
| Warm session, first search page | 5 | 460 ms | 20 rows returned; avoids a continuation request. |
| Same paging experiment, 40 rows | 5 | 1,162 ms | More results, therefore more work. First-page display is the useful product comparison. |
| Current helper, audio resolution | 5 | 1,596 ms | All samples advanced in mpv. |
| Fresh in-process yt-dlp | 5 | 1,232 ms | All samples advanced in mpv. |
| Retained yt-dlp | 5 | 1,010 ms | All samples advanced in mpv. |
| Fresh yt-dlp with shared disk cache | 5 | 1,059 ms | All played; overlapping ranges do not establish a large independent disk-cache benefit. |
| ytmusicapi `get_song` | 3 | 159 ms | Player response OK, but zero direct audio URLs; ciphered formats need further work. |
| YouTube.js Music search, first page | 5 | 477 ms | Comparable to retained ytmusicapi's first page. |
| YouTube.js Music info / decipher | 3 | 210 / 11 ms | URLs generated, but playback failed; see below. |
| DisTube 5.0.2 default `getInfo` | 3 | 891 ms | First 1,801 ms; later 891 and 722 ms. All passed 2 MiB reads and mpv. |
| DisTube, optional `/next` metadata omitted | 3 | 188 ms | 267, 188, 130 ms. All passed 2 MiB reads and mpv. Research-only intervention. |

Python startup itself was about 10 ms; importing ytmusicapi about 86 ms and
yt-dlp about 207 ms, including process startup. Network bootstrap and unnecessary
requests matter more than the interpreter alone. The default anonymous yt-dlp
path did not fetch player JavaScript in the baseline trace, so these measurements
do not prove that enabling a player-script cache is a major win for this path.
Authenticated paths may differ.

The paging harness uses `limit=1` to stop ytmusicapi after the server's first
page, which contains 20 results. It does **not** mean a one-result UI. Shipping
pagination needs a continuation-capable adapter or upstream iterator API, rather
than repeatedly rerunning increasingly large searches.

mpv's silent output generally advanced in roughly 0.12–0.37 s after launch/load
in the baseline. This checks initial decoding, not hardware-audible latency,
seeking, a complete track, or gapless transitions. DisTube also passed bounded
2 MiB reads. Do not directly compare these component timings with the older
full-application measurements in [performance.md](performance.md).

### A fast URL is not necessarily playable

YouTube.js 18.3.0 bootstrapped in 180 ms without loading its player and 637 ms
with it. Music info plus deciphering was fast, and all three 4 KiB range requests
returned HTTP 206. However, **all three 2 MiB requests returned HTTP 403 and all
three mpv probes failed**. Ordinary YouTube `getBasicInfo` returned formats without
direct or ciphered URLs in this experiment, consistent with a SABR-only delivery
path. No PO-token provider or real Premium session was supplied.

This does not establish that YouTube.js cannot play audio. It establishes that
the simple anonymous `getInfo → decipher → mpv` integration tested here is not
enough. Its [authentication guidance](https://ytjs.dev/guide/authentication)
recommends cookies for web clients and limits OAuth to the TV client. Additional
token/transport work is real integration work, not a free consequence of switching
libraries. [BgUtils](https://github.com/LuanRT/BgUtils) and
[googlevideo](https://github.com/LuanRT/googlevideo) address parts of that work;
SABR/UMP support may require a local streaming adapter rather than one mpv URL.

DisTube 5.0.2, released on 26 October, is a current implementation; older search
snippets describing an archived project were misleading. Its default path
selected VisionOS and worked. Much of `getInfo` time was a `/next` request for
related metadata. The experiment supplies an empty response for that optional
request through the package's fetch hook. This models an upstream
`includeRelated: false` option, **which does not currently exist**. It is not a
recommendation to ship a request interceptor. A supported minimal resolve API is
the desired implementation. See [info.ts](https://github.com/distubejs/ytdl-core/blob/main/src/info.ts).

The forced DisTube `mweb` path also worked with automatic PO-token generation in
one sample: 2.19 s initialization/resolution and process RSS around 593 MiB,
versus roughly 100 MiB on the default path. This is one process snapshot, not a
general memory benchmark. Token machinery can cost more than the simple path.
The [client planner](https://github.com/distubejs/ytdl-core/blob/main/src/resolve.ts)
tries anonymous requests before authenticated ones for optional-auth clients;
VisionOS is anonymous-only. There is no Music client in the
[client set](https://github.com/distubejs/ytdl-core/blob/main/src/clients.ts).
Supplying cookies therefore does not imply Premium-aware selection.

### Updating yt-dlp needs a playback check

The latest release, [2026.10.26](https://github.com/yt-dlp/yt-dlp/releases/tag/2026.10.26),
was tested from a separately unpacked wheel, without changing Uta's dependencies.
With the existing options it resolved URLs in a median 2.15 s through the helper,
but all 20 default/fresh/retained/cache samples failed mpv playback.

A follow-up alternated the installed and latest versions on two identical IDs.
The installed version selected `VISIONOS`: HTTP 206, 2 MiB received, mpv advanced.
The latest selected `TVHTML5`: HTTP 403, no media, mpv failed. Explicit VisionOS
selection on the latest version worked in one additional sample. This is a
reproducible local client-selection regression, not a claim that the release is
broken for every user. Do not upgrade blindly or force an anonymous client for
connected Premium accounts.

Forcing `web_music` in yt-dlp was not a performance improvement here: it selected
itag 18, a muxed video/AAC format, rather than equivalent audio-only output.
Forcing `web` failed extraction. Client names are capability choices, not merely
speed switches.

## Alternatives and their fit

| Approach | Customer setup | Fit for Uta | Principal limitation |
| --- | --- | --- | --- |
| Retained ytmusicapi + yt-dlp workers | Current Connect account | Best immediate improvement; mature fallback coverage | Still unofficial; retains Python and protocol maintenance |
| YouTube.js service | Browser cookies / browser bridge | Broad YouTube + Music model, explicit continuations, abortable async work, MIT | Tested simple anonymous audio path failed; tokens, JS execution and SABR may be needed |
| DisTube 5.x playback service | Cookies if authenticated path used | Most promising measured minimal resolver experiment; MIT | No Music metadata API, anonymous-first defaults, new implementation; Premium unverified |
| Rust Music client + direct player resolver | Browser cookies / browser bridge | Most native runtime, precise control of requests and caching | Own protocol maintenance; Rust alone does not eliminate signatures/tokens |
| Browser extension for connection only | Install/enable extension, connect selected account | Replaces browser database/decryption compatibility code | Extra installation step; extraction still needed |
| Genuine Music page as service/player | Sign in normally in a browser | Browser handles session refresh, player scripts, transport and website behavior | Browser resource/lifecycle cost; page internals remain unofficial |
| Dedicated temporary WebView for sign-in | In-app sign-in window | Can avoid browser-profile discovery | Google login compatibility, isolated profile persistence, account challenges; Linux WebKit dependency |
| Official Data API + visible IFrame player | Ordinary Google consent where private data needed | Publicly documented metadata and embedded video route | Does not supply Music-specific library/radio or native audio URLs/background playback |
| Invidious / Piped / hosted scraping API | None for public search; separate service accounts | Convenient public metadata, possibly playback URLs | Does not preserve Google Premium identity/library; proxy reliability, privacy and operating cost |
| NewPipeExtractor / other extractors | Usually anonymous or cookies | Another extraction implementation | Java/runtime or license changes; no automatic Premium account solution |
| Existing Music Assistant / Pear Desktop backend | Install/sign in to another application | Useful compatibility prototype or optional bridge | Moves maintenance into another app and adds lifecycle/API dependencies |
| Cast, phone remote, official app container | Another device or Android environment | Could offer an optional remote-control mode | Does not deliver the desired standalone Aizome-native local player |
| Direct Google/YouTube partnership | Developer commercial arrangement | Only avenue to investigate for a supported API covering the complete product | Access, scope, terms and feasibility unknown |

### Official APIs and OAuth

Customers do **not** need their own Google Cloud project for an official API
integration. Aizome can register its application once; customers grant access in
their system browser. Desktop OAuth uses PKCE and a loopback redirect, with the
app treated as a public client. Scope verification and service quota management
belong to the developer. [Google's desktop OAuth guide](https://developers.google.com/youtube/data/guides/auth/installed-apps)
documents this flow.

This could support YouTube playlists, subscriptions, ratings and video metadata.
It is not a documented source of Music home/recommendations, Music-specific
library semantics, or playable Premium audio. A bearer token also does not sign
an embedded player into the same account. Mixing official metadata with an
unofficial playback resolver leaves two authentication/capability systems to
maintain and does not confer approval on the latter.

The [current quota page](https://developers.google.com/youtube/data/determine_quota_cost),
updated 15 October 2026, lists a default **100 search calls/day**, 100 video
insert calls/day, and 10,000 daily units for other endpoints. Search and video
insert now have separate buckets. This is shared project capacity, not 100
searches per customer. More quota requires the applicable
[quota/compliance process](https://developers.google.com/youtube/data/guides/quota_and_compliance_audits).

An IFrame player needs no customer API key, but is a browser-hosted video player
with visibility/size and client-identification requirements, not an audio SDK.
[Required functionality](https://developers.google.com/youtube/terms/required-minimum-functionality)
also covers embedded-player identification. YouTube's
[developer policies](https://developers.google.com/youtube/terms/developer-policies)
restrict separated audio and hidden/background players. They allow some
commercial API clients subject to conditions; selling an app is not categorically
forbidden. A consumer Premium subscription should not be represented as a
developer playback license. These are material product constraints, not latency
issues a library swap fixes. The ordinary
[YouTube terms](https://www.youtube.com/static?template=terms) also matter for
unofficial access.

Do not confuse official desktop OAuth with unofficial TV-client authentication.
[ytmusicapi's OAuth setup](https://ytmusicapi.readthedocs.io/en/stable/setup/oauth.html)
requires supplied client credentials and follows a different integration path;
using developer-owned credentials could hide setup from customers, but does not
make the Music endpoints official. [yt-dlp's extractor documentation](https://github.com/yt-dlp/yt-dlp/wiki/Extractors)
says YouTube OAuth no longer works for its extraction and recommends cookies.
Borrowing a first-party TV application's client credentials is not a durable
desktop product foundation. Google's [limited-input-device flow](https://developers.google.com/youtube/data/guides/auth/devices)
is designed for that class of device.

### A browser can be an implementation component without becoming Uta's UI

There are two substantially different browser-bridge designs:

1. **Session bridge.** An extension offers “Connect this YouTube account,” sends
   only the required session/account state to a local native host, and updates it
   when it changes. Uta retains native metadata and audio execution. This avoids
   reverse-engineering SQLite and browser encryption, but requires explicit
   permissions, account/container selection and careful cookie scope handling.
2. **Page/player bridge.** A real YouTube Music page owns login, player and network
   behavior. Uta sends structured commands and renders returned state using Aizome.
   This reduces our involvement in signatures, tokens and media transport.
   Search/library extraction and controls beyond stable browser APIs still depend
   on page internals. Browser suspension, crashes, autoplay, media keys and queue
   ownership must be handled. It is a technical alternative, not an assertion
   that a hidden website player is an approved use of the official IFrame API.

[Chrome native messaging](https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging)
and [Firefox's cookies API](https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/API/cookies)
provide supported browser integration mechanisms. Prefer these over attaching
remote debugging to a user's daily browser. Since Chrome 136,
[remote debugging requires a non-default data directory](https://developer.chrome.com/blog/remote-debugging-port).

[Pear Desktop](https://github.com/pear-devs/pear-desktop) demonstrates the
website-backed approach; its API-server plugin exposes search and playback
controls. It is useful as a reference or experimental bridge, not a reason to
replace Aizome's UI with Electron. A dedicated WebView used only for login is
another possibility, as in [ytm-rs's login implementation](https://github.com/grapedjuice/ytm-rs/blob/main/src/login.rs).
Google's [OAuth policy](https://developers.google.com/identity/protocols/oauth2/policies)
disallows embedded user-agents for OAuth authorization. That is distinct from
ordinary website login; the latter can still encounter provider restrictions or
challenges and must be tested, rather than assumed reliable.

### Rust and other native implementations

[RustyPipe](https://docs.rs/rustypipe/latest/rustypipe/) 0.11.4 offers typed async
YouTube/Music access, player extraction and related caching. It is a serious
candidate if its GPL-3.0 licensing fits the distribution plan. Its default cache
can persist credentials, and parser diagnostics can write response data; Uta
would need a deliberate cache/logging adapter. It can require additional
BotGuard support. No latency result for it was measured here.

[ytmapi-rs in youtui](https://github.com/nick42d/youtui) is a permissively licensed
Rust Music API option, but its published feature matrix does not yet cover all
Uta operations. Other libraries such as
[rusty_ytdl](https://github.com/Mithronn/rusty_ytdl) move extraction to Rust;
they do not remove client-selection, authentication or protocol churn. A language
rewrite needs a feature and playback comparison, not an assumption of lower
network latency.

[ytm-rs](https://github.com/grapedjuice/ytm-rs) is a particularly relevant
architectural precedent: a native UI, authenticated WEB_REMIX player calls,
cached QuickJS solving using yt-dlp EJS, and a yt-dlp fallback. Its performance
claims were not reproduced here. The application is GPL-3.0; Uta is
MIT OR Apache-2.0, so copying implementation code changes the licensing analysis.
The standalone [yt-dlp EJS project](https://github.com/yt-dlp/ejs) is Unlicense
and is a possible independent building block, with its dependency notices.

This suggests a narrow native resolver rather than a full reimplementation:
fetch only the necessary player response, use cached player/solver artifacts,
select an audio format for the actual account and hand the verified stream to
mpv. Current [PO-token guidance](https://github.com/yt-dlp/yt-dlp/wiki/PO-Token-Guide)
says Premium subscribers do not currently require GVS PO tokens; that is a
conditional upstream observation, not a lasting contract or exemption from
every player/token requirement. Execute downloaded player logic in a restricted
runtime, as the YouTube.js experiment does, not with application filesystem and
credential access.

### Proxies, wrappers and adjacent services

[Invidious](https://docs.invidious.io/api/) and
[Piped](https://docs.piped.video/docs/api-documentation/) expose simpler HTTP APIs,
but their public/instance-local account models are not Google Premium sessions.
A hosted extractor can move work off the desktop, but server-IP blocking,
IP/session-bound URLs and media relay bandwidth remain. A paid third-party
scraping API changes who operates that machinery; it does not establish a
supported Music/Premium integration. Relaying customer Google cookies to a
third-party backend would also be a much larger trust boundary than Uta has now.

[NewPipeExtractor](https://github.com/TeamNewPipe/NewPipeExtractor) is a mature
public-content alternative with a Java and GPL-3.0 integration cost.
[Music Assistant's YouTube Music provider](https://github.com/music-assistant/beta.music-assistant.io/blob/main/docs/music-providers/youtube-music.md)
still depends on cookies/token machinery and Premium; adding it is unlikely to
simplify a standalone desktop installation.

Using mpv's built-in YouTube hook still generally means using yt-dlp. libmpv,
GStreamer or Rodio change decoding/control and potentially transition latency,
not access to YouTube streams. RSS and oEmbed only cover narrow metadata uses.
Cast/phone control and an Android container could be optional modes, but do not
meet the local standalone-player requirement. Official
[supported Music devices](https://support.google.com/youtubemusic/answer/9231765?hl=en)
and [Sonos SMAPI](https://docs.sonos.com/docs/smapi) demonstrate separate partner
integrations, not a generally available YouTube playback API for Uta.

## Suggested implementation sequence

1. **Evaluate RustyPipe on the user's other computer.** Follow the handoff below.
   Establish which Uta operations it can replace, whether the actual account is
   used for playback, and what processes/resources the complete path requires.
   Keep this as a focused prototype before changing the application backend.
2. **Keep the current backend available; improve it where useful.** Maintain
   session-scoped clients in supervised workers. Give interactive playback a
   separate lane from large library requests; do not put every request behind one
   blocking queue. Bound concurrency and avoid sharing mutable client objects
   unsafely. Cancel obsolete requests, retain generation checks, apply operation
   deadlines and restart workers when needed. Return first pages and cache
   normalized metadata with sensible expiry. Keep credentials in Secret Service,
   not general client caches. These changes preserve the existing service schema
   where practical and should be measured before a rewrite.
3. **Choose the native service and resolver boundary from the evidence.** Compare
   RustyPipe with a narrow Rust implementation, using ytmapi-rs where it covers
   the needed Music operations. Request player data without related-video/browse
   work. For a custom resolver, prototype authenticated WEB_REMIX with cached
   QuickJS solving and an on-demand yt-dlp fallback. An upstream minimal DisTube
   API remains an alternative if its runtime cost is acceptable. Choose explicit
   behavior for signed-in accounts;
   never silently substitute an anonymous stream while claiming Premium use.
   Validate media before promoting a path; on expiry/403 invalidate and retry
   through the appropriate fallback with a bounded deadline.
4. **Preserve browser-independent operation and the current connection flow.**
   Test account selection, cookie rotation, logout and expired sessions. Revisit
   a connection-only extension or temporary login window only if it materially
   improves these cases. Do not add browser-backed playback to the implementation
   plan under the current product priorities.
5. **Optimize the audio process after the access path.** A retained mpv or libmpv
   instance could remove process/IPC setup and improve transitions, but retain
   explicit track-generation ownership. Existing successful prefetching already
   hides most extraction time for the next track; improve random access first.

Before selecting a replacement for paying customers, test real Premium and free
accounts separately: full-track playback and seeking; highest available quality;
multiple Google accounts and brand channels; account switching/revocation;
long queues; regional/age restrictions; uploads if offered; suspend/resume and
network changes; radio continuity; and history/recommendation behavior. Up to
256 kbps AAC/Opus is a documented
[Premium quality option](https://support.google.com/youtubemusic/answer/9076559?hl=en),
but one codec/bitrate or a supplied cookie jar is not proof of correct entitlement.

The key next experiment is an authenticated comparison on a consenting Premium
test account. That work remains unperformed. Anonymous benchmark success should
not be used to claim this product requirement is already solved.

## RustyPipe handoff

The research branch contains the report, four standalone benchmark scripts and
seven sanitized JSON records. It changes no application dependencies or production
code. The user plans to perform the RustyPipe experiment elsewhere; the following
is a suggested evaluation, not completed work.

Start a separate prototype branch from this research branch on the other computer:

```sh
git fetch origin
git switch -c research/rustypipe-native origin/research/youtube-integration-options
```

The version inspected during research was 0.11.4. Pin the version selected for the
experiment and record it with the Rust toolchain and any helper versions. Start
with the [RustyPipe API documentation](https://docs.rs/rustypipe/latest/rustypipe/)
and [repository](https://github.com/TeamPiped/rustypipe), including its linked
upstream project and BotGuard documentation.

| Evaluation | Concrete check |
| --- | --- |
| Feature parity | Map Uta's `handle()` operations to RustyPipe: top/filtered search, liked songs, library albums/playlists, album/playlist tracks, history, radio/watch queue, like/unlike, account identity and audio resolution. Record gaps rather than silently dropping behavior. |
| Pagination | Return the first page promptly and follow explicit continuations for long lists; compare against the retained ytmusicapi first-page baseline. |
| Authentication | Use the existing cookie/session model initially. Verify the selected Google account and brand channel for both metadata and playback; cookies alone do not prove account fidelity. |
| Authentication API | RustyPipe documents TV-only OAuth with limited capabilities; use its cookie-authenticated desktop/Music path for account-library evaluation. Check which queries need explicit `.authenticated()` rather than relying on automatic fallback. |
| Premium playback | Identify the selected client and audio format; require substantial media delivery, full-track playback and seeking. Detect anonymous fallback explicitly and verify behavior with the browser closed. |
| Token/helper cost | Determine when `rustypipe-botguard` is invoked, whether any external runtime/browser process is required, and its cold/warm memory and CPU costs. Do not infer total footprint from the Rust library alone. |
| Cache integration | Adapt credential persistence to Uta's Secret Service storage and disable default raw-response reports for ordinary use. Keep reusable non-secret player/cache artifacts distinct from account state. |
| Recovery | Test expired cookies/URLs, account switching, HTTP 403, network changes and worker failures. Preserve Uta's generation checks and prevent obsolete work from changing current playback. |
| Distribution | Review GPL-3.0 requirements against Uta's existing MIT OR Apache-2.0 distribution and document the chosen approach before integration. |
| Performance | Compare equivalent operations and audio quality. Record process-tree PSS/peak memory, idle CPU, first-page latency, resolve time and actual playback start, with all helpers included. |

Begin with public content and the IDs in the baseline JSON, then use an explicitly
authorized test account for private-library and Premium checks. Keep credentials,
signed media URLs and raw authenticated responses out of committed fixtures and
logs. The existing benchmark's mpv probe can check initial silent playback;
full-track and seek validation must be added to the evaluation separately.

An affirmative result should demonstrate native operation with the browser closed,
acceptable account/feature coverage, a lower measured resource budget, and a
maintainable fallback for unsupported playback. A negative result is still useful
if it identifies the specific feature, authentication or helper cost that favors
the narrower Rust-plus-QuickJS alternative. No fixed memory or latency acceptance
threshold has been agreed yet.

## Artifacts and reproduction

All JSON contains public video IDs, versions, timings, counts, endpoint
categories and sanitized outcomes. It excludes signed URLs, cookies, response
bodies and raw third-party error messages. `ok`/summary `successes` means the
timed operation returned, **not** necessarily successful playback; inspect
`playback`, `media_ok` and HTTP status fields.

- [Baseline](research/youtube-benchmark-2026-10-27.json)
- [First-page comparison](research/youtube-paging-2026-10-27.json)
- [YouTube.js media validation](research/youtubejs-playback-2026-10-27.json)
- [DisTube defaults and mweb](research/distube-2026-10-27.json)
- [DisTube optional-metadata experiment](research/distube-omit-next-2026-10-27.json)
- [Latest yt-dlp](research/ytdlp-latest-2026-10-27.json)
- [Interleaved yt-dlp versions](research/ytdlp-versions-2026-10-27.json)

Run from the repository root. These contact public YouTube anonymously and
briefly decode audio to mpv's silent sink. Repeated results will change with the
service and network. Use a new output path to preserve the recorded evidence.

```sh
nix develop --command python3 scripts/benchmark-youtube \
  --rounds 5 --output /tmp/uta-baseline.json
nix develop --command python3 scripts/benchmark-youtube \
  --phase paging --rounds 5 --output /tmp/uta-paging.json

nix develop --command env -u LD_LIBRARY_PATH \
  DENO_DIR=/tmp/uta-youtube-research-deno \
  deno run --no-config --no-lock --unstable-worker-options --allow-net \
  --allow-run=python3 --allow-write=/tmp/uta-youtubejs.json \
  scripts/benchmark-youtubejs.mjs /tmp/uta-youtubejs.json

nix shell github:NixOS/nixpkgs/7fd4004d0511991fff3a0524856a9be09e28c5f0#nodejs_24 \
  --command npm install --prefix /tmp/uta-distube-research \
  --ignore-scripts --no-audit --no-fund @distube/ytdl-core@5.0.2
nix develop --command nix shell \
  github:NixOS/nixpkgs/7fd4004d0511991fff3a0524856a9be09e28c5f0#nodejs_24 \
  --command node scripts/benchmark-distube.mjs \
  /tmp/uta-distube-research/node_modules/@distube/ytdl-core/dist/index.js \
  /tmp/uta-distube.json
# Append `omit-next` and use another output path for the optional-metadata experiment.
```

The version-comparison harness expects the 2026.10.26 wheel unpacked to a
directory, supplied as its first argument. The wheel used here has SHA-256
`a05c5b5d8562f7e8a62f5334348b406af4930d56d3a65f8021d14af0e22339ae`.
Obtain that version from PyPI, verify the digest and unpack outside the repository.

```sh
nix develop --command python3 scripts/benchmark-ytdlp-versions \
  /tmp/uta-ytdlp-2026.10.26 /tmp/uta-ytdlp-versions.json
nix develop --command env PYTHONPATH=/tmp/uta-ytdlp-2026.10.26 \
  python3 scripts/benchmark-youtube --phase extract --rounds 5 \
  --ids-from docs/research/youtube-benchmark-2026-10-27.json \
  --output /tmp/uta-ytdlp-latest.json
```

The Python request tracer includes requests-session body handling, while
yt-dlp/JavaScript fetch traces primarily measure response-header availability;
do not add or compare those component fields as equivalent network timings.
No full browser-backed or Rust-library benchmark was performed.

Validation of the unchanged application during this investigation:
`python3 -m unittest discover -s service/tests -q` (24 passed), and
`cargo test --test network -- --ignored --nocapture` (1 passed), both in
`nix develop`. The latter exercised Uta's service and actual silent mpv playback.
