//! Retained, anonymous RustyPipe adapter. Account requests stay on the established
//! helper until cookie/account selection and Premium playback have been verified.
use crate::{
    model::{Collection, Stream, Track},
    service::{Live, Reply, Request, SONG_LIMIT, Service},
};
use rustypipe::{
    client::{ClientType, RustyPipe},
    model::{AlbumItem, ArtistId, TrackItem},
    param::StreamFilter,
};
use std::{collections::HashSet, time::Duration};

pub struct Native {
    pipe: RustyPipe,
    runtime: tokio::runtime::Runtime,
}

impl Native {
    pub fn new() -> Result<Self, String> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|_| "Could not start native music service")?;
        let _guard = runtime.enter();
        let pipe = RustyPipe::builder()
            .no_storage()
            .no_reporter()
            .no_botguard()
            .unauthenticated()
            .n_http_retries(0)
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| "Could not start native music service")?;
        drop(_guard);
        Ok(Self { pipe, runtime })
    }

    /// No fallback: exposes native failures to the anonymous benchmark.
    pub fn call_native(&self, request: &Request) -> Result<Reply, String> {
        self.runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(20), self.request(request))
                .await
                .map_err(|_| "Native music request timed out".to_string())?
                .map_err(|_| "Native music request failed".to_string())
        })
    }

    async fn request(&self, request: &Request) -> Result<Reply, rustypipe::error::Error> {
        let query = self.pipe.query().unauthenticated();
        let mut reply = Reply {
            backend: "rustypipe".into(),
            ..Reply::default()
        };
        match request {
            Request::Search {
                query: text,
                filter,
            } => {
                let text = text.trim();
                if text.is_empty() || text.chars().count() > 500 {
                    return Err(rustypipe::error::Error::Other("invalid search".into()));
                }
                match filter.as_str() {
                    "songs" | "videos" => {
                        let mut found = if filter == "songs" {
                            query.music_search_tracks(text).await?
                        } else {
                            query.music_search_videos(text).await?
                        };
                        found.items.extend_limit(&query, 40).await?;
                        reply.tracks = tracks(found.items.items.into_iter().take(40));
                    }
                    "albums" => {
                        let mut found = query.music_search_albums(text).await?;
                        found.items.extend_limit(&query, 40).await?;
                        reply.collections =
                            found.items.items.into_iter().take(40).map(album).collect();
                    }
                    // The current top-results layout is not parsed by 0.11.4;
                    // RustyPipe also splits community and curated playlists.
                    // Preserve those searches in the Python adapter.
                    _ => return Err(rustypipe::error::Error::Other("unsupported search".into())),
                }
            }
            Request::Collection { id, kind } if valid_collection_id(id) => {
                if kind == "album" {
                    let found = query.music_album(id).await?;
                    reply.title = found.name;
                    reply.tracks = tracks(found.tracks.into_iter().take(SONG_LIMIT));
                } else if kind == "playlist" {
                    let mut found = query.music_playlist(id).await?;
                    found.tracks.extend_limit(&query, SONG_LIMIT).await?;
                    reply.title = found.name;
                    reply.tracks = tracks(found.tracks.items.into_iter().take(SONG_LIMIT));
                } else {
                    return Err(rustypipe::error::Error::Other(
                        "unsupported collection".into(),
                    ));
                }
            }
            Request::Watch { id, radio: true } if valid_video_id(id) => {
                let mut found = query.music_radio_track(id).await?;
                found.extend_limit(&query, 50).await?;
                reply.title = "Radio".into();
                reply.tracks = tracks(found.items.into_iter().take(50));
            }
            Request::Resolve { id } if valid_video_id(id) => {
                let player = query.player_from_client(id, ClientType::VisionOs).await?;
                let audio = player
                    .select_audio_stream(&StreamFilter::default())
                    .filter(|audio| audio.url.starts_with("https://"))
                    .ok_or_else(|| rustypipe::error::Error::Other("no audio stream".into()))?;
                reply.stream = Some(Stream {
                    url: audio.url.clone(),
                    headers: [(
                        "User-Agent".into(),
                        query.user_agent(player.client_type).into_owned(),
                    )]
                    .into(),
                    codec: serde_json::to_value(audio.codec)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .unwrap_or_else(|| "audio".into()),
                    bitrate: f64::from(audio.average_bitrate) / 1000.0,
                    authenticated: false,
                    native: true,
                });
            }
            _ => return Err(rustypipe::error::Error::Other("unsupported request".into())),
        }
        Ok(reply)
    }
}

fn supported(request: &Request, authenticated: bool) -> bool {
    !authenticated
        && match request {
            Request::Search { filter, .. } => {
                matches!(filter.as_str(), "songs" | "videos" | "albums")
            }
            Request::Collection { .. }
            | Request::Resolve { .. }
            | Request::Watch { radio: true, .. } => true,
            _ => false,
        }
}

impl Service for Native {
    fn call(&self, request: Request, authenticated: bool) -> Result<Reply, String> {
        dispatch(
            request,
            authenticated,
            |request| self.call_native(request),
            &Live,
        )
    }
}

