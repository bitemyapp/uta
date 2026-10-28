import contextlib
import hashlib
import http.cookiejar
import importlib.util
import json
import os
import shutil
import sqlite3
import tempfile
import time
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

spec = importlib.util.spec_from_file_location("uta_service", Path(__file__).resolve().parents[1] / "uta_service.py")
service = importlib.util.module_from_spec(spec)
spec.loader.exec_module(service)


def cookie(domain, name="__Secure-3PAPISID", value="test-session", expires=None):
    return http.cookiejar.Cookie(0, name, value, None, False, domain, True, domain.startswith("."),
                                 "/", True, True, expires, expires is None, None, None, {}, False)


def session():
    return {"cookies": service.encode_cookies([cookie(".youtube.com")])}


class ServiceTests(unittest.TestCase):
    def test_native_session_requires_auth_and_preserves_selection_and_expiry(self):
        saved = session()
        saved["authuser"] = "2"
        with patch.object(service, "saved_session", return_value=saved):
            result = service.handle({"op": "native_session", "authenticated": True})
            self.assertEqual(result["credentials"]["authuser"], "2")
            self.assertIn("__Secure-3PAPISID=", result["credentials"]["cookie"])
            self.assertLessEqual(result["credentials"]["expires"], time.time() + 1800)
        with patch.object(service, "keyring", side_effect=AssertionError("no keyring")):
            with self.assertRaises(service.UserError):
                service.handle({"op": "native_session", "authenticated": False})
        saved["authuser"] = "0\r\nCookie: bad"
        with patch.object(service, "saved_session", return_value=saved):
            with self.assertRaises(service.UserError):
                service.handle({"op": "native_session", "authenticated": True})

    def test_fallback_preserves_selected_account_and_refuses_a_mismatch(self):
        from yt_dlp.extractor.youtube import YoutubeIE
        extractor = service.account_extractor("2")
        self.assertEqual(extractor.ie_key(), "Youtube")
        self.assertEqual(extractor._extract_session_index({"SESSION_INDEX": "2"}), 2)
        with self.assertRaises(service.UserError):
            extractor._extract_session_index({"SESSION_INDEX": "0"})
        with patch.object(YoutubeIE, "_download_webpage", return_value="html") as fetch:
            extractor._download_webpage("https://music.youtube.com/", "id", query={"keep": "yes"})
            self.assertEqual(fetch.call_args.kwargs["query"], {"keep": "yes", "authuser": "2"})

    def test_only_youtube_cookies_survive_and_expiry_is_respected(self):
        jar = [cookie(".youtube.com"), cookie("music.youtube.com", "SID"),
               cookie("mail.google.com"), cookie("youtube.com.evil.example"),
               cookie(".youtube.com", "expired", expires=int(time.time()) - 60)]
        rows = service.encode_cookies(jar)
        self.assertEqual({r["domain"] for r in rows}, {".youtube.com", "music.youtube.com"})
        self.assertEqual(len(rows), 2)
        headers = service.auth_headers(service.decode_cookies(rows))
        self.assertIn("__Secure-3PAPISID=test-session", headers["cookie"])
        self.assertNotIn("expired", headers["cookie"])
        self.assertEqual(headers["origin"], "https://music.youtube.com")

    def test_browser_auth_is_recognized_and_signed_by_ytmusicapi(self):
        from ytmusicapi import YTMusic
        headers = service.auth_headers(service.decode_cookies(session()["cookies"]))
        headers["X-Goog-Visitor-Id"] = "test-visitor"
        api = YTMusic(auth=headers)
        self.assertTrue(api.headers["authorization"].startswith("SAPISIDHASH "))
        self.assertNotIn("test-session", api.headers["authorization"])

    def test_invalid_or_expired_session_never_resolves_anonymously(self):
        with patch("yt_dlp.YoutubeDL") as downloader:
            with self.assertRaises(service.UserError):
                service.resolve("abcdefghijk", {"cookies": []})
            downloader.assert_not_called()

    def test_authenticated_resolution_passes_the_session_to_the_extractor(self):
        seen = {}

        class Downloader:
            def __init__(self, options):
                seen["options"] = options
                self.cookiejar = http.cookiejar.CookieJar()
            def add_info_extractor(self, extractor): seen["extractor"] = extractor
            def __enter__(self): return self
            def __exit__(self, *_args): pass
            def extract_info(self, url, download):
                seen["url"] = url
                seen["cookies"] = list(self.cookiejar)
                self_test.assertFalse(download)
                return {"url": "https://media.example/audio", "acodec": "opus", "abr": 251,
                        "http_headers": {"Cookie": "must-not-leak", "Authorization": "secret",
                                         "User-Agent": "test", "Origin": "bad\r\ninjection"}}

        self_test = self
        with patch("yt_dlp.YoutubeDL", Downloader):
            result = service.resolve("abcdefghijk", session())["stream"]
        self.assertEqual(seen["cookies"][0].value, "test-session")
        self.assertEqual(seen["url"], "https://music.youtube.com/watch?v=abcdefghijk")
        self.assertTrue(result["authenticated"])
        self.assertEqual(result["headers"], {"User-Agent": "test"})
        self.assertFalse(seen["options"]["cachedir"])

    def test_anonymous_search_does_not_touch_a_keyring_or_browser(self):
        api = Mock()
        api.search.return_value = [{"videoId": "abcdefghijk", "title": "A song", "duration": "3:21"}]
        with patch.object(service, "keyring", side_effect=AssertionError("must not read keyring")), patch.object(service, "music", return_value=api):
            result = service.handle({"op": "search", "query": "song", "filter": "songs", "authenticated": False})
        self.assertEqual(result["tracks"][0]["duration"], 201)

    def test_top_results_keep_youtube_music_order_and_include_videos(self):
        api = Mock()
        api.search.return_value = [
            {"category": "Top result", "resultType": "video", "videoId": "L0IcM-WLP98", "title": "A long mix"},
            {"resultType": "artist", "browseId": "UCx", "artist": "Someone"},
            {"resultType": "song", "videoId": "3h1Abo6j-Tk", "title": "A song", "duration": "2:01"},
            {"resultType": "video", "videoId": "L0IcM-WLP98", "title": "A long mix, again"},
            {"resultType": "album", "browseId": "MPREb_x", "title": "An album"}]
        with patch.object(service, "music", return_value=api):
            result = service.handle({"op": "search", "query": "mix", "authenticated": False})
        api.search.assert_called_once_with("mix", limit=40)
        self.assertEqual([t["title"] for t in result["tracks"]], ["A long mix", "A song"])
        with patch.object(service, "music", return_value=api):
            service.handle({"op": "search", "query": "mix", "filter": "videos", "authenticated": False})
        api.search.assert_called_with("mix", filter="videos", limit=40)

    def test_connected_account_failure_does_not_fall_back_to_anonymous(self):
        with patch.object(service, "saved_session", side_effect=service.UserError("Keyring locked")), patch.object(service, "music") as music:
            with self.assertRaisesRegex(service.UserError, "Keyring locked"):
                service.handle({"op": "search", "query": "song", "authenticated": True})
            music.assert_not_called()

    def test_failed_connection_preserves_the_previous_saved_session(self):
        vault = Mock()
        api = Mock()
        api.get_account_info.side_effect = RuntimeError("HTTP failed")
        with patch.object(service, "keyring", return_value=vault), patch.object(service, "music", return_value=api):
            with self.assertRaises(service.UserError):
                service.connect([cookie(".youtube.com")], {"id": "gecko:/x", "label": "Firefox"})
        vault.save.assert_not_called()

    def test_a_session_saved_by_an_earlier_version_still_reports_its_account(self):
        vault = Mock()
        vault.status.return_value = {"connected": True, "account": "YouTube account"}
        vault.load.return_value = {"account": "Listener", "cookies": []}
        with patch.object(service, "keyring", return_value=vault):
            status = service.handle({"op": "status"})
        self.assertEqual(status, {"connected": True, "account": "Listener", "source": "", "source_id": ""})

    def test_locked_saved_account_remains_connected_in_status(self):
        vault = Mock()
        vault.status.return_value = {"connected": True, "account": "YouTube account"}
        vault.load.side_effect = service.UserError("Keyring locked")
        with patch.object(service, "keyring", return_value=vault):
            status = service.handle({"op": "status"})
        self.assertTrue(status["connected"])
        self.assertEqual(status["message"], "Keyring locked")

    def test_unavailable_and_malformed_songs_are_omitted(self):
        rows = service.tracks([{"videoId": "abcdefghijk", "title": "Valid", "artists": [{"name": "Artist"}], "duration": "1:02:03"},
                               {"videoId": "abcdefghij-", "isAvailable": False}, {"title": "Deleted"},
                               {"videoId": "file:///etc/passwd"}])
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0]["duration"], 3723)
        self.assertEqual(rows[0]["artists"], "Artist")

    def test_playlists_from_search_and_the_library_both_open(self):
        found = service.collections([
            {"browseId": "VLPL8F6B0753B2CCA128", "playlistId": None, "title": "Jazz", "author": "A listener"},
            {"playlistId": "PLlibrary", "title": "Mine", "author": [{"name": "Me"}]},
            {"browseId": "MPREb_album", "title": "Not a playlist"}], "playlist")
        self.assertEqual([(c["id"], c["subtitle"]) for c in found], [("PL8F6B0753B2CCA128", "A listener"), ("PLlibrary", "Me")])

    def test_watch_lists_and_album_tracks_keep_their_length_artwork_and_album(self):
        watch = service.track({"videoId": "abcdefghijk", "title": "Radio song", "length": "3:07",
                               "thumbnail": [{"url": "https://i.ytimg.com/small"}, {"url": "https://i.ytimg.com/large"}],
                               "album": {"name": "An album", "id": "MPREb_x"}})
        self.assertEqual((watch["duration"], watch["thumbnail"], watch["album"]), (187, "https://i.ytimg.com/large", "An album"))
        album = {"title": "After the rain", "thumbnails": [{"url": "https://lh3.googleusercontent.com/cover"}]}
        row = service.tracks([{"videoId": "abcdefghijk", "title": "Soft light", "thumbnails": None, "album": None}], album)[0]
        self.assertEqual((row["album"], row["thumbnail"]), ("After the rain", "https://lh3.googleusercontent.com/cover"))


