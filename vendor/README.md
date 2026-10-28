# RustyPipe

`rustypipe/` contains the library source, normalized Cargo manifest, README and
GPL-3.0 license from the published **0.11.4** crate:

- Source: <https://crates.io/api/v1/crates/rustypipe/0.11.4/download>
- Archive SHA-256: `3bf8d7c42ac7860a078f63f90d6513dd3833585337e65b5fca9f74ddba2d59ef`
- Upstream: <https://codeberg.org/ThetaDev/rustypipe>

The sole source change is recorded in `rustypipe-visionos.patch`: an explicit,
anonymous `ClientType::VisionOs`, its request context and transport headers, and
omission of an empty platform value (YouTube rejects that enum value). Client
constants match the working VisionOS context in the pinned yt-dlp 2026.8.19.
Existing clients and the upstream default client order are unchanged. Uta selects
VisionOS explicitly for anonymous playback; it cannot authenticate an account.

This patch is needed because the released RustyPipe iOS client resolved URLs
which failed both mpv and a 2 MiB media read with HTTP 403 on the test network.
The TV/desktop paths also failed extraction. The patched client is tested for
actual media delivery, not just successful API responses.

The dependency is optional. Uta's own source remains MIT OR Apache-2.0; the
combined executable built with `--features rustypipe` includes GPL-3.0-only code.
The `uta-rustypipe` Nix package declares that license. The ordinary package does
not link this library. Keep the upstream license and this provenance with the
vendored source; do not describe the combined build as exclusively permissive.