fn dispatch(
    request: Request,
    authenticated: bool,
    native: impl FnOnce(&Request) -> Result<Reply, String>,
    fallback: &dyn Service,
) -> Result<Reply, String> {
    if supported(&request, authenticated) {
        if let Ok(reply) = native(&request) {
            return Ok(reply);
        }
        // Never print the upstream error: it may contain URLs/response bodies.
        log::debug!("Native music request failed; trying Python service");
    }
    fallback.call(request, authenticated)
}

fn valid_video_id(id: &str) -> bool {
    id.len() == 11 && valid_collection_id(id)
}

fn valid_collection_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 200
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

fn artists(items: &[ArtistId]) -> String {
    items
        .iter()
        .map(|a| a.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn tracks(items: impl IntoIterator<Item = TrackItem>) -> Vec<Track> {
    let mut seen = HashSet::new();
    items
        .into_iter()
        .filter(|t| valid_video_id(&t.id) && seen.insert(t.id.clone()))
        .map(|t| Track {
            id: t.id,
            title: t.name,
            artists: artists(&t.artists),
            album: t.album.map(|a| a.name).unwrap_or_default(),
            duration: f64::from(t.duration.unwrap_or_default()),
            thumbnail: t
                .cover
                .into_iter()
                .max_by_key(|t| t.width)
                .map(|t| t.url)
                .unwrap_or_default(),
            liked: false,
        })
        .collect()
}

fn album(item: AlbumItem) -> Collection {
    let mut subtitle = artists(&item.artists);
    if let Some(year) = item.year {
        if !subtitle.is_empty() {
            subtitle.push_str(" · ");
        }
        subtitle.push_str(&year.to_string());
    }
    Collection {
        id: item.id,
        title: item.name,
        subtitle,
        kind: "album".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Fallback(Mutex<Vec<(String, bool)>>);
    impl Service for Fallback {
        fn call(&self, request: Request, authenticated: bool) -> Result<Reply, String> {
            self.0
                .lock()
                .unwrap()
                .push((serde_json::to_string(&request).unwrap(), authenticated));
            Err("helper rejected session".into())
        }
    }

    #[test]
    fn connected_requests_never_enter_anonymous_adapter_or_retry_anonymously() {
        let fallback = Fallback::default();
        for request in [
            Request::Resolve {
                id: "Az4SnX521pI".into(),
            },
            Request::Search {
                query: "music".into(),
                filter: "songs".into(),
            },
            Request::Liked,
            Request::Rate {
                id: "Az4SnX521pI".into(),
                like: true,
            },
        ] {
            assert!(dispatch(request, true, |_| panic!("must not run native"), &fallback).is_err());
        }
        let calls = fallback.0.lock().unwrap();
        assert_eq!(calls.len(), 4);
        assert!(calls.iter().all(|(_, authenticated)| *authenticated));
    }

    #[test]
    fn failed_native_request_falls_back_once_with_original_request() {
        let request = Request::Resolve {
            id: "Az4SnX521pI".into(),
        };
        let expected = serde_json::to_string(&request).unwrap();
        let fallback = Fallback::default();
        assert_eq!(
            dispatch(
                request,
                false,
                |_| Err("upstream URL must stay private".into()),
                &fallback
            )
            .unwrap_err(),
            "helper rejected session"
        );
        assert_eq!(*fallback.0.lock().unwrap(), vec![(expected, false)]);
    }

    #[test]
    fn successful_native_request_does_not_start_helper() {
        let fallback = Fallback::default();
        let result = dispatch(
            Request::Resolve {
                id: "Az4SnX521pI".into(),
            },
            false,
            |_| {
                Ok(Reply {
                    backend: "rustypipe".into(),
                    ..Reply::default()
                })
            },
            &fallback,
        )
        .unwrap();
        assert_eq!(result.backend, "rustypipe");
        assert!(fallback.0.lock().unwrap().is_empty());
    }

    #[test]
    fn account_operations_and_different_playlist_search_semantics_use_helper() {
        for request in [
            Request::Status,
            Request::Discover,
            Request::Disconnect,
            Request::Connect {
                source: "synthetic".into(),
            },
            Request::Search {
                query: "music".into(),
                filter: "playlists".into(),
            },
            Request::Watch {
                id: "Az4SnX521pI".into(),
                radio: false,
            },
        ] {
            assert!(!supported(&request, false));
        }
    }

    #[test]
    fn track_mapping_deduplicates_and_rejects_invalid_ids() {
        let track: TrackItem = serde_json::from_value(serde_json::json!({
            "id": "Az4SnX521pI", "name": "Example", "duration": 123,
            "cover": [{"url":"https://i.ytimg.com/low.jpg", "width":120, "height":120}, {"url":"https://i.ytimg.com/high.jpg", "width":300, "height":300}],
            "artists": [{"id":null,"name":"Artist"}], "artist_id":null,
            "album":{"id":"MPREexample","name":"Album"}, "view_count":null,
            "track_type":"track", "track_nr":null, "by_va":false
        })).unwrap();
        let mut invalid = track.clone();
        invalid.id = "invalid".into();
        let mapped = tracks([track.clone(), track, invalid]);
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].artists, "Artist");
        assert_eq!(mapped[0].album, "Album");
        assert_eq!(mapped[0].duration, 123.0);
        assert_eq!(mapped[0].thumbnail, "https://i.ytimg.com/high.jpg");
        assert!(!mapped[0].liked);
    }
}