FUTURE = int(time.time()) + 86400 * 365
SIGNED = [("__Secure-3PAPISID", "sapisid"), ("__Secure-3PSID", "psid"), ("PREF", "f6=40000000")]


class Browsers(unittest.TestCase):
    """Synthetic browser profiles in a temporary home: never the developer's own."""

    def setUp(self):
        self.home = tempfile.TemporaryDirectory()
        self.addCleanup(self.home.cleanup)
        environment = patch.dict(os.environ, {"HOME": self.home.name, "XDG_CONFIG_HOME": "", "XDG_DATA_HOME": "",
                                              "XDG_RUNTIME_DIR": self.home.name, "XDG_CURRENT_DESKTOP": "Aizome"})
        environment.start()
        self.addCleanup(environment.stop)

    def path(self, *parts):
        path = os.path.join(self.home.name, *parts)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        return path

    def firefox(self, rows, profile="abcd1234.default-release", name="default-release"):
        root = self.path(".mozilla", "firefox", "profiles.ini")
        Path(root).write_text(f"[Profile0]\nName={name}\nIsRelative=1\nPath={profile}\n")
        db = sqlite3.connect(self.path(".mozilla", "firefox", profile, "cookies.sqlite"))
        db.execute("PRAGMA journal_mode=WAL")
        db.execute("PRAGMA wal_autocheckpoint=0")
        db.execute("PRAGMA user_version=16")
        db.execute("CREATE TABLE moz_cookies (id INTEGER PRIMARY KEY, originAttributes TEXT NOT NULL DEFAULT '', "
                   "name TEXT, value TEXT, host TEXT, path TEXT, expiry INTEGER, lastAccessed INTEGER, "
                   "isSecure INTEGER, isHttpOnly INTEGER)")
        for origin, name_, value, host, used in rows:
            db.execute("INSERT INTO moz_cookies (originAttributes, name, value, host, path, expiry, lastAccessed, isSecure, isHttpOnly) "
                       "VALUES (?, ?, ?, ?, '/', ?, ?, 1, 1)", (origin, name_, value, host, FUTURE * 1000, used * 1_000_000))
        db.commit()
        # The browser keeps it open: the sign-in stays in the write-ahead log.
        self.addCleanup(db.close)
        return db

    def chromium(self, rows, folder=("chromium",), profile="Default", meta=24, password=b"peanuts", version=b"v10"):
        root = self.path(".config", *folder, "Local State")
        Path(root).write_text(json.dumps({"profile": {"info_cache": {profile: {"name": "Person 1"}, "Profile 9": {"name": "Your Chromium"}}}}))
        db = sqlite3.connect(self.path(".config", *folder, profile, "Network", "Cookies"))
        db.execute("CREATE TABLE meta(key LONGVARCHAR NOT NULL UNIQUE PRIMARY KEY, value LONGVARCHAR)")
        db.execute("INSERT INTO meta VALUES ('version', ?)", (str(meta),))
        db.execute("CREATE TABLE cookies(host_key TEXT, top_frame_site_key TEXT, name TEXT, value TEXT, "
                   "encrypted_value BLOB, path TEXT, expires_utc INTEGER, is_secure INTEGER, last_access_utc INTEGER)")
        for host, name_, value, partition in rows:
            db.execute("INSERT INTO cookies VALUES (?, ?, ?, '', ?, '/', ?, 1, ?)",
                       (host, partition, name_, seal(value, host, password, version, meta >= 24),
                        (FUTURE + 11644473600) * 1_000_000, (int(time.time()) + 11644473600) * 1_000_000))
        db.commit()
        db.close()

    def test_a_fresh_firefox_sign_in_is_found_in_the_write_ahead_log(self):
        self.firefox([("", n, v, ".youtube.com", 1000) for n, v in SIGNED])
        # A copy of the database file alone, as yt-dlp makes, has none of it.
        alone = os.path.join(self.home.name, "alone.sqlite")
        shutil.copyfile(self.path(".mozilla", "firefox", "abcd1234.default-release", "cookies.sqlite"), alone)
        with contextlib.closing(sqlite3.connect(alone)) as db:
            self.assertEqual(db.execute("SELECT count(*) FROM sqlite_master").fetchone()[0], 0)
        found = service.discover()["sources"]
        self.assertEqual([(s["browser"], s["profile"], s["signed_in"]) for s in found], [("Firefox", "default-release", True)])
        source = service.source_from_id(found[0]["id"])
        values = {c.name: c.value for c in service.read_cookies(source)}
        self.assertEqual(values["__Secure-3PAPISID"], "sapisid")

    def test_containers_are_separate_sign_ins_and_partitioned_cookies_are_ignored(self):
        Path(self.path(".mozilla", "firefox", "abcd1234.default-release", "containers.json")).write_text(
            json.dumps({"identities": [{"userContextId": 2, "public": True, "l10nId": "user-context-work"}]}))
        self.firefox([("^userContextId=2", n, v, ".youtube.com", 2000) for n, v in SIGNED]
                     + [("^partitionKey=%28https%2Cexample.com%29", "__Secure-3PAPISID", "embedded", ".youtube.com", 3000),
                        ("^partitionKey=%28https%2Cyoutube.com%29&userContextId=2", "__Secure-ROLLOUT_TOKEN", "own", ".youtube.com", 3000),
                        ("^partitionKey=%28https%2Cyoutube.com%29&userContextId=2", "__Secure-3PSID", "stale", ".youtube.com", 3000),
                        ("", "__Secure-3PAPISID", "other-site", ".youtube.com.evil.example", 3000)])
        found = service.discover()["sources"]
        self.assertEqual([(s["profile"], s["signed_in"]) for s in found],
                         [("default-release, Work", True), ("default-release", False)])
        jar = http.cookiejar.CookieJar()
        for c in service.read_cookies(service.source_from_id(found[0]["id"])):
            jar.set_cookie(c)
        work = {c.name: c.value for c in jar}
        # YouTube's own partition is sent to music.youtube.com; an
        # unpartitioned cookie of the same name wins over it.
        self.assertEqual((work["__Secure-3PAPISID"], work["__Secure-ROLLOUT_TOKEN"], work["__Secure-3PSID"]),
                         ("sapisid", "own", "psid"))
        self.assertEqual(service.read_cookies(service.source_from_id(found[1]["id"])), [])

    def test_chromium_sealed_with_the_desktop_keyring_is_read_on_aizome(self):
        # Sekka starts Chromium with --password-store=gnome-libsecret, so its
        # cookies are v11. yt-dlp would pick the fixed key for an unknown desktop.
        self.chromium([(".youtube.com", n, v, "") for n, v in SIGNED]
                      + [(".youtube.com", "__Secure-3PAPISID", "partitioned", "https://example.com"),
                         (".youtube.com", "__Secure-ROLLOUT_TOKEN", "own", "https://youtube.com")],
                      password=b"from the keyring", version=b"v11")
        with patch.object(service, "safe_storage_passwords", side_effect=AssertionError("discovery must not unlock keys")):
            found = service.discover()["sources"]
        self.assertEqual([(s["browser"], s["profile"], s["signed_in"]) for s in found], [("Chromium", "Person 1", True)])
        with patch.object(service, "safe_storage_passwords", return_value=[b"another browser", b"from the keyring"]):
            values = {c.name: c.value for c in service.read_cookies(service.source_from_id(found[0]["id"]))}
        self.assertEqual(values, dict(SIGNED, **{"__Secure-ROLLOUT_TOKEN": "own"}))

    def test_older_and_unsealed_chromium_cookies_need_no_keyring(self):
        self.chromium([(".youtube.com", n, v, "") for n, v in SIGNED], folder=("BraveSoftware", "Brave-Browser"), meta=21)
        found = service.discover()["sources"]
        with patch.object(service, "safe_storage_passwords", side_effect=AssertionError("v10 needs no keyring")):
            values = {c.name: c.value for c in service.read_cookies(service.source_from_id(found[0]["id"]))}
        self.assertEqual(found[0]["browser"], "Brave")
        self.assertEqual(values["__Secure-3PSID"], "psid")

    def test_a_locked_keyring_is_explained_instead_of_saying_signed_out(self):
        self.chromium([(".youtube.com", n, v, "") for n, v in SIGNED], password=b"right", version=b"v11")
        source = service.source_from_id(service.discover()["sources"][0]["id"])
        with patch.object(service, "safe_storage_passwords", return_value=[]):
            with self.assertRaisesRegex(service.UserError, "keyring"):
                service.read_cookies(source)

    def test_sign_ins_come_first_and_unknown_profiles_are_refused(self):
        self.firefox([("", "PREF", "x", ".youtube.com", 1000)])
        self.chromium([(".youtube.com", n, v, "") for n, v in SIGNED])
        found = service.discover()["sources"]
        self.assertEqual([(s["browser"], s["signed_in"]) for s in found], [("Chromium", True), ("Firefox", False)])
        with self.assertRaisesRegex(service.UserError, "no longer available"):
            service.source_from_id("chromium:/etc/passwd")
        vault, api = Mock(), Mock()
        api.get_account_info.return_value = {"accountName": "Listener"}
        with patch.object(service, "keyring", return_value=vault), patch.object(service, "music", return_value=api):
            status = service.handle({"op": "connect", "source": found[0]["id"]})
        self.assertEqual((status["account"], status["source"]), ("Listener", "Chromium · Person 1"))
        saved = vault.save.call_args[0][0]
        self.assertEqual(saved["source"], {"id": found[0]["id"], "label": "Chromium · Person 1"})
        self.assertEqual({c["name"] for c in saved["cookies"]}, {n for n, _ in SIGNED})

    def test_a_chosen_folder_finds_its_signed_in_profile(self):
        self.chromium([(".youtube.com", n, v, "") for n, v in SIGNED], folder=("portable",), profile="Profile 2")
        source = service.source_from_id("folder:" + self.path(".config", "portable", "x")[:-2])
        self.assertEqual(source["engine"], "chromium")
        self.assertEqual({c.name: c.value for c in service.read_cookies(source)}["__Secure-3PAPISID"], "sapisid")
        with self.assertRaisesRegex(service.UserError, "no browser cookies"):
            service.source_from_id("folder:" + self.home.name)


