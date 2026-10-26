# Uta 歌 — YouTube Music

Uta is a native YouTube Music client project for
[Aizome](https://github.com/bitemyapp/aizome),
[Sekka](https://github.com/bitemyapp/sekka)'s desktop environment.
*Uta* means “song,” following Aizome's convention of Japanese names that
describe an app's purpose.

**Status:** Initial project setup. The application is not implemented yet.

## Goals

- A native Rust interface built with Aizome's `aizome-kit` and
  [Sidestep](https://github.com/bitemyapp/sidestep).
- Search, playback, queues, playlists, and access to the user's music library.
- Account linking without requiring users to create Google API credentials.
- Authenticated playback that uses the listener's YouTube Music Premium
  benefits where supported by the service and playback backend.
- Aizome's keyboard commands, menus, command palette, and Day and Night themes.

## App identity

The planned identifiers follow [Aizome's app conventions](https://github.com/bitemyapp/aizome/blob/main/docs/apps.md):

| Field | Value |
| --- | --- |
| Name | Uta 歌 |
| Generic name | YouTube Music |
| Binary | `aizome-uta` |
| App ID | `org.aizome.Uta` |
