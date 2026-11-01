"""Creator navigation and verified reverse-playlist discovery without network I/O."""
import unittest
from unittest.mock import Mock, patch

from test_service import service

VIDEO = "abcdefghijk"
CHANNEL = "UC" + "a" * 22
OTHER = "UC" + "b" * 22


def scan(ids):
    return {"video_id": VIDEO, "uploader_id": CHANNEL, "title": "A song", "candidates": ids,
            "checked": 0, "unavailable": 0, "incomplete": 0, "discovery_failures": 0}


class BrowsingTests(unittest.TestCase):
    def test_artist_count_uses_distinct_credits_and_thumbnails_survive_mapping(self):
        images = [{"url": "https://i.ytimg.com/tiny", "width": 60},
                  {"url": "https://i.ytimg.com/row", "width": 300},
                  {"url": "https://i.ytimg.com/huge", "width": 1920}]
        row = service.track({"videoId": VIDEO, "artists": [{"id": CHANNEL, "name": "Earth, Wind & Fire"}], "thumbnails": images})
        self.assertEqual(row["artist_count"], 1)
        self.assertEqual(row["thumbnail"], images[1]["url"])
        row = service.track({"videoId": VIDEO, "artists": [{"id": CHANNEL, "name": "One"},
            {"id": OTHER, "name": "Two"}, {"id": CHANNEL, "name": "One"}]})
        self.assertEqual(row["artist_count"], 2)
        album = service.collection({"browseId": "MPREb_album", "title": "Album", "thumbnails": images}, "album")
        self.assertEqual(album["thumbnail"], images[1]["url"])

    def test_artist_search_uses_artist_name_and_browse_id(self):
        api = Mock()
        api.search.return_value = [{"artist": "The artist", "browseId": CHANNEL}]
        with patch.object(service, "music", return_value=api):
            result = service.handle({"op": "search", "query": "artist", "filter": "artists"})
        self.assertEqual(result["collections"], [{**service.folder(CHANNEL, "The artist", "artist"), "thumbnail": ""}])

    def test_track_artists_use_exact_video_and_preserve_multiple_artists(self):
        api = Mock()
        api.get_watch_playlist.return_value = {"tracks": [
            {"videoId": "other_video", "artists": [{"id": OTHER, "name": "Wrong"}]},
            {"videoId": VIDEO, "artists": [{"id": CHANNEL, "name": "One"},
                {"id": OTHER, "name": "Two"}, {"id": CHANNEL, "name": "Duplicate"}, {"name": "Missing ID"}]}]}
        result = service.browse_creator(api, VIDEO, "track_artists")
        self.assertEqual([r["title"] for r in result["collections"]], ["One", "Two"])

    def test_single_artist_and_matching_uploader_open_the_same_songs_first_catalog(self):
        api = Mock()
        api.get_watch_playlist.return_value = {"tracks": [{"videoId": VIDEO,
            "artists": [{"id": CHANNEL, "name": "Artist"}, {"id": CHANNEL, "name": "Artist"}]}]}
        api.get_song.return_value = {"videoDetails": {"channelId": CHANNEL, "author": "Artist"}}
        api.get_artist.return_value = {"name": "Artist", "songs": {"results": []},
                                      "albums": {"results": []}}
        artist = service.browse_creator(api, VIDEO, "track_artists")
        uploader = service.browse_creator(api, VIDEO, "track_uploader")
        self.assertEqual(artist, uploader)
        self.assertNotIn("collections", artist)
        self.assertEqual(artist["browse"]["tabs"][0]["kind"], "artist_songs")
        self.assertTrue(all(r["id"] == CHANNEL for r in artist["browse"]["tabs"]))

    def test_songs_is_default_even_when_the_artist_only_lists_albums(self):
        api = Mock()
        api.get_artist.return_value = {"name": "Artist", "albums": {"results": []}}
        result = service.browse_creator(api, CHANNEL, "artist")
        self.assertEqual(result["browse"]["tabs"][0]["kind"], "artist_songs")
        self.assertEqual(service.browse_creator(api, CHANNEL, "artist_songs")["tracks"], [])

    def test_artist_sections_follow_full_catalog_pointers(self):
        api = Mock()
        api.get_artist.return_value = {"name": "Artist", "songs": {"browseId": "VLPLsongs"},
                                      "albums": {"browseId": CHANNEL, "params": "albums-param"}}
        api.get_playlist.return_value = {"tracks": [{"videoId": VIDEO, "title": "Song"}]}
        api.get_artist_albums.return_value = [{"browseId": "MPREb_album", "title": "Album"}]
        landing = service.browse_creator(api, CHANNEL, "artist")
        self.assertEqual([r["kind"] for r in landing["browse"]["tabs"]],
                         ["artist_songs", "artist_albums", "channel_uploads", "channel_playlists"])
        self.assertEqual(service.browse_creator(api, CHANNEL, "artist_songs")["tracks"][0]["id"], VIDEO)
        api.get_playlist.assert_called_once_with("PLsongs", limit=1000)
        self.assertEqual(service.browse_creator(api, CHANNEL, "artist_albums")["collections"][0]["id"], "MPREb_album")
        api.get_artist_albums.assert_called_once_with(CHANNEL, "albums-param", limit=100)

    def test_uploader_identity_comes_from_video_not_artist_display_name(self):
        api = Mock()
        api.get_song.return_value = {"videoDetails": {"channelId": CHANNEL, "author": "Uploader"}}
        api.get_artist.side_effect = KeyError("musicImmersiveHeaderRenderer")
        result = service.browse_creator(api, VIDEO, "track_uploader")
        self.assertEqual(result["title"], "Uploader")
        self.assertEqual([r["id"] for r in result["browse"]["tabs"]], [CHANNEL, CHANNEL])
        api.get_artist.assert_called_once_with(CHANNEL)
        api.get_watch_playlist.assert_not_called()
        with patch.object(service, "channel_catalog", return_value={"channel": "Uploader", "entries": [
            {"id": VIDEO, "title": "Upload", "duration": 23}, None, {"id": "bad"}]}):
            rows = service.browse_creator(api, CHANNEL, "channel_uploads")["tracks"]
            self.assertEqual([(r["id"], r["artists"]) for r in rows], [(VIDEO, "Uploader")])

    def test_membership_is_verified_and_uploader_is_compared_by_id(self):
        api = Mock()
        api.get_playlist.side_effect = [
            {"title": "Other", "author": {"id": OTHER, "name": "same name"}, "tracks": [{"videoId": VIDEO}]},
            {"title": "False positive", "tracks": [{"videoId": "another_song", "title": "A song"}]},
            {"title": "Uploader", "author": {"id": CHANNEL, "name": "same name"}, "tracks": [{"videoId": VIDEO}]}]
        result = service.track_playlists(api, VIDEO, None, scan(["PLother", "PLwrong", "PLuploader"]))
        self.assertEqual([r["id"] for r in result["collections"]], ["PLuploader", "PLother"])
        self.assertEqual([r["uploader_owned"] for r in result["collections"]], [True, False])
        self.assertIsNone(result["playlist_scan"])

    def test_uploader_tabs_keep_video_channel_even_if_music_returns_another_channel(self):
        api = Mock()
        api.get_song.return_value = {"videoDetails": {"channelId": CHANNEL}}
        api.get_artist.return_value = {"name": "Artist", "channelId": OTHER, "songs": {"results": []}}
        tabs = service.browse_creator(api, VIDEO, "track_uploader")["browse"]["tabs"]
        self.assertTrue(all(tab["id"] == CHANNEL for tab in tabs))

    def test_batches_continue_without_repeating_discovery_or_checks(self):
        api = Mock()
        api.get_playlist.return_value = {"tracks": []}
        result = service.track_playlists(api, VIDEO, None, scan([f"PL{i}" for i in range(8)]))
        self.assertEqual(result["playlist_scan"]["checked"], 6)
        final = service.track_playlists(api, VIDEO, None, result["playlist_scan"])
        self.assertIsNone(final["playlist_scan"])
        self.assertEqual([call.args[0] for call in api.get_playlist.call_args_list], [f"PL{i}" for i in range(8)])
        api.get_song.assert_not_called()
        api.search.assert_not_called()

    def test_discovery_deduplicates_and_checks_owner_candidates_first(self):
        api = Mock()
        api.get_song.return_value = {"videoDetails": {"channelId": CHANNEL, "title": "A song"}}
        api.get_library_playlists.return_value = [{"playlistId": "PLown"}, {"playlistId": "PLsaved"}]
        api.search.return_value = [{"browseId": "VLPLsearch"}, {"playlistId": "PLown"}]
        api.get_playlist.return_value = {"tracks": []}
        with patch.object(service, "channel_catalog", return_value={"entries": [None, {"id": "PLown"}]}):
            service.track_playlists(api, VIDEO, {"connected": True})
        self.assertEqual([call.args[0] for call in api.get_playlist.call_args_list], ["PLown", "PLsaved", "PLsearch"])

    def test_partial_failures_and_long_playlists_are_reported(self):
        api = Mock()
        api.get_playlist.side_effect = [RuntimeError("private URL must not leak"), {"tracks": [{"videoId": "other"}] * 1000}]
        result = service.track_playlists(api, VIDEO, None, scan(["PLprivate", "PLlong"]))
        self.assertIn("1 playlists unreadable", result["message"])
        self.assertIn("1 large playlists only partly checked", result["message"])
        self.assertNotIn("private URL", str(result))
        self.assertEqual(result["collections"], [])

    def test_invalid_continuations_are_rejected_before_network(self):
        api = Mock()
        for field, value in [("video_id", "different00"), ("checked", -1), ("checked", 4),
                             ("candidates", ["https://evil.invalid"]), ("candidates", ["PLx"] * 301)]:
            cursor = scan(["PLx"])
            cursor[field] = value
            with self.assertRaises(service.UserError):
                service.track_playlists(api, VIDEO, None, cursor)
        api.get_playlist.assert_not_called()

    def test_unknown_uploader_or_invalid_artist_does_not_trigger_catalog_fetch(self):
        api = Mock()
        api.get_song.return_value = {"videoDetails": {}}
        for identity, kind in [(VIDEO, "track_uploader"), ("bad", "artist"), ("../etc", "channel")]:
            with self.assertRaises(service.UserError):
                service.browse_creator(api, identity, kind)
        api.get_artist.assert_not_called()

    def test_non_catalog_artist_credit_opens_ordinary_uploader(self):
        api = Mock()
        api.get_artist.side_effect = KeyError("musicImmersiveHeaderRenderer")
        result = service.browse_creator(api, CHANNEL, "artist")
        self.assertEqual([r["kind"] for r in result["browse"]["tabs"]], ["channel_uploads", "channel_playlists"])