class Pasted(unittest.TestCase):
    def values(self, text):
        cookies, authuser = service.pasted_cookies(text)
        return {c.name: c.value for c in service.decode_cookies(service.encode_cookies(cookies))}, authuser

    def test_curl_from_chrome_and_firefox(self):
        chrome = ("curl 'https://music.youtube.com/youtubei/v1/browse?prettyPrint=false' \\\n"
                  "  -H 'accept: */*' \\\n  -b 'SID=abc; __Secure-3PAPISID=xyz/A' \\\n  -H 'x-goog-authuser: 1'")
        self.assertEqual(self.values(chrome), ({"SID": "abc", "__Secure-3PAPISID": "xyz/A"}, "1"))
        firefox = "curl 'https://music.youtube.com/' -H 'User-Agent: Mozilla/5.0' -H 'Cookie: SID=abc; __Secure-3PAPISID=xyz'"
        self.assertEqual(self.values(firefox)[0], {"SID": "abc", "__Secure-3PAPISID": "xyz"})

    def test_request_headers_in_either_layout(self):
        raw = "POST /youtubei/v1/browse HTTP/2\nHost: music.youtube.com\nCookie: SID=abc; __Secure-3PAPISID=xyz\nX-Goog-AuthUser: 0\n"
        self.assertEqual(self.values(raw), ({"SID": "abc", "__Secure-3PAPISID": "xyz"}, "0"))
        devtools = "accept\n*/*\ncookie\nSID=abc; __Secure-3PAPISID=xyz\norigin\nhttps://music.youtube.com\n"
        self.assertEqual(self.values(devtools)[0], {"SID": "abc", "__Secure-3PAPISID": "xyz"})

    def test_cookies_txt_keeps_only_youtube(self):
        text = ("# Netscape HTTP Cookie File\n"
                f"#HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t{FUTURE}\t__Secure-3PSID\tpsid\n"
                f".youtube.com\tTRUE\t/\tTRUE\t{FUTURE}\t__Secure-3PAPISID\tsapisid\n"
                f".google.com\tTRUE\t/\tTRUE\t{FUTURE}\tSID\tgoogle-only\n")
        self.assertEqual(self.values(text)[0], {"__Secure-3PSID": "psid", "__Secure-3PAPISID": "sapisid"})

    def test_text_without_a_sign_in_is_refused_before_youtube_is_asked(self):
        with patch.object(service, "keyring", return_value=Mock()), patch.object(service, "music") as music:
            with self.assertRaisesRegex(service.UserError, "developer tools"):
                service.connect_text("just some words")
        music.assert_not_called()


def seal(value, host, password, version, hashed):
    from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
    plain = (hashlib.sha256(host.encode()).digest() if hashed else b"") + value.encode()
    pad = 16 - len(plain) % 16
    encryptor = Cipher(algorithms.AES(service.derive(password)), modes.CBC(b" " * 16)).encryptor()
    return version + encryptor.update(plain + bytes([pad]) * pad) + encryptor.finalize()


if __name__ == "__main__":
    unittest.main()
