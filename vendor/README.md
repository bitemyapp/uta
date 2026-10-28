# RustyPipe

`rustypipe/` contains the library source, normalized Cargo manifest, README and
GPL-3.0 license from the published **0.11.4** crate:

- Source: <https://crates.io/api/v1/crates/rustypipe/0.11.4/download>
- Archive SHA-256: `3bf8d7c42ac7860a078f63f90d6513dd3833585337e65b5fca9f74ddba2d59ef`
- Upstream: <https://codeberg.org/ThetaDev/rustypipe>

The original source change is recorded in `rustypipe-visionos.patch`: an explicit,
anonymous `ClientType::VisionOs`, its request context and transport headers, and
omission of an empty platform value (YouTube rejects that enum value). Client
constants match the working VisionOS context in the pinned yt-dlp 2026.8.19.
Existing clients and the upstream default client order are unchanged. Uta selects
VisionOS explicitly for anonymous playback; it cannot authenticate an account.

This patch is needed because the released RustyPipe iOS client resolved URLs
which failed both mpv and a 2 MiB media read with HTTP 403 on the test network.
The TV/desktop paths also failed extraction. The patched client is tested for
actual media delivery, not just successful API responses.

The dependency is enabled by default. Uta's own source remains MIT OR Apache-2.0;
the combined executable includes GPL-3.0-only code. The default `uta` Nix package
and its `uta-rustypipe` alias declare that license. `uta-python` and Cargo builds
with `--no-default-features` do not link this library.
Keep the upstream license and this provenance with the
vendored source; do not describe the combined build as exclusively permissive.

## Premium branch additions

The Premium branch additionally preserves/checks the explicitly selected account
index when importing a cookie session, accepts `__Secure-3PAPISID` when SAPISID is
absent, and adds an EJS fallback for current signature/throttling extraction.
QuickJS is updated from rquickjs 0.9.0 to pinned 0.14.0. A bounded worker
retains the current solver context across request threads.
See [EJS provenance](rustypipe/ejs/README.md). JavaScript runs without host APIs
and with memory and execution limits. The deobfuscation module is public for
integration tests against current public player scripts.

Uta still disables upstream disk storage, raw reports and external BotGuard.
Credentials are obtained through Uta's existing keyring helper, retained in memory,
and invalidated when the session expires or the account changes.
